//! A composer's own inputs and controls, independent of transcript deltas.
use super::model_settings::ModelQuickControls;
use crate::{
    models::{Model, ModelRef},
    session::ProviderKind,
    state::{
        Draft, Navigation, Same, Snapshot,
        operations::{OperationKey, OperationPhase},
    },
};
use std::sync::{Arc, OnceLock};

#[derive(Clone, PartialEq)]
#[cfg_attr(feature = "bindings", derive(uniffi::Record))]
pub struct ComposerControls {
    pub navigation: Navigation,
    pub connected: bool,
    /// The selected conversation is loaded, or the draft starts a new one.
    pub loaded: bool,
    pub provider: Option<ProviderKind>,
    pub selected_model: Option<ModelRef>,
    pub quick: ModelQuickControls,
    pub sending: bool,
    pub transcribing: bool,
    pub running_turn: Option<agent_protocol::ids::TurnId>,
    pub interrupt_phase: Option<OperationPhase>,
    pub unavailable: Option<String>,
    pub question: Option<crate::questions::QuestionPrompt>,
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
            unavailable: session
                .and_then(|id| snapshot.input_unavailable_reason(id.clone(), provider)),
            question: session.and_then(|id| snapshot.question_prompt(id.clone())),
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
            .or_else(|| {
                self.thread_metadata(&session)
                    .and_then(crate::session::input_unavailable_reason)
            })
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
        controls.connected && controls.loaded && !controls.sending && !controls.transcribing
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
