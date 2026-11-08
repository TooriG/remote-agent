//! Windows folder selections identify both a Linux path and its owning distro.
//! Keep these paths out of Windows Git and do not resolve them in another distro.
//!
//! A distro's projects run on a Linux Host inside it, started from the runtime
//! bundled beside the Windows client and reached as another Host of this PC.
use crate::{LocalClientAccess, launch::HostScope};
use anyhow::{Context, Result, bail, ensure};
use std::{path::Path, process::Stdio, time::Duration};

const HOST_PREFIX: &str = "wsl:";

/// The Host id a client lists `distro`'s Host under.
pub fn host_id(distro: &str) -> String {
    format!("{HOST_PREFIX}{distro}")
}

/// The distro of a Host id [`host_id`] made.
pub fn host_distro(id: &str) -> Option<&str> {
    id.strip_prefix(HOST_PREFIX)
}

/// The name a client shows for `distro`'s Host.
pub fn host_name(distro: &str) -> String {
    format!("WSL ({distro})")
}

/// The distro a folder picked on Windows belongs to, with its Linux path;
/// `None` for a Windows folder, and on every other system.
pub fn folder(path: &str) -> Result<Option<WslPath>> {
    if cfg!(windows) {
        parse_unc(path)
    } else {
        Ok(None)
    }
}

/// The distros installed for this user; none off Windows.
pub async fn distributions() -> Result<Vec<String>> {
    if !cfg!(windows) {
        return Ok(Vec::new());
    }
    parse_distros(&run(None, &["--list", "--quiet"], None).await?)
}

/// Starts `distro`'s Host from the runtime bundled beside `executable` when it
/// is stopped, or reaches the one running, and returns its local client's
/// access. Only `allow_upgrade` lets a newer bundle replace a running older
/// Host, so a still-running older client cannot undo a newer one's update.
/// An isolated `scope` keeps its own Linux Host state, as the Windows Host does.
pub async fn bootstrap(
    distro: &str,
    executable: &Path,
    scope: &HostScope,
    allow_upgrade: bool,
) -> Result<LocalClientAccess> {
    let bundle = executable.with_file_name("wsl-runtime");
    ensure!(
        bundle.join("revision").is_file(),
        "The WSL runtime is missing; install the complete Windows package"
    );
    // Positional arguments protect spaces and shell metacharacters in the
    // Windows install path; the login shell supplies the user's provider PATH.
    let path = bundle.to_str().context("Invalid WSL runtime path")?;
    let bundle_env = format!("BEX_WSL_BUNDLE={}", path.replace('\\', "/"));
    let scope_env = format!("BEX_WSL_SCOPE={}", isolation_scope(scope));
    let upgrade_env = format!("BEX_WSL_ALLOW_UPGRADE={}", u8::from(allow_upgrade));
    let bytes = run(
        Some(distro),
        &[
            "--exec",
            "env",
            &bundle_env,
            &scope_env,
            &upgrade_env,
            "sh",
            "-c",
            "exec \"${SHELL:-/bin/sh}\" -lc 'exec sh -s -- \"$BEX_WSL_BUNDLE\"'",
        ],
        Some(include_bytes!("wsl-bootstrap.sh")),
    )
    .await?;
    LocalClientAccess::from_process_output(bytes)
}

/// The Linux Host state an isolated Windows scope keeps apart: the digest of
/// its state directory. Empty for the user's normal Host.
fn isolation_scope(scope: &HostScope) -> String {
    if !scope.isolated {
        return String::new();
    }
    ring::digest::digest(
        &ring::digest::SHA256,
        scope.directory.as_os_str().as_encoded_bytes(),
    )
    .as_ref()
    .iter()
    .map(|byte| format!("{byte:02x}"))
    .collect()
}

