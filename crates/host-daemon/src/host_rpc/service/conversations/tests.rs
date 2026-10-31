use super::*;
use crate::providers::{
    AccountCommand, AccountReply, AgentChange, AgentEvent, Identity, SessionPage, SessionSummary,
    SubmissionState, emit,
};
use agent_protocol::{
    composer::ComposerCatalog,
    execution::{ItemStatus, SessionStatus, TurnStatus},
    ids::TurnId,
    items::{AssistantPhase, ItemBody},
    models::{Empty, Item, ModelRef, Turn},
    permissions::{PermissionMode, PermissionSettings},
    session::{Capabilities, SessionChange},
};
use serde::de::DeserializeOwned;
use std::{
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::sync::mpsc;

/// Independent native sessions, including persisted history and provider
/// acknowledgements. Assertions observe the Host boundary, not a reimplementation
/// of its handoff/routing decisions.
struct NativeAgent {
    provider: ProviderKind,
    directory: PathBuf,
    records: Arc<Mutex<HashMap<String, ThreadResponse>>>,
    inputs: Arc<Mutex<Vec<op::Submission>>>,
    injected: Arc<Mutex<Vec<(String, handoff::Context)>>>,
    released: Arc<Mutex<Vec<String>>>,
    next: Arc<AtomicUsize>,
    fail_submit: AtomicUsize,
    fail_inject: AtomicUsize,
    fail_open: AtomicUsize,
    events: mpsc::Sender<AgentEvent>,
    receiver: Mutex<Option<mpsc::Receiver<AgentEvent>>>,
}

fn unsupported() -> Failure {
    Failure::new("unsupported", "not used in this fixture")
}

impl NativeAgent {
    fn new(provider: ProviderKind, directory: PathBuf) -> Arc<Self> {
        let (events, receiver) = mpsc::channel(32);
        Arc::new(Self {
            provider,
            directory,
            records: Default::default(),
            inputs: Default::default(),
            injected: Default::default(),
            released: Default::default(),
            next: Default::default(),
            fail_submit: AtomicUsize::new(0),
            fail_inject: AtomicUsize::new(0),
            fail_open: AtomicUsize::new(0),
            events,
            receiver: Mutex::new(Some(receiver)),
        })
    }
    fn restarted(&self) -> Arc<Self> {
        let (events, receiver) = mpsc::channel(32);
        Arc::new(Self {
            provider: self.provider,
            directory: self.directory.clone(),
            records: self.records.clone(),
            inputs: self.inputs.clone(),
            injected: self.injected.clone(),
            released: self.released.clone(),
            next: self.next.clone(),
            fail_submit: AtomicUsize::new(0),
            fail_inject: AtomicUsize::new(0),
            fail_open: AtomicUsize::new(0),
            events,
            receiver: Mutex::new(Some(receiver)),
        })
    }
    fn backend(self: &Arc<Self>) -> Backend {
        Backend {
            provider: self.provider,
            agent: Ok(self.clone()),
            dictation: None,
        }
    }
}

#[async_trait::async_trait]
impl Identity for NativeAgent {
    async fn list(&self) -> Result<op::Accounts, Failure> {
        Ok(op::Accounts {
            accounts: vec![],
            selected: Default::default(),
            error: None,
        })
    }
    async fn account(&self, _: AccountCommand) -> Result<AccountReply, Failure> {
        Err(unsupported())
    }
    async fn usage(&self, _: &str) -> Result<op::AccountUsage, Failure> {
        Err(unsupported())
    }
}

#[async_trait::async_trait]
impl Agent for NativeAgent {
    fn data_recipient(&self) -> &'static str {
        "fixture"
    }
    fn running_input(&self) -> crate::providers::RunningInput {
        crate::providers::RunningInput::SteerOrQueue
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            additional_input: true,
            model_change: true,
            rename: true,
            fork: self.provider == ProviderKind::Codex,
        }
    }
    fn availability(&self) -> Result<(), Failure> {
        Ok(())
    }
    fn validate_create(&self) -> Result<(), Failure> {
        Ok(())
    }
    fn storage_directory(&self) -> &Path {
        &self.directory
    }
    async fn list(
        &self,
        search: &str,
        _: Option<String>,
        scope: SessionListScope<'_>,
        _: u32,
    ) -> Result<SessionPage, Failure> {
        let mut data = self
            .records
            .lock()
            .unwrap()
            .values()
            .filter(|record| {
                !matches!(scope, SessionListScope::Descendants(_))
                    && record
                        .thread
                        .preview
                        .as_deref()
                        .unwrap_or("")
                        .contains(search)
            })
            .map(|record| {
                let mut thread = record.thread.clone();
                thread.turns = None;
                SessionSummary {
                    thread,
                    branch: None,
                }
            })
            .collect::<Vec<_>>();
        data.sort_by(|a, b| {
            b.thread
                .updated_at
                .partial_cmp(&a.thread.updated_at)
                .unwrap()
        });
        Ok(SessionPage {
            data,
            next_cursor: None,
        })
    }
    async fn open(&self, id: &str, limit: usize, _: bool) -> Result<ThreadResponse, Failure> {
        if self.fail_open.swap(0, Ordering::SeqCst) != 0 {
            return Err(Failure::new(
                "native_resume_failed",
                "native session cannot be loaded",
            ));
        }
        let mut response = self
            .records
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(unsupported)?;
        let turns = response.thread.turns.take().unwrap_or_default();
        let start = turns.len().saturating_sub(limit);
        response.thread.turns = Some(turns[start..].to_vec());
        response.thread.history_cursor = (start > 0).then(|| start.to_string());
        response.thread.history_has_more = Some(start > 0);
        Ok(response)
    }
    async fn read_history(&self, id: &str, cursor: &str, _: bool) -> Result<HistoryPage, Failure> {
        let response = self
            .records
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(unsupported)?;
        let end: usize = cursor.parse().unwrap();
        let start = end.saturating_sub(100);
        let turns = response.thread.turns.unwrap();
        Ok(HistoryPage {
            turns: turns[start..end].to_vec(),
            next_cursor: (start > 0).then(|| start.to_string()),
        })
    }
    async fn read_item(&self, params: &op::ReadItem) -> Result<op::ItemResponse, Failure> {
        let records = self.records.lock().unwrap();
        let item = records
            .get(&params.thread_id.id)
            .and_then(|record| record.thread.turns.as_ref())
            .and_then(|turns| turns.iter().rev().find(|turn| turn.id == params.turn_id))
            .and_then(|turn| turn.items.as_ref())
            .and_then(|items| items.iter().find(|item| item.id == params.item_id))
            .cloned()
            .ok_or_else(unsupported)?;
        Ok(op::ItemResponse {
            item: (*item).clone(),
            transfer: None,
        })
    }
    async fn read_turn_items(&self, id: &str, turn: &TurnId) -> Result<Vec<Arc<Item>>, Failure> {
        Ok(self
            .records
            .lock()
            .unwrap()
            .get(id)
            .and_then(|record| record.thread.turns.as_ref())
            .and_then(|turns| turns.iter().rev().find(|candidate| &candidate.id == turn))
            .and_then(|turn| turn.items.clone())
            .ok_or_else(unsupported)?)
    }
    async fn create(
        &self,
        cwd: &str,
        model: Option<&str>,
        _: Option<serde_json::Value>,
    ) -> Result<ThreadResponse, Failure> {
        let id = format!(
            "{}-{}",
            self.provider.key(),
            self.next.fetch_add(1, Ordering::SeqCst)
        );
        let response = ThreadResponse {
            thread: Thread {
                id: Some(SessionRef {
                    provider: self.provider,
                    id: id.clone(),
                }),
                cwd: Some(cwd.into()),
                name: Some(format!("{} conversation", self.provider.key())),
                status: SessionStatus::Idle,
                turns: Some(vec![]),
                ..Default::default()
            },
            model: Some(ModelRef {
                provider: self.provider,
                id: model.unwrap_or("model").into(),
            }),
            ..Default::default()
        };
        self.records.lock().unwrap().insert(id, response.clone());
        Ok(response)
    }
    async fn state(&self, id: &str) -> Result<SubmissionState, Failure> {
        Ok(SubmissionState {
            response: self.open(id, usize::MAX, true).await?,
            needs_reload: false,
        })
    }
    async fn inject_history(&self, id: &str, context: &handoff::Context) -> Result<bool, Failure> {
        if self.provider != ProviderKind::Codex {
            return Ok(false);
        }
        self.injected
            .lock()
            .unwrap()
            .push((id.into(), context.clone()));
        if self.fail_inject.swap(0, Ordering::SeqCst) != 0 {
            return Err(Failure::unknown(
                "injection_unknown",
                "connection lost after writing history",
            ));
        }
        Ok(true)
    }
    async fn unload(&self, id: &str) -> Result<(), Failure> {
        self.released.lock().unwrap().push(id.into());
        Ok(())
    }
    async fn submit(
        &self,
        input: &op::Submission,
        _: crate::providers::SubmissionTarget<'_>,
        _: bool,
        _: Option<serde_json::Value>,
    ) -> Result<op::SubmissionReceipt, Failure> {
        match self.fail_submit.swap(0, Ordering::SeqCst) {
            1 => return Err(Failure::new("refused", "prompt refused before consumption")),
            2 => {
                return Err(Failure::unknown(
                    "submission_unknown",
                    "acknowledgement lost",
                ));
            }
            _ => {}
        }
        self.inputs.lock().unwrap().push(input.clone());
        let ordinal = self.records.lock().unwrap()[&input.thread_id.id]
            .thread
            .turns
            .as_ref()
            .map_or(0, Vec::len);
        let mut user = Item::new(
            format!("user-{ordinal}").into(),
            ItemStatus::Completed,
            ItemBody::UserMessage {
                text: None,
                content: op::Input::message_parts(&input.input),
            },
        );
        user.client_input_id = Some(input.client_user_message_id.clone());
        let turn_id: TurnId = format!("turn-{ordinal}").into();
        let turn = Turn {
            id: turn_id.clone(),
            status: TurnStatus::Completed,
            items: Some(vec![
                Arc::new(user),
                Arc::new(Item::new(
                    format!("answer-{ordinal}").into(),
                    ItemStatus::Completed,
                    ItemBody::AssistantText {
                        text: format!("answer {}", input.client_user_message_id),
                        phase: AssistantPhase::Final,
                    },
                )),
            ]),
            ..Default::default()
        };
        {
            let mut records = self.records.lock().unwrap();
            let record = records.get_mut(&input.thread_id.id).unwrap();
            record
                .thread
                .turns
                .get_or_insert_default()
                .push(Arc::new(turn.clone()));
            record.thread.updated_at = Some(record.thread.turns.as_ref().unwrap().len() as f64);
            record
                .thread
                .preview
                .get_or_insert_with(|| format!("request {}", input.client_user_message_id));
            record.model = input.model.clone();
        }
        emit(
            &self.events,
            AgentChange::Session {
                session: input.thread_id.clone(),
                change: SessionChange::Turn {
                    turn,
                    completed: true,
                },
            },
        )
        .await
        .map_err(|error| Failure::new("event_failed", error))?;
        Ok(op::SubmissionReceipt {
            turn_id: Some(turn_id),
        })
    }
    async fn interrupt(&self, _: &str, _: &TurnId) -> Result<Empty, Failure> {
        Ok(Empty {})
    }
    async fn models(&self, _: &op::ListModels) -> Result<op::ModelPage, Failure> {
        Ok(op::ModelPage {
            data: vec![],
            next_cursor: None,
            provider_errors: None,
        })
    }
    async fn catalog(&self, _: &str) -> ComposerCatalog {
        ComposerCatalog::default()
    }
    async fn discard_workspace_processes(&self, _: &Path) -> Result<(), Failure> {
        Ok(())
    }
    async fn read_permissions(&self) -> Result<PermissionSettings, Failure> {
        Err(unsupported())
    }
    async fn update_permissions(
        &self,
        _: PermissionMode,
        _: &str,
    ) -> Result<PermissionSettings, Failure> {
        Err(unsupported())
    }
    async fn fork(
        &self,
        id: &str,
        turn: &str,
        _: Option<serde_json::Value>,
    ) -> Result<ThreadResponse, Failure> {
        let source = self.open(id, usize::MAX, true).await?;
        let mut response = self
            .create(source.thread.cwd.as_deref().unwrap(), None, None)
            .await?;
        let mut turns = source.thread.turns.unwrap();
        turns.truncate(
            turns
                .iter()
                .rposition(|candidate| candidate.id.as_str() == turn)
                .unwrap()
                + 1,
        );
        response.thread.turns = Some(turns);
        self.records.lock().unwrap().insert(
            response.thread.id.as_ref().unwrap().id.clone(),
            response.clone(),
        );
        Ok(response)
    }
    async fn rename(&self, _: &str, _: &str) -> Result<Empty, Failure> {
        Ok(Empty {})
    }
    fn event_stream(&self) -> Option<mpsc::Receiver<AgentEvent>> {
        self.receiver.lock().unwrap().take()
    }
    async fn shutdown(&self) {}
}

