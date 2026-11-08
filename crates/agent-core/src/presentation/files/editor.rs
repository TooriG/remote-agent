#[cfg(feature = "bindings")]
use super::Files;
use crate::{models::FileContent, state::FileDraft};

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "bindings", derive(uniffi::Record))]
pub struct FileEditorPresentation {
    /// A truncated file shows its saved prefix while its draft remains untouched.
    pub text: String,
    pub truncated: bool,
    pub loaded_revision: Option<String>,
    pub modified: bool,
    pub can_edit: bool,
    pub can_save: bool,
    pub conflict_notice: Option<String>,
}

fn project(
    path: &str,
    file: Option<&FileContent>,
    draft: Option<&FileDraft>,
) -> FileEditorPresentation {
    let file = file.filter(|file| file.path == path);
    let saved = file.map_or("", |file| file.text.as_str());
    let text = draft.map(|draft| draft.text.as_str()).unwrap_or(saved);
    let revision = draft
        .map(|draft| draft.revision.as_str())
        .or_else(|| file.map(|file| file.revision.as_str()))
        .unwrap_or("");
    let truncated = file.is_some_and(|file| file.truncated);
    let modified = text != saved;
    let can_edit = !revision.is_empty() && !truncated;
    FileEditorPresentation {
        text: if truncated { saved } else { text }.into(),
        truncated,
        loaded_revision: file.map(|file| file.revision.clone()),
        modified,
        can_edit,
        can_save: can_edit && modified,
        conflict_notice: file.zip(draft).filter(|(file, draft)| file.revision != draft.revision)
            .map(|_| "ホストのファイルが変更されています。下書きは保持しました。再読込すると下書きを破棄します。".into()),
    }
}

#[cfg(feature = "bindings")]
#[uniffi::export]
impl Files {
    pub fn file_editor(&self, path: String) -> FileEditorPresentation {
        project(
            &path,
            self.selection.file.as_deref(),
            self.resources.file_drafts.get(&path).map(AsRef::as_ref),
        )
    }
}

impl crate::state::PaneState {
    /// The editor of `path` in this pane, its unsaved draft in `drafts`.
    pub fn file_editor(
        &self,
        drafts: &std::collections::BTreeMap<String, std::sync::Arc<FileDraft>>,
        path: &str,
    ) -> FileEditorPresentation {
        project(
            path,
            self.workspace.file.as_deref(),
            drafts.get(path).map(AsRef::as_ref),
        )
    }
}

/// A one-based Host file position becomes a native text control's UTF-16 offset.
/// Positions beyond the file clamp to the last line, including a trailing empty line.
#[cfg_attr(feature = "bindings", uniffi::export)]
pub fn file_line_start_utf16(text: String, line: u32) -> u32 {
    let mut remaining = line.saturating_sub(1);
    let mut offset = 0_u32;
    let mut start = 0;
    for unit in text.encode_utf16() {
        if remaining == 0 {
            break;
        }
        offset = offset.saturating_add(1);
        if unit == u16::from(b'\n') {
            start = offset;
            remaining -= 1;
        }
    }
    start
}

#[cfg_attr(feature = "bindings", uniffi::export)]
pub fn file_edit_prompt(path: String) -> String {
    format!("このファイルを編集してください: {path}\n変更内容: ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file() -> FileContent {
        FileContent {
            path: "/file.txt".into(),
            revision: "saved".into(),
            text: "保存済み".into(),
            size: 12,
            truncated: false,
        }
    }

    #[test]
    fn a_loaded_file_can_be_edited_but_only_changes_can_be_saved() {
        let file = file();
        let loaded = project(&file.path, Some(&file), None);
        assert_eq!(loaded.text, file.text);
        assert!(loaded.can_edit);
        assert!(!loaded.can_save);
        let draft = FileDraft {
            revision: file.revision.clone(),
            text: "変更 👋".into(),
        };
        let changed = project(&file.path, Some(&file), Some(&draft));
        assert_eq!(changed.text, draft.text);
        assert!(changed.can_save && changed.modified);
        assert!(changed.conflict_notice.is_none());
    }

    #[test]
    fn a_host_revision_change_keeps_the_draft_and_reports_the_conflict() {
        let file = file();
        let draft = FileDraft {
            revision: "previous".into(),
            text: "保持する編集".into(),
        };
        let shown = project(&file.path, Some(&file), Some(&draft));
        assert_eq!(shown.text, draft.text);
        assert_eq!(shown.loaded_revision.as_deref(), Some("saved"));
        assert!(shown.conflict_notice.is_some());
        assert!(shown.can_edit);
    }

    #[test]
    fn a_truncated_read_is_a_preview_and_cannot_overwrite_the_host() {
        let mut file = file();
        file.truncated = true;
        let draft = FileDraft {
            revision: file.revision.clone(),
            text: "未保存の全文".into(),
        };
        let shown = project(&file.path, Some(&file), Some(&draft));
        assert_eq!(shown.text, file.text);
        assert!(shown.truncated && shown.modified);
        assert!(!shown.can_edit && !shown.can_save);
    }

    #[test]
    fn an_unrelated_read_never_supplies_an_editing_revision() {
        let file = file();
        let shown = project("/other.txt", Some(&file), None);
        assert!(shown.text.is_empty());
        assert!(shown.loaded_revision.is_none());
        assert!(!shown.can_edit && !shown.can_save);
        let draft = FileDraft {
            revision: "other".into(),
            text: "復元した下書き".into(),
        };
        assert_eq!(
            project("/other.txt", Some(&file), Some(&draft)).text,
            draft.text
        );
    }

    #[test]
    fn line_positions_count_native_utf16_units_and_clamp_to_the_last_line() {
        for (text, expected) in [
            ("", [0, 0, 0, 0, 0]),
            ("🍎日本\n次\n最後", [0, 0, 5, 7, 7]),
            ("a\r\nb\r\n", [0, 0, 3, 6, 6]),
        ] {
            for (line, offset) in expected.into_iter().enumerate() {
                assert_eq!(
                    file_line_start_utf16(text.into(), line as u32),
                    offset,
                    "{text:?}, line {line}"
                );
            }
        }
    }
}
