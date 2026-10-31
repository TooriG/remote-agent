//! The Host's RPC entry point. Dispatch admits each call, then routes it to the
//! owner of its resource; workflows that span owners live in the domain files.
mod activity;
mod conversations;
mod model_selections;
mod project_import;
mod sessions;
mod titles;
mod update;
mod worktrees;

use super::routing::{HostReply, HostSession, SessionId, SessionRouter};
use crate::ProjectStore;
use crate::providers::{
    Agent, Backend, Failure, SessionListScope, read_session_page, session_pages,
};
use agent_protocol::{
    models::{Thread, ThreadResponse},
    operations as op,
    protocol::{Body, Call, Response},
    session::ProviderKind,
};
use futures_util::{StreamExt, TryStreamExt};
use sessions::describe_thread;
use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
};
use worktrees::worktree_active;

const SESSION_LIST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

#[derive(Clone)]
pub struct HostRpcService {
    inner: Arc<ServiceInner>,
}

struct ServiceInner {
    browser: OnceLock<Arc<crate::browser::Browser>>,
    providers: crate::providers::Registry,
    projects: ProjectStore,
    router: SessionRouter,
    event_pumps: std::sync::Once,
    files: crate::workspace_files::WorkspaceFiles,
    worktrees: crate::worktrees::Worktrees,
    worktree_access: tokio::sync::RwLock<()>,
    permission_settings_access: tokio::sync::Mutex<()>,
    terminals: crate::terminals::Terminals,
    dictation: crate::dictation::Dictation,
    conversations: super::conversations::Conversations,
    model_selections: model_selections::ModelSelections,
    history_scope: OnceLock<(std::path::PathBuf, bool)>,
    updating: std::sync::atomic::AtomicBool,
}

impl HostRpcService {
    pub fn new(backends: impl IntoIterator<Item = Backend>, projects: ProjectStore) -> Self {
        let files = crate::workspace_files::WorkspaceFiles::new(
            projects.path().with_file_name("bex-attachments"),
        );
        let router = SessionRouter::new()
            .with_read_state(projects.path().with_file_name("bex-read-state.json"));
        let (providers, dictation) = crate::providers::Registry::new(backends);
        Self {
            inner: Arc::new(ServiceInner {
                history_scope: OnceLock::new(),
                updating: std::sync::atomic::AtomicBool::new(false),
                conversations: super::conversations::Conversations::new(
                    projects.path().with_file_name("bex-conversations.json"),
                ),
                model_selections: model_selections::ModelSelections::new(
                    projects.path().with_file_name("bex-model-selections.json"),
                ),
                browser: OnceLock::new(),
                providers,
                dictation,
                worktrees: crate::worktrees::Worktrees::new(projects.path()),
                worktree_access: tokio::sync::RwLock::new(()),
                permission_settings_access: Default::default(),
                terminals: Default::default(),
                projects,
                router,
                event_pumps: std::sync::Once::new(),
                files,
            }),
        }
    }

    /// Clients compare this with cached state at the start of every connection.
    pub(crate) fn storage_scope(&self) -> Result<String, serde_json::Error> {
        self.inner.providers.storage_scope()
    }

    async fn account_request(&self, request: Call) -> Result<Body, Failure> {
        if matches!(request, Call::ListAccounts(_)) {
            return Ok(self.inner.providers.accounts().await.into());
        }
        use crate::providers::{AccountCommand as Command, AccountReply};
        let (provider, command) = match request {
            Call::StartAccountLogin(p) => (p.provider, Command::StartLogin),
            Call::SelectAccount(p) => (p.provider, Command::Select { id: p.id }),
            Call::LogoutAccount(p) => (p.provider, Command::Logout { id: p.id }),
            Call::ReadAccountLogin(p) => (p.provider, Command::ReadLogin { id: p.id }),
            Call::CancelAccountLogin(p) => (p.provider, Command::CancelLogin { id: p.id }),
            Call::SubmitAccountLogin(p) => (
                p.provider,
                Command::SubmitLogin {
                    id: p.id,
                    code: p.code,
                },
            ),
            _ => return Err(Failure::new("invalid_params", "not an account request")),
        };
        Ok(
            match self.inner.providers.get(provider)?.account(command).await? {
                AccountReply::Selection(value) => value.into(),
                AccountReply::Login(value) => value.into(),
                AccountReply::Status(value) => value.into(),
                AccountReply::Complete => agent_protocol::models::Empty {}.into(),
            },
        )
    }