fn service(path: &Path, a: &Arc<NativeAgent>, b: &Arc<NativeAgent>) -> HostRpcService {
    HostRpcService::new(
        [a.backend(), b.backend()],
        ProjectStore::new(path.join("worktrees.json")),
    )
}

#[tokio::test]
async fn app_update_resumes_each_interrupted_conversation_once_with_its_settings() {
    use agent_local_host::update::{ContinuationDelivery, RECOVERY_FILE, Recovery};
    let directory = tempfile::tempdir().unwrap();
    let a = NativeAgent::new(ProviderKind::Codex, directory.path().join("codex"));
    let b = NativeAgent::new(ProviderKind::Claude, directory.path().join("claude"));
    let mut ids = Vec::new();
    for agent in [&a, &b] {
        let mut response = agent
            .create(
                directory.path().to_str().unwrap(),
                Some("selected-model"),
                None,
            )
            .await
            .unwrap();
        let id = response.thread.id.clone().unwrap();
        response.thread.status = SessionStatus::Running;
        response.thread.turns = Some(vec![Arc::new(Turn {
            id: "interrupted-turn".into(),
            status: TurnStatus::Running,
            ..Default::default()
        })]);
        response.effort = Some("high".into());
        response.service_tier = Some("priority".into());
        agent
            .records
            .lock()
            .unwrap()
            .insert(id.id.clone(), response);
        ids.push(id);
    }
    let host = service(directory.path(), &a, &b);
    host.set_history_scope(directory.path().to_owned(), true);
    host.prepare_update().await.unwrap();
    let recovery_path = directory.path().join(RECOVERY_FILE);
    let recovery: Recovery =
        serde_json::from_slice(&std::fs::read(&recovery_path).unwrap()).unwrap();
    assert_eq!(recovery.tasks.len(), 2);
    assert!(
        recovery
            .tasks
            .iter()
            .all(
                |task| task.last_turn.as_ref().unwrap().as_str() == "interrupted-turn"
                    && task.cwd.as_deref() == directory.path().to_str()
            )
    );
    let connection = host.open_session();
    assert_eq!(
        send(
            &host,
            connection.id(),
            &ids[0],
            ProviderKind::Codex,
            "during-update",
            "new input"
        )
        .await
        .unwrap_err()
        .code,
        "host_updating"
    );
    assert!(host.prepare_update().await.is_err());
    drop(host);
    let (a, b) = (a.restarted(), b.restarted());
    for agent in [&a, &b] {
        for response in agent.records.lock().unwrap().values_mut() {
            response.thread.status = SessionStatus::Idle;
            Arc::make_mut(&mut response.thread.turns.as_mut().unwrap()[0]).status =
                TurnStatus::Interrupted;
        }
    }
    let host = service(directory.path(), &a, &b);
    host.resume_after_update(directory.path()).await.unwrap();
    host.resume_after_update(directory.path()).await.unwrap();
    for (agent, id) in [&a, &b].into_iter().zip(ids) {
        let inputs = agent.inputs.lock().unwrap();
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].thread_id, id);
        assert_eq!(inputs[0].model.as_ref().unwrap().id, "selected-model");
        assert_eq!(inputs[0].effort.as_deref(), Some("high"));
        assert_eq!(inputs[0].service_tier.as_deref(), Some("priority"));
        assert!(
            matches!(&inputs[0].input[0], op::Input::Text { text } if text.contains("中断したところから"))
        );
    }
    let recovery: Recovery =
        serde_json::from_slice(&std::fs::read(recovery_path).unwrap()).unwrap();
    assert!(recovery.tasks.iter().all(|task| matches!(&task.delivery, ContinuationDelivery::Accepted(turn) if turn.as_str() == "turn-1")));
}

