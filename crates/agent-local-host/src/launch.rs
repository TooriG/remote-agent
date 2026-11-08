//! Which local Host a client on this machine uses, and starting the Host
//! bundled beside the client when that Host is stopped.
use crate::{LocalHost, LocalHostRegistry, LocalHostState};
use agent_transport::transport::Ticket;
use anyhow::{Result, anyhow, bail};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Duration,
};

const STARTUP: Duration = Duration::from_secs(10);
const ATTEMPT: Duration = Duration::from_secs(1);

/// The state directory and whether its Host is isolated from the user's normal Host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostScope {
    pub directory: PathBuf,
    pub isolated: bool,
}

impl HostScope {
    /// `AGENT_STATE_DIR` and `AGENT_ISOLATED_HOST` select the scope. Release
    /// builds share the normal Host; source development uses an isolated one.
    pub fn from_environment(development: bool) -> Result<Self> {
        let configured = std::env::var_os("AGENT_STATE_DIR").map(PathBuf::from);
        let value = match std::env::var("AGENT_ISOLATED_HOST") {
            Ok(value) => Some(value),
            Err(std::env::VarError::NotPresent) => None,
            Err(_) => bail!("AGENT_ISOLATED_HOST must be 0 or 1"),
        };
        let isolated = isolation(value.as_deref(), configured.is_some(), development)?;
        let directory = match configured {
            Some(directory) => directory,
            None => crate::state_directory(development)?,
        };
        Ok(Self {
            directory,
            isolated,
        })
    }

    fn registry(&self) -> Result<LocalHostRegistry> {
        if self.isolated {
            Ok(LocalHostRegistry::new(self.directory.clone()))
        } else {
            LocalHostRegistry::for_user()
        }
    }

    /// The registered Host, or where this scope's Host starts. File access blocks.
    pub fn resolve(&self) -> Result<LocalHost> {
        self.registry()?.resolve(&self.directory)
    }

    pub async fn discover(&self) -> Result<LocalHost> {
        let scope = self.clone();
        tokio::task::spawn_blocking(move || scope.resolve()).await?
    }
}

fn isolation(value: Option<&str>, has_directory: bool, development: bool) -> Result<bool> {
    match value {
        None => Ok(development),
        Some("0") => Ok(false),
        Some("1") if has_directory || development => Ok(true),
        Some("1") => Err(anyhow!("AGENT_ISOLATED_HOST=1 requires AGENT_STATE_DIR")),
        _ => Err(anyhow!("AGENT_ISOLATED_HOST must be 0 or 1")),
    }
}

/// `AGENT_HOST_DAEMON`, or the Host bundled beside the running client.
pub fn host_executable() -> Result<PathBuf> {
    match std::env::var_os("AGENT_HOST_DAEMON") {
        Some(path) => Ok(path.into()),
        None => Ok(crate::bundled_host_path(std::env::current_exe()?)),
    }
}

