use crate::{Runtime, appearance};
use agent_core::store::Store;
use agent_protocol::{
    browser::{BrowserAction, BrowserCredentials, BrowserFrame, BrowserKey, BrowserRequest},
    session::SessionRef,
};
use gpui_kit::{
    component::{
        Disableable, Icon, IconName, Sizable,
        button::{Button, ButtonVariants},
        h_flex,
        input::{Input, InputEvent, InputState},
        menu::{DropdownMenu, PopupMenuItem},
        v_flex,
    },
    prelude::FluentBuilder,
    *,
};
use std::{collections::VecDeque, ops::Range, sync::Arc, time::Duration};

pub(crate) struct Browser {
    store: Option<Arc<Store>>,
    thread: Option<SessionRef>,
    runtime: Runtime,
    frame: BrowserFrame,
    image: Option<Arc<RenderImage>>,
    address: Entity<InputState>,
    dialog_input: Entity<InputState>,
    error: Option<String>,
    connection_error: Option<String>,
    notice: Option<String>,
    credentials: Option<BrowserCredentials>,
    pending: VecDeque<BrowserRequest>,
    visible: bool,
    focus: FocusHandle,
    bounds: Bounds<Pixels>,
    composition: String,
    composition_selection: Range<usize>,
    _poll: Task<()>,
    initial_url: Option<String>,
    _subscription: Subscription,
}

