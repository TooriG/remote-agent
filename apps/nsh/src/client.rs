//! UI intents and publications cross this boundary; ClientSessions owns Host state.
use crate::storage::{self, Host, Storage, error};
use agent_core::{
    client::{
        AgentError,
        session::{ClientBuild, ClientSession, ClientSessions},
    },
    presentation::view::Presentation,
    state::Intent,
};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

pub(crate) enum Update {
    Hosts(Vec<Host>),
    Presentation {
        host: String,
        value: Arc<Presentation>,
    },
    Error(String),
    Paired(Result<String, String>),
}

pub(crate) struct LocalFile {
    _directory: tempfile::TempDir,
    pub path: PathBuf,
}

impl LocalFile {
    fn create(root: PathBuf, filename: &str) -> Result<Self, AgentError> {
        agent_local_host::create_state_directory(&root).map_err(error)?;
        let directory = tempfile::tempdir_in(root).map_err(error)?;
        Ok(Self {
            path: directory.path().join(filename),
            _directory: directory,
        })
    }
}

pub(crate) struct Client {
    runtime: tokio::runtime::Handle,
    jobs: tokio_util::task::TaskTracker,
    closing: AtomicBool,
    sessions: Arc<ClientSessions>,
    root: PathBuf,
    hosts: tokio::sync::Mutex<Vec<Host>>,
    updates: async_channel::Sender<Update>,
}

impl Client {
    pub fn new(
        root: PathBuf,
    ) -> Result<
        (
            tokio::runtime::Runtime,
            Arc<Self>,
            async_channel::Receiver<Update>,
        ),
        AgentError,
    > {
        let runtime = tokio::runtime::Runtime::new().map_err(error)?;
        let (updates, incoming) = async_channel::unbounded();
        let client = Arc::new(Self {
            runtime: runtime.handle().clone(),
            jobs: Default::default(),
            closing: AtomicBool::new(false),
            sessions: ClientSessions::new(
                true,
                ClientBuild {
                    number: 0,
                    revision: option_env!("AGENT_BUILD_REVISION")
                        .unwrap_or("development")
                        .into(),
                },
                "Bex NSH".into(),
            ),
            root,
            hosts: Default::default(),
            updates,
        });
        let owner = client.clone();
        client.spawn(async move {
            if let Err(reason) = owner.restore().await {
                owner.report(reason);
            }
        });
        Ok((runtime, client, incoming))
    }

    fn spawn(&self, work: impl Future<Output = ()> + Send + 'static) {
        self.jobs.spawn_on(work, &self.runtime);
    }

    fn report(&self, reason: impl std::fmt::Display) {
        let _ = self.updates.try_send(Update::Error(reason.to_string()));
    }

    async fn restore(self: &Arc<Self>) -> Result<(), AgentError> {
        let path = self.root.join("hosts.json");
        let (bytes, local) = tokio::task::spawn_blocking(move || {
            Ok::<_, AgentError>((storage::read(&path)?, storage::local_host()?))
        })
        .await
        .map_err(error)??;
        let mut hosts: Vec<Host> = if bytes.is_empty() {
            Vec::new()
        } else {
            serde_json::from_slice(&bytes).map_err(error)?
        };
        hosts.retain(|host| host.id != "local");
        if let Some(local) = local {
            hosts.insert(0, local);
        }
        let mut registry = self.hosts.lock().await;
        if self.closing.load(Ordering::SeqCst) {
            return Ok(());
        }
        *registry = hosts.clone();
        self.updates
            .send(Update::Hosts(hosts.clone()))
            .await
            .map_err(error)?;
        for host in hosts {
            self.open(host);
        }
        Ok(())
    }

    fn open(self: &Arc<Self>, host: Host) {
        let storage = Arc::new(Storage {
            root: self.root.clone(),
            host: host.clone(),
        });
        let session = self
            .sessions
            .open(host.id.clone(), storage.clone(), storage);
        let owner = self.clone();
        self.spawn(async move {
            match session.clone().ready().await {
                Ok(_) => {
                    owner.observe(host.id.clone(), session.clone());
                    owner.resume(session, host.ticket).await;
                }
                Err(reason) => owner.report(reason),
            }
        });
    }

