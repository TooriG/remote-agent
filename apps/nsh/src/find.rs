//! T3 Code's find in thread (⌘F): a bar over the conversation's corner that
//! counts what the loaded messages say and walks the list through it.
use crate::{
    composer::shortcut,
    conversation::Timeline,
    l10n::tr,
    ui::{self, Icon, colors},
};
use agent_core::presentation::conversation::find_occurrences;
use nsh_core::{keymap::When, prelude::*};

#[derive(Clone, Copy)]
pub(crate) struct Find {
    open: Signal<bool>,
    query: Signal<String>,
    current: Signal<usize>,
}

impl Find {
    pub(crate) fn new() -> Self {
        Self {
            open: signal(false),
            query: signal(String::new()),
            current: signal(0),
        }
    }

    /// What the messages mark, empty while the bar is closed.
    pub(crate) fn highlighted(self) -> String {
        if self.open.get() {
            self.query.get()
        } else {
            String::new()
        }
    }

    fn close(self) {
        self.open.set(false);
        self.query.set(String::new());
        self.current.set(0);
    }
}

/// The bar, and ⌘F, while the terminal does not hold the keyboard.
pub(crate) fn bar(find: Find, timeline: Timeline) -> impl IntoElement {
    let terminal = use_context::<crate::terminal::Focus>().unwrap();
    view()
        .absolute()
        .top(px(12.0))
        .right(px(16.0))
        .child(when(
            move || !terminal.get(),
            move || {
                shortcut("mod+f", When::Always, move |_| {
                    find.open.set(true);
                })
            },
        ))
        .child(when(
            move || find.open.get(),
            move || open_bar(find, timeline),
        ))
}

fn open_bar(find: Find, timeline: Timeline) -> View {
    let field = element_ref();
    let focused = signal(false);
    let occurrences = computed(move || {
        let query = find.query.get();
        timeline.message_texts(|rows| {
            find_occurrences(rows.iter().map(|(_, text)| text.as_str()), &query)
                .into_iter()
                .map(|row| rows[row].0)
                .collect::<Vec<_>>()
        })
    });
    let count = move || occurrences.with(Vec::len);
    let step = move |by: isize| {
        let total = count() as isize;
        if total > 0 {
            let at = find.current.get_untracked() as isize;
            find.current.set((at + by).rem_euclid(total) as usize);
        }
    };
    effect(move || {
        let row = occurrences.with(|rows| rows.get(find.current.get()).copied());
        if let Some(row) = row {
            timeline.reveal(row);
        }
    });
    after_answer(move || field.focus());
    view()
        .w(px(280.0))
        .h(px(36.0))
        .flex_row()
        .items_center()
        .gap_1()
        .px_3()
        .rounded_full()
        .border_1()
        .border_color(colors::border())
        .bg(colors::SURFACE)
        .child(shortcut("escape", When::Always, move |_| find.close()))
        .child(when(
            move || focused.get(),
            move || shortcut("shift+enter", When::Always, move |_| step(-1)),
        ))
        .child(
            ui::field()
                .element_ref(&field)
                .flex_1()
                .min_w_0()
                .text_size(px(14.0))
                .value(move || find.query.get())
                .placeholder(tr("会話内を検索"))
                .submit_behavior(SubmitBehavior::Submit)
                .on_focus(move || focused.set(true))
                .on_blur(move || focused.set(false))
                .on_submit(move |_: &str| step(1))
                .on_change(move |text: &str| {
                    find.query.set(text.to_owned());
                    find.current.set(0);
                }),
        )
        .child(
            text(move || match count() {
                0 => String::new(),
                total => format!("{}/{total}", find.current.get() + 1),
            })
            .text_size(px(12.0))
            .text_color(colors::muted()),
        )
        .child(ui::icon_button(
            Icon::CHEVRON_UP,
            tr("前の一致"),
            move || step(-1),
        ))
        .child(ui::icon_button(
            Icon::CHEVRON_DOWN,
            tr("次の一致"),
            move || step(1),
        ))
        .child(ui::icon_button(Icon::CLOSE, tr("閉じる"), move || {
            find.close()
        }))
}
