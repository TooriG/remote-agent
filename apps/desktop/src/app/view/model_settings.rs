use super::*;
use agent_protocol::session::ProviderKind;
use gpui_kit::Rgba;

fn provider_icon(provider: ProviderKind) -> Icon {
    Icon::default().path(match provider {
        ProviderKind::Codex => "icons/openai.svg",
        ProviderKind::Claude => "icons/anthropic.svg",
        ProviderKind::DeepSeek => "icons/deepseek.svg",
    })
}

fn fast_icon(fast: bool) -> Icon {
    Icon::default().path(if fast {
        "icons/bolt-fill.svg"
    } else {
        "icons/bolt.svg"
    })
}

fn account_identity(provider: Option<ProviderKind>, identity: &str) -> Div {
    h_flex()
        .min_w_0()
        .gap_1()
        .when_some(provider, |row, provider| {
            row.child(provider_icon(provider).size(px(12.)))
        })
        .child(div().min_w_0().text_ellipsis().child(identity.to_owned()))
}

impl Desktop {
    pub(super) fn refresh_accounts_and_models(&self) {
        self.dispatch(Intent::ListAccounts(op::ListAccounts {}));
        self.dispatch(Intent::RefreshModels);
        if self.account_provider() == Some(ProviderKind::DeepSeek) {
            self.dispatch(Intent::ReadDshCredentials(op::ReadDshCredentials {}));
        }
    }

