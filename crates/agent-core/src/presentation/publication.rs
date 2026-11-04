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
            && (self
                .task_activity
                .as_ref()
                .zip(other.task_activity.as_ref())
                .is_some_and(|(a, b)| Arc::ptr_eq(a, b))
                || self.waiting_sessions() == other.waiting_sessions())
            && Arc::ptr_eq(&self.expanded_projects, &other.expanded_projects)
            && Arc::ptr_eq(&self.project_preferences, &other.project_preferences)
            && self.storage_scope == other.storage_scope
            && Arc::ptr_eq(&self.project_threads, &other.project_threads)
            && Arc::ptr_eq(&self.archived_scopes, &other.archived_scopes)
            && self.connected == other.connected
            && (Arc::ptr_eq(&self.operations, &other.operations) || {
                [
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
                }) && self.threads.as_ref().is_none_or(|list| {
                    list.projects.iter().all(|project| {
                        let key = crate::state::operations::OperationKey::ProjectList {
                            project_id: project.id.clone(),
                        };
                        self.operations.get(&key).map(|state| &state.phase)
                            == other.operations.get(&key).map(|state| &state.phase)
                    })
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
    fn list_publication_tracks_visible_operation_phases_without_reacting_to_generations() {
        use crate::{
            models::{ListPart, Project, ThreadList},
            state::operations::{OperationKey, OperationPhase, OperationState},
        };
        for count in [0, 1, 16, 512] {
            let projects: Vec<_> = (0..count)
                .map(|index| Project {
                    id: format!("project{index}"),
                    ..Default::default()
                })
                .collect();
            let keys: Vec<_> = [
                OperationKey::SessionList,
                OperationKey::SessionPage {
                    part: ListPart::Projects,
                },
                OperationKey::SessionPage {
                    part: ListPart::Chats,
                },
            ]
            .into_iter()
            .chain(projects.iter().map(|project| OperationKey::ProjectList {
                project_id: project.id.clone(),
            }))
            .collect();
            let previous = Arc::new(Snapshot {
                threads: Some(Arc::new(ThreadList {
                    revision: 1,
                    limit: 5,
                    data: Vec::new(),
                    projects,
                    has_more: false,
                    has_more_projects: false,
                    project_pages: Default::default(),
                    provider_errors: None,
                })),
                operations: Arc::new(
                    keys.iter()
                        .cloned()
                        .map(|key| {
                            (
                                key,
                                OperationState {
                                    generation: 1,
                                    phase: OperationPhase::Running,
                                },
                            )
                        })
                        .collect(),
                ),
                ..Default::default()
            });
            assert!(!previous.changes(previous.clone()).list);
            for key in keys.iter().take(3).chain(keys.last().filter(|_| count > 0)) {
                let mut next = previous.as_ref().clone();
                let state = Arc::make_mut(&mut next.operations).get_mut(key).unwrap();
                state.generation += 1;
                let generation = next.changes(previous.clone());
                assert!(generation.changed && !generation.list);
                Arc::make_mut(&mut next.operations)
                    .get_mut(key)
                    .unwrap()
                    .phase = OperationPhase::Failed {
                    message: "first failure".into(),
                };
                let failed = Arc::new(next.clone());
                let phase = next.changes(previous.clone());
                assert!(phase.list && !phase.models && !phase.conversation);
                Arc::make_mut(&mut next.operations)
                    .get_mut(key)
                    .unwrap()
                    .phase = OperationPhase::Failed {
                    message: "different failure".into(),
                };
                assert!(next.changes(failed).list);
                Arc::make_mut(&mut next.operations).remove(key);
                assert!(next.changes(previous.clone()).list);
            }
            for key in [
                OperationKey::WorkspaceReview,
                OperationKey::ProjectList {
                    project_id: "not-visible".into(),
                },
            ] {
                let mut next = previous.as_ref().clone();
                Arc::make_mut(&mut next.operations).insert(
                    key,
                    OperationState {
                        generation: 2,
                        phase: OperationPhase::Failed {
                            message: "unrelated".into(),
                        },
                    },
                );
                let changes = next.changes(previous.clone());
                assert!(
                    changes.changed && !changes.list && !changes.models && !changes.conversation
                );
            }
        }
    }

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
    #[test]
    fn waiting_publication_keeps_distinct_state_and_empty_state_comparisons() {
        use crate::session::{ProviderKind, SessionRef};
        use agent_protocol::live_activity::{TaskActivityState, TaskActivitySummary};
        let activity = |count| {
            Arc::new(TaskActivityState {
                revision: 1,
                statuses: vec![],
                waiting: (0..count)
                    .map(|index| {
                        SessionRef::new(ProviderKind::Codex, format!("task{index}")).unwrap()
                    })
                    .collect(),
                unread: vec![],
                display: TaskActivitySummary::default().display(),
            })
        };
        for count in [0, 1, 128, 4000] {
            let previous = Arc::new(Snapshot {
                task_activity: Some(activity(count)),
                ..Default::default()
            });
            let mut next = previous.as_ref().clone();
            assert!(next.list_unchanged(previous.clone()));
            Arc::make_mut(next.task_activity.as_mut().unwrap()).revision += 1;
            assert!(next.list_unchanged(previous.clone()));
            if count > 0 {
                Arc::make_mut(next.task_activity.as_mut().unwrap())
                    .waiting
                    .last_mut()
                    .unwrap()
                    .provider = ProviderKind::Claude;
                assert!(!next.list_unchanged(previous.clone()));
            }
            next.task_activity = None;
            assert_eq!(next.list_unchanged(previous.clone()), count == 0);
        }
    }
}
