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
        id: agent_local_host::wsl::host_id(distro),
        name: agent_local_host::wsl::host_name(distro),
        ticket: String::new(),
    }
}

pub(crate) async fn distributions() -> anyhow::Result<Vec<String>> {
    agent_local_host::wsl::distributions().await
}

async fn bootstrap(distro: &str, allow_upgrade: bool) -> anyhow::Result<LocalClientAccess> {
    let scope = agent_local_host::launch::HostScope {
        directory: state_dir().map_err(anyhow::Error::msg)?,
        isolated: isolated_host()?,
    };
    agent_local_host::wsl::bootstrap(distro, &std::env::current_exe()?, &scope, allow_upgrade).await
}

impl Connections {
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
                .wait_for(|snapshot| !snapshot.connection.connected)
                .await
                .is_ok()
            {
                tokio::time::sleep(Duration::from_secs(1)).await;
                if snapshots.has_changed().is_err() {
                    break;
                }
                if snapshots.borrow().connection.connected {
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
        if let Some(path) = agent_local_host::wsl::folder(cwd)? {
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