    pub(super) fn model_menu(
        label: String,
        render_owner: EntityId,
        cx: &Context<Self>,
    ) -> AnyElement {
        let language = crate::localization::language(cx);
        let entity = cx.entity().downgrade();
        let opening = entity.clone();
        popover::Popover::new("model-controls")
            .bg(rgb(appearance::SURFACE))
            .rounded(px(10.))
            .border_color(rgb(appearance::BORDER))
            // Open toward the conversation. Native terminal/browser views in
            // the right panel sit above GPUI's in-window popup layer.
            .anchor(Anchor::BottomRight)
            .trigger(
                Button::new("model-select")
                    .label(label)
                    .debug_selector(|| "model-select".into())
                    .accessibility_label(language.text("モデルとアカウント"))
                    .tooltip(language.text("モデルとアカウント"))
                    .large()
                    .px_1()
                    .max_w(px(180.))
                    .h(px(44.))
                    .ghost(),
            )
            .on_open_change(move |open, window, cx| {
                if *open {
                    let _ = opening.update(cx, |view, cx| {
                        if view.snapshot.account.login.is_some() {
                            view.model_controls.panel = ModelPanel::Accounts;
                        } else {
                            view.model_controls.provider = None;
                            view.model_controls.panel = ModelPanel::Models;
                        }
                        view.model_controls
                            .search
                            .update(cx, |input, cx| input.set_value("", window, cx));
                        view.account.sign_out = None;
                        view.refresh_accounts_and_models();
                    });
                }
            })
            .content(move |_, window, cx| {
                observe_popup_desktop(&entity, render_owner, window, cx);
                entity
                    .update(cx, |s, cx| {
                        v_flex()
                            .w(px(440.))
                            .child(s.model_panel_content(cx))
                            .into_any_element()
                    })
                    .unwrap_or_else(|_| div().into_any_element())
            })
            .into_any_element()
    }
    fn account_row(
        &self,
        in_settings: bool,
        selected: bool,
        (index, account): (usize, &agent_protocol::operations::Account),
        disabled: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        let language = crate::localization::language(cx);
        let id = account.id.clone();
        let provider = account.provider;
        let logout_id = id.clone();
        let identity = account.email.as_deref().unwrap_or(&account.id);
        let confirming = self.account.sign_out.as_deref() == Some(account.id.as_str());
        let header = h_flex()
            .items_center()
            .gap_3()
            .child(
                Button::new(format!("account-choice-{index}"))
                    .accessibility_label(identity.to_owned())
                    .small()
                    .ghost()
                    .h_auto()
                    .min_h(px(40.))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child(account_identity(Some(account.provider), identity))
                            .when_some(account.plan_type.as_ref(), |body, plan| {
                                body.child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(appearance::MUTED))
                                        .child(plan.clone()),
                                )
                            }),
                    )
                    .on_click(cx.listener(move |s, _, _, cx| {
                        s.account_operation(Intent::SelectAccountForDraft(
                            op::SelectAccountForDraft {
                                provider,
                                id: id.clone(),
                                thread_id: s.draft_key().clone(),
                            },
                        ));
                        cx.notify();
                    }))
                    .debug_selector(move || format!("account-choice-{index}"))
                    .when(selected, |button| {
                        button.child(if in_settings {
                            div()
                                .px_2()
                                .py_1()
                                .rounded(px(4.))
                                .border_1()
                                .border_color(rgb(appearance::BORDER))
                                .text_xs()
                                .child(language.text("使用中"))
                                .into_any_element()
                        } else {
                            Icon::new(IconName::Check).size(px(14.)).into_any_element()
                        })
                    })
                    .flex_1()
                    .min_w_0()
                    .disabled(disabled),
            )
            .when(!confirming, |header| {
                let id = logout_id.clone();
                header.child(
                    button(
                        format!("account-logout-{index}"),
                        language.text("サインアウト"),
                        cx,
                        move |s, _, _| s.account.sign_out = Some(id.clone()),
                    )
                    .debug_selector(move || format!("account-logout-{index}"))
                    .text_color(rgb(appearance::MUTED))
                    .disabled(disabled),
                )
            });
        let mut row = if in_settings {
            settings_group().p_5().gap_4()
        } else {
            v_flex()
                .gap_2()
                .p_3()
                .rounded(px(10.))
                .bg(rgb(appearance::SELECTED))
        };
        row = row.child(header).child(
            v_flex()
                .gap_3()
                .when(in_settings, |body| {
                    body.pt_4()
                        .border_t_1()
                        .border_color(rgb(appearance::BORDER))
                })
                .child(div().font_semibold().child(language.text("使用量")))
                .child(account_usage_view(account.usage.as_ref(), language)),
        );
        if provider == ProviderKind::Codex {
            let manage_usage = button(
                format!("account-usage-manage-{index}"),
                language.text("追加クレジット・上限を管理"),
                cx,
                |s, _, cx| {
                    s.open_browser(s.panel_scope(), "https://chatgpt.com/settings/usage", cx);
                },
            );
            let hint = div().text_xs().text_color(rgb(appearance::MUTED))
                .child(language.text("利用枠は表示時刻に自動リセットされます。上限の変更・追加クレジットは管理画面で行えます。"));
            row = row.child(manage_usage).child(hint);
        }
        if confirming {
            row = row
                .child(div().text_sm().child(
                    language.text("サインアウトしますか？ 再び使うにはサインインが必要です。"),
                ))
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            button(
                                format!("account-logout-confirm-{index}"),
                                language.text("サインアウト"),
                                cx,
                                move |s, _, _| {
                                    s.account.sign_out = None;
                                    s.account_operation(Intent::LogoutAccount(op::LogoutAccount {
                                        provider,
                                        id: logout_id.clone(),
                                    }));
                                },
                            )
                            .disabled(disabled),
                        )
                        .child(
                            button(
                                format!("account-logout-cancel-{index}"),
                                language.text("キャンセル"),
                                cx,
                                |s, _, _| s.account.sign_out = None,
                            )
                            .debug_selector(move || format!("account-logout-cancel-{index}")),
                        ),
                );
        }
        row.into_any_element()
    }
    pub(super) fn account_controls(&self, cx: &Context<Self>) -> AnyElement {
        let language = crate::localization::language(cx);
        let in_settings = self.tab == Tab::Settings;
        let accounts = self.snapshot.account.accounts.as_ref();
        let provider = self.account_provider();
        let disabled = self.account.busy
            || self.busy > 0
            || self.snapshot.account.login.is_some()
            || !self.snapshot.connected;
        let mut services = h_flex().gap_1().when(in_settings, |row| {
            row.w_full()
                .border_b_1()
                .border_color(rgb(appearance::BORDER))
        });
        for (value, label) in [
            (ProviderKind::Codex, "Codex"),
            (ProviderKind::Claude, "Claude"),
            (ProviderKind::DeepSeek, "DeepSeek"),
        ] {
            services = services.child(
                button(
                    format!("model-provider-{label}"),
                    if value == ProviderKind::DeepSeek {
                        "DSH"
                    } else {
                        label
                    },
                    cx,
                    move |s, window, cx| {
                        if s.model_controls.provider != Some(value) {
                            s.account
                                .code
                                .update(cx, |input, cx| input.set_value("", window, cx));
                        }
                        s.model_controls.provider = Some(value);
                        s.account.sign_out = None;
                        if value == ProviderKind::DeepSeek {
                            s.dispatch(Intent::ReadDshCredentials(op::ReadDshCredentials {}));
                        }
                    },
                )
                .debug_selector(move || format!("model-provider-{label}"))
                .selected(!in_settings && provider == Some(value))
                .when(in_settings, |button| {
                    button
                        .icon(provider_icon(value))
                        .w(px(120.))
                        .h(px(40.))
                        .rounded(px(0.))
                        .border_b_2()
                        .border_color(if provider == Some(value) {
                            rgb(appearance::FOREGROUND)
                        } else {
                            rgb(appearance::BORDER).opacity(0.)
                        })
                        .text_color(if provider == Some(value) {
                            rgb(appearance::FOREGROUND)
                        } else {
                            rgb(appearance::MUTED)
                        })
                })
                .disabled(disabled)
                .when(!in_settings, |button| button.flex_1()),
            );
        }
        let mut body = v_flex()
            .id("model-account-controls")
            .gap_3()
            .when(in_settings, |body| body.gap_6())
            .child(services);
        let Some(provider) = provider else {
            return body
                .child(language.text("エージェントを選択してください。"))
                .into_any_element();
        };
        if provider == ProviderKind::DeepSeek {
            let credentials = self.credential_controls(cx);
            return body
                .child(if in_settings {
                    settings_group().p_5().child(credentials).into_any_element()
                } else {
                    credentials
                })
                .into_any_element();
        }
        if self.snapshot.account.login.is_some() {
            body = body.child(self.account_login_controls(None, cx));
        } else {
            body = body.child(
                h_flex()
                    .items_center()
                    .flex_wrap()
                    .gap_3()
                    .when(in_settings, |header| {
                        header.child(
                            div()
                                .flex_1()
                                .text_lg()
                                .font_semibold()
                                .child(language.text("アカウント")),
                        )
                    })
                    .child(
                        button(
                            "account-start-login",
                            language.text("アカウントを追加"),
                            cx,
                            move |s, window, cx| {
                                s.start_provider_setup(provider, window, cx);
                            },
                        )
                        .icon(IconName::Plus)
                        .secondary()
                        .outline()
                        .h(px(32.))
                        .debug_selector(|| "account-start-login".into())
                        .disabled(disabled),
                    )
                    .child(
                        button(
                            "accounts-refresh",
                            language.text("使用量を再取得"),
                            cx,
                            |s, _, _| s.refresh_accounts_and_models(),
                        )
                        .icon(IconName::RotateCw)
                        .debug_selector(|| "accounts-refresh".into())
                        .disabled(disabled),
                    ),
            );
            if let Some(accounts) = accounts {
                let mut matching = accounts
                    .accounts
                    .iter()
                    .enumerate()
                    .filter(|(_, account)| account.provider == provider)
                    .peekable();
                if matching.peek().is_none() {
                    body = body.child(
                        div()
                            .text_sm()
                            .child(language.text("サインインして利用を開始できます。")),
                    );
                }
                for entry in matching {
                    body = body.child(self.account_row(
                        in_settings,
                        accounts.is_selected(entry.1),
                        entry,
                        disabled,
                        cx,
                    ));
                }
            } else {
                body = body.child(language.text("アカウントを読み込み中…"));
            }
        }
        body =
            body.child(div().text_xs().text_color(rgb(appearance::MUTED)).child(
                language.text("アカウントの切替は、同じ接続先を使う端末にも反映されます。"),
            ))
            .when(self.account.busy, |body| {
                body.child(div().text_sm().child(language.text("アカウントを更新中…")))
            })
            .when_some(
                accounts.and_then(|accounts| accounts.errors.get(&provider)),
                |body, error| {
                    body.child(
                        div()
                            .text_xs()
                            .text_color(rgb(0xff7777))
                            .child(error_view(&error_text(error), language)),
                    )
                },
            )
            .children(
                self.snapshot
                    .model_error_messages(Some(provider))
                    .into_iter()
                    .map(|error| {
                        div()
                            .text_xs()
                            .text_color(rgb(0xff7777))
                            .child(error_view(&error_text(&error), language))
                    }),
            );
        if let Some(error) = self
            .snapshot
            .operation_error(op::OperationKey::AccountLogin { provider })
        {
            body = body.child(
                div()
                    .text_sm()
                    .text_color(rgb(0xff8e86))
                    .child(error_view(&error, language)),
            );
        }
        if !in_settings {
            body = body.child(self.error_notices(ErrorScope::Settings, cx));
        }
        if provider == ProviderKind::Codex {
            body = body.child(self.codex_cli_controls(disabled, cx));
        }
        body.into_any_element()
    }

    fn codex_cli_controls(&self, disabled: bool, cx: &Context<Self>) -> Div {
        let language = crate::localization::language(cx);
        let cli = self.snapshot.account.codex_cli.as_deref();
        let updating = self.snapshot.operation_running(op::OperationKey::CodexCli);
        v_flex().gap_4().mt_2()
            .child(div().text_lg().font_semibold().child("Codex CLI"))
            .child(settings_group().p_5().gap_4()
                .child(h_flex().items_center().flex_wrap().gap_4()
                    .child(v_flex().flex_1().min_w(px(160.)).gap_1()
                        .child(div().font_semibold().child(cli.and_then(|status| status.version.clone()).unwrap_or_else(|| language.text("バージョン未確認").into()))))
                    .child(h_flex().gap_2()
                        .child(button("codex-cli-refresh", language.text("状態を確認"), cx, |s, _, _| {
                            s.dispatch(Intent::ManageCodexCli(op::ManageCodexCli { update: false }));
                            s.refresh_accounts_and_models();
                        }).secondary().outline().h(px(32.)).disabled(disabled || updating))
                        .child(button("codex-cli-update", if updating { language.text("確認・更新中…") } else { language.text("最新版に更新") }, cx, |s, _, _| {
                            s.dispatch(Intent::ManageCodexCli(op::ManageCodexCli { update: true }));
                        }).secondary().outline().h(px(32.)).disabled(disabled || updating).debug_selector(|| "codex-cli-update".into()))))
                .child(div().pt_4().border_t_1().border_color(rgb(appearance::BORDER))
                    .text_xs().text_color(rgb(appearance::MUTED))
                    .child(language.text("選択中の環境のCLIを更新します。更新後は実行中の会話が終わってからホストを再起動してください。モデル一覧は再接続時に再取得します。")))
                .when(cli.is_some_and(|cli| cli.restart_required), |body| body.child(language.text("更新が完了しました。新しいCLIを使用するにはホストを再起動してください。")))
                .when_some(cli.and_then(|cli| cli.error.as_ref()), |body, error| body.child(div().text_sm().text_color(rgb(0xff8e86)).child(error.clone()))))
    }
}