    async fn resume(&self, session: Arc<ClientSession>, ticket: String) {
        if let Err(reason) = session.clone().resume(ticket).await {
            let message = reason.to_string();
            if !session.store().is_some_and(|store| {
                store.snapshot().connection.connection_failure.as_deref() == Some(&message)
            }) {
                // Identity and ticket preparation can fail before Core records an attempt.
                self.report(message);
            }
        }
    }

    fn observe(self: &Arc<Self>, host: String, session: Arc<ClientSession>) {
        let owner = self.clone();
        self.spawn(async move {
            let Some(store) = session.store() else {
                return;
            };
            let mut value = store.presentation();
            loop {
                if owner
                    .updates
                    .send(Update::Presentation {
                        host: host.clone(),
                        value: value.clone(),
                    })
                    .await
                    .is_err()
                {
                    break;
                }
                match store.next_presentation(value).await {
                    Ok(next) => value = next,
                    Err(_) => break,
                }
            }
        });
    }

    pub fn dispatch(&self, host: &str, intent: Intent) {
        // Core enqueues effects synchronously and publishes their errors.
        // A receipt is needed only when its result or resource lifetime is needed.
        if let Err(reason) = self.execute(host, intent) {
            self.report(reason);
        }
    }

    fn execute(
        &self,
        host: &str,
        intent: Intent,
    ) -> Result<Arc<agent_core::client::native::Receipt>, AgentError> {
        self.sessions
            .session(host.into())
            .ok_or_else(|| error("接続先が選択されていません"))?
            .dispatch(intent)
    }

    pub fn request(
        &self,
        host: &str,
        intent: Intent,
    ) -> async_channel::Receiver<Result<agent_core::store::Outcome, AgentError>> {
        let receipt = self.execute(host, intent);
        let (send, receive) = async_channel::bounded(1);
        self.spawn(async move {
            let result = match receipt {
                Ok(receipt) => receipt.wait().await,
                Err(reason) => Err(reason),
            };
            let _ = send.send(result).await;
        });
        receive
    }

    pub fn download(
        self: &Arc<Self>,
        host: String,
        source: String,
    ) -> async_channel::Receiver<Result<Arc<LocalFile>, AgentError>> {
        let (send, receive) = async_channel::bounded(1);
        let owner = self.clone();
        self.spawn(async move {
            let result = owner.download_file(&host, source).await;
            // If the view went away, dropping the result releases only this transfer's directory.
            let _ = send.send(result).await;
        });
        receive
    }

    async fn download_file(
        &self,
        host: &str,
        source: String,
    ) -> Result<Arc<LocalFile>, AgentError> {
        let root = self.root.join("downloads");
        let filename = source
            .rsplit(['/', '\\'])
            .next()
            .filter(|name| !["", ".", ".."].contains(name))
            .unwrap_or("file")
            .to_owned();
        let file = tokio::task::spawn_blocking(move || LocalFile::create(root, &filename))
            .await
            .map_err(error)??;
        self.execute(
            host,
            Intent::DownloadFile(agent_core::state::operations::DownloadFile {
                source,
                destination: file.path.to_string_lossy().into_owned(),
            }),
        )?
        .wait()
        .await?;
        Ok(Arc::new(file))
    }

