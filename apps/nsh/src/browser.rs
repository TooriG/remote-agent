//! The browser's controls send displayed-page-scoped operations to the Host's browser;
//! the page itself stays the platform's web view or the Host's rendered image.
use crate::l10n::{localized, tr};
use crate::{Connection, native, shared::Shared, workbench::Workbench};
use agent_core::state::DraftKey;
use agent_protocol::{browser::*, session::SessionRef};
use base64::Engine;
use nsh_core::{AutoCapitalize, KeyboardType, element::input, prelude::*};
use serde_json::json;
use std::{sync::Arc, time::Duration};

/// The Mac's WebKit pages live in this hidden container while no viewer shows
/// them. Windows shows the Host's rendered pages and renders none itself.
#[cfg(desktop)]
pub(crate) fn host() -> AnyElement {
    #[cfg(target_os = "macos")]
    return native("bex.browser.host")
        .w(px(0.0))
        .h(px(0.0))
        .into_element();
    #[cfg(not(target_os = "macos"))]
    view().into_element()
}

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
            move || text(tr("会話を始めると、AIと同じブラウザを利用できます。")).p_4(),
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
        ready.get() && native::shown() && connection_failure.get().is_none()
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
        notice.set(importing.then(|| tr("ブラウザデータを取り込み中… 接続先のMacでOSのアクセス確認が出たら許可してください。").into()));
        true
    };
    effect(move || {
        if !native::shown() {
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
                                agent_core::browser_view::displayed_address(&result).to_owned(),
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
            .map(localized)
    });
    #[cfg(any(target_os = "macos", target_os = "ios"))]
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
    let page = Page {
        frame,
        address,
        address_editing,
        accepts_input,
        native_page,
    };
    let controls = Controls {
        send,
        open,
        toggle_keyboard: move || set_keyboard(!keyboard.get_untracked()),
        choose: move |key: &str| {
            let viewport = viewport.get_untracked();
            if let Some(action) = frame.with_untracked(|f| menu_action(key, f, viewport)) {
                send(action);
            }
        },
    };
    view()
        .flex_1()
        .min_h_0()
        .child(when(is_desktop, move || header(page, controls)))
        .child(when(
            move || !is_desktop(),
            move || address_row(page, controls),
        ))
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
                #[cfg(not(any(target_os = "macos", target_os = "ios")))]
                return drawn::page(
                    image,
                    frame,
                    accepts_input,
                    answering,
                    credentials,
                    viewport,
                    send,
                );
                #[cfg(any(target_os = "macos", target_os = "ios"))]
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
                let typing = Typing {
                    frame,
                    text: input_text,
                    composing,
                    accepts_input,
                };
                keyboard_row(typing, send, insert, move || set_keyboard(false))
            },
        ))
        .child(when(move || !is_desktop(), move || toolbar(page, controls)))
}

/// What the browser's controls read.
#[derive(Clone, Copy)]
struct Page {
    frame: Signal<Shared<BrowserFrame>>,
    address: Signal<String>,
    address_editing: Signal<bool>,
    accepts_input: Memo<bool>,
    native_page: Memo<bool>,
}

/// What the browser's controls do.
#[derive(Clone, Copy)]
struct Controls<Send, Open, Toggle, Choose> {
    send: Send,
    open: Open,
    toggle_keyboard: Toggle,
    choose: Choose,
}

/// The text the browser types into the page's selected field.
#[derive(Clone, Copy)]
struct Typing {
    frame: Signal<Shared<BrowserFrame>>,
    text: Signal<String>,
    composing: Signal<bool>,
    accepts_input: Memo<bool>,
}

