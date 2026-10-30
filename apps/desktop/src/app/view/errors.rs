use super::*;
use agent_core::presentation::error::ErrorNotice;

/// An error notice as the error list shows it.
#[derive(PartialEq)]
pub(in crate::app) struct ErrorRow {
    notice: ErrorNotice,
    /// Raised by this window rather than the Store.
    local: bool,
    recovery: Option<String>,
}

impl Desktop {
    pub(in crate::app) fn local_error_scope(&self) -> ErrorScope {
        if self.onboarding || self.import_projects || self.project_editor.is_some() {
            ErrorScope::Projects
        } else if self.tab == Tab::Settings {
            ErrorScope::Settings
        } else {
            ErrorScope::Conversation {
                draft_key: self.draft_key().clone(),
            }
        }
    }

    pub(in crate::app) fn error_rows(&self, scope: &ErrorScope) -> Vec<ErrorRow> {
        let mut rows: Vec<_> = self
            .snapshot
            .error_notices(scope.clone())
            .into_iter()
            .map(|notice| ErrorRow {
                recovery: self
                    .snapshot
                    .error_recovery(notice.id.clone())
                    .map(|recovery| recovery.label),
                notice,
                local: false,
            })
            .collect();
        if let Some(message) = self.local_errors.get(scope) {
            rows.push(ErrorRow {
                notice: ErrorNotice {
                    id: op::ErrorId::Application {
                        message: error_message(message),
                    },
                    context: scope.label().into(),
                    title: "操作に失敗しました".into(),
                    message: error_message(message),
                },
                local: true,
                recovery: None,
            });
        }
        rows
    }

    pub(super) fn error_notices(&self, scope: ErrorScope, cx: &Context<Self>) -> Div {
        let reconnect = (scope == ErrorScope::Application && !self.snapshot.connected)
            .then_some(self.connecting);
        Self::error_list(&self.error_rows(&scope), scope, reconnect, cx)
    }