    pub fn cache_image(
        self: &Arc<Self>,
        bytes: Vec<u8>,
        extension: String,
    ) -> async_channel::Receiver<Result<Arc<LocalFile>, AgentError>> {
        let (send, receive) = async_channel::bounded(1);
        let root = self.root.join("downloads");
        self.spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                let file = LocalFile::create(root, &format!("image.{extension}"))?;
                agent_local_host::save_private_bytes(&file.path, &bytes).map_err(error)?;
                Ok::<_, AgentError>(Arc::new(file))
            })
            .await
            .map_err(error)
            .and_then(|result| result);
            let _ = send.send(result).await;
        });
        receive
    }

    pub fn reconnect(self: &Arc<Self>, host: String) {
        let owner = self.clone();
        self.spawn(async move {
            let profile = owner
                .hosts
                .lock()
                .await
                .iter()
                .find(|profile| profile.id == host)
                .cloned();
            if let Some(profile) = profile
                && let Some(session) = owner.sessions.session(host)
            {
                owner.resume(session, profile.ticket).await;
            }
        });
    }

    pub fn pair(self: &Arc<Self>, invitation: agent_protocol::models::Invitation) {
        let owner = self.clone();
        self.spawn(async move {
            let result = owner
                .pair_host(invitation)
                .await
                .map_err(|reason| reason.to_string());
            let _ = owner.updates.send(Update::Paired(result)).await;
        });
    }

    async fn pair_host(
        self: &Arc<Self>,
        invitation: agent_protocol::models::Invitation,
    ) -> Result<String, AgentError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(error)?
            .as_secs();
        agent_core::client::native::validate_invitation(invitation.clone(), now)?;
        let id = agent_core::client::native::ticket_identity(invitation.endpoint.clone())?;
        let host = Host {
            id: id.clone(),
            name: invitation.host_name.clone(),
            ticket: invitation.endpoint.clone(),
            local_directory: None,
        };
        let storage = Arc::new(Storage {
            root: self.root.clone(),
            host: host.clone(),
        });
        let session = self
            .sessions
            .pair(storage.clone(), storage, invitation)
            .await?;
        let mut hosts = self.hosts.lock().await;
        if self.closing.load(Ordering::SeqCst) {
            session.shutdown().await?;
            return Err(error("client is closing"));
        }
        let mut next = hosts.clone();
        next.retain(|previous| previous.id != id);
        next.push(host);
        let path = self.root.join("hosts.json");
        let saved = next
            .iter()
            .filter(|host| host.id != "local")
            .cloned()
            .collect::<Vec<_>>();
        if let Err(reason) = tokio::task::spawn_blocking(move || {
            agent_local_host::save_private_json(&path, &saved).map_err(error)
        })
        .await
        .map_err(error)?
        {
            let _ = session.shutdown().await;
            return Err(reason);
        }
        *hosts = next.clone();
        self.sessions.adopt(id.clone(), session.clone());
        self.observe(id.clone(), session);
        self.updates
            .send(Update::Hosts(next))
            .await
            .map_err(error)?;
        Ok(id)
    }

    pub fn scene(&self, active: bool) {
        self.sessions.record_scene(if active { 1 } else { 0 });
        if !active {
            let sessions = self.sessions.clone();
            let updates = self.updates.clone();
            self.spawn(async move {
                if let Err(reason) = sessions.flush().await {
                    let _ = updates.send(Update::Error(reason.to_string())).await;
                }
            });
        }
    }

    pub fn attach(
        &self,
        host: &str,
        draft_key: agent_core::state::DraftKey,
        path: PathBuf,
        is_image: bool,
    ) {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        self.dispatch(
            host,
            Intent::UploadAttachment(agent_core::state::operations::UploadAttachment {
                draft_key,
                attachment: agent_core::state::Attachment {
                    path: path.to_string_lossy().into_owned(),
                    name,
                    is_image,
                },
            }),
        );
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn prepare_dictation(
        &self,
        host: &str,
    ) -> Result<Arc<agent_core::client::DictationPreparation>, AgentError> {
        self.sessions
            .session(host.into())
            .and_then(|session| session.store())
            .map(|store| store.prepare_dictation())
            .ok_or_else(|| error("Host 未接続"))
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn dictate(
        self: &Arc<Self>,
        host: String,
        draft_key: agent_core::state::DraftKey,
        preparation: Arc<agent_core::client::DictationPreparation>,
        audio: Vec<u8>,
        send: bool,
    ) {
        let Some(session) = self.sessions.session(host) else {
            self.report("Host 未接続");
            return;
        };
        let intent = Intent::Transcribe(agent_core::state::operations::Dictate {
            draft_key,
            preparation: Some(preparation.id()),
            audio,
            send,
            client_user_message_id: uuid::Uuid::new_v4().to_string().into(),
        });
        match session.dispatch(intent) {
            Ok(receipt) => {
                self.spawn(async move {
                    let _ = receipt.wait().await;
                    drop(preparation);
                });
            }
            Err(reason) => self.report(reason),
        }
    }

    pub fn upload(
        self: &Arc<Self>,
        host: String,
        draft_key: agent_core::state::DraftKey,
        name: String,
        bytes: Vec<u8>,
    ) {
        let owner = self.clone();
        self.spawn(async move {
            let root = owner.root.join("uploads");
            let result = tokio::task::spawn_blocking(move || {
                let file = LocalFile::create(root, "image")?;
                agent_local_host::save_private_bytes(&file.path, &bytes).map_err(error)?;
                Ok::<_, AgentError>(file)
            })
            .await
            .map_err(error)
            .and_then(|result| result);
            match result {
                Ok(file) => {
                    match owner.execute(
                        &host,
                        Intent::UploadAttachment(agent_core::state::operations::UploadAttachment {
                            draft_key,
                            attachment: agent_core::state::Attachment {
                                path: file.path.to_string_lossy().into_owned(),
                                name,
                                is_image: true,
                            },
                        }),
                    ) {
                        Ok(receipt) => {
                            let _ = receipt.wait().await;
                        }
                        Err(reason) => owner.report(reason),
                    }
                    drop(file);
                }
                Err(reason) => owner.report(reason),
            }
        });
    }

    pub async fn shutdown(&self) -> Result<(), AgentError> {
        self.closing.store(true, Ordering::SeqCst);
        let hosts = self.hosts.lock().await;
        self.sessions.retire();
        drop(hosts);
        self.jobs.close();
        self.jobs.wait().await;
        self.sessions.shutdown().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{
        client::persistence::{ClientStorage, StorageKey},
        state::Snapshot,
    };
    use std::time::Duration;

    fn saved_host(root: &std::path::Path) -> Host {
        let host = Host {
            id: "test-host".into(),
            name: "Saved PC".into(),
            ticket: "offline".into(),
            local_directory: None,
        };
        let storage = Storage {
            root: root.into(),
            host: host.clone(),
        };
        let mut snapshot = Snapshot::default();
        snapshot.connection.connection_disabled = true;
        storage
            .save(
                StorageKey::LocalState,
                agent_core::persistence::encode(&snapshot).unwrap(),
            )
            .unwrap();
        agent_local_host::save_private_json(&root.join("hosts.json"), &vec![host.clone()]).unwrap();
        host
    }

    async fn publication(
        incoming: &async_channel::Receiver<Update>,
        text: &str,
    ) -> Arc<Presentation> {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Update::Presentation { value, .. } = incoming.recv().await.unwrap()
                    && value.composer.draft.text == text
                {
                    return value;
                }
            }
        })
        .await
        .expect("client publication stalled")
    }

    #[test]
    fn shutdown_flushes_accepted_input_and_a_new_client_restores_it() {
        let directory = tempfile::tempdir().unwrap();
        let host = saved_host(directory.path());
        let (runtime, client, incoming) = Client::new(directory.path().into()).unwrap();
        let initial = runtime.block_on(publication(&incoming, ""));
        let key = initial.snapshot.pane.navigation.draft_key.clone();
        client.dispatch(
            &host.id,
            Intent::SetDraftText {
                thread_id: key.clone(),
                text: "終了直前の日本語 👋".into(),
            },
        );
        runtime.block_on(async {
            publication(&incoming, "終了直前の日本語 👋").await;
            tokio::time::timeout(Duration::from_secs(10), client.shutdown())
                .await
                .unwrap()
                .unwrap();
        });
        drop(client);
        drop(runtime);
        let (runtime, client, incoming) = Client::new(directory.path().into()).unwrap();
        let restored = runtime.block_on(publication(&incoming, "終了直前の日本語 👋"));
        assert_eq!(restored.snapshot.pane.navigation.draft_key, key);
        runtime.block_on(client.shutdown()).unwrap();
    }

    #[test]
    fn failed_pairing_preserves_existing_connections_and_drafts() {
        let directory = tempfile::tempdir().unwrap();
        let host = saved_host(directory.path());
        let (runtime, client, incoming) = Client::new(directory.path().into()).unwrap();
        let initial = runtime.block_on(publication(&incoming, ""));
        client.dispatch(
            &host.id,
            Intent::SetDraftText {
                thread_id: initial.snapshot.pane.navigation.draft_key.clone(),
                text: "保持する入力".into(),
            },
        );
        runtime.block_on(publication(&incoming, "保持する入力"));
        client.pair(agent_protocol::models::Invitation {
            endpoint: "invalid".into(),
            invitation: uuid::Uuid::new_v4(),
            expires_at: 0,
            host_name: "Expired PC".into(),
            ai_recipients: Vec::new(),
            transcription_recipient: None,
        });
        runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    if let Update::Paired(Err(reason)) = incoming.recv().await.unwrap() {
                        assert!(!reason.is_empty());
                        break;
                    }
                }
            })
            .await
            .unwrap();
            assert_eq!(client.hosts.lock().await.len(), 1);
            assert_eq!(
                client
                    .sessions
                    .session(host.id)
                    .unwrap()
                    .presentation()
                    .composer
                    .draft
                    .text,
                "保持する入力"
            );
            client.shutdown().await.unwrap();
        });
    }

    #[test]
    fn closing_during_restore_cannot_open_a_late_session() {
        let directory = tempfile::tempdir().unwrap();
        saved_host(directory.path());
        let (runtime, client, _) = Client::new(directory.path().into()).unwrap();
        runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(10), client.shutdown())
                .await
                .unwrap()
                .unwrap();
            if let Some(session) = client.sessions.session("test-host".into()) {
                assert!(session.clone().ready().await.is_err());
            }
        });
    }

    #[test]
    fn a_failed_operation_has_one_dismissible_core_notice() {
        use agent_core::{presentation::error::ErrorScope, state::operations as op};
        let directory = tempfile::tempdir().unwrap();
        let host = saved_host(directory.path());
        let (runtime, client, incoming) = Client::new(directory.path().into()).unwrap();
        runtime.block_on(publication(&incoming, ""));
        client.dispatch(
            &host.id,
            Intent::ReadFile(op::ReadFile {
                path: "/missing.txt".into(),
                discard_draft: false,
            }),
        );
        runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    match incoming.recv().await.unwrap() {
                        Update::Error(reason) => panic!("duplicate client error: {reason}"),
                        Update::Presentation { value, .. }
                            if value
                                .snapshot
                                .operation_error(op::OperationKey::File)
                                .is_some() =>
                        {
                            let notices = value.snapshot.error_notices(ErrorScope::Workspace);
                            assert_eq!(notices.len(), 1);
                            client.dispatch(
                                &host.id,
                                Intent::DismissError {
                                    id: notices[0].id.clone(),
                                },
                            );
                            break;
                        }
                        _ => {}
                    }
                }
                loop {
                    match incoming.recv().await.unwrap() {
                        Update::Error(reason) => panic!("undismissed client error: {reason}"),
                        Update::Presentation { value, .. }
                            if value
                                .snapshot
                                .error_notices(ErrorScope::Workspace)
                                .is_empty() =>
                        {
                            break;
                        }
                        _ => {}
                    }
                }
            })
            .await
            .unwrap();
            client.shutdown().await.unwrap();
            while let Ok(update) = incoming.try_recv() {
                if let Update::Error(reason) = update {
                    panic!("late client error: {reason}");
                }
            }
        });
    }

    #[test]
    fn a_preview_retains_its_file_until_its_last_owner_closes() {
        let directory = tempfile::tempdir().unwrap();
        saved_host(directory.path());
        let (runtime, client, _) = Client::new(directory.path().into()).unwrap();
        let file = runtime
            .block_on(client.cache_image(vec![1, 2, 3], "png".into()).recv())
            .unwrap()
            .unwrap();
        let path = file.path.clone();
        let preview = file.clone();
        drop(file);
        assert_eq!(std::fs::read(&path).unwrap(), [1, 2, 3]);
        let unrelated = path.parent().unwrap().parent().unwrap().join("keep.txt");
        std::fs::write(&unrelated, "keep").unwrap();
        drop(preview);
        assert!(!path.exists());
        assert_eq!(std::fs::read_to_string(unrelated).unwrap(), "keep");
        runtime.block_on(client.shutdown()).unwrap();
    }

    #[test]
    fn closing_an_image_view_before_transfer_finishes_leaves_no_file() {
        let directory = tempfile::tempdir().unwrap();
        saved_host(directory.path());
        let (runtime, client, _) = Client::new(directory.path().into()).unwrap();
        drop(client.cache_image(vec![1, 2, 3], "png".into()));
        runtime.block_on(client.shutdown()).unwrap();
        assert_eq!(
            std::fs::read_dir(directory.path().join("downloads"))
                .unwrap()
                .count(),
            0
        );
    }
}
