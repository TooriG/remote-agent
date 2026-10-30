use super::*;
use agent_core::presentation::conversation::{
    ActivityExpansion, ConversationRowContent, WorkProgress, activity_is_expanded,
};

struct WorkProgressView {
    progress: WorkProgress,
    _clock: Task<()>,
}

impl WorkProgressView {
    fn view(id: &str, progress: WorkProgress, window: &mut Window, cx: &mut App) -> Entity<Self> {
        // use_keyed_state observes its entity and notifies the parent view.
        // Retain the clock without forwarding each tick to the conversation.
        let view = window.with_global_id(
            SharedString::from(format!("work-progress-{id}")).into(),
            |id, window| {
                window.with_element_state::<Entity<Self>, _>(id, |retained, _| {
                    let view = retained.unwrap_or_else(|| {
                        cx.new(|cx| Self {
                            progress: progress.clone(),
                            _clock: cx.spawn(async move |view, cx| {
                                loop {
                                    cx.background_executor()
                                        .timer(std::time::Duration::from_secs(1))
                                        .await;
                                    if view.update(cx, |_, cx| cx.notify()).is_err() {
                                        break;
                                    }
                                }
                            }),
                        })
                    });
                    (view.clone(), view)
                })
            },
        );
        view.update(cx, |view, cx| {
            if view.progress != progress {
                view.progress = progress;
                cx.notify();
            }
        });
        view
    }
    fn label(&self) -> String {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0., |duration| duration.as_secs_f64());
        self.progress.label(now)
    }
}

impl Render for WorkProgressView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().child(self.label())
    }
}

