use crate::l10n::{localized, tr, tr_format};
use crate::ui::Enabled;
use crate::{Connection, shared::Shared};
use agent_core::{
    presentation::conversation::{
        ActivityPresentation, ConversationRowContent, RenderedConversation, WorkProgress,
        activity_is_expanded,
    },
    state::{Intent, operations as op},
};
use nsh_base::theme;
use nsh_core::geometry::{Point, Points};
use nsh_core::prelude::*;
use std::collections::{HashMap, HashSet};

const FOLLOW_END_THRESHOLD: f32 = 60.0;
/// A message column and its padding, which the minimap must not overlap.
const CONTENT_WIDTH: f32 = 780.0 + 2.0 * 24.0;

pub(crate) fn reveal_row(list: ElementRef, index: u32) {
    after_answer(move || {
        list.scroll_to_index(index, nsh_core::geometry::Percentage(0.0), px(0.0), false);
    });
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    User,
    Response,
    Activity,
}

#[derive(Clone, PartialEq)]
struct Row {
    id: String,
    body: SharedString,
    kind: Kind,
    actions: bool,
    activity: Option<ActivityPresentation>,
    fork: Option<agent_protocol::ids::TurnId>,
    retry: Option<agent_protocol::operations::RetryPreparation>,
    images: Vec<String>,
    detail: Option<crate::activity::Item>,
    progress: Option<WorkProgress>,
    /// The local time it was sent or answered (`HH:MM`).
    time: Option<String>,
    /// A message sent while its turn worked; only the first of a run says so.
    steer: bool,
}

fn project(
    conversation: &RenderedConversation,
    expanded_work: &HashMap<String, String>,
) -> Vec<Row> {
    conversation
        .turns
        .iter()
        .flat_map(|turn| {
            let mut expanded = false;
            let progress_shown = turn.rows.iter().any(|row| matches!(&row.content, ConversationRowContent::ActivityHeader { activity } if activity.is_in_progress));
            turn.conversation_rows().into_iter().filter_map(move |row| {
                match &row.content {
                    ConversationRowContent::ActivityHeader { activity } => {
                        expanded = activity_is_expanded(&activity.status, expanded_work.get(&activity.id).cloned());
                    }
                    ConversationRowContent::Activity { .. } if !expanded => return None,
                    ConversationRowContent::InProgress { .. } if progress_shown => return None,
                    _ => {}
                }
                let progress = match &row.content {
                    ConversationRowContent::ActivityHeader { activity } if activity.is_in_progress => Some(turn.work_progress(!expanded)),
                    ConversationRowContent::InProgress { .. } => Some(turn.work_progress(false)),
                    _ => None,
                };
                Some((row, progress))
            })
        })
        .chain(conversation.queued.iter().map(|item| {
            (agent_core::presentation::conversation::ConversationRow {
                id: item.data.id.clone(),
                content: ConversationRowContent::User { item: item.clone() },
                timestamp_ms: item.data.timestamp_ms,
                steer: false,
            }, None)
        }))
        .chain(conversation.session_rows.iter().cloned().map(|row| (row, None)))
        .filter_map(|(row, progress)| {
            let images = match &row.content {
                ConversationRowContent::User { item }
                | ConversationRowContent::Activity { item, .. }
                | ConversationRowContent::Response { item, .. } => item.data.image_sources.clone(),
                _ => Vec::new(),
            };
            let steer = row.steer;
            let time = row
                .timestamp_ms
                .and_then(|ms| i64::try_from(ms).ok())
                .and_then(chrono::DateTime::from_timestamp_millis)
                .map(|time| time.with_timezone(&chrono::Local).format("%H:%M").to_string());
            let activity = match &row.content { ConversationRowContent::ActivityHeader { activity } => Some(activity.clone()), _ => None };
            let actions = matches!(&row.content, ConversationRowContent::Response { show_actions: true, .. });
            let fork = match &row.content { ConversationRowContent::Response { fork_turn_id, .. } => fork_turn_id.clone(), _ => None };
            let retry = match &row.content { ConversationRowContent::Preparation { preparation } => preparation.retry.clone(), _ => None };
            let detail = match &row.content {
                ConversationRowContent::Activity { item, turn_id } if item.data.collapsible => {
                    Some(crate::activity::Item { source: Shared(item.clone()), turn: turn_id.clone() })
                }
                _ => None,
            };
            let (kind, body) = match row.content {
                ConversationRowContent::User { item } => {
                    (Kind::User, item.data.body.clone().unwrap_or_default())
                }
                ConversationRowContent::Response { item, .. } => {
                    (Kind::Response, item.data.body.clone().unwrap_or_default())
                }
                ConversationRowContent::Activity { item, .. } => {
                    (Kind::Activity, item.data.body.clone().or_else(|| item.data.title.clone()).unwrap_or_default())
                }
                ConversationRowContent::ActivityHeader { activity } => {
                    (Kind::Activity, localized(activity.activity_summary))
                }
                ConversationRowContent::Preparation { preparation } => {
                    (Kind::Activity, localized(preparation.text))
                }
                ConversationRowContent::ProviderHandoff { text } => (Kind::Activity, localized(text)),
                ConversationRowContent::Error { error } => (Kind::Activity, localized(error.message)),
                ConversationRowContent::InProgress { .. } => (Kind::Activity, tr("作業中…").into()),
                ConversationRowContent::PendingRequest { .. } => return None,
            };
            Some(Row {
                id: row.id,
                body: body.into(),
                kind,
                actions,
                activity,
                fork,
                retry,
                images,
                detail,
                progress,
                time,
                steer,
            })
        })
        .collect::<Vec<_>>()
        .into_iter()
        .scan(false, |previous_steer, mut row: Row| {
            let steer = row.steer;
            row.steer = steer && !*previous_steer;
            *previous_steer = steer;
            Some(row)
        })
        .collect()
}

