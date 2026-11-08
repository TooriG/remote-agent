use crate::l10n::{localized, tr, tr_format};
use crate::{Connection, application::Host, native};
use nsh_core::{element::input, prelude::*};
use serde_json::json;

pub(crate) fn selector(
    hosts: Memo<Vec<Host>>,
    selected: Memo<String>,
    list_scope: bool,
) -> nsh_core::element::NativeView {
    let connection = use_context::<Connection>().unwrap();
    native::menu(
        "desktopcomputer",
        tr("接続先"),
        move || {
            let all = list_scope && connection.state.with(|state| state.all_hosts);
            let mut entries = Vec::new();
            if list_scope {
                entries.push(json!({"id":"", "title":tr("すべての接続先"), "symbol":"square.stack", "selected":all}));
                entries.push(json!({"separator":true}));
            }
            entries.extend(hosts.with(|hosts| hosts.iter().map(|host| json!({"id":host.id,"title":host.name,"selected":!all && host.id == selected.get()})).collect::<Vec<_>>()));
            json!(entries)
        },
        move |name| {
            connection.application.with(|application| {
                if list_scope && name.is_empty() { application.select_all_hosts(); }
                else { application.select_host(name.to_owned(), !list_scope); }
            });
        },
    )
    .prop("title", move || {
        if list_scope && connection.state.with(|state| state.all_hosts) { return json!(tr("すべての接続先")); }
        json!(hosts.with(|hosts| {
            hosts
                .iter()
                .find(|host| host.id == selected.get())
                .map(|host| {
                    if is_desktop() && host.id == "local" {
                        "Local".into()
                    } else {
                        host.name.clone()
                    }
                })
                .unwrap_or_else(|| tr("接続先").into())
        }))
    })
    .prop("identifier", json!(if list_scope { "connections.list" } else { "connections.destination" }))
    .min_h(px(if is_desktop() { 28.0 } else { 44.0 }))
}

