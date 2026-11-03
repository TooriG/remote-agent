pub(super) mod chips;
mod detection;
pub use chips::{MarkdownFileChip, desktop_file_reference};
mod lookup;
mod paths;
pub use lookup::{
    WORKSPACE_BASENAME_LOOKUP_LIMIT, needs_workspace_basename_lookup,
    pick_stripped_workspace_match, pick_workspace_basename_match, strip_repeated_workspace_prefix,
};
pub use paths::basename as file_basename;

pub(super) use detection::Syntax;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "bindings", derive(uniffi::Enum))]
pub enum MarkdownFileSurface {
    Desktop,
    Mobile,
}

pub fn file_workspace_relative_path(path: &str, workspace_root: &str) -> Option<String> {
    paths::relative(path, workspace_root)
}

pub fn file_path_for_clipboard(path: &str, host_path: &str) -> String {
    if paths::windows(host_path) {
        path.replace('/', "\\")
    } else {
        path.into()
    }
}

pub fn file_position_target(path: &str, line: Option<u32>, column: Option<u32>) -> String {
    match line {
        Some(line) => format!(
            "{path}:{line}{}",
            column
                .map(|column| format!(":{column}"))
                .unwrap_or_default()
        ),
        None => path.into(),
    }
}

/// Expand only an authenticated Host home. Workspace paths never imply a home.
#[cfg_attr(feature = "bindings", uniffi::export)]
pub fn file_host_path(path: String, home: Option<String>) -> String {
    let Some(home) = home.filter(|home| paths::absolute(home)) else {
        return path;
    };
    if path == "~" {
        return home;
    }
    let suffix = path.strip_prefix("~/").or_else(|| {
        paths::windows(&home)
            .then(|| path.strip_prefix("~\\"))
            .flatten()
    });
    suffix
        .and_then(|suffix| {
            let suffix = if paths::windows(&home) {
                suffix.trim_start_matches(['/', '\\'])
            } else {
                suffix.trim_start_matches('/')
            };
            paths::resolve(suffix, &home)
        })
        .unwrap_or(path)
}

pub(super) fn file_reference(source: &str, syntax: Syntax) -> Option<MarkdownFileReference> {
    let candidate = detection::candidate(source, syntax)?;
    let source = candidate.trim();
    let source = source
        .strip_prefix('<')
        .and_then(|source| source.strip_suffix('>'))
        .unwrap_or(source);
    if source.is_empty() || source.starts_with('#') || source.starts_with("//") {
        return None;
    }
    let (path, hash) = source_parts(source)?;
    let path = paths::decode(path.trim());
    let path = paths::strip_drive_slash(&path);
    let hash = paths::decode(hash.trim());
    if path.is_empty() || detection::external_scheme(path) {
        return None;
    }
    let (file_path, line, column) = paths::position(path, &hash);
    if !detection::file_shape(file_path, path) {
        return None;
    }
    let kind = markdown_file_kind(file_path);
    let mut file = MarkdownFileReference {
        path: file_path.into(),
        label: String::new(),
        kind,
        line,
        column,
    };
    file.label = file_label(&file, None);
    Some(file)
}

fn source_parts(source: &str) -> Option<(String, String)> {
    if source
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("file:"))
    {
        let url = url::Url::parse(source).ok()?;
        let host = url
            .host_str()
            .filter(|host| !host.is_empty() && !host.eq_ignore_ascii_case("localhost"));
        let path = host.map_or_else(
            || url.path().to_owned(),
            |host| format!("\\\\{host}{}", url.path().replace('/', "\\")),
        );
        return Some((
            path,
            url.fragment()
                .map_or_else(String::new, |hash| format!("#{hash}")),
        ));
    }
    let (path, hash) = source
        .split_once('#')
        .map_or((source, String::new()), |(path, hash)| {
            (path, format!("#{hash}"))
        });
    Some((path.split('?').next().unwrap_or_default().into(), hash))
}

pub(super) fn is_file_label(label: &str, file: &MarkdownFileReference) -> bool {
    let (label_path, line, column) = paths::position(label.trim(), "");
    if line.is_some_and(|line| Some(line) != file.line)
        || column.is_some_and(|column| Some(column) != file.column)
    {
        return false;
    }
    let normalize = |path: &str| {
        let path = path.replace('\\', "/");
        let path = path
            .strip_prefix("./")
            .unwrap_or(&path)
            .trim_end_matches('/');
        if paths::windows(&file.path) {
            path.to_lowercase()
        } else {
            path.into()
        }
    };
    let label = normalize(label_path);
    let destination = normalize(&file.path);
    label.is_empty() || destination == label || destination.ends_with(&format!("/{label}"))
}

/// Home expansion is performed only by the connected Host, never inferred from cwd.
#[cfg_attr(feature = "bindings", uniffi::export)]
pub fn markdown_file_target(
    source: String,
    cwd: String,
    surface: MarkdownFileSurface,
) -> Option<MarkdownFileReference> {
    let mut file = file_reference(&source, Syntax::Link)?;
    file.path = paths::resolve(&file.path, &cwd)?;
    if surface == MarkdownFileSurface::Desktop
        && agent_protocol::paths::is_network_host_path(&file.path)
        && paths::relative(&file.path, &cwd).is_none()
    {
        return None;
    }
    Some(file)
}

fn file_label(file: &MarkdownFileReference, parent: Option<&str>) -> String {
    let mut parts = vec![paths::basename(&file.path).to_owned()];
    if let Some(parent) = parent {
        parts.push(parent.into());
    }
    if let Some(line) = file.line {
        parts.push(format!(
            "L{line}{}",
            file.column
                .map(|column| format!(":C{column}"))
                .unwrap_or_default()
        ));
    }
    parts.join(" · ")
}

fn file_labels(
    files: impl Iterator<Item = MarkdownFileReference>,
) -> std::collections::HashMap<String, String> {
    let mut groups = std::collections::HashMap::<String, std::collections::HashSet<String>>::new();
    for file in files {
        groups
            .entry(paths::basename(&file.path).into())
            .or_default()
            .insert(file.path.replace('\\', "/"));
    }
    let mut labels = std::collections::HashMap::new();
    for group in groups.into_values().filter(|paths| paths.len() > 1) {
        let parents: Vec<_> = group
            .iter()
            .map(|path| {
                let mut segments: Vec<_> = path
                    .split('/')
                    .filter(|segment| !segment.is_empty())
                    .collect();
                segments.pop();
                (path, segments)
            })
            .collect();
        for (path, segments) in &parents {
            if segments.is_empty() {
                continue;
            }
            let unique_depth = (1..=segments.len())
                .find(|depth| {
                    let candidate = &segments[segments.len() - depth..];
                    !parents.iter().any(|(other, other_segments)| {
                        other != path
                            && other_segments[other_segments.len().saturating_sub(*depth)..]
                                == *candidate
                    })
                })
                .unwrap_or(segments.len());
            let depth = unique_depth.max(2).min(segments.len());
            labels.insert(
                (*path).clone(),
                segments[segments.len() - depth..].join("/"),
            );
        }
    }
    labels
}

fn relabel(file: &mut MarkdownFileReference, labels: &std::collections::HashMap<String, String>) {
    file.label = file_label(
        file,
        labels
            .get(&file.path.replace('\\', "/"))
            .map(String::as_str),
    );
}

#[cfg(test)]
mod tests;
