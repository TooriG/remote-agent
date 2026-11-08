//! Conversation chrome and actions, with no dependency on a draft's text.
use super::{
    conversation::HistoryControl,
    error::{ErrorNotice, ErrorScope},
};
use crate::state::{
    Navigation, Snapshot,
    operations::{OperationKey, OperationPhase},
};
use std::sync::Arc;

/// A turn's own control: forking from it, or loading the work its folded
/// row expands to. Its progress and failure show beside that control.
#[derive(Clone, Copy, PartialEq)]
enum TurnAction {
    Fork,
    Items,
}

#[derive(Clone, PartialEq)]
struct TurnControl {
    action: TurnAction,
    turn: agent_protocol::ids::TurnId,
    phase: OperationPhase,
}

/// What the pane shows for its selected conversation while it has no history
/// to render: never a new chat's prompt, which would read as a different one.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "bindings", derive(uniffi::Enum))]
pub enum ConversationOpening {
    /// Its history is on its way from the Host.
    Loading,
    /// Reading its history failed; opening it again retries.
    Failed { error: super::error::ErrorText },
}

impl ConversationOpening {
    fn project(snapshot: &Snapshot) -> Option<Self> {
        let session = snapshot.pane.navigation.thread_id.clone()?;
        if snapshot.conversation_thread().is_some() {
            return None;
        }
        Some(
            match snapshot.operation_error(OperationKey::History { session }) {
                Some(error) => Self::Failed { error },
                None => Self::Loading,
            },
        )
    }
}

#[derive(Clone, PartialEq)]
#[cfg_attr(feature = "bindings", derive(uniffi::Object))]
pub struct ConversationContext {
    navigation: Navigation,
    opening: Option<ConversationOpening>,
    title: Option<String>,
    home: Option<String>,
    directory: String,
    history: Option<HistoryControl>,
    history_notice: Option<String>,
    notices: Vec<ErrorNotice>,
    turn_controls: Vec<TurnControl>,
    workspace_error: Option<super::error::ErrorText>,
}

