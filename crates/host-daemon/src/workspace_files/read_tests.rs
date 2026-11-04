use super::*;

#[cfg(unix)]
#[test]
fn folder_listings_follow_symlinks_without_losing_broken_links() {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("folder")).unwrap();
    fs::write(directory.path().join("file.txt"), "content").unwrap();
    for (target, alias) in [
        ("folder", "linked-folder"),
        ("file.txt", "linked-file.txt"),
        ("missing", "broken-link"),
    ] {
        std::os::unix::fs::symlink(target, directory.path().join(alias)).unwrap();
    }
    let files = WorkspaceFiles::new(directory.path().join("uploads"));
    let Body::Files(listing) = files
        .dispatch(
            1,
            Call::ListFiles(PathParams {
                path: directory.path().to_str().unwrap().into(),
            }),
        )
        .unwrap()
    else {
        panic!("expected folder listing")
    };
    let folder = listing
        .entries
        .iter()
        .find(|entry| entry.name == "linked-folder")
        .unwrap();
    assert!(folder.directory, "a linked folder must remain navigable");
    assert_eq!(
        folder.path,
        directory
            .path()
            .canonicalize()
            .unwrap()
            .join("linked-folder")
            .to_str()
            .unwrap()
    );
    let file = listing
        .entries
        .iter()
        .find(|entry| entry.name == "linked-file.txt")
        .unwrap();
    assert!(!file.directory);
    assert_eq!(file.size, 7);
    assert!(
        listing
            .entries
            .iter()
            .any(|entry| entry.name == "broken-link")
    );
    let Body::Files(children) = files
        .dispatch(
            1,
            Call::ListFiles(PathParams {
                path: folder.path.clone(),
            }),
        )
        .unwrap()
    else {
        panic!("expected linked folder listing")
    };
    assert!(children.entries.is_empty());
}

#[cfg(unix)]
#[test]
fn file_operations_reject_named_pipes_without_waiting_for_a_writer() {
    use std::os::unix::fs::OpenOptionsExt;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("pipe");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    let files = WorkspaceFiles::new(directory.path().join("uploads"));
    let params = PathParams {
        path: path.to_str().unwrap().into(),
    };
    for request in [
        Call::ReadFile(params.clone()),
        Call::WriteFile(agent_protocol::operations::WriteFile {
            path: params.path.clone(),
            revision: hash(b""),
            text: "replacement".into(),
        }),
        Call::Download(params),
    ] {
        let files = files.clone();
        let (completed, result) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            completed.send(files.dispatch(1, request)).unwrap();
        });
        let rejected = result.recv_timeout(Duration::from_secs(1));
        // Release a broken implementation before failing, so no blocked worker
        // survives the regression test.
        if rejected.is_err() {
            drop(
                File::options()
                    .write(true)
                    .custom_flags(libc::O_NONBLOCK)
                    .open(&path)
                    .unwrap(),
            );
        }
        worker.join().unwrap();
        assert!(
            rejected
                .expect("opening a named pipe blocked the file service")
                .is_err()
        );
    }
    assert!(files.grants.lock().unwrap().is_empty());
}

#[test]
fn a_save_that_cannot_be_read_back_never_replaces_the_original_file() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("notes.txt");
    fs::write(&path, "original").unwrap();
    let files = WorkspaceFiles::new(directory.path().join("uploads"));
    let original = read_file(&path, &path).unwrap();
    let result = files.dispatch(
        1,
        Call::WriteFile(agent_protocol::operations::WriteFile {
            path: path.to_str().unwrap().into(),
            revision: original.revision.clone(),
            text: "text\0with a null byte".into(),
        }),
    );
    assert!(result.is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "original");
    files
        .dispatch(
            1,
            Call::WriteFile(agent_protocol::operations::WriteFile {
                path: path.to_str().unwrap().into(),
                revision: original.revision,
                text: "valid edit".into(),
            }),
        )
        .unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "valid edit");
}

#[test]
fn text_at_the_limit_is_editable_and_larger_text_is_a_read_only_prefix() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("large.txt");
    let files = WorkspaceFiles::new(directory.path().join("uploads"));
    for length in [
        EDIT_LIMIT as usize - 1,
        EDIT_LIMIT as usize,
        EDIT_LIMIT as usize + 1,
    ] {
        let bytes = vec![b'x'; length];
        fs::write(&path, &bytes).unwrap();
        let content = read_file(&path, &path).unwrap();
        assert_eq!(content.truncated, length as u64 > EDIT_LIMIT);
        assert_eq!(content.size, length as u64);
        assert_eq!(content.text.len(), length.min(EDIT_LIMIT as usize));
        assert_eq!(content.text.as_bytes(), &bytes[..content.text.len()]);
        let result = files.dispatch(
            1,
            Call::WriteFile(agent_protocol::operations::WriteFile {
                path: path.to_str().unwrap().into(),
                revision: content.revision,
                text: "replacement".into(),
            }),
        );
        if length as u64 > EDIT_LIMIT {
            assert!(result.is_err());
            assert_eq!(
                fs::read(&path).unwrap(),
                bytes,
                "a preview must never replace the full file"
            );
        } else {
            assert!(result.is_ok());
            assert_eq!(fs::read(&path).unwrap(), b"replacement");
        }
    }
}

#[test]
fn preview_drops_only_a_utf8_character_split_by_the_byte_limit() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("unicode.txt");
    for character in ["é", "あ", "🦀"] {
        for remaining in 1..character.len() {
            let prefix = "a".repeat(EDIT_LIMIT as usize - remaining);
            fs::write(&path, format!("{prefix}{character}tail")).unwrap();
            let content = read_file(&path, &path).unwrap();
            assert!(content.truncated);
            assert_eq!(content.text, prefix);
            assert_eq!(content.size as usize, prefix.len() + character.len() + 4);
        }
    }
    for bytes in [vec![0xff], vec![b'a', 0], vec![0xe3, 0x81]] {
        fs::write(&path, bytes).unwrap();
        assert!(read_file(&path, &path).is_err());
    }
    let mut invalid_prefix = vec![b'x'; EDIT_LIMIT as usize + 1];
    invalid_prefix[100] = 0xff;
    fs::write(&path, invalid_prefix).unwrap();
    assert!(read_file(&path, &path).is_err());
}
