//! The application owns UI resources shared by native views; each view observes immutable snapshots.
use crate::{
    client::{Client, Update},
    shared::Shared,
};
use agent_core::{
    client::AgentError,
    panels::{PanelKind, PanelLayout, PanelScope, Panels},
    presentation::view::Presentation,
    state::{DraftKey, Intent},
};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

mod worktrees;
pub(crate) use worktrees::{WorktreeAction, WorktreeEditor};
mod connections;
pub(crate) use connections::{ListRequest, ListScope, ListSource};

#[derive(Clone, PartialEq)]
pub(crate) struct Host {
    pub id: String,
    pub name: String,
}

pub(crate) enum Navigation {
    Settings(bool),
    Provider(Option<agent_protocol::session::ProviderKind>),
    ToggleSidebar,
}

#[derive(Clone)]
pub(crate) struct SideChat {
    pub publication: Option<Shared<Presentation>>,
    pub pending: bool,
    pub failure: Option<agent_core::presentation::error::ErrorText>,
    provider: Option<agent_protocol::session::ProviderKind>,
    selection: Arc<str>,
}

#[derive(Clone)]
pub(crate) struct State {
    pub hosts: Vec<Host>,
    pub host: String,
    pub all_hosts: bool,
    pub publications: BTreeMap<String, Shared<Presentation>>,
    pub panels: Arc<Panels>,
    pub failure: Option<String>,
    pub pairing: bool,
    pub settings: bool,
    pub provider: Option<agent_protocol::session::ProviderKind>,
    pub account_pending: bool,
    pub sidebar: bool,
    pub file_line: Option<(u64, Option<u32>)>,
    pub side_chats: BTreeMap<u64, SideChat>,
    pub browser_renderer: String,
    pub worktrees: Option<WorktreeEditor>,
    uploads: BTreeMap<uuid::Uuid, (String, DraftKey)>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            hosts: Vec::new(),
            host: String::new(),
            all_hosts: false,
            publications: BTreeMap::new(),
            panels: Panels::new(),
            failure: None,
            pairing: false,
            settings: false,
            provider: None,
            account_pending: false,
            sidebar: true,
            file_line: None,
            side_chats: BTreeMap::new(),
            browser_renderer: String::new(),
            worktrees: None,
            uploads: BTreeMap::new(),
        }
    }
}

impl State {
    pub fn account_locked(&self) -> bool {
        self.account_pending
            || self.publications.get(&self.host).is_some_and(|p| {
                p.settings.account_login().is_some()
                    || self.provider.is_some_and(|provider| {
                        p.snapshot.operation_running(
                            agent_core::state::operations::OperationKey::AccountLogin { provider },
                        )
                    })
            })
    }

    fn receive(&mut self, update: Update) {
        match update {
            Update::AttachmentsFinished(id) => {
                self.uploads.remove(&id);
            }
            Update::Hosts(hosts) => {
                self.hosts = hosts
                    .into_iter()
                    .map(|host| Host {
                        id: host.id,
                        name: host.name,
                    })
                    .collect();
                self.resolve_selection();
            }
            Update::Presentation { host, value } => {
                let before = self.publications.get(&host).map(|p| {
                    p.snapshot
                        .panel_scope(host.clone(), p.context.navigation().draft_key)
                });
                if let Some(before) = before
                    && let Some(session) = value.snapshot.submitted_session(before.draft.clone())
                {
                    self.panels = self.panels.clone().rekey(
                        before,
                        value
                            .snapshot
                            .panel_scope(host.clone(), DraftKey::Session { session }),
                    );
                }
                // Rekey and publication are one update, so every native view sees the same scope.
                self.publications.insert(host, Shared(value));
            }
            Update::Error(reason) => self.failure = Some(reason),
            Update::PanePresentation { id, value } => {
                if let Some(pane) = self.side_chats.get_mut(&id) {
                    pane.publication = Some(Shared(value));
                }
            }
            Update::SideChatReady { id, result } => {
                if let Some(pane) = self.side_chats.get_mut(&id) {
                    pane.pending = false;
                    match result {
                        Ok(value) => {
                            pane.publication = Some(Shared(value));
                            pane.failure = None;
                        }
                        Err(reason) => {
                            pane.failure =
                                Some(agent_core::presentation::error::error_text(&reason))
                        }
                    }
                }
            }
            Update::Paired(result) => match result {
                Ok(host) => {
                    self.host = host;
                    self.all_hosts = false;
                    self.pairing = false;
                    self.settings = false;
                    self.provider = None;
                }
                Err(reason) => {
                    self.pairing = false;
                    self.failure = Some(reason);
                }
            },
        }
        if self
            .worktrees
            .as_ref()
            .is_some_and(|editor| editor.host != self.host)
        {
            self.worktrees = None;
        }
    }