/// The composer's destination: the Host the next chat runs on.
pub(crate) fn destination(hosts: Memo<Vec<Host>>, selected: Memo<String>) -> impl IntoElement {
    #[cfg(not(target_os = "macos"))]
    use crate::ui::{self, Icon};
    let connection = use_context::<Connection>().unwrap();
    let name = move || {
        hosts.with(|hosts| {
            hosts
                .iter()
                .find(|host| host.id == selected.get())
                .map(|host| {
                    if host.id == "local" {
                        "Local".into()
                    } else {
                        host.name.clone()
                    }
                })
                .unwrap_or_else(|| "Local".into())
        })
    };
    let items = move || {
        json!(hosts.with(|hosts| hosts
            .iter()
            .map(|host| json!({"id": host.id, "title": host.name, "selected": host.id == selected.get()}))
            .collect::<Vec<_>>()))
    };
    let select = move |id: &str| {
        connection
            .application
            .with(|application| application.select_host(id.to_owned(), true))
    };
    #[cfg(target_os = "macos")]
    {
        native::menu("desktopcomputer", tr("接続先"), items, select)
            .prop("title", move || json!(name()))
            .prop("appearance", json!("plain"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        ui::labeled_button(Icon::MONITOR, name, move || {
            ui::popup(&untracked(items), select)
        })
        .child(ui::icon(Icon::CHEVRON_DOWN, 14.0))
    }
}

/// The Hosts this device can work on, and pairing another PC.
pub(crate) fn screen(hosts: Memo<Vec<Host>>) -> impl IntoElement {
    use crate::ui::{self, Enabled, colors};
    let connection = use_context::<Connection>().unwrap();
    let invitation = signal(String::new());
    let confirmation = signal(None::<agent_protocol::models::Invitation>);
    let error = signal(None::<String>);
    let heading = |title: &'static str| {
        text(title)
            .text_size(px(16.0))
            .font_weight(FontWeight::SEMIBOLD)
    };
    let confirmed = move |read: fn(&agent_protocol::models::Invitation) -> String| {
        move || confirmation.with(|value| value.as_ref().map(read).unwrap_or_default())
    };
    view()
        .gap_4()
        .text_color(move || nsh_base::theme().foreground)
        .child(heading(tr("接続先")))
        .child(
            view()
                .rounded(px(12.0))
                .border_1()
                .border_color(colors::BORDER)
                .bg(colors::SURFACE)
                .child(each(
                    move || hosts.get(),
                    |host: &Host| host.id.clone(),
                    move |host, index| {
                        let host = host.get_untracked();
                        let id = host.id.clone();
                        ui::setting(
                            host.name,
                            String::new,
                            ui::text_button(tr("接続"), move || {
                                connection.application.with(|application| {
                                    application.select_host(id.clone(), false)
                                });
                            }),
                        )
                        .style_when(move || index.get() > 0, |s| {
                            s.border_t_1().border_t_color(colors::BORDER)
                        })
                    },
                )),
        )
        .child(heading(tr("PCを追加")))
        .child(when(move || confirmation.get().is_none(), move || {
            ui::card(vec![view()
                .px_5()
                .py_4()
                .gap_3()
                .child(text(tr("PCのBexで発行した接続情報を貼り付けてください。")).text_size(px(13.0)).text_color(colors::MUTED))
                .child(input().masked(true)
                    .auto_correct(false).auto_capitalize(AutoCapitalize::None)
                    .value(move || invitation.get())
                    .on_change(move |text: &str| invitation.set(text.into()))
                    .placeholder(tr("接続情報"))
                    .text_color(move || nsh_base::theme().foreground)
                    .w_full()
                    .h(px(36.0))
                    .px_3()
                    .rounded(px(ui::RADIUS))
                    .border_1()
                    .border_color(colors::BORDER))
                .child(view().flex_row().justify_end().child(ui::text_button(tr("接続先を確認"), move || {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|time| time.as_secs()).unwrap_or_default();
                    match agent_core::client::native::parse_invitation(invitation.get_untracked(), now) {
                        Ok(value) => { confirmation.set(Some(value)); error.set(None); }
                        Err(reason) => error.set(Some(localized(reason.to_string()))),
                    }
                })))])
        }))
        .child(when(move || confirmation.get().is_some(), move || {
            ui::card(vec![view()
                .px_5()
                .py_4()
                .gap_3()
                .child(text(confirmed(|value| value.host_name.clone())).font_weight(FontWeight::MEDIUM))
                .child(text(tr("メッセージ・添付ファイル・作業に必要なプロジェクトの内容を、このPCと利用するAIサービスに送信します。")))
                .child(text(confirmed(|value| {
                    let recipients = Some(value.ai_recipients.join(tr("、"))).filter(|value| !value.is_empty()).unwrap_or_else(|| tr("未設定").into());
                    tr_format("AI処理：{0}", [&recipients])
                })))
                .child(text(confirmed(|value| value.transcription_recipient.as_ref().map(|recipient| tr_format("音声入力：{0}（文字起こし）", [recipient])).unwrap_or_default())))
                .child(text(tr("共有PCでは、管理者などが内容を閲覧できる場合があります。")).text_size(px(13.0)).text_color(colors::MUTED))
                .child(view().flex_row().justify_end().gap_2()
                    .child(ui::text_button(tr("変更"), move || {
                        confirmation.set(None); invitation.set(String::new());
                    }).enabled(move || !connection.pairing.get()))
                    .child(ui::text_button(tr("同意して接続"), move || {
                        if !connection.pairing.get_untracked() && let Some(invitation) = confirmation.get_untracked() {
                            connection.application.with(|application| application.pair(invitation));
                        }
                    }).enabled(move || !connection.pairing.get())))])
        }))
        .child(text(move || error.get().unwrap_or_default()).selectable(true))
}
