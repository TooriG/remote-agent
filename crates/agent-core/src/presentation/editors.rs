use agent_protocol::editors::{EditorId, FileManagerRevealKind};

pub fn preferred_editor(saved: Option<EditorId>, available: &[EditorId]) -> Option<EditorId> {
    saved
        .filter(|editor| available.contains(editor))
        .or_else(|| {
            EditorId::ALL
                .iter()
                .copied()
                .find(|editor| available.contains(editor))
        })
}

pub fn editor_label(editor: EditorId, file_manager: FileManagerRevealKind) -> &'static str {
    if editor != EditorId::FileManager {
        return editor.label();
    }
    match file_manager {
        FileManagerRevealKind::Finder => "Finder",
        FileManagerRevealKind::FileExplorer => "エクスプローラー",
        FileManagerRevealKind::Files => "ファイルマネージャー",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preference_falls_back_in_catalog_order_including_the_file_manager() {
        let available = [EditorId::FileManager, EditorId::VsCode, EditorId::Cursor];
        assert_eq!(
            preferred_editor(Some(EditorId::VsCode), &available),
            Some(EditorId::VsCode)
        );
        assert_eq!(
            preferred_editor(Some(EditorId::Zed), &available),
            Some(EditorId::Cursor)
        );
        assert_eq!(
            preferred_editor(None, &[EditorId::FileManager]),
            Some(EditorId::FileManager)
        );
        assert_eq!(preferred_editor(Some(EditorId::Cursor), &[]), None);
    }
}
