use crate::l10n::{localized, tr, tr_format};
use crate::{Connection, native};
use agent_core::presentation::folder_menu::{FolderChoice, folder_choices};
use agent_core::state::{Intent, operations as op};
use nsh_core::{GlassEffect, element::input, prelude::*};
use serde_json::json;

pub(crate) fn composer(following: Signal<bool>) -> impl IntoElement {
    let connection = use_context::<Connection>().unwrap();
    let preview = use_context::<crate::media::Preview>().unwrap();
    let files = use_context::<crate::files::Browser>().unwrap();
    let composing = signal(false);
    let focused = signal(false);
    let field_ref = connection.composer_ref;
    let file_pick = signal(0_u64);
    let file_target = stored(None::<(String, agent_core::state::DraftKey)>);
    #[cfg(dictation)]
    let dictation = crate::dictation::Dictation::new();
    #[cfg(dictation)]
    let (recording, requesting) = (dictation.active, dictation.requesting);
    #[cfg(not(dictation))]
    let (recording, requesting) = (signal(false), signal(false));
    let composer = connection.composer;
    let controls = computed(move || composer.with(|c| c.controls.clone()));
    let draft = computed(move || composer.with(|c| c.draft.text.clone()));
    let answering = computed(move || {
        if recording.get() || requesting.get() || controls.with(|c| c.transcribing) {
            None
        } else {
            controls.with(|c| c.question.clone())
        }
    });
    // A question's answer has no attachments; the draft keeps them.
    let can_attach = move || {
        composer.with(|c| c.ready_for_input())
            && !recording.get()
            && !requesting.get()
            && !connection.uploading()
            && answering.with(Option::is_none)
    };
    let stopping = move || {
        !recording.get()
            && !requesting.get()
            && !controls.with(|c| c.transcribing)
            && controls.with(|c| c.running_turn.is_some())
            && answering.with(Option::is_none)
            && composer.with(|c| c.draft.text.trim().is_empty() && c.draft.attachments.is_empty())
    };
    let stop_label = move || {
        controls.with(|c| {
            if c.interrupt_phase == Some(op::OperationPhase::Running) {
                tr("停止中")
            } else {
                tr("停止")
            }
        })
    };
    let primary_label = computed(move || {
        if let Some(question) = answering.get() {
            question.action
        } else if stopping() {
            stop_label().into()
        } else if recording.get() {
            tr("文字起こしして送信").into()
        } else {
            tr("送信").into()
        }
    });
    let enabled = move || {
        !composing.get()
            && !requesting.get()
            && composer.with(|c| {
                if answering.with(Option::is_some) {
                    c.can_advance_question()
                } else if stopping() {
                    c.can_interrupt()
                } else {
                    !connection.uploading() && c.can_submit(recording.get())
                }
            })
    };
    let collapsed = move || {
        !recording.get()
            && !requesting.get()
            && !focused.get()
            && !connection.uploading()
            && draft.with(String::is_empty)
            && composer.with(|c| {
                c.draft.attachments.is_empty()
                    && c.controls.navigation.thread_id.is_some()
                    && c.controls.question.is_none()
                    && !c.controls.transcribing
                    && !c.controls.transferring
            })
    };
    let interrupt = move || {
        if composer.with_untracked(|c| c.can_interrupt())
            && let Some((thread_id, turn_id)) = controls
                .with_untracked(|c| c.navigation.thread_id.clone().zip(c.running_turn.clone()))
        {
            connection.dispatch(Intent::Interrupt(op::Interrupt { thread_id, turn_id }));
        }
    };
    let submit = move || {
        if !enabled() {
            return;
        }
        #[cfg(dictation)]
        if dictation.active.get_untracked() {
            following.set(true);
            dictation.finish(connection.client.get(), true);
            return;
        }
        if let Some(question) = answering.get_untracked() {
            following.set(true);
            connection.dispatch(Intent::AnswerQuestion {
                request_id: question.request_id,
                action: agent_core::questions::QuestionAction::Advance,
            });
        } else if stopping() {
            interrupt();
        } else if !composing.get_untracked() {
            following.set(true);
            connection.dispatch(Intent::Submit {
                thread_id: controls.with_untracked(|c| c.navigation.thread_id.clone()),
                client_user_message_id: uuid::Uuid::new_v4().to_string().into(),
            });
        }
    };
    let prompt = move || -> String {
        answering.with(|question| {
            question
                .as_ref()
                .map(|question| localized(question.placeholder.clone()))
                .unwrap_or_else(|| {
                    if is_desktop() {
                        tr("AI に依頼する")
                    } else {
                        tr("メッセージを入力")
                    }
                    .into()
                })
        })
    };
    let field = move || {
        input()
            .element_ref(&field_ref)
            .auto_focus(connection.pane.is_some())
            .multiline(true)
            .value(move || {
                answering
                    .with(|q| q.as_ref().map(|q| q.text.clone()))
                    .unwrap_or_else(|| draft.get())
            })
            .placeholder(prompt)
            // A multiline field shows its placeholder as a separate label, so
            // screen readers name the field by the same prompt.
            .label(prompt)
            .masked(move || answering.with(|q| q.as_ref().is_some_and(|q| q.question.secret)))
            .readonly(move || {
                recording.get()
                    || requesting.get()
                    || controls.with(|c| c.transcribing)
                    || answering.with(|q| {
                        q.as_ref().is_some_and(|q| {
                            !q.can_respond
                                || !q.question.allow_free_text
                                || composer.with(|c| c.request_phase(q.request_id.clone()))
                                    == Some(op::OperationPhase::Running)
                        })
                    })
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
                if let Some(question) = answering.get_untracked() {
                    connection.dispatch(Intent::AnswerQuestion {
                        request_id: question.request_id,
                        action: agent_core::questions::QuestionAction::Edit { text: text.into() },
                    });
                } else {
                    connection.dispatch(Intent::EditComposer {
                        thread_id: controls.with_untracked(|c| c.navigation.draft_key.clone()),
                        text: text.into(),
                        cursor: text.len() as u32,
                    });
                }
            })
            // As the desktop client's composer, a pasted image (a screenshot) is
            // attached. The field pastes text itself, so a paste stays undoable;
            // NSH settles that before the app hears of the paste, so text copied
            // with an image is pasted as well.
            .on_paste(move |paste: &nsh_core::PasteEvent| {
                let images: Vec<_> = paste
                    .files
                    .iter()
                    .filter(|file| file.mime.starts_with("image/"))
                    .map(|file| crate::client::AttachmentSource::Image {
                        name: file.name.clone(),
                        bytes: file.data.clone(),
                    })
                    .collect();
                if !images.is_empty() && can_attach() {
                    let host = connection.host.get_untracked();
                    let key = controls.with_untracked(|c| c.navigation.draft_key.clone());
                    connection
                        .application
                        .with(|app| app.attach(host, connection.pane, key, images));
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
        #[cfg(dictation)]
        {
            dictation.button()
        }
        #[cfg(not(dictation))]
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
                    tr("添付"),
                    native::choices(&[tr("写真を選択"), tr("ファイルを選択")]),
                    move |item| {
                        let host = connection.host.get_untracked();
                        let key = controls.with_untracked(|c| c.navigation.draft_key.clone());
                        if item == tr("写真を選択") {
                            let picked = pick_images(nsh_core::picker::PickRequest::new(true, 10));
                            spawn(async move {
                                let images = picked.await;
                                connection.application.with(|app| {
                                    app.attach(
                                        host,
                                        connection.pane,
                                        key,
                                        images
                                            .into_iter()
                                            .map(|file| crate::client::AttachmentSource::Image {
                                                name: file.name,
                                                bytes: file.data.to_vec(),
                                            })
                                            .collect(),
                                    )
                                });
                            });
                        } else if !is_desktop() {
                            file_target.replace(Some((host, key)));
                            file_pick.update(|value| *value += 1);
                        } else {
                            pick_files(connection.application.get(), host, connection.pane, key);
                        }
                    },
                )
                .prop(
                    "appearance",
                    json!(if is_desktop() { "plain" } else { "automatic" }),
                )
                .prop("enabled", move || json!(can_attach()))
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
                view()
                    .child(when(
                        move || answering.with(Option::is_none),
                        dictation_button,
                    ))
                    .child(when(
                        move || {
                            answering.with(Option::is_some)
                                && controls.with(|c| c.running_turn.is_some())
                        },
                        move || {
                            native::button("stop.fill", tr("停止"), interrupt)
                                .min_w(px(44.0))
                                .min_h(px(44.0))
                                .prop("enabled", move || {
                                    json!(composer.with(|c| c.can_interrupt()))
                                })
                                .prop("label", move || json!(stop_label()))
                        },
                    ))
                    .col_start(move || if collapsed() { 3_i16 } else { 4_i16 })
                    .row_start(move || if collapsed() { 1_i16 } else { 2_i16 }),
            )
            .child(
                native::button("arrow.up", tr("送信"), submit)
                    .min_w(px(44.0))
                    .min_h(px(44.0))
                    .col_start(move || if collapsed() { 4_i16 } else { 5_i16 })
                    .row_start(move || if collapsed() { 1_i16 } else { 2_i16 })
                    .prop("symbol", move || {
                        json!(if answering.with(Option::is_some) {
                            None
                        } else {
                            Some(if stopping() { "stop.fill" } else { "arrow.up" })
                        })
                    })
                    .prop("title", move || {
                        json!(
                            answering
                                .with(|q| q.as_ref().map(|q| q.action.clone()).unwrap_or_default())
                        )
                    })
                    .prop("label", move || json!(primary_label.get()))
                    .prop("appearance", move || {
                        json!(if is_desktop() && answering.with(Option::is_none) {
                            "circular"
                        } else {
                            "prominent"
                        })
                    })
                    .prop("enabled", move || json!(enabled())),
            )
    };
    // The existing desktop client's composer: the field over one row of controls.
    let desktop_box = move || {
        use crate::ui::{self, Enabled, Icon, colors};
        let inactive = move || {
            !enabled() || (composer.with(|c| c.draft.text.trim().is_empty()) && !stopping())
        };
        let controls_row = view().flex_row().items_center().child(when(
            move || answering.with(Option::is_none),
            move || {
                ui::icon_button_sized(
                    Icon::PLUS,
                    tr("ファイルを添付"),
                    40.0,
                    24.0,
                    move || {
                        let host = connection.host.get_untracked();
                        let key = controls.with_untracked(|c| c.navigation.draft_key.clone());
                        pick_files(connection.application.get(), host, connection.pane, key);
                    },
                )
                .enabled(can_attach)
            },
        ));
        #[cfg(desktop)]
        let controls_row = controls_row.child(crate::permissions::control());
        view()
            .element_ref(&connection.composer_box)
            .w_full()
            .p(px(7.0))
            .gap_2()
            .rounded(px(22.0))
            .bg(colors::SURFACE)
            .border_1()
            .border_color(colors::BORDER)
            .child(
                view()
                    .px_2()
                    .pt(px(10.0))
                    .pb_1()
                    .child(field().w_full().h(px(40.0))),
            )
            .child(
                controls_row
                    .child(view().flex_1())
                    .child(crate::model_controls::controls())
                    .child(when(
                        move || answering.with(Option::is_none),
                        dictation_button,
                    ))
                    .child(when(
                        move || answering.with(Option::is_some),
                        move || {
                            ui::text_button(
                                move || {
                                    SharedString::from(answering.with(|q| {
                                        q.as_ref().map(|q| q.action.clone()).unwrap_or_default()
                                    }))
                                },
                                submit,
                            )
                            .enabled(enabled)
                        },
                    ))
                    .child(when(
                        move || answering.with(Option::is_none),
                        move || {
                            view()
                                .w(px(40.0))
                                .h(px(40.0))
                                .rounded_full()
                                .items_center()
                                .justify_center()
                                .role(AccessibilityRole::Button)
                                .label(move || primary_label.get())
                                .bg(move || {
                                    if inactive() {
                                        colors::BORDER
                                    } else {
                                        rgb(0xffffff)
                                    }
                                })
                                .text_color(move || {
                                    if inactive() {
                                        colors::MUTED
                                    } else {
                                        colors::SIDEBAR
                                    }
                                })
                                .style_when(inactive, |s| s.opacity(0.5))
                                // VoiceOver says so when it cannot send or stop.
                                .disabled(move || !enabled())
                                .on_press(submit)
                                .child(when(move || !stopping(), || ui::icon(Icon::ARROW_UP, 24.0)))
                                .child(when(stopping, || ui::icon(Icon::STOP, 24.0)))
                        },
                    )),
            )
    };
    let desktop_context = move || {
        use crate::ui::{self, Enabled, Icon};
        let hosts = computed(move || connection.state.with(|s| s.hosts.clone()));
        let directory = computed(move || connection.context.with(|p| p.selected_directory()));
        let choose = move || {
            let host = connection.host.get_untracked();
            let selected = directory.get_untracked();
            let choices = connection.state.with_untracked(|state| {
                let list = state
                    .list_inputs()
                    .into_iter()
                    .find(|(list_host, _)| *list_host == host)
                    .map(|(_, list)| list);
                folder_choices(list.as_deref(), &selected, host == "local")
            });
            ui::popup(&folder_items(&choices), move |id| {
                if let Some(cwd) = id.strip_prefix(FOLDER) {
                    connection.dispatch(Intent::NewChat { cwd: cwd.into() });
                    field_ref.focus();
                } else if id == MORE_PROJECTS {
                    connection.application.with(|application| {
                        application.request_list(crate::application::ListRequest::MoreProjects)
                    });
                } else if id == CHOOSE_FOLDER {
                    connection.state.with_untracked(|state| {
                        crate::projects::add_folder(
                            connection.application,
                            &connection.host.get_untracked(),
                            &state.list_sources(),
                        )
                    });
                }
            });
        };
        view()
            .flex_row()
            .items_center()
            .gap_2()
            .px_2()
            .child(
                ui::labeled_button(
                    Icon::FOLDER,
                    move || {
                        let directory = directory.get();
                        if directory.is_empty() {
                            tr("チャット").to_owned()
                        } else {
                            agent_core::presentation::markdown::file_basename(&directory).to_owned()
                        }
                    },
                    move || {
                        let directory = directory.get();
                        if directory.is_empty() {
                            tr("フォルダ: チャット").to_owned()
                        } else {
                            tr_format("フォルダ: {0}", [&directory])
                        }
                    },
                    choose,
                )
                .enabled(move || {
                    !recording.get()
                        && !requesting.get()
                        && !connection.uploading()
                        && controls.with(|c| !c.sending && !c.transcribing && !c.transferring)
                }),
            )
            .child(crate::connections::destination(hosts, connection.host))
    };
    let column = view()
        .w_full()
        .max_w(move || px(if is_desktop() { 828.0 } else { 780.0 }))
        .self_center()
        .px(move || if is_desktop() { px(24.0) } else { px(12.0) })
        .pt(move || if is_desktop() { px(8.0) } else { px(12.0) })
        .pb(move || if is_desktop() { px(16.0) } else { px(12.0) })
        .gap(move || if is_desktop() { px(4.0) } else { px(8.0) });
    #[cfg(desktop)]
    let column = column.child(when(is_desktop, crate::desktop_workspace::review_card));
    // As the desktop client's composer, Escape discards a recording (or its
    // permission request) before it is transcribed.
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    let column = column.child(when(
        move || recording.get() || requesting.get(),
        move || {
            on_key_with(
                "escape",
                KeyOptions {
                    when: nsh_core::keymap::When::Always,
                    repeat: false,
                },
                move |_| dictation.cancel(),
            );
            view()
        },
    ));
    column
        .child(crate::requests::panels(following))
        .child(when(
            move || {
                !composing.get()
                    && !recording.get()
                    && !requesting.get()
                    && !controls.with(|c| c.transcribing)
                    && answering.with(Option::is_none)
            },
            move || crate::invocations::suggestions(move || field_ref.focus()),
        ))
        .child(when(
            move || connection.uploading() || controls.with(|c| c.transferring),
            || text(tr("添付ファイルを転送中…")),
        ))
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
                            let sources = event
                                .data
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(serde_json::Value::as_str)
                                .map(|path| crate::client::AttachmentSource::File {
                                    path: path.into(),
                                    is_image: native::image_file(std::path::Path::new(path)),
                                })
                                .collect();
                            connection
                                .application
                                .with(|app| app.attach(host, connection.pane, key, sources));
                        }
                    })
            },
        ))
        .child(when(
            move || {
                answering.with(Option::is_none)
                    && composer.with(|c| !c.draft.attachments.is_empty())
            },
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
                            attachment(connection, preview, files, file, move || {
                                connection.dispatch(Intent::RemoveAttachment {
                                    draft_key: controls
                                        .with_untracked(|c| c.navigation.draft_key.clone()),
                                    index: index.get_untracked() as u32,
                                })
                            })
                        },
                    )))
            },
        ))
        .child(when(is_desktop, desktop_context))
        .child(when(is_desktop, desktop_box))
        .child(when(
            move || !is_desktop(),
            move || {
                view()
                    .p_2()
                    .gap_1()
                    .rounded(px(26.0))
                    .glass_effect(GlassEffect::Regular)
                    .child(actions())
            },
        ))
        .child(when(
            move || controls.with(|c| !c.connected),
            move || {
                text(tr("Host 未接続"))
                    .text_color(move || nsh_base::theme().muted_foreground)
                    .text_size(px(11.0))
            },
        ))
}

