//! Window-local input state. Saved values always come from the core snapshot;
//! these types only decide when local input may replace or be replaced by them.
use agent_core::{questions::QuestionPrompt, state::DraftKey};
use agent_protocol::{
    dsh::KeyProvider, ids::RequestId, models::WorktreeSettings, session::ProviderKind,
};
use gpui_kit::{
    component::input::{EditorState, InputState, TextareaState},
    *,
};

use super::ModelPanel;

/// Text typed locally and saved to a core draft. Snapshot text replaces the
/// local value only while no local edit is waiting for its save.
#[derive(Default)]
pub(super) struct DraftText {
    value: SharedString,
    revision: u64,
    pub(super) pending: Option<u64>,
}

impl DraftText {
    pub(super) fn differs(&self, text: &str) -> bool {
        self.value.as_ref() != text
    }

    /// Records a local edit and returns the revision its save completes.
    pub(super) fn record(&mut self, value: SharedString) -> u64 {
        self.value = value;
        self.revision += 1;
        self.pending = Some(self.revision);
        self.revision
    }

    pub(super) fn saved(&mut self, revision: u64) {
        if self.pending == Some(revision) {
            self.pending = None;
        }
    }

    /// Returns the text to show when the snapshot may replace the local value.
    pub(super) fn adopt(&mut self, text: &str) -> Option<SharedString> {
        (self.pending.is_none() && self.differs(text)).then(|| self.replace(text))
    }

    /// Replaces the local value and forgets any edit still being saved.
    pub(super) fn replace(&mut self, text: &str) -> SharedString {
        self.value = text.to_owned().into();
        self.pending = None;
        self.value.clone()
    }
}

pub(super) struct ComposerInput {
    pub(super) input: Entity<TextareaState>,
    pub(super) text: DraftText,
    pub(super) completion_index: usize,
    pub(super) completion_dismissed: bool,
}

impl ComposerInput {
    pub(super) fn new(window: &mut Window, cx: &mut App) -> Self {
        let language = crate::localization::language(cx);
        Self {
            input: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .placeholder(language.text("AI に依頼する"))
                    .auto_grow(2, 8)
            }),
            text: DraftText::default(),
            completion_index: 0,
            completion_dismissed: false,
        }
    }

    /// Records typed text and returns the revision to save, if it changed.
    pub(super) fn typed(&mut self, value: SharedString) -> Option<u64> {
        if !self.text.differs(&value) {
            return None;
        }
        self.completion_index = 0;
        self.completion_dismissed = false;
        Some(self.text.record(value))
    }
}

/// The composer's field while it answers a question. Its text is the active
/// question's own answer, saved to core like a draft.
pub(super) struct QuestionInput {
    pub(super) input: Entity<TextareaState>,
    /// Only a single-line input can mask a secret answer.
    pub(super) secret: Entity<InputState>,
    pub(super) text: DraftText,
    /// The question shown; another question replaces the text instead of adopting it.
    shown: Option<(RequestId, String)>,
    /// A collapse belongs to one request and question.
    pub(super) collapsed: Option<(RequestId, String)>,
}

impl QuestionInput {
    pub(super) fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            input: cx.new(|cx| TextareaState::new(window, cx).auto_grow(1, 6)),
            secret: cx.new(|cx| InputState::new(window, cx).masked(true)),
            text: DraftText::default(),
            shown: None,
            collapsed: None,
        }
    }

    /// Whether Enter commits IME composition rather than answering.
    pub(super) fn composing(&self, secret: bool, window: &mut Window, cx: &mut App) -> bool {
        if secret {
            self.secret.update(cx, |input, cx| {
                input.marked_text_range(window, cx).is_some()
            })
        } else {
            self.input.update(cx, |input, cx| {
                input.marked_text_range(window, cx).is_some()
            })
        }
    }

    pub(super) fn set_placeholder(&self, placeholder: &str, window: &mut Window, cx: &mut App) {
        if self.input.read(cx).presentation().placeholder().as_ref() == placeholder {
            return;
        }
        // Plain and secret fields share the same placeholder.
        let placeholder = SharedString::from(placeholder.to_owned());
        self.input.update(cx, |input, cx| {
            input.set_placeholder(placeholder.clone(), window, cx)
        });
        self.secret.update(cx, |input, cx| {
            input.set_placeholder(placeholder, window, cx)
        });
    }

    pub(super) fn set_value(&self, value: SharedString, window: &mut Window, cx: &mut App) {
        self.input
            .update(cx, |input, cx| input.set_value(value.clone(), window, cx));
        self.secret
            .update(cx, |input, cx| input.set_value(value, window, cx));
    }

    /// Returns the text to show for `prompt`, if the field must change.
    pub(super) fn show(&mut self, prompt: Option<&QuestionPrompt>) -> Option<SharedString> {
        let Some(prompt) = prompt else {
            self.shown = None;
            return None;
        };
        let shown = (prompt.request_id.clone(), prompt.question.id.clone());
        if self.shown.as_ref() != Some(&shown) {
            self.shown = Some(shown);
            return Some(self.text.replace(&prompt.text));
        }
        self.text.adopt(&prompt.text)
    }
}

