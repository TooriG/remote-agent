use super::model_settings::provider_icon;
use super::sidebar::{SidebarEntry, SidebarRow};
use super::*;
use agent_protocol::session::ProviderKind;

impl Preferences {
    fn provider_settings(&self, cx: &Context<Self>) -> AnyElement {
        let language = crate::localization::language(cx);
        let selected = self.account_provider();
        let disabled = !self.snapshot.connection.connected
            || self.account.busy
            || self.snapshot.catalog.account.login.is_some();
        let mut providers = settings_group()
            .w(px(250.))
            .flex_shrink_0()
            .overflow_hidden();
        for provider in self.snapshot.connection_setup().agents {
            let id = provider.provider;
            let name = provider.name.clone();
            providers = providers.child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .p_3()
                    .when(selected == Some(id), |row| {
                        row.bg(rgb(appearance::SELECTED))
                    })
                    .border_b_1()
                    .border_color(rgb(appearance::BORDER))
                    .child(
                        Button::new(format!("provider-{}", id.key()))
                            .ghost()
                            .flex_1()
                            .min_w_0()
                            .h_auto()
                            .child(
                                h_flex()
                                    .w_full()
                                    .items_center()
                                    .gap_2()
                                    .child(provider_icon(id))
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .min_w_0()
                                            .gap_1()
                                            .child(div().text_sm().font_semibold().child(name))
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(rgb(appearance::MUTED))
                                                    .child(
                                                        language.text(&provider.label).to_owned(),
                                                    ),
                                            ),
                                    ),
                            )
                            .disabled(disabled)
                            .on_click(cx.listener(move |view, _, window, cx| {
                                view.model_controls.provider = Some(id);
                                view.account.sign_out = None;
                                view.account
                                    .code
                                    .update(cx, |input, cx| input.set_value("", window, cx));
                                if id == ProviderKind::DeepSeek {
                                    view.dispatch(Intent::ReadDshCredentials(
                                        op::ReadDshCredentials {},
                                    ));
                                }
                                if id == ProviderKind::Codex {
                                    view.dispatch(Intent::ManageCodexCli(op::ManageCodexCli {
                                        update: false,
                                    }));
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        switch::Switch::new(format!("provider-enabled-{}", id.key()))
                            .accessibility_label(format!(
                                "{} {}",
                                provider.name,
                                language.text("プロバイダー")
                            ))
                            .checked(provider.enabled)
                            .disabled(
                                disabled
                                    || self.snapshot.catalog.provider_settings.is_none()
                                    || self
                                        .snapshot
                                        .operation_running(op::OperationKey::ProviderSettings),
                            )
                            .on_click(cx.listener(move |view, enabled, _, cx| {
                                view.dispatch(Intent::SetProviderEnabled(op::SetProviderEnabled {
                                    provider: id,
                                    enabled: *enabled,
                                }));
                                cx.notify();
                            })),
                    ),
            );
        }
        v_flex()
            .gap_4()
            .child(
                h_flex().justify_end().child(
                    button(
                        "providers-refresh",
                        language.text("状態を確認"),
                        cx,
                        |view, _, _| {
                            view.dispatch(Intent::ReadProviderSettings(
                                op::ReadProviderSettings {},
                            ));
                        },
                    )
                    .icon(IconName::Redo)
                    .ghost()
                    .disabled(!self.snapshot.connection.connected),
                ),
            )
            .child(
                h_flex()
                    .items_start()
                    .gap_6()
                    .child(providers)
                    .child(div().flex_1().min_w_0().child(self.account_controls(cx))),
            )
            .into_any_element()
    }

    pub(super) fn close_settings(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.snapshot.catalog.account.login.is_some() || self.account.busy {
            return;
        }
        self.opened = false;
        cx.emit(super::super::preferences::PreferencesEvent::Closed);
        cx.notify();
    }

    pub(in crate::app) fn settings_modal(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("settings-modal-backdrop")
            .debug_selector(|| "settings-modal-backdrop".into())
            .absolute()
            .inset_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .p_10()
            .bg(rgba(0x000000b3))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _, window, cx| view.close_settings(window, cx)),
            )
            .child(
                h_flex()
                    .id("settings-modal")
                    .debug_selector(|| "settings-modal".into())
                    .track_focus(&self.settings_focus)
                    .on_key_down(cx.listener(|view, event: &KeyDownEvent, window, cx| {
                        if event.keystroke.key == "escape" {
                            view.close_settings(window, cx);
                            cx.stop_propagation();
                        }
                    }))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .size_full()
                    .max_w(px(1280.))
                    .max_h(px(960.))
                    .rounded(px(16.))
                    .border_1()
                    .border_color(rgb(appearance::BORDER))
                    .shadow_lg()
                    .overflow_hidden()
                    .bg(rgb(appearance::BACKGROUND))
                    .child(
                        self.settings_navigation.clone().cached(
                            StyleRefinement::default()
                                .w(px(240.))
                                .h_full()
                                .flex_shrink_0(),
                        ),
                    )
                    .child(self.settings(cx)),
            )
            .into_any_element()
    }

    pub(in crate::app) fn open_settings(
        &mut self,
        page: SettingsPage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.opened = true;
        self.settings_page = page;
        let query = self.settings_search.read(cx).value().to_string();
        self.settings_navigation
            .update(cx, |view, cx| view.sync(page, query, cx));
        self.settings_focus.focus(window, cx);
        match page {
            SettingsPage::General | SettingsPage::Connections => {}
            SettingsPage::Providers => {
                self.model_controls.provider =
                    self.account_provider().or(Some(ProviderKind::Codex));
                self.account.sign_out = None;
                self.dispatch(Intent::ReadProviderSettings(op::ReadProviderSettings {}));
                self.dispatch(Intent::ManageCodexCli(op::ManageCodexCli { update: false }));
            }
            SettingsPage::Worktrees => {
                self.worktrees.removal = None;
                self.dispatch(Intent::ReadWorktreeSettings(op::ReadWorktreeSettings {}));
                self.dispatch(Intent::ListWorktrees(op::ListWorktrees {}));
            }
        }
    }

    pub(super) fn settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let language = crate::localization::language(cx);
        let (title, subtitle) = match self.settings_page {
            SettingsPage::General => (
                language.text("一般"),
                language.text("この端末で使用するアプリの設定です。"),
            ),
            SettingsPage::Providers => (
                language.text("プロバイダー"),
                language.text("選択した環境のプロバイダー・アカウント・更新を管理します。"),
            ),
            SettingsPage::Connections => (
                language.text("環境"),
                language.text("作業する環境と、このPCへ接続する端末を管理します。"),
            ),
            SettingsPage::Worktrees => (
                language.text("ワークツリー"),
                language.text("新しい会話の作業場所と、作成済みのワークツリーを管理します。"),
            ),
        };
        let body = match self.settings_page {
            SettingsPage::General => self.general_settings(cx),
            SettingsPage::Providers => self.provider_settings(cx),
            SettingsPage::Connections => self.connection_settings(cx),
            SettingsPage::Worktrees => self.worktree_settings(cx),
        };
        let errors = self.settings_errors();
        let header = if self.settings_page == SettingsPage::General {
            h_flex()
                .debug_selector(|| "settings-general-breadcrumb".into())
                .gap_3()
                .child(
                    div()
                        .text_color(rgb(appearance::MUTED))
                        .child(language.text("設定")),
                )
                .child(div().text_color(rgb(appearance::MUTED)).child("/"))
                .child(title)
                .into_any_element()
        } else {
            self.settings_scope(cx)
        };
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(
                h_flex()
                    .h(px(crate::WINDOW_HEADER_HEIGHT))
                    .flex_shrink_0()
                    .px_5()
                    .justify_between()
                    .text_xs()
                    .child(header)
                    .child(
                        icon_button(
                            "settings-close",
                            IconName::Close,
                            language.text("閉じる"),
                            cx,
                            |view, window, cx| view.close_settings(window, cx),
                        )
                        .debug_selector(|| "settings-close".into())
                        .disabled(
                            self.snapshot.catalog.account.login.is_some() || self.account.busy,
                        ),
                    )
                    .border_b_1()
                    .border_color(rgb(appearance::BORDER)),
            )
            .child(
                div()
                    .id(match self.settings_page {
                        SettingsPage::General => "settings-general-scroll",
                        SettingsPage::Providers => "settings-providers-scroll",
                        SettingsPage::Connections => "settings-connections-scroll",
                        SettingsPage::Worktrees => "settings-worktrees-scroll",
                    })
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        v_flex()
                            .w_full()
                            .max_w(px(928.))
                            .p_8()
                            .gap_8()
                            .child(
                                v_flex()
                                    .debug_selector(|| "settings-page-heading".into())
                                    .gap_2()
                                    .child(div().text_size(px(26.)).font_semibold().child(title))
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(rgb(appearance::MUTED))
                                            .child(subtitle),
                                    ),
                            )
                            .when(!errors.is_empty(), |body| {
                                body.child(errors::error_list(
                                    &errors,
                                    ErrorScope::Settings,
                                    None,
                                    cx,
                                    Self::notice_action,
                                ))
                            })
                            .child(body),
                    ),
            )
            .into_any_element()
    }

    fn settings_scope(&self, cx: &Context<Self>) -> AnyElement {
        let language = crate::localization::language(cx);
        let environment = if self.settings_page == SettingsPage::Connections {
            div().child(language.text("このPC")).into_any_element()
        } else if let Some(hosts) = &self.hosts {
            Hosts::menu(
                hosts,
                "settings-scope-environment",
                self.remote.as_ref().map(|host| host.id.as_str()),
                self.remote
                    .as_ref()
                    .map_or("Local", |host| host.name.as_str()),
                self.busy > 0
                    || self.worktrees.dirty
                    || self.worktrees.saving
                    || self.snapshot.catalog.account.login.is_some(),
                cx,
            )
            .into_any_element()
        } else {
            div().child(language.text("このPC")).into_any_element()
        };
        h_flex()
            .debug_selector(|| "settings-scope".into())
            .w_full()
            .min_w_0()
            .gap_2()
            .child(
                div()
                    .text_color(rgb(appearance::MUTED))
                    .child(language.text("設定の適用先")),
            )
            .child(environment)
            .child(div().text_color(rgb(appearance::MUTED)).child("/"))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_ellipsis()
                    .child(language.text("すべてのプロジェクト")),
            )
            .child(
                div()
                    .text_color(rgb(appearance::MUTED))
                    .child(language.text(if self.snapshot.connection.connected {
                        "接続済み"
                    } else {
                        "未接続"
                    })),
            )
            .into_any_element()
    }

    fn general_settings(&self, cx: &Context<Self>) -> AnyElement {
        let language = crate::localization::language(cx);
        v_flex()
            .gap_4()
            .child(
                settings_group()
                    .p_5()
                    .child(language_settings(cx, |view, error| {
                        view.local_errors.insert(ErrorScope::Settings, error);
                    })),
            )
            .when_some(self.app_updates.as_ref(), |body, updates| {
                body.child(
                    settings_group()
                        .p_5()
                        .child(crate::updates::Updates::settings(updates, cx)),
                )
            })
            .child(
                h_flex()
                    .gap_2()
                    .text_xs()
                    .text_color(rgb(appearance::MUTED))
                    .child(Icon::default().path("icons/monitor.svg").size(px(14.)))
                    .child(language.text("この端末に保存されます。")),
            )
            .into_any_element()
    }

    fn connection_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let language = crate::localization::language(cx);
        let setup = self.snapshot.connection_setup();
        let agent_controls = self.connection_agent_controls(&setup.agents, cx);
        let header = h_flex()
            .flex_wrap()
            .items_center()
            .gap_4()
            .child(
                section_heading(
                    language.text("作業するコンピューター"),
                    language.text("このPCや、自分のクラウドVMを選びます。"),
                )
                .flex_1(),
            )
            .child(
                button(
                    "settings-import-projects",
                    language.text("既存のプロジェクトを取り込む"),
                    cx,
                    |_, _, cx| cx.emit(super::super::preferences::PreferencesEvent::ImportProjects),
                )
                .secondary()
                .outline()
                .h(px(32.))
                .debug_selector(|| "settings-import-projects".into())
                .disabled(!self.snapshot.connection.connected),
            );
        v_flex()
            .gap_8()
            .child(header)
            .children(self.hosts.as_ref().map(|hosts| {
                hosts.update(cx, |view, cx| {
                    v_flex()
                        .gap_8()
                        .child(view.connection_choices(
                            ConnectionLayout::Settings,
                            self.remote.as_ref().map(|host| host.id.as_str()),
                            self.snapshot.connection.connected,
                            self.snapshot.connection_enabled(),
                            agent_controls,
                            cx,
                        ))
                        .child(hosts.clone())
                })
            }))
            .into_any_element()
    }

    fn worktree_settings(&self, cx: &Context<Self>) -> AnyElement {
        let language = crate::localization::language(cx);
        let disabled = !self.snapshot.connection.connected
            || self.snapshot.files.workspace.settings.is_none()
            || self.busy > 0;
        let settings = self.snapshot.files.workspace.settings.as_ref();
        let body = v_flex()
            .gap_8()
            .child(settings_group().p_5().child(self.worktree_scope(cx)));
        if let Some(project) = self.worktrees.scope.clone() {
            return body
                .child(
                    settings_group()
                        .p_5()
                        .child(self.project_worktree_cleanup(project, disabled, cx)),
                )
                .into_any_element();
        }
        body
            .child(settings_group().p_5().gap_5()
                .child(div().text_lg().font_semibold().child(language.text("新しい会話の作業場所")))
                .child(h_flex().gap_5().items_center()
                    .child(v_flex().flex_1().min_w_0().gap_1()
                        .child(language.text("ワークツリーで開始"))
                        .child(div().text_sm().text_color(rgb(appearance::MUTED)).child(language.text("元の作業フォルダと分けて、新しい会話を進めます。"))))
                    .child(switch::Switch::new("worktree-create")
                        .accessibility_label(language.text("新規セッションをワークツリーで開始"))
                        .checked(settings.is_some_and(|settings| settings.create_on_new_session))
                        .disabled(disabled || self.worktrees.saving)
                        .on_click(cx.listener(|s, checked, _, cx| {
                            s.save_worktree_settings(Some(WorktreeToggle::Create(*checked)), cx);
                            cx.notify();
                        }))))
                .child(v_flex().gap_2()
                    .child(div().text_sm().child(language.text("保存先")))
                    .child(Input::new(&self.worktrees.directory).aria_label(language.text("ワークツリーの保存先")).disabled(disabled))
                    .child(div().text_xs().text_color(rgb(appearance::MUTED)).child(language.text("空欄の場合は、元のリポジトリ内の .worktree に保存します。指定先には「セッション名/リポジトリ名」の構成で作ります。")))))
            .child(settings_group().p_5().gap_5()
                .child(h_flex().gap_5().items_center()
                    .child(section_heading(language.text("ファイルの引き継ぎ"), language.text("作成時に、指定したファイルを元のリポジトリからコピーします。"))
                        .flex_1().min_w_0())
                    .child(switch::Switch::new("worktree-copy")
                        .accessibility_label(language.text("ワークツリー作成時にファイルをコピー"))
                        .checked(settings.is_some_and(|settings| settings.copy_on_create))
                        .disabled(disabled || self.worktrees.saving)
                        .on_click(cx.listener(|s, checked, _, cx| {
                            s.save_worktree_settings(Some(WorktreeToggle::Copy(*checked)), cx);
                            cx.notify();
                        }))))
                .child(v_flex().gap_2()
                    .child(div().text_sm().child(language.text("コピーするファイル")))
                    .child(Textarea::new(&self.worktrees.copy_paths).aria_label(language.text("コピー対象")).readonly(disabled))
                    .child(div().text_xs().text_color(rgb(appearance::MUTED)).child(language.text("リポジトリからの相対パスを1行に1つ入力します。例: .env.local、config/local。存在しないパスはスキップし、シンボリックリンクはコピーしません。"))))
                .child(div().text_xs().text_color(rgb(appearance::MUTED)).child(language.text("最初のメッセージ送信時に作成・コピーします。指定ファイルはコピー元の内容で置き換え、既存セッションでは再実行しません。"))))
            .child(settings_group().p_5().child(h_flex().gap_5().items_center()
                .child(section_heading(language.text("取り込み済みを自動削除"), language.text("既定ブランチより先のコミットがない作業場所を1時間ごとに削除します。実行中・回答待ち・ターミナル使用中・ローカル変更ありの場合は保留します。ブランチと会話履歴は残ります。"))
                    .flex_1().min_w_0())
                .child(switch::Switch::new("worktree-delete-unchanged")
                    .accessibility_label(language.text("取り込み済みワークツリーを自動削除"))
                    .checked(settings.is_some_and(|settings| settings.delete_unchanged))
                    .disabled(disabled || self.worktrees.saving)
                    .on_click(cx.listener(|s, checked, _, cx| {
                        s.save_worktree_settings(Some(WorktreeToggle::DeleteUnchanged(*checked)), cx);
                        cx.notify();
                    })))))
            .child(div().text_xs().text_color(rgb(appearance::MUTED)).child(if self.worktrees.saved { language.text("保存しました") } else { language.text("変更は自動で保存されます。既存のワークツリーは移動しません。") }))
            .child(self.managed_worktrees(cx))
            .into_any_element()
    }

    /// Only cleanup varies by project; creation, copying and storage are shared.
    fn worktree_scope(&self, cx: &Context<Self>) -> Div {
        let language = crate::localization::language(cx);
        let scope = self.worktrees.scope.as_deref();
        h_flex()
            .gap_2()
            .flex_wrap()
            .items_center()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(language.text("適用範囲"))
                    .child(div().text_xs().text_color(rgb(appearance::MUTED)).child(
                        language.text("プロジェクトごとに変えられるのは自動削除だけです。"),
                    )),
            )
            .child(
                button(
                    "worktree-scope-all",
                    language.text("すべてのプロジェクト"),
                    cx,
                    |view, _, cx| {
                        view.worktrees.scope = None;
                        cx.notify();
                    },
                )
                .selected(scope.is_none()),
            )
            .children(self.projects.iter().enumerate().map(|(index, (id, name))| {
                let selected = scope == Some(id.as_str());
                let id = id.clone();
                button(
                    format!("worktree-scope-{index}"),
                    name.clone(),
                    cx,
                    move |view, _, cx| {
                        view.worktrees.scope = Some(id.clone());
                        cx.notify();
                    },
                )
                .selected(selected)
            }))
    }

    fn project_worktree_cleanup(&self, project: String, disabled: bool, cx: &Context<Self>) -> Div {
        let language = crate::localization::language(cx);
        let settings = self.snapshot.files.workspace.settings.as_deref();
        let current =
            settings.and_then(|settings| settings.project_delete_unchanged.get(&project).copied());
        let shared = settings.is_some_and(|settings| settings.delete_unchanged);
        h_flex()
            .gap_2()
            .flex_wrap()
            .items_center()
            .child(
                section_heading(
                    language.text("取り込み済みを自動削除"),
                    language.text("このプロジェクトの作業場所に適用します。"),
                )
                .flex_1()
                .min_w_0(),
            )
            .children(
                [
                    (
                        None,
                        if shared {
                            language.text("すべてのプロジェクトと同じ（オン）")
                        } else {
                            language.text("すべてのプロジェクトと同じ（オフ）")
                        },
                    ),
                    (Some(true), language.text("オン")),
                    (Some(false), language.text("オフ")),
                ]
                .into_iter()
                .enumerate()
                .map(|(index, (value, label))| {
                    let project = project.clone();
                    button(
                        format!("worktree-project-cleanup-{index}"),
                        label,
                        cx,
                        move |view, _, cx| {
                            view.save_worktree_settings(
                                Some(WorktreeToggle::ProjectDeleteUnchanged(
                                    project.clone(),
                                    value,
                                )),
                                cx,
                            );
                            cx.notify();
                        },
                    )
                    .selected(current == value)
                    .disabled(disabled || self.worktrees.saving)
                }),
            )
    }

    // Mirrors T3's thread details "Open in <editor>" split row: the primary half
    // opens the preferred editor, and choosing a menu entry opens that editor and
    // makes it preferred.
}

