use super::*;
use agent_protocol::file_metadata::{
    FileMetadata, FileMetadataEntries, FileMetadataKind, FilePathMetadata, ReadFileMetadata,
};
use futures_util::{StreamExt, stream};
use regex::Regex;
use std::{collections::BTreeSet, sync::LazyLock};

pub(super) async fn read(params: ReadFileMetadata) -> Result<FileMetadataEntries> {
    params.validate().map_err(anyhow::Error::msg)?;
    let paths = params.paths.iter().cloned().collect::<BTreeSet<_>>();
    let mut pending = stream::iter(paths.into_iter().map(|path| async move {
        let requested = path.clone();
        let value = tokio::task::spawn_blocking(move || inspect(&path)).await??;
        Ok::<_, anyhow::Error>((requested, value))
    }))
    .buffer_unordered(8);
    let mut resolved = HashMap::new();
    while let Some(value) = pending.next().await {
        let (path, value) = value?;
        resolved.insert(path, value);
    }
    Ok(FileMetadataEntries {
        entries: params
            .paths
            .iter()
            .map(|path| resolved[path].clone())
            .collect(),
    })
}

fn inspect(requested: &str) -> Result<FilePathMetadata> {
    if agent_protocol::paths::is_network_host_path(requested) {
        return Ok(FilePathMetadata {
            path: requested.into(),
            metadata: None,
        });
    }
    let Ok(path) = absolute_path(&requested) else {
        return Ok(FilePathMetadata {
            path: requested.into(),
            metadata: None,
        });
    };
    let resolved = path.to_str().context("file path is not UTF-8")?.to_owned();
    // T3 rejects automatic network stats before touching the filesystem: on
    // Windows even stat may sign in to the remote host with the user's NTLM.
    let network = agent_protocol::paths::is_network_host_path(&resolved);
    let metadata = if network {
        None
    } else {
        fs::metadata(&path).ok().map(|stat| {
            let kind = if stat.is_dir() {
                FileMetadataKind::Directory
            } else if stat.is_file() {
                FileMetadataKind::File
            } else {
                FileMetadataKind::Other
            };
            FileMetadata {
                kind,
                byte_length: stat.is_file().then_some(stat.len()),
                mime_type: if stat.is_file() && stat.len() > 0 && path.extension().is_none() {
                    header(&path).ok().flatten().map(str::to_owned)
                } else {
                    None
                },
            }
        })
    };
    Ok(FilePathMetadata {
        path: resolved,
        metadata,
    })
}

fn header(path: &Path) -> Result<Option<&'static str>> {
    let (file, _) = open_regular_file(path)?;
    let mut bytes = Vec::new();
    file.take(512).read_to_end(&mut bytes)?;
    Ok(header_mime(&bytes))
}

