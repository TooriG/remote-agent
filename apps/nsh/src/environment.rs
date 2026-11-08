use crate::l10n::{localized, tr, tr_format};
use crate::ui::{self, Enabled};
use crate::{Connection, application::Host, connections, native};
use agent_core::presentation::connections::ConnectionAgent;
use agent_core::state::{Intent, operations as op};
use nsh_core::prelude::*;
use serde_json::json;

pub(crate) fn overview(hosts: Memo<Vec<Host>>) -> impl IntoElement {
    let connection = use_context::<Connection>().unwrap();
    read_settings(connection);
    scroll().flex_1().child(
        view()
            .p_4()
            .gap_4()
            .child(text(tr("接続")).text_size(px(20.0)))
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .child(
                        view()
                            .flex_1()
                            .child(text(move || host_name(connection)))
                            .child(text(move || connection_status(connection))),
                    )
                    .child(connection_switch(connection)),
            )
            .child(text("Bex Host").text_size(px(20.0)))
            .child(text(move || host_software(connection)).selectable(true))
            .child(
                native::text_button(
                    json!(tr("バージョン情報を再読み込み")),
                    move || reload_host_software(connection),
                )
                .prop("enabled", move || {
                    json!(connection.settings.with(|p| p.connected()))
                }),
            )
            .child(native::text_button(
                json!(tr("アップデートを確認")),
                check_updates,
            ))
            .child(text(update_note()))
            .child(text(tr("プロジェクトと会話")).text_size(px(20.0)))
            .child(
                native::text_button(json!(tr("ワークツリー設定")), crate::worktrees::open)
                    .prop("symbol", json!("arrow.triangle.branch"))
                    .prop("enabled", move || {
                        json!(connection.settings.with(|p| p.connected()))
                    }),
            )
            .child(text(tr("プロバイダー")).text_size(px(20.0)))
            .child(
                native::text_button(
                    json!(tr("プロバイダーを再読み込み")),
                    move || reload_providers(connection),
                )
                .prop("enabled", move || json!(providers_reloadable(connection))),
            )
            .child(each(
                move || agents(connection),
                |agent: &ConnectionAgent| agent.provider,
                move |agent, _| {
                    view()
                        .flex_row()
                        .gap_3()
                        .items_center()
                        .child(
                            view()
                                .flex_1()
                                .gap_2()
                                .child(
                                    native::text_button(
                                        move || json!(agent.with(|a| a.name.clone())),
                                        move || {
                                            connection.application.with(|app| {
                                                app.navigate(crate::Navigation::Provider(Some(
                                                    agent.with_untracked(|a| a.provider),
                                                )))
                                            });
                                        },
                                    )
                                    .prop("asset", move || {
                                        json!(crate::accounts::icon(agent.with(|a| a.provider)))
                                    }),
                                )
                                .child(text(move || agent.with(|a| localized(a.label.clone()))))
                                .child(text(move || agent.with(agent_error))),
                        )
                        .child(provider_switch(connection, agent))
                },
            ))
            .child(text(providers_note()))
            .child(connections::screen(hosts)),
    )
}

fn update_note() -> &'static str {
    tr("更新はこのPCでBexを開いて適用できます。")
}

pub(crate) fn providers_note() -> &'static str {
    tr("選択したPCに適用され、同じ環境を使う端末にも反映されます。")
}

/// Reads the selected Host's providers and software once it connects.
pub(crate) fn read_settings(connection: Connection) {
    let ready = computed(move || {
        (
            connection.host.get(),
            connection.settings.with(|p| p.connected()),
        )
    });
    effect(move || {
        if ready.get().1 {
            connection.dispatch(Intent::ReadProviderSettings(op::ReadProviderSettings {}));
            connection.dispatch(Intent::ReadHostSoftware(op::ReadHostSoftware {}));
        }
    });
}

pub(crate) fn host_name(connection: Connection) -> String {
    connection
        .settings
        .with(|p| p.host_name().unwrap_or_else(|| tr("接続先").into()))
}

pub(crate) fn connection_status(connection: Connection) -> String {
    connection
        .settings
        .with(|p| localized(p.connection_status_label()))
}

/// Whether this PC accepts connections from the selected Host's devices.
pub(crate) fn connection_switch(connection: Connection) -> ui::Switch {
    ui::switch(
        || tr("このPCへの接続").into(),
        move || connection.settings.with(|p| p.connection_enabled()),
        move |enabled| {
            connection.dispatch(Intent::SetConnectionEnabled { enabled });
        },
    )
    .enabled(move || !connection.host.get().is_empty())
}

pub(crate) fn host_software(connection: Connection) -> String {
    connection.publication.with(|p| {
        p.settings
            .host_software()
            .map(|software| {
                tr_format(
                    "{0} · ビルド {1}\n{2}",
                    [&software.version, &software.build, &software.revision],
                )
            })
            .unwrap_or_default()
    })
}

pub(crate) fn reload_host_software(connection: Connection) {
    connection.dispatch(Intent::ReadHostSoftware(op::ReadHostSoftware {}));
}

fn check_updates() {
    native::open_link("https://github.com/ttizze/remote-agent/releases");
}

pub(crate) fn agents(connection: Connection) -> Vec<ConnectionAgent> {
    connection.settings.with(|p| p.connection_setup().agents)
}

pub(crate) fn agent_error(agent: &ConnectionAgent) -> String {
    agent
        .error
        .as_ref()
        .map(|error| localized(error.to_string()))
        .unwrap_or_default()
}

pub(crate) fn reload_providers(connection: Connection) {
    connection.dispatch(Intent::ReadProviderSettings(op::ReadProviderSettings {}));
}

pub(crate) fn providers_reloadable(connection: Connection) -> bool {
    connection
        .publication
        .with(|p| p.settings.connected() && !p.settings.provider_settings_busy())
}

/// Turns a provider on or off on the selected Host.
pub(crate) fn provider_switch(
    connection: Connection,
    agent: ReadSignal<ConnectionAgent>,
) -> ui::Switch {
    ui::switch(
        move || agent.with(|a| tr_format("{0}を使用", [&a.name])),
        move || agent.with(|a| a.enabled),
        move |enabled| {
            connection.dispatch(Intent::SetProviderEnabled(op::SetProviderEnabled {
                provider: agent.with_untracked(|a| a.provider),
                enabled,
            }))
        },
    )
    .enabled(move || {
        connection.settings.with(|p| {
            p.connected()
                && p.provider_settings_loaded()
                && !p.provider_settings_busy()
                && p.account_login().is_none()
        })
    })
}
