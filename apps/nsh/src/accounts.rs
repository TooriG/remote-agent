use crate::{Connection, native};
use agent_core::state::{Intent, operations as op};
use agent_protocol::{dsh::KeyProvider, session::ProviderKind};
use nsh_core::{element::input, platform::Lifecycle, prelude::*};
use serde_json::json;

pub(crate) fn icon(provider: ProviderKind) -> &'static str {
    match provider {
        ProviderKind::Codex => "openai",
        ProviderKind::Claude => "claude",
        ProviderKind::DeepSeek => "deepseek",
    }
}

pub(crate) fn screen(
    provider: ProviderKind,
    close: impl Fn() + Copy + 'static,
) -> impl IntoElement {
    let connection = use_context::<Connection>().unwrap();
    let publication = connection.publication;
    let agent = computed(move || {
        publication.with(|p| {
            p.settings
                .connection_setup()
                .agents
                .into_iter()
                .find(|a| a.provider == provider)
        })
    });
    let host = stored(connection.host.get_untracked());
    let pending = computed(move || connection.state.with(|state| state.account_pending));
    let perform = move |intent| {
        connection
            .application
            .with(|app| app.account_request(host.get(), intent))
    };
    let login = computed(move || connection.settings.with(|p| p.account_login()));
    let busy = computed(move || {
        pending.get()
            || login.get().is_some()
            || publication.with(|p| {
                !p.settings.connected() || p.snapshot.operation_running(op::OperationKey::Accounts)
            })
    });
    let ready = computed(move || connection.settings.with(|p| p.connected()));
    effect(move || {
        if ready.get() {
            connection.dispatch(Intent::ListAccounts(op::ListAccounts {}));
            if provider == ProviderKind::DeepSeek {
                connection.dispatch(Intent::ReadDshCredentials(op::ReadDshCredentials {}));
            }
            if provider == ProviderKind::Codex {
                connection.dispatch(Intent::ManageCodexCli(op::ManageCodexCli { update: false }));
            }
        }
    });
    scroll().flex_1().child(
        view()
            .p_4()
            .gap_4()
            .pb_keyboard()
            .child(when(is_desktop, move || {
                native::text_button(json!("戻る"), close).prop("enabled", move || {
                    json!(!pending.get() && login.get().is_none())
                })
            }))
            .child(when(is_desktop, move || {
                text(move || {
                    agent.with(|agent| agent.as_ref().map(|a| a.name.clone()).unwrap_or_default())
                })
                .text_size(px(22.0))
            }))
            .child(text(move || {
                connection
                    .settings
                    .with(|p| p.host_name().unwrap_or_default())
            }))
            .child(
                text(move || {
                    publication.with(|p| {
                        p.settings
                            .account_login_error(provider)
                            .map(|error| error.to_string())
                            .unwrap_or_default()
                    })
                })
                .selectable(true),
            )
            .child(when(
                move || login.get().is_some(),
                move || login_panel(login, pending, perform),
            ))
            .child(when(
                move || login.get().is_none(),
                move || {
                    view()
                        .gap_4()
                        .child(
                            text(move || {
                                agent.with(|agent| {
                                    agent
                                        .as_ref()
                                        .map(|a| {
                                            a.error
                                                .as_ref()
                                                .map(ToString::to_string)
                                                .unwrap_or_else(|| a.label.clone())
                                        })
                                        .unwrap_or_default()
                                })
                            })
                            .selectable(true),
                        )
                        .child(if provider == ProviderKind::DeepSeek {
                            credentials(busy, perform).into_element()
                        } else {
                            account_list(provider, busy, perform).into_element()
                        })
                        .child(runtime(provider, busy, perform))
                },
            )),
    )
}

