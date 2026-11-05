mod qr_text;

use super::view::{section_heading, settings_group};
use crate::{Runtime, appearance, store_session::StoreSession};
use agent_core::{
    state::{Intent, Snapshot, operations as op},
    store::Outcome,
};
use agent_protocol::models::{Invitation, RemoteHost};
use gpui_kit::{
    component::{
        button::{Button, ButtonVariants},
        input::{Input, InputState, Textarea, TextareaState},
        menu::{DropdownMenu, PopupMenuItem},
        *,
    },
    prelude::FluentBuilder,
    *,
};
use std::{io::Write, sync::Arc};

pub(super) enum HostEvent {
    Selected(Option<RemoteHost>),
    Removed(String),
    Failed(String),
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ConnectionLayout {
    Onboarding,
    Settings,
}
enum Action {
    None,
    Invite,
    Pair,
    Remove(String),
}
enum Update {
    WslDistributions(Result<Vec<String>, String>),
    WslHost(Result<RemoteHost, String>),
    Connected(Result<StoreSession, String>),
    Snapshot,
    Completed(Action, Result<Outcome, String>),
    SshInvitation(Result<Invitation, String>),
}

/// Local management is a separate view/session from the selected conversation
/// host. The parent borrows this snapshot for the host menu, never its connection.
pub(super) struct Hosts {
    session: Option<StoreSession>,
    snapshot: Arc<Snapshot>,
    runtime: Runtime,
    updates: async_channel::Sender<Update>,
    pairing: Entity<TextareaState>,
    ssh: Entity<InputState>,
    adding_remote: bool,
    use_ssh: bool,
    invitation: Option<(String, tempfile::NamedTempFile)>,
    busy: bool,
    connecting: bool,
    wsl_distros: Vec<String>,
    error: Option<String>,
    _language: Subscription,
}
impl EventEmitter<HostEvent> for Hosts {}
impl Hosts {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let language = crate::localization::language(cx);
        StoreSession::on_app_quit(cx, |view| &mut view.session);
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
        let mut view = Self {
            session: None,
            snapshot: Arc::default(),
            runtime: cx.global::<Runtime>().clone(),
            updates,
            pairing: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .placeholder(language.text("相手の端末で発行した招待を貼り付け"))
                    .auto_grow(3, 6)
            }),
            ssh: cx.new(|cx| InputState::new(window, cx).placeholder("user@your-server")),
            adding_remote: false,
            use_ssh: true,
            invitation: None,
            busy: false,
            connecting: false,
            wsl_distros: Vec::new(),
            error: None,
            _language: cx.observe_global_in::<crate::localization::Language>(
                window,
                |view, window, cx| {
                    let language = crate::localization::language(cx);
                    view.pairing.update(cx, |input, cx| {
                        input.set_placeholder(
                            language.text("相手の端末で発行した招待を貼り付け"),
                            window,
                            cx,
                        )
                    });
                    cx.notify();
                },
            ),
        };
        view.connect();
        view.discover_wsl();
        view
    }
    fn discover_wsl(&self) {
        let updates = self.updates.clone();
        self.runtime.handle.spawn(async move {
            let result = crate::platform::wsl_distributions()
                .await
                .map_err(|error| error.to_string());
            let _ = updates.send(Update::WslDistributions(result)).await;
        });
    }
    fn choices(&self) -> Vec<RemoteHost> {
        self.wsl_distros
            .iter()
            .map(|distro| RemoteHost {
                id: format!("wsl:{distro}"),
                name: format!("WSL ({distro})"),
                ticket: String::new(),
            })
            .chain(self.snapshot.connection.management.remotes.iter().cloned())
            .collect()
    }
    fn select(&mut self, remote: Option<RemoteHost>, cx: &mut Context<Self>) {
        if let Some(distro) = remote
            .as_ref()
            .and_then(|host| host.id.strip_prefix("wsl:"))
        {
            if self.busy {
                return;
            }
            self.busy = true;
            self.error = None;
            let distro = distro.to_owned();
            let connections = self.runtime.connections.clone();
            let updates = self.updates.clone();
            self.runtime.handle.spawn(async move {
                let result = connections
                    .wsl_host(&distro)
                    .await
                    .map_err(|error| format!("{error:#}"));
                let _ = updates.send(Update::WslHost(result)).await;
            });
        } else {
            cx.emit(HostEvent::Selected(remote));
        }
        cx.notify();
    }
    fn connect(&mut self) {
        if self.connecting {
            return;
        }
        self.connecting = true;
        self.error = None;
        self.session.take();
        let updates = self.updates.clone();
        let runtime = self.runtime.clone();
        self.runtime.handle.spawn(async move {
            match runtime
                .sessions
                .open(None, runtime.clone())
                .await
                .and_then(|session| session.side(String::new()))
            {
                Ok(session) => {
                    StoreSession::observe(session, updates, Update::Connected, |_| Update::Snapshot)
                        .await
                }
                Err(error) => {
                    let _ = updates.send(Update::Connected(Err(error))).await;
                }
            }
        });
    }
    pub(super) fn refresh(&mut self) {
        if self.busy || self.connecting {
            return;
        }
        self.discover_wsl();
        if self.snapshot.connection.connected {
            self.dispatch(
                Intent::LoadHostManagement(op::LoadHostManagement {}),
                Action::None,
            );
        } else {
            self.connect();
        }
    }
    fn dispatch(&mut self, intent: Intent, action: Action) {
        let Some(store) = self.session.as_ref().map(|session| &session.pane) else {
            return;
        };
        self.busy = true;
        self.error = None;
        let receipt = store.dispatch(intent);
        let updates = self.updates.clone();
        self.runtime.handle.spawn(async move {
            let result = receipt.await.map_err(|error| error.to_string());
            let _ = updates.send(Update::Completed(action, result)).await;
        });
    }
    fn receive(&mut self, update: Update, window: &mut Window, cx: &mut Context<Self>) {
        match update {
            Update::WslDistributions(result) => match result {
                Ok(distros) => self.wsl_distros = distros,
                Err(error) => {
                    tracing::warn!(target: "agent", operation = "wsl.discovery", message = %error)
                }
            },
            Update::WslHost(result) => {
                self.busy = false;
                match result {
                    Ok(host) => cx.emit(HostEvent::Selected(Some(host))),
                    Err(error) => {
                        self.error = Some(error.clone());
                        cx.emit(HostEvent::Failed(error));
                    }
                }
            }
            Update::SshInvitation(result) => {
                self.busy = false;
                match result {
                    Ok(invitation) => self.pair(invitation),
                    Err(error) => self.error = Some(error),
                }
            }
            Update::Connected(result) => {
                self.connecting = false;
                match result {
                    Ok(store) => {
                        self.snapshot = store.pane.snapshot().expect("connected management pane");
                        self.session = Some(store);
                        self.dispatch(
                            Intent::LoadHostManagement(op::LoadHostManagement {}),
                            Action::None,
                        );
                    }
                    Err(error) => self.error = Some(error),
                }
            }
            Update::Snapshot => {
                let next = self
                    .session
                    .as_ref()
                    .and_then(|session| session.pane.snapshot().ok());
                match next {
                    Some(next) if self.snapshot.connection != next.connection => {
                        self.snapshot = next
                    }
                    _ => return,
                }
            }
            Update::Completed(action, result) => {
                self.busy = false;
                if let Some(next) = self
                    .session
                    .as_ref()
                    .and_then(|session| session.pane.snapshot().ok())
                {
                    self.snapshot = next;
                }
                match result {
                    Err(error) => self.error = Some(error),
                    Ok(outcome) => match action {
                        Action::Invite => {
                            if let Some(invitation) =
                                &self.snapshot.connection.management.invitation
                            {
                                let result = (|| -> Result<_, String> {
                                    let text = serde_json::to_string(invitation)
                                        .map_err(|error| error.to_string())?;
                                    let text = qr_text::ascii_json(&text);
                                    let code = qrcode::QrCode::with_error_correction_level(
                                        text.as_bytes(),
                                        qrcode::EcLevel::M,
                                    )
                                    .map_err(|error| error.to_string())?;
                                    let svg = code
                                        .render::<qrcode::render::svg::Color>()
                                        .min_dimensions(320, 320)
                                        .build();
                                    let mut file = tempfile::Builder::new()
                                        .prefix("agent-invitation-")
                                        .suffix(".svg")
                                        .tempfile()
                                        .map_err(|error| error.to_string())?;
                                    file.write_all(svg.as_bytes())
                                        .map_err(|error| error.to_string())?;
                                    Ok((text, file))
                                })();
                                match result {
                                    Ok(invitation) => self.invitation = Some(invitation),
                                    Err(error) => self.error = Some(error),
                                }
                            }
                        }
                        Action::Pair => {
                            self.adding_remote = false;
                            self.pairing
                                .update(cx, |input, cx| input.set_value("", window, cx));
                            if let Outcome::RemoteHostPaired { id } = outcome
                                && let Some(host) = self
                                    .snapshot
                                    .connection
                                    .management
                                    .remotes
                                    .iter()
                                    .find(|host| host.id == id)
                            {
                                cx.emit(HostEvent::Selected(Some(host.clone())));
                            }
                        }
                        Action::Remove(id) => cx.emit(HostEvent::Removed(id)),
                        Action::None => {}
                    },
                }
            }
        }
        cx.notify();
    }
    fn pair(&mut self, invitation: Invitation) {
        self.dispatch(
            Intent::PairRemoteHost(op::PairRemoteHost {
                invitation,
                name: "Bex Desktop".into(),
            }),
            Action::Pair,
        );
    }

    pub(super) fn connection_choices(
        &self,
        layout: ConnectionLayout,
        current: Option<&str>,
        connected: bool,
        agent_controls: Div,
        cx: &Context<Self>,
    ) -> Div {
        let language = crate::localization::language(cx);
        let cards = layout == ConnectionLayout::Onboarding;
        let mut agent_controls = Some(agent_controls);
        let mut choices = if cards {
            v_flex().gap_3()
        } else {
            settings_group().overflow_hidden()
        };
        let targets = self.choices();
        for (index, remote) in std::iter::once(None)
            .chain(targets.iter().map(Some))
            .enumerate()
        {
            let selected = current == remote.map(|host| host.id.as_str());
            let destination = remote.cloned();
            let name = remote.map_or(language.text("このPC"), |host| host.name.as_str());
            let label = if selected {
                if connected {
                    language.text("接続済み")
                } else {
                    language.text("接続中…")
                }
            } else if (remote.is_none() || remote.is_some_and(|host| host.id.starts_with("wsl:")))
                && self.snapshot.connection.connected
            {
                language.text("利用可能")
            } else {
                language.text("保存済み")
            };
            let mut card = v_flex()
                .gap_3()
                .p_4()
                .when(cards, |view| {
                    view.rounded(px(12.))
                        .border_1()
                        .border_color(if selected {
                            rgb(appearance::ACCENT)
                        } else {
                            rgb(appearance::BORDER)
                        })
                        .bg(rgb(appearance::SURFACE))
                })
                .when(!cards, |view| {
                    view.p_5()
                        .border_b_1()
                        .border_color(rgb(appearance::BORDER))
                })
                .child(
                    h_flex()
                        .items_center()
                        .gap_3()
                        .child(
                            Button::new(format!("onboarding-host-{index}"))
                                .accessibility_label(name.to_owned())
                                .child(
                                    h_flex()
                                        .w_full()
                                        .gap_2()
                                        .child(if remote.is_some() {
                                            Icon::new(IconName::Network)
                                        } else {
                                            Icon::default().path("icons/monitor.svg")
                                        })
                                        .child(name.to_owned()),
                                )
                                .ghost()
                                .flex_1()
                                .disabled(self.busy)
                                .on_click(cx.listener(move |view, _, _, cx| {
                                    view.select(destination.clone(), cx);
                                })),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(if selected && connected {
                                    rgb(0x88c9a0)
                                } else {
                                    rgb(appearance::MUTED)
                                })
                                .child(label),
                        )
                        .when_some(
                            remote.filter(|host| !cards && !host.id.starts_with("wsl:")),
                            |row, remote| {
                                let id = remote.id.clone();
                                let ticket = remote.ticket.clone();
                                row.child(
                                    Button::new(format!("remove-{id}"))
                                        .icon(IconName::Close)
                                        .small()
                                        .ghost()
                                        .accessibility_label(language.text("保存した接続を削除"))
                                        .tooltip(language.text("保存した接続を削除"))
                                        .disabled(self.busy || !self.snapshot.connection.connected)
                                        .on_click(cx.listener(move |view, _, _, cx| {
                                            view.dispatch(
                                                Intent::RemoveRemoteHost(op::RemoveRemoteHost {
                                                    id: id.clone(),
                                                }),
                                                Action::Remove(id.clone()),
                                            );
                                            if let Ok(ticket) =
                                                ticket.parse::<agent_transport::transport::Ticket>()
                                                && let Ok(copies) =
                                                    crate::platform::conversation_copies(
                                                        &ticket.node_id().to_string(),
                                                    )
                                            {
                                                std::thread::spawn(move || {
                                                    agent_core::conversation_cache::clear(&copies)
                                                });
                                            }
                                            cx.notify();
                                        })),
                                )
                            },
                        ),
                );
            if selected {
                let details = v_flex()
                    .gap_2()
                    .when(cards, |view| {
                        view.pt_3()
                            .border_t_1()
                            .border_color(rgb(appearance::BORDER))
                    })
                    .when(!cards, |view| {
                        view.ml_8()
                            .p_3()
                            .rounded(px(6.))
                            .bg(rgb(appearance::RAISED))
                    })
                    .children(agent_controls.take());
                card = card.child(details);
            }
            choices = choices.child(card);
        }
        choices = choices.child(
            Button::new("onboarding-add-remote")
                .accessibility_label(if cards {
                    language.text("クラウドVM・別のPCに接続")
                } else {
                    language.text("接続先を追加")
                })
                .child(
                    h_flex()
                        .when(!cards, |row| row.w_full())
                        .gap_2()
                        .child(Icon::new(if cards {
                            IconName::Network
                        } else {
                            IconName::Plus
                        }))
                        .child(if cards {
                            language.text("クラウドVM・別のPCに接続")
                        } else {
                            language.text("接続先を追加")
                        }),
                )
                .ghost()
                .w_full()
                .when(!cards, |button| {
                    button.h(px(48.)).px_4().text_color(rgb(0x8bb7f9))
                })
                .disabled(self.busy)
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.adding_remote = !view.adding_remote;
                    view.error = None;
                    cx.notify();
                })),
        );
        if self.adding_remote {
            let disabled = self.busy || !self.snapshot.connection.connected;
            let mut methods = h_flex().gap_2();
            for (id, label, use_ssh) in [
                ("onboarding-use-ssh", language.text("SSHで接続"), true),
                (
                    "onboarding-use-invitation",
                    language.text("接続情報を貼り付け"),
                    false,
                ),
            ] {
                methods = methods.child(
                    Button::new(id)
                        .label(label)
                        .small()
                        .ghost()
                        .selected(self.use_ssh == use_ssh)
                        .disabled(disabled)
                        .on_click(cx.listener(move |view, _, _, cx| {
                            view.use_ssh = use_ssh;
                            view.error = None;
                            cx.notify();
                        })),
                );
            }
            let mut form = v_flex()
                .gap_3()
                .p_4()
                .rounded(px(12.))
                .border_1()
                .border_color(rgb(appearance::BORDER))
                .bg(rgb(appearance::SURFACE))
                .when(!cards, |view| view.mx_4().mb_4())
                .child(methods);
            if self.use_ssh {
                form = form
                    .child(language.text("SSH接続先"))
                    .child(
                        Input::new(&self.ssh)
                            .aria_label(language.text("SSH接続先"))
                            .disabled(disabled),
                    )
                    .child(div().text_xs().text_color(rgb(appearance::MUTED)).child(
                        language.text(
                            "SSHの鍵で接続でき、Bexのホストを起動済みのサーバーにつなぎます。",
                        ),
                    ));
            } else {
                form = form.child(
                    Textarea::new(&self.pairing)
                        .aria_label(language.text("接続情報"))
                        .readonly(disabled),
                );
            }
            choices =
                choices.child(
                    form.child(
                        Button::new("onboarding-pair")
                            .label(if self.busy {
                                language.text("接続中…")
                            } else {
                                language.text("接続")
                            })
                            .disabled(disabled)
                            .on_click(cx.listener(move |view, _, _, cx| {
                                if view.use_ssh {
                                    view.busy = true;
                                    view.error = None;
                                    let destination = view.ssh.read(cx).value().to_string();
                                    let updates = view.updates.clone();
                                    view.runtime.handle.spawn(async move {
                                        let result =
                                            crate::platform::ssh_invitation(&destination).await;
                                        let _ = updates.send(Update::SshInvitation(result)).await;
                                    });
                                } else {
                                    match serde_json::from_str::<Invitation>(
                                        view.pairing.read(cx).value().as_ref(),
                                    ) {
                                        Ok(invitation) => view.pair(invitation),
                                        Err(_) => view.error = Some(
                                            language.text("接続先で発行したBexの接続情報を貼り付けてください。")
                                                .into(),
                                        ),
                                    }
                                }
                                cx.notify();
                            })),
                    ),
                );
        }
        choices = choices.child(
            h_flex()
                .items_center()
                .gap_3()
                .p_4()
                .when(cards, |view| view.rounded(px(12.)).border_1())
                .when(!cards, |view| view.border_t_1())
                .border_color(rgb(appearance::BORDER))
                .text_color(rgb(appearance::MUTED))
                .child(Icon::new(IconName::Network))
                .child(div().flex_1().child(language.text("Bexの実行環境")))
                .child(div().text_xs().child(language.text("今後対応"))),
        );
        if let Some(error) = self.error.as_ref().or(self.snapshot.pane.error.as_ref()) {
            choices = choices.child(
                div()
                    .when(!cards, |view| view.p_4())
                    .text_color(rgb(0xff8e86))
                    .child(super::view::error_view(
                        &agent_core::presentation::error::error_text(error),
                        language,
                    )),
            );
        }
        choices
    }
    pub(super) fn menu(
        owner: &Entity<Self>,
        id: &'static str,
        current: Option<&str>,
        fallback_name: &str,
        disabled: bool,
        cx: &App,
    ) -> impl IntoElement {
        let language = crate::localization::language(cx);
        let view = owner.read(cx);
        let hosts = view.choices();
        let name: SharedString = current
            .and_then(|id| hosts.iter().find(|host| host.id == id))
            .map_or_else(
                || fallback_name.to_owned().into(),
                |host| host.name.clone().into(),
            );
        let current = current.map(str::to_owned);
        let owner = owner.downgrade();
        Button::new(id)
            .debug_selector(move || id.into())
            .disabled(disabled || view.busy)
            .accessibility_label(agent_core::localization::localized_ui_format(
                language,
                "実行先: {0}".into(),
                vec![name.to_string()],
            ))
            .tooltip(agent_core::localization::localized_ui_format(
                language,
                "実行先: {0}".into(),
                vec![name.to_string()],
            ))
            .label(name)
            .icon(Icon::default().path("icons/monitor.svg"))
            .dropdown_caret(true)
            .h_8()
            .min_w_0()
            .max_w(px(200.))
            .flex_shrink_1()
            .ghost()
            .dropdown_menu(move |mut menu, _, _| {
                let local = owner.clone();
                menu = menu.item(
                    PopupMenuItem::new(language.text("この端末"))
                        .checked(current.is_none())
                        .on_click(move |_, _, cx| {
                            let _ = local.update(cx, |view, cx| view.select(None, cx));
                        }),
                );
                for host in &hosts {
                    let owner = owner.clone();
                    let host = host.clone();
                    menu = menu.item(
                        PopupMenuItem::new(host.name.clone())
                            .checked(current.as_deref() == Some(&host.id))
                            .on_click(move |_, _, cx| {
                                let _ = owner
                                    .update(cx, |view, cx| view.select(Some(host.clone()), cx));
                            }),
                    );
                }
                menu
            })
    }
}
impl Render for Hosts {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let language = crate::localization::language(cx);
        let disabled = !self.snapshot.connection.connected || self.busy;
        let mut body = v_flex().w_full().gap_5();
        if !self.snapshot.connection.connected {
            return body
                .child(
                    div()
                        .text_color(rgb(appearance::MUTED))
                        .child(language.text("このPCの接続情報を読み込めません。")),
                )
                .child(
                    Button::new("connect-host")
                        .label(if self.connecting {
                            language.text("接続中…")
                        } else {
                            language.text("ホストに接続")
                        })
                        .disabled(self.connecting)
                        .on_click(cx.listener(move |view, _, _, cx| {
                            view.connect();
                            cx.notify();
                        })),
                )
                .into_any_element();
        }
        body = body.child(
            h_flex()
                .items_center()
                .flex_wrap()
                .gap_4()
                .child(
                    section_heading(
                        language.text("スマートフォンから接続"),
                        language.text("このPCの会話を、iPhoneやAndroidから続けられます。"),
                    )
                    .flex_1()
                    .min_w(px(220.)),
                )
                .child(
                    Button::new("invite")
                        .label(language.text("接続用QRを表示"))
                        .icon(Icon::default().path("icons/qr-code.svg"))
                        .secondary()
                        .outline()
                        .h(px(32.))
                        .disabled(disabled)
                        .on_click(cx.listener(move |view, _, _, cx| {
                            view.dispatch(
                                Intent::CreateInvitation(op::CreateInvitation {}),
                                Action::Invite,
                            );
                            cx.notify();
                        })),
                ),
        );
        if let Some((_, file)) = &self.invitation {
            body = body.child(
                h_flex().flex_wrap().gap_5().p_5().rounded(px(8.))
                    .border_1().border_color(rgb(appearance::BORDER))
                    .child(img(file.path().to_path_buf()).w(px(200.)).h(px(200.)))
                    .child(v_flex().flex_1().min_w(px(200.)).gap_3()
                        .child(div().font_semibold().child(language.text("BexアプリでQRを読み取る")))
                        .child(div().text_sm().text_color(rgb(appearance::MUTED))
                            .child(language.text("1回限りの招待です。QRを読み取るか、招待をコピーして相手の端末に貼り付けてください。")))
                        .child(Button::new("copy-invite")
                            .label(language.text("招待をコピー"))
                            .icon(IconName::Copy)
                            .ghost()
                            .on_click(cx.listener(move |view, _, _, cx| {
                                if let Some((text, _)) = &view.invitation {
                                    cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                                }
                            })))),
            );
        }
        let mut devices =
            v_flex()
                .gap_3()
                .pt_6()
                .border_t_1()
                .border_color(rgb(appearance::BORDER))
                .child(
                    h_flex()
                        .items_center()
                        .gap_3()
                        .child(
                            v_flex()
                                .flex_1()
                                .gap_1()
                                .child(
                                    div()
                                        .font_semibold()
                                        .child(language.text("接続を許可した端末")),
                                )
                                .child(div().text_sm().text_color(rgb(appearance::MUTED)).child(
                                    language.text("このPCへアクセスできる端末を管理します。"),
                                )),
                        )
                        .child(
                            Button::new("refresh-hosts")
                                .icon(IconName::RotateCw)
                                .small()
                                .ghost()
                                .accessibility_label(language.text("接続一覧を更新"))
                                .tooltip(language.text("接続一覧を更新"))
                                .disabled(disabled)
                                .on_click(cx.listener(move |view, _, _, cx| {
                                    view.refresh();
                                    cx.notify();
                                })),
                        ),
                );
        if let Some(status) = &self.snapshot.connection.management.status {
            let mut list = settings_group().overflow_hidden();
            if status.devices.is_empty() {
                list = list.child(
                    div()
                        .p_5()
                        .text_sm()
                        .text_color(rgb(appearance::MUTED))
                        .child(language.text("接続を許可した端末はありません。")),
                );
            }
            for (index, device) in status.devices.iter().enumerate() {
                let node = device.id.clone();
                list = list.child(
                    h_flex()
                        .items_center()
                        .gap_4()
                        .p_5()
                        .when(index > 0, |row| {
                            row.border_t_1().border_color(rgb(appearance::BORDER))
                        })
                        .child(Icon::new(IconName::Network).text_color(rgb(appearance::MUTED)))
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap_1()
                                .child(div().text_ellipsis().child(
                                    device.name.clone().unwrap_or_else(|| {
                                        language.text("名前未取得の端末").into()
                                    }),
                                ))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(appearance::MUTED))
                                        .text_ellipsis()
                                        .child(node.clone()),
                                ),
                        )
                        .child(
                            Button::new(format!("revoke-{index}"))
                                .label(language.text("接続を解除"))
                                .small()
                                .ghost()
                                .text_color(rgb(appearance::MUTED))
                                .disabled(disabled)
                                .on_click(cx.listener(move |view, _, _, cx| {
                                    view.dispatch(
                                        Intent::RevokeDevice(op::RevokeDevice { id: node.clone() }),
                                        Action::None,
                                    );
                                    cx.notify();
                                })),
                        ),
                );
            }
            devices = devices.child(list);
        } else {
            devices = devices.child(
                div()
                    .text_sm()
                    .text_color(rgb(appearance::MUTED))
                    .child(language.text("端末を読み込み中…")),
            );
        }
        body.child(devices).into_any_element()
    }
}
