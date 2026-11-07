//! Native clients use the same GFM parser as GPUI's Markdown renderer.
use ::markdown::{
    ParseOptions,
    mdast::{AlignKind, Node},
};
use std::{borrow::Cow, collections::HashMap};
mod code;
#[cfg(feature = "diagrams")]
mod diagram;
#[cfg(feature = "diagrams")]
pub use diagram::{MarkdownDiagram, markdown_diagram};
mod links;
pub use links::{
    MarkdownFileChip, MarkdownFileKind, MarkdownFileReference, MarkdownFileSurface,
    WORKSPACE_BASENAME_LOOKUP_LIMIT, desktop_file_reference, file_basename, file_host_path,
    file_path_for_clipboard, file_position_target, file_workspace_relative_path,
    markdown_file_kind, markdown_file_target, needs_workspace_basename_lookup,
    pick_stripped_workspace_match, pick_workspace_basename_match, strip_repeated_workspace_prefix,
};

#[cfg_attr(feature = "bindings", uniffi::export)]
pub fn append_selection_quote(draft: String, text: String) -> String {
    let quote = text
        .split('\n')
        .map(|line| format!("> {line}"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n\n";
    if draft.is_empty() {
        quote
    } else {
        format!("{draft}\n\n{quote}")
    }
}

#[cfg(test)]
mod selection_tests {
    use super::append_selection_quote;

    #[test]
    fn selected_text_is_quoted_without_losing_blank_lines_or_existing_drafts() {
        for (draft, selected, expected) in [
            ("", "日本語 👋", "> 日本語 👋\n\n"),
            (
                "下書き",
                "first\n\nlast\n",
                "下書き\n\n> first\n> \n> last\n> \n\n",
            ),
            ("line\n", "a\r\nb", "line\n\n\n> a\r\n> b\n\n"),
            ("", "", "> \n\n"),
        ] {
            assert_eq!(
                append_selection_quote(draft.into(), selected.into()),
                expected
            );
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "bindings", derive(uniffi::Enum))]
pub enum MarkdownBlock {
    Visualization {
        path: String,
    },
    Paragraph {
        runs: Vec<MarkdownRun>,
        style: MarkdownStyle,
    },
    Table {
        source: String,
        columns: Vec<MarkdownAlignment>,
        rows: Vec<Vec<MarkdownCell>>,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "bindings", derive(uniffi::Enum))]
pub enum MarkdownAlignment {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "bindings", derive(uniffi::Record))]
pub struct MarkdownStyle {
    pub alignment: MarkdownAlignment,
    pub header: Option<u8>,
    pub marker: Option<String>,
    pub code: bool,
    pub quoted: bool,
    pub list_depth: u32,
    pub language: Option<String>,
    pub filename: Option<String>,
    pub rule: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "bindings", derive(uniffi::Record))]
pub struct MarkdownCell {
    pub runs: Vec<MarkdownRun>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "bindings", derive(uniffi::Record))]
pub struct MarkdownRun {
    pub text: String,
    pub strong: bool,
    pub emphasis: bool,
    pub strikethrough: bool,
    pub code: bool,
    pub link: Option<String>,
    pub image: Option<String>,
    pub file: Option<MarkdownFileReference>,
    pub dark_color: Option<u32>,
    pub light_color: Option<u32>,
}

fn parse(source: &str) -> Node {
    // GFM has no syntax errors; only the disabled MDX extensions can fail.
    ::markdown::to_mdast(source, &ParseOptions::gfm()).expect("GFM is infallible")
}

fn is_disclosure_tag(source: &str) -> bool {
    let Some(name) = source.strip_prefix('<') else {
        return false;
    };
    let name = name.strip_prefix('/').unwrap_or(name);
    let name = name
        .split(|c: char| c.is_ascii_whitespace() || c == '/' || c == '>')
        .next()
        .unwrap_or_default();
    name.eq_ignore_ascii_case("details") || name.eq_ignore_ascii_case("summary")
}

/// Show disclosure captions and contents on every client without exposing HTML wrappers.
/// Use parsed HTML positions so escaped text and code examples keep their literal tags.
fn without_disclosure_tags(source: &str) -> Cow<'_, str> {
    if !source
        .match_indices('<')
        .any(|(offset, _)| is_disclosure_tag(&source[offset..]))
    {
        return Cow::Borrowed(source);
    }
    fn tags(node: &Node, base: usize, ranges: &mut Vec<std::ops::Range<usize>>) {
        if let Node::Html(html) = node
            && is_disclosure_tag(&html.value)
        {
            let position = node.position().expect("parsed HTML has a source position");
            ranges.push(base + position.start.offset..base + position.end.offset);
        }
        for child in node.children().into_iter().flatten() {
            tags(child, base, ranges);
        }
    }
    fn visit(node: &Node, source: &str, ranges: &mut Vec<std::ops::Range<usize>>) {
        if let Node::Html(html) = node {
            if is_disclosure_tag(html.value.trim_start()) {
                let position = node.position().expect("parsed HTML has a source position");
                // A flow HTML node can swallow the entire disclosure, including images.
                // Read its tags as inline HTML while retaining Markdown code boundaries.
                let mut options = ParseOptions::gfm();
                options.constructs.html_flow = false;
                let fragment = ::markdown::to_mdast(
                    &source[position.start.offset..position.end.offset],
                    &options,
                )
                .expect("GFM is infallible");
                tags(&fragment, position.start.offset, ranges);
            }
            return;
        }
        for child in node.children().into_iter().flatten() {
            visit(child, source, ranges);
        }
    }
    let mut ranges = Vec::new();
    visit(&parse(source), source, &mut ranges);
    if ranges.is_empty() {
        return Cow::Borrowed(source);
    }
    let mut text = String::new();
    let mut previous = 0;
    for range in ranges {
        text.push_str(&source[previous..range.start]);
        previous = range.end;
    }
    text.push_str(&source[previous..]);
    Cow::Owned(text)
}