pub(crate) fn list(following: Signal<bool>) -> impl IntoElement {
    let surroundings = crate::rich_text::Surroundings::current();
    let connection = surroundings.connection;
    let list = element_ref();
    let expanded = signal(HashMap::<String, String>::new());
    let expanded_items = signal(HashSet::<String>::new());
    let dragging = stored(false);
    let scroll_pending = stored(false);
    let clock = signal(std::time::SystemTime::now());
    let top_row = signal(0_usize);
    let width = signal(0_f32);
    spawn(async move {
        loop {
            timer(std::time::Duration::from_secs(1)).await;
            if connection
                .composer
                .with_untracked(|p| p.controls.running_turn.is_some())
            {
                clock.set(std::time::SystemTime::now());
            }
        }
    });
    let target = computed(move || {
        (
            connection.host.get(),
            connection
                .composer
                .with(|p| p.controls.navigation.draft_key.clone()),
        )
    });
    effect(move || {
        target.get();
        following.set(true);
        expanded.set(HashMap::new());
        expanded_items.set(HashSet::new());
    });
    let rows = computed(move || {
        connection.conversation.with(|conversation| {
            conversation
                .as_ref()
                .map(|conversation| expanded.with(|expanded| project(conversation, expanded)))
                .unwrap_or_default()
        })
    });
    effect(move || {
        target.get();
        if rows.with(|rows| !rows.is_empty())
            && following.get()
            && !dragging.get()
            && !scroll_pending.get()
        {
            scroll_pending.replace(true);
            // Row keys must reach the list before its end can be targeted.
            after_answer(move || {
                scroll_pending.replace(false);
                if following.get_untracked() && !dragging.get() {
                    list.scroll_to_end(false);
                }
            });
        }
    });
    let history = computed(move || connection.context.with(|p| p.history()));
    view()
        .flex_1()
        .min_h_0()
        .on_layout(move |bounds: &Bounds<Points>| width.set(bounds.size.width.0))
        .child(when(
            move || history.get().is_some(),
            move || {
                view()
                    .gap_2()
                    .items_center()
                    .child(
                        crate::ui::text_button(
                            move || {
                                SharedString::from(history.with(|history| {
                                    history
                                        .as_ref()
                                        .map(|history| localized(history.load_label.clone()))
                                        .unwrap_or_default()
                                }))
                            },
                            move || {
                                following.set(false);
                                if let Some(thread_id) =
                                    connection.context.with_untracked(|p| p.navigation().thread_id)
                                {
                                    connection.dispatch(Intent::ReadOlder { thread_id });
                                }
                            },
                        )
                        .enabled(move || {
                            history.with(|history| {
                                history.as_ref().is_some_and(|history| !history.loading)
                            })
                        }),
                    )
                    .child(text(move || {
                        history.with(|history| {
                            history
                                .as_ref()
                                .and_then(|history| history.error.as_ref())
                                .map(|error| localized(error.to_string()))
                                .unwrap_or_default()
                        })
                    }))
            },
        ))
        .child(text(move || {
            connection
                .context
                .with(|p| p.history_notice().map(localized).unwrap_or_default())
        }))
        .child(
            virtual_list(
                move || rows.with(Vec::len),
                move |at| {
                    let row = computed(move || rows.with(|rows| rows.get(at.index()).cloned()));
                    let user =
                        move || row.with(|r| r.as_ref().is_some_and(|r| r.kind == Kind::User));
                    let activity =
                        move || row.with(|r| r.as_ref().is_some_and(|r| r.kind == Kind::Activity));
                    let detail = computed(move || row.with(|r| r.as_ref().and_then(|r| r.detail.clone())));
                    let label = move || row.with(|row| row.as_ref().map(|row| {
                        row.progress.as_ref().map(|progress| localized(progress.label(clock.get().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs_f64())).into()).unwrap_or_else(|| row.body.clone())
                    }));
                    view()
                        .w_full()
                        .items_center()
                        .px(move || if is_desktop() { px(24.0) } else { px(16.0) })
                        .py_3()
                        .child(
                            view()
                                .group()
                                .w_full()
                                .max_w(px(CONTENT_WIDTH - 48.0))
                                .items_start()
                                .style_when(user, |s| s.items_end())
                                .child(when(
                                    move || row.with(|row| row.as_ref().is_some_and(|row| row.steer)),
                                    steer_marker,
                                ))
                                .child(when(
                                    move || {
                                        row.with(|row| {
                                            row.as_ref().is_some_and(|row| row.activity.as_ref().is_some_and(|activity| activity.activity_can_collapse))
                                        })
                                    },
                                    move || {
                                        let open = move || {
                                            row.with(|row| {
                                                row.as_ref().and_then(|row| row.activity.as_ref()).is_some_and(|activity| {
                                                    expanded.with(|all| activity_is_expanded(&activity.status, all.get(&activity.id).cloned()))
                                                })
                                            })
                                        };
                                        fold_row(
                                            move || label().unwrap_or_default().to_string(),
                                            open,
                                            move || {
                                                if let Some(activity) = row.with_untracked(|row| {
                                                    row.as_ref()
                                                        .and_then(|row| row.activity.clone())
                                                }) {
                                                    let opening = !expanded.with_untracked(|all| {
                                                        activity_is_expanded(
                                                            &activity.status,
                                                            all.get(&activity.id).cloned(),
                                                        )
                                                    });
                                                    following.set(false);
                                                    expanded.update(|all| {
                                                        if opening {
                                                            all.insert(
                                                                activity.id.clone(),
                                                                activity.status,
                                                            );
                                                        } else {
                                                            all.remove(&activity.id);
                                                        }
                                                    });
                                                    if opening
                                                        && let Some(load) = activity.load_items
                                                    {
                                                        connection
                                                            .dispatch(Intent::LoadTurnItems(load));
                                                    }
                                                    if opening {
                                                        reveal_row(list, at.index() as u32);
                                                    }
                                                }
                                            },
                                        )
                                    },
                                ))
                                .child(when(move || detail.get().is_some(), move || crate::activity::details(surroundings, detail, expanded_items, list, at, following)))
                                .child(
                                    view()
                                        .max_w(relative(1.0))
                                        .style_when(user, |s| {
                                            s.max_w(relative(0.9))
                                                .px(px(16.0))
                                                .py(px(14.0))
                                                .rounded(px(22.0))
                                                .bg(move || theme().muted)
                                        })
                                        .style_when(move || !user(), |s| s.w_full())
                                        .child(when(
                                            move || row.with(|row| row.as_ref().is_some_and(|row| {
                                                !row.activity.as_ref().is_some_and(|activity| activity.activity_can_collapse) && row.detail.is_none()
                                            })),
                                            move || crate::rich_text::content(
                                                surroundings,
                                                move || label().map(|text| agent_core::presentation::markdown::markdown_blocks(text.to_string())).unwrap_or_default(),
                                                activity,
                                            ),
                                        )),
                                )
                                .child(each(
                                    move || {
                                        row.with(|row| {
                                            row.as_ref()
                                                .map(|row| row.images.clone())
                                                .unwrap_or_default()
                                        })
                                    },
                                    |source: &String| source.clone(),
                                    move |source, _| crate::media::image_view(connection, surroundings.preview, move || source.get(), 320.0, 240.0),
                                ))
                                .child(when(
                                    move || {
                                        row.with(|row| row.as_ref().is_some_and(|row| row.actions))
                                    },
                                    move || response_actions(connection, row),
                                ))
                                .child(when(user, move || user_actions(connection, row))),
                        )
                        .child(when(
                            move || {
                                row.with(|row| row.as_ref().is_some_and(|row| row.retry.is_some()))
                            },
                            move || {
                                crate::ui::row_text_button(tr("再試行"), move || {
                                    following.set(true);
                                    if let Some(retry) = row.with_untracked(|row| {
                                        row.as_ref().and_then(|row| row.retry.clone())
                                    }) {
                                        connection.dispatch(Intent::RetryPreparation(retry));
                                    }
                                })
                            },
                        ))
                },
            )
            .keys(move || rows.with(|rows| rows.iter().map(|r| r.id.clone()).collect::<Vec<_>>()))
            .element_ref(&list)
            .estimated_item_size(px(170.0))
            .follow_end(px(FOLLOW_END_THRESHOLD))
            .on_scroll_drag_start(move |_: &Point<Points>| { dragging.replace(true); })
            .on_scroll(move |event: &ScrollEvent| {
                if dragging.get() {
                    following.set(event.offset.y + event.size.height >= event.content_size.height - px(FOLLOW_END_THRESHOLD));
                }
            })
            .on_scroll_end(move |_: &Point<Points>| { dragging.replace(false); })
            .on_momentum_end(move |_: &Point<Points>| { dragging.replace(false); })
            .on_viewable_items_change(move |items: &ViewableItems| {
                if let Some((first, _)) = items.viewable {
                    top_row.set(first);
                }
            })
            .flex_1()
            .min_h_0(),
        )
        .child(when(is_desktop, move || {
            minimap(rows, top_row, list, following, move || {
                agent_core::presentation::conversation::minimap_persistent(width.get(), CONTENT_WIDTH)
            })
        }))
        .child(when(move || !following.get(), move || {
            crate::ui::icon_button_sized(crate::ui::Icon::ARROW_DOWN, tr("最新のメッセージへ"), 32.0, 16.0, move || following.set(true))
                .bg(crate::ui::colors::RAISED)
                .border_1()
                .border_color(crate::ui::colors::BORDER)
                .rounded_full()
                .absolute().self_center().bottom(px(6.0))
        }))
}