/// `wsl.exe` with `args`, in `distro` when given, fed `input`; its output.
async fn run(distro: Option<&str>, args: &[&str], input: Option<&[u8]>) -> Result<Vec<u8>> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    const LIMIT: u64 = 64 * 1024;
    let executable = std::path::PathBuf::from(
        std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into()),
    )
    .join("System32")
    .join("wsl.exe");
    let mut command = tokio::process::Command::new(executable);
    // CREATE_NO_WINDOW: no console flashes over the app.
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    if let Some(distro) = distro {
        validate_distro(distro)?;
        command.args(["--distribution", distro]);
    }
    let mut child = command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    tokio::time::timeout(Duration::from_secs(60), async {
        if let Some(input) = input {
            child
                .stdin
                .take()
                .context("WSL stdin")?
                .write_all(input)
                .await?;
        }
        drop(child.stdin.take());
        let mut stdout = child.stdout.take().context("WSL stdout")?.take(LIMIT + 1);
        let mut stderr = child.stderr.take().context("WSL stderr")?.take(LIMIT + 1);
        let (mut output, mut error) = (Vec::new(), Vec::new());
        tokio::try_join!(
            stdout.read_to_end(&mut output),
            stderr.read_to_end(&mut error)
        )?;
        ensure!(
            output.len() as u64 <= LIMIT && error.len() as u64 <= LIMIT,
            "WSL response is too large"
        );
        let status = child.wait().await?;
        ensure!(
            status.success(),
            "WSL: {}",
            String::from_utf8_lossy(&error).trim()
        );
        Ok(output)
    })
    .await
    .map_err(|_| anyhow::anyhow!("WSL startup timed out"))?
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WslPath {
    pub distro: String,
    pub linux_path: String,
}

pub fn to_unc(distro: &str, linux_path: &str) -> Result<String> {
    validate_distro(distro)?;
    if !linux_path.starts_with('/') || linux_path.contains('\\') {
        bail!("WSL folder requires a Linux absolute path");
    }
    let path = format!(r"\\wsl.localhost\{distro}{}", linux_path.replace('/', "\\"));
    parse_unc(&path)?;
    Ok(path)
}

pub fn parse_unc(input: &str) -> Result<Option<WslPath>> {
    let path = input.trim().replace('\\', "/");
    let lower = path.to_ascii_lowercase();
    let Some(prefix) = [
        "//wsl.localhost/",
        "//wsl$/",
        "//?/UNC/wsl.localhost/",
        "//?/UNC/wsl$/",
    ]
    .into_iter()
    .find(|prefix| lower.starts_with(&prefix.to_ascii_lowercase())) else {
        return Ok(None);
    };
    let (distro, rest) = path[prefix.len()..]
        .split_once('/')
        .unwrap_or((&path[prefix.len()..], ""));
    validate_distro(distro)?;
    if rest.chars().any(char::is_control) || rest.split('/').any(|part| matches!(part, "." | ".."))
    {
        bail!("WSL folder requires an absolute path without parent components");
    }
    Ok(Some(WslPath {
        distro: distro.into(),
        linux_path: format!("/{}", rest.trim_start_matches('/')),
    }))
}

pub fn validate_distro(name: &str) -> Result<()> {
    if name.is_empty()
        || !name.starts_with(|c: char| c.is_ascii_alphanumeric())
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._- ".contains(c))
        || name.ends_with(' ')
    {
        bail!("Invalid WSL distribution name");
    }
    Ok(())
}