impl Desktop {
    pub(super) fn conversation_navigation(&self, cx: &Context<Self>) -> AnyElement {
        let entries: Vec<_> = self
            .timeline
            .rows
            .iter()
            .enumerate()
            .filter_map(|(index, row)| {
                let ConversationRow::Turn {
                    turn,
                    index: row_index,
                    ..
                } = row
                else {
                    return None;
                };
                match &turn.rows[*row_index].content {
                    ConversationRowContent::User { item } => {
                        item.data.body.as_ref().map(|text| (index, text, turn))
                    }
                    _ => None,
                }
            })
            .collect();
        let count = entries.len().min(8);
        let mut markers = v_flex()
            .absolute()
            .left_0()
            .top_0()
            .h_full()
            .w(px(56.))
            .justify_center();
        let current = self.timeline.list.logical_scroll_top().item_ix;
        for position in 0..count {
            let sample = position * (entries.len() - 1) / count.saturating_sub(1).max(1);
            let (index, user, turn) = entries[sample];
            let active = current >= index
                && (position + 1 == count
                    || current < entries[(position + 1) * (entries.len() - 1) / (count - 1)].0);
            let user: String = user.chars().take(160).collect();
            let distance = self
                .timeline
                .hovered_marker
                .map(|hovered| position.abs_diff(hovered));
            let width = match distance {
                Some(0) => 28.,
                Some(1) => 20.,
                Some(2) => 14.,
                Some(3) => 10.,
                _ => 7.,
            };
            let marker_id = SharedString::from(format!("conversation-marker-{index}"));
            let preview = (distance == Some(0)).then(|| {
                let answer = turn.rows.iter().find_map(|row| match &row.content {
                    ConversationRowContent::Response { item, .. } => item
                        .data
                        .body
                        .as_ref()
                        .map(|text| text.chars().take(240).collect::<String>()),
                    _ => None,
                });
                let preview_width = (self.timeline.list.viewport_bounds().size.width - px(48.))
                    .clamp(px(100.), px(300.));
                v_flex()
                    .absolute()
                    .left(px(56.))
                    .top(-px(24.))
                    .w(preview_width)
                    .p_3()
                    .gap_2()
                    .rounded_lg()
                    .bg(rgb(appearance::RAISED))
                    .border_1()
                    .border_color(rgb(appearance::BORDER))
                    .shadow_md()
                    .text_sm()
                    .child(div().max_h(px(44.)).overflow_hidden().child(user.clone()))
                    .children(answer.map(|answer| {
                        div()
                            .max_h(px(66.))
                            .overflow_hidden()
                            .text_color(rgb(appearance::MUTED))
                            .child(answer)
                    }))
            });
            markers = markers.child(
                div()
                    .id(marker_id.clone())
                    .on_hover(cx.listener(move |view, hovered, _, cx| {
                        if *hovered {
                            view.timeline.hovered_marker = Some(position);
                        } else if view.timeline.hovered_marker == Some(position) {
                            view.timeline.hovered_marker = None;
                        }
                        cx.notify();
                    }))
                    .relative()
                    .child(
                        Button::new(marker_id)
                            .text()
                            .small()
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.timeline.list.scroll_to(ListOffset {
                                    item_ix: index,
                                    offset_in_item: px(0.),
                                });
                                cx.notify();
                            }))
                            .debug_selector(move || format!("conversation-marker-{index}"))
                            .accessibility_label(format!("この発言へ移動: {user}"))
                            .w(px(56.))
                            .h(px(10.))
                            .pl(px(16.5))
                            .pr(px(11.5))
                            .child(
                                h_flex().w_full().child(
                                    div()
                                        .debug_selector(move || {
                                            format!("conversation-marker-line-{index}")
                                        })
                                        .h(px(if distance == Some(0) { 2. } else { 1. }))
                                        .w(px(width))
                                        .bg(rgb(if active || distance == Some(0) {
                                            appearance::FOREGROUND
                                        } else {
                                            appearance::MUTED
                                        })),
                                ),
                            ),
                    )
                    .children(preview),
            );
        }
        markers.into_any_element()
    }

    pub(super) fn diff(
        diffs: &mut HashMap<String, Entity<DiffView>>,
        id: String,
        patch: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let view = diffs
            .entry(id)
            .or_insert_with(|| cx.new(|_| DiffView::new(patch.to_owned().into(), false)))
            .clone();
        view.update(cx, |s, cx| s.set_source(patch, cx));
        view.into_any_element()
    }
    pub(super) fn activity_text(id: String, content: &str, language: &str) -> TextView {
        // Reserve the output viewport before asynchronous Markdown parsing.
        // Long tool output scrolls inside it instead of moving the conversation.
        let lines = content.lines().take(18).count().max(2);
        TextView::markdown(SharedString::from(id), fenced(content, language))
            .selectable(true)
            .scrollable(true)
            .h(px(lines as f32 * 22. + 32.))
    }
    pub(super) fn item(
        &mut self,
        projected: &agent_core::presentation::conversation::RenderedItem,
        item: &Item,
        turn: Option<&Turn>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = item.id.to_string();
        let title = projected.data.title.as_deref();
        let body_text = projected.data.body.as_deref();
        let expanded = self.timeline.expanded_items.contains(&id);
        let deferred = projected.data.deferred;
        if expanded && deferred {
            let key = (
                turn.map(|turn| turn.id.clone()).unwrap_or_default(),
                item.id.clone(),
            );
            let operation = self
                .selected()
                .cloned()
                .map(|thread_id| op::OperationKey::Item {
                    item: op::ReadItem {
                        thread_id,
                        turn_id: turn.map(|turn| turn.id.clone()).unwrap_or_default(),
                        item_id: item.id.clone(),
                    },
                });
            let label = if operation
                .as_ref()
                .is_some_and(|key| self.snapshot.operation_running(key.clone()))
            {
                "詳細を読み込み中…".to_owned()
            } else if let Some(error) = operation.and_then(|key| self.snapshot.operation_error(key))
            {
                format!("{error} · 再試行")
            } else {
                "詳細を読み込む".to_owned()
            };
            let title = title.expect("activity has a title");
            let toggle = id.clone();
            return v_flex()
                .gap_2()
                .child(Self::button(
                    format!("collapse-{id}"),
                    format!("⌄ {title}"),
                    cx,
                    move |s, _, _| {
                        s.timeline.expanded_items.remove(&toggle);
                        s.timeline.remeasure_item(&toggle);
                    },
                ))
                .child(Self::button(
                    format!("detail-{id}"),
                    label,
                    cx,
                    move |s, _, _| {
                        s.detail(key.0.clone(), key.1.clone());
                        s.timeline.remeasure_item(key.1.as_str());
                    },
                ))
                .into_any_element();
        }
        let turn_id = turn.map(|turn| turn.id.clone()).unwrap_or_default();
        match item.body() {
            agent_protocol::items::ItemBody::AssistantText { .. } => {
                self.markdown(id, body_text.expect("assistant message has text"), cx)
            }
            agent_protocol::items::ItemBody::ImageGeneration { .. } => {
                let mut body = v_flex().gap_2().w_full();
                if projected.data.image_placeholder {
                    body = body.child(image_skeleton(320., cx.theme().secondary));
                }
                for source in &projected.data.image_sources {
                    body = body.child(self.image(source, false, 320., true, cx));
                }
                body.when_some(body_text, |body, text| {
                    body.child(div().text_sm().child(text.to_owned()))
                })
                .into_any_element()
            }
            agent_protocol::items::ItemBody::UserMessage { text, content } => {
                let mut body = user_message_bubble();
                let mut has_body = false;
                if !content.is_empty() {
                    for (i, part) in content.iter().enumerate() {
                        use agent_protocol::items::MessagePart;
                        let rendered = match part {
                            MessagePart::Image { .. } => continue,
                            MessagePart::Text { text } if text.is_empty() => continue,
                            MessagePart::Text { text } => {
                                self.markdown(format!("{id}-{i}"), text, cx)
                            }
                            MessagePart::Attachment { name, path }
                            | MessagePart::Invocation { name, path } => {
                                let path = path.clone();
                                Self::button(
                                    format!("{id}-{i}"),
                                    if path.is_empty() {
                                        name.clone()
                                    } else {
                                        file_name(&path)
                                    },
                                    cx,
                                    move |s, _, _| {
                                        if !path.is_empty() {
                                            s.download(path.clone());
                                        }
                                    },
                                )
                                .into_any_element()
                            }
                        };
                        has_body = true;
                        body = body.child(rendered);
                    }
                } else {
                    has_body = !text.as_deref().unwrap_or_default().is_empty();
                    body = body.child(self.markdown(
                        id.clone(),
                        text.as_deref().unwrap_or_default(),
                        cx,
                    ));
                }
                let body = self.user_message_content(
                    body,
                    has_body,
                    projected.data.image_sources.iter().map(String::as_str),
                    cx,
                );
                let text = body_text.expect("user message has text");
                let copied = text.to_owned();
                let edit = Self::button(format!("edit-{id}"), "", cx, move |s, window, cx| {
                    selection::set_composer_text(
                        &s.composer.input,
                        copied.clone().into(),
                        window,
                        cx,
                    );
                });
                let timestamp = turn.and_then(|turn| turn.started_at.map(|time| time as i64));
                user_message_row(&id, text, body, timestamp, edit).into_any_element()
            }
            agent_protocol::items::ItemBody::CommandExecution {
                command,
                output,
                cwd,
                exit_code,
                ..
            } => {
                let label = title.expect("command has a title");
                let toggle = id.clone();
                let mut body = v_flex().gap_2().child(
                    Self::button(
                        format!("expand-{id}"),
                        format!("{label} {}", if expanded { "⌄" } else { "›" }),
                        cx,
                        move |s, _, _| {
                            toggle_set(&mut s.timeline.expanded_items, &toggle);
                            if s.timeline.expanded_items.contains(&toggle) {
                                s.detail(turn_id.clone(), toggle.clone().into());
                            }
                            s.timeline.pause_tail();
                            s.timeline.remeasure_item(&toggle);
                        },
                    )
                    .icon(IconName::SquareTerminal)
                    .text_color(rgb(appearance::MUTED)),
                );
                if expanded {
                    let copied = output.clone();
                    if let Some(cwd) = cwd {
                        body = body.child(
                            div()
                                .text_xs()
                                .text_color(rgb(appearance::MUTED))
                                .child(format!("cwd: {cwd}")),
                        );
                    }
                    body = body.child(Self::activity_text(format!("command-{id}"), command, "sh"));
                    body = body
                        .child(
                            h_flex()
                                .justify_between()
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(rgb(appearance::MUTED))
                                        .child("プレーンテキスト"),
                                )
                                .child(
                                    Button::new(format!("copy-output-{id}"))
                                        .label("出力をコピー")
                                        .small()
                                        .ghost()
                                        .on_click(move |_, _, cx| {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                copied.clone(),
                                            ))
                                        }),
                                ),
                        )
                        .child(Self::activity_text(format!("output-{id}"), output, ""))
                        .child(
                            div()
                                .text_sm()
                                .text_color(rgb(appearance::MUTED))
                                .child(format!(
                                    "{} {}",
                                    item.status.label(),
                                    exit_code.map(|v| format!("exit {v}")).unwrap_or_default()
                                )),
                        );
                }
                body.into_any_element()
            }
            agent_protocol::items::ItemBody::FileChange { .. } => {
                let mut body = v_flex().gap_3();
                for (i, change) in agent_core::presentation::body::file_changes(item).enumerate() {
                    let path = change.path.into_owned();
                    let toggle = id.clone();
                    let turn_id = turn_id.clone();
                    let selector = format!("change-{id}-{i}");
                    let row = h_flex()
                        .debug_selector(move || selector.clone())
                        .gap_2()
                        .child(Self::button(
                            format!("change-{id}-{i}"),
                            format!("{} {}", if expanded { "⌄" } else { "›" }, file_name(&path)),
                            cx,
                            move |s, _, _| {
                                toggle_set(&mut s.timeline.expanded_items, &toggle);
                                if s.timeline.expanded_items.contains(&toggle) {
                                    s.detail(turn_id.clone(), toggle.clone().into());
                                }
                                s.timeline.pause_tail();
                                s.timeline.remeasure_item(&toggle);
                            },
                        ))
                        .child(Self::button(
                            format!("download-{id}-{i}"),
                            "ダウンロード",
                            cx,
                            move |s, _, _| s.download(path.clone()),
                        ));
                    body = body.child(row);
                    if expanded {
                        if let Some(diff) = &change.diff {
                            body = body.child(Self::diff(
                                &mut self.timeline.diffs,
                                format!("diff-{id}-{i}"),
                                diff,
                                cx,
                            ));
                        }
                        if change.diff.is_none() || change.proposal.is_some() {
                            let details = agent_core::presentation::body::file_change_details(
                                None,
                                change.proposal,
                            );
                            body = body.child(Self::activity_text(
                                format!("file-details-{id}-{i}"),
                                &details,
                                "",
                            ));
                        }
                    }
                }
                body.into_any_element()
            }
            agent_protocol::items::ItemBody::Plan { text } => {
                let copied = text.clone();
                let content = self.markdown(id.clone(), text, cx);
                v_flex()
                    .gap_2()
                    .w_full()
                    .p_3()
                    .border_1()
                    .border_color(cx.theme().border)
                    .rounded_md()
                    .child(
                        h_flex().justify_between().child("計画").child(
                            Button::new(format!("copy-plan-{id}"))
                                .icon(IconName::Copy)
                                .small()
                                .ghost()
                                .on_click(move |_, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(copied.clone()))
                                }),
                        ),
                    )
                    .child(content)
                    .into_any_element()
            }
            _ => {
                let toggle = id.clone();
                v_flex()
                    .gap_2()
                    .child(Self::button(
                        format!("unknown-{id}"),
                        format!(
                            "{} {}",
                            if expanded { "⌄" } else { "›" },
                            title.expect("activity has a title")
                        ),
                        cx,
                        move |s, _, _| {
                            toggle_set(&mut s.timeline.expanded_items, &toggle);
                            if s.timeline.expanded_items.contains(&toggle) {
                                s.detail(turn_id.clone(), toggle.clone().into());
                            }
                            s.timeline.pause_tail();
                            s.timeline.remeasure_item(&toggle);
                        },
                    ))
                    .when(expanded, |body| {
                        body.child(Self::activity_text(
                            format!("json-{id}"),
                            &projected.expanded_body(),
                            "json",
                        ))
                    })
                    .into_any_element()
            }
        }
    }
    pub(super) fn turn_row(
        &mut self,
        session: Option<&SessionRef>,
        projected: &Arc<agent_core::presentation::conversation::RenderedTurn>,
        index: usize,
        last: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let turn = &projected.source;
        let mut body = v_flex().w_full().max_w(px(CHAT_WIDTH)).gap_4();
        let row = &projected.rows[index];
        match &row.content {
            ConversationRowContent::User { item } => {
                body = body.child(self.projected_item(item, turn, cx));
            }
            ConversationRowContent::ActivityHeader { activity } => {
                let id = &activity.id;
                let expanded =
                    activity_is_expanded(activity, self.timeline.expanded_work.get(id).cloned());
                let (label, content) = if activity.is_in_progress {
                    let view = WorkProgressView::view(
                        &row.id,
                        projected.work_progress(!expanded),
                        window,
                        cx,
                    );
                    (view.read(cx).label(), view.into_any_element())
                } else {
                    let label = activity.activity_summary.clone();
                    (label.clone(), div().child(label).into_any_element())
                };
                let header = if activity.activity_can_collapse {
                    let toggle = id.clone();
                    let status = activity.status.clone();
                    let load_items = activity.load_items.clone();
                    Button::new(format!("work-{id}"))
                        .accessibility_label(format!(
                            "{label} {}",
                            if expanded { "⌄" } else { "›" }
                        ))
                        .small()
                        .ghost()
                        .child(div().flex_1().min_w_0().text_ellipsis().child(content))
                        .child(if expanded { "⌄" } else { "›" })
                        .on_click(cx.listener(move |view, _, _, cx| {
                            view.timeline.expanded_work.insert(
                                toggle.clone(),
                                ActivityExpansion {
                                    status: status.clone(),
                                    expanded: !expanded,
                                },
                            );
                            if !expanded && let Some(params) = &load_items {
                                view.dispatch(Intent::LoadTurnItems(params.clone()));
                            }
                            view.timeline.pause_tail();
                            view.timeline.sync(&view.snapshot, false);
                            cx.notify();
                        }))
                        .text_color(rgb(appearance::MUTED))
                        .into_any_element()
                } else {
                    div()
                        .text_color(rgb(appearance::MUTED))
                        .child(content)
                        .into_any_element()
                };
                let selector = format!("work-{id}");
                body = body.child(
                    h_flex()
                        .debug_selector(move || selector.clone())
                        .gap_2()
                        .child(header)
                        .when(activity.is_in_progress, |row| {
                            row.child(crate::activity::indicator())
                        }),
                );
            }
            ConversationRowContent::Activity { item, .. } => {
                body = body.child(self.projected_item(item, turn, cx));
            }
            ConversationRowContent::PendingRequest { request } => {
                body = body.child(self.request_card(request, cx));
            }
            ConversationRowContent::ProviderHandoff { text } => {
                body = body.child(
                    div()
                        .text_xs()
                        .text_color(rgb(appearance::MUTED))
                        .py_2()
                        .child(text.clone()),
                );
            }
            ConversationRowContent::Error { error } => {
                let provider = turn.provider.or_else(|| session.map(|s| s.provider));
                let accent = if error.is_reconnecting {
                    rgb(appearance::MUTED)
                } else if error.is_usage_limit {
                    rgb(0xe5b567)
                } else {
                    rgb(0xff8e86)
                };
                let copied = format!(
                    "{}\n{}\n{}",
                    error.title,
                    error.message,
                    error.details.as_deref().unwrap_or_default()
                );
                body = body.child(
                    v_flex()
                        .debug_selector(|| "turn-error".into())
                        .p_3()
                        .rounded_lg()
                        .bg(rgb(appearance::RAISED))
                        .border_1()
                        .border_color(accent.opacity(0.25))
                        .gap_2()
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(appearance::MUTED))
                                .child("この応答"),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .when(error.is_reconnecting, |heading| {
                                    heading.child(crate::activity::indicator())
                                })
                                .child(
                                    div()
                                        .flex_1()
                                        .font_semibold()
                                        .text_color(accent)
                                        .child(error.title.clone()),
                                )
                                .child(Self::icon_button(
                                    format!("copy-turn-error-{}", turn.id),
                                    IconName::Copy,
                                    "エラーの詳細をコピー",
                                    cx,
                                    move |_, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            copied.clone(),
                                        ))
                                    },
                                )),
                        )
                        .child(div().text_sm().child(error.message.clone()))
                        .when_some(error.details.clone(), |card, details| {
                            card.child(
                                div()
                                    .id(format!("turn-error-details-{}", turn.id))
                                    .text_xs()
                                    .line_clamp(3)
                                    .text_color(rgb(appearance::MUTED))
                                    .tooltip({
                                        let details = details.clone();
                                        move |window, cx| {
                                            tooltip::Tooltip::new(details.clone()).build(window, cx)
                                        }
                                    })
                                    .child(details),
                            )
                        })
                        .when(error.is_usage_limit, |card| {
                            card.child(
                                h_flex()
                                    .gap_2()
                                    .flex_wrap()
                                    .child(
                                        Self::button(
                                            format!("usage-settings-{}", turn.id),
                                            "使用量・リセット時刻",
                                            cx,
                                            move |s, _, _| {
                                                s.open_settings();
                                                s.model_controls.provider = provider;
                                                s.model_controls.panel = ModelPanel::Manage;
                                            },
                                        )
                                        .small(),
                                    )
                                    .when(
                                        provider
                                            == Some(agent_protocol::session::ProviderKind::Codex),
                                        |actions| {
                                            actions.child(
                                                Self::button(
                                                    format!("usage-manage-{}", turn.id),
                                                    "追加クレジット・上限を管理",
                                                    cx,
                                                    |s, _, cx| {
                                                        s.open_browser(
                                                            s.panel_scope(),
                                                            "https://chatgpt.com/settings/usage",
                                                            cx,
                                                        );
                                                    },
                                                )
                                                .small(),
                                            )
                                        },
                                    ),
                            )
                        }),
                );
            }
            ConversationRowContent::Response { item, fork_turn_id } => {
                body = body.child(self.projected_item(item, turn, cx));
                if item.data.kind == "agent" {
                    let text = item.data.body.clone().expect("assistant message has text");
                    body = body.child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new(format!("copy-{}", item.data.id))
                                    .icon(IconName::Copy)
                                    .small()
                                    .ghost()
                                    .debug_selector(|| "response-copy".into())
                                    .tooltip("回答をコピー")
                                    .accessibility_label("回答をコピー")
                                    .on_click(move |_, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            text.clone(),
                                        ));
                                    }),
                            )
                            .when_some(
                                session.cloned().zip(fork_turn_id.clone()),
                                |actions, (thread_id, turn_id)| {
                                    let fork_error =
                                        self.snapshot.operation_error(op::OperationKey::Fork {
                                            session: thread_id.clone(),
                                            turn: turn_id.clone(),
                                        });
                                    let forking =
                                        self.snapshot.operation_running(op::OperationKey::Fork {
                                            session: thread_id.clone(),
                                            turn: turn_id.clone(),
                                        });
                                    actions
                                        .when_some(fork_error, |row, error| {
                                            row.child(
                                                div()
                                                    .text_xs()
                                                    .text_color(rgb(0xff8e86))
                                                    .child(error_message(&error)),
                                            )
                                        })
                                        .child(
                                            Button::new(format!("fork-{}", item.data.id))
                                                .small()
                                                .ghost()
                                                .loading(forking)
                                                .tooltip(if forking {
                                                    "分岐を作成中…"
                                                } else {
                                                    "ここから会話を分岐"
                                                })
                                                .accessibility_label(if forking {
                                                    "分岐を作成中…"
                                                } else {
                                                    "ここから会話を分岐"
                                                })
                                                .on_click(cx.listener(
                                                    move |view, _, window, cx| {
                                                        if !view.snapshot.connected || view.busy > 0
                                                        {
                                                            return;
                                                        }
                                                        view.busy += 1;
                                                        view.perform(
                                                            Intent::ForkSession(
                                                                op::ForkSession::new(
                                                                    thread_id.clone(),
                                                                    turn_id.clone(),
                                                                ),
                                                            ),
                                                            OperationCompletion::Busy,
                                                        );
                                                        view.accept_snapshot(window, cx);
                                                        view.timeline.list.remeasure();
                                                        cx.notify();
                                                    },
                                                ))
                                                .icon(Icon::default().path("icons/branch.svg"))
                                                .debug_selector(|| "response-fork".into())
                                                .disabled(
                                                    !self.snapshot.connected
                                                        || self.busy > 0
                                                        || forking,
                                                ),
                                        )
                                        .when(forking, |actions| {
                                            actions.child(
                                                h_flex()
                                                    .gap_2()
                                                    .text_xs()
                                                    .text_color(rgb(appearance::MUTED))
                                                    .child("分岐を作成中…")
                                                    .debug_selector(|| {
                                                        "response-fork-progress".into()
                                                    }),
                                            )
                                        })
                                },
                            )
                            .children(
                                row.timestamp_ms
                                    .and_then(|time| {
                                        i64::try_from(time)
                                            .ok()
                                            .and_then(chrono::DateTime::from_timestamp_millis)
                                    })
                                    .map(|time| {
                                        div().text_xs().text_color(rgb(appearance::MUTED)).child(
                                            time.with_timezone(&chrono::Local)
                                                .format("%H:%M")
                                                .to_string(),
                                        )
                                    }),
                            ),
                    );
                }
            }
            ConversationRowContent::InProgress { .. } => {
                body = body.child(
                    h_flex()
                        .debug_selector(|| "conversation-in-progress".into())
                        .gap_2()
                        .text_sm()
                        .text_color(rgb(appearance::MUTED))
                        .child(crate::activity::indicator())
                        .child(WorkProgressView::view(
                            &row.id,
                            projected.work_progress(false),
                            window,
                            cx,
                        )),
                );
            }
        }
        let selector = format!("conversation-row-{}", row.id);
        h_flex()
            .debug_selector(move || selector.clone())
            .justify_center()
            .w_full()
            .px_6()
            .pb(px(if last { 32. } else { 16. }))
            .child(body)
            .into_any_element()
    }
    pub(super) fn projected_item(
        &mut self,
        item: &agent_core::presentation::conversation::RenderedItem,
        turn: &Arc<Turn>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match &item.source {
            agent_core::presentation::conversation::ItemSource::Native(native) => {
                self.item(item, native, Some(turn), cx)
            }
            agent_core::presentation::conversation::ItemSource::Pending(id, pending) => {
                self.pending_item(id, pending, cx)
            }
        }
    }
    pub(super) fn pending_item(
        &mut self,
        id: &str,
        pending: &agent_core::state::PendingSubmission,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let draft = &pending.draft;
        let mut body = user_message_bubble();
        if !draft.text.is_empty() {
            body = body.child(
                TextView::markdown(
                    SharedString::from(format!("pending-{id}")),
                    literal(&draft.text),
                )
                .selectable(true),
            );
        }
        for (index, attachment) in draft.attachments.iter().enumerate() {
            if !attachment.is_image {
                let path = attachment.path.clone();
                body = body.child(Self::button(
                    format!("pending-{id}-file-{index}"),
                    attachment.name.clone(),
                    cx,
                    move |view, _, _| view.download(path.clone()),
                ));
            }
        }
        let body = self.user_message_content(
            body,
            !draft.text.is_empty() || draft.attachments.iter().any(|file| !file.is_image),
            draft
                .attachments
                .iter()
                .filter(|file| file.is_image)
                .map(|file| file.path.as_str()),
            cx,
        );
        v_flex()
            .w_full()
            .min_w_0()
            .items_end()
            .gap_2()
            .my_4()
            .child(body)
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(appearance::MUTED))
                    .child(pending.delivery_label()),
            )
            .into_any_element()
    }

    fn user_message_content<'a>(
        &mut self,
        body: Div,
        has_body: bool,
        sources: impl Iterator<Item = &'a str>,
        cx: &mut Context<Self>,
    ) -> Div {
        let images: Vec<_> = sources
            .map(|source| {
                div()
                    .w(px(80.))
                    .h(px(80.))
                    .flex_shrink_0()
                    .rounded_lg()
                    .overflow_hidden()
                    .border_1()
                    .border_color(rgb(appearance::BORDER))
                    .debug_selector(|| "user-image-thumbnail".into())
                    .child(self.image(source, false, 80., true, cx))
            })
            .collect();
        v_flex()
            .w_full()
            .min_w_0()
            .max_w(px(560.))
            .items_end()
            .gap_2()
            .when(!images.is_empty(), |column| {
                column.child(
                    h_flex()
                        .justify_end()
                        .gap_2()
                        .flex_wrap()
                        .max_w_full()
                        .children(images),
                )
            })
            .when(has_body, |column| column.child(body))
    }

    pub(super) fn request_card(
        &mut self,
        request: &agent_core::presentation::conversation::Request,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use agent_protocol::requests::{Answer, ElicitationAnswer, ElicitationInput, RequestBody};
        let key = &request.id;
        let Some(inputs) = self.timeline.requests.get(key) else {
            return div().into_any_element();
        };
        let is_question = matches!(request.request_body, RequestBody::Question { .. });
        let mut body = v_flex()
            .gap_3()
            .p_4()
            .rounded_lg()
            .bg(rgb(if is_question {
                appearance::SURFACE
            } else {
                0x30312a
            }))
            .child(request.title.clone())
            .when(
                !is_question || inputs.sent || !request.can_respond,
                |body| body.child(request.body.clone()),
            );
        if let Some(error) = self
            .snapshot
            .operation_error(op::OperationKey::Request { id: key.clone() })
        {
            body = body.child(
                div()
                    .text_sm()
                    .text_color(rgb(0xff8e86))
                    .child(error_message(&error)),
            );
        }
        if !request.details.is_empty() {
            body = body.child(
                TextView::markdown(
                    SharedString::from(format!("request-{key}")),
                    fenced(&request.details, "json"),
                )
                .selectable(true),
            );
        }
        if inputs.sent || !request.can_respond {
            return body.into_any_element();
        }
        match &request.request_body {
            RequestBody::Question { .. } => {
                for question in &inputs.questions {
                    if !question.definition.header.is_empty() {
                        body = body.child(
                            div()
                                .text_sm()
                                .font_semibold()
                                .child(question.definition.header.clone()),
                        );
                    }
                    body = body.child(question.definition.prompt.clone());
                    for (index, option) in question.definition.choices.iter().enumerate() {
                        let request_key = key.to_owned();
                        let question_id = question.definition.id.clone();
                        let choice_id = option.id.clone();
                        let selected = question.selected.contains(&option.id);
                        let number = index + 1;
                        body = body.child(
                            Button::new(format!("answer-{key}-{}-{}", question_id, option.id))
                                .accessibility_label(format!(
                                    "{number}. {} {}",
                                    option.label, option.description
                                ))
                                .ghost()
                                .selected(selected)
                                .toggled(selected)
                                .w_full()
                                .h_auto()
                                .py_2()
                                .child(
                                    h_flex()
                                        .w_full()
                                        .min_w_0()
                                        .items_start()
                                        .gap_3()
                                        .child(
                                            div()
                                                .flex_shrink_0()
                                                .min_w_6()
                                                .h_6()
                                                .px_1()
                                                .rounded_sm()
                                                .bg(rgb(appearance::RAISED))
                                                .text_sm()
                                                .text_color(rgb(appearance::MUTED))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .child(number.to_string()),
                                        )
                                        .child(
                                            v_flex()
                                                .flex_1()
                                                .min_w_0()
                                                .gap_1()
                                                .whitespace_normal()
                                                .text_left()
                                                .child(option.label.clone())
                                                .when(!option.description.is_empty(), |column| {
                                                    column.child(
                                                        div()
                                                            .text_xs()
                                                            .text_color(rgb(appearance::MUTED))
                                                            .child(option.description.clone()),
                                                    )
                                                }),
                                        )
                                        .child(div().flex_shrink_0().size_6().children(
                                            selected.then(|| Icon::new(IconName::Check).size_4()),
                                        )),
                                )
                                .on_click(cx.listener(move |view, _, window, cx| {
                                    let Some(question) =
                                        view.timeline.requests.get_mut(&request_key).and_then(
                                            |inputs| {
                                                inputs.questions.iter_mut().find(|question| {
                                                    question.definition.id == question_id
                                                })
                                            },
                                        )
                                    else {
                                        return;
                                    };
                                    if question.definition.multiple {
                                        toggle_set(&mut question.selected, &choice_id);
                                    } else {
                                        question.selected.clear();
                                        question.selected.insert(choice_id.clone());
                                    }
                                    question
                                        .input
                                        .update(cx, |input, cx| input.set_value("", window, cx));
                                    cx.notify();
                                })),
                        );
                    }
                    if question.definition.allow_free_text {
                        body = body.child(Input::new(&question.input));
                    }
                }
                let key = key.to_owned();
                body = body.child(Self::button(
                    format!("respond-{key}"),
                    "回答を送信",
                    cx,
                    move |view, _, cx| {
                        let Some(inputs) = view.timeline.requests.get(&key) else {
                            return;
                        };
                        let answers = inputs
                            .questions
                            .iter()
                            .map(|question| {
                                (
                                    question.definition.id.clone(),
                                    agent_core::presentation::conversation::build_question_answer(
                                        question.definition.multiple,
                                        question.input.read(cx).value().to_string(),
                                        question.selected.iter().cloned().collect(),
                                    ),
                                )
                            })
                            .collect();
                        view.respond(key.clone(), Answer::Questions { answers });
                    },
                ));
            }
            RequestBody::Approval { choices, .. } | RequestBody::Permission { choices, .. } => {
                let permission = matches!(request.request_body, RequestBody::Permission { .. });
                let mut row = h_flex().gap_2().flex_wrap();
                for choice in choices {
                    let key = key.to_owned();
                    let choice_id = choice.id.clone();
                    row = row.child(Self::button(
                        format!("decision-{key}-{}", choice.id),
                        choice.label.clone(),
                        cx,
                        move |view, _, _| {
                            view.respond(
                                key.clone(),
                                if permission {
                                    Answer::Permission {
                                        choice_id: choice_id.clone(),
                                    }
                                } else {
                                    Answer::Approval {
                                        choice_id: choice_id.clone(),
                                    }
                                },
                            )
                        },
                    ));
                    if !choice.description.is_empty() {
                        body = body.child(choice.description.clone());
                    }
                }
                body = body.child(row);
            }
            RequestBody::Elicitation { input, .. } => {
                if let ElicitationInput::Url { url } = input {
                    let url = url.clone();
                    body = body.child(Self::button(
                        format!("elicitation-url-{key}"),
                        "リンクを開く",
                        cx,
                        move |view, window, cx| {
                            view.click_conversation_link(&url, window.modifiers().control, cx)
                        },
                    ));
                } else {
                    body = body.child(Textarea::new(&inputs.response));
                }
                let accept_key = key.to_owned();
                let request_body = request.request_body.clone();
                body = body.child(Self::button(
                    format!("elicitation-accept-{accept_key}"),
                    "確認して送信",
                    cx,
                    move |view, _, cx| {
                        let Some(inputs) = view.timeline.requests.get(&accept_key) else {
                            return;
                        };
                        match agent_core::presentation::conversation::answer_from_json(
                            &request_body,
                            &inputs.response.read(cx).value(),
                        ) {
                            Ok(answer) => view.respond(accept_key.clone(), answer),
                            Err(error) => view.set_error(error),
                        }
                    },
                ));
                for (action, label) in [
                    (ElicitationAnswer::Decline, "辞退"),
                    (ElicitationAnswer::Cancel, "キャンセル"),
                ] {
                    let key = key.to_owned();
                    body = body.child(Self::button(
                        format!("elicitation-{label}-{key}"),
                        label,
                        cx,
                        move |view, _, _| {
                            view.respond(
                                key.clone(),
                                Answer::Elicitation {
                                    action: action.clone(),
                                },
                            )
                        },
                    ));
                }
            }
            RequestBody::ToolExecution { .. } => {
                let key = key.to_owned();
                let request_body = request.request_body.clone();
                body = body
                    .child(Textarea::new(&inputs.response))
                    .child(Self::button(
                        format!("tool-answer-{key}"),
                        "実行結果を送信",
                        cx,
                        move |view, _, cx| {
                            let Some(inputs) = view.timeline.requests.get(&key) else {
                                return;
                            };
                            match agent_core::presentation::conversation::answer_from_json(
                                &request_body,
                                &inputs.response.read(cx).value(),
                            ) {
                                Ok(answer) => view.respond(key.clone(), answer),
                                Err(error) => view.set_error(error),
                            }
                        },
                    ));
            }
        }
        h_flex()
            .w_full()
            .justify_center()
            .px_6()
            .pb_5()
            .child(body.max_w(px(CHAT_WIDTH)))
            .into_any_element()
    }
    pub(in crate::app) fn sync_request_inputs(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.timeline
            .requests
            .retain(|key, _| self.snapshot.request(key).is_some());
        for request in self.snapshot.requests() {
            let key = &request.id;
            if self.timeline.requests.contains_key(key) {
                continue;
            }
            let questions = match &request.body {
                agent_protocol::requests::RequestBody::Question { questions } => questions
                    .iter()
                    .map(|question| Question {
                        definition: question.clone(),
                        selected: HashSet::new(),
                        input: cx.new(|cx| {
                            InputState::new(window, cx)
                                .placeholder("回答を入力")
                                .masked(question.secret)
                        }),
                    })
                    .collect(),
                _ => Vec::new(),
            };
            self.timeline.requests.insert(
                key.clone(),
                RequestInputs {
                    questions,
                    response: cx.new(|cx| {
                        TextareaState::new(window, cx)
                            .default_value(
                                agent_core::presentation::conversation::request_input_default(
                                    request.body.clone(),
                                ),
                            )
                            .auto_grow(3, 8)
                    }),
                    sent: false,
                },
            );
        }
    }
}

