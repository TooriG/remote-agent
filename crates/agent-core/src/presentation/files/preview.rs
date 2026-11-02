//! File viewer classification from T3's shared/filePreview.ts and mediaPreview.ts.
//! Callers pass literal Host filenames and metadata, never Markdown destinations.
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "bindings", derive(uniffi::Enum))]
pub enum FilePreviewKind {
    Image,
    Video,
    Audio,
    Pdf,
    Html,
    Markdown,
    Mermaid,
    Font,
    Archive,
    Sqlite,
    Docx,
    Xlsx,
    Pptx,
    Odt,
    Ods,
    Odp,
    Rtf,
    Epub,
    Ipynb,
    Text,
    Unsupported,
}

const OFFICE: &[(&str, &str, FilePreviewKind)] = &[
    (
        "docx",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        FilePreviewKind::Docx,
    ),
    (
        "xlsx",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        FilePreviewKind::Xlsx,
    ),
    (
        "pptx",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        FilePreviewKind::Pptx,
    ),
    (
        "odt",
        "application/vnd.oasis.opendocument.text",
        FilePreviewKind::Odt,
    ),
    (
        "ods",
        "application/vnd.oasis.opendocument.spreadsheet",
        FilePreviewKind::Ods,
    ),
    (
        "odp",
        "application/vnd.oasis.opendocument.presentation",
        FilePreviewKind::Odp,
    ),
    ("rtf", "application/rtf", FilePreviewKind::Rtf),
    ("epub", "application/epub+zip", FilePreviewKind::Epub),
    ("ipynb", "application/x-ipynb+json", FilePreviewKind::Ipynb),
];
const MEDIA: &[(&str, &str)] = &[
    ("avif", "image/avif"),
    ("bmp", "image/bmp"),
    ("heic", "image/heic"),
    ("heif", "image/heif"),
    ("gif", "image/gif"),
    ("ico", "image/x-icon"),
    ("jpeg", "image/jpeg"),
    ("jpg", "image/jpeg"),
    ("png", "image/png"),
    ("svg", "image/svg+xml"),
    ("tif", "image/tiff"),
    ("tiff", "image/tiff"),
    ("webp", "image/webp"),
    ("avi", "video/x-msvideo"),
    ("m4v", "video/mp4"),
    ("mkv", "video/x-matroska"),
    ("mov", "video/quicktime"),
    ("mp4", "video/mp4"),
    ("ogv", "video/ogg"),
    ("webm", "video/webm"),
];
const AUDIO: &[(&str, &str)] = &[
    ("mp3", "audio/mpeg"),
    ("wav", "audio/wav"),
    ("ogg", "audio/ogg"),
    ("oga", "audio/ogg"),
    ("flac", "audio/flac"),
    ("aac", "audio/aac"),
    ("m4a", "audio/mp4"),
    ("opus", "audio/ogg"),
    ("aiff", "audio/aiff"),
    ("webm-audio", "audio/webm"),
    ("weba", "audio/webm"),
];
const BINARY: &[(&str, &str)] = &[
    ("zip", "application/zip"),
    ("jar", "application/java-archive"),
    ("tar", "application/x-tar"),
    ("gz", "application/gzip"),
    ("tgz", "application/gzip"),
    ("sqlite", "application/vnd.sqlite3"),
    ("sqlite3", "application/vnd.sqlite3"),
    ("db", "application/vnd.sqlite3"),
];
const FONT: &[(&str, &str)] = &[
    ("ttf", "font/ttf"),
    ("otf", "font/otf"),
    ("woff", "font/woff"),
    ("woff2", "font/woff2"),
];
const BROWSER: &[(&str, &str)] = &[
    ("html", "text/html"),
    ("htm", "text/html"),
    ("pdf", "application/pdf"),
];
const TEXT: &[&str] = &[
    "txt",
    "log",
    "json",
    "jsonc",
    "jsonl",
    "ndjson",
    "yaml",
    "yml",
    "toml",
    "ini",
    "conf",
    "config",
    "env",
    "csv",
    "tsv",
    "xml",
    "css",
    "scss",
    "sass",
    "less",
    "js",
    "jsx",
    "mjs",
    "cjs",
    "ts",
    "tsx",
    "mts",
    "cts",
    "py",
    "pyi",
    "rb",
    "go",
    "rs",
    "swift",
    "kt",
    "kts",
    "java",
    "c",
    "h",
    "cc",
    "cpp",
    "hpp",
    "cs",
    "php",
    "sh",
    "bash",
    "zsh",
    "fish",
    "sql",
    "graphql",
    "gql",
    "vue",
    "svelte",
    "r",
    "lua",
    "ex",
    "exs",
    "erl",
    "hs",
    "clj",
    "dart",
    "diff",
    "patch",
    "lock",
    "properties",
    "gradle",
];

