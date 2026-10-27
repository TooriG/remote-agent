use agent_core::state::operations as op;
mod clipboard;
mod completions;
mod dictation;
mod forms;
mod hosts;
mod selection;
mod view;

use crate::{Runtime, appearance, diff::DiffView, platform, store_session::StoreSession};
use agent_core::{
    panels::{PanelKind as Panel, PanelLayout, PanelScope, Panels},
    presentation::conversation::{ActivityExpansion, ConversationRowContent},
    state::{Attachment, Draft, DraftKey, Intent, ModelDefaultsScope, PendingSubmission, Snapshot},
    store::Outcome,
};
use agent_protocol::{
    ids::{ItemId, RequestId, TurnId},
    models::{Item, Model, RemoteHost, Thread, Turn},
    requests::Answer,
    session::SessionRef,
};
use dictation::{Dictation, Phase};
use gpui_kit::{
    component::{
        button::{Button, ButtonVariants},
        input::{Editor, Input, InputEvent, InputState, Textarea, TextareaState},
        menu::{DropdownMenu, PopupMenuItem},
        text::TextView,
        *,
    },
    prelude::FluentBuilder,
    *,
};
use hosts::{ConnectionLayout, HostEvent, Hosts};
use std::{
    collections::{HashMap, HashSet},
    future::Future,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

struct ProjectEditor {
    id: String,
    scope: String,
    name: Entity<InputState>,
    section: Entity<InputState>,
}

enum OperationCompletion {
    ProjectImport,
    Busy,
    Composer(u64),
    Editor(u64),
    Item { generation: u64, item_id: ItemId },
    WorktreeSettings,
    Request(RequestId),
    Dictation(uuid::Uuid),
    RemoveWorktree,
    Account,
    Gallery(uuid::Uuid),
}
enum Update {
    Connected(Result<(StoreSession, PathBuf), String>),
    Snapshot,
    Completed(OperationCompletion, Result<Outcome, String>),
    Folder(Result<Option<PathBuf>, String>),
    Image {
        key: String,
        result: Result<String, String>,
    },
    Recording(uuid::Uuid, platform::RecordingEvent),
    PersistenceError(String),
    OnboardingCompleted(Result<(), String>),
}
#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Chat,
    Settings,
}
#[derive(Clone, Copy, PartialEq)]
enum SettingsPage {
    Models,
    Agents,
    Connections,
    Worktrees,
}
enum WorktreeToggle {
    Create(bool),
    Copy(bool),
    DeleteMerged(bool),
}
#[derive(Clone, Copy, PartialEq)]
enum ModelPanel {
    Models,
    Accounts,
    Manage,
}
enum SurfaceResource {
    Diff(Entity<DiffView>),
    Pending {
        url: Option<String>,
    },
    Browser {
        view: Entity<crate::browser::Browser>,
        _observation: Subscription,
    },
    Terminal(Entity<crate::terminal::Terminal>),
    SideChat(Entity<Desktop>),
}
pub(crate) enum Mode {
    Main,
    SideChat {
        remote: Option<RemoteHost>,
        cwd: String,
    },
}
struct Question {
    definition: agent_protocol::requests::Question,
    input: Entity<InputState>,
    selected: HashSet<String>,
}
struct RequestInputs {
    questions: Vec<Question>,
    response: Entity<TextareaState>,
    sent: bool,
}
struct ImageGallery {
    zoom: f32,
    id: uuid::Uuid,
    entries: Vec<(Arc<String>, bool)>,
    initial: (Arc<String>, bool),
    selected: Option<usize>,
    list: ListState,
    loading: bool,
    saving: bool,
    saved: bool,
    error: String,
}
impl ImageGallery {
    fn current_image(&self) -> &(Arc<String>, bool) {
        self.selected
            .and_then(|index| self.entries.get(index))
            .unwrap_or(&self.initial)
    }
}
struct ImageState {
    source: Arc<String>,
    path: Option<ImageSource>,
    error: Option<String>,
}
struct MarkdownContent {
    source: SharedString,
    rendered: SharedString,
    images: Rc<[String]>,
    code_headers: Arc<HashMap<usize, String>>,
    diagrams: HashMap<String, Arc<RenderImage>>,
    wrap_code: bool,
    compact_tables: bool,
}
#[derive(Clone)]
enum ConversationRow {
    History(agent_core::presentation::conversation::HistoryControl),
    Turn {
        turn: Arc<agent_core::presentation::conversation::RenderedTurn>,
        index: usize,
        last: bool,
    },
    Pending(String, Arc<PendingSubmission>),
    Request(Box<agent_core::presentation::conversation::Request>),
}
impl ConversationRow {
    fn same_identity(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::History(_), Self::History(_)) => true,
            (
                Self::Turn {
                    turn: a, index: ai, ..
                },
                Self::Turn {
                    turn: b, index: bi, ..
                },
            ) => a.source.id == b.source.id && a.rows[*ai].id == b.rows[*bi].id,
            (Self::Pending(a, _), Self::Pending(b, _)) => a == b,
            (Self::Request(a), Self::Request(b)) => a.id == b.id,
            _ => false,
        }
    }
    fn unchanged(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::History(a), Self::History(b)) => a == b,
            (
                Self::Turn {
                    turn: a,
                    index: ai,
                    last: al,
                },
                Self::Turn {
                    turn: b,
                    index: bi,
                    last: bl,
                },
            ) => {
                let a_row = &a.rows[*ai];
                let b_row = &b.rows[*bi];
                if al != bl || a_row.timestamp_ms != b_row.timestamp_ms {
                    return false;
                }
                match (&a_row.content, &b_row.content) {
                    (
                        ConversationRowContent::User { item: a },
                        ConversationRowContent::User { item: b },
                    )
                    | (
                        ConversationRowContent::Activity { item: a, .. },
                        ConversationRowContent::Activity { item: b, .. },
                    ) => Arc::ptr_eq(a, b),
                    (
                        ConversationRowContent::Response {
                            item: a,
                            fork_turn_id: af,
                        },
                        ConversationRowContent::Response {
                            item: b,
                            fork_turn_id: bf,
                        },
                    ) => Arc::ptr_eq(a, b) && af == bf,
                    (
                        ConversationRowContent::ActivityHeader { activity: x },
                        ConversationRowContent::ActivityHeader { activity: y },
                    ) => {
                        x == y
                            && (!x.is_in_progress || a.work_progress(true) == b.work_progress(true))
                    }
                    (
                        ConversationRowContent::InProgress { .. },
                        ConversationRowContent::InProgress { .. },
                    ) => a.work_progress(false) == b.work_progress(false),
                    (
                        ConversationRowContent::ProviderHandoff { text: a },
                        ConversationRowContent::ProviderHandoff { text: b },
                    ) => a == b,
                    (
                        ConversationRowContent::PendingRequest { request: a },
                        ConversationRowContent::PendingRequest { request: b },
                    ) => a == b,
                    (
                        ConversationRowContent::Error { error: a },
                        ConversationRowContent::Error { error: b },
                    ) => a == b,
                    _ => false,
                }
            }
            (Self::Pending(a, x), Self::Pending(b, y)) => a == b && Arc::ptr_eq(x, y),
            (Self::Request(a), Self::Request(b)) => a == b,
            _ => false,
        }
    }
}