fn user_message_row(
    id: &str,
    text: &str,
    body: Div,
    timestamp: Option<i64>,
    edit: Button,
) -> impl IntoElement {
    let time = timestamp
        .and_then(|value| chrono::DateTime::from_timestamp(value, 0))
        .map(|value| {
            value
                .with_timezone(&chrono::Local)
                .format("%H:%M")
                .to_string()
        });
    let copy_text = text.to_owned();
    let group = SharedString::from(format!("user-message-{id}"));
    v_flex()
        .w_full()
        .min_w_0()
        .group(group.clone())
        .items_end()
        .gap_1()
        .my_4()
        .child(body)
        .child(
            h_flex()
                .debug_selector(|| "user-message-actions".into())
                .invisible()
                .group_hover(group, |style| style.visible())
                .children(time.map(|time| {
                    div()
                        .text_xs()
                        .text_color(rgb(appearance::MUTED))
                        .mr_2()
                        .child(time)
                }))
                .child(
                    Button::new(format!("copy-{id}"))
                        .debug_selector(|| "user-message-copy".into())
                        .icon(IconName::Copy)
                        .small()
                        .ghost()
                        .tooltip("発言をコピー")
                        .accessibility_label("発言をコピー")
                        .disabled(copy_text.is_empty())
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(copy_text.clone()));
                        }),
                )
                .child(
                    edit.icon(Icon::default().path("icons/pencil.svg"))
                        .small()
                        .ghost()
                        .debug_selector(|| "user-message-edit".into())
                        .tooltip("入力欄で編集")
                        .accessibility_label("入力欄で編集")
                        .disabled(text.is_empty()),
                ),
        )
}