fn definitions(node: &Node) -> HashMap<&str, &str> {
    fn collect<'a>(node: &'a Node, result: &mut HashMap<&'a str, &'a str>) {
        if let Node::Definition(def) = node {
            result.entry(&def.identifier).or_insert(&def.url);
        }
        for child in node.children().into_iter().flatten() {
            collect(child, result);
        }
    }
    let mut result = HashMap::new();
    collect(node, &mut result);
    result
}

fn image<'a>(
    node: &'a Node,
    definition: impl Fn(&str) -> Option<&'a str>,
) -> Option<(&'a str, &'a str)> {
    match node {
        Node::Image(image) => Some((&image.url, &image.alt)),
        Node::ImageReference(image) => {
            definition(&image.identifier).map(|url| (url, image.alt.as_str()))
        }
        _ => None,
    }
}

fn image_paragraph<'a>(
    node: &'a Node,
    definition: impl Fn(&str) -> Option<&'a str> + Copy,
) -> Option<Vec<&'a str>> {
    let Node::Paragraph(paragraph) = node else {
        return None;
    };
    let mut images = Vec::new();
    for child in &paragraph.children {
        match child {
            Node::Text(text) if text.value.trim().is_empty() => {}
            Node::Break(_) => {}
            child => images.push(image(child, definition)?.0),
        }
    }
    (!images.is_empty()).then_some(images)
}

/// The images of a paragraph that contains nothing else, which a native
/// renderer draws in place of the paragraph; `None` for any other node.
pub fn markdown_image_paragraph(
    node: &Node,
    definitions: &HashMap<String, String>,
) -> Option<Vec<String>> {
    image_paragraph(node, |id| definitions.get(id).map(String::as_str))
        .map(|images| images.into_iter().map(str::to_owned).collect())
}

/// Every image in reading order, including references.
pub fn markdown_images(source: &str) -> Vec<String> {
    fn visit(node: &Node, definitions: &HashMap<&str, &str>, images: &mut Vec<String>) {
        if let Some((url, _)) = image(node, |id| definitions.get(id).copied()) {
            images.push(url.to_owned());
        }
        for child in node.children().into_iter().flatten() {
            visit(child, definitions, images);
        }
    }
    let source = without_disclosure_tags(source);
    let mut images = Vec::new();
    // Inline and reference images both start with `![`; most text needs no parse.
    if source.contains("![") {
        let root = parse(&source);
        visit(&root, &definitions(&root), &mut images);
    }
    images
}

/// Desktop draws an image only where it stands alone in a paragraph. An image
/// that shares a paragraph, heading or table cell moves verbatim into its own
/// paragraph after the enclosing top-level block, keeping its reading order.
fn markdown_with_image_paragraphs(source: &str) -> Cow<'_, str> {
    fn displaced(
        node: &Node,
        definitions: &HashMap<&str, &str>,
        ranges: &mut Vec<std::ops::Range<usize>>,
    ) {
        let definition = |id: &str| definitions.get(id).copied();
        if image_paragraph(node, definition).is_some() {
            return;
        }
        if image(node, definition).is_some() {
            let position = node.position().expect("parsed image has a source position");
            ranges.push(position.start.offset..position.end.offset);
            return;
        }
        for child in node.children().into_iter().flatten() {
            displaced(child, definitions, ranges);
        }
    }
    let source = without_disclosure_tags(source);
    if !source.contains("![") {
        return source;
    }
    let root = parse(&source);
    let definitions = definitions(&root);
    let mut text = String::new();
    let mut previous = 0;
    for block in root.children().into_iter().flatten() {
        let mut ranges = Vec::new();
        displaced(block, &definitions, &mut ranges);
        if ranges.is_empty() {
            continue;
        }
        for range in &ranges {
            text.push_str(&source[previous..range.start]);
            previous = range.end;
        }
        let end = block
            .position()
            .expect("parsed block has a source position")
            .end
            .offset;
        text.push_str(&source[previous..end]);
        previous = end;
        for range in ranges {
            text.push_str("\n\n");
            text.push_str(&source[range]);
        }
        text.push_str("\n\n");
    }
    if previous == 0 {
        return source;
    }
    text.push_str(&source[previous..]);
    Cow::Owned(text)
}

#[cfg_attr(feature = "bindings", uniffi::export)]
pub fn markdown_blocks(source: String) -> Vec<MarkdownBlock> {
    let source = without_disclosure_tags(&source);
    let root = parse(&source);
    let mut blocks = Vec::new();
    block(
        &root,
        &source,
        &definitions(&root),
        MarkdownStyle::default(),
        &mut blocks,
    );
    blocks
}

/// CSV always quotes cells, including embedded delimiters, quotes and newlines.
#[cfg_attr(feature = "bindings", uniffi::export)]
pub fn markdown_table_csv(rows: Vec<Vec<String>>) -> String {
    let mut csv = String::new();
    for (row_index, row) in rows.into_iter().enumerate() {
        if row_index > 0 {
            csv.push_str("\r\n");
        }
        for (column, cell) in row.into_iter().enumerate() {
            if column > 0 {
                csv.push(',');
            }
            csv.push('"');
            for (quote, part) in cell.split('"').enumerate() {
                if quote > 0 {
                    csv.push_str("\"\"");
                }
                csv.push_str(part);
            }
            csv.push('"');
        }
    }
    csv
}

