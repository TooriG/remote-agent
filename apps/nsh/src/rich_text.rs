//! Core supplies Markdown semantics; TextKit owns native selection, layout and menus.
use crate::Connection;
use crate::conversation::Kind;
use crate::l10n::tr;
#[cfg(any(desktop, target_os = "ios"))]
use agent_core::{panels::PanelScope, state::Intent, store::SideChatStart};
use agent_core::{
    presentation::markdown::{
        MarkdownAlignment, MarkdownBlock, MarkdownFileReference, MarkdownRun, MarkdownStyle,
    },
    state::DraftKey,
};
use nsh_core::{element::ImageSrc, prelude::*};
use serde_json::{Value, json};

#[derive(Clone, PartialEq)]
enum Part {
    Text(Vec<Value>),
    Image(String),
    Table {
        source: String,
        csv: String,
        rows: Vec<Vec<Vec<Part>>>,
    },
    Link(String),
    Rule,
    Code {
        label: String,
        source: String,
        blocks: Vec<Value>,
        /// A complete Mermaid block's drawing, shown in place of its source.
        diagram: Option<Diagram>,
    },
}

/// A Mermaid block as Core draws it.
#[derive(Clone, PartialEq)]
struct Diagram {
    svg: String,
    width: f32,
    height: f32,
}

/// What a message's file runs show: the chip each names, if it shows one.
type Chip<'a> = &'a dyn Fn(&MarkdownFileReference) -> Option<Value>;

thread_local! {
    /// Drawn Mermaid blocks by their fenced source: a streamed message's
    /// earlier diagrams are not laid out again for every delta.
    static DIAGRAMS: std::cell::RefCell<Diagrams> = Default::default();
}

/// Drawn Mermaid blocks by their code.
type Diagrams = std::collections::HashMap<String, Option<(String, Diagram)>>;

