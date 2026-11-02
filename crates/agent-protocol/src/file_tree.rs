//! Workspace-relative directory and path-search contracts. Filesystem checks
//! (including symbolic-link containment) belong to the authenticated Host.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    Directory,
    File,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeEntry {
    pub path: String,
    pub kind: EntryKind,
    pub ignored: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeEntries {
    pub entries: Vec<TreeEntry>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListTreeEntries {
    pub root: String,
    pub directory: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchTreeEntries {
    pub root: String,
    pub query: String,
    pub limit: u32,
    /// File references search files before applying the limit; the filer includes directories.
    pub files_only: bool,
}

pub fn valid_tree_path(path: &str) -> bool {
    !path.contains('\0')
        && !path.starts_with('/')
        && path
            .split('/')
            .all(|part| !matches!(part, "" | "." | ".." | ".git"))
}
fn validate_root(root: &str) -> Result<(), &'static str> {
    if !root.contains('\0')
        && (crate::paths::is_absolute_host_path(root) || root == "~" || root.starts_with("~/"))
    {
        Ok(())
    } else {
        Err("workspace root must be an absolute Host path")
    }
}
impl ListTreeEntries {
    pub fn validate(&self) -> Result<(), &'static str> {
        validate_root(&self.root)?;
        let windows = crate::paths::is_windows_absolute_host_path(&self.root);
        if (self.directory.is_empty() || valid_tree_path(&self.directory))
            && (!windows
                || (!self.directory.contains('\\')
                    && !crate::paths::is_absolute_host_path(&self.directory)))
        {
            Ok(())
        } else {
            Err("directory must be inside the workspace and outside .git")
        }
    }
}
impl SearchTreeEntries {
    pub fn validate(&self) -> Result<(), &'static str> {
        validate_root(&self.root)?;
        if self.query.contains('\0')
            || self.query.encode_utf16().count() > 256
            || !(1..=200).contains(&self.limit)
        {
            return Err("invalid workspace path search");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn directory_contract_rejects_traversal_and_git_internals() {
        for directory in [
            "",
            "src",
            "src/nested",
            ".github",
            "日本語/space name",
            "a\\b",
            "C:/data",
        ] {
            assert!(
                ListTreeEntries {
                    root: "/workspace".into(),
                    directory: directory.into()
                }
                .validate()
                .is_ok()
            );
        }
        for directory in [
            "/absolute",
            "..",
            "a/../b",
            "a//b",
            "a/./b",
            "a/.git/objects",
            ".git",
            "a\0b",
        ] {
            assert!(
                ListTreeEntries {
                    root: "/workspace".into(),
                    directory: directory.into()
                }
                .validate()
                .is_err(),
                "{directory}"
            );
        }
        for directory in ["a\\b", "a\\..\\escape", "C:/data"] {
            assert!(
                ListTreeEntries {
                    root: "C:/workspace".into(),
                    directory: directory.into()
                }
                .validate()
                .is_err()
            );
        }
        for root in ["", "relative", "~someone", "/null\0", "~/null\0"] {
            assert!(
                ListTreeEntries {
                    root: root.into(),
                    directory: String::new()
                }
                .validate()
                .is_err()
            );
        }
    }
    #[test]
    fn search_bounds_match_the_filer_and_count_utf16_units() {
        let mut request = SearchTreeEntries {
            root: "C:/workspace".into(),
            query: "x".repeat(256),
            limit: 200,
            files_only: false,
        };
        assert!(request.validate().is_ok());
        request.query.push('x');
        assert!(request.validate().is_err());
        request.query = "🦀".repeat(128);
        assert!(request.validate().is_ok());
        request.query.push('x');
        assert!(request.validate().is_err());
        request.query = "a\0b".into();
        assert!(request.validate().is_err());
        request.query.clear();
        for limit in [0, 201, u32::MAX] {
            request.limit = limit;
            assert!(request.validate().is_err());
        }
    }
}
