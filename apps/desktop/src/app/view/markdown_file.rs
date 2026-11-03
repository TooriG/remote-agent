use super::*;
use agent_core::presentation::{
    files::{desktop_file_chip_color, desktop_metadata_icon, file_reference_tooltip},
    markdown::{MarkdownFileChip, MarkdownFileReference, MarkdownRun, file_position_target},
};
use agent_protocol::file_metadata::{FileMetadata, FileMetadataKind};
use gpui_kit::base::text::{
    InlineElement, InlineRenderContext, MarkdownNode, MarkdownParseContext, MarkdownPlugin,
    markdown_ast,
};

#[derive(Clone)]
pub(in crate::app) struct FileChips {
    pub(super) files: Arc<HashMap<usize, MarkdownFileChip>>,
    views: Arc<HashMap<usize, Entity<FileChip>>>,
}
impl FileChips {
    pub(super) fn new(
        files: Arc<HashMap<usize, MarkdownFileChip>>,
        desktop: &Entity<Desktop>,
        cx: &mut App,
    ) -> Self {
        let views = files
            .iter()
            .map(|(offset, chip)| {
                (
                    *offset,
                    cx.new(|cx| {
                        FileChip::new(
                            chip.file.clone(),
                            chip.lookup_path.clone(),
                            chip.caption.clone(),
                            desktop,
                            cx,
                        )
                    }),
                )
            })
            .collect();
        let paths: Vec<_> = files.values().map(|chip| chip.file.path.clone()).collect();
        let owner = desktop.downgrade();
        let epoch = desktop.read(cx).snapshot.epoch;
        if !paths.is_empty() {
            cx.defer(move |cx| {
                let _ = owner.update(cx, |desktop, _| {
                    if desktop.snapshot.epoch == epoch {
                        desktop.ensure_reference_metadata(paths);
                    }
                });
            });
        }
        Self {
            files,
            views: Arc::new(views),
        }
    }
}
impl MarkdownPlugin for FileChips {
    fn name(&self) -> &str {
        "file-reference"
    }
    fn parse(
        &self,
        node: &markdown_ast::Node,
        cx: &MarkdownParseContext<'_>,
    ) -> Option<MarkdownNode> {
        if !matches!(
            node,
            markdown_ast::Node::Link(_)
                | markdown_ast::Node::LinkReference(_)
                | markdown_ast::Node::InlineCode(_)
        ) {
            return None;
        }
        let offset = node.position()?.start.offset + cx.offset();
        let chip = self.files.get(&offset)?;
        Some(
            MarkdownNode::new(self.name().to_owned(), offset)
                .text(if chip.caption.is_some() {
                    format!("{} {}", node.to_string(), chip.file.label)
                } else {
                    chip.file.label.clone()
                })
                .accessibility_label(file_position_target(
                    &chip.file.path,
                    chip.file.line,
                    chip.file.column,
                ))
                .markdown(chip.copy_markdown.clone()),
        )
    }
    fn render_inline(
        &self,
        node: &MarkdownNode,
        context: &InlineRenderContext,
        _: &mut Window,
        _: &mut App,
    ) -> Option<InlineElement> {
        Some(InlineElement::new(
            div()
                .text_size(context.font_size())
                .line_height(relative(1.2))
                .child(self.views.get(node.data::<usize>()?)?.clone()),
        ))
    }
}

