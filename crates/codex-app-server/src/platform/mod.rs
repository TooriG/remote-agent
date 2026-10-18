#[cfg(unix)]
use std::fs;
use std::{borrow::Cow, path::Path};

pub(crate) fn bundled_codex_path() -> Option<&'static Path> {
    #[cfg(target_os = "macos")]
    {
        Some(Path::new(
            "/Applications/ChatGPT.app/Contents/Resources/codex-cli/bin/codex",
        ))
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// Where a missing Codex can come from, matching the lookup order of `resolve`.
#[cfg(target_os = "macos")]
pub(crate) const NOT_FOUND_HINT: &str = "Install ChatGPT.app in /Applications, put codex on PATH, \
     or pass --codex with its path (BEX_CODEX for the desktop app)";
#[cfg(not(target_os = "macos"))]
pub(crate) const NOT_FOUND_HINT: &str =
    "Put codex on PATH, or pass --codex with its path (BEX_CODEX for the desktop app)";

pub(crate) fn executable_path(path: &Path) -> Cow<'_, Path> {
    #[cfg(windows)]
    if path.extension().is_none() {
        return Cow::Owned(path.with_extension("exe"));
    }
    Cow::Borrowed(path)
}

pub(crate) fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path)
            .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}
