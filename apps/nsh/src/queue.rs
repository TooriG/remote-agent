//! The messages waiting for the running turn, as T3 Code's queue above its
//! composer: each can be edited in the composer, sent as a steer now, or
//! removed, and the queue folds to its count. The Host owns the queue, so every
//! client shows the same one.
use crate::{
    Connection,
    l10n::{tr, tr_format},
    ui::{self, Icon, colors},
};
use agent_core::{
    presentation::composer::QueuedMessage,
    state::{Intent, follow_up::QueueAction},
};
use nsh_core::prelude::*;

pub(crate) fn queued(connection: Connection) -> View {
    let follow_up = computed(move || connection.composer.with(|c| c.controls.follow_up.clone()));
    let expanded = signal(true);
    let count = move || follow_up.with(|f| f.queue.len());
    let act = move |action| {
        if let Some(thread_id) = connection
            .composer
            .with_untracked(|c| c.controls.navigation.thread_id.clone())
        {
            connection.dispatch(Intent::Queue { thread_id, action });
        }
    };
    let header = view()
        .flex_row()
        .items_center()
        .gap_2()
        .px_2()
        .h(px(28.0))
        .rounded(px(ui::RADIUS))
        .role(AccessibilityRole::Button)
        .label(move || {
            if expanded.get() {
                tr("キューのメッセージを折りたたむ")
            } else {
                tr("キューのメッセージを展開")
            }
        })
        .on_press(move || expanded.update(|value| *value = !*value))
        .child(ui::icon(Icon::QUEUED, 14.0))
        .child(
            text(tr("キューのメッセージ"))
                .text_size(px(13.0))
                .text_color(colors::MUTED),
        )
        .child(view().flex_1())
        .child(
            text(move || count().to_string())
                .text_size(px(12.0))
                .text_color(colors::MUTED),
        )
        .child(dynamic(move || {
            ui::icon(
                if expanded.get() {
                    Icon::CHEVRON_UP
                } else {
                    Icon::CHEVRON_DOWN
                },
                14.0,
            )
        }));
    view()
        .w_full()
        .p_1()
        .gap_1()
        .rounded(px(12.0))
        .border_1()
        .border_color(colors::BORDER)
        .bg(colors::SURFACE)
        .label(move || tr_format("キューのメッセージ {0}件", [&count()]))
        .child(header)
        .child(when(expanded, move || {
            scroll().max_h(px(128.0)).child(view().gap_1().child(each(
                move || follow_up.with(|f| f.queue.clone()),
                |message: &QueuedMessage| message.id.to_string(),
                move |message, _| row(follow_up, message, act),
            )))
        }))
}

/// A queued message's height, which a drag moves it by per row.
const ROW_HEIGHT: f32 = 28.0;

/// T3 Code's grip: dragging it moves its message by whole rows, and the arrow
/// keys move it by one while it has the keyboard focus.
fn grip(dragging: Signal<bool>, reorder: impl Fn(i32) + Copy + 'static) -> View {
    let focused = signal(false);
    view()
        .w(px(24.0))
        .h(px(24.0))
        .items_center()
        .justify_center()
        .rounded(px(ui::RADIUS))
        .text_color(colors::MUTED)
        .hover(|s| s.bg(colors::GHOST_HOVER))
        .role(AccessibilityRole::Button)
        .label(tr(
            "キューのメッセージを並べ替え（ドラッグ、または矢印キー）",
        ))
        .on_press(|| {})
        .on_focus(move || focused.set(true))
        .on_blur(move || focused.set(false))
        .on_pan_start(move |_: &PanEvent| dragging.set(true))
        .on_pan_end(move |event: &PanEvent| {
            dragging.set(false);
            if !event.cancelled {
                reorder((event.translation.y.0 / ROW_HEIGHT).round() as i32);
            }
        })
        .child(ui::icon(Icon::GRIP, 14.0))
        .child(when(focused, move || {
            view()
                .child(crate::composer::shortcut(
                    "up",
                    nsh_core::keymap::When::NotEditing,
                    move |_| reorder(-1),
                ))
                .child(crate::composer::shortcut(
                    "down",
                    nsh_core::keymap::When::NotEditing,
                    move |_| reorder(1),
                ))
        }))
}

/// One waiting message: its text and what can be done with it.
fn row(
    follow_up: Memo<agent_core::presentation::composer::FollowUpControls>,
    message: ReadSignal<QueuedMessage>,
    act: impl Fn(QueueAction) + Copy + 'static,
) -> View {
    let id = move || message.with(|message| message.id.clone());
    let editing = move || follow_up.with(|f| f.editing.as_ref() == Some(&id()));
    let idle = move || !message.with(|message| message.pending) && !follow_up.with(|f| f.busy);
    let preview = move || {
        message.with(|message| {
            if message.text.trim().is_empty() {
                tr_format("添付 {0}件", [&message.attachments])
            } else {
                message.text.lines().collect::<Vec<_>>().join(" ")
            }
        })
    };
    // The row being dragged dims, as in T3 Code; it moves when released.
    let dragging = signal(false);
    let reorder = move |steps: i32| {
        let ids: Vec<_> = follow_up.with_untracked(|f| {
            f.queue
                .iter()
                .filter(|row| !row.pending)
                .map(|row| row.id.clone())
                .collect()
        });
        if let Some(action) = agent_core::state::follow_up::reorder(&ids, &id(), steps) {
            act(action);
        }
    };
    view()
        .flex_row()
        .items_center()
        .gap_1()
        .h(px(ROW_HEIGHT))
        .rounded(px(ui::RADIUS))
        .style_when(move || dragging.get(), |s| s.opacity(0.5))
        .bg(move || {
            if editing() {
                colors::SELECTED
            } else {
                rgba(0x00000000)
            }
        })
        .child(when(idle, move || grip(dragging, reorder)))
        .child(when(
            move || message.with(|message| message.pending),
            || {
                view()
                    .label(tr("キューへ保存中"))
                    .text_color(colors::MUTED)
                    .child(ui::icon(Icon::SAVING, 12.0))
            },
        ))
        .child(
            text(preview)
                .text_size(px(13.0))
                .number_of_lines(1)
                .flex_1()
                .min_w_0(),
        )
        .child(when(editing, move || {
            ui::row_text_button(tr("キャンセル"), move || act(QueueAction::CancelEdit))
        }))
        .child(when(
            move || !editing(),
            move || {
                view()
                    .flex_row()
                    .items_center()
                    .child(
                        ui::row_icon_button(
                            Icon::PENCIL,
                            tr("コンポーザーで編集"),
                            move || {
                                act(QueueAction::Edit {
                                    client_user_message_id: id(),
                                })
                            },
                        )
                        .disabled(move || !idle()),
                    )
                    .child(
                        ui::row_labeled_button(Icon::STEER_NOW, tr("追加の指示"), move || {
                            act(QueueAction::Steer {
                                client_user_message_id: id(),
                            })
                        })
                        .disabled(move || !idle() || !follow_up.with(|f| f.can_steer)),
                    )
                    .child(
                        ui::row_icon_button(Icon::CLOSE, tr("キューから削除"), move || {
                            act(QueueAction::Remove {
                                client_user_message_id: id(),
                            })
                        })
                        .disabled(move || !idle()),
                    )
            },
        ))
}