    pub fn uploading(&self, host: &str, draft: &DraftKey) -> bool {
        self.uploads
            .values()
            .any(|(target, key)| target == host && key == draft)
    }
}

pub(crate) struct Application {
    pub client: Arc<Client>,
    instance: uuid::Uuid,
    state: watch::Sender<Arc<State>>,
    stop: CancellationToken,
    runtime: tokio::runtime::Handle,
    jobs: tokio_util::task::TaskTracker,
    selection_path: PathBuf,
}

impl Application {
    #[cfg(target_os = "macos")]
    pub fn set_browser_renderer(&self, id: String) {
        self.state
            .send_modify(|state| Arc::make_mut(state).browser_renderer = id);
    }

    pub fn new(
        root: PathBuf,
        local: Option<crate::storage::Host>,
    ) -> Result<(tokio::runtime::Runtime, Arc<Self>), AgentError> {
        let selection_path = root.join("connection-selection.json");
        let selection = connections::load_selection(&selection_path)?;
        let (runtime, client, incoming) = Client::new(root, local)?;
        let (state, _) = watch::channel(Arc::new(State {
            host: selection.host_id.clone().unwrap_or_default(),
            all_hosts: selection.all,
            ..State::default()
        }));
        let application = Arc::new(Self {
            client,
            instance: uuid::Uuid::new_v4(),
            state,
            stop: CancellationToken::new(),
            runtime: runtime.handle().clone(),
            jobs: Default::default(),
            selection_path,
        });
        application.persist_selection(selection);
        let owner = Arc::downgrade(&application);
        let stop = application.stop.clone();
        application.jobs.spawn_on(async move {
            loop {
                let update = tokio::select! {
                    _ = stop.cancelled() => break,
                    update = incoming.recv() => match update { Ok(update) => update, Err(_) => break },
                };
                let Some(owner) = owner.upgrade() else { break };
                owner.state.send_modify(|state| Arc::make_mut(state).receive(update));
            }
        }, runtime.handle());
        Ok((runtime, application))
    }

    pub fn snapshot(&self) -> Arc<State> {
        self.state.borrow().clone()
    }
    pub fn subscribe(&self) -> watch::Receiver<Arc<State>> {
        self.state.subscribe()
    }

    pub fn pair(&self, invitation: agent_protocol::models::Invitation) {
        let start = self.state.send_if_modified(|state| {
            if state.pairing {
                return false;
            }
            Arc::make_mut(state).pairing = true;
            true
        });
        if start {
            self.client.pair(invitation);
        }
    }

    pub fn dismiss_failure(&self) {
        self.state
            .send_modify(|state| Arc::make_mut(state).failure = None);
    }

    pub fn navigate(&self, action: Navigation) {
        self.state.send_modify(|state| {
            let state = Arc::make_mut(state);
            match action {
                Navigation::Settings(open) => {
                    state.settings = open;
                    state.provider = None;
                }
                Navigation::Provider(provider) => state.provider = provider,
                Navigation::ToggleSidebar => state.sidebar = !state.sidebar,
            }
        });
    }