/// T3 Code's timeline minimap: marks for user messages at the left edge that
/// widen toward the pointer, preview the exchange and jump to it.
fn minimap(
    rows: Memo<Vec<Row>>,
    top_row: Signal<usize>,
    list: ElementRef,
    following: Signal<bool>,
    persistent: impl Fn() -> bool + 'static,
) -> View {
    use crate::ui::colors;
    use agent_core::presentation::conversation::{minimap_marker_width, minimap_markers};
    let hovered = signal(None::<usize>);
    let over = signal(false);
    let user_rows = computed(move || {
        rows.with(|rows| {
            rows.iter()
                .enumerate()
                .filter(|(_, row)| row.kind == Kind::User)
                .map(|(index, _)| index)
                .collect::<Vec<_>>()
        })
    });
    let markers = computed(move || user_rows.with(|rows| minimap_markers(rows, top_row.get())));
    let exchange = move |row: usize| {
        rows.with(|rows| {
            let user = rows
                .get(row)
                .map(|r| r.body.to_string())
                .unwrap_or_default();
            let answer = rows
                .iter()
                .skip(row + 1)
                .take_while(|r| r.kind != Kind::User)
                .filter(|r| r.kind == Kind::Response)
                .last()
                .map(|r| r.body.to_string());
            (user, answer)
        })
    };
    view()
        .absolute()
        .left(px(0.0))
        .top(px(0.0))
        .bottom(px(0.0))
        .w(px(56.0))
        .justify_center()
        .on_mouse_enter(move |_: &MouseEvent| over.set(true))
        .on_mouse_leave(move |_: &MouseEvent| over.set(false))
        .opacity(move || {
            if persistent() || over.get() {
                1.0_f32
            } else {
                0.0_f32
            }
        })
        .child(each(
            move || markers.get(),
            |marker: &agent_core::presentation::conversation::MinimapMarker| marker.row,
            move |marker, position| {
                let distance = move || hovered.get().map(|h| h.abs_diff(position.get()));
                let lit = move || marker.with(|m| m.current) || distance() == Some(0);
                view()
                    .on_mouse_enter(move |_: &MouseEvent| {
                        hovered.set(Some(position.get_untracked()))
                    })
                    .on_mouse_leave(move |_: &MouseEvent| {
                        if hovered.get_untracked() == Some(position.get_untracked()) {
                            hovered.set(None);
                        }
                    })
                    .child(
                        nsh_base::button()
                            .accessibility_label(Some(SharedString::from(tr_format(
                                "この発言へ移動: {0}",
                                [&exchange(marker.with_untracked(|m| m.row)).0],
                            ))))
                            .w(px(56.0))
                            .h(px(10.0))
                            .pl(px(16.5))
                            .pr(px(11.5))
                            .justify_center()
                            .on_press(move || {
                                following.set(false);
                                let row = marker.with_untracked(|m| m.row) as u32;
                                list.scroll_to_index(
                                    row,
                                    nsh_core::geometry::Percentage(0.0),
                                    px(0.0),
                                    true,
                                );
                            })
                            .child(
                                view()
                                    .h(move || px(if distance() == Some(0) { 2.0 } else { 1.0 }))
                                    .w(move || px(minimap_marker_width(distance())))
                                    .bg(move || {
                                        if lit() {
                                            colors::FOREGROUND
                                        } else {
                                            colors::MUTED
                                        }
                                    }),
                            ),
                    )
                    .child(when(
                        move || distance() == Some(0),
                        move || {
                            let (user, answer) = exchange(marker.with_untracked(|m| m.row));
                            view()
                                .absolute()
                                .left(px(56.0))
                                .top(px(-24.0))
                                .w(px(300.0))
                                .p_3()
                                .gap_2()
                                .rounded(px(8.0))
                                .bg(colors::RAISED)
                                .border_1()
                                .border_color(colors::BORDER)
                                .child(text(user).text_size(px(13.0)).number_of_lines(2))
                                .child(
                                    text(answer.unwrap_or_default())
                                        .text_size(px(13.0))
                                        .text_color(colors::MUTED)
                                        .number_of_lines(3),
                                )
                        },
                    ))
            },
        ))
}

