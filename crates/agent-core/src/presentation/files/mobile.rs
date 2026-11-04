//! T3 mobile's features/threads/fileChipMenu.ts. Apps supply native menus/share sheets.
use crate::presentation::markdown::{
    MarkdownFileSurface, file_host_path, file_path_for_clipboard, file_position_target,
    file_workspace_relative_path, markdown_file_target,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "bindings", derive(uniffi::Enum))]
pub enum MobileFileAction {
    CopyFullPath,
    CopyRelativePath,
    Open,
    SaveOrShare,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "bindings", derive(uniffi::Record))]
pub struct MobileFileMenuItem {
    pub action: MobileFileAction,
    pub label: String,
    pub path: String,
    pub mime_type: Option<String>,
}

#[cfg_attr(feature = "bindings", uniffi::export)]
pub fn mobile_file_menu(
    source: String,
    cwd: String,
    home: Option<String>,
) -> Vec<MobileFileMenuItem> {
    let cwd = file_host_path(cwd, home.clone());
    let Some(file) = markdown_file_target(source, cwd.clone(), MarkdownFileSurface::Mobile) else {
        return Vec::new();
    };
    let path = file_host_path(file.path, home);
    let relative = file_workspace_relative_path(&path, &cwd);
    let full = agent_protocol::paths::is_absolute_host_path(&path);
    if !full && relative.is_none() {
        return Vec::new();
    }
    let target = file_position_target(&path, file.line, file.column);
    let mut items = Vec::new();
    if full {
        items.push(MobileFileMenuItem {
            mime_type: None,
            action: MobileFileAction::CopyFullPath,
            label: "フルパスをコピー".into(),
            path: file_path_for_clipboard(&target, &path),
        });
    }
    if let Some(relative) = relative {
        items.push(MobileFileMenuItem {
            mime_type: None,
            action: MobileFileAction::CopyRelativePath,
            label: "相対パスをコピー".into(),
            path: file_path_for_clipboard(
                &file_position_target(&relative, file.line, file.column),
                &path,
            ),
        });
    }
    items.push(MobileFileMenuItem {
        mime_type: None,
        action: MobileFileAction::Open,
        label: "ファイルビューアで開く".into(),
        path: path.clone(),
    });
    // Source metadata uses the literal final extension, including a position suffix.
    if let Some(mime_type) = share_mime_type(&target) {
        items.push(MobileFileMenuItem {
            mime_type: Some(mime_type.into()),
            action: MobileFileAction::SaveOrShare,
            label: "保存・共有".into(),
            path: target,
        });
    }
    items
}

#[cfg_attr(feature = "bindings", uniffi::export)]
pub fn mobile_file_open_request(
    source: String,
    cwd: String,
    home: Option<String>,
) -> Option<crate::state::operations::ResolveFileReference> {
    use crate::presentation::markdown::needs_workspace_basename_lookup;
    let root = file_host_path(cwd, home.clone());
    let authored = markdown_file_target(source.clone(), String::new(), MarkdownFileSurface::Mobile);
    let file = markdown_file_target(source.clone(), root.clone(), MarkdownFileSurface::Mobile)?;
    let path = file_host_path(file.path, home);
    // Absolute/home references never search by basename. Relative references do.
    let authored_absolute = authored.is_some();
    let relative = file_workspace_relative_path(&path, &root);
    let lookup_path =
        relative.filter(|path| !authored_absolute && needs_workspace_basename_lookup(path));
    Some(crate::state::operations::ResolveFileReference {
        surface: MarkdownFileSurface::Mobile,
        path,
        root,
        lookup_path,
        inspect_directory: true,
    })
}

fn share_mime_type(path: &str) -> Option<&'static str> {
    let name = super::super::markdown::file_basename(path);
    let extension = name.rsplit_once('.')?.1.to_ascii_lowercase();
    SHARE_MIME_TYPES
        .iter()
        .find(|(suffix, _)| *suffix == extension)
        .map(|(_, mime)| *mime)
}