/// The drawings of `source`'s complete Mermaid blocks, by their code.
fn diagrams(source: &str) -> Vec<(String, Diagram)> {
    use agent_core::presentation::markdown::{markdown_diagram, markdown_mermaid_fences};
    let fences = markdown_mermaid_fences(source);
    if fences.is_empty() {
        return Vec::new();
    }
    DIAGRAMS.with_borrow_mut(|cache| {
        if cache.len() > 64 {
            cache.clear();
        }
        fences
            .into_iter()
            .filter_map(|fence| {
                cache
                    .entry(fence)
                    .or_insert_with_key(|fence| {
                        markdown_diagram(fence).map(|diagram| {
                            let drawing = Diagram {
                                svg: diagram.svg,
                                width: diagram.width,
                                height: diagram.height,
                            };
                            (diagram.source, drawing)
                        })
                    })
                    .clone()
            })
            .collect()
    })
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Target {
    host: String,
    pane: Option<u64>,
    draft: DraftKey,
    storage: String,
    cwd: String,
    home: Option<String>,
    provider: Option<agent_protocol::session::ProviderKind>,
}

/// What message content reaches. List rows are made outside the context
/// their list was made in, so they receive these rather than look them up.
#[derive(Clone, Copy)]
pub(crate) struct Surroundings {
    pub connection: Connection,
    pub files: crate::files::Browser,
    pub preview: crate::media::Preview,
    pub workspace: crate::workbench::Workbench,
    /// The bar of actions on the text selected in the list's messages.
    #[cfg(desktop)]
    pub selection: crate::selection_bar::SelectionBar,
}

impl Surroundings {
    /// The surroundings of the list being made, with a selection bar of its own.
    pub(crate) fn current() -> Self {
        Self {
            connection: use_context::<Connection>().unwrap(),
            files: use_context::<crate::files::Browser>().unwrap(),
            preview: use_context::<crate::media::Preview>().unwrap(),
            workspace: use_context::<crate::workbench::Workbench>().unwrap(),
            #[cfg(desktop)]
            selection: crate::selection_bar::SelectionBar::new(),
        }
    }

    /// The selection bar's view: its actions act as the text's own do.
    #[cfg(desktop)]
    pub(crate) fn selection_bar(self) -> impl IntoElement {
        self.selection.view(move |name, selected| {
            action(
                self,
                &json!({"action": name, "text": selected.text, "target": selected.target}),
            )
        })
    }

    /// Opens a link clicked in `target`'s text, `command` (⌘, Ctrl off the
    /// Mac) held or not, as the desktop client's links did: a web page in the
    /// in-app browser with it; on the desktop, a file by its primary action,
    /// which the editor takes with it; anything else as the file browser does.
    fn open_link(self, link: String, target: Target, command: bool) {
        if command && (link.starts_with("https://") || link.starts_with("http://")) {
            return self.workspace.browse(link);
        }
        #[cfg(desktop)]
        crate::file_actions::press(self.connection, self.files, target.link(link), command);
        #[cfg(not(desktop))]
        self.files.open(
            link,
            target.cwd,
            target.home,
            target.host,
            self.connection.client.get(),
        );
    }
}

/// `kind` is whose text it is: the native text names its role for accessibility
/// and dims an activity's.
pub(crate) fn content(
    surroundings: Surroundings,
    blocks: impl Fn() -> Vec<MarkdownBlock> + 'static,
    kind: impl Fn() -> Kind + Copy + 'static,
) -> nsh_core::element::View {
    let content = computed(move || parts(blocks(), &[], &|_| None));
    view_parts(
        surroundings,
        move || content.get(),
        target(surroundings.connection),
        kind,
    )
}

/// A message's Markdown `source`: on the desktop, its file links and paths as
/// the desktop client's chips, and its complete Mermaid blocks drawn.
pub(crate) fn message(
    surroundings: Surroundings,
    source: impl Fn() -> Option<String> + 'static,
    kind: impl Fn() -> Kind + Copy + 'static,
) -> nsh_core::element::View {
    let connection = surroundings.connection;
    let document = computed(move || {
        let Some(source) = source() else {
            return (Vec::new(), Vec::new());
        };
        let diagrams = diagrams(&source);
        #[cfg(desktop)]
        let blocks = agent_core::presentation::markdown::desktop_markdown_blocks(
            source,
            connection
                .composer
                .with(|c| c.controls.navigation.cwd.clone()),
            connection.context.with(|c| c.home()),
        );
        #[cfg(not(desktop))]
        let blocks = agent_core::presentation::markdown::markdown_blocks(source);
        (blocks, diagrams)
    });
    #[cfg(desktop)]
    let content = {
        let chips = computed(move || crate::file_actions::Chips::read(connection));
        let paths = computed(move || {
            document.with(|(blocks, _)| {
                let mut paths = Vec::new();
                for_each_file(blocks, &mut |file| paths.push(file.path.clone()));
                paths
            })
        });
        effect(move || crate::file_actions::read_metadata(connection, paths.get()));
        computed(move || {
            chips.with(|chips| {
                document.with(|(blocks, diagrams)| {
                    parts(blocks.clone(), diagrams, &|file| Some(chips.chip(file)))
                })
            })
        })
    };
    #[cfg(not(desktop))]
    let content = computed(move || {
        document.with(|(blocks, diagrams)| parts(blocks.clone(), diagrams, &|_| None))
    });
    view_parts(
        surroundings,
        move || content.get(),
        target(connection),
        kind,
    )
}

/// Every file a document's runs name, its tables' included.
#[cfg(desktop)]
fn for_each_file(blocks: &[MarkdownBlock], visit: &mut impl FnMut(&MarkdownFileReference)) {
    for block in blocks {
        let runs: Vec<&MarkdownRun> = match block {
            MarkdownBlock::Paragraph { runs, .. } => runs.iter().collect(),
            MarkdownBlock::Table { rows, .. } => {
                rows.iter().flatten().flat_map(|cell| &cell.runs).collect()
            }
            MarkdownBlock::Visualization { .. } => Vec::new(),
        };
        runs.into_iter()
            .filter_map(|run| run.file.as_ref())
            .for_each(&mut *visit);
    }
}

/// Where the content's menus and links act: its Host, draft and workspace.
fn target(connection: Connection) -> Memo<String> {
    computed(move || {
        let scope = connection.scope.get();
        let (cwd, provider) = connection
            .composer
            .with(|c| (c.controls.navigation.cwd.clone(), c.controls.provider));
        serde_json::to_string(&Target {
            host: scope.host,
            storage: scope.storage,
            draft: scope.draft,
            pane: connection.pane,
            cwd,
            home: connection.context.with(|c| c.home()),
            provider,
        })
        .expect("display targets contain only serializable values")
    })
}

/// What message text asks for: a selection quoted, explained or asked about
/// in a side chat, a link pressed, or an item of a file chip's menu chosen.
#[cfg(any(desktop, target_os = "ios"))]
fn action(surroundings: Surroundings, data: &Value) {
    let Surroundings {
        connection, files, ..
    } = surroundings;
    // TextKit's selection changed: where it is, in the window's content.
    #[cfg(target_os = "macos")]
    if data["action"] == "selection" {
        return surroundings.selection.select(selected(data));
    }
    let Some(text) = data["text"].as_str().filter(|s| !s.is_empty()) else {
        return;
    };
    let Some(target) = data["target"]
        .as_str()
        .and_then(|s| serde_json::from_str::<Target>(s).ok())
    else {
        return;
    };
    match data["action"].as_str() {
        Some("quote") => {
            let visible = connection.host.get_untracked() == target.host
                && connection.pane == target.pane
                && connection
                    .publication
                    .with_untracked(|p| p.composer.controls.navigation.draft_key == target.draft);
            connection.client.with(|client| {
                client.dispatch(
                    &target.host,
                    target.pane,
                    Intent::QuoteSelection {
                        storage_scope: target.storage,
                        thread_id: target.draft,
                        text: text.into(),
                    },
                )
            });
            if visible {
                after_answer(move || connection.composer_ref.focus());
            }
        }
        // From a side chat, it explains in a conversation of its own there.
        Some("explain") if let Some(pane) = target.pane => connection
            .application
            .with(|app| app.explain_in_side_chat(pane, text.into())),
        Some(action @ ("sideChat" | "explain")) => connection.application.with(|app| {
            app.open_side_chat(
                PanelScope {
                    host: target.host,
                    storage: target.storage,
                    draft: target.draft,
                },
                target.cwd,
                target.provider,
                if action == "explain" {
                    SideChatStart::Explain(text.into())
                } else {
                    SideChatStart::Quote(text.into())
                },
            );
        }),
        Some("link") => surroundings.open_link(
            text.into(),
            target,
            data["command"].as_bool().unwrap_or(false),
        ),
        #[cfg(desktop)]
        Some("file") => crate::file_actions::choose(
            connection,
            files,
            target.link(text.into()),
            data["id"].as_str().unwrap_or_default(),
        ),
        _ => {}
    }
}

/// A selection TextKit reported (`"selection"`): its text, its message's
/// target, and its box in the window's content; nothing once it is empty.
#[cfg(target_os = "macos")]
fn selected(data: &Value) -> Option<crate::selection_bar::Selected> {
    let text = data["text"].as_str().filter(|text| !text.is_empty())?;
    let frame: Vec<f32> = serde_json::from_value(data["bounds"].clone()).ok()?;
    let [x, y, width, height] = frame[..] else {
        return None;
    };
    Some(crate::selection_bar::Selected {
        text: text.into(),
        target: data["target"].as_str()?.into(),
        bounds: Bounds::new(point(px(x), px(y)), size(px(width), px(height))),
        side_chat: data["sideChat"].as_bool().unwrap_or(false),
    })
}

#[cfg(desktop)]
impl Target {
    fn link(self, link: String) -> crate::file_actions::LinkTarget {
        crate::file_actions::LinkTarget {
            link,
            cwd: self.cwd,
            home: self.home,
            host: self.host,
        }
    }
}

fn view_parts(
    surroundings: Surroundings,
    parts: impl Fn() -> Vec<Part> + 'static,
    target: Memo<String>,
    kind: impl Fn() -> Kind + Copy + 'static,
) -> nsh_core::element::View {
    let Surroundings {
        connection, files, ..
    } = surroundings;
    view().w_full().gap_3().child(each(
        move || parts().into_iter().enumerate().collect::<Vec<_>>(),
        |(index, part): &(usize, Part)| (*index, std::mem::discriminant(part)),
        move |part, _| match part.with_untracked(|(_, part)| part.clone()) {
            Part::Rule => view()
                .w_full()
                .h(px(1.0))
                .bg(crate::ui::colors::BORDER)
                .into_element(),
            Part::Text(_) | Part::Code { .. } => {
                #[cfg(any(target_os = "macos", target_os = "ios"))]
                let message = move || {
                    native("bex.messageText")
                        .prop("data", move || {
                            part.with(|(_, part)| {
                                let (Part::Text(blocks) | Part::Code { blocks, .. }) = part else {
                                    return Value::Null;
                                };
                                json!({"blocks": blocks, "secondary": kind() == Kind::Activity,
                        "role": if kind() == Kind::User { "user" } else { "assistant" },
                        "target": target.get(), "sideChat": connection.pane.is_none()})
                            })
                        })
                        .on_native_event(move |event: &NativeEvent| {
                            action(surroundings, &event.data)
                        })
                        .min_h(px(20.0))
                };
                #[cfg(not(any(target_os = "macos", target_os = "ios")))]
                let message = move || {
                    drawn::message(part, move || kind() == Kind::Activity, target, surroundings)
                };
                frame(part, move || message().w_full().into_element())
            }
            Part::Image(_) => crate::media::image_view(
                surroundings.connection,
                surroundings.preview,
                move || {
                    part.with(|(_, part)| {
                        if let Part::Image(source) = part {
                            source.clone()
                        } else {
                            String::new()
                        }
                    })
                },
                320.0,
                240.0,
            )
            .into_element(),
            Part::Link(_) => crate::ui::row_text_button(tr("ファイルを開く"), move || {
                let target = serde_json::from_str::<Target>(&target.get_untracked()).unwrap();
                let Part::Link(path) = part.with_untracked(|(_, part)| part.clone()) else {
                    return;
                };
                files.open(
                    path,
                    target.cwd,
                    target.home,
                    target.host,
                    connection.client.get(),
                );
            })
            .into_element(),
            Part::Table { .. } => table(surroundings, part, target, kind).into_element(),
        },
    ))
}

/// Text as it is, or a code block in the desktop client's frame: its language
/// and actions at its top right, its lines wrapped or scrolled sideways. A
/// Mermaid block shows its drawing, at most its own size, until its source is
/// asked for; the drawing enlarges and its source copies.
fn frame(
    part: ReadSignal<(usize, Part)>,
    message: impl Fn() -> AnyElement + Copy + 'static,
) -> AnyElement {
    let code = move || part.with(|(_, part)| matches!(part, Part::Code { .. }));
    let diagram = computed(move || {
        part.with(|(_, part)| match part {
            Part::Code { diagram, .. } => diagram.clone(),
            _ => None,
        })
    });
    let wrap = signal(false);
    let source_shown = signal(false);
    let large = signal(false);
    let drawn = move || diagram.with(Option::is_some) && !source_shown.get();
    let shown_code = move || code() && !drawn();
    view()
        .w_full()
        .gap_2()
        .style_when(code, |s| {
            s.px_4()
                .pt_2()
                .pb_4()
                .rounded(px(8.0))
                .border_1()
                .border_color(crate::ui::colors::BORDER)
                .bg(crate::ui::colors::SURFACE)
        })
        .child(when(code, move || {
            code_header(part, diagram, wrap, source_shown, large)
        }))
        .child(when(
            move || !code() || (shown_code() && wrap.get()),
            message,
        ))
        .child(when(
            move || shown_code() && !wrap.get(),
            move || scroll().horizontal().w_full().child(message()),
        ))
        .child(when(drawn, move || {
            // The drawing keeps its proportions and never grows past its own
            // size, or past twice the height when enlarged.
            let size =
                move || diagram.with(|d| d.as_ref().map_or((1.0, 1.0), |d| (d.width, d.height)));
            let limit = move || {
                let (width, height) = size();
                width
                    * if large.get() {
                        640.0
                    } else {
                        300.0_f32.min(height)
                    }
                    / height
            };
            svg(move || {
                ImageSrc::svg_markup(
                    &diagram.with(|d| d.as_ref().map(|d| d.svg.clone()).unwrap_or_default()),
                )
            })
            .w_full()
            .max_w(move || px(limit()))
            .aspect_ratio(move || {
                let (width, height) = size();
                width / height
            })
        }))
        .into_element()
}

fn code_header(
    part: ReadSignal<(usize, Part)>,
    diagram: Memo<Option<Diagram>>,
    wrap: Signal<bool>,
    source_shown: Signal<bool>,
    large: Signal<bool>,
) -> impl IntoElement {
    let toggle = |flag: Signal<bool>| move || flag.update(|flag| *flag = !*flag);
    let mermaid = move || diagram.with(Option::is_some);
    view()
        .flex_row()
        .items_center()
        .justify_end()
        .gap_2()
        .child(
            text(move || {
                part.with(|(_, part)| match part {
                    Part::Code { label, .. } => label.clone(),
                    _ => String::new(),
                })
            })
            .max_w(px(220.0))
            .number_of_lines(1)
            .text_size(px(12.0)),
        )
        .child(when(mermaid, move || {
            crate::ui::row_text_button(
                move || {
                    SharedString::from(if source_shown.get() {
                        tr("図を表示")
                    } else {
                        tr("ソース")
                    })
                },
                toggle(source_shown),
            )
        }))
        .child(when(
            move || mermaid() && !source_shown.get(),
            move || {
                crate::ui::row_text_button(
                    move || {
                        SharedString::from(if large.get() {
                            tr("縮小")
                        } else {
                            tr("拡大")
                        })
                    },
                    toggle(large),
                )
            },
        ))
        .child(when(
            move || !mermaid() || source_shown.get(),
            move || {
                crate::ui::row_text_button(
                    move || {
                        SharedString::from(if wrap.get() {
                            tr("折り返し解除")
                        } else {
                            tr("折り返し")
                        })
                    },
                    toggle(wrap),
                )
            },
        ))
        .child(crate::ui::icon_button(
            crate::ui::Icon::COPY,
            move || {
                SharedString::from(if mermaid() {
                    tr("Mermaidをコピー")
                } else {
                    tr("コードをコピー")
                })
            },
            move || {
                part.with_untracked(|(_, part)| {
                    if let Part::Code { source, .. } = part {
                        set_clipboard(source.clone());
                    }
                })
            },
        ))
}

/// A table in the desktop client's frame: ruled cells, the header row raised, its copy
/// actions below.
fn table(
    surroundings: Surroundings,
    part: ReadSignal<(usize, Part)>,
    target: Memo<String>,
    kind: impl Fn() -> Kind + Copy + 'static,
) -> impl IntoElement {
    use crate::ui::colors;
    let copy = move |markdown: bool| {
        part.with_untracked(|(_, part)| {
            if let Part::Table { source, csv, .. } = part {
                set_clipboard(if markdown {
                    source.clone()
                } else {
                    csv.clone()
                });
            }
        })
    };
    let compact = signal(false);
    let frame = move || {
        view()
            .w_full()
            .rounded(px(8.0))
            .border_1()
            .border_color(colors::BORDER)
            .overflow_hidden()
            .child(each(
                move || {
                    part.with(|(_, part)| {
                        if let Part::Table { rows, .. } = part {
                            rows.clone().into_iter().enumerate().collect()
                        } else {
                            Vec::new()
                        }
                    })
                },
                |(index, _): &(usize, Vec<Vec<Part>>)| *index,
                move |row, position| {
                    let header = row.with_untracked(|(index, _)| *index == 0);
                    view()
                        .flex_row()
                        .items_stretch()
                        .style_when(
                            move || position.get() > 0,
                            |s| s.border_t_1().border_t_color(colors::BORDER),
                        )
                        .style_when(move || header, |s| s.bg(colors::RAISED))
                        .child(each(
                            move || {
                                row.with(|(_, row)| {
                                    row.clone().into_iter().enumerate().collect::<Vec<_>>()
                                })
                            },
                            |(index, _): &(usize, Vec<Part>)| *index,
                            move |cell, column| {
                                view()
                                    .flex_1()
                                    .min_w_0()
                                    .px_3()
                                    .py_2()
                                    .style_when(
                                        move || column.get() > 0,
                                        |s| s.border_l_1().border_l_color(colors::BORDER),
                                    )
                                    .child(view_parts(
                                        surroundings,
                                        move || cell.with(|(_, parts)| parts.clone()),
                                        target,
                                        kind,
                                    ))
                            },
                        ))
                },
            ))
    };
    view()
        .w_full()
        .gap_2()
        .child(when(move || !compact.get(), frame))
        .child(when(
            move || compact.get(),
            move || scroll().horizontal().w_full().child(frame()),
        ))
        .child(
            view()
                .flex_row()
                .justify_end()
                .gap_2()
                .child(crate::ui::row_text_button(
                    move || {
                        SharedString::from(if compact.get() {
                            tr("セルを折り返す")
                        } else {
                            tr("セルをコンパクトに表示")
                        })
                    },
                    move || compact.update(|compact| *compact = !*compact),
                ))
                .child(crate::ui::row_text_button(
                    tr("Markdownをコピー"),
                    move || copy(true),
                ))
                .child(crate::ui::row_text_button(
                    tr("CSVをコピー"),
                    move || copy(false),
                )),
        )
}

/// Message text NSH lays out where TextKit does not: each paragraph of the
/// native document as selectable styled text, its links opened as TextKit's
/// are. Selection copies; quoting a selection and asking a side chat about it
/// are TextKit's menu, which these texts do not have.
#[cfg(not(any(target_os = "macos", target_os = "ios")))]
mod drawn {
    use super::{Part, Surroundings, Target};
    use crate::ui::colors;
    use nsh_core::{
        element::{ImageSrc, rgb},
        prelude::*,
    };
    use serde_json::Value;

    pub(super) fn message(
        part: ReadSignal<(usize, Part)>,
        secondary: impl Fn() -> bool + Copy + 'static,
        target_json: Memo<String>,
        surroundings: Surroundings,
    ) -> View {
        let target = move || serde_json::from_str::<Target>(&target_json.get_untracked()).ok();
        let links = Links {
            open: move |link: String, command: bool| {
                if let Some(target) = target() {
                    surroundings.open_link(link, target, command);
                }
            },
            choose: move |link: String, id: String| {
                #[cfg(desktop)]
                if let Some(target) = target() {
                    crate::file_actions::choose(
                        surroundings.connection,
                        surroundings.files,
                        target.link(link),
                        &id,
                    );
                }
                #[cfg(not(desktop))]
                let _ = (link, id);
            },
        };
        let message = view().w_full().min_h(px(20.0)).gap_2().selectable(true);
        #[cfg(desktop)]
        let message = selection_actions(message, target_json, surroundings);
        message.child(each(
            move || {
                part.with(|(_, part)| match part {
                    Part::Text(blocks) | Part::Code { blocks, .. } => blocks
                        .iter()
                        .enumerate()
                        .map(|(index, block)| (index, block.clone()))
                        .collect(),
                    _ => Vec::new(),
                })
            },
            |(index, block): &(usize, Value)| (*index, block.to_string()),
            move |block, _| paragraph(block.get_untracked().1, secondary(), links),
        ))
    }

    /// The selection's actions, as the Mac's text has them: the bar above the
    /// selection, and a right click's menu with them beside Copy and Select
    /// All, which act on the selection.
    #[cfg(desktop)]
    fn selection_actions(message: View, target: Memo<String>, surroundings: Surroundings) -> View {
        use crate::l10n::tr;
        use nsh_core::{
            desktop::MenuRole,
            element::desktop::{MenuItem, popup_menu_at},
        };
        let side_chat = surroundings.connection.pane.is_none();
        let selected = stored(String::new());
        message
            .on_text_selection_change(move |event: &TextSelectionEvent| {
                selected.replace(event.text.clone());
                surroundings.selection.select(
                    event
                        .page_bounds
                        .filter(|_| !event.text.is_empty())
                        .map(|bounds| crate::selection_bar::Selected {
                            text: event.text.clone(),
                            target: target.get_untracked(),
                            bounds,
                            side_chat,
                        }),
                );
            })
            .on_context_menu(move |event: &ContextMenuEvent| {
                let text = selected.get();
                let act = |name: &'static str| {
                    let text = text.clone();
                    move || {
                        super::action(
                            surroundings,
                            &serde_json::json!({
                                "action": name, "text": text, "target": target.get_untracked(),
                            }),
                        )
                    }
                };
                let mut items = Vec::new();
                if !text.is_empty() {
                    items.push(MenuItem::new(tr("チャットに追加")).on_select(act("quote")));
                    items.push(MenuItem::new(tr("詳細を表示")).on_select(act("explain")));
                    if side_chat {
                        items.push(
                            MenuItem::new(tr("サイドチャットで質問")).on_select(act("sideChat")),
                        );
                    }
                    items.push(MenuItem::separator());
                }
                items.push(MenuItem::role(MenuRole::Copy).disabled(text.is_empty()));
                items.push(MenuItem::role(MenuRole::SelectAll));
                surroundings.selection.dismiss();
                drop(popup_menu_at(items, event.page_position));
            })
    }

    /// What a paragraph's links do: `open` a link pressed, ⌘ (Ctrl here)
    /// held or not, and `choose` an item of a file chip's menu.
    #[derive(Clone, Copy)]
    struct Links<O, C> {
        open: O,
        choose: C,
    }

    /// One paragraph of the document `rich_text::paragraph` writes.
    fn paragraph<O, C>(block: Value, secondary: bool, links: Links<O, C>) -> View
    where
        O: Fn(String, bool) + Copy + 'static,
        C: Fn(String, String) + Copy + 'static,
    {
        let code = block["code"].as_bool().unwrap_or(false);
        let size = match block["header"].as_u64() {
            Some(1) => 22.0,
            Some(2) => 18.0,
            Some(3) => 16.0,
            _ if code => 13.0,
            _ => 14.0,
        };
        let mut line = text("").text_size(px(size)).text_color(if secondary {
            colors::MUTED
        } else {
            colors::FOREGROUND
        });
        if block["header"].is_u64() {
            line = line.font_weight(FontWeight::SEMIBOLD);
        }
        if code {
            line = line.font_family(SharedString::from("monospace"));
        }
        line = match block["alignment"].as_str() {
            Some("center") => line.text_center(),
            Some("right") => line.text_right(),
            _ => line,
        };
        if let Some(marker) = block["marker"].as_str() {
            line = line.child(text(format!("{marker} ")).text_color(colors::MUTED));
        }
        for run in block["runs"].as_array().into_iter().flatten() {
            let mut span = text(run["text"].as_str().unwrap_or_default().to_owned());
            if run["strong"].as_bool().unwrap_or(false) {
                span = span.font_weight(FontWeight::BOLD);
            }
            if run["emphasis"].as_bool().unwrap_or(false) {
                span = span.italic();
            }
            if run["strikethrough"].as_bool().unwrap_or(false) {
                span = span.line_through();
            }
            if run["code"].as_bool().unwrap_or(false) && !code {
                span = span
                    .font_family(SharedString::from("monospace"))
                    .bg(colors::RAISED);
            }
            // The app's look is dark: a run's own color is its dark variant.
            if let Some(color) = run["darkColor"]
                .as_u64()
                .and_then(|c| u32::try_from(c).ok())
            {
                span = span.text_color(rgb(color));
            }
            if let Some(link) = run["link"].as_str().map(str::to_owned) {
                if let Some(chip) = run.get("file") {
                    line = line.child(chip_span(span, chip, link, links));
                    continue;
                }
                let open = links.open;
                span = span
                    .text_color(colors::ACCENT)
                    .underline()
                    .on_press(move |event: &PressEvent| open(link.clone(), event.modifiers.ctrl));
            }
            line = line.child(span);
        }
        let depth = block["depth"].as_u64().unwrap_or(0) as f32;
        let quoted = block["quoted"].as_bool().unwrap_or(false);
        view()
            .w_full()
            .pl(px(depth * 16.0 + if quoted { 12.0 } else { 0.0 }))
            .style_when(
                move || quoted,
                |s| s.border_l_1().border_l_color(colors::BORDER),
            )
            .child(line)
    }

    /// A file chip as the desktop client's: its glyph and label in the file's accent, at
    /// medium weight, its full path and size over it, its menu on a secondary
    /// press; a press opens it as a link does.
    fn chip_span<O, C>(label: Text, chip: &Value, link: String, links: Links<O, C>) -> Text
    where
        O: Fn(String, bool) + Copy + 'static,
        C: Fn(String, String) + Copy + 'static,
    {
        let color = chip["darkColor"]
            .as_u64()
            .and_then(|c| u32::try_from(c).ok())
            .map_or(colors::ACCENT, rgb);
        let glyph = if chip["folder"].as_bool().unwrap_or(false) {
            crate::ui::icon(crate::ui::Icon::FOLDER, 14.0).into_element()
        } else {
            svg(ImageSrc::svg_markup(
                chip["icon"].as_str().unwrap_or_default(),
            ))
            .w(px(14.0))
            .h(px(14.0))
            .into_element()
        };
        let menu = chip["menu"].clone();
        let (open, choose) = (links.open, links.choose);
        let pressed = link.clone();
        text("")
            .text_color(color)
            .font_weight(FontWeight::MEDIUM)
            .tooltip(crate::ui::tooltip(SharedString::from(
                chip["tooltip"].as_str().unwrap_or_default().to_owned(),
            )))
            .child(glyph)
            .child(text(" "))
            .child(label)
            .on_press(move |event: &PressEvent| open(pressed.clone(), event.modifiers.ctrl))
            .on_context_menu(move |_: &ContextMenuEvent| {
                let link = link.clone();
                crate::ui::popup(&menu, move |id: &str| choose(link.clone(), id.into()));
            })
    }
}

