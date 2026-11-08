//! Conversation chrome and actions, with no dependency on a draft's text.
use super::{
    conversation::HistoryControl,
    error::{ErrorNotice, ErrorScope},
};
use crate::state::{
    Navigation, Snapshot,
    operations::{OperationKey, OperationPhase},
};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, PartialEq)]
struct OperationControl {
    phase: OperationPhase,
    error: Option<super::error::ErrorText>,
}

#[derive(Clone, Default, PartialEq)]
#[cfg_attr(feature = "bindings", derive(uniffi::Object))]
pub struct ConversationContext {
    pub navigation: Navigation,
    pub connected: bool,
    pub epoch: u64,
    title: Option<String>,
    pub home: Option<String>,
    directory: String,
    pub history: Option<HistoryControl>,
    pub docked: Vec<super::conversation::Request>,
    notices: Vec<ErrorNotice>,
    operations: BTreeMap<OperationKey, OperationControl>,
    workspace_error: Option<super::error::ErrorText>,
}

impl ConversationContext {
    pub(crate) fn project(
        snapshot: &Snapshot,
        pane: &crate::state::PaneState,
        pane_id: &str,
        previous: Option<&Arc<Self>>,
    ) -> Arc<Self> {
        let session = pane.navigation.thread_id.as_ref();
        let next = Self {
            workspace_error: snapshot.operation_error(OperationKey::WorkspaceReview {
                pane: (pane_id).to_owned(),
            }),
            navigation: pane.navigation.as_ref().clone(),
            connected: snapshot.connection.connected,
            epoch: snapshot.connection.epoch,
            docked: session
                .into_iter()
                .flat_map(|id| snapshot.docked_requests(id.clone()))
                .filter(|request| {
                    !matches!(
                        request.request_body,
                        agent_protocol::requests::RequestBody::Question { .. }
                    )
                })
                .collect(),
            title: session.and_then(|id| {
                snapshot
                    .sessions
                    .conversations
                    .get(id)
                    .and_then(|thread| thread.name.clone())
                    .or_else(|| {
                        snapshot
                            .listed_threads()
                            .find(|thread| thread.id.as_ref() == Some(id))
                            .and_then(|thread| thread.name.clone())
                    })
            }),
            home: snapshot.connection.host_home_directory.clone(),
            directory: snapshot.selected_directory(pane),
            history: session.and_then(|id| snapshot.history_control(id.clone())),
            notices: snapshot.error_notices(
                pane_id,
                ErrorScope::Conversation {
                    draft_key: pane.navigation.draft_key.clone(),
                },
            ),
            operations: snapshot
                .operations
                .iter()
                .filter(|(key, _)| match key {
                    OperationKey::Fork {
                        session: target, ..
                    } => Some(target) == session,
                    OperationKey::Item { item } => Some(&item.thread_id) == session,
                    OperationKey::Request { id } => session
                        .and_then(|id| snapshot.sessions.conversations.get(id))
                        .is_some_and(|thread| thread.requests.contains_key(id)),
                    _ => false,
                })
                .map(|(key, state)| {
                    (
                        key.clone(),
                        OperationControl {
                            phase: state.phase.clone(),
                            error: snapshot.operation_error(key.clone()),
                        },
                    )
                })
                .collect(),
        };
        if let Some(previous) = previous
            && next == **previous
        {
            return previous.clone();
        }
        Arc::new(next)
    }
    pub fn operation_running(&self, key: &OperationKey) -> bool {
        self.operations
            .get(key)
            .is_some_and(|control| control.phase == OperationPhase::Running)
    }
    pub fn operation_error(&self, key: &OperationKey) -> Option<&super::error::ErrorText> {
        self.operations
            .get(key)
            .and_then(|control| control.error.as_ref())
    }
}

