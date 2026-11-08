use crate::l10n::{localized, tr};
use crate::{Connection, native, shared::Shared, workbench::Workbench};
use agent_core::{
    panels::PanelSurface,
    presentation::terminals::terminal_grid,
    state::{Intent, operations as op},
};
use nsh_core::prelude::*;
use serde_json::json;

pub(crate) fn screen(surface: PanelSurface, host: String) -> impl IntoElement {
    let connection = use_context::<Connection>().unwrap();
    let workspace = use_context::<Workbench>().unwrap();
    let host = stored(host);
    let cwd = stored(surface.path.unwrap_or_default());
    let handle = stored(workspace.terminal_handle(surface.id));
    let started = stored(false);
    let size = signal(None::<agent_protocol::operations::TerminalSize>);
    let resource = computed(move || {
        connection
            .publication
            .with(|p| p.snapshot.terminals.get(&handle.get()).cloned().map(Shared))
    });
    let terminal = computed(move || {
        resource.get();
        connection
            .publication
            .with_untracked(|p| p.snapshot.terminal_view(handle.get()))
    });
    let input = computed(move || {
        terminal.with(|terminal| {
            terminal
                .as_ref()
                .is_some_and(|terminal| terminal.accepts_input)
        })
    });
    let loading = computed(move || {
        terminal.with(|terminal| terminal.as_ref().is_some_and(|terminal| terminal.loading))
    });
    let send = move |intent| {
        connection
            .client
            .with(|client| client.dispatch(&host.get(), None, intent))
    };
    let start = move |size| {
        started.replace(true);
        send(Intent::StartTerminal(op::StartTerminal {
            handle: handle.get(),
            cwd: cwd.get(),
            size,
        }));
    };
    let resize_target = computed(move || (size.get(), input.get()));
    effect(move || {
        let (Some(size), true) = resize_target.get() else {
            return;
        };
        let actual =
            resource.with_untracked(|terminal| terminal.as_ref().map(|terminal| terminal.size));
        if actual != Some(size) {
            // Layout can finish before the Starting publication arrives. Retain that
            // size and apply it once core accepts input, including after reconnecting.
            send(Intent::ResizeTerminal(op::ResizeTerminal {
                handle: handle.get(),
                size,
            }));
        }
    });
    cleanup(move || {
        if started.get() {
            send(Intent::DetachTerminal(op::DetachTerminal {
                handle: handle.get(),
            }));
        }
    });
    view()
        .flex_1()
        .min_h_0()
        .gap_2()
        .child(
            text(move || cwd.get())
                .text_size(px(12.0))
                .selectable(true)
                .truncate(),
        )
        .child(text(move || {
            if loading.get() {
                tr("接続中…").to_owned()
            } else {
                terminal.with(|terminal| {
                    terminal
                        .as_ref()
                        .and_then(|terminal| terminal.status.clone())
                        .map(localized)
                        .unwrap_or_default()
                })
            }
        }))
        .child(when(
            move || {
                terminal.with(|terminal| {
                    terminal
                        .as_ref()
                        .is_some_and(|terminal| terminal.status.is_some())
                })
            },
            move || {
                native::text_button(json!(tr("再接続")), move || {
                    if let Some(size) = size.get_untracked() {
                        start(size);
                    }
                })
                .prop("enabled", move || {
                    json!(
                        size.get().is_some()
                            && connection.settings.with(|p| p.connected())
                            && !loading.get()
                    )
                })
            },
        ))
        .child(
            native("bex.terminal")
                .prop("input", move || json!(input.get()))
                .prop("output", move || {
                    terminal.with(|terminal| {
                        terminal
                            .as_ref()
                            .map(|terminal| json!(&terminal.output))
                            .unwrap_or_else(|| json!([]))
                    })
                })
                .on_native_event(move |event: &NativeEvent| match &*event.name {
                    "resize" => {
                        let Some(next) = event.data["cols"]
                            .as_i64()
                            .and_then(|cols| i32::try_from(cols).ok())
                            .zip(
                                event.data["rows"]
                                    .as_i64()
                                    .and_then(|rows| i32::try_from(rows).ok()),
                            )
                            .and_then(|(cols, rows)| terminal_grid(cols, rows))
                        else {
                            return;
                        };
                        if size.get_untracked() == Some(next) {
                            return;
                        }
                        size.set(Some(next));
                        if !started.get() {
                            start(next);
                        }
                    }
                    "input" if input.get_untracked() => {
                        if let Ok(data) = serde_json::from_value::<Vec<u8>>(event.data.clone()) {
                            send(Intent::WriteTerminal(op::WriteTerminal {
                                handle: handle.get(),
                                data,
                            }));
                        }
                    }
                    "ack" => {
                        if let Some(sequence) = event.data.as_u64() {
                            send(Intent::AcknowledgeTerminal {
                                handle: handle.get(),
                                sequence,
                            });
                        }
                    }
                    _ => {}
                })
                .flex_1()
                .min_h_0(),
        )
}
