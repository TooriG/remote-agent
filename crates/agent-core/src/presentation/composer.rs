//! A composer's own inputs and controls, independent of transcript deltas.
use super::{conversation::Request, error::ErrorText, model_settings::ModelQuickControls};
use crate::{
    models::{Model, ModelRef},
    session::ProviderKind,
    state::{
        Draft, Navigation, Same, Snapshot,
        operations::{OperationKey, OperationPhase},
    },
};
use std::sync::{Arc, OnceLock};

#[cfg(test)]
mod tests;

#[derive(Clone, PartialEq)]
#[cfg_attr(feature = "bindings", derive(uniffi::Record))]
pub struct RequestControl {
    pub id: agent_protocol::ids::RequestId,
    pub phase: Option<OperationPhase>,
}

#[derive(Clone, PartialEq)]
#[cfg_attr(feature = "bindings", derive(uniffi::Record))]
pub struct ComposerControls {
    pub navigation: Navigation,
    pub connected: bool,
    pub provider: Option<ProviderKind>,
    pub selected_model: Option<ModelRef>,
    pub quick: ModelQuickControls,
    pub sending: bool,
    pub transferring: bool,
    pub transcribing: bool,
    pub running_turn: Option<agent_protocol::ids::TurnId>,
    /// The work the answered running turn waits on. As in T3 Code, the
    /// composer sends while it waits; the work's own strip offers Stop.
    pub background: Option<super::background_work::BackgroundWork>,
    pub interrupt_phase: Option<OperationPhase>,
    pub unavailable: Option<String>,
    pub question: Option<crate::questions::QuestionPrompt>,
    pub request: Option<Request>,
    pub requests: Vec<RequestControl>,
}

#[cfg_attr(feature = "bindings", derive(uniffi::Object))]
pub struct ComposerPresentation {
    pub draft: Arc<Draft>,
    pub controls: ComposerControls,
    models: Arc<Vec<Model>>,
    catalog: Option<Arc<agent_protocol::composer::ComposerCatalog>>,
}

impl ComposerPresentation {
    pub(crate) fn equivalent(&self, other: &Self) -> bool {
        self.draft.same(&other.draft)
            && self.controls == other.controls
            && self.models.same(&other.models)
            && self.catalog.same(&other.catalog)
    }
    pub(crate) fn project(snapshot: &Snapshot, previous: Option<&Arc<Self>>) -> Arc<Self> {
        static EMPTY: OnceLock<Arc<Draft>> = OnceLock::new();
        let key = &snapshot.pane.navigation.draft_key;
        let draft = snapshot
            .input
            .drafts
            .get(key)
            .cloned()
            .unwrap_or_else(|| EMPTY.get_or_init(Arc::default).clone());
        let session = snapshot.pane.navigation.thread_id.as_ref();
        let thread = session.and_then(|id| snapshot.sessions.conversations.get(id));
        let running_turn = thread.and_then(|thread| thread.active_turn_id());
        let provider = snapshot.model_provider_for_draft(key.clone());
        let requests = session
            .map(|id| snapshot.docked_requests(id.clone()))
            .unwrap_or_default();
        let controls = ComposerControls {
            navigation: snapshot.pane.navigation.as_ref().clone(),
            connected: snapshot.connection.connected,
            provider,
            selected_model: draft.model.clone().or_else(|| {
                provider.and_then(|provider| snapshot.model_for_provider(key.clone(), provider))
            }),
            quick: snapshot.model_quick_controls(key.clone()),
            sending: snapshot.operation_running(OperationKey::Submission {
                draft_key: key.clone(),
            }),
            transferring: snapshot.operation_running(OperationKey::Attachment {
                draft_key: key.clone(),
            }),
            transcribing: snapshot.operation_running(OperationKey::Dictation {
                draft_key: key.clone(),
            }),
            interrupt_phase: session
                .zip(running_turn.as_ref())
                .and_then(|(session, turn)| {
                    snapshot.operation_phase(OperationKey::Interrupt {
                        session: session.clone(),
                        turn: turn.clone(),
                    })
                }),
            running_turn,
            background: thread
                .and_then(|thread| super::background_work::BackgroundWork::new(&thread.background)),
            unavailable: session
                .and_then(|id| snapshot.input_unavailable_reason(id.clone(), provider)),
            question: session.and_then(|id| snapshot.question_prompt(id.clone())),
            request: requests
                .iter()
                .find(|request| {
                    !matches!(
                        request.request_body,
                        agent_protocol::requests::RequestBody::Question { .. }
                    )
                })
                .cloned(),
            requests: requests
                .iter()
                .map(|request| RequestControl {
                    id: request.id.clone(),
                    phase: snapshot.operation_phase(OperationKey::Request {
                        id: request.id.clone(),
                    }),
                })
                .collect(),
        };
        let catalog = snapshot
            .catalog
            .composer_catalogs
            .get(&snapshot.pane.navigation.cwd)
            .cloned();
        let next = Self {
            draft,
            controls,
            models: snapshot.catalog.models.clone(),
            catalog,
        };
        if let Some(previous) = previous
            && next.equivalent(previous)
        {
            return previous.clone();
        }
        Arc::new(next)
    }
}