impl Desktop {
    pub(super) fn account_provider(&self) -> Option<ProviderKind> {
        self.model_controls
            .provider
            .or_else(|| {
                self.snapshot
                    .account
                    .login
                    .as_ref()
                    .map(|login| login.provider)
            })
            .or_else(|| {
                self.snapshot
                    .model_provider_for_draft(self.draft_key().clone())
            })
    }

    pub(in crate::app) fn start_provider_setup(
        &mut self,
        provider: ProviderKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.account
            .code
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.model_controls.provider = Some(provider);
        if provider == ProviderKind::DeepSeek {
            self.account_operation(Intent::ReadDshCredentials(op::ReadDshCredentials {}));
        } else {
            self.account_operation(Intent::StartAccountLogin(op::StartAccountLogin {
                provider,
            }));
        }
    }
    pub(in crate::app) fn account_operation(&mut self, intent: Intent) {
        if self.account.busy || self.session.is_none() || !self.snapshot.connected {
            return;
        }
        if let Intent::StartAccountLogin(start) = &intent {
            self.model_controls.provider = Some(start.provider);
            self.account.login_draft = Some(self.draft_key().clone());
        }
        self.account.busy = true;
        self.perform(intent, OperationCompletion::Account);
    }

    fn credential_controls(&self, cx: &Context<Self>) -> AnyElement {
        let language = crate::localization::language(cx);
        let disabled = !self.snapshot.connected
            || self
                .snapshot
                .operation_running(op::OperationKey::DshCredentials);
        let controls = self.snapshot.credential_controls();
        let selected = controls
            .iter()
            .find(|route| route.provider == self.account.dsh_key_provider)
            .expect("all credential routes");
        let url = selected.key_url.clone();
        let mut form = v_flex()
            .gap_2()
            .child(language.text("接続先のAPIキー"))
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .children(controls.iter().map(|route| {
                        let provider = route.provider;
                        button(
                            format!("dsh-key-{}", provider.reference()),
                            route.label.clone(),
                            cx,
                            move |s, window, cx| {
                                s.account.dsh_key_provider = provider;
                                s.account
                                    .code
                                    .update(cx, |input, cx| input.set_value("", window, cx));
                            },
                        )
                        .small()
                        .selected(self.account.dsh_key_provider == provider)
                        .disabled(disabled)
                        .debug_selector(move || format!("dsh-key-{}", provider.reference()))
                    })),
            )
            .child(agent_core::localization::localized_ui_text(
                language,
                selected.status.clone(),
            ))
            .child(
                button(
                    "account-open-login",
                    language.text("APIキーを取得"),
                    cx,
                    move |_, _, cx| cx.open_url(&url),
                )
                .small(),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div().flex_1().min_w_0().child(
                            Input::new(&self.account.code).aria_label(language.text("APIキー")),
                        ),
                    )
                    .child(
                        button(
                            "account-submit-code",
                            language.text("保存"),
                            cx,
                            |s, window, cx| {
                                let key = s.account.code.read(cx).value().to_string();
                                if key.trim().is_empty() {
                                    return;
                                }
                                s.account_operation(Intent::UpdateDshCredential(
                                    op::UpdateDshCredential {
                                        provider: s.account.dsh_key_provider,
                                        key: Some(key),
                                    },
                                ));
                                s.account
                                    .code
                                    .update(cx, |input, cx| input.set_value("", window, cx));
                            },
                        )
                        .disabled(disabled || self.account.code.read(cx).value().trim().is_empty())
                        .debug_selector(|| "account-submit-code".into()),
                    ),
            )
            .child(
                button(
                    "dsh-credentials-refresh",
                    language.text("設定状態を取得"),
                    cx,
                    |s, _, _| s.dispatch(Intent::ReadDshCredentials(op::ReadDshCredentials {})),
                )
                .small()
                .disabled(disabled),
            );
        if selected.can_remove {
            form = form.child(
                button(
                    "dsh-key-remove",
                    language.text("この接続先のキーを削除"),
                    cx,
                    |s, _, _| {
                        s.account_operation(Intent::UpdateDshCredential(op::UpdateDshCredential {
                            provider: s.account.dsh_key_provider,
                            key: None,
                        }));
                    },
                )
                .small()
                .ghost()
                .disabled(disabled)
                .debug_selector(|| "dsh-key-remove".into()),
            );
        }
        if selected.provider == agent_protocol::dsh::KeyProvider::OpenRouter && selected.can_remove
        {
            form = form.child(
                button(
                    "dsh-openrouter-models",
                    language.text("OpenRouterのモデル一覧を開く"),
                    cx,
                    |s, window, cx| s.show_openrouter_models(window, cx),
                )
                .small()
                .disabled(disabled)
                .debug_selector(|| "dsh-openrouter-models".into()),
            );
        }
        if let Some(error) = self
            .snapshot
            .operation_error(op::OperationKey::DshCredentials)
        {
            form = form.child(
                div()
                    .text_sm()
                    .text_color(rgb(0xff7777))
                    .child(error_view(&error, language)),
            );
        }
        form.into_any_element()
    }

    pub(super) fn account_login_controls(
        &self,
        title: Option<&str>,
        cx: &Context<Self>,
    ) -> AnyElement {
        let language = crate::localization::language(cx);
        let Some(login) = &self.snapshot.account.login else {
            return div().into_any_element();
        };
        let disabled = !self.snapshot.connected || self.account.busy || self.busy > 0;
        let url = login.verification_url.clone();
        let cancel_id = login.login_id.clone();
        let provider = login.provider;
        let cancel = button(
            "account-cancel-login",
            language.text("キャンセル"),
            cx,
            move |s, _, _| {
                s.account_operation(Intent::CancelAccountLogin(op::CancelAccountLogin {
                    provider,
                    id: cancel_id.clone(),
                }));
            },
        )
        .debug_selector(|| "account-cancel-login".into())
        .small()
        .ghost()
        .disabled(disabled);
        let mut form = v_flex()
            .gap_3()
            .text_sm()
            .child(if login.requires_code_submission {
                language.text("ブラウザでログインし、認証コードを貼り付けてください。")
            } else {
                language.text("ブラウザでログインし、次のコードを入力してください。")
            })
            .child(
                button(
                    "account-open-login",
                    language.text("ブラウザでログイン"),
                    cx,
                    move |_, _, cx| {
                        cx.open_url(&url);
                    },
                )
                .debug_selector(|| "account-open-login".into())
                .small(),
            );
        if login.requires_code_submission {
            let submit_id = login.login_id.clone();
            form = form.child(
                h_flex()
                    .gap_2()
                    .child(div().flex_1().min_w_0().child(
                        Input::new(&self.account.code).aria_label(language.text("認証コード")),
                    ))
                    .child(
                        button(
                            "account-submit-code",
                            language.text("接続"),
                            cx,
                            move |s, window, cx| {
                                let code = s.account.code.read(cx).value().to_string();
                                if code.trim().is_empty() {
                                    return;
                                }
                                s.account_operation(Intent::SubmitAccountLogin(
                                    op::SubmitAccountLogin {
                                        provider,
                                        id: submit_id.clone(),
                                        code,
                                    },
                                ));
                                s.account
                                    .code
                                    .update(cx, |input, cx| input.set_value("", window, cx));
                            },
                        )
                        .debug_selector(|| "account-submit-code".into())
                        .disabled(disabled || self.account.code.read(cx).value().trim().is_empty()),
                    ),
            );
        } else {
            let code = login.user_code.clone();
            form = form.child(
                h_flex()
                    .gap_3()
                    .child(div().text_xl().child(login.user_code.clone()))
                    .child(
                        button(
                            "account-copy-code",
                            language.text("コードをコピー"),
                            cx,
                            move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(code.clone()));
                            },
                        )
                        .small()
                        .ghost(),
                    ),
            );
        }
        form = form
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(appearance::MUTED))
                    .child(language.text("ログインが完了すると、自動で接続されます。")),
            )
            .when(!self.account.polling && !self.account.busy, |form| {
                form.child(
                    button(
                        "account-retry-login",
                        language.text("接続状態を再確認"),
                        cx,
                        |s, _, _| {
                            s.account.polling = true;
                        },
                    )
                    .small()
                    .ghost()
                    .disabled(disabled),
                )
            });
        match title {
            Some(title) => v_flex()
                .gap_2()
                .child(
                    h_flex()
                        .gap_3()
                        .py_1()
                        .child(div().flex_1().child(title.to_owned()))
                        .child(
                            div()
                                .text_sm()
                                .text_color(rgb(appearance::MUTED))
                                .child(language.text("ログイン中")),
                        )
                        .child(cancel),
                )
                .child(
                    form.p_3()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(rgb(appearance::BORDER))
                        .bg(rgb(appearance::RAISED)),
                )
                .into_any_element(),
            None => form.child(cancel).into_any_element(),
        }
    }
}

