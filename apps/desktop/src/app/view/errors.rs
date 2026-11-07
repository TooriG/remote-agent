use super::*;
use agent_core::presentation::error::{ErrorNotice, ErrorText};

/// Error copy in the UI language above its verbatim cause.
pub(in crate::app) fn error_view(text: &ErrorText) -> Div {
    v_flex().min_w_0().child(text.message.clone()).when_some(
        text.details.clone(),
        |body, details| {
            body.child(
                div()
                    .text_xs()
                    .text_color(rgb(appearance::MUTED))
                    .child(details),
            )
        },
    )
}

/// An error notice as the error list shows it.
#[derive(Clone, PartialEq)]
pub(in crate::app) struct ErrorRow {
    pub(in crate::app) notice: ErrorNotice,
    /// Raised by this window rather than the Store.
    pub(in crate::app) local: Option<uuid::Uuid>,
}

pub(in crate::app) struct LocalNotice {
    id: uuid::Uuid,
    message: ErrorSource,
    pub(in crate::app) lifetime: NoticeLifetime,
}

#[derive(Clone, Copy, PartialEq)]
pub(in crate::app) enum NoticeLifetime {
    Connection,
    Window,
}

impl LocalNotice {
    pub(in crate::app) fn new(message: impl Into<ErrorSource>, lifetime: NoticeLifetime) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            message: message.into(),
            lifetime,
        }
    }
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
                notice,
                local: None,
            })
            .collect();
        if let Some(notice) = self.local_errors.get(scope) {
            rows.push(local_error_row(
                self.snapshot.catalog.ui_language,
                scope,
                notice,
            ));
        }
        rows
    }

    pub(in crate::app) fn raise_notice(
        &mut self,
        scope: ErrorScope,
        message: impl Into<ErrorSource>,
    ) {
        self.local_errors
            .insert(scope, LocalNotice::new(message, NoticeLifetime::Connection));
    }

    pub(in crate::app) fn dismiss_notice(&mut self, scope: &ErrorScope, id: uuid::Uuid) {
        if self
            .local_errors
            .get(scope)
            .is_some_and(|notice| notice.id == id)
        {
            self.local_errors.remove(scope);
        }
    }

    pub(super) fn error_notices(&self, scope: ErrorScope, cx: &Context<Self>) -> Div {
        let disconnected = scope == ErrorScope::Application && !self.snapshot.connection.connected;
        let rows = self.error_rows(&scope);
        // A lost connection is not an error, but desktop waits for the user to reconnect.
        let idle = disconnected && !self.connecting && rows.is_empty();
        error_list(
            &rows,
            scope,
            disconnected.then_some(self.connecting),
            cx,
            Self::notice_action,
        )
        .when(idle, |list| list.child(Self::disconnected_notice(cx)))
    }

    fn disconnected_notice(cx: &Context<Self>) -> impl IntoElement {
        let language = crate::localization::language(cx);
        h_flex()
            .debug_selector(|| "host-disconnected".into())
            .items_center()
            .gap_3()
            .p_3()
            .min_w_0()
            .rounded_lg()
            .bg(rgb(appearance::RAISED))
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .text_color(rgb(appearance::MUTED))
                    .child(language.text("ホストに接続していません")),
            )
            .child(button(
                "reconnect-host",
                language.text("再接続"),
                cx,
                |view, window, cx| view.switch_host(view.remote.clone(), window, cx),
            ))
    }

    pub(in crate::app) fn notice_action(
        &mut self,
        action: NoticeAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            NoticeAction::Dispatch(intent) => self.dispatch(intent),
            NoticeAction::Reconnect => self.switch_host(self.remote.clone(), window, cx),
            NoticeAction::DismissLocal(scope, id) => self.dismiss_notice(&scope, id),
        }
    }
}

#[derive(Clone)]
pub(in crate::app) enum NoticeAction {
    Dispatch(Intent),
    Reconnect,
    DismissLocal(ErrorScope, uuid::Uuid),
}

/// `reconnect` offers to reconnect, disabled while connecting.
pub(in crate::app) fn error_list<V: 'static>(
    rows: &[ErrorRow],
    scope: ErrorScope,
    reconnect: Option<bool>,
    cx: &Context<V>,
    action: fn(&mut V, NoticeAction, &mut Window, &mut Context<V>),
) -> Div {
    let language = crate::localization::language(cx);
    v_flex()
        .gap_2()
        .min_w_0()
        .children(rows.iter().enumerate().map(|(index, row)| {
            let local = row.local;
            let notice = row.notice.clone();
            let target = scope.clone();
            let dismiss_target = scope.clone();
            let copied = format!("{} · {}\n{}", notice.context, notice.title, notice.text);
            let dismissed = notice.id.clone();
            let recovered = notice.id.clone();
            let detail = notice.text.to_string();
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
                                .child(error_view(&notice.text)),
                        )
                        .when_some(row.notice.recovery_label.clone(), |body, label| {
                            body.child(button(
                                format!("recover-error-{index}"),
                                label,
                                cx,
                                move |view, window, cx| {
                                    action(
                                        view,
                                        NoticeAction::Dispatch(Intent::RecoverError {
                                            id: recovered.clone(),
                                        }),
                                        window,
                                        cx,
                                    );
                                },
                            ))
                        })
                        .when_some(reconnect, |body, connecting| {
                            body.child(
                                button(
                                    format!("reconnect-error-{index}"),
                                    language.text("再接続"),
                                    cx,
                                    move |view, window, cx| {
                                        action(view, NoticeAction::Reconnect, window, cx)
                                    },
                                )
                                .disabled(connecting),
                            )
                        }),
                )
                .child(icon_button(
                    format!("copy-error-{}-{index}", scope.label()),
                    IconName::Copy,
                    language.text("エラーの詳細をコピー"),
                    cx,
                    move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(copied.clone()))
                    },
                ))
                .child(
                    icon_button(
                        format!("dismiss-error-{}-{index}", scope.label()),
                        IconName::Close,
                        language.text("エラーを閉じる"),
                        cx,
                        move |view, window, cx| {
                            let notice = if let Some(id) = local {
                                NoticeAction::DismissLocal(dismiss_target.clone(), id)
                            } else {
                                NoticeAction::Dispatch(Intent::DismissError {
                                    id: dismissed.clone(),
                                })
                            };
                            action(view, notice, window, cx);
                        },
                    )
                    .debug_selector({
                        let scope = scope.clone();
                        move || format!("dismiss-error-{}-{index}", scope.label())
                    }),
                )
        }))
}

