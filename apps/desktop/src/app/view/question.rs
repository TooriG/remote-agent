//! The question the composer answers, docked above it as on iOS and in T3 Code.
use super::composer::ComposerAction;
use super::*;
use agent_core::questions::{QuestionAction, QuestionPrompt};

#[derive(PartialEq)]
pub(super) struct ComposerQuestion {
    pub(super) prompt: QuestionPrompt,
    /// Its answer is being sent.
    pub(super) busy: bool,
    pub(super) error: Option<agent_core::presentation::error::ErrorText>,
    pub(super) collapsed: bool,
    pub(super) can_advance: bool,
}

impl Desktop {
    pub(in crate::app) fn question_prompt(&self) -> Option<QuestionPrompt> {
        self.snapshot.question_prompt(self.selected()?.clone())
    }

    pub(super) fn composer_question(&self) -> Option<ComposerQuestion> {
        let prompt = self.question_prompt()?;
        let key = op::OperationKey::Request {
            id: prompt.request_id.clone(),
        };
        let busy = self.snapshot.operation_running(key.clone());
        Some(ComposerQuestion {
            can_advance: prompt.can_advance && self.snapshot.connection.connected && !busy,
            error: self.snapshot.operation_error(key),
            collapsed: self
                .question
                .collapsed
                .as_ref()
                .is_some_and(|(request, id)| {
                    request == &prompt.request_id && id == &prompt.question.id
                }),
            busy,
            prompt,
        })
    }

    fn answer_question(&self, request_id: RequestId, action: QuestionAction) {
        self.dispatch(Intent::AnswerQuestion { request_id, action });
    }

    /// Saves typed text as the active question's own answer.
    pub(in crate::app) fn answer_typed(&mut self, value: SharedString) {
        if !self.question.text.differs(&value) {
            return;
        }
        let Some(prompt) = self.question_prompt() else {
            return;
        };
        let revision = self.question.text.record(value.clone());
        self.perform(
            Intent::AnswerQuestion {
                request_id: prompt.request_id,
                action: QuestionAction::Edit {
                    text: value.to_string(),
                },
            },
            OperationCompletion::Answer(revision),
        );
    }

    /// Enter answers; Shift+Enter keeps a line break in a multi-line answer.
    pub(super) fn answer_enter(
        &mut self,
        action: &gpui_kit::component::input::Enter,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(question) = self.composer_question() else {
            return;
        };
        if action.shift
            || action.secondary
            || self
                .question
                .composing(question.prompt.question.secret, window, cx)
        {
            return;
        }
        cx.stop_propagation();
        if question.can_advance {
            self.answer_question(question.prompt.request_id, QuestionAction::Advance);
        }
    }
}

impl super::composer::ComposerContent {
    pub(super) fn question_panel(question: &ComposerQuestion, cx: &Context<Self>) -> AnyElement {
        let language = crate::localization::language(cx);
        let prompt = &question.prompt;
        let definition = &prompt.question;
        let inactive = question.busy || !prompt.can_respond;
        let question_key = (prompt.request_id.clone(), definition.id.clone());
        let collapsed = question.collapsed;
        let header = h_flex()
            .gap_2()
            .items_center()
            .child(
                h_flex()
                    .flex_shrink_0()
                    .gap_1p5()
                    .text_sm()
                    .text_color(rgb(appearance::MUTED))
                    .child(
                        Icon::default()
                            .path("icons/message-circle-question.svg")
                            .size_4(),
                    )
                    .child(agent_core::localization::localized_ui_text(
                        language,
                        prompt.status.clone().unwrap_or_else(|| "質問".into()),
                    )),
            )
            .when(!definition.header.is_empty(), |row| {
                row.child(
                    div()
                        .min_w_0()
                        .text_sm()
                        .font_semibold()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(definition.header.clone()),
                )
            })
            .child(div().flex_1())
            .when_some(prompt.progress.clone(), |row, progress| {
                row.child(
                    div()
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(rgb(appearance::MUTED))
                        .child(progress),
                )
            })
            .when(question.busy, |row| row.child(crate::activity::indicator()))
            .child(
                icon_button(
                    "question-toggle",
                    if collapsed {
                        IconName::ChevronUp
                    } else {
                        IconName::ChevronDown
                    },
                    language.text(if collapsed {
                        "質問を開く"
                    } else {
                        "質問を閉じる"
                    }),
                    cx,
                    move |_, _, cx| {
                        cx.emit(ComposerAction::QuestionCollapsed(
                            (!collapsed).then(|| question_key.clone()),
                        ))
                    },
                )
                .debug_selector(|| "question-toggle".into())
                .disabled(inactive),
            );
        let mut panel = v_flex()
            .w_full()
            .gap_3()
            .p_4()
            .rounded(px(22.))
            .bg(rgb(appearance::SURFACE))
            .border_1()
            .border_color(rgb(appearance::BORDER))
            .child(header);
        if collapsed {
            panel = panel.child(
                div()
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .text_ellipsis()
                    .text_color(rgb(appearance::MUTED))
                    .child(definition.prompt.clone()),
            );
        } else {
            let request_id = prompt.request_id.clone();
            panel = panel
                .child(definition.prompt.clone())
                .when(definition.multiple, |panel| {
                    panel.child(
                        div()
                            .text_xs()
                            .text_color(rgb(appearance::MUTED))
                            .child(language.text("複数選択できます")),
                    )
                })
                .child(Self::question_choices(prompt, inactive, cx))
                .child(
                    h_flex().justify_end().child(
                        Button::new("question-skip")
                            .debug_selector(|| "question-skip".into())
                            .label(language.text("スキップする"))
                            .outline()
                            .rounded_full()
                            .px_5()
                            .disabled(inactive)
                            .on_click(cx.listener(move |_, _, _, cx| {
                                cx.emit(ComposerAction::Respond(
                                    request_id.clone(),
                                    Answer::SkipQuestions,
                                ));
                                cx.notify();
                            })),
                    ),
                );
        }
        panel
            .when_some(question.error.as_ref(), |panel, error| {
                panel.child(
                    div()
                        .text_xs()
                        .text_color(rgb(0xff8e86))
                        .child(error_view(error, language)),
                )
            })
            .into_any_element()
    }

