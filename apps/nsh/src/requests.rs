//! Requests wait above the composer as T3 Code's cards: an approval's
//! decisions in Core's tones, an input request's answer, and the question the
//! composer answers, with its choices.
use crate::l10n::{localized, tr};
use crate::ui::{self, Enabled, Icon, colors};
use crate::{Connection, native};
use agent_core::{
    presentation::conversation::{
        ActionTone, Request, answer_from_json, choice_tone, request_input_default,
    },
    questions::{QuestionAction, QuestionPrompt},
    state::{Intent, operations as op},
};
use agent_protocol::{
    ids::RequestId,
    requests::{Answer, ElicitationAnswer, ElicitationInput, RequestBody},
};
use nsh_core::{
    element::{Rgba, input, rgb, rgba},
    prelude::*,
};

const ERROR: Rgba = rgb(0xff8e86);

pub(crate) fn panels(following: Signal<bool>) -> impl IntoElement {
    let connection = use_context::<Connection>().unwrap();
    view()
        .gap_2()
        .child(each(
            move || {
                connection
                    .context
                    .with(|p| p.docked_request().into_iter().collect::<Vec<_>>())
            },
            |request: &Request| request.id.clone(),
            move |request, _| request_card(connection, following, request),
        ))
        .child(each(
            move || {
                connection
                    .composer
                    .with(|p| p.controls.question.clone().into_iter().collect::<Vec<_>>())
            },
            |prompt: &QuestionPrompt| (prompt.request_id.clone(), prompt.question.id.clone()),
            move |prompt, _| question_card(connection, following, prompt),
        ))
}

/// Whether the request's answer is being sent.
fn sending(connection: Connection, id: RequestId) -> bool {
    connection.publication.with(|p| {
        matches!(
            p.context.request_phase(id),
            Some(op::OperationPhase::Running)
        )
    })
}

/// The failure of the request's last answer, below its card.
fn request_error(connection: Connection, id: impl Fn() -> RequestId + 'static) -> impl IntoElement {
    text(move || {
        connection.context.with(|p| {
            p.request_error(id())
                .map(|error| localized(error.to_string()))
                .unwrap_or_default()
        })
    })
    .selectable(true)
    .text_size(px(12.0))
    .text_color(ERROR)
}

fn request_card(
    connection: Connection,
    following: Signal<bool>,
    request: ReadSignal<Request>,
) -> impl IntoElement {
    let response = signal(request_input_default(
        request.with_untracked(|r| r.request_body.clone()),
    ));
    let error = signal(None::<String>);
    let enabled = move || {
        connection.composer.with(|p| p.controls.connected)
            && request.with(|request| request.can_respond)
            && !sending(connection, request.with(|request| request.id.clone()))
    };
    let respond = move |answer| {
        following.set(true);
        connection.dispatch(Intent::Respond(op::Respond {
            request_id: request.with_untracked(|r| r.id.clone()),
            answer,
        }))
    };
    let submit = move || match answer_from_json(
        &request.with_untracked(|r| r.request_body.clone()),
        &response.get_untracked(),
    ) {
        Ok(answer) => {
            error.set(None);
            respond(answer);
        }
        Err(reason) => error.set(Some(localized(reason))),
    };
    let answered = move || {
        request.with(|r| {
            matches!(
                r.request_body,
                RequestBody::Elicitation { .. } | RequestBody::ToolExecution { .. }
            )
        })
    };
    ui::card(vec![
        view()
            .px_5()
            .py_4()
            .gap_3()
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .text_color(colors::MUTED)
                    .child(ui::icon(Icon::SHIELD, 14.0))
                    .child(
                        text(move || request.with(|r| localized(r.title.clone())))
                            .text_size(px(13.0)),
                    ),
            )
            .child(
                text(move || request.with(|r| r.body.clone()))
                    .selectable(true)
                    .text_size(px(14.0)),
            )
            .child(when(
                move || request.with(|r| !r.details.is_empty()),
                move || {
                    text(move || request.with(|r| r.details.clone()))
                        .selectable(true)
                        .text_size(px(12.0))
                        .text_color(colors::MUTED)
                        .p_3()
                        .rounded(px(ui::RADIUS))
                        .bg(colors::RAISED)
                },
            ))
            .child(decisions(request, enabled, respond))
            .child(when(answered, move || {
                answer_form(request, response, enabled, submit, respond)
            }))
            .child(when(
                move || error.get().is_some(),
                move || {
                    text(move || error.get().unwrap_or_default())
                        .selectable(true)
                        .text_size(px(12.0))
                        .text_color(ERROR)
                },
            ))
            .child(request_error(connection, move || {
                request.with(|request| request.id.clone())
            })),
    ])
}