    pub async fn enable_browser(&self, profile: std::path::PathBuf) -> Result<(), String> {
        let browser = crate::browser::Browser::start(profile).await?;
        self.inner
            .browser
            .set(browser)
            .map_err(|_| "BEX browser is already configured".to_owned())
    }

    fn browser_config(&self, thread: &str) -> Result<Option<serde_json::Value>, Failure> {
        self.inner
            .browser
            .get()
            .map(|browser| {
                browser
                    .provider_config(thread)
                    .map_err(|e| Failure::new("browser_unavailable", e))
            })
            .transpose()
    }

    pub(crate) async fn shutdown_owned_processes(&self) {
        if let Some(browser) = self.inner.browser.get() {
            browser.shutdown().await;
        }
        self.inner.terminals.shutdown().await;
        self.inner.providers.shutdown().await;
    }

    pub(crate) fn revoke_device(&self, principal: &str) {
        self.inner.terminals.revoke_device(principal);
        self.inner.router.revoke_device(principal);
    }
    pub fn open_session(&self) -> HostSession {
        self.start_event_pumps();
        self.inner.router.open_session()
    }

    pub(crate) fn open_authenticated_session(&self, principal: String) -> HostSession {
        self.start_event_pumps();
        self.inner
            .router
            .open_authenticated_session(Some(principal))
    }

    pub fn close_session(&self, session: SessionId) {
        self.inner.router.close_session(session);
        self.inner.terminals.close_session(session);
        self.inner.files.clear_session(session);
        self.inner.dictation.close_session(session);
    }

    pub(crate) fn data_recipients(&self) -> (Vec<String>, Option<String>) {
        (
            self.inner.providers.data_recipients(),
            self.inner.dictation.data_recipient(),
        )
    }
    pub(crate) fn provider_errors(&self) -> serde_json::Value {
        self.inner.providers.errors()
    }

    pub fn start(&self) {
        self.start_event_pumps();
    }

    /// Dispatch a classified message from an authenticated session.
    pub async fn dispatch(&self, session: SessionId, message: &Call) -> Result<HostReply, String> {
        self.inner.router.ensure_session(session)?;
        if let Call::OpenSession(params) = message {
            return self.session_open(session, params).await;
        }
        if let Call::CreateSession(params) = message {
            let _workspace = self.inner.worktree_access.read().await;
            let result = async {
                let response = self.create_session(params.clone()).await?;
                let target = response.thread.id.clone().ok_or_else(|| {
                    Failure::new("invalid_thread", "created session ID is missing")
                })?;
                let read = self
                    .inner
                    .router
                    .retain_execution(target)
                    .map_err(|error| Failure::new("invalid_thread", error))?;
                self.inner
                    .router
                    .finish_session_read(read, session, response)
                    .map_err(|error| Failure::new("session_create_failed", error))
            }
            .await;
            return match result {
                Ok(reply) => Ok(reply),
                Err(error) => Ok(Response::from_result::<(), _>(Err(error)).into()),
            };
        }
        let result = async {
            let (target, input_id) = session_target(message);
            let target = target.cloned();
            let submission = target.as_ref().zip(input_id);
            // Keep the execution alive and serialize decisions through the
            // provider acknowledgement, including concurrent client inputs.
            let _read = submission
                .map(|(target, _)| self.inner.router.retain_execution(target.clone()))
                .transpose()
                .map_err(|error| Failure::new("invalid_params", error))?;
            let _serial = if let Some(target) = target.as_ref().filter(|_| {
                matches!(
                    message,
                    Call::Submit(_) | Call::RenameSession(_) | Call::ForkSession(_)
                )
            }) {
                Some(self.inner.router.submission_lock(target).lock_owned().await)
            } else {
                None
            };
            let _workspace_read = if matches!(
                message,
                Call::Submit(_)
                    | Call::ForkSession(_)
                    | Call::AnswerSession(_)
                    | Call::StartTerminal(_)
                    | Call::WriteFile(_)
                    | Call::Upload(_)
                    | Call::ReviewWorkspace(_)
            ) {
                Some(self.inner.worktree_access.read().await)
            } else {
                None
            };
            if _workspace_read.is_some()
                && self
                    .inner
                    .updating
                    .load(std::sync::atomic::Ordering::Acquire)
            {
                return Err(Failure::new(
                    "host_updating",
                    "Bexを更新しています。再接続後にもう一度お試しください。",
                ));
            }
            if let Some((target, id)) = submission {
                if matches!(message, Call::Submit(_))
                    && let Some(receipt) = self.inner.router.submission_receipt(target, id)
                {
                    return Ok(receipt.into());
                }
                self.inner.router.begin_submission(target, id)?;
            }
            let result = if let Call::Submit(input) = message {
                self.submit_input(target.as_ref().expect("submission target"), input)
                    .await
                    .map(Into::into)
            } else {
                self.request(session, message, target.as_ref()).await
            };
            if let Some((target, id)) = submission {
                use agent_protocol::session::SubmissionDelivery;
                let delivery = match &result {
                    Ok(Body::Submission(receipt)) => SubmissionDelivery::Accepted {
                        turn_id: receipt.turn_id.clone(),
                    },
                    Ok(_) => unreachable!("only submission returns delivery evidence"),
                    Err(error) if error.delivery == agent_transport::peer::Delivery::NotSent => {
                        SubmissionDelivery::Rejected
                    }
                    Err(_) => SubmissionDelivery::Unknown,
                };
                self.inner.router.finish_submission(target, id, delivery);
            }
            result
        }
        .await;
        if let Err(error) = &result {
            agent_transport::diagnostics::rpc_error(
                message.method(),
                None,
                &serde_json::value::to_raw_value(error).map_err(invalid_message)?,
            );
        }
        Ok(Response::from_result(result).into())
    }

