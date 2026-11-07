use crate::Connection;
use agent_core::{
    composer::{candidate_label, insert_invocation},
    state::Intent,
};
use nsh_core::prelude::*;
use serde_json::json;

/// Catalog filtering and insertion remain core decisions; the OS owns the list.
pub(crate) fn suggestions(focus: impl Fn() + Copy + 'static) -> impl IntoElement {
    let connection = use_context::<Connection>().unwrap();
    let suggestions = computed(move || {
        connection.composer.with(|p| {
            let text = p.draft.text.clone();
            let cursor = text.len() as u32;
            p.composer_suggestions(text, cursor)
        })
    });
    when(
        move || suggestions.with(Option::is_some),
        move || {
            view().gap_1()
        .child(text(move || suggestions.with(|s| s.as_ref().and_then(|s| s.status.clone()).unwrap_or_default()))
            .text_size(px(12.0)).text_color(move || nsh_base::theme().muted_foreground))
        .child(when(move || suggestions.with(|s| s.as_ref().is_some_and(|s| !s.candidates.is_empty())), move || {
            native("bex.list")
                .h(move || px(suggestions.with(|s| s.as_ref().map_or(0, |s| s.candidates.len()).min(4)) as f32 * 44.0))
                .prop("items", move || json!(suggestions.with(|s| s.as_ref().into_iter().flat_map(|s| &s.candidates).map(|candidate| json!({
                    "id": candidate.invocation.path,
                    "title": candidate_label(&format!("{} — {}", candidate.invocation.name, candidate.description)),
                    "symbol": if candidate.invocation.kind == agent_protocol::composer::InvocationKind::Skill { "sparkles" } else { "puzzlepiece.extension" },
                    "asset": null, "level": 0, "disclosure": false, "group": false, "expanded": false,
                })).collect::<Vec<_>>())))
                .on_native_event(move |event: &NativeEvent| {
                    if &*event.name != "select" { return; }
                    let candidate = suggestions.with_untracked(|s| s.as_ref()?.candidates.iter()
                        .find(|candidate| Some(candidate.invocation.path.as_str()) == event.data.as_str())
                        .map(|candidate| candidate.invocation.clone()));
                    let Some(invocation) = candidate else { return; };
                    let (thread_id, text) = connection.publication.with_untracked(|p| (
                        p.composer.controls.navigation.draft_key.clone(), p.composer.draft.text.clone(),
                    ));
                    if let Some(insertion) = insert_invocation(text.clone(), text.len() as u32, invocation.kind, invocation.name.clone()) {
                        connection.dispatch(Intent::InsertInvocation { thread_id, text: insertion.text, invocation });
                        focus();
                    }
                })
        }))
        },
    )
}
