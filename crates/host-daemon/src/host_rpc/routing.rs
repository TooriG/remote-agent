//! Live conversation routing. One registry lock covers connections, task facts
//! and request ownership; a conversation actor lock is always taken first.
use super::session_actor::SessionActor;
use crate::providers::{AgentChange, Failure, RequestOrigin};
use agent_protocol::live_activity::TaskState;
use agent_protocol::{
    models::{Thread, ThreadResponse},
    protocol::{self, Notification},
    session::{OpenedSession, ProviderKind, SessionChange, SessionRef},
};
use serde_json::Value;
use std::{
    collections::HashMap,
    fmt,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed},
    },
};
use tokio::sync::mpsc;

mod activity;
mod connections;
mod read_state;
mod requests;
#[cfg(test)]
mod tests;

use activity::{Task, record_task_facts};
pub use connections::HostSession;
pub(super) use connections::Outbound;
use connections::{MAX_QUEUED_BYTES, frame_cost};
use requests::native_key;

pub type SessionId = u64;

pub(super) struct SessionLease {
    router: SessionRouter,
    target: SessionRef,
    actor: Arc<Mutex<SessionActor>>,
}
impl Drop for SessionLease {
    fn drop(&mut self) {
        let mut actor = lock_state(&self.actor);
        actor.leases -= 1;
        drop(actor);
        self.router.prune(&self.target, &self.actor);
    }
}

/// The stream owns its subscription. Conversation content is never owned by a connection.
pub struct HostSubscription {
    id: uuid::Uuid,
    target: SessionRef,
    actor: Weak<Mutex<SessionActor>>,
    receiver: mpsc::UnboundedReceiver<Vec<u8>>,
    queued_bytes: Arc<AtomicUsize>,
    state: Weak<Mutex<State>>,
}
impl HostSubscription {
    pub async fn recv(&mut self) -> Option<Vec<u8>> {
        let frame = self.receiver.recv().await?;
        self.queued_bytes
            .fetch_sub(frame_cost(frame.capacity()), Relaxed);
        Some(frame)
    }
}
impl Drop for HostSubscription {
    fn drop(&mut self) {
        if let Some(actor) = self.actor.upgrade() {
            lock_state(&actor).subscriptions.remove(&self.id);
            if let Some(state) = self.state.upgrade() {
                lock_state(&state).subscriptions.remove(&self.id);
                SessionRouter { state }.prune(&self.target, &actor);
            }
        }
        while let Ok(frame) = self.receiver.try_recv() {
            self.queued_bytes
                .fetch_sub(frame_cost(frame.capacity()), Relaxed);
        }
    }
}
pub struct HostReply {
    pub initial: Vec<u8>,
    pub updates: Option<HostSubscription>,
}
impl From<protocol::Response> for HostReply {
    fn from(response: protocol::Response) -> Self {
        Self {
            initial: protocol::response_frame(response).expect("response encodes"),
            updates: None,
        }
    }
}

/// Connections and compact task facts share this lock; conversation history has its own lock.
#[derive(Default)]
struct State {
    apns: Option<Arc<crate::apns::Apns>>,
    tasks: HashMap<SessionRef, Task>,
    reads: read_state::ReadState,
    task_revision: u64,
    list_revision: u64,
    next_session_id: SessionId,
    sessions: HashMap<SessionId, Outbound>,
    executions: HashMap<SessionRef, Arc<Mutex<SessionActor>>>,
    subscriptions: HashMap<uuid::Uuid, (SessionRef, SessionId)>,
    requests: HashMap<agent_protocol::ids::RequestId, SessionRef>,
    native_requests: HashMap<(uuid::Uuid, String), agent_protocol::ids::RequestId>,
}
#[derive(Clone)]
pub(crate) struct SessionRouter {
    state: Arc<Mutex<State>>,
}
fn lock_state<T>(state: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    state.lock().unwrap_or_else(|e| e.into_inner())
}