fn local_error_row(
    language: agent_core::localization::UiLanguage,
    scope: &ErrorScope,
    notice: &LocalNotice,
) -> ErrorRow {
    let message = &notice.message;
    ErrorRow {
        notice: ErrorNotice {
            id: op::ErrorId::Application {
                message: message.to_string(),
            },
            context: language.text(scope.label()).into(),
            title: language.text("操作に失敗しました").into(),
            text: message.text(language),
            recovery_label: None,
        },
        local: Some(notice.id),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Arc, Desktop, DraftKey, ErrorScope, LocalNotice, Mode, NoticeLifetime, Runtime, appearance,
        platform,
    };
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
                sessions: Default::default(),
                connections: Arc::new(platform::Connections::default()),
                closing: tokio_util::task::TaskTracker::new(),
                logging_error: None,
            });
        });
        let first = DraftKey::from("first");
        let mut handle = None;
        let (_, window) = cx.add_window_view(|window, cx| {
            let desktop = Desktop::new(
                Mode::Fixture {
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
                view.raise_notice(ErrorScope::Settings, "repeated");
                let old_id = view.error_rows(&ErrorScope::Settings)[0].local.unwrap();
                view.raise_notice(ErrorScope::Settings, "repeated");
                let latest = view.error_rows(&ErrorScope::Settings)[0].local.unwrap();
                assert_ne!(old_id, latest);
                view.dismiss_notice(&ErrorScope::Settings, old_id);
                assert_eq!(
                    view.error_rows(&ErrorScope::Settings)[0].local,
                    Some(latest)
                );
                view.dismiss_notice(&ErrorScope::Projects, latest);
                assert!(!view.error_rows(&ErrorScope::Settings).is_empty());
                view.dismiss_notice(&ErrorScope::Settings, latest);
                assert!(view.error_rows(&ErrorScope::Settings).is_empty());
                let device_notice =
                    LocalNotice::new("language save failed", NoticeLifetime::Window);
                let device_id = device_notice.id;
                view.local_errors
                    .insert(ErrorScope::Settings, device_notice);
                view.raise_notice(ErrorScope::Projects, "pairing failed");
                view.connect(None, None);
                assert_eq!(
                    view.error_rows(&ErrorScope::Settings)[0].local,
                    Some(device_id)
                );
                assert!(view.error_rows(&ErrorScope::Projects).is_empty());
                view.dismiss_notice(&ErrorScope::Settings, device_id);
                let snapshot = Arc::make_mut(&mut view.snapshot);
                snapshot.connection.connected = true;
                Arc::make_mut(&mut Arc::make_mut(&mut snapshot.pane).navigation).draft_key =
                    first.clone();
                view.raise_notice(ErrorScope::Application, "draft persistence failed");
                view.raise_notice(
                    ErrorScope::Conversation {
                        draft_key: first.clone(),
                    },
                    "attachment failed",
                );
                view.project_fixture();
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
                    Arc::make_mut(
                        &mut Arc::make_mut(&mut Arc::make_mut(&mut view.snapshot).pane).navigation,
                    )
                    .draft_key = key.clone();
                    view.project_fixture();
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

    #[gpui::test]
    fn lost_connection_offers_reconnect_without_reporting_an_error(cx: &mut TestAppContext) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        cx.update(|cx| {
            appearance::init(cx);
            cx.set_global(Runtime {
                handle: runtime.handle().clone(),
                sessions: Default::default(),
                connections: Arc::new(platform::Connections::default()),
                closing: tokio_util::task::TaskTracker::new(),
                logging_error: None,
            });
        });
        let mut handle = None;
        let (_, window) = cx.add_window_view(|window, cx| {
            let desktop = Desktop::new(
                Mode::Fixture {
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
        window.simulate_resize(size(px(900.), px(700.)));
        let mut show = |connected: bool, connecting: bool, failure: Option<&str>| {
            window.update(|_, cx| {
                desktop.update(cx, |view, cx| {
                    let snapshot = Arc::make_mut(&mut view.snapshot);
                    snapshot.connection.connected = connected;
                    snapshot.connection.connection_failure = failure.map(Into::into);
                    view.connecting = connecting;
                    cx.notify();
                })
            });
            window.run_until_parked();
            (
                window.debug_bounds("host-disconnected").is_some(),
                window.debug_bounds("error-notice-アプリ全体-0").is_some(),
            )
        };
        assert_eq!(show(false, false, None), (true, false));
        assert_eq!(show(false, true, None), (false, false));
        assert_eq!(show(false, false, Some("Host unavailable")), (false, true));
        assert_eq!(show(true, false, None), (false, false));
    }
}