/// The folder menu's item IDs: a folder's carries its path after the prefix.
const FOLDER: &str = "folder:";
const MORE_PROJECTS: &str = "more-projects";
const CHOOSE_FOLDER: &str = "choose-folder";

fn folder_items(choices: &[FolderChoice]) -> serde_json::Value {
    json!(
        choices
            .iter()
            .map(|choice| match choice {
                FolderChoice::Folder {
                    path,
                    title,
                    checked,
                } => json!({"id": format!("{FOLDER}{path}"), "title": localized(title.clone()), "selected": checked}),
                FolderChoice::MoreProjects => {
                    json!({"id": MORE_PROJECTS, "title": tr("さらにプロジェクトを読み込む")})
                }
                FolderChoice::ChooseFolder => {
                    json!({"id": CHOOSE_FOLDER, "title": tr("別のフォルダを選択…")})
                }
            })
            .collect::<Vec<_>>()
    )
}

/// An attached file as T3 Code's composer chips: an image's thumbnail or a
/// file's name, which opens it, with its removal at the corner.
fn attachment(
    connection: Connection,
    preview: crate::media::Preview,
    files: crate::files::Browser,
    file: ReadSignal<agent_core::state::Attachment>,
    remove: impl Fn() + 'static,
) -> impl IntoElement {
    use crate::ui::{self, Icon, colors};
    let open = move || {
        let (cwd, home) = connection
            .publication
            .with_untracked(|p| (p.composer.controls.navigation.cwd.clone(), p.context.home()));
        files.open(
            file.with_untracked(|file| file.path.clone()),
            cwd,
            home,
            connection.host.get_untracked(),
            connection.client.get(),
        );
    };
    view()
        .rounded(px(ui::RADIUS))
        .border_1()
        .border_color(colors::BORDER)
        .bg(colors::RAISED)
        .overflow_hidden()
        .child(when(
            move || file.with(|file| file.is_image),
            move || {
                crate::media::image_view(
                    connection,
                    preview,
                    move || file.with(|file| file.path.clone()),
                    120.0,
                    120.0,
                )
            },
        ))
        .child(when(
            move || file.with(|file| !file.is_image),
            move || {
                view()
                    .h(px(40.0))
                    .pl_1()
                    .pr(px(32.0))
                    .justify_center()
                    .child(ui::labeled_button(
                        Icon::FILE_TEXT,
                        move || file.with(|file| file.name.clone()),
                        move || file.with(|file| file.name.clone()),
                        open,
                    ))
            },
        ))
        .child(
            view()
                .absolute()
                .top(px(4.0))
                .right(px(4.0))
                .rounded_full()
                .bg(colors::SURFACE)
                .child(ui::small_icon_button(Icon::CLOSE, tr("添付を削除"), remove)),
        )
}

/// Both the composer and desktop workspace use the OS picker and Core's upload owner.
pub(crate) fn pick_files(
    application: std::sync::Arc<crate::application::Application>,
    host: String,
    pane: Option<u64>,
    key: agent_core::state::DraftKey,
) {
    let picked = pick_paths(nsh_core::file_dialog::OpenDialog::files().multiple(true));
    spawn(async move {
        match picked.await {
            Ok(Some(paths)) => application.attach(
                host,
                pane,
                key,
                paths
                    .into_iter()
                    .map(|path| {
                        let is_image = native::image_file(&path);
                        crate::client::AttachmentSource::File { path, is_image }
                    })
                    .collect(),
            ),
            Ok(None) => {}
            Err(reason) => native::message(tr("添付"), reason),
        }
    });
}