#[tokio::test]
async fn app_update_does_not_repeat_uncertain_failed_or_finished_continuations() {
    use agent_local_host::update::{
        ContinuationDelivery, InterruptedTask, RECOVERY_FILE, Recovery,
    };
    let directory = tempfile::tempdir().unwrap();
    let a = NativeAgent::new(ProviderKind::Codex, directory.path().join("codex"));
    let b = NativeAgent::new(ProviderKind::Claude, directory.path().join("claude"));
    let mut tasks = Vec::new();
    for (delivery, finished) in [
        (ContinuationDelivery::Pending, false),
        (ContinuationDelivery::Sending, false),
        (ContinuationDelivery::Failed("already failed".into()), false),
        (ContinuationDelivery::Pending, true),
    ] {
        let mut response = a
            .create(directory.path().to_str().unwrap(), None, None)
            .await
            .unwrap();
        if finished {
            response.thread.turns = Some(vec![Arc::new(Turn {
                id: "finished-during-shutdown".into(),
                status: TurnStatus::Completed,
                ..Default::default()
            })]);
            a.records.lock().unwrap().insert(
                response.thread.id.as_ref().unwrap().id.clone(),
                response.clone(),
            );
        }
        let last_turn = finished.then(|| "finished-during-shutdown".into());
        let session = response.thread.id.unwrap();
        tasks.push(InterruptedTask {
            native_session: session.clone(),
            submission: op::Submission {
                thread_id: session,
                client_user_message_id: uuid::Uuid::new_v4().to_string().into(),
                input: vec![op::Input::Text {
                    text: "Continue".into(),
                }],
                model: None,
                effort: None,
                service_tier: None,
            },
            cwd: None,
            last_turn,
            delivery,
        });
    }
    let path = directory.path().join(RECOVERY_FILE);
    agent_local_host::save_private_json(&path, &Recovery { tasks }).unwrap();
    a.fail_submit.store(1, Ordering::SeqCst);
    let host = service(directory.path(), &a, &b);
    host.resume_after_update(directory.path()).await.unwrap();
    host.resume_after_update(directory.path()).await.unwrap();
    assert!(a.inputs.lock().unwrap().is_empty());
    let recovery: Recovery = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(matches!(
        recovery.tasks[0].delivery,
        ContinuationDelivery::Failed(_)
    ));
    assert_eq!(recovery.tasks[1].delivery, ContinuationDelivery::Sending);
    assert!(matches!(
        recovery.tasks[2].delivery,
        ContinuationDelivery::Failed(_)
    ));
    assert_eq!(recovery.tasks[3].delivery, ContinuationDelivery::Completed);
}

