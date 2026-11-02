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
    let words = line
        .split(|char: char| !char.is_ascii_alphanumeric() && char != '.' && char != '_')
        .collect::<Vec<_>>();
    if words.iter().any(|word| {
        word.strip_prefix("python").is_some_and(|suffix| {
            suffix
                .chars()
                .all(|char| char.is_ascii_digit() || char == '.')
        })
    }) {
        return Some("text/x-python");
    }
    [
        (&["node", "nodejs"][..], "text/javascript"),
        (&["ruby"][..], "text/x-ruby"),
        (&["sh", "bash", "zsh", "fish"][..], "text/x-shellscript"),
    ]
    .into_iter()
    .find(|(interpreters, _)| words.iter().any(|word| interpreters.contains(word)))
    .map(|(_, mime)| mime)
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
            read(request)
                .unwrap_err()
                .to_string()
                .contains("revision_conflict")
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
    }
}
