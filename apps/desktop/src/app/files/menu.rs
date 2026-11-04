use super::*;
use agent_core::presentation::{
    editors::preferred_editor,
    files::{FileAction, FileMenuItem, append_file_mentions, file_tree_mention, file_tree_menu},
    markdown::file_path_for_clipboard,
};
use gpui_kit::component::native_menu::NativeMenu;

#[derive(Clone, PartialEq)]
pub(in crate::app) struct FileMenuTarget {
    pub path: String,
    pub relative: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub mention: String,
    pub resolve_reference: bool,
    pub lookup_path: Option<String>,
    pub root: String,
}

#[derive(Clone, PartialEq, gpui::Action)]
#[action(no_json, no_register)]
pub(in crate::app) struct FileMenuCommand {
    owner: WeakEntity<Desktop>,
    epoch: u64,
    target: FileMenuTarget,
    action: FileAction,
}

pub(in crate::app) fn run_file_menu_command(
    command: &FileMenuCommand,
    window: &mut Window,
    cx: &mut App,
) {
    cx.stop_propagation();
    let _ = command.owner.update(cx, |view, cx| {
        if view.snapshot.epoch == command.epoch {
            view.activate_file_action(&command.target, command.action, window, cx);
        }
    });
}

fn native_menu(
    items: Vec<FileMenuItem>,
    owner: &WeakEntity<Desktop>,
    epoch: u64,
    target: &FileMenuTarget,
    language: agent_core::localization::UiLanguage,
) -> NativeMenu {
    items.into_iter().fold(NativeMenu::new(), |menu, item| {
        let label = agent_core::localization::localized_ui_text(language, item.label);
        if !item.children.is_empty() {
            return menu.submenu(
                label,
                native_menu(item.children, owner, epoch, target, language),
            );
        }
        let Some(action) = item.action else {
            return menu;
        };
        let command = Box::new(FileMenuCommand {
            owner: owner.clone(),
            epoch,
            target: target.clone(),
            action,
        });
        // ElectronMenu.buildTemplate does not render ordinary item icons.
        menu.menu(label, command)
    })
}

impl Desktop {
    pub(in crate::app) fn show_file_menu(
        &self,
        items: Vec<FileMenuItem>,
        target: FileMenuTarget,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        native_menu(
            items,
            &cx.entity().downgrade(),
            self.snapshot.epoch,
            &target,
            crate::localization::language(cx),
        )
        .show(position, window, cx);
    }
    pub(in crate::app) fn show_tree_file_menu(
        &self,
        key: &agent_core::presentation::files::FileTreeKey,
        path: &str,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .file_browser
            .as_ref()
            .is_none_or(|browser| &browser.model.key != key)
        {
            return;
        }
        let path = agent_core::presentation::files::file_tree_path(&key.root, path);
        let capabilities = self.snapshot.file_open_capabilities.as_deref();
        let items = file_tree_menu(
            true,
            self.local_shell,
            capabilities.map_or(&[], |value| value.editors.as_slice()),
            capabilities.and_then(|value| value.reveal),
        );
        let relative =
            agent_core::presentation::markdown::file_workspace_relative_path(&path, &key.root)
                .unwrap_or_default();
        self.show_file_menu(
            items,
            FileMenuTarget {
                mention: file_tree_mention(&key.root, &self.snapshot.navigation.cwd, &relative),
                path,
                relative: Some(relative),
                line: None,
                column: None,
                resolve_reference: false,
                lookup_path: None,
                root: key.root.clone(),
            },
            position,
            window,
            cx,
        );
    }
    pub(super) fn activate_file_action(
        &mut self,
        target: &FileMenuTarget,
        action: FileAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if target.resolve_reference && target.lookup_path.is_some() && self.snapshot.connected {
            self.resolve_reference_action(target.clone(), super::ReferenceAction::Menu(action));
            return;
        }
        self.finish_file_action(target, action, window, cx);
    }
    pub(super) fn finish_file_action(
        &mut self,
        target: &FileMenuTarget,
        action: FileAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let editors = self
            .snapshot
            .file_open_capabilities
            .as_ref()
            .map_or(&[][..], |value| value.editors.as_slice());
        match action {
            FileAction::OpenEditor { editor } => {
                if let Some(editor) =
                    editor.or_else(|| preferred_editor(self.snapshot.preferred_editor, editors))
                {
                    self.open_in_editor(
                        target.path.clone(),
                        target.line,
                        target.column,
                        editor,
                        false,
                        cx,
                    );
                } else {
                    self.set_error(format!("No available editor can open {}", target.path));
                    cx.notify();
                }
            }
            FileAction::Reveal => self.open_in_editor(
                target.path.clone(),
                None,
                None,
                agent_protocol::editors::EditorId::FileManager,
                true,
                cx,
            ),
            FileAction::CopyRelativePath => {
                if let Some(path) = &target.relative {
                    cx.write_to_clipboard(ClipboardItem::new_string(file_path_for_clipboard(
                        &agent_core::presentation::markdown::file_position_target(
                            path,
                            target.line,
                            target.column,
                        ),
                        &target.path,
                    )));
                }
            }
            FileAction::CopyFullPath => {
                cx.write_to_clipboard(ClipboardItem::new_string(file_path_for_clipboard(
                    &agent_core::presentation::markdown::file_position_target(
                        &target.path,
                        target.line,
                        target.column,
                    ),
                    &target.path,
                )))
            }
            FileAction::CopyMention => {
                cx.write_to_clipboard(ClipboardItem::new_string(target.mention.clone()))
            }
            FileAction::AddToChat => self.add_file_mentions(&target.mention, window, cx),
        }
    }
    pub(in crate::app) fn add_file_mentions(
        &mut self,
        mentions: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = append_file_mentions(self.composer.input.read(cx).value().as_ref(), mentions);
        selection::set_composer_text(&self.composer.input, text.into(), window, cx);
    }
}
