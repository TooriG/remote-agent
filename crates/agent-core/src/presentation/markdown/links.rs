use super::{MarkdownBlock, MarkdownRun, definitions};
use markdown::mdast::Node;
use percent_encoding::percent_decode_str;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "bindings", derive(uniffi::Enum))]
pub enum MarkdownFileKind {
    Markdown,
    Code,
    Image,
    Document,
    Folder,
    File,
}

/// Classify a Host filename without interpreting or resolving a link.
#[cfg_attr(feature = "bindings", uniffi::export)]
pub fn markdown_file_kind(path: &str) -> MarkdownFileKind {
    let basename = path
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default();
    let extension = basename
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase());
    match extension.as_deref() {
        Some("md" | "mdx" | "markdown") => MarkdownFileKind::Markdown,
        Some(
            "rs" | "swift" | "kt" | "kts" | "ts" | "tsx" | "js" | "jsx" | "py" | "go" | "c" | "h"
            | "cpp" | "hpp" | "cs" | "java" | "rb" | "sh" | "bash" | "zsh" | "nix" | "json"
            | "toml" | "yaml" | "yml" | "xml" | "html" | "css" | "sql" | "vue" | "svelte" | "lock",
        ) => MarkdownFileKind::Code,
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "heic" | "avif") => {
            MarkdownFileKind::Image
        }
        Some("pdf" | "txt" | "csv" | "tsv" | "docx" | "xlsx") => MarkdownFileKind::Document,
        _ if path.ends_with(['/', '\\']) => MarkdownFileKind::Folder,
        _ => MarkdownFileKind::File,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "bindings", derive(uniffi::Record))]
