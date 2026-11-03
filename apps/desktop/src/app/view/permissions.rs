use super::*;
use agent_core::presentation::permissions::PERMISSION_CHOICES;
use agent_protocol::{permissions::PermissionMode, session::ProviderKind};

impl Desktop {
    pub(super) fn permission_menu(
        label: &'static str,
        mode: Option<PermissionMode>,
        enabled: bool,
        render_owner: EntityId,
        cx: &Context<Self>,
    ) -> AnyElement {
        let language = crate::localization::language(cx);
        let entity = cx.entity().downgrade();
        let opening = entity.clone();
        popover::Popover::new("permission-controls")
            .bg(rgb(appearance::SURFACE))
            .rounded(px(16.))
            .border_color(rgb(appearance::BORDER))
            .anchor(Anchor::BottomLeft)
            .trigger(
                Button::new("composer-permissions")
                    .debug_selector(|| "composer-permissions".into())
                    .icon(Icon::default().path("icons/shield.svg"))
                    .label(agent_core::localization::localized_ui_text(
                        language,
                        label.to_string(),
                    ))
                    .accessibility_label(language.text("承認方法を変更"))
                    .ghost()
                    .h(px(40.))
                    .when(mode == Some(PermissionMode::FullAccess), |button| {
                        button.text_color(rgb(0xf58b42))
                    })
                    .disabled(!enabled),
            )
            .on_open_change(move |open, _, cx| {
                if *open {
                    let _ = opening.update(cx, |view, _| {
                        if let Some(provider) =
                            view.snapshot.permission_control(view.draft_key()).provider
                        {
                            view.dispatch(Intent::ReadPermissionSettings(
                                op::ReadPermissionSettings { provider },
                            ));
                        }
                    });
                }
            })
            .content(move |_, window, cx| {
                observe_popup_desktop(&entity, render_owner, window, cx);
                entity
                    .update(cx, |view, cx| view.permission_menu_content(cx))
                    .unwrap_or_else(|_| div().into_any_element())
            })
            .into_any_element()
    }

    fn permission_menu_content(&self, cx: &Context<Self>) -> AnyElement {
        let language = crate::localization::language(cx);
        let control = self.snapshot.permission_control(self.draft_key());
        let provider = control.provider;
        let mut body = v_flex().w(px(430.)).p_3().gap_1().child(
            div()
                .text_xs()
                .text_color(rgb(appearance::MUTED))
                .pb_2()
                .child(match provider {
                    Some(ProviderKind::Codex) => language.text("Codex の承認方法"),
                    Some(ProviderKind::Claude) => language.text("Claude の承認方法"),
                    Some(ProviderKind::DeepSeek) => language.text("DeepSeek Harness の承認方法"),
                    None => language.text("エージェント未選択"),
                }),
        );
        for (index, (mode, label, description)) in PERMISSION_CHOICES.into_iter().enumerate() {
            let version = control.version.map(str::to_owned);
            body = body.child(
                Button::new(format!("permission-choice-{index}"))
                    .debug_selector(move || format!("permission-choice-{index}"))
                    .accessibility_label(label)
                    .ghost()
                    .w_full()
                    .h_auto()
                    .py_2()
                    .icon(Icon::default().path("icons/shield.svg"))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .items_start()
                            .child(div().text_sm().child(
                                agent_core::localization::localized_ui_text(
                                    language,
                                    label.to_string(),
                                ),
                            ))
                            .child(
                                div()
                                    .text_xs()
                                    .whitespace_normal()
                                    .text_color(rgb(appearance::MUTED))
                                    .child(description),
                            ),
                    )
                    .when(control.mode == Some(mode), |button| {
                        button.child(Icon::new(IconName::Check).size(px(14.)))
                    })
                    .when(
                        mode == PermissionMode::FullAccess && control.mode == Some(mode),
                        |button| button.text_color(rgb(0xf58b42)),
                    )
                    .disabled(control.loading || version.is_none() || !self.snapshot.connected)
                    .on_click(cx.listener(move |view, _, _, _| {
                        if let (Some(provider), Some(version)) = (provider, &version) {
                            view.dispatch(Intent::UpdatePermissionSettings(
                                op::UpdatePermissionSettings {
                                    provider,
                                    mode,
                                    version: version.clone(),
                                },
                            ));
                        }
                    })),
            );
        }
        if control.loading {
            body = body.child(div().text_xs().child(language.text("読み込み中…")));
        }
        if let Some(error) = control.error {
            body = body
                .child(div().text_sm().whitespace_normal().child(error.to_owned()))
                .child(button(
                    "permissions-retry",
                    language.text("再読み込み"),
                    cx,
                    move |view, _, _| {
                        if let Some(provider) = provider {
                            view.dispatch(Intent::ReadPermissionSettings(
                                op::ReadPermissionSettings { provider },
                            ));
                        }
                    },
                ));
        }
        body.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{Desktop, Mode, RemoteHost};
    use gpui_kit as gpui;
    use gpui_kit::{
        AppContext, Context, Entity, IntoElement, Modifiers, Render, TestAppContext, Window,
    };
    use std::sync::Arc;

    struct ComposerView(Entity<Desktop>);
    impl Render for ComposerView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.0.update(cx, |view, _| view.chat())
        }
    }

    #[gpui::test]
    fn permissions_menu_is_right_of_attach_and_exposes_three_choices(cx: &mut TestAppContext) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        cx.update(|cx| {
            crate::appearance::init(cx);
            cx.set_global(crate::Runtime {
                handle: runtime.handle().clone(),
                connections: Arc::new(crate::platform::Connections::default()),
                closing: tokio_util::task::TaskTracker::new(),
                logging_error: None,
            });
        });
        let (_, window) = cx.add_window_view(|window, cx| {
            let desktop = cx.new(|cx| {
                Desktop::new(
                    Mode::SideChat {
                        remote: Some(RemoteHost {
                            id: "fixture".into(),
                            name: "fixture".into(),
                            ticket: "invalid-fixture-ticket".into(),
                        }),
                        cwd: "/fixture".into(),
                    },
                    window,
                    cx,
                )
            });
            desktop.update(cx, |view, _| {
                let snapshot = Arc::make_mut(&mut view.snapshot);
                snapshot.connected = true;
                Arc::make_mut(&mut snapshot.drafts).insert(
                    snapshot.navigation.draft_key.clone(),
                    Arc::new(agent_core::state::Draft {
                        model: Some(agent_protocol::models::ModelRef {
                            provider: agent_protocol::session::ProviderKind::Codex,
                            id: "model".into(),
                        }),
                        ..Default::default()
                    }),
                );
            });
            cx.observe(&desktop, |_, _, cx| cx.notify()).detach();
            ComposerView(desktop)
        });
        window.run_until_parked();
        let permission = window.debug_bounds("composer-permissions").unwrap();
        assert!(window.debug_bounds("attach").unwrap().right() <= permission.left());
        assert!(permission.right() <= window.debug_bounds("model-select").unwrap().left());
        window.simulate_click(permission.center(), Modifiers::default());
        window.run_until_parked();
        for selector in [
            "permission-choice-0",
            "permission-choice-1",
            "permission-choice-2",
        ] {
            assert!(window.debug_bounds(selector).is_some());
        }
    }
}
