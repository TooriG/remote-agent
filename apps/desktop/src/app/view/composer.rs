use super::*;
use gpui_kit::component::button::ButtonCustomVariant;

impl Desktop {
    pub(in crate::app) fn sync_composer(&self, cx: &mut App) {
        let model = self.composer_model(cx);
        self.composer_view
            .read(cx)
            .content
            .clone()
            .update(cx, |view, cx| {
                view.set_model(model, cx);
            });
    }

    fn composer_context(model: &ComposerModel, cx: &Context<Self>) -> AnyElement {
        let mut row = h_flex()
            .w_full()
            .min_w_0()
            .gap_2()
            .px_2()
            .child(Self::composer_folder(model, cx));
        if let Some(hosts) = &model.hosts {
            row = row.child(Hosts::menu(
                hosts,
                "composer-host",
                model.remote.as_ref().map(|remote| remote.id.as_str()),
                model
                    .remote
                    .as_ref()
                    .map_or("Local", |remote| remote.name.as_str()),
                model.busy || model.phase.is_some(),
                cx,
            ));
        } else {
            let name = model
                .remote
                .as_ref()
                .map_or("Local", |remote| remote.name.as_str())
                .to_owned();
            row = row.child(
                h_flex()
                    .id("composer-host")
                    .min_w_0()
                    .max_w(px(200.))
                    .h_8()
                    .px_2()
                    .gap_2()
                    .tooltip({
                        let name = name.clone();
                        move |window, cx| {
                            tooltip::Tooltip::new(format!("実行先: {name}")).build(window, cx)
                        }
                    })
                    .child(Icon::default().path("icons/monitor.svg").size_4())
                    .child(div().min_w_0().text_ellipsis().child(name)),
            );
        }
        if let Some(review) = model
            .review
            .as_ref()
            .filter(|review| !review.branch.is_empty())
        {
            let entity = cx.entity().downgrade();
            row = row.child(
                Button::new("composer-branch")
                    .debug_selector(|| "composer-branch".into())
                    .label(review.branch.clone())
                    .icon(Icon::default().path("icons/branch.svg"))
                    .accessibility_label(format!("現在のブランチ: {}", review.branch))
                    .tooltip(format!("現在のブランチ: {}", review.branch))
                    .dropdown_caret(true)
                    .h_8()
                    .min_w_0()
                    .max_w(px(260.))
                    .flex_shrink_1()
                    .ghost()
                    .disabled(!model.connected || model.phase.is_some())
                    .dropdown_menu(move |menu, _, _| {
                        let refresh = entity.clone();
                        let changes = entity.clone();
                        menu.item(PopupMenuItem::new("変更を表示").on_click(move |_, _, cx| {
                            let _ = changes.update(cx, |s, cx| {
                                s.open_review(None, cx);
                                cx.notify();
                            });
                        }))
                        .item(PopupMenuItem::new("更新").on_click(move |_, _, cx| {
                            let _ = refresh.update(cx, |s, _| s.refresh_review());
                        }))
                    }),
            );
        }
        row.into_any_element()
    }

