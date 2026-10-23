use std::{os::unix::process::CommandExt, process::Command};

pub(super) fn prepare_host(command: &mut Command) {
    command.process_group(0);
    let mut paths = Vec::new();
    // Provider scripts with an env-node shebang also use the bundled runtime.
    if let Ok(executable) = std::env::current_exe()
        && let Some(node) = host_daemon::bundled_node_path(&executable)
        && let Some(directory) = node.parent()
    {
        paths.push(directory.to_path_buf());
    }
    // Preserve the caller's provider selection. User installation directories
    // supplement Finder's minimal PATH instead of overriding a terminal's PATH.
    if let Some(inherited) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&inherited));
    }
    if let Some(base) = directories::BaseDirs::new() {
        paths.push(base.home_dir().join(".local/bin"));
        paths.push(base.home_dir().join(".nix-profile/bin"));
    }
    if let Some(user) = std::env::var_os("USER") {
        paths.push(
            std::path::Path::new("/etc/profiles/per-user")
                .join(user)
                .join("bin"),
        );
    }
    paths.extend(
        [
            "/opt/homebrew/bin",
            "/usr/local/bin",
            "/run/current-system/sw/bin",
            "/usr/bin",
            "/bin",
            "/usr/sbin",
            "/sbin",
        ]
        .map(Into::into),
    );
    if let Ok(path) = std::env::join_paths(paths) {
        command.env("PATH", path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_preserves_inherited_provider_priority_before_fallback_directories() {
        let inherited = std::env::var_os("PATH").expect("test process PATH");
        let expected: Vec<_> = std::env::split_paths(&inherited).collect();
        assert!(!expected.is_empty());
        let mut command = Command::new("host-daemon");
        prepare_host(&mut command);
        let path = command
            .get_envs()
            .find(|(key, _)| *key == "PATH")
            .unwrap()
            .1
            .unwrap();
        let actual: Vec<_> = std::env::split_paths(path).collect();
        // Only the app's private Node runtime may precede the inherited PATH.
        let offset = usize::from(
            std::env::current_exe()
                .ok()
                .and_then(|executable| host_daemon::bundled_node_path(&executable))
                .is_some(),
        );
        assert_eq!(
            &actual[offset..offset + expected.len()],
            expected.as_slice()
        );
        let fallback = directories::BaseDirs::new()
            .unwrap()
            .home_dir()
            .join(".local/bin");
        assert!(actual[offset + expected.len()..].contains(&fallback));
    }
}
