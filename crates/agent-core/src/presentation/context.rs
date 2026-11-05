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

#[derive(Clone, PartialEq)]
struct ForkControl {
    turn: agent_protocol::ids::TurnId,
    phase: OperationPhase,
}

#[derive(Clone, PartialEq)]
#[cfg_attr(feature = "bindings", derive(uniffi::Object))]
pub struct ConversationContext {
    navigation: Navigation,
    home: Option<String>,
    directory: String,
    history: Option<HistoryControl>,
    history_notice: Option<String>,
    notices: Vec<ErrorNotice>,
    forks: Vec<ForkControl>,
}

impl ConversationContext {
    pub(crate) fn project(snapshot: &Snapshot, previous: Option<&Arc<Self>>) -> Arc<Self> {
        let session = snapshot.pane.navigation.thread_id.as_ref();
        let next = Self {
            navigation: snapshot.pane.navigation.as_ref().clone(),
            home: snapshot.connection.host_home_directory.clone(),
            directory: snapshot.selected_directory(),
            history: session.and_then(|id| snapshot.history_control(id.clone())),
            history_notice: session
                .and_then(|id| snapshot.sessions.conversations.get(id))
                .and_then(|thread| super::conversation::history_notice(thread)),
            notices: snapshot.error_notices(ErrorScope::Conversation {
                draft_key: snapshot.pane.navigation.draft_key.clone(),
            }),
            forks: snapshot
                .operations
                .iter()
                .filter_map(|(key, state)| match key {
                    OperationKey::Fork {
                        session: target,
                        turn,
                    } if Some(target) == session => Some(ForkControl {
                        turn: turn.clone(),
                        phase: state.phase.clone(),
                    }),
                    _ => None,
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
        self.forks
            .iter()
            .find(|entry| entry.turn == turn)
            .map(|entry| entry.phase.clone())
    }
    pub fn fork_error(&self, turn: agent_protocol::ids::TurnId) -> Option<super::error::ErrorText> {
        match self.fork_phase(turn) {
            Some(OperationPhase::Failed { message }) => Some(super::error::error_text(&message)),
            _ => None,
        }
    }
}
