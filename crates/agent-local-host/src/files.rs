//! Owner-only state files written atomically.
use anyhow::{Context, Result};
use std::{fs, io, path::Path};

/// Unix state is owner-only; Windows directories inherit the parent DACL.
/// The default state parent is the current user's local application data folder.
pub fn create_state_directory(path: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

pub fn private_file_options() -> fs::OpenOptions {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}

/// Atomically replace owner-only JSON state and flush its contents before rename.
pub fn save_private_json(path: &Path, value: &impl serde::Serialize) -> Result<()> {
    let parent = path.parent().context("state path has no parent")?;
    create_state_directory(parent)?;
    save_private_json_inner(path, value)
        .with_context(|| format!("could not save state {}", path.display()))
}

fn save_private_json_inner(path: &Path, value: &impl serde::Serialize) -> Result<()> {
    let mut retries = 0;
    loop {
        let result = atomicwrites::AtomicFile::new(path, atomicwrites::AllowOverwrite)
            .write_with_options(
                |file| {
                    serde_json::to_writer(&mut *file, value)?;
                    file.sync_all()
                },
                private_file_options(),
            );
        // MoveFileExW can report ACCESS_DENIED while another process is
        // replacing this same destination (including another BEX window).
        // Retry only OS failures, never serialization errors. Each attempt
        // still atomically replaces the complete, flushed JSON document.
        if cfg!(windows)
            && let Err(atomicwrites::Error::Internal(error)) = &result
            && matches!(error.raw_os_error(), Some(5 | 32 | 33))
            && retries < 10
        {
            retries += 1;
            std::thread::sleep(std::time::Duration::from_millis(10 * retries));
            continue;
        }
        return result.map_err(Into::into);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_state_replacements_keep_complete_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        save_private_json(&path, &serde_json::json!({"writer": 0, "sequence": 0})).unwrap();
        let start = std::sync::Barrier::new(8);
        std::thread::scope(|scope| {
            for writer in 0..8 {
                let (path, start) = (&path, &start);
                scope.spawn(move || {
                    start.wait();
                    for sequence in 0..50 {
                        save_private_json(
                            path,
                            &serde_json::json!({"writer": writer, "sequence": sequence}),
                        )
                        .unwrap();
                        let saved: serde_json::Value =
                            serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
                        assert!(saved["writer"].as_u64().unwrap() < 8);
                        assert!(saved["sequence"].as_u64().unwrap() < 50);
                    }
                });
            }
        });
    }

    #[cfg(windows)]
    #[test]
    fn state_save_waits_for_a_temporary_windows_file_lock() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        save_private_json(&path, &"previous").unwrap();
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&path)
            .unwrap();
        std::thread::scope(|scope| {
            scope.spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(60));
                drop(held);
            });
            save_private_json(&path, &"updated").unwrap();
        });
        assert_eq!(fs::read_to_string(path).unwrap(), "\"updated\"");
    }

    #[test]
    fn serialization_failure_preserves_previous_state_and_names_the_path() {
        struct Invalid;
        impl serde::Serialize for Invalid {
            fn serialize<S: serde::Serializer>(
                &self,
                _: S,
            ) -> std::result::Result<S::Ok, S::Error> {
                Err(serde::ser::Error::custom("invalid state"))
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        save_private_json(&path, &"previous").unwrap();
        let error = save_private_json(&path, &Invalid).unwrap_err();
        assert!(format!("{error:#}").contains("invalid state"));
        assert!(error.to_string().contains("state.json"));
        assert_eq!(fs::read_to_string(path).unwrap(), "\"previous\"");
    }
}
