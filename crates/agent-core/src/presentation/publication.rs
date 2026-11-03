//! Shared publication decisions for native clients.
use crate::state::Snapshot;
use std::sync::Arc;

#[cfg_attr(feature = "bindings", derive(uniffi::Record))]
pub struct SnapshotChanges {
    pub changed: bool,
    pub list: bool,
    pub models: bool,
    pub conversation: bool,
    pub reconnected: bool,
}

#[cfg_attr(feature = "bindings", uniffi::export)]
impl Snapshot {
    pub fn unchanged(&self, other: Arc<Self>) -> bool {
        std::ptr::eq(self, other.as_ref())
    }
    pub fn list_unchanged(&self, other: Arc<Self>) -> bool {
        let same_list = match (&self.threads, &other.threads) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        self.recent_chats == other.recent_chats
            && same_list
            && Arc::ptr_eq(&self.account, &other.account)
            && Arc::ptr_eq(&self.activity, &other.activity)
            && self.waiting_sessions() == other.waiting_sessions()
            && Arc::ptr_eq(&self.expanded_projects, &other.expanded_projects)
            && Arc::ptr_eq(&self.project_preferences, &other.project_preferences)
            && self.storage_scope == other.storage_scope
            && Arc::ptr_eq(&self.project_threads, &other.project_threads)
            && Arc::ptr_eq(&self.archived_scopes, &other.archived_scopes)
            && self.connected == other.connected
            && [
                crate::state::operations::OperationKey::SessionList,
                crate::state::operations::OperationKey::SessionPage {
                    part: crate::models::ListPart::Projects,
                },
                crate::state::operations::OperationKey::SessionPage {
                    part: crate::models::ListPart::Chats,
                },
            ]
            .iter()
            .all(|key| {
                self.operations.get(key).map(|state| &state.phase)
                    == other.operations.get(key).map(|state| &state.phase)
            })
            && self.threads.as_ref().is_none_or(|list| {
                list.projects.iter().all(|project| {
                    let key = crate::state::operations::OperationKey::ProjectList {
                        project_id: project.id.clone(),
                    };
                    self.operations.get(&key).map(|state| &state.phase)
                        == other.operations.get(&key).map(|state| &state.phase)
                })
            })
    }
    pub fn conversation_unchanged(&self, other: Arc<Self>) -> bool {
        let id = self.navigation.thread_id.as_ref();
        if id != other.navigation.thread_id.as_ref()
            || self.navigation.draft_key != other.navigation.draft_key
        {
            return false;
        }
        let same_thread = match (
            id.and_then(|id| self.conversations.get(id)),
            id.and_then(|id| other.conversations.get(id)),
        ) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        same_thread && Arc::ptr_eq(&self.pending_submissions, &other.pending_submissions)
    }
    pub fn changes(&self, previous: Arc<Self>) -> SnapshotChanges {
        SnapshotChanges {
            changed: !self.unchanged(previous.clone()),
            list: !self.list_unchanged(previous.clone()),
            models: !Arc::ptr_eq(&self.models, &previous.models),
            conversation: !self.conversation_unchanged(previous.clone()),
            reconnected: self.connected
                && self.connection_generation != previous.connection_generation,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waiting_membership_refreshes_lists_without_republishing_other_surfaces() {
        use agent_protocol::{
            live_activity::{TaskActivityState, TaskActivitySummary},
            models::SessionStatus,
            session::{ProviderKind, SessionRef},
        };
        let id = SessionRef::new(ProviderKind::Codex, "question".into()).unwrap();
        let mut snapshot = Snapshot {
            threads: Some(Arc::new(
                serde_json::from_value(serde_json::json!({
                    "data":[{"id":id,"status":"running"}],
                    "projects":[],"hasMore":false,"hasMoreProjects":false,
                    "projectPages":{},"limit":5
                }))
                .unwrap(),
            )),
            ..Default::default()
        };
        Arc::make_mut(&mut snapshot.activity)
            .active
            .insert(id.clone(), true);
        for (revision, waiting, changed) in [
            (1, false, false),
            (2, true, true),
            (3, true, false),
            (4, false, true),
        ] {
            let before = Arc::new(snapshot.clone());
            let _ = snapshot.accept_task_activity(TaskActivityState {
                revision,
                statuses: vec![(id.clone(), SessionStatus::Running)],
                waiting: if waiting {
                    vec![id.clone()]
                } else {
                    Vec::new()
                },
                unread: Vec::new(),
                display: TaskActivitySummary {
                    waiting: u32::from(waiting),
                    running: u32::from(!waiting),
                    ..Default::default()
                }
                .display(),
            });
            assert!(Arc::ptr_eq(&snapshot.activity, &before.activity));
            assert!(Arc::ptr_eq(
                snapshot.threads.as_ref().unwrap(),
                before.threads.as_ref().unwrap()
            ));
            let publication = snapshot.changes(before);
            assert_eq!(publication.list, changed, "waiting={waiting}");
            assert!(!publication.models && !publication.conversation);
            assert_eq!(snapshot.thread_list().unwrap().threads[0].waiting, waiting);
        }
    }

    #[test]
    fn publication_changes_keep_independent_surfaces_and_connection_epochs_distinct() {
        let before = Arc::new(Snapshot::default());
        let duplicate = before.changes(before.clone());
        assert!(
            !duplicate.changed && !duplicate.list && !duplicate.models && !duplicate.conversation
        );
        assert!(!duplicate.reconnected);
        let mut next = (*before).clone();
        next.models = Arc::new(Vec::new());
        let catalog = next.changes(before.clone());
        assert!(catalog.changed && catalog.models);
        assert!(!catalog.list && !catalog.conversation && !catalog.reconnected);
        next = (*before).clone();
        next.error = Some("failed".into());
        assert!(next.changes(before.clone()).changed);
        next.connection_generation += 1;
        assert!(
            !next.changes(before.clone()).reconnected,
            "a disconnected epoch is not recovery"
        );
        next.connected = true;
        let connected = next.changes(before);
        assert!(connected.reconnected && connected.list);
        assert!(!connected.models && !connected.conversation);
    }
}