    async fn request(
        &self,
        session: SessionId,
        request: &Call,
        target_session: Option<&agent_protocol::session::SessionRef>,
    ) -> Result<Body, Failure> {
        if let Call::Browser(params) = request {
            let browser = self.inner.browser.get().ok_or_else(|| {
                Failure::new(
                    "browser_unavailable",
                    "このHostではBEXブラウザが有効になっていません。",
                )
            })?;
            return browser
                .request(params)
                .await
                .map(Into::into)
                .map_err(|e| Failure::new("browser_failed", e));
        }
        if let Call::ReadItem(params) = request {
            let Some(target) = target_session else {
                return Err(Failure::new("invalid_params", "session ID is required"));
            };
            return self
                .read_item(session, params, target)
                .await
                .map(Into::into);
        }
        if let Some(target) = target_session {
            self.check_native_operation(target, request)?;
        }
        let response = match request {
            Call::ReadHandoffHistory(params) => self.read_handoff_history(params).await?.into(),
            Call::Unsupported => {
                return Err(Failure::new(
                    "method_not_found",
                    "このHostはこの操作に対応していません。PC側のBEXを更新してください。",
                ));
            }
            Call::MarkSessionRead(params) => {
                self.inner.router.mark_session_read(params)?;
                agent_protocol::models::Empty {}.into()
            }
            Call::ReadTaskActivity(_) => self.read_task_activity(false).await?.into(),
            Call::RegisterLiveActivity(params) => {
                self.register_live_activity(session, params)?.into()
            }
            Call::UnregisterLiveActivity(params) => {
                self.unregister_live_activity(session, params)?.into()
            }
            Call::ReadTurnItems(params) => self.read_turn_items(session, params).await?.into(),
            Call::ReadHistory(params) => self.read_history(params).await?.into(),

            Call::RequestSession(params) => (self
                .inner
                .router
                .request_session(&params.request_id.to_string())
                .ok_or_else(|| {
                    Failure::new("request_unavailable", "request is no longer pending")
                })?)
            .into(),
            Call::ConnectionPerformance(performance) => {
                let performance = performance.clone();
                tokio::task::spawn_blocking(move || {
                    agent_transport::diagnostics::connection_performance(&performance)
                })
                .await
                .map_err(|error| Failure::new("diagnostic_write_failed", error.to_string()))?;
                agent_protocol::models::Empty {}.into()
            }
            Call::AnswerSession(params) => (self
                .answer_request(session, params.request_id.clone(), params.answer.clone())
                .await?)
                .into(),
            Call::AddProject(params) => (self
                .inner
                .projects
                .register(std::path::Path::new(&params.cwd))
                .await
                .map_err(|error| Failure::new("project_add_failed", error))?)
            .into(),
            Call::ScanProjects(_) => self.scan_projects().await?.into(),
            Call::ImportProjects(params) => self.import_projects(&params.paths).await?.into(),
            Call::ReadListDecorations(params) => self.list_decorations(params).await?.into(),
            Call::ListSessions(params) => self.list_sessions(params).await?.into(),
            Call::ListProjectSessions(params) => self.list_project_sessions(params).await?.into(),
            Call::ListAgents(params) => self.list_agents(params).await?.into(),
            Call::ReadWorktreeSettings(_) | Call::UpdateWorktreeSettings(_) => {
                let update = if let Call::UpdateWorktreeSettings(settings) = request {
                    Some(settings.clone())
                } else {
                    None
                };
                (self
                    .inner
                    .worktrees
                    .settings(update)
                    .await
                    .map_err(|error| Failure::new("worktree_settings_failed", error))?)
                .into()
            }
            Call::ListWorktrees(_) => (self.worktree_list().await?).into(),
            Call::RemoveWorktree(params) => {
                (self.remove_worktree(params.path.clone()).await?).into()
            }

            Call::CreateSession(_) => unreachable!("creation owns its subscription"),
            Call::StartTerminal(params) => (self
                .inner
                .terminals
                .start(
                    self.inner.router.clone(),
                    session,
                    params.handle.clone(),
                    params.cwd.clone(),
                    params.size,
                )
                .await
                .map_err(|error| Failure::new("terminal_start_failed", error))?)
            .into(),
            Call::WriteTerminal(_)
            | Call::ResizeTerminal(_)
            | Call::KillTerminal(_)
            | Call::DetachTerminal(_) => (self
                .inner
                .terminals
                .request(session, request)
                .await
                .map_err(|error| Failure::new("terminal_operation_failed", error))?)
            .into(),
            Call::PrepareDictation(params) => {
                self.inner
                    .dictation
                    .prepare(session, params.id.clone())
                    .map_err(|error| Failure::new("dictation_failed", error))?;
                agent_protocol::models::Empty {}.into()
            }
            Call::CancelDictation(params) => {
                self.inner.dictation.cancel(session, &params.id);
                agent_protocol::models::Empty {}.into()
            }
            Call::Transcribe(params) => (self
                .inner
                .dictation
                .transcribe(session, params.preparation.as_deref(), &params.audio)
                .await
                .map_err(|error| Failure::new("dictation_failed", error))?)
            .into(),
            Call::ReviewWorkspace(params) => {
                (crate::inspect_workspace(params.cwd.clone())
                    .await
                    .map_err(|error| Failure::new("workspace_review_failed", error))?)
                .into()
            }
            Call::ListFiles(_)
            | Call::ReadFile(_)
            | Call::WriteFile(_)
            | Call::Upload(_)
            | Call::Download(_)
            | Call::ReadVisualization(_) => self
                .inner
                .files
                .request(session, request.clone())
                .await
                .map_err(|error| Failure::new("file_operation_failed", error))?,
            Call::ForkSession(params) => {
                let target = target_session.expect("session-scoped fork");
                self.fork_session(target, params).await?.into()
            }
            Call::Interrupt(params) => {
                let target = target_session.expect("session-scoped interrupt");
                self.interrupt(target, params).await?.into()
            }
            Call::RenameSession(params) => {
                let target = target_session.expect("session-scoped rename");
                self.rename_session(target, params).await?.into()
            }
            _ => self.provider_request(request).await?,
        };
        Ok(response)
    }