async fn call<T: DeserializeOwned>(
    service: &HostRpcService,
    session: SessionId,
    request: Call,
) -> Result<T, agent_protocol::error::RpcFailure> {
    let reply = service.dispatch(session, &request).await.unwrap();
    match agent_protocol::protocol::decode::<Response<T>>(&reply.initial).unwrap() {
        Response::Success { result } => Ok(result),
        Response::Failure { error } => Err(error),
    }
}

async fn send(
    service: &HostRpcService,
    session: SessionId,
    root: &SessionRef,
    provider: ProviderKind,
    id: &str,
    text: &str,
) -> Result<op::SubmissionReceipt, agent_protocol::error::RpcFailure> {
    call(
        service,
        session,
        Call::Submit(op::Submission {
            thread_id: root.clone(),
            client_user_message_id: id.into(),
            input: vec![op::Input::Text { text: text.into() }],
            model: Some(ModelRef {
                provider,
                id: "model".into(),
            }),
            effort: None,
            service_tier: None,
        }),
    )
    .await
}

async fn history(
    service: &HostRpcService,
    session: SessionId,
    root: &SessionRef,
) -> ThreadResponse {
    call::<agent_protocol::session::OpenedSession>(
        service,
        session,
        Call::OpenSession(agent_protocol::session::OpenSession {
            session: root.clone(),
            limit: 1000,
            include_activity: true,
        }),
    )
    .await
    .unwrap()
    .response
}

#[tokio::test]
async fn native_fork_rejects_an_unsupported_provider_before_execution() {
    let directory = tempfile::tempdir().unwrap();
    let a = NativeAgent::new(ProviderKind::Codex, directory.path().join("codex"));
    let b = NativeAgent::new(ProviderKind::Claude, directory.path().join("claude"));
    let root = b
        .create(directory.path().to_str().unwrap(), None, None)
        .await
        .unwrap()
        .thread
        .id
        .unwrap();
    let host = service(directory.path(), &a, &b);
    let client = host.open_session();
    let turn = send(
        &host,
        client.id(),
        &root,
        ProviderKind::Claude,
        "native",
        "Native task",
    )
    .await
    .unwrap()
    .turn_id
    .unwrap();
    let denied = call::<ThreadResponse>(
        &host,
        client.id(),
        Call::ForkSession(op::ForkSession::new(root, turn)),
    )
    .await
    .unwrap_err();
    assert_eq!(denied.code, "unsupported_operation");
    assert_eq!(
        denied.message,
        "host/session/fork is unsupported by this provider"
    );
}

