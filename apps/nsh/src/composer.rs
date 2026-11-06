use crate::shared::Shared;
use crate::{Connection, native};
use agent_core::{
    presentation::view::Presentation,
    state::{Intent, operations as op},
};
use nsh_core::{GlassEffect, element::input, prelude::*};
use serde_json::json;

pub(crate) fn composer(publication: Memo<Shared<Presentation>>) -> impl IntoElement {
    let connection = use_context::<Connection>().unwrap();
    let files = use_context::<crate::files::Browser>().unwrap();
    let composing = signal(false);
    let focused = signal(false);
    let file_pick = signal(0_u64);
    let file_target = stored(None::<(String, agent_core::state::DraftKey)>);
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    let dictation = crate::dictation::Dictation::new();
    let composer = computed(move || publication.with(|p| Shared(p.composer.clone())));
    let controls = computed(move || composer.with(|c| c.controls.clone()));
    let draft = computed(move || composer.with(|c| c.draft.text.clone()));
    let stopping = move || {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        if dictation.active.get() {
            return false;
        }
        controls.with(|c| c.running_turn.is_some() && c.question.is_none())
            && composer.with(|c| c.draft.text.trim().is_empty() && c.draft.attachments.is_empty())
    };
    let enabled = move || {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        if dictation.requesting.get() {
            return false;
        }
        !composing.get()
            && controls
                .with(|c| c.connected && !c.sending && !c.transcribing && c.unavailable.is_none())
            && controls
                .with(|c| c.question.as_ref().map(|question| question.can_advance))
                .unwrap_or_else(|| {
                    #[cfg(any(target_os = "macos", target_os = "ios"))]
                    if dictation.active.get() {
                        return true;
                    }
                    stopping()
                        || composer.with(|c| {
                            !c.draft.text.trim().is_empty() || !c.draft.attachments.is_empty()
                        })
                })
    };
    let collapsed = move || {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        if dictation.active.get() || dictation.requesting.get() {
            return false;
        }
        !focused.get()
            && draft.with(String::is_empty)
            && composer.with(|c| {
                c.draft.attachments.is_empty()
                    && c.controls.navigation.thread_id.is_some()
                    && c.controls.question.is_none()
                    && !c.controls.transcribing
            })
    };
    let submit = move || {
        if !enabled() {
            return;
        }
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        if dictation.active.get_untracked() {
            dictation.finish(connection.client.get(), true);
            return;
        }
        let controls = controls.get_untracked();
        if let Some(question) = controls.question {
            connection.dispatch(Intent::AnswerQuestion {
                request_id: question.request_id,
                action: agent_core::questions::QuestionAction::Advance,
            });
        } else if stopping()
            && let Some(turn_id) = controls.running_turn
        {
            if let Some(thread_id) = controls.navigation.thread_id {
                connection.dispatch(Intent::Interrupt(op::Interrupt { thread_id, turn_id }));
            }
        } else if !composing.get_untracked() {
            connection.dispatch(Intent::Submit {
                thread_id: controls.navigation.thread_id,
                client_user_message_id: uuid::Uuid::new_v4().to_string().into(),
            });
        }
    };
    let field = move || {
        input()
            .multiline(true)
            .value(move || {
                controls
                    .with(|c| c.question.as_ref().map(|q| q.text.clone()))
                    .unwrap_or_else(|| draft.get())
            })
            .placeholder(move || {
                controls.with(|c| {
                    c.question
                        .as_ref()
                        .map(|question| question.placeholder.clone())
                        .unwrap_or_else(|| "メッセージを入力".into())
                })
            })
            .masked(move || {
                controls.with(|c| c.question.as_ref().is_some_and(|q| q.question.secret))
            })
            .text_color(move || nsh_base::theme().foreground)
            .placeholder_color(move || color(nsh_base::theme().muted_foreground))
            .submit_behavior(if is_desktop() {
                SubmitBehavior::Submit
            } else {
                SubmitBehavior::Newline
            })
            .on_submit(move |_: &str| submit())
            .on_change(move |text: &str| {
                if let Some(question) = controls.with_untracked(|c| c.question.clone()) {
                    connection.dispatch(Intent::AnswerQuestion {
                        request_id: question.request_id,
                        action: agent_core::questions::QuestionAction::Edit { text: text.into() },
                    });
                } else {
                    connection.dispatch(Intent::SetDraftText {
                        thread_id: controls.with_untracked(|c| c.navigation.draft_key.clone()),
                        text: text.into(),
                    });
                }
            })
            .on_focus(move || focused.set(true))
            .on_blur(move || focused.set(false))
            .on_composition_change(move |event: &Composition| composing.set(event.composing))
            .auto_correct(false)
            .auto_capitalize(AutoCapitalize::None)
            .h(move || if collapsed() { px(36.0) } else { px(64.0) })
    };
    let dictation_button = move || {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            dictation.button()
        }
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        {
            view()
        }
    };
    let actions = move || {
        view()
            .grid()
            .grid_template_columns(move || {
                if collapsed() {
                    "44px minmax(0, 1fr) auto auto"
                } else {
                    "44px minmax(0, 1fr) auto auto auto"
                }
            })
            .grid_template_rows(move || if collapsed() { "auto" } else { "auto auto" })
            .items_center()
            .gap_1()
            .child(
                field()
                    .min_w_0()
                    .w_full()
                    .col_start(move || if collapsed() { 2_i16 } else { 1_i16 })
                    .col_end(move || if collapsed() { 3_i16 } else { 6_i16 })
                    .row_start(1),
            )
            .child(
                native::menu(
                    "plus",
                    "添付",
                    native::choices(&["写真を選択", "ファイルを選択"]),
                    move |item| {
                        let host = connection.host.get_untracked();
                        let key = controls.with_untracked(|c| c.navigation.draft_key.clone());
                        if item == "写真を選択" {
                            let picked = pick_images(nsh_core::picker::PickRequest::new(true, 10));
                            spawn(async move {
                                for file in picked.await {
                                    connection.client.with(|client| {
                                        client.upload(
                                            host.clone(),
                                            key.clone(),
                                            file.name,
                                            file.data.to_vec(),
                                        )
                                    });
                                }
                            });
                        } else if !is_desktop() {
                            file_target.replace(Some((host, key)));
                            file_pick.update(|value| *value += 1);
                        } else {
                            let picked = pick_paths(nsh_core::file_dialog::OpenDialog::default());
                            spawn(async move {
                                match picked.await {
                                    Ok(Some(paths)) => {
                                        for path in paths {
                                            let is_image = native::image_file(&path);
                                            connection.client.with(|client| {
                                                client.attach(&host, key.clone(), path, is_image)
                                            });
                                        }
                                    }
                                    Ok(None) => {}
                                    Err(reason) => native::message("添付", reason),
                                }
                            });
                        }
                    },
                )
                .min_h(px(44.0))
                .min_w(px(44.0))
                .col_start(1)
                .row_start(move || if collapsed() { 1_i16 } else { 2_i16 }),
            )
            .child(when(
                move || !collapsed(),
                || crate::model_controls::controls().col_start(3).row_start(2),
            ))
            .child(
                dictation_button()
                    .col_start(move || if collapsed() { 3_i16 } else { 4_i16 })
                    .row_start(move || if collapsed() { 1_i16 } else { 2_i16 }),
            )
            .child(
                native::button("arrow.up", "送信", submit)
                    .min_w(px(44.0))
                    .min_h(px(44.0))
                    .col_start(move || if collapsed() { 4_i16 } else { 5_i16 })
                    .row_start(move || if collapsed() { 1_i16 } else { 2_i16 })
                    .prop("symbol", move || {
                        json!(if stopping() { "stop.fill" } else { "arrow.up" })
                    })
                    .prop("label", move || {
                        json!(if stopping() { "停止" } else { "送信" })
                    })
                    .prop("appearance", json!("prominent"))
                    .prop("enabled", move || json!(enabled())),
            )
    };
    view()
        .w_full()
        .max_w(px(780.0))
        .self_center()
        .p_3()
        .gap_2()
        .child(crate::requests::panels())
        .child(when(
            move || !is_desktop(),
            move || {
                native("bex.filePicker")
                    .h(px(0.0))
                    .prop("open", move || json!(file_pick.get()))
                    .on_native_event(move |event: &NativeEvent| {
                        if &*event.name == "picked"
                            && let Some((host, key)) = file_target.replace(None)
                        {
                            for path in event
                                .data
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(serde_json::Value::as_str)
                            {
                                connection.client.with(|client| {
                                    client.attach(
                                        &host,
                                        key.clone(),
                                        path.into(),
                                        native::image_file(std::path::Path::new(path)),
                                    )
                                });
                            }
                        }
                    })
            },
        ))
        .child(when(
            move || composer.with(|c| !c.draft.attachments.is_empty()),
            move || {
                scroll()
                    .horizontal()
                    .h(move || {
                        px(
                            if composer
                                .with(|c| c.draft.attachments.iter().any(|file| file.is_image))
                            {
                                136.0
                            } else {
                                60.0
                            },
                        )
                    })
                    .child(view().flex_row().items_start().gap_3().p_2().child(each(
                        move || composer.with(|c| c.draft.attachments.clone()),
                        |file: &agent_core::state::Attachment| file.path.clone(),
                        move |file, index| {
                            view()
                                .child(when(
                                    move || file.with(|file| file.is_image),
                                    move || {
                                        crate::media::image_view(
                                            move || file.with(|file| file.path.clone()),
                                            120.0,
                                            120.0,
                                        )
                                    },
                                ))
                                .child(when(
                                    move || file.with(|file| !file.is_image),
                                    move || {
                                        native::text_button(
                                            move || json!(file.with(|file| file.name.clone())),
                                            move || {
                                                let (cwd, home) =
                                                    connection.publication.with_untracked(|p| {
                                                        (
                                                            p.composer
                                                                .controls
                                                                .navigation
                                                                .cwd
                                                                .clone(),
                                                            p.context.home(),
                                                        )
                                                    });
                                                files.open(
                                                    file.with_untracked(|file| file.path.clone()),
                                                    cwd,
                                                    home,
                                                    connection.host.get_untracked(),
                                                    connection.client.get(),
                                                );
                                            },
                                        )
                                        .max_w(px(240.0))
                                        .min_h(px(44.0))
                                        .pr(px(44.0))
                                    },
                                ))
                                .child(
                                    native::button("xmark", "添付を削除", move || {
                                        connection.dispatch(Intent::RemoveAttachment {
                                            draft_key: controls
                                                .with_untracked(|c| c.navigation.draft_key.clone()),
                                            index: index.get_untracked() as u32,
                                        })
                                    })
                                    .prop("appearance", json!("glass"))
                                    .absolute()
                                    .top(px(0.0))
                                    .right(px(0.0))
                                    .w(px(44.0))
                                    .h(px(44.0)),
                                )
                        },
                    )))
            },
        ))
        .child(
            view()
                .p_2()
                .gap_1()
                .rounded(px(26.0))
                .glass_effect(GlassEffect::Regular)
                .child(actions()),
        )
        .child(
            text(move || controls.with(|c| if c.connected { "" } else { "Host 未接続" }))
                .text_color(move || nsh_base::theme().muted_foreground)
                .text_size(px(11.0)),
        )
}
