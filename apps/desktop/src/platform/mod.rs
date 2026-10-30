use agent_core::{state::Snapshot, store::Store};
use agent_local_host::{LocalHost, LocalHostRegistry, LocalHostState};
use agent_transport::transport::{Endpoint, Relays, Ticket};
use std::{
    path::PathBuf,
    process::{Command, Stdio},
    sync::Arc,
    time::Duration,
};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub(crate) mod task_menu;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "linux")]
use linux as os;
#[cfg(target_os = "macos")]
use macos as os;
#[cfg(target_os = "windows")]
use windows as os;

mod microphone;
#[cfg(target_os = "macos")]
pub(crate) use microphone::prepare_microphone;
pub(crate) use microphone::{Recording, RecordingEvent, start_recording};

pub(crate) fn state_dir() -> Result<PathBuf, String> {
    std::env::var_os("AGENT_STATE_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            directories::ProjectDirs::from(
                "app",
                "bex",
                if development_app() { "BEX-Dev" } else { "BEX" },
            )
            .map(|project| project.data_local_dir().to_path_buf())
        })
        .ok_or_else(|| "application data directory unavailable".into())
}

fn development_app() -> bool {
    #[cfg(target_os = "macos")]
    {
        objc2_foundation::NSBundle::mainBundle()
            .bundleIdentifier()
            .is_some_and(|id| id.to_string() == "app.bex.dev.desktop")
    }
    #[cfg(not(target_os = "macos"))]
    false
}

pub(crate) async fn ssh_invitation(
    destination: &str,
) -> Result<agent_protocol::models::Invitation, String> {
    use tokio::io::AsyncReadExt;
    let destination = agent_core::presentation::connections::validate_ssh_destination(destination)?;
    let mut child = tokio::process::Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=yes",
            "-o",
            "ConnectTimeout=10",
            "--",
            destination,
            "sh -lc 'exec host-daemon invite'",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| "SSHを起動できません。SSHがインストールされているか確認してください。")?;
    let result = tokio::time::timeout(Duration::from_secs(20), async {
        let mut contents = Vec::new();
        child.stdout.take().expect("SSH stdout is piped").take(64 * 1024 + 1)
            .read_to_end(&mut contents).await
            .map_err(|_| "SSHの応答を読み取れません。")?;
        if contents.len() > 64 * 1024 {
            return Err("SSHの応答が長すぎます。".into());
        }
        let status = child.wait().await.map_err(|_| "SSHの終了を確認できません。")?;
        if !status.success() {
            return Err("接続できません。SSHの鍵と接続先を確認し、サーバーでBex Hostを起動してください。".into());
        }
        serde_json::from_slice(&contents).map_err(|_| "Bexの接続情報を取得できません。接続先で host-daemon invite を実行できるか確認してください。".into())
    }).await;
    result.map_err(|_| "SSH接続がタイムアウトしました。".to_owned())?
}

/// One identity with separate local and remote endpoints. Views own sessions.
#[derive(Default)]
pub(crate) struct Connections {
    endpoint: tokio::sync::OnceCell<(PathBuf, Endpoint)>,
    local_endpoint: tokio::sync::OnceCell<(PathBuf, Endpoint)>,
    startup: tokio::sync::Mutex<()>,
}
impl Connections {
    pub(crate) async fn connect(
        self: &Arc<Self>,
        remote: Option<&str>,
        snapshot: Snapshot,
    ) -> anyhow::Result<Arc<Store>> {
        let device_name = tokio::task::spawn_blocking(agent_local_host::machine_name).await?;
        let startup = self.startup.lock().await;
        if let Some(remote) = remote {
            let ticket = remote.parse::<Ticket>()?;
            let endpoint = match self.endpoint.get() {
                Some((_, endpoint)) => endpoint,
                None => {
                    self.endpoint_for(&discover_local_host().await?.directory, false)
                        .await?
                }
            };
            drop(startup);
            return Ok(Arc::new(
                Store::connect(endpoint, &ticket, snapshot, None, &device_name).await?,
            ));
        }
        let store = Arc::new(self.connect_local(snapshot, &device_name).await?);
        self.recover_local(store.clone(), device_name);
        Ok(store)
    }