fn account_usage_view(
    usage: Option<&agent_protocol::operations::AccountUsage>,
    language: agent_core::localization::UiLanguage,
) -> AnyElement {
    let mut body = v_flex().w_full().gap_2().text_sm();
    let Some(usage) = usage else {
        return body
            .text_color(rgb(appearance::MUTED))
            .child(language.text("使用量は未取得です"))
            .into_any_element();
    };
    if let Some(error) = &usage.error {
        body = body.child(
            div()
                .text_color(rgb(0xe9b56f))
                .child(error_view(&error_text(error), language)),
        );
    }
    let mut windows = h_flex().w_full().flex_wrap().gap_5();
    for window in &usage.windows {
        windows = windows.child(
            usage_window_view(window, language).when_some(
                window
                    .resets_at
                    .and_then(|at| chrono::DateTime::from_timestamp(at, 0)),
                |body, at| {
                    body.child(div().text_color(rgb(appearance::MUTED)).child(
                        agent_core::localization::localized_ui_format(
                            language,
                            "{0} にリセット".into(),
                            vec![format!(
                                "{}",
                                at.with_timezone(&chrono::Local).format("%m/%d %H:%M")
                            )],
                        ),
                    ))
                },
            ),
        );
    }
    body.child(windows)
        .when_some(
            chrono::DateTime::from_timestamp(usage.fetched_at, 0).filter(|_| usage.error.is_none()),
            |body, at| {
                body.child(div().text_color(rgb(appearance::MUTED)).child(
                    agent_core::localization::localized_ui_format(
                        language,
                        "{0} 時点".into(),
                        vec![format!(
                            "{}",
                            at.with_timezone(&chrono::Local).format("%H:%M")
                        )],
                    ),
                ))
            },
        )
        .into_any_element()
}

impl Desktop {
    fn show_openrouter_models(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.model_controls.provider = Some(ProviderKind::DeepSeek);
        self.model_controls.panel = ModelPanel::Models;
        self.model_controls
            .search
            .update(cx, |input, cx| input.set_value("openrouter", window, cx));
        self.refresh_accounts_and_models();
        cx.notify();
    }

    fn dsh_model_controls(&self, disabled: bool, cx: &Context<Self>) -> AnyElement {
        let language = crate::localization::language(cx);
        let mut controls = Vec::new();
        if let Some(selected) = self
            .selected_model()
            .and_then(|model| model.credential_route)
        {
            controls.push(
                div()
                    .text_sm()
                    .font_semibold()
                    .id("dsh-selected-route")
                    .debug_selector(|| "dsh-selected-route".into())
                    .child(agent_core::localization::localized_ui_format(
                        language,
                        "選択中の送信先: {0}".into(),
                        vec![selected.model_label().to_string()],
                    ))
                    .into_any_element(),
            );
        }
        if let Some(model) = self
            .snapshot
            .models_matching(
                Some(ProviderKind::DeepSeek),
                r#"["openrouter","deepseek/deepseek-v4.1-flash"]"#.into(),
            )
            .into_iter()
            .next()
        {
            let value = model.model;
            let selected = self.draft().model.as_ref() == Some(&value);
            controls.push(
                Button::new("model-select-openrouter-flash")
                    .label(if selected {
                        language.text("OpenRouter · Flash 4.1 選択中")
                    } else {
                        language.text("OpenRouter経由のFlash 4.1を選択")
                    })
                    .accessibility_label(language.text("OpenRouterでDeepSeek V4.1 Flashを選択"))
                    .debug_selector(|| "model-select-openrouter-flash".into())
                    .small()
                    .selected(selected)
                    .disabled(disabled)
                    .on_click(cx.listener(move |s, _, window, cx| {
                        s.dispatch(Intent::SelectModel {
                            thread_id: s.draft_key().clone(),
                            model: value.clone(),
                        });
                        s.accept_snapshot(window, cx);
                        s.model_controls
                            .search
                            .update(cx, |input, cx| input.set_value("openrouter", window, cx));
                        cx.notify();
                    }))
                    .into_any_element(),
            );
        }
        controls.push(
            Button::new("model-openrouter")
                .label(language.text("OpenRouterのモデル一覧を表示"))
                .accessibility_label(language.text("OpenRouterのモデル一覧を表示"))
                .debug_selector(|| "model-openrouter".into())
                .small()
                .ghost()
                .disabled(disabled)
                .on_click(cx.listener(|s, _, window, cx| s.show_openrouter_models(window, cx)))
                .into_any_element(),
        );
        v_flex().gap_2().children(controls).into_any_element()
    }