pub(super) struct FileEditor {
    pub(super) input: Entity<EditorState>,
    pub(super) text: DraftText,
    pub(super) path: Option<String>,
    pub(super) pending_line: Option<(String, u32)>,
}

impl FileEditor {
    pub(super) fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            input: cx.new(|cx| EditorState::new(window, cx)),
            text: DraftText::default(),
            path: None,
            pending_line: None,
        }
    }

    /// Records typed text and returns the file and revision to save, if it changed.
    pub(super) fn typed(&mut self, value: SharedString) -> Option<(String, u64)> {
        if !self.text.differs(&value) {
            return None;
        }
        let path = self.path.clone()?;
        Some((path, self.text.record(value)))
    }

    /// Returns the text to show for the snapshot's file, if the input must change.
    pub(super) fn show(&mut self, path: Option<&str>, text: &str) -> Option<SharedString> {
        if self.path.as_deref() != path {
            self.path = path.map(str::to_owned);
            return Some(self.text.replace(text));
        }
        self.text.adopt(text)
    }
}

pub(super) struct WorktreeForm {
    pub(super) directory: Entity<InputState>,
    pub(super) copy_paths: Entity<TextareaState>,
    /// The inputs differ from the saved settings.
    pub(super) dirty: bool,
    pub(super) saved: bool,
    pub(super) saving: bool,
    /// Inputs changed while a save was in flight and must be saved after it.
    pub(super) save_pending: bool,
    /// The worktree whose removal awaits confirmation.
    pub(super) removal: Option<String>,
    pub(super) removing: bool,
    /// The project whose cleanup rule is shown; `None` shows the shared settings.
    pub(super) scope: Option<String>,
}

impl WorktreeForm {
    pub(super) fn new(window: &mut Window, cx: &mut App) -> Self {
        let language = crate::localization::language(cx);
        Self {
            directory: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(language.text("ホスト上の絶対パス（空欄で既定）"))
            }),
            copy_paths: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .placeholder(".env\n.env.local\nconfig/local")
                    .auto_grow(3, 8)
            }),
            dirty: false,
            saved: false,
            saving: false,
            save_pending: false,
            removal: None,
            removing: false,
            scope: None,
        }
    }

    /// The saved settings with the typed directory and copy paths applied.
    pub(super) fn edited(&self, saved: &WorktreeSettings, cx: &App) -> WorktreeSettings {
        WorktreeSettings {
            worktree_directory: self.directory.read(cx).value().trim().into(),
            copy_paths: self
                .copy_paths
                .read(cx)
                .value()
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
                .collect(),
            ..saved.clone()
        }
    }

    pub(super) fn matches(&self, saved: Option<&WorktreeSettings>, cx: &App) -> bool {
        saved.is_some_and(|saved| self.edited(saved, cx) == *saved)
    }

    pub(super) fn show(&self, settings: &WorktreeSettings, window: &mut Window, cx: &mut App) {
        self.copy_paths.update(cx, |input, cx| {
            input.set_value(settings.copy_paths.join("\n"), window, cx)
        });
        self.directory.update(cx, |input, cx| {
            input.set_value(settings.worktree_directory.clone(), window, cx)
        });
    }
}

pub(super) struct AccountForm {
    pub(super) code: Entity<InputState>,
    pub(super) busy: bool,
    pub(super) polling: bool,
    pub(super) login_draft: Option<DraftKey>,
    pub(super) sign_out: Option<String>,
    pub(super) dsh_key_provider: KeyProvider,
}

impl AccountForm {
    pub(super) fn new(window: &mut Window, cx: &mut App) -> Self {
        let language = crate::localization::language(cx);
        Self {
            code: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(language.text("認証コード・APIキーを貼り付け"))
                    .masked(true)
            }),
            busy: false,
            polling: false,
            login_draft: None,
            sign_out: None,
            dsh_key_provider: Default::default(),
        }
    }
}

pub(super) struct ModelControls {
    pub(super) panel: ModelPanel,
    pub(super) search: Entity<InputState>,
    pub(super) provider: Option<ProviderKind>,
}

impl ModelControls {
    pub(super) fn new(window: &mut Window, cx: &mut App) -> Self {
        let language = crate::localization::language(cx);
        Self {
            panel: ModelPanel::Models,
            search: cx
                .new(|cx| InputState::new(window, cx).placeholder(language.text("モデルを検索"))),
            provider: None,
        }
    }
}