/// The menu's choice as a browser operation.
fn menu_action(key: &str, frame: &BrowserFrame, viewport: (f64, f64)) -> Option<BrowserAction> {
    match key {
        "credentials" => Some(BrowserAction::Credentials),
        "new" => Some(BrowserAction::OpenTab {
            url: "about:blank".into(),
        }),
        "close" => Some(BrowserAction::CloseTab {
            id: frame.tab_id.clone(),
        }),
        "fit" => {
            agent_core::browser_view::fit_action(viewport.0, viewport.1, frame.width, frame.height)
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
    }
}

fn address_field(page: Page, open: impl Fn() + 'static) -> nsh_core::element::Input {
    let Page {
        address,
        address_editing,
        accepts_input,
        ..
    } = page;
    input()
        .test_id("browser.address")
        .auto_correct(false)
        .auto_capitalize(AutoCapitalize::None)
        .keyboard_type(KeyboardType::Url)
        .value(move || address.get())
        .placeholder(tr("検索または URL を入力"))
        .readonly(move || !accepts_input.get())
        .on_focus(move || address_editing.set(true))
        .on_blur(move || address_editing.set(false))
        .on_change(move |value: &str| address.set(value.to_owned()))
        .on_submit(move |_: &str| open())
}

/// T3 Code's preview chrome: navigation, the address with its page opened
/// in the default browser, then typing into the page and its menu.
fn header(
    page: Page,
    controls: Controls<
        impl Fn(BrowserAction) -> bool + Copy + 'static,
        impl Fn() + Copy + 'static,
        impl Fn() + Copy + 'static,
        impl Fn(&str) + Copy + 'static,
    >,
) -> impl IntoElement {
    use crate::ui::{self, Enabled, Icon, colors};
    let Page {
        frame,
        accepts_input,
        native_page,
        ..
    } = page;
    let send = controls.send;
    let external =
        computed(move || frame.with(|f| agent_core::browser_view::displayed_address(f).to_owned()));
    view()
        .flex_row()
        .items_center()
        .gap_1()
        .h(px(40.0))
        .flex_shrink_0()
        .px_2()
        .border_b_1()
        .border_b_color(colors::BORDER)
        .child(
            ui::icon_button(Icon::ARROW_LEFT, tr("前のページ"), move || {
                send(BrowserAction::Back);
            })
            .enabled(move || accepts_input.get() && frame.with(|f| f.can_go_back)),
        )
        .child(
            ui::icon_button(Icon::ARROW_RIGHT, tr("次のページ"), move || {
                send(BrowserAction::Forward);
            })
            .enabled(move || accepts_input.get() && frame.with(|f| f.can_go_forward)),
        )
        .child(
            ui::icon_button(Icon::REFRESH, tr("再読み込み"), move || {
                send(BrowserAction::Reload);
            })
            .enabled(move || accepts_input.get()),
        )
        .child(
            view()
                .flex_1()
                .min_w_0()
                .flex_row()
                .items_center()
                .h(px(28.0))
                .pl_2()
                .pr_1()
                .rounded(px(ui::RADIUS))
                .bg(colors::SURFACE)
                .child(
                    address_field(page, controls.open)
                        .label(tr("ブラウザの URL"))
                        .text_size(px(13.0))
                        .text_color(colors::FOREGROUND)
                        .placeholder_color(colors::MUTED)
                        .flex_1()
                        .min_w_0(),
                )
                .child(when(
                    move || external.with(|address| !address.is_empty()),
                    move || {
                        ui::small_icon_button(
                            Icon::EXTERNAL_LINK,
                            tr("既定のブラウザで開く"),
                            move || native::open_link(external.get_untracked()),
                        )
                    },
                )),
        )
        .child(when(
            move || !native_page.get(),
            move || {
                ui::icon_button(Icon::KEYBOARD, tr("文字入力"), controls.toggle_keyboard)
                    .enabled(move || accepts_input.get())
            },
        ))
        .child(
            ui::icon_button(
                Icon::ELLIPSIS,
                tr("ブラウザのメニュー"),
                move || ui::popup(&frame.with_untracked(|f| menu(f)), controls.choose),
            )
            .enabled(move || accepts_input.get()),
        )
}

/// The phone's address row: the field, opening it and reloading.
fn address_row(
    page: Page,
    controls: Controls<
        impl Fn(BrowserAction) -> bool + Copy + 'static,
        impl Fn() + Copy + 'static,
        impl Fn() + Copy + 'static,
        impl Fn(&str) + Copy + 'static,
    >,
) -> impl IntoElement {
    let accepts_input = page.accepts_input;
    let send = controls.send;
    view()
        .flex_row()
        .items_center()
        .gap_2()
        .p_2()
        .child(address_field(page, controls.open).flex_1())
        .child(
            native::button("arrow.right", tr("開く"), controls.open)
                .prop("identifier", json!("browser.open"))
                .prop("enabled", move || json!(accepts_input.get()))
                .w(px(44.))
                .h(px(44.)),
        )
        .child(
            native::button("arrow.clockwise", tr("再読み込み"), move || {
                send(BrowserAction::Reload);
            })
            .prop("enabled", move || json!(accepts_input.get()))
            .w(px(44.))
            .h(px(44.)),
        )
}

/// The phone's toolbar under the page.
fn toolbar(
    page: Page,
    controls: Controls<
        impl Fn(BrowserAction) -> bool + Copy + 'static,
        impl Fn() + Copy + 'static,
        impl Fn() + Copy + 'static,
        impl Fn(&str) + Copy + 'static,
    >,
) -> impl IntoElement {
    let Page {
        frame,
        accepts_input,
        native_page,
        ..
    } = page;
    let send = controls.send;
    view()
        .flex_row()
        .items_center()
        .gap_2()
        .p_2()
        .child(
            native::button("chevron.left", tr("前のページ"), move || {
                send(BrowserAction::Back);
            })
            .prop("enabled", move || {
                json!(accepts_input.get() && frame.with(|f| f.can_go_back))
            })
            .w(px(44.))
            .h(px(44.)),
        )
        .child(
            native::button("chevron.right", tr("次のページ"), move || {
                send(BrowserAction::Forward);
            })
            .prop("enabled", move || {
                json!(accepts_input.get() && frame.with(|f| f.can_go_forward))
            })
            .w(px(44.))
            .h(px(44.)),
        )
        .child(
            native::button("keyboard", tr("文字入力"), controls.toggle_keyboard)
                .prop("enabled", move || {
                    json!(accepts_input.get() && !native_page.get())
                })
                .w(px(44.))
                .h(px(44.)),
        )
        .child(
            native::menu(
                "ellipsis.circle",
                tr("ブラウザのメニュー"),
                move || frame.with(|frame| menu(frame)),
                controls.choose,
            )
            .prop("enabled", move || json!(accepts_input.get()))
            .w(px(44.))
            .h(px(44.)),
        )
}

/// Typing into the page's selected field, with the keys a field needs.
fn keyboard_row(
    typing: Typing,
    send: impl Fn(BrowserAction) -> bool + Copy + 'static,
    insert: impl Fn() + Copy + 'static,
    close: impl Fn() + 'static,
) -> impl IntoElement {
    use crate::ui::{self, Enabled, Icon};
    let Typing {
        frame,
        text: input_text,
        composing,
        accepts_input,
    } = typing;
    let key = move |label: &'static str, key: BrowserKey| {
        ui::text_button(label, move || {
            send(BrowserAction::Key { key });
        })
        .enabled(move || accepts_input.get())
    };
    view()
        .gap_2()
        .p_2()
        .child(
            view()
                .flex_row()
                .items_center()
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
                        .placeholder(tr("選択した欄に入力"))
                        .readonly(move || !accepts_input.get())
                        .on_change(move |text: &str| input_text.set(text.to_owned()))
                        .on_composition_change(move |event: &Composition| {
                            composing.set(event.composing)
                        })
                        .on_submit(move |_: &str| insert())
                        .flex_1(),
                )
                .child(ui::text_button(tr("入力"), insert).enabled(move || {
                    accepts_input.get() && !composing.get() && !input_text.get().is_empty()
                })),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .child(ui::icon_button(Icon::CLOSE, tr("入力を閉じる"), close))
                .child(key("Tab", BrowserKey::Tab))
                .child(key(tr("全選択"), BrowserKey::SelectAll))
                .child(key("⌫", BrowserKey::Backspace))
                .child(key("Enter", BrowserKey::Enter)),
        )
}

