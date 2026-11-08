use crate::Connection;
use crate::ui::{self, Icon, colors};
use agent_core::{
    composer::{candidate_label, insert_invocation},
    state::Intent,
};
use agent_protocol::composer::{ComposerCandidate, InvocationKind};
use nsh_core::prelude::*;

/// Catalog filtering and insertion remain core decisions; the list is T3 Code's
/// command menu above the composer.
pub(crate) fn suggestions(focus: impl Fn() + Copy + 'static) -> impl IntoElement {
    let connection = use_context::<Connection>().unwrap();
    let suggestions = computed(move || {
        connection.composer.with(|p| {
            let text = p.draft.text.clone();
            let cursor = text.len() as u32;
            p.composer_suggestions(text, cursor)
        })
    });
    let insert = move |invocation: agent_protocol::composer::Invocation| {
        let (thread_id, text) = connection.publication.with_untracked(|p| {
            (
                p.composer.controls.navigation.draft_key.clone(),
                p.composer.draft.text.clone(),
            )
        });
        if let Some(insertion) = insert_invocation(
            text.clone(),
            text.len() as u32,
            invocation.kind,
            invocation.name.clone(),
        ) {
            connection.dispatch(Intent::InsertInvocation {
                thread_id,
                text: insertion.text,
                invocation,
            });
            focus();
        }
    };
    when(
        move || suggestions.with(Option::is_some),
        move || {
            view()
                .gap_1()
                .child(
                    text(move || {
                        suggestions.with(|s| {
                            s.as_ref()
                                .and_then(|s| s.status.clone())
                                .unwrap_or_default()
                        })
                    })
                    .text_size(px(12.0))
                    .text_color(colors::MUTED),
                )
                .child(when(
                    move || {
                        suggestions.with(|s| s.as_ref().is_some_and(|s| !s.candidates.is_empty()))
                    },
                    move || {
                        ui::card(vec![
                            view().child(scroll().max_h(px(4.0 * 40.0 + 8.0)).child(
                                view().p_1().child(each(
                                    move || {
                                        suggestions.with(|s| {
                                            s.as_ref()
                                                .map(|s| s.candidates.clone())
                                                .unwrap_or_default()
                                        })
                                    },
                                    |candidate: &ComposerCandidate| {
                                        candidate.invocation.path.clone()
                                    },
                                    move |candidate, _| row(candidate.get_untracked(), insert),
                                )),
                            )),
                        ])
                    },
                ))
        },
    )
}

/// A skill or plugin: its icon, name and description.
fn row(
    candidate: ComposerCandidate,
    insert: impl Fn(agent_protocol::composer::Invocation) + 'static,
) -> impl IntoElement {
    let ComposerCandidate {
        invocation,
        description,
    } = candidate;
    let name = candidate_label(&invocation.name);
    let glyph = if invocation.kind == InvocationKind::Skill {
        Icon::SPARKLES
    } else {
        Icon::PUZZLE
    };
    nsh_base::button()
        .accessibility_label(Some(SharedString::from(name.clone())))
        .flex_row()
        .items_center()
        .gap_2()
        .h(px(40.0))
        .px_2()
        .rounded(px(ui::RADIUS))
        .text_color(colors::FOREGROUND)
        .hover(|s| s.bg(colors::GHOST_HOVER))
        .on_press(move || insert(invocation.clone()))
        .child(
            view()
                .text_color(colors::MUTED)
                .child(ui::icon(glyph, 16.0)),
        )
        .child(
            text(name)
                .text_size(px(14.0))
                .number_of_lines(1)
                .flex_shrink_0(),
        )
        .child(
            text(candidate_label(&description))
                .text_size(px(13.0))
                .text_color(colors::MUTED)
                .number_of_lines(1)
                .flex_1()
                .min_w_0(),
        )
}
