use super::*;
use agent_local_host::LocalClientAccess;
use agent_protocol::models::RemoteHost;
use agent_transport::transport::Identity;
use std::collections::BTreeMap;

pub(super) struct WslConnection {
    pub distro: String,
    pub remote: RemoteHost,
    pub endpoint: Endpoint,
}

pub(super) type WslConnections = tokio::sync::Mutex<BTreeMap<String, WslConnection>>;

pub(crate) struct ProjectSelection {
    pub remote: Option<RemoteHost>,
    pub cwd: String,
}

pub(crate) fn choice(distro: &str) -> RemoteHost {
    RemoteHost {
        id: format!("wsl:{distro}"),
        name: format!("WSL ({distro})"),
        ticket: String::new(),
    }
}

pub(crate) async fn distributions() -> anyhow::Result<Vec<String>> {
    if !cfg!(windows) {
        return Ok(Vec::new());
    }
    let output = run(None, &["--list", "--quiet"], None).await?;
    agent_local_host::wsl::parse_distros(&output)
}

async fn run(distro: Option<&str>, args: &[&str], input: Option<&[u8]>) -> anyhow::Result<Vec<u8>> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let executable =
        PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into()))
            .join("System32")
            .join("wsl.exe");
    let mut command = tokio::process::Command::new(executable);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    if let Some(distro) = distro {
        agent_local_host::wsl::validate_distro(distro)?;
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
            child.stdin.take().unwrap().write_all(input).await?;
        }
        drop(child.stdin.take());
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let read = |reader: tokio::process::ChildStdout| async move {
            let mut bytes = Vec::new();
            reader.take(65537).read_to_end(&mut bytes).await?;
            Ok::<_, std::io::Error>(bytes)
        };
        let mut error = Vec::new();
        let mut stderr = stderr.take(65537);
        let (stdout, _) = tokio::try_join!(read(stdout), stderr.read_to_end(&mut error))?;
        anyhow::ensure!(
            stdout.len() <= 65536 && error.len() <= 65536,
            "WSL response is too large"
        );
        let status = child.wait().await?;
        anyhow::ensure!(
            status.success(),
            "WSL: {}",
            String::from_utf8_lossy(&error).trim()
        );
        Ok::<_, anyhow::Error>(stdout)
    })
    .await
    .map_err(|_| anyhow::anyhow!("WSL startup timed out"))?
}

async fn bootstrap(distro: &str, allow_upgrade: bool) -> anyhow::Result<LocalClientAccess> {
    let bundle = std::env::current_exe()?.with_file_name("wsl-runtime");
    anyhow::ensure!(
        bundle.join("revision").is_file(),
        "The WSL runtime is missing; install the complete Windows package"
    );
    // Positional arguments protect spaces and shell metacharacters in the
    // Windows install path; the login shell supplies the user's provider PATH.
    let path = bundle
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Invalid WSL runtime path"))?;
    let bundle_env = format!("BEX_WSL_BUNDLE={}", path.replace('\\', "/"));
    let scope = if isolated_host()? {
        let directory = state_dir().map_err(anyhow::Error::msg)?;
        ring::digest::digest(
            &ring::digest::SHA256,
            directory.as_os_str().as_encoded_bytes(),
        )
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
    } else {
        String::new()
    };
    let scope_env = format!("BEX_WSL_SCOPE={scope}");
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
        Some(include_bytes!(
            "../../../../crates/agent-local-host/src/wsl-bootstrap.sh"
        )),
    )
    .await?;
    LocalClientAccess::from_process_output(bytes)
}

