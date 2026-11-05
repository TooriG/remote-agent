//! Native settings forms and their asynchronous work belong to this owner.
//! The window only opens/closes the surface and supplies immutable core state.
use super::*;
use view::{errors, settings::SettingsNavigation};

pub(super) enum PreferencesEvent {
    Closed,
    ImportProjects,
    Notice(errors::NoticeAction),
    OpenBrowser(String),
    OpenChat(SessionRef),
}

pub(super) enum Completion {
    Account,
    WorktreeSettings,
    RemoveWorktree,
}

#[derive(PartialEq)]
struct DisplayInputs {
    connection: agent_core::state::Connection,
    catalog: agent_core::state::Catalog,
    settings: Option<Arc<agent_protocol::models::WorktreeSettings>>,
    worktrees: Option<Arc<Vec<agent_protocol::models::Worktree>>>,
    cwd: String,
    key: DraftKey,
    provider: Option<agent_protocol::session::ProviderKind>,
    model: Option<agent_protocol::models::ModelRef>,
    effort: Option<String>,
    tier: Option<String>,
    operations: agent_core::state::Operations,
}
impl DisplayInputs {
    fn from_snapshot(snapshot: &Snapshot) -> Self {
        let key = &snapshot.pane.navigation.draft_key;
        let draft = snapshot.input.drafts.get(key);
        Self {
            connection: snapshot.connection.clone(),
            catalog: snapshot.catalog.clone(),
            settings: snapshot.files.workspace.settings.clone(),
            worktrees: snapshot.files.workspace.worktrees.clone(),
            cwd: snapshot.pane.navigation.cwd.clone(),
            key: key.clone(),
            provider: snapshot.model_provider_for_draft(key.clone()),
            model: draft.and_then(|draft| draft.model.clone()),
            effort: draft.and_then(|draft| draft.effort.clone()),
            tier: draft.and_then(|draft| draft.service_tier.clone()),
            operations: snapshot.operations.clone(),
        }
    }
}

pub(super) struct Preferences {
    pub(super) snapshot: Arc<Snapshot>,
    inputs: DisplayInputs,
    pub(super) pane: Option<Arc<agent_core::store::Pane>>,
    handle: tokio::runtime::Handle,
    epoch: u64,
    updates: async_channel::Sender<(u64, Completion, Result<Outcome, String>)>,
    pub(super) account: forms::AccountForm,
    pub(super) worktrees: forms::WorktreeForm,
    pub(super) model_controls: forms::ModelControls,
    pub(super) remote: Option<RemoteHost>,
    pub(super) hosts: Option<Entity<Hosts>>,
    pub(super) app_updates: Option<Entity<crate::updates::Updates>>,
    pub(super) busy: usize,
    pub(super) settings_page: SettingsPage,
    pub(super) settings_search: Entity<InputState>,
    pub(super) settings_focus: FocusHandle,
    pub(super) settings_navigation: Entity<SettingsNavigation>,
    pub(super) local_errors: BTreeMap<ErrorScope, String>,
    pub(super) window_error: Option<errors::ErrorRow>,
    pub(super) opened: bool,
    _subscriptions: Vec<Subscription>,
}
impl EventEmitter<PreferencesEvent> for Preferences {}

