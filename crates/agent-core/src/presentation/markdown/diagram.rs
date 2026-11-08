use super::{Node, parse, without_disclosure_tags};
use mermaid_rs_renderer::{LayoutConfig, Theme, compute_layout, parse_mermaid, render_svg};

#[derive(Clone)]
pub struct MarkdownDiagram {
    pub source: String,
    pub svg: String,
    pub width: f32,
    pub height: f32,
}

/// Each Mermaid block of a message as its own fenced source, in reading order,
/// for [`markdown_diagram`]: the document its code blocks come from.
pub fn markdown_mermaid_fences(source: &str) -> Vec<String> {
    fn visit(node: &Node, source: &str, fences: &mut Vec<String>) {
        if let Node::Code(code) = node {
            if code.lang.as_deref() == Some("mermaid")
                && let Some(position) = node.position()
            {
                fences.push(source[position.start.offset..position.end.offset].into());
            }
            return;
        }
        for child in node.children().into_iter().flatten() {
            visit(child, source, fences);
        }
    }
    let source = without_disclosure_tags(source);
    if !source.contains("mermaid") {
        return Vec::new();
    }
    let mut fences = Vec::new();
    visit(&parse(&source), &source, &mut fences);
    fences
}

/// Render complete, bounded Mermaid blocks; unsupported and streaming blocks stay selectable code.
pub fn markdown_diagram(fenced_source: &str) -> Option<MarkdownDiagram> {
    if fenced_source.len() > 16 * 1024 {
        return None;
    }
    let root = parse(fenced_source);
    let children = root.children()?;
    let [Node::Code(code)] = children.as_slice() else {
        return None;
    };
    if code.lang.as_deref() != Some("mermaid") {
        return None;
    }
    let mut lines = fenced_source.lines();
    let first = lines.next()?.trim_start();
    let fence = first.chars().next()?;
    if !matches!(fence, '`' | '~') {
        return None;
    }
    let opening = first.chars().take_while(|c| *c == fence).count();
    let last = lines.last()?.trim();
    if opening < 3 || last.chars().count() < opening || !last.chars().all(|c| c == fence) {
        return None;
    }
    let parsed = parse_mermaid(&code.value).ok()?;
    if parsed.graph.nodes.len() > 80 || parsed.graph.edges.len() > 160 {
        return None;
    }
    let theme = Theme::modern();
    let config = LayoutConfig::default();
    let layout = compute_layout(&parsed.graph, &theme, &config);
    if !layout.width.is_finite()
        || !layout.height.is_finite()
        || layout.width <= 0.0
        || layout.height <= 0.0
    {
        return None;
    }
    Some(MarkdownDiagram {
        source: code.value.clone(),
        svg: outlined(&render_svg(&layout, &theme, &config))?,
        width: layout.width,
        height: layout.height,
    })
}

/// `svg` with its labels as outlines drawn with this system's fonts: the
/// renderers that draw a diagram may have no fonts for its text.
fn outlined(svg: &str) -> Option<String> {
    use std::sync::{Arc, LazyLock};
    static FONTS: LazyLock<Arc<usvg::fontdb::Database>> = LazyLock::new(|| {
        let mut fonts = usvg::fontdb::Database::new();
        fonts.load_system_fonts();
        Arc::new(fonts)
    });
    let options = usvg::Options {
        fontdb: FONTS.clone(),
        ..Default::default()
    };
    let tree = usvg::Tree::from_str(svg, &options).ok()?;
    Some(tree.to_string(&usvg::WriteOptions::default()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_complete_valid_diagrams_replace_code() {
        let diagram =
            markdown_diagram("```mermaid\nflowchart LR\nA[開始] --> B[完了]\n```").unwrap();
        assert!(diagram.svg.contains("<svg"));
        assert!(!diagram.svg.contains("<text"), "labels are outlines");
        assert!(diagram.source.contains("開始"));
        assert!(markdown_diagram("```mermaid\nflowchart LR\nA --> B").is_none());
        assert!(markdown_diagram("```rust\nflowchart LR\nA --> B\n```").is_none());
        assert!(markdown_diagram("```mermaid\ninvalidDiagram\n```").is_none());
        let message = "前\n\n- 項目\n\n  ```mermaid\n  flowchart LR\n  A --> B\n  ```\n\n```rust\nfn a() {}\n```\n\n<details><summary>図</summary>\n\n~~~mermaid\nflowchart TD\nC --> D\n~~~\n\n</details>\n\n```mermaid\nflowchart LR";
        let fences = markdown_mermaid_fences(message);
        assert_eq!(fences.len(), 3);
        assert!(
            markdown_diagram(&fences[0])
                .unwrap()
                .source
                .contains("A --> B")
        );
        assert!(
            markdown_diagram(&fences[1])
                .unwrap()
                .source
                .contains("C --> D")
        );
        assert!(
            markdown_diagram(&fences[2]).is_none(),
            "a streaming block stays code"
        );
        assert!(markdown_mermaid_fences("no diagrams").is_empty());
        assert!(
            markdown_diagram(&format!(
                "```mermaid\nflowchart LR\n{}\n```",
                (0..81).map(|n| format!("N{n};")).collect::<String>()
            ))
            .is_none()
        );
    }
}