/// An approval's or a permission's choices: their descriptions, then the
/// decisions in the tones Core ranks them with.
fn decisions(
    request: ReadSignal<Request>,
    enabled: impl Fn() -> bool + Copy + 'static,
    respond: impl Fn(Answer) + Copy + 'static,
) -> impl IntoElement {
    let choices = move || request.with(|r| r.request_body.choices().to_vec());
    view()
        .gap_3()
        .child(each(
            choices,
            |choice: &agent_protocol::requests::Choice| choice.id.clone(),
            move |choice, _| {
                when(
                    move || choice.with(|c| !c.description.is_empty()),
                    move || {
                        text(move || choice.with(|c| c.description.clone()))
                            .text_size(px(13.0))
                            .text_color(colors::MUTED)
                    },
                )
            },
        ))
        .child(view().flex_row().flex_wrap().gap_2().child(each(
            choices,
            |choice: &agent_protocol::requests::Choice| choice.id.clone(),
            move |choice, _| {
                let choice = choice.get_untracked();
                let id = choice.id.clone();
                ui::action_button(
                    localized(choice.label),
                    choice_tone(choice.effect),
                    move || {
                        respond(
                            if request.with_untracked(|r| {
                                matches!(r.request_body, RequestBody::Permission { .. })
                            }) {
                                Answer::Permission {
                                    choice_id: id.clone(),
                                }
                            } else {
                                Answer::Approval {
                                    choice_id: id.clone(),
                                }
                            },
                        )
                    },
                )
                .enabled(enabled)
            },
        )))
}

/// An input request's answer: its link or field, then sending, declining or
/// cancelling it.
fn answer_form(
    request: ReadSignal<Request>,
    response: Signal<String>,
    enabled: impl Fn() -> bool + Copy + 'static,
    submit: impl Fn() + 'static,
    respond: impl Fn(Answer) + Copy + 'static,
) -> impl IntoElement {
    let url = computed(move || {
        request.with(|r| match &r.request_body {
            RequestBody::Elicitation {
                input: ElicitationInput::Url { url },
                ..
            } => Some(url.clone()),
            _ => None,
        })
    });
    let elicitation =
        move || request.with(|r| matches!(r.request_body, RequestBody::Elicitation { .. }));
    let refusal = move |label: &'static str, tone, action: ElicitationAnswer| {
        ui::action_button(label, tone, move || {
            respond(Answer::Elicitation {
                action: action.clone(),
            })
        })
        .enabled(enabled)
    };
    view()
        .gap_3()
        .child(when(
            move || url.get().is_some(),
            move || {
                view()
                    .flex_row()
                    .child(ui::text_button(tr("リンクを開く"), move || {
                        if let Some(url) = url.get_untracked() {
                            let opened = open_url(url);
                            spawn(async move {
                                if !opened.await {
                                    native::message(
                                        tr("リンクを開けませんでした"),
                                        tr("OSがリンクを開けませんでした。"),
                                    );
                                }
                            });
                        }
                    }))
            },
        ))
        .child(when(
            move || url.get().is_none(),
            move || {
                input()
                    .multiline(true)
                    .value(move || response.get())
                    .on_change(move |text: &str| response.set(text.into()))
                    .text_color(move || nsh_base::theme().foreground)
                    .text_size(px(13.0))
                    .h(px(100.0))
                    .p_2()
                    .rounded(px(ui::RADIUS))
                    .border_1()
                    .border_color(colors::BORDER)
            },
        ))
        .child(
            view()
                .flex_row()
                .flex_wrap()
                .gap_2()
                .child(
                    ui::action_button(tr("確認して送信"), ActionTone::Primary, submit)
                        .enabled(enabled),
                )
                .child(when(elicitation, move || {
                    refusal(tr("辞退"), ActionTone::Danger, ElicitationAnswer::Decline)
                }))
                .child(when(elicitation, move || {
                    refusal(
                        tr("キャンセル"),
                        ActionTone::Secondary,
                        ElicitationAnswer::Cancel,
                    )
                })),
        )
}