    fn model_panel_content(&self, cx: &Context<Self>) -> AnyElement {
        let language = crate::localization::language(cx);
        let mut body = v_flex().flex_1().min_w_0().gap_2();
        if self.model_controls.panel == ModelPanel::Accounts {
            let header = h_flex()
                .gap_2()
                .h(px(44.))
                .child(
                    icon_button(
                        "model-panel-back",
                        IconName::ArrowLeft,
                        language.text("戻る"),
                        cx,
                        |s, _, _| {
                            s.account.sign_out = None;
                            s.model_controls.provider = None;
                            s.model_controls.panel = ModelPanel::Models;
                        },
                    )
                    .disabled(self.snapshot.account.login.is_some())
                    .debug_selector(|| "model-panel-back".into()),
                )
                .child(div().text_base().child(language.text("アカウント")));
            return body
                .w_full()
                .child(header)
                .child(
                    div()
                        .id("model-accounts-scroll")
                        .max_h(px(440.))
                        .overflow_y_scroll()
                        .child(self.account_controls(cx)),
                )
                .into_any_element();
        }
        let provider = self.account_provider();
        let disabled = self.account.busy
            || self.busy > 0
            || !self.snapshot.connected
            || self.snapshot.account.login.is_some();
        let mut agents = v_flex()
            .flex_shrink_0()
            .gap_2()
            .pr_2()
            .border_r_1()
            .border_color(rgb(appearance::BORDER));
        for (value, label) in [
            (ProviderKind::Codex, "Codex"),
            (ProviderKind::Claude, "Claude"),
            (ProviderKind::DeepSeek, "DeepSeek"),
        ] {
            agents = agents.child(
                Button::new(format!("model-agent-{label}"))
                    .icon(if value == ProviderKind::Claude {
                        Icon::default()
                            .path("icons/claude.svg")
                            .text_color(rgb(0xd97757))
                    } else {
                        provider_icon(value)
                    })
                    .large()
                    .ghost()
                    .size(px(44.))
                    .rounded(px(8.))
                    .selected(provider == Some(value))
                    .when(provider == Some(value), |button| {
                        button.bg(rgb(appearance::SELECTED))
                    })
                    .accessibility_label(label)
                    .tooltip(label)
                    .debug_selector(move || format!("model-agent-{label}"))
                    .disabled(disabled)
                    .on_click(cx.listener(move |s, _, window, cx| {
                        s.model_controls.provider = Some(value);
                        s.model_controls
                            .search
                            .update(cx, |input, cx| input.set_value("", window, cx));
                        if let Some(model) =
                            s.snapshot.model_for_provider(s.draft_key().clone(), value)
                        {
                            s.dispatch(Intent::SelectModel {
                                thread_id: s.draft_key().clone(),
                                model,
                            });
                        }
                        cx.notify();
                    })),
            );
        }
        let account = self
            .snapshot
            .account
            .accounts
            .as_ref()
            .and_then(|accounts| {
                accounts.accounts.iter().find(|account| {
                    Some(account.provider) == provider && accounts.is_selected(account)
                })
            });
        body = body.child(
            Button::new("model-account-summary")
                .accessibility_label(language.text("アカウントと週間残量"))
                .ghost()
                .w_full()
                .h_auto()
                .debug_selector(|| "model-account-summary".into())
                .p_2()
                .rounded(px(8.))
                .on_click(cx.listener(|s, _, _, cx| {
                    s.model_controls.panel = ModelPanel::Accounts;
                    cx.notify();
                }))
                .child(
                    v_flex()
                        .id("model-account-row")
                        .debug_selector(|| "model-account-row".into())
                        .w_full()
                        .gap_2()
                        .child(
                            h_flex()
                                .w_full()
                                .justify_between()
                                .gap_2()
                                .text_base()
                                .line_height(px(20.))
                                .font_weight(FontWeight::NORMAL)
                                .child(
                                    account_identity(
                                        None,
                                        account.map_or(language.text("未選択"), |a| {
                                            a.email.as_deref().unwrap_or(&a.id)
                                        }),
                                    )
                                    .flex_1(),
                                )
                                .child(Icon::new(IconName::ChevronRight).size(px(16.))),
                        )
                        .child(weekly_usage_view(
                            account
                                .map(|a| {
                                    self.snapshot.account_weekly_usage(a.provider, a.id.clone())
                                })
                                .unwrap_or_default(),
                            language,
                        )),
                ),
        );
        body = body
            .child(div().h(px(1.)).w_full().bg(rgb(appearance::RAISED)))
            .child(
                Input::new(&self.model_controls.search)
                    .prefix(IconName::Search)
                    .appearance(false)
                    .large()
                    .aria_label(language.text("モデルを検索")),
            );
        let custom_model = self
            .snapshot
            .custom_model_choice(provider, &self.model_controls.search.read(cx).value());
        if provider == Some(ProviderKind::DeepSeek) {
            body = body.child(self.dsh_model_controls(disabled, cx));
        }
        let models = self.snapshot.models_matching(
            provider,
            self.model_controls.search.read(cx).value().to_string(),
        );
        let mut list = v_flex()
            .id("model-catalog")
            .max_h(px(264.))
            .overflow_y_scroll();
        if models.is_empty() && custom_model.is_none() {
            list = list.child(language.text(if self.snapshot.models_loading(provider) {
                "モデルを読み込み中…"
            } else {
                "利用可能なモデルがありません"
            }));
        }
        for model in models {
            let value = model.model.clone();
            let selector = format!("model-choice-{}", model.id);
            let selected = self.draft().model.as_ref() == Some(&model.model);
            list = list.child(
                Button::new(selector.clone())
                    .accessibility_label(model.display_name.clone())
                    .on_click(cx.listener(move |s, _, _, _| {
                        s.dispatch(Intent::SelectModel {
                            thread_id: s.draft_key().clone(),
                            model: value.clone(),
                        });
                    }))
                    .debug_selector(move || selector.clone())
                    .ghost()
                    .w_full()
                    .h(px(44.))
                    .px_2()
                    .disabled(disabled)
                    .selected(selected)
                    .when(selected, |button| button.bg(rgb(appearance::RAISED)))
                    .child(
                        h_flex()
                            .w_full()
                            .justify_between()
                            .gap_2()
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_base()
                                    .line_height(px(20.))
                                    .font_weight(FontWeight::NORMAL)
                                    .child(model.display_name),
                            )
                            .when(selected, |row| {
                                row.child(
                                    Icon::new(IconName::Check)
                                        .size(px(20.))
                                        .text_color(rgb(appearance::ACCENT)),
                                )
                            }),
                    ),
            );
        }
        if let Some(model) = custom_model {
            let label = agent_core::localization::localized_ui_format(
                language,
                "モデルIDを指定: {0}".into(),
                vec![model.id.to_string()],
            );
            list = list.child(
                Button::new("custom-model-choice")
                    .debug_selector(|| "custom-model-choice".into())
                    .label(label)
                    .ghost()
                    .w_full()
                    .disabled(disabled)
                    .on_click(cx.listener(move |s, _, _, _| {
                        s.dispatch(Intent::SelectModel {
                            thread_id: s.draft_key().clone(),
                            model: model.clone(),
                        });
                    })),
            );
        }
        body = body
            .child(list)
            .children(
                self.snapshot
                    .model_error_messages(provider)
                    .into_iter()
                    .map(|error| {
                        div()
                            .text_xs()
                            .text_color(rgb(0xff7777))
                            .child(error_view(&error_text(&error), language))
                    }),
            )
            .when(
                self.draft().model.as_ref().map(|model| model.provider) == provider,
                |body| {
                    body.child(
                        h_flex()
                            .gap_3()
                            .border_t_1()
                            .border_color(rgb(appearance::BORDER))
                            .child(Self::effort_control(
                                self.snapshot.model_quick_controls(self.draft_key().clone()),
                                self.model_controls_enabled(),
                                "model-picker-effort",
                                true,
                                cx,
                            ))
                            .child(Self::fast_control(
                                self.snapshot.model_quick_controls(self.draft_key().clone()),
                                self.model_controls_enabled(),
                                "model-picker-speed",
                                true,
                                cx,
                            )),
                    )
                },
            );
        h_flex()
            .w_full()
            .items_stretch()
            .gap_3()
            .child(agents)
            .child(body)
            .into_any_element()
    }

    pub(super) fn fast_control(
        controls: agent_core::presentation::model_settings::ModelQuickControls,
        enabled: bool,
        id: &'static str,
        expanded: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        let language = crate::localization::language(cx);
        let Some(next) = controls.toggle_fast_to else {
            return div().into_any_element();
        };
        let fast = controls.fast;
        let selected_speed = controls.speed_choices.iter().find(|choice| choice.selected);
        let speed_label = selected_speed.map_or(language.text("通常").to_owned(), |choice| {
            choice.label.clone()
        });
        let speed_tooltip = selected_speed
            .and_then(|choice| choice.description.clone())
            .unwrap_or_else(|| speed_label.clone());
        let label = if fast {
            language.text("Fast：オン")
        } else {
            language.text("Fast：オフ")
        };
        let button = Button::new(id)
            .icon(fast_icon(fast))
            .ghost()
            .accessibility_label(label)
            .tooltip(speed_tooltip)
            .debug_selector(move || id.into())
            .h(px(44.))
            .text_color(if fast {
                rgb(appearance::FOREGROUND)
            } else {
                rgb(appearance::MUTED)
            })
            .disabled(!enabled);
        if expanded {
            let entity = cx.entity().downgrade();
            button
                .label(speed_label)
                .child(Icon::new(IconName::ChevronDown).size(px(14.)))
                .dropdown_menu_with_anchor(Anchor::BottomRight, move |mut menu, _, _| {
                    for choice in &controls.speed_choices {
                        let value = choice.id.clone();
                        let label = choice.description.as_ref().map_or_else(
                            || choice.label.clone(),
                            |description| format!("{} · {}", choice.label, description),
                        );
                        let entity = entity.clone();
                        menu =
                            menu.item(PopupMenuItem::new(label).checked(choice.selected).on_click(
                                move |_, _, cx| {
                                    let _ = entity.update(cx, |s, cx| {
                                        s.dispatch(Intent::SelectServiceTier {
                                            thread_id: s.draft_key().clone(),
                                            service_tier: value.clone(),
                                        });
                                        cx.notify();
                                    });
                                },
                            ));
                    }
                    menu
                })
                .into_any_element()
        } else {
            button
                .w(px(40.))
                .on_click(cx.listener(move |s, _, _, _| {
                    s.dispatch(Intent::SelectServiceTier {
                        thread_id: s.draft_key().clone(),
                        service_tier: next.clone(),
                    });
                }))
                .into_any_element()
        }
    }

    pub(super) fn effort_control(
        controls: agent_core::presentation::model_settings::ModelQuickControls,
        enabled: bool,
        id: &'static str,
        expanded: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        let language = crate::localization::language(cx);
        if controls.efforts.is_empty() {
            return div().into_any_element();
        }
        let label = agent_core::localization::localized_ui_format(
            language,
            "推論の強度：{0}".into(),
            vec![controls.effort.to_string()],
        );
        let mut bars = h_flex().items_end().gap(px(2.));
        for index in 0..controls.efforts.len() {
            bars = bars.child(
                div()
                    .w(px(3.))
                    .h(px(
                        6. + 10. * (index + 1) as f32 / controls.efforts.len() as f32
                    ))
                    .rounded(px(1.))
                    .bg(if (index as u32) < controls.effort_level {
                        rgb(appearance::FOREGROUND)
                    } else {
                        rgb(0x555555)
                    }),
            );
        }
        let entity = cx.entity().downgrade();
        Button::new(id)
            .child(bars)
            .ghost()
            .when(!expanded, |button| button.w(px(44.)))
            .when(expanded, |button| {
                button
                    .label(controls.effort.clone())
                    .child(Icon::new(IconName::ChevronDown).size(px(14.)))
            })
            .h(px(44.))
            .debug_selector(move || id.into())
            .tooltip(label.clone())
            .accessibility_label(label)
            .disabled(!enabled)
            .dropdown_menu_with_anchor(Anchor::BottomRight, move |mut menu, _, _| {
                for effort in &controls.efforts {
                    let value = effort.clone();
                    let entity = entity.clone();
                    menu = menu.item(
                        PopupMenuItem::new(effort.clone())
                            .checked(*effort == controls.effort)
                            .on_click(move |_, _, cx| {
                                let _ = entity.update(cx, |s, cx| {
                                    s.dispatch(Intent::SelectEffort {
                                        thread_id: s.draft_key().clone(),
                                        effort: value.clone(),
                                    });
                                    cx.notify();
                                });
                            }),
                    );
                }
                menu
            })
            .into_any_element()
    }
}

