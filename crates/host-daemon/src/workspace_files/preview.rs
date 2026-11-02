use super::*;
use agent_protocol::file_preview::{FilePreviewMetadata, FileSlice, ReadFileSlice};
use std::io::{Seek, SeekFrom};

pub(super) fn metadata(source: &str) -> Result<FilePreviewMetadata> {
    let path = absolute_path(&source)?;
    let metadata = fs::metadata(&path)?;
    let mime_type = if metadata.is_file() && metadata.len() > 0 && path.extension().is_none() {
        let mut header = [0; 4096];
        let (mut file, _) = open_regular(&path)?;
        let length = file.read(&mut header)?;
        header_mime(&header[..length]).map(str::to_owned)
    } else {
        None
    };
    Ok(FilePreviewMetadata {
        entry: FileEntry {
            name: path
                .file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy()
                .into_owned(),
            path: path.to_str().context("file path is not UTF-8")?.into(),
            directory: metadata.is_dir(),
            size: metadata.len(),
        },
        mime_type,
        revision: revision(&metadata),
    })
}

pub(super) fn read(request: ReadFileSlice) -> Result<FileSlice> {
    request.validate().map_err(anyhow::Error::msg)?;
    let path = absolute_path(&request.path)?;
    let (mut file, before) = open_regular(&path)?;
    if revision(&before) != request.revision {
        return Err(anyhow!(
            "revision_conflict: file changed; reload before previewing"
        ));
    }
    if request.offset > before.len() {
        return Err(anyhow!("file slice is outside the file"));
    }
    file.seek(SeekFrom::Start(request.offset))?;
    let length = u64::from(request.length).min(before.len() - request.offset) as usize;
    let mut bytes = vec![0; length];
    file.read_exact(&mut bytes)?;
    if revision(&file.metadata()?) != request.revision {
        return Err(anyhow!("revision_conflict: file changed while reading"));
    }
    Ok(FileSlice { bytes })
}

fn revision(metadata: &fs::Metadata) -> String {
    let time = |value: std::io::Result<std::time::SystemTime>| {
        value
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|time| time.as_nanos())
            .unwrap_or_default()
    };
    format!(
        "{}-{}-{}",
        metadata.len(),
        time(metadata.modified()),
        time(metadata.created())
    )
}

fn open_regular(path: &Path) -> Result<(File, fs::Metadata)> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(anyhow!("preview target is not a regular file"));
    }
    Ok((file, metadata))
}

fn header_mime(bytes: &[u8]) -> Option<&'static str> {
    for (signature, mime) in [
        (&b"\x89PNG\r\n\x1a\n"[..], "image/png"),
        (&b"\xff\xd8\xff"[..], "image/jpeg"),
        (&b"PK\x03\x04"[..], "application/zip"),
        (&b"GIF87a"[..], "image/gif"),
        (&b"GIF89a"[..], "image/gif"),
        (&b"%PDF-"[..], "application/pdf"),
        (&b"SQLite format 3\0"[..], "application/vnd.sqlite3"),
    ] {
        if bytes.starts_with(signature) {
            return Some(mime);
        }
    }
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        return Some("image/webp");
    }
    if !bytes.starts_with(b"#!") {
        return None;
    }
    let line = String::from_utf8_lossy(bytes)
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned();
    // JavaScript's word boundaries are ASCII, including next to non-Latin text.
    static INTERPRETERS: std::sync::LazyLock<[(regex::Regex, &str); 4]> =
        std::sync::LazyLock::new(|| {
            [
                (r"(?-u:\b)python[0-9.]*(?-u:\b)", "text/x-python"),
                (r"(?-u:\b)(?:node|nodejs)(?-u:\b)", "text/javascript"),
                (r"(?-u:\b)ruby(?-u:\b)", "text/x-ruby"),
                (
                    r"(?-u:\b)(?:sh|bash|zsh|fish)(?-u:\b)",
                    "text/x-shellscript",
                ),
            ]
            .map(|(pattern, mime)| (regex::Regex::new(pattern).unwrap(), mime))
        });
    INTERPRETERS
        .iter()
        .find(|(pattern, _)| pattern.is_match(&line))
        .map(|(_, mime)| *mime)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn range_reads_reject_changed_files_and_keep_the_named_path() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("movie.mp4");
        fs::write(&path, b"0123456789").unwrap();
        let metadata = metadata(path.to_str().unwrap()).unwrap();
        let request = ReadFileSlice {
            path: metadata.entry.path.clone(),
            offset: 3,
            length: 4,
            revision: metadata.revision,
        };
        assert_eq!(read(request.clone()).unwrap().bytes, b"3456");
        let mut last = request.clone();
        last.offset = 8;
        assert_eq!(read(last).unwrap().bytes, b"89");
        fs::write(&path, b"changed length").unwrap();
        assert!(
            read(request.clone())
                .unwrap_err()
                .to_string()
                .contains("revision_conflict")
        );
        let refreshed = super::metadata(path.to_str().unwrap()).unwrap();
        let request = ReadFileSlice {
            offset: 0,
            revision: refreshed.revision,
            ..request
        };
        assert_eq!(read(request.clone()).unwrap().bytes, b"chan");
        assert!(
            read(ReadFileSlice {
                offset: 14,
                ..request.clone()
            })
            .unwrap()
            .bytes
            .is_empty()
        );
        assert!(
            read(ReadFileSlice {
                offset: 15,
                ..request
            })
            .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn preview_metadata_and_reads_preserve_a_symlinks_display_name() {
        let root = tempfile::tempdir().unwrap();
        let contents = root.path().join("contents");
        fs::write(&contents, b"<svg/>").unwrap();
        let named_path = root.path().join("chart.svg");
        std::os::unix::fs::symlink(&contents, &named_path).unwrap();
        let metadata = metadata(named_path.to_str().unwrap()).unwrap();
        assert_eq!(metadata.entry.path, named_path.to_str().unwrap());
        assert_eq!(metadata.entry.name, "chart.svg");
        assert!(!metadata.entry.directory);
        assert_eq!(
            read(ReadFileSlice {
                path: metadata.entry.path,
                offset: 0,
                length: 6,
                revision: metadata.revision,
            })
            .unwrap()
            .bytes,
            b"<svg/>"
        );
    }
    #[test]
    fn header_metadata_uses_t3s_extensionless_signatures_and_shebangs() {
        assert_eq!(header_mime(b"%PDF-1.7"), Some("application/pdf"));
        assert_eq!(
            header_mime(b"#!/usr/bin/env python3.12\nprint('a')"),
            Some("text/x-python")
        );
        assert_eq!(
            header_mime(b"#!/usr/bin/env node\nconst a = 1;"),
            Some("text/javascript")
        );
        assert_eq!(header_mime(b"plain text"), None);
        assert_eq!(
            header_mime(b"#!/bin/bash.exe\n"),
            Some("text/x-shellscript")
        );
        assert_eq!(
            header_mime("#!/usr/bin/python日本語\n".as_bytes()),
            Some("text/x-python")
        );
        assert_eq!(header_mime(b"#!/usr/bin/python_helper\n"), None);
        assert_eq!(header_mime(b"#!/usr/bin/ruby.rb\n"), Some("text/x-ruby"));
    }
}
