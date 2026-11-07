use crate::Runtime;
use agent_core::client::persistence::Persistence;
use agent_core::store::{Pane, Store};
use gpui_kit::Context;
use std::sync::{Arc, Mutex};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// One Host owns its connection and save loop, independently of its panes.
pub(crate) struct HostSession {
    pub(crate) store: Arc<Store>,
    handle: tokio::runtime::Handle,
    closing: tokio_util::task::TaskTracker,
    persistence: Mutex<Option<Arc<Persistence>>>,
    stop: CancellationToken,
}
impl HostSession {
    pub(crate) fn persistence(&self) -> Option<Arc<Persistence>> {
        self.persistence.lock().unwrap().clone()
    }
    fn close(&self) -> Option<JoinHandle<()>> {
        let mut persistence = self.persistence.lock().unwrap();
        if self.stop.is_cancelled() {
            return None;
        }
        self.stop.cancel();
        let store = self.store.clone();
        let persistence = persistence.take();
        Some(self.closing.spawn_on(
            async move {
                let _ = store.close().await;
                if let Some(persistence) = persistence
                    && let Ok(saved) = persistence.finish(store.snapshot())
                {
                    let _ = saved.await;
                }
            },
            &self.handle,
        ))
    }
}
impl Drop for HostSession {
    fn drop(&mut self) {
        self.close();
    }
}

type HostEntry = Arc<tokio::sync::Mutex<Option<Arc<HostSession>>>>;

/// Open Hosts outlive navigation and are shut down once when the app exits.
#[derive(Default)]
pub(crate) struct Sessions {
    hosts: Mutex<std::collections::BTreeMap<String, HostEntry>>,
    local_node: std::sync::OnceLock<agent_transport::transport::NodeId>,
    management: tokio::sync::OnceCell<Arc<HostSession>>,
    stop: CancellationToken,
}
impl Sessions {
    #[cfg(test)]
    pub(crate) fn install_fixture(
        &self,
        id: &str,
        store: Arc<Store>,
        runtime: Runtime,
    ) -> StoreSession {
        let session = StoreSession::new(store, runtime);
        self.hosts.lock().unwrap().insert(
            id.into(),
            Arc::new(tokio::sync::Mutex::new(Some(session.host.clone()))),
        );
        session
    }

    pub(crate) async fn management(&self, runtime: Runtime) -> Result<StoreSession, String> {
        if self.stop.is_cancelled() {
            return Err("sessions are closed".into());
        }
        let owner = self
            .management
            .get_or_try_init(|| async {
                let store = runtime
                    .connections
                    .connect(None, agent_core::state::Snapshot::default())
                    .await
                    .map_err(|error| error.to_string())?;
                Ok::<_, String>(StoreSession::new(store, runtime).host.clone())
            })
            .await?;
        if self.stop.is_cancelled() {
            if let Some(close) = owner.close() {
                let _ = close.await;
            }
            return Err("sessions are closed".into());
        }
        Ok(StoreSession::primary(owner.clone()))
    }

    pub(crate) fn remove(
        &self,
        host: &str,
    ) -> impl Future<Output = Result<(), String>> + Send + use<> {
        let entry = self
            .hosts
            .lock()
            .unwrap()
            .entry(host.to_owned())
            .or_default()
            .clone();
        let host = host.to_owned();
        async move {
            // A new pairing of this Host must wait for the old final save and
            // cache cleanup before restoring the same files.
            let mut entry = entry.lock().await;
            if let Some(owner) = entry.take()
                && let Some(closed) = owner.close()
            {
                closed.await.map_err(|error| error.to_string())?;
            }
            let copies = crate::platform::conversation_copies(&host)?;
            tokio::task::spawn_blocking(move || agent_core::conversation_cache::clear(&copies))
                .await
                .map_err(|error| error.to_string())?;
            Ok(())
        }
    }

    pub(crate) fn store(&self, remote: Option<&str>) -> Option<Arc<Store>> {
        let host = self.host_key(remote).ok()?;
        let entry = self.hosts.lock().unwrap().get(&host)?.clone();
        Some(entry.try_lock().ok()?.as_ref()?.store.clone())
    }

