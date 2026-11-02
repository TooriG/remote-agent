use serde::{Deserialize, Serialize};

pub const FILE_SLICE_MAX_BYTES: u32 = 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReadFilePreviewMetadata {
    pub path: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilePreviewMetadata {
    pub entry: crate::models::FileEntry,
    pub mime_type: Option<String>,
    pub revision: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadFileSlice {
    pub path: String,
    pub offset: u64,
    pub length: u32,
    pub revision: String,
}

impl ReadFileSlice {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.path.is_empty() || self.path.contains('\0') {
            return Err("file path is required");
        }
        if self.length > FILE_SLICE_MAX_BYTES {
            return Err("file slice exceeds 1 MiB");
        }
        if self.offset.checked_add(u64::from(self.length)).is_none() {
            return Err("file slice overflows");
        }
        if self.revision.is_empty() || self.revision.len() > 256 {
            return Err("file revision is required");
        }
        Ok(())
    }
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileSlice {
    pub bytes: Vec<u8>,
}

impl std::fmt::Debug for FileSlice {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FileSlice")
            .field("byte_length", &self.bytes.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn slices_are_bounded_and_debug_never_exposes_file_contents() {
        let mut request = ReadFileSlice {
            path: "/file".into(),
            offset: 0,
            length: FILE_SLICE_MAX_BYTES,
            revision: "revision".into(),
        };
        assert!(request.validate().is_ok());
        request.length += 1;
        assert!(request.validate().is_err());
        request.length = 1;
        request.offset = u64::MAX;
        assert!(request.validate().is_err());
        let slice = FileSlice {
            bytes: b"private-file-content".to_vec(),
        };
        assert!(!format!("{slice:?}").contains("private-file-content"));
    }
}
