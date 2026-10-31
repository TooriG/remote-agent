use super::*;
use agent_core::presentation::markdown::MarkdownDiagram;
use gpui_kit::base::text::MarkdownNode;

use super::conversation::ConversationView;

impl ConversationView {
    pub(super) fn markdown_diagram(
        &mut self,
        message: &str,
        node: &MarkdownNode,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let language = crate::localization::language(cx);
        let Some(diagram) = node.data::<MarkdownDiagram>() else {
            return div().into_any_element();
        };
        let source_key = format!("mermaid-source-{message}-{}", diagram.source);
        let large_key = format!("mermaid-large-{message}-{}", diagram.source);
        let source_visible = self.timeline.expanded_items.contains(&source_key);
        let large = self.timeline.expanded_items.contains(&large_key);
        let message = message.to_owned();
        let source_message = message.clone();
        let large_message = message.clone();
        let source = diagram.source.clone();
        let header = h_flex()
            .gap_2()
            .items_center()
            .child(div().text_xs().child("MERMAID"))
            .child(
                Button::new("diagram-source")
                    .label(if source_visible {
                        language.text("図を表示")
                    } else {
                        language.text("ソース")
                    })
                    .small()
                    .ghost()
                    .on_click(cx.listener(move |view, _, _, cx| {
                        if !view.timeline.expanded_items.remove(&source_key) {
                            view.timeline.expanded_items.insert(source_key.clone());
                        }
                        view.timeline.remeasure_item(&source_message);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("diagram-size")
                    .label(if large {
                        language.text("縮小")
                    } else {
                        language.text("拡大")
                    })
                    .small()
                    .ghost()
                    .on_click(cx.listener(move |view, _, _, cx| {
                        if !view.timeline.expanded_items.remove(&large_key) {
                            view.timeline.expanded_items.insert(large_key.clone());
                        }
                        view.timeline.remeasure_item(&large_message);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("copy-diagram")
                    .icon(IconName::Copy)
                    .small()
                    .ghost()
                    .tooltip(language.text("Mermaidをコピー"))
                    .on_click(move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(source.clone()))
                    }),
            );
        let mut body = v_flex()
            .w_full()
            .min_w_0()
            .border_1()
            .border_color(cx.theme().border)
            .rounded_md()
            .p_3()
            .gap_2()
            .child(header);
        if source_visible {
            return body
                .child(
                    TextView::markdown(
                        SharedString::from(format!("{message}-mermaid-source")),
                        node.as_markdown().to_owned(),
                    )
                    .selectable(true),
                )
                .into_any_element();
        }
        let Some(cached) = self.timeline.markdown.get_mut(&message) else {
            return body.into_any_element();
        };
        if !cached.diagrams.contains_key(&diagram.source) {
            let renderer = cx.svg_renderer();
            let image = renderer.parse_svg(diagram.svg.as_bytes()).and_then(|svg| {
                let scale = (1536.0 / diagram.width)
                    .min(1536.0 / diagram.height)
                    .min(2.0);
                renderer.render_parsed(
                    &svg,
                    SvgSize::ExactSize(size(
                        DevicePixels((diagram.width * scale).ceil().max(1.0) as i32),
                        DevicePixels((diagram.height * scale).ceil().max(1.0) as i32),
                    )),
                )
            });
            if let Ok(image) = image {
                cached.diagrams.insert(diagram.source.clone(), image);
            }
        }
        if let Some(image) = cached.diagrams.get(&diagram.source) {
            body = body.child(
                img(image.clone())
                    .w_full()
                    .h(px(if large { 640. } else { 300. }))
                    .object_fit(ObjectFit::Contain),
            );
        } else {
            body = body.child(
                TextView::markdown(
                    SharedString::from(format!("{message}-mermaid-fallback")),
                    node.as_markdown().to_owned(),
                )
                .selectable(true),
            );
        }
        body.into_any_element()
    }
}
