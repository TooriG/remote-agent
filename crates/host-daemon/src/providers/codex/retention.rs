//! The Host is app-server's only client, so its subscription alone keeps a
//! thread loaded with its MCP servers. App-server unloads an unsubscribed
//! inactive thread by itself; the next start resumes it.
use crate::providers::{AgentChange, Failure};
use agent_protocol::{
    execution::ItemStatus,
    models::SessionStatus,
    session::{ProviderKind, SessionChange},
};
use codex_app_server::CodexAppServer;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

/// Release a subscription after this much silence.
const IDLE: Duration = Duration::from_secs(30 * 60);
/// Running background work keeps its thread while it reports within this time.
const MAX_PIN: Duration = Duration::from_secs(4 * 60 * 60);
const CHECK_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Default)]
pub(super) struct Subscriptions {
    activity: Mutex<Activity>,
    /// Held across subscription requests so a release cannot overtake a resume.
    released: tokio::sync::Mutex<HashSet<String>>,
}
impl Subscriptions {
    pub(super) fn observe(&self, change: &AgentChange) {
        lock(&self.activity).observe(change, Instant::now());
    }
    pub(super) fn touch(&self, id: &str) {
        lock(&self.activity).touch(id, Instant::now());
    }
    /// Resume before starting work when the native thread is unloaded or this
    /// Host released its subscription; a loaded thread alone sends no events.
    pub(super) async fn start(
        &self,
        id: &str,
        reload: bool,
        resume: impl Future<Output = Result<(), Failure>>,
    ) -> Result<(), Failure> {
        let mut released = self.released.lock().await;
        if reload || released.contains(id) {
            resume.await?;
            released.remove(id);
        }
        self.touch(id);
        Ok(())
    }
    /// An idle release skips a thread that a start has resumed meanwhile.
    pub(super) async fn release(
        &self,
        process: &CodexAppServer,
        id: &str,
        only_idle: bool,
    ) -> Result<(), String> {
        let mut released = self.released.lock().await;
        if only_idle && !lock(&self.activity).releasable(id, Instant::now()) {
            return Ok(());
        }
        process
            .request::<_, serde_json::Value>(
                "thread/unsubscribe",
                &serde_json::json!({"threadId": id}),
            )
            .await
            .map_err(|error| error.to_string())?
            .outcome
            .map_err(|error| error.to_string())?;
        released.insert(id.to_owned());
        lock(&self.activity).forget(id);
        Ok(())
    }
    pub(super) fn spawn_releaser(
        self: Arc<Self>,
        process: Arc<CodexAppServer>,
        stopped: CancellationToken,
    ) {
        tokio::spawn(async move {
            let mut check = tokio::time::interval(CHECK_INTERVAL);
            loop {
                tokio::select! {
                    _ = stopped.cancelled() => return,
                    _ = check.tick() => self.release_idle(&process).await,
                }
            }
        });
    }
    async fn release_idle(&self, process: &CodexAppServer) {
        let due = lock(&self.activity).due(Instant::now());
        for id in due {
            if let Err(error) = self.release(process, &id, true).await {
                tracing::warn!(target: "agent", operation = "host.codex.release_idle", thread = %id, message = %error);
                // Retry after another idle period instead of every check.
                self.touch(&id);
            }
        }
    }
}

fn lock<T>(state: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    state.lock().unwrap_or_else(|e| e.into_inner())
}

struct Thread {
    subscribed: bool,
    active: bool,
    running: HashSet<String>,
    parent: Option<String>,
    last: Instant,
}
impl Thread {
    fn new(now: Instant) -> Self {
        Self {
            subscribed: false,
            active: false,
            running: HashSet::new(),
            parent: None,
            last: now,
        }
    }
}