fn menu(frame: &BrowserFrame) -> serde_json::Value {
    let mut items = vec![
        json!({"id":"credentials", "title":tr("ログイン状態・保存パスワード"), "symbol":"key"}),
    ];
    items.extend(frame.tabs.iter().map(|tab| json!({"id": format!("tab:{}", tab.id), "title": agent_core::browser_view::tab_label(tab).unwrap_or(tr("新しいタブ")), "symbol":"globe", "selected": frame.tab_id == tab.id})));
    items.extend([
        json!({"id":"new", "title":tr("新しいタブ"), "symbol":"plus"}),
        json!({"id":"close", "title":tr("タブを閉じる"), "symbol":"xmark", "enabled": !frame.tab_id.is_empty()}),
        json!({"id":"fit", "title":tr("画面に合わせる"), "symbol":"arrow.up.left.and.arrow.down.right"}),
        json!({"id":"desktop", "title":"PC · 1280 × 800", "symbol":"desktopcomputer"}),
        json!({"id":"phone", "title":tr("スマートフォン · 390 × 844"), "symbol":"iphone"}),
    ]);
    json!(items)
}

/// The Host's rendered page where SwiftUI does not show it: the image fitted
/// to the panel, a click at its page coordinates, and the page's dialog. The
/// Host's credential imports are offered only on Apple platforms.
#[cfg(not(any(target_os = "macos", target_os = "ios")))]
mod drawn {
    use crate::l10n::tr;
    use crate::{shared::Shared, ui};
    use agent_core::presentation::conversation::ActionTone;
    use agent_protocol::browser::{BrowserAction, BrowserCredentials, BrowserFrame};
    use nsh_core::prelude::*;