    /// `reconnect` offers to reconnect, disabled while connecting.
    pub(super) fn error_list(
        rows: &[ErrorRow],
        scope: ErrorScope,
        reconnect: Option<bool>,
        cx: &Context<Self>,
    ) -> Div {
        v_flex()
            .gap_2()
            .min_w_0()
            .children(rows.iter().enumerate().map(|(index, row)| {
                let local = row.local;
                let notice = row.notice.clone();
                let target = scope.clone();
                let dismiss_target = scope.clone();
                let copied = format!("{} · {}\n{}", notice.context, notice.title, notice.message);
                let dismissed = notice.clone();
                let recovered = notice.id.clone();
                let detail = notice.message.clone();
                h_flex()
                    .debug_selector(move || format!("error-notice-{}-{index}", target.label()))
                    .items_start()
                    .gap_3()
                    .p_3()
                    .min_w_0()
                    .rounded_lg()
                    .bg(rgb(appearance::RAISED))
                    .border_1()
                    .border_color(rgba(0xff8e8638))
                    .child(Icon::new(IconName::TriangleAlert).text_color(rgb(0xff8e86)))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(appearance::MUTED))
                                    .child(notice.context),
                            )
                            .child(div().text_sm().font_semibold().child(notice.title))
                            .child(
                                div()
                                    .id(format!("error-details-{}-{index}", scope.label()))
                                    .text_sm()
                                    .text_color(rgb(appearance::MUTED))
                                    .line_clamp(3)
                                    .tooltip(move |window, cx| {
                                        tooltip::Tooltip::new(detail.clone()).build(window, cx)
                                    })
                                    .child(notice.message),
                            )
                            .when_some(row.recovery.clone(), |body, label| {
                                body.child(Self::button(
                                    format!("recover-error-{index}"),
                                    label,
                                    cx,
                                    move |view, _, _| {
                                        if let Some(recovery) =
                                            view.snapshot.error_recovery(recovered.clone())
                                        {
                                            view.dispatch(recovery.intent)
                                        }
                                    },
                                ))
                            })
                            .when_some(reconnect, |body, connecting| {
                                body.child(
                                    Self::button(
                                        format!("reconnect-error-{index}"),
                                        "再接続",
                                        cx,
                                        |view, window, cx| {
                                            view.switch_host(view.remote.clone(), window, cx)
                                        },
                                    )
                                    .disabled(connecting),
                                )
                            }),
                    )
                    .child(Self::icon_button(
                        format!("copy-error-{}-{index}", scope.label()),
                        IconName::Copy,
                        "エラーの詳細をコピー",
                        cx,
                        move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(copied.clone()))
                        },
                    ))
                    .child(
                        Self::icon_button(
                            format!("dismiss-error-{}-{index}", scope.label()),
                            IconName::Close,
                            "エラーを閉じる",
                            cx,
                            move |view, _, _| {
                                if local {
                                    if view
                                        .local_errors
                                        .get(&dismiss_target)
                                        .map(|message| error_message(message))
                                        .as_deref()
                                        == Some(dismissed.message.as_str())
                                    {
                                        view.local_errors.remove(&dismiss_target);
                                    }
                                } else {
                                    view.dispatch(Intent::DismissError {
                                        id: dismissed.id.clone(),
                                    });
                                }
                            },
                        )
                        .debug_selector({
                            let scope = scope.clone();
                            move || format!("dismiss-error-{}-{index}", scope.label())
                        }),
                    )
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::{Arc, Desktop, DraftKey, ErrorScope, Mode, Runtime, appearance, platform};
    use gpui_kit as gpui;
    use gpui_kit::{Modifiers, TestAppContext, px, size};

    #[gpui::test]
    fn conversation_notice_hides_on_navigation_and_stays_dismissed_without_hiding_global_notice(
        cx: &mut TestAppContext,
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        cx.update(|cx| {
            appearance::init(cx);
            cx.set_global(Runtime {
                handle: runtime.handle().clone(),
                connections: Arc::new(platform::Connections::default()),
                closing: tokio_util::task::TaskTracker::new(),
                logging_error: None,
            });
        });
        let first = DraftKey::from("first");
        let mut handle = None;
        let (_, window) = cx.add_window_view(|window, cx| {
            let desktop = Desktop::new(
                Mode::SideChat {
                    remote: None,
                    cwd: "/fixture".into(),
                },
                window,
                cx,
            );
            handle = Some(cx.entity());
            desktop
        });
        let desktop = handle.unwrap();
        window.update(|_, cx| {
            desktop.update(cx, |view, cx| {
                let snapshot = Arc::make_mut(&mut view.snapshot);
                snapshot.connected = true;
                Arc::make_mut(&mut snapshot.navigation).draft_key = first.clone();
                view.local_errors
                    .insert(ErrorScope::Application, "draft persistence failed".into());
                view.local_errors.insert(
                    ErrorScope::Conversation {
                        draft_key: first.clone(),
                    },
                    "attachment failed".into(),
                );
                cx.notify();
            })
        });
        window.simulate_resize(size(px(900.), px(700.)));
        window.run_until_parked();
        assert!(window.debug_bounds("error-notice-アプリ全体-0").is_some());
        assert!(window.debug_bounds("error-notice-この会話-0").is_some());
        for key in [DraftKey::from("other"), first.clone()] {
            window.update(|_, cx| {
                desktop.update(cx, |view, cx| {
                    Arc::make_mut(&mut Arc::make_mut(&mut view.snapshot).navigation).draft_key =
                        key.clone();
                    cx.notify();
                })
            });
            window.run_until_parked();
            assert_eq!(
                window.debug_bounds("error-notice-この会話-0").is_some(),
                key == first
            );
            assert!(window.debug_bounds("error-notice-アプリ全体-0").is_some());
        }
        let dismiss = window.debug_bounds("dismiss-error-この会話-0").unwrap();
        window.simulate_click(dismiss.center(), Modifiers::default());
        window.run_until_parked();
        assert!(window.debug_bounds("error-notice-この会話-0").is_none());
        assert!(window.debug_bounds("error-notice-アプリ全体-0").is_some());
    }
}
