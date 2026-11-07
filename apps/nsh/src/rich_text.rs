//! Core supplies Markdown semantics; TextKit owns native selection, layout and menus.
use crate::Connection;
use agent_core::{
    panels::PanelScope,
    presentation::markdown::{MarkdownAlignment, MarkdownBlock, MarkdownRun, MarkdownStyle},
    state::{DraftKey, Intent},
};
use nsh_core::prelude::*;
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
    },
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

pub(crate) fn content(
    blocks: impl Fn() -> Vec<MarkdownBlock> + 'static,
    secondary: impl Fn() -> bool + Copy + 'static,
) -> nsh_core::element::View {
    let connection = use_context::<Connection>().unwrap();
    let target = computed(move || {
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
    });
    let content = computed(move || parts(blocks()));
    view_parts(move || content.get(), target, secondary)
}

fn view_parts(
    parts: impl Fn() -> Vec<Part> + 'static,
    target: Memo<String>,
    secondary: impl Fn() -> bool + Copy + 'static,
) -> nsh_core::element::View {
    let connection = use_context::<Connection>().unwrap();
    let files = use_context::<crate::files::Browser>().unwrap();
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
                let is_code = move || part.with(|(_, part)| matches!(part, Part::Code { .. }));
                let wrap = signal(false);
                let message = move || {
                    native("bex.messageText")
                        .prop("data", move || {
                            part.with(|(_, part)| {
                        let (Part::Text(blocks) | Part::Code { blocks, .. }) = part else {
                            return Value::Null;
                        };
                        json!({"blocks": blocks, "secondary": secondary(), "target": target.get(),
                        "sideChat": connection.pane.is_none()})
                    })
                        })
                        .on_native_event(move |event: &NativeEvent| {
                            let Some(text) = event.data["text"].as_str().filter(|s| !s.is_empty())
                            else {
                                return;
                            };
                            let Some(target) = event.data["target"]
                                .as_str()
                                .and_then(|s| serde_json::from_str::<Target>(s).ok())
                            else {
                                return;
                            };
                            match event.data["action"].as_str() {
                                Some("quote") => {
                                    let visible = connection.host.get_untracked() == target.host
                                        && connection.pane == target.pane
                                        && connection.publication.with_untracked(|p| {
                                            p.composer.controls.navigation.draft_key == target.draft
                                        });
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
                                Some("sideChat") if target.pane.is_none() => {
                                    connection.application.with(|app| {
                                        app.open_side_chat(
                                            PanelScope {
                                                host: target.host,
                                                storage: target.storage,
                                                draft: target.draft,
                                            },
                                            target.cwd,
                                            target.provider,
                                            text.into(),
                                        );
                                    })
                                }
                                Some("link") => {
                                    files.open(
                                        text.into(),
                                        target.cwd,
                                        target.home,
                                        target.host,
                                        connection.client.get(),
                                    );
                                }
                                _ => {}
                            }
                        })
                        .min_h(px(20.0))
                };
                view()
                    .w_full()
                    .gap_2()
                    // A code block sits in the desktop client's frame, its language and
                    // copy action at its top right.
                    .style_when(
                        move || part.with(|(_, part)| matches!(part, Part::Code { .. })),
                        |s| {
                            s.px_4()
                                .pt_2()
                                .pb_4()
                                .rounded(px(8.0))
                                .border_1()
                                .border_color(crate::ui::colors::BORDER)
                                .bg(crate::ui::colors::SURFACE)
                        },
                    )
                    .child(when(
                        move || part.with(|(_, part)| matches!(part, Part::Code { .. })),
                        move || {
                            view()
                                .flex_row()
                                .items_center()
                                .justify_end()
                                .gap_2()
                                .child(
                                    text(move || {
                                        part.with(|(_, part)| {
                                            if let Part::Code { label, .. } = part {
                                                label.clone()
                                            } else {
                                                String::new()
                                            }
                                        })
                                    })
                                    .max_w(px(220.0))
                                    .number_of_lines(1)
                                    .text_size(px(12.0)),
                                )
                                .child(crate::ui::text_button(
                                    move || {
                                        SharedString::from(if wrap.get() {
                                            "折り返し解除"
                                        } else {
                                            "折り返し"
                                        })
                                    },
                                    move || wrap.update(|wrap| *wrap = !*wrap),
                                ))
                                .child(crate::ui::icon_button(
                                    crate::ui::Icon::COPY,
                                    "コードをコピー",
                                    move || {
                                        part.with_untracked(|(_, part)| {
                                            if let Part::Code { source, .. } = part {
                                                set_clipboard(source.clone());
                                            }
                                        })
                                    },
                                ))
                        },
                    ))
                    .child(when(
                        move || !is_code() || wrap.get(),
                        move || message().w_full(),
                    ))
                    // An unwrapped code block keeps its lines and scrolls sideways.
                    .child(when(
                        move || is_code() && !wrap.get(),
                        move || scroll().horizontal().w_full().child(message()),
                    ))
                    .into_element()
            }
            Part::Image(_) => crate::media::image_view(
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
            Part::Link(_) => crate::ui::text_button("ファイルを開く", move || {
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
            Part::Table { .. } => table(part, target, secondary).into_element(),
        },
    ))
}