    pub fn account_request(self: &Arc<Self>, host: String, intent: Intent) {
        if self.stop.is_cancelled() {
            return;
        }
        let accepted = self.state.send_if_modified(|state| {
            if state.account_pending || state.host != host {
                return false;
            }
            Arc::make_mut(state).account_pending = true;
            true
        });
        if !accepted {
            return;
        }
        let response = self.client.request(&host, None, intent);
        let owner = Arc::downgrade(self);
        let stop = self.stop.clone();
        self.jobs.spawn_on(
            async move {
                tokio::select! { _ = stop.cancelled() => return, _ = response.recv() => {} }
                if let Some(owner) = owner.upgrade() {
                    owner
                        .state
                        .send_modify(|state| Arc::make_mut(state).account_pending = false);
                }
            },
            &self.runtime,
        );
    }

    pub fn set_panel_layout(&self, scope: PanelScope, layout: PanelLayout) {
        self.state.send_modify(|state| {
            let panels = state.panels.clone().set_layout(scope, layout);
            Arc::make_mut(state).panels = panels;
        });
    }

    pub fn attach(
        &self,
        host: String,
        pane: Option<u64>,
        draft: DraftKey,
        sources: Vec<crate::client::AttachmentSource>,
    ) {
        if sources.is_empty()
            || pane.is_some_and(|id| !self.snapshot().side_chats.contains_key(&id))
        {
            return;
        }
        let id = uuid::Uuid::new_v4();
        self.state.send_modify(|state| {
            Arc::make_mut(state)
                .uploads
                .insert(id, (host.clone(), draft.clone()));
        });
        self.client.attach_batch(id, host, pane, draft, sources);
    }

    pub fn open_side_chat(
        &self,
        scope: PanelScope,
        cwd: String,
        provider: Option<agent_protocol::session::ProviderKind>,
        selection: String,
    ) {
        let mut id = 0;
        self.state.send_modify(|state| {
            let opened = state
                .panels
                .clone()
                .open(scope, PanelKind::SideChat, Some(cwd));
            id = opened.id;
            let state = Arc::make_mut(state);
            state.panels = opened.panels;
            state.side_chats.insert(
                id,
                SideChat {
                    publication: None,
                    pending: false,
                    failure: None,
                    provider,
                    selection: selection.into(),
                },
            );
        });
        self.prepare_side_chat(id);
    }

    pub fn prepare_side_chat(&self, id: u64) {
        let mut request = None;
        self.state.send_if_modified(|state| {
            let Some(scope) = state.panels.scope(id) else {
                return false;
            };
            let cwd = state
                .panels
                .get(scope.clone())
                .surfaces
                .into_iter()
                .find(|s| s.id == id)
                .and_then(|s| s.path)
                .unwrap_or_default();
            let Some(pane) = Arc::make_mut(state)
                .side_chats
                .get_mut(&id)
                .filter(|pane| !pane.pending)
            else {
                return false;
            };
            pane.pending = true;
            pane.failure = None;
            request = Some((scope.host, cwd, pane.provider, pane.selection.to_string()));
            true
        });
        if let Some((host, cwd, provider, selection)) = request {
            self.client
                .prepare_side_chat(id, host, cwd, provider, selection);
        }
    }

    pub fn open_panel(
        &self,
        scope: PanelScope,
        kind: PanelKind,
        path: Option<String>,
        line: Option<u32>,
    ) {
        self.state.send_modify(|state| {
            let next = state.panels.clone().open(scope, kind, path);
            let state = Arc::make_mut(state);
            state.panels = next.panels;
            if kind == PanelKind::File {
                state.file_line = Some((next.id, line));
            }
        });
    }

    pub fn set_panel_open(&self, scope: PanelScope, open: bool) {
        self.state.send_modify(|state| {
            let panels = state.panels.clone().set_open(scope, open);
            Arc::make_mut(state).panels = panels;
        });
    }