/// A turn's folded work, as T3 Code's: its label and disclosure, ruled below.
fn fold_row(
    label: impl Fn() -> String + 'static,
    open: impl Fn() -> bool + Copy + 'static,
    toggle: impl Fn() + 'static,
) -> View {
    use crate::ui::{self, Icon, colors};
    view()
        .w_full()
        .pt_1()
        .pb_2()
        .border_b_1()
        .border_b_color(colors::BORDER)
        .child(
            nsh_base::button()
                .self_start()
                .flex_row()
                .items_center()
                .gap_1()
                .px_1()
                .rounded(px(ui::RADIUS))
                .text_color(colors::MUTED)
                .hover(|s| s.text_color(colors::FOREGROUND))
                .on_press(toggle)
                .child(text(label).text_size(px(14.0)))
                .child(when(open, || ui::icon(Icon::CHEVRON_DOWN, 14.0)))
                .child(when(
                    move || !open(),
                    || ui::icon(Icon::CHEVRON_RIGHT, 14.0),
                )),
        )
}

/// Marks a message that steered the working turn (T3 Code's "Steer").
fn steer_marker() -> View {
    use crate::ui::{self, Icon, colors};
    view()
        .flex_row()
        .items_center()
        .gap_1()
        .mr_1()
        .mb_1()
        .text_color(colors::MUTED)
        .label(tr("実行中の作業に追加で指示しました"))
        .child(ui::icon(Icon::STEER, 12.0))
        .child(text(tr("追加の指示")).text_size(px(12.0)))
}