#[cfg(test)]
mod tests {
    use super::{user_message_bubble, user_message_row};
    use gpui_kit as gpui;
    use gpui_kit::{
        ClipboardItem, Context, IntoElement, Modifiers, ParentElement, Render, TestAppContext,
        Window,
    };

    struct Message;
    impl Render for Message {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            user_message_row(
                "test",
                "一行目\nsecond line",
                user_message_bubble().child("一行目\nsecond line"),
                Some(1),
                gpui::component::button::Button::new("edit"),
            )
        }
    }

    #[gpui::test]
    fn hovering_own_message_allows_copying_the_complete_text(cx: &mut TestAppContext) {
        cx.update(crate::appearance::init);
        let (_, cx) = cx.add_window_view(|_, _| Message);
        cx.run_until_parked();
        let bounds = cx.debug_bounds("user-message-actions").unwrap();
        cx.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string("previous".into())));
        cx.simulate_mouse_move(bounds.center(), None, Modifiers::default());
        cx.run_until_parked();
        let copy = cx.debug_bounds("user-message-copy").unwrap().center();
        cx.simulate_click(copy, Modifiers::default());
        cx.update(|_, cx| {
            assert_eq!(
                cx.read_from_clipboard().unwrap().text().unwrap(),
                "一行目\nsecond line"
            );
        });
    }
}

