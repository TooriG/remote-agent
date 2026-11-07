use crate::{
    Connection,
    files::{Browser, TextFile},
    media::Preview,
    native,
};
use agent_core::{
    presentation::files::{
        file_edit_prompt, file_line_start_utf16, file_preview_truncation_notice,
    },
    state::{Intent, operations as op},
};
use nsh_core::{element::input, prelude::*};
use serde_json::json;

pub(crate) fn screen(target: ReadSignal<TextFile>) -> impl Element {
    let connection = use_context::<Connection>().unwrap();
    let browser = use_context::<Browser>().unwrap();
    let preview = use_context::<Preview>().unwrap();
    let path = stored(target.with_untracked(|target| target.path.clone()));
    let host = stored(connection.host.get_untracked());
    let draft = stored(
        connection
            .composer
            .with_untracked(|p| p.controls.navigation.draft_key.clone()),
    );
    let controls = computed(move || {
        connection
            .publication
            .with(|p| p.snapshot.file_editor(&path.get()))
    });
    let busy = move || {
        connection.publication.with(|p| {
            p.snapshot.operation_running(op::OperationKey::File)
                || p.snapshot
                    .operation_running(op::OperationKey::SaveFile { path: path.get() })
        })
    };
    let field = element_ref();
    let composing = signal(false);
    let laid_out = signal(false);
    let position = computed(move || {
        (
            target.with(|target| target.line),
            controls.with(|controls| controls.loaded_revision.clone()),
            laid_out.get(),
        )
    });
    effect(move || {
        if let (Some(line), Some(_), true) = position.get() {
            let offset = file_line_start_utf16(
                controls.with_untracked(|controls| controls.text.clone()),
                line,
            );
            field.set_selection(offset, offset);
        }
    });
    view()
        .flex_1()
        .min_h_0()
        .gap_2()
        .p_2()
        .pb_keyboard()
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .child(
                    text(move || path.get())
                        .truncate()
                        .selectable(true)
                        .text_size(px(12.0))
                        .flex_1()
                        .min_w_0(),
                )
                .child(
                    native::text_button(json!("保存"), move || {
                        connection.client.with(|client| {
                            client.dispatch(
                                &host.get(),
                                None,
                                Intent::SaveFile(op::SaveFile { path: path.get() }),
                            )
                        })
                    })
                    .prop("enabled", move || {
                        json!(
                            controls.with(|controls| controls.can_save)
                                && !busy()
                                && !composing.get()
                        )
                    }),
                ),
        )
        .child(text(move || {
            controls.with(|controls| controls.conflict_notice.clone().unwrap_or_default())
        }))
        .child(when(busy, || text("処理中…")))
        .child(when(
            move || controls.with(|controls| controls.truncated),
            || text(file_preview_truncation_notice()),
        ))
        .child(
            input()
                .multiline(true)
                .submit_behavior(SubmitBehavior::Newline)
                .readonly(move || controls.with(|controls| !controls.can_edit))
                .on_layout(
                    move |_: &nsh_core::geometry::Bounds<nsh_core::geometry::Points>| {
                        laid_out.set(true)
                    },
                )
                .value(move || controls.with(|controls| controls.text.clone()))
                .on_change(move |text: &str| {
                    connection.client.with(|client| {
                        client.dispatch(
                            &host.get(),
                            None,
                            Intent::SetFileDraft {
                                path: path.get(),
                                text: text.into(),
                            },
                        )
                    })
                })
                .on_composition_change(move |event: &Composition| composing.set(event.composing))
                .auto_correct(false)
                .auto_capitalize(AutoCapitalize::None)
                .font_family(SharedString::from("monospace"))
                .text_size(if is_desktop() { px(14.0) } else { px(17.0) })
                .text_color(move || nsh_base::theme().foreground)
                .element_ref(&field)
                .flex_1()
                .min_h_0()
                .w_full(),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .child(
                    native::button("arrow.clockwise", "再読込", move || {
                        let modified = controls.with_untracked(|controls| controls.modified);
                        spawn(async move {
                            if modified
                                && !nsh_base::alert_dialog()
                                    .title("再読込")
                                    .description("保存していない編集を破棄して再読込しますか？")
                                    .cancel("cancel", "キャンセル")
                                    .destructive("reload", "編集を破棄して再読込")
                                    .show()
                                    .await
                                    .is_ok_and(|answer| answer.action.as_deref() == Some("reload"))
                            {
                                return;
                            }
                            connection.client.with(|client| {
                                client.dispatch(
                                    &host.get(),
                                    None,
                                    Intent::ReadFile(op::ReadFile {
                                        path: path.get(),
                                        discard_draft: true,
                                    }),
                                )
                            });
                        });
                    })
                    .prop("enabled", move || json!(!busy()))
                    .w(px(44.0))
                    .h(px(44.0)),
                )
                .child(
                    native::button(
                        "square.and.arrow.up",
                        "プレビュー・共有",
                        move || {
                            preview.load(crate::media::receive(
                                connection.client.get().download(host.get(), path.get()),
                            ))
                        },
                    )
                    .w(px(44.0))
                    .h(px(44.0)),
                )
                .child(native::text_button(json!("AIで編集"), move || {
                    connection.client.with(|client| {
                        client.dispatch(
                            &host.get(),
                            None,
                            Intent::SetDraftText {
                                thread_id: draft.get(),
                                text: file_edit_prompt(path.get()),
                            },
                        )
                    });
                    browser.close();
                })),
        )
}

pub(crate) fn editors() -> impl Element {
    let browser = use_context::<Browser>().unwrap();
    view().flex_1().min_h_0().child(each(
        move || browser.file.get().into_iter().collect::<Vec<_>>(),
        |target: &TextFile| target.id,
        |target, _| screen(target),
    ))
}