    async fn endpoint_for(
        &self,
        directory: &std::path::Path,
        local: bool,
    ) -> anyhow::Result<&Endpoint> {
        let directory = tokio::fs::canonicalize(directory).await?;
        let cell = if local {
            &self.local_endpoint
        } else {
            &self.endpoint
        };
        let (identity_directory, endpoint) = cell
            .get_or_try_init(|| async {
                let identity_directory = directory.clone();
                let identity = tokio::task::spawn_blocking(move || {
                    agent_local_host::load_local_identity(&identity_directory)
                })
                .await??;
                let endpoint = if local {
                    Endpoint::bind(identity, Relays::Loopback).await?
                } else {
                    Endpoint::bind(identity, Relays::Default).await?
                };
                Ok::<_, anyhow::Error>((directory.clone(), endpoint))
            })
            .await?;
        if identity_directory != &directory {
            return Err(anyhow::anyhow!(
                "Local Host changed; restart Bex to use its identity"
            ));
        }
        Ok(endpoint)
    }

    async fn connect_local(&self, snapshot: Snapshot, device_name: &str) -> anyhow::Result<Store> {
        let isolated = isolated_host()?;
        let mut child = None;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        let mut last_error = anyhow::anyhow!("Host startup timed out");
        let ready = async {
            loop {
                let location = discover_local_host().await?;
                match &location.state {
                    LocalHostState::Ready(ticket) => {
                        let endpoint = self.endpoint_for(&location.directory, true).await?;
                        let attempt = tokio::time::timeout(
                            Duration::from_secs(1),
                            Store::connect(endpoint, ticket, snapshot.clone(), None, device_name),
                        )
                        .await;
                        match attempt {
                            Ok(Ok(store)) => break Ok(store),
                            Ok(Err(error)) => last_error = error.into(),
                            Err(_) => {
                                last_error = anyhow::anyhow!("Host did not answer during startup")
                            }
                        }
                    }
                    LocalHostState::Stopped if child.is_none() => {
                        child = Some(start_host(&location.directory, isolated)?);
                    }
                    _ => {}
                }
                if let Some(child) = child.as_mut()
                    && let Some(status) = child.try_wait()?
                    && matches!(location.state, LocalHostState::Stopped)
                {
                    break Err(anyhow::anyhow!("Host exited during startup ({status})"));
                }
                if tokio::time::Instant::now() >= deadline {
                    break Err(last_error);
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
        .await;
        if let Some(mut child) = child {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        ready
    }

    fn recover_local(self: &Arc<Self>, store: Arc<Store>, device_name: String) {
        let connections = self.clone();
        tokio::spawn(async move {
            let mut snapshots = store.subscribe();
            let mut delay = Duration::from_millis(250);
            while snapshots
                .wait_for(|snapshot| !snapshot.connected)
                .await
                .is_ok()
            {
                tokio::time::sleep(delay).await;
                if snapshots.has_changed().is_err() {
                    break;
                }
                if snapshots.borrow().connected {
                    delay = Duration::from_millis(250);
                    continue;
                }
                let result = tokio::time::timeout(Duration::from_secs(10), async {
                    let location = discover_local_host().await?;
                    let LocalHostState::Ready(ticket) = &location.state else {
                        return Err(anyhow::anyhow!("Local Host is not ready"));
                    };
                    let endpoint = connections.endpoint_for(&location.directory, true).await?;
                    store.resume(endpoint, ticket, &device_name).await?;
                    Ok::<_, anyhow::Error>(())
                })
                .await;
                delay = if matches!(result, Ok(Ok(()))) {
                    Duration::from_millis(250)
                } else {
                    (delay * 2).min(Duration::from_secs(5))
                };
            }
        });
    }

    pub(crate) async fn close(&self) {
        for cell in [&self.local_endpoint, &self.endpoint] {
            if let Some((_, endpoint)) = cell.get() {
                endpoint.close().await;
            }
        }
    }
}

pub(crate) fn isolated_host() -> anyhow::Result<bool> {
    let value = match std::env::var("AGENT_ISOLATED_HOST") {
        Ok(value) => Some(value),
        Err(std::env::VarError::NotPresent) => None,
        Err(_) => anyhow::bail!("AGENT_ISOLATED_HOST must be 0 or 1"),
    };
    isolation_setting(
        value.as_deref(),
        std::env::var_os("AGENT_STATE_DIR").is_some(),
        development_app(),
    )
}

fn isolation_setting(
    value: Option<&str>,
    has_directory: bool,
    development: bool,
) -> anyhow::Result<bool> {
    match value {
        None => Ok(development),
        Some("0") => Ok(false),
        Some("1") if has_directory || development => Ok(true),
        Some("1") => Err(anyhow::anyhow!(
            "AGENT_ISOLATED_HOST=1 requires AGENT_STATE_DIR"
        )),
        _ => Err(anyhow::anyhow!("AGENT_ISOLATED_HOST must be 0 or 1")),
    }
}

async fn discover_local_host() -> anyhow::Result<LocalHost> {
    let preferred = state_dir().map_err(anyhow::Error::msg)?;
    let isolated = isolated_host()?;
    tokio::task::spawn_blocking(move || {
        let registry = if isolated {
            LocalHostRegistry::new(preferred.clone())
        } else {
            LocalHostRegistry::for_user()?
        };
        registry.resolve(&preferred)
    })
    .await?
}

fn start_host(directory: &std::path::Path, isolated: bool) -> anyhow::Result<std::process::Child> {
    let executable = std::env::var_os("AGENT_HOST_DAEMON")
        .map(PathBuf::from)
        .map(Ok)
        .unwrap_or_else(|| std::env::current_exe().map(agent_local_host::bundled_host_path))?;
    let mut command = Command::new(executable);
    command
        .arg("--state-dir")
        .arg(directory)
        .arg("--codex")
        .arg(std::env::var_os("AGENT_CODEX").unwrap_or_else(|| "codex".into()))
        .arg("--claude")
        .arg(std::env::var_os("AGENT_CLAUDE").unwrap_or_else(|| "claude".into()))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if isolated {
        command.arg("--isolated");
    }
    let accounts = std::env::var_os("AGENT_ACCOUNT_STATE_DIR").map(PathBuf::from);
    let accounts = if accounts.is_none() && development_app() {
        let registry = LocalHostRegistry::for_user()?;
        Some(registry.resolve(registry.directory())?.directory)
    } else {
        accounts
    };
    if let Some(directory) = accounts {
        command.arg("--account-state-dir").arg(directory);
    }
    os::prepare_host(&mut command);
    command.spawn().map_err(Into::into)
}

pub(crate) fn choose_files() -> Option<Vec<PathBuf>> {
    rfd::FileDialog::new().pick_files()
}
pub(crate) fn choose_folder() -> Option<PathBuf> {
    rfd::FileDialog::new().pick_folder()
}
pub(crate) fn choose_destination(name: &str) -> Option<PathBuf> {
    rfd::FileDialog::new().set_file_name(name).save_file()
}

#[cfg(test)]
mod tests {
    use super::isolation_setting;

    #[test]
    fn dev_launch_is_isolated_without_environment_and_explicit_settings_are_validated() {
        for (value, directory, development, expected) in [
            (None, false, false, false),
            (None, true, false, false),
            (None, false, true, true),
            (None, true, true, true),
            (Some("0"), false, false, false),
            (Some("0"), true, false, false),
            (Some("0"), false, true, false),
            (Some("0"), true, true, false),
            (Some("1"), true, false, true),
            (Some("1"), false, true, true),
            (Some("1"), true, true, true),
        ] {
            assert_eq!(
                isolation_setting(value, directory, development).unwrap(),
                expected
            );
        }
        assert!(isolation_setting(Some("1"), false, false).is_err());
        for invalid in ["", "true", "2"] {
            assert!(isolation_setting(Some(invalid), true, true).is_err());
        }
    }
}