impl Browser {
    pub(crate) fn new(
        store: Option<Arc<Store>>,
        thread: Option<SessionRef>,
        url: Option<String>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        let runtime = cx.global::<Runtime>().clone();
        let address = cx.new(|cx| InputState::new(window, cx).placeholder("検索または URL を入力"));
        let dialog_input = cx.new(|cx| InputState::new(window, cx));
        let focus = cx.focus_handle();
        cx.new(|cx: &mut Context<Self>| {
            let subscription = cx.subscribe_in(&address, window, |view, _, event, _, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    view.navigate(cx);
                }
            });
            let poll = cx.spawn_in(window, async move |view, cx| {
                loop {
                    let Ok(job) = view.update_in(cx, |view, _, _| view.next_request()) else {
                        break;
                    };
                    if let Some((runtime, store, request)) = job {
                        let reading = matches!(request.action, BrowserAction::Read);
                        let result = runtime
                            .handle
                            .spawn(async move {
                                let frame = store
                                    .browser(request)
                                    .await
                                    .map_err(|error| error.to_string())?;
                                let image = if frame.image.is_empty() {
                                    None
                                } else {
                                    let mut rgba = image::load_from_memory(&frame.image)
                                        .map_err(|_| "ブラウザ画像を読み取れません。")?
                                        .into_rgba8();
                                    for pixel in rgba.pixels_mut() {
                                        pixel.0.swap(0, 2);
                                    }
                                    Some(Arc::new(RenderImage::new(vec![image::Frame::new(rgba)])))
                                };
                                Ok::<_, String>((frame, image))
                            })
                            .await;
                        let result =
                            result.unwrap_or_else(|_| Err("ブラウザとの接続が切れました。".into()));
                        if view
                            .update_in(cx, |view, window, cx| {
                                match result {
                                    Ok((mut frame, image)) => {
                                        if !view
                                            .address
                                            .read(cx)
                                            .focus_handle(cx)
                                            .is_focused(window)
                                        {
                                            let url = frame
                                                .tabs
                                                .iter()
                                                .find(|tab| tab.id == frame.tab_id)
                                                .map(|tab| {
                                                    if tab.url == "about:blank" {
                                                        String::new()
                                                    } else {
                                                        tab.url.clone()
                                                    }
                                                })
                                                .unwrap_or_default();
                                            view.address.update(cx, |input, cx| {
                                                input.set_value(url, window, cx)
                                            });
                                        }
                                        if image.is_some() {
                                            view.image = image;
                                        }
                                        if let Some(metadata) = frame.credentials.take() {
                                            view.credentials = Some(metadata);
                                        }
                                        if !frame.value.is_empty() {
                                            view.notice = Some(std::mem::take(&mut frame.value));
                                        }
                                        view.frame = frame;
                                        if let Some(url) = view.initial_url.take() {
                                            view.send(BrowserAction::Navigate { url }, cx);
                                        }
                                        view.connection_error = None;
                                    }
                                    Err(error) => {
                                        if reading {
                                            view.connection_error = Some(error);
                                        } else {
                                            view.error = Some(error);
                                            view.notice = None;
                                        }
                                    }
                                }
                                cx.notify();
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    cx.background_executor()
                        .timer(Duration::from_millis(150))
                        .await;
                }
            });
            Self {
                store,
                thread,
                runtime,
                frame: BrowserFrame::default(),
                image: None,
                address,
                dialog_input,
                error: None,
                connection_error: None,
                notice: None,
                credentials: None,
                pending: VecDeque::new(),
                visible: false,
                focus,
                bounds: Bounds::default(),
                composition: String::new(),
                composition_selection: 0..0,
                _poll: poll,
                initial_url: url,
                _subscription: subscription,
            }
        })
    }

    fn next_request(&mut self) -> Option<(Runtime, Arc<Store>, BrowserRequest)> {
        let store = self.store.clone()?;
        self.thread.as_ref()?;
        let request = self
            .pending
            .pop_front()
            .or_else(|| self.visible.then(|| self.request(BrowserAction::Read)))?;
        Some((self.runtime.clone(), store, request))
    }

    fn request(&self, action: BrowserAction) -> BrowserRequest {
        BrowserRequest {
            thread_id: self
                .thread
                .clone()
                .expect("a shared browser has a conversation"),
            generation: self.frame.generation,
            tab_id: self.frame.tab_id.clone(),
            image_id: self.frame.image_id.clone(),
            action,
        }
    }

    fn send(&mut self, action: BrowserAction, cx: &mut Context<Self>) {
        self.error = None;
        self.notice = None;
        if matches!(
            action,
            BrowserAction::ImportCookies { .. } | BrowserAction::ImportPasswords { .. }
        ) {
            self.notice = Some(
                "ブラウザデータを取り込み中… OSのアクセス確認が出たら許可してください。".into(),
            );
        }
        self.credentials = None;
        if self.thread.is_some() && !self.frame.tab_id.is_empty() && self.pending.len() < 64 {
            self.pending.push_back(self.request(action));
        }
        cx.notify();
    }

    fn navigate(&mut self, cx: &mut Context<Self>) {
        match agent_core::browser_view::address_url(self.address.read(cx).value().as_ref()) {
            Ok(url) => self.send(BrowserAction::Navigate { url }, cx),
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }

    fn actions_menu(&self, cx: &Context<Self>) -> impl IntoElement {
        let owner = cx.entity().downgrade();
        let tab_id = self.frame.tab_id.clone();
        let url = self
            .frame
            .tabs
            .iter()
            .find(|tab| tab.id == tab_id)
            .map(|tab| tab.url.clone());
        Button::new("browser-actions")
            .icon(IconName::Ellipsis)
            .xsmall()
            .ghost()
            .accessibility_label("ブラウザのメニュー")
            .disabled(self.frame.tab_id.is_empty())
            .dropdown_menu(move |menu, _, _| {
                let new_tab = owner.clone();
                let close_tab = owner.clone();
                let fit = owner.clone();
                let desktop = owner.clone();
                let mobile = owner.clone();
                let credentials = owner.clone();
                let tab_id = tab_id.clone();
                let url = url.clone();
                menu.item(PopupMenuItem::new("ログイン状態・保存パスワード").on_click(
                    move |_, _, cx| {
                        let _ = credentials
                            .update(cx, |view, cx| view.send(BrowserAction::Credentials, cx));
                    },
                ))
                .separator()
                .item(PopupMenuItem::new("新しいタブ").on_click(move |_, _, cx| {
                    let _ = new_tab.update(cx, |view, cx| {
                        view.send(
                            BrowserAction::OpenTab {
                                url: "about:blank".into(),
                            },
                            cx,
                        )
                    });
                }))
                .item(
                    PopupMenuItem::new("タブを閉じる").on_click(move |_, _, cx| {
                        let _ = close_tab.update(cx, |view, cx| {
                            view.send(BrowserAction::CloseTab { id: tab_id.clone() }, cx)
                        });
                    }),
                )
                .separator()
                .item(
                    PopupMenuItem::new("パネルに合わせる").on_click(move |_, _, cx| {
                        let _ = fit.update(cx, |view, cx| {
                            if let Some(action) = agent_core::browser_view::fit_action(
                                f64::from(view.bounds.size.width / px(1.)),
                                f64::from(view.bounds.size.height / px(1.)),
                                view.frame.width,
                                view.frame.height,
                            ) {
                                view.send(action, cx);
                            }
                        });
                    }),
                )
                .item(
                    PopupMenuItem::new("デスクトップ · 1280 × 800").on_click(move |_, _, cx| {
                        let _ = desktop.update(cx, |view, cx| {
                            view.send(
                                BrowserAction::Resize {
                                    width: 1280,
                                    height: 800,
                                },
                                cx,
                            )
                        });
                    }),
                )
                .item(
                    PopupMenuItem::new("モバイル · 390 × 844").on_click(move |_, _, cx| {
                        let _ = mobile.update(cx, |view, cx| {
                            view.send(
                                BrowserAction::Resize {
                                    width: 390,
                                    height: 844,
                                },
                                cx,
                            )
                        });
                    }),
                )
                .separator()
                .item(
                    PopupMenuItem::new("URL をコピー")
                        .disabled(url.is_none())
                        .on_click(move |_, _, cx| {
                            if let Some(url) = &url {
                                cx.write_to_clipboard(ClipboardItem::new_string(url.clone()));
                            }
                        }),
                )
            })
    }

    fn credentials_panel(
        &self,
        metadata: BrowserCredentials,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut panel = v_flex()
            .id("browser-credentials")
            .max_h(px(360.))
            .overflow_y_scroll()
            .p_3()
            .gap_2()
            .border_b_1()
            .border_color(rgb(0x202020))
            .child(
                h_flex().justify_between().child("ブラウザデータ").child(
                    Button::new("close-browser-data")
                        .icon(IconName::Close)
                        .ghost()
                        .xsmall()
                        .accessibility_label("ブラウザデータを閉じる")
                        .on_click(cx.listener(|view, _, _, cx| {
                            view.credentials = None;
                            cx.notify();
                        })),
                ),
            )
            .child(div().text_sm().child("Hostのブラウザから取り込む"));
        if metadata.profiles.is_empty() {
            panel = panel.child(
                div()
                    .text_sm()
                    .child("取り込めるブラウザプロファイルがありません。"),
            );
        }
        for (index, profile) in metadata.profiles.into_iter().enumerate() {
            let cookies = profile.id.clone();
            let passwords = profile.id;
            panel = panel.child(
                v_flex().gap_1().child(profile.label).child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new(SharedString::from(format!("import-cookies-{index}")))
                                .label("ログイン状態（Cookie）")
                                .small()
                                .ghost()
                                .disabled(cookies.is_empty())
                                .on_click(cx.listener(move |view, _, _, cx| {
                                    view.send(
                                        BrowserAction::ImportCookies {
                                            profile: cookies.clone(),
                                        },
                                        cx,
                                    )
                                })),
                        )
                        .child(
                            Button::new(SharedString::from(format!("import-passwords-{index}")))
                                .label("保存パスワード → Keychain")
                                .small()
                                .ghost()
                                .disabled(passwords.is_empty())
                                .on_click(cx.listener(move |view, _, _, cx| {
                                    view.send(
                                        BrowserAction::ImportPasswords {
                                            profile: passwords.clone(),
                                        },
                                        cx,
                                    )
                                })),
                        ),
                ),
            );
        }
        if let Some(origin) = metadata.origin {
            panel = panel.child(
                div()
                    .mt_2()
                    .text_sm()
                    .child(format!("保存パスワード · {origin}")),
            );
            if metadata.accounts.is_empty() {
                panel = panel.child(
                    div().text_sm().child(
                        metadata
                            .password_error
                            .unwrap_or_else(|| "このサイトの保存パスワードはありません。".into()),
                    ),
                );
            }
            for (index, username) in metadata.accounts.iter().enumerate() {
                let origin = origin.clone();
                let username = username.clone();
                panel = panel.child(
                    Button::new(SharedString::from(format!("fill-password-{index}")))
                        .label(if username.is_empty() {
                            "ユーザー名なし".into()
                        } else {
                            username.clone()
                        })
                        .small()
                        .ghost()
                        .on_click(cx.listener(move |view, _, _, cx| {
                            view.send(
                                BrowserAction::FillPassword {
                                    origin: origin.clone(),
                                    username: username.clone(),
                                },
                                cx,
                            )
                        })),
                );
            }
            panel = panel.child(
                Button::new("delete-site-passwords")
                    .label("このサイトの保存パスワードを削除")
                    .small()
                    .ghost()
                    .disabled(metadata.accounts.is_empty())
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.send(
                            BrowserAction::DeletePasswords {
                                origin: origin.clone(),
                            },
                            cx,
                        )
                    })),
            );
        }
        panel
    }

    pub(crate) fn title(&self) -> String {
        self.frame
            .tabs
            .iter()
            .find(|tab| tab.id == self.frame.tab_id)
            .map(|tab| {
                if tab.title.is_empty() || tab.url == "about:blank" {
                    "ブラウザ".into()
                } else {
                    tab.title.clone()
                }
            })
            .unwrap_or_else(|| "ブラウザ".into())
    }

    pub(crate) fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    fn page_point(&self, position: Point<Pixels>) -> Option<(f64, f64)> {
        let width = self.frame.width as f32;
        let height = self.frame.height as f32;
        if width == 0. || height == 0. {
            return None;
        }
        let scale = (self.bounds.size.width / px(width)).min(self.bounds.size.height / px(height));
        let x = ((position.x - self.bounds.left()) / px(1.)
            - (self.bounds.size.width / px(1.) - width * scale) / 2.)
            / scale;
        let y = ((position.y - self.bounds.top()) / px(1.)
            - (self.bounds.size.height / px(1.) - height * scale) / 2.)
            / scale;
        ((0.0..width).contains(&x) && (0.0..height).contains(&y))
            .then_some((f64::from(x), f64::from(y)))
    }

    fn key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        let action = match key.key.as_str() {
            "enter" => Some(BrowserKey::Enter),
            "tab" => Some(BrowserKey::Tab),
            "backspace" => Some(BrowserKey::Backspace),
            "escape" => Some(BrowserKey::Escape),
            "up" => Some(BrowserKey::ArrowUp),
            "down" => Some(BrowserKey::ArrowDown),
            "left" => Some(BrowserKey::ArrowLeft),
            "right" => Some(BrowserKey::ArrowRight),
            "delete" => Some(BrowserKey::Delete),
            "home" => Some(BrowserKey::Home),
            "end" => Some(BrowserKey::End),
            "pageup" => Some(BrowserKey::PageUp),
            "pagedown" => Some(BrowserKey::PageDown),
            "a" if key.modifiers.platform || key.modifiers.control => Some(BrowserKey::SelectAll),
            _ => None,
        };
        if let Some(key) = action {
            self.send(BrowserAction::Key { key }, cx);
            cx.stop_propagation();
        } else if key.key == "v" && (key.modifiers.platform || key.modifiers.control) {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                self.send(BrowserAction::Type { text }, cx);
            }
            cx.stop_propagation();
        }
    }
}

