use crate::l10n::tr;
use crate::shared::Shared;
use crate::ui::Enabled;
use agent_core::{
    presentation::conversation::RenderedItem,
    state::{Intent, operations as op},
};
use agent_protocol::ids::TurnId;
use nsh_core::prelude::*;
use std::collections::HashSet;

#[derive(Clone, PartialEq)]
pub(crate) struct Item {
    pub source: Shared<RenderedItem>,
    pub turn: TurnId,
    /// Its turn's live row (`RenderedTurn::live_activity`), which shimmers.
    pub live: bool,
}

pub(crate) fn details(
    surroundings: crate::rich_text::Surroundings,
    item: Memo<Option<Item>>,
    expanded: Signal<HashSet<String>>,
    list: ElementRef,
    at: Position,
    following: Signal<bool>,
) -> impl Element {
    let connection = surroundings.connection;
    let open = move || {
        item.with(|item| {
            item.as_ref()
                .is_some_and(|item| expanded.with(|all| all.contains(&item.source.data.id)))
        })
    };
    let request = computed(move || {
        let item = item.get()?;
        Some(op::ReadItem {
            thread_id: connection.context.with(|p| p.navigation().thread_id)?,
            turn_id: item.turn,
            item_id: item.source.data.native_id.clone()?,
        })
    });
    let running = move || {
        request.with(|read| {
            read.as_ref().is_some_and(|read| {
                connection.publication.with(|p| {
                    p.snapshot
                        .operation_running(op::OperationKey::Item { item: read.clone() })
                })
            })
        })
    };
    let error = move || {
        request.with(|read| {
            read.as_ref().and_then(|read| {
                connection.publication.with(|p| {
                    p.snapshot
                        .operation_error(op::OperationKey::Item { item: read.clone() })
                })
            })
        })
    };
    let load = move || {
        if let Some(read) = request.get_untracked() {
            connection.dispatch(Intent::ReadItem(read));
        }
    };
    let toggle = move || {
        let Some(item) = item.get_untracked() else {
            return;
        };
        let opening = !open();
        following.set(false);
        expanded.update(|all| {
            if opening {
                all.insert(item.source.data.id.clone());
            } else {
                all.remove(&item.source.data.id);
            }
        });
        if opening && item.source.data.deferred {
            load();
        }
        if opening {
            crate::conversation::reveal_row(list, at.index() as u32);
        }
    };
    let label = computed(move || {
        item.with(|item| {
            item.as_ref()
                .map(|item| item.source.work_row_label(item.live))
        })
    });
    let heading = move || {
        label.with(|label| {
            label
                .as_ref()
                .map(|label| label.heading.clone())
                .unwrap_or_default()
        })
    };
    // As T3 Code's work log: reasoning reads as its trace on one line, and
    // expanded as Thinking above the whole of it.
    let preview = move || {
        label
            .with(|label| label.as_ref().and_then(|l| l.preview.clone()))
            .filter(|_| !open())
    };
    let summary = move || preview().unwrap_or_else(heading);
    let accessible = move || {
        SharedString::from(match preview() {
            Some(preview) => format!("{}: {preview}", heading()),
            None => heading(),
        })
    };
    let live = move || item.with(|item| item.as_ref().is_some_and(|item| item.live));
    view()
        .w_full()
        .gap_1()
        // As T3 Code's work log: the tool's icon and a one-line summary.
        .child(
            nsh_base::button()
                .accessibility_label(nsh_core::element::Prop::from(
                    accessible.into_reactive().map(Some),
                ))
                .flex_row()
                .items_center()
                .gap_3()
                .h(px(24.0))
                .text_color(crate::ui::colors::MUTED)
                .hover(|s| s.text_color(crate::ui::colors::FOREGROUND))
                .on_press(toggle)
                .child(crate::ui::icon(
                    item.with_untracked(|item| {
                        crate::ui::Icon::work(
                            item.as_ref()
                                .map_or("", |item| item.source.data.kind.as_str()),
                        )
                    }),
                    16.0,
                ))
                .child(when(live, move || {
                    // As T3's `live-tool-shine`, over the label alone.
                    crate::shimmer::line(None, summary, crate::shimmer::Sweep::Shine, 14.0)
                }))
                .child(when(
                    move || !live(),
                    move || {
                        text(summary)
                            .flex_1()
                            .min_w_0()
                            .number_of_lines(1)
                            .text_size(px(14.0))
                    },
                )),
        )
        .child(when(open, move || {
            view()
                .w_full()
                .pl(px(28.0))
                .gap_2()
                .child(when(
                    move || {
                        item.with(|item| {
                            item.as_ref().is_some_and(|item| item.source.data.deferred)
                        })
                    },
                    move || {
                        view()
                            .gap_2()
                            .child(text(move || {
                                error().map(|error| error.to_string()).unwrap_or_default()
                            }))
                            .child(
                                crate::ui::row_text_button(
                                    move || {
                                        SharedString::from(if running() {
                                            tr("詳細を読み込み中…")
                                        } else {
                                            tr("詳細を読み込む")
                                        })
                                    },
                                    load,
                                )
                                .enabled(move || !running()),
                            )
                    },
                ))
                .child(crate::rich_text::content(
                    surroundings,
                    move || {
                        item.with(|item| {
                            item.as_ref()
                                .filter(|item| !item.source.data.deferred)
                                .map(|item| item.source.detail_blocks())
                                .unwrap_or_default()
                        })
                    },
                    || crate::conversation::Kind::Response,
                ))
        }))
}

/// A live status line that is not a work row: T3 Code's Thinking row, with
/// its brain icon, and setting up a worktree, which its body names. Both
/// sweep as T3 Code's `ActivityShimmerOverlay`.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Status {
    /// The turn works with nothing else live.
    Thinking,
    SettingUp,
}

/// A row keeps its status: it is made for the one it shows.
pub(crate) fn status(status: Memo<Option<Status>>, body: impl Fn() -> String + 'static) -> View {
    let thinking = status.get_untracked() == Some(Status::Thinking);
    view()
        .w_full()
        .h(px(24.0))
        .flex_row()
        .items_center()
        .child(crate::shimmer::line(
            thinking.then(|| crate::ui::Icon::work("reasoning")),
            move || {
                if thinking {
                    tr("思考中").to_owned()
                } else {
                    body()
                }
            },
            crate::shimmer::Sweep::Focus,
            14.0,
        ))
}