/// The ready Host's directory and ticket reach `attempt`; a failed or slow
/// attempt is retried until the startup deadline. A stopped Host is started
/// once from `executable`, detached so it outlives this client.
pub async fn connect<T, F, Fut>(scope: &HostScope, executable: &Path, mut attempt: F) -> Result<T>
where
    F: FnMut(PathBuf, Ticket) -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let mut child: Option<Child> = None;
    let deadline = tokio::time::Instant::now() + STARTUP;
    let mut last_error = anyhow!("Host startup timed out");
    let result = loop {
        let host = match scope.discover().await {
            Ok(host) => host,
            Err(error) => break Err(error),
        };
        match host.state {
            LocalHostState::Ready(ticket) => {
                match tokio::time::timeout(ATTEMPT, attempt(host.directory, ticket)).await {
                    Ok(Ok(value)) => break Ok(value),
                    Ok(Err(error)) => last_error = error,
                    Err(_) => last_error = anyhow!("Host did not answer during startup"),
                }
            }
            LocalHostState::Stopped => match child.as_mut() {
                None => match start(executable, &host.directory, scope.isolated) {
                    Ok(started) => child = Some(started),
                    Err(error) => break Err(error),
                },
                Some(started) => match started.try_wait() {
                    Ok(Some(status)) => {
                        break Err(anyhow!("Host exited during startup ({status})"));
                    }
                    Ok(None) => {}
                    Err(error) => break Err(error.into()),
                },
            },
            LocalHostState::Starting => {}
        }
        if tokio::time::Instant::now() >= deadline {
            break Err(last_error);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    if let Some(mut child) = child {
        // Reap the detached Host when it exits; it is not this client's child to stop.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
    result
}

fn start(executable: &Path, directory: &Path, isolated: bool) -> Result<Child> {
    let variable =
        |name, default: &str| std::env::var_os(name).unwrap_or_else(|| OsString::from(default));
    let mut command = Command::new(executable);
    command
        .arg("--state-dir")
        .arg(directory)
        .arg("--codex")
        .arg(variable("AGENT_CODEX", "codex"))
        .arg("--claude")
        .arg(variable("AGENT_CLAUDE", "claude"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if isolated {
        command.arg("--isolated");
    }
    if let Some(directory) = std::env::var_os("AGENT_ACCOUNT_STATE_DIR") {
        command.arg("--account-state-dir").arg(directory);
    }
    detach(&mut command);
    Ok(command.spawn()?)
}

/// A process that outlives the client and receives no console or terminal signals.
pub(crate) fn detach(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW
        command.creation_flags(0x00000200 | 0x08000000);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HostLease;
    use agent_transport::transport::{Endpoint, Identity, Relays};

    #[test]
    fn release_channels_share_the_normal_host_and_source_development_is_isolated() {
        for directory in [false, true] {
            for development in [false, true] {
                assert_eq!(
                    isolation(None, directory, development).unwrap(),
                    development
                );
                assert!(!isolation(Some("0"), directory, development).unwrap());
            }
        }
        assert!(isolation(Some("1"), true, false).unwrap());
        assert!(isolation(Some("1"), false, true).unwrap());
        let error = isolation(Some("1"), false, false).unwrap_err();
        assert!(error.to_string().contains("AGENT_STATE_DIR"));
        for invalid in ["", "true", "2"] {
            let error = isolation(Some(invalid), true, true).unwrap_err();
            assert!(error.to_string().contains("0 or 1"));
        }
    }

    fn scope(directory: &Path) -> HostScope {
        HostScope {
            directory: directory.to_owned(),
            isolated: true,
        }
    }

    #[tokio::test]
    async fn a_ready_host_is_used_without_starting_another_and_failed_attempts_retry() {
        let directory = tempfile::tempdir().unwrap();
        let lease = HostLease::isolated(directory.path()).unwrap();
        let endpoint = Endpoint::bind(Identity::generate(), Relays::Loopback)
            .await
            .unwrap();
        lease.publish(&endpoint.ticket()).unwrap();
        let mut attempts = 0;
        let (path, ticket) = connect(
            &scope(directory.path()),
            Path::new("/nonexistent/host-daemon"),
            |path, ticket| {
                attempts += 1;
                let retry = attempts < 3;
                async move {
                    if retry {
                        bail!("not answering yet");
                    }
                    Ok((path, ticket))
                }
            },
        )
        .await
        .unwrap();
        assert_eq!(attempts, 3);
        assert_eq!(path, dunce::canonicalize(directory.path()).unwrap());
        assert!(ticket == endpoint.ticket());
        endpoint.close().await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_stopped_host_is_started_with_its_scope_and_an_early_exit_fails_startup() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let state = directory.path().join("state");
        let executable = directory.path().join("host-daemon");
        let arguments = directory.path().join("arguments");
        std::fs::write(
            &executable,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nexit 3\n",
                arguments.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let error = connect(&scope(&state), &executable, |_, _| async { Ok(()) })
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("exited during startup"),
            "{error}"
        );
        let arguments = std::fs::read_to_string(arguments).unwrap();
        let arguments: Vec<_> = arguments.lines().collect();
        assert_eq!(arguments[..2], ["--state-dir", state.to_str().unwrap()]);
        assert!(arguments.contains(&"--isolated"));
    }
}