/// Business state is owned by Store. Everything else here is a widget, render
/// cache, pending UI effect, or immutable snapshot retained for display.
pub(crate) struct Desktop {
    session: Option<StoreSession>,
    snapshot: Arc<Snapshot>,
    runtime: Runtime,
    updates: async_channel::Sender<(u64, Update)>,
    epoch: u64,
    connecting: bool,
    remote: Option<RemoteHost>,
    hosts: Option<Entity<Hosts>>,
    side_chat_mode: bool,
    onboarding: bool,
    import_projects: bool,
    initial_cwd: Option<String>,
    busy: usize,
    error: String,
    composer: forms::ComposerInput,
    selection: Entity<selection::ConversationSelection>,
    pending_quote: Option<String>,
    pending_explanation: Option<String>,
    editor: forms::FileEditor,
    project_editor: Option<ProjectEditor>,
    search: Entity<InputState>,
    path: Entity<InputState>,
    model_controls: forms::ModelControls,
    account: forms::AccountForm,
    worktrees: forms::WorktreeForm,
    expanded_items: HashSet<String>,
    expanded_work: HashMap<String, ActivityExpansion>,
    tab: Tab,
    settings_page: SettingsPage,
    sidebar: bool,
    panels: Panels,
    surface_resources: HashMap<u64, SurfaceResource>,
    panel_focus: FocusHandle,
    dictation: Option<Dictation>,
    review_expanded: bool,
    source_paths: Vec<String>,
    source_items: Vec<Arc<Item>>,
    requests: HashMap<RequestId, RequestInputs>,
    list: ListState,
    hovered_conversation_marker: Option<usize>,
    rows: Vec<ConversationRow>,
    rendered: Option<Arc<agent_core::presentation::conversation::RenderedConversation>>,
    diffs: HashMap<String, Entity<DiffView>>,
    images: HashMap<String, ImageState>,
    image_gallery: Option<ImageGallery>,
    image_dir: tempfile::TempDir,
    markdown_cache: HashMap<String, MarkdownContent>,
    _subscriptions: Vec<Subscription>,
}
impl Desktop {
    fn set_error(&mut self, error: String) {
        tracing::error!(target: "bex", operation = "desktop", message = %error);
        self.error = error;
    }
    pub(crate) fn new(mode: Mode, window: &mut Window, cx: &mut Context<Self>) -> Self {
        StoreSession::on_app_quit(cx, |view| &mut view.session);
        let (remote, initial_cwd, side_chat_mode) = match mode {
            Mode::Main => (None, None, false),
            Mode::SideChat { remote, cwd } => (remote, Some(cwd), true),
        };
        let (updates, incoming) = async_channel::unbounded();
        cx.spawn_in(window, async move |view, cx| {
            while let Ok(update) = incoming.recv().await {
                if view
                    .update_in(cx, |view, window, cx| view.receive(update, window, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        cx.spawn_in(window, async move |view, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(1))
                    .await;
                if view
                    .update(cx, |view, _| {
                        if view.account.polling
                            && !view.account.busy
                            && view.snapshot.connected
                            && let Some(login) = &view.snapshot.account.login
                        {
                            let id = login.login_id.clone();
                            let provider = login.provider;
                            view.account.busy = true;
                            view.perform(
                                Intent::ReadAccountLogin(op::ReadAccountLogin {
                                    provider,
                                    id,
                                    thread_id: view.account.login_draft.clone(),
                                }),
                                OperationCompletion::Account,
                            );
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        let composer = forms::ComposerInput::new(window, cx);
        let editor = forms::FileEditor::new(window, cx);
        let model_controls = forms::ModelControls::new(window, cx);
        let account = forms::AccountForm::new(window, cx);
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("会話を検索"));
        let path = cx.new(|cx| InputState::new(window, cx).placeholder("絶対パス"));
        let worktrees = forms::WorktreeForm::new(window, cx);
        let hosts = (!side_chat_mode).then(|| cx.new(|cx| Hosts::new(window, cx)));
        let selection = cx.new(|_| selection::ConversationSelection::new(!side_chat_mode));
        let mut subscriptions = vec![
            cx.subscribe_in(
                &selection,
                window,
                |view, _, action, window, cx| match action {
                    selection::SelectionAction::AddToChat(text) => {
                        view.quote_selection(text, window, cx)
                    }
                    selection::SelectionAction::AskSideChat(text) => {
                        view.open_panel(Panel::SideChat, window, cx);
                        if let Some(SurfaceResource::SideChat(chat)) = view
                            .panels
                            .get(view.panel_scope())
                            .active
                            .and_then(|id| view.surface_resources.get(&id))
                        {
                            chat.update(cx, |chat, cx| chat.quote_selection(text, window, cx));
                        }
                    }
                    selection::SelectionAction::Explain(text) => {
                        if view.side_chat_mode {
                            view.explain_selection(text, cx);
                        } else {
                            view.open_panel(Panel::SideChat, window, cx);
                            if let Some(SurfaceResource::SideChat(chat)) = view
                                .panels
                                .get(view.panel_scope())
                                .active
                                .and_then(|id| view.surface_resources.get(&id))
                            {
                                chat.update(cx, |chat, cx| chat.explain_selection(text, cx));
                            }
                        }
                    }
                },
            ),
            cx.subscribe(&composer.input, |view, input, event, cx| {
                if matches!(event, InputEvent::Change) {
                    let value = input.read(cx).value();
                    if let Some(revision) = view.composer.typed(value.clone()) {
                        let cursor = input.read(cx).cursor();
                        view.perform(
                            Intent::EditComposer {
                                thread_id: view.draft_key().clone(),
                                text: value.to_string(),
                                cursor: cursor as u32,
                            },
                            OperationCompletion::Composer(revision),
                        );
                    }
                    cx.notify();
                }
            }),
            cx.subscribe(&editor.input, |view, input, event, cx| {
                if matches!(event, InputEvent::Change) {
                    let value = input.read(cx).value();
                    if let Some((path, revision)) = view.editor.typed(value.clone()) {
                        view.perform(
                            Intent::SetFileDraft {
                                path,
                                text: value.to_string(),
                            },
                            OperationCompletion::Editor(revision),
                        );
                    }
                }
            }),
            cx.subscribe(&model_controls.search, |_, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.subscribe(&account.code, |_, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.subscribe(&search, |view, input, event, cx| {
                if matches!(event, InputEvent::Change) {
                    let value = input.read(cx).value();
                    if value.as_ref() != view.snapshot.list_query.search_term {
                        let mut query = (*view.snapshot.list_query).clone();
                        query.search_term = value.to_string();
                        view.dispatch(Intent::ListSessions(op::ListSessions::new(query)));
                    }
                }
            }),
            cx.subscribe(&worktrees.copy_paths, |view, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    view.worktree_input_changed(cx);
                    cx.notify();
                }
                if matches!(event, InputEvent::Blur) {
                    view.save_worktree_settings(None, cx);
                }
            }),
            cx.subscribe(&worktrees.directory, |view, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    view.worktree_input_changed(cx);
                    cx.notify();
                }
                if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    view.save_worktree_settings(None, cx);
                }
            }),
        ];
        if let Some(hosts) = &hosts {
            subscriptions.push(cx.observe(hosts, |_, _, cx| cx.notify()));
            subscriptions.push(
                cx.subscribe_in(hosts, window, |view, _, event, window, cx| {
                    match event {
                        HostEvent::Selected(remote) => view.switch_host(remote.clone(), window, cx),
                        HostEvent::Removed(id)
                            if view.remote.as_ref().is_some_and(|remote| &remote.id == id) =>
                        {
                            view.switch_host(None, window, cx)
                        }
                        _ => {}
                    }
                    cx.notify();
                }),
            );
        }
        let list = ListState::new(0, ListAlignment::Bottom, px(600.));
        list.set_follow_mode(FollowMode::Tail);
        let mut view = Self {
            session: None,
            snapshot: Arc::default(),
            runtime: cx.global::<Runtime>().clone(),
            updates,
            epoch: 0,
            connecting: false,
            remote,
            hosts,
            side_chat_mode,
            onboarding: !side_chat_mode
                && !platform::state_dir()
                    .is_ok_and(|directory| directory.join("onboarding.completed").is_file()),
            import_projects: false,
            initial_cwd,
            busy: 0,
            error: String::new(),
            composer,
            selection,
            pending_quote: None,
            pending_explanation: None,
            editor,
            project_editor: None,
            search,
            path,
            model_controls,
            account,
            worktrees,
            expanded_items: HashSet::new(),
            expanded_work: HashMap::new(),
            tab: Tab::Chat,
            settings_page: SettingsPage::Agents,
            sidebar: true,
            panels: Panels::default(),
            surface_resources: HashMap::new(),
            panel_focus: cx.focus_handle(),
            dictation: None,
            review_expanded: false,
            source_paths: Vec::new(),
            source_items: Vec::new(),
            requests: HashMap::new(),
            list,
            hovered_conversation_marker: None,
            rows: Vec::new(),
            rendered: None,
            diffs: HashMap::new(),
            images: HashMap::new(),
            image_gallery: None,
            image_dir: tempfile::Builder::new()
                .prefix("bex-images-")
                .tempdir()
                .expect("image temporary directory"),
            markdown_cache: HashMap::new(),
            _subscriptions: subscriptions,
        };
        view.connect(None);
        view
    }
    fn connect(&mut self, preferences: Option<Vec<u8>>) {
        self.epoch += 1;
        self.connecting = true;
        self.busy = 0;
        self.worktrees.removal = None;
        self.worktrees.removing = false;
        self.account.busy = false;
        self.account.polling = false;
        self.model_controls.provider = None;
        self.account.sign_out = None;
        self.account.login_draft = None;
        self.error = self.runtime.logging_error.clone().unwrap_or_default();
        let epoch = self.epoch;
        let remote = self.remote.clone();
        let side = self.side_chat_mode;
        let initial_cwd = self.initial_cwd.take();
        let updates = self.updates.clone();
        let connections = self.runtime.connections.clone();
        let runtime = self.runtime.clone();
        self.runtime.handle.spawn(async move {
            let result = async {
                let host = if let Some(remote) = &remote {
                    remote
                        .ticket
                        .parse::<agent_transport::transport::Ticket>()
                        .map_err(|error| error.to_string())?
                        .node_id()
                        .to_string()
                } else {
                    "local".into()
                };
                let path = platform::state_dir()?.join(format!(
                    "desktop-{}-{host}.json",
                    if side { "side" } else { "main" }
                ));
                let mut snapshot: Snapshot = match tokio::fs::read(&path).await {
                    Ok(bytes) => agent_core::persistence::decode(&bytes)
                        .map_err(|error| format!("保存した入力状態を読み込めません: {error}"))?,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        Snapshot::default()
                    }
                    Err(error) => return Err(error.to_string()),
                };
                let preferences = match preferences {
                    Some(preferences) => Some(preferences),
                    None => {
                        match tokio::fs::read(path.with_file_name("model-preferences.json")).await {
                            Ok(bytes) => Some(bytes),
                            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                            Err(error) => return Err(error.to_string()),
                        }
                    }
                };
                if let Some(preferences) = preferences {
                    let saved = agent_core::persistence::encode(&snapshot)
                        .map_err(|error| error.to_string())?;
                    snapshot = agent_core::persistence::decode(
                        &agent_core::persistence::apply_model_preferences(&saved, &preferences)
                            .map_err(|error| error.to_string())?,
                    )
                    .map_err(|error| error.to_string())?;
                }
                let initial_cwd = initial_cwd.or_else(|| {
                    snapshot
                        .navigation
                        .thread_id
                        .is_none()
                        .then(|| snapshot.navigation.cwd.clone())
                });
                let store = connections
                    .connect(
                        remote.as_ref().map(|remote| remote.ticket.as_str()),
                        snapshot,
                    )
                    .await
                    .map_err(|error| format!("{error:#}"))?;
                if let Some(cwd) = initial_cwd {
                    drop(store.dispatch(Intent::NewChat { cwd }));
                }
                Ok::<_, String>((store, path))
            }
            .await;
            match result {
                Ok((store, path)) => {
                    StoreSession::publish(
                        Ok(store),
                        runtime,
                        updates,
                        move |session| {
                            (
                                epoch,
                                Update::Connected(session.map(|session| (session, path))),
                            )
                        },
                        move |_| (epoch, Update::Snapshot),
                    )
                    .await;
                }
                Err(error) => {
                    let _ = updates.send((epoch, Update::Connected(Err(error)))).await;
                }
            }
        });
    }
    fn effect<T: Send + 'static>(
        &self,
        future: impl Future<Output = Result<T, String>> + Send + 'static,
        complete: impl FnOnce(Result<T, String>) -> Update + Send + 'static,
    ) {
        let epoch = self.epoch;
        let updates = self.updates.clone();
        self.runtime.handle.spawn(async move {
            let completion = complete(future.await);
            let _ = updates.send((epoch, completion)).await;
        });
    }
    fn perform(&self, intent: Intent, completion: OperationCompletion) {
        let Some(store) = self.session.as_ref().map(|session| &session.store) else {
            return;
        };
        let receipt = store.dispatch(intent);
        self.effect(
            async move { receipt.await.map_err(|error| error.to_string()) },
            move |result| Update::Completed(completion, result),
        );
    }
    fn dispatch(&self, intent: Intent) {
        if let Some(session) = &self.session {
            drop(session.store.dispatch(intent));
        }
    }
    fn receive(
        &mut self,
        (epoch, update): (u64, Update),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if epoch != self.epoch {
            return;
        }
        match update {
            Update::Connected(result) => {
                self.connecting = false;
                match result {
                    Ok((mut session, path)) => {
                        session.persist(path, self.updates.clone(), move |error| {
                            (epoch, Update::PersistenceError(error))
                        });
                        self.session = Some(session);
                        self.accept_snapshot(window, cx);
                        if let Some(text) = self.pending_quote.take() {
                            self.quote_selection(&text, window, cx);
                        }
                        if let Some(text) = self.pending_explanation.take() {
                            self.explain_selection(&text, cx);
                        }
                        self.dispatch(Intent::ReadWorktreeSettings(op::ReadWorktreeSettings {}));
                        if self.import_projects {
                            self.dispatch(Intent::ScanProjects(op::ScanProjects {}));
                        }
                        if self.tab == Tab::Settings {
                            self.dispatch(Intent::ListWorktrees(op::ListWorktrees {}));
                        }
                    }
                    Err(error) => self.set_error(error),
                }
            }
            Update::Snapshot => self.accept_snapshot(window, cx),
            Update::Completed(kind, result) => self.operation_completed(kind, result, window, cx),
            Update::Recording(id, event) => self.recording_update(id, event),
            Update::PersistenceError(error) => self.set_error(error),
            Update::OnboardingCompleted(result) => match result {
                Ok(()) => {
                    self.onboarding = false;
                    self.import_projects = false;
                }
                Err(error) => self.set_error(error),
            },
            Update::Folder(result) => {
                self.busy = self.busy.saturating_sub(1);
                match result {
                    Ok(Some(path)) => {
                        self.tab = Tab::Chat;
                        self.cancel_recording();
                        self.dispatch(Intent::AddProject(op::AddProject {
                            cwd: path.to_string_lossy().into_owned(),
                        }));
                    }
                    Ok(None) => {}
                    Err(error) => self.set_error(error),
                }
            }
            Update::Image { key, result } => {
                if let Some(image) = self.images.get_mut(&key) {
                    match result {
                        Ok(path) => {
                            image.path = Some(if Path::new(&path).is_absolute() {
                                PathBuf::from(path).into()
                            } else {
                                ImageSource::from(path)
                            })
                        }
                        Err(error) => {
                            tracing::error!(target: "bex", operation = "image.load", message = %error);
                            image.error = Some(error);
                        }
                    }
                }
                self.list.remeasure();
            }
        }
        cx.notify();
    }
    fn operation_completed(
        &mut self,
        kind: OperationCompletion,
        result: Result<Outcome, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match kind {
            OperationCompletion::ProjectImport => {
                self.accept_snapshot(window, cx);
                if result.is_ok()
                    && self
                        .snapshot
                        .project_import
                        .result
                        .as_ref()
                        .is_some_and(|result| result.errors.is_empty())
                {
                    self.complete_onboarding();
                } else if let Err(error) = result {
                    self.set_error(error);
                }
                return;
            }
            OperationCompletion::Composer(revision) => self.composer.text.saved(revision),
            OperationCompletion::Editor(revision) => self.editor.text.saved(revision),
            OperationCompletion::Busy => {
                self.busy = self.busy.saturating_sub(1);
            }
            OperationCompletion::Item {
                generation,
                item_id,
            } => {
                if self.snapshot.epoch != generation {
                    return;
                }
                self.accept_snapshot(window, cx);
                self.remeasure_item(item_id.as_str());
                return;
            }
            OperationCompletion::WorktreeSettings => {
                self.worktrees.saving = false;
                if let Err(error) = result {
                    self.set_error(error);
                }
                self.accept_snapshot(window, cx);
                self.worktrees.dirty = !self
                    .worktrees
                    .matches(self.snapshot.workspace.settings.as_deref(), cx);
                self.worktrees.saved = !self.worktrees.dirty;
                if std::mem::take(&mut self.worktrees.save_pending) {
                    self.save_worktree_settings(None, cx);
                }
                return;
            }
            OperationCompletion::Request(key) => {
                if let Err(error) = result {
                    self.set_error(error);
                    if let Some(inputs) = self.requests.get_mut(&key) {
                        inputs.sent = false;
                    }
                }
                self.accept_snapshot(window, cx);
                self.list.remeasure();
                return;
            }
            OperationCompletion::Account => {
                self.account.busy = false;
                if let Err(error) = result {
                    self.account.polling = false;
                    self.set_error(error);
                    self.dispatch(Intent::ListAccounts(op::ListAccounts {}));
                } else {
                    self.accept_snapshot(window, cx);
                    self.account.polling = self.snapshot.account.login.is_some();
                }
                if self.snapshot.account.login.is_none() {
                    self.account.login_draft = None;
                }
                return;
            }
            OperationCompletion::RemoveWorktree => {
                self.worktrees.removing = false;
                match result {
                    Ok(_) => self.worktrees.removal = None,
                    Err(error) => {
                        self.set_error(error);
                        self.dispatch(Intent::ListWorktrees(op::ListWorktrees {}));
                    }
                }
                self.accept_snapshot(window, cx);
                return;
            }
            OperationCompletion::Dictation(id) => {
                if self.dictation.as_ref().is_some_and(|state| state.id == id) {
                    self.dictation = None;
                }
            }
            OperationCompletion::Gallery(id) => {
                let Some(gallery) = self
                    .image_gallery
                    .as_mut()
                    .filter(|gallery| gallery.id == id)
                else {
                    return;
                };
                gallery.loading = false;
                match result {
                    Ok(Outcome::SessionImages { images }) => {
                        let initial = gallery.current_image().clone();
                        let entries: Vec<_> = images
                            .into_iter()
                            .map(|image| (Arc::new(image.source), image.encoded))
                            .collect();
                        gallery.selected = entries.iter().position(|entry| entry == &initial);
                        gallery
                            .list
                            .splice(0..gallery.list.item_count(), entries.len());
                        gallery.entries = entries;
                        if let Some(index) = gallery.selected {
                            gallery.list.scroll_to_reveal_item(index);
                        }
                    }
                    Err(error) => {
                        tracing::error!(target: "bex", operation = "gallery.load", message = %error);
                        gallery.error = error;
                    }
                    _ => unreachable!("session image outcome"),
                }
                return;
            }
        }
        if let Err(error) = result {
            self.set_error(error);
        }
        self.accept_snapshot(window, cx);
    }
    fn accept_snapshot(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(store) = self.session.as_ref().map(|session| &session.store) else {
            return;
        };
        let snapshot = store.snapshot();
        let changed = !Arc::ptr_eq(&snapshot, &self.snapshot);
        let previous = std::mem::replace(&mut self.snapshot, snapshot);
        if previous.account.login.as_ref().map(|login| &login.login_id)
            != self
                .snapshot
                .account
                .login
                .as_ref()
                .map(|login| &login.login_id)
        {
            self.account
                .code
                .update(cx, |input, cx| input.set_value("", window, cx));
        }
        if let Some(request) = self
            .snapshot
            .permission_control(self.draft_key())
            .load_request
        {
            self.dispatch(Intent::ReadPermissionSettings(request));
        }
        if previous.error != self.snapshot.error
            && let Some(error) = &self.snapshot.error
        {
            tracing::error!(target: "bex", operation = "store", message = %error);
        }
        sync_error_banner(
            &mut self.error,
            previous.error.as_deref(),
            self.snapshot.error.as_deref(),
        );
        let previous_draft = previous.drafts.get(&previous.navigation.draft_key);
        let sources_changed = previous_draft.map(|draft| &draft.attachments)
            != self
                .snapshot
                .drafts
                .get(self.draft_key())
                .map(|draft| &draft.attachments);
        let navigated = previous.navigation.draft_key != self.snapshot.navigation.draft_key;
        // Store snapshots may coalesce creation and send acceptance. Use the
        // durable core identity transition rather than transient pending sends.
        for (draft, session) in self.snapshot.draft_sessions.iter() {
            if previous.draft_sessions.get(draft) != Some(session) {
                let before = PanelScope {
                    draft: draft.clone(),
                    ..self.panel_scope()
                };
                let after = PanelScope {
                    draft: session.into(),
                    ..self.panel_scope()
                };
                self.panels.rekey(before, after);
            }
        }
        let project_for_selected = |snapshot: &Snapshot| {
            snapshot
                .thread_metadata(snapshot.navigation.thread_id.as_ref()?)
                .filter(|thread| thread.parent_id.is_none())?
                .project_id
                .as_ref()
                .cloned()
        };
        if (navigated || project_for_selected(&previous) != project_for_selected(&self.snapshot))
            && let Some(project) = project_for_selected(&self.snapshot)
        {
            self.dispatch(Intent::SetProjectExpanded {
                project_id: project,
                expanded: true,
            });
        }
        if navigated {
            self.selection
                .update(cx, |selection, cx| selection.clear(cx));
            self.cancel_recording();
            self.composer.text.pending = None;
            self.rendered = None;
            self.diffs.clear();
            self.markdown_cache.clear();
        }
        let text = self.draft().text.clone();
        if let Some(value) = self.composer.text.adopt(&text) {
            self.composer
                .input
                .update(cx, |input, cx| input.set_value(value, window, cx));
        }
        let file = self.snapshot.workspace.file.as_ref();
        let text = file.map_or("", |file| {
            self.snapshot
                .file_drafts
                .get(&file.path)
                .map_or(file.text.as_str(), |draft| draft.text.as_str())
        });
        if let Some(value) = self.editor.show(file.map(|file| file.path.as_str()), text) {
            self.editor
                .input
                .update(cx, |input, cx| input.set_value(value, window, cx));
        }
        if !self.worktrees.dirty
            && !self.worktrees.saving
            && previous.workspace.settings != self.snapshot.workspace.settings
            && let Some(settings) = &self.snapshot.workspace.settings
        {
            self.worktrees.show(settings, window, cx);
        }
        let conversations_changed =
            !Arc::ptr_eq(&previous.conversations, &self.snapshot.conversations);
        if conversations_changed {
            self.sync_request_inputs(window, cx);
        }
        if navigated
            || conversations_changed
            || self.history_key().is_some_and(|key| {
                previous.operations.get(&key) != self.snapshot.operations.get(&key)
            })
            || !Arc::ptr_eq(
                &previous.pending_submissions,
                &self.snapshot.pending_submissions,
            )
        {
            self.sync_rows(navigated);
        }
        let user_items_changed = {
            let mut current = self.user_items();
            !self.source_items.iter().all(|previous| {
                current
                    .next()
                    .is_some_and(|item| Arc::ptr_eq(previous, item))
            }) || current.next().is_some()
        };
        if sources_changed || user_items_changed || navigated {
            let mut paths: std::collections::BTreeSet<&str> = self
                .draft()
                .attachments
                .iter()
                .map(|attachment| attachment.path.as_str())
                .collect();
            for item in self.user_items() {
                if let agent_protocol::items::ItemBody::UserMessage { content, .. } = item.body() {
                    paths.extend(content.iter().filter_map(|part| match part {
                        agent_protocol::items::MessagePart::Image { source }
                            if !source.starts_with("data:") =>
                        {
                            Some(source.as_str())
                        }
                        agent_protocol::items::MessagePart::Attachment { path, .. }
                        | agent_protocol::items::MessagePart::Invocation { path, .. } => {
                            Some(path.as_str())
                        }
                        _ => None,
                    }));
                }
            }
            self.source_paths = paths.into_iter().map(str::to_owned).collect();
            self.source_items = self.user_items().cloned().collect();
        }
        if previous.workspace.directory != self.snapshot.workspace.directory
            && let Some(directory) = &self.snapshot.workspace.directory
        {
            let previous_path = previous
                .workspace
                .directory
                .as_ref()
                .map_or("", |directory| directory.path.as_str());
            if self.path.read(cx).value().as_ref() == previous_path {
                self.path.update(cx, |input, cx| {
                    input.set_value(directory.path.clone(), window, cx)
                });
            }
        }
        let cwd_changed = previous.navigation.cwd != self.snapshot.navigation.cwd;
        if cwd_changed {
            let cwd = self.snapshot.navigation.cwd.clone();
            self.path
                .update(cx, |input, cx| input.set_value(cwd, window, cx));
        }
        if changed && let Some(session) = &self.session {
            session.save(self.snapshot.clone());
        }
        if navigated {
            let panel = self.panels.get(self.panel_scope());
            if panel.open
                && matches!(panel.kind(), Panel::File | Panel::Files)
                && let Some(id) = panel.active
            {
                self.activate_surface(id, window, cx);
            }
        }
    }
    fn draft_key(&self) -> &DraftKey {
        &self.snapshot.navigation.draft_key
    }
    fn selected(&self) -> Option<&SessionRef> {
        self.snapshot.navigation.thread_id.as_ref()
    }
    fn thread(&self) -> Option<&Arc<Thread>> {
        self.selected()
            .and_then(|id| self.snapshot.conversations.get(id))
    }
    fn draft(&self) -> &Draft {
        static EMPTY: Draft = Draft {
            invocations: Vec::new(),
            text: String::new(),
            attachments: Vec::new(),
            model: None,
            effort: None,
            service_tier: None,
        };
        self.snapshot
            .drafts
            .get(self.draft_key())
            .map_or(&EMPTY, Arc::as_ref)
    }
    fn selected_model(&self) -> Option<&Model> {
        self.snapshot
            .models
            .iter()
            .find(|model| Some(&model.model) == self.draft().model.as_ref())
    }
    fn remote_key(&self) -> &str {
        self.remote.as_ref().map_or("local", |remote| &remote.id)
    }
    fn image_key(&self, source: &str, encoded: bool) -> String {
        format!(
            "{}:{}:{encoded}:{source}",
            self.remote_key(),
            self.snapshot.navigation.cwd
        )
    }
    fn history_key(&self) -> Option<op::OperationKey> {
        self.selected()
            .cloned()
            .map(|session| op::OperationKey::History { session })
    }
    fn user_items(&self) -> impl Iterator<Item = &Arc<Item>> {
        self.thread()
            .and_then(|thread| thread.turns.as_deref())
            .unwrap_or_default()
            .iter()
            .flat_map(|turn| turn.items.as_deref().unwrap_or_default())
            .filter(|item| {
                matches!(
                    item.body(),
                    agent_protocol::items::ItemBody::UserMessage { .. }
                )
            })
    }
    fn sync_rows(&mut self, reset: bool) {
        self.rendered = self.snapshot.conversation_thread().map(|thread| {
            agent_core::presentation::conversation::project_conversation(
                &self.snapshot,
                thread,
                &self.rendered,
            )
        });
        let mut rows = conversation_rows(&self.rendered, &self.expanded_work);
        if let Some(control) = self
            .selected()
            .cloned()
            .and_then(|session| self.snapshot.history_control(session))
        {
            rows.insert(0, ConversationRow::History(control));
        }
        if reset {
            self.list.reset(rows.len());
            self.list.scroll_to_end();
            self.rows = rows;
            return;
        }
        let old = &self.rows;
        let prefix = old
            .iter()
            .zip(&rows)
            .take_while(|(a, b)| a.same_identity(b))
            .count();
        let suffix = old[prefix..]
            .iter()
            .rev()
            .zip(rows[prefix..].iter().rev())
            .take_while(|(a, b)| a.same_identity(b))
            .count();
        let structure_changed = prefix + suffix != old.len() || old.len() != rows.len();
        let anchor = self.list.logical_scroll_top();
        // Match T3's maintainVisibleContentPosition: retain a visible row by
        // identity, even when a preceding history/work header is replaced.
        let visible_anchor = (structure_changed && !self.list.is_following_tail())
            .then(|| {
                let viewport = self.list.viewport_bounds();
                for (index, row) in old.iter().enumerate().skip(anchor.item_ix) {
                    let bounds = self.list.bounds_for_item(index)?;
                    if bounds.top() >= viewport.bottom() {
                        break;
                    }
                    match row {
                        ConversationRow::History(_) => continue,
                        ConversationRow::Turn { turn, index, .. }
                            if matches!(
                                turn.rows[*index].content,
                                ConversationRowContent::ActivityHeader { .. }
                                    | ConversationRowContent::InProgress { .. }
                            ) =>
                        {
                            continue;
                        }
                        _ => (),
                    }
                    if bounds.bottom() > viewport.top()
                        && let Some(item_ix) = rows.iter().position(|next| row.same_identity(next))
                    {
                        return Some(ListOffset {
                            item_ix,
                            offset_in_item: viewport.top() - bounds.top(),
                        });
                    }
                }
                None
            })
            .flatten();
        if structure_changed {
            self.list
                .splice(prefix..old.len() - suffix, rows.len() - prefix - suffix);
        }
        for index in 0..prefix {
            if !old[index].unchanged(&rows[index]) {
                self.list.remeasure_items(index..index + 1);
            }
        }
        for offset in 0..suffix {
            let index = rows.len() - suffix + offset;
            if !old[old.len() - suffix + offset].unchanged(&rows[index]) {
                self.list.remeasure_items(index..index + 1);
            }
        }
        self.rows = rows;
        if let Some(anchor) = visible_anchor {
            self.list.scroll_to(anchor);
        }
    }
    fn pause_tail(&self) {
        if self.list.logical_scroll_top().item_ix == self.list.item_count() {
            self.list
                .scroll_by(-self.list.viewport_bounds().size.height);
        }
        self.list.pause_following_tail();
    }
    fn remeasure_item(&self, id: &str) {
        for (index, row) in self.rows.iter().enumerate() {
            if let ConversationRow::Turn {
                turn,
                index: row_index,
                ..
            } = row
                && match &turn.rows[*row_index].content {
                    ConversationRowContent::User { item }
                    | ConversationRowContent::Activity { item, .. }
                    | ConversationRowContent::Response { item, .. } => {
                        item.data
                            .native_id
                            .as_ref()
                            .map_or(item.data.id.as_str(), |native| native.as_str())
                            == id
                    }
                    _ => false,
                }
            {
                self.list.remeasure_items(index..index + 1);
            }
        }
    }
    fn new_chat(&mut self, cwd: String, window: &mut Window, cx: &mut Context<Self>) {
        self.tab = Tab::Chat;
        self.cancel_recording();
        self.dispatch(Intent::NewChat { cwd });
        self.composer
            .input
            .read(cx)
            .focus_handle(cx)
            .focus(window, cx);
    }
    fn open_chat(&mut self, id: SessionRef, window: &mut Window, cx: &mut Context<Self>) {
        self.tab = Tab::Chat;
        self.cancel_recording();
        self.dispatch(Intent::ReadThread(op::ReadThread::open(id)));
        self.composer
            .input
            .read(cx)
            .focus_handle(cx)
            .focus(window, cx);
    }
    fn detail(&mut self, turn_id: TurnId, item_id: ItemId) {
        let Some(thread_id) = self.selected().cloned() else {
            return;
        };
        let read = op::ReadItem {
            thread_id,
            turn_id,
            item_id,
        };
        if self
            .snapshot
            .operation_running(op::OperationKey::Item { item: read.clone() })
        {
            return;
        }
        let needed = self
            .thread()
            .and_then(|thread| thread.turns.as_ref())
            .and_then(|turns| turns.iter().find(|turn| turn.id == read.turn_id))
            .and_then(|turn| turn.items.as_ref())
            .and_then(|items| items.iter().find(|item| item.id == read.item_id))
            .is_some_and(|item| item.is_deferred());
        if !needed {
            return;
        }
        let generation = self.snapshot.epoch;
        let item_id = read.item_id.clone();
        self.perform(
            Intent::ReadItem(read),
            OperationCompletion::Item {
                generation,
                item_id,
            },
        );
    }
    fn switch_host(
        &mut self,
        remote: Option<RemoteHost>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.remote == remote && self.snapshot.connected {
            return;
        }
        self.cancel_recording();
        self.dictation = None;
        let preferences = self
            .session
            .as_ref()
            .map(|session| {
                agent_core::persistence::encode_model_preferences(&session.store.snapshot())
            })
            .transpose();
        let preferences = match preferences {
            Ok(preferences) => preferences,
            Err(error) => {
                self.error = error.to_string();
                return;
            }
        };
        self.session.take();
        self.remote = remote;
        self.model_controls.settings_scope = ModelDefaultsScope::Global;
        self.snapshot = Arc::default();
        self.pending_quote = None;
        self.pending_explanation = None;
        self.editor.text.pending = None;
        self.editor.path = None;
        self.worktrees.dirty = false;
        self.worktrees.saving = false;
        self.worktrees.save_pending = false;
        self.images.clear();
        self.image_gallery = None;
        self.rows.clear();
        self.list.reset(0);
        self.rendered = None;
        self.requests.clear();
        self.diffs.clear();
        self.markdown_cache.clear();
        let value = self.composer.text.replace("");
        self.composer
            .input
            .update(cx, |input, cx| input.set_value(value, window, cx));
        self.connect(preferences);
    }
    fn send(&mut self, cx: &Context<Self>) {
        if !self.snapshot.connected || self.busy > 0 {
            return;
        }
        if let Some(dictation) = &self.dictation {
            if dictation.phase == Phase::Recording {
                self.finish_dictation(true);
            }
            return;
        }
        if self.composer.input.read(cx).value().trim().is_empty()
            && self.draft().attachments.is_empty()
        {
            return;
        }
        self.busy += 1;
        self.perform(
            Intent::Submit {
                thread_id: None,
                client_user_message_id: uuid::Uuid::new_v4().to_string().into(),
            },
            OperationCompletion::Busy,
        );
    }
    fn quote_selection(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.session.is_none() {
            let queued = self.pending_quote.get_or_insert_with(String::new);
            if !queued.is_empty() {
                queued.push_str("\n\n");
            }
            queued.push_str(text);
            return;
        }
        selection::append_to_composer(&self.composer.input, text, window, cx);
    }

    fn explain_selection(&mut self, text: &str, cx: &mut Context<Self>) {
        let Some(store) = self.session.as_ref().map(|session| session.store.clone()) else {
            self.pending_explanation = Some(text.into());
            return;
        };
        let cwd = self.snapshot.selected_directory();
        let Some(provider) = self
            .snapshot
            .model_provider_for_draft(self.draft_key().clone())
        else {
            self.error = "利用可能なエージェントを選択してください。".into();
            return;
        };
        let prompt = format!("次の選択範囲について詳しく説明してください。\n\n{text}");
        self.busy += 1;
        self.effect(
            async move {
                let Outcome::StartedThread { id } = store
                    .dispatch(Intent::CreateSession(op::CreateSession {
                        provider,
                        cwd: Some(cwd),
                        model: None,
                    }))
                    .await
                    .map_err(|error| error.to_string())?
                else {
                    return Err("新しいサイドチャットを作成できませんでした".into());
                };
                // The new thread has its own draft; existing side-chat input stays intact.
                store
                    .dispatch(Intent::SetDraftText {
                        thread_id: id.clone().into(),
                        text: prompt,
                    })
                    .await
                    .map_err(|error| error.to_string())?;
                store
                    .dispatch(Intent::Submit {
                        thread_id: Some(id),
                        client_user_message_id: uuid::Uuid::new_v4().to_string().into(),
                    })
                    .await
                    .map_err(|error| error.to_string())
            },
            |result| Update::Completed(OperationCompletion::Busy, result),
        );
        cx.notify();
    }

    fn refresh_threads(&self) {
        self.dispatch(Intent::ListSessions(op::ListSessions::new(
            (*self.snapshot.list_query).clone(),
        )));
    }
    fn refresh_review(&self) {
        if !self.snapshot.selected_directory().is_empty() {
            self.dispatch(Intent::ReviewWorkspace(op::ReviewWorkspace {
                cwd: self.snapshot.navigation.cwd.clone(),
            }));
        }
    }
    fn panel_scope(&self) -> PanelScope {
        self.snapshot.panel_scope(
            self.remote
                .as_ref()
                .map_or_else(|| "local".into(), |host| host.id.clone()),
            self.draft_key().clone(),
        )
    }
    fn open_panel(&mut self, kind: Panel, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.panels.open(self.panel_scope(), kind, None);
        self.tab = Tab::Chat;
        self.activate_surface(id, window, cx);
    }
    fn hide_surfaces(&mut self, cx: &mut App) {
        self.cancel_recording();
        for resource in self.surface_resources.values() {
            match resource {
                SurfaceResource::Browser { view, .. } => {
                    view.update(cx, |view, _| view.set_visible(false))
                }
                SurfaceResource::SideChat(view) => {
                    view.update(cx, |view, cx| view.hide_surfaces(cx))
                }
                _ => (),
            }
        }
    }
    fn open_browser(&mut self, scope: PanelScope, url: &str, cx: &mut Context<Self>) {
        if let Err(error) = agent_protocol::browser::browser_url(url) {
            self.set_error(error);
            cx.notify();
            return;
        }
        let current = scope == self.panel_scope();
        let id = self.panels.open(scope, Panel::Browser, None);
        self.surface_resources.insert(
            id,
            SurfaceResource::Pending {
                url: Some(url.into()),
            },
        );
        if current {
            self.tab = Tab::Chat;
        }
        cx.notify();
    }
    fn activate_surface(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let key = self.panel_scope();
        if !self.panels.activate(key.clone(), id) {
            return;
        }
        let surface = self.panels.get(key).selected().unwrap().clone();
        match surface.kind {
            Panel::Browser => {
                self.surface_resources
                    .entry(id)
                    .or_insert(SurfaceResource::Pending { url: None });
            }
            Panel::Terminal => {
                if !self.surface_resources.contains_key(&id) {
                    let terminal = crate::terminal::Terminal::new(
                        self.remote.as_ref().map_or("", |remote| &remote.ticket),
                        self.snapshot.navigation.cwd.clone(),
                        format!("bex-terminal-{}", uuid::Uuid::new_v4()),
                        window,
                        cx,
                    );
                    self.surface_resources
                        .insert(id, SurfaceResource::Terminal(terminal));
                }
                if let Some(SurfaceResource::Terminal(terminal)) = self.surface_resources.get(&id) {
                    terminal.update(cx, |terminal, cx| terminal.focus(window, cx));
                }
            }
            Panel::SideChat => {
                if !self.surface_resources.contains_key(&id) {
                    let chat = cx.new(|cx| {
                        Self::new(
                            Mode::SideChat {
                                remote: self.remote.clone(),
                                cwd: self.snapshot.selected_directory(),
                            },
                            window,
                            cx,
                        )
                    });
                    self.surface_resources
                        .insert(id, SurfaceResource::SideChat(chat));
                }
            }
            Panel::File => {
                if let Some(path) = surface.path {
                    self.dispatch(Intent::ReadFile(op::ReadFile {
                        path,
                        discard_draft: false,
                    }));
                }
            }
            Panel::Files => self.dispatch(Intent::ListFiles(op::ListFiles {
                path: surface
                    .path
                    .unwrap_or_else(|| self.snapshot.navigation.cwd.clone()),
            })),
            Panel::Diff => self.refresh_review(),
            Panel::Agents | Panel::Home => (),
        }
        cx.notify();
    }
    fn close_surface(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.panels.scope(id) != Some(self.panel_scope()) {
            return;
        }
        self.panels.close(self.panel_scope(), id);
        match self.surface_resources.remove(&id) {
            Some(SurfaceResource::Browser { view, .. }) => {
                view.update(cx, |view, _| view.set_visible(false));
            }
            Some(SurfaceResource::Terminal(view)) => view.update(cx, |view, _| view.close()),
            Some(SurfaceResource::SideChat(view)) => {
                view.update(cx, |view, cx| view.hide_surfaces(cx))
            }
            _ => (),
        }
        if let Some(next) = self.panels.get(self.panel_scope()).active {
            self.activate_surface(next, window, cx);
        } else {
            self.panel_focus.focus(window, cx);
        }
        cx.notify();
    }
    fn pick_folder(&mut self) {
        self.busy += 1;
        self.effect(
            async {
                tokio::task::spawn_blocking(platform::choose_folder)
                    .await
                    .map_err(|error| error.to_string())
            },
            Update::Folder,
        );
    }
    fn composer_arrow(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.completion_key(key, window, cx) {
            return;
        }
        let moved = self.composer.input.update(cx, |input, cx| {
            if input.marked_text_range(window, cx).is_some() || !input.selected_range().is_empty() {
                return false;
            }
            let cursor = input.cursor();
            let target = if key == "up" { 0 } else { input.text().len() };
            // Off-screen offsets can be clamped to the first visible row by the input.
            if input.value()[cursor.min(target)..cursor.max(target)].contains('\n') {
                return false;
            }
            // Compare rendered rows so soft-wrapped text keeps normal vertical movement.
            let Some(caret) = input.range_to_bounds(&(cursor..cursor)) else {
                return false;
            };
            let Some(edge) = input.range_to_bounds(&(target..target)) else {
                return false;
            };
            if caret.origin.y != edge.origin.y {
                return false;
            }
            input.set_selected_range(target..target, cx);
            true
        });
        if moved {
            cx.stop_propagation();
        }
    }
    fn composer_enter(
        &mut self,
        action: &gpui_kit::component::input::Enter,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !action.shift && !action.secondary && self.accept_completion(window, cx) {
            cx.stop_propagation();
            return;
        }
        let submit = self.composer.input.update(cx, |input, cx| {
            composer_should_submit(input, action, window, cx)
        });
        if submit && self.snapshot.connected && self.busy == 0 {
            cx.stop_propagation();
            self.send(cx);
        }
    }
    fn paste_image(
        &mut self,
        _: &gpui_kit::component::input::Paste,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };
        if !item
            .entries()
            .iter()
            .any(|entry| matches!(entry, ClipboardEntry::Image(_)))
        {
            return;
        }
        cx.stop_propagation();
        if self.busy > 0 || self.dictation.is_some() {
            return;
        }
        self.attach_sources(move || {
            let directory = platform::state_dir()?.join("attachments");
            item.into_entries()
                .filter_map(|entry| match entry {
                    ClipboardEntry::Image(image) => {
                        Some(clipboard::save_image(&directory, image).map(PathBuf::from))
                    }
                    _ => None,
                })
                .collect()
        });
        cx.notify();
    }
    fn attach(&mut self) {
        self.attach_sources(|| Ok(platform::choose_files().unwrap_or_default()));
    }
    fn attach_sources(
        &mut self,
        sources: impl FnOnce() -> Result<Vec<PathBuf>, String> + Send + 'static,
    ) {
        let Some(store) = self.session.as_ref().map(|session| session.store.clone()) else {
            return;
        };
        let key = self.draft_key().to_owned();
        let remote = self.remote.is_some();
        let directory = self.snapshot.navigation.cwd.clone();
        self.busy += 1;
        self.effect(
            async move {
                let paths = tokio::task::spawn_blocking(sources)
                    .await
                    .map_err(|error| error.to_string())??;
                for path in paths {
                    let is_image = path
                        .extension()
                        .and_then(|extension| extension.to_str())
                        .and_then(image::ImageFormat::from_extension)
                        .is_some();
                    let name = path
                        .file_name()
                        .unwrap_or(path.as_os_str())
                        .to_string_lossy()
                        .into_owned();
                    let attachment = Attachment {
                        path: path.to_string_lossy().into_owned(),
                        name,
                        is_image,
                    };
                    let intent = if remote {
                        Intent::UploadAttachment(op::UploadAttachment {
                            draft_key: key.clone(),
                            attachment,
                            directory: directory.clone(),
                        })
                    } else {
                        Intent::AddAttachment {
                            draft_key: key.clone(),
                            attachment,
                        }
                    };
                    store
                        .dispatch(intent)
                        .await
                        .map_err(|error| error.to_string())?;
                }
                Ok(Outcome::Applied)
            },
            |result| Update::Completed(OperationCompletion::Busy, result),
        );
    }
    fn download(&mut self, source: String) {
        let Some(store) = self.session.as_ref().map(|session| session.store.clone()) else {
            return;
        };
        self.busy += 1;
        self.effect(
            async move {
                let name = Path::new(&source)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                let destination =
                    tokio::task::spawn_blocking(move || platform::choose_destination(&name))
                        .await
                        .map_err(|error| error.to_string())?;
                if let Some(destination) = destination {
                    store
                        .dispatch(Intent::DownloadFile(op::DownloadFile {
                            source,
                            destination: destination
                                .into_os_string()
                                .into_string()
                                .map_err(|_| "download destination is not UTF-8")?,
                        }))
                        .await
                        .map_err(|error| error.to_string())?;
                }
                Ok(Outcome::Applied)
            },
            |result| Update::Completed(OperationCompletion::Busy, result),
        );
    }
    fn browse(&mut self, path: String) {
        self.panels
            .open(self.panel_scope(), Panel::Files, Some(path.clone()));
        self.tab = Tab::Chat;
        self.dispatch(Intent::ListFiles(op::ListFiles { path }));
    }
    fn edit(&mut self, path: String, discard_draft: bool) {
        self.panels
            .open(self.panel_scope(), Panel::File, Some(path.clone()));
        self.dispatch(Intent::ReadFile(op::ReadFile {
            path,
            discard_draft,
        }));
    }
    fn save_file(&mut self) {
        let Some(path) = self.editor.path.clone() else {
            return;
        };
        self.busy += 1;
        self.perform(
            Intent::SaveFile(op::SaveFile { path }),
            OperationCompletion::Busy,
        );
    }
    fn worktree_input_changed(&mut self, cx: &App) {
        self.worktrees.dirty = !self
            .worktrees
            .matches(self.snapshot.workspace.settings.as_deref(), cx);
        self.worktrees.saved = false;
    }
    fn save_worktree_settings(&mut self, toggle: Option<WorktreeToggle>, cx: &Context<Self>) {
        let Some(settings) = &self.snapshot.workspace.settings else {
            return;
        };
        let mut settings = self.worktrees.edited(settings, cx);
        if let Some(toggle) = toggle {
            match toggle {
                WorktreeToggle::Create(checked) => settings.create_on_new_session = checked,
                WorktreeToggle::Copy(checked) => settings.copy_on_create = checked,
                WorktreeToggle::DeleteMerged(checked) => settings.delete_merged = checked,
            }
        }
        if self.snapshot.workspace.settings.as_deref() == Some(&settings) {
            return;
        }
        if self.worktrees.saving {
            self.worktrees.save_pending = true;
            return;
        }
        self.worktrees.saving = true;
        self.worktrees.saved = false;
        self.perform(
            Intent::UpdateWorktreeSettings(op::UpdateWorktreeSettings { settings }),
            OperationCompletion::WorktreeSettings,
        );
    }
    fn respond(&mut self, id: RequestId, answer: Answer) {
        if let Some(inputs) = self.requests.get_mut(&id) {
            inputs.sent = true;
        }
        self.perform(
            Intent::Respond(op::Respond {
                request_id: id.clone(),
                answer,
            }),
            OperationCompletion::Request(id),
        );
    }
}
fn conversation_rows(
    rendered: &Option<Arc<agent_core::presentation::conversation::RenderedConversation>>,
    expanded_work: &HashMap<String, agent_core::presentation::conversation::ActivityExpansion>,
) -> Vec<ConversationRow> {
    let mut rows = Vec::new();
    if let Some(rendered) = rendered {
        // T3's timeline virtualizes messages and work rows, never an entire
        // provider turn. A long-running turn must not mount its offscreen text.
        for turn in &rendered.turns {
            let start = rows.len();
            let mut expanded = false;
            for (index, row) in turn.rows.iter().enumerate() {
                match &row.content {
                    ConversationRowContent::ActivityHeader { activity } => {
                        expanded = agent_core::presentation::conversation::activity_is_expanded(
                            activity,
                            expanded_work.get(&activity.id).cloned(),
                        );
                    }
                    ConversationRowContent::Activity { .. } if !expanded => continue,
                    _ => (),
                }
                rows.push(ConversationRow::Turn {
                    turn: turn.clone(),
                    index,
                    last: false,
                });
            }
            if rows.len() > start
                && let Some(ConversationRow::Turn { last, .. }) = rows.last_mut()
            {
                *last = true;
            }
        }
        rows.extend(rendered.queued.iter().filter_map(|item| {
            if let agent_core::presentation::conversation::ItemSource::Pending(id, pending) =
                &item.source
            {
                Some(ConversationRow::Pending(id.clone(), pending.clone()))
            } else {
                None
            }
        }));
    }
    rows.extend(
        rendered
            .iter()
            .flat_map(|conversation| &conversation.request_rows)
            .filter_map(|row| {
                if let ConversationRowContent::PendingRequest { request } = &row.content {
                    Some(ConversationRow::Request(request.clone()))
                } else {
                    None
                }
            }),
    );
    rows
}

fn toggle_set(set: &mut HashSet<String>, key: &str) {
    if !set.remove(key) {
        set.insert(key.into());
    }
}
fn fenced(text: &str, language: &str) -> String {
    let longest = text
        .lines()
        .map(|line| line.chars().take_while(|c| *c == '`').count())
        .max()
        .unwrap_or(0)
        .max(2)
        + 1;
    let fence = "`".repeat(longest);
    format!("{fence}{language}\n{text}\n{fence}")
}
fn literal(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    for c in text.chars() {
        if "\\`*_{}[]<>()#+-.!|>~".contains(c) {
            result.push('\\');
        }
        if c == '\n' {
            result.push_str("  ");
        }
        result.push(c);
    }
    result
}
fn sync_error_banner(banner: &mut String, previous: Option<&str>, next: Option<&str>) {
    if previous != next {
        if let Some(error) = next {
            *banner = error.to_owned();
        } else if previous == Some(banner.as_str()) {
            banner.clear();
        }
    }
}

fn composer_should_submit(
    input: &TextareaState,
    action: &gpui_kit::component::input::Enter,
    window: &mut Window,
    cx: &mut Context<TextareaState>,
) -> bool {
    !action.shift && !action.secondary && input.marked_text_range(window, cx).is_none()
}

#[cfg(test)]
mod composer_tests {
    use super::{TextareaState, composer_should_submit};
    use gpui_kit as gpui;
    use gpui_kit::{EntityInputHandler, TestAppContext};
    #[gpui::test]
    fn enter_submits_committed_text_regardless_of_cursor_or_selection(cx: &mut TestAppContext) {
        cx.update(crate::appearance::init);
        let input = cx.add_window(TextareaState::new);
        input
            .update(cx, |input, window, cx| {
                let enter = gpui_kit::component::input::Enter {
                    secondary: false,
                    shift: false,
                };
                input.set_value("日本語🙂", window, cx);
                let end = input.text().len();
                input.set_selected_range(end..end, cx);
                assert!(composer_should_submit(input, &enter, window, cx));
                input.set_selected_range(3..3, cx);
                assert!(composer_should_submit(input, &enter, window, cx));
                input.set_selected_range(0..end, cx);
                assert!(composer_should_submit(input, &enter, window, cx));
                input.set_selected_range(end..end, cx);
                assert!(!composer_should_submit(
                    input,
                    &gpui_kit::component::input::Enter {
                        shift: true,
                        secondary: false
                    },
                    window,
                    cx
                ));
                input.replace_and_mark_text_in_range(None, "変換", Some(2..2), window, cx);
                assert!(!composer_should_submit(input, &enter, window, cx));
                input.unmark_text(window, cx);
                assert!(composer_should_submit(input, &enter, window, cx));
            })
            .unwrap();
    }
}

#[cfg(test)]
mod error_tests {
    use super::sync_error_banner;

