use crate::{Connection, media::Preview};
use agent_core::presentation::markdown::{MarkdownBlock, MarkdownRun};
use nsh_base::text_view::{
    Block, CodeBlock, ColumnAlign, Document, Inline, Table, TableCell, TextViewElement,
};
use nsh_core::prelude::*;

pub(crate) fn view() -> TextViewElement {
    let connection = use_context::<Connection>().unwrap();
    let preview = use_context::<Preview>().unwrap();
    let files = use_context::<crate::files::Browser>().unwrap();
    nsh_base::text_view::text_view()
        .selectable(true)
        .on_image_press(move |image: &nsh_base::text_view::Image| {
            preview.load(crate::media::load_image(
                connection.client.get(),
                connection.host.get_untracked(),
                connection
                    .publication
                    .with_untracked(|p| p.composer.controls.navigation.cwd.clone()),
                image.src.clone(),
            ));
        })
        .on_link_press(move |event: &nsh_base::text_view::TextViewLinkPressEvent| {
            let (cwd, home) = connection
                .publication
                .with_untracked(|p| (p.composer.controls.navigation.cwd.clone(), p.context.home()));
            files.open(
                event.url.to_string(),
                cwd,
                home,
                connection.host.get_untracked(),
                connection.client.get(),
            );
        })
        .render_image(|image: &nsh_base::text_view::Image| {
            let source = image.src.to_string();
            crate::media::image_view(move || source.clone(), 320.0, 240.0)
        })
}

// Core determines what is code, a link, or formatted text. This adapter only
// translates that typed content to NSH; tool output is never parsed as markup.
pub(crate) fn document(blocks: Vec<MarkdownBlock>) -> Document {
    Document::new(
        blocks
            .into_iter()
            .map(|block| match block {
                MarkdownBlock::Paragraph { runs, style } if style.code => {
                    let mut code = CodeBlock::new(
                        style.language,
                        runs.into_iter().map(|run| run.text).collect::<String>(),
                    );
                    code.meta = style.filename;
                    Block::CodeBlock(code)
                }
                MarkdownBlock::Paragraph { runs, style } => {
                    let mut content = inlines(runs);
                    if let Some(marker) = style.marker {
                        content.insert(0, Inline::text(format!("{marker} ")));
                    }
                    let block = if style.rule {
                        Block::Rule
                    } else if let Some(level) = style.header {
                        Block::Heading {
                            level: level.clamp(1, 6),
                            content,
                            anchor: None,
                        }
                    } else {
                        Block::paragraph(content)
                    };
                    if style.quoted {
                        Block::Blockquote {
                            children: vec![block],
                            alert: None,
                        }
                    } else {
                        block
                    }
                }
                MarkdownBlock::Table { columns, rows, .. } => {
                    let mut rows = rows.into_iter().map(|row| {
                        row.into_iter()
                            .map(|cell| TableCell::new(inlines(cell.runs)))
                            .collect()
                    });
                    let mut table = Table::default();
                    table.align = columns
                        .into_iter()
                        .map(|column| match column {
                            agent_core::presentation::markdown::MarkdownAlignment::Left => {
                                ColumnAlign::Left
                            }
                            agent_core::presentation::markdown::MarkdownAlignment::Center => {
                                ColumnAlign::Center
                            }
                            agent_core::presentation::markdown::MarkdownAlignment::Right => {
                                ColumnAlign::Right
                            }
                        })
                        .collect();
                    table.head = rows.next().unwrap_or_default();
                    table.rows = rows.collect();
                    Block::Table(table)
                }
                MarkdownBlock::Visualization { path } => Block::paragraph(vec![Inline::Link {
                    url: path,
                    title: None,
                    content: vec![Inline::text("ファイルを開く")],
                }]),
            })
            .collect(),
    )
}

fn inlines(runs: Vec<MarkdownRun>) -> Vec<Inline> {
    runs.into_iter()
        .map(|run| {
            let mut node = if let Some(source) = run.image {
                let mut image = nsh_base::text_view::Image::new(source);
                image.alt = run.text;
                Inline::Image(image)
            } else if run.code {
                Inline::Code { code: run.text }
            } else {
                Inline::text(run.text)
            };
            if run.strong {
                node = Inline::Strong {
                    content: vec![node],
                };
            }
            if run.emphasis {
                node = Inline::Emphasis {
                    content: vec![node],
                };
            }
            if run.strikethrough {
                node = Inline::Strikethrough {
                    content: vec![node],
                };
            }
            if let Some(url) = run.link {
                node = Inline::Link {
                    url,
                    title: None,
                    content: vec![node],
                };
            }
            node
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invocation_and_currency_text_keeps_the_same_gfm_meaning_as_existing_clients() {
        let text = "$fixture-review @Fixture Plugin [success] costs $5 · 日本語 👋";
        let document = document(agent_core::presentation::markdown::markdown_blocks(
            text.into(),
        ));
        assert_eq!(document.plain_text(), text);
        assert!(
            matches!(&document.blocks[..], [Block::Paragraph { content }] if content.iter().all(|inline| matches!(inline, Inline::Text { .. })))
        );
    }

    #[test]
    fn tool_output_stays_literal_in_the_native_document() {
        let output = "# output\n[open](https://example.com)\n<img src=remote>\n```\n日本語";
        let document = document(vec![
            agent_core::presentation::markdown::markdown_code_block(
                output.into(),
                None,
                Some("出力".into()),
            ),
        ]);
        assert_eq!(document.plain_text(), output);
        assert!(matches!(&document.blocks[..], [Block::CodeBlock(code)] if code.code == output));
    }
}