    fn composer_folder(model: &ComposerModel, cx: &Context<Self>) -> AnyElement {
        let selected_directory = model.selected_directory.clone();
        let entity = cx.entity().downgrade();
        Button::new("composer-folder")
            .debug_selector(|| "composer-folder".into())
            .label(if selected_directory.is_empty() {
                "チャット".into()
            } else {
                file_name(&selected_directory)
            })
            .accessibility_label(if selected_directory.is_empty() {
                "フォルダ: チャット".into()
            } else {
                format!("フォルダ: {}", selected_directory)
            })
            .icon(IconName::Folder)
            .tooltip(if selected_directory.is_empty() {
                "チャット".into()
            } else {
                selected_directory.clone()
            })
            .h_8()
            .min_w_0()
            .max_w(px(260.))
            .flex_shrink_1()
            .ghost()
            .disabled(model.busy || model.phase.is_some())
            .dropdown_menu(move |mut menu, _, cx| {
                let Some(owner) = entity.upgrade() else {
                    return menu;
                };
                let state = owner.read(cx);
                let selected_directory = state.snapshot.selected_directory();
                let list = state.snapshot.thread_list();
                let unassigned = entity.clone();
                menu = menu.item(
                    PopupMenuItem::new("チャット")
                        .checked(selected_directory.is_empty())
                        .on_click(move |_, window, cx| {
                            let _ = unassigned.update(cx, |s, cx| {
                                s.new_chat(String::new(), window, cx);
                                cx.notify();
                            });
                        }),
                );
                for project in list
                    .as_ref()
                    .map(|page| page.projects.as_slice())
                    .unwrap_or_default()
                {
                    for root in &project.roots {
                        let path = root.path.clone();
                        let label = if project.roots.len() == 1 {
                            project.name.clone()
                        } else {
                            path.clone()
                        };
                        let target = entity.clone();
                        menu = menu.item(
                            PopupMenuItem::new(label)
                                .checked(path == selected_directory)
                                .on_click(move |_, window, cx| {
                                    let _ = target.update(cx, |s, cx| {
                                        s.new_chat(path.clone(), window, cx);
                                        cx.notify();
                                    });
                                }),
                        );
                    }
                }
                if list.as_ref().is_some_and(|page| page.has_more_projects) {
                    let target = entity.clone();
                    menu = menu.item(PopupMenuItem::new("さらにプロジェクトを読み込む").on_click(
                        move |_, _, cx| {
                            let _ = target.update(cx, |s, cx| {
                                s.dispatch(Intent::ExpandProjects);
                                cx.notify();
                            });
                        },
                    ));
                }
                if state.remote.is_none() {
                    let target = entity.clone();
                    menu = menu.item(PopupMenuItem::new("別のフォルダを選択…").on_click(
                        move |_, _, cx| {
                            let _ = target.update(cx, |s, _| s.pick_folder());
                        },
                    ));
                }
                menu
            })
            .into_any_element()
    }
    pub(super) fn chat(&self) -> AnyElement {
        v_flex()
            .debug_selector(|| "conversation-chat".into())
            .flex_1()
            .min_w_0()
            .h_full()
            .child(
                self.conversation
                    .clone()
                    .cached(StyleRefinement::default().flex_1().min_h_0().w_full()),
            )
            .child(self.composer_view.clone())
            .into_any_element()
    }