impl Render for Browser {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let enabled = !self.frame.tab_id.is_empty();
        let tabs = self.frame.tabs.clone();
        let active = self.frame.tab_id.clone();
        let entity = cx.entity();
        v_flex()
            .size_full()
            .min_h_0()
            .bg(rgb(appearance::BACKGROUND))
            .map(|body| {
                body.when(tabs.len() > 1, |body| body.child(
                    h_flex()
                        .id("browser-tabs")
                        .h_8()
                        .px_2()
                        .gap_1()
                        .overflow_x_scroll()
                        .children(tabs.into_iter().map(|tab| {
                            let id = tab.id.clone();
                            let close = id.clone();
                            h_flex()
                                .gap_1()
                                .rounded_md()
                                .when(tab.id == active, |tab| tab.bg(rgb(appearance::RAISED)))
                                .child(
                                    Button::new(SharedString::from(format!("browser-tab-{id}")))
                                        .label(
                                            if tab.title.is_empty() || tab.url == "about:blank" {
                                                "新しいタブ".into()
                                            } else {
                                                tab.title
                                            },
                                        )
                                        .small()
                                        .ghost()
                                        .on_click(cx.listener(move |view, _, _, cx| {
                                            view.send(
                                                BrowserAction::SelectTab { id: id.clone() },
                                                cx,
                                            )
                                        })),
                                )
                                .child(
                                    Button::new(SharedString::from(format!(
                                        "browser-close-{close}"
                                    )))
                                    .icon(IconName::Close)
                                    .small()
                                    .ghost()
                                    .accessibility_label("タブを閉じる")
                                    .on_click(cx.listener(move |view, _, _, cx| {
                                        view.send(BrowserAction::CloseTab { id: close.clone() }, cx)
                                    })),
                                )
                        }))
                        .child(
                            Button::new("browser-new-tab")
                                .icon(IconName::Plus)
                                .small()
                                .ghost()
                                .accessibility_label("新しいタブ")
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.send(
                                        BrowserAction::OpenTab {
                                            url: "about:blank".into(),
                                        },
                                        cx,
                                    )
                                })),
                        ),
                ))
                .child(
                    h_flex()
                        .h_10()
                        .px_2()
                        .gap_1()
                        .border_b_1()
                        .border_color(rgb(0x202020))
                        .child(
                            Button::new("browser-back")
                                .icon(IconName::ArrowLeft)
                                .xsmall()
                                .ghost()
                                .disabled(!self.frame.can_go_back)
                                .accessibility_label("前のページ")
                                .on_click(
                                    cx.listener(|view, _, _, cx| {
                                        view.send(BrowserAction::Back, cx)
                                    }),
                                ),
                        )
                        .child(
                            Button::new("browser-forward")
                                .icon(IconName::ArrowRight)
                                .xsmall()
                                .ghost()
                                .disabled(!self.frame.can_go_forward)
                                .accessibility_label("次のページ")
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.send(BrowserAction::Forward, cx)
                                })),
                        )
                        .child(
                            Button::new("browser-reload")
                                .icon(IconName::RotateCw)
                                .xsmall()
                                .ghost()
                                .disabled(!enabled)
                                .accessibility_label("再読み込み")
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.send(BrowserAction::Reload, cx)
                                })),
                        )
                        .child(
                            div().flex_1().min_w_0().child(
                                Input::new(&self.address)
                                    .small()
                                    .appearance(false)
                                    .disabled(!enabled)
                                    .aria_label("ブラウザの URL"),
                            ),
                        )
                        .child(self.actions_menu(cx)),
                )
                .when_some(self.credentials.clone(), |body, metadata| body.child(self.credentials_panel(metadata,cx)))
                .when_some(self.notice.clone(), |body, notice| body.child(div().px_3().py_2().text_sm().child(notice)))
                .when_some(self.error.clone().or(self.connection_error.clone()), |body, error| {
                    body.child(div().px_3().py_2().text_sm().child(error))
                })
                .when_some(self.frame.dialog.clone(), |body, dialog| {
                    body.child(
                        v_flex()
                            .p_3()
                            .gap_2()
                            .child(dialog.message)
                            .when(dialog.prompt, |body| {
                                body.child(Input::new(&self.dialog_input).small())
                            })
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(
                                        Button::new("browser-dialog-cancel")
                                            .label("キャンセル")
                                            .small()
                                            .on_click(cx.listener(|view, _, _, cx| {
                                                view.send(
                                                    BrowserAction::Dialog {
                                                        accept: false,
                                                        text: String::new(),
                                                    },
                                                    cx,
                                                )
                                            })),
                                    )
                                    .child(
                                        Button::new("browser-dialog-ok")
                                            .label("OK")
                                            .small()
                                            .on_click(cx.listener(|view, _, _, cx| {
                                                let text =
                                                    view.dialog_input.read(cx).value().to_string();
                                                view.send(
                                                    BrowserAction::Dialog { accept: true, text },
                                                    cx,
                                                );
                                            })),
                                    ),
                            ),
                    )
                })
                .child(
                    div()
                        .id("shared-browser-content")
                        .relative()
                        .flex_1()
                        .min_h_0()
                        .overflow_hidden()
                        .track_focus(&self.focus)
                        .on_key_down(cx.listener(Self::key))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|view, event: &MouseDownEvent, window, cx| {
                                view.focus.focus(window, cx);
                                if let Some((x, y)) = view.page_point(event.position)
                                {
                                    view.send(BrowserAction::Click { x, y }, cx);
                                }
                            }),
                        )
                        .on_scroll_wheel(cx.listener(|view, event: &ScrollWheelEvent, _, cx| {
                            if let Some((x, y)) = view.page_point(event.position)
                            {
                                let delta = event.delta.pixel_delta(px(16.));
                                view.send(
                                    BrowserAction::Scroll {
                                        x,
                                        y,
                                        delta_x: f64::from(
                                            (-delta.x / px(1.)).clamp(-4096., 4096.),
                                        ),
                                        delta_y: f64::from(
                                            (-delta.y / px(1.)).clamp(-4096., 4096.),
                                        ),
                                    },
                                    cx,
                                );
                            }
                        }))
                        .map(|body| {
                            if let Some(image) = self.image.clone().filter(|_| {
                                self.frame
                                    .tabs
                                    .iter()
                                    .any(|tab| tab.id == active && tab.url != "about:blank")
                            }) {
                                // The viewport determines layout; intrinsic image dimensions must not size it.
                                body.child(img(image).absolute().top_0().left_0().size_full().object_fit(ObjectFit::Contain))
                            } else {
                                body.child(
                                    v_flex()
                                        .w_full()
                                        .items_center()
                                        .pt_12()
                                        .px_6()
                                        .gap_6()
                                        .child(
                                            div().size_9().flex().items_center().justify_center()
                                                .rounded_md().border_1().border_color(rgb(appearance::BORDER))
                                                .bg(rgb(appearance::RAISED)).mb_6()
                                                .child(Icon::new(IconName::Globe).size(px(18.)).text_color(rgb(appearance::MUTED))),
                                        )
                                        .child(div().text_xl().font_weight(FontWeight::SEMIBOLD).child("プレビューはまだありません"))
                                        .child(div().max_w(px(384.)).text_sm().text_center()
                                            .text_color(rgb(appearance::MUTED)).child(if self.thread.is_some() {
                                                "上の欄に URL を入力するか、AI にブラウザ操作を依頼してください。"
                                            } else {
                                                "会話を始めると、AI と同じタブを利用できます。"
                                            })),
                                )
                            }
                        })
                        .child(
                            canvas(
                                move |bounds, _, cx| {
                                    entity.update(cx, |view, _| view.bounds = bounds);
                                },
                                {
                                    let entity = cx.entity();
                                    move |bounds, _, window, cx| {
                                        entity.update(cx, |view, cx| {
                                            window.handle_input(
                                                &view.focus,
                                                ElementInputHandler::new(bounds, entity.clone()),
                                                cx,
                                            );
                                        });
                                    }
                                },
                            )
                            .absolute()
                            .top_0()
                            .left_0()
                            .size_full(),
                        ),
                )

            })
    }
}

impl EntityInputHandler for Browser {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let chars: Vec<u16> = self.composition.encode_utf16().collect();
        let end = range.end.min(chars.len());
        let start = range.start.min(end);
        *actual = Some(start..end);
        Some(String::from_utf16_lossy(&chars[start..end]))
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.composition_selection.clone(),
            reversed: false,
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        (!self.composition.is_empty()).then(|| 0..self.composition.encode_utf16().count())
    }
    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.composition.clear();
        self.composition_selection = 0..0;
        cx.notify();
    }
    fn replace_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.composition.clear();
        self.composition_selection = 0..0;
        self.send(BrowserAction::Type { text: text.into() }, cx);
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.composition = text.into();
        let end = text.encode_utf16().count();
        self.composition_selection = selected.unwrap_or(end..end);
        cx.notify();
    }
    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        Some(self.bounds)
    }
    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(0)
    }
}