/// Desktop Markdown and its code labels share a parse. Moving images can change
/// block boundaries, so it precedes parsing; file chips only replace inline nodes.
pub struct MarkdownPresentation<'a> {
    /// Borrowed only when the complete input is unchanged, so native caches
    /// can share their original text allocation with the rendered source.
    pub source: Cow<'a, str>,
    /// Resolves reference images in [`markdown_image_paragraph`]; empty without images.
    pub definitions: HashMap<String, String>,
    pub code_headers: HashMap<usize, String>,
    pub files: HashMap<usize, MarkdownFileChip>,
}

pub fn markdown_presentation<'a>(
    source: &'a str,
    cwd: &str,
    home: Option<&str>,
) -> MarkdownPresentation<'a> {
    let source = markdown_with_image_paragraphs(source);
    // File chips need a link or inline-code opener. Fenced labels need a fence;
    // ordinary prose and indented code can go straight to the retained renderer.
    if !source.contains(['[', '<', '`', '~']) {
        return MarkdownPresentation {
            source,
            definitions: HashMap::new(),
            code_headers: HashMap::new(),
            files: HashMap::new(),
        };
    }
    let root = parse(&source);
    let files = links::chips::file_chips(&source, &root, cwd, home);
    let definitions = if source.contains("![") {
        definitions(&root)
            .into_iter()
            .map(|(id, url)| (id.to_owned(), url.to_owned()))
            .collect()
    } else {
        HashMap::new()
    };
    MarkdownPresentation {
        source,
        definitions,
        code_headers: code_headers(&root).into_iter().collect(),
        files,
    }
}

fn code_headers(root: &Node) -> Vec<(usize, String)> {
    fn visit(node: &Node, headers: &mut Vec<(usize, String)>) {
        if let Node::Code(code) = node
            && let Some(position) = node.position()
        {
            let label = code::fence_filename(code.meta.as_deref())
                .or_else(|| code::fence_filename(code.lang.as_deref()))
                .or_else(|| code.lang.clone())
                .unwrap_or_else(|| "CODE".into());
            headers.push((position.start.offset, label));
        }
        for child in node.children().into_iter().flatten() {
            visit(child, headers);
        }
    }
    let mut headers = Vec::new();
    visit(root, &mut headers);
    headers
}

/// Render plain code without allowing its contents to become Markdown links or directives.
#[cfg_attr(feature = "bindings", uniffi::export)]
pub fn markdown_code_block(
    text: String,
    language: Option<String>,
    filename: Option<String>,
) -> MarkdownBlock {
    MarkdownBlock::Paragraph {
        runs: code::highlight(&text, language.as_deref()),
        style: MarkdownStyle {
            code: true,
            language,
            filename,
            ..Default::default()
        },
    }
}

fn inline(
    node: &Node,
    definitions: &HashMap<&str, &str>,
    mut style: MarkdownRun,
    runs: &mut Vec<MarkdownRun>,
) {
    if let Some((url, alt)) = image(node, |id| definitions.get(id).copied()) {
        style.image = Some(url.to_owned());
        style.text = alt.to_owned();
        runs.push(style);
        return;
    }
    let destination = match node {
        Node::Link(link) => Some(link.url.as_str()),
        Node::LinkReference(link) => definitions.get(link.identifier.as_str()).copied(),
        _ => None,
    };
    if let Some(destination) = destination
        && let Some(mut file) = links::file_reference(destination, links::Syntax::Link)
    {
        // Keep authored descriptions, as T3 Code does beside its file chips.
        // Native chips use that description as their label instead of an opaque basename.
        let caption = links::caption_text(node);
        if !links::is_file_label(&caption, &file) {
            file.label = caption;
        }
        runs.push(MarkdownRun {
            text: file.label.clone(),
            link: Some(destination.into()),
            file: Some(file),
            ..style
        });
        return;
    }
    match node {
        Node::Strong(_) => style.strong = true,
        Node::Emphasis(_) => style.emphasis = true,
        Node::Delete(_) => style.strikethrough = true,
        Node::Link(link) => style.link = Some(link.url.clone()),
        Node::LinkReference(link) => {
            style.link = definitions
                .get(link.identifier.as_str())
                .map(|url| (*url).to_owned())
        }
        Node::InlineCode(code) => {
            style.code = true;
            style.text = code.value.clone();
            if style.link.is_none() {
                style.file = links::file_reference(&code.value, links::Syntax::MobileCode);
                if let Some(file) = &style.file {
                    style.link = Some(code.value.clone());
                    style.text = file.label.clone();
                }
            }
        }
        Node::Text(text) => style.text = text.value.clone(),
        Node::Break(_) => style.text = "\n".into(),
        Node::Html(html) => style.text = html.value.clone(),
        Node::FootnoteReference(foot) => style.text = format!("[{}]", foot.identifier),
        _ => {}
    }
    if let Some(children) = node.children() {
        for child in children {
            inline(child, definitions, style.clone(), runs);
        }
    } else if !style.text.is_empty() {
        runs.push(style);
    }
}

