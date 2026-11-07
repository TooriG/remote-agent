//! Native controls send displayed-page-scoped operations to the Host's browser.
use crate::{Connection, native, shared::Shared, workbench::Workbench};
use agent_core::state::DraftKey;
use agent_protocol::{browser::*, session::SessionRef};
use base64::Engine;
use nsh_core::{AutoCapitalize, KeyboardType, element::input, platform::Lifecycle, prelude::*};
use serde_json::json;
use std::{sync::Arc, time::Duration};

pub(crate) fn screen() -> impl IntoElement {
    let workspace = use_context::<Workbench>().unwrap();
    let thread = computed(move || match workspace.scope.get().draft {
        DraftKey::Session { session } => Some(session),
        _ => None,
    });
    view()
        .flex_1()
        .min_h_0()
        .child(when(
            move || thread.get().is_none(),
            move || text("会話を始めると、AIと同じブラウザを利用できます。").p_4(),
        ))
        .child(each(
            move || thread.get().into_iter().collect::<Vec<_>>(),
            |thread: &SessionRef| thread.to_string(),
            move |thread, _| viewer(thread.get_untracked()),
        ))
}

fn viewer(thread: SessionRef) -> impl IntoElement {
    let connection = use_context::<Connection>().unwrap();
    let host = stored(connection.host.get_untracked());
    let scope = stored(thread.to_string());
    let thread = stored(thread);
    let frame = signal(Shared(Arc::new(BrowserFrame::default())));
    let ready = signal(false);
    let address = signal(String::new());
    let address_editing = signal(false);
    let input_text = signal(String::new());
    let keyboard = signal(false);
    let composing = signal(false);
    let set_keyboard = move |open| {
        keyboard.set(open);
        if !open {
            composing.set(false);
            dismiss_keyboard();
        }
    };
    let image = signal(String::new());
    let credentials = signal(None::<BrowserCredentials>);
    let answering = signal(false);
    let failure = signal(None::<String>);
    let connection_failure = signal(None::<String>);
    let notice = signal(None::<String>);
    let viewport = signal((0., 0.));
    let renderer = computed(move || {
        if host.with(|h| h == "local") {
            connection
                .state
                .with(|state| state.browser_renderer.clone())
        } else {
            String::new()
        }
    });
    let accepts_input = computed(move || {
        ready.get() && lifecycle() == Lifecycle::Active && connection_failure.get().is_none()
    });
    let request = move |action| {
        frame.with_untracked(|frame| BrowserRequest {
            thread_id: thread.get(),
            tab_id: frame.tab_id.clone(),
            image_id: frame.image_id.clone(),
            generation: frame.generation,
            renderer_id: renderer.get_untracked(),
            action,
        })
    };
    let (outgoing, incoming) = async_channel::bounded(64);
    let outgoing = stored(outgoing);
    let incoming = stored(incoming);
    let send = move |action: BrowserAction| {
        if !accepts_input.get_untracked() {
            return false;
        }
        let dialog = matches!(action, BrowserAction::Dialog { .. });
        let importing = matches!(
            action,
            BrowserAction::ImportCookies { .. } | BrowserAction::ImportPasswords { .. }
        );
        if outgoing.with(|queue| queue.try_send(request(action)).is_err()) {
            return false;
        }
        if dialog {
            answering.set(true);
        }
        credentials.set(None);
        failure.set(None);
        notice.set(importing.then(|| "ブラウザデータを取り込み中… 接続先のMacでOSのアクセス確認が出たら許可してください。".into()));
        true
    };
    effect(move || {
        if lifecycle() != Lifecycle::Active {
            while incoming.with(|queue| queue.try_recv().is_ok()) {}
            answering.set(false);
            set_keyboard(false);
            return;
        }
        spawn(async move {
            loop {
                let request = incoming
                    .with(|queue| queue.try_recv())
                    .unwrap_or_else(|_| request(BrowserAction::Read));
                let action = request.action.clone();
                let reply = connection
                    .client
                    .with(|client| client.browser(host.get(), request));
                match reply.recv().await {
                    Ok(Ok(mut result)) => {
                        if frame.with_untracked(|f| f.input_kind) != result.input_kind {
                            composing.set(false);
                            input_text.set(String::new());
                        }
                        if let Some(focus) =
                            agent_core::browser_view::input_focus(&action, result.input_kind)
                        {
                            set_keyboard(focus);
                        }
                        if result.tab_id.is_empty() || result.image_id.is_empty() {
                            image.set(String::new());
                        }
                        if !result.image.is_empty() {
                            image.set(
                                base64::engine::general_purpose::STANDARD.encode(&result.image),
                            );
                        }
                        result.image.clear();
                        if let Some(metadata) = result.credentials.take() {
                            credentials.set(Some(metadata));
                        }
                        if !result.value.is_empty() {
                            notice.set(Some(result.value.clone()));
                        }
                        if !address_editing.get_untracked() {
                            address.set(
                                result
                                    .tabs
                                    .iter()
                                    .find(|t| t.id == result.tab_id)
                                    .map(|t| {
                                        if t.url == "about:blank" {
                                            String::new()
                                        } else {
                                            t.url.clone()
                                        }
                                    })
                                    .unwrap_or_default(),
                            );
                        }
                        frame.set(Shared(Arc::new(result)));
                        ready.set(true);
                        connection_failure.set(None);
                    }
                    Ok(Err(reason)) => {
                        if matches!(action, BrowserAction::Read) {
                            connection_failure.set(Some(reason.to_string()));
                        } else {
                            failure.set(Some(reason.to_string()));
                        }
                    }
                    Err(_) => return,
                }
                if matches!(action, BrowserAction::Dialog { .. }) {
                    answering.set(false);
                }
                timer(Duration::from_millis(
                    if connection_failure.get_untracked().is_some() {
                        1500
                    } else {
                        150
                    },
                ))
                .await;
            }
        });
    });
    let open = move || match agent_core::browser_view::address_url(&address.get_untracked()) {
        Ok(url) => {
            address_editing.set(false);
            dismiss_keyboard();
            send(BrowserAction::Navigate { url });
        }
        Err(reason) => failure.set(Some(reason)),
    };
    let insert = move || {
        let text = input_text.get_untracked();
        if !composing.get_untracked() && !text.is_empty() && send(BrowserAction::Type { text }) {
            input_text.set(String::new());
        }
    };
    let native_page = computed(move || {
        let local = renderer.get();
        !local.is_empty()
            && credentials.get().is_none()
            && frame.with(|f| f.renderer_id == local && !f.tab_id.is_empty() && f.dialog.is_none())
    });
    let message = computed(move || {
        failure
            .get()
            .or_else(|| connection_failure.get())
            .or_else(|| notice.get())
    });
    let native_action = move |event: &NativeEvent| {
        if event.data.get("DismissCredentials").is_some() {
            credentials.set(None);
            return;
        }
        if let Some(size) = event.data.get("ViewportSize") {
            if let (Some(width), Some(height)) = (size["width"].as_f64(), size["height"].as_f64()) {
                viewport.set((width, height));
            }
            return;
        }
        if let Ok(action) = serde_json::from_value::<BrowserAction>(event.data.clone()) {
            send(action);
        }
    };
    view()
        .flex_1()
        .min_h_0()
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .p_2()
                .child(
                    input()
                        .test_id("browser.address")
                        .auto_correct(false)
                        .auto_capitalize(AutoCapitalize::None)
                        .keyboard_type(KeyboardType::Url)
                        .value(move || address.get())
                        .placeholder("検索または URL を入力")
                        .readonly(move || !accepts_input.get())
                        .on_focus(move || address_editing.set(true))
                        .on_blur(move || address_editing.set(false))
                        .on_change(move |value: &str| address.set(value.to_owned()))
                        .on_submit(move |_: &str| open())
                        .flex_1(),
                )
                .child(
                    native::button("arrow.right", "開く", open)
                        .prop("identifier", json!("browser.open"))
                        .prop("enabled", move || json!(accepts_input.get()))
                        .w(px(44.))
                        .h(px(44.)),
                )
                .child(
                    native::button("arrow.clockwise", "再読み込み", move || {
                        send(BrowserAction::Reload);
                    })
                    .prop("enabled", move || json!(accepts_input.get()))
                    .w(px(44.))
                    .h(px(44.)),
                ),
        )
        .child(when(
            move || message.get().is_some(),
            move || {
                text(move || message.get().unwrap_or_default())
                    .selectable(true)
                    .text_size(px(12.))
                    .p_2()
            },
        ))
        .child(when(
            move || !native_page.get(),
            move || {
                native("bex.browser.remote")
                    .prop("data", move || {
                        browser_data(
                            &image.get(),
                            accepts_input.get(),
                            if answering.get() {
                                None
                            } else {
                                frame.with(|f| f.dialog.clone())
                            }
                            .as_ref(),
                            credentials.get().as_ref(),
                        )
                    })
                    .on_native_event(native_action)
                    .flex_1()
                    .min_h_0()
            },
        ))
        .child(when(
            move || native_page.get(),
            move || {
                native("bex.browser.native")
                    .prop("scope", move || json!(scope.get()))
                    .prop("tab", move || json!(frame.with(|f| f.tab_id.clone())))
                    .prop("renderer", move || {
                        json!(frame.with(|f| f.renderer_id.clone()))
                    })
                    .on_native_event(move |event: &NativeEvent| {
                        if &*event.name == "size"
                            && let (Some(w), Some(h)) =
                                (event.data["width"].as_f64(), event.data["height"].as_f64())
                        {
                            viewport.set((w, h));
                        }
                    })
                    .flex_1()
                    .min_h_0()
            },
        ))
        .child(when(
            move || keyboard.get() && !native_page.get(),
            move || {
                view()
                    .gap_2()
                    .p_2()
                    .child(
                        view()
                            .flex_row()
                            .gap_2()
                            .child(
                                input()
                                    .test_id("browser.text")
                                    .auto_focus(true)
                                    .auto_correct(false)
                                    .auto_capitalize(AutoCapitalize::None)
                                    .value(move || input_text.get())
                                    .masked(move || {
                                        frame.with(|f| f.input_kind != Some(BrowserInputKind::Text))
                                    })
                                    .placeholder("選択した欄に入力")
                                    .readonly(move || !accepts_input.get())
                                    .on_change(move |text: &str| input_text.set(text.to_owned()))
                                    .on_composition_change(move |event: &Composition| {
                                        composing.set(event.composing)
                                    })
                                    .on_submit(move |_: &str| insert())
                                    .flex_1(),
                            )
                            .child(native::text_button(json!("入力"), insert).prop(
                                "enabled",
                                move || {
                                    json!(
                                        accepts_input.get()
                                            && !composing.get()
                                            && !input_text.get().is_empty()
                                    )
                                },
                            )),
                    )
                    .child(
                        view()
                            .flex_row()
                            .gap_2()
                            .child(
                                native::button(
                                    "keyboard.chevron.compact.down",
                                    "入力を閉じる",
                                    move || set_keyboard(false),
                                )
                                .w(px(44.))
                                .h(px(44.)),
                            )
                            .child(native::text_button(json!("Tab"), move || {
                                send(BrowserAction::Key {
                                    key: BrowserKey::Tab,
                                });
                            }))
                            .child(native::text_button(json!("全選択"), move || {
                                send(BrowserAction::Key {
                                    key: BrowserKey::SelectAll,
                                });
                            }))
                            .child(native::text_button(json!("⌫"), move || {
                                send(BrowserAction::Key {
                                    key: BrowserKey::Backspace,
                                });
                            }))
                            .child(native::text_button(json!("Enter"), move || {
                                send(BrowserAction::Key {
                                    key: BrowserKey::Enter,
                                });
                            })),
                    )
            },
        ))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .p_2()
                .child(
                    native::button("chevron.left", "前のページ", move || {
                        send(BrowserAction::Back);
                    })
                    .prop("enabled", move || {
                        json!(accepts_input.get() && frame.with(|f| f.can_go_back))
                    })
                    .w(px(44.))
                    .h(px(44.)),
                )
                .child(
                    native::button("chevron.right", "次のページ", move || {
                        send(BrowserAction::Forward);
                    })
                    .prop("enabled", move || {
                        json!(accepts_input.get() && frame.with(|f| f.can_go_forward))
                    })
                    .w(px(44.))
                    .h(px(44.)),
                )
                .child(
                    native::button("keyboard", "文字入力", move || {
                        set_keyboard(!keyboard.get_untracked())
                    })
                    .prop("enabled", move || {
                        json!(accepts_input.get() && !native_page.get())
                    })
                    .w(px(44.))
                    .h(px(44.)),
                )
                .child(
                    native::menu(
                        "ellipsis.circle",
                        "ブラウザのメニュー",
                        move || frame.with(|frame| menu(frame)),
                        move |key| {
                            let action = match key {
                                "credentials" => Some(BrowserAction::Credentials),
                                "new" => Some(BrowserAction::OpenTab {
                                    url: "about:blank".into(),
                                }),
                                "close" => Some(BrowserAction::CloseTab {
                                    id: frame.with_untracked(|f| f.tab_id.clone()),
                                }),
                                "fit" => {
                                    let (w, h) = viewport.get_untracked();
                                    frame.with_untracked(|f| {
                                        agent_core::browser_view::fit_action(
                                            w, h, f.width, f.height,
                                        )
                                    })
                                }
                                "desktop" => Some(BrowserAction::Resize {
                                    width: 1280,
                                    height: 800,
                                }),
                                "phone" => Some(BrowserAction::Resize {
                                    width: 390,
                                    height: 844,
                                }),
                                key => key
                                    .strip_prefix("tab:")
                                    .map(|id| BrowserAction::SelectTab { id: id.into() }),
                            };
                            if let Some(action) = action {
                                send(action);
                            }
                        },
                    )
                    .prop("enabled", move || json!(accepts_input.get()))
                    .w(px(44.))
                    .h(px(44.)),
                ),
        )
}

