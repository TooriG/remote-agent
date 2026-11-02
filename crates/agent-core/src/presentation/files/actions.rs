//! Shared menu content from T3's MarkdownFileLink and fileContextMenu.
//! Native clients render the result and the Host owns editor/reveal operations.
use crate::presentation::editors::{open_editor_label, reveal_label};
use agent_protocol::editors::{EditorId, FileManagerRevealKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileAction {
    OpenEditor { editor: Option<EditorId> },
    Reveal,
    CopyRelativePath,
    CopyFullPath,
    CopyMention,
    AddToChat,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileMenuItem {
    pub action: Option<FileAction>,
    pub label: String,
    pub children: Vec<FileMenuItem>,
}
impl FileMenuItem {
    fn action(action: FileAction, label: impl Into<String>) -> Self {
        Self {
            action: Some(action),
            label: label.into(),
            children: Vec::new(),
        }
    }
}

/// Media and browser preview entries are intentionally omitted by product choice.
pub fn file_reference_menu(
    local_shell: bool,
    preferred_editor: Option<EditorId>,
    reveal: Option<FileManagerRevealKind>,
    workspace_relative: bool,
) -> Vec<FileMenuItem> {
    let mut items = Vec::new();
    if local_shell {
        items.push(FileMenuItem::action(
            FileAction::OpenEditor {
                editor: preferred_editor,
            },
            open_editor_label(preferred_editor),
        ));
        if let Some(kind) = reveal {
            items.push(FileMenuItem::action(FileAction::Reveal, reveal_label(kind)));
        }
    }
    if workspace_relative {
        items.push(FileMenuItem::action(
            FileAction::CopyRelativePath,
            "Copy relative path",
        ));
    }
    items.push(FileMenuItem::action(
        FileAction::CopyFullPath,
        "Copy full path",
    ));
    items
}

pub fn file_tree_menu(
    has_absolute_path: bool,
    local_shell: bool,
    editors: &[EditorId],
    reveal: Option<FileManagerRevealKind>,
) -> Vec<FileMenuItem> {
    let mut items = Vec::new();
    if has_absolute_path && local_shell {
        if editors.contains(&EditorId::FileManager) {
            items.push(FileMenuItem::action(
                FileAction::OpenEditor {
                    editor: Some(EditorId::FileManager),
                },
                "Open",
            ));
            if let Some(kind) = reveal {
                items.push(FileMenuItem::action(FileAction::Reveal, reveal_label(kind)));
            }
        }
        let children = editors
            .iter()
            .copied()
            .filter(|editor| *editor != EditorId::FileManager)
            .map(|editor| {
                FileMenuItem::action(
                    FileAction::OpenEditor {
                        editor: Some(editor),
                    },
                    editor.label(),
                )
            })
            .collect::<Vec<_>>();
        if !children.is_empty() {
            items.push(FileMenuItem {
                action: None,
                label: "Open with".into(),
                children,
            });
        }
    }
    items.push(FileMenuItem::action(
        FileAction::CopyMention,
        "Copy mention",
    ));
    items.push(FileMenuItem::action(FileAction::AddToChat, "Add to chat"));
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conversation_menu_has_t3_order_and_omits_unavailable_shell_actions() {
        let menu = file_reference_menu(
            true,
            Some(EditorId::VsCode),
            Some(FileManagerRevealKind::Finder),
            true,
        );
        assert_eq!(
            menu.iter()
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>(),
            [
                "Open in VS Code",
                "Reveal in Finder",
                "Copy relative path",
                "Copy full path"
            ]
        );
        let remote = file_reference_menu(
            false,
            Some(EditorId::VsCode),
            Some(FileManagerRevealKind::Finder),
            false,
        );
        assert_eq!(
            remote,
            vec![FileMenuItem::action(
                FileAction::CopyFullPath,
                "Copy full path"
            )]
        );
        assert_eq!(
            file_reference_menu(true, None, None, true)[0].label,
            "Open in editor"
        );
    }
    #[test]
    fn tree_menu_uses_file_manager_for_open_and_excludes_it_from_the_submenu() {
        let menu = file_tree_menu(
            true,
            true,
            &[EditorId::FileManager, EditorId::Zed, EditorId::VsCode],
            Some(FileManagerRevealKind::FileExplorer),
        );
        assert_eq!(
            menu.iter()
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>(),
            [
                "Open",
                "Reveal in File Explorer",
                "Open with",
                "Copy mention",
                "Add to chat"
            ]
        );
        assert_eq!(
            menu[0].action,
            Some(FileAction::OpenEditor {
                editor: Some(EditorId::FileManager)
            })
        );
        assert_eq!(
            menu[2]
                .children
                .iter()
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>(),
            ["Zed", "VS Code"]
        );
        for (absolute, local) in [(false, true), (true, false), (false, false)] {
            assert_eq!(
                file_tree_menu(
                    absolute,
                    local,
                    &[EditorId::FileManager, EditorId::Zed],
                    Some(FileManagerRevealKind::Finder)
                )
                .len(),
                2
            );
        }
        let no_manager = file_tree_menu(
            true,
            true,
            &[EditorId::Zed],
            Some(FileManagerRevealKind::Finder),
        );
        assert_eq!(no_manager[0].label, "Open with");
        assert_eq!(file_tree_menu(true, true, &[], None).len(), 2);
    }
}