/// A paragraph of the native document; a run naming a file carries its chip.
fn paragraph(runs: Vec<MarkdownRun>, style: &MarkdownStyle, chip: Chip) -> Value {
    json!({
        "header": style.header, "marker": style.marker, "code": style.code,
        "quoted": style.quoted, "depth": style.list_depth,
        "alignment": match style.alignment { MarkdownAlignment::Left => "left", MarkdownAlignment::Center => "center", MarkdownAlignment::Right => "right" },
        "runs": runs.into_iter().map(|run| {
            let mut value = json!({
                "text": run.text, "strong": run.strong, "emphasis": run.emphasis,
                "strikethrough": run.strikethrough, "code": run.code, "link": run.link,
                "darkColor": run.dark_color, "lightColor": run.light_color,
            });
            if let Some(chip) = run.file.as_ref().and_then(chip) {
                value["file"] = chip;
            }
            value
        }).collect::<Vec<_>>()
    })
}

fn flush(text: &mut Vec<Value>, parts: &mut Vec<Part>) {
    if !text.is_empty() {
        parts.push(Part::Text(std::mem::take(text)));
    }
}

/// A code block, labelled with its file or language; a Mermaid block with
/// its drawing when Core drew it.
fn code(
    runs: Vec<MarkdownRun>,
    style: MarkdownStyle,
    diagrams: &[(String, Diagram)],
    chip: Chip,
) -> Part {
    let source: String = runs.iter().map(|run| run.text.as_str()).collect();
    let diagram = (style.language.as_deref() == Some("mermaid"))
        .then(|| {
            diagrams
                .iter()
                .find(|(code, _)| code.trim_end() == source.trim_end())
        })
        .flatten()
        .map(|(_, diagram)| diagram.clone());
    Part::Code {
        label: style
            .filename
            .clone()
            .or_else(|| style.language.clone())
            .unwrap_or_else(|| "CODE".into()),
        source,
        blocks: vec![paragraph(runs, &style, chip)],
        diagram,
    }
}

