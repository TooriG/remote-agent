use crate::localization::{UiLanguage, localized_ui_format};
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

pub fn open_editor_label(language: UiLanguage, editor: Option<EditorId>) -> String {
    match editor {
        None | Some(EditorId::FileManager) => language.text("エディターで開く").into(),
        Some(editor) => {
            localized_ui_format(language, "{0}で開く".into(), vec![editor.label().into()])
        }
    }
}

pub fn reveal_label(language: UiLanguage, kind: FileManagerRevealKind) -> &'static str {
    language.text(match kind {
        FileManagerRevealKind::Finder => "Finderで表示",
        FileManagerRevealKind::FileExplorer => "エクスプローラーで表示",
        FileManagerRevealKind::Files => "ファイルマネージャーで表示",
    })
}

pub fn editor_label(
    language: UiLanguage,
    editor: EditorId,
    file_manager: FileManagerRevealKind,
) -> &'static str {
    if editor != EditorId::FileManager {
        return editor.label();
    }
    language.text(match file_manager {
        FileManagerRevealKind::Finder => "Finder",
        FileManagerRevealKind::FileExplorer => "エクスプローラー",
        FileManagerRevealKind::Files => "ファイルマネージャー",
    })
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

    #[test]
    fn shell_menu_labels_follow_the_actual_host_capability() {
        assert_eq!(
            open_editor_label(UiLanguage::Japanese, Some(EditorId::VsCode)),
            "VS Codeで開く"
        );
        assert_eq!(
            open_editor_label(UiLanguage::Japanese, Some(EditorId::FileManager)),
            "エディターで開く"
        );
        assert_eq!(
            reveal_label(UiLanguage::Japanese, FileManagerRevealKind::FileExplorer),
            "エクスプローラーで表示"
        );
    }

    #[test]
    fn english_editor_copy_uses_host_capabilities_and_preserves_product_names() {
        assert_eq!(
            open_editor_label(UiLanguage::English, None),
            "Open in editor"
        );
        assert_eq!(
            open_editor_label(UiLanguage::English, Some(EditorId::FileManager)),
            "Open in editor"
        );
        assert_eq!(
            open_editor_label(UiLanguage::English, Some(EditorId::VsCode)),
            "Open in VS Code"
        );
        for (kind, name, reveal) in [
            (FileManagerRevealKind::Finder, "Finder", "Reveal in Finder"),
            (
                FileManagerRevealKind::FileExplorer,
                "File Explorer",
                "Reveal in File Explorer",
            ),
            (FileManagerRevealKind::Files, "Files", "Reveal in Files"),
        ] {
            assert_eq!(
                editor_label(UiLanguage::English, EditorId::FileManager, kind),
                name
            );
            assert_eq!(reveal_label(UiLanguage::English, kind), reveal);
            assert_eq!(
                editor_label(UiLanguage::English, EditorId::VsCode, kind),
                "VS Code"
            );
        }
    }
}