fn menu(frame: &BrowserFrame) -> serde_json::Value {
    let mut items =
        vec![json!({"id":"credentials", "title":"ログイン状態・保存パスワード", "symbol":"key"})];
    items.extend(frame.tabs.iter().map(|tab| json!({"id": format!("tab:{}", tab.id), "title": if tab.url == "about:blank" { "新しいタブ" } else if tab.title.is_empty() { &tab.url } else { &tab.title }, "symbol":"globe", "selected": frame.tab_id == tab.id})));
    items.extend([
        json!({"id":"new", "title":"新しいタブ", "symbol":"plus"}),
        json!({"id":"close", "title":"タブを閉じる", "symbol":"xmark", "enabled": !frame.tab_id.is_empty()}),
        json!({"id":"fit", "title":"画面に合わせる", "symbol":"arrow.up.left.and.arrow.down.right"}),
        json!({"id":"desktop", "title":"PC · 1280 × 800", "symbol":"desktopcomputer"}),
        json!({"id":"phone", "title":"スマートフォン · 390 × 844", "symbol":"iphone"}),
    ]);
    json!(items)
}

fn browser_data(
    image: &str,
    enabled: bool,
    dialog: Option<&BrowserDialog>,
    credentials: Option<&BrowserCredentials>,
) -> serde_json::Value {
    json!({"image": image, "enabled": enabled, "dialog": dialog, "credentials": credentials})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_display_contract_preserves_dialog_and_host_credential_metadata() {
        let credentials = BrowserCredentials {
            profiles: vec![BrowserImportProfile {
                id: "safari.test".into(),
                label: "検証プロファイル 👋".into(),
            }],
            import_errors: vec![BrowserImportError {
                browser: "Fixture browser".into(),
                message: "権限を確認してください。".into(),
            }],
            origin: Some("https://example.invalid".into()),
            accounts: vec!["検証ユーザー".into()],
            password_error: None,
        };
        let dialog = BrowserDialog {
            message: "入力してください 👋".into(),
            prompt: true,
        };
        let expected: serde_json::Value = serde_json::from_str(include_str!(
            "../apple/Tests/BexNshNativeTests/Fixtures/browser.json"
        ))
        .unwrap();
        assert_eq!(
            browser_data("", true, Some(&dialog), Some(&credentials)),
            expected
        );
    }
}