impl Preferences {
    fn managed_worktrees(&self, cx: &Context<Self>) -> AnyElement {
        let language = crate::localization::language(cx);
        let mut body = v_flex().gap_3().child(
            h_flex()
                .justify_between()
                .child(
                    div()
                        .text_lg()
                        .font_semibold()
                        .child(language.text("作成済みのワークツリー")),
                )
                .child(
                    icon_button(
                        "refresh-worktrees",
                        IconName::RotateCw,
                        language.text("ワークツリー一覧を更新"),
                        cx,
                        |s, _, _| {
                            s.dispatch(Intent::ListWorktrees(op::ListWorktrees {}));
                        },
                    )
                    .disabled(!self.snapshot.connection.connected || self.worktrees.removing),
                ),
        );
        let Some(worktrees) = &self.snapshot.files.workspace.worktrees else {
            return body
                .child(language.text("一覧を読み込み中…"))
                .into_any_element();
        };
        if worktrees.is_empty() {
            body = body.child(
                settings_group()
                    .p_5()
                    .text_sm()
                    .text_color(rgb(appearance::MUTED))
                    .child(language.text("Bexで作成したワークツリーはありません。")),
            );
        }
        for (index, worktree) in worktrees.iter().enumerate() {
            let path = worktree.path.clone();
            let current = Path::new(&self.snapshot.pane.navigation.cwd).starts_with(&worktree.path);
            let reason = worktree
                .blocked_reason
                .as_deref()
                .or(current.then_some(language.text(
                    "現在開いている会話の作業場所です。別の会話に移動してから削除してください。",
                )));
            let confirming = self.worktrees.removal.as_deref() == Some(path.as_str());
            let mut entry = settings_group()
                .p_5()
                .gap_3()
                .child(
                    h_flex()
                        .justify_between()
                        .gap_3()
                        .child(div().flex_1().min_w_0().child(format!(
                            "{} · {}",
                            file_name(&worktree.project_path),
                            worktree.branch
                        )))
                        .child(
                            button(
                                format!("remove-worktree-{index}"),
                                language.text("削除…"),
                                cx,
                                move |s, _, _| {
                                    s.worktrees.removal = Some(path.clone());
                                },
                            )
                            .secondary()
                            .outline()
                            .disabled(
                                reason.is_some()
                                    || !self.snapshot.connection.connected
                                    || self.worktrees.removing,
                            ),
                        ),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(appearance::MUTED))
                        .child(worktree.path.clone()),
                );
            for (thread_index, thread) in worktree.threads.iter().enumerate() {
                let id = thread.id.clone();
                entry = entry.child(
                    button(
                        format!("worktree-thread-{index}-{thread_index}"),
                        format!(
                            "{}{}",
                            if thread.active {
                                language.text("実行中 · ")
                            } else {
                                ""
                            },
                            thread.name
                        ),
                        cx,
                        move |_, _, cx| {
                            cx.emit(super::super::preferences::PreferencesEvent::OpenChat(
                                id.clone(),
                            ))
                        },
                    )
                    .icon(IconName::FileText)
                    .disabled(!self.snapshot.connection.connected || self.worktrees.removing),
                );
            }
            if let Some(reason) = reason {
                entry = entry.child(
                    div()
                        .text_sm()
                        .text_color(rgb(appearance::MUTED))
                        .child(reason.to_owned()),
                );
            }
            if confirming {
                let path = worktree.path.clone();
                entry = entry.child(div().text_sm().child(language.text("この作業ディレクトリを削除します。ブランチと会話履歴は残ります。次のメッセージ送信時に、元プロジェクトの main から同じ場所に作り直します。")))
                    .child(h_flex().gap_2()
                        .child(button(format!("cancel-remove-worktree-{index}"), language.text("取消"), cx, |s, _, _| s.worktrees.removal = None).disabled(self.worktrees.removing))
                        .child(button(format!("confirm-remove-worktree-{index}"), language.text("ワークツリーを削除"), cx, move |s, _, _| s.remove_worktree(path.clone()))
                            .disabled(reason.is_some() || !self.snapshot.connection.connected || self.worktrees.removing)));
            }
            body = body.child(entry);
        }
        body.into_any_element()
    }

    fn remove_worktree(&mut self, path: String) {
        if self.worktrees.removing || !self.snapshot.connection.connected {
            return;
        }
        self.worktrees.removing = true;
        self.perform(
            Intent::RemoveWorktree(op::RemoveWorktree { path }),
            super::super::preferences::Completion::RemoveWorktree,
        );
    }
}

