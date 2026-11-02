//! T3's workspaceBasenameLookup.ts. These rules apply only on an explicit
//! open/copy action; background metadata and hover use the literal link target.
use super::paths;
use agent_protocol::file_tree::{EntryKind, TreeEntry};
use regex::Regex;
use std::sync::LazyLock;

pub const WORKSPACE_BASENAME_LOOKUP_LIMIT: u32 = 25;
static SEPARATORS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\\/]+").unwrap());

pub fn needs_workspace_basename_lookup(path: &str) -> bool {
    let path = path.trim();
    !matches!(path, "" | "." | "..") && !path.contains(['/', '\\'])
}

pub fn pick_workspace_basename_match(basename: &str, entries: &[TreeEntry]) -> Option<String> {
    let target = basename.trim();
    if target.is_empty() {
        return None;
    }
    let files = || entries.iter().filter(|entry| entry.kind == EntryKind::File);
    if let Some(entry) = files().find(|entry| entry_basename(&entry.path) == target) {
        return Some(entry.path.clone());
    }
    let folded = target.to_lowercase();
    let mut matches = files().filter(|entry| entry_basename(&entry.path).to_lowercase() == folded);
    let first = matches.next()?;
    matches.next().is_none().then(|| first.path.clone())
}
fn entry_basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

pub fn strip_repeated_workspace_prefix(path: &str, cwd: &str) -> Option<String> {
    if paths::absolute(path) {
        return None;
    }
    let windows = paths::windows(cwd);
    let segments: Vec<_> = SEPARATORS.split(path).collect();
    let cwd: Vec<_> = SEPARATORS
        .split(cwd)
        .filter(|part| !part.is_empty())
        .collect();
    for count in (1..=cwd.len().min(segments.len().saturating_sub(1))).rev() {
        let prefix = &cwd[cwd.len() - count..];
        if prefix.iter().zip(&segments).all(|(a, b)| {
            if windows {
                a.to_lowercase() == b.to_lowercase()
            } else {
                a == b
            }
        }) {
            let result = segments[count..].join("/");
            return (!result.is_empty()).then_some(result);
        }
    }
    None
}

pub fn pick_stripped_workspace_match(
    path: &str,
    cwd: &str,
    entries: &[TreeEntry],
) -> Option<String> {
    let key = |path: &str| {
        let path = path.replace('\\', "/");
        if paths::windows(cwd) {
            path.to_lowercase()
        } else {
            path
        }
    };
    let target = key(path);
    entries
        .iter()
        .find(|entry| entry.kind == EntryKind::File && key(&entry.path) == target)
        .map(|entry| entry.path.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn file(path: &str) -> TreeEntry {
        TreeEntry {
            path: path.into(),
            kind: EntryKind::File,
            ignored: false,
        }
    }
    #[test]
    fn basename_search_preserves_exact_match_order_and_requires_a_unique_folded_match() {
        let entries = [file("a/Main.rs"), file("b/main.rs"), file("c/main.rs")];
        assert_eq!(
            pick_workspace_basename_match("main.rs", &entries).as_deref(),
            Some("b/main.rs")
        );
        assert_eq!(pick_workspace_basename_match("MAIN.RS", &entries), None);
        assert_eq!(
            pick_workspace_basename_match("MAIN.RS", &entries[..1]).as_deref(),
            Some("a/Main.rs")
        );
        let mut directory = file("Main.rs");
        directory.kind = EntryKind::Directory;
        assert_eq!(pick_workspace_basename_match("Main.rs", &[directory]), None);
        for path in ["a.rs", " README "] {
            assert!(needs_workspace_basename_lookup(path));
        }
        for path in ["", ".", "..", "src/a.rs", r"src\a.rs"] {
            assert!(!needs_workspace_basename_lookup(path));
        }
    }
    #[test]
    fn repeated_prefixes_use_the_longest_workspace_suffix_and_windows_case_rules() {
        assert_eq!(
            strip_repeated_workspace_prefix("a/a/file.rs", "/work/a/a").as_deref(),
            Some("file.rs")
        );
        assert_eq!(
            strip_repeated_workspace_prefix("repo/app/src/a.rs", "/work/repo/app").as_deref(),
            Some("src/a.rs")
        );
        assert_eq!(
            strip_repeated_workspace_prefix("app/src/a.rs", "/work/repo/app").as_deref(),
            Some("src/a.rs")
        );
        assert_eq!(
            strip_repeated_workspace_prefix(r"REPO\\app\src/a.rs", r"C:\work\Repo\App").as_deref(),
            Some("src/a.rs")
        );
        assert_eq!(
            strip_repeated_workspace_prefix("APP/src/a.rs", "/work/app"),
            None
        );
        assert_eq!(
            strip_repeated_workspace_prefix("/app/a.rs", "/work/app"),
            None
        );
        assert_eq!(strip_repeated_workspace_prefix("app", "/work/app"), None);
        assert_eq!(strip_repeated_workspace_prefix("app/", "/work/app"), None);
        assert_eq!(
            strip_repeated_workspace_prefix("app/src/", "/work/app").as_deref(),
            Some("src/")
        );
    }
    #[test]
    fn stripped_matches_normalize_only_separators_and_windows_case() {
        let entries = [file(r"src\MAIN.rs"), file("src/other.rs")];
        assert_eq!(
            pick_stripped_workspace_match("SRC/main.rs", r"C:\work", &entries).as_deref(),
            Some(r"src\MAIN.rs")
        );
        assert_eq!(
            pick_stripped_workspace_match("SRC/main.rs", "/work", &entries),
            None
        );
        assert_eq!(
            pick_stripped_workspace_match("src/main.rs", "/work", &entries),
            None
        );
    }
}