    fn check_native_operation(
        &self,
        target: &agent_protocol::session::SessionRef,
        request: &Call,
    ) -> Result<(), Failure> {
        let method = request.method();
        let row = self.inner.conversations.get(target)?;
        let native = row.as_ref().map_or(target, |row| &row.active);
        let capabilities = self.inner.providers.get(native.provider)?.capabilities();
        if matches!(request, Call::ForkSession(_)) && row.is_none() && !capabilities.fork
            || matches!(request, Call::RenameSession(_)) && row.is_none() && !capabilities.rename
        {
            return Err(Failure::new(
                "unsupported_operation",
                format!("{method} is unsupported by this provider"),
            ));
        }
        Ok(())
    }

    /// Dispatch remaining provider requests; unknown requests return method_not_found.
    async fn provider_request(&self, request: &Call) -> Result<Body, Failure> {
        Ok(match request {
            Call::ReadPermissionSettings(params) => {
                let _guard = self.inner.permission_settings_access.lock().await;
                self.inner
                    .providers
                    .get(params.provider)?
                    .read_permissions()
                    .await?
                    .into()
            }
            Call::UpdatePermissionSettings(params) => {
                let _guard = self.inner.permission_settings_access.lock().await;
                self.inner
                    .providers
                    .get(params.provider)?
                    .update_permissions(params.mode, &params.version)
                    .await?
                    .into()
            }
            Call::ComposerCatalog(params) => self.inner.providers.catalog(&params.cwd).await.into(),
            Call::ReadAccountUsage(params) => self
                .inner
                .providers
                .get(params.provider)?
                .usage(&params.id)
                .await?
                .into(),
            Call::ListAccounts(_)
            | Call::SelectAccount(_)
            | Call::LogoutAccount(_)
            | Call::StartAccountLogin(_)
            | Call::ReadAccountLogin(_)
            | Call::SubmitAccountLogin(_)
            | Call::CancelAccountLogin(_) => self.account_request(request.clone()).await?,

            Call::CodexCli(params) => self
                .inner
                .providers
                .get(ProviderKind::Codex)?
                .codex_cli(params.update)
                .await?
                .into(),
            Call::ReadModelSettings(params) => self
                .inner
                .providers
                .get(params.model.provider)?
                .model_settings(&params.model.id)
                .await?
                .into(),
            Call::ReadDshCredentials(_) => self
                .inner
                .providers
                .get(ProviderKind::DeepSeek)?
                .dsh_credentials()
                .await?
                .into(),
            Call::UpdateDshCredential(update) => self
                .inner
                .providers
                .get(ProviderKind::DeepSeek)?
                .update_dsh_credential(update.clone())
                .await?
                .into(),
            Call::ListModels(params) => self.inner.providers.models(params).await?.into(),
            _ => {
                return Err(Failure::new(
                    "method_not_found",
                    format!("unregistered method: {}", request.method()),
                ));
            }
        })
    }