impl Preferences {
    pub(super) fn new(
        hosts: Option<Entity<Hosts>>,
        app_updates: Option<Entity<crate::updates::Updates>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let language = crate::localization::language(cx);
        let account = forms::AccountForm::new(window, cx);
        let worktrees = forms::WorktreeForm::new(window, cx);
        let model_controls = forms::ModelControls::new(window, cx);
        let settings_search =
            cx.new(|cx| InputState::new(window, cx).placeholder(language.text("設定を検索")));
        let settings_navigation = cx.new(|_| SettingsNavigation::new(settings_search.clone()));
        let (updates, receive) = async_channel::unbounded();
        cx.spawn_in(window, async move |view, cx| {
            while let Ok((epoch, completion, result)) = receive.recv().await {
                if view
                    .update_in(cx, |view: &mut Self, window, cx| {
                        if epoch == view.epoch {
                            view.completed(epoch, completion, result, window, cx);
                        }
                    })
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
                    .update(cx, |view: &mut Self, _| view.poll_account())
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        let subscriptions = vec![
            cx.observe_global_in::<crate::localization::Language>(window, |view, window, cx| {
                view.language_changed(window, cx)
            }),
            cx.subscribe_in(
                &settings_navigation,
                window,
                |view, _, page: &SettingsPage, window, cx| {
                    view.open_settings(*page, window, cx);
                    cx.notify();
                },
            ),
            cx.subscribe(&settings_search, |view, input, event, cx| {
                if matches!(event, InputEvent::Change) {
                    let query = input.read(cx).value().to_string();
                    view.settings_navigation
                        .update(cx, |nav, cx| nav.sync(view.settings_page, query, cx));
                }
            }),
            cx.subscribe(&account.code, |_, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.subscribe(&model_controls.search, |_, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
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
        Self {
            snapshot: Arc::default(),
            inputs: DisplayInputs::from_snapshot(&Snapshot::default()),
            pane: None,
            handle: cx.global::<Runtime>().handle.clone(),
            epoch: 0,
            updates,
            account,
            worktrees,
            model_controls,
            remote: None,
            hosts,
            app_updates,
            busy: 0,
            settings_page: SettingsPage::General,
            settings_search,
            settings_focus: cx.focus_handle(),
            settings_navigation,
            local_errors: BTreeMap::new(),
            window_error: None,
            opened: false,
            _subscriptions: subscriptions,
        }
    }

    pub(super) fn sync(
        &mut self,
        snapshot: Arc<Snapshot>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let inputs = DisplayInputs::from_snapshot(&snapshot);
        if self.snapshot.connection.epoch != snapshot.connection.epoch {
            self.epoch += 1;
            self.account.busy = false;
            self.account.polling = false;
            self.worktrees.saving = false;
            self.worktrees.removing = false;
            self.worktrees.save_pending = false;
        }
        if self
            .snapshot
            .catalog
            .account
            .login
            .as_ref()
            .map(|login| &login.login_id)
            != snapshot
                .catalog
                .account
                .login
                .as_ref()
                .map(|login| &login.login_id)
        {
            self.account
                .code
                .update(cx, |input, cx| input.set_value("", window, cx));
        }
        if !self.worktrees.dirty
            && !self.worktrees.saving
            && self.snapshot.files.workspace.settings != snapshot.files.workspace.settings
            && let Some(settings) = &snapshot.files.workspace.settings
        {
            self.worktrees.show(settings, window, cx);
        }
        self.snapshot = snapshot;
        if self.inputs != inputs {
            self.inputs = inputs;
            cx.notify();
        }
    }
    pub(super) fn bind(
        &mut self,
        pane: Option<Arc<agent_core::store::Pane>>,
        remote: Option<RemoteHost>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let same = match (&self.pane, &pane) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if !same {
            self.epoch += 1;
            self.account.busy = false;
            self.account.polling = false;
            self.account.login_draft = None;
            self.account.sign_out = None;
            self.model_controls.provider = None;
            self.worktrees.removal = None;
            self.worktrees.removing = false;
            self.worktrees.saving = false;
            self.worktrees.dirty = false;
            self.worktrees.save_pending = false;
            self.pane = pane;
            self.account
                .code
                .update(cx, |input, cx| input.set_value("", window, cx));
            self.worktrees.scope = None;
            self.snapshot = Arc::default();
            cx.notify();
        }
        if self.remote != remote {
            self.remote = remote;
            cx.notify();
        }
    }
    pub(super) fn perform(&self, intent: Intent, completion: Completion) {
        let Some(pane) = &self.pane else {
            return;
        };
        let receipt = pane.dispatch(intent);
        let updates = self.updates.clone();
        let epoch = self.epoch;
        self.handle.spawn(async move {
            let result = receipt.await.map_err(|error| error.to_string());
            let _ = updates.send((epoch, completion, result)).await;
        });
    }
    pub(super) fn dispatch(&self, intent: Intent) {
        if let Some(pane) = &self.pane {
            drop(pane.dispatch(intent));
        }
    }
    pub(super) fn sync_from_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(snapshot) = self.pane.as_ref().and_then(|pane| pane.snapshot().ok()) {
            self.sync(snapshot, window, cx);
        }
    }
    fn poll_account(&mut self) {
        if self.account.polling
            && !self.account.busy
            && self.snapshot.connection.connected
            && let Some(login) = &self.snapshot.catalog.account.login
        {
            let (provider, id) = (login.provider, login.login_id.clone());
            self.account.busy = true;
            self.perform(
                Intent::ReadAccountLogin(op::ReadAccountLogin {
                    provider,
                    id,
                    thread_id: self.account.login_draft.clone(),
                }),
                Completion::Account,
            );
        }
    }
    fn completed(
        &mut self,
        epoch: u64,
        completion: Completion,
        result: Result<Outcome, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sync_from_pane(window, cx);
        if self.epoch != epoch {
            return;
        }
        match completion {
            Completion::Account => {
                self.account.busy = false;
                self.account.polling =
                    result.is_ok() && self.snapshot.catalog.account.login.is_some();
                if result.is_err() {
                    self.dispatch(Intent::ListAccounts(op::ListAccounts {}));
                }
                if self.snapshot.catalog.account.login.is_none() {
                    self.account.login_draft = None;
                }
            }
            Completion::WorktreeSettings => {
                self.worktrees.saving = false;
                self.worktrees.dirty = !self
                    .worktrees
                    .matches(self.snapshot.files.workspace.settings.as_deref(), cx);
                self.worktrees.saved = !self.worktrees.dirty;
                if std::mem::take(&mut self.worktrees.save_pending) {
                    self.save_worktree_settings(None, cx);
                }
            }
            Completion::RemoveWorktree => {
                self.worktrees.removing = false;
                if result.is_ok() {
                    self.worktrees.removal = None;
                } else {
                    self.dispatch(Intent::ListWorktrees(op::ListWorktrees {}));
                }
            }
        }
        cx.notify();
    }
    pub(super) fn model_controls_enabled(&self) -> bool {
        self.snapshot.connection.connected
            && !self.account.busy
            && self.busy == 0
            && self.snapshot.catalog.account.login.is_none()
    }
    pub(super) fn draft_key(&self) -> &DraftKey {
        &self.snapshot.pane.navigation.draft_key
    }
    pub(super) fn draft(&self) -> &Draft {
        static EMPTY: std::sync::LazyLock<Draft> = std::sync::LazyLock::new(Draft::default);
        self.snapshot
            .input
            .drafts
            .get(self.draft_key())
            .map_or(&EMPTY, AsRef::as_ref)
    }
    pub(super) fn selected_model(&self) -> Option<&Model> {
        selected_model(&self.snapshot.catalog.models, self.draft().model.as_ref())
    }
    pub(super) fn language_changed(&self, window: &mut Window, cx: &mut Context<Self>) {
        let language = crate::localization::language(cx);
        for (input, placeholder) in [
            (&self.model_controls.search, "モデルを検索"),
            (&self.account.code, "認証コード・APIキーを貼り付け"),
            (
                &self.worktrees.directory,
                "ホスト上の絶対パス（空欄で既定）",
            ),
            (&self.settings_search, "設定を検索"),
        ] {
            input.update(cx, |input, cx| {
                input.set_placeholder(language.text(placeholder), window, cx)
            });
        }
        cx.notify();
    }
}

impl Render for Preferences {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.settings_modal(cx)
    }
}

impl Preferences {
    fn worktree_input_changed(&mut self, cx: &App) {
        self.worktrees.dirty = !self
            .worktrees
            .matches(self.snapshot.files.workspace.settings.as_deref(), cx);
        self.worktrees.saved = false;
    }
    pub(super) fn save_worktree_settings(
        &mut self,
        toggle: Option<WorktreeToggle>,
        cx: &Context<Self>,
    ) {
        let Some(settings) = &self.snapshot.files.workspace.settings else {
            return;
        };
        let mut settings = self.worktrees.edited(settings, cx);
        if let Some(toggle) = toggle {
            match toggle {
                WorktreeToggle::Create(checked) => settings.create_on_new_session = checked,
                WorktreeToggle::Copy(checked) => settings.copy_on_create = checked,
                WorktreeToggle::DeleteUnchanged(checked) => settings.delete_unchanged = checked,
                WorktreeToggle::ProjectDeleteUnchanged(project, Some(checked)) => {
                    settings.project_delete_unchanged.insert(project, checked);
                }
                WorktreeToggle::ProjectDeleteUnchanged(project, None) => {
                    settings.project_delete_unchanged.remove(&project);
                }
            }
        }
        if self.snapshot.files.workspace.settings.as_deref() == Some(&settings) {
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
            Completion::WorktreeSettings,
        );
    }
}

impl Preferences {
    pub(in crate::app) fn notice_action(
        &mut self,
        action: errors::NoticeAction,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let errors::NoticeAction::DismissLocal(scope, op::ErrorId::Application { message }) =
            &action
            && self.local_errors.get(scope) == Some(message)
        {
            self.local_errors.remove(scope);
        } else {
            cx.emit(PreferencesEvent::Notice(action));
        }
    }
}

impl Preferences {
    pub(in crate::app) fn settings_errors(&self) -> Vec<errors::ErrorRow> {
        let mut rows =
            errors::error_rows(&self.snapshot, &self.local_errors, &ErrorScope::Settings);
        if let Some(error) = &self.window_error {
            rows.push(error.clone());
        }
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_kit as gpui;
    use std::cell::Cell;

    fn runtime(cx: &mut TestAppContext) -> tokio::runtime::Runtime {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        cx.update(|cx| {
            crate::appearance::init(cx);
            cx.set_global(Runtime {
                handle: runtime.handle().clone(),
                sessions: Default::default(),
                connections: Default::default(),
                closing: Default::default(),
                logging_error: None,
            });
        });
        runtime
    }

    #[gpui::test]
    fn typing_a_settings_field_does_not_notify_the_window(cx: &mut TestAppContext) {
        let _runtime = runtime(cx);
        let mut desktop = None;
        let window = cx.open_window(size(px(700.), px(700.)), |window, cx| {
            let view = cx.new(|cx| {
                Desktop::new(
                    Mode::Fixture {
                        remote: None,
                        cwd: "/fixture".into(),
                    },
                    window,
                    cx,
                )
            });
            desktop = Some(view.clone());
            crate::activity::ActivityWindow::new(view, window, cx)
        });
        cx.run_until_parked();
        let desktop = desktop.unwrap();
        let notifications = Rc::new(Cell::new(0));
        let count = notifications.clone();
        cx.update(|cx| {
            cx.observe(&desktop, move |_, _| count.set(count.get() + 1))
                .detach();
        });
        let preferences = desktop.read_with(cx, |view, _| view.preferences.clone());
        let changes = Rc::new(Cell::new(0));
        let count = changes.clone();
        cx.update(|cx| {
            cx.observe(&preferences, move |_, _| count.set(count.get() + 1))
                .detach();
        });
        window
            .update(cx, |_, window, cx| {
                preferences.update(cx, |view, cx| {
                    view.account.code.update(cx, |input, cx| {
                        input.set_value("typed-key", window, cx);
                        cx.emit(InputEvent::Change);
                    })
                });
            })
            .unwrap();
        cx.run_until_parked();
        assert!(changes.get() > 0, "the field must notify its own controls");
        assert_eq!(
            notifications.get(),
            0,
            "settings input must not broadcast Desktop changes"
        );
    }

    #[gpui::test]
    fn draft_text_and_conversation_publications_do_not_notify_settings(cx: &mut TestAppContext) {
        let _runtime = runtime(cx);
        let (view, window) =
            cx.add_window_view(|window, cx| Preferences::new(None, None, window, cx));
        window.run_until_parked();
        let notifications = Rc::new(Cell::new(0));
        let count = notifications.clone();
        window.update(|_, cx| {
            let app: &mut App = cx;
            app.observe(&view, move |_, _| count.set(count.get() + 1))
                .detach();
        });
        let mut snapshot = Snapshot::default();
        for text in ["a", "ab", "abc"] {
            Arc::make_mut(&mut snapshot.input.drafts).insert(
                snapshot.pane.navigation.draft_key.clone(),
                Arc::new(Draft {
                    text: text.into(),
                    ..Default::default()
                }),
            );
            let id = SessionRef {
                provider: agent_protocol::session::ProviderKind::Codex,
                id: "stream".into(),
            };
            Arc::make_mut(&mut snapshot.sessions.conversations).insert(
                id.clone(),
                Arc::new(Thread {
                    id: Some(id),
                    name: Some(text.into()),
                    ..Default::default()
                }),
            );
            window.update(|window, cx| {
                view.update(cx, |view, cx| {
                    view.sync(Arc::new(snapshot.clone()), window, cx)
                })
            });
            window.run_until_parked();
        }
        assert_eq!(notifications.get(), 0);
        window.update(|window, cx| {
            Arc::make_mut(&mut snapshot.catalog.models).push(
                serde_json::from_value(serde_json::json!({
                    "id":"new","model":{"provider":"codex","id":"new"},"displayName":"New"
                }))
                .unwrap(),
            );
            view.update(cx, |view, cx| view.sync(Arc::new(snapshot), window, cx));
        });
        window.run_until_parked();
        assert!(
            notifications.get() > 0,
            "catalog changes must still reach open controls"
        );
    }

    #[gpui::test]
    fn replacing_a_pane_discards_credentials_and_old_completions_but_allows_retry(
        cx: &mut TestAppContext,
    ) {
        let runtime = runtime(cx);
        let stores: Vec<_> = (0..2)
            .map(|_| {
                runtime.block_on(async {
                    Arc::new(agent_core::store::Store::offline(Snapshot::default()))
                })
            })
            .collect();
        let (view, window) =
            cx.add_window_view(|window, cx| Preferences::new(None, None, window, cx));
        let mut old_epoch = 0;
        window.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.bind(Some(stores[0].primary_pane()), None, window, cx);
                view.sync_from_pane(window, cx);
                view.account
                    .code
                    .update(cx, |input, cx| input.set_value("previous-key", window, cx));
                old_epoch = view.epoch;
                view.bind(Some(stores[1].primary_pane()), None, window, cx);
                view.sync_from_pane(window, cx);
                assert!(view.account.code.read(cx).value().is_empty());
                view.account.busy = true;
                view.updates
                    .try_send((old_epoch, Completion::Account, Err("old".into())))
                    .unwrap();
            })
        });
        window.run_until_parked();
        window.update(|_, cx| {
            view.update(cx, |view, _| {
                assert!(
                    view.account.busy,
                    "an old pane cannot finish a new pane's operation"
                );
                view.updates
                    .try_send((view.epoch, Completion::Account, Err("current".into())))
                    .unwrap();
            })
        });
        window.run_until_parked();
        window.update(|window, cx| {
            view.update(cx, |view, cx| {
                assert!(!view.account.busy);
                assert!(!view.account.polling);
                let mut snapshot = (*view.snapshot).clone();
                snapshot.connection.connected = true;
                view.sync(Arc::new(snapshot), window, cx);
                view.account_operation(Intent::StartAccountLogin(op::StartAccountLogin {
                    provider: agent_protocol::session::ProviderKind::Codex,
                }));
                assert!(
                    view.account.busy,
                    "a current connection can retry after failure"
                );
            })
        });
    }
}
