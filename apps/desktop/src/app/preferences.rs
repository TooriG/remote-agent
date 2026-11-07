//! Native settings forms and their asynchronous work belong to this owner.
//! The window only opens/closes the surface and supplies immutable core state.
use super::*;
use agent_core::presentation::settings::SettingsPresentation;
use view::{errors, settings::SettingsNavigation};

pub(super) enum PreferencesEvent {
    Closed,
    ImportProjects,
    Notice(errors::NoticeAction),
    Failed(String),
    OpenBrowser(String),
    OpenChat(SessionRef),
    Completed {
        epoch: u64,
        completion: Completion,
        result: Result<Outcome, String>,
    },
}

#[derive(Clone, Copy)]
pub(super) enum Completion {
    Account,
    WorktreeSettings,
    RemoveWorktree,
}

pub(super) struct Preferences {
    pub(super) metadata: Arc<SettingsPresentation>,
    pub(super) projects: Vec<(String, String)>,
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
    pub(super) blocked: bool,
    pub(super) settings_page: SettingsPage,
    pub(super) settings_search: Entity<InputState>,
    pub(super) settings_focus: FocusHandle,
    pub(super) settings_navigation: Entity<SettingsNavigation>,
    pub(super) notices: Vec<errors::ErrorRow>,
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
                    .update(cx, |view: &mut Self, cx| {
                        if epoch == view.epoch {
                            cx.emit(PreferencesEvent::Completed {
                                epoch,
                                completion,
                                result,
                            });
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
                    .update(cx, |view: &mut Self, cx| view.poll_account(cx))
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
            metadata: agent_core::presentation::view::Presentation::empty().settings(),
            projects: Vec::new(),
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
            blocked: false,
            settings_page: SettingsPage::General,
            settings_search,
            settings_focus: cx.focus_handle(),
            settings_navigation,
            notices: Vec::new(),
            opened: false,
            _subscriptions: subscriptions,
        }
    }

    pub(super) fn sync(
        &mut self,
        metadata: Arc<SettingsPresentation>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.metadata.connection_epoch != metadata.connection_epoch {
            self.epoch += 1;
            self.account.busy = false;
            self.account.polling = false;
            self.worktrees.saving = false;
            self.worktrees.removing = false;
            self.worktrees.save_pending = false;
        }
        if self.metadata.login.as_ref().map(|login| &login.login_id)
            != metadata.login.as_ref().map(|login| &login.login_id)
        {
            self.account
                .code
                .update(cx, |input, cx| input.set_value("", window, cx));
        }
        if !self.worktrees.dirty
            && !self.worktrees.saving
            && self.metadata.worktree_settings != metadata.worktree_settings
            && let Some(settings) = &metadata.worktree_settings
        {
            self.worktrees.show(settings, window, cx);
        }
        if !self.metadata.equivalent(&metadata) {
            cx.notify();
        }
        self.metadata = metadata;
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
            self.metadata = agent_core::presentation::view::Presentation::empty().settings();
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
    fn poll_account(&mut self, cx: &mut Context<Self>) {
        if self.account.polling
            && !self.account.busy
            && self.metadata.connected()
            && let Some(login) = &self.metadata.login
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
            cx.notify();
        }
    }
    pub(super) fn completed(
        &mut self,
        epoch: u64,
        completion: Completion,
        result: Result<Outcome, String>,
        cx: &mut Context<Self>,
    ) {
        if self.epoch != epoch {
            return;
        }
        match completion {
            Completion::Account => {
                self.account.busy = false;
                self.account.polling = result.is_ok() && self.metadata.login.is_some();
                if result.is_err() {
                    self.dispatch(Intent::ListAccounts(op::ListAccounts {}));
                }
                if self.metadata.login.is_none() {
                    self.account.login_draft = None;
                }
            }
            Completion::WorktreeSettings => {
                self.worktrees.saving = false;
                self.worktrees.dirty = !self
                    .worktrees
                    .matches(self.metadata.worktree_settings.as_deref(), cx);
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
        self.metadata.connected()
            && !self.account.busy
            && !self.blocked
            && self.metadata.login.is_none()
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
            .matches(self.metadata.worktree_settings.as_deref(), cx);
        self.worktrees.saved = false;
    }
    pub(super) fn save_worktree_settings(
        &mut self,
        toggle: Option<WorktreeToggle>,
        cx: &Context<Self>,
    ) {
        let Some(settings) = &self.metadata.worktree_settings else {
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
        if self.metadata.worktree_settings.as_deref() == Some(&settings) {
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
        cx.emit(PreferencesEvent::Notice(action));
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
        let renders = desktop.read_with(cx, |view, cx| {
            view.composer_view.read(cx).content.read(cx).renders
        });
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
            desktop.read_with(cx, |view, cx| view
                .composer_view
                .read(cx)
                .content
                .read(cx)
                .renders),
            renders,
            "settings text must not redraw the composer"
        );
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
        let mut publication =
            agent_core::presentation::view::Presentation::project(Arc::default(), None);
        window.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.sync(publication.settings.clone(), window, cx)
            })
        });
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
                publication = agent_core::presentation::view::Presentation::project(
                    Arc::new(snapshot.clone()),
                    Some(&publication),
                );
                view.update(cx, |view, cx| {
                    view.sync(publication.settings.clone(), window, cx)
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
            publication = agent_core::presentation::view::Presentation::project(
                Arc::new(snapshot),
                Some(&publication),
            );
            view.update(cx, |view, cx| {
                view.sync(publication.settings.clone(), window, cx)
            });
        });
        window.run_until_parked();
        assert!(
            notifications.get() > 0,
            "catalog changes must still reach open controls"
        );
    }