/// wsl.exe emits UTF-16LE even when its stdout is redirected to a pipe.
pub fn parse_distros(bytes: &[u8]) -> Result<Vec<String>> {
    let text = if bytes.starts_with(&[0xff, 0xfe]) || bytes.contains(&0) {
        if !bytes.len().is_multiple_of(2) {
            bail!("Invalid WSL distribution list");
        }
        String::from_utf16(
            &bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes(*b))
                .collect::<Vec<_>>(),
        )?
    } else {
        String::from_utf8(bytes.to_vec())?
    };
    text.trim_start_matches('\u{feff}')
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| {
            validate_distro(name)?;
            Ok(name.to_owned())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_selection_preserves_owning_distro_and_linux_filename() {
        for input in [
            r"\\wsl.localhost\Ubuntu\home\user\my repo",
            r"\\wsl$\Ubuntu\home\user\my repo",
            "//WSL.LOCALHOST/Ubuntu/home/user/my repo",
            r"\\?\UNC\wsl.localhost\Ubuntu\home\user\my repo",
        ] {
            assert_eq!(
                parse_unc(input).unwrap(),
                Some(WslPath {
                    distro: "Ubuntu".into(),
                    linux_path: "/home/user/my repo".into()
                })
            );
        }
        assert_eq!(
            parse_unc(r"\\wsl$\Debian").unwrap().unwrap().linux_path,
            "/"
        );
        for input in [r"C:\repo", "/home/user/repo", r"\\server\share\repo"] {
            assert_eq!(parse_unc(input).unwrap(), None);
        }
        for input in [
            r"\\wsl$\Ubuntu$HOME\repo",
            r"\\wsl$\Ubuntu\..\repo",
            "//wsl$/Ubuntu/home/\nrepo",
            "//wsl$//home",
        ] {
            assert!(parse_unc(input).is_err(), "{input}");
        }
    }

    #[test]
    fn distribution_discovery_accepts_pipe_encodings_and_rejects_corruption() {
        let text = "\u{feff}Ubuntu\r\nDebian\r\nMy Distro\r\n";
        let utf16: Vec<_> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(
            parse_distros(&utf16).unwrap(),
            ["Ubuntu", "Debian", "My Distro"]
        );
        let without_bom: Vec<_> = "Ubuntu\r\nDebian\r\n"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(parse_distros(&without_bom).unwrap(), ["Ubuntu", "Debian"]);
        assert_eq!(
            parse_distros(b"Ubuntu\nDebian\n").unwrap(),
            ["Ubuntu", "Debian"]
        );
        assert!(parse_distros(&[0xff, 0xfe, 1]).is_err());
        assert!(parse_distros(b"--exec\n").is_err());
        assert!(parse_distros(b"Ubuntu$HOME\n").is_err());
        assert!(parse_distros(b"Ubuntu\nBad\tName\n").is_err());
    }

    #[test]
    fn a_distro_host_is_listed_under_its_own_id_and_name() {
        assert_eq!(host_id("Ubuntu 24.04"), "wsl:Ubuntu 24.04");
        assert_eq!(host_distro(&host_id("Ubuntu 24.04")), Some("Ubuntu 24.04"));
        assert_eq!(host_distro("local"), None);
        assert_eq!(host_name("Debian"), "WSL (Debian)");
        // Only Windows has distros; a UNC path elsewhere is an ordinary path.
        assert_eq!(
            folder(r"\\wsl$\Ubuntu\home").unwrap().is_some(),
            cfg!(windows)
        );
        assert_eq!(folder(r"C:\repo").unwrap(), None);
    }

    #[test]
    fn isolated_scopes_keep_their_own_linux_host_state() {
        let scope = |directory: &str, isolated| HostScope {
            directory: directory.into(),
            isolated,
        };
        assert_eq!(isolation_scope(&scope(r"C:\state", false)), "");
        let isolated = isolation_scope(&scope(r"C:\state", true));
        // The bootstrap accepts only lowercase hex.
        assert_eq!(isolated.len(), 64);
        assert!(
            isolated
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
        assert_eq!(isolated, isolation_scope(&scope(r"C:\state", true)));
        assert_ne!(isolated, isolation_scope(&scope(r"C:\other", true)));
    }

    #[tokio::test]
    async fn a_missing_bundled_runtime_fails_before_wsl_runs() {
        let directory = tempfile::tempdir().unwrap();
        // The access holds a secret, so it has no Debug for `unwrap_err`.
        let Err(error) = bootstrap(
            "Ubuntu",
            &directory.path().join("app.exe"),
            &HostScope {
                directory: directory.path().into(),
                isolated: true,
            },
            true,
        )
        .await
        else {
            panic!("a bundle without its runtime cannot start a WSL Host");
        };
        assert!(
            error.to_string().contains("WSL runtime is missing"),
            "{error}"
        );
    }

    #[test]
    fn folder_picker_starts_in_the_selected_wsl_environment() {
        assert_eq!(
            to_unc("Ubuntu", "/home/user/my repo").unwrap(),
            r"\\wsl.localhost\Ubuntu\home\user\my repo"
        );
        assert_eq!(
            parse_unc(&to_unc("Debian", "/").unwrap()).unwrap().unwrap(),
            WslPath {
                distro: "Debian".into(),
                linux_path: "/".into()
            }
        );
        for path in [
            "home/user",
            r"C:\repo",
            "/home/../user",
            "/home/\nrepo",
            r"/home/user\repo",
        ] {
            assert!(to_unc("Ubuntu", path).is_err(), "{path}");
        }
        assert!(to_unc("--exec", "/home").is_err());
    }
}