fn block(
    node: &Node,
    source: &str,
    definitions: &HashMap<&str, &str>,
    mut style: MarkdownStyle,
    blocks: &mut Vec<MarkdownBlock>,
) {
    match node {
        Node::Definition(_) => return,
        Node::Table(table) => {
            let columns: Vec<_> = table
                .align
                .iter()
                .map(|alignment| match alignment {
                    AlignKind::Center => MarkdownAlignment::Center,
                    AlignKind::Right => MarkdownAlignment::Right,
                    _ => MarkdownAlignment::Left,
                })
                .collect();
            let rows = table
                .children
                .iter()
                .enumerate()
                .map(|(row_index, row)| {
                    let mut cells: Vec<_> = row
                        .children()
                        .into_iter()
                        .flatten()
                        .take(columns.len())
                        .map(|cell| {
                            let mut runs = Vec::new();
                            inline(
                                cell,
                                definitions,
                                MarkdownRun {
                                    strong: row_index == 0,
                                    ..Default::default()
                                },
                                &mut runs,
                            );
                            MarkdownCell { runs }
                        })
                        .collect();
                    cells.resize_with(columns.len(), MarkdownCell::default);
                    cells
                })
                .collect();
            let position = node.position().expect("parsed table has a source position");
            blocks.push(MarkdownBlock::Table {
                source: source[position.start.offset..position.end.offset].into(),
                columns,
                rows,
            });
            return;
        }
        Node::List(list) => {
            let depth = style.list_depth;
            for (index, child) in list.children.iter().enumerate() {
                let mut item_style = style.clone();
                item_style.list_depth = depth + 1;
                item_style.marker = Some(
                    if let Node::ListItem(item) = child
                        && let Some(checked) = item.checked
                    {
                        if checked { "☑" } else { "☐" }.into()
                    } else if list.ordered {
                        format!("{}.", u64::from(list.start.unwrap_or(1)) + index as u64)
                    } else {
                        ["•", "◦", "▪"][depth as usize % 3].into()
                    },
                );
                block(child, source, definitions, item_style, blocks);
            }
            return;
        }
        Node::Blockquote(_) => style.quoted = true,
        Node::Code(code) => {
            style.code = true;
            style.language = code.lang.clone();
            style.filename = code::fence_filename(code.meta.as_deref())
                .or_else(|| code::fence_filename(code.lang.as_deref()));
            blocks.push(MarkdownBlock::Paragraph {
                runs: code::highlight(&code.value, code.lang.as_deref()),
                style,
            });
            return;
        }
        Node::Heading(heading) => style.header = Some(heading.depth),
        Node::ThematicBreak(_) => {
            style.rule = true;
            blocks.push(MarkdownBlock::Paragraph {
                runs: vec![MarkdownRun {
                    text: "―".into(),
                    ..Default::default()
                }],
                style,
            });
            return;
        }
        _ => {}
    }
    // Read opaque JSON before Markdown unescapes Windows path separators.
    if matches!(node, Node::Paragraph(_))
        && let Some(position) = node.position()
        && let Some(path) =
            visualization_reference(&source[position.start.offset..position.end.offset])
    {
        blocks.push(MarkdownBlock::Visualization { path });
        return;
    }
    if matches!(node, Node::Paragraph(_) | Node::Heading(_) | Node::Html(_)) {
        let mut runs = Vec::new();
        inline(node, definitions, MarkdownRun::default(), &mut runs);
        blocks.push(MarkdownBlock::Paragraph { runs, style });
    } else {
        for child in node.children().into_iter().flatten() {
            block(child, source, definitions, style.clone(), blocks);
            // A list marker belongs to the first paragraph, not every continuation.
            if matches!(node, Node::ListItem(_)) {
                style.marker = None;
            }
        }
    }
}