/// A response's actions, as the desktop client's: copy, fork from here, its time.
fn response_actions(connection: Connection, row: Memo<Option<Row>>) -> View {
    use crate::ui::{self, Enabled, Icon, colors};
    let fork = move || row.with(|row| row.as_ref().and_then(|row| row.fork.clone()));
    let thread = move || {
        connection
            .context
            .with(|context| context.navigation().thread_id)
    };
    let forking = move || match (thread(), fork()) {
        (Some(session), Some(turn)) => connection.publication.with(|p| {
            p.snapshot
                .operation_running(op::OperationKey::Fork { session, turn })
        }),
        _ => false,
    };
    view()
        .flex_row()
        .items_center()
        .gap_4()
        .mt_2()
        .child(ui::row_icon_button(
            Icon::COPY,
            tr("回答をコピー"),
            move || {
                if let Some(row) = row.get_untracked() {
                    set_clipboard(row.body.to_string());
                }
            },
        ))
        .child(when(
            move || fork().is_some(),
            move || {
                ui::row_icon_button(Icon::FORK, tr("ここから会話を分岐"), move || {
                    if let (Some(thread_id), Some(last_turn_id)) =
                        (untracked(thread), untracked(fork))
                    {
                        connection.dispatch(Intent::ForkSession(op::ForkSession {
                            thread_id,
                            last_turn_id,
                        }));
                    }
                })
                // The branch turned to point forward: a new chat continues from here.
                .rotate(deg(90.0))
                .enabled(move || !forking())
            },
        ))
        .child(when(forking, || {
            text(tr("分岐を作成中…"))
                .text_size(px(12.0))
                .text_color(colors::MUTED)
        }))
        .child(time(row))
}

