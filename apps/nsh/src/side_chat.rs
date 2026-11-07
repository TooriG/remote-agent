use crate::{Connection, native, shared::Shared};
use agent_core::presentation::view::Presentation;
use nsh_core::prelude::*;
use serde_json::json;

pub(crate) fn screen(id: u64, host: String) -> impl IntoElement {
    let parent = use_context::<Connection>().unwrap();
    let host = stored(host);
    let empty = stored(Shared(Presentation::empty()));
    let publication = computed(move || {
        parent.state.with(|state| {
            state
                .side_chats
                .get(&id)
                .and_then(|pane| pane.publication.clone())
                .unwrap_or_else(|| empty.get())
        })
    });
    let pending = computed(move || {
        parent.state.with(|state| {
            state.side_chats.get(&id).is_none_or(|pane| {
                pane.pending || pane.publication.is_none() && pane.failure.is_none()
            })
        })
    });
    let failure = computed(move || {
        parent.state.with(|state| {
            state
                .side_chats
                .get(&id)
                .and_then(|pane| pane.failure.clone())
        })
    });
    provide_context(Connection {
        pane: Some(id),
        host: computed(move || host.get()),
        publication,
        ..parent
    });
    view()
        .flex_1()
        .min_w_0()
        .child(when(
            move || pending.get(),
            move || {
                native("bex.details")
                    .prop("data", json!({"sideChatLoading": {}}))
                    .flex_1()
            },
        ))
        .child(when(
            move || failure.get().is_some(),
            move || {
                view()
                    .p_4()
                    .gap_3()
                    .child(
                        text(move || {
                            failure
                                .get()
                                .map(|error| error.to_string())
                                .unwrap_or_default()
                        })
                        .selectable(true)
                        .text_color(move || nsh_base::theme().destructive),
                    )
                    .child(
                        native::text_button(json!("再試行"), move || {
                            parent.application.with(|app| app.prepare_side_chat(id))
                        })
                        .prop("identifier", json!("sideChat.retry")),
                    )
            },
        ))
        .child(when(
            move || !pending.get() && failure.get().is_none(),
            move || {
                let following = signal(true);
                view()
                    .flex_1()
                    .min_w_0()
                    .child(crate::notices::notices(|| false, || false))
                    .child(crate::conversation::list(publication, following))
                    .child(crate::composer::composer(publication, following))
            },
        ))
}