pub struct MarkdownFileReference {
    pub path: String,
    pub label: String,
    pub kind: MarkdownFileKind,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

fn position(path: &str) -> (&str, Option<u32>, Option<u32>) {
    let Some((prefix, last)) = path.rsplit_once(':') else {
        return (path, None, None);
    };
    let Some(last) = last.parse::<u32>().ok().filter(|value| *value > 0) else {
        return (path, None, None);
    };
    if let Some((path, line)) = prefix.rsplit_once(':')
        && let Some(line) = line.parse::<u32>().ok().filter(|value| *value > 0)
    {
        return (path, Some(line), Some(last));
    }
    (prefix, Some(last), None)
}

pub(super) fn file_reference(source: &str, implicit: bool) -> Option<MarkdownFileReference> {
    if source.is_empty() || source.chars().any(char::is_control) || source.starts_with('#') {
        return None;
    }
    if implicit && source.chars().any(char::is_whitespace) {
        return None;
    }
    let windows = source.as_bytes().get(1) == Some(&b':')
        && source.as_bytes()[0].is_ascii_alphabetic()
        && matches!(source.as_bytes().get(2), Some(b'/' | b'\\'));
    let (source_path, hash) = source
        .split_once('#')
        .map_or((source, None), |(path, hash)| (path, Some(hash)));
    let source_path = source_path.split('?').next()?;
    let (source_path, mut line, column) = position(source_path);
    if let Some(hash_line) = hash
        .and_then(|hash| hash.strip_prefix('L'))
        .and_then(|line| line.split('-').next())
        .and_then(|line| line.parse::<u32>().ok())
        .filter(|line| *line > 0)
    {
        line = Some(hash_line);
    }
    let path = if source_path.starts_with("file:") {
        let url = url::Url::parse(source_path).ok()?;
        if url
            .host_str()
            .is_some_and(|host| !host.is_empty() && host != "localhost")
        {
            return None;
        }
        percent_decode_str(url.path())
            .decode_utf8()
            .ok()?
            .into_owned()
    } else {
        // A URI with any other scheme is never a workspace reference.
        if !windows
            && let Ok(url) = url::Url::parse(source_path)
            && !url.scheme().is_empty()
        {
            return None;
        }
        percent_decode_str(source_path)
            .decode_utf8()
            .ok()?
            .into_owned()
    }
    .replace('\\', "/");
    if path.is_empty() || path.chars().any(char::is_control) {
        return None;
    }
    let basename = path.trim_end_matches('/').rsplit('/').next()?;
    let kind = markdown_file_kind(&path);
    let explicit = path.contains('/') || line.is_some() || source.starts_with("file:");
    let known_name = matches!(
        basename,
        "Makefile" | "Dockerfile" | "LICENSE" | "AGENTS.md" | "README" | "justfile" | ".gitignore"
    );
    if !explicit && (implicit || (kind == MarkdownFileKind::File && !known_name)) {
        return None;
    }
    if implicit && !path.contains('/') && kind == MarkdownFileKind::File && !known_name {
        return None;
    }
    if implicit
        && let Some(first) = path.split('/').next()
        && first.rsplit_once('.').is_some_and(|(_, suffix)| {
            matches!(suffix, "com" | "org" | "net" | "io" | "dev" | "app" | "ai")
        })
    {
        return None;
    }
    let label = format!(
        "{basename}{}{}",
        line.map(|line| format!(":{line}")).unwrap_or_default(),
        column
            .map(|column| format!(":{column}"))
            .unwrap_or_default()
    );
    Some(MarkdownFileReference {
        path,
        label,
        kind,
        line,
        column,
    })
}

pub(super) fn is_file_label(label: &str, file: &MarkdownFileReference) -> bool {
    let label = label.trim().replace('\\', "/");
    label == file.label
        || label == file.path
        || file_reference(&label, false)
            .is_some_and(|candidate| candidate.path == file.path || candidate.label == file.label)
}

/// Resolve workspace-relative links without interpreting Host-owned home paths.
#[cfg_attr(feature = "bindings", uniffi::export)]
pub fn markdown_file_target(source: String, cwd: String) -> Option<MarkdownFileReference> {
    let mut file = file_reference(&source, false)?;
    if file.path.starts_with("~/") {
        return Some(file);
    }
    let cwd = cwd
        .replace('\\', "/")
        .replace('%', "%25")
        .replace('#', "%23")
        .replace('?', "%3F");
    let windows =
        cwd.as_bytes().get(1) == Some(&b':') || file.path.as_bytes().get(1) == Some(&b':');
    let base = if cwd.starts_with('/') {
        format!("file://{}/", cwd.trim_end_matches('/'))
    } else if windows {
        format!("file:///{}/", cwd.trim_end_matches('/'))
    } else {
        return None;
    };
    let path = file
        .path
        .replace('%', "%25")
        .replace('#', "%23")
        .replace('?', "%3F");
    let path = if path.as_bytes().get(1) == Some(&b':') {
        format!("/{path}")
    } else {
        path
    };
    let target = url::Url::parse(&base).ok()?.join(&path).ok()?;
    if target.scheme() != "file" || target.host_str().is_some_and(|host| !host.is_empty()) {
        return None;
    }
    file.path = percent_decode_str(target.path())
        .decode_utf8()
        .ok()?
        .into_owned();
    if windows && file.path.as_bytes().get(2) == Some(&b':') {
        file.path.remove(0);
    }
    Some(file)
}

fn chip_markdown(file: &MarkdownFileReference, target: &str) -> String {
    let fence = "`".repeat(file.label.chars().filter(|char| *char == '`').count() + 1);
    let target = target.replace('<', "%3C").replace('>', "%3E");
    format!("[{fence}{}{fence}](<{target}>)", file.label)
}

pub(super) fn file_reference_edits(
    source: &str,
    root: &Node,
) -> Vec<(std::ops::Range<usize>, String)> {
    fn visit(
        node: &Node,
        definitions: &std::collections::HashMap<&str, &str>,
        source: &str,
        edits: &mut Vec<(
            std::ops::Range<usize>,
            String,
            MarkdownFileReference,
            String,
        )>,
    ) {
        let destination = match node {
            Node::Link(link) => Some((link.url.as_str(), false)),
            Node::LinkReference(link) => definitions
                .get(link.identifier.as_str())
                .map(|url| (*url, false)),
            Node::InlineCode(code) => Some((code.value.as_str(), true)),
            Node::Code(_) => return,
            _ => None,
        };
        if let Some((target, implicit)) = destination {
            if let Some(file) = file_reference(target, implicit)
                && let Some(position) = node.position()
            {
                let range = position.start.offset..position.end.offset;
                let original = &source[range.clone()];
                let mut runs = Vec::new();
                for child in node.children().into_iter().flatten() {
                    super::inline(child, definitions, MarkdownRun::default(), &mut runs);
                }
                let label: String = runs.iter().map(|run| run.text.as_str()).collect();
                let prefix = if implicit || is_file_label(&label, &file) {
                    String::new()
                } else {
                    format!("{original} ")
                };
                edits.push((range, prefix, file, target.into()));
            }
            return;
        }
        for child in node.children().into_iter().flatten() {
            visit(child, definitions, source, edits);
        }
    }
    let mut edits = Vec::new();
    visit(root, &definitions(root), source, &mut edits);
    let labels = file_labels(edits.iter().map(|(_, _, file, _)| file.clone()));
    edits
        .into_iter()
        .map(|(range, prefix, mut file, target)| {
            relabel(&mut file, &labels);
            (range, format!("{prefix}{}", chip_markdown(&file, &target)))
        })
        .collect()
}

fn file_labels(
    files: impl Iterator<Item = MarkdownFileReference>,
) -> std::collections::HashMap<String, String> {
    let mut paths = std::collections::HashMap::<String, std::collections::HashSet<String>>::new();
    for file in files {
        paths
            .entry(
                file.path
                    .trim_end_matches('/')
                    .rsplit('/')
                    .next()
                    .unwrap_or_default()
                    .into(),
            )
            .or_default()
            .insert(file.path);
    }
    let mut labels = std::collections::HashMap::new();
    for paths in paths.into_values().filter(|paths| paths.len() > 1) {
        for path in &paths {
            let segments: Vec<_> = path.split('/').collect();
            for depth in 2..=segments.len() {
                let suffix = segments[segments.len() - depth..].join("/");
                if paths.iter().filter(|path| path.ends_with(&suffix)).count() == 1 {
                    labels.insert(path.clone(), suffix);
                    break;
                }
            }
        }
    }
    labels
}

fn relabel(file: &mut MarkdownFileReference, labels: &std::collections::HashMap<String, String>) {
    if let Some(label) = labels.get(&file.path) {
        file.label = format!(
            "{label}{}{}",
            file.line.map(|line| format!(":{line}")).unwrap_or_default(),
            file.column
                .map(|column| format!(":{column}"))
                .unwrap_or_default()
        );
    }
}

pub(super) fn disambiguate(blocks: &mut [MarkdownBlock]) {
    fn runs(block: &mut MarkdownBlock) -> Vec<&mut MarkdownRun> {
        match block {
            MarkdownBlock::Paragraph { runs, .. } => runs.iter_mut().collect(),
            MarkdownBlock::Table { rows, .. } => rows
                .iter_mut()
                .flatten()
                .flat_map(|cell| &mut cell.runs)
                .collect(),
            _ => Vec::new(),
        }
    }
    let labels = file_labels(
        blocks
            .iter_mut()
            .flat_map(runs)
            .filter_map(|run| run.file.clone()),
    );
    for run in blocks.iter_mut().flat_map(runs) {
        if let Some(file) = &mut run.file {
            relabel(file, &labels);
            run.text = file.label.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[rstest::rstest]
    #[case("src/main.rs", "src/main.rs", None, None)]
    #[case("src/main.rs:0", "src/main.rs:0", None, None)]
    #[case("src/main.rs:4294967296", "src/main.rs:4294967296", None, None)]
    #[case("src/main.rs:12", "src/main.rs", Some(12), None)]
    #[case("src/main.rs:12:3", "src/main.rs", Some(12), Some(3))]
    #[case("src/main.rs:0:3", "src/main.rs:0", Some(3), None)]
    #[case("src/main.rs:12:0", "src/main.rs:12:0", None, None)]
    fn positions_require_positive_u32_values(
        #[case] source: &str,
        #[case] path: &str,
        #[case] line: Option<u32>,
        #[case] column: Option<u32>,
    ) {
        assert_eq!(position(source), (path, line, column));
    }

    #[test]
    fn filename_positions_are_not_uri_schemes_or_web_hosts() {
        let file = markdown_file_target("Makefile:12:3".into(), "/workspace".into()).unwrap();
        assert_eq!(file.path, "/workspace/Makefile");
        assert_eq!((file.line, file.column), (Some(12), Some(3)));
        assert_eq!(
            markdown_file_target("src/main.rs".into(), "/workspace/100% done#?".into())
                .unwrap()
                .path,
            "/workspace/100% done#?/src/main.rs"
        );
        for target in [
            "example.com/docs",
            "example.dev/src/main.rs",
            "https://example.com/a.rs",
            "mailto:a@example.com",
            "javascript:alert(1)",
        ] {
            assert!(file_reference(target, true).is_none(), "{target}");
        }
    }
    #[rstest::rstest]
    #[case("src/main.rs:12:3", "/workspace/src/main.rs", Some(12), Some(3))]
    #[case("../docs/概要.md#L7-L10", "/docs/概要.md", Some(7), None)]
    #[case("file:///tmp/hello%20world.md", "/tmp/hello world.md", None, None)]
    #[case("/tmp/a%23b.rs", "/tmp/a#b.rs", None, None)]
    #[case("~/Movies/bex-promo-ja.mp4", "~/Movies/bex-promo-ja.mp4", None, None)]
    #[case("~/Documents/", "~/Documents/", None, None)]
    #[case("~/概要%20a.md:12:3", "~/概要 a.md", Some(12), Some(3))]
    fn host_targets(
        #[case] source: &str,
        #[case] path: &str,
        #[case] line: Option<u32>,
        #[case] column: Option<u32>,
    ) {
        let reference = markdown_file_target(source.into(), "/workspace".into()).unwrap();
        assert_eq!(reference.path, path);
        assert_eq!((reference.line, reference.column), (line, column));
    }
    #[test]
    fn windows_host_paths_are_not_client_paths() {
        let file = markdown_file_target("src\\main.rs:8:2".into(), "C:\\workspace".into()).unwrap();
        assert_eq!(file.path, "C:/workspace/src/main.rs");
        assert_eq!((file.line, file.column), (Some(8), Some(2)));
        assert_eq!(
            markdown_file_target("D:\\docs\\a.md".into(), "C:\\workspace".into())
                .unwrap()
                .path,
            "D:/docs/a.md"
        );
    }
    #[test]
    fn home_paths_do_not_need_a_workspace_or_use_the_client_home() {
        for cwd in ["", "/workspace", "C:\\workspace"] {
            for path in ["~/", "~/Movies/video.mp4", "~/docs/../notes.md"] {
                let file = markdown_file_target(path.into(), cwd.into()).unwrap();
                assert_eq!(file.path, path);
            }
        }
        let blocks = super::super::markdown_blocks("`~/Movies/bex-promo-ja.mp4`".into());
        let MarkdownBlock::Paragraph { runs, .. } = &blocks[0] else {
            panic!()
        };
        assert_eq!(
            runs[0].file.as_ref().unwrap().path,
            "~/Movies/bex-promo-ja.mp4"
        );
        assert_eq!(
            markdown_file_target("./~/notes.md".into(), "/workspace".into())
                .unwrap()
                .path,
            "/workspace/~/notes.md"
        );
    }
    #[test]
    fn tilde_operators_and_versions_stay_inline_code() {
        let source = "`~` 演算子と semver `~1.2`、ホームは `~/Documents/`";
        let blocks = super::super::markdown_blocks(source.into());
        let MarkdownBlock::Paragraph { runs, .. } = &blocks[0] else {
            panic!()
        };
        let files: Vec<_> = runs.iter().filter_map(|run| run.file.as_ref()).collect();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "~/Documents/");
        for text in ["~", "~1.2"] {
            assert!(
                runs.iter()
                    .any(|run| run.text == text && run.code && run.file.is_none())
            );
        }
        assert!(markdown_file_target("~".into(), "/workspace".into()).is_none());
        assert!(
            super::super::markdown_presentation(source)
                .source
                .contains("`~` 演算子と semver `~1.2`")
        );
    }

    #[rstest::rstest]
    #[case("/workspace/readme.MD", MarkdownFileKind::Markdown)]
    #[case(r"C:\host\src\main.RS", MarkdownFileKind::Code)]
    #[case("~/Pictures/photo.PNG", MarkdownFileKind::Image)]
    #[case("notes.txt", MarkdownFileKind::Document)]
    #[case("~/Documents/", MarkdownFileKind::Folder)]
    #[case("~", MarkdownFileKind::File)]
    #[case("notes%2Emd", MarkdownFileKind::File)]
    fn kinds_classify_filenames_without_link_resolution(
        #[case] path: &str,
        #[case] kind: MarkdownFileKind,
    ) {
        assert_eq!(markdown_file_kind(path), kind);
    }
    #[rstest::rstest]
    #[case("https://example.com/a.rs")]
    #[case("javascript:alert(1)")]
    #[case("mailto:a@example.com")]
    #[case("file://other-host/private.md")]
    #[case("example.com")]
    #[case("gpt-6.1")]
    #[case("#section")]
    fn other_links_are_not_files(#[case] source: &str) {
        assert!(file_reference(source, false).is_none());
        assert!(markdown_file_target(source.into(), "/workspace".into()).is_none());
    }
    #[test]
    fn explanations_inline_paths_and_code_keep_distinct_meanings() {
        let source = "[設計資料](docs/overview.md) and `src/main.rs:4` and `README.md`\n\n```rs\nsrc/main.rs\n```";
        let blocks = super::super::markdown_blocks(source.into());
        let MarkdownBlock::Paragraph { runs, .. } = &blocks[0] else {
            panic!()
        };
        assert_eq!(
            runs.iter().map(|run| run.text.as_str()).collect::<String>(),
            "設計資料 overview.md and main.rs:4 and README.md"
        );
        assert_eq!(runs.iter().filter(|run| run.file.is_some()).count(), 2);
        assert!(
            matches!(&blocks[1], MarkdownBlock::Paragraph { runs, .. } if runs.iter().all(|run| run.file.is_none()))
        );
        let displayed = super::super::markdown_presentation(source).source;
        assert!(
            displayed.contains("[設計資料](docs/overview.md) [`overview.md`](<docs/overview.md>)")
        );
        assert!(displayed.contains("```rs\nsrc/main.rs\n```"));
    }
    #[test]
    fn same_names_get_the_shortest_distinct_parent() {
        let blocks = super::super::markdown_blocks(
            "[index.ts](src/a/index.ts) [index.ts](src/b/index.ts) [index.ts](src/a/index.ts)"
                .into(),
        );
        let MarkdownBlock::Paragraph { runs, .. } = &blocks[0] else {
            panic!()
        };
        let labels: Vec<_> = runs
            .iter()
            .filter_map(|run| run.file.as_ref().map(|file| file.label.as_str()))
            .collect();
        assert_eq!(labels, ["a/index.ts", "b/index.ts", "a/index.ts"]);
    }
    proptest::proptest! {
        #[test]
        fn unicode_paths_round_trip(name in "[a-zあ-ん]{1,24}", line in 1u32..5000, column in 1u32..2000) {
            let source = format!("src/{name}.rs:{line}:{column}");
            let file = markdown_file_target(source.clone(), "/workspace".into()).unwrap();
            proptest::prop_assert_eq!(file.path, format!("/workspace/src/{name}.rs"));
            proptest::prop_assert_eq!((file.line, file.column), (Some(line), Some(column)));
            let markdown = super::super::markdown_presentation(&format!("`{source}`")).source;
            proptest::prop_assert!(markdown.contains(&source));
        }
    }
}