    pub(crate) fn files(&self) -> &crate::workspace_files::WorkspaceFiles {
        &self.inner.files
    }

    fn start_event_pumps(&self) {
        self.inner.event_pumps.call_once(|| {
            if self.inner.router.apns().is_some() {
                let owner = Arc::downgrade(&self.inner);
                tokio::spawn(async move {
                    let mut delay = 1;
                    loop {
                        let Some(inner) = owner.upgrade() else { break };
                        let service = Self { inner };
                        let Some(apns) = service.inner.router.apns() else {
                            break;
                        };
                        if apns.is_ready() {
                            break;
                        }
                        let _ = service.read_task_activity(false).await;
                        if apns.is_ready() {
                            break;
                        }
                        drop(apns);
                        drop(service);
                        tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                        delay = (delay * 2).min(60);
                    }
                });
            }

            for (_, agent) in self.inner.providers.all() {
                if let Some(mut events) = agent.event_stream() {
                    let supports_fork = agent.capabilities().fork;
                    let router = self.inner.router.clone();
                    let owner = Arc::downgrade(&self.inner);
                    tokio::spawn(async move {
                        while let Some(event) = events.recv().await {
                            let Some(inner) = owner.upgrade() else {
                                break;
                            };
                            let bindings = if matches!(
                                event.change,
                                crate::providers::AgentChange::Stopped { .. }
                            ) {
                                inner.conversations.active_bindings()
                            } else {
                                Ok(Vec::new())
                            };
                            let result = bindings
                                .and_then(|bindings| {
                                    inner
                                        .conversations
                                        .route(event.change, supports_fork)
                                        .map(|event| (bindings, event))
                                })
                                .map_err(|error| error.to_string())
                                .and_then(|(bindings, event)| {
                                    event.map_or(Ok(()), |event| router.apply(event, &bindings))
                                });
                            if let Some(applied) = event.applied {
                                let _ = applied.send(result);
                            }
                        }
                    });
                }
            }
        });
    }
}