struct FileChip {
    file: MarkdownFileReference,
    lookup_path: Option<String>,
    caption: Option<Vec<MarkdownRun>>,
    desktop: WeakEntity<Desktop>,
    metadata: Option<FileMetadata>,
    focus: FocusHandle,
    epoch: u64,
    _observation: Subscription,
}
impl FileChip {
    fn new(
        file: MarkdownFileReference,
        lookup_path: Option<String>,
        caption: Option<Vec<MarkdownRun>>,
        desktop: &Entity<Desktop>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            metadata: desktop
                .read(cx)
                .snapshot
                .file_metadata(file.path.clone())
                .and_then(|entry| entry.metadata),
            file,
            lookup_path,
            caption,
            desktop: desktop.downgrade(),
            focus: cx.focus_handle(),
            epoch: desktop.read(cx).snapshot.epoch,
            _observation: cx.observe(desktop, |view, desktop, cx| {
                let metadata = desktop
                    .read(cx)
                    .snapshot
                    .file_metadata(view.file.path.clone())
                    .and_then(|entry| entry.metadata);
                if view.metadata != metadata {
                    view.metadata = metadata;
                    cx.notify();
                }
            }),
        }
    }
    fn activate(
        &self,
        modifier: bool,
        position: Point<Pixels>,
        menu: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        let _ = self.desktop.update(cx, |desktop, cx| {
            if desktop.snapshot.epoch != self.epoch {
                return;
            }
            if menu {
                desktop.show_reference_menu(
                    &self.file,
                    self.lookup_path.as_deref(),
                    position,
                    window,
                    cx,
                );
            } else {
                desktop.activate_reference(
                    &self.file,
                    self.lookup_path.as_deref(),
                    modifier,
                    position,
                    window,
                    cx,
                );
            }
        });
    }
}
impl Render for FileChip {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let font_size = window.text_style().font_size.to_pixels(window.rem_size());
        let metadata = self.metadata.as_ref();
        let color = rgb(desktop_file_chip_color(
            &self.file.path,
            &[
                appearance::BACKGROUND,
                appearance::SURFACE,
                appearance::RAISED,
            ],
        ));
        let icon = if metadata.is_some_and(|metadata| metadata.kind == FileMetadataKind::Directory)
        {
            Icon::new(IconName::Folder)
                .size(font_size)
                .into_any_element()
        } else {
            let name = desktop_metadata_icon(
                &self.file.path,
                metadata.and_then(|metadata| metadata.mime_type.as_deref()),
            );
            let theme = if cx.theme().is_dark() {
                "dark"
            } else {
                "light"
            };
            let asset = SharedString::from(format!("icons/file-icons/{name}-{theme}.svg"));
            if matches!(
                name,
                "t3-file-icon-pdf"
                    | "t3-file-icon-word"
                    | "t3-file-icon-spreadsheet"
                    | "t3-file-icon-slides"
                    | "t3-file-icon-epub"
            ) {
                Icon::default()
                    .path(asset)
                    .size(font_size)
                    .text_color(color)
                    .into_any_element()
            } else {
                img(asset)
                    .size(font_size)
                    .flex_shrink_0()
                    .into_any_element()
            }
        };
        let target = file_position_target(&self.file.path, self.file.line, self.file.column);
        let tooltip =
            file_reference_tooltip(&target, metadata.and_then(|metadata| metadata.byte_length));
        h_flex()
            .id("file-reference")
            .debug_selector(|| format!("markdown-file:{}", self.file.path))
            .track_focus(&self.focus)
            .tab_stop(true)
            .gap(font_size * 0.2)
            .max_w_full()
            .min_w_0()
            .whitespace_nowrap()
            .cursor_pointer()
            .rounded_sm()
            .focus_visible(|style| {
                style.shadow(vec![BoxShadow {
                    color: color.into(),
                    offset: point(px(0.), px(0.)),
                    blur_radius: px(0.),
                    spread_radius: px(2.),
                    inset: true,
                }])
            })
            // GPUI Kit's public inline API exposes one atomic element. Keep
            // the authored caption and chip together: neither wraps nor offers
            // partial text selection, unlike T3's separate caption text runs.
            .children(self.caption.as_ref().map(|caption| {
                div()
                    .debug_selector(|| format!("markdown-file-caption:{}", self.file.path))
                    .child(caption_text(
                        caption,
                        window.text_style(),
                        cx.theme().mono_font_family.clone(),
                        gpui_kit::base::text::TextViewStyle::from_theme(
                            &gpui_kit::base::Theme::global(cx),
                        )
                        .inline_code(),
                    ))
            }))
            .child(
                h_flex()
                    .gap(font_size * 0.2)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(color)
                    .child(icon)
                    .child(
                        div()
                            .min_w_0()
                            .text_ellipsis()
                            .group_hover("file-reference", |style| style.underline())
                            .child(self.file.label.clone()),
                    ),
            )
            .group("file-reference")
            .tooltip(move |window, cx| {
                let tooltip = tooltip.clone();
                let width = px(480.).min(window.viewport_size().width - px(10.));
                tooltip::Tooltip::element(move |_, cx| {
                    div()
                        .id("file-tooltip-path")
                        .max_w((width - px(18.)).max(px(0.)))
                        .overflow_x_scroll()
                        .whitespace_nowrap()
                        .font_family(cx.theme().mono_font_family.clone())
                        .child(tooltip.clone())
                })
                .max_w(width)
                .py_1()
                .text_size(px(11.))
                .line_height(relative(1.625))
                .build(window, cx)
            })
            .on_hover(cx.listener(|view, hovered, _, cx| {
                if *hovered {
                    let _ = view.desktop.update(cx, |desktop, _| {
                        if desktop.snapshot.epoch == view.epoch {
                            desktop.ensure_reference_metadata(vec![view.file.path.clone()]);
                        }
                    });
                }
            }))
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_mouse_up(
                MouseButton::Right,
                cx.listener(|view, event: &MouseUpEvent, window, cx| {
                    view.activate(false, event.position, true, window, cx)
                }),
            )
            .on_click(cx.listener(|view, event: &ClickEvent, window, cx| {
                let modifier = editor_modifier(event.modifiers());
                view.activate(modifier, window.mouse_position(), false, window, cx);
            }))
            .on_key_down(cx.listener(|view, event: &KeyDownEvent, window, cx| {
                let key = event.keystroke.key.as_str();
                if matches!(key, "enter" | "space")
                    || (key == "f10" && event.keystroke.modifiers.shift)
                {
                    let modifier = editor_modifier(event.keystroke.modifiers);
                    view.activate(modifier, window.mouse_position(), key == "f10", window, cx);
                }
            }))
    }
}
fn caption_text(
    caption: &[MarkdownRun],
    style: gpui_kit::TextStyle,
    mono_font: SharedString,
    code_style: HighlightStyle,
) -> gpui_kit::StyledText {
    let mut text = String::new();
    let runs = caption
        .iter()
        .map(|run| {
            let mut style = style.clone();
            if run.code {
                style = style.highlight(code_style);
                style.font_family = mono_font.clone();
            }
            if run.strong {
                style.font_weight = FontWeight::BOLD;
            }
            if run.emphasis {
                style.font_style = gpui_kit::FontStyle::Italic;
            }
            if run.strikethrough {
                style.strikethrough = Some(Default::default());
            }
            let fragment = run.text.replace(['\r', '\n'], " ");
            text.push_str(&fragment);
            style.to_run(fragment.len())
        })
        .collect();
    gpui_kit::StyledText::new(text).with_runs(runs)
}