#[cfg(test)]
mod rendering_tests {
    use super::{Desktop, Mode, Snapshot};
    use crate::app::ImageState;
    use agent_core::state::operations as op;
    use gpui_kit as gpui;
    use gpui_kit::{
        AppContext, Context, Entity, IntoElement, ParentElement, Render, RenderImage, Styled,
        TestAppContext, Window, px, size,
    };
    use std::sync::Arc;

    struct ClockView {
        progress: super::WorkProgress,
        clock: Option<Entity<super::WorkProgressView>>,
    }

    impl Render for ClockView {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let clock = super::WorkProgressView::view("fixture", self.progress.clone(), window, cx);
            self.clock = Some(clock.clone());
            clock
        }
    }

    struct ConversationView {
        desktop: Entity<Desktop>,
    }
    impl Render for ConversationView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.desktop.update(cx, |desktop, cx| {
                gpui_kit::div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .child(desktop.conversation_list(cx))
            })
        }
    }

    fn init(cx: &mut TestAppContext) -> tokio::runtime::Runtime {
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
        runtime
    }

    impl ConversationView {
        fn new(
            mut snapshot: Snapshot,
            source: agent_protocol::models::Thread,
            window: &mut Window,
            cx: &mut Context<Self>,
        ) -> Self {
            Arc::make_mut(&mut snapshot.navigation).thread_id = source.id.clone();
            Arc::make_mut(&mut snapshot.conversations)
                .insert(source.id.clone().unwrap(), Arc::new(source));
            let desktop = cx.new(|cx| {
                let mut desktop = Desktop::new(
                    Mode::SideChat {
                        // Reject before reading preferences or connecting to a real Host.
                        remote: Some(agent_protocol::models::RemoteHost {
                            id: "fixture".into(),
                            name: "fixture".into(),
                            ticket: "invalid-fixture-ticket".into(),
                        }),
                        cwd: "/fixture".into(),
                    },
                    window,
                    cx,
                );
                desktop.snapshot = Arc::new(snapshot);
                desktop.timeline.sync(&desktop.snapshot, true);
                desktop
            });
            Self { desktop }
        }
    }

    #[gpui::test]
    fn elapsed_work_ticks_without_notifying_its_conversation(cx: &mut TestAppContext) {
        use std::{cell::Cell, rc::Rc, time::Duration};
        let _runtime = init(cx);
        let source = serde_json::from_value(serde_json::json!({
            "turns":[{"id":"running", "status":"running", "startedAt":100.}]
        }))
        .unwrap();
        let conversation = agent_core::presentation::conversation::project_conversation(
            &Snapshot::default(),
            Arc::new(source),
            &None,
        );
        let progress = conversation.turns[0].work_progress(false);
        let (view, window) = cx.add_window_view(|_, _| ClockView {
            progress,
            clock: None,
        });
        window.run_until_parked();
        let clock = view.read_with(window, |view, _| view.clock.clone().unwrap());
        let parent_notifications = Rc::new(Cell::new(0));
        let ticks = Rc::new(Cell::new(0));
        let _subscriptions = window.update(|_, cx| {
            let parents = parent_notifications.clone();
            let ticks = ticks.clone();
            (
                cx.observe(&view, move |_, _| parents.set(parents.get() + 1)),
                cx.observe(&clock, move |_, _| ticks.set(ticks.get() + 1)),
            )
        });
        window.executor().advance_clock(Duration::from_secs(1));
        window.run_until_parked();
        assert!(ticks.get() > 0, "the visible elapsed label must update");
        assert_eq!(
            parent_notifications.get(),
            0,
            "a clock tick must not invalidate the conversation"
        );
    }

    #[gpui::test]
    fn a_long_single_turn_only_mounts_messages_near_the_viewport(cx: &mut TestAppContext) {
        use std::{cell::Cell, rc::Rc, time::Duration};
        let _runtime = init(cx);
        let source = serde_json::from_value(serde_json::json!({
            "id": {"provider":"codex", "id":"fixture"},
            "turns": [{"id":"long-turn", "status":"running", "items":
                (0..1000).map(|index| serde_json::json!({
                    "id":format!("answer-{index}"),
                    "body":{"inline":{"body":{"assistantText":{
                        "text":format!("Update {index}\n\nA paragraph explaining the current work."),
                        "phase":"final"
                    }}}}
                })).collect::<Vec<_>>()
            }]
        })).unwrap();
        let (view, window) = cx.add_window_view(|window, cx| {
            ConversationView::new(Snapshot::default(), source, window, cx)
        });
        window.simulate_resize(size(px(900.), px(650.)));
        window.run_until_parked();
        view.update(window, |view, cx| {
            let desktop = view.desktop.read(cx);
            assert!(desktop.timeline.rows.len() >= 1000);
            assert!(desktop.timeline.markdown.contains_key("answer-999"));
            assert!(!desktop.timeline.markdown.contains_key("answer-0"));
            assert!(
                desktop.timeline.markdown.len() < 40,
                "opening a turn must not parse its offscreen messages"
            );
        });
        let notifications = Rc::new(Cell::new(0));
        let _subscription = window.update(|_, cx| {
            let count = notifications.clone();
            let desktop = view.read(cx).desktop.clone();
            cx.observe(&desktop, move |_, _| count.set(count.get() + 1))
        });
        window.executor().advance_clock(Duration::from_secs(1));
        window.run_until_parked();
        assert_eq!(
            notifications.get(),
            0,
            "elapsed time must not notify Desktop or remeasure the whole active turn"
        );
        view.update(window, |view, cx| {
            view.desktop.update(cx, |desktop, cx| {
                desktop
                    .timeline
                    .list
                    .scroll_to(gpui_kit::ListOffset::default());
                cx.notify();
            });
        });
        window.run_until_parked();
        view.update(window, |view, cx| {
            assert!(
                view.desktop
                    .read(cx)
                    .timeline
                    .markdown
                    .contains_key("answer-0")
            );
        });
        let first_message = "conversation-row-history-item:long-turn:answer-0:occurrence:0";
        let before = window.debug_bounds(first_message).unwrap().top();
        view.update(window, |view, cx| {
            view.desktop.update(cx, |desktop, cx| {
                let session = desktop.selected().unwrap().clone();
                let snapshot = Arc::make_mut(&mut desktop.snapshot);
                let thread = Arc::make_mut(
                    Arc::make_mut(&mut snapshot.conversations)
                        .get_mut(&session)
                        .unwrap(),
                );
                let items = Arc::make_mut(&mut thread.turns.as_mut().unwrap()[0])
                    .items
                    .as_mut()
                    .unwrap();
                items.insert(
                    0,
                    Arc::new(
                        serde_json::from_value(serde_json::json!({
                            "id":"older-answer", "body":{"inline":{"body":{"assistantText":{
                                "text":"An earlier message from the same turn.", "phase":"final"
                            }}}}
                        }))
                        .unwrap(),
                    ),
                );
                items.push(Arc::new(
                    serde_json::from_value(serde_json::json!({
                        "id":"new-answer", "body":{"inline":{"body":{"assistantText":{
                            "text":"A live update arriving with the history page.", "phase":"final"
                        }}}}
                    }))
                    .unwrap(),
                ));
                desktop.timeline.sync(&desktop.snapshot, false);
                cx.notify();
            })
        });
        window.run_until_parked();
        view.update(window, |view, cx| {
            let desktop = view.desktop.read(cx);
            assert!(!desktop.timeline.list.is_following_tail());
        });
        assert!(
            (window.debug_bounds(first_message).unwrap().top() - before).abs() < px(1.),
            "prepending history while the same turn streams must preserve the visible message"
        );
    }

    #[gpui::test]
    fn fork_progress_is_visible_only_at_the_pending_source(cx: &mut TestAppContext) {
        let _runtime = init(cx);
        for (session, turn, phase, visible) in [
            ("fixture", "turn", op::OperationPhase::Running, true),
            ("other", "turn", op::OperationPhase::Running, false),
            ("fixture", "other", op::OperationPhase::Running, false),
            (
                "fixture",
                "turn",
                op::OperationPhase::Failed {
                    message: "failed".into(),
                },
                false,
            ),
        ] {
            let source = serde_json::from_value(serde_json::json!({"id":{"provider":"codex","id":"fixture"},"capabilities":{"additionalInput":true,"fork":true,"rename":true,"modelChange":true},"turns":[{"id":"turn","status":"completed","items":[{"id":"answer","status":"unknown","clientInputId":null,"body":{"inline":{"body":{"assistantText":{"text":"Answer","phase":"final"}}}}}]}]})).unwrap();
            let (_, window) = cx.add_window_view(|window, cx| {
                let view = ConversationView::new(Snapshot::default(), source, window, cx);
                view.desktop.update(cx, |desktop, _| {
                    Arc::make_mut(&mut Arc::make_mut(&mut desktop.snapshot).operations).insert(
                        op::OperationKey::Fork {
                            session: agent_protocol::session::SessionRef {
                                provider: agent_protocol::session::ProviderKind::Codex,
                                id: session.into(),
                            },
                            turn: turn.into(),
                        },
                        op::OperationState {
                            generation: 1,
                            phase,
                        },
                    );
                });
                view
            });
            window.run_until_parked();
            assert_eq!(
                window.debug_bounds("response-fork-progress").is_some(),
                visible
            );
        }
    }

    #[gpui::test]
    fn completed_response_offers_fork_but_streaming_response_does_not(cx: &mut TestAppContext) {
        let _runtime = init(cx);
        for status in ["completed", "running"] {
            let source = serde_json::from_value(serde_json::json!({"id":{"provider":"codex","id":"fixture"},"capabilities":{"additionalInput":true,"fork":true,"rename":true,"modelChange":true},"turns":[{"id":"turn","status":status,"items":[{"id":"answer","status":"unknown","clientInputId":null,"body":{"inline":{"body":{"assistantText":{"text":"Answer","phase":"final"}}}}}]}]}))
            .unwrap();
            let (_, window) = cx.add_window_view(|window, cx| {
                ConversationView::new(Snapshot::default(), source, window, cx)
            });
            window.run_until_parked();
            assert_eq!(
                window.debug_bounds("response-fork").is_some(),
                status == "completed"
            );
        }
    }

    #[gpui::test]
    fn background_work_keeps_progress_below_the_response_until_the_turn_ends(
        cx: &mut TestAppContext,
    ) {
        let _runtime = init(cx);
        for provider in ["codex", "claude"] {
            for status in ["running", "completed", "failed", "interrupted"] {
                let source = serde_json::from_value(serde_json::json!({
                    "id":{"provider":provider,"id":"fixture"}, "turns":[{
                        "id":"turn", "status":status, "items":[
                            {"id":"command","status":"completed","body":{"inline":{"body":{
                                "commandExecution":{"command":"review","cwd":"/fixture","output":"launched","exitCode":null}
                            }}}},
                            {"id":"answer","status":"completed","body":{"inline":{"body":{
                                "assistantText":{"text":"Long answer\n\n".repeat(20)+"結果が出たら報告します。","phase":"final"}
                            }}}}
                        ]
                    }]
                })).unwrap();
                let (_, window) = cx.add_window_view(|window, cx| {
                    ConversationView::new(Snapshot::default(), source, window, cx)
                });
                window.run_until_parked();
                let progress = window.debug_bounds("conversation-in-progress");
                if status == "running" {
                    let progress = progress.expect("background work must remain visible");
                    let response = window.debug_bounds("response-copy").unwrap();
                    assert!(progress.top() >= response.bottom());
                    assert!(progress.size.height > px(0.));
                } else {
                    assert!(
                        progress.is_none(),
                        "{provider}/{status} still shows progress"
                    );
                }
            }
        }
    }

    struct ChatNavigationView(Entity<Desktop>);
    impl Render for ChatNavigationView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.0.update(cx, |desktop, cx| desktop.chat(cx))
        }
    }

    #[gpui::test]
    fn conversation_navigation_returns_to_latest_and_resumes_following(cx: &mut TestAppContext) {
        let _runtime = init(cx);
        let source = serde_json::from_value(serde_json::json!({
            "id": {"provider":"codex","id":"fixture"}, "turns": (0..20).map(|i| serde_json::json!({
                "id": format!("turn-{i}"), "status": "completed", "items": [
                    {"id": format!("user-{i}"), "body": {"inline":{"body":{"userMessage":{"text":format!("Question {i}"),"content":[]}}}}},
                    {"id": format!("answer-{i}"), "body":{"inline":{"body":{"assistantText":{"text":"A long answer\n".repeat(10),"phase":"final"}}}}}
                ]
            })).collect::<Vec<_>>()
        })).unwrap();
        let (view, window) = cx.add_window_view(|window, cx| {
            let fixture =
                cx.new(|cx| ConversationView::new(Snapshot::default(), source, window, cx));
            ChatNavigationView(fixture.read(cx).desktop.clone())
        });
        window.simulate_resize(size(px(900.), px(650.)));
        window.run_until_parked();
        view.update(window, |_, cx| cx.notify());
        window.run_until_parked();
        assert!(window.debug_bounds("conversation-marker-0").is_some());
        assert!(window.debug_bounds("conversation-marker-38").is_some());
        assert!(
            window.debug_bounds("conversation-marker-1").is_none(),
            "Long histories must have at most eight evenly spaced markers"
        );
        let first = window.debug_bounds("conversation-marker-0").unwrap();
        let marker_left = window
            .debug_bounds("conversation-marker-line-0")
            .unwrap()
            .left();
        assert_eq!(
            marker_left - first.left(),
            px(16.5),
            "Preserve the original resting marker position"
        );
        let marker_width = window
            .debug_bounds("conversation-marker-line-0")
            .unwrap()
            .size
            .width;
        window.simulate_mouse_move(first.center(), None, gpui_kit::Modifiers::default());
        view.update(window, |_, cx| cx.notify());
        window.run_until_parked();
        let hovered_width = window
            .debug_bounds("conversation-marker-line-0")
            .unwrap()
            .size
            .width;
        let neighbor_width = window
            .debug_bounds("conversation-marker-line-4")
            .unwrap()
            .size
            .width;
        let next_width = window
            .debug_bounds("conversation-marker-line-10")
            .unwrap()
            .size
            .width;
        for selector in [
            "conversation-marker-line-0",
            "conversation-marker-line-4",
            "conversation-marker-line-10",
        ] {
            assert_eq!(
                window.debug_bounds(selector).unwrap().left(),
                marker_left,
                "Hover must keep the left edge fixed and expand only to the right"
            );
        }
        assert!(hovered_width >= px(28.));
        assert!(
            hovered_width > neighbor_width
                && neighbor_width > next_width
                && next_width > marker_width
        );
        window.simulate_mouse_move(
            gpui_kit::point(px(800.), px(30.)),
            None,
            gpui_kit::Modifiers::default(),
        );
        window.run_until_parked();
        assert_eq!(
            window
                .debug_bounds("conversation-marker-line-4")
                .unwrap()
                .size
                .width,
            marker_width
        );
        window.simulate_click(first.center(), gpui_kit::Modifiers::default());
        view.update(window, |_, cx| cx.notify());
        window.run_until_parked();
        let latest = window.debug_bounds("conversation-latest").unwrap();
        view.update(window, |view, cx| {
            assert!(!view.0.read(cx).timeline.list.is_following_tail());
            assert_eq!(
                view.0.read(cx).timeline.list.logical_scroll_top().item_ix,
                0
            );
        });
        window.simulate_click(latest.center(), gpui_kit::Modifiers::default());
        view.update(window, |_, cx| cx.notify());
        window.run_until_parked();
        let last = window
            .debug_bounds("conversation-row-history-item:turn-19:answer-19:occurrence:0")
            .unwrap();
        view.update(window, |view, cx| {
            let desktop = view.0.read(cx);
            assert!(desktop.timeline.list.is_following_tail());
            assert!(
                (last.bottom() - desktop.timeline.list.viewport_bounds().bottom()).abs() < px(1.)
            );
        });
        assert!(window.debug_bounds("conversation-latest").is_none());
    }

    #[gpui::test]
    fn file_changes_render_and_expand_in_the_desktop_view(cx: &mut TestAppContext) {
        let _runtime = init(cx);
        let source = serde_json::from_value(serde_json::json!({"id":{"provider":"codex","id":"fixture"},"turns":[{"id":"turn","status":"completed","items":[{"id":"files","status":"completed","clientInputId":"client-files","body":{"inline":{"body":{"fileChange":{"changes":[{"path":"/fixture/a.txt","kind":{"update":{"movePath":null}},"diff":"@@ -1 +1 @@\n-old\n+new","proposal":null},{"path":"/fixture/b.txt","kind":"add","diff":"@@ -0,0 +1 @@\n+second","proposal":null}],"output":""}}}}}]}]}))
        .unwrap();
        let (view, window) = cx.add_window_view(|window, cx| {
            ConversationView::new(Snapshot::default(), source, window, cx)
        });
        window.run_until_parked();
        assert!(
            window.debug_bounds("change-files-0").is_none(),
            "completed work starts collapsed"
        );
        let group = window.debug_bounds("work-turn").unwrap();
        window.simulate_click(
            gpui_kit::point(group.left() + px(20.), group.center().y),
            gpui_kit::Modifiers::default(),
        );
        view.update(window, |_, cx| cx.notify());
        window.run_until_parked();
        for selector in ["change-files-0", "change-files-1"] {
            assert!(
                window.debug_bounds(selector).is_some(),
                "file header must be visible"
            );
        }
        let before = window.debug_bounds("change-files-1").unwrap().top()
            - window.debug_bounds("change-files-0").unwrap().top();
        let bounds = window.debug_bounds("change-files-0").unwrap();
        window.simulate_click(
            gpui_kit::point(bounds.left() + px(20.), bounds.center().y),
            gpui_kit::Modifiers::default(),
        );
        view.update(window, |_, cx| {
            cx.notify();
        });
        window.run_until_parked();
        assert!(
            window.debug_bounds("change-files-1").unwrap().top()
                - window.debug_bounds("change-files-0").unwrap().top()
                > before,
            "expanded diff must occupy space in the rendered conversation"
        );
    }
    fn user_message_fixture(
        text: &str,
        image: Option<&str>,
        pending: bool,
    ) -> (Snapshot, agent_protocol::models::Thread) {
        let message = image.map_or_else(
            || serde_json::json!({"text":text,"content":[]}),
            |source| serde_json::json!({"text":null,"content":[{"text":{"text":text}},{"image":{"source":source}}]}),
        );
        let items = if pending {
            serde_json::json!([])
        } else {
            serde_json::json!([{"id":"user","status":"unknown","clientInputId":null,"body":{"inline":{"body":{"userMessage":message}}}}])
        };
        let mut snapshot = Snapshot::default();
        if pending {
            Arc::make_mut(&mut snapshot.pending_submissions).insert(
                "pending".into(),
                Arc::new(agent_core::state::PendingSubmission {
                    sequence: 0,
                    draft_key: agent_protocol::session::SessionRef {
                        provider: agent_protocol::session::ProviderKind::Codex,
                        id: "fixture".into(),
                    }
                    .into(),
                    draft: Arc::new(agent_core::state::Draft {
                        text: text.into(),
                        attachments: image
                            .map(|source| agent_core::state::Attachment {
                                path: source.into(),
                                name: "image.png".into(),
                                is_image: true,
                            })
                            .into_iter()
                            .collect(),
                        ..Default::default()
                    }),
                    turn_id: Some("turn".into()),
                    after_item_id: None,
                    accepted: true,
                    delivery_unknown: false,
                }),
            );
        }
        let source = serde_json::from_value(serde_json::json!({
            "id":{"provider":"codex","id":"fixture"},
            "turns":[{"id":"turn","status":"completed","items":items}]
        }))
        .unwrap();
        (snapshot, source)
    }

    #[gpui::test]
    fn user_messages_keep_readable_widths(cx: &mut TestAppContext) {
        let _runtime = init(cx);
        for pending in [false, true] {
            let long = "The development Host inherits `$SHELL` from its environment. \
                長い発言も会話の幅に合わせて折り返し、箇条書きの本文をすべて読めるようにする。";
            for text in ["おｋ".to_owned(), long.to_owned(), format!("- {long}")] {
                let short = text == "おｋ";
                let (snapshot, source) = user_message_fixture(&text, None, pending);
                let (view, window) = cx.add_window_view(|window, cx| {
                    ConversationView::new(snapshot, source, window, cx)
                });
                for viewport_width in [780_f32, 360.] {
                    window.simulate_resize(size(px(viewport_width), px(600.)));
                    window.run_until_parked();
                    view.update(window, |_, cx| cx.notify());
                    window.run_until_parked();
                    let bubble = window.debug_bounds("user-message-bubble").unwrap();
                    if short {
                        assert!(bubble.size.width < px(120.), "short message: {bubble:?}");
                    } else {
                        // List items keep the bullet column outside their measured text.
                        assert!(
                            bubble.size.width >= px((viewport_width - 48.).min(560.) * 0.9),
                            "long message must use the available width (pending={pending}): {bubble:?}"
                        );
                    }
                    assert!(
                        bubble.size.height < px(240.),
                        "message must wrap into readable lines: {bubble:?}"
                    );
                    assert!(
                        bubble.left() >= px(0.) && bubble.right() <= px(viewport_width),
                        "message overflows the conversation (pending={pending}, viewport={viewport_width}): {bubble:?}"
                    );
                }
            }
        }
    }

    #[gpui::test]
    fn chat_images_stay_inside_the_bubble_at_different_window_sizes(cx: &mut TestAppContext) {
        let _runtime = init(cx);
        for pending in [false, true] {
            for (width, height) in [(1156, 78), (78, 1156), (400, 400)] {
                let image = Arc::new(RenderImage::new(vec![image::Frame::new(
                    image::RgbaImage::new(width, height),
                )]));
                let (snapshot, source) =
                    user_message_fixture("caption", Some("/fixture/image.png"), pending);
                let (view, window) = cx.add_window_view(|window, cx| {
                    let view = ConversationView::new(snapshot, source, window, cx);
                    view.desktop.update(cx, |desktop, _| {
                        let source = "/fixture/image.png".to_owned();
                        let key = desktop.image_key(&source, false);
                        desktop.timeline.images.insert(
                            key,
                            ImageState {
                                source: Arc::new(source),
                                path: Some(image.into()),
                                error: None,
                            },
                        );
                    });
                    view
                });
                for viewport_width in [780_f32, 360.] {
                    window.simulate_resize(size(px(viewport_width), px(600.)));
                    window.run_until_parked();
                    view.update(window, |_, cx| cx.notify());
                    window.run_until_parked();
                    let thumbnail = window.debug_bounds("user-image-thumbnail").unwrap();
                    let bubble = window.debug_bounds("user-message-bubble").unwrap();
                    assert_eq!(thumbnail.size, size(px(80.), px(80.)));
                    assert!(thumbnail.bottom() <= bubble.top());
                    assert_eq!(thumbnail.right(), bubble.right());
                    let bounds = window
                        .debug_bounds("chat-image")
                        .expect("image must render");
                    assert!(
                        bounds.size.width > px(0.),
                        "image must remain visible: {bounds:?}"
                    );
                    assert!(
                        bounds.size.width <= px(528_f32.min(viewport_width - 32.)),
                        "image exceeds bubble: {bounds:?}"
                    );
                    assert!(
                        bounds.left() >= px(0.) && bounds.right() <= px(viewport_width),
                        "image overflows conversation: {bounds:?}"
                    );
                }
            }
        }
    }
}
