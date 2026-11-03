use std::{fs, io, path::Path};

pub(crate) fn worktree_directory(parent: &Path) -> io::Result<tempfile::TempDir> {
    let mut builder = tempfile::Builder::new();
    builder.prefix("session-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(fs::Permissions::from_mode(0o700));
    }
    builder.tempdir_in(parent)
}

/// Resolve the interactive shell on the Host, which may run a different OS
/// from the client opening the terminal.
pub(crate) fn terminal_command() -> &'static [&'static str] {
    #[cfg(windows)]
    {
        &["cmd.exe"]
    }
    #[cfg(not(windows))]
    {
        &["/bin/sh", "-c", "exec \"${SHELL:-/bin/sh}\" -l"]
    }
}