/// The user's own message's actions, shown while the pointer is over it: its time,
/// copy, and editing it in the composer.
fn user_actions(connection: Connection, row: Memo<Option<Row>>) -> View {
    use crate::ui::{self, Icon};
    let body = move || row.with_untracked(|row| row.as_ref().map(|row| row.body.to_string()));
    view()
        .flex_row()
        .items_center()
        .gap_1()
        .opacity(0.0)
        .group_hover(|s| s.opacity(1.0))
        .child(time(row).mr_2())
        .child(ui::row_icon_button(
            Icon::COPY,
            tr("発言をコピー"),
            move || {
                if let Some(text) = body() {
                    set_clipboard(text);
                }
            },
        ))
        .child(ui::row_icon_button(
            Icon::PENCIL,
            tr("入力欄で編集"),
            move || {
                let Some(text) = body() else { return };
                let draft_key = connection
                    .composer
                    .with_untracked(|c| c.controls.navigation.draft_key.clone());
                connection.dispatch(Intent::EditComposer {
                    thread_id: draft_key,
                    cursor: text.len() as u32,
                    text,
                });
                connection.composer_ref.focus();
            },
        ))
}

fn time(row: Memo<Option<Row>>) -> Text {
    text(move || {
        row.with(|row| {
            row.as_ref()
                .and_then(|row| row.time.clone())
                .unwrap_or_default()
        })
    })
    .text_size(px(12.0))
    .text_color(crate::ui::colors::MUTED)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expanding_running_work_reveals_output_without_a_second_progress_row() {
        let source: agent_protocol::models::Thread = serde_json::from_value(serde_json::json!({
            "id": {"provider":"codex", "id":"thread"},
            "turns": [{"id":"turn", "status":"running", "items":[
                {"id":"command", "status":"completed", "clientInputId":null,
                 "body":{"inline":{"body":{"commandExecution":{
                     "command":"pwd", "cwd":null, "output":"/project", "exitCode":0
                 }}}}},
                {"id":"answer", "status":"unknown", "clientInputId":null,
                 "body":{"inline":{"body":{"assistantText":{"text":"answer", "phase":"unknown"}}}}}
            ]}]
        }))
        .unwrap();
        let key = source.id.as_ref().unwrap().into();
        let conversation = agent_core::presentation::conversation::project_conversation(
            &key,
            &Default::default(),
            std::sync::Arc::new(source),
            &None,
        );
        let collapsed = project(&conversation, &HashMap::new());
        assert!(!collapsed.iter().any(|row| row.detail.is_some()));
        let activity = collapsed
            .iter()
            .find_map(|row| row.activity.as_ref())
            .unwrap();
        let expanded = project(
            &conversation,
            &[(activity.id.clone(), activity.status.clone())].into(),
        );
        assert!(expanded.iter().any(|row| row.detail.is_some()));
        assert!(expanded.iter().any(|row| row.body.as_ref() == "answer"));
        assert_eq!(
            expanded
                .iter()
                .filter(|row| row.kind == Kind::Activity)
                .count(),
            2
        );
    }
}