fn weekly_usage_view(
    windows: Vec<agent_protocol::operations::UsageWindow>,
    language: agent_core::localization::UiLanguage,
) -> AnyElement {
    let mut body = v_flex()
        .w_full()
        .gap_2()
        .text_sm()
        .font_weight(FontWeight::NORMAL);
    if windows.is_empty() {
        return body
            .text_color(rgb(appearance::MUTED))
            .child(language.text("残量未取得"))
            .into_any_element();
    }
    for window in windows {
        body = body.child(
            h_flex()
                .w_full()
                .gap_3()
                .child(div().text_xs().text_color(rgb(appearance::MUTED)).child(
                    agent_core::localization::localized_ui_text(language, window.label.clone()),
                ))
                .child(usage_bar(window.remaining_percent, rgb(appearance::ACCENT)).flex_1())
                .child(
                    div()
                        .line_height(px(20.))
                        .text_color(rgb(appearance::MUTED))
                        .child(format!("{}%", window.remaining_percent)),
                ),
        );
    }
    body.into_any_element()
}

fn usage_window_view(
    window: &agent_protocol::operations::UsageWindow,
    language: agent_core::localization::UiLanguage,
) -> Div {
    v_flex()
        .flex_1()
        .min_w(px(180.))
        .gap_1()
        .child(
            h_flex()
                .justify_between()
                .gap_2()
                .child(agent_core::localization::localized_ui_text(
                    language,
                    window.label.clone(),
                ))
                .child(agent_core::localization::localized_ui_format(
                    language,
                    "残り {0}%".into(),
                    vec![window.remaining_percent.to_string()],
                )),
        )
        .child(usage_bar(
            window.remaining_percent,
            rgb(appearance::FOREGROUND),
        ))
}

fn usage_bar(remaining_percent: u32, tint: Rgba) -> Div {
    div()
        .h(px(4.))
        .w_full()
        .rounded(px(4.))
        .bg(rgb(appearance::BORDER))
        .child(
            div()
                .h_full()
                .w(relative(remaining_percent.min(100) as f32 / 100.))
                .rounded(px(4.))
                .bg(if remaining_percent <= 20 {
                    rgb(0xe9b56f)
                } else {
                    tint
                }),
        )
}

#[cfg(test)]
mod tests {
    use super::{Desktop, Draft, Mode, ModelPanel, RemoteHost};
    use gpui_kit as gpui;
    use gpui_kit::component::{h_flex, v_flex};
    use gpui_kit::{AppContext, Modifiers, Render, TestAppContext};
    use gpui_kit::{Context, Entity, IntoElement, ParentElement, Styled, Window, px};
    use std::sync::Arc;