#[tokio::test]
async fn unavailable_current_provider_does_not_block_earlier_segments_or_local_rename() {
    let directory = tempfile::tempdir().unwrap();
    let a = NativeAgent::new(ProviderKind::Codex, directory.path().join("codex"));
    let b = NativeAgent::new(ProviderKind::Claude, directory.path().join("claude"));
    let root = a
        .create(directory.path().to_str().unwrap(), None, None)
        .await
        .unwrap()
        .thread
        .id
        .unwrap();
    let host = service(directory.path(), &a, &b);
    let client = host.open_session();
    let original = send(
        &host,
        client.id(),
        &root,
        ProviderKind::Codex,
        "original",
        "Original task",
    )
    .await
    .unwrap()
    .turn_id
    .unwrap();
    let current = send(
        &host,
        client.id(),
        &root,
        ProviderKind::Claude,
        "review",
        "Review the task",
    )
    .await
    .unwrap()
    .turn_id
    .unwrap();
    let cached = history(&host, client.id(), &root).await;
    drop(client);
    drop(host);

    // The saved handoff survives a restart, while only its current provider fails.
    let host = HostRpcService::new(
        [
            a.restarted().backend(),
            Backend::unavailable(ProviderKind::Claude, "Claude failed to start"),
        ],
        ProjectStore::new(directory.path().join("worktrees.json")),
    );
    let mut client = host.open_session();
    // Detail reads hydrate the established stream, independently of provider startup.
    let lease = host.inner.router.retain_execution(root.clone()).unwrap();
    let mut updates = host
        .inner
        .router
        .finish_session_read(lease, client.id(), cached)
        .unwrap()
        .updates
        .unwrap();
    let _: Empty = call(
        &host,
        client.id(),
        Call::RenameSession(op::RenameSession {
            thread_id: root.clone(),
            name: "Local title".into(),
        }),
    )
    .await
    .unwrap();
    let notice = tokio::time::timeout(std::time::Duration::from_secs(2), client.recv())
        .await
        .unwrap()
        .unwrap();
    let notice: agent_protocol::protocol::Notification =
        agent_protocol::protocol::decode(&notice).unwrap();
    assert!(
        matches!(notice, agent_protocol::protocol::Notification::SessionRenamed { session, name: Some(name), .. } if session == root && name == "Local title")
    );
    assert_eq!(
        host.inner
            .conversations
            .get(&root)
            .unwrap()
            .unwrap()
            .name
            .as_deref(),
        Some("Local title")
    );
    let _: Empty = call(
        &host,
        client.id(),
        Call::ReadTurnItems(agent_protocol::session::ReadTurnItems {
            session: root.clone(),
            turn_id: original.clone(),
        }),
    )
    .await
    .unwrap();
    let hydration = tokio::time::timeout(std::time::Duration::from_secs(2), updates.recv())
        .await
        .unwrap()
        .unwrap();
    let hydration: SessionChange = agent_protocol::protocol::decode(&hydration).unwrap();
    let SessionChange::TurnItems { turn_id, items } = hydration else {
        panic!("turn hydration missing")
    };
    assert_eq!(turn_id, original);
    let texts = handoff::Message::from_turn(&Turn {
        items: Some(items),
        ..Default::default()
    })
    .into_iter()
    .map(|message| message.text)
    .collect::<Vec<_>>();
    assert_eq!(texts, ["Original task", "answer original"]);
    let fork: ThreadResponse = call(
        &host,
        client.id(),
        Call::ForkSession(op::ForkSession::new(root.clone(), original)),
    )
    .await
    .unwrap();
    assert_ne!(fork.thread.id.as_ref(), Some(&root));
    assert_eq!(fork.thread.active_provider, Some(ProviderKind::Codex));
    let texts = fork
        .thread
        .turns
        .as_ref()
        .unwrap()
        .iter()
        .flat_map(|turn| handoff::Message::from_turn(turn))
        .map(|message| message.text)
        .collect::<Vec<_>>();
    assert_eq!(texts, ["Original task", "answer original"]);
    let error = call::<Empty>(
        &host,
        client.id(),
        Call::Interrupt(op::Interrupt {
            thread_id: root,
            turn_id: current,
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "provider_unavailable");
    assert_eq!(error.message, "Claude failed to start");
}

#[tokio::test]
async fn reopening_after_restart_restores_only_accepted_turn_settings() {
    let directory = tempfile::tempdir().unwrap();
    let a = NativeAgent::new(ProviderKind::Codex, directory.path().join("codex"));
    let b = NativeAgent::new(ProviderKind::Claude, directory.path().join("claude"));
    let root = a
        .create(directory.path().to_str().unwrap(), Some("model"), None)
        .await
        .unwrap()
        .thread
        .id
        .unwrap();
    let host = service(directory.path(), &a, &b);
    let client = host.open_session();
    let input = |id: &str, provider, effort: &str, tier: &str| op::Submission {
        thread_id: root.clone(),
        client_user_message_id: id.into(),
        input: vec![op::Input::Text {
            text: "Continue".into(),
        }],
        model: Some(ModelRef {
            provider,
            id: "model".into(),
        }),
        effort: Some(effort.into()),
        service_tier: Some(tier.into()),
    };
    call::<op::SubmissionReceipt>(
        &host,
        client.id(),
        Call::Submit(input("first", ProviderKind::Codex, "high", "priority")),
    )
    .await
    .unwrap();
    let second_client = host.open_session();
    let reopened = history(&host, second_client.id(), &root).await;
    assert_eq!(reopened.effort.as_deref(), Some("high"));
    assert_eq!(reopened.service_tier.as_deref(), Some("priority"));
    // Switching providers must address settings by native identity while the
    // client continues opening the original public conversation.
    call::<op::SubmissionReceipt>(
        &host,
        client.id(),
        Call::Submit(input("switch", ProviderKind::Claude, "max", "default")),
    )
    .await
    .unwrap();
    drop(second_client);
    drop(client);
    drop(host);
    let a = a.restarted();
    let b = b.restarted();
    let host = service(directory.path(), &a, &b);
    let client = host.open_session();
    let reopened = history(&host, client.id(), &root).await;
    assert_eq!(reopened.model.unwrap().provider, ProviderKind::Claude);
    assert_eq!(reopened.effort.as_deref(), Some("max"));
    assert_eq!(reopened.service_tier.as_deref(), Some("default"));
    b.fail_submit.store(1, Ordering::SeqCst);
    assert!(
        call::<op::SubmissionReceipt>(
            &host,
            client.id(),
            Call::Submit(input("refused", ProviderKind::Claude, "low", "priority"))
        )
        .await
        .is_err()
    );
    let reopened = history(&host, client.id(), &root).await;
    assert_eq!(reopened.effort.as_deref(), Some("max"));
    assert_eq!(reopened.service_tier.as_deref(), Some("default"));
}

#[tokio::test]
async fn round_trip_reuses_native_sessions_transfers_only_missing_history_and_reopens_after_restart()
 {
    let directory = tempfile::tempdir().unwrap();
    let a = NativeAgent::new(ProviderKind::Codex, directory.path().join("codex"));
    let b = NativeAgent::new(ProviderKind::Claude, directory.path().join("claude"));
    let root = a
        .create(directory.path().to_str().unwrap(), Some("model"), None)
        .await
        .unwrap()
        .thread
        .id
        .unwrap();
    let host = service(directory.path(), &a, &b);
    let client = host.open_session();
    send(
        &host,
        client.id(),
        &root,
        ProviderKind::Codex,
        "a1",
        "Keep this original constraint",
    )
    .await
    .unwrap();
    let first = send(
        &host,
        client.id(),
        &root,
        ProviderKind::Claude,
        "b1",
        "Review the implementation",
    )
    .await
    .unwrap();
    let b_input = b.inputs.lock().unwrap()[0].clone();
    assert_eq!(
        a.released.lock().unwrap().as_slice(),
        std::slice::from_ref(&root.id)
    );
    let op::Input::Text { text } = &b_input.input[0] else {
        panic!()
    };
    assert!(text.contains("Keep this original constraint"));
    assert!(text.contains("answer a1"));
    let second = send(
        &host,
        client.id(),
        &root,
        ProviderKind::Codex,
        "a2",
        "Fix the review findings",
    )
    .await
    .unwrap();
    let injection = a.injected.lock().unwrap()[0].clone();
    assert_eq!(injection.0, root.id);
    assert_eq!(
        injection
            .1
            .messages
            .iter()
            .map(|message| message.text.as_str())
            .collect::<Vec<_>>(),
        ["Review the implementation", "answer b1"]
    );
    assert_eq!(a.next.load(Ordering::SeqCst), 1);
    let _: agent_protocol::models::Empty = call(
        &host,
        client.id(),
        Call::RenameSession(op::RenameSession {
            thread_id: root.clone(),
            name: "Shared logical title".into(),
        }),
    )
    .await
    .unwrap();
    let listed: agent_protocol::models::ThreadList = call(
        &host,
        client.id(),
        Call::ListSessions(op::ListSessions {
            query: agent_protocol::models::ListQuery {
                search_term: "Shared logical".into(),
                ..Default::default()
            },
        }),
    )
    .await
    .unwrap();
    assert_eq!(listed.data.len(), 1);
    assert_eq!(listed.data[0].id.as_ref(), Some(&root));
    let visible = history(&host, client.id(), &root).await;
    assert_eq!(visible.thread.id.as_ref(), Some(&root));
    assert_eq!(visible.thread.active_provider, Some(ProviderKind::Codex));
    let turns = visible.thread.turns.as_ref().unwrap();
    assert_eq!(
        turns
            .iter()
            .map(|turn| turn.provider.unwrap())
            .collect::<Vec<_>>(),
        [
            ProviderKind::Codex,
            ProviderKind::Claude,
            ProviderKind::Codex
        ]
    );
    let texts = turns
        .iter()
        .flat_map(|turn| handoff::Message::from_turn(turn))
        .map(|message| message.text)
        .collect::<Vec<_>>();
    assert_eq!(
        texts,
        [
            "Keep this original constraint",
            "answer a1",
            "Review the implementation",
            "answer b1",
            "Fix the review findings",
            "answer a2"
        ]
    );
    assert_ne!(first.turn_id, second.turn_id);
    let detail: op::ItemResponse = call(
        &host,
        client.id(),
        Call::ReadItem(op::ReadItem {
            thread_id: root.clone(),
            turn_id: turns[1].id.clone(),
            item_id: turns[1].items.as_ref().unwrap()[0].id.clone(),
        }),
    )
    .await
    .unwrap();
    assert_eq!(
        handoff::Message::from_turn(&Turn {
            items: Some(vec![Arc::new(detail.item)]),
            ..Default::default()
        })[0]
            .text,
        "Review the implementation"
    );
    let fork: ThreadResponse = call(
        &host,
        client.id(),
        Call::ForkSession(op::ForkSession::new(root.clone(), turns[2].id.clone())),
    )
    .await
    .unwrap();
    assert_eq!(fork.thread.turns.as_ref().unwrap().len(), 3);
    assert_ne!(fork.thread.id, Some(root.clone()));
    drop(client);
    drop(host);
    let a = a.restarted();
    let b = b.restarted();
    let restarted = service(directory.path(), &a, &b);
    let client = restarted.open_session();
    assert_eq!(
        history(&restarted, client.id(), &root)
            .await
            .thread
            .turns
            .as_ref()
            .unwrap()
            .len(),
        3
    );
    send(
        &restarted,
        client.id(),
        &root,
        ProviderKind::Claude,
        "b2",
        "Check the fix",
    )
    .await
    .unwrap();
    assert_eq!(b.next.load(Ordering::SeqCst), 1);
    let inputs = b.inputs.lock().unwrap();
    let op::Input::Text { text } = &inputs[1].input[0] else {
        panic!()
    };
    assert!(text.contains("Fix the review findings"));
    assert!(!text.contains("Keep this original constraint"));
}

#[tokio::test]
async fn replayed_handoff_receipts_do_not_block_later_provider_switches() {
    use agent_protocol::session::SubmissionDelivery;

    let directory = tempfile::tempdir().unwrap();
    let a = NativeAgent::new(ProviderKind::Codex, directory.path().join("codex"));
    let b = NativeAgent::new(ProviderKind::Claude, directory.path().join("claude"));
    let root = b
        .create(directory.path().to_str().unwrap(), None, None)
        .await
        .unwrap()
        .thread
        .id
        .unwrap();
    let host = service(directory.path(), &a, &b);
    let client = host.open_session();
    send(
        &host,
        client.id(),
        &root,
        ProviderKind::Claude,
        "b1",
        "Original",
    )
    .await
    .unwrap();
    let receipt = send(
        &host,
        client.id(),
        &root,
        ProviderKind::Codex,
        "a1",
        "Continue",
    )
    .await
    .unwrap();
    // A retry replays the persisted acknowledgement without another native
    // completion event. Its public turn ID still identifies finished history.
    let replayed = send(
        &host,
        client.id(),
        &root,
        ProviderKind::Codex,
        "a1",
        "Continue",
    )
    .await
    .unwrap();
    assert_eq!(receipt.turn_id, replayed.turn_id);
    assert_eq!(a.inputs.lock().unwrap().len(), 1);
    let opened = history(&host, client.id(), &root).await;
    assert!(opened.thread.active_turn_id().is_none());
    assert!(opened.thread.submissions.contains_key("a1"));
    let row = host.inner.conversations.get(&root).unwrap().unwrap();
    host.inner
        .router
        .begin_submission(&root, "pending")
        .unwrap();
    for delivery in [
        SubmissionDelivery::Sending,
        SubmissionDelivery::Unknown,
        SubmissionDelivery::Accepted { turn_id: None },
        SubmissionDelivery::Accepted {
            turn_id: Some(row.identity(&row.active, "unconfirmed").into()),
        },
        SubmissionDelivery::Accepted {
            turn_id: Some("bex:invalid".into()),
        },
    ] {
        host.inner
            .router
            .finish_submission(&root, "pending", delivery);
        assert_eq!(
            send(
                &host,
                client.id(),
                &root,
                ProviderKind::Claude,
                "b2",
                "Review"
            )
            .await
            .unwrap_err()
            .code,
            "provider_switch_busy"
        );
    }
    host.inner
        .router
        .finish_submission(&root, "pending", SubmissionDelivery::Rejected);
    send(
        &host,
        client.id(),
        &root,
        ProviderKind::Claude,
        "b2",
        "Review",
    )
    .await
    .unwrap();
    send(
        &host,
        client.id(),
        &root,
        ProviderKind::Codex,
        "a2",
        "Finish",
    )
    .await
    .unwrap();
    assert_eq!(a.inputs.lock().unwrap().len(), 2);
    assert_eq!(b.inputs.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn large_saved_history_and_new_requests_reach_the_same_native_session_for_compaction() {
    for provider in [ProviderKind::Codex, ProviderKind::Claude] {
        for (history_text, request_text) in [
            ("history".repeat(25_000), "Continue".into()),
            ("history".into(), "request".repeat(25_000)),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let a = NativeAgent::new(provider, directory.path().join("a"));
            let other = if provider == ProviderKind::Codex {
                ProviderKind::Claude
            } else {
                ProviderKind::Codex
            };
            let b = NativeAgent::new(other, directory.path().join("b"));
            let root = a
                .create(directory.path().to_str().unwrap(), Some("model"), None)
                .await
                .unwrap()
                .thread
                .id
                .unwrap();
            let host = service(directory.path(), &a, &b);
            let client = host.open_session();
            send(&host, client.id(), &root, provider, "first", &history_text)
                .await
                .unwrap();
            send(&host, client.id(), &root, other, "switch", &request_text)
                .await
                .unwrap();
            send(&host, client.id(), &root, provider, "return", &request_text)
                .await
                .unwrap();

            assert_eq!(a.next.load(Ordering::SeqCst), 1);
            assert_eq!(b.next.load(Ordering::SeqCst), 1);
            for agent in [&a, &b] {
                let inputs = agent.inputs.lock().unwrap();
                let last = inputs.last().unwrap();
                assert_eq!(
                    last.input.last(),
                    Some(&op::Input::Text {
                        text: request_text.clone()
                    })
                );
            }
            let visible = history(&host, client.id(), &root).await;
            assert_eq!(visible.thread.active_provider, Some(provider));
            assert_eq!(visible.thread.turns.as_ref().unwrap().len(), 3);
            let original = handoff::Message::from_turn(&visible.thread.turns.as_ref().unwrap()[0]);
            assert_eq!(original[0].text, history_text);
        }
    }
}

#[tokio::test]
async fn refused_inline_handoff_is_retryable_and_unknown_turn_delivery_is_not_replayed() {
    let directory = tempfile::tempdir().unwrap();
    let a = NativeAgent::new(ProviderKind::Codex, directory.path().join("a"));
    let b = NativeAgent::new(ProviderKind::Claude, directory.path().join("b"));
    let root = a
        .create(directory.path().to_str().unwrap(), None, None)
        .await
        .unwrap()
        .thread
        .id
        .unwrap();
    let host = service(directory.path(), &a, &b);
    let client = host.open_session();
    send(
        &host,
        client.id(),
        &root,
        ProviderKind::Codex,
        "original",
        "Original constraint",
    )
    .await
    .unwrap();
    b.fail_submit.store(1, Ordering::SeqCst);
    assert_eq!(
        send(
            &host,
            client.id(),
            &root,
            ProviderKind::Claude,
            "refused",
            "Review"
        )
        .await
        .unwrap_err()
        .delivery,
        agent_protocol::error::Delivery::NotSent
    );
    send(
        &host,
        client.id(),
        &root,
        ProviderKind::Codex,
        "extra",
        "A new source requirement",
    )
    .await
    .unwrap();
    send(
        &host,
        client.id(),
        &root,
        ProviderKind::Claude,
        "retry",
        "Review",
    )
    .await
    .unwrap();
    {
        let inputs = b.inputs.lock().unwrap();
        let op::Input::Text { text } = &inputs[0].input[0] else {
            panic!()
        };
        assert!(text.contains("Original constraint"));
        assert!(text.contains("A new source requirement"));
    }
    a.fail_submit.store(2, Ordering::SeqCst);
    assert_eq!(
        send(
            &host,
            client.id(),
            &root,
            ProviderKind::Codex,
            "unknown",
            "May already have run"
        )
        .await
        .unwrap_err()
        .delivery,
        agent_protocol::error::Delivery::Unknown
    );
    assert_eq!(
        send(
            &host,
            client.id(),
            &root,
            ProviderKind::Codex,
            "unknown",
            "May already have run"
        )
        .await
        .unwrap_err()
        .delivery,
        agent_protocol::error::Delivery::Unknown
    );
    assert_eq!(a.inputs.lock().unwrap().len(), 2);
    drop(client);
    drop(host);
    let restarted = service(directory.path(), &a.restarted(), &b.restarted());
    let client = restarted.open_session();
    assert_eq!(
        send(
            &restarted,
            client.id(),
            &root,
            ProviderKind::Codex,
            "unknown",
            "May already have run"
        )
        .await
        .unwrap_err()
        .delivery,
        agent_protocol::error::Delivery::Unknown
    );
}

#[tokio::test]
async fn uncertain_native_injection_uses_a_new_binding_without_replaying_the_request() {
    let directory = tempfile::tempdir().unwrap();
    let a = NativeAgent::new(ProviderKind::Codex, directory.path().join("a"));
    let b = NativeAgent::new(ProviderKind::Claude, directory.path().join("b"));
    let root = b
        .create(directory.path().to_str().unwrap(), None, None)
        .await
        .unwrap()
        .thread
        .id
        .unwrap();
    let host = service(directory.path(), &a, &b);
    let client = host.open_session();
    send(
        &host,
        client.id(),
        &root,
        ProviderKind::Claude,
        "b1",
        "Original",
    )
    .await
    .unwrap();
    a.fail_inject.store(1, Ordering::SeqCst);
    assert!(
        send(
            &host,
            client.id(),
            &root,
            ProviderKind::Codex,
            "a1",
            "Continue"
        )
        .await
        .is_err()
    );
    assert!(a.inputs.lock().unwrap().is_empty());
    let receipt = send(
        &host,
        client.id(),
        &root,
        ProviderKind::Codex,
        "a1",
        "Continue",
    )
    .await
    .unwrap();
    let duplicate = send(
        &host,
        client.id(),
        &root,
        ProviderKind::Codex,
        "a1",
        "Continue",
    )
    .await
    .unwrap();
    assert_eq!(receipt.turn_id, duplicate.turn_id);
    assert_eq!(a.next.load(Ordering::SeqCst), 2);
    assert_eq!(a.inputs.lock().unwrap().len(), 1);
    let injections = a.injected.lock().unwrap();
    assert_ne!(injections[0].0, injections[1].0);
    assert_eq!(injections[1].1.messages[0].text, "Original");
}

#[tokio::test]
async fn a_running_source_blocks_switching_and_a_failed_resume_gets_full_context() {
    let directory = tempfile::tempdir().unwrap();
    let a = NativeAgent::new(ProviderKind::Codex, directory.path().join("a"));
    let b = NativeAgent::new(ProviderKind::Claude, directory.path().join("b"));
    let root = a
        .create(directory.path().to_str().unwrap(), None, None)
        .await
        .unwrap()
        .thread
        .id
        .unwrap();
    let host = service(directory.path(), &a, &b);
    let client = host.open_session();
    a.records
        .lock()
        .unwrap()
        .get_mut(&root.id)
        .unwrap()
        .thread
        .status = SessionStatus::Running;
    assert_eq!(
        send(
            &host,
            client.id(),
            &root,
            ProviderKind::Claude,
            "blocked",
            "Review"
        )
        .await
        .unwrap_err()
        .code,
        "provider_switch_busy"
    );
    assert_eq!(b.next.load(Ordering::SeqCst), 0);
    a.records
        .lock()
        .unwrap()
        .get_mut(&root.id)
        .unwrap()
        .thread
        .status = SessionStatus::Idle;
    send(
        &host,
        client.id(),
        &root,
        ProviderKind::Codex,
        "a1",
        "Original constraint",
    )
    .await
    .unwrap();
    send(
        &host,
        client.id(),
        &root,
        ProviderKind::Claude,
        "b1",
        "Review",
    )
    .await
    .unwrap();
    a.fail_open.store(1, Ordering::SeqCst);
    send(
        &host,
        client.id(),
        &root,
        ProviderKind::Codex,
        "a2",
        "Continue",
    )
    .await
    .unwrap();
    assert_eq!(a.next.load(Ordering::SeqCst), 2);
    {
        let injections = a.injected.lock().unwrap();
        assert_ne!(injections[0].0, root.id);
        assert_eq!(
            injections[0]
                .1
                .messages
                .iter()
                .map(|message| message.text.as_str())
                .collect::<Vec<_>>(),
            ["Original constraint", "answer a1", "Review", "answer b1"]
        );
    }
    let visible = history(&host, client.id(), &root).await;
    let ids = visible
        .thread
        .turns
        .as_ref()
        .unwrap()
        .iter()
        .map(|turn| &turn.id)
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(
        ids.len(),
        3,
        "native turn-0 occurs in three sessions and must remain distinct"
    );
}

#[tokio::test]
async fn claude_history_beyond_one_hundred_turns_opens_and_pages_in_order() {
    let directory = tempfile::tempdir().unwrap();
    let native_home = directory.path().join("native");
    let project = native_home.join("projects/workspace");
    std::fs::create_dir_all(&project).unwrap();
    let root = SessionRef {
        provider: ProviderKind::Claude,
        id: uuid::Uuid::new_v4().to_string(),
    };
    let mut rows = Vec::new();
    for index in 0usize..201 {
        rows.push(
            serde_json::json!({
                "type": "user", "sessionId": root.id,
                "cwd": directory.path(), "uuid": format!("user-{index}"),
                "parentUuid": index.checked_sub(1).map(|previous| format!("answer-{previous}")),
                "message": {"content": format!("request {index}")},
            })
            .to_string(),
        );
        rows.push(
            serde_json::json!({
                "type": "assistant", "sessionId": root.id,
                "uuid": format!("answer-{index}"), "parentUuid": format!("user-{index}"),
                "message": {"id": format!("reply-{index}"), "stop_reason": "end_turn",
                    "content": [{"type": "text", "text": format!("response {index}")}]},
            })
            .to_string(),
        );
    }
    let transcript = project.join(format!("{}.jsonl", root.id));
    std::fs::write(&transcript, rows.join("\n") + "\n").unwrap();
    let claude = crate::providers::Claude::load(
        directory.path().join("unused-cli"),
        directory.path().join("adapter"),
        Some(native_home),
    )
    .await
    .unwrap();
    let host = HostRpcService::new(
        [Backend {
            provider: ProviderKind::Claude,
            agent: Ok(Arc::new(claude)),
            dictation: None,
        }],
        ProjectStore::new(directory.path().join("worktrees.json")),
    );
    let client = host.open_session();
    let complete = host.native_history(&root, false).await.unwrap();
    assert_eq!(complete.thread.turns.unwrap().len(), 201);
    let opened: agent_protocol::session::OpenedSession = call(
        &host,
        client.id(),
        Call::OpenSession(agent_protocol::session::OpenSession {
            session: root.clone(),
            limit: 5,
            include_activity: true,
        }),
    )
    .await
    .unwrap();
    let mut thread = opened.response.thread;
    assert_eq!(thread.turns.as_ref().unwrap().len(), 5);
    assert_eq!(thread.turns.as_ref().unwrap()[0].id.as_str(), "user-196");
    // Appending a turn must not move the boundary of an already issued cursor.
    rows.push(
        serde_json::json!({
            "type": "user", "sessionId": root.id, "uuid": "user-201",
            "parentUuid": "answer-200", "message": {"content": "new request"},
        })
        .to_string(),
    );
    std::fs::write(&transcript, rows.join("\n") + "\n").unwrap();
    while let Some(cursor) = thread.history_cursor.clone() {
        let page: HistoryPage = call(
            &host,
            client.id(),
            Call::ReadHistory(agent_protocol::session::ReadHistory {
                session: root.clone(),
                cursor,
                include_activity: true,
            }),
        )
        .await
        .unwrap();
        page.prepend_to(&mut thread);
    }
    assert_eq!(
        thread
            .turns
            .unwrap()
            .iter()
            .map(|turn| turn.id.to_string())
            .collect::<Vec<_>>(),
        (0..201)
            .map(|index| format!("user-{index}"))
            .collect::<Vec<_>>()
    );
    for cursor in ["", "invalid", "0", "203"] {
        assert!(
            call::<HistoryPage>(
                &host,
                client.id(),
                Call::ReadHistory(agent_protocol::session::ReadHistory {
                    session: root.clone(),
                    cursor: cursor.into(),
                    include_activity: true,
                }),
            )
            .await
            .is_err()
        );
    }
}

#[tokio::test]
async fn long_history_is_transferred_beyond_the_visible_page_and_older_pages_keep_order() {
    let directory = tempfile::tempdir().unwrap();
    let a = NativeAgent::new(ProviderKind::Codex, directory.path().join("a"));
    let b = NativeAgent::new(ProviderKind::Claude, directory.path().join("b"));
    let root = a
        .create(directory.path().to_str().unwrap(), None, None)
        .await
        .unwrap()
        .thread
        .id
        .unwrap();
    let mut turns = Vec::new();
    for index in 0..150 {
        turns.push(Arc::new(Turn {
            id: format!("old-{index}").into(),
            status: TurnStatus::Completed,
            items: Some(vec![Arc::new(Item::new(
                format!("user-{index}").into(),
                ItemStatus::Completed,
                ItemBody::UserMessage {
                    text: Some(if index == 0 {
                        "Original requirement outside the first native page".into()
                    } else {
                        format!("request {index}")
                    }),
                    content: vec![],
                },
            ))]),
            ..Default::default()
        }));
    }
    a.records
        .lock()
        .unwrap()
        .get_mut(&root.id)
        .unwrap()
        .thread
        .turns = Some(turns);
    let host = service(directory.path(), &a, &b);
    let client = host.open_session();
    send(
        &host,
        client.id(),
        &root,
        ProviderKind::Claude,
        "b1",
        "Continue",
    )
    .await
    .unwrap();
    {
        let inputs = b.inputs.lock().unwrap();
        let op::Input::Text { text } = &inputs[0].input[0] else {
            panic!()
        };
        assert!(text.contains("Original requirement outside the first native page"));
        assert!(text.contains("request 149"));
    }
    let opened: agent_protocol::session::OpenedSession = call(
        &host,
        client.id(),
        Call::OpenSession(agent_protocol::session::OpenSession {
            session: root.clone(),
            limit: 5,
            include_activity: true,
        }),
    )
    .await
    .unwrap();
    assert_eq!(opened.response.thread.turns.as_ref().unwrap().len(), 5);
    let mut count = 5;
    let mut cursor = opened.response.thread.history_cursor;
    while let Some(value) = cursor {
        let page: HistoryPage = call(
            &host,
            client.id(),
            Call::ReadHistory(agent_protocol::session::ReadHistory {
                session: root.clone(),
                cursor: value,
                include_activity: true,
            }),
        )
        .await
        .unwrap();
        count += page.turns.len();
        cursor = page.next_cursor;
    }
    assert_eq!(count, 151);
    let page: handoff::Page = call(
        &host,
        client.id(),
        Call::ReadHandoffHistory(handoff::ReadHistory {
            session: root,
            offset: 0,
            limit: 1,
            text_offset: 0,
        }),
    )
    .await
    .unwrap();
    assert_eq!(
        page.items[0].message.text,
        "Original requirement outside the first native page"
    );
    assert_eq!(page.next_offset, Some(1));
}