    #[gpui::test]
    fn reconnect_preserves_edits_and_rejects_old_completions(cx: &mut TestAppContext) {
        let _runtime = runtime(cx);
        let (view, window) =
            cx.add_window_view(|window, cx| Preferences::new(None, None, window, cx));
        window.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.worktrees
                    .directory
                    .update(cx, |input, cx| input.set_value("/my/edits", window, cx));
                view.worktrees.dirty = true;
                view.account.busy = true;
                view.account.polling = true;
                view.worktrees.saving = true;
                view.worktrees.removing = true;
                view.worktrees.save_pending = true;
                let old_epoch = view.epoch;
                let mut snapshot = Snapshot::default();
                snapshot.connection.epoch = 1;
                Arc::make_mut(&mut snapshot.files.workspace).settings = Some(Arc::default());
                view.sync(
                    SettingsPresentation::from_snapshot(Arc::new(snapshot)),
                    window,
                    cx,
                );
                assert!(!view.account.busy && !view.account.polling);
                assert!(!view.worktrees.saving && !view.worktrees.removing);
                assert!(!view.worktrees.save_pending);
                assert!(view.worktrees.dirty);
                assert_eq!(
                    view.worktrees.directory.read(cx).value().as_str(),
                    "/my/edits"
                );
                view.account.busy = true;
                view.worktrees.saving = true;
                view.completed(old_epoch, Completion::Account, Ok(Outcome::Applied), cx);
                view.completed(
                    old_epoch,
                    Completion::WorktreeSettings,
                    Ok(Outcome::Applied),
                    cx,
                );
                assert!(view.account.busy && view.worktrees.saving);
            });
        });
    }

    #[gpui::test]
    fn completion_adopts_settings_before_the_next_display_frame(cx: &mut TestAppContext) {
        let runtime = runtime(cx);
        let settings = Arc::new(agent_protocol::models::WorktreeSettings {
            worktree_directory: "/saved".into(),
            ..Default::default()
        });
        let mut snapshot = Snapshot::default();
        Arc::make_mut(&mut snapshot.files.workspace).settings = Some(settings.clone());
        let store =
            runtime.block_on(async { Arc::new(agent_core::store::Store::offline(snapshot, None)) });
        let (owner, window) = cx.add_window_view(|window, cx| {
            Desktop::new(
                Mode::Fixture {
                    remote: None,
                    cwd: "/fixture".into(),
                },
                window,
                cx,
            )
        });
        let preferences = owner.read_with(window, |owner, _| owner.preferences.clone());
        window.update(|window, cx| {
            owner.update(cx, |owner, cx| {
                let session = StoreSession::new(store.clone(), cx.global::<Runtime>().clone());
                preferences.update(cx, |view, cx| {
                    view.bind(Some(session.pane.clone()), None, window, cx);
                    let mut stale = store.snapshot().as_ref().clone();
                    Arc::make_mut(&mut stale.files.workspace).settings = Some(Arc::default());
                    view.sync(
                        SettingsPresentation::from_snapshot(Arc::new(stale)),
                        window,
                        cx,
                    );
                    view.worktrees.show(&settings, window, cx);
                    view.worktrees.dirty = true;
                    view.worktrees.saving = true;
                    view.updates
                        .try_send((
                            view.epoch,
                            Completion::WorktreeSettings,
                            Ok(Outcome::Applied),
                        ))
                        .unwrap();
                });
                owner.session = Some(session);
            });
        });
        window.run_until_parked();
        preferences.read_with(window, |view, _| {
            assert_eq!(view.metadata.worktree_settings.as_ref(), Some(&settings));
            assert!(!view.worktrees.saving && !view.worktrees.dirty);
            assert!(view.worktrees.saved);
        });
    }

    #[gpui::test]
    fn replacing_a_pane_discards_credentials_and_old_completions_but_allows_retry(
        cx: &mut TestAppContext,
    ) {
        let runtime = runtime(cx);
        let stores: Vec<_> = (0..2)
            .map(|_| {
                runtime.block_on(async {
                    Arc::new(agent_core::store::Store::offline(Snapshot::default(), None))
                })
            })
            .collect();
        let (owner, window) = cx.add_window_view(|window, cx| {
            Desktop::new(
                Mode::Fixture {
                    remote: None,
                    cwd: "/fixture".into(),
                },
                window,
                cx,
            )
        });
        let view = owner.read_with(window, |owner, _| owner.preferences.clone());
        let mut old_epoch = 0;
        window.update(|window, cx| {
            owner.update(cx, |owner, cx| {
                owner.session = Some(StoreSession::new(
                    stores[0].clone(),
                    cx.global::<Runtime>().clone(),
                ));
                owner.accept_snapshot(window, cx);
            });
            view.update(cx, |view, cx| {
                view.account
                    .code
                    .update(cx, |input, cx| input.set_value("previous-key", window, cx));
                old_epoch = view.epoch;
            });
            owner.update(cx, |owner, cx| {
                owner.session = Some(StoreSession::new(
                    stores[1].clone(),
                    cx.global::<Runtime>().clone(),
                ));
                owner.accept_snapshot(window, cx);
            });
            view.update(cx, |view, cx| {
                assert!(view.account.code.read(cx).value().is_empty());
                view.account.busy = true;
                view.updates
                    .try_send((old_epoch, Completion::Account, Err("old".into())))
                    .unwrap();
            });
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
            });
        });
        window.run_until_parked();
        window.update(|window, cx| {
            view.update(cx, |view, cx| {
                assert!(!view.account.busy);
                assert!(!view.account.polling);
                let mut snapshot = Snapshot::default();
                snapshot.connection.connected = true;
                view.sync(
                    SettingsPresentation::from_snapshot(Arc::new(snapshot)),
                    window,
                    cx,
                );
                view.account_operation(Intent::StartAccountLogin(op::StartAccountLogin {
                    provider: agent_protocol::session::ProviderKind::Codex,
                }));
                assert!(
                    view.account.busy,
                    "a current connection can retry after failure"
                );
            });
        });
    }
}