    struct PickerView(Entity<Desktop>);
    impl Render for PickerView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.0.update(cx, |view, cx| {
                v_flex()
                    .w(px(420.))
                    .gap_3()
                    .child(
                        h_flex()
                            .child(Desktop::fast_control(
                                view.snapshot.model_quick_controls(view.draft_key().clone()),
                                view.model_controls_enabled(),
                                "model-fast",
                                false,
                                cx,
                            ))
                            .child(Desktop::model_menu(
                                view.selected_model()
                                    .map_or("モデルを選択".to_owned(), |model| {
                                        model.display_name.clone()
                                    }),
                                cx.entity().entity_id(),
                                cx,
                            ))
                            .child(Desktop::effort_control(
                                view.snapshot.model_quick_controls(view.draft_key().clone()),
                                view.model_controls_enabled(),
                                "model-effort",
                                false,
                                cx,
                            )),
                    )
                    .child(view.model_panel_content(cx))
            })
        }
    }

    #[gpui::test]
    fn model_picker_keeps_quick_controls_and_routes_quota_to_account_management(
        cx: &mut TestAppContext,
    ) {
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
        let (view, window) = cx.add_window_view(|window, cx| {
            let desktop = cx.new(|cx| Desktop::new(Mode::SideChat {
                remote: Some(RemoteHost { id: "fixture".into(), name: "fixture".into(), ticket: "invalid-fixture-ticket".into() }), cwd: "/fixture".into(),
            }, window, cx));
            desktop.update(cx, |view, _| {
                let key = view.draft_key().to_owned();
                let snapshot = Arc::make_mut(&mut view.snapshot);
                snapshot.connected = true;
                Arc::make_mut(&mut snapshot.account).accounts = Some(Arc::new(serde_json::from_value(serde_json::json!({
                    "accounts":[{"provider":"codex","id":"first","email":"first@example.invalid","usage":{"windows":[{"label":"週間枠","remainingPercent":42}],"fetchedAt":1}},
                    {"provider":"claude","id":"claude:second"}],"selected":{"codex":"first","claude":"claude:second"}
                })).unwrap()));
                snapshot.models = Arc::new(serde_json::from_value(serde_json::json!([
                    {"id":"gpt","model":{"provider": "codex", "id": "gpt"},"displayName":"GPT", "settings": {"defaultReasoningEffort":"medium","supportedReasoningEfforts":[{"reasoningEffort":"medium"},{"reasoningEffort":"high"}],"serviceTiers":[{"id":"priority","fast":true}]}},
                    {"id":"claude:sonnet","model":{"provider": "claude", "id": "sonnet"},"displayName":"Sonnet", "settings": {"defaultReasoningEffort":"","supportedReasoningEfforts":[]}}
                ])).unwrap());
                Arc::make_mut(&mut snapshot.drafts).insert(key, Arc::new(Draft { model: Some(agent_protocol::models::ModelRef { provider: agent_protocol::session::ProviderKind::Codex, id: "gpt".into() }), ..Default::default() }));
            });
            cx.observe(&desktop, |_, _, cx| cx.notify()).detach();
            PickerView(desktop)
        });
        window.run_until_parked();
        assert!(
            window.debug_bounds("model-fast").unwrap().right()
                <= window.debug_bounds("model-select").unwrap().left()
        );
        assert!(
            window.debug_bounds("model-select").unwrap().right()
                <= window.debug_bounds("model-effort").unwrap().left()
        );
        assert!(window.debug_bounds("model-choice-gpt").is_some());
        let model = window.debug_bounds("model-choice-gpt").unwrap();
        let effort = window.debug_bounds("model-picker-effort").unwrap();
        let speed = window.debug_bounds("model-picker-speed").unwrap();
        assert!(model.bottom() <= effort.top());
        assert_eq!(effort.top(), speed.top());
        assert!(effort.right() <= speed.left());
        assert!(window.debug_bounds("model-choice-claude:sonnet").is_none());
        assert!(window.debug_bounds("account-logout-0").is_none());
        let summary = window.debug_bounds("model-account-summary").unwrap();
        let codex = window.debug_bounds("model-agent-Codex").unwrap();
        let claude = window.debug_bounds("model-agent-Claude").unwrap();
        assert!(codex.right() <= summary.left());
        assert!(codex.bottom() <= claude.top());
        assert!(window.debug_bounds("model-agent-label").is_none());
        assert!(window.debug_bounds("model-account-label").is_none());
        let account_row = window.debug_bounds("model-account-row").unwrap();
        assert!(account_row.size.width >= summary.size.width - px(24.));
        window.simulate_click(claude.center(), Modifiers::default());
        window.run_until_parked();
        assert!(window.debug_bounds("model-choice-claude:sonnet").is_some());
        assert!(window.debug_bounds("model-choice-gpt").is_none());
        assert!(window.debug_bounds("model-picker-effort").is_none());
        assert!(window.debug_bounds("model-picker-speed").is_none());
        window.simulate_click(codex.center(), Modifiers::default());
        window.run_until_parked();
        assert!(window.debug_bounds("model-choice-gpt").is_some());
        assert!(window.debug_bounds("model-picker-effort").is_some());
        window.simulate_click(summary.center(), Modifiers::default());
        window.run_until_parked();
        assert!(window.debug_bounds("account-choice-0").is_some());
        assert!(window.debug_bounds("account-choice-1").is_none());
        assert!(window.debug_bounds("account-start-login").is_some());
        assert!(window.debug_bounds("accounts-refresh").is_some());
        assert!(window.debug_bounds("model-choice-gpt").is_none());
        let logout = window.debug_bounds("account-logout-0").unwrap();
        window.simulate_click(logout.center(), Modifiers::default());
        window.run_until_parked();
        window.update(|_, cx| {
            assert_eq!(
                view.read(cx).0.read(cx).account.sign_out.as_deref(),
                Some("first")
            )
        });
        let cancel = window.debug_bounds("account-logout-cancel-0").unwrap();
        window.simulate_click(cancel.center(), Modifiers::default());
        window.run_until_parked();
        window.update(|_, cx| assert!(view.read(cx).0.read(cx).account.sign_out.is_none()));
        window.simulate_click(logout.center(), Modifiers::default());
        window.run_until_parked();
        let back = window.debug_bounds("model-panel-back").unwrap();
        window.simulate_click(back.center(), Modifiers::default());
        window.run_until_parked();
        assert!(window.debug_bounds("model-choice-gpt").is_some());
        assert!(window.debug_bounds("account-logout-0").is_none());
        window.update(|_, cx| assert!(view.read(cx).0.read(cx).account.sign_out.is_none()));
        let summary = window.debug_bounds("model-account-summary").unwrap();
        window.simulate_click(summary.center(), Modifiers::default());
        window.run_until_parked();
        let claude = window.debug_bounds("model-provider-Claude").unwrap();
        window.simulate_click(claude.center(), Modifiers::default());
        window.run_until_parked();
        assert!(window.debug_bounds("account-choice-0").is_none());
        assert!(window.debug_bounds("account-choice-1").is_some());
        assert!(window.debug_bounds("account-logout-1").is_some());
        window.simulate_click(back.center(), Modifiers::default());
        window.run_until_parked();
        assert!(window.debug_bounds("model-choice-gpt").is_some());
        window.update(|window, cx| {
            view.read(cx).0.clone().update(cx, |view, cx| {
                view.model_controls
                    .search
                    .update(cx, |input, cx| input.set_value("no match", window, cx));
                cx.notify();
            })
        });
        window.run_until_parked();
        assert!(window.debug_bounds("model-choice-gpt").is_none());
        window.update(|window, cx| {
            view.read(cx).0.clone().update(cx, |view, cx| {
                let draft = view.draft().clone();
                let session = agent_protocol::session::SessionRef {
                    provider: agent_protocol::session::ProviderKind::Codex,
                    id: "existing".into(),
                };
                let snapshot = Arc::make_mut(&mut view.snapshot);
                Arc::make_mut(&mut snapshot.navigation).thread_id = Some(session.clone());
                Arc::make_mut(&mut snapshot.navigation).draft_key = session.clone().into();
                Arc::make_mut(&mut snapshot.drafts).insert(session.into(), Arc::new(draft));
                view.model_controls.provider = None;
                view.model_controls
                    .search
                    .update(cx, |input, cx| input.set_value("", window, cx));
                cx.notify();
            })
        });
        window.run_until_parked();
        let claude = window.debug_bounds("model-agent-Claude").unwrap();
        window.simulate_click(claude.center(), Modifiers::default());
        window.run_until_parked();
        window.update(|_, cx| {
            let desktop = view.read(cx).0.read(cx);
            assert_eq!(
                desktop.model_controls.provider,
                Some(agent_protocol::session::ProviderKind::Claude)
            );
            assert_eq!(desktop.selected().unwrap().id, "existing");
        });
        assert!(window.debug_bounds("model-choice-claude:sonnet").is_some());
        window.update(|_, cx| {
            view.read(cx).0.clone().update(cx, |view, cx| {
                view.model_controls.panel = ModelPanel::Accounts;
                view.account.busy = true;
                cx.notify();
            })
        });
        window.run_until_parked();
        let claude = window.debug_bounds("model-provider-Claude").unwrap();
        window.simulate_click(claude.center(), Modifiers::default());
        window.update(|_, cx| {
            assert_eq!(
                view.read(cx).0.read(cx).model_controls.provider,
                Some(agent_protocol::session::ProviderKind::Claude)
            )
        });
    }
    #[gpui::test]
    fn dsh_key_form_switches_provider_without_carrying_the_previous_secret(
        cx: &mut TestAppContext,
    ) {
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
        let (view, window) = cx.add_window_view(|window, cx| {
            let desktop = cx.new(|cx| {
                Desktop::new(
                    Mode::SideChat {
                        remote: Some(RemoteHost {
                            id: "fixture".into(),
                            name: "fixture".into(),
                            ticket: "invalid-fixture-ticket".into(),
                        }),
                        cwd: "/fixture".into(),
                    },
                    window,
                    cx,
                )
            });
            desktop.update(cx, |view, cx| {
                view.model_controls.provider =
                    Some(agent_protocol::session::ProviderKind::DeepSeek);
                view.model_controls.panel = ModelPanel::Accounts;
                let snapshot = Arc::make_mut(&mut view.snapshot);
                snapshot.connected = true;
                Arc::make_mut(&mut snapshot.account).dsh_credentials =
                    Some(Arc::new(agent_protocol::dsh::Credentials {
                        routes: vec![agent_protocol::dsh::CredentialRoute {
                            provider: agent_protocol::dsh::KeyProvider::OpenRouter,
                            source: agent_protocol::dsh::CredentialSource::Managed,
                            needs_attention: false,
                        }],
                    }));
                view.account.code.update(cx, |input, cx| {
                    input.set_value("previous-secret", window, cx)
                });
            });
            cx.observe(&desktop, |_, _, cx| cx.notify()).detach();
            PickerView(desktop)
        });
        window.run_until_parked();
        assert!(window.debug_bounds("dsh-key-remove").is_none());
        let router = window.debug_bounds("dsh-key-OPENROUTER_API_KEY").unwrap();
        window.simulate_click(router.center(), Modifiers::default());
        window.run_until_parked();
        window.update(|_, cx| {
            let desktop = view.read(cx).0.read(cx);
            assert!(
                desktop.account.dsh_key_provider == agent_protocol::dsh::KeyProvider::OpenRouter
            );
            assert!(desktop.account.code.read(cx).value().is_empty());
        });
        assert!(window.debug_bounds("dsh-key-remove").is_some());
        assert!(window.debug_bounds("account-submit-code").is_some());
        assert!(window.debug_bounds("dsh-openrouter-models").is_some());
        window.update(|window, cx| {
            view.read(cx).0.clone().update(cx, |view, cx| {
                view.account.code.update(cx, |input, cx| {
                    input.set_value("unsaved-route-key", window, cx)
                });
            });
        });
        let codex = window.debug_bounds("model-provider-Codex").unwrap();
        window.simulate_click(codex.center(), Modifiers::default());
        window.run_until_parked();
        window.update(|_, cx| {
            assert!(
                view.read(cx)
                    .0
                    .read(cx)
                    .account
                    .code
                    .read(cx)
                    .value()
                    .is_empty()
            )
        });
        let dsh = window.debug_bounds("model-provider-DeepSeek").unwrap();
        window.simulate_click(dsh.center(), Modifiers::default());
        window.run_until_parked();
        window.update(|_, cx| {
            view.read(cx).0.clone().update(cx, |view, _| {
                Arc::make_mut(&mut view.snapshot).models = Arc::new(serde_json::from_value(serde_json::json!([
                    {"id":"official","credentialRoute":"DeepSeek","model":{"provider":"deepseek","id":"[\"deepseek-official\",\"deepseek-flash\"]"},"displayName":"DeepSeek", "settings": {"defaultReasoningEffort":"","supportedReasoningEfforts":[]}},
                    {"id":"router","credentialRoute":"OpenRouter","model":{"provider":"deepseek","id":"[\"openrouter\",\"deepseek/deepseek-v4.1-flash\"]"},"displayName":"OpenRouter · DeepSeek V4.1 Flash", "settings": {"defaultReasoningEffort":"","supportedReasoningEfforts":[]}}
                ])).unwrap());
            });
        });
        let models = window.debug_bounds("dsh-openrouter-models").unwrap();
        window.simulate_click(models.center(), Modifiers::default());
        window.run_until_parked();
        assert!(window.debug_bounds("model-choice-router").is_some());
        assert!(window.debug_bounds("model-choice-official").is_none());
        window.update(|_, cx| {
            let desktop = view.read(cx).0.read(cx);
            assert!(desktop.model_controls.panel == ModelPanel::Models);
            assert_eq!(desktop.model_controls.search.read(cx).value(), "openrouter");
        });
    }

    #[gpui::test]
    fn openrouter_selection_changes_the_real_store_and_keeps_unfinished_input(
        cx: &mut TestAppContext,
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let app_runtime = crate::Runtime {
            handle: runtime.handle().clone(),
            connections: Arc::new(crate::platform::Connections::default()),
            closing: tokio_util::task::TaskTracker::new(),
            logging_error: None,
        };
        let mut snapshot = agent_core::state::Snapshot {
            models: Arc::new(serde_json::from_value(serde_json::json!([
            {"id":"official","credentialRoute":"DeepSeek","model":{"provider":"deepseek","id":"[\"deepseek-official\",\"deepseek-flash\"]"},"displayName":"DeepSeek公式 · Flash", "settings": {"defaultReasoningEffort":"","supportedReasoningEfforts":[]}},
            {"id":"router","credentialRoute":"OpenRouter","model":{"provider":"deepseek","id":"[\"openrouter\",\"deepseek/deepseek-v4.1-flash\"]"},"displayName":"OpenRouter · DeepSeek V4.1 Flash", "settings": {"defaultReasoningEffort":"","supportedReasoningEfforts":[]}}
        ])).unwrap()),
            ..Default::default()
        };
        let key = snapshot.navigation.draft_key.clone();
        let official = snapshot.models[0].model.clone();
        Arc::make_mut(&mut snapshot.drafts).insert(
            key,
            Arc::new(Draft {
                text: "keep my unfinished input".into(),
                model: Some(official),
                ..Default::default()
            }),
        );
        let store =
            runtime.block_on(async { Arc::new(agent_core::store::Store::offline(snapshot)) });
        let session = runtime.block_on(async {
            let (updates, receive) = async_channel::unbounded();
            tokio::spawn(crate::store_session::StoreSession::publish(
                Ok(store.clone()),
                app_runtime.clone(),
                updates,
                Some,
                |_| None,
            ));
            receive.recv().await.unwrap().unwrap().unwrap()
        });
        cx.update(|cx| {
            crate::appearance::init(cx);
            cx.set_global(app_runtime);
        });
        let (view, window) = cx.add_window_view(|window, cx| {
            let desktop = cx.new(|cx| {
                Desktop::new(
                    Mode::SideChat {
                        remote: Some(RemoteHost {
                            id: "fixture".into(),
                            name: "fixture".into(),
                            ticket: "invalid-fixture-ticket".into(),
                        }),
                        cwd: "/fixture".into(),
                    },
                    window,
                    cx,
                )
            });
            desktop.update(cx, |view, _| {
                view.snapshot = store.snapshot();
                Arc::make_mut(&mut view.snapshot).connected = true;
                view.session = Some(session);
                view.model_controls.provider =
                    Some(agent_protocol::session::ProviderKind::DeepSeek);
            });
            cx.observe(&desktop, |_, _, cx| cx.notify()).detach();
            PickerView(desktop)
        });
        window.run_until_parked();
        assert!(window.debug_bounds("dsh-selected-route").is_some());
        let select = window
            .debug_bounds("model-select-openrouter-flash")
            .unwrap();
        window.simulate_click(select.center(), Modifiers::default());
        window.run_until_parked();
        window.update(|_, cx| {
            let desktop = view.read(cx).0.read(cx);
            assert_eq!(
                desktop.draft().model.as_ref().unwrap().id,
                r#"["openrouter","deepseek/deepseek-v4.1-flash"]"#
            );
            assert_eq!(desktop.draft().text, "keep my unfinished input");
            assert_eq!(
                store.snapshot().drafts[desktop.draft_key()].model,
                desktop.draft().model
            );
        });
    }
}