/// T3's fileHeaderMimeType: only the first 512 bytes of extensionless regular
/// files are inspected. This metadata supports glyphs, never file previews.
fn header_mime(bytes: &[u8]) -> Option<&'static str> {
    for (signature, mime) in [
        (&b"\x89PNG\r\n\x1a\n"[..], "image/png"),
        (&b"\xff\xd8\xff"[..], "image/jpeg"),
        (&b"PK\x03\x04"[..], "application/zip"),
    ] {
        if bytes.starts_with(signature) {
            return Some(mime);
        }
    }
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        return Some("image/webp");
    }
    for (signature, mime) in [
        (&b"GIF87a"[..], "image/gif"),
        (&b"GIF89a"[..], "image/gif"),
        (&b"%PDF-"[..], "application/pdf"),
        (&b"SQLite format 3\0"[..], "application/vnd.sqlite3"),
    ] {
        if bytes.starts_with(signature) {
            return Some(mime);
        }
    }
    if !bytes.starts_with(b"#!") {
        return None;
    }
    let end = memchr::memchr(b'\n', bytes).unwrap_or(bytes.len());
    let line = String::from_utf8_lossy(&bytes[..end]);
    static INTERPRETERS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
        [
            (r"(?-u:\bpython[0-9.]*\b)", "text/x-python"),
            (r"(?-u:\b(?:node|nodejs)\b)", "text/javascript"),
            (r"(?-u:\bruby\b)", "text/x-ruby"),
            (r"(?-u:\b(?:sh|bash|zsh|fish)\b)", "text/x-shellscript"),
        ]
        .map(|(pattern, mime)| (Regex::new(pattern).unwrap(), mime))
        .into()
    });
    INTERPRETERS
        .iter()
        .find_map(|(regex, mime)| regex.is_match(&line).then_some(*mime))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn batches_preserve_order_duplicates_missing_paths_and_host_home() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("run");
        fs::write(&path, b"#!/usr/bin/env python3\nprint('ok')").unwrap();
        let file = path.to_str().unwrap().to_owned();
        let missing = directory
            .path()
            .join("missing")
            .to_str()
            .unwrap()
            .to_owned();
        let files = WorkspaceFiles::new(directory.path().join("attachments"));
        let Body::FileMetadataEntries(output) = files
            .request(
                1,
                Call::ReadFileMetadata(ReadFileMetadata {
                    paths: vec![file.clone(), missing.clone(), "~".into(), file],
                }),
            )
            .await
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(output.entries[0], output.entries[3]);
        assert_eq!(
            output.entries[0]
                .metadata
                .as_ref()
                .unwrap()
                .mime_type
                .as_deref(),
            Some("text/x-python")
        );
        assert_eq!(
            output.entries[1],
            FilePathMetadata {
                path: missing,
                metadata: None
            }
        );
        assert_eq!(
            output.entries[2].path,
            directories::BaseDirs::new()
                .unwrap()
                .home_dir()
                .to_str()
                .unwrap()
        );
        assert_eq!(
            output.entries[2].metadata.as_ref().unwrap().kind,
            FileMetadataKind::Directory
        );
        assert_eq!(
            output.entries[2].metadata.as_ref().unwrap().byte_length,
            None
        );
        assert!(
            files
                .request(
                    1,
                    Call::ReadFileMetadata(ReadFileMetadata {
                        paths: vec!["/a".into(); 65]
                    })
                )
                .await
                .is_err()
        );
    }
    #[test]
    fn metadata_never_stats_network_paths_and_header_reads_are_bounded() {
        for path in [
            "//not-a-host.invalid/share/private",
            r"\\not-a-host.invalid\share\private",
        ] {
            let network = inspect(path).unwrap();
            assert_eq!(network.path, path);
            assert!(network.metadata.is_none());
        }
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("script");
        for (padding, expected) in [(504, Some("text/x-python")), (505, None)] {
            let mut bytes = b"#!".to_vec();
            bytes.extend(vec![b' '; padding]);
            bytes.extend(b"python\n");
            fs::write(&path, bytes).unwrap();
            assert_eq!(
                inspect(path.to_str().unwrap())
                    .unwrap()
                    .metadata
                    .unwrap()
                    .mime_type
                    .as_deref(),
                expected
            );
        }
        let named = directory.path().join("script.txt");
        fs::write(&named, b"#!/usr/bin/python").unwrap();
        assert_eq!(
            inspect(named.to_str().unwrap())
                .unwrap()
                .metadata
                .unwrap()
                .mime_type,
            None
        );
    }
    #[cfg(unix)]
    #[test]
    fn non_regular_entries_have_no_length_or_header_and_are_never_opened() {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("socket");
        let _listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let metadata = inspect(socket.to_str().unwrap()).unwrap().metadata.unwrap();
        assert_eq!(
            metadata,
            FileMetadata {
                kind: FileMetadataKind::Other,
                byte_length: None,
                mime_type: None
            }
        );
    }
    #[test]
    fn header_mime_matches_t3_signatures_and_first_line_interpreters() {
        for (bytes, mime) in [
            (&b"\x89PNG\r\n\x1a\n"[..], "image/png"),
            (&b"\xff\xd8\xff"[..], "image/jpeg"),
            (&b"PK\x03\x04"[..], "application/zip"),
            (&b"RIFFabcdWEBP"[..], "image/webp"),
            (&b"GIF87a"[..], "image/gif"),
            (&b"GIF89a"[..], "image/gif"),
            (&b"%PDF-"[..], "application/pdf"),
            (&b"SQLite format 3\0"[..], "application/vnd.sqlite3"),
            (&b"#!/usr/bin/python3.13"[..], "text/x-python"),
            (&b"#!\xffpython3"[..], "text/x-python"),
            (&b"#!python\xf0\nruby"[..], "text/x-python"),
            (&b"#!env\xe2\x80\xa8python"[..], "text/x-python"),
            (&b"#!/usr/bin/env node"[..], "text/javascript"),
            (&b"#!/usr/bin/ruby"[..], "text/x-ruby"),
            (&b"#!/bin/zsh"[..], "text/x-shellscript"),
        ] {
            assert_eq!(header_mime(bytes), Some(mime));
            if !bytes.starts_with(b"#!") {
                let mut binary = bytes.to_vec();
                binary.extend([0xff, 0xfe, 0x80]);
                assert_eq!(header_mime(&binary), Some(mime));
                for end in 0..bytes.len() {
                    assert_eq!(header_mime(&bytes[..end]), None);
                }
            }
        }
        for bytes in [
            &b""[..],
            &b"RIFF"[..],
            &b"python"[..],
            &b"#!notpython"[..],
            &b"#!env\npython"[..],
            &b"\xff\xfe#!python"[..],
            &b"#!\xf0\x90\npython"[..],
        ] {
            assert_eq!(header_mime(bytes), None);
        }
    }
}
