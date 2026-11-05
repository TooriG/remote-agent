//! Compact task facts shared by every connection, Live Activities and APNs.
use super::*;

#[derive(Default)]
pub(super) struct Task {
    pub(super) facts: Option<TaskState>,
    pub(super) subagent: bool,
    // Subagent discovery must not fence out an already pending read of execution facts.
    pub(super) facts_revision: u64,
}
/// Only user conversations contribute to system surfaces. Subagent work,
/// requests and completions belong inside their conversation, not its task count.
fn task_display(
    tasks: &HashMap<SessionRef, Task>,
    unread: &[(SessionRef, agent_protocol::ids::TurnId)],
) -> agent_protocol::live_activity::TaskActivityDisplay {
    let mut summary = agent_protocol::live_activity::TaskActivitySummary::from_statuses(
        tasks
            .values()
            .filter(|task| !task.subagent)
            .filter_map(|task| task.facts.map(|facts| facts.phase().0)),
    );
    summary.finished = unread
        .iter()
        .filter(|(session, _)| {
            tasks.get(session).is_none_or(|task| {
                !task.subagent
                    && task.facts.is_none_or(|facts| {
                        facts.session_status() == agent_protocol::models::SessionStatus::Idle
                    })
            })
        })
        .count() as u32;
    summary.display()
}
pub(super) fn task_activity_state(
    revision: u64,
    tasks: &HashMap<SessionRef, Task>,
    unread: Vec<(SessionRef, agent_protocol::ids::TurnId)>,
) -> agent_protocol::live_activity::TaskActivityState {
    let statuses: std::collections::BTreeMap<_, _> = tasks
        .iter()
        .filter_map(|(id, task)| Some((id.clone(), task.facts?.session_status())))
        .collect();
    let mut waiting: Vec<_> = tasks
        .iter()
        .filter(|(_, task)| !task.subagent && task.facts.is_some_and(|facts| facts.waiting))
        .map(|(session, _)| session.clone())
        .collect();
    waiting.sort();
    agent_protocol::live_activity::TaskActivityState {
        revision,
        display: task_display(tasks, &unread),
        statuses: statuses.into_iter().collect(),
        waiting,
        unread,
    }
}
pub(super) fn publish_task_activity(
    apns: Option<&crate::apns::Apns>,
    sessions: &HashMap<SessionId, Outbound>,
    state: &agent_protocol::live_activity::TaskActivityState,
) -> Vec<SessionId> {
    if let Some(apns) = apns {
        apns.update(state.display.clone());
    }
    let frame = protocol::encode(Notification::TaskActivity {
        state: state.clone(),
    })
    .expect("task activity encodes");
    sessions
        .iter()
        .filter_map(|(id, output)| output.try_send(*id, frame.clone()).err().map(|_| *id))
        .collect()
}

/// Record a conversation's latest execution facts. Unknown facts never replace
/// a known phase that has not finished. Returns connections that failed delivery.
pub(super) fn record_task_facts(
    state: &mut State,
    target: &SessionRef,
    timeline: &agent_protocol::session::Timeline,
    change: &SessionChange,
) -> Vec<SessionId> {
    let previous = state.tasks.get(target).and_then(|task| task.facts);
    let task = TaskState {
        status: if timeline.status != agent_protocol::models::SessionStatus::Unknown
            || matches!(change, SessionChange::Status { .. })
        {
            timeline.status
        } else {
            previous.map(|task| task.status).unwrap_or_default()
        },
        waiting: timeline
            .requests
            .values()
            .any(|request| request.delivery == agent_protocol::session::RequestDelivery::Awaiting),
        latest: timeline
            .turns
            .as_ref()
            .and_then(|turns| turns.last())
            .map(|turn| turn.status)
            .or_else(|| previous.and_then(|task| task.latest)),
    };
    let completed = match change {
        SessionChange::Turn {
            completed: true,
            turn,
        } if turn.status == agent_protocol::execution::TurnStatus::Completed => Some(&turn.id),
        _ => None,
    };
    let read_changed = state.tasks.get(target).is_none_or(|task| !task.subagent)
        && state.reads.record(
            target,
            task.session_status() == agent_protocol::models::SessionStatus::Running,
            completed,
        );
    let facts_changed = previous != Some(task)
        && (task.phase().0 != "unknown" || previous.is_some_and(|task| task.phase().1));
    if read_changed || facts_changed {
        state.task_revision += 1;
        let revision = state.task_revision;
        // A new completion is fresh evidence even when its phase is unchanged.
        // An older running list must not clear that completion's unread badge.
        let owned = state.tasks.entry(target.clone()).or_default();
        owned.facts = Some(task);
        owned.facts_revision = revision;
        let activity = task_activity_state(state.task_revision, &state.tasks, state.reads.unread());
        return publish_task_activity(state.apns.as_deref(), &state.sessions, &activity);
    }
    Vec::new()
}

