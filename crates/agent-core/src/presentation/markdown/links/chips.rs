use super::super::{MarkdownRun, definitions, inline};
use super::*;
use markdown::mdast::Node;
use std::collections::HashMap;

/// Desktop presentation attached to the original inline AST byte offset.
/// The original source remains intact for parsing and text selection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkdownFileChip {
    pub file: MarkdownFileReference,
    pub lookup_path: Option<String>,
    /// Inline caption formatting, displayed with the chip as one atomic element.
    pub caption: Option<Vec<MarkdownRun>>,
    pub copy_markdown: String,
}

pub fn desktop_file_reference(
    source: &str,
    cwd: &str,
    home: Option<&str>,
) -> Option<(MarkdownFileReference, Option<String>)> {
    let mut file = file_reference(source, Syntax::Link)?;
    let cwd = file_host_path(cwd.into(), home.map(str::to_owned));
    let (path, lookup) = resolve_path(&file.path, &cwd, home)?;
    file.path = path;
    Some((file, lookup))
}

fn resolve_path(path: &str, cwd: &str, home: Option<&str>) -> Option<(String, Option<String>)> {
    let lookup = (!paths::absolute(path) && !path.starts_with('~')).then(|| path.to_owned());
    let path = file_host_path(paths::resolve(path, cwd)?, home.map(str::to_owned));
    if agent_protocol::paths::is_network_host_path(&path) && paths::relative(&path, cwd).is_none() {
        return None;
    }
    Some((path, lookup))
}

pub(in crate::presentation::markdown) fn file_chips(
    source: &str,
    root: &Node,
    cwd: &str,
    home: Option<&str>,
) -> HashMap<usize, MarkdownFileChip> {
    let mut chips = HashMap::new();
    let definitions = definitions(root);
    let cwd = file_host_path(cwd.into(), home.map(str::to_owned));
    let resolve = |node: &Node| -> Option<MarkdownFileChip> {
        let (target, code) = match node {
            Node::Link(link) => (link.url.as_str(), None),
            Node::LinkReference(link) => (*definitions.get(link.identifier.as_str())?, None),
            Node::InlineCode(code) => (code.value.as_str(), Some(code)),
            _ => return None,
        };
        let mut file = file_reference(
            target,
            if code.is_some() {
                Syntax::DesktopCode
            } else {
                Syntax::Link
            },
        )?;
        let (path, lookup_path) = resolve_path(&file.path, &cwd, home)?;
        let label = caption_text(node);
        let show_caption = code.is_none() && !is_file_label(&label, &file);
        file.path = path;
        let position = node.position()?;
        let authored = &source[position.start.offset..position.end.offset];
        let copy_markdown = if let Some(code) = code {
            format!("`{}`", code.value)
        } else if authored.starts_with('[') && authored.contains("](") {
            authored.into()
        } else {
            let label = if label.is_empty() {
                paths::basename(&file.path)
            } else {
                &label
            };
            let label = label
                .replace('\\', "\\\\")
                .replace('[', "\\[")
                .replace(']', "\\]");
            let target = if target
                .get(..5)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("file:"))
            {
                let (path, hash) = source_parts(target)?;
                format!("{}{hash}", paths::strip_drive_slash(&path))
            } else {
                target.into()
            };
            let target = if paths::windows(&target)
                && !agent_protocol::paths::is_network_host_path(&target)
            {
                target.replace('\\', "/")
            } else {
                target
            };
            format!("[{label}]({target})")
        };
        Some(MarkdownFileChip {
            caption: show_caption.then(|| {
                let mut runs = Vec::new();
                // Preserve inline syntax from the original AST. Parsing the
                // caption alone would turn e.g. "# design" into a heading.
                let style = MarkdownRun {
                    link: Some(target.into()),
                    ..Default::default()
                };
                for child in node.children().into_iter().flatten() {
                    inline(child, &definitions, style.clone(), &mut runs);
                }
                for run in &mut runs {
                    run.link = None;
                }
                runs
            }),
            file,
            lookup_path,
            copy_markdown,
        })
    };
    fn visit(
        node: &Node,
        resolve: &impl Fn(&Node) -> Option<MarkdownFileChip>,
        chips: &mut HashMap<usize, MarkdownFileChip>,
    ) {
        if let Some(chip) = resolve(node) {
            chips.insert(node.position().unwrap().start.offset, chip);
        }
        if !matches!(node, Node::Code(_)) {
            for child in node.children().into_iter().flatten() {
                visit(child, resolve, chips);
            }
        }
    }
    visit(root, &resolve, &mut chips);
    let labels = file_labels(chips.values().map(|chip| chip.file.path.as_str()));
    for chip in chips.values_mut() {
        let parent = if chip.file.path.contains('\\') {
            labels.get(&chip.file.path.replace('\\', "/"))
        } else {
            labels.get(&chip.file.path)
        };
        chip.file.label = file_label(
            &chip.file.path,
            parent.map(String::as_str),
            chip.file.line,
            chip.file.column,
        );
    }
    chips
}