const SHARE_MIME_TYPES: &[(&str, &str)] = &[
    ("aac", "audio/aac"),
    ("aiff", "audio/aiff"),
    ("avi", "video/x-msvideo"),
    ("avif", "image/avif"),
    ("bmp", "image/bmp"),
    ("db", "application/vnd.sqlite3"),
    (
        "docx",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    ),
    ("epub", "application/epub+zip"),
    ("flac", "audio/flac"),
    ("gif", "image/gif"),
    ("gz", "application/gzip"),
    ("heic", "image/heic"),
    ("heif", "image/heif"),
    ("htm", "text/html"),
    ("html", "text/html"),
    ("ico", "image/x-icon"),
    ("ipynb", "application/x-ipynb+json"),
    ("jar", "application/java-archive"),
    ("jpeg", "image/jpeg"),
    ("jpg", "image/jpeg"),
    ("m4a", "audio/mp4"),
    ("m4v", "video/mp4"),
    ("mkv", "video/x-matroska"),
    ("mov", "video/quicktime"),
    ("mp3", "audio/mpeg"),
    ("mp4", "video/mp4"),
    ("odp", "application/vnd.oasis.opendocument.presentation"),
    ("ods", "application/vnd.oasis.opendocument.spreadsheet"),
    ("odt", "application/vnd.oasis.opendocument.text"),
    ("oga", "audio/ogg"),
    ("ogg", "audio/ogg"),
    ("ogv", "video/ogg"),
    ("opus", "audio/ogg"),
    ("otf", "font/otf"),
    ("pdf", "application/pdf"),
    ("png", "image/png"),
    (
        "pptx",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    ),
    ("rtf", "application/rtf"),
    ("sqlite", "application/vnd.sqlite3"),
    ("sqlite3", "application/vnd.sqlite3"),
    ("svg", "image/svg+xml"),
    ("tar", "application/x-tar"),
    ("tgz", "application/gzip"),
    ("tif", "image/tiff"),
    ("tiff", "image/tiff"),
    ("ttf", "font/ttf"),
    ("wav", "audio/wav"),
    ("weba", "audio/webm"),
    ("webm", "video/webm"),
    ("webm-audio", "audio/webm"),
    ("webp", "image/webp"),
    ("woff", "font/woff"),
    ("woff2", "font/woff2"),
    (
        "xlsx",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    ),
    ("zip", "application/zip"),
];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_authored_relative_basenames_request_workspace_lookup() {
        let request = mobile_file_open_request("main.rs:12".into(), "/repo".into(), None).unwrap();
        assert_eq!(request.path, "/repo/main.rs");
        assert_eq!(request.root, "/repo");
        assert_eq!(request.lookup_path.as_deref(), Some("main.rs"));
        assert!(request.inspect_directory);
        for source in [
            "src/main.rs",
            "/repo/main.rs",
            "file:///repo/main.rs",
            "~/main.rs",
        ] {
            let request =
                mobile_file_open_request(source.into(), "/repo".into(), Some("/actual".into()))
                    .unwrap();
            assert_eq!(request.lookup_path, None, "{source}");
        }
        let request =
            mobile_file_open_request("a.rs".into(), "~/repo".into(), Some("/actual".into()))
                .unwrap();
        assert_eq!(request.path, "/actual/repo/a.rs");
        assert!(mobile_file_open_request("a.rs".into(), String::new(), None).is_none());
    }

    #[test]
    fn menu_order_scope_positions_windows_and_explicit_sharing_follow_mobile() {
        let menu = mobile_file_menu("src/a.rs:12:3".into(), "/repo".into(), None);
        assert_eq!(
            menu.iter().map(|item| item.action).collect::<Vec<_>>(),
            [
                MobileFileAction::CopyFullPath,
                MobileFileAction::CopyRelativePath,
                MobileFileAction::Open
            ]
        );
        assert_eq!(menu[0].path, "/repo/src/a.rs:12:3");
        assert_eq!(menu[1].path, "src/a.rs:12:3");
        let menu = mobile_file_menu("C:/other/a.pdf".into(), "C:\\repo".into(), None);
        assert_eq!(menu[0].path, "C:\\other\\a.pdf");
        assert_eq!(
            menu.iter().map(|item| item.action).collect::<Vec<_>>(),
            [
                MobileFileAction::CopyFullPath,
                MobileFileAction::Open,
                MobileFileAction::SaveOrShare
            ]
        );
        assert!(
            !mobile_file_menu("a.pdf:12".into(), "/repo".into(), None)
                .iter()
                .any(|item| item.action == MobileFileAction::SaveOrShare)
        );
        let menu = mobile_file_menu(
            "~/a.png".into(),
            "/repo".into(),
            Some("/actual/home".into()),
        );
        assert_eq!(menu[0].path, "/actual/home/a.png");
        assert!(mobile_file_menu("a.rs".into(), "".into(), None).is_empty());
    }
}