    /// Natural height, scrolling only when the options exceed the limit.
    fn question_choices(prompt: &QuestionPrompt, inactive: bool, cx: &Context<Self>) -> AnyElement {
        let choices = &prompt.question.choices;
        let mut list = v_flex()
            .id("question-choices")
            .max_h(px(240.))
            .overflow_y_scroll();
        for (index, choice) in choices.iter().enumerate() {
            if index > 0 {
                list = list.child(div().h(px(1.)).bg(rgb(appearance::BORDER)));
            }
            let chosen = prompt.chosen_choice_ids.contains(&choice.id);
            let request_id = prompt.request_id.clone();
            let choice_id = choice.id.clone();
            let selector = format!("question-choice-{}", choice.id);
            list = list.child(
                Button::new(selector.clone())
                    .debug_selector(move || selector.clone())
                    .accessibility_label(if choice.description.is_empty() {
                        choice.label.clone()
                    } else {
                        format!("{}, {}", choice.label, choice.description)
                    })
                    .toggled(chosen)
                    .ghost()
                    .w_full()
                    .h_auto()
                    .py_3()
                    .disabled(inactive)
                    .child(
                        h_flex()
                            .w_full()
                            .min_w_0()
                            .gap_3()
                            .child(choice_mark(chosen))
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap_1()
                                    .whitespace_normal()
                                    .text_left()
                                    .child(choice.label.clone())
                                    .when(!choice.description.is_empty(), |column| {
                                        column.child(
                                            div()
                                                .text_xs()
                                                .text_color(rgb(appearance::MUTED))
                                                .child(choice.description.clone()),
                                        )
                                    }),
                            ),
                    )
                    .on_click(cx.listener(move |view, _, _, _| {
                        view.dispatch(Intent::AnswerQuestion {
                            request_id: request_id.clone(),
                            action: QuestionAction::Choose {
                                choice_id: choice_id.clone(),
                            },
                        });
                    })),
            );
        }
        list.into_any_element()
    }

    /// Replace dictation and sending while the composer answers a question.
    pub(super) fn question_actions(
        question: &ComposerQuestion,
        cx: &Context<Self>,
    ) -> impl IntoIterator<Item = Button> {
        let language = crate::localization::language(cx);
        let prompt = &question.prompt;
        let back = prompt.can_go_back.then(|| {
            let request_id = prompt.request_id.clone();
            button(
                "question-back",
                language.text("前へ"),
                cx,
                move |view, _, _| {
                    view.dispatch(Intent::AnswerQuestion {
                        request_id: request_id.clone(),
                        action: QuestionAction::Back,
                    })
                },
            )
            .debug_selector(|| "question-back".into())
            .h(px(40.))
        });
        let request_id = prompt.request_id.clone();
        let advance = Button::new("question-advance")
            .debug_selector(|| "question-advance".into())
            .label(language.text(&prompt.action).to_owned())
            .primary()
            .rounded_full()
            .h(px(32.))
            .px_4()
            .disabled(!question.can_advance)
            .on_click(cx.listener(move |view, _, _, _| {
                view.dispatch(Intent::AnswerQuestion {
                    request_id: request_id.clone(),
                    action: QuestionAction::Advance,
                });
            }));
        back.into_iter().chain([advance])
    }
}

fn choice_mark(chosen: bool) -> Div {
    let mark = div()
        .flex_shrink_0()
        .size_6()
        .rounded_full()
        .flex()
        .items_center()
        .justify_center();
    if chosen {
        mark.bg(rgb(appearance::FOREGROUND)).child(
            Icon::new(IconName::Check)
                .size_3p5()
                .text_color(rgb(appearance::BACKGROUND)),
        )
    } else {
        mark.border_2().border_color(rgb(appearance::BORDER))
    }
}
