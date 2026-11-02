//! T3 WorkspaceEntries directory semantics, including ignored children and
//! symlink cycles. No directory outside the authenticated workspace is followed.
use super::*;
use agent_protocol::file_tree::{EntryKind, ListTreeEntries, TreeEntries, TreeEntry};
use std::process::Stdio;
mod search;
pub(super) use search::SearchIndexes;

pub(super) async fn list(request: ListTreeEntries) -> Result<TreeEntries> {
    request.validate().map_err(anyhow::Error::msg)?;
    let (root, entries) = tokio::task::spawn_blocking(move || list_children(request)).await??;
    let mut entries = entries;
    // Git's stdin form preserves whitespace, newlines and pathspec-like names.
    // Its classification is optional outside repositories and through symlinks.
    for chunk in entries.chunks_mut(1000) {
        let paths: Vec<_> = chunk.iter().map(|entry| entry.path.as_str()).collect();
        let Some(ignored) = ignored(&root, &paths).await else {
            break;
        };
        for entry in chunk {
            entry.ignored = ignored.contains(&entry.path);
        }
    }
    Ok(TreeEntries {
        entries,
        truncated: false,
    })
}

fn list_children(request: ListTreeEntries) -> Result<(PathBuf, Vec<TreeEntry>)> {
    let root = dunce::canonicalize(absolute_path(&request.root)?)?;
    let authored = root.join(&request.directory);
    let directory = dunce::canonicalize(&authored)?;
    ensure_workspace_directory(&root, &directory)?;
    let mut ancestors = vec![root.clone()];
    let mut parent = root.clone();
    for part in request.directory.split('/').filter(|part| !part.is_empty()) {
        parent.push(part);
        ancestors.push(dunce::canonicalize(&parent)?);
    }
    let mut entries = Vec::new();
    for child in fs::read_dir(&directory)? {
        let child = child?;
        let name = child.file_name();
        if name == ".git" {
            continue;
        }
        let Some(name) = name.to_str() else {
            continue;
        };
        let kind = child.file_type()?;
        let kind = if kind.is_dir() {
            Some(EntryKind::Directory)
        } else if kind.is_file() {
            Some(EntryKind::File)
        } else if kind.is_symlink() {
            symlink_kind(&root, &child.path(), &ancestors)
        } else {
            None
        };
        if let Some(kind) = kind {
            entries.push(TreeEntry {
                path: if request.directory.is_empty() {
                    name.into()
                } else {
                    format!("{}/{name}", request.directory)
                },
                kind,
                ignored: false,
            });
        }
    }
    Ok((root, entries))
}

fn ensure_workspace_directory(root: &Path, directory: &Path) -> Result<()> {
    if !root.is_dir() || !directory.is_dir() || !inside_workspace(root, directory) {
        return Err(anyhow!(
            "Directory must be inside the workspace and outside .git."
        ));
    }
    Ok(())
}
fn inside_workspace(root: &Path, path: &Path) -> bool {
    path.strip_prefix(root)
        .is_ok_and(|relative| !relative.components().any(|part| part.as_os_str() == ".git"))
}
fn symlink_kind(root: &Path, path: &Path, ancestors: &[PathBuf]) -> Option<EntryKind> {
    let target = dunce::canonicalize(path).ok()?;
    if !inside_workspace(root, &target) {
        return None;
    }
    let stat = fs::metadata(&target).ok()?;
    if stat.is_file() {
        return Some(EntryKind::File);
    }
    (stat.is_dir() && !ancestors.iter().any(|parent| parent.starts_with(&target)))
        .then_some(EntryKind::Directory)
}

