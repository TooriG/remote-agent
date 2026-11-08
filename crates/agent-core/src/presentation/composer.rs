//! A composer's own inputs and controls, independent of transcript deltas.
use super::model_settings::ModelQuickControls;
use crate::{
    models::{Model, ModelRef},
    session::ProviderKind,
    state::{
        ComposerPreferences, Draft, Navigation, Same, Snapshot, follow_up,
        operations::{OperationKey, OperationPhase},
    },
};
use agent_protocol::{ids::ClientInputId, operations::FollowUp};
use std::sync::{Arc, OnceLock};

pub mod footer;
#[cfg(test)]
mod tests;

#[derive(Clone, PartialEq)]
#[cfg_attr(feature = "bindings", derive(uniffi::Record))]
pub struct ComposerControls {
    pub navigation: Navigation,
    pub connected: bool,
    /// The selected conversation is loaded, or the draft starts a new one. One
    /// still opening, or whose read failed, takes typing but nothing that would
    /// send into it: no message, recording or attachment.
    pub loaded: bool,
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
    /// Where the docked question's answer is in its delivery.
    pub question_phase: Option<OperationPhase>,
    pub follow_up: FollowUpControls,
}

/// What sending does while a turn runs and the messages waiting behind it, as
/// T3 Code's composer and the queue above it.
#[derive(Clone, PartialEq)]
#[cfg_attr(feature = "bindings", derive(uniffi::Record))]
pub struct FollowUpControls {
    pub preferences: ComposerPreferences,
    /// A send now goes into or behind a running turn, or behind waiting messages.
    pub active: bool,
    /// What a plain send does, and what the alternate does, as the provider allows.
    pub default_action: FollowUp,
    pub alternate_action: FollowUp,
    pub queue: Vec<QueuedMessage>,
    /// A stop or failure holds the queue until it is resumed.
    pub held: bool,
    /// Waiting messages can steer the running turn instead.
    pub can_steer: bool,
    /// The waiting message the composer edits; sending saves it.
    pub editing: Option<ClientInputId>,
    /// A queue change is on its way to the Host.
    pub busy: bool,
}

#[derive(Clone, PartialEq)]
#[cfg_attr(feature = "bindings", derive(uniffi::Record))]
pub struct QueuedMessage {
    pub id: ClientInputId,
    pub text: String,
    /// Images and files sent with it.
    pub attachments: u32,
    /// Sent, and not yet in the Host's queue.
    pub pending: bool,
}

fn follow_up_controls(
    snapshot: &Snapshot,
    thread: Option<&crate::models::Thread>,
) -> FollowUpControls {
    let preferences = snapshot.input.composer_preferences;
    let action = |alternate| {
        let requested = follow_up::requested(preferences.follow_up, alternate);
        if thread.is_some_and(|thread| follow_up::waits(thread, requested)) {
            FollowUp::Queue
        } else {
            FollowUp::Steer
        }
    };
    let Some(thread) = thread else {
        return FollowUpControls {
            preferences,
            active: false,
            default_action: preferences.follow_up,
            alternate_action: follow_up::requested(preferences.follow_up, true),
            queue: Vec::new(),
            held: false,
            can_steer: false,
            editing: None,
            busy: false,
        };
    };
    let session = thread.id.as_ref();
    let waiting = thread.queue.inputs.iter().map(|input| QueuedMessage {
        id: input.client_user_message_id.clone(),
        text: follow_up::queued_text(input),
        attachments: input
            .input
            .iter()
            .filter(|part| !matches!(part, agent_protocol::operations::Input::Text { .. }))
            .count() as u32,
        pending: false,
    });
    let key = session.map(crate::state::DraftKey::from);
    let mut sending: Vec<_> = snapshot
        .sessions
        .pending_submissions
        .iter()
        .filter(|(id, pending)| {
            pending.queued
                && !pending.accepted
                && Some(&pending.draft_key) == key.as_ref()
                && !thread
                    .queue
                    .inputs
                    .iter()
                    .any(|input| &input.client_user_message_id == *id)
        })
        .collect();
    sending.sort_by_key(|(_, pending)| pending.sequence);
    FollowUpControls {
        preferences,
        active: follow_up::working(thread)
            || (!thread.queue.held && !thread.queue.inputs.is_empty()),
        default_action: action(false),
        alternate_action: action(true),
        queue: waiting
            .chain(sending.into_iter().map(|(id, pending)| QueuedMessage {
                id: id.clone(),
                text: pending.draft.text.clone(),
                attachments: pending.draft.attachments.len() as u32,
                pending: true,
            }))
            .collect(),
        held: thread.queue.held,
        can_steer: thread.active_turn_id().is_some() && follow_up::steers(thread),
        editing: session.and_then(|session| {
            snapshot
                .input
                .queued_edits
                .get(session)
                .map(|edit| edit.client_user_message_id.clone())
        }),
        busy: session.is_some_and(|session| {
            snapshot.operation_running(OperationKey::Queue {
                session: session.clone(),
            })
        }),
    }
}