pub(in crate::app) struct SettingsNavigation {
    current: SettingsPage,
    query: String,
    search: Entity<InputState>,
}
impl EventEmitter<SettingsPage> for SettingsNavigation {}
impl SettingsNavigation {
    pub(in crate::app) fn new(search: Entity<InputState>) -> Self {
        Self {
            current: SettingsPage::General,
            query: String::new(),
            search,
        }
    }
    pub(in crate::app) fn sync(
        &mut self,
        current: SettingsPage,
        query: String,
        cx: &mut Context<Self>,
    ) {
        if self.current != current || self.query != query {
            self.current = current;
            self.query = query;
            cx.notify();
        }
    }
}
impl Render for SettingsNavigation {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let language = crate::localization::language(cx);
        let mut navigation: Vec<SidebarRow> = Vec::new();
        let query = self.query.trim().to_lowercase();
        for (label, icon, page) in [
            (
                language.text("一般"),
                IconName::Settings,
                SettingsPage::General,
            ),
            (
                language.text("プロバイダー"),
                IconName::User,
                SettingsPage::Providers,
            ),
            (
                language.text("環境"),
                IconName::Network,
                SettingsPage::Connections,
            ),
            (
                language.text("ワークツリー"),
                IconName::Folder,
                SettingsPage::Worktrees,
            ),
        ] {
            if !query.is_empty() && !label.to_lowercase().contains(&query) {
                continue;
            }
            navigation.push(
                SidebarMenuItem::new(label)
                    .icon(icon)
                    .active(self.current == page)
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(page);
                    }))
                    .into(),
            );
        }
        Sidebar::new("settings-sidebar")
            .header(
                div().p_3().child(
                    Input::new(&self.search)
                        .aria_label(language.text("設定を検索"))
                        .prefix(Icon::new(IconName::Search).size(px(14.))),
                ),
            )
            .child(SidebarEntry::from(SidebarRow::Section {
                label: language.text("設定"),
                add_project: None,
            }))
            .children(navigation.into_iter().map(SidebarEntry::from))
    }
}