/// The changed files a conversation's composer offers to review, as T3 Code's
/// card above it: the totals, the listed files, and how many the folded card hides.
#[derive(Debug, Clone, PartialEq)]
pub struct ComposerReview {
    pub files: usize,
    pub additions: u64,
    pub deletions: u64,
    pub rows: Vec<agent_protocol::models::ChangedFile>,
    /// The files past the folded card's rows, when there are any: the toggle's count.
    pub folded: Option<usize>,
}

/// The files the card lists until it is expanded.
const REVIEW_ROWS: usize = 3;

impl Snapshot {
    /// The card for the open conversation's workspace changes; none for a draft
    /// or a clean workspace.
    pub fn composer_review(&self, expanded: bool) -> Option<ComposerReview> {
        self.pane.navigation.thread_id.as_ref()?;
        let review = self
            .pane
            .workspace
            .review
            .as_ref()
            .filter(|review| !review.files.is_empty())?;
        let files = review.files.len();
        let shown = if expanded {
            files
        } else {
            files.min(REVIEW_ROWS)
        };
        Some(ComposerReview {
            files,
            additions: review.additions,
            deletions: review.deletions,
            rows: review.files[..shown].to_vec(),
            folded: files.checked_sub(REVIEW_ROWS).filter(|more| *more > 0),
        })
    }

    pub(crate) fn submission_unavailable_reason(
        &self,
        session: &crate::session::SessionRef,
        selected: Option<ProviderKind>,
    ) -> Option<String> {
        let thread = self.thread_metadata(session)?;
        crate::session::direct_input_unavailable_reason(thread.can_accept_direct_input).or_else(|| {
            let active = thread.active_provider.unwrap_or(session.provider);
            let busy = agent_protocol::models::task_active(None, thread.status)
                || thread.active_turn_id().is_some()
                || !thread.requests.is_empty()
                || thread.preparation.is_some();
            if selected.is_some_and(|provider| provider != active) && busy {
                Some("AIを切り替えて送信するには、現在の応答・確認・送信の完了を待つか、新しいチャットを開いてください。".into())
            } else {
                None
            }
        })
    }
}

#[cfg_attr(feature = "bindings", uniffi::export)]
impl Snapshot {
    pub fn input_unavailable_reason(
        &self,
        session: crate::session::SessionRef,
        selected: Option<ProviderKind>,
    ) -> Option<String> {
        self.submission_unavailable_reason(&session, selected)
            .or_else(|| {
                self.thread_metadata(&session)
                    .and_then(crate::session::input_unavailable_reason)
            })
    }
}

#[cfg_attr(feature = "bindings", uniffi::export)]
impl ComposerPresentation {
    #[cfg_attr(feature = "bindings", uniffi::constructor)]
    pub fn from_snapshot(snapshot: Arc<Snapshot>) -> Arc<Self> {
        Self::project(&snapshot, None)
    }
    pub fn draft(&self) -> Draft {
        self.draft.as_ref().clone()
    }
    pub fn controls(&self) -> ComposerControls {
        self.controls.clone()
    }
    pub fn current_model(&self) -> Option<Model> {
        super::model_settings::current_model(&self.models, self.draft.model.as_ref()).cloned()
    }
    /// Recording is native UI state; all Host and draft readiness is owned here.
    pub fn can_submit(&self, recording: bool) -> bool {
        let controls = &self.controls;
        controls.connected
            && controls.provider.is_some()
            && !controls.sending
            && !controls.transferring
            && !controls.transcribing
            && controls.unavailable.is_none()
            && (recording
                || !self.draft.text.trim().is_empty()
                || !self.draft.attachments.is_empty())
    }
    pub fn can_interrupt(&self) -> bool {
        self.controls.connected
            && self.controls.running_turn.is_some()
            && self.controls.interrupt_phase != Some(OperationPhase::Running)
    }
    pub fn can_advance_question(&self) -> bool {
        self.controls.connected
            && self.controls.question.as_ref().is_some_and(|question| {
                question.can_advance
                    && self.request_phase(question.request_id.clone())
                        != Some(OperationPhase::Running)
            })
    }
    pub fn request_phase(&self, id: agent_protocol::ids::RequestId) -> Option<OperationPhase> {
        self.controls
            .requests
            .iter()
            .find(|request| request.id == id)
            .and_then(|request| request.phase.clone())
    }
    pub fn request_error(&self, id: agent_protocol::ids::RequestId) -> Option<ErrorText> {
        match self.request_phase(id) {
            Some(OperationPhase::Failed { message }) => Some(super::error::error_text(&message)),
            _ => None,
        }
    }
    pub fn composer_suggestions(
        &self,
        text: String,
        cursor: u32,
    ) -> Option<crate::composer::ComposerSuggestions> {
        crate::composer::suggestions(
            self.catalog.as_deref(),
            self.controls.provider,
            text,
            cursor,
        )
    }
    pub fn unchanged(&self, other: Arc<Self>) -> bool {
        self.equivalent(&other)
    }
}