impl SessionRouter {
    pub(in crate::host_rpc) fn set_apns(&self, apns: Option<Arc<crate::apns::Apns>>) {
        lock_state(&self.state).apns = apns;
    }
    pub(in crate::host_rpc) fn apns(&self) -> Option<Arc<crate::apns::Apns>> {
        lock_state(&self.state).apns.clone()
    }
    pub(in crate::host_rpc) fn task_activity_revision(&self) -> u64 {
        lock_state(&self.state).task_revision
    }
    pub(in crate::host_rpc) fn task_subagent(&self, session: SessionRef) {
        let mut state = lock_state(&self.state);
        if state.tasks.get(&session).is_some_and(|task| task.subagent) {
            return;
        }
        state.task_revision += 1;
        let revision = state.task_revision;
        state.reads.forget(&session);
        let task = state.tasks.entry(session).or_default();
        task.subagent = true;
        let activity = task_activity_state(revision, &state.tasks, state.reads.unread());
        let failed = publish_task_activity(state.apns.as_deref(), &state.sessions, &activity);
        drop(state);
        self.close_failed(failed);
    }
    pub(in crate::host_rpc) fn task_activity(
        &self,
        native: Vec<Thread>,
        read_revision: u64,
    ) -> agent_protocol::live_activity::TaskActivityState {
        let tasks: Vec<_> = native
            .into_iter()
            .filter_map(|thread| {
                let target = thread.id.clone()?;
                let subagent = thread.parent_id.is_some();
                let actor = self.actor(&target);
                let owned = lock_state(&actor);
                let task = if owned.timeline.status
                    == agent_protocol::models::SessionStatus::Unknown
                    && thread.list_stale == Some(true)
                {
                    None
                } else {
                    let thread = owned.overlay(thread);
                    Some(TaskState {
                        status: thread.status,
                        waiting: thread.requests.values().any(|request| {
                            request.delivery == agent_protocol::session::RequestDelivery::Awaiting
                        }),
                        latest: thread
                            .turns
                            .as_ref()
                            .and_then(|turns| turns.last())
                            .map(|turn| turn.status),
                    })
                };
                drop(owned);
                self.prune(&target, &actor);
                Some((target, task, subagent))
            })
            .collect();
        let mut state = lock_state(&self.state);
        let before = state.task_revision;
        // Provider events received while loading the list take precedence, including completion.
        for (session, task, subagent) in tasks {
            let previous = state.tasks.get(&session);
            // Subagent identity is immutable and can arrive with an older execution read.
            let subagent = subagent || previous.is_some_and(|old| old.subagent);
            let previous_facts = previous.and_then(|old| old.facts);
            let accepts_facts = !previous.is_some_and(|old| old.facts_revision > read_revision)
                && task.is_some_and(|task| {
                    (task.phase().0 != "unknown" || previous_facts.is_some_and(|old| old.phase().1))
                        && (previous_facts.is_some() || task.phase().1)
                });
            let facts = if accepts_facts { task } else { previous_facts };
            let unchanged =
                previous_facts == facts && previous.is_some_and(|old| old.subagent) == subagent;
            let read_changed = if subagent {
                state.reads.forget(&session)
            } else {
                accepts_facts
                    && state.reads.record(
                        &session,
                        facts.unwrap().session_status()
                            == agent_protocol::models::SessionStatus::Running,
                        None,
                    )
            };
            if unchanged && !read_changed {
                continue;
            }
            state.task_revision += 1;
            let revision = state.task_revision;
            let owned = state.tasks.entry(session).or_default();
            owned.subagent = subagent;
            if owned.facts != facts {
                owned.facts = facts;
                owned.facts_revision = revision;
            }
        }
        let result = task_activity_state(state.task_revision, &state.tasks, state.reads.unread());
        let failed = if state.task_revision != before {
            publish_task_activity(state.apns.as_deref(), &state.sessions, &result)
        } else {
            Vec::new()
        };
        drop(state);
        self.close_failed(failed);
        result
    }
    pub(in crate::host_rpc) fn register_live_activity(
        &self,
        connection: SessionId,
        params: &agent_protocol::live_activity::RegisterLiveActivity,
    ) -> Result<(), String> {
        let state = lock_state(&self.state);
        let principal = state
            .sessions
            .get(&connection)
            .filter(|output| output.alive.load(Relaxed))
            .ok_or("connection is closed")?;
        state.apns.as_ref().ok_or("APNs is unavailable")?.register(
            &principal.principal,
            params,
            task_display(&state.tasks, &state.reads.unread()),
        )
    }
}