impl Desktop {
    pub(in crate::app) fn open_settings(
        &mut self,
        page: SettingsPage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.tab = Tab::Settings;
        self.publish_preferences(window, cx);
        self.preferences
            .update(cx, |view, cx| view.open_settings(page, window, cx));
    }
}

pub(super) fn language_settings<V: 'static>(cx: &Context<V>, failed: fn(&mut V, String)) -> Div {
    use agent_core::localization::UiLanguage;
    let language = crate::localization::language(cx);
    let owner = cx.entity().downgrade();
    h_flex()
        .gap_4()
        .items_center()
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_1()
                .child(div().font_semibold().child(language.text("表示言語")))
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(appearance::MUTED))
                        .child(language.text("アプリで使用する言語を選択します。")),
                ),
        )
        .child(
            Button::new("settings-language")
                .debug_selector(|| "settings-language".into())
                .accessibility_label(language.text("表示言語"))
                .label(match language {
                    UiLanguage::English => "English",
                    UiLanguage::Japanese => "日本語",
                })
                .icon(IconName::Globe)
                .dropdown_caret(true)
                .secondary()
                .outline()
                .w(px(180.))
                .h(px(36.))
                .dropdown_menu(move |mut menu, _, _| {
                    for (value, label) in [
                        (UiLanguage::Japanese, "日本語"),
                        (UiLanguage::English, "English"),
                    ] {
                        let owner = owner.clone();
                        menu = menu.item(
                            PopupMenuItem::new(label)
                                .checked(value == language)
                                .on_click(move |_, _, cx| {
                                    let _ = owner.update(cx, |view, cx| {
                                        if let Err(error) = crate::localization::select(value, cx) {
                                            failed(view, error);
                                        }
                                        cx.notify();
                                    });
                                }),
                        );
                    }
                    menu
                }),
        )
}