fn account_list(
    provider: ProviderKind,
    busy: Memo<bool>,
    perform: impl Fn(Intent) + Copy + 'static,
) -> impl IntoElement {
    let publication = use_context::<Connection>().unwrap().publication;
    view()
        .gap_3()
        .child(
            native::text_button(json!("アカウントを再読み込み"), move || {
                perform(Intent::ListAccounts(op::ListAccounts {}))
            })
            .prop("enabled", move || json!(!busy.get())),
        )
        .child(each(
            move || {
                publication.with(|p| {
                    p.settings
                        .accounts()
                        .map(|accounts| {
                            accounts
                                .accounts
                                .into_iter()
                                .filter(|a| a.provider == provider)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default()
                })
            },
            |account: &agent_protocol::operations::Account| account.id.clone(),
            move |account, _| account_row(account, busy, perform),
        ))
        .child(
            native::text_button(json!("アカウントを追加"), move || {
                perform(Intent::StartAccountLogin(op::StartAccountLogin {
                    provider,
                }))
            })
            .prop("enabled", move || json!(!busy.get())),
        )
}

fn account_row(
    account: ReadSignal<agent_protocol::operations::Account>,
    busy: Memo<bool>,
    perform: impl Fn(Intent) + Copy + 'static,
) -> impl IntoElement {
    let publication = use_context::<Connection>().unwrap().publication;
    let selected = move || {
        publication
            .with(|p| account.with(|a| p.settings.account_is_selected(a.provider, a.id.clone())))
    };
    view().gap_2()
        .child(text(move || account.with(|a| a.email.as_ref().unwrap_or(&a.id).clone())))
        .child(text(move || account.with(|a| a.plan_type.clone().unwrap_or_default())))
        .child(when(selected, move || usage(account)))
        .child(native::text_button(move || json!(if selected() { "選択中" } else { "このアカウントを使う" }), move || {
            let (provider, id) = account.with_untracked(|a| (a.provider, a.id.clone()));
            perform(Intent::SelectAccount(op::SelectAccount { provider, id }));
        }).prop("enabled", move || json!(!busy.get() && !selected())))
        .child(native::text_button(json!("サインアウト"), move || {
            let (provider, id) = account.with_untracked(|a| (a.provider, a.id.clone()));
            spawn(async move {
                if nsh_base::alert_dialog().title("サインアウトしますか？")
                    .description("この環境に保存されたアカウントからサインアウトします。再び使うにはサインインが必要です。")
                    .cancel("cancel", "キャンセル").destructive("logout", "サインアウト")
                    .show().await.is_ok_and(|answer| answer.action.as_deref() == Some("logout")) {
                    perform(Intent::LogoutAccount(op::LogoutAccount { provider, id }));
                }
            });
        }).prop("enabled", move || json!(!busy.get())))
}

fn usage(account: ReadSignal<agent_protocol::operations::Account>) -> impl IntoElement {
    view()
        .gap_2()
        .child(when(
            move || account.with(|a| a.usage.is_none()),
            || text("使用量を取得中…"),
        ))
        .child(
            text(move || {
                account.with(|a| {
                    a.usage
                        .as_ref()
                        .and_then(|u| u.error.clone())
                        .unwrap_or_default()
                })
            })
            .selectable(true),
        )
        .child(each(
            move || {
                account.with(|a| {
                    a.usage
                        .as_ref()
                        .map(|u| u.windows.clone())
                        .unwrap_or_default()
                })
            },
            |window: &agent_protocol::operations::UsageWindow| window.label.clone(),
            move |window, _| {
                view()
                    .gap_2()
                    .child(text(move || {
                        window.with(|w| format!("{} 残り {}%", w.label, w.remaining_percent))
                    }))
                    .child(
                        native("bex.progress")
                            .prop("value", move || json!(window.with(|w| w.remaining_percent)))
                            .w_full()
                            .h(px(8.0)),
                    )
                    .child(text(move || {
                        window.with(|w| {
                            w.resets_at
                                .map(|at| format!("リセット: {}", native::date_time(at)))
                                .unwrap_or_default()
                        })
                    }))
            },
        ))
        .child(text(move || {
            account.with(|a| {
                a.usage
                    .as_ref()
                    .filter(|u| u.error.is_none())
                    .map(|u| format!("{} 更新", native::date_time(u.fetched_at)))
                    .unwrap_or_default()
            })
        }))
}

fn login_panel(
    login: Memo<Option<agent_protocol::operations::AccountLogin>>,
    pending: Memo<bool>,
    perform: impl Fn(Intent) + Copy + 'static,
) -> impl IntoElement {
    let connection = use_context::<Connection>().unwrap();
    let code = signal(String::new());
    let poll_failed = signal(false);
    let retry = signal(0_u64);
    let target = computed(move || {
        (
            connection.host.get(),
            login.get(),
            lifecycle(),
            connection.settings.with(|p| p.connected()),
        )
    });
    effect(move || {
        let (host, Some(login), Lifecycle::Active, true) = target.get() else {
            return;
        };
        retry.get();
        poll_failed.set(false);
        spawn(async move {
            loop {
                timer(std::time::Duration::from_secs(2)).await;
                if pending.get_untracked() {
                    continue;
                }
                let response = connection.client.with(|client| {
                    client.request(
                        &host,
                        None,
                        Intent::ReadAccountLogin(op::ReadAccountLogin {
                            provider: login.provider,
                            id: login.login_id.clone(),
                            thread_id: None,
                        }),
                    )
                });
                if !matches!(response.recv().await, Ok(Ok(_))) {
                    poll_failed.set(true);
                    break;
                }
            }
        });
    });
    let requires_code =
        move || login.with(|l| l.as_ref().is_some_and(|l| l.requires_code_submission));
    let current = move || login.get_untracked();
    view()
        .gap_3()
        .child(text("1. ブラウザでサインイン").text_size(px(18.0)))
        .child(when(
            move || !requires_code(),
            move || {
                view()
                    .gap_2()
                    .child(text("ログインページで次のコードを入力してください。"))
                    .child(
                        text(move || {
                            login.with(|l| {
                                l.as_ref().map(|l| l.user_code.clone()).unwrap_or_default()
                            })
                        })
                        .selectable(true),
                    )
                    .child(native::text_button(json!("コピー"), move || {
                        if let Some(login) = current() {
                            set_clipboard(login.user_code);
                        }
                    }))
            },
        ))
        .child(native::text_button(
            json!("ログインページを開く"),
            move || {
                if let Some(login) = current()
                    && login.verification_url.starts_with("https://")
                {
                    native::open_link(login.verification_url);
                }
            },
        ))
        .child(when(requires_code, move || {
            view()
                .gap_2()
                .child(text("2. 認証コードを貼り付け"))
                .child(
                    input()
                        .masked(true)
                        .auto_correct(false)
                        .auto_capitalize(AutoCapitalize::None)
                        .placeholder("認証コード")
                        .value(move || code.get())
                        .on_change(move |value: &str| code.set(value.into()))
                        .text_color(move || nsh_base::theme().foreground)
                        .h(px(44.0)),
                )
                .child(
                    native::text_button(json!("サインインを完了"), move || {
                        if let Some(login) = current() {
                            let value = code.get_untracked();
                            code.set(String::new());
                            perform(Intent::SubmitAccountLogin(op::SubmitAccountLogin {
                                provider: login.provider,
                                id: login.login_id,
                                code: value,
                            }));
                        }
                    })
                    .prop("enabled", move || {
                        json!(!pending.get() && !code.with(|code| code.trim().is_empty()))
                    }),
                )
        }))
        .child(text(move || {
            if pending.get() {
                "認証を確認中…"
            } else {
                "ブラウザでの認証を待っています…"
            }
        }))
        .child(when(
            move || poll_failed.get(),
            move || {
                native::text_button(json!("認証状態を再確認"), move || {
                    retry.update(|r| *r += 1)
                })
            },
        ))
        .child(
            native::text_button(json!("サインインを中止"), move || {
                if let Some(login) = current() {
                    code.set(String::new());
                    perform(Intent::CancelAccountLogin(op::CancelAccountLogin {
                        provider: login.provider,
                        id: login.login_id,
                    }));
                }
            })
            .prop("enabled", move || json!(!pending.get())),
        )
}

fn credentials(busy: Memo<bool>, perform: impl Fn(Intent) + Copy + 'static) -> impl IntoElement {
    let connection = use_context::<Connection>().unwrap();
    let selected = signal(KeyProvider::DeepSeek);
    let key = signal(String::new());
    let control = computed(move || {
        connection.publication.with(|p| {
            p.settings
                .credential_controls()
                .into_iter()
                .find(|c| c.provider == selected.get())
        })
    });
    let enabled = move || !busy.get() && !connection.settings.with(|p| p.credential_busy());
    view().gap_3()
        .child(text("APIキー").text_size(px(18.0)))
        .child(native::menu("key", "接続先", native::items(KeyProvider::ALL.into_iter().map(|provider| (provider.reference(), provider.label()))), move |id| {
            if let Some(provider) = KeyProvider::ALL.into_iter().find(|p| p.reference() == id) { selected.set(provider); key.set(String::new()); }
        }).prop("title", move || json!(selected.get().label())).prop("enabled", move || json!(enabled())))
        .child(text(move || control.with(|c| c.as_ref().map(|c| c.status.clone()).unwrap_or_default())))
        .child(native::text_button(json!("キーの発行ページ"), move || { native::open_link(selected.get_untracked().key_url()); }))
        .child(input().masked(true).auto_correct(false).auto_capitalize(AutoCapitalize::None)
            .placeholder("APIキー").value(move || key.get()).on_change(move |value: &str| key.set(value.into()))
            .text_color(move || nsh_base::theme().foreground).h(px(44.0)))
        .child(native::text_button(json!("保存"), move || {
            let value = key.get_untracked(); key.set(String::new());
            perform(Intent::UpdateDshCredential(op::UpdateDshCredential { provider: selected.get_untracked(), key: Some(value) }));
        }).prop("enabled", move || json!(enabled() && !key.with(|key| key.trim().is_empty()))))
        .child(when(move || control.with(|c| c.as_ref().is_some_and(|c| c.can_remove)), move || {
            native::text_button(json!("保存済みキーを削除"), move || {
                let provider = selected.get_untracked();
                spawn(async move {
                    if nsh_base::alert_dialog().title("保存済みキーを削除しますか？")
                        .description("このPCにBexが保存した選択中のキーを削除します。ホストの外部設定は変更されません。")
                        .cancel("cancel", "キャンセル").destructive("remove", "削除")
                        .show().await.is_ok_and(|a| a.action.as_deref() == Some("remove")) {
                        perform(Intent::UpdateDshCredential(op::UpdateDshCredential { provider, key: None }));
                    }
                });
            }).prop("enabled", move || json!(enabled()))
        }))
        .child(native::text_button(json!("設定状態を取得"), move || perform(Intent::ReadDshCredentials(op::ReadDshCredentials {})))
            .prop("enabled", move || json!(enabled())))
        .child(text(move || connection.settings.with(|p| p.credential_error().map(|error| error.to_string()).unwrap_or_default())).selectable(true))
        .child(text("APIキーは選択したPCに保存されます。保存済みのキーは表示されません。"))
}

fn runtime(
    provider: ProviderKind,
    busy: Memo<bool>,
    perform: impl Fn(Intent) + Copy + 'static,
) -> impl IntoElement {
    let connection = use_context::<Connection>().unwrap();
    if provider != ProviderKind::Codex {
        return text("このプロバイダーはBexと一緒に更新されます。").into_element();
    }
    let status = computed(move || connection.settings.with(|p| p.codex_cli_status()));
    view()
        .gap_2()
        .child(text("Codex CLI").text_size(px(18.0)))
        .child(text(move || {
            status.with(|s| {
                s.as_ref()
                    .and_then(|s| s.version.clone())
                    .unwrap_or_default()
            })
        }))
        .child(
            text(move || {
                status.with(|s| s.as_ref().and_then(|s| s.error.clone()).unwrap_or_default())
            })
            .selectable(true),
        )
        .child(when(
            move || status.with(|s| s.as_ref().is_some_and(|s| s.restart_required)),
            || text("更新が完了しました。新しいCLIを使用するにはホストを再起動してください。"),
        ))
        .child(
            native::text_button(json!("状態を確認"), move || {
                perform(Intent::ManageCodexCli(op::ManageCodexCli { update: false }))
            })
            .prop("enabled", move || {
                json!(!busy.get() && !connection.settings.with(|p| p.codex_cli_busy()))
            }),
        )
        .child(
            native::text_button(json!("最新版に更新"), move || {
                perform(Intent::ManageCodexCli(op::ManageCodexCli { update: true }))
            })
            .prop("enabled", move || {
                json!(!busy.get() && !connection.settings.with(|p| p.codex_cli_busy()))
            }),
        )
        .into_element()
}