#[cfg_attr(feature = "bindings", uniffi::export)]
impl ConversationContext {
    pub fn navigation(&self) -> Navigation {
        self.navigation.clone()
    }
    pub fn title(&self) -> Option<String> {
        self.title.clone()
    }
    pub fn workspace_error(&self) -> Option<super::error::ErrorText> {
        self.workspace_error.clone()
    }
    pub fn home(&self) -> Option<String> {
        self.home.clone()
    }
    pub fn selected_directory(&self) -> String {
        self.directory.clone()
    }
    pub fn history(&self) -> Option<HistoryControl> {
        self.history.clone()
    }
    pub fn notices(&self) -> Vec<ErrorNotice> {
        self.notices.clone()
    }
    pub fn docked_request(&self) -> Option<super::conversation::Request> {
        self.docked.first().cloned()
    }
    pub fn request_phase(&self, id: agent_protocol::ids::RequestId) -> Option<OperationPhase> {
        self.operations
            .get(&OperationKey::Request { id })
            .map(|control| control.phase.clone())
    }
    pub fn request_error(
        &self,
        id: agent_protocol::ids::RequestId,
    ) -> Option<super::error::ErrorText> {
        self.operation_error(&OperationKey::Request { id }).cloned()
    }
    pub fn fork_phase(&self, turn: agent_protocol::ids::TurnId) -> Option<OperationPhase> {
        let session = self.navigation.thread_id.clone()?;
        self.operations
            .get(&OperationKey::Fork { session, turn })
            .map(|control| control.phase.clone())
    }
    pub fn fork_error(&self, turn: agent_protocol::ids::TurnId) -> Option<super::error::ErrorText> {
        let session = self.navigation.thread_id.clone()?;
        self.operation_error(&OperationKey::Fork { session, turn })
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::operations::{OperationState, ReadItem};
    use agent_protocol::{
        requests::{Request, RequestBody, RequestTarget},
        session::{ProviderKind, RequestDelivery, SessionRef},
    };

    #[test]
    fn actions_follow_the_selected_conversation_and_reuse_unchanged_controls() {
        let selected = SessionRef::new(ProviderKind::Codex, "selected".into()).unwrap();
        let other = SessionRef::new(ProviderKind::Codex, "other".into()).unwrap();
        let mut snapshot = Snapshot::default();
        Arc::make_mut(&mut Arc::make_mut(snapshot.panes.primary_mut()).navigation).thread_id =
            Some(selected.clone());
        let request = Request {
            id: "request".into(),
            target: RequestTarget::Session,
            delivery: RequestDelivery::Awaiting,
            body: RequestBody::ToolExecution {
                tool: "tool".into(),
                namespace: None,
                arguments: serde_json::json!({}),
            },
        };
        let question = Request {
            id: "question".into(),
            body: RequestBody::Question {
                questions: Vec::new(),
            },
            ..request.clone()
        };
        Arc::make_mut(&mut snapshot.sessions.conversations).insert(
            selected.clone(),
            Arc::new(crate::models::Thread {
                id: Some(selected.clone()),
                requests: [request, question]
                    .into_iter()
                    .map(|request| (request.id.clone(), Arc::new(request)))
                    .collect(),
                ..Default::default()
            }),
        );
        let fork = OperationKey::Fork {
            session: selected.clone(),
            turn: "turn".into(),
        };
        let item = OperationKey::Item {
            item: ReadItem {
                thread_id: selected,
                turn_id: "turn".into(),
                item_id: "item".into(),
            },
        };
        let request = OperationKey::Request {
            id: "request".into(),
        };
        for key in [&fork, &item, &request] {
            Arc::make_mut(&mut snapshot.operations).insert(
                key.clone(),
                OperationState {
                    generation: 1,
                    phase: OperationPhase::Running,
                },
            );
        }
        let initial = ConversationContext::project(&snapshot, snapshot.panes.primary(), "", None);
        assert_eq!(
            initial.docked.len(),
            1,
            "questions use the composer's field"
        );
        assert_eq!(initial.docked[0].id.as_str(), "request");
        for key in [&fork, &item, &request] {
            assert!(initial.operation_running(key));
            assert!(initial.operation_error(key).is_none());
        }
        assert_eq!(
            initial.fork_phase("turn".into()),
            Some(OperationPhase::Running)
        );

        for key in [
            OperationKey::Fork {
                session: other.clone(),
                turn: "turn".into(),
            },
            OperationKey::Item {
                item: ReadItem {
                    thread_id: other,
                    turn_id: "turn".into(),
                    item_id: "item".into(),
                },
            },
            OperationKey::Request {
                id: "unrelated".into(),
            },
        ] {
            Arc::make_mut(&mut snapshot.operations).insert(
                key.clone(),
                OperationState {
                    generation: 2,
                    phase: OperationPhase::Running,
                },
            );
            let next = ConversationContext::project(
                &snapshot,
                snapshot.panes.primary(),
                "",
                Some(&initial),
            );
            assert!(Arc::ptr_eq(&initial, &next));
            assert!(!next.operation_running(&key));
        }
        for key in [&fork, &item, &request] {
            Arc::make_mut(&mut snapshot.operations)
                .get_mut(key)
                .unwrap()
                .generation += 1;
        }
        assert!(Arc::ptr_eq(
            &initial,
            &ConversationContext::project(&snapshot, snapshot.panes.primary(), "", Some(&initial)),
        ));
        for key in [&fork, &item, &request] {
            Arc::make_mut(&mut snapshot.operations)
                .get_mut(key)
                .unwrap()
                .phase = OperationPhase::Failed {
                message: "failure".into(),
            };
        }
        let failed =
            ConversationContext::project(&snapshot, snapshot.panes.primary(), "", Some(&initial));
        for key in [&fork, &item, &request] {
            assert!(!failed.operation_running(key));
            assert_eq!(
                failed.operation_error(key),
                snapshot.operation_error(key.clone()).as_ref()
            );
        }
        assert!(failed.fork_error("turn".into()).is_some());
        Arc::make_mut(&mut Arc::make_mut(snapshot.panes.primary_mut()).navigation).thread_id = None;
        let empty =
            ConversationContext::project(&snapshot, snapshot.panes.primary(), "", Some(&failed));
        assert!(empty.fork_phase("turn".into()).is_none());
        assert!(empty.fork_error("turn".into()).is_none());
        for key in [&fork, &item, &request] {
            assert!(empty.operation_error(key).is_none());
        }
    }
}