#[cfg(test)]
mod tests {
    use super::super::super::markdown_presentation;

    #[test]
    fn aliases_are_resolved_before_disambiguation_without_inventing_a_home() {
        let source = "[index.ts](src/a/index.ts) [index.ts](/repo/src/a/index.ts) [index.ts](~/src/b/index.ts)";
        let presentation = markdown_presentation(source, "/repo", Some("/home/actual"));
        let first = &presentation.files[&0];
        assert_eq!(first.file.path, "/repo/src/a/index.ts");
        assert_eq!(first.lookup_path.as_deref(), Some("src/a/index.ts"));
        assert!(first.caption.is_none());
        let absolute = &presentation.files[&source.find("[index.ts](/repo").unwrap()];
        assert!(absolute.lookup_path.is_none());
        assert!(
            presentation
                .files
                .values()
                .filter(|chip| chip.file.path.starts_with("/home/"))
                .all(|chip| chip.lookup_path.is_none())
        );
        assert!(
            presentation
                .files
                .values()
                .any(|chip| chip.file.path == "/home/actual/src/b/index.ts"
                    && chip.file.label == "index.ts · src/b")
        );
        assert!(
            presentation
                .files
                .values()
                .filter(|chip| chip.file.path == first.file.path)
                .all(|chip| chip.file.label == "index.ts · src/a")
        );
        let unresolved = markdown_presentation("[a.rs](~/a.rs)", "/Users/guessed/repo", None);
        assert_eq!(unresolved.files[&0].file.path, "~/a.rs");
        assert!(
            markdown_presentation("[a.rs](src/a.rs)", "", None)
                .files
                .is_empty()
        );
    }

    #[test]
    fn authored_and_reference_links_keep_their_distinct_clipboard_contracts() {
        let source =
            "[**説明**](<docs/a b.md> \"title\") [説明][ref] ``src/main.rs:4``\n\n[ref]: docs/a.md";
        let presentation = markdown_presentation(source, "/repo", None);
        let authored = &presentation.files[&0];
        let caption = authored.caption.as_ref().unwrap();
        assert_eq!(caption.len(), 1);
        assert_eq!(caption[0].text, "説明");
        assert!(caption[0].strong);
        assert_eq!(
            authored.copy_markdown,
            "[**説明**](<docs/a b.md> \"title\")"
        );
        let reference = &presentation.files[&source.find("[説明][ref]").unwrap()];
        assert_eq!(reference.copy_markdown, "[説明](docs/a.md)");
        let code = &presentation.files[&source.find("``").unwrap()];
        assert!(code.caption.is_none());
        assert_eq!(code.copy_markdown, "`src/main.rs:4`");
        assert_eq!(code.file.line, Some(4));
        assert_eq!(presentation.source, source);
        let caption =
            markdown_presentation("[`src/main.rs`][ref]\n\n[ref]: docs/a.md", "/repo", None);
        assert_eq!(caption.files[&0].copy_markdown, "[src/main.rs](docs/a.md)");
        let html = markdown_presentation("[<span>src/main.rs</span>](src/main.rs)", "/repo", None);
        assert!(html.files[&0].caption.is_none());
        let posix =
            markdown_presentation("[説明][ref]\n\n[ref]: /repo/docs/a\\b.rs", "/repo", None);
        assert_eq!(posix.files[&0].copy_markdown, "[説明](/repo/docs/a\\b.rs)");
        let windows =
            markdown_presentation("[説明][ref]\n\n[ref]: C:\\repo\\a.rs", r"C:\repo", None);
        assert_eq!(windows.files[&0].copy_markdown, "[説明](C:/repo/a.rs)");
    }

    #[test]
    fn captions_keep_inline_semantics_instead_of_becoming_standalone_blocks() {
        let source = "[# *設計* `src/a.rs`](docs/a.md)";
        let presentation = markdown_presentation(source, "/repo", None);
        let caption = presentation.files[&0].caption.as_ref().unwrap();
        assert_eq!(
            caption
                .iter()
                .map(|run| run.text.as_str())
                .collect::<String>(),
            "# 設計 src/a.rs"
        );
        assert!(caption.iter().any(|run| run.emphasis && run.text == "設計"));
        assert!(caption.iter().any(|run| run.code && run.text == "src/a.rs"));
        assert!(
            caption
                .iter()
                .all(|run| run.file.is_none() && run.link.is_none())
        );
    }

    #[test]
    fn nested_caption_code_is_available_but_fences_and_bare_text_are_not_chips() {
        let source = "[Check `src/a.rs`](docs/b.md) src/c.rs\n\n```text\nsrc/d.rs\n```";
        let presentation = markdown_presentation(source, "/repo", None);
        assert_eq!(presentation.files.len(), 2);
        assert_eq!(presentation.files[&0].file.path, "/repo/docs/b.md");
        assert_eq!(
            presentation.files[&source.find('`').unwrap()].file.path,
            "/repo/src/a.rs"
        );
    }
}