fn invalid_message(error: impl std::fmt::Display) -> String {
    format!("invalid request: {error}")
}

// Serialize the sorted map directly: a JSON Value's object ordering can vary
// with serde_json features unified by unrelated client dependencies.
// Every session-scoped operation provides its target here. Submission IDs
// additionally identify the operations whose delivery must be tracked.
fn session_target(request: &Call) -> (Option<&agent_protocol::session::SessionRef>, Option<&str>) {
    match request {
        Call::Submit(p) => (Some(&p.thread_id), Some(p.client_user_message_id.as_str())),
        Call::ForkSession(p) => (Some(&p.thread_id), None),
        Call::Interrupt(p) => (Some(&p.thread_id), None),
        Call::ReadItem(p) => (Some(&p.thread_id), None),
        Call::ReadTurnItems(p) => (Some(&p.session), None),
        Call::RenameSession(p) => (Some(&p.thread_id), None),
        _ => (None, None),
    }
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn failed_model_providers_remain_identifiable_when_no_catalog_is_available() {
        use super::*;
        let root = tempfile::tempdir().unwrap();
        let providers = [
            ProviderKind::Codex,
            ProviderKind::Claude,
            ProviderKind::DeepSeek,
        ];
        let service = HostRpcService::new(
            providers.map(|provider| Backend::unavailable(provider, "offline")),
            ProjectStore::new(root.path().join("worktrees.json")),
        );
        let session = service.open_session();
        let response = service
            .dispatch(
                session.id(),
                &Call::ListModels(op::ListModels {
                    cursor: None,
                    limit: 100,
                }),
            )
            .await
            .unwrap();
        let Response::Success { result: page } =
            agent_protocol::protocol::decode::<Response<op::ModelPage>>(&response.initial).unwrap()
        else {
            panic!("catalog failures must retain each provider's error");
        };
        assert!(page.data.is_empty());
        assert!(page.next_cursor.is_none());
        let errors = page.provider_errors.unwrap();
        assert_eq!(errors.len(), providers.len());
        for provider in providers {
            assert_eq!(errors[provider.key()]["code"], "provider_unavailable");
        }
    }

    #[tokio::test]
    async fn successful_configuration_clears_the_provider_startup_error() {
        use super::*;
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("claude");
        let registry = directory.join("accounts/accounts.json");
        std::fs::create_dir_all(registry.parent().unwrap()).unwrap();
        std::fs::write(&registry, "invalid registry").unwrap();
        let program = root.path().join("native-cli.exe");
        std::fs::write(&program, []).unwrap();
        let configure = || {
            crate::providers::Claude::load(
                program.clone(),
                directory.clone(),
                Some(root.path().join("native")),
            )
        };
        let error = match configure().await {
            Err(error) => error,
            Ok(_) => panic!("invalid registry was accepted"),
        };
        let service = HostRpcService::new(
            [Backend::unavailable(ProviderKind::Claude, error)],
            ProjectStore::new(root.path().join("worktrees.json")),
        );
        assert_eq!(
            service.provider_errors()["claude"]["code"],
            "provider_unavailable"
        );
        std::fs::write(&registry, r#"{"accounts":[],"selectedId":null}"#).unwrap();
        let service = HostRpcService::new(
            [configure().await.unwrap().into()],
            ProjectStore::new(root.path().join("worktrees.json")),
        );
        assert!(service.provider_errors().get("claude").is_none());
        let session = service.open_session();
        let response = service
            .dispatch(
                session.id(),
                &Call::CreateSession(op::CreateSession {
                    provider: ProviderKind::Claude,
                    cwd: Some(root.path().to_string_lossy().into_owned()),
                    model: None,
                }),
            )
            .await
            .unwrap();
        let Response::Success { result: response } = agent_protocol::protocol::decode::<
            Response<agent_protocol::session::OpenedSession>,
        >(&response.initial)
        .unwrap() else {
            panic!("provider did not recover");
        };
        assert_eq!(
            response.response.thread.id.unwrap().provider,
            ProviderKind::Claude
        );
    }

    #[test]
    fn unknown_and_native_methods_are_rejected_before_dispatch() {
        for method in [
            "not/public",
            "provider",
            "thread/list",
            "thread/read",
            "thread/resume",
            "turn/start",
            "turn/steer",
            "thread/queue/add",
            "account/read",
        ] {
            for params in [
                serde_json::json!({}),
                serde_json::json!({"threadId":{"provider":"claude","id":"native"}}),
            ] {
                assert!(
                    agent_protocol::protocol::json_boundary::call(method, params).is_err(),
                    "{method}"
                );
            }
        }
    }
    #[tokio::test]
    async fn provider_capabilities_are_checked_before_provider_availability() {
        use super::*;
        let root = tempfile::tempdir().unwrap();
        let claude = crate::providers::Claude::load(
            root.path().join("missing-claude"),
            root.path().join("claude"),
            Some(root.path().join("native")),
        )
        .await
        .unwrap();
        let service = HostRpcService::new(
            [claude.into()],
            ProjectStore::new(root.path().join("bex-worktrees.json")),
        );
        let session = service.open_session();
        for (method, expected) in [
            ("host/session/rename", "unsupported_operation"),
            ("host/session/submit", "provider_unavailable"),
        ] {
            let call = agent_protocol::protocol::json_boundary::call(method, serde_json::json!({"threadId":{"provider":"claude","id":"native"},"clientUserMessageId":method,"lastTurnId":"turn","name":"Renamed","input":[],"expectedTurnId":"turn"})).unwrap();
            let response = service.dispatch(session.id(), &call).await.unwrap();
            let response = agent_protocol::protocol::decode::<
                agent_protocol::protocol::Response<agent_protocol::session::OpenedSession>,
            >(&response.initial)
            .unwrap()
            .into_value();
            assert_eq!(response["error"]["code"], expected, "{method}");
        }
    }

    #[tokio::test]
    async fn unavailable_provider_does_not_retain_a_submission_as_in_flight() {
        use super::*;
        let root = tempfile::tempdir().unwrap();
        let service = HostRpcService::new(
            [Backend::unavailable(ProviderKind::Codex, "unavailable")],
            ProjectStore::new(root.path().join("bex-worktrees.json")),
        );
        let session = service.open_session();
        for method in ["host/session/submit"] {
            let call = agent_protocol::protocol::json_boundary::call(
                method,
                serde_json::json!({"threadId":{"provider":"codex","id":"native"},"clientUserMessageId":"input","input":[],"expectedTurnId":"turn"}),
            ).unwrap();
            for _ in 0..2 {
                let response = service.dispatch(session.id(), &call).await.unwrap();
                let response = agent_protocol::protocol::decode::<Response<()>>(&response.initial)
                    .unwrap()
                    .into_value();
                assert_eq!(response["error"]["code"], "provider_unavailable");
                assert_eq!(response["error"]["delivery"], "notSent");
            }
        }
    }

    #[tokio::test]
    async fn enabling_host_owned_harness_keeps_the_existing_draft_namespace() {
        let root = tempfile::tempdir().unwrap();
        let claude = crate::providers::claude::Claude::load(
            "claude".into(),
            root.path().join("accounts"),
            Some(root.path().join("native")),
        )
        .await
        .unwrap();
        let service = super::HostRpcService::new(
            [claude.into()],
            crate::ProjectStore::new(root.path().join("projects.json")),
        );
        let before = service.storage_scope().unwrap();
        let claude = crate::providers::claude::Claude::load(
            "claude".into(),
            root.path().join("accounts"),
            Some(root.path().join("native")),
        )
        .await
        .unwrap();
        let harness =
            crate::providers::deepseek::DeepSeek::load("dsh".into(), root.path().join("harness"))
                .await
                .unwrap();
        let service = super::HostRpcService::new(
            [claude.into(), harness.into()],
            crate::ProjectStore::new(root.path().join("projects.json")),
        );
        let after = service.storage_scope().unwrap();
        assert_eq!(
            before, after,
            "Adding Harness must not archive existing drafts or navigation"
        );
    }
}
