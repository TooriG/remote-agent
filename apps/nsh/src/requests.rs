use crate::l10n::{localized, tr};
use crate::{Connection, native};
use agent_core::{
    presentation::conversation::{answer_from_json, request_input_default},
    state::{Intent, operations as op},
};
use agent_protocol::requests::{Answer, ElicitationAnswer, ElicitationInput, RequestBody};
use nsh_core::{element::input, prelude::*};
use serde_json::json;

pub(crate) fn panels(following: Signal<bool>) -> impl IntoElement {
    let connection = use_context::<Connection>().unwrap();
    view()
        .gap_2()
        .child(each(
            move || {
                connection
                    .composer
                    .with(|p| p.controls.request.clone().into_iter().collect::<Vec<_>>())
            },
            |request: &agent_core::presentation::conversation::Request| request.id.clone(),
            move |request, _| {
                let initial = request.get_untracked();
                let response = signal(request_input_default(initial.request_body));
                let error = signal(None::<String>);
                let enabled = move || {
                    connection.composer.with(|p| p.controls.connected)
                        && request.with(|request| request.can_respond)
                        && !connection.publication.with(|p| {
                            matches!(
                                p.composer
                                    .request_phase(request.with(|request| request.id.clone())),
                                Some(op::OperationPhase::Running)
                            )
                        })
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
                view()
                    .gap_2()
                    .p_2()
                    .child(text(move || request.with(|r| localized(r.title.clone()))))
                    .child(text(move || request.with(|r| r.body.clone())).selectable(true))
                    .child(text(move || request.with(|r| r.details.clone())).selectable(true))
                    .child(each(
                        move || request.with(|r| r.request_body.choices().to_vec()),
                        |choice: &agent_protocol::requests::Choice| choice.id.clone(),
                        move |choice, _| {
                            native::text_button(
                                move || json!(choice.with(|c| localized(c.label.clone()))),
                                move || {
                                    let id = choice.with_untracked(|c| c.id.clone());
                                    let answer = if request.with_untracked(|r| {
                                        matches!(r.request_body, RequestBody::Permission { .. })
                                    }) {
                                        Answer::Permission { choice_id: id }
                                    } else {
                                        Answer::Approval { choice_id: id }
                                    };
                                    respond(answer);
                                },
                            )
                            .prop("enabled", move || json!(enabled()))
                        },
                    ))
                    .child(when(
                        move || {
                            request.with(|r| {
                                matches!(
                                    r.request_body,
                                    RequestBody::Elicitation { .. }
                                        | RequestBody::ToolExecution { .. }
                                )
                            })
                        },
                        move || {
                            let url = computed(move || {
                                request.with(|r| match &r.request_body {
                                    RequestBody::Elicitation {
                                        input: ElicitationInput::Url { url },
                                        ..
                                    } => Some(url.clone()),
                                    _ => None,
                                })
                            });
                            view()
                                .gap_2()
                                .child(when(
                                    move || url.get().is_some(),
                                    move || {
                                        native::text_button(
                                            json!(tr("リンクを開く")),
                                            move || {
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
                                            },
                                        )
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
                                            .h(px(100.0))
                                    },
                                ))
                                .child(
                                    native::text_button(json!(tr("確認して送信")), submit)
                                        .prop("enabled", move || json!(enabled())),
                                )
                                .child(when(
                                    move || {
                                        request.with(|r| {
                                            matches!(
                                                r.request_body,
                                                RequestBody::Elicitation { .. }
                                            )
                                        })
                                    },
                                    move || {
                                        view()
                                            .flex_row()
                                            .gap_2()
                                            .child(
                                                native::text_button(json!(tr("辞退")), move || {
                                                    respond(Answer::Elicitation {
                                                        action: ElicitationAnswer::Decline,
                                                    })
                                                })
                                                .prop("enabled", move || json!(enabled())),
                                            )
                                            .child(
                                                native::text_button(
                                                    json!(tr("キャンセル")),
                                                    move || {
                                                        respond(Answer::Elicitation {
                                                            action: ElicitationAnswer::Cancel,
                                                        })
                                                    },
                                                )
                                                .prop("enabled", move || json!(enabled())),
                                            )
                                    },
                                ))
                        },
                    ))
                    .child(when(
                        move || error.get().is_some(),
                        move || text(move || error.get().unwrap_or_default()),
                    ))
                    .child(text(move || {
                        connection.composer.with(|p| {
                            p.request_error(request.with(|request| request.id.clone()))
                                .map(|error| localized(error.to_string()))
                                .unwrap_or_default()
                        })
                    }))
            },
        ))
        .child(each(
            move || {
                connection
                    .composer
                    .with(|p| p.controls.question.clone().into_iter().collect::<Vec<_>>())
            },
            |prompt: &agent_core::questions::QuestionPrompt| {
                (prompt.request_id.clone(), prompt.question.id.clone())
            },
            move |prompt, _| {
                let enabled = move || {
                    connection.composer.with(|p| p.controls.connected)
                        && prompt.with(|p| p.can_respond)
                        && !connection.publication.with(|p| {
                            matches!(
                                p.composer
                                    .request_phase(prompt.with(|p| p.request_id.clone())),
                                Some(op::OperationPhase::Running)
                            )
                        })
                };
                view()
                    .gap_2()
                    .p_2()
                    .child(text(move || prompt.with(|p| p.question.header.clone())))
                    .child(text(move || prompt.with(|p| p.question.prompt.clone())))
                    .child(text(move || {
                        prompt.with(|p| p.progress.clone().map(localized).unwrap_or_default())
                    }))
                    .child(when(
                        move || prompt.with(|p| p.status.is_some()),
                        move || {
                            text(move || {
                                prompt.with(|p| p.status.clone().map(localized).unwrap_or_default())
                            })
                        },
                    ))
                    .child(each(
                        move || prompt.with(|p| p.question.choices.clone()),
                        |choice: &agent_protocol::requests::QuestionChoice| choice.id.clone(),
                        move |choice, _| {
                            view()
                                .gap_1()
                                .child(
                                    native::text_button(
                                        move || json!(choice.with(|c| c.label.clone())),
                                        move || {
                                            following.set(true);
                                            connection.dispatch(Intent::AnswerQuestion {
                                                request_id: prompt
                                                    .with_untracked(|p| p.request_id.clone()),
                                                action:
                                                    agent_core::questions::QuestionAction::Choose {
                                                        choice_id: choice
                                                            .with_untracked(|c| c.id.clone()),
                                                    },
                                            })
                                        },
                                    )
                                    .prop("enabled", move || json!(enabled()))
                                    .prop(
                                        "selected",
                                        move || {
                                            json!(prompt.with(|p| {
                                                p.chosen_choice_ids
                                                    .contains(&choice.with(|c| c.id.clone()))
                                            }))
                                        },
                                    ),
                                )
                                .child(text(move || choice.with(|c| c.description.clone())))
                        },
                    ))
                    .child(when(
                        move || prompt.with(|p| p.can_go_back),
                        move || {
                            native::text_button(json!(tr("前の質問へ")), move || {
                                connection.dispatch(Intent::AnswerQuestion {
                                    request_id: prompt.with_untracked(|p| p.request_id.clone()),
                                    action: agent_core::questions::QuestionAction::Back,
                                })
                            })
                            .prop("enabled", move || json!(enabled()))
                        },
                    ))
                    .child(
                        native::text_button(json!(tr("スキップ")), move || {
                            following.set(true);
                            connection.dispatch(Intent::Respond(op::Respond {
                                request_id: prompt.with_untracked(|p| p.request_id.clone()),
                                answer: Answer::SkipQuestions,
                            }));
                        })
                        .prop("enabled", move || json!(enabled())),
                    )
                    .child(text(move || {
                        connection.composer.with(|p| {
                            p.request_error(prompt.with(|p| p.request_id.clone()))
                                .map(|error| localized(error.to_string()))
                                .unwrap_or_default()
                        })
                    }))
            },
        ))
}