    fn host_key(&self, remote: Option<&str>) -> Result<String, String> {
        let host = match remote {
            Some(ticket) => {
                let node = ticket
                    .parse::<agent_transport::transport::Ticket>()
                    .map_err(|error| error.to_string())?
                    .node_id();
                if self.local_node.get() == Some(&node) {
                    "local".into()
                } else {
                    node.to_string()
                }
            }
            None => "local".into(),
        };
        Ok(host)
    }

    pub(crate) async fn open(
        &self,
        remote: Option<&str>,
        runtime: Runtime,
        enabled: Option<bool>,
    ) -> Result<StoreSession, String> {
        if self.local_node.get().is_none()
            && let Ok(agent_local_host::LocalHost {
                state: agent_local_host::LocalHostState::Ready(local),
                ..
            }) = crate::platform::discover_local_host().await
        {
            let _ = self.local_node.set(local.node_id());
        }
        let host = self.host_key(remote)?;
        let entry = {
            let mut hosts = self.hosts.lock().unwrap();
            if self.stop.is_cancelled() {
                return Err("sessions are closed".into());
            }
            hosts.entry(host.clone()).or_default().clone()
        };
        let mut entry = entry.lock().await;
        if self.stop.is_cancelled() {
            return Err("sessions are closed".into());
        }
        if let Some(owner) = entry.as_ref() {
            if let Some(enabled) = enabled {
                owner
                    .store
                    .dispatch(agent_core::state::Intent::SetConnectionEnabled { enabled })
                    .await
                    .map_err(|error| error.to_string())?;
            }
            if !owner.store.snapshot().connection.connected
                && owner.store.snapshot().connection_enabled()
            {
                let _ = runtime.connections.resume(&owner.store, remote).await;
            }
            return Ok(StoreSession::primary(owner.clone()));
        }
        let path = crate::platform::state_dir()?.join(format!("desktop-main-{host}.json"));
        let persistence = Persistence::new(
            Arc::new(crate::platform::SessionStorage { path }),
            runtime.connections.preferences.clone(),
        );
        let mut snapshot = persistence
            .restore(false, Some(crate::platform::conversation_copies(&host)?))
            .await
            .map_err(|error| match error {
                agent_core::client::persistence::RestoreError::InvalidState(reason) => {
                    format!("保存した入力状態を読み込めません: {reason}")
                }
                agent_core::client::persistence::RestoreError::Storage(reason) => {
                    reason.to_string()
                }
            })?;
        if let Some(enabled) = enabled {
            snapshot = agent_core::state::reduce(
                &snapshot,
                agent_core::state::Event::Intent(agent_core::state::Intent::SetConnectionEnabled {
                    enabled,
                }),
            )
            .0;
        }
        let cwd = snapshot
            .pane
            .navigation
            .thread_id
            .is_none()
            .then(|| snapshot.pane.navigation.cwd.clone());
        let store = match runtime.connections.connect(remote, snapshot.clone()).await {
            Ok(store) => store,
            Err(error) => {
                snapshot.connection.connected = false;
                snapshot.connection.connection_failure = Some(format!("{error:#}"));
                Arc::new(Store::offline(
                    snapshot,
                    Some(runtime.connections.preferences.clone()),
                ))
            }
        };
        if let Some(cwd) = cwd {
            drop(store.dispatch(agent_core::state::Intent::NewChat { cwd }));
        }
        let session = StoreSession::new(store, runtime);
        session.start_persistence(persistence.clone());
        *entry = Some(session.host.clone());
        Ok(session)
    }

    pub(crate) async fn close(&self) {
        let hosts = {
            let mut hosts = self.hosts.lock().unwrap();
            self.stop.cancel();
            std::mem::take(&mut *hosts)
        };
        let mut closes = Vec::new();
        for entry in hosts.values() {
            if let Some(host) = entry.lock().await.take()
                && let Some(close) = host.close()
            {
                closes.push(close);
            }
        }
        if let Some(owner) = self.management.get()
            && let Some(close) = owner.close()
        {
            closes.push(close);
        }
        for close in closes {
            let _ = close.await;
        }
    }
}

