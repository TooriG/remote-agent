use crate::{Connection, application::Host, connections, native};
use agent_core::state::{Intent, operations as op};
use nsh_core::prelude::*;
use serde_json::json;

pub(crate) fn overview(hosts: Memo<Vec<Host>>, close: impl Fn() + 'static) -> impl IntoElement {
    let connection = use_context::<Connection>().unwrap();
    let publication = connection.publication;
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
    scroll().flex_1().child(
        view()
            .p_4()
            .gap_4()
            .child(text("接続").text_size(px(20.0)))
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .child(
                        view()
                            .flex_1()
                            .child(text(move || {
                                publication.with(|p| {
                                    p.settings.host_name().unwrap_or_else(|| "接続先".into())
                                })
                            }))
                            .child(text(move || {
                                connection.settings.with(|p| p.connection_status_label())
                            })),
                    )
                    .child(
                        native::switch(
                            json!("このPCへの接続"),
                            move || json!(connection.settings.with(|p| p.connection_enabled())),
                            move |enabled| {
                                connection.dispatch(Intent::SetConnectionEnabled { enabled });
                            },
                        )
                        .prop("enabled", move || json!(!connection.host.get().is_empty())),
                    ),
            )
            .child(text("Bex Host").text_size(px(20.0)))
            .child(
                text(move || {
                    publication.with(|p| {
                        p.settings
                            .host_software()
                            .map(|software| {
                                format!(
                                    "{} · ビルド {}\n{}",
                                    software.version, software.build, software.revision
                                )
                            })
                            .unwrap_or_default()
                    })
                })
                .selectable(true),
            )
            .child(
                native::text_button(
                    json!("バージョン情報を再読み込み"),
                    move || {
                        connection.dispatch(Intent::ReadHostSoftware(op::ReadHostSoftware {}));
                    },
                )
                .prop("enabled", move || {
                    json!(connection.settings.with(|p| p.connected()))
                }),
            )
            .child(native::text_button(
                json!("アップデートを確認"),
                || {
                    native::open_link("https://github.com/ttizze/remote-agent/releases");
                },
            ))
            .child(text("更新はこのPCでBexを開いて適用できます。"))
            .child(text("プロジェクトと会話").text_size(px(20.0)))
            .child(
                native::text_button(json!("ワークツリー設定"), crate::worktrees::open)
                    .prop("symbol", json!("arrow.triangle.branch"))
                    .prop("enabled", move || {
                        json!(connection.settings.with(|p| p.connected()))
                    }),
            )
            .child(text("プロバイダー").text_size(px(20.0)))
            .child(
                native::text_button(json!("プロバイダーを再読み込み"), move || {
                    connection.dispatch(Intent::ReadProviderSettings(op::ReadProviderSettings {}));
                })
                .prop("enabled", move || {
                    json!(
                        publication.with(
                            |p| p.settings.connected() && !p.settings.provider_settings_busy()
                        )
                    )
                }),
            )
            .child(each(
                move || connection.settings.with(|p| p.connection_setup().agents),
                |agent: &agent_core::presentation::connections::ConnectionAgent| agent.provider,
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
                                .child(text(move || agent.with(|a| a.label.clone())))
                                .child(text(move || {
                                    agent.with(|a| {
                                        a.error
                                            .as_ref()
                                            .map(ToString::to_string)
                                            .unwrap_or_default()
                                    })
                                })),
                        )
                        .child(
                            native::switch(
                                move || json!(agent.with(|a| format!("{}を使用", a.name))),
                                move || json!(agent.with(|a| a.enabled)),
                                move |enabled| {
                                    connection.dispatch(Intent::SetProviderEnabled(
                                        op::SetProviderEnabled {
                                            provider: agent.with_untracked(|a| a.provider),
                                            enabled,
                                        },
                                    ))
                                },
                            )
                            .prop("enabled", move || {
                                json!(connection.settings.with(|p| p.connected()
                                    && p.provider_settings_loaded()
                                    && !p.provider_settings_busy()
                                    && p.account_login().is_none()))
                            }),
                        )
                },
            ))
            .child(text(
                "選択したPCに適用され、同じ環境を使う端末にも反映されます。",
            ))
            .child(connections::screen(hosts, close)),
    )
}