    #[test]
    fn recovered_store_error_clears_its_banner() {
        let mut banner = String::new();
        sync_error_banner(&mut banner, None, Some("thread read failed"));
        assert_eq!(banner, "thread read failed");
        sync_error_banner(&mut banner, Some("thread read failed"), None);
        assert!(banner.is_empty());
    }

    #[test]
    fn recovery_preserves_a_newer_local_error_and_respects_dismissal() {
        let mut banner = "draft save failed".to_owned();
        sync_error_banner(&mut banner, Some("thread read failed"), None);
        assert_eq!(banner, "draft save failed");
        banner.clear();
        sync_error_banner(
            &mut banner,
            Some("thread read failed"),
            Some("thread read failed"),
        );
        assert!(banner.is_empty());
        sync_error_banner(
            &mut banner,
            Some("thread read failed"),
            Some("disconnected"),
        );
        assert_eq!(banner, "disconnected");
    }
}

#[cfg(test)]
mod completion_tests {
    use super::{
        Arc, Context, ConversationRow, ConversationRowContent, Desktop, Mode, OperationCompletion,
        Outcome, Snapshot, Thread, Update, Window, conversation_rows,
    };
    use gpui_kit as gpui;
    use gpui_kit::TestAppContext;

    #[gpui::test]
    fn completion_messages_preserve_newer_host_navigation_and_input(cx: &mut TestAppContext) {
        // Do not drive this runtime: this test exercises UI result delivery, not provisioning.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        cx.update(|cx| {
            crate::appearance::init(cx);
            cx.set_global(crate::Runtime {
                handle: runtime.handle().clone(),
                connections: Arc::new(crate::platform::Connections::default()),
                closing: tokio_util::task::TaskTracker::new(),
                logging_error: None,
            });
        });
        let view = cx.add_window(|window, cx| {
            Desktop::new(
                Mode::SideChat {
                    remote: None,
                    cwd: "/fixture".into(),
                },
                window,
                cx,
            )
        });
        view.update(cx, |view, window, cx| {
            view.epoch = 3;
            Arc::make_mut(&mut view.snapshot).epoch = 5;
            view.busy = 2;
            view.composer.text.pending = Some(9);
            view.editor.text.pending = Some(12);
            let deliver = |view: &mut Desktop,
                           host,
                           kind,
                           result,
                           window: &mut Window,
                           cx: &mut Context<Desktop>| {
                view.receive((host, Update::Completed(kind, result)), window, cx);
            };
            deliver(
                view,
                2,
                OperationCompletion::Busy,
                Err("obsolete host".into()),
                window,
                cx,
            );
            assert_eq!(view.busy, 2);
            assert!(view.error.is_empty());
            deliver(
                view,
                3,
                OperationCompletion::Composer(8),
                Ok(Outcome::Applied),
                window,
                cx,
            );
            deliver(
                view,
                3,
                OperationCompletion::Editor(11),
                Ok(Outcome::Applied),
                window,
                cx,
            );
            assert_eq!(view.composer.text.pending, Some(9));
            assert_eq!(view.editor.text.pending, Some(12));
            deliver(
                view,
                3,
                OperationCompletion::Composer(9),
                Ok(Outcome::Applied),
                window,
                cx,
            );
            deliver(
                view,
                3,
                OperationCompletion::Editor(12),
                Ok(Outcome::Applied),
                window,
                cx,
            );
            assert_eq!(view.composer.text.pending, None);
            assert_eq!(view.editor.text.pending, None);
            deliver(
                view,
                3,
                OperationCompletion::Busy,
                Ok(Outcome::Applied),
                window,
                cx,
            );
            assert_eq!(view.busy, 1);
        })
        .unwrap();
    }

    #[test]
    fn row_projection_keeps_repeated_turns_and_scopes_requests_without_changing_input() {
        let mut source: Arc<Thread> = Arc::new(serde_json::from_value(serde_json::json!({"id":{"provider":"codex","id":"selected"},"turns":[{"id":"repeated","items":[{"id":"first","status":"unknown","clientInputId":null,"body":{"inline":{"body":{"assistantText":{"text":"first answer","phase":"unknown"}}}}}],"status":"unknown"},{"id":"repeated","items":[{"id":"second","status":"unknown","clientInputId":null,"body":{"inline":{"body":{"assistantText":{"text":"second answer","phase":"unknown"}}}}}],"status":"unknown"}]})).unwrap());
        let mut snapshot = Snapshot::default();
        Arc::make_mut(&mut snapshot.navigation).thread_id =
            Some(agent_protocol::session::SessionRef {
                provider: agent_protocol::session::ProviderKind::Codex,
                id: "selected".into(),
            });
        let request = Arc::new(agent_protocol::requests::Request {
            id: "global".into(),
            target: agent_protocol::requests::RequestTarget::Session,
            delivery: agent_protocol::session::RequestDelivery::Awaiting,
            body: agent_protocol::requests::RequestBody::Elicitation {
                server: "fixture".into(),
                message: "input".into(),
                input: agent_protocol::requests::ElicitationInput::Form { fields: vec![] },
            },
        });
        Arc::make_mut(&mut source)
            .requests
            .insert(request.id.clone(), request.clone());
        let mut other = (*source).clone();
        let request = Arc::make_mut(other.requests.get_mut("global").unwrap());
        request.id = "other".into();
        other.id = Some(agent_protocol::session::SessionRef {
            provider: agent_protocol::session::ProviderKind::Codex,
            id: "other".into(),
        });
        Arc::make_mut(&mut snapshot.conversations)
            .insert(other.id.clone().unwrap(), Arc::new(other));
        let before = snapshot.clone();
        let rendered = Some(
            agent_core::presentation::conversation::project_conversation(
                &snapshot,
                source.clone(),
                &None,
            ),
        );
        let rows = conversation_rows(&rendered, &Default::default());
        let repeated = conversation_rows(&rendered, &Default::default());
        let messages: Vec<_> = rows
            .iter()
            .filter_map(|row| {
                let ConversationRow::Turn { turn, index, .. } = row else {
                    return None;
                };
                let ConversationRowContent::Response { item, .. } = &turn.rows[*index].content
                else {
                    return None;
                };
                Some((&turn.source, item.data.body.as_deref()))
            })
            .collect();
        assert_eq!(messages.len(), 2);
        for ((row, body), (turn, expected)) in messages.iter().zip(
            source
                .turns
                .as_ref()
                .unwrap()
                .iter()
                .zip(["first answer", "second answer"]),
        ) {
            assert!(Arc::ptr_eq(row, turn));
            assert_eq!(*body, Some(expected));
        }
        let requests: Vec<_> = rows
            .iter()
            .filter_map(|row| match row {
                ConversationRow::Request(request) => Some(request.id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(requests, ["global"]);
        assert!(rows.iter().zip(&repeated).all(|(a, b)| a.unchanged(b)));
        assert_eq!(snapshot, before);
    }
}
