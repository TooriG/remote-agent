//! DeepSeek Harness owns inference, tools, credentials and native sessions.
//! BEX speaks its standard ACP v1 stdio interface and checkpoints display state.
use crate::host_rpc::{
    agent::{
        AccountCommand, AccountReply, Agent, AgentChange, AgentEvent, Identity, SessionListScope,
        SessionPage, SessionSummary, SubmissionState, emit,
    },
    requests::{AnswerSource, RequestOrigin},
    service::Failure,
    submission::{RunningInput, SubmissionTarget},
};
use agent_protocol::{
    composer::ComposerCatalog,
    ids::TurnId,
    models::*,
    operations as op,
    permissions::{PermissionMode, PermissionSettings},
    requests::{Answer, Choice, Request, RequestBody, RequestTarget},
    session::{Capabilities, ProviderKind, RequestDelivery, SessionChange, SessionRef},
};
use agent_transport::peer::{JsonlReader, JsonlWriter};
use futures_util::future::BoxFuture;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    process::ChildStdin,
    sync::{Mutex, mpsc, oneshot},
};
use tokio_util::sync::CancellationToken;

const PROVIDER: ProviderKind = ProviderKind::DeepSeek;
const ACCOUNT: &str = "deepseek:native";
const DEFAULT_MODEL: &str = "[\"deepseek-official\",\"deepseek-flash\"]";
type Pending = HashMap<u64, oneshot::Sender<Result<Value, String>>>;

#[derive(Clone)]
pub struct DeepSeek(Arc<Inner>);
struct Inner {
    program: PathBuf,
    directory: PathBuf,
    records: Mutex<BTreeMap<String, ThreadResponse>>,
    rpc: Mutex<Option<Arc<Rpc>>>,
    events: mpsc::Sender<AgentEvent>,
    receiver: std::sync::Mutex<Option<mpsc::Receiver<AgentEvent>>>,
    stop: CancellationToken,
    workers: Mutex<tokio::task::JoinSet<()>>,
    login: Mutex<Option<String>>,
}
struct Rpc {
    input: Arc<Mutex<JsonlWriter<ChildStdin>>>,
    pending: Mutex<Pending>,
    next: AtomicU64,
    loaded: Mutex<HashSet<String>>,
    instance: uuid::Uuid,
    stop: CancellationToken,
    stopped: CancellationToken,
}
impl Drop for Inner {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}
impl From<DeepSeek> for super::Backend {
    fn from(agent: DeepSeek) -> Self {
        Self {
            provider: PROVIDER,
            agent: Ok(Arc::new(agent)),
            dictation: None,
        }
    }
}
fn failure(error: impl std::fmt::Display) -> Failure {
    Failure::new("deepseek_failed", error)
}
fn unsupported() -> Failure {
    Failure::new(
        "unsupported_operation",
        "DeepSeek Harness does not expose this operation over ACP",
    )
}
fn session(id: &str) -> SessionRef {
    SessionRef {
        provider: PROVIDER,
        id: id.into(),
    }
}
fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

