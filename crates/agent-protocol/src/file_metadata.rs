//! Batched Host metadata for file references. A resolved path is returned even
//! when the entry is missing; opening an editor and copying paths do not require
//! the file to exist. Metadata never follows a network path automatically.
use serde::{Deserialize, Serialize};

pub const METADATA_BATCH_LIMIT: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileMetadataKind {
    File,
    Directory,
    Other,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileMetadata {
    pub kind: FileMetadataKind,
    pub byte_length: Option<u64>,
    pub mime_type: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilePathMetadata {
    pub path: String,
    pub metadata: Option<FileMetadata>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileMetadataEntries {
    pub entries: Vec<FilePathMetadata>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReadFileMetadata {
    pub paths: Vec<String>,
}

pub fn valid_metadata_path(path: &str) -> bool {
    path.encode_utf16().nth(512).is_none()
        && (crate::paths::is_absolute_host_path(path)
            || (!path.contains('\0')
                && (path == "~" || path.starts_with("~/") || path.starts_with("~\\"))))
}

impl ReadFileMetadata {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !(1..=METADATA_BATCH_LIMIT).contains(&self.paths.len()) {
            return Err("metadata requires between 1 and 64 paths");
        }
        if self.paths.iter().any(|path| !valid_metadata_path(path)) {
            return Err("metadata requires absolute Host paths of at most 512 UTF-16 units");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_bounds_preserve_windows_home_and_unicode_paths() {
        for path in ["/a", "~/a", "~", r"C:\a", r"\\server\share\a", r"~\a"] {
            assert!(
                ReadFileMetadata {
                    paths: vec![path.into()]
                }
                .validate()
                .is_ok()
            );
        }
        for path in ["", " ", "a", "/a\0b", "~/a\0b", "~\\a\0b"] {
            assert!(
                ReadFileMetadata {
                    paths: vec![path.into()]
                }
                .validate()
                .is_err()
            );
        }
        for count in [0, 65] {
            assert!(
                ReadFileMetadata {
                    paths: vec!["/a".into(); count]
                }
                .validate()
                .is_err()
            );
        }
        assert!(
            ReadFileMetadata {
                paths: vec![format!("/{}a", "😀".repeat(255))]
            }
            .validate()
            .is_ok()
        );
        assert!(
            ReadFileMetadata {
                paths: vec![format!("/{}ab", "😀".repeat(255))]
            }
            .validate()
            .is_err()
        );
    }

    proptest::proptest! {
        #[test]
        fn absolute_metadata_paths_enforce_utf16_length_and_reject_nuls(body in ".{0,520}") {
            let path = format!("/{body}");
            let expected = !body.contains('\0') && body.encode_utf16().count() < 512;
            proptest::prop_assert_eq!(valid_metadata_path(&path), expected);
        }
    }
}