impl SessionRouter {
    /// Apply one routed provider change to live sessions and subscribers.
    pub(crate) fn apply(
        &self,
        change: AgentChange,
        bindings: &[(SessionRef, ProviderKind)],
    ) -> Result<(), String> {
        match change {
            AgentChange::TaskParent { session, parent } => self.task_parent(session, parent),
            AgentChange::Session { session, change } => self.session_change(&session, change),
            AgentChange::Request {
                session,
                origin,
                request,
            } => return self.request(session, origin, request),
            AgentChange::Resolved {
                instance,
                native_id,
            } => self.resolve_native_request(instance, &native_id),
            AgentChange::SourceClosed(instance) => self.close_request_source(instance),
            AgentChange::Renamed { session, name } => {
                self.broadcast(Notification::SessionRenamed {
                    session,
                    name,
                    revision: 0,
                })
            }
            AgentChange::Stopped { provider, reason } => {
                self.fail_provider(provider, &reason, bindings)
            }
        }
        Ok(())
    }
    pub(crate) fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                next_session_id: 1,
                ..Default::default()
            })),
        }
    }
    fn actor(&self, target: &SessionRef) -> Arc<Mutex<SessionActor>> {
        lock_state(&self.state)
            .executions
            .entry(target.clone())
            .or_default()
            .clone()
    }
    fn existing(&self, target: &SessionRef) -> Option<Arc<Mutex<SessionActor>>> {
        lock_state(&self.state).executions.get(target).cloned()
    }
    fn prune(&self, target: &SessionRef, actor: &Arc<Mutex<SessionActor>>) {
        let mut owned = lock_state(actor);
        if owned.release() || !owned.subscriptions.is_empty() {
            return;
        }
        let mut state = lock_state(&self.state);
        if Arc::strong_count(actor) == 2
            && state
                .executions
                .get(target)
                .is_some_and(|current| Arc::ptr_eq(current, actor))
        {
            state.executions.remove(target);
        }
    }
    pub(super) fn submission_lock(&self, target: &SessionRef) -> Arc<tokio::sync::Mutex<()>> {
        lock_state(&self.actor(target)).submission_lock.clone()
    }
    pub(crate) fn overlay_execution(&self, target: &SessionRef, mut thread: Thread) -> Thread {
        if let Some(actor) = self.existing(target) {
            thread = lock_state(&actor).overlay(thread);
            self.prune(target, &actor);
        }
        thread
    }
    pub(super) fn execution_targets(&self) -> Vec<SessionRef> {
        lock_state(&self.state).executions.keys().cloned().collect()
    }
    pub(super) fn submission_receipt(
        &self,
        target: &SessionRef,
        id: &str,
    ) -> Option<agent_protocol::operations::SubmissionReceipt> {
        let actor = self.existing(target)?;
        let receipt = match lock_state(&actor).timeline.submissions.get(id) {
            Some(agent_protocol::session::SubmissionDelivery::Accepted { turn_id }) => {
                Some(agent_protocol::operations::SubmissionReceipt {
                    turn_id: turn_id.clone(),
                })
            }
            _ => None,
        };
        self.prune(target, &actor);
        receipt
    }

    pub(super) fn begin_submission(&self, target: &SessionRef, id: &str) -> Result<(), Failure> {
        if id.is_empty() || id.len() > 256 {
            return Err(Failure::new(
                "invalid_params",
                "clientUserMessageId is required",
            ));
        }
        let actor = self.actor(target);
        let mut owned = lock_state(&actor);
        if owned.timeline.submissions.get(id).is_some_and(|delivery| {
            *delivery != agent_protocol::session::SubmissionDelivery::Rejected
        }) {
            return Err(Failure::unknown(
                "submission_outcome_unknown",
                "submission is already in flight; read the session before sending again",
            ));
        }
        if owned.timeline.submissions.len() >= 128 {
            owned.timeline.submissions.retain(|_, delivery| {
                *delivery != agent_protocol::session::SubmissionDelivery::Rejected
            });
        }
        if owned.timeline.submissions.len() >= 128 {
            return Err(Failure::new(
                "input_capacity_reached",
                "active input capacity reached",
            ));
        }
        let mut failed = Vec::new();
        self.change_locked(
            &mut owned,
            target,
            &SessionChange::Submission {
                id: id.into(),
                delivery: agent_protocol::session::SubmissionDelivery::Sending,
            },
            &mut failed,
        );
        drop(owned);
        self.close_failed(failed);
        Ok(())
    }
    pub(super) fn finish_submission(
        &self,
        target: &SessionRef,
        id: &str,
        delivery: agent_protocol::session::SubmissionDelivery,
    ) {
        let Some(actor) = self.existing(target) else {
            return;
        };
        let mut owned = lock_state(&actor);
        let mut failed = Vec::new();
        if owned.timeline.submissions.contains_key(id) {
            self.change_locked(
                &mut owned,
                target,
                &SessionChange::Submission {
                    id: id.into(),
                    delivery,
                },
                &mut failed,
            );
        }
        drop(owned);
        self.close_failed(failed);
        self.prune(target, &actor);
    }
    pub(crate) fn current_turn(
        &self,
        target: &SessionRef,
        turn: &str,
    ) -> Option<Arc<agent_protocol::models::Turn>> {
        let actor = self.existing(target)?;
        let result = lock_state(&actor)
            .timeline
            .turns
            .as_ref()
            .and_then(|turns| {
                turns
                    .iter()
                    .rfind(|current| current.id.as_str() == turn)
                    .cloned()
            });
        self.prune(target, &actor);
        result
    }

    pub(super) fn retain_execution(&self, target: SessionRef) -> Result<SessionLease, String> {
        target.validate()?;
        let actor = self.actor(&target);
        lock_state(&actor).leases += 1;
        Ok(SessionLease {
            router: self.clone(),
            target,
            actor,
        })
    }
    /// Publish hydration on the existing ordered stream, between live changes.
    /// Native history stays in the requesting client's cache.
    pub(super) fn finish_turn_read(
        &self,
        read: SessionLease,
        session: SessionId,
        turn_id: agent_protocol::ids::TurnId,
        native_items: Vec<Arc<agent_protocol::models::Item>>,
    ) -> Result<(), String> {
        let actor = lock_state(&read.actor);
        if !actor
            .subscriptions
            .values()
            .any(|(owner, _)| *owner == session)
        {
            return Err("conversation subscription changed during detail read".into());
        }
        let live = actor
            .timeline
            .turns
            .iter()
            .flatten()
            .rfind(|current| current.id == turn_id);
        let mut items = agent_protocol::session::append_items(
            native_items,
            live.and_then(|turn| turn.items.as_deref())
                .unwrap_or_default(),
        );
        agent_protocol::models::defer_items(
            &mut items,
            agent_protocol::models::MAX_INLINE_ITEM_BYTES,
        );
        let mut change = SessionChange::TurnItems { turn_id, items };
        let mut frame = protocol::encode(&change).map_err(|error| error.to_string())?;
        if frame.len() > MAX_QUEUED_BYTES {
            if let SessionChange::TurnItems { items, .. } = &mut change {
                agent_protocol::models::defer_items(items, 0);
            }
            frame = protocol::encode(&change).map_err(|error| error.to_string())?;
        }
        let failed = actor
            .subscriptions
            .values()
            .filter(|(owner, _)| *owner == session)
            .any(|(owner, output)| output.try_send(*owner, frame.clone()).is_err());
        drop(actor);
        if failed {
            self.close_failed(vec![session]);
            return Err("conversation detail stream is unavailable".into());
        }
        Ok(())
    }
    pub(super) fn finish_session_read(
        &self,
        read: SessionLease,
        session: SessionId,
        mut response: ThreadResponse,
    ) -> Result<HostReply, String> {
        let connection = self
            .connection(session)
            .ok_or("connection closed during session open")?;
        let mut actor = lock_state(&read.actor);
        if response.thread.id.as_ref() != Some(&read.target) {
            return Err("native session ID does not match".into());
        }
        response.thread = actor.overlay(response.thread);
        response
            .thread
            .defer_item_details(agent_protocol::models::MAX_INLINE_ITEM_BYTES);
        let mut opened = OpenedSession {
            session: read.target.clone(),
            subscription_id: uuid::Uuid::nil(),
            response,
        };
        let mut initial = protocol::encode(protocol::Response::Success { result: &opened })
            .map_err(|e| e.to_string())?;
        if initial.len() > MAX_QUEUED_BYTES {
            opened.response.thread.defer_item_details(0);
            initial = protocol::encode(protocol::Response::Success { result: &opened })
                .map_err(|e| e.to_string())?;
        }
        if initial.len() > MAX_QUEUED_BYTES {
            return Ok(protocol::Response::error(
                "response_too_large",
                &"Session metadata exceeds the RPC limit; request fewer turns",
            )
            .into());
        }
        let id = uuid::Uuid::new_v4();
        let (sender, receiver) = mpsc::unbounded_channel();
        // Conversation -> registry is the only nested lock order. Registration,
        // removal and claims all verify the same live connection here.
        let mut state = lock_state(&self.state);
        if !state.sessions.contains_key(&session) || !connection.alive.load(Relaxed) {
            return Err("connection closed during session open".into());
        }
        state
            .subscriptions
            .insert(id, (read.target.clone(), session));
        actor.subscriptions.insert(
            id,
            (
                session,
                Outbound {
                    sender,
                    ..connection.clone()
                },
            ),
        );
        drop(state);
        drop(actor);
        Ok(HostReply {
            initial,
            updates: Some(HostSubscription {
                id,
                target: read.target.clone(),
                actor: Arc::downgrade(&read.actor),
                receiver,
                queued_bytes: connection.bytes,
                state: Arc::downgrade(&self.state),
            }),
        })
    }
    pub(crate) fn session_change(&self, target: &SessionRef, change: SessionChange) {
        let actor = self.actor(target);
        let mut failed = Vec::new();
        self.change_locked(&mut lock_state(&actor), target, &change, &mut failed);
        self.close_failed(failed);
        self.prune(target, &actor);
    }
    pub(crate) fn fail_provider(
        &self,
        provider: ProviderKind,
        message: &str,
        bindings: &[(SessionRef, ProviderKind)],
    ) {
        let actors: Vec<_> = lock_state(&self.state)
            .executions
            .iter()
            .filter(|(target, _)| {
                bindings
                    .iter()
                    .find(|(root, _)| root == *target)
                    .map_or(target.provider, |(_, active)| *active)
                    == provider
            })
            .map(|(target, actor)| (target.clone(), actor.clone()))
            .collect();
        for (target, actor) in actors {
            let mut owned = lock_state(&actor);
            let mut changes: Vec<_> = owned
                .timeline
                .requests
                .keys()
                .cloned()
                .map(|request_id| SessionChange::ResolveRequest { request_id })
                .collect();
            changes.extend(
                owned
                    .timeline
                    .turns
                    .iter()
                    .flatten()
                    .filter(|turn| turn.status == agent_protocol::execution::TurnStatus::Running)
                    .map(|turn| {
                        let mut turn = (**turn).clone();
                        turn.status = agent_protocol::execution::TurnStatus::Failed;
                        turn.error = Some(agent_protocol::execution::ExecutionError {
                            category: agent_protocol::execution::ErrorCategory::Network,
                            message: message.into(),
                            ..Default::default()
                        });
                        SessionChange::Turn {
                            turn,
                            completed: true,
                        }
                    }),
            );
            changes.extend(
                owned
                    .timeline
                    .submissions
                    .iter()
                    .filter(|(_, delivery)| {
                        matches!(
                            delivery,
                            agent_protocol::session::SubmissionDelivery::Sending
                        )
                    })
                    .map(|(id, _)| SessionChange::Submission {
                        id: id.clone(),
                        delivery: agent_protocol::session::SubmissionDelivery::Unknown,
                    }),
            );
            changes.push(SessionChange::Status {
                status: agent_protocol::models::SessionStatus::Unavailable,
            });
            let mut failed = Vec::new();
            for change in changes {
                self.change_locked(&mut owned, &target, &change, &mut failed);
            }
            drop(owned);
            self.close_failed(failed);
            self.prune(&target, &actor);
        }
    }
    fn change_locked(
        &self,
        actor: &mut SessionActor,
        target: &SessionRef,
        change: &SessionChange,
        failed: &mut Vec<SessionId>,
    ) {
        let expired: Vec<_> = actor.timeline.requests.values().filter(|request| match change {
            SessionChange::Turn { turn, completed: true } => matches!(&request.target, agent_protocol::requests::RequestTarget::Turn { turn_id, .. } if turn_id == &turn.id),
            SessionChange::Item { turn_id, item } => {
                let terminal = matches!(item.status, agent_protocol::execution::ItemStatus::Completed | agent_protocol::execution::ItemStatus::Failed | agent_protocol::execution::ItemStatus::Declined | agent_protocol::execution::ItemStatus::Interrupted);
                (terminal || request.delivery != agent_protocol::session::RequestDelivery::Awaiting) && matches!(&request.target, agent_protocol::requests::RequestTarget::Turn { turn_id: request_turn, item_id } if request_turn == turn_id && item_id.as_ref() == Some(&item.id))
            }
            _ => false,
        }).map(|request| request.id.clone()).collect();
        for request_id in expired {
            self.change_locked(
                actor,
                target,
                &SessionChange::ResolveRequest { request_id },
                failed,
            );
        }
        if let SessionChange::ResolveRequest { request_id } = change
            && let Some(origin) = actor.request_origins.remove(request_id)
        {
            let mut state = lock_state(&self.state);
            state.requests.remove(request_id);
            state
                .native_requests
                .remove(&native_key(origin.instance, &origin.native_id));
        }
        let (next, result) = change.apply_timeline(std::mem::take(&mut actor.timeline));
        actor.timeline = next;
        // An update may target native history that the Host deliberately does
        // not retain. Subscribers still apply it to their own history window.
        if let Err(reason) = result
            && reason != agent_protocol::session::UpdateError::MissingTurn
        {
            tracing::warn!(target: "agent", operation = "host.session.invalid_update", message = %reason);
            actor.release();
            return;
        }
        let frame = protocol::encode(change).expect("change encodes");
        for (connection, output) in actor.subscriptions.values() {
            if output.try_send(*connection, frame.clone()).is_err() {
                failed.push(*connection);
            }
        }
        if matches!(
            change,
            SessionChange::Status { .. } | SessionChange::Turn { .. }
        ) {
            let active = actor.timeline.status == agent_protocol::models::SessionStatus::Running;
            let frame = protocol::encode(Notification::Activity {
                session: target.clone(),
                active,
            })
            .expect("activity encodes");
            failed.extend(self.broadcast_frames(frame));
        }
        let mut state = lock_state(&self.state);
        failed.extend(record_task_facts(
            &mut state,
            target,
            &actor.timeline,
            change,
        ));
        drop(state);
        actor.release();
    }
}
