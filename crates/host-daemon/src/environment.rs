use std::{
    ffi::{OsStr, OsString},
    path::Path,
};

pub(crate) fn initialize() {
    let executable = std::env::current_exe().expect("Host executable path");
    let base = directories::BaseDirs::new();
    let user = std::env::var_os("USER");
    let inherited = std::env::var_os("PATH");
    let path = provider_path(
        executable.parent().expect("Host executable directory"),
        base.as_ref().map(|base| base.home_dir()),
        user.as_deref(),
        inherited.as_deref(),
    )
    .expect("provider search path");
    // main calls this before starting Tokio or any other threads. The Host
    // owns its environment, including when launched directly by an updater.
    unsafe { std::env::set_var("PATH", path) };
    // Node/Bun providers do not automatically use Nix's configured CA bundle.
    // Expose it for OAuth token exchange and refresh without replacing an
    // explicitly configured Node trust store.
    if std::env::var_os("NODE_EXTRA_CA_CERTS").is_none()
        && let Some(certificates) =
            std::env::var_os("SSL_CERT_FILE").or_else(|| std::env::var_os("NIX_SSL_CERT_FILE"))
    {
        unsafe { std::env::set_var("NODE_EXTRA_CA_CERTS", certificates) };
    }
}

fn provider_path(
    directory: &Path,
    home: Option<&Path>,
    user: Option<&OsStr>,
    inherited: Option<&OsStr>,
) -> Result<OsString, std::env::JoinPathsError> {
    let mut paths = Vec::new();
    // Provider scripts with an env-node shebang also use the bundled runtime.
    if let Some(node) = agent_local_host::bundled_node_path(directory) {
        paths.push(node.parent().expect("Node has a parent").to_path_buf());
    }
    if let Some(home) = home {
        paths.push(home.join(".local/bin"));
        paths.push(home.join(".nix-profile/bin"));
    }
    if let Some(user) = user {
        paths.push(Path::new("/etc/profiles/per-user").join(user).join("bin"));
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
    if let Some(inherited) = inherited {
        paths.extend(std::env::split_paths(inherited));
    }
    let mut seen = std::collections::HashSet::new();
    paths.retain(|path| seen.insert(path.clone()));
    std::env::join_paths(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn providers_inherit_configured_ca_certificates() {
        if let Some(expected) = std::env::var_os("AGENT_TEST_PROVIDER_CA") {
            initialize();
            assert_eq!(
                std::env::var_os("NODE_EXTRA_CA_CERTS"),
                (expected != "unset").then_some(expected)
            );
            return;
        }
        for (node, ssl, nix, expected) in [
            (None, None, None, "unset"),
            (None, None, Some("/nix-ca.pem"), "/nix-ca.pem"),
            (
                None,
                Some("/ssl-ca.pem"),
                Some("/nix-ca.pem"),
                "/ssl-ca.pem",
            ),
            (
                Some("/node-ca.pem"),
                Some("/ssl-ca.pem"),
                Some("/nix-ca.pem"),
                "/node-ca.pem",
            ),
            (Some(""), Some("/ssl-ca.pem"), Some("/nix-ca.pem"), ""),
        ] {
            // Environment initialization runs before threads in the Host. Use
            // an isolated process so concurrent tests retain their own trust.
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    "environment::tests::providers_inherit_configured_ca_certificates",
                ])
                .env("AGENT_TEST_PROVIDER_CA", expected);
            for (key, value) in [
                ("NODE_EXTRA_CA_CERTS", node),
                ("SSL_CERT_FILE", ssl),
                ("NIX_SSL_CERT_FILE", nix),
            ] {
                command.env_remove(key);
                if let Some(value) = value {
                    command.env(key, value);
                }
            }
            let output = command.output().unwrap();
            assert!(
                output.status.success(),
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    #[test]
    fn direct_host_launch_finds_user_tools_and_bundled_node_with_a_minimal_path() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("Bex Dev.app/Contents/MacOS");
        let node = agent_local_host::bundled_node_path(&directory).unwrap();
        let home = root.path().join("user");
        let claude = home.join(".nix-profile/bin/claude");
        for binary in [&node, &claude] {
            std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
            std::fs::write(binary, b"fixture executable").unwrap();
        }
        let inherited = std::env::join_paths(["/usr/bin", "/bin", "/custom/bin"]).unwrap();
        let path = provider_path(
            &directory,
            Some(&home),
            Some(OsStr::new("fixture-user")),
            Some(&inherited),
        )
        .unwrap();
        let find = |name: &str| {
            std::env::split_paths(&path)
                .map(|directory| directory.join(name))
                .find(|candidate| candidate.is_file())
        };
        assert_eq!(find("claude"), Some(claude));
        assert_eq!(find("node"), Some(node));
        assert!(
            std::env::split_paths(&path)
                .any(|directory| directory == Path::new("/etc/profiles/per-user/fixture-user/bin"))
        );
        assert_eq!(
            std::env::split_paths(&path).last().unwrap(),
            Path::new("/custom/bin")
        );
        assert_eq!(
            provider_path(
                &directory,
                Some(&home),
                Some(OsStr::new("fixture-user")),
                Some(&path)
            )
            .unwrap(),
            path
        );
    }
}