#[cfg_attr(feature = "bindings", derive(uniffi::Object))]
pub struct ComposerPresentation {
    language: crate::localization::UiLanguage,
    pub draft: Arc<Draft>,
    pub controls: ComposerControls,
    models: Arc<Vec<Model>>,
    catalog: Option<Arc<agent_protocol::composer::ComposerCatalog>>,
}

impl ComposerPresentation {
    pub(crate) fn equivalent(&self, other: &Self) -> bool {
        self.language == other.language
            && self.draft.same(&other.draft)
            && self.controls == other.controls
            && self.models.same(&other.models)
            && self.catalog.same(&other.catalog)
    }
    pub(crate) fn project(
        snapshot: &Snapshot,
        pane: &crate::state::PaneState,
        previous: Option<&Arc<Self>>,
    ) -> Arc<Self> {
        static EMPTY: OnceLock<Arc<Draft>> = OnceLock::new();
        let key = &pane.navigation.draft_key;
        let draft = snapshot
            .input
            .drafts
            .get(key)
            .cloned()
            .unwrap_or_else(|| EMPTY.get_or_init(Arc::default).clone());
        let session = pane.navigation.thread_id.as_ref();
        let thread = session.and_then(|id| snapshot.sessions.conversations.get(id));
        let running_turn = thread.and_then(|thread| thread.active_turn_id());
        let provider = snapshot.model_provider_for_draft(key.clone());
        let question = session.and_then(|id| snapshot.question_prompt(id.clone()));
        let controls = ComposerControls {
            navigation: pane.navigation.as_ref().clone(),
            connected: snapshot.connection.connected,
            loaded: session.is_none() || thread.is_some(),
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
            question_phase: question.as_ref().and_then(|question| {
                snapshot.operation_phase(OperationKey::Request {
                    id: question.request_id.clone(),
                })
            }),
            question,
            follow_up: follow_up_controls(snapshot, thread.map(AsRef::as_ref)),
        };
        let catalog = snapshot
            .catalog
            .composer_catalogs
            .get(&pane.navigation.cwd)
            .cloned();
        let next = Self {
            language: snapshot.catalog.ui_language,
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

/// Whether a card this wide names its review action beside the icon, as T3
/// Code's changed-files card does from 24rem (`@[24rem]/changed-files`);
/// narrower, the action is its icon and the totals keep the room.
pub fn review_action_labelled(card_width: f32) -> bool {
    card_width >= 384.
}

impl crate::state::PaneState {
    /// The card for the open conversation's workspace changes; none for a draft
    /// or a clean workspace.
    pub fn composer_review(&self, expanded: bool) -> Option<ComposerReview> {
        self.navigation.thread_id.as_ref()?;
        let review = self
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
}

impl Snapshot {
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
            .map(|reason| self.catalog.ui_language.text(&reason).to_owned())
    }
}

#[cfg_attr(feature = "bindings", uniffi::export)]
impl ComposerPresentation {
    pub fn draft(&self) -> Draft {
        self.draft.as_ref().clone()
    }
    pub fn controls(&self) -> ComposerControls {
        self.controls.clone()
    }
    pub fn current_model(&self) -> Option<Model> {
        super::model_settings::current_model(&self.models, self.draft.model.as_ref()).cloned()
    }
    pub fn can_interrupt(&self) -> bool {
        self.controls.connected
            && self.controls.running_turn.is_some()
            && self.controls.interrupt_phase != Some(OperationPhase::Running)
    }
    /// Whether the docked question's action answers now: it can, the Host is
    /// reached, and its last answer is not still being delivered.
    pub fn can_advance_question(&self) -> bool {
        self.controls.connected
            && self.controls.question_phase != Some(OperationPhase::Running)
            && self
                .controls
                .question
                .as_ref()
                .is_some_and(|question| question.can_advance)
    }
    pub fn composer_suggestions(
        &self,
        text: String,
        cursor: u32,
    ) -> Option<crate::composer::ComposerSuggestions> {
        crate::composer::suggestions(
            self.language,
            self.catalog.as_deref(),
            self.controls.provider,
            text,
            cursor,
        )
    }
    pub fn unchanged(&self, other: Arc<Self>) -> bool {
        self.equivalent(&other)
    }
    /// Whether core state lets the composer send `has_content`. Clients add
    /// their own media transfer, recording, and permission state.
    pub fn can_send(&self, has_content: bool) -> bool {
        has_content
            && self.accepts_input()
            && self.controls.provider.is_some()
            && self.controls.unavailable.is_none()
    }
    /// Transcription only fills the draft, so it does not wait for a provider.
    pub fn can_dictate(&self) -> bool {
        self.accepts_input()
    }
    pub fn can_attach(&self) -> bool {
        self.accepts_input()
    }
}

impl ComposerPresentation {
    fn accepts_input(&self) -> bool {
        let controls = &self.controls;
        controls.connected
            && controls.loaded
            && !controls.sending
            && !controls.transferring
            && !controls.transcribing
    }
}

#[cfg(test)]
mod availability_tests {
    use super::*;
    use crate::state::DraftKey;

    fn presentation(edit: impl FnOnce(&mut Snapshot)) -> Arc<ComposerPresentation> {
        let mut snapshot = Snapshot::default();
        snapshot.connection.connected = true;
        Arc::make_mut(&mut snapshot.catalog.models).push(
            serde_json::from_value(serde_json::json!({
                "id":"gpt","model":{"provider":"codex","id":"gpt"},"displayName":"GPT"
            }))
            .unwrap(),
        );
        let key = snapshot.panes.primary().navigation.draft_key.clone();
        Arc::make_mut(&mut snapshot.input.drafts).insert(
            key,
            Arc::new(Draft {
                model: Some(ModelRef {
                    provider: ProviderKind::Codex,
                    id: "gpt".into(),
                }),
                ..Default::default()
            }),
        );
        edit(&mut snapshot);
        ComposerPresentation::project(&snapshot, snapshot.panes.primary(), None)
    }

    fn running(snapshot: &mut Snapshot, key: OperationKey) {
        Arc::make_mut(&mut snapshot.operations).insert(
            key,
            crate::state::operations::OperationState {
                generation: 1,
                phase: OperationPhase::Running,
            },
        );
    }

    #[test]
    fn a_ready_composer_sends_content_and_accepts_media() {
        let ready = presentation(|_| ());
        assert!(ready.can_send(true));
        assert!(!ready.can_send(false));
        assert!(ready.can_dictate() && ready.can_attach());
    }

    #[test]
    fn core_activity_blocks_every_input_until_it_finishes() {
        fn draft(snapshot: &Snapshot) -> DraftKey {
            snapshot.panes.primary().navigation.draft_key.clone()
        }
        type Edit = fn(&mut Snapshot);
        let blocked: [(&str, Edit); 4] = [
            ("disconnected", |s| s.connection.connected = false),
            ("conversation not loaded", |s| {
                Arc::make_mut(&mut Arc::make_mut(s.panes.primary_mut()).navigation).thread_id =
                    Some(crate::session::SessionRef {
                        provider: ProviderKind::Codex,
                        id: "unloaded".into(),
                    });
            }),
            ("sending", |s| {
                let draft_key = draft(s);
                running(s, OperationKey::Submission { draft_key });
            }),
            ("transcribing", |s| {
                let draft_key = draft(s);
                running(s, OperationKey::Dictation { draft_key });
            }),
        ];
        for (name, edit) in blocked {
            let composer = presentation(edit);
            assert!(!composer.can_send(true), "{name}");
            assert!(!composer.can_dictate(), "{name}");
            assert!(!composer.can_attach(), "{name}");
        }
    }

    #[test]
    fn sending_alone_needs_a_provider() {
        let composer = presentation(|snapshot| {
            Arc::make_mut(&mut snapshot.input.drafts).clear();
            snapshot.catalog.models = Arc::default();
        });
        assert!(composer.controls.provider.is_none());
        assert!(!composer.can_send(true));
        assert!(composer.can_dictate() && composer.can_attach());
    }
}
