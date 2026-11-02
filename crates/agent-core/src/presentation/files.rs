/// How desktop clients open a Host file, from either a link or a file listing.
/// Mobile clients retain their native preview flows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileOpenAction {
    InApp,
    OpenWithSystem,
    Save,
}

pub fn file_open_action(path: &str) -> FileOpenAction {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let extension = name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase());
    match extension.as_deref() {
        Some(
            "mp4" | "mov" | "webm" | "mkv" | "mp3" | "wav" | "ogg" | "flac" | "pdf" | "doc"
            | "docx" | "xls" | "xlsx" | "ppt" | "pptx",
        ) => FileOpenAction::OpenWithSystem,
        Some(
            "exe" | "msi" | "msix" | "msixbundle" | "appx" | "dll" | "apk" | "aab" | "zip" | "7z"
            | "rar" | "tar" | "gz" | "bz2" | "xz" | "zst" | "dmg" | "pkg" | "deb" | "rpm" | "iso"
            | "wasm" | "bin" | "ttf" | "otf" | "woff" | "woff2",
        ) => FileOpenAction::Save,
        _ => FileOpenAction::InApp,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_and_office_documents_open_with_the_system() {
        for extension in [
            "mp4", "mov", "webm", "mkv", "mp3", "wav", "ogg", "flac", "pdf", "doc", "docx", "xls",
            "xlsx", "ppt", "pptx",
        ] {
            for path in [
                format!("~/Movies/promo.{extension}"),
                format!("C:\\Users\\host\\promo.{}", extension.to_ascii_uppercase()),
            ] {
                assert_eq!(
                    file_open_action(&path),
                    FileOpenAction::OpenWithSystem,
                    "{path}"
                );
            }
        }
    }

    #[test]
    fn installers_archives_executables_and_fonts_are_only_saved() {
        for extension in [
            "exe",
            "msi",
            "msix",
            "msixbundle",
            "appx",
            "dll",
            "apk",
            "aab",
            "zip",
            "7z",
            "rar",
            "tar",
            "gz",
            "bz2",
            "xz",
            "zst",
            "dmg",
            "pkg",
            "deb",
            "rpm",
            "iso",
            "wasm",
            "bin",
            "ttf",
            "otf",
            "woff",
            "woff2",
        ] {
            let path = format!("C:\\build\\artifact.{}", extension.to_ascii_uppercase());
            assert_eq!(file_open_action(&path), FileOpenAction::Save, "{path}");
        }
        assert_eq!(file_open_action("bundle.tar.gz"), FileOpenAction::Save);
    }

    #[test]
    fn source_images_and_unknown_files_keep_the_in_app_route() {
        for path in [
            "main.rs",
            "readme.txt",
            "README",
            "large.PNG",
            "installer.exe.md",
            "promo.mp4.txt",
            "/pdf/main.ts",
            "C:\\pdf\\README",
            "unknown.data",
            "/folder.mp4/README",
            "file.",
        ] {
            assert_eq!(file_open_action(path), FileOpenAction::InApp, "{path}");
        }
    }
}
mod preview;
pub use preview::{FilePreviewKind, file_preview_kind, media_preview_kind, preview_mime_type};
