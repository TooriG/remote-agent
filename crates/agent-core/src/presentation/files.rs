mod actions;
mod icons;
pub mod tree;
pub use actions::{FileAction, FileMenuItem, file_reference_menu, file_tree_menu};
pub use icons::desktop_file_icon;
pub use tree::{
    FileTreeAction, FileTreeKey, FileTreeRequest, FileTreeRow, FileTreeSurface, FileTreeView,
    FileTrees, file_tree_path,
};

/// T3's explorer follows the workspace, except when displaying a Host folder.
/// An absolute file outside the workspace still leaves its explorer at cwd.
pub fn desktop_file_tree_root(workspace: &str, selected: Option<&str>, directory: bool) -> String {
    selected
        .filter(|path| {
            directory && super::markdown::file_workspace_relative_path(path, workspace).is_none()
        })
        .unwrap_or(workspace)
        .into()
}

/// T3's composerTrigger.serializeComposerFileLink, shared by menus and dragging.
pub fn file_mention(path: &str) -> String {
    use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
    const DESTINATION: &percent_encoding::AsciiSet = &NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'_')
        .remove(b'.')
        .remove(b'!')
        .remove(b'~')
        .remove(b'*')
        .remove(b'\'')
        .remove(b';')
        .remove(b'/')
        .remove(b':')
        .remove(b'@')
        .remove(b'&')
        .remove(b'=')
        .remove(b'+')
        .remove(b'$')
        .remove(b',');
    let name = path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .replace('\\', "\\\\")
        .replace('[', "\\[")
        .replace(']', "\\]");
    format!("[{name}]({})", utf8_percent_encode(path, DESTINATION))
}

pub fn file_tree_mention(root: &str, workspace: &str, path: &str) -> String {
    let host_folder = super::markdown::file_workspace_relative_path(root, workspace).is_none();
    file_mention(&if host_folder {
        file_tree_path(root, path)
    } else {
        path.into()
    })
}

pub fn append_file_mentions(text: &str, mentions: &str) -> String {
    let boundary = if text.chars().last().is_some_and(|ch| !ch.is_whitespace()) {
        " "
    } else {
        ""
    };
    format!("{text}{boundary}{mentions} ")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mentions_escape_markdown_and_uri_destinations_without_losing_host_paths() {
        assert_eq!(
            file_mention("src/a [b](c)#?.rs"),
            "[a \\[b\\](c)#?.rs](src/a%20%5Bb%5D%28c%29%23%3F.rs)"
        );
        assert_eq!(
            file_mention("C:\\日本語\\a.rs"),
            "[a.rs](C:%5C%E6%97%A5%E6%9C%AC%E8%AA%9E%5Ca.rs)"
        );
        assert_eq!(
            file_tree_mention("/workspace", "/workspace", "src/a.rs"),
            "[a.rs](src/a.rs)"
        );
        assert_eq!(
            file_tree_mention("/other", "/workspace", "a.rs"),
            "[a.rs](/other/a.rs)"
        );
        assert_eq!(append_file_mentions("draft", "[a](a)"), "draft [a](a) ");
        assert_eq!(append_file_mentions("draft\n", "[a](a)"), "draft\n[a](a) ");
    }
}
