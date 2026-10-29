//! Compact task facts shared by every connection, Live Activities and APNs.
use super::*;

#[derive(Default)]
pub(super) struct Task {
    pub(super) facts: Option<TaskState>,
    pub(super) parent: Option<SessionRef>,
    // Parent discovery must not fence out an already pending read of execution facts.
    pub(super) facts_revision: u64,
}
fn task_groups(
    tasks: &HashMap<SessionRef, Task>,
) -> HashMap<&SessionRef, agent_protocol::live_activity::TaskActivitySummary> {
    use agent_protocol::live_activity::TaskActivitySummary;
    // A conversation and all of its descendants contribute one icon and one count.
    let mut conversations: HashMap<&SessionRef, TaskActivitySummary> = HashMap::new();
    for (session, task) in tasks {
        let Some(facts) = task.facts else { continue };
        let contribution = TaskActivitySummary::from_statuses([facts.phase().0]);
        if contribution == TaskActivitySummary::default() {
            continue;
        }
        let mut root = session;
        let mut depth = 0;
        while let Some(parent) = tasks.get(root).and_then(|task| task.parent.as_ref()) {
            if depth == tasks.len() {
                // Invalid ancestry cannot certify that known work has finished.
                root = session;
                break;
            }
            root = parent;
            depth += 1;
        }
        let summary = conversations.entry(root).or_default();
        summary.waiting += contribution.waiting;
        summary.running += contribution.running;
        summary.unknown += contribution.unknown;
    }
    conversations
}
fn task_display(
    groups: &HashMap<&SessionRef, agent_protocol::live_activity::TaskActivitySummary>,
) -> agent_protocol::live_activity::TaskActivityDisplay {
    agent_protocol::live_activity::TaskActivitySummary::from_statuses(groups.values().map(
        |summary| {
            if summary.waiting > 0 {
                "waiting"
            } else if summary.running > 0 {
                "running"
            } else {
                "unknown"
            }
        },
    ))
    .display()
}
pub(super) fn task_activity_state(
    revision: u64,
    tasks: &HashMap<SessionRef, Task>,
    unread: Vec<(SessionRef, agent_protocol::ids::TurnId)>,
) -> agent_protocol::live_activity::TaskActivityState {
    let mut statuses: std::collections::BTreeMap<_, _> = tasks
        .iter()
        .filter_map(|(id, task)| Some((id.clone(), task.facts?.session_status())))
        .collect();
    let groups = task_groups(tasks);
    for (root, summary) in &groups {
        statuses.insert(
            (*root).clone(),
            if summary.running + summary.waiting > 0 {
                agent_protocol::models::SessionStatus::Running
            } else {
                agent_protocol::models::SessionStatus::Unknown
            },
        );
    }
    agent_protocol::live_activity::TaskActivityState {
        revision,
        display: task_display(&groups),
        statuses: statuses.into_iter().collect(),
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
    let read_changed = state.reads.record(
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
    pub(in crate::host_rpc) fn task_parent(&self, session: SessionRef, parent: SessionRef) {
        let mut state = lock_state(&self.state);
        if state
            .tasks
            .get(&session)
            .and_then(|task| task.parent.as_ref())
            == Some(&parent)
        {
            return;
        }
        state.task_revision += 1;
        let revision = state.task_revision;
        let task = state.tasks.entry(session).or_default();
        task.parent = Some(parent);
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
                let thread_parent = thread.parent_id.clone();
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
                task.map(|task| (target, task, thread_parent))
            })
            .collect();
        let mut state = lock_state(&self.state);
        let before = state.task_revision;
        // Provider events received while loading the list take precedence, including completion.
        for (session, task, parent) in tasks {
            let previous = state.tasks.get(&session);
            // Parent identity is immutable and can arrive with an older execution read.
            let parent = parent.or_else(|| previous.and_then(|old| old.parent.clone()));
            let previous_facts = previous.and_then(|old| old.facts);
            let accepts_facts = !previous.is_some_and(|old| old.facts_revision > read_revision)
                && (task.phase().0 != "unknown" || previous_facts.is_some_and(|old| old.phase().1))
                && (previous_facts.is_some() || task.phase().1);
            let facts = if accepts_facts {
                Some(task)
            } else {
                previous_facts
            };
            let unchanged = previous_facts == facts
                && previous.and_then(|old| old.parent.as_ref()) == parent.as_ref();
            let read_changed = accepts_facts
                && state.reads.record(
                    &session,
                    task.session_status() == agent_protocol::models::SessionStatus::Running,
                    None,
                );
            if unchanged && !read_changed {
                continue;
            }
            state.task_revision += 1;
            let revision = state.task_revision;
            let owned = state.tasks.entry(session).or_default();
            owned.parent = parent;
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
            task_display(&task_groups(&state.tasks)),
        )
    }
}