/// Native activity of loaded threads. Subagents are released with their root.
#[derive(Default)]
struct Activity {
    threads: HashMap<String, Thread>,
}
impl Activity {
    /// The Host subscribed to, or started work in, this thread.
    fn touch(&mut self, id: &str, now: Instant) {
        let thread = self.thread(id, now);
        thread.subscribed = true;
        thread.last = now;
    }
    fn observe(&mut self, change: &AgentChange, now: Instant) {
        match change {
            AgentChange::TaskParent { session, parent }
                if session.provider == ProviderKind::Codex =>
            {
                self.thread(&session.id, now).parent = Some(parent.id.clone());
            }
            AgentChange::Session { session, change } if session.provider == ProviderKind::Codex => {
                let thread = self.thread(&session.id, now);
                thread.last = now;
                match change {
                    SessionChange::Status { status } => {
                        thread.active = *status == SessionStatus::Running;
                    }
                    SessionChange::Item { item, .. } if item.status == ItemStatus::Running => {
                        thread.running.insert(item.id.to_string());
                    }
                    SessionChange::Item { item, .. } => {
                        thread.running.remove(item.id.as_str());
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    /// Subscribed root threads whose whole tree has been silent long enough.
    fn due(&self, now: Instant) -> Vec<String> {
        self.threads
            .iter()
            .filter(|(id, thread)| thread.subscribed && thread.parent.is_none() && self.releasable(id, now))
            .map(|(id, _)| id.clone())
            .collect()
    }
    fn releasable(&self, id: &str, now: Instant) -> bool {
        let tree = self.tree(id);
        let Some(last) = tree.iter().map(|thread| thread.last).max() else {
            return false;
        };
        let silence = now.saturating_duration_since(last);
        let pinned = tree.iter().any(|thread| !thread.running.is_empty());
        !tree.iter().any(|thread| thread.active)
            && silence >= if pinned { MAX_PIN } else { IDLE }
    }
    fn forget(&mut self, id: &str) {
        let ids = self.tree_ids(id);
        self.threads.retain(|id, _| !ids.contains(id));
    }
    fn thread(&mut self, id: &str, now: Instant) -> &mut Thread {
        self.threads
            .entry(id.to_owned())
            .or_insert_with(|| Thread::new(now))
    }
    fn tree(&self, id: &str) -> Vec<&Thread> {
        self.tree_ids(id)
            .iter()
            .filter_map(|id| self.threads.get(id))
            .collect()
    }
    fn tree_ids(&self, root: &str) -> HashSet<String> {
        let mut ids = HashSet::from([root.to_owned()]);
        // Invalid ancestry cannot grow the set forever.
        loop {
            let children: Vec<_> = self
                .threads
                .iter()
                .filter(|(id, thread)| {
                    !ids.contains(*id)
                        && thread.parent.as_ref().is_some_and(|parent| ids.contains(parent))
                })
                .map(|(id, _)| id.clone())
                .collect();
            if children.is_empty() {
                return ids;
            }
            ids.extend(children);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_protocol::{
        items::{ItemBody, ItemContent},
        models::Item,
        session::SessionRef,
    };
    use std::sync::Arc;

    fn session(id: &str) -> SessionRef {
        SessionRef::new(ProviderKind::Codex, id.into()).unwrap()
    }
    fn status(id: &str, status: SessionStatus) -> AgentChange {
        AgentChange::Session {
            session: session(id),
            change: SessionChange::Status { status },
        }
    }
    fn item(id: &str, status: ItemStatus) -> AgentChange {
        AgentChange::Session {
            session: session(id),
            change: SessionChange::Item {
                turn_id: "turn".into(),
                item: Arc::new(Item {
                    id: "command".into(),
                    status,
                    client_input_id: None,
                    body: ItemContent::Inline {
                        body: Box::new(ItemBody::UserMessage {
                            text: None,
                            content: Vec::new(),
                        }),
                    },
                }),
            },
        }
    }

    #[test]
    fn an_idle_subscription_is_released_after_the_idle_timeout() {
        let start = Instant::now();
        let mut activity = Activity::default();
        activity.touch("root", start);
        activity.observe(&status("root", SessionStatus::Running), start);
        activity.observe(&status("root", SessionStatus::Idle), start);
        assert!(activity.due(start + IDLE - Duration::from_secs(1)).is_empty());
        assert_eq!(activity.due(start + IDLE), ["root"]);
        activity.forget("root");
        assert!(activity.due(start + MAX_PIN).is_empty());
    }

    #[test]
    fn running_turns_and_waiting_requests_are_never_released() {
        let start = Instant::now();
        let mut activity = Activity::default();
        activity.touch("root", start);
        activity.observe(&status("root", SessionStatus::Running), start);
        assert!(activity.due(start + MAX_PIN * 2).is_empty());
    }

    #[test]
    fn unsubscribed_threads_are_not_released() {
        let start = Instant::now();
        let mut activity = Activity::default();
        activity.observe(&status("other", SessionStatus::Idle), start);
        assert!(activity.due(start + MAX_PIN).is_empty());
    }

    #[test]
    fn background_work_pins_until_it_has_been_silent_for_the_pin_limit() {
        let start = Instant::now();
        let mut activity = Activity::default();
        activity.touch("root", start);
        activity.observe(&item("root", ItemStatus::Running), start);
        activity.observe(&status("root", SessionStatus::Idle), start);
        assert!(activity.due(start + IDLE).is_empty());
        // Reports from the work restart the pin budget.
        let report = start + MAX_PIN - Duration::from_secs(1);
        activity.observe(&status("root", SessionStatus::Idle), report);
        assert!(activity.due(start + MAX_PIN).is_empty());
        assert_eq!(activity.due(report + MAX_PIN), ["root"]);
        // Finished work leaves only the idle timeout.
        activity.observe(&item("root", ItemStatus::Completed), report);
        assert_eq!(activity.due(report + IDLE), ["root"]);
    }

    #[test]
    fn subagents_keep_their_root_and_are_released_only_with_it() {
        let start = Instant::now();
        let mut activity = Activity::default();
        activity.touch("root", start);
        activity.observe(
            &AgentChange::TaskParent {
                session: session("child"),
                parent: session("root"),
            },
            start,
        );
        activity.touch("child", start);
        activity.observe(&status("child", SessionStatus::Running), start);
        assert!(activity.due(start + IDLE * 2).is_empty());
        let finished = start + IDLE;
        activity.observe(&status("child", SessionStatus::Idle), finished);
        assert!(activity.due(finished + IDLE - Duration::from_secs(1)).is_empty());
        assert_eq!(activity.due(finished + IDLE), ["root"]);
        activity.forget("root");
        assert!(activity.threads.is_empty());
    }

    #[test]
    fn cyclic_ancestry_terminates() {
        let start = Instant::now();
        let mut activity = Activity::default();
        for (child, parent) in [("a", "b"), ("b", "a")] {
            activity.observe(
                &AgentChange::TaskParent {
                    session: session(child),
                    parent: session(parent),
                },
                start,
            );
        }
        assert_eq!(activity.tree_ids("a").len(), 2);
    }
}