fn visualization_reference(text: &str) -> Option<String> {
    let json = text.trim().strip_prefix("visualize")?.strip_suffix("")?;
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let path = value.get("path")?.as_str()?;
    (!path.is_empty()).then(|| path.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn table_copy_preserves_original_markdown_and_quotes_csv() {
        let original = "| **太字** *斜体* ~~削除~~ | ``a`b`` |\n| :--- | ---: |\n| [資料](https://example.com) | a\\|b |";
        let blocks = markdown_blocks(format!("before\n\n{original}\n\nafter"));
        let MarkdownBlock::Table { source, .. } = &blocks[1] else {
            panic!()
        };
        assert_eq!(source, original);
        assert_eq!(
            markdown_table_csv(vec![vec!["a,b".into(), "a\"b\nc".into()]]),
            "\"a,b\",\"a\"\"b\nc\""
        );
    }

    #[test]
    fn csv_preserves_empty_rows_and_empty_cells() {
        for (rows, expected) in [
            (vec![], ""),
            (vec![vec![]], ""),
            (vec![vec![], vec![]], "\r\n"),
            (vec![vec![], vec![""], vec![]], "\r\n\"\"\r\n"),
            (vec![vec!["", ""], vec![]], "\"\",\"\"\r\n"),
        ] {
            let rows = rows
                .into_iter()
                .map(|row| row.into_iter().map(str::to_owned).collect())
                .collect();
            assert_eq!(markdown_table_csv(rows), expected);
        }
    }

    #[test]
    fn csv_doubles_adjacent_quotes_and_keeps_unicode_and_line_endings() {
        assert_eq!(markdown_table_csv(vec![vec!["\"".into()]]), "\"\"\"\"");
        assert_eq!(
            markdown_table_csv(vec![vec!["a\"\"b".into()]]),
            "\"a\"\"\"\"b\""
        );
        assert_eq!(
            markdown_table_csv(vec![
                vec!["日,本".into(), "a\r\nb".into()],
                vec!["a\0b".into()]
            ]),
            "\"日,本\",\"a\r\nb\"\r\n\"a\0b\""
        );
    }

    #[test]
    fn fenced_metadata_nested_lists_and_rules_retain_structure() {
        let blocks = markdown_blocks("- outer\n  - inner\n\n    continuation\n\n```rust title=\"src/hello world.rs\"\nfn main() {}\n```\n\n---".into());
        let styles: Vec<_> = blocks
            .iter()
            .filter_map(|block| match block {
                MarkdownBlock::Paragraph { style, .. } => Some(style),
                _ => None,
            })
            .collect();
        assert_eq!(styles[0].list_depth, 1);
        assert_eq!(styles[1].list_depth, 2);
        assert_eq!(styles[2].list_depth, 2);
        assert!(styles[2].marker.is_none());
        assert_eq!(styles[3].language.as_deref(), Some("rust"));
        assert_eq!(styles[3].filename.as_deref(), Some("src/hello world.rs"));
        assert!(styles[4].rule);
    }

    #[test]
    fn visualize_reference_is_a_distinct_block() {
        let blocks = markdown_blocks("Before\n\nvisualize{\"path\":\"/fixture/icon-options.html\",\"mode\":\"wide\"}\n\nAfter".into());
        assert_eq!(blocks.len(), 3);
        assert_eq!(
            blocks[1],
            MarkdownBlock::Visualization {
                path: "/fixture/icon-options.html".into()
            }
        );
        for source in [
            "`visualize{\"path\":\"x.html\"}`",
            "```text\nvisualize{\"path\":\"x.html\"}\n```",
            "visualize{broken}",
            "visualize{\"path\":\"\"}",
            "visualize{\"path\":null}",
            "visualize{\"path\":\"x.html\"}",
        ] {
            assert!(matches!(
                markdown_blocks(source.into())[0],
                MarkdownBlock::Paragraph { .. }
            ));
        }
    }

    proptest::proptest! {
        #[test]
        fn visualization_paths_preserve_json_escapes(name in "[^\\x00\\r\\n]{1,32}") {
            let path = format!("C:\\fixture\\{name}.html");
            let source = format!(
                "Before\n\nvisualize{}\n\nAfter",
                serde_json::json!({"path": path}),
            );
            let blocks = markdown_blocks(source);
            proptest::prop_assert_eq!(blocks.len(), 3);
            proptest::prop_assert_eq!(
                &blocks[1],
                &MarkdownBlock::Visualization { path },
            );
        }
    }

    const TABLE: &str = include_str!("../../tests/fixtures/markdown/table.md");

    fn cell_text(cell: &MarkdownCell) -> String {
        cell.runs.iter().map(|run| run.text.as_str()).collect()
    }

    fn paragraph_text(block: &MarkdownBlock) -> String {
        let MarkdownBlock::Paragraph { runs, .. } = block else {
            panic!("expected paragraph")
        };
        runs.iter().map(|run| run.text.as_str()).collect()
    }

    /// The image paragraphs a desktop renderer finds, in reading order.
    fn image_paragraphs(source: &str) -> Vec<Vec<String>> {
        fn visit(node: &Node, definitions: &HashMap<String, String>, found: &mut Vec<Vec<String>>) {
            if let Some(images) = markdown_image_paragraph(node, definitions) {
                found.push(images);
            }
            for child in node.children().into_iter().flatten() {
                visit(child, definitions, found);
            }
        }
        let root = parse(source);
        let definitions = definitions(&root)
            .into_iter()
            .map(|(id, url)| (id.to_owned(), url.to_owned()))
            .collect();
        let mut found = Vec::new();
        visit(&root, &definitions, &mut found);
        found
    }

    #[test]
    fn desktop_images_keep_their_place_in_reading_order() {
        let standalone =
            "前\n\n![a](a.png)\n\n- 項目\n\n  ![b](b.png)\n\n![c](c.png)\n![d](d.png)\n\n後";
        assert!(matches!(
            markdown_with_image_paragraphs(standalone),
            Cow::Borrowed(_)
        ));
        assert_eq!(
            image_paragraphs(standalone),
            [vec!["a.png"], vec!["b.png"], vec!["c.png", "d.png"]]
        );
        let shared = "前 ![a](a.png) 後\n\n| x |\n|---|\n| ![b](b.png) |\n\n- 項目 ![c](c.png)\n- 次\n\n最後";
        let rendered = markdown_with_image_paragraphs(shared);
        // Each image follows the paragraph, table or list that held it.
        let order: Vec<_> = [
            "前  後",
            "![a](a.png)",
            "|  |",
            "![b](b.png)",
            "- 項目 \n- 次",
            "![c](c.png)",
            "最後",
        ]
        .iter()
        .map(|part| {
            rendered
                .find(part)
                .unwrap_or_else(|| panic!("{part} in {rendered}"))
        })
        .collect();
        assert!(order.is_sorted(), "{rendered}");
        assert_eq!(
            image_paragraphs(&rendered),
            [["a.png"], ["b.png"], ["c.png"]]
        );
        assert!(matches!(
            markdown_with_image_paragraphs(&rendered),
            Cow::Borrowed(_)
        ));
        assert!(image_paragraphs("![a](a.png) text").is_empty());
        assert!(image_paragraphs("![a](a.png) ![undefined]").is_empty());
    }

    #[test]
    fn markdown_table_preserves_reported_cells_and_surrounding_prose() {
        let blocks = markdown_blocks(format!("Before\n\n{TABLE}\nAfter"));
        assert_eq!(blocks.len(), 3);
        assert_eq!(paragraph_text(&blocks[0]), "Before");
        assert_eq!(paragraph_text(&blocks[2]), "After");
        let MarkdownBlock::Table { columns, rows, .. } = &blocks[1] else {
            panic!("table flattened into prose")
        };
        assert_eq!(columns, &[MarkdownAlignment::Left; 3]);
        insta::assert_json_snapshot!(
            rows.iter()
                .map(|row| row.iter().map(cell_text).collect::<Vec<_>>())
                .collect::<Vec<_>>(),
        );
    }

    #[test]
    fn markdown_table_preserves_empty_cells_alignment_and_inline_semantics() {
        let source = "| Left | Center | Right |\n|:---|:---:|---:|\n| **bold** and [link][ref] | | `code` |\n| | | |\n| last | escaped \\| pipe | ~~gone~~ |\n| | | |\n\n[ref]: https://example.com\n";
        let blocks = markdown_blocks(source.into());
        let MarkdownBlock::Table { columns, rows, .. } = &blocks[0] else {
            panic!("expected table")
        };
        assert_eq!(
            columns,
            &[
                MarkdownAlignment::Left,
                MarkdownAlignment::Center,
                MarkdownAlignment::Right
            ]
        );
        insta::assert_json_snapshot!(
            rows.iter()
                .map(|row| row.iter().map(cell_text).collect::<Vec<_>>())
                .collect::<Vec<_>>(),
        );
        assert!(rows[1][0].runs[0].strong);
        assert!(
            rows[0]
                .iter()
                .flat_map(|cell| &cell.runs)
                .all(|run| run.strong)
        );
        assert!(!rows[3][0].runs[0].strong);
        assert_eq!(
            rows[1][0].runs.last().unwrap().link.as_deref(),
            Some("https://example.com")
        );
        assert!(rows[1][2].runs[0].code);
        assert!(rows[3][2].runs[0].strikethrough);
    }

    #[test]
    fn markdown_table_streaming_and_non_table_source_remain_valid() {
        for (end, _) in TABLE.char_indices() {
            let blocks = markdown_blocks(TABLE[..end].into());
            for block in blocks {
                if let MarkdownBlock::Table { columns, rows, .. } = block {
                    assert!(rows.iter().all(|row| row.len() == columns.len()));
                }
            }
        }
        for (source, expected) in [
            ("No table | here".to_owned(), "No table | here".to_owned()),
            (
                format!("```text\n{TABLE}```"),
                TABLE.trim_end_matches(['\r', '\n']).to_owned(),
            ),
            (
                "| incomplete |\n| text".into(),
                "| incomplete |\n| text".into(),
            ),
        ] {
            let blocks = markdown_blocks(source);
            assert_eq!(blocks.len(), 1);
            assert_eq!(paragraph_text(&blocks[0]), expected);
        }
        // A single hyphen is already a valid GFM delimiter while streaming.
        assert!(
            matches!(markdown_blocks("| incomplete |\n| -".into()).as_slice(),
            [MarkdownBlock::Table { rows, .. }] if rows.len() == 1)
        );
        assert_eq!(markdown_blocks(format!("{TABLE}\n{TABLE}")).len(), 2);
    }
    #[test]
    fn markdown_document_preserves_shared_prose_semantics() {
        let source = include_str!("../../tests/fixtures/markdown/document.md");
        let blocks = markdown_blocks(source.into());
        let paragraphs: Vec<_> = blocks
            .iter()
            .map(|block| {
                let MarkdownBlock::Paragraph { runs, style } = block else {
                    panic!("unexpected table")
                };
                (runs, style)
            })
            .collect();
        assert_eq!(paragraphs.len(), 10);
        assert_eq!(paragraphs[0].1.header, Some(1));
        assert_eq!(paragraph_text(&blocks[1]), "前の 太字と 強調、取消、参照。");
        assert!(
            paragraphs[1]
                .0
                .iter()
                .any(|run| run.text == "強調" && run.strong && run.emphasis)
        );
        assert!(
            paragraphs[1]
                .0
                .iter()
                .any(|run| run.text == "取消" && run.strikethrough)
        );
        assert!(paragraphs[2].1.quoted);
        assert_eq!(paragraphs[3].1.marker.as_deref(), Some("3."));
        assert_eq!(paragraphs[4].1.marker.as_deref(), Some("4."));
        assert_eq!(paragraphs[5].1.marker.as_deref(), Some("☑"));
        assert_eq!(paragraphs[6].1.marker.as_deref(), Some("☐"));
        assert!(paragraphs[7].1.code);
        assert_eq!(paragraphs[7].0[0].image, None);
        assert_eq!(
            paragraphs[8].0[0].image.as_deref(),
            Some("images/example.png")
        );
        for index in [1, 9] {
            assert!(
                paragraphs[index]
                    .0
                    .iter()
                    .any(|run| run.link.as_deref() == Some("https://example.com/reference"))
            );
        }
    }

    #[test]
    fn desktop_image_extraction_borrows_image_free_markdown() {
        for source in [
            "",
            "Hello! **日本語** [link](target)",
            r"!\[escaped](image.png)",
            "# Heading\n\n> quoted\n\n- list\n\n`code`",
            "[picture]: image.png\n\n[picture]",
            "&#33;[alt](image.png)",
            "!&#91;alt](image.png)",
            TABLE,
        ] {
            let rendered = markdown_with_image_paragraphs(source);
            assert!(matches!(rendered, Cow::Borrowed(_)));
            assert_eq!(rendered, source);
            assert!(markdown_images(source).is_empty());
        }
    }

    #[test]
    fn disclosure_captions_and_images_are_visible_without_html_wrappers() {
        for separator in ["", "\n", "\n\n", "\r\n"] {
            for (open, summary, close_summary, close) in [
                ("<details>", "<summary>", "</summary>", "</details>"),
                (
                    "<DETAILS open data-note='a>b'>",
                    "<SUMMARY class='caption'>",
                    "</SUMMARY>",
                    "</DETAILS>",
                ),
            ] {
                let source = format!(
                    "Before\n\n{open}{separator}{summary}**東映の解約完了画面**{close_summary}{separator}![証拠][receipt]{separator}{close}\n\nAfter\n\n[receipt]: images/receipt.png"
                );
                let blocks = markdown_blocks(source.clone());
                let runs: Vec<_> = blocks
                    .iter()
                    .flat_map(|block| match block {
                        MarkdownBlock::Paragraph { runs, .. } => runs.as_slice(),
                        _ => panic!("unexpected block"),
                    })
                    .collect();
                assert_eq!(
                    runs.iter()
                        .filter_map(|run| run.image.as_deref())
                        .collect::<Vec<_>>(),
                    ["images/receipt.png"]
                );
                assert!(
                    runs.iter()
                        .any(|run| run.text == "東映の解約完了画面" && run.strong)
                );
                let text: String = runs.iter().map(|run| run.text.as_str()).collect();
                assert_eq!(
                    text.replace(['\r', '\n'], ""),
                    "Before東映の解約完了画面証拠After"
                );
                assert_eq!(markdown_images(&source), ["images/receipt.png"]);
                let rendered = markdown_with_image_paragraphs(&source);
                assert!(!rendered.contains("<"));
                let caption = rendered.find("**東映の解約完了画面**").unwrap();
                let image = rendered.find("![証拠][receipt]").unwrap();
                assert!(caption < image && image < rendered.find("After").unwrap());
                assert_eq!(image_paragraphs(&rendered), [["images/receipt.png"]]);
            }
        }
        let source = "<details><summary>説明だけ</summary>本文</details>";
        assert_eq!(markdown_with_image_paragraphs(source), "説明だけ本文");
        assert_eq!(
            paragraph_text(&markdown_blocks(source.into())[0]),
            "説明だけ本文"
        );
        assert_eq!(
            markdown_with_image_paragraphs("<details/>本文<summary/>"),
            "本文"
        );
    }

    #[test]
    fn disclosure_wrappers_preserve_code_and_nested_contents() {
        let source = "<details>\n<summary>外側</summary>\n```html\n<summary>![literal](code.png)</summary>\n```\n<details>\n<summary>内側</summary>\n\n![画像](nested.png)\n</details>\n</details>";
        let blocks = markdown_blocks(source.into());
        let code = blocks
            .iter()
            .find(|block| {
                matches!(block,
                    MarkdownBlock::Paragraph { style, .. } if style.code
                )
            })
            .expect("code remains code inside a disclosure");
        assert_eq!(
            paragraph_text(code),
            "<summary>![literal](code.png)</summary>"
        );
        assert!(blocks.iter().any(|block| paragraph_text(block) == "外側"));
        assert!(
            blocks
                .iter()
                .any(|block| paragraph_text(block).trim() == "内側")
        );
        assert_eq!(markdown_images(source), ["nested.png"]);
        assert_eq!(
            markdown_with_image_paragraphs(source),
            "\n外側\n```html\n<summary>![literal](code.png)</summary>\n```\n\n内側\n\n![画像](nested.png)\n\n"
        );
    }

    #[test]
    fn disclosure_tags_in_literals_and_unrelated_html_are_unchanged() {
        for source in [
            "`<details>`",
            "`<summary>`",
            "```html\n<details><summary>![literal](image.png)</summary></details>\n```",
            r"\<details>\<summary>literal\</summary>\</details>",
            "<details-extra>literal</details-extra>",
            "<summary-note>literal</summary-note>",
            "<script>const example = '<details>![literal](image.png)</details>';</script>",
            "<details unfinished='attribute",
            "<summary unfinished='attribute",
            "<div>![literal](image.png)</div>",
        ] {
            let rendered = markdown_with_image_paragraphs(source);
            assert!(matches!(rendered, Cow::Borrowed(_)), "{source}");
            assert_eq!(rendered, source);
            assert!(markdown_images(source).is_empty());
            assert_eq!(
                paragraph_text(&markdown_blocks(source.into())[0]),
                match source {
                    source if source.starts_with("```") => source
                        .strip_prefix("```html\n")
                        .unwrap()
                        .strip_suffix("\n```")
                        .unwrap(),
                    source if source.starts_with('`') => source.trim_matches('`'),
                    source if source.starts_with('\\') =>
                        "<details><summary>literal</summary></details>",
                    source => source,
                }
            );
        }
    }

    #[test]
    fn presentation_preserves_source_and_describes_file_autolinks() {
        let source = "日本語の文章🌸\n\n    plain code";
        let presentation = markdown_presentation(source, "/repo", None);
        assert_eq!(presentation.source, source);
        assert!(matches!(presentation.source, Cow::Borrowed(_)));
        assert!(std::ptr::eq(presentation.source.as_ref(), source));
        assert!(presentation.definitions.is_empty());
        assert!(presentation.code_headers.is_empty());
        let presentation = markdown_presentation("<file:///tmp/main.rs>", "/repo", None);
        assert_eq!(presentation.source, "<file:///tmp/main.rs>");
        assert!(matches!(presentation.source, Cow::Borrowed(_)));
        let chip = &presentation.files[&0];
        assert!(chip.caption.is_some());
        assert_eq!(chip.file.label, "main.rs");
        assert_eq!(chip.copy_markdown, "[file:///tmp/main.rs](/tmp/main.rs)");
    }

    #[test]
    fn transformed_presentation_owns_its_rendered_text() {
        for (source, expected) in [
            ("<summary>caption</summary>", "caption"),
            (
                "前 ![image](image.png) 後",
                "前  後\n\n![image](image.png)\n\n",
            ),
        ] {
            let presentation = markdown_presentation(source, "/repo", None);
            assert_eq!(presentation.source, expected);
            assert!(matches!(presentation.source, Cow::Owned(_)));
        }
    }

    #[test]
    fn presentation_keeps_code_labels_at_displayed_offsets() {
        let source = "[長い説明](src/first.rs) ![証拠](image.png)\n\n```rust filename=src/一.rs\nfn first() {}\n```\n\n`src/a/index.ts` and `src/b/index.ts`\n\n~~~ts title=src/second.ts\nconst second = 2;\n~~~\n\n`src/last.rs`";
        let presentation = markdown_presentation(source, "/repo", None);
        assert!(matches!(presentation.source, Cow::Owned(_)));
        assert_eq!(image_paragraphs(&presentation.source), [["image.png"]]);
        let labels: Vec<_> = presentation
            .files
            .values()
            .map(|chip| chip.file.label.as_str())
            .collect();
        assert!(labels.contains(&"index.ts · src/a"));
        assert!(labels.contains(&"index.ts · src/b"));
        assert_eq!(presentation.code_headers.len(), 2);
        assert_eq!(
            presentation.code_headers[&presentation.source.find("```rust").unwrap()],
            "src/一.rs"
        );
        assert_eq!(
            presentation.code_headers[&presentation.source.find("~~~ts").unwrap()],
            "src/second.ts"
        );
        for end in source
            .char_indices()
            .map(|(end, _)| end)
            .chain([source.len()])
        {
            let partial = markdown_presentation(&source[..end], "/repo", None);
            for offset in partial.code_headers.keys() {
                assert!(partial.source.is_char_boundary(*offset));
                assert!(partial.source[*offset..].starts_with(['`', '~']));
            }
        }
    }

    #[test]
    fn disclosure_images_remain_valid_at_every_stream_boundary() {
        let source = "[receipt]: images/receipt.png\n\n<details>\n<summary>日本語の説明🌸</summary>\n\n![証拠][receipt]\n\n</details>";
        for end in source
            .char_indices()
            .map(|(end, _)| end)
            .chain([source.len()])
        {
            let partial = &source[..end];
            let images = markdown_images(partial);
            assert!(images.len() <= 1);
            assert!(images.iter().all(|url| url == "images/receipt.png"));
            markdown_blocks(partial.into());
            markdown_presentation(partial, "/repo", None);
        }
        assert_eq!(markdown_images(source), ["images/receipt.png"]);
    }

    #[test]
    fn desktop_image_extraction_preserves_literal_image_openers() {
        for source in [
            r"\![escaped](image.png)",
            "`![inline code](image.png)`",
            "```markdown\n![fenced code](image.png)\n```",
            "![unfinished](",
            "![undefined reference]",
        ] {
            let rendered = markdown_with_image_paragraphs(source);
            assert!(matches!(rendered, Cow::Borrowed(_)));
            assert_eq!(rendered, source);
            assert!(markdown_images(source).is_empty());
        }
    }

    #[test]
    fn desktop_image_extraction_handles_every_reference_form() {
        for image in [
            "![alt](image.png)",
            "![alt][picture]",
            "![picture][]",
            "![picture]",
        ] {
            let source = format!("前 {image} 後\n\n[picture]: image.png");
            assert_eq!(
                markdown_with_image_paragraphs(&source),
                format!("前  後\n\n{image}\n\n\n\n[picture]: image.png")
            );
            assert_eq!(markdown_images(&source), ["image.png"]);
            assert_eq!(
                image_paragraphs(&markdown_presentation(&source, "", None).source),
                [["image.png"]]
            );
        }
    }

    #[test]
    fn desktop_image_extraction_preserves_source_and_reference_context() {
        let source = include_str!("../../tests/fixtures/markdown/document.md");
        assert_eq!(markdown_images(source), ["images/example.png"]);
        let rendered = markdown_with_image_paragraphs(source);
        assert!(matches!(rendered, Cow::Borrowed(_)));
        assert_eq!(image_paragraphs(&rendered), [["images/example.png"]]);
        let source =
            "![first](a.png) **![second](b.png)** [![third][image]](target)\n\n[image]: c.png";
        assert_eq!(markdown_images(source), ["a.png", "b.png", "c.png"]);
        let rendered = markdown_with_image_paragraphs(source);
        assert_eq!(
            rendered,
            " **** [](target)\n\n![first](a.png)\n\n![second](b.png)\n\n![third][image]\n\n\n\n[image]: c.png"
        );
        assert_eq!(
            image_paragraphs(&rendered),
            [["a.png"], ["b.png"], ["c.png"]]
        );
        assert_eq!(markdown_with_image_paragraphs(TABLE), TABLE);
        assert!(markdown_images(TABLE).is_empty());
    }
}