impl ConversationContext {
    pub(crate) fn project(snapshot: &Snapshot, previous: Option<&Arc<Self>>) -> Arc<Self> {
        let session = snapshot.pane.navigation.thread_id.as_ref();
        let next = Self {
            workspace_error: snapshot.operation_error(OperationKey::WorkspaceReview),
            navigation: snapshot.pane.navigation.as_ref().clone(),
            opening: ConversationOpening::project(snapshot),
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
            directory: snapshot.selected_directory(),
            history: session.and_then(|id| snapshot.history_control(id.clone())),
            history_notice: session
                .and_then(|id| snapshot.sessions.conversations.get(id))
                .and_then(|thread| super::conversation::history_notice(thread)),
            notices: snapshot.error_notices(ErrorScope::Conversation {
                draft_key: snapshot.pane.navigation.draft_key.clone(),
            }),
            turn_controls: snapshot
                .operations
                .iter()
                .filter_map(|(key, state)| {
                    let (action, target, turn) = match key {
                        OperationKey::Fork { session, turn } => (TurnAction::Fork, session, turn),
                        OperationKey::TurnItems { session, turn } => {
                            (TurnAction::Items, session, turn)
                        }
                        _ => return None,
                    };
                    (Some(target) == session).then(|| TurnControl {
                        action,
                        turn: turn.clone(),
                        phase: state.phase.clone(),
                    })
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
}

#[cfg_attr(feature = "bindings", uniffi::export)]
impl ConversationContext {
    #[cfg_attr(feature = "bindings", uniffi::constructor)]
    pub fn from_snapshot(snapshot: Arc<Snapshot>) -> Arc<Self> {
        Self::project(&snapshot, None)
    }
    pub fn navigation(&self) -> Navigation {
        self.navigation.clone()
    }
    pub fn opening(&self) -> Option<ConversationOpening> {
        self.opening.clone()
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
    pub fn history_notice(&self) -> Option<String> {
        self.history_notice.clone()
    }
    pub fn notices(&self) -> Vec<ErrorNotice> {
        self.notices.clone()
    }
    pub fn fork_phase(&self, turn: agent_protocol::ids::TurnId) -> Option<OperationPhase> {
        self.turn_phase(TurnAction::Fork, &turn)
    }
    pub fn fork_error(&self, turn: agent_protocol::ids::TurnId) -> Option<super::error::ErrorText> {
        failure(self.turn_phase(TurnAction::Fork, &turn))
    }
    /// Why the work a folded turn expands to could not be loaded.
    pub fn turn_items_error(
        &self,
        turn: agent_protocol::ids::TurnId,
    ) -> Option<super::error::ErrorText> {
        failure(self.turn_phase(TurnAction::Items, &turn))
    }
}

impl ConversationContext {
    fn turn_phase(
        &self,
        action: TurnAction,
        turn: &agent_protocol::ids::TurnId,
    ) -> Option<OperationPhase> {
        self.turn_controls
            .iter()
            .find(|entry| entry.action == action && &entry.turn == turn)
            .map(|entry| entry.phase.clone())
    }
}

fn failure(phase: Option<OperationPhase>) -> Option<super::error::ErrorText> {
    match phase {
        Some(OperationPhase::Failed { message }) => Some(super::error::error_text(&message)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        models,
        session::{ProviderKind, SessionRef},
        state::operations::OperationState,
    };

    #[test]
    fn a_selected_conversation_without_history_loads_then_offers_a_retry() {
        let session = SessionRef::new(ProviderKind::Codex, "opening".into()).unwrap();
        let mut snapshot = Snapshot::default();
        let opening = |snapshot: &Snapshot| ConversationContext::project(snapshot, None).opening();
        assert_eq!(opening(&snapshot), None, "a new chat opens nothing");

        Arc::make_mut(&mut Arc::make_mut(&mut snapshot.pane).navigation).thread_id =
            Some(session.clone());
        assert_eq!(opening(&snapshot), Some(ConversationOpening::Loading));
        let key = OperationKey::History {
            session: session.clone(),
        };
        snapshot.operations.insert(
            key.clone(),
            OperationState {
                generation: 1,
                phase: OperationPhase::Running,
            },
        );
        assert_eq!(opening(&snapshot), Some(ConversationOpening::Loading));
        snapshot.operations.insert(
            key.clone(),
            OperationState {
                generation: 2,
                phase: OperationPhase::Failed {
                    message: "history unavailable".into(),
                },
            },
        );
        assert_eq!(
            opening(&snapshot),
            Some(ConversationOpening::Failed {
                error: crate::presentation::error::error_text("history unavailable")
            })
        );

        Arc::make_mut(&mut snapshot.sessions.conversations)
            .insert(session, Arc::new(models::Thread::default()));
        assert_eq!(opening(&snapshot), None, "its history shows instead");
    }

    #[test]
    fn a_turns_fork_and_work_load_report_their_own_progress_and_failure() {
        let session = SessionRef::new(ProviderKind::Codex, "selected".into()).unwrap();
        let other = SessionRef::new(ProviderKind::Codex, "other".into()).unwrap();
        let mut snapshot = Snapshot::default();
        Arc::make_mut(&mut Arc::make_mut(&mut snapshot.pane).navigation).thread_id =
            Some(session.clone());
        let failed = |message: &str| OperationState {
            generation: 1,
            phase: OperationPhase::Failed {
                message: message.into(),
            },
        };
        snapshot.operations.insert(
            OperationKey::TurnItems {
                session: session.clone(),
                turn: "work".into(),
            },
            failed("items unavailable"),
        );
        snapshot.operations.insert(
            OperationKey::Fork {
                session: session.clone(),
                turn: "work".into(),
            },
            OperationState {
                generation: 2,
                phase: OperationPhase::Running,
            },
        );
        snapshot.operations.insert(
            OperationKey::TurnItems {
                session: other,
                turn: "elsewhere".into(),
            },
            failed("other conversation"),
        );
        let context = ConversationContext::project(&snapshot, None);
        assert_eq!(
            context.turn_items_error("work".into()),
            Some(crate::presentation::error::error_text("items unavailable"))
        );
        assert_eq!(context.fork_error("work".into()), None);
        assert_eq!(
            context.fork_phase("work".into()),
            Some(OperationPhase::Running)
        );
        assert_eq!(
            context.turn_items_error("elsewhere".into()),
            None,
            "another conversation's load stays with it"
        );
    }
}