/// A view owns only its pane and observation; another pane keeps the Host alive.
pub(crate) struct StoreSession {
    pub(crate) host: Arc<HostSession>,
    pub(crate) pane: Arc<Pane>,
    stop: CancellationToken,
}
impl StoreSession {
    pub(crate) fn new(store: Arc<Store>, runtime: Runtime) -> Self {
        let pane = store.primary_pane();
        Self {
            host: Arc::new(HostSession {
                store,
                handle: runtime.handle,
                closing: runtime.closing,
                persistence: Mutex::new(None),
                stop: CancellationToken::new(),
            }),
            pane,
            stop: CancellationToken::new(),
        }
    }

    pub(crate) fn primary(host: Arc<HostSession>) -> Self {
        Self {
            pane: host.store.primary_pane(),
            host,
            stop: CancellationToken::new(),
        }
    }

    pub(crate) fn side(&self, cwd: String) -> Result<Self, String> {
        let _entered = self.host.handle.enter();
        Ok(Self {
            pane: self
                .host
                .store
                .open_pane(cwd)
                .map_err(|error| error.to_string())?,
            host: self.host.clone(),
            stop: CancellationToken::new(),
        })
    }

    pub(crate) fn on_app_quit<V: 'static>(
        cx: &mut Context<V>,
        session: fn(&mut V) -> &mut Option<Self>,
    ) {
        cx.on_app_quit(move |view, _| {
            if let Some(mut session) = session(view).take()
                && let Some(close) = session.close()
            {
                let _ = session.host.handle.block_on(close);
            }
            async {}
        })
        .detach();
    }

    #[cfg(test)]
    pub(crate) async fn publish<E: Send>(
        result: anyhow::Result<Arc<Store>>,
        runtime: Runtime,
        updates: async_channel::Sender<E>,
        connected: impl FnOnce(Result<Self, String>) -> E,
        snapshot: impl Fn(Arc<agent_core::presentation::view::Presentation>) -> E,
    ) {
        match result {
            Ok(store) => {
                Self::observe(Self::new(store, runtime), updates, connected, snapshot).await
            }
            Err(error) => {
                let _ = updates.send(connected(Err(format!("{error:#}")))).await;
            }
        }
    }

    pub(crate) async fn observe<E: Send>(
        session: Self,
        updates: async_channel::Sender<E>,
        connected: impl FnOnce(Result<Self, String>) -> E,
        snapshot: impl Fn(Arc<agent_core::presentation::view::Presentation>) -> E,
    ) {
        let mut snapshots = session.pane.subscribe();
        let stop = session.stop.clone();
        if updates.send(connected(Ok(session))).await.is_err() {
            return;
        }
        loop {
            let current = snapshots.borrow_and_update().clone();
            if updates.send(snapshot(current)).await.is_err() {
                break;
            }
            tokio::select! {
                biased;
                _ = stop.cancelled() => break,
                result = snapshots.changed() => if result.is_err() { break; },
            }
        }
    }

    pub(crate) fn persist<E: Send + 'static>(
        &self,
        persistence: Arc<Persistence>,
        mut device_failures: tokio::sync::watch::Receiver<Option<String>>,
        updates: async_channel::Sender<E>,
        failure: impl Fn(String) -> E + Send + 'static,
    ) {
        self.start_persistence(persistence);
        let persistence = self
            .host
            .persistence
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .clone();
        let handle = &self.host.handle;
        let mut failures = persistence.failures();
        let stop = self.stop.clone();
        handle.spawn(async move {
            let initial = [
                failures.borrow_and_update().clone(),
                device_failures.borrow_and_update().clone(),
            ];
            for error in initial.into_iter().flatten() {
                if updates.send(failure(error)).await.is_err() {
                    return;
                }
            }
            loop {
                let error = tokio::select! {
                    biased;
                    _ = stop.cancelled() => break,
                    result = failures.changed() => {
                        if result.is_err() { break; }
                        failures.borrow_and_update().clone()
                    },
                    result = device_failures.changed() => {
                        if result.is_err() { break; }
                        device_failures.borrow_and_update().clone()
                    },
                };
                if let Some(error) = error
                    && updates.send(failure(error)).await.is_err()
                {
                    break;
                }
            }
        });
    }

    fn start_persistence(&self, persistence: Arc<Persistence>) {
        let mut owner = self.host.persistence.lock().unwrap();
        if owner.is_some() {
            return;
        }
        if let Some(task) = persistence.start(self.host.store.subscribe()) {
            self.host.handle.spawn(task);
        }
        self.host.handle.spawn(
            persistence
                .clone()
                .observe(self.host.store.subscribe(), self.host.stop.clone()),
        );
        *owner = Some(persistence);
    }

    pub(crate) fn close(&mut self) -> Option<JoinHandle<()>> {
        if self.stop.is_cancelled() {
            return None;
        }
        self.stop.cancel();
        self.pane.close();
        Arc::get_mut(&mut self.host).and_then(|host| host.close())
    }
}
impl Drop for StoreSession {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::state::{Intent, Snapshot};
    use std::{
        path::{Path, PathBuf},
        time::Duration,
    };

