//! Downloads handed to external apps must outlive the originating window and
//! Bex itself: viewers may open the file lazily or reopen it after Bex quits.
//! Keep completed downloads for seven days, then prune on launch and download.
//! Failed transfers use TempDir's immediate cleanup. Busy Windows files remain
//! for the next cleanup attempt, as do leftovers from interrupted processes.
use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};
use tempfile::TempDir;

const RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);

pub(crate) fn cache_directory() -> Result<PathBuf, String> {
    super::state_dir().map(|directory| directory.join("opened-files"))
}

pub(crate) fn prepare(directory: &Path, source: &str) -> io::Result<(TempDir, PathBuf)> {
    agent_local_host::create_state_directory(directory)?;
    cleanup(directory, SystemTime::now())?;
    let directory = tempfile::Builder::new()
        .prefix("open-")
        .tempdir_in(directory)?;
    // Only the extension is needed for app association. Host filenames can
    // contain characters or reserved names that Windows cannot represent.
    let extension = source
        .rsplit_once('.')
        .map(|(_, extension)| extension)
        .filter(|extension| {
            !extension.is_empty() && extension.bytes().all(|b| b.is_ascii_alphanumeric())
        })
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "file extension is invalid"))?;
    let path = directory.path().join(format!("preview.{extension}"));
    Ok((directory, path))
}

pub(crate) fn cleanup(directory: &Path, now: SystemTime) -> io::Result<()> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let result = (|| -> io::Result<()> {
            let entry = entry?;
            if !entry.file_name().to_string_lossy().starts_with("open-")
                || !entry.file_type()?.is_dir()
            {
                return Ok(());
            }
            let modified = entry.metadata()?.modified()?;
            if now
                .duration_since(modified)
                .is_ok_and(|age| age >= RETENTION)
            {
                fs::remove_dir_all(entry.path())?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            tracing::warn!(target: "agent", operation = "downloads.cleanup", message = %error);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const SEVEN_DAYS: Duration = Duration::from_secs(604_800);

    #[test]
    fn downloads_are_unique_portable_and_keep_the_association_extension() {
        let cache = tempfile::tempdir().unwrap();
        let (first, path) = prepare(cache.path(), r"C:\Movies\promo.MP4").unwrap();
        let (_second, other) = prepare(cache.path(), "/movies/promo.MP4").unwrap();
        assert_ne!(path, other);
        assert_eq!(path.file_name().unwrap(), "preview.MP4");
        assert!(
            !path.exists(),
            "authenticated transfer creates the destination"
        );
        fs::write(&path, b"video").unwrap();
        drop(first);
        assert!(!path.exists(), "failed or cancelled preparation is removed");
    }

    #[test]
    fn completed_downloads_survive_until_retention_expires() {
        let cache = tempfile::tempdir().unwrap();
        let (directory, path) = prepare(cache.path(), "/movies/promo.mp4").unwrap();
        fs::write(&path, b"video").unwrap();
        let retained = directory.keep();
        let modified = fs::metadata(&retained).unwrap().modified().unwrap();
        cleanup(cache.path(), modified - Duration::from_secs(1)).unwrap();
        cleanup(cache.path(), modified + SEVEN_DAYS - Duration::from_secs(1)).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"video");
        cleanup(cache.path(), modified + SEVEN_DAYS).unwrap();
        assert!(!retained.exists());
    }

    #[test]
    fn cleanup_tolerates_missing_cache_and_preserves_unowned_entries() {
        let cache = tempfile::tempdir().unwrap();
        cleanup(&cache.path().join("missing"), SystemTime::now()).unwrap();
        let other = cache.path().join("unrelated");
        fs::create_dir(&other).unwrap();
        let file = cache.path().join("open-file");
        fs::write(&file, b"keep").unwrap();
        assert!(cleanup(&file, SystemTime::now()).is_err());
        cleanup(cache.path(), SystemTime::now() + SEVEN_DAYS).unwrap();
        assert!(other.exists());
        assert_eq!(fs::read(file).unwrap(), b"keep");
    }

    #[cfg(unix)]
    #[test]
    fn cleanup_never_follows_directory_symlinks() {
        let cache = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        let link = cache.path().join("open-link");
        std::os::unix::fs::symlink(target.path(), &link).unwrap();
        cleanup(cache.path(), SystemTime::now() + SEVEN_DAYS).unwrap();
        assert!(link.is_symlink());
        assert!(target.path().exists());
    }

    #[cfg(windows)]
    #[test]
    fn cleanup_retries_files_in_use_by_an_external_app() {
        use std::os::windows::fs::OpenOptionsExt;
        let cache = tempfile::tempdir().unwrap();
        let (directory, path) = prepare(cache.path(), "video.mp4").unwrap();
        fs::write(&path, b"video").unwrap();
        directory.keep();
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&path)
            .unwrap();
        let expired = SystemTime::now() + SEVEN_DAYS;
        cleanup(cache.path(), expired).unwrap();
        assert!(path.exists());
        drop(held);
        cleanup(cache.path(), expired).unwrap();
        assert!(!path.exists());
    }
}