impl Connections {
    pub(crate) async fn initial_environment(&self) -> anyhow::Result<Option<RemoteHost>> {
        use agent_core::persistence::LocalEnvironment;
        let path = state_dir()
            .map_err(anyhow::Error::msg)?
            .join("local-environment.json");
        let preference = tokio::task::spawn_blocking(move || match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(anyhow::Error::from),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(LocalEnvironment::Native)
            }
            Err(error) => Err(error.into()),
        })
        .await??;
        match preference {
            LocalEnvironment::Native => Ok(None),
            LocalEnvironment::Wsl { distro } => Ok(Some(self.wsl_host(&distro).await?)),
        }
    }

    pub(crate) async fn remember_environment(
        &self,
        remote: Option<&RemoteHost>,
    ) -> anyhow::Result<()> {
        use agent_core::persistence::LocalEnvironment;
        if !cfg!(windows) {
            return Ok(());
        }
        let preference = match remote {
            None => LocalEnvironment::Native,
            Some(remote) => match remote.id.strip_prefix("wsl:") {
                Some(distro) => LocalEnvironment::Wsl {
                    distro: distro.into(),
                },
                None => return Ok(()),
            },
        };
        let path = state_dir()
            .map_err(anyhow::Error::msg)?
            .join("local-environment.json");
        tokio::task::spawn_blocking(move || agent_local_host::save_private_json(&path, &preference))
            .await?
    }

    pub(crate) async fn wsl_host(&self, distro: &str) -> anyhow::Result<RemoteHost> {
        let mut hosts = self.wsl.lock().await;
        let distro = match hosts.keys().find(|name| name.eq_ignore_ascii_case(distro)) {
            Some(name) => name.clone(),
            None => distributions()
                .await?
                .into_iter()
                .find(|name| name.eq_ignore_ascii_case(distro))
                .ok_or_else(|| anyhow::anyhow!("WSL distribution is not installed: {distro}"))?,
        };
        let distro = distro.as_str();
        // Only a newly selected app instance may replace the old image. A
        // still-running older app must not undo a newer app's Host update.
        let access = bootstrap(distro, !hosts.contains_key(distro)).await?;
        let remote = RemoteHost {
            ticket: access.ticket.to_string(),
            ..choice(distro)
        };
        if let Some(host) = hosts.get_mut(distro) {
            anyhow::ensure!(
                host.endpoint.node_id() == Identity::from_bytes(access.secret).node_id(),
                "WSL Host identity changed; restart Bex"
            );
            host.remote = remote.clone();
        } else {
            let endpoint =
                Endpoint::bind(Identity::from_bytes(access.secret), Relays::Disabled).await?;
            hosts.insert(
                distro.into(),
                WslConnection {
                    distro: distro.into(),
                    remote: remote.clone(),
                    endpoint,
                },
            );
        }
        Ok(remote)
    }

    pub(super) async fn wsl_target(&self, ticket: &Ticket) -> Option<String> {
        self.wsl
            .lock()
            .await
            .values()
            .find(|host| {
                host.remote
                    .ticket
                    .parse::<Ticket>()
                    .is_ok_and(|current| current.node_id() == ticket.node_id())
            })
            .map(|host| host.distro.clone())
    }

    pub(super) async fn connect_wsl(
        self: &Arc<Self>,
        distro: String,
        snapshot: Snapshot,
        device_name: &str,
    ) -> anyhow::Result<Arc<Store>> {
        let remote = self.wsl_host(&distro).await?;
        let endpoint = self.wsl.lock().await.get(&distro).unwrap().endpoint.clone();
        let store = Arc::new(
            Store::connect(
                &endpoint,
                &remote.ticket.parse()?,
                snapshot,
                None,
                device_name,
            )
            .await?,
        );
        self.recover_wsl(store.clone(), distro, device_name.into());
        Ok(store)
    }

    fn recover_wsl(self: &Arc<Self>, store: Arc<Store>, distro: String, device_name: String) {
        let connections = self.clone();
        tokio::spawn(async move {
            let mut snapshots = store.subscribe();
            while snapshots
                .wait_for(|snapshot| !snapshot.connected)
                .await
                .is_ok()
            {
                tokio::time::sleep(Duration::from_secs(1)).await;
                if snapshots.has_changed().is_err() {
                    break;
                }
                if snapshots.borrow().connected {
                    continue;
                }
                let result = async {
                    let remote = connections.wsl_host(&distro).await?;
                    let endpoint = connections
                        .wsl
                        .lock()
                        .await
                        .get(&distro)
                        .unwrap()
                        .endpoint
                        .clone();
                    store
                        .resume(&endpoint, &remote.ticket.parse()?, &device_name)
                        .await?;
                    Ok::<_, anyhow::Error>(())
                }
                .await;
                if let Err(error) = result {
                    tracing::warn!(target: "agent", operation = "wsl.reconnect", message = %error);
                }
            }
        });
    }

    pub(crate) async fn project_target(&self, path: PathBuf) -> anyhow::Result<ProjectSelection> {
        let cwd = path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid project path"))?;
        if cfg!(windows)
            && let Some(path) = agent_local_host::wsl::parse_unc(cwd)?
        {
            return Ok(ProjectSelection {
                remote: Some(self.wsl_host(&path.distro).await?),
                cwd: path.linux_path,
            });
        }
        Ok(ProjectSelection {
            remote: None,
            cwd: cwd.into(),
        })
    }
}