    enum Update {
        Connected(Result<StoreSession, String>),
        Snapshot,
        Error,
    }
    fn runtime(directory: Option<&Path>) -> Runtime {
        Runtime {
            handle: tokio::runtime::Handle::current(),
            connections: directory.map_or_else(Arc::default, |directory| {
                Arc::new(crate::platform::Connections::new(
                    agent_core::client::persistence::DevicePreferences::new(Arc::new(
                        crate::platform::DeviceStorage {
                            directory: Ok(directory.to_owned()),
                        },
                    )),
                ))
            }),
            sessions: Arc::default(),
            closing: tokio_util::task::TaskTracker::new(),
            logging_error: None,
        }
    }

    fn persistence(
        path: PathBuf,
        preferences: Arc<agent_core::client::persistence::DevicePreferences>,
    ) -> Arc<Persistence> {
        Persistence::new(
            Arc::new(crate::platform::SessionStorage { path }),
            preferences,
        )
    }

    #[test]
    fn persistence_starts_outside_the_runtime_and_flushes_on_close() {
        let executor = tokio::runtime::Runtime::new().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("snapshot.json");
        let runtime = executor.block_on(async { runtime(Some(directory.path())) });
        let store = executor.block_on(async {
            Arc::new(Store::offline(
                Snapshot::default(),
                Some(runtime.connections.preferences.clone()),
            ))
        });
        let (updates, incoming) = async_channel::unbounded();
        let publish = runtime.handle.spawn(StoreSession::publish(
            Ok(store.clone()),
            runtime.clone(),
            updates.clone(),
            Update::Connected,
            |_| Update::Snapshot,
        ));
        let Update::Connected(Ok(session)) = executor.block_on(incoming.recv()).unwrap() else {
            panic!("missing session")
        };
        // GPUI delivers Connected on its main thread, without a Tokio context.
        assert!(tokio::runtime::Handle::try_current().is_err());
        session.persist(
            persistence(path.clone(), runtime.connections.preferences.clone()),
            runtime.connections.preferences.failures(),
            updates,
            |_| Update::Error,
        );
        executor.block_on(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                store
                    .dispatch(Intent::SetDraftText {
                        thread_id: "draft".into(),
                        text: "saved from the UI thread".into(),
                    })
                    .await
                    .unwrap();
                drop(session);
                runtime.closing.close();
                runtime.closing.wait().await;
                publish.await.unwrap();
            })
            .await
            .expect("persistence outside the runtime stalled");
        });
        let restored = agent_core::persistence::decode(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(
            restored.input.drafts[&"draft".into()].text,
            "saved from the UI thread"
        );
    }

    #[tokio::test]
    async fn connection_switches_address_the_owner_without_changing_its_panes() {
        use agent_transport::transport::{Endpoint, Identity, Relays};
        let runtime = runtime(None);
        let endpoint = Endpoint::bind(Identity::generate(), Relays::Loopback)
            .await
            .unwrap();
        let local_endpoint = Endpoint::bind(Identity::generate(), Relays::Loopback)
            .await
            .unwrap();
        runtime
            .sessions
            .local_node
            .set(local_endpoint.node_id())
            .unwrap();
        let ticket = endpoint.ticket().to_string();
        let local = StoreSession::new(
            Arc::new(Store::offline(Snapshot::default(), None)),
            runtime.clone(),
        );
        let remote = StoreSession::new(
            Arc::new(Store::offline(Snapshot::default(), None)),
            runtime.clone(),
        );
        for (key, session) in [
            ("local".to_owned(), &local),
            (endpoint.node_id().to_string(), &remote),
        ] {
            runtime.sessions.hosts.lock().unwrap().insert(
                key,
                Arc::new(tokio::sync::Mutex::new(Some(session.host.clone()))),
            );
        }
        local
            .pane
            .dispatch(Intent::NewChat {
                cwd: "/local".into(),
            })
            .await
            .unwrap();
        remote
            .pane
            .dispatch(Intent::NewChat {
                cwd: "/remote".into(),
            })
            .await
            .unwrap();
        let store = runtime.sessions.store(Some(&ticket)).unwrap();
        assert!(Arc::ptr_eq(
            &runtime
                .sessions
                .store(Some(&local_endpoint.ticket().to_string()))
                .unwrap(),
            &local.host.store
        ));
        store
            .dispatch(Intent::SetConnectionEnabled { enabled: false })
            .await
            .unwrap();
        assert!(
            !runtime
                .sessions
                .store(Some(&ticket))
                .unwrap()
                .snapshot()
                .connection_enabled()
        );
        assert!(
            runtime
                .sessions
                .store(None)
                .unwrap()
                .snapshot()
                .connection_enabled()
        );
        assert_eq!(local.pane.snapshot().unwrap().pane.navigation.cwd, "/local");
        assert_eq!(
            remote.pane.snapshot().unwrap().pane.navigation.cwd,
            "/remote"
        );
        assert!(runtime.sessions.store(Some("invalid ticket")).is_none());
        runtime
            .sessions
            .remove(&endpoint.node_id().to_string())
            .await
            .unwrap();
        assert!(store.dispatch(Intent::ShowThreadList).await.is_err());
        assert!(runtime.sessions.store(Some(&ticket)).is_none());
        assert!(local.pane.snapshot().is_ok());
        runtime.sessions.close().await;
        assert!(runtime.sessions.store(Some(&ticket)).is_none());
        endpoint.close().await;
        local_endpoint.close().await;
    }

    #[tokio::test]
    async fn application_shutdown_closes_registered_hosts_and_prevents_reopening() {
        let runtime = runtime(None);
        let store = Arc::new(Store::offline(Snapshot::default(), None));
        let primary = StoreSession::new(store.clone(), runtime.clone());
        let side = primary.side("/side".into()).unwrap();
        runtime.sessions.hosts.lock().unwrap().insert(
            "local".into(),
            Arc::new(tokio::sync::Mutex::new(Some(primary.host.clone()))),
        );
        runtime.sessions.close().await;
        assert!(primary.pane.snapshot().is_err());
        assert!(side.pane.snapshot().is_err());
        assert!(runtime.sessions.management(runtime.clone()).await.is_err());
        assert!(
            runtime
                .sessions
                .open(None, runtime.clone(), None)
                .await
                .is_err()
        );
        assert!(store.dispatch(Intent::ShowThreadList).await.is_err());
    }

    #[tokio::test]
    async fn side_panes_share_one_host_and_survive_primary_view_replacement() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("snapshot.json");
        let runtime = runtime(Some(directory.path()));
        let store = Arc::new(Store::offline(
            Snapshot::default(),
            Some(runtime.connections.preferences.clone()),
        ));
        let mut primary = StoreSession::new(store.clone(), runtime.clone());
        primary.start_persistence(persistence(
            path.clone(),
            runtime.connections.preferences.clone(),
        ));
        let mut side = primary.side("/side".into()).unwrap();
        assert!(Arc::ptr_eq(&primary.host, &side.host));
        let key = side
            .pane
            .snapshot()
            .unwrap()
            .pane
            .navigation
            .draft_key
            .clone();
        side.pane
            .dispatch(Intent::SetDraftText {
                thread_id: key.clone(),
                text: "independent side input".into(),
            })
            .await
            .unwrap();
        assert_ne!(store.snapshot().pane.navigation.draft_key, key);
        assert!(primary.close().is_none());
        side.pane
            .dispatch(Intent::NewChat {
                cwd: "/other".into(),
            })
            .await
            .unwrap();
        let mut replacement = StoreSession::primary(side.host.clone());
        replacement
            .pane
            .dispatch(Intent::NewChat {
                cwd: "/primary".into(),
            })
            .await
            .unwrap();
        assert_eq!(side.pane.snapshot().unwrap().pane.navigation.cwd, "/other");
        assert!(side.close().is_none());
        drop(side);
        drop(primary);
        let closed = replacement
            .close()
            .expect("last view closes its unregistered Host");
        closed.await.unwrap();
        assert!(store.dispatch(Intent::ShowThreadList).await.is_err());
        let restored = agent_core::persistence::decode(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(restored.input.drafts[&key].text, "independent side input");
    }

    #[tokio::test]
    async fn closing_flushes_the_latest_draft_and_closes_the_store() {
        tokio::time::timeout(Duration::from_secs(5), async {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("snapshot.json");
            let runtime = runtime(Some(directory.path()));
            let store = Arc::new(Store::offline(
                Snapshot::default(),
                Some(runtime.connections.preferences.clone()),
            ));
            let (updates, incoming) = async_channel::unbounded();
            let publish = tokio::spawn(StoreSession::publish(
                Ok(store.clone()),
                runtime.clone(),
                updates.clone(),
                Update::Connected,
                |_| Update::Snapshot,
            ));
            let Update::Connected(Ok(session)) = incoming.recv().await.unwrap() else {
                panic!("missing session")
            };
            session.persist(
                persistence(path.clone(), runtime.connections.preferences.clone()),
                runtime.connections.preferences.failures(),
                updates,
                |_| Update::Error,
            );
            store
                .dispatch(Intent::SelectModel {
                    thread_id: "draft".into(),
                    model: agent_protocol::models::ModelRef {
                        provider: agent_protocol::session::ProviderKind::Codex,
                        id: "gpt-6.1-sol".into(),
                    },
                })
                .await
                .unwrap();
            store
                .dispatch(Intent::SetDraftText {
                    thread_id: "draft".into(),
                    text: "last edit before close".into(),
                })
                .await
                .unwrap();
            // No UI snapshot/save notification is needed for the final flush.
            drop(session);
            runtime.closing.close();
            runtime.closing.wait().await;
            publish.await.unwrap();
            let restored: Snapshot =
                agent_core::persistence::decode(&std::fs::read(path).unwrap()).unwrap();
            assert_eq!(
                restored.input.drafts[&agent_core::state::DraftKey::from("draft")].text,
                "last edit before close"
            );
            assert!(store.dispatch(Intent::ShowThreadList).await.is_err());
            let preferences =
                std::fs::read(directory.path().join("model-preferences.json")).unwrap();
            assert_eq!(
                serde_json::from_slice::<agent_core::state::ModelPreferences>(&preferences)
                    .unwrap(),
                store.snapshot().catalog.model_preferences
            );
        })
        .await
        .expect("session close stalled");
    }

    #[tokio::test]
    async fn persistence_failure_is_reported_and_a_later_save_recovers() {
        tokio::time::timeout(Duration::from_secs(5), async {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("snapshot.json");
            std::fs::create_dir(&path).unwrap();
            let runtime = runtime(Some(directory.path()));
            let store = Arc::new(Store::offline(
                Snapshot::default(),
                Some(runtime.connections.preferences.clone()),
            ));
            let (updates, incoming) = async_channel::unbounded();
            let publish = tokio::spawn(StoreSession::publish(
                Ok(store.clone()),
                runtime.clone(),
                updates.clone(),
                Update::Connected,
                |_| Update::Snapshot,
            ));
            let Update::Connected(Ok(session)) = incoming.recv().await.unwrap() else {
                panic!("missing session")
            };
            session.persist(
                persistence(path.clone(), runtime.connections.preferences.clone()),
                runtime.connections.preferences.failures(),
                updates,
                |_| Update::Error,
            );
            while !matches!(incoming.recv().await.unwrap(), Update::Error) {}
            std::fs::remove_dir(&path).unwrap();
            store
                .dispatch(Intent::SetDraftText {
                    thread_id: "draft".into(),
                    text: "recovered".into(),
                })
                .await
                .unwrap();
            drop(session);
            runtime.closing.close();
            runtime.closing.wait().await;
            publish.await.unwrap();
            let restored: Snapshot =
                agent_core::persistence::decode(&std::fs::read(path).unwrap()).unwrap();
            assert_eq!(
                restored.input.drafts[&agent_core::state::DraftKey::from("draft")].text,
                "recovered"
            );
        })
        .await
        .expect("persistence recovery stalled");
    }

    #[tokio::test]
    async fn failed_connection_is_delivered_and_undelivered_session_closes_its_store() {
        tokio::time::timeout(Duration::from_secs(5), async {
            let runtime = runtime(None);
            let store = Arc::new(Store::offline(Snapshot::default(), Some(runtime.connections.preferences.clone())));
            let (updates, incoming) = async_channel::unbounded();
            StoreSession::publish(
                Err(anyhow::anyhow!("connection failed").context("cannot open session")), runtime.clone(), updates.clone(),
                Update::Connected, |_| Update::Snapshot,
            ).await;
            assert!(matches!(incoming.recv().await.unwrap(), Update::Connected(Err(error)) if error == "cannot open session: connection failed"));
            assert!(incoming.try_recv().is_err());
            drop(incoming);
            StoreSession::publish(
                Ok(store.clone()),
                runtime.clone(),
                updates,
                Update::Connected,
                |_| Update::Snapshot,
            )
            .await;
            runtime.closing.close();
            runtime.closing.wait().await;
            assert!(store.dispatch(Intent::ShowThreadList).await.is_err());
        })
        .await
        .expect("undelivered session remained open");
    }

    #[tokio::test]
    async fn preference_failures_reach_the_window_without_rewriting_host_state() {
        use agent_core::client::{
            AgentError,
            persistence::{ClientStorage, DevicePreferences, PreferenceKey, PreferenceStorage},
        };
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        #[derive(Default)]
        struct Device {
            fail: AtomicBool,
        }
        impl PreferenceStorage for Device {
            fn load(&self, _: PreferenceKey) -> Result<Vec<u8>, AgentError> {
                Ok(Vec::new())
            }
            fn save(&self, _: PreferenceKey, _: Vec<u8>) -> Result<(), AgentError> {
                if self.fail.load(Ordering::SeqCst) {
                    Err(AgentError::Failed {
                        reason: "preferences unavailable".into(),
                    })
                } else {
                    Ok(())
                }
            }
        }
        #[derive(Default)]
        struct Local {
            writes: AtomicUsize,
            saved: tokio::sync::Notify,
        }
        impl ClientStorage for Local {
            fn load(&self) -> Result<Vec<u8>, AgentError> {
                Ok(Vec::new())
            }
            fn save(&self, _: Vec<u8>) -> Result<(), AgentError> {
                self.writes.fetch_add(1, Ordering::SeqCst);
                self.saved.notify_one();
                Ok(())
            }
        }
        tokio::time::timeout(Duration::from_secs(5), async {
            let disk = Arc::new(Device::default());
            let preferences = DevicePreferences::new(disk.clone());
            let local = Arc::new(Local::default());
            let persistence = Persistence::new(local.clone(), preferences.clone());
            let mut runtime = runtime(None);
            runtime.connections = Arc::new(crate::platform::Connections::new(preferences.clone()));
            let store = Arc::new(Store::offline(
                Snapshot::default(),
                Some(preferences.clone()),
            ));
            let session = StoreSession::new(store.clone(), runtime.clone());
            let (updates, incoming) = async_channel::unbounded();
            session.persist(persistence.clone(), preferences.failures(), updates, |_| {
                Update::Error
            });
            local.saved.notified().await;
            let mut failures = preferences.failures();
            disk.fail.store(true, Ordering::SeqCst);
            store
                .dispatch(Intent::SetPreferredEditor {
                    editor: agent_protocol::editors::EditorId::Zed,
                })
                .await
                .unwrap();
            assert!(matches!(incoming.recv().await.unwrap(), Update::Error));
            assert!(
                persistence
                    .failure()
                    .unwrap()
                    .contains("preferences unavailable")
            );
            assert_eq!(local.writes.load(Ordering::SeqCst), 1);
            assert_eq!(
                store.snapshot().files.preferred_editor,
                Some(agent_protocol::editors::EditorId::Zed)
            );
            failures.borrow_and_update();
            disk.fail.store(false, Ordering::SeqCst);
            store
                .dispatch(Intent::SetPreferredEditor {
                    editor: agent_protocol::editors::EditorId::Zed,
                })
                .await
                .unwrap();
            failures.wait_for(Option::is_none).await.unwrap();
            assert!(persistence.failure().is_none());
            assert_eq!(local.writes.load(Ordering::SeqCst), 1);
            drop(session);
            runtime.closing.close();
            runtime.closing.wait().await;
        })
        .await
        .expect("preference error notification stalled");
    }
}