    pub(super) fn page(
        image: Signal<String>,
        frame: Signal<Shared<BrowserFrame>>,
        accepts_input: Memo<bool>,
        answering: Signal<bool>,
        credentials: Signal<Option<BrowserCredentials>>,
        viewport: Signal<(f64, f64)>,
        send: impl Fn(BrowserAction) -> bool + Copy + 'static,
    ) -> AnyElement {
        // The page's own size, which clicks are reported in.
        let page = move || frame.with(|f| (f64::from(f.width), f64::from(f.height)));
        let scale = move || {
            let ((width, height), (shown_width, shown_height)) = (page(), viewport.get());
            if width <= 0.0 || height <= 0.0 {
                return 1.0;
            }
            (shown_width / width).min(shown_height / height).min(1.0)
        };
        let dialog = computed(move || {
            if answering.get() {
                None
            } else {
                frame.with(|f| f.dialog.clone())
            }
        });
        let reply = signal(String::new());
        effect(move || {
            if credentials.with(Option::is_some) {
                credentials.set(None);
                crate::native::message(
                    tr("ブラウザのメニュー"),
                    tr("この端末ではブラウザデータを取り込めません。"),
                );
            }
        });
        view()
            .flex_1()
            .min_h_0()
            .bg(ui::colors::SURFACE)
            .label(tr("ブラウザ"))
            .on_layout(move |bounds: &Bounds<Points>| {
                viewport.set((
                    f64::from(bounds.size.width.0),
                    f64::from(bounds.size.height.0),
                ))
            })
            .child(when(
                move || image.with(String::is_empty),
                || {
                    view()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .p_4()
                        .child(text(tr("プレビューはまだありません")).text_size(px(15.0)))
                        .child(
                            text(tr("URLを入力するか、AIにブラウザ操作を依頼してください。"))
                                .text_color(ui::colors::MUTED),
                        )
                },
            ))
            .child(when(
                move || !image.with(String::is_empty),
                move || {
                    nsh_core::element::image(move || {
                        format!("data:image/jpeg;base64,{}", image.get())
                    })
                    .w(move || px((page().0 * scale()) as f32))
                    .h(move || px((page().1 * scale()) as f32))
                    .on_press(move |event: &PressEvent| {
                        if !accepts_input.get_untracked() {
                            return;
                        }
                        let scale = scale();
                        send(BrowserAction::Click {
                            x: f64::from(event.position.x.0) / scale,
                            y: f64::from(event.position.y.0) / scale,
                        });
                    })
                },
            ))
            .child(when(
                move || dialog.with(Option::is_some),
                move || {
                    let answer = move |accept: bool| {
                        let text = if accept {
                            reply.get_untracked()
                        } else {
                            String::new()
                        };
                        reply.set(String::new());
                        send(BrowserAction::Dialog { accept, text });
                    };
                    view()
                        .absolute()
                        .left(px(16.0))
                        .right(px(16.0))
                        .top(px(16.0))
                        .gap_3()
                        .p_4()
                        .rounded(px(12.0))
                        .border_1()
                        .border_color(ui::colors::BORDER)
                        .bg(ui::colors::BACKGROUND)
                        .child(
                            text(move || {
                                dialog
                                    .with(|d| d.as_ref().map(|d| d.message.clone()))
                                    .unwrap_or_default()
                            })
                            .selectable(true),
                        )
                        .child(when(
                            move || dialog.with(|d| d.as_ref().is_some_and(|d| d.prompt)),
                            move || {
                                ui::field()
                                    .label(tr("入力"))
                                    .value(move || reply.get())
                                    .on_change(move |text: &str| reply.set(text.to_owned()))
                            },
                        ))
                        .child(
                            view()
                                .flex_row()
                                .justify_end()
                                .gap_2()
                                .child(ui::action_button(
                                    tr("キャンセル"),
                                    ActionTone::Secondary,
                                    move || answer(false),
                                ))
                                .child(ui::action_button("OK", ActionTone::Primary, move || {
                                    answer(true)
                                })),
                        )
                },
            ))
            .into_element()
    }
}

#[cfg(any(target_os = "macos", target_os = "ios", test))]
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
