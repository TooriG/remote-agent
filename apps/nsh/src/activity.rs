use crate::l10n::{localized, tr};
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
    view()
        .w_full()
        .gap_1()
        // As T3 Code's work log: the tool's icon and a one-line summary.
        .child(
            nsh_base::button()
                .accessibility_label(Some(SharedString::from(item.with_untracked(|item| {
                    item.as_ref()
                        .and_then(|item| item.source.data.title.clone())
                        .map(localized)
                        .unwrap_or_default()
                }))))
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
                .child(
                    text(move || {
                        item.with(|item| {
                            item.as_ref()
                                .and_then(|item| item.source.data.title.clone())
                                .map(localized)
                                .unwrap_or_default()
                        })
                    })
                    .flex_1()
                    .min_w_0()
                    .number_of_lines(1)
                    .text_size(px(14.0)),
                ),
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
                                error()
                                    .map(|error| localized(error.to_string()))
                                    .unwrap_or_default()
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
                    || false,
                ))
        }))
}
