use crate::{Connection, native, shared::Shared};
use agent_core::{
    presentation::conversation::RenderedItem,
    state::{Intent, operations as op},
};
use agent_protocol::ids::TurnId;
use nsh_core::prelude::*;
use serde_json::json;
use std::collections::HashSet;

#[derive(Clone, PartialEq)]
pub(crate) struct Item {
    pub source: Shared<RenderedItem>,
    pub turn: TurnId,
}

pub(crate) fn details(
    item: Memo<Option<Item>>,
    expanded: Signal<HashSet<String>>,
    list: ElementRef,
    at: Position,
    following: Signal<bool>,
) -> impl Element {
    let connection = use_context::<Connection>().unwrap();
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
    view()
        .w_full()
        .gap_2()
        .child(
            native::text_button(
                move || {
                    json!(item.with(|item| {
                        item.as_ref()
                            .and_then(|item| item.source.data.title.clone())
                            .unwrap_or_default()
                    }))
                },
                move || {
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
                },
            )
            .prop("selected", move || json!(open()))
            .self_start(),
        )
        .child(when(open, move || {
            view()
                .w_full()
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
                                native::text_button(
                                    move || {
                                        json!(if running() {
                                            "詳細を読み込み中…"
                                        } else {
                                            "詳細を読み込む"
                                        })
                                    },
                                    load,
                                )
                                .prop("enabled", move || json!(!running())),
                            )
                    },
                ))
                .child(crate::rich_text::content(
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