fn extension(name: &str) -> &str {
    name.rsplit_once('.').map_or("", |(_, extension)| extension)
}
fn lookup(extension: &str, table: &[(&str, &'static str)]) -> Option<&'static str> {
    table
        .iter()
        .find(|(candidate, _)| *candidate == extension)
        .map(|(_, mime)| *mime)
}

pub fn preview_mime_type(name: &str) -> Option<&'static str> {
    let name = name.to_ascii_lowercase();
    let extension = extension(&name);
    lookup(extension, MEDIA)
        .or_else(|| lookup(extension, AUDIO))
        .or_else(|| lookup(extension, BROWSER))
        .or_else(|| lookup(extension, FONT))
        .or_else(|| lookup(extension, BINARY))
        .or_else(|| {
            OFFICE
                .iter()
                .find(|(candidate, _, _)| *candidate == extension)
                .map(|(_, mime, _)| *mime)
        })
}

pub fn media_preview_kind(name: &str) -> Option<FilePreviewKind> {
    let name = name.to_ascii_lowercase();
    lookup(extension(&name), MEDIA).map(|mime| {
        if mime.starts_with("video/") {
            FilePreviewKind::Video
        } else {
            FilePreviewKind::Image
        }
    })
}

fn generic_mime(mime: &str) -> bool {
    [
        "",
        "text/plain",
        "application/octet-stream",
        "binary/octet-stream",
        "application/unknown",
        "video/mp2t",
        "video/vnd.dlna.mpeg-tts",
    ]
    .contains(&mime)
}

fn office_kind(name: &str, mime: &str) -> Option<FilePreviewKind> {
    if mime == "text/rtf" {
        return Some(FilePreviewKind::Rtf);
    }
    OFFICE
        .iter()
        .find(|(extension, expected, kind)| {
            mime == *expected
                || ((["", "application/octet-stream", "text/plain"].contains(&mime)
                    || (*kind == FilePreviewKind::Ipynb && mime == "application/json"))
                    && name.ends_with(&format!(".{extension}")))
        })
        .map(|(_, _, kind)| *kind)
}

fn binary_kind(name: &str, mime: &str) -> Option<FilePreviewKind> {
    use FilePreviewKind::{Archive, Sqlite};
    if [
        "application/vnd.sqlite3",
        "application/x-sqlite3",
        "application/x-sqlite",
    ]
    .contains(&mime)
    {
        return Some(Sqlite);
    }
    if [
        "application/zip",
        "application/x-zip-compressed",
        "application/java-archive",
        "application/x-java-archive",
        "application/x-tar",
        "application/gzip",
        "application/x-gzip",
        "application/x-compressed-tar",
    ]
    .contains(&mime)
    {
        return Some(Archive);
    }
    if !["", "application/octet-stream", "text/plain"].contains(&mime) {
        return None;
    }
    lookup(extension(name), BINARY).map(|mime| {
        if mime == "application/vnd.sqlite3" {
            Sqlite
        } else {
            Archive
        }
    })
}

fn mime_kind(mime: &str, rich: bool, ext: &str) -> Option<FilePreviewKind> {
    use FilePreviewKind::*;
    match mime {
        "application/pdf" => Some(Pdf),
        "text/html" => Some(Html),
        "text/markdown" | "text/x-markdown" => Some(Markdown),
        "text/vnd.mermaid" if rich => Some(Mermaid),
        "application/vnd.oasis.opendocument.formula-template" if rich && ext == "otf" => Some(Font),
        _ if rich && mime.starts_with("font/") => Some(Font),
        _ if mime.starts_with("image/") => Some(Image),
        _ if mime.starts_with("video/") && !generic_mime(mime) => Some(Video),
        _ if mime.starts_with("audio/") => Some(Audio),
        _ => None,
    }
}

fn filename_kind(name: &str, rich: bool) -> Option<FilePreviewKind> {
    use FilePreviewKind::*;
    let extension = extension(name);
    match extension {
        "pdf" => return Some(Pdf),
        "htm" | "html" => return Some(Html),
        "md" | "markdown" | "mdown" | "mkd" | "mdx" => return Some(Markdown),
        "mmd" | "mermaid" if rich => return Some(Mermaid),
        "ttf" | "otf" | "woff" | "woff2" if rich => return Some(Font),
        "rtf" if rich => return Some(Text),
        _ => {}
    }
    if let Some(kind) = media_preview_kind(name) {
        return Some(kind);
    }
    if lookup(extension, AUDIO).is_some() {
        return Some(Audio);
    }
    if TEXT.contains(&extension) {
        return Some(Text);
    }
    let basename = name.rsplit(['/', '\\']).next().unwrap_or(name);
    [
        "dockerfile",
        "makefile",
        "gemfile",
        "rakefile",
        "license",
        "readme",
        ".gitignore",
        ".gitattributes",
        ".editorconfig",
        ".env",
    ]
    .iter()
    .any(|stem| basename == *stem || basename.starts_with(&format!("{stem}.")))
    .then_some(Text)
}

fn text_mime(mime: &str) -> bool {
    if mime.starts_with("text/") {
        return true;
    }
    let Some(kind) = mime.strip_prefix("application/") else {
        return false;
    };
    [
        "json",
        "xml",
        "javascript",
        "x-javascript",
        "yaml",
        "x-yaml",
        "toml",
        "sql",
    ]
    .contains(&kind)
        || kind.ends_with("+json")
        || kind.ends_with("+xml")
}

#[cfg_attr(feature = "bindings", uniffi::export)]
pub fn file_preview_kind(
    name: String,
    mime_type: Option<String>,
    rich_documents: bool,
) -> FilePreviewKind {
    let name = name.to_lowercase();
    let mime = mime_type
        .as_deref()
        .unwrap_or_default()
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    rich_documents
        .then(|| office_kind(&name, &mime))
        .flatten()
        .or_else(|| binary_kind(&name, &mime))
        .or_else(|| mime_kind(&mime, rich_documents, extension(&name)))
        .or_else(|| {
            generic_mime(&mime)
                .then(|| filename_kind(&name, rich_documents))
                .flatten()
        })
        .or_else(|| text_mime(&mime).then_some(FilePreviewKind::Text))
        .unwrap_or(FilePreviewKind::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_t3_rich_document_archive_database_and_font_format_is_classified() {
        for (extension, _, kind) in OFFICE {
            assert_eq!(
                file_preview_kind(format!("file.{extension}"), None, true),
                *kind
            );
        }
        for extension in ["zip", "jar", "tar", "gz", "tgz"] {
            assert_eq!(
                file_preview_kind(format!("file.{extension}"), None, true),
                FilePreviewKind::Archive
            );
        }
        for extension in ["sqlite", "sqlite3", "db"] {
            assert_eq!(
                file_preview_kind(format!("file.{extension}"), None, true),
                FilePreviewKind::Sqlite
            );
        }
        for extension in ["ttf", "otf", "woff", "woff2"] {
            assert_eq!(
                file_preview_kind(format!("file.{extension}"), None, true),
                FilePreviewKind::Font
            );
        }
        for extension in ["exe", "dmg", "pkg", "7z", "rar", "bz2", "doc", "xls", "ppt"] {
            assert_eq!(
                file_preview_kind(format!("file.{extension}"), None, true),
                FilePreviewKind::Unsupported
            );
        }
    }

    #[test]
    fn definite_mime_types_override_extensions_and_transport_stream_mime_does_not_hide_source_code()
    {
        assert_eq!(
            file_preview_kind("image.txt".into(), Some("image/png".into()), true),
            FilePreviewKind::Image
        );
        assert_eq!(
            file_preview_kind("main.ts".into(), Some("video/mp2t".into()), true),
            FilePreviewKind::Text
        );
        assert_eq!(
            file_preview_kind("data.unknown".into(), Some("application/pdf".into()), true),
            FilePreviewKind::Pdf
        );
        assert_eq!(
            file_preview_kind("notes.docx".into(), Some("image/png".into()), true),
            FilePreviewKind::Image
        );
    }

    #[test]
    fn media_menu_excludes_audio_while_viewers_still_play_it() {
        assert_eq!(media_preview_kind("clip.MP4"), Some(FilePreviewKind::Video));
        assert_eq!(media_preview_kind("song.mp3"), None);
        assert_eq!(
            file_preview_kind("song.mp3".into(), None, true),
            FilePreviewKind::Audio
        );
        assert_eq!(
            file_preview_kind("sample.docx".into(), None, false),
            FilePreviewKind::Unsupported
        );
    }
}