    pub fn activate_panel(&self, scope: PanelScope, id: u64) {
        self.state.send_modify(|state| {
            let panels = state.panels.clone().activate(scope, id).panels;
            Arc::make_mut(state).panels = panels;
        });
    }

    pub fn close_panel(&self, scope: PanelScope, id: u64) {
        let mut terminal = false;
        let closed = self.state.send_if_modified(|state| {
            if state.panels.scope(id).as_ref() != Some(&scope) {
                return false;
            }
            terminal = state
                .panels
                .get(scope.clone())
                .surfaces
                .iter()
                .any(|surface| surface.id == id && surface.kind == PanelKind::Terminal);
            let panels = state.panels.clone().close(scope.clone(), id);
            let state = Arc::make_mut(state);
            state.panels = panels;
            state.side_chats.remove(&id);
            if state.file_line.is_some_and(|(file, _)| file == id) {
                state.file_line = None;
            }
            true
        });
        if !closed {
            return;
        }
        self.client.close_pane(id);
        if terminal {
            self.client.dispatch(
                &scope.host,
                None,
                Intent::CloseTerminal {
                    handle: self.terminal_handle(id),
                },
            );
        }
    }

    pub fn terminal_handle(&self, id: u64) -> String {
        format!("agent-terminal-{}-{id}", self.instance)
    }