impl DeepSeek {
    pub async fn load(program: PathBuf, directory: PathBuf) -> anyhow::Result<Self> {
        crate::platform::create_state_directory(&directory)?;
        let mut records: BTreeMap<String, ThreadResponse> =
            match tokio::fs::read(directory.join("conversations.json")).await {
                Ok(bytes) => serde_json::from_slice(&bytes)?,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
                Err(e) => return Err(e.into()),
            };
        // A new Host never claims that a dead ACP process is still running.
        for record in records.values_mut() {
            record.thread.status = SessionStatus::Idle;
            for turn in record.thread.turns.iter_mut().flatten() {
                if turn.status == TurnStatus::Running {
                    Arc::make_mut(turn).status = TurnStatus::Interrupted;
                }
            }
        }
        let (events, receiver) = mpsc::channel(256);
        Ok(Self(Arc::new(Inner {
            program,
            directory,
            records: Mutex::new(records),
            rpc: Mutex::new(None),
            events,
            receiver: std::sync::Mutex::new(Some(receiver)),
            stop: CancellationToken::new(),
            workers: Mutex::new(tokio::task::JoinSet::new()),
            login: Mutex::new(None),
        })))
    }
    fn command(&self) -> Result<tokio::process::Command, Failure> {
        let program = &self.0.program;
        if program.extension().is_some_and(|e| e == "js" || e == "mjs") {
            let runtime = std::env::var_os("BEX_DEEPSEEK_NODE").unwrap_or_else(|| "node".into());
            let mut command = bex_process::command(Path::new(&runtime)).map_err(failure)?;
            command.arg(program);
            return Ok(command);
        }
        #[cfg(windows)]
        {
            let managed = directories::BaseDirs::new().map(|base| {
                base.data_local_dir()
                    .join("BEX/deepseek-harness/node_modules/@deepseek-ai/dsh/lib/bin.js")
            });
            let path = std::env::var_os("PATH").unwrap_or_default();
            let script = managed
                .filter(|p| p.is_file())
                .into_iter()
                .chain(
                    std::env::split_paths(&path)
                        .map(|p| p.join("node_modules/@deepseek-ai/dsh/lib/bin.js")),
                )
                .find(|p| p.is_file());
            if program == Path::new("dsh")
                && let Some(script) = script
            {
                let runtime = directories::BaseDirs::new()
                    .map(|base| {
                        base.data_local_dir()
                            .join("BEX/deepseek-harness/runtime/node.exe")
                    })
                    .filter(|p| p.is_file());
                let runtime = std::env::var_os("BEX_DEEPSEEK_NODE")
                    .or_else(|| runtime.map(|p| p.into_os_string()))
                    .unwrap_or_else(|| "node".into());
                let mut command = bex_process::command(Path::new(&runtime)).map_err(failure)?;
                command.arg(script);
                return Ok(command);
            }
        }
        bex_process::command(program).map_err(failure)
    }
    async fn rpc(&self) -> Result<Arc<Rpc>, Failure> {
        let mut slot = self.0.rpc.lock().await;
        if let Some(rpc) = slot.as_ref().filter(|rpc| !rpc.stop.is_cancelled()) {
            return Ok(rpc.clone());
        }
        let mut command = self.command()?;
        let patch = self.0.directory.join("acp.patch.json");
        crate::platform::save_private_json(&patch,&json!([{"id":"acp","config":{"provider":"deepseek-official","model":"deepseek-flash"}}])).map_err(failure)?;
        command
            .args(["--profile", "acp", "--patch"])
            .arg(patch)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some(key) = self.api_key().await? {
            command.env("DEEPSEEK_API_KEY", &*key);
        }
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        let mut child = command.spawn().map_err(|e| {
            Failure::new(
                "provider_unavailable",
                format!(
                    "DeepSeek Harness を起動できません。npm install -g @deepseek-ai/dsh@next: {e}"
                ),
            )
        })?;
        let input = child
            .stdin
            .take()
            .ok_or_else(|| failure("Harness stdin unavailable"))?;
        let mut output = JsonlReader::new(
            child
                .stdout
                .take()
                .ok_or_else(|| failure("Harness stdout unavailable"))?,
        );
        let rpc = Arc::new(Rpc {
            input: Arc::new(Mutex::new(JsonlWriter::new(input))),
            pending: Mutex::new(HashMap::new()),
            next: AtomicU64::new(1),
            loaded: Mutex::new(HashSet::new()),
            instance: uuid::Uuid::new_v4(),
            stop: self.0.stop.child_token(),
            stopped: CancellationToken::new(),
        });
        let weak = Arc::downgrade(&self.0);
        let reader = rpc.clone();
        tokio::spawn(async move {
            loop {
                let line = tokio::select! { _ = reader.stop.cancelled() => break, line = output.read_line() => line };
                let Ok(Some(line)) = line else {
                    break;
                };
                let Ok(value) = serde_json::from_str::<Value>(&line) else {
                    break;
                };
                if let Some(method) = value["method"].as_str() {
                    if let Some(inner) = weak.upgrade() {
                        let adapter = DeepSeek(inner);
                        if method == "session/update" {
                            if adapter.update(&value["params"]).await.is_err() {
                                break;
                            }
                        } else if value.get("id").is_some() {
                            if method == "session/request_permission" {
                                if adapter.permission(&reader, &value).await.is_err() {
                                    break;
                                }
                            } else {
                                let _ = reader.write(json!({"jsonrpc":"2.0","id":value["id"],"error":{"code":-32601,"message":"Unsupported client method"}})).await;
                            }
                        }
                    } else {
                        break;
                    }
                } else if let Some(id) = value["id"].as_u64() {
                    if let Some(reply) = reader.pending.lock().await.remove(&id) {
                        let result = if value.get("error").is_some() {
                            Err(value["error"]["message"]
                                .as_str()
                                .unwrap_or("Harness request failed")
                                .into())
                        } else {
                            Ok(value["result"].clone())
                        };
                        let _ = reply.send(result);
                    }
                }
            }
            reader.stop.cancel();
            let _ = reader.input.lock().await.shutdown().await;
            if tokio::time::timeout(Duration::from_secs(10), child.wait())
                .await
                .is_err()
            {
                let _ = child.kill().await;
            }
            reader.stopped.cancel();
            for (_, reply) in reader.pending.lock().await.drain() {
                let _ = reply.send(Err("DeepSeek Harness disconnected".into()));
            }
            if let Some(inner) = weak.upgrade() {
                let _ = emit(&inner.events, AgentChange::SourceClosed(reader.instance)).await;
            }
        });
        let initialized = match rpc.call("initialize", json!({"protocolVersion":1,"clientCapabilities":{},"clientInfo":{"name":"bex","version":env!("CARGO_PKG_VERSION")}}), Some(Duration::from_secs(90))).await {Ok(value)=>value,Err(error)=>{rpc.stop.cancel();return Err(error);}};
        if initialized["protocolVersion"] != 1 {
            rpc.stop.cancel();
            return Err(failure("Harness requires ACP v1"));
        }
        *slot = Some(rpc.clone());
        Ok(rpc)
    }
    async fn api_key(&self) -> Result<Option<zeroize::Zeroizing<String>>, Failure> {
        match tokio::fs::read(self.0.directory.join("api-key.json")).await {
            Ok(bytes) => {
                let key: String = serde_json::from_slice(&bytes)
                    .map_err(|_| failure("DeepSeek API key settings could not be read"))?;
                Ok(Some(zeroize::Zeroizing::new(key)))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(failure(e)),
        }
    }
    async fn configured(&self) -> Result<bool, Failure> {
        if self.api_key().await?.is_some()
            || std::env::var("DEEPSEEK_API_KEY").is_ok_and(|v| !v.trim().is_empty())
        {
            return Ok(true);
        }
        let home = std::env::var_os("DSH_HOME")
            .map(PathBuf::from)
            .or_else(|| directories::BaseDirs::new().map(|b| b.home_dir().join(".dsh")));
        Ok(if let Some(home) = home {
            tokio::fs::read_to_string(home.join(".credentials.yaml"))
                .await
                .is_ok_and(|text| {
                    text.lines()
                        .any(|line| line.trim_start().starts_with("DEEPSEEK_API_KEY:"))
                })
        } else {
            false
        })
    }
    async fn reset_rpc(&self) -> Result<(), Failure> {
        if self
            .0
            .records
            .lock()
            .await
            .values()
            .any(|r| r.thread.status == SessionStatus::Running)
        {
            return Err(failure(
                "DeepSeek のタスクが完了してから認証設定を変更してください。",
            ));
        }
        if let Some(rpc) = self.0.rpc.lock().await.take() {
            let loaded: Vec<_> = rpc.loaded.lock().await.iter().cloned().collect();
            for id in loaded {
                rpc.call(
                    "session/close",
                    json!({"sessionId":id}),
                    Some(Duration::from_secs(30)),
                )
                .await?;
            }
            rpc.stop.cancel();
            let _ = tokio::time::timeout(Duration::from_secs(15), rpc.stopped.cancelled()).await;
        }
        Ok(())
    }
    async fn save(&self, records: &BTreeMap<String, ThreadResponse>) -> Result<(), Failure> {
        crate::platform::save_private_json(&self.0.directory.join("conversations.json"), records)
            .map_err(failure)
    }
    async fn change(&self, id: &str, change: SessionChange) -> Result<(), Failure> {
        let mut records = self.0.records.lock().await;
        let record = records
            .get_mut(id)
            .ok_or_else(|| failure("Unknown DeepSeek conversation"))?;
        record.thread = change.apply(&record.thread).map_err(failure)?;
        record.thread.updated_at = Some(now());
        self.save(&records).await?;
        drop(records);
        emit(
            &self.0.events,
            AgentChange::Session {
                session: session(id),
                change,
            },
        )
        .await
        .map_err(failure)
    }
    async fn ensure_loaded(&self, rpc: &Rpc, record: &ThreadResponse) -> Result<(), Failure> {
        let id = &record
            .thread
            .id
            .as_ref()
            .ok_or_else(|| failure("Missing session ID"))?
            .id;
        let mut loaded = rpc.loaded.lock().await;
        if !loaded.contains(id) {
            rpc.call(
                "session/resume",
                json!({"sessionId":id,"cwd":record.thread.cwd,"mcpServers":[]}),
                Some(Duration::from_secs(90)),
            )
            .await?;
            loaded.insert(id.clone());
        }
        Ok(())
    }
    async fn select(
        &self,
        rpc: &Rpc,
        id: &str,
        model: Option<&str>,
        effort: Option<&str>,
    ) -> Result<(), Failure> {
        if let Some(model) = model {
            rpc.call(
                "session/set_config_option",
                json!({"sessionId":id,"configId":"model","value":model}),
                Some(Duration::from_secs(30)),
            )
            .await?;
        }
        if let Some(effort) = effort.filter(|e| !e.is_empty()) {
            rpc.call(
                "session/set_config_option",
                json!({"sessionId":id,"configId":"reasoning_effort","value":effort}),
                Some(Duration::from_secs(30)),
            )
            .await?;
        }
        Ok(())
    }
    async fn update(&self, params: &Value) -> Result<(), Failure> {
        let Some(id) = params["sessionId"].as_str() else {
            return Ok(());
        };
        let update = &params["update"];
        let records = self.0.records.lock().await;
        let Some(record) = records.get(id) else {
            return Ok(());
        };
        let Some(turn) = record
            .thread
            .turns
            .as_ref()
            .and_then(|t| t.last())
            .filter(|t| t.status == TurnStatus::Running)
        else {
            return Ok(());
        };
        let turn_id = turn.id.clone();
        let item = translate_update(update, turn);
        drop(records);
        if let Some(item) = item {
            self.change(
                id,
                SessionChange::Item {
                    turn_id,
                    item: Arc::new(item),
                },
            )
            .await?;
        }
        Ok(())
    }
    async fn permission(&self, rpc: &Arc<Rpc>, value: &Value) -> Result<(), Failure> {
        let params = &value["params"];
        let id = params["sessionId"]
            .as_str()
            .ok_or_else(|| failure("Missing permission session"))?;
        let choices = params["options"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|option| {
                Some(Choice {
                    id: option["optionId"].as_str()?.into(),
                    label: option["name"].as_str().unwrap_or("Permission").into(),
                    description: option["kind"].as_str().unwrap_or_default().into(),
                })
            })
            .collect();
        let request = Request {
            id: format!("deepseek:{}:{}", rpc.instance, value["id"]).into(),
            target: RequestTarget::Session,
            delivery: RequestDelivery::Awaiting,
            body: RequestBody::Permission {
                description: params["toolCall"]["title"]
                    .as_str()
                    .unwrap_or("DeepSeek Harness permission")
                    .into(),
                details: serde_json::to_string_pretty(&params["toolCall"]).unwrap_or_default(),
                choices,
            },
        };
        emit(
            &self.0.events,
            AgentChange::Request {
                session: session(id),
                origin: RequestOrigin {
                    instance: rpc.instance,
                    native_id: value["id"].clone(),
                    provider: PROVIDER,
                    source: rpc.clone(),
                },
                request,
            },
        )
        .await
        .map_err(failure)
    }
}

impl Rpc {
    async fn write(&self, value: Value) -> Result<(), Failure> {
        self.input
            .lock()
            .await
            .write_line(&value.to_string())
            .await
            .map_err(failure)
    }
    async fn call(
        &self,
        method: &str,
        params: Value,
        timeout: Option<Duration>,
    ) -> Result<Value, Failure> {
        if self.stop.is_cancelled() {
            return Err(failure("Harness disconnected"));
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (send, receive) = oneshot::channel();
        self.pending.lock().await.insert(id, send);
        if let Err(e) = self
            .write(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await
        {
            self.pending.lock().await.remove(&id);
            return Err(e);
        }
        let result = if let Some(timeout) = timeout {
            match tokio::time::timeout(timeout, receive).await {
                Ok(result) => result.map_err(failure)?,
                Err(_) => {
                    self.pending.lock().await.remove(&id);
                    return Err(Failure::unknown(
                        "deepseek_timeout",
                        "Harness did not acknowledge the request",
                    ));
                }
            }
        } else {
            receive.await.map_err(failure)?
        };
        result.map_err(failure)
    }
}
#[async_trait::async_trait]
impl AnswerSource for Rpc {
    fn is_alive(&self) -> bool {
        !self.stop.is_cancelled()
    }
    async fn prepare(
        &self,
        native_id: &Value,
        body: &RequestBody,
        answer: &Answer,
    ) -> Result<BoxFuture<'static, Result<(), Failure>>, Failure> {
        agent_protocol::requests::validate_answer(body, answer).map_err(failure)?;
        let Answer::Permission { choice_id } = answer else {
            return Err(unsupported());
        };
        let response = json!({"jsonrpc":"2.0","id":native_id,"result":{"outcome":{"outcome":"selected","optionId":choice_id}}});
        let input = self.input.clone();
        let stop = self.stop.clone();
        Ok(Box::pin(async move {
            if stop.is_cancelled() {
                return Err(failure("Harness disconnected"));
            }
            input
                .lock()
                .await
                .write_line(&response.to_string())
                .await
                .map_err(|e| Failure::unknown("answer_delivery_unknown", e))
        }))
    }
}

fn translate_update(update: &Value, turn: &Turn) -> Option<Item> {
    let kind = update["sessionUpdate"].as_str()?;
    let items = turn.items.as_deref().unwrap_or_default();
    match kind {
        "agent_message_chunk" | "agent_thought_chunk" => {
            let text = update["content"]["text"].as_str()?;
            let reasoning = kind == "agent_thought_chunk";
            let previous = items.last().filter(|item| {
                if reasoning {
                    matches!(item.body(), ItemBody::Reasoning { .. })
                } else {
                    matches!(item.body(), ItemBody::AssistantText { .. })
                }
            });
            let id = previous
                .map(|i| i.id.clone())
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string().into());
            let body = if reasoning {
                let mut content = previous
                    .and_then(|i| {
                        if let ItemBody::Reasoning { content, .. } = i.body() {
                            Some(content.clone())
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| vec![String::new()]);
                content.last_mut()?.push_str(text);
                ItemBody::Reasoning {
                    content,
                    summary: vec![],
                }
            } else {
                let mut content = previous
                    .and_then(|i| {
                        if let ItemBody::AssistantText { text, .. } = i.body() {
                            Some(text.clone())
                        } else {
                            None
                        }
                    })
                    .unwrap_or_default();
                content.push_str(text);
                ItemBody::AssistantText {
                    text: content,
                    phase: AssistantPhase::Unknown,
                }
            };
            Some(Item::new(id, ItemStatus::Completed, body))
        }
        "tool_call" | "tool_call_update" => {
            let id = update["toolCallId"].as_str()?;
            let previous = items.iter().find(|item| item.id.as_str() == id);
            let status = match update["status"].as_str() {
                Some("completed") => ItemStatus::Completed,
                Some("failed") => ItemStatus::Failed,
                Some(_) => ItemStatus::Running,
                None => previous.map(|i| i.status).unwrap_or(ItemStatus::Running),
            };
            let mut body =
                previous
                    .map(|i| i.body().clone())
                    .unwrap_or_else(|| ItemBody::ToolCall {
                        kind: ToolKind::Local,
                        tool: update["title"].as_str().unwrap_or("Harness tool").into(),
                        server: None,
                        namespace: None,
                        resource_uri: None,
                        plugin_id: None,
                        arguments: update["rawInput"].clone(),
                        result: None,
                        error: None,
                        content: vec![],
                        success: None,
                        duration_ms: None,
                    });
            if let ItemBody::ToolCall {
                result,
                content,
                success,
                arguments,
                ..
            } = &mut body
            {
                if update.get("rawInput").is_some() {
                    *arguments = update["rawInput"].clone();
                }
                if update.get("rawOutput").is_some() {
                    *result = Some(update["rawOutput"].clone());
                }
                if let Some(blocks) = update["content"].as_array() {
                    *content = blocks
                        .iter()
                        .filter_map(|b| {
                            b["content"]["text"].as_str().map(|s| {
                                agent_protocol::requests::ToolContent::Text { text: s.into() }
                            })
                        })
                        .collect();
                }
                *success = match status {
                    ItemStatus::Completed => Some(true),
                    ItemStatus::Failed => Some(false),
                    _ => None,
                };
            }
            Some(Item::new(id.into(), status, body))
        }
        "plan" => Some(Item::new(
            "deepseek-plan".into(),
            ItemStatus::Completed,
            ItemBody::Plan {
                text: update["entries"]
                    .as_array()?
                    .iter()
                    .filter_map(|e| e["content"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
            },
        )),
        _ => None,
    }
}

#[async_trait::async_trait]
impl Identity for DeepSeek {
    async fn list(&self) -> Result<op::Accounts, Failure> {
        let configured = self.configured().await?;
        Ok(op::Accounts {
            accounts: if configured {
                vec![op::Account {
                    id: ACCOUNT.into(),
                    provider: PROVIDER,
                    email: Some("DeepSeek API".into()),
                    plan_type: Some("API".into()),
                    usage: None,
                }]
            } else {
                vec![]
            },
            selected: if configured {
                HashMap::from([(PROVIDER, ACCOUNT.into())])
            } else {
                HashMap::new()
            },
            error: None,
        })
    }
    async fn account(&self, command: AccountCommand) -> Result<AccountReply, Failure> {
        match command {
            AccountCommand::Select { id } if id == ACCOUNT && self.configured().await? => {
                Ok(op::AccountSelection {
                    provider: PROVIDER,
                    selected_id: id,
                    persistence_error: None,
                }
                .into())
            }
            AccountCommand::StartLogin => {
                let id = uuid::Uuid::new_v4().to_string();
                *self.0.login.lock().await = Some(id.clone());
                Ok(op::AccountLogin {
                    provider: PROVIDER,
                    login_id: id,
                    requires_code_submission: true,
                    user_code: String::new(),
                    verification_url: "https://platform.deepseek.com/api_keys".into(),
                }
                .into())
            }
            AccountCommand::SubmitLogin { id, code } => {
                if self.0.login.lock().await.as_ref() != Some(&id) {
                    return Err(failure("API key setup has expired"));
                }
                let code = zeroize::Zeroizing::new(code);
                let key = zeroize::Zeroizing::new(code.trim().to_owned());
                if key.is_empty() || key.len() > 4096 || key.chars().any(char::is_whitespace) {
                    return Err(failure("有効なDeepSeek APIキーを入力してください。"));
                }
                self.reset_rpc().await?;
                crate::platform::save_private_json(&self.0.directory.join("api-key.json"), &*key)
                    .map_err(failure)?;
                *self.0.login.lock().await = None;
                Ok(AccountReply::Complete)
            }
            AccountCommand::ReadLogin { id } => Ok(op::AccountLoginStatus {
                completed: self.0.login.lock().await.as_ref() != Some(&id)
                    && self.configured().await?,
                account_id: self.configured().await?.then(|| ACCOUNT.into()),
            }
            .into()),
            AccountCommand::CancelLogin { id } => {
                let mut login = self.0.login.lock().await;
                if login.as_ref() == Some(&id) {
                    *login = None;
                }
                Ok(AccountReply::Complete)
            }
            AccountCommand::Logout { id } if id == ACCOUNT => {
                self.reset_rpc().await?;
                match tokio::fs::remove_file(self.0.directory.join("api-key.json")).await {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        return Err(failure(
                            "Harnessや環境変数に設定したAPIキーは、設定元で削除してください。",
                        ));
                    }
                    Err(e) => return Err(failure(e)),
                };
                Ok(AccountReply::Complete)
            }
            _ => Err(failure("Unknown DeepSeek account")),
        }
    }
    async fn usage(&self, _: &str) -> Result<op::AccountUsage, Failure> {
        Err(unsupported())
    }
}

#[async_trait::async_trait]
impl Agent for DeepSeek {
    fn data_recipient(&self) -> &'static str {
        "DeepSeek"
    }
    fn running_input(&self) -> RunningInput {
        RunningInput::Queue
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            additional_input: false,
            fork: false,
            rename: true,
            model_change: true,
        }
    }
    fn availability(&self) -> Result<(), Failure> {
        self.validate_create()
    }
    fn validate_create(&self) -> Result<(), Failure> {
        if self.0.stop.is_cancelled() {
            Err(failure("Harness is shutting down"))
        } else {
            Ok(())
        }
    }
    fn storage_directory(&self) -> &Path {
        &self.0.directory
    }
    async fn list(
        &self,
        search: &str,
        cursor: Option<String>,
        scope: SessionListScope<'_>,
        limit: u32,
    ) -> Result<SessionPage, Failure> {
        let records = self.0.records.lock().await;
        let mut data: Vec<_> = records
            .iter()
            .filter(|(id, r)| {
                !matches!(scope, SessionListScope::Descendants(_))
                    && cursor.as_ref().is_none_or(|c| *id > c)
                    && format!(
                        "{} {}",
                        r.thread.name.as_deref().unwrap_or_default(),
                        r.thread.preview.as_deref().unwrap_or_default()
                    )
                    .to_lowercase()
                    .contains(&search.to_lowercase())
            })
            .take(limit.max(1) as usize + 1)
            .map(|(_, r)| {
                let mut thread = r.thread.clone();
                thread.turns = None;
                SessionSummary {
                    thread,
                    branch: None,
                }
            })
            .collect();
        let next_cursor = if data.len() > limit.max(1) as usize {
            data.pop();
            data.last()
                .and_then(|r| r.thread.id.as_ref())
                .map(|id| id.id.clone())
        } else {
            None
        };
        Ok(SessionPage { data, next_cursor })
    }
    async fn open(&self, id: &str, _: usize, _: bool) -> Result<ThreadResponse, Failure> {
        self.0
            .records
            .lock()
            .await
            .get(id)
            .cloned()
            .ok_or_else(|| failure("Unknown DeepSeek conversation"))
    }
    async fn read_history(
        &self,
        _: &str,
        _: &str,
        _: bool,
    ) -> Result<agent_protocol::session::HistoryPage, Failure> {
        Err(unsupported())
    }
    async fn read_item(&self, params: &op::ReadItem) -> Result<op::ItemResponse, Failure> {
        let record = self.open(&params.thread_id.id, usize::MAX, true).await?;
        let item = record
            .thread
            .turns
            .iter()
            .flatten()
            .filter(|t| t.id == params.turn_id)
            .flat_map(|t| t.items.iter().flatten())
            .find(|i| i.id == params.item_id)
            .ok_or_else(|| failure("Item not found"))?;
        Ok(op::ItemResponse {
            item: (**item).clone(),
            transfer: None,
        })
    }
    async fn read_turn_items(&self, id: &str, turn_id: &TurnId) -> Result<Vec<Arc<Item>>, Failure> {
        let record = self.open(id, usize::MAX, true).await?;
        record
            .thread
            .turns
            .iter()
            .flatten()
            .find(|t| &t.id == turn_id)
            .and_then(|t| t.items.clone())
            .ok_or_else(|| failure("Turn not found"))
    }
    async fn create(
        &self,
        cwd: &str,
        model: Option<&str>,
        _: Option<Value>,
    ) -> Result<ThreadResponse, Failure> {
        let rpc = self.rpc().await?;
        let created = rpc
            .call(
                "session/new",
                json!({"cwd":cwd,"mcpServers":[]}),
                Some(Duration::from_secs(90)),
            )
            .await?;
        let id = created["sessionId"]
            .as_str()
            .ok_or_else(|| failure("Harness did not return a session ID"))?;
        rpc.loaded.lock().await.insert(id.into());
        if let Err(error) = self.select(&rpc, id, model, None).await {
            let _ = rpc
                .call(
                    "session/close",
                    json!({"sessionId":id}),
                    Some(Duration::from_secs(15)),
                )
                .await;
            return Err(error);
        }
        let record = ThreadResponse {
            thread: Thread {
                id: Some(session(id)),
                cwd: Some(cwd.into()),
                status: SessionStatus::Idle,
                capabilities: Some(self.capabilities()),
                turns: Some(vec![]),
                updated_at: Some(now()),
                ..Default::default()
            },
            model: Some(ModelRef {
                provider: PROVIDER,
                id: model
                    .or_else(|| {
                        created["configOptions"]
                            .as_array()
                            .and_then(|v| v.iter().find(|o| o["id"] == "model"))
                            .and_then(|o| o["currentValue"].as_str())
                    })
                    .unwrap_or(DEFAULT_MODEL)
                    .into(),
            }),
        };
        let mut records = self.0.records.lock().await;
        records.insert(id.into(), record.clone());
        self.save(&records).await?;
        Ok(record)
    }
    async fn state(&self, id: &str) -> Result<SubmissionState, Failure> {
        Ok(SubmissionState {
            response: self.open(id, usize::MAX, true).await?,
            needs_reload: false,
        })
    }
    async fn submit(
        &self,
        input: &op::Submission,
        route: SubmissionTarget<'_>,
        _: bool,
        _: Option<Value>,
    ) -> Result<op::SubmissionReceipt, Failure> {
        if !matches!(route, SubmissionTarget::Start { .. }) {
            return Err(Failure::new(
                "unsupported_operation",
                "Harness accepts input after the running task finishes",
            ));
        }
        let id = input.thread_id.id.clone();
        let record = self.open(&id, usize::MAX, true).await?;
        let rpc = self.rpc().await?;
        self.ensure_loaded(&rpc, &record).await?;
        self.select(
            &rpc,
            &id,
            input.model.as_ref().map(|m| m.id.as_str()),
            input.effort.as_deref(),
        )
        .await?;
        let prompt = prompt(&input.input).await?;
        {
            let mut records = self.0.records.lock().await;
            if let Some(record) = records.get_mut(&id) {
                if let Some(model) = &input.model {
                    record.model = Some(model.clone());
                }
                if record.thread.preview.is_none() {
                    record.thread.preview = input.input.iter().find_map(|part| {
                        if let op::Input::Text { text } = part {
                            Some(text.chars().take(200).collect())
                        } else {
                            None
                        }
                    });
                }
            }
            self.save(&records).await?;
        }
        let turn_id: TurnId = uuid::Uuid::new_v4().to_string().into();
        let mut user = Item::new(
            input.client_user_message_id.as_str().into(),
            ItemStatus::Completed,
            ItemBody::UserMessage {
                text: None,
                content: op::Input::message_parts(&input.input),
            },
        );
        user.client_input_id = Some(input.client_user_message_id.clone());
        self.change(
            &id,
            SessionChange::Turn {
                turn: Turn {
                    id: turn_id.clone(),
                    status: TurnStatus::Running,
                    items: Some(vec![Arc::new(user)]),
                    started_at: Some(now()),
                    ..Default::default()
                },
                completed: false,
            },
        )
        .await?;
        self.change(
            &id,
            SessionChange::Status {
                status: SessionStatus::Running,
            },
        )
        .await?;
        let adapter = self.clone();
        let running_turn = turn_id.clone();
        self.0.workers.lock().await.spawn(async move {
            let result = rpc
                .call(
                    "session/prompt",
                    json!({"sessionId":id,"prompt":prompt}),
                    None,
                )
                .await;
            let status = match &result {
                Ok(value) if value["stopReason"] == "cancelled" => TurnStatus::Interrupted,
                Ok(_) => TurnStatus::Completed,
                Err(_) => TurnStatus::Failed,
            };
            let record = adapter.open(&id, usize::MAX, true).await;
            if let Ok(record) = record
                && let Some(turn) = record
                    .thread
                    .turns
                    .iter()
                    .flatten()
                    .find(|t| t.id == running_turn)
            {
                let mut turn = (**turn).clone();
                turn.status = status;
                if let Err(error) = result {
                    turn.error = Some(ExecutionError {
                        message: error.message,
                        ..Default::default()
                    });
                }
                let _ = adapter
                    .change(
                        &id,
                        SessionChange::Turn {
                            turn,
                            completed: true,
                        },
                    )
                    .await;
            }
            let _ = adapter
                .change(
                    &id,
                    SessionChange::Status {
                        status: SessionStatus::Idle,
                    },
                )
                .await;
        });
        Ok(op::SubmissionReceipt {
            turn_id: Some(turn_id),
        })
    }
    async fn interrupt(&self, id: &str, turn: &TurnId) -> Result<Empty, Failure> {
        let record = self.open(id, usize::MAX, true).await?;
        if record.thread.active_turn_id().as_ref() != Some(turn) {
            return Err(failure("Turn is no longer running"));
        }
        self.rpc()
            .await?
            .write(json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":id}}))
            .await?;
        Ok(Empty {})
    }
    async fn models(&self, params: &op::ListModels) -> Result<op::ModelPage, Failure> {
        let rpc = self.rpc().await?;
        let data = if params.cursor.is_some() {
            vec![]
        } else {
            let created = rpc
                .call(
                    "session/new",
                    json!({"cwd":self.0.directory,"mcpServers":[]}),
                    Some(Duration::from_secs(90)),
                )
                .await?;
            let models = catalog(&created);
            if let Some(id) = created["sessionId"].as_str() {
                rpc.call(
                    "session/close",
                    json!({"sessionId":id}),
                    Some(Duration::from_secs(30)),
                )
                .await?;
            }
            models
        };
        Ok(op::ModelPage {
            data,
            next_cursor: None,
            provider_errors: None,
        })
    }
    async fn catalog(&self, cwd: &str) -> ComposerCatalog {
        ComposerCatalog {
            cwd: cwd.into(),
            ..Default::default()
        }
    }
    async fn discard_workspace_processes(&self, dir: &Path) -> Result<(), Failure> {
        let rpc = self.rpc().await?;
        let ids: Vec<_> = self
            .0
            .records
            .lock()
            .await
            .iter()
            .filter(|(_, r)| {
                r.thread
                    .cwd
                    .as_deref()
                    .is_some_and(|p| Path::new(p).starts_with(dir))
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            if rpc.loaded.lock().await.remove(&id) {
                rpc.call(
                    "session/close",
                    json!({"sessionId":id}),
                    Some(Duration::from_secs(30)),
                )
                .await?;
            }
        }
        Ok(())
    }
    async fn read_permissions(&self) -> Result<PermissionSettings, Failure> {
        Ok(PermissionSettings {
            mode: None,
            version: "deepseek-native".into(),
        })
    }
    async fn update_permissions(
        &self,
        _: PermissionMode,
        _: &str,
    ) -> Result<PermissionSettings, Failure> {
        Err(Failure::new(
            "native_permissions",
            "Harness の権限設定を使用します。必要な操作はBEXの承認画面に表示されます。",
        ))
    }
    async fn fork(&self, _: &str, _: &str, _: Option<Value>) -> Result<ThreadResponse, Failure> {
        Err(unsupported())
    }
    async fn rename(&self, id: &str, name: &str) -> Result<Empty, Failure> {
        let mut records = self.0.records.lock().await;
        records
            .get_mut(id)
            .ok_or_else(|| failure("Unknown conversation"))?
            .thread
            .name = Some(name.into());
        self.save(&records).await?;
        drop(records);
        emit(
            &self.0.events,
            AgentChange::Renamed {
                session: session(id),
                name: Some(name.into()),
            },
        )
        .await
        .map_err(failure)?;
        Ok(Empty {})
    }
    fn event_stream(&self) -> Option<mpsc::Receiver<AgentEvent>> {
        self.0.receiver.lock().unwrap().take()
    }
    async fn shutdown(&self) {
        if let Some(rpc) = self.0.rpc.lock().await.take() {
            let loaded: Vec<_> = rpc.loaded.lock().await.iter().cloned().collect();
            futures_util::future::join_all(loaded.into_iter().map(|id| {
                let rpc = rpc.clone();
                async move {
                    let _ = rpc
                        .call(
                            "session/close",
                            json!({"sessionId":id}),
                            Some(Duration::from_secs(30)),
                        )
                        .await;
                }
            }))
            .await;
            rpc.stop.cancel();
            let _ = tokio::time::timeout(Duration::from_secs(15), rpc.stopped.cancelled()).await;
        }
        self.0.stop.cancel();
        let mut workers = self.0.workers.lock().await;
        while workers.join_next().await.is_some() {}
    }
}

async fn prompt(input: &[op::Input]) -> Result<Vec<Value>, Failure> {
    use base64::Engine;
    let mut prompt = vec![];
    for part in input {
        prompt.push(match part {
        op::Input::Text {text}=>json!({"type":"text","text":text}),
        op::Input::Mention {path,name}|op::Input::Skill {path,name}=>json!({"type":"text","text":format!("{name}: {path}")}),
        op::Input::LocalImage {path}=>{let bytes=tokio::fs::read(path).await.map_err(failure)?;let format=image::guess_format(&bytes).map_err(failure)?;json!({"type":"image","mimeType":format.to_mime_type(),"data":base64::engine::general_purpose::STANDARD.encode(bytes)})},
    });
    }
    Ok(prompt)
}
fn catalog(created: &Value) -> Vec<Model> {
    let options = created["configOptions"].as_array();
    let model = options.into_iter().flatten().find(|o| o["id"] == "model");
    let reasoning = options
        .into_iter()
        .flatten()
        .find(|o| o["id"] == "reasoning_effort");
    model
        .into_iter()
        .flat_map(|m| m["options"].as_array().into_iter().flatten())
        .flat_map(|option| {
            option["options"]
                .as_array()
                .map(|v| v.iter().collect::<Vec<_>>())
                .unwrap_or_else(|| vec![option])
        })
        .filter_map(|option| {
            let id = option["value"].as_str()?;
            let current = model.is_some_and(|m| m["currentValue"] == id);
            let name = if id == DEFAULT_MODEL {
                "DeepSeek V4.1 Flash 1017"
            } else {
                option["name"].as_str().unwrap_or(id)
            };
            Some(Model {
                id: format!("deepseek:{id}"),
                model: ModelRef {
                    provider: PROVIDER,
                    id: id.into(),
                },
                display_name: name.into(),
                default_reasoning_effort: if current {
                    reasoning
                        .and_then(|o| o["currentValue"].as_str())
                        .unwrap_or_default()
                        .into()
                } else {
                    String::new()
                },
                supported_reasoning_efforts: if current {
                    reasoning
                        .into_iter()
                        .flat_map(|o| o["options"].as_array().into_iter().flatten())
                        .filter_map(|o| {
                            o["value"].as_str().map(|e| ReasoningEffort {
                                reasoning_effort: e.into(),
                            })
                        })
                        .collect()
                } else {
                    vec![]
                },
                service_tiers: None,
                default_service_tier: None,
                is_default: Some(current),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[tokio::test]
    #[ignore = "requires an installed DeepSeek Harness and its supervisor companion"]
    async fn installed_harness_creates_selects_and_resumes_the_same_native_session() {
        let dir = tempfile::tempdir().unwrap();
        let adapter = DeepSeek::load("dsh".into(), dir.path().into())
            .await
            .unwrap();
        let mut events = adapter.event_stream().unwrap();
        tokio::spawn(async move {
            while let Some(event) = events.recv().await {
                if let Some(applied) = event.applied {
                    let _ = applied.send(Ok(()));
                }
            }
        });
        let models = adapter
            .models(&op::ListModels {
                limit: 100,
                cursor: None,
            })
            .await
            .unwrap();
        assert!(models.data.iter().any(|m| m.model.id == DEFAULT_MODEL));
        let created = adapter
            .create(dir.path().to_str().unwrap(), Some(DEFAULT_MODEL), None)
            .await
            .unwrap();
        let id = created.thread.id.as_ref().unwrap().id.clone();
        adapter.rename(&id, "Harness smoke check").await.unwrap();
        adapter.reset_rpc().await.unwrap();
        let rpc = adapter.rpc().await.unwrap();
        adapter.ensure_loaded(&rpc, &created).await.unwrap();
        assert!(rpc.loaded.lock().await.contains(&id));
        let reopened = adapter.open(&id, 100, true).await.unwrap();
        assert_eq!(reopened.thread.name.as_deref(), Some("Harness smoke check"));
        assert_eq!(reopened.model.unwrap().id, DEFAULT_MODEL);
        rpc.call(
            "session/close",
            json!({"sessionId":id}),
            Some(Duration::from_secs(30)),
        )
        .await
        .unwrap();
        adapter.shutdown().await;
    }

    #[test]
    fn catalog_preserves_opaque_model_routes_and_native_reasoning() {
        let value = json!({"configOptions":[{"id":"model","currentValue":DEFAULT_MODEL,"options":[{"name":"DeepSeek","options":[{"value":DEFAULT_MODEL,"name":"deepseek-flash"},{"value":"other-route","name":"Other"}]}]},{"id":"reasoning_effort","currentValue":"high","options":[{"value":"off"},{"value":"high"},{"value":"max"}]}]});
        let models = catalog(&value);
        assert_eq!(models[0].model.id, DEFAULT_MODEL);
        assert_eq!(models[0].display_name, "DeepSeek V4.1 Flash 1017");
        assert_eq!(models[0].default_reasoning_effort, "high");
        assert_eq!(models[0].supported_reasoning_efforts.len(), 3);
        assert!(models[1].supported_reasoning_efforts.is_empty());
        assert_eq!(models[1].model.id, "other-route");
    }

    proptest! {
        #[test]
        fn assistant_chunks_preserve_unicode_and_interleaved_reasoning(parts in prop::collection::vec(".{0,60}",1..12)) {
            let mut turn=Turn {id:"turn".into(),status:TurnStatus::Running,items:Some(vec![]),..Default::default()};
            for part in &parts {
                let item=translate_update(&json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":part}}),&turn).unwrap();
                turn.items=Some(vec![Arc::new(item)]);
            }
            let item=turn.items.as_ref().unwrap().last().unwrap();
            let ItemBody::AssistantText {text,..}=item.body() else {prop_assert!(false);return Ok(());};
            prop_assert_eq!(text, &parts.concat());
            let thought=translate_update(&json!({"sessionUpdate":"agent_thought_chunk","content":{"text":"think"}}),&turn).unwrap();
            prop_assert_ne!(&thought.id,&item.id);
            prop_assert!(matches!(thought.body(),ItemBody::Reasoning {..}), "thought must remain separate");
        }
    }

    #[test]
    fn partial_tool_updates_keep_the_original_name_arguments_and_result() {
        let mut turn = Turn::default();
        let first=translate_update(&json!({"sessionUpdate":"tool_call","toolCallId":"tool","title":"Read","rawInput":{"path":"a.txt"},"status":"in_progress"}),&turn).unwrap();
        turn.items = Some(vec![Arc::new(first)]);
        let done=translate_update(&json!({"sessionUpdate":"tool_call_update","toolCallId":"tool","status":"completed","rawOutput":"ok","content":[{"type":"content","content":{"type":"text","text":"contents"}}]}),&turn).unwrap();
        assert_eq!(done.status, ItemStatus::Completed);
        let ItemBody::ToolCall {
            tool,
            arguments,
            result,
            success,
            content,
            ..
        } = done.body()
        else {
            panic!("tool call expected")
        };
        assert_eq!(tool, "Read");
        assert_eq!(arguments["path"], "a.txt");
        assert_eq!(result.as_ref().unwrap(), "ok");
        assert_eq!(*success, Some(true));
        assert_eq!(content.len(), 1);
        assert!(
            translate_update(&json!({"sessionUpdate":"config_option_update"}), &turn).is_none()
        );
    }

    #[tokio::test]
    async fn restarting_marks_running_turns_interrupted_and_preserves_conversations() {
        let dir = tempfile::tempdir().unwrap();
        let record = ThreadResponse {
            thread: Thread {
                id: Some(session("native")),
                name: Some("Keep me".into()),
                status: SessionStatus::Running,
                turns: Some(vec![Arc::new(Turn {
                    id: "turn".into(),
                    status: TurnStatus::Running,
                    ..Default::default()
                })]),
                ..Default::default()
            },
            model: Some(ModelRef {
                provider: PROVIDER,
                id: DEFAULT_MODEL.into(),
            }),
        };
        crate::platform::save_private_json(
            &dir.path().join("conversations.json"),
            &BTreeMap::from([("native", record)]),
        )
        .unwrap();
        let adapter = DeepSeek::load("dsh".into(), dir.path().into())
            .await
            .unwrap();
        let restored = adapter.open("native", 100, true).await.unwrap();
        assert_eq!(restored.thread.name.as_deref(), Some("Keep me"));
        assert_eq!(restored.thread.status, SessionStatus::Idle);
        assert_eq!(
            restored.thread.turns.as_ref().unwrap()[0].status,
            TurnStatus::Interrupted
        );
        assert_eq!(restored.model.unwrap().id, DEFAULT_MODEL);
    }

    #[tokio::test]
    async fn key_setup_requires_a_current_login_and_keeps_secrets_out_of_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let adapter = DeepSeek::load("dsh".into(), dir.path().into())
            .await
            .unwrap();
        assert!(
            adapter
                .account(AccountCommand::SubmitLogin {
                    id: "wrong".into(),
                    code: "secret".into()
                })
                .await
                .is_err()
        );
        let AccountReply::Login(login) = adapter.account(AccountCommand::StartLogin).await.unwrap()
        else {
            panic!("login expected")
        };
        assert!(matches!(
            adapter
                .account(AccountCommand::SubmitLogin {
                    id: login.login_id.clone(),
                    code: "test-secret".into(),
                })
                .await
                .unwrap(),
            AccountReply::Complete
        ));
        let AccountReply::Status(status) = adapter
            .account(AccountCommand::ReadLogin { id: login.login_id })
            .await
            .unwrap()
        else {
            panic!("status expected")
        };
        assert!(status.completed);
        assert_eq!(
            &**adapter.api_key().await.unwrap().as_ref().unwrap(),
            "test-secret"
        );
        assert!(
            !serde_json::to_string(&Identity::list(&adapter).await.unwrap())
                .unwrap()
                .contains("test-secret")
        );
        adapter
            .account(AccountCommand::Logout { id: ACCOUNT.into() })
            .await
            .unwrap();
        assert!(adapter.api_key().await.unwrap().is_none());
    }
}