async fn ignored(root: &Path, paths: &[&str]) -> Option<std::collections::HashSet<String>> {
    let mut child = tokio::process::Command::new("git")
        .args([
            "-c",
            "core.fsmonitor=false",
            "check-ignore",
            "-z",
            "--stdin",
        ])
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .ok()?;
    let mut input = child.stdin.take()?;
    let output = child.stdout.take()?;
    let paths = format!("{}\0", paths.join("\0"));
    let write =
        async move { tokio::io::AsyncWriteExt::write_all(&mut input, paths.as_bytes()).await };
    const MAX_OUTPUT: u64 = 16 * 1024 * 1024;
    let read = async move {
        use tokio::io::AsyncReadExt;
        let mut bytes = Vec::new();
        output.take(MAX_OUTPUT + 1).read_to_end(&mut bytes).await?;
        std::io::Result::Ok(bytes)
    };
    let result = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(write, read, child.wait())
    })
    .await
    .ok()?;
    result.0.ok()?;
    let output = result.1.ok()?;
    if output.len() as u64 > MAX_OUTPUT || !matches!(result.2.ok()?.code(), Some(0 | 1)) {
        return None;
    }
    Some(
        output
            .split(|byte| *byte == 0)
            .filter(|path| !path.is_empty())
            .filter_map(|path| std::str::from_utf8(path).ok())
            .map(str::to_owned)
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn directory_reads_include_ignored_children_and_exclude_git_internals() {
        let root = tempfile::tempdir().unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "--quiet"])
                .arg(root.path())
                .status()
                .unwrap()
                .success()
        );
        fs::write(root.path().join(".gitignore"), "ignored/\nignored.txt\n").unwrap();
        fs::create_dir(root.path().join("ignored")).unwrap();
        fs::write(root.path().join("ignored/child.txt"), "ignored").unwrap();
        fs::write(root.path().join("ignored.txt"), "ignored").unwrap();
        fs::write(root.path().join("kept.txt"), "kept").unwrap();
        let request = ListTreeEntries {
            root: root.path().to_str().unwrap().into(),
            directory: String::new(),
        };
        let listing = list(request.clone()).await.unwrap();
        assert!(!listing.truncated);
        assert!(!listing.entries.iter().any(|entry| entry.path == ".git"));
        assert!(listing.entries.iter().any(|entry| entry.path == "ignored"
            && entry.ignored
            && entry.kind == EntryKind::Directory));
        assert!(
            listing
                .entries
                .iter()
                .any(|entry| entry.path == "ignored.txt" && entry.ignored)
        );
        assert!(
            listing
                .entries
                .iter()
                .any(|entry| entry.path == "kept.txt" && !entry.ignored)
        );
        let nested = list(ListTreeEntries {
            directory: "ignored".into(),
            ..request
        })
        .await
        .unwrap();
        assert_eq!(
            nested.entries,
            vec![TreeEntry {
                path: "ignored/child.txt".into(),
                kind: EntryKind::File,
                ignored: true
            }]
        );
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn symlinks_cannot_escape_enter_git_or_cycle_to_an_ancestor() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("real")).unwrap();
        fs::create_dir(root.path().join(".git")).unwrap();
        fs::write(root.path().join("real/file"), "file").unwrap();
        symlink(root.path().join("real"), root.path().join("linked")).unwrap();
        symlink(root.path(), root.path().join("cycle")).unwrap();
        symlink(outside.path(), root.path().join("escape")).unwrap();
        symlink(root.path().join(".git"), root.path().join("git-link")).unwrap();
        symlink(root.path(), root.path().join("real/parent")).unwrap();
        let request = ListTreeEntries {
            root: root.path().to_str().unwrap().into(),
            directory: String::new(),
        };
        let listing = list(request.clone()).await.unwrap();
        assert_eq!(listing.entries.len(), 2);
        let nested = list(ListTreeEntries {
            directory: "linked".into(),
            ..request.clone()
        })
        .await
        .unwrap();
        assert_eq!(
            nested
                .entries
                .iter()
                .map(|entry| entry.path.as_str())
                .collect::<Vec<_>>(),
            ["linked/file"]
        );
        for directory in ["escape", "git-link", ".git", "../escape"] {
            assert!(
                list(ListTreeEntries {
                    directory: directory.into(),
                    ..request.clone()
                })
                .await
                .is_err()
            );
        }
    }
}