    pub async fn shutdown(&self) -> Result<(), AgentError> {
        self.stop.cancel();
        self.jobs.close();
        self.jobs.wait().await;
        // Producer jobs have stopped; include changes accepted during a save.
        let saved =
            connections::save_selection(self.selection_path.clone(), self.snapshot().selection())
                .await;
        let closed = self.client.shutdown().await;
        closed.and(saved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::state::Snapshot;
    use agent_protocol::session::{ProviderKind, SessionRef};

    #[test]
    fn hiding_a_side_chat_keeps_its_resource_and_late_updates_cannot_reopen_a_closed_tab() {
        let directory = tempfile::tempdir().unwrap();
        let (runtime, application) = Application::new(directory.path().into(), None).unwrap();
        let scope =
            Snapshot::default().panel_scope("missing-host".into(), DraftKey::from("parent"));
        application.open_side_chat(
            scope.clone(),
            "/side".into(),
            Some(ProviderKind::Codex),
            "quote".into(),
        );
        let id = application
            .snapshot()
            .panels
            .get(scope.clone())
            .active
            .unwrap();
        application.set_panel_open(scope.clone(), false);
        assert!(application.snapshot().side_chats.contains_key(&id));
        let mut wrong_scope = scope.clone();
        wrong_scope.host = "another-host".into();
        application.close_panel(wrong_scope, id);
        assert!(application.snapshot().side_chats.contains_key(&id));
        application.close_panel(scope.clone(), id);
        application.state.send_modify(|state| {
            let state = Arc::make_mut(state);
            state.receive(Update::PanePresentation {
                id,
                value: Presentation::empty(),
            });
            state.receive(Update::SideChatReady {
                id,
                result: Ok(Presentation::empty()),
            });
        });
        assert!(!application.snapshot().side_chats.contains_key(&id));
        assert!(application.snapshot().panels.get(scope).surfaces.is_empty());
        runtime.block_on(application.shutdown()).unwrap();
    }

    #[test]
    fn submitting_a_draft_publishes_its_conversation_and_rekeyed_tabs_together() {
        let mut snapshot = Snapshot::default();
        snapshot.connection.storage_scope = "storage".into();
        let initial = Presentation::project(Arc::new(snapshot.clone()), None);
        let draft = initial.context.navigation().draft_key;
        let before = snapshot.panel_scope("host".into(), draft.clone());
        let session = SessionRef::new(ProviderKind::Codex, "submitted".into()).unwrap();
        let after = snapshot.panel_scope(
            "host".into(),
            DraftKey::Session {
                session: session.clone(),
            },
        );
        let mut state = State::default();
        state.receive(Update::Presentation {
            host: "host".into(),
            value: initial,
        });
        let opened =
            state
                .panels
                .clone()
                .open(before.clone(), PanelKind::Terminal, Some("/project".into()));
        state.panels = opened.panels;
        let retained = state.clone();
        Arc::make_mut(&mut snapshot.sessions.draft_sessions).insert(draft, session.clone());
        Arc::make_mut(&mut Arc::make_mut(&mut snapshot.pane).navigation).draft_key =
            DraftKey::Session { session };
        state.receive(Update::Presentation {
            host: "host".into(),
            value: Presentation::project(Arc::new(snapshot), None),
        });
        assert_eq!(state.panels.get(after.clone()).active, Some(opened.id));
        assert!(state.panels.get(before.clone()).surfaces.is_empty());
        assert_eq!(
            state.publications["host"]
                .snapshot
                .pane
                .navigation
                .draft_key,
            after.draft
        );
        assert_eq!(retained.panels.get(before).active, Some(opened.id));
    }

    #[test]
    fn finishing_one_batch_keeps_other_hosts_and_conversations_uploading() {
        let mut state = State::default();
        let a = DraftKey::from("a");
        let b = DraftKey::from("b");
        let first = uuid::Uuid::new_v4();
        let second = uuid::Uuid::new_v4();
        state.uploads.insert(first, ("host".into(), a.clone()));
        state.uploads.insert(second, ("host".into(), a.clone()));
        state
            .uploads
            .insert(uuid::Uuid::new_v4(), ("other".into(), b.clone()));
        state.receive(Update::AttachmentsFinished(first));
        assert!(state.uploading("host", &a));
        assert!(!state.uploading("host", &b));
        assert!(state.uploading("other", &b));
        state.receive(Update::AttachmentsFinished(second));
        assert!(!state.uploading("host", &a));
        assert!(state.uploading("other", &b));
    }

    #[test]
    fn native_views_observe_the_same_tabs_layout_navigation_and_file_target() {
        let directory = tempfile::tempdir().unwrap();
        let (runtime, application) = Application::new(directory.path().into(), None).unwrap();
        let mut sidebar = application.subscribe();
        let mut tools = application.subscribe();
        let scope = Snapshot::default().panel_scope("host".into(), DraftKey::from("draft"));
        application.open_panel(
            scope.clone(),
            PanelKind::File,
            Some("/a.rs".into()),
            Some(7),
        );
        let id = application
            .snapshot()
            .panels
            .get(scope.clone())
            .active
            .unwrap();
        application.set_panel_layout(scope.clone(), PanelLayout::Bottom);
        application.navigate(Navigation::Settings(true));
        application.navigate(Navigation::Provider(Some(ProviderKind::Codex)));
        let first = sidebar.borrow_and_update().clone();
        let second = tools.borrow_and_update().clone();
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(first.file_line, Some((id, Some(7))));
        assert_eq!(first.panels.get(scope.clone()).layout, PanelLayout::Bottom);
        assert!(first.settings);
        assert_eq!(first.provider, Some(ProviderKind::Codex));
        application.close_panel(scope.clone(), id);
        application.navigate(Navigation::Settings(false));
        let closed = application.snapshot();
        assert!(closed.panels.get(scope).surfaces.is_empty());
        assert!(closed.file_line.is_none());
        assert!(!closed.settings);
        assert!(closed.provider.is_none());
        assert_eq!(
            first.file_line,
            Some((id, Some(7))),
            "mounted views retain immutable snapshots"
        );
        runtime.block_on(application.shutdown()).unwrap();
    }

    #[test]
    fn pairing_failure_keeps_navigation_and_success_closes_setup_atomically() {
        let mut state = State {
            host: "existing".into(),
            settings: true,
            provider: Some(ProviderKind::Codex),
            pairing: true,
            ..Default::default()
        };
        state.receive(Update::Paired(Err("unavailable".into())));
        assert!(state.settings);
        assert_eq!(state.host, "existing");
        assert!(!state.pairing);
        state.receive(Update::Paired(Ok("paired".into())));
        assert_eq!(state.host, "paired");
        assert!(!state.pairing);
        assert!(!state.settings);
        assert!(state.provider.is_none());
    }

    #[test]
    fn host_updates_preserve_the_selection_until_that_host_is_removed() {
        let host = |id: &str| crate::storage::Host {
            id: id.into(),
            name: id.into(),
            ticket: "offline".into(),
            local_directory: None,
        };
        let mut state = State {
            host: "b".into(),
            ..Default::default()
        };
        state.receive(Update::Hosts(vec![host("a"), host("b")]));
        assert_eq!(state.host, "b");
        state.receive(Update::Hosts(vec![host("a")]));
        assert_eq!(state.host, "a");
        state.receive(Update::Hosts(Vec::new()));
        assert!(state.host.is_empty());
    }

    #[test]
    fn an_account_request_for_an_old_host_is_ignored_and_failure_unlocks_navigation() {
        let directory = tempfile::tempdir().unwrap();
        let (runtime, application) = Application::new(directory.path().into(), None).unwrap();
        let mut updates = application.subscribe();
        application.account_request(
            "old-host".into(),
            Intent::ListAccounts(agent_core::state::operations::ListAccounts {}),
        );
        assert!(!application.snapshot().account_pending);
        application.account_request(
            String::new(),
            Intent::ListAccounts(agent_core::state::operations::ListAccounts {}),
        );
        runtime.block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(10), async {
                loop {
                    let state = updates.borrow_and_update().clone();
                    if state.failure.is_some() && !state.account_locked() {
                        break;
                    }
                    updates.changed().await.unwrap();
                }
            })
            .await
            .unwrap();
            application.shutdown().await.unwrap();
        });
    }

    #[test]
    fn pending_requests_login_prompts_and_the_selected_providers_login_each_lock_navigation() {
        use agent_core::state::operations::{OperationKey, OperationPhase, OperationState};
        let mut state = State {
            host: "host".into(),
            ..Default::default()
        };
        assert!(!state.account_locked());
        state.account_pending = true;
        assert!(
            state.account_locked(),
            "the request is locked before core publishes"
        );
        state.account_pending = false;
        let mut snapshot = Snapshot::default();
        Arc::make_mut(&mut snapshot.catalog.account).login =
            Some(Arc::new(agent_protocol::operations::AccountLogin {
                provider: ProviderKind::Codex,
                login_id: "fixture-login".into(),
                requires_code_submission: false,
                user_code: String::new(),
                verification_url: String::new(),
            }));
        state.receive(Update::Presentation {
            host: "host".into(),
            value: Presentation::project(Arc::new(snapshot.clone()), None),
        });
        assert!(
            state.account_locked(),
            "an active login keeps navigation locked between polls"
        );
        Arc::make_mut(&mut snapshot.catalog.account).login = None;
        let key = OperationKey::AccountLogin {
            provider: ProviderKind::Codex,
        };
        snapshot.operations.insert(
            key.clone(),
            OperationState {
                generation: 1,
                phase: OperationPhase::Running,
            },
        );
        state.receive(Update::Presentation {
            host: "host".into(),
            value: Presentation::project(Arc::new(snapshot.clone()), None),
        });
        state.provider = Some(ProviderKind::Claude);
        assert!(!state.account_locked());
        state.provider = Some(ProviderKind::Codex);
        assert!(state.account_locked());
        snapshot.operations.insert(
            key,
            OperationState {
                generation: 1,
                phase: OperationPhase::Failed {
                    message: "cancelled".into(),
                },
            },
        );
        state.receive(Update::Presentation {
            host: "host".into(),
            value: Presentation::project(Arc::new(snapshot), None),
        });
        assert!(
            !state.account_locked(),
            "failed login operations permit recovery and navigation"
        );
    }
}