/// A table in the desktop client's frame: ruled cells, the header row raised, its copy
/// actions below.
fn table(
    part: ReadSignal<(usize, Part)>,
    target: Memo<String>,
    secondary: impl Fn() -> bool + Copy + 'static,
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
                                        move || cell.with(|(_, parts)| parts.clone()),
                                        target,
                                        secondary,
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
                .child(crate::ui::text_button(
                    move || {
                        SharedString::from(if compact.get() {
                            "セルを折り返す"
                        } else {
                            "セルをコンパクトに表示"
                        })
                    },
                    move || compact.update(|compact| *compact = !*compact),
                ))
                .child(crate::ui::text_button("Markdownをコピー", move || {
                    copy(true)
                }))
                .child(crate::ui::text_button("CSVをコピー", move || {
                    copy(false)
                })),
        )
}

fn paragraph(runs: Vec<MarkdownRun>, style: &MarkdownStyle) -> Value {
    json!({
        "header": style.header, "marker": style.marker, "code": style.code,
        "quoted": style.quoted, "depth": style.list_depth,
        "alignment": match style.alignment { MarkdownAlignment::Left => "left", MarkdownAlignment::Center => "center", MarkdownAlignment::Right => "right" },
        "runs": runs.into_iter().map(|run| json!({
            "text": run.text, "strong": run.strong, "emphasis": run.emphasis,
            "strikethrough": run.strikethrough, "code": run.code, "link": run.link,
            "darkColor": run.dark_color, "lightColor": run.light_color,
        })).collect::<Vec<_>>()
    })
}

fn flush(text: &mut Vec<Value>, parts: &mut Vec<Part>) {
    if !text.is_empty() {
        parts.push(Part::Text(std::mem::take(text)));
    }
}

fn parts(blocks: Vec<MarkdownBlock>) -> Vec<Part> {
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
                result.push(Part::Code {
                    label: style
                        .filename
                        .clone()
                        .or_else(|| style.language.clone())
                        .unwrap_or_else(|| "CODE".into()),
                    source: runs.iter().map(|run| run.text.as_str()).collect(),
                    blocks: vec![paragraph(runs, &style)],
                });
            }
            MarkdownBlock::Paragraph { runs, style } => {
                let mut content = Vec::new();
                for run in runs {
                    if let Some(image) = run.image {
                        if !content.is_empty() {
                            text.push(paragraph(std::mem::take(&mut content), &style));
                        }
                        flush(&mut text, &mut result);
                        result.push(Part::Image(image));
                    } else {
                        content.push(run);
                    }
                }
                if !content.is_empty() {
                    text.push(paragraph(content, &style));
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
                                    parts(vec![MarkdownBlock::Paragraph {
                                        runs: cell.runs,
                                        style: MarkdownStyle {
                                            alignment: columns
                                                .get(column)
                                                .copied()
                                                .unwrap_or_default(),
                                            ..Default::default()
                                        },
                                    }])
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
        )];
        let expected: Value = serde_json::from_str(include_str!(
            "../apple/Tests/BexNshNativeTests/Fixtures/message.json"
        ))
        .unwrap();
        assert_eq!(
            json!({"blocks": blocks, "secondary": false, "target": "opaque-native-menu-target", "sideChat": true}),
            expected
        );
    }

    #[test]
    fn native_parts_preserve_media_code_rules_and_table_copy_sources_in_order() {
        let source = "前文 **強調**\n\n![画像](https://example.invalid/image.png)\n\n```rust title=main.rs\nlet 日本語 = 1;\n```\n\n---\n\n|名前|値|\n|:---|---:|\n|日本語|a,b|";
        let parts = parts(agent_core::presentation::markdown::markdown_blocks(
            source.into(),
        ));
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
    fn invocation_and_currency_text_keeps_the_same_gfm_meaning_as_existing_clients() {
        let text = "$fixture-review @Fixture Plugin [success] costs $5 · 日本語 👋";
        let parts = parts(agent_core::presentation::markdown::markdown_blocks(
            text.into(),
        ));
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
        let parts = parts(vec![
            agent_core::presentation::markdown::markdown_code_block(
                output.into(),
                None,
                Some("出力".into()),
            ),
        ]);
        let [
            Part::Code {
                blocks,
                source,
                label,
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
