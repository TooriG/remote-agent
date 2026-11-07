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
        "接続先",
        move || {
            let all = list_scope && connection.state.with(|state| state.all_hosts);
            let mut entries = Vec::new();
            if list_scope {
                entries.push(json!({"id":"", "title":"すべての接続先", "symbol":"square.stack", "selected":all}));
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
        if list_scope && connection.state.with(|state| state.all_hosts) { return json!("すべての接続先"); }
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
                .unwrap_or_else(|| "接続先".into())
        }))
    })
    .prop("identifier", json!(if list_scope { "connections.list" } else { "connections.destination" }))
    .min_h(px(if is_desktop() { 28.0 } else { 44.0 }))
}

/// The composer's destination: the Host the next chat runs on.
pub(crate) fn destination(hosts: Memo<Vec<Host>>, selected: Memo<String>) -> impl IntoElement {
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
    ui::labeled_button(Icon::Monitor, name, move || {
        let items = json!(hosts.with_untracked(|hosts| hosts
            .iter()
            .map(|host| json!({"id": host.id, "title": host.name, "selected": host.id == selected.get_untracked()}))
            .collect::<Vec<_>>()));
        ui::popup(&items, move |id: &str| {
            connection
                .application
                .with(|application| application.select_host(id.to_owned(), true))
        })
    })
    .child(ui::icon(Icon::ChevronDown, 14.0))
}

pub(crate) fn screen(hosts: Memo<Vec<Host>>, close: impl Fn() + 'static) -> impl IntoElement {
    let connection = use_context::<Connection>().unwrap();
    let invitation = signal(String::new());
    let confirmation = signal(None::<agent_protocol::models::Invitation>);
    let error = signal(None::<String>);
    view()
        .p_4()
        .gap_3()
        .text_color(move || nsh_base::theme().foreground)
        .child(text("接続先").text_size(px(20.0)))
        .child(each(
            move || hosts.get(),
            |host: &Host| host.id.clone(),
            move |host, _| {
                let host = host.get_untracked();
                let id = host.id.clone();
                view()
                    .flex_row()
                    .gap_2()
                    .child(text(host.name))
                    .child(native::text_button(json!("接続"), move || {
                        connection
                            .application
                            .with(|application| application.select_host(id.clone(), false));
                    }))
            },
        ))
        .child(when(move || confirmation.get().is_none(), move || {
            view().gap_3()
                .child(text("PCを追加"))
                .child(text("PCのBexで発行した接続情報を貼り付けてください。"))
                .child(input().masked(true)
                    .auto_correct(false).auto_capitalize(AutoCapitalize::None)
                    .value(move || invitation.get())
                    .on_change(move |text: &str| invitation.set(text.into()))
                    .placeholder("接続情報")
                    .text_color(move || nsh_base::theme().foreground)
                    .h(px(44.0)))
                .child(native::text_button(json!("接続先を確認"), move || {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|time| time.as_secs()).unwrap_or_default();
                    match agent_core::client::native::parse_invitation(invitation.get_untracked(), now) {
                        Ok(value) => { confirmation.set(Some(value)); error.set(None); }
                        Err(reason) => error.set(Some(reason.to_string())),
                    }
                }))
        }))
        .child(when(move || confirmation.get().is_some(), move || {
            view().gap_3()
                .child(text(move || confirmation.with(|value| value.as_ref().map(|value| value.host_name.clone()).unwrap_or_default())))
                .child(text("メッセージ・添付ファイル・作業に必要なプロジェクトの内容を、このPCと利用するAIサービスに送信します。"))
                .child(text(move || confirmation.with(|value| {
                    let recipients = value.as_ref().map(|value| value.ai_recipients.join("、")).filter(|value| !value.is_empty()).unwrap_or_else(|| "未設定".into());
                    format!("AI処理：{recipients}")
                })))
                .child(text(move || confirmation.with(|value| value.as_ref().and_then(|value| value.transcription_recipient.as_ref()).map(|recipient| format!("音声入力：{recipient}（文字起こし）")).unwrap_or_default())))
                .child(text("共有PCでは、管理者などが内容を閲覧できる場合があります。"))
                .child(native::text_button(json!("同意して接続"), move || {
                    if !connection.pairing.get_untracked() && let Some(invitation) = confirmation.get_untracked() {
                        connection.application.with(|application| application.pair(invitation));
                    }
                }).prop("enabled", move || json!(!connection.pairing.get())))
                .child(native::text_button(json!("変更"), move || {
                    confirmation.set(None); invitation.set(String::new());
                }).prop("enabled", move || json!(!connection.pairing.get())))
        }))
        .child(text(move || error.get().unwrap_or_default()))
        .child(native::text_button(json!("閉じる"), close))
}
