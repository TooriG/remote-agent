use crate::{Connection, native, storage::Host};
use nsh_core::{element::input, prelude::*};
use serde_json::json;

pub(crate) fn selector(hosts: Signal<Vec<Host>>, selected: Signal<String>) -> impl IntoElement {
    let connection = use_context::<Connection>().unwrap();
    native::menu(
        "desktopcomputer",
        "接続先",
        move || {
            hosts.with(|hosts| {
                native::items(
                    hosts
                        .iter()
                        .map(|host| (host.id.clone(), host.name.clone())),
                )
            })
        },
        move |name| {
            if let Some(host) = hosts.with(|hosts| {
                hosts
                    .iter()
                    .find(|host| host.id == name)
                    .map(|host| host.id.clone())
            }) {
                selected.set(host.clone());
                connection.client.with(|client| client.reconnect(host));
            }
        },
    )
    .prop("title", move || {
        json!(hosts.with(|hosts| {
            hosts
                .iter()
                .find(|host| host.id == selected.get())
                .map(|host| host.name.clone())
                .unwrap_or_else(|| "接続先".into())
        }))
    })
    .min_h(px(44.0))
}

pub(crate) fn screen(
    hosts: Signal<Vec<Host>>,
    selected: Signal<String>,
    close: impl Fn() + 'static,
) -> impl IntoElement {
    let connection = use_context::<Connection>().unwrap();
    let invitation = signal(String::new());
    let confirmation = signal(None::<agent_protocol::models::Invitation>);
    let error = signal(None::<String>);
    view()
        .flex_1()
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
                        selected.set(id.clone());
                        connection
                            .client
                            .with(|client| client.reconnect(id.clone()));
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
                        connection.pairing.set(true);
                        connection.client.with(|client| client.pair(invitation));
                    }
                }).prop("enabled", move || json!(!connection.pairing.get())))
                .child(native::text_button(json!("変更"), move || {
                    confirmation.set(None); invitation.set(String::new());
                }).prop("enabled", move || json!(!connection.pairing.get())))
        }))
        .child(text(move || error.get().unwrap_or_default()))
        .child(native::text_button(json!("閉じる"), close))
}
