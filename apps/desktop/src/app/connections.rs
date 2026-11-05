use super::*;
use agent_core::presentation::connection_list::{
    ConnectionSelection, resolve_connection_selection,
};

fn selection_path() -> Result<PathBuf, String> {
    Ok(platform::state_dir()?.join("connection-selection.json"))
}

impl Desktop {
    pub(super) async fn restore_connections(
        runtime: Runtime,
    ) -> Result<(ConnectionSelection, Option<RemoteHost>), String> {
        let saved = tokio::task::spawn_blocking(|| match std::fs::read(selection_path()?) {
            Ok(bytes) => serde_json::from_slice::<ConnectionSelection>(&bytes)
                .map_err(|error| error.to_string()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(ConnectionSelection {
                host_id: Some("local".into()),
                all: false,
            }),
            Err(error) => Err(error.to_string()),
        })
        .await
        .map_err(|error| error.to_string())??;
        let local = runtime.sessions.management(runtime.clone()).await?;
        local
            .pane
            .dispatch(Intent::LoadHostManagement(op::LoadHostManagement {}))
            .await
            .map_err(|error| error.to_string())?;
        let snapshot = local.pane.snapshot().map_err(|error| error.to_string())?;
        let remotes = &snapshot.connection.management.remotes;
        let mut hosts = vec!["local".to_string()];
        hosts.extend(remotes.iter().map(|host| host.id.clone()));
        // WSL is optional on Windows; ordinary saved destinations restore independently.
        if saved
            .host_id
            .as_deref()
            .is_some_and(|id| id.starts_with("wsl:"))
        {
            hosts.extend(
                platform::wsl_distributions()
                    .await
                    .map_err(|error| error.to_string())?
                    .into_iter()
                    .map(|distro| format!("wsl:{distro}")),
            );
        }
        let selection = resolve_connection_selection(hosts, saved.host_id, saved.all);
        let remote = match selection.host_id.as_deref() {
            Some(id) if id.starts_with("wsl:") => Some(
                runtime
                    .connections
                    .wsl_host(&id[4..])
                    .await
                    .map_err(|error| error.to_string())?,
            ),
            Some(id) => remotes.iter().find(|host| host.id == id).cloned(),
            None => None,
        };
        Ok((selection, remote))
    }

    pub(super) fn save_connection_selection(&mut self) {
        if self.side_chat_mode {
            return;
        }
        if let Err(error) = selection_path().and_then(|path| {
            agent_local_host::save_private_json(&path, &self.connection_selection)
                .map_err(|error| error.to_string())
        }) {
            self.set_error(error);
        }
    }

    pub(super) fn connection_id(&self) -> String {
        self.remote
            .as_ref()
            .map_or_else(|| "local".into(), |host| host.id.clone())
    }

    pub(super) fn connection_remote(&self, id: &str, cx: &App) -> Option<RemoteHost> {
        self.hosts
            .as_ref()
            .and_then(|hosts| {
                hosts
                    .read(cx)
                    .choices()
                    .into_iter()
                    .find(|host| host.id == id)
            })
            .or_else(|| self.remote.as_ref().filter(|host| host.id == id).cloned())
    }

    pub(super) fn connection_store(
        &self,
        host: &str,
        cx: &App,
    ) -> Option<Arc<agent_core::store::Store>> {
        if host.is_empty() || host == self.connection_id() {
            return self
                .session
                .as_ref()
                .map(|session| session.host.store.clone());
        }
        if host == "local" {
            return self.runtime.sessions.store(None);
        }
        let remote = self.connection_remote(host, cx)?;
        self.runtime.sessions.store(Some(&remote.ticket))
    }

    pub(super) fn dispatch_host(&self, host: &str, intent: Intent, cx: &App) {
        if host.is_empty() || host == self.connection_id() {
            self.dispatch(intent);
        } else if let Some(store) = self.connection_store(host, cx) {
            drop(store.dispatch(intent));
        }
    }

    pub(super) fn navigate_host(
        &mut self,
        host: &str,
        intent: Intent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if host.is_empty() || host == self.connection_id() {
            self.tab = Tab::Chat;
            self.dispatch(intent);
        } else if let Some(store) = self.connection_store(host, cx) {
            drop(store.dispatch(intent));
            let remote = self.connection_remote(host, cx);
            if host != "local" && remote.is_none() {
                return;
            }
            self.switch_host(remote, window, cx);
        }
        self.composer
            .input
            .read(cx)
            .focus_handle(cx)
            .focus(window, cx);
    }

    pub(super) fn select_connection(&mut self, host: Option<String>, cx: &mut Context<Self>) {
        self.connection_selection.all = host.is_none();
        if let Some(host) = host {
            let remote = self.connection_remote(&host, cx);
            if let Some(hosts) = self.hosts.clone() {
                hosts.update(cx, |hosts, cx| hosts.select(remote, None, cx));
            }
        }
        self.save_connection_selection();
        self.rebuild_connection_list(cx);
    }

    pub(super) fn rebuild_connection_list(&mut self, cx: &mut Context<Self>) {
        self.connected_hosts = self
            .hosts
            .as_ref()
            .map(|hosts| hosts.read(cx).connected_hosts())
            .unwrap_or_default();
        self.connection_names = vec![(
            "local".into(),
            self.runtime
                .sessions
                .store(None)
                .and_then(|store| store.snapshot().connection.host_name.clone())
                .unwrap_or_else(|| "このPC".into()),
        )];
        if let Some(hosts) = &self.hosts {
            self.connection_names.extend(
                hosts
                    .read(cx)
                    .choices()
                    .into_iter()
                    .map(|host| (host.id, host.name)),
            );
        }
        self.connection_list = self.hosts.as_ref().and_then(|hosts| {
            hosts
                .read(cx)
                .thread_list(if self.connection_selection.all {
                    None
                } else {
                    self.connection_selection.host_id.as_deref()
                })
                .map(Arc::new)
        });
        cx.notify();
    }

    pub(super) fn dispatch_list(&self, intent: Intent, cx: &App) {
        if self.connection_selection.all {
            for host in self
                .hosts
                .as_ref()
                .map(|hosts| hosts.read(cx).list_hosts())
                .unwrap_or_default()
            {
                self.dispatch_host(&host, intent.clone(), cx);
            }
        } else {
            self.dispatch(intent);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::store::Store;
    use agent_transport::transport::{Endpoint, Identity, Relays};
    use core::prelude::v1::test;
    use gpui::TestAppContext;

    fn snapshot() -> Snapshot {
        let mut snapshot = Snapshot::default();
        snapshot.lists.threads = Some(Arc::new(serde_json::from_value(serde_json::json!({
            "data":[{"id":{"provider":"codex","id":"same-task"},"name":"Same task","projectId":"same-project"}],
            "projects":[{"id":"same-project","name":"Same project","roots":[]}],
            "hasMore":false,"hasMoreProjects":false,"projectPages":{},"limit":5
        })).unwrap()));
        snapshot
    }

    #[gpui::test]
    fn all_connections_route_identical_ids_to_their_owner_and_keep_the_last_destination(
        cx: &mut TestAppContext,
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let app_runtime = Runtime {
            handle: runtime.handle().clone(),
            sessions: Default::default(),
            connections: Default::default(),
            closing: tokio_util::task::TaskTracker::new(),
            logging_error: None,
        };
        let endpoint = runtime
            .block_on(Endpoint::bind(Identity::generate(), Relays::Loopback))
            .unwrap();
        let remote = RemoteHost {
            id: endpoint.node_id().to_string(),
            name: "Same machine".into(),
            ticket: endpoint.ticket().to_string(),
        };
        let local = runtime.block_on(async { Arc::new(Store::offline(snapshot())) });
        let distant = runtime.block_on(async { Arc::new(Store::offline(snapshot())) });
        let local_session =
            app_runtime
                .sessions
                .install_fixture("local", local.clone(), app_runtime.clone());
        let _distant_session =
            app_runtime
                .sessions
                .install_fixture(&remote.id, distant.clone(), app_runtime.clone());
        cx.update(|cx| {
            crate::appearance::init(cx);
            cx.set_global(app_runtime);
        });
        let (view, window) = cx.add_window_view(|window, cx| {
            let mut view = Desktop::new(
                Mode::Fixture {
                    remote: None,
                    cwd: String::new(),
                },
                window,
                cx,
            );
            view.snapshot = local.snapshot();
            view.project_fixture();
            view.session = Some(local_session);
            let hosts = cx.new(|cx| Hosts::new(window, cx));
            hosts.update(cx, |hosts, _| hosts.fixture_remotes(vec![remote.clone()]));
            view.hosts = Some(hosts);
            view.connection_selection.all = true;
            view
        });
        view.update(window, |view, cx| {
            view.dispatch_host(
                &remote.id,
                Intent::SetProjectPinned {
                    project_id: "same-project".into(),
                    pinned: true,
                },
                cx,
            );
            assert!(distant.snapshot().thread_list().unwrap().projects[0].pinned);
            assert!(!local.snapshot().thread_list().unwrap().projects[0].pinned);
        });
        window.update(|window, cx| {
            view.update(cx, |view, cx| {
                let id = SessionRef {
                    provider: agent_protocol::session::ProviderKind::Codex,
                    id: "same-task".into(),
                };
                view.navigate_host(
                    &remote.id,
                    Intent::ReadThread(op::ReadThread::open(id.clone())),
                    window,
                    cx,
                );
                assert_eq!(
                    distant.snapshot().pane.navigation.thread_id.as_ref(),
                    Some(&id)
                );
                assert!(local.snapshot().pane.navigation.thread_id.is_none());
                assert!(view.connection_selection.all);
                assert_eq!(
                    view.connection_selection.host_id.as_deref(),
                    Some(remote.id.as_str())
                );
                assert_eq!(view.remote.as_ref().map(|host| &host.id), Some(&remote.id));
            })
        });
        runtime.block_on(endpoint.close());
    }
}