/// `blocks` as the parts views lay out, a Mermaid block with its drawing.
fn parts(blocks: Vec<MarkdownBlock>, diagrams: &[(String, Diagram)], chip: Chip) -> Vec<Part> {
    let mut result = Vec::new();
    let mut text = Vec::new();
    for block in blocks {
        match block {
            MarkdownBlock::Paragraph { style, .. } if style.rule => {
                flush(&mut text, &mut result);
                result.push(Part::Rule);
            }
            MarkdownBlock::Paragraph { runs, style } if style.code => {
                flush(&mut text, &mut result);
                result.push(code(runs, style, diagrams, chip));
            }
            MarkdownBlock::Paragraph { runs, style } => {
                let mut content = Vec::new();
                for run in runs {
                    if let Some(image) = run.image {
                        if !content.is_empty() {
                            text.push(paragraph(std::mem::take(&mut content), &style, chip));
                        }
                        flush(&mut text, &mut result);
                        result.push(Part::Image(image));
                    } else {
                        content.push(run);
                    }
                }
                if !content.is_empty() {
                    text.push(paragraph(content, &style, chip));
                }
            }
            MarkdownBlock::Visualization { path } => {
                flush(&mut text, &mut result);
                result.push(Part::Link(path));
            }
            MarkdownBlock::Table {
                source,
                columns,
                rows,
            } => {
                flush(&mut text, &mut result);
                result.push(Part::Table {
                    source,
                    csv: agent_core::presentation::markdown::markdown_table_csv(
                        rows.iter()
                            .map(|row| {
                                row.iter()
                                    .map(|cell| {
                                        cell.runs.iter().map(|run| run.text.as_str()).collect()
                                    })
                                    .collect()
                            })
                            .collect(),
                    ),
                    rows: rows
                        .into_iter()
                        .map(|row| {
                            row.into_iter()
                                .enumerate()
                                .map(|(column, cell)| {
                                    parts(
                                        vec![MarkdownBlock::Paragraph {
                                            runs: cell.runs,
                                            style: MarkdownStyle {
                                                alignment: columns
                                                    .get(column)
                                                    .copied()
                                                    .unwrap_or_default(),
                                                ..Default::default()
                                            },
                                        }],
                                        diagrams,
                                        chip,
                                    )
                                })
                                .collect()
                        })
                        .collect(),
                });
            }
        }
    }
    flush(&mut text, &mut result);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_text_contract_preserves_unicode_style_links_and_selection_target() {
        let blocks = vec![paragraph(
            vec![
                MarkdownRun {
                    text: "日本語 👋".into(),
                    strong: true,
                    ..Default::default()
                },
                MarkdownRun {
                    text: " ファイル".into(),
                    link: Some("file:///fixture/%E6%97%A5%E6%9C%AC%E8%AA%9E.txt".into()),
                    ..Default::default()
                },
            ],
            &MarkdownStyle {
                header: Some(2),
                ..Default::default()
            },
            &|_| None,
        )];
        let expected: Value = serde_json::from_str(include_str!(
            "../apple/Tests/BexNshNativeTests/Fixtures/message.json"
        ))
        .unwrap();
        assert_eq!(
            json!({"blocks": blocks, "secondary": false, "role": "user", "target": "opaque-native-menu-target", "sideChat": true}),
            expected
        );
    }

    #[test]
    fn native_parts_preserve_media_code_rules_and_table_copy_sources_in_order() {
        let source = "前文 **強調**\n\n![画像](https://example.invalid/image.png)\n\n```rust title=main.rs\nlet 日本語 = 1;\n```\n\n---\n\n|名前|値|\n|:---|---:|\n|日本語|a,b|";
        let parts = parts(
            agent_core::presentation::markdown::markdown_blocks(source.into()),
            &[],
            &|_| None,
        );
        assert!(matches!(&parts[0], Part::Text(blocks) if blocks[0]["runs"][1]["strong"] == true));
        assert!(
            matches!(&parts[1], Part::Image(source) if source == "https://example.invalid/image.png")
        );
        assert!(matches!(&parts[2], Part::Code { source, .. } if source == "let 日本語 = 1;"));
        assert!(matches!(&parts[3], Part::Rule));
        let Part::Table { source, csv, rows } = &parts[4] else {
            panic!("table is retained");
        };
        assert!(source.starts_with("|名前|値|"));
        assert_eq!(csv, "\"名前\",\"値\"\r\n\"日本語\",\"a,b\"");
        assert_eq!(rows.len(), 2);
        assert!(
            matches!(&rows[1][1][0], Part::Text(blocks) if blocks[0]["alignment"] == "right" && blocks[0]["runs"][0]["text"] == "a,b")
        );
        assert_eq!(parts.len(), 5);
    }

    #[test]
    fn complete_mermaid_blocks_draw_and_a_streaming_one_stays_code() {
        let source = "図です。\n\n```mermaid\nflowchart LR\n  A --> B\n```";
        let parsed = |source: &str| {
            parts(
                agent_core::presentation::markdown::markdown_blocks(source.into()),
                &diagrams(source),
                &|_| None,
            )
        };
        let drawn = parsed(source);
        let Some(Part::Code {
            diagram: Some(diagram),
            source: code,
            ..
        }) = drawn.last()
        else {
            panic!("a complete Mermaid block draws: {}", drawn.len());
        };
        assert!(diagram.svg.contains("<svg") && diagram.width > 0.0 && diagram.height > 0.0);
        assert_eq!(code.trim_end(), "flowchart LR\n  A --> B");
        let streaming = parsed(&source[..source.len() - 3]);
        assert!(
            matches!(streaming.last(), Some(Part::Code { diagram: None, .. })),
            "an unfinished fence is still being written"
        );
    }

    #[test]
    fn a_file_run_carries_the_chip_its_reference_shows() {
        let file = MarkdownFileReference {
            path: "/repo/a.rs".into(),
            label: "a.rs".into(),
            kind: agent_core::presentation::markdown::MarkdownFileKind::Code,
            line: None,
            column: None,
        };
        let block = paragraph(
            vec![
                MarkdownRun {
                    text: "a.rs".into(),
                    link: Some("a.rs".into()),
                    file: Some(file),
                    ..Default::default()
                },
                MarkdownRun {
                    text: " です".into(),
                    ..Default::default()
                },
            ],
            &MarkdownStyle::default(),
            &|file| Some(json!({"tooltip": file.path})),
        );
        assert_eq!(block["runs"][0]["file"]["tooltip"], "/repo/a.rs");
        assert!(block["runs"][1].get("file").is_none());
    }

    #[test]
    fn invocation_and_currency_text_keeps_the_same_gfm_meaning_as_existing_clients() {
        let text = "$fixture-review @Fixture Plugin [success] costs $5 · 日本語 👋";
        let parts = parts(
            agent_core::presentation::markdown::markdown_blocks(text.into()),
            &[],
            &|_| None,
        );
        let [Part::Text(blocks)] = &parts[..] else {
            panic!("plain text stays text");
        };
        assert_eq!(blocks[0]["runs"][0]["text"], text);
        assert_eq!(blocks[0]["runs"][0]["link"], Value::Null);
        assert_eq!(blocks[0]["code"], false);
    }

    #[test]
    fn tool_output_stays_literal_in_the_native_document() {
        let output = "# output\n[open](https://example.com)\n<img src=remote>\n```\n日本語";
        let parts = parts(
            vec![agent_core::presentation::markdown::markdown_code_block(
                output.into(),
                None,
                Some("出力".into()),
            )],
            &[],
            &|_| None,
        );
        let [
            Part::Code {
                blocks,
                source,
                label,
                diagram: None,
            },
        ] = &parts[..]
        else {
            panic!("tool output stays code");
        };
        assert_eq!(source, output);
        assert_eq!(label, "出力");
        assert_eq!(
            blocks[0]["runs"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|run| run["text"].as_str())
                .collect::<String>(),
            output
        );
        assert_eq!(blocks[0]["code"], true);
    }
}