    fn render_composer(
        &mut self,
        model: &ComposerModel,
        input: &Entity<TextareaState>,
        render_owner: EntityId,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = model.key.clone();
        let attachments = &model.attachments;
        let mut files = h_flex().gap_3().p_2().flex_wrap();
        for (i, file) in attachments.iter().enumerate() {
            let key = key.clone();
            let remove = icon_button(
                format!("attachment-{i}"),
                IconName::Close,
                format!("{}を外す", file.name),
                cx,
                move |s, _, cx| {
                    cx.stop_propagation();
                    s.dispatch(Intent::RemoveAttachment {
                        draft_key: key.clone(),
                        index: i as u32,
                    });
                },
            )
            .debug_selector(move || format!("attachment-remove-{i}"))
            .xsmall()
            .rounded_full();
            files = files.child(if file.is_image {
                div()
                    .id(format!("draft-attachment-{i}"))
                    .relative()
                    .size(px(120.))
                    .flex_shrink_0()
                    .child(
                        div()
                            .size_full()
                            .rounded_lg()
                            .overflow_hidden()
                            .child(self.image(&file.path, false, 120., true, cx)),
                    )
                    .child(deferred(
                        div()
                            .id(format!("attachment-remove-hitbox-{i}"))
                            .occlude()
                            .absolute()
                            .top(px(-4.))
                            .right(px(-4.))
                            .size(px(18.))
                            .child(
                                remove
                                    .size(px(18.))
                                    .custom(
                                        ButtonCustomVariant::new(cx)
                                            .foreground(rgb(0x222222).into())
                                            .hover(rgb(0xe6e6e6).into())
                                            .active(rgb(0xd4d4d4).into()),
                                    )
                                    .bg(rgb(0xffffff)),
                            ),
                    ))
                    .into_any_element()
            } else {
                h_flex()
                    .gap_2()
                    .child(file.name.clone())
                    .child(remove.size(px(28.)).bg(rgb(appearance::RAISED)))
                    .into_any_element()
            });
        }
        let running = model.running.clone();
        let empty = model.empty;
        let inactive = empty && running.is_none();
        let phase = model.phase;
        let send = if let Some(id) = running.filter(|_| empty) {
            icon_button(
                "stop",
                Icon::default().path("icons/stop.svg"),
                "停止",
                cx,
                move |s, _, _| {
                    s.dispatch(Intent::Interrupt(op::Interrupt {
                        thread_id: s.selected().expect("selected conversation").clone(),
                        turn_id: id.clone(),
                    }));
                },
            )
            .disabled(!model.connected || model.busy)
        } else {
            icon_button("send", IconName::ArrowUp, "送信", cx, |s, _, cx| {
                s.send(cx)
            })
            .disabled(
                !model.connected
                    || !model.provider_available
                    || model.busy
                    || empty
                    || model.input_unavailable,
            )
        };
        let microphone = Button::new("dictation-toggle")
            .icon(Icon::default().path("icons/microphone.svg").size(px(20.)))
            .ghost()
            .w(px(40.))
            .h(px(40.))
            .large()
            .tooltip("音声をCodexで文字起こし")
            .accessibility_label("音声をCodexで文字起こし")
            .disabled(!model.connected || model.busy)
            .on_click(cx.listener(|s, _, window, cx| {
                s.composer.input.read(cx).focus_handle(cx).focus(window, cx);
                s.start_dictation();
                cx.notify();
            }));
        let composer = v_flex()
            .key_context("ChatComposer")
            .track_focus(&input.read(cx).focus_handle(cx))
            .capture_key_down(cx.listener(|s, event: &KeyDownEvent, window, cx| {
                let modifiers = event.keystroke.modifiers;
                if !(modifiers.shift || modifiers.control || modifiers.alt || modifiers.platform)
                    && s.completion_key(&event.keystroke.key, window, cx)
                {
                    return;
                }
                if event.keystroke.key == "escape"
                    && s.dictation
                        .as_ref()
                        .is_some_and(|state| state.phase != Phase::Transcribing)
                {
                    s.cancel_recording();
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .capture_action(cx.listener(Self::paste_image))
            .capture_action(cx.listener(Self::composer_enter))
            .capture_action(
                cx.listener(|s, _: &gpui_kit::component::input::MoveUp, window, cx| {
                    s.composer_arrow("up", window, cx);
                }),
            )
            .capture_action(cx.listener(
                |s, _: &gpui_kit::component::input::MoveDown, window, cx| {
                    s.composer_arrow("down", window, cx);
                },
            ))
            .w_full()
            .max_w(px(CHAT_WIDTH))
            .p(px(7.))
            .gap_2()
            .rounded(px(22.))
            .child(Self::completion_menu(
                model.suggestions.as_ref(),
                model.completion_index,
                cx,
            ))
            .when(!attachments.is_empty(), |composer| composer.child(files))
            .bg(rgb(appearance::SURFACE))
            .border_1()
            .border_color(rgb(appearance::BORDER))
            .when(phase.is_none(), |composer| {
                composer.child(
                    div().px_2().pt(px(10.)).pb_1().child(
                        Textarea::new(input)
                            .appearance(false)
                            .bordered(false)
                            .text_size(px(14.))
                            .aria_label("AI に依頼する")
                            .readonly(!model.connected),
                    ),
                )
            })
            .when(phase.is_none(), |composer| {
                composer.child(
                    h_flex()
                        .child(
                            icon_button(
                                "attach",
                                IconName::Plus,
                                "ファイルを添付",
                                cx,
                                |s, _, _| s.attach(),
                            )
                            .debug_selector(|| "attach".into())
                            .w(px(40.))
                            .h(px(40.))
                            .large()
                            .disabled(
                                !model.connected
                                    || (!model.remote.is_none() && model.cwd.is_empty())
                                    || model.busy
                                    || phase.is_some(),
                            ),
                        )
                        .child(Self::permission_menu(
                            model.permission_label,
                            model.permission_mode,
                            model.connected && model.provider_available,
                            render_owner,
                            cx,
                        ))
                        .child(div().flex_1())
                        .child(Self::fast_control(
                            model.quick_controls.clone(),
                            model.controls_enabled,
                            "model-fast",
                            false,
                            cx,
                        ))
                        .child(Self::model_menu(
                            model.model_label.clone(),
                            render_owner,
                            cx,
                        ))
                        .child(Self::effort_control(
                            model.quick_controls.clone(),
                            model.controls_enabled,
                            "model-effort",
                            false,
                            cx,
                        ))
                        .child(microphone)
                        .child(
                            send.large().rounded_full().w(px(40.)).h(px(40.)).custom(
                                ButtonCustomVariant::new(cx)
                                    .color(
                                        rgb(if inactive {
                                            appearance::BORDER
                                        } else {
                                            0xffffff
                                        })
                                        .into(),
                                    )
                                    .foreground(
                                        rgb(if inactive {
                                            appearance::MUTED
                                        } else {
                                            appearance::SIDEBAR
                                        })
                                        .into(),
                                    )
                                    .hover(rgb(0xd4d4d4).into())
                                    .active(rgb(appearance::MUTED).into()),
                            ),
                        ),
                )
            })
            .when(phase.is_some(), |composer| {
                composer.child(Self::dictation_bar(
                    model.phase,
                    &model.levels,
                    model.connected,
                    cx,
                ))
            });
        let controls = v_flex()
            .w_full()
            .max_w(px(CHAT_WIDTH))
            .gap_1()
            .when_some(
                model
                    .review
                    .as_deref()
                    .filter(|review| model.has_session && !review.files.is_empty()),
                |column, review| {
                    column.child(Self::review_card(
                        review,
                        model.review_expanded,
                        model.connected,
                        cx,
                    ))
                },
            )
            .child(Self::composer_context(model, cx))
            .child(composer);
        h_flex()
            .justify_center()
            .px_6()
            .pt_2()
            .pb_4()
            .child(controls)
            .into_any_element()
    }
}

#[derive(PartialEq)]
struct ComposerModel {
    key: DraftKey,
    attachments: Vec<Attachment>,
    running: Option<TurnId>,
    empty: bool,
    connected: bool,
    busy: bool,
    provider_available: bool,
    input_unavailable: bool,
    phase: Option<Phase>,
    levels: std::collections::VecDeque<f32>,
    remote: Option<RemoteHost>,
    hosts: Option<Entity<Hosts>>,
    cwd: String,
    selected_directory: String,
    has_session: bool,
    review: Option<Arc<agent_protocol::models::WorkspaceReview>>,
    review_expanded: bool,
    model_label: String,
    permission_label: &'static str,
    permission_mode: Option<agent_protocol::permissions::PermissionMode>,
    quick_controls: agent_core::presentation::model_settings::ModelQuickControls,
    controls_enabled: bool,
    suggestions: Option<agent_core::composer::ComposerSuggestions>,
    completion_index: usize,
}
impl Desktop {
    pub(super) fn model_controls_enabled(&self) -> bool {
        self.snapshot.connected
            && !self.account.busy
            && self.busy == 0
            && self.snapshot.account.login.is_none()
    }
    fn composer_model(&self, cx: &App) -> ComposerModel {
        let permission = self.snapshot.permission_control(self.draft_key());
        ComposerModel {
            key: self.draft_key().clone(),
            attachments: self.draft().attachments.clone(),
            running: self.thread().and_then(|thread| thread.active_turn_id()),
            empty: self.composer.input.read(cx).value().trim().is_empty()
                && self.draft().attachments.is_empty(),
            connected: self.snapshot.connected,
            busy: self.busy > 0,
            provider_available: permission.provider.is_some(),
            input_unavailable: self
                .thread()
                .and_then(|thread| agent_protocol::session::input_unavailable_reason(thread))
                .is_some(),
            phase: self.dictation.as_ref().map(|state| state.phase),
            levels: self
                .dictation
                .as_ref()
                .map(|state| state.levels.clone())
                .unwrap_or_default(),
            remote: self.remote.clone(),
            hosts: self.hosts.clone(),
            cwd: self.snapshot.navigation.cwd.clone(),
            selected_directory: self.snapshot.selected_directory(),
            has_session: self.selected().is_some(),
            review: self.snapshot.workspace.review.clone(),
            review_expanded: self.review_expanded,
            model_label: self
                .selected_model()
                .map_or("モデルを選択".to_owned(), |model| {
                    model.display_name.clone()
                }),
            permission_label: permission.label,
            permission_mode: permission.mode,
            quick_controls: self.snapshot.model_quick_controls(self.draft_key().clone()),
            controls_enabled: self.model_controls_enabled(),
            suggestions: self.completion_suggestions(cx),
            completion_index: self.composer.completion_index,
        }
    }
}

/// The outer view measures natural height; only the fixed-size content is cached.
/// GPUI's cached views cannot measure their contents to determine their own size.
pub(in crate::app) struct ComposerView {
    pub(in crate::app) content: Entity<ComposerContent>,
    pub(super) height: Option<Pixels>,
    _measurement: Subscription,
}
pub(in crate::app) struct ComposerContent {
    desktop: WeakEntity<Desktop>,
    input: Entity<TextareaState>,
    model: Option<ComposerModel>,
    _observations: Vec<Subscription>,
    #[cfg(test)]
    pub(in crate::app) renders: usize,
}
struct ComposerHeight(Pixels);
impl EventEmitter<ComposerHeight> for ComposerContent {}
impl ComposerView {
    pub(in crate::app) fn new(
        desktop: &Entity<Desktop>,
        input: Entity<TextareaState>,
        cx: &mut Context<Self>,
    ) -> Self {
        let content = cx.new(|cx| {
            let owner = desktop.downgrade();
            ComposerContent {
                desktop: owner.clone(),
                input: input.clone(),
                model: None,
                _observations: vec![
                    cx.observe(desktop, |view: &mut ComposerContent, desktop, cx| {
                        let model = desktop.read(cx).composer_model(cx);
                        view.set_model(model, cx);
                    }),
                    cx.observe(&input, move |view: &mut ComposerContent, _, cx| {
                        if let Some(desktop) = owner.upgrade() {
                            let model = desktop.read(cx).composer_model(cx);
                            view.set_model(model, cx);
                        }
                    }),
                ],
                #[cfg(test)]
                renders: 0,
            }
        });
        Self {
            _measurement: cx.subscribe(&content, |view, _, height: &ComposerHeight, cx| {
                if view.height != Some(height.0) {
                    view.height = Some(height.0);
                    cx.notify();
                }
            }),
            content,
            height: None,
        }
    }
}
impl ComposerContent {
    fn set_model(&mut self, model: ComposerModel, cx: &mut Context<Self>) {
        if self.model.as_ref() != Some(&model) {
            self.model = Some(model);
            cx.notify();
        }
    }
}
impl Render for ComposerView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        match self.height {
            Some(height) => self
                .content
                .clone()
                .cached(
                    StyleRefinement::default()
                        .w_full()
                        .h(height)
                        .flex_shrink_0(),
                )
                .into_any_element(),
            None => self.content.clone().into_any_element(),
        }
    }
}
impl Render for ComposerContent {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        {
            self.renders += 1;
        }
        let render_owner = cx.entity().entity_id();
        let owner = cx.entity().downgrade();
        let content = self
            .desktop
            .update(cx, |desktop, cx| {
                // Initialize before the first layout so the conversation never
                // sees an empty composer and incorrectly resumes tail-following.
                let model = self.model.get_or_insert_with(|| desktop.composer_model(cx));
                desktop.render_composer(model, &self.input, render_owner, cx)
            })
            .unwrap_or_else(|_| div().into_any_element());
        div()
            .relative()
            .w_full()
            .flex_shrink_0()
            .child(content)
            .child(
                canvas(
                    move |bounds, _, cx| {
                        let _ =
                            owner.update(cx, |_, cx| cx.emit(ComposerHeight(bounds.size.height)));
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod composer_tests {
    use super::*;
    use agent_protocol::{execution::TurnStatus, items::ItemBody};
    use core::prelude::v1::test;
    use gpui_kit as gpui;

    struct ComposerView(Entity<Desktop>);
    impl Render for ComposerView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.0.update(cx, |view, _| view.chat())
        }
    }

    #[gpui::test]
    fn history_control_preserves_visible_content_and_offers_retry_after_a_failed_page(
        cx: &mut TestAppContext,
    ) {
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
        let session = SessionRef {
            provider: agent_protocol::session::ProviderKind::Codex,
            id: "history".into(),
        };
        let turns = |range: std::ops::Range<usize>| {
            range
                .map(|index| {
                    Arc::new(Turn {
                        id: format!("turn-{index}").into(),
                        status: TurnStatus::Completed,
                        items: Some(vec![Arc::new(Item::new(
                            format!("answer-{index}").into(),
                            Default::default(),
                            ItemBody::AssistantText {
                                text: format!(
                                    "Answer {index}\n{}",
                                    "Earlier conversation text.\n".repeat(8)
                                ),
                                phase: agent_protocol::items::AssistantPhase::Final,
                            },
                        ))]),
                        ..Default::default()
                    })
                })
                .collect::<Vec<_>>()
        };
        let mut handle = None;
        let (_, window) = cx.add_window_view(|window, cx| {
            let desktop = cx.new(|cx| {
                Desktop::new(
                    Mode::SideChat {
                        remote: Some(RemoteHost {
                            id: "fixture".into(),
                            name: "fixture".into(),
                            ticket: "invalid".into(),
                        }),
                        cwd: "/fixture".into(),
                    },
                    window,
                    cx,
                )
            });
            desktop.update(cx, |view, cx| {
                let snapshot = Arc::make_mut(&mut view.snapshot);
                let navigation = Arc::make_mut(&mut snapshot.navigation);
                navigation.thread_id = Some(session.clone());
                navigation.draft_key = session.clone().into();
                Arc::make_mut(&mut snapshot.conversations).insert(
                    session.clone(),
                    Arc::new(Thread {
                        id: Some(session.clone()),
                        turns: Some(turns(2..8)),
                        history_has_more: Some(true),
                        history_cursor: Some("older-page".into()),
                        history_read_state: Some(agent_protocol::session::HistoryReadState::new(
                            agent_protocol::session::HistoryReadKind::Partial,
                            Vec::new(),
                        )),
                        ..Default::default()
                    }),
                );
                view.sync_conversation(window, cx);
                view.conversation.update(cx, |view, cx| {
                    view.timeline.list.scroll_to(ListOffset::default());
                    cx.notify();
                });
            });
            handle = Some(desktop.clone());
            ComposerView(desktop)
        });
        let desktop = handle.unwrap();
        window.simulate_resize(size(px(800.), px(700.)));
        window.run_until_parked();
        let button = window
            .debug_bounds("conversation-history-load")
            .expect("older-history control at the top");
        window.simulate_click(button.center(), Modifiers::default());
        let top = window.update(|_, cx| {
            desktop.update(cx, |view, cx| {
                assert!(!view.conversation.read(cx).timeline.list.is_following_tail());
                view.conversation
                    .read(cx)
                    .timeline
                    .list
                    .bounds_for_item(1)
                    .unwrap()
                    .top()
            })
        });
        window.update(|_, cx| {
            desktop.update(cx, |view, cx| {
                let thread = Arc::make_mut(
                    Arc::make_mut(&mut Arc::make_mut(&mut view.snapshot).conversations)
                        .get_mut(&session)
                        .unwrap(),
                );
                thread.turns = Some(turns(0..8));

                cx.notify();
            })
        });
        window.run_until_parked();
        window.update(|_, cx| {
            desktop.update(cx, |view, cx| {
                assert!(
                    (view
                        .conversation
                        .read(cx)
                        .timeline
                        .list
                        .bounds_for_item(3)
                        .unwrap()
                        .top()
                        - top)
                        .abs()
                        < px(1.),
                    "prepending history must keep the old first answer at the same screen position"
                );
                view.conversation.update(cx, |view, cx| {
                    view.timeline.list.scroll_to(ListOffset::default());
                    cx.notify();
                });
            })
        });
        let key = op::OperationKey::History {
            session: session.clone(),
        };
        let mut store_handle = None;
        window.update(|_, cx| {
            desktop.update(cx, |view, cx| {
                let store = runtime.block_on(async {
                    Arc::new(agent_core::store::Store::offline((*view.snapshot).clone()))
                });
                view.snapshot = store.snapshot();
                let app_runtime = view.runtime.clone();
                view.session = Some(runtime.block_on(async {
                    let (updates, receive) = async_channel::unbounded();
                    tokio::spawn(StoreSession::publish(
                        Ok(store.clone()),
                        app_runtime,
                        updates,
                        Some,
                        |_| None,
                    ));
                    receive.recv().await.unwrap().unwrap().unwrap()
                }));
                store_handle = Some(store);

                cx.notify();
            })
        });
        window.run_until_parked();
        let store = store_handle.unwrap();
        for attempt in 0..2 {
            let previous_generation = store
                .snapshot()
                .operations
                .get(&key)
                .map_or(0, |state| state.generation);
            let button = window.debug_bounds("conversation-history-load").unwrap();
            window.simulate_click(button.center(), Modifiers::default());
            runtime.block_on(async {
                let mut updates = store.subscribe();
                tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    loop {
                        let failed = updates
                            .borrow_and_update()
                            .operations
                            .get(&key)
                            .is_some_and(|state| {
                                state.generation > previous_generation
                                    && matches!(state.phase, op::OperationPhase::Failed { .. })
                            });
                        if failed {
                            break;
                        }
                        updates.changed().await.unwrap();
                    }
                })
                .await
                .expect("each button press must execute a history request, including retry");
            });
            window.update(|window, cx| {
                desktop.update(cx, |view, cx| {
                    view.accept_snapshot(window, cx);
                    cx.notify();
                })
            });
            window.run_until_parked();
            assert!(
                window.debug_bounds("conversation-history-error").is_some(),
                "request failure {attempt} must appear inline"
            );
        }
        assert!(
            window.debug_bounds("conversation-history-load").is_some(),
            "the same control remains available to retry"
        );
        window.update(|_, cx| {
            desktop.update(cx, |view, cx| {
                view.session.take();
                let snapshot = Arc::make_mut(&mut view.snapshot);
                Arc::make_mut(&mut snapshot.operations).remove(&key);
                let thread = Arc::make_mut(
                    Arc::make_mut(&mut snapshot.conversations)
                        .get_mut(&session)
                        .unwrap(),
                );
                thread.history_has_more = Some(false);
                thread.history_cursor = None;

                cx.notify();
            })
        });
        window.run_until_parked();
        assert!(window.debug_bounds("conversation-history-load").is_none());
        assert!(window.debug_bounds("conversation-history-error").is_none());
    }

    #[gpui::test]
    fn removing_an_image_does_not_open_its_gallery(cx: &mut TestAppContext) {
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
        let image_directory = tempfile::tempdir().unwrap();
        let image_path = image_directory.path().join("attachment.png");
        image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]))
            .save(&image_path)
            .unwrap();
        let source = image_path.to_string_lossy().into_owned();
        let mut desktop_handle = None;
        let mut store_handle = None;
        let mut draft_key = None;
        let (_, window) = cx.add_window_view(|window, cx| {
            let desktop = cx.new(|cx| {
                Desktop::new(
                    Mode::SideChat {
                        remote: Some(RemoteHost {
                            id: "fixture".into(),
                            name: "fixture".into(),
                            ticket: "invalid".into(),
                        }),
                        cwd: image_directory.path().to_string_lossy().into_owned(),
                    },
                    window,
                    cx,
                )
            });
            desktop.update(cx, |view, _| {
                let key = view.draft_key().clone();
                draft_key = Some(key.clone());
                Arc::make_mut(&mut Arc::make_mut(&mut view.snapshot).drafts).insert(
                    key,
                    Arc::new(Draft {
                        text: "Keep this draft".into(),
                        attachments: vec![agent_core::state::Attachment {
                            path: source.clone(),
                            name: "attachment.png".into(),
                            is_image: true,
                        }],
                        ..Default::default()
                    }),
                );
                view.media.images.insert(
                    view.image_key(&source, false),
                    ImageState {
                        source: Arc::new(source.clone()),
                        path: Some(source.clone().into()),
                        error: None,
                    },
                );
                let store = runtime.block_on(async {
                    Arc::new(agent_core::store::Store::offline((*view.snapshot).clone()))
                });
                let app_runtime = view.runtime.clone();
                view.session = Some(runtime.block_on(async {
                    let (updates, receive) = async_channel::unbounded();
                    tokio::spawn(StoreSession::publish(
                        Ok(store.clone()),
                        app_runtime,
                        updates,
                        Some,
                        |_| None,
                    ));
                    receive.recv().await.unwrap().unwrap().unwrap()
                }));
                store_handle = Some(store);
            });
            desktop_handle = Some(desktop.clone());
            let content = cx.new(|_| ComposerView(desktop));
            crate::activity::ActivityWindow::new(content, window, cx)
        });
        window.simulate_resize(size(px(700.), px(700.)));
        window.run_until_parked();
        let bounds = window.debug_bounds("chat-image").unwrap();
        window.simulate_click(bounds.center(), Modifiers::default());
        let desktop = desktop_handle.unwrap();
        window.update(|_, cx| {
            desktop.update(cx, |view, _| {
                assert!(
                    view.media.gallery.is_some(),
                    "the thumbnail still opens the gallery"
                );
                view.media.gallery = None;
            })
        });
        window.update(|_, cx| {
            desktop.update(cx, |view, cx| {
                let key = view.image_key(&source, false);
                view.media.images.get_mut(&key).unwrap().path = None;
                view.composer_view
                    .read(cx)
                    .content
                    .clone()
                    .update(cx, |_, cx| cx.notify());
            })
        });
        window.simulate_mouse_move(bounds.center(), None, Modifiers::default());
        window.run_until_parked();
        let remove = window.debug_bounds("attachment-remove-0").unwrap().center();
        window.update(|window, _| {
            let scale = window.scale_factor();
            let point = remove.scale(scale);
            let quads = window.painted_quads();
            let pulse = quads
                .iter()
                .filter(|quad| {
                    quad.bounds.size == size(px(120.), px(120.)).scale(scale)
                        && quad.bounds.contains(&point)
                })
                .max_by_key(|quad| quad.order)
                .expect("loading image pulse must be painted");
            let button = quads
                .iter()
                .find(|quad| {
                    quad.bounds.size == size(px(18.), px(18.)).scale(scale)
                        && quad.bounds.contains(&point)
                        && quad.background == rgb(0xffffff).into()
                })
                .expect("remove button must be painted");
            assert!(
                button.order > pulse.order,
                "the remove button must paint above the image loading pulse"
            );
        });
        window.simulate_click(remove, Modifiers::default());
        window.update(|_, cx| desktop.update(cx, |view, _| assert!(view.media.gallery.is_none())));
        let snapshot = store_handle.unwrap().snapshot();
        let draft = &snapshot.drafts[&draft_key.unwrap()];
        assert!(draft.attachments.is_empty());
        assert_eq!(draft.text, "Keep this draft");
    }
}