fn editor_modifier(modifiers: Modifiers) -> bool {
    if cfg!(target_os = "macos") {
        modifiers.platform
    } else {
        modifiers.control
    }
}
impl Desktop {
    fn ensure_reference_metadata(&self, paths: Vec<String>) {
        if !paths.is_empty() {
            self.dispatch(Intent::EnsureFileMetadata {
                paths,
                now_ms: chrono::Utc::now().timestamp_millis().max(0) as u64,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::FileChips;
    use gpui_kit as gpui;
    use gpui_kit::{
        AppContext, Context, Entity, IntoElement, Render, Window,
        base::text::{SelectionFormat, TextView, TextViewState},
    };
    use std::sync::Arc;

    struct ReferenceText {
        state: Entity<TextViewState>,
        chips: FileChips,
    }

    impl Render for ReferenceText {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            TextView::new(&self.state)
                .selection_format(SelectionFormat::Source)
                .plugin(self.chips.clone())
        }
    }

    #[gpui::test]
    fn public_inline_plugin_preserves_caption_text_and_copies_links_once(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(crate::appearance::init);
        let source = "before [**設計資料**](docs/a.md) and `src/main.rs:4` after";
        let presentation =
            agent_core::presentation::markdown::markdown_presentation(source, "/repo", None);
        let (view, window) = cx.add_window_view(|_, cx| ReferenceText {
            state: cx.new(|cx| TextViewState::markdown(source, cx)),
            chips: FileChips {
                files: presentation.files.into(),
                // No native views: exercise the public API's atomic text fallback
                // as well as its parsed copy representation.
                views: Arc::default(),
            },
        });
        window.run_until_parked();
        let state = view.read_with(window, |view, _| view.state.clone());
        state.update(window, |state, cx| {
            state.select_all(cx);
            assert_eq!(state.selected_text(), source);
            state.set_selection_format(SelectionFormat::Plain, cx);
            assert_eq!(
                state.selected_text().trim(),
                "before 設計資料 a.md and main.rs · L4 after"
            );
        });
    }
}