/// The question the composer answers: its header and progress, the prompt,
/// its choices, and going back or skipping.
fn question_card(
    connection: Connection,
    following: Signal<bool>,
    prompt: ReadSignal<QuestionPrompt>,
) -> impl IntoElement {
    let request_id = move || prompt.with(|p| p.request_id.clone());
    let busy = move || sending(connection, request_id());
    let enabled = move || {
        connection.composer.with(|p| p.controls.connected)
            && prompt.with(|p| p.can_respond)
            && !busy()
    };
    let answer = move |action| {
        connection.dispatch(Intent::AnswerQuestion {
            request_id: prompt.with_untracked(|p| p.request_id.clone()),
            action,
        })
    };
    ui::card(vec![
        view()
            .px_5()
            .py_4()
            .gap_3()
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .flex_shrink_0()
                            .text_color(colors::MUTED)
                            .child(ui::icon(Icon::QUESTION, 14.0))
                            .child(
                                text(move || {
                                    prompt.with(|p| {
                                        p.status
                                            .clone()
                                            .map(localized)
                                            .unwrap_or_else(|| tr("質問").into())
                                    })
                                })
                                .text_size(px(13.0)),
                            ),
                    )
                    .child(
                        text(move || prompt.with(|p| p.question.header.clone()))
                            .text_size(px(13.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .truncate()
                            .min_w_0(),
                    )
                    .child(view().flex_1())
                    .child(
                        text(move || {
                            prompt.with(|p| p.progress.clone().map(localized).unwrap_or_default())
                        })
                        .text_size(px(12.0))
                        .text_color(colors::MUTED),
                    )
                    .child(when(busy, ui::spinner)),
            )
            .child(
                text(move || prompt.with(|p| p.question.prompt.clone()))
                    .selectable(true)
                    .text_size(px(14.0)),
            )
            .child(when(
                move || prompt.with(|p| p.question.multiple),
                || {
                    text(tr("複数選択できます"))
                        .text_size(px(12.0))
                        .text_color(colors::MUTED)
                },
            ))
            .child(choices(prompt, enabled, move |choice_id| {
                following.set(true);
                answer(QuestionAction::Choose { choice_id });
            }))
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .justify_end()
                    .gap_2()
                    .child(when(
                        move || prompt.with(|p| p.can_go_back),
                        move || {
                            ui::text_button(tr("前の質問へ"), move || {
                                answer(QuestionAction::Back)
                            })
                            .enabled(enabled)
                        },
                    ))
                    .child(
                        ui::action_button(
                            tr("スキップする"),
                            ActionTone::Secondary,
                            move || {
                                following.set(true);
                                connection.dispatch(Intent::Respond(op::Respond {
                                    request_id: prompt.with_untracked(|p| p.request_id.clone()),
                                    answer: Answer::SkipQuestions,
                                }));
                            },
                        )
                        .enabled(enabled),
                    ),
            )
            .child(request_error(connection, request_id)),
    ])
}

/// The question's choices as rows divided by rules, each marked when chosen.
fn choices(
    prompt: ReadSignal<QuestionPrompt>,
    enabled: impl Fn() -> bool + Copy + 'static,
    choose: impl Fn(String) + Copy + 'static,
) -> impl IntoElement {
    each(
        move || prompt.with(|p| p.question.choices.clone()),
        |choice: &agent_protocol::requests::QuestionChoice| choice.id.clone(),
        move |choice, index| {
            let chosen = move || {
                prompt.with(|p| p.chosen_choice_ids.contains(&choice.with(|c| c.id.clone())))
            };
            nsh_base::button()
                .accessibility_label(move || {
                    Some(SharedString::from(choice.with(|c| c.label.clone())))
                })
                .flex_row()
                .items_center()
                .gap_3()
                .w_full()
                .px_2()
                .py_2()
                .rounded(px(ui::RADIUS))
                .text_color(colors::FOREGROUND)
                .hover(|s| s.bg(colors::GHOST_HOVER))
                .disabled_style(|s| s.opacity(0.5))
                .style_when(
                    move || index.get() > 0,
                    |s| s.border_t_1().border_t_color(colors::BORDER),
                )
                .on_press(move || choose(choice.with_untracked(|c| c.id.clone())))
                .enabled(enabled)
                .child(mark(chosen))
                .child(
                    view()
                        .flex_1()
                        .min_w_0()
                        .gap_1()
                        .child(text(move || choice.with(|c| c.label.clone())).text_size(px(14.0)))
                        .child(when(
                            move || choice.with(|c| !c.description.is_empty()),
                            move || {
                                text(move || choice.with(|c| c.description.clone()))
                                    .text_size(px(12.0))
                                    .text_color(colors::MUTED)
                            },
                        )),
                )
        },
    )
}

/// A chosen choice's filled check, else an empty ring.
fn mark(chosen: impl Fn() -> bool + Copy + 'static) -> impl IntoElement {
    view()
        .w(px(20.0))
        .h(px(20.0))
        .flex_shrink_0()
        .rounded_full()
        .items_center()
        .justify_center()
        .border_2()
        .border_color(move || {
            if chosen() {
                colors::FOREGROUND
            } else {
                colors::BORDER
            }
        })
        .bg(move || {
            if chosen() {
                colors::FOREGROUND
            } else {
                rgba(0x00000000)
            }
        })
        .text_color(colors::SURFACE)
        .child(when(chosen, || ui::icon(Icon::CHECK, 12.0)))
}
