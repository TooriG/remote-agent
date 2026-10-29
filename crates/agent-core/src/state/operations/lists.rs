//! Conversation lists: recent chats, project pages, decorations and sub-agents.
use super::*;

pub use agent_protocol::operations::AddProject;

impl Operation for AddProject {
    rpc_operation!();
    fn invalidates(&self) -> bool {
        true
    }
    fn apply(self, snapshot: &mut Snapshot, cwd: Self::Output) -> Vec<Effect> {
        let (next, mut effects) = reduce_intent(snapshot, Intent::NewChat { cwd });
        *snapshot = next;
        effects.push(Effect::execute(ListSessions::new(
            (*snapshot.list_query).clone(),
        )));
        effects
    }
    fn stale(self, snapshot: &mut Snapshot, _: Self::Output) -> Vec<Effect> {
        vec![Effect::execute(ListSessions::new(
            (*snapshot.list_query).clone(),
        ))]
    }
}

pub use agent_protocol::operations::ListAgents;
pub use agent_protocol::operations::ListProjectSessions;
pub use agent_protocol::operations::ListSessions;

pub use agent_protocol::operations::ReadListDecorations;

fn decorations(
    scope: agent_protocol::operations::ListDecorationScope,
    projects: &[crate::models::Project],
    threads: &[crate::models::Thread],
) -> Option<Effect> {
    let projects: Vec<_> = projects
        .iter()
        .filter(|project| !project.roots.is_empty())
        .map(|project| {
            (
                project.id.clone(),
                project.roots.iter().map(|root| root.path.clone()).collect(),
            )
        })
        .collect();
    let threads: Vec<_> = threads
        .iter()
        .filter_map(|thread| {
            Some((
                thread.id.clone()?,
                thread.cwd.clone()?,
                thread.git_branch.clone(),
            ))
        })
        .collect();
    (!projects.is_empty() || !threads.is_empty()).then(|| {
        Effect::execute(ReadListDecorations {
            scope,
            projects,
            threads,
        })
    })
}

impl Operation for ReadListDecorations {
    rpc_operation!();
    const BACKGROUND: bool = true;
    fn key(&self) -> Option<OperationKey> {
        Some(OperationKey::ListDecorations {
            scope: self.scope.clone(),
        })
    }
    fn apply(self, snapshot: &mut Snapshot, output: Self::Output) -> Vec<Effect> {
        if let Some(list) = &mut snapshot.threads {
            for project in &mut Arc::make_mut(list).projects {
                if let Some(icon) = output.icons.get(&project.id) {
                    project.favicon_png = icon.clone();
                }
            }
        }
        for (id, status) in output.statuses {
            let source = self.threads.iter().find(|(source, _, _)| source == &id);
            for list in Arc::make_mut(&mut snapshot.project_threads)
                .values_mut()
                .chain(snapshot.threads.iter_mut())
            {
                for thread in &mut Arc::make_mut(list).data {
                    if thread.id.as_ref() == Some(&id)
                        && source.is_some_and(|(_, cwd, branch)| {
                            thread.cwd.as_ref() == Some(cwd) && &thread.git_branch == branch
                        })
                    {
                        thread.worktree_status = status;
                    }
                }
            }
        }
        Vec::new()
    }
}

pub(crate) fn refresh_agents(
    connected: bool,
    thread_id: Option<&crate::session::SessionRef>,
) -> Vec<Effect> {
    thread_id
        .filter(|_| connected)
        .map(|thread_id| {
            Effect::execute(ListAgents {
                thread_id: thread_id.clone(),
            })
        })
        .into_iter()
        .collect()
}

impl Operation for ListAgents {
    rpc_operation!();
    fn key(&self) -> Option<OperationKey> {
        Some(OperationKey::Agents {
            session: self.thread_id.clone(),
        })
    }
    fn scheduling(&self) -> Scheduling {
        Scheduling::LatestAgents(self.thread_id.clone())
    }
    fn apply(self, snapshot: &mut Snapshot, agents: Self::Output) -> Vec<Effect> {
        if snapshot.observed_agents.as_ref() != Some(&self.thread_id) {
            return Vec::new();
        }
        for agent in agents {
            let thread = Arc::make_mut(
                Arc::make_mut(&mut snapshot.conversations)
                    .entry(agent.id.clone())
                    .or_default(),
            );
            thread.id = Some(agent.id);
            thread.parent_id = Some(agent.parent_id);
            thread.name = agent.name;
            thread.status = agent.status;
        }
        Vec::new()
    }
}

fn merge_received(
    mut threads: crate::models::ThreadList,
    previous: Option<&crate::models::ThreadList>,
) -> crate::models::ThreadList {
    if let Some(previous) = previous {
        // Notifications carry complete list metadata. Preserve only changes
        // newer than this read; a later explicit refresh can remove old rows.
        for cached in previous
            .data
            .iter()
            .filter(|row| row.list_revision > threads.revision)
        {
            if let Some(row) = threads.data.iter_mut().find(|row| row.id == cached.id) {
                *row = cached.clone();
            } else {
                threads.data.push(cached.clone());
            }
        }
        let newer_projects: Vec<_> = previous
            .projects
            .iter()
            .filter(|project| project.list_revision > threads.revision)
            .cloned()
            .collect();
        threads
            .projects
            .retain(|project| !newer_projects.iter().any(|newer| newer.id == project.id));
        threads.projects.splice(0..0, newer_projects);
        for project in &mut threads.projects {
            if let Some(old) = previous
                .projects
                .iter()
                .find(|old| old.id == project.id && old.roots == project.roots)
            {
                project.favicon_png = old.favicon_png.clone();
            }
        }
        for thread in &mut threads.data {
            if let Some(old) = previous.data.iter().find(|old| {
                old.id == thread.id && old.cwd == thread.cwd && old.git_branch == thread.git_branch
            }) {
                thread.worktree_status = old.worktree_status;
            }
        }
    }
    if let Some(errors) = threads.provider_errors.as_ref()
        && let Some(previous) = previous
    {
        for cached in &previous.data {
            let Some(id) = cached.id.as_ref() else {
                continue;
            };
            let provider = cached.active_provider.unwrap_or(id.provider).key();
            if errors.contains_key(provider)
                && !threads
                    .data
                    .iter()
                    .any(|thread| thread.id.as_ref() == Some(id))
            {
                let mut cached = cached.clone();
                cached.list_stale = Some(true);
                cached.status = crate::models::SessionStatus::Unknown;
                threads.data.push(cached);
            }
        }
        for project in &previous.projects {
            if !threads
                .projects
                .iter()
                .any(|current| current.id == project.id)
                && threads
                    .data
                    .iter()
                    .any(|thread| thread.project_id.as_ref() == Some(&project.id))
            {
                threads.projects.push(project.clone());
            }
        }
    }
    threads
}

impl Operation for ListProjectSessions {
    const BACKGROUND: bool = true;
    rpc_operation!();
    fn key(&self) -> Option<OperationKey> {
        Some(OperationKey::ProjectList {
            project_id: self.project_id.clone(),
        })
    }
    fn scheduling(&self) -> Scheduling {
        Scheduling::LatestProject(self.project_id.clone())
    }
    fn apply(self, snapshot: &mut Snapshot, output: Self::Output) -> Vec<Effect> {
        if snapshot.threads.as_ref().is_none_or(|list| {
            !list
                .projects
                .iter()
                .any(|project| project.id == self.project_id)
        }) {
            return Vec::new();
        }
        let mut output = merge_received(
            output,
            snapshot
                .project_threads
                .get(&self.project_id)
                .map(AsRef::as_ref),
        );
        let (data, more) = page_rows(output.data, output.limit);
        output.data = data;
        output.has_more |= more;
        update_titles(snapshot, &output.data);
        if let Some(limit) =
            Arc::make_mut(&mut snapshot.expanded_projects).get_mut(&self.project_id)
        {
            *limit = output.limit;
        }
        let effect = decorations(
            agent_protocol::operations::ListDecorationScope::Project {
                project_id: self.project_id.clone(),
            },
            &[],
            &output.data,
        );
        Arc::make_mut(&mut snapshot.project_threads).insert(self.project_id, Arc::new(output));
        effect.into_iter().collect()
    }
}

fn update_titles(snapshot: &mut Snapshot, summaries: &[crate::models::Thread]) {
    for summary in summaries {
        if let Some(id) = &summary.id
            && snapshot.conversations.get(id).is_some_and(|thread| {
                thread.name != summary.name && thread.list_revision <= summary.list_revision
            })
            && let Some(thread) = shared_mut(&mut snapshot.conversations, id)
        {
            thread.name = summary.name.clone();
            thread.list_revision = summary.list_revision;
        }
    }
}

impl Operation for ListSessions {
    type Input = crate::models::ListQuery;
    type Output = crate::models::ThreadList;
    fn key(&self) -> Option<OperationKey> {
        Some(OperationKey::list(self.query.part))
    }
    fn scheduling(&self) -> Scheduling {
        Scheduling::LatestList(self.query.part)
    }
    fn capture(&self, snapshot: &Snapshot) -> Result<Self::Input, PeerError> {
        let mut query = self.query.clone();
        if query.project_limits.is_none() && !snapshot.project_threads.is_empty() {
            query.project_limits = Some(
                snapshot
                    .expanded_projects
                    .iter()
                    .map(|(id, limit)| (id.clone(), *limit))
                    .collect(),
            );
        }
        Ok(query)
    }
    async fn run(
        &self,
        query: Self::Input,
        context: &mut Execution<'_>,
    ) -> Result<Self::Output, PeerError> {
        context.call(&Self::new(query)).await
    }
    fn prepare(&mut self, snapshot: &mut Snapshot) -> Result<(), String> {
        if snapshot.list_query.search_term != self.query.search_term {
            snapshot.expanded_projects = Arc::default();
            snapshot.project_threads = Arc::default();
            self.query.project_limits = None;
            Arc::make_mut(&mut snapshot.operations).retain(|key, _| {
                !matches!(
                    key,
                    OperationKey::ProjectList { .. }
                        | OperationKey::SessionPage { .. }
                        | OperationKey::ListDecorations { .. }
                )
            });
        } else {
            let paging = self.query.limit > snapshot.list_query.limit
                || self.query.project_limit > snapshot.list_query.project_limit;
            self.query.project_limits = paging.then(Default::default);
            if !paging {
                self.query.part = None;
            }
        }
        let mut retained = self.query.clone();
        retained.project_limits = None;
        retained.part = None;
        snapshot.list_query = Arc::new(retained);
        Ok(())
    }
    fn apply(self, snapshot: &mut Snapshot, threads: Self::Output) -> Vec<Effect> {
        let current = &snapshot.list_query;
        if self.query.search_term != current.search_term
            || (self.query.part != Some(crate::models::ListPart::Projects)
                && self.query.limit != current.limit)
            || (self.query.part != Some(crate::models::ListPart::Chats)
                && self.query.project_limit != current.project_limit)
        {
            return Vec::new();
        }

        let mut threads = merge_received(threads, snapshot.threads.as_deref());
        let projects: Vec<_> = threads
            .projects
            .iter()
            .filter(|project| {
                self.query.part != Some(crate::models::ListPart::Projects)
                    || snapshot.threads.as_ref().is_none_or(|previous| {
                        !previous.projects.iter().any(|old| old.id == project.id)
                    })
            })
            .cloned()
            .collect();
        let decoration = decorations(
            agent_protocol::operations::ListDecorationScope::Root {
                part: self.query.part,
            },
            &projects,
            &threads.data,
        );
        if let Some(previous) = snapshot.threads.as_ref() {
            match self.query.part {
                Some(crate::models::ListPart::Projects) => {
                    threads.data = previous.data.clone();
                    threads.has_more = previous.has_more;
                }
                Some(crate::models::ListPart::Chats) => {
                    threads.projects = previous.projects.clone();
                    threads.has_more_projects = previous.has_more_projects;
                }
                None => {}
            }
        }
        threads
            .projects
            .truncate(snapshot.list_query.project_limit as usize);
        update_titles(snapshot, &threads.data);
        let visible: std::collections::HashSet<_> =
            threads.projects.iter().map(|p| p.id.as_str()).collect();
        Arc::make_mut(&mut snapshot.expanded_projects)
            .retain(|id, _| visible.contains(id.as_str()));
        if !snapshot.list_query.search_term.trim().is_empty() {
            for id in &visible {
                Arc::make_mut(&mut snapshot.expanded_projects)
                    .entry((*id).to_owned())
                    .or_insert(5);
            }
        }
        let pages = Arc::make_mut(&mut snapshot.project_threads);
        pages.retain(|id, _| visible.contains(id.as_str()));
        for (id, page) in &threads.project_pages {
            if pages
                .get(id)
                .is_some_and(|current| current.limit > page.limit)
            {
                continue;
            }
            let mut received = merge_received(
                crate::models::ThreadList {
                    revision: threads.revision,
                    data: threads
                        .data
                        .iter()
                        .filter(|thread| thread.project_id.as_ref() == Some(id))
                        .cloned()
                        .collect(),
                    projects: Vec::new(),
                    has_more: page.has_more,
                    has_more_projects: false,
                    limit: page.limit,
                    project_pages: Default::default(),
                    provider_errors: threads.provider_errors.clone(),
                },
                pages.get(id).map(AsRef::as_ref),
            );
            let (data, more) = page_rows(received.data, received.limit);
            received.data = data;
            received.has_more |= more;
            pages.insert(id.clone(), Arc::new(received));
        }
        threads.data.retain(|thread| {
            thread
                .project_id
                .as_ref()
                .is_none_or(|id| !visible.contains(id.as_str()))
        });
        threads.project_pages.clear();
        let (data, more) = page_rows(threads.data, threads.limit);
        threads.data = data;
        threads.has_more |= more;
        snapshot.threads = Some(Arc::new(threads));
        decoration
            .into_iter()
            .chain(
                snapshot
                    .expanded_projects
                    .iter()
                    .filter(|(id, _)| {
                        snapshot.connected && !snapshot.project_threads.contains_key(*id)
                    })
                    .map(|(id, limit)| {
                        Effect::execute(ListProjectSessions {
                            project_id: id.clone(),
                            limit: *limit,
                            search_term: snapshot.list_query.search_term.clone(),
                        })
                    }),
            )
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{ProviderKind, SessionRef};

    #[allow(dead_code)]
    mod host_fixture {
        include!("../../../tests/support/host.rs");
    }

    #[test]
    fn recent_paging_does_not_read_unopened_project_pages() {
        let mut snapshot = Snapshot {
            connected: true,
            recent_chats: true,
            ..Default::default()
        };
        let effects = ListSessions::new(Default::default()).apply(
            &mut snapshot,
            serde_json::from_value(serde_json::json!({
                "data":[],"projects":[{"id":"p","name":"P","roots":[]}],
                "hasMore":false,"hasMoreProjects":false,"projectPages":{},"limit":5
            }))
            .unwrap(),
        );
        assert!(effects.is_empty());
        let mut operation = ListSessions::new(crate::models::ListQuery {
            limit: 15,
            part: Some(crate::models::ListPart::Chats),
            ..Default::default()
        });
        operation.prepare(&mut snapshot).unwrap();
        assert!(
            operation
                .capture(&snapshot)
                .unwrap()
                .project_limits
                .unwrap()
                .is_empty()
        );
        let effects = operation.apply(&mut snapshot, serde_json::from_value(serde_json::json!({
            "data":[],"projects":[{"id":"p","name":"P","roots":[]},{"id":"q","name":"Q","roots":[]}],
            "hasMore":false,"hasMoreProjects":false,"projectPages":{},"limit":15
        })).unwrap());
        assert!(effects.is_empty());
        assert!(snapshot.expanded_projects.is_empty());
        assert!(
            snapshot
                .project_threads
                .values()
                .all(|page| page.limit == 0 && page.data.is_empty())
        );
    }

    #[test]
    fn list_and_conversation_reads_cannot_overwrite_a_newer_name_notice() {
        let id =
            crate::session::SessionRef::new(crate::session::ProviderKind::Codex, "task".into())
                .unwrap();
        let old: crate::models::ThreadList = serde_json::from_value(serde_json::json!({"data":[{"id":id,"name":"Old","updatedAt":1}],"projects":[],"limit":5,"hasMore":false,"hasMoreProjects":false,"projectPages":{}})).unwrap();
        let mut state = Snapshot {
            threads: Some(Arc::new(old.clone())),
            conversations: Arc::new([(id.clone(), Arc::new(old.data[0].clone()))].into()),
            ..Default::default()
        };
        let (next, effects) = super::super::super::notifications::notification(
            &state,
            crate::protocol::Notification::SessionRenamed {
                session: id.clone(),
                name: Some("New".into()),
                revision: 2,
            },
        );
        assert!(effects.is_empty());
        state = next;
        let (next, effects) = super::super::super::notifications::notification(
            &state,
            crate::protocol::Notification::SessionUpdated {
                thread: Thread {
                    id: Some(SessionRef::new(ProviderKind::Codex, "created".into()).unwrap()),
                    project_id: crate::models::ProjectMembership::Assigned("new-project".into()),
                    list_revision: 2,
                    ..Default::default()
                }
                .into(),
                project: Some(crate::models::Project {
                    id: "new-project".into(),
                    name: "Created project".into(),
                    list_revision: 2,
                    ..Default::default()
                }),
            },
        );
        assert!(effects.is_empty());
        state = next;
        ListSessions::new(Default::default()).apply(&mut state, old.clone());
        assert_eq!(
            state.threads.as_ref().unwrap().projects[0].id,
            "new-project"
        );
        assert_eq!(
            state.threads.as_ref().unwrap().data[0].name.as_deref(),
            Some("New")
        );
        refresh_thread(&mut state, old.data[0].clone());
        assert_eq!(state.conversations[&id].name.as_deref(), Some("New"));
        let mut authoritative = old;
        authoritative.revision = 2;
        authoritative.data.clear();
        ListSessions::new(Default::default()).apply(&mut state, authoritative);
        assert!(
            state.threads.as_ref().unwrap().data.is_empty(),
            "later refresh can remove a deleted task"
        );
        assert!(
            state.threads.as_ref().unwrap().projects.is_empty(),
            "later refresh can remove a project"
        );
    }

    #[test]
    fn reconnect_retains_received_pages_and_accepts_a_restarted_host_revision() {
        let id = SessionRef::new(ProviderKind::Codex, "task".into()).unwrap();
        let page = serde_json::from_value(serde_json::json!({"revision":99,"data":[{"id":id,"name":"Before restart","listRevision":99}],"projects":[],"limit":15,"hasMore":false,"hasMoreProjects":false,"projectPages":{}})).unwrap();
        let snapshot = Snapshot {
            threads: Some(Arc::new(page)),
            ..Default::default()
        };
        let (connected, effects) = reduce(&snapshot, Event::Connected);
        assert_eq!(connected.threads.as_ref().unwrap().limit, 15);
        assert_eq!(
            effects.len(),
            3,
            "status, models, and accounts only; no list read"
        );
        let (renamed, effects) = super::super::super::notifications::notification(
            &connected,
            crate::protocol::Notification::SessionRenamed {
                session: id,
                name: Some("After restart".into()),
                revision: 1,
            },
        );
        assert!(effects.is_empty());
        assert_eq!(
            renamed.threads.as_ref().unwrap().data[0].name.as_deref(),
            Some("After restart")
        );
    }

    #[test]
    fn recent_pages_more_and_refresh_have_independent_read_targets_and_counts() {
        use crate::models::{ListPage, ListPart, Project, Thread, ThreadList};
        let project = |id: &str| Project {
            id: id.into(),
            name: id.into(),
            ..Default::default()
        };
        let row = |id: &str| Thread {
            id: Some(SessionRef::new(ProviderKind::Codex, id.into()).unwrap()),
            project_id: crate::models::ProjectMembership::Assigned(id.into()),
            ..Default::default()
        };
        let root = ThreadList {
            revision: 0,
            data: (0..5).map(|i| row(&format!("p{i}"))).collect(),
            projects: (0..5).map(|i| project(&format!("p{i}"))).collect(),
            limit: 5,
            has_more: false,
            has_more_projects: true,
            project_pages: (0..5)
                .map(|i| {
                    (
                        format!("p{i}"),
                        ListPage {
                            limit: 5,
                            has_more: false,
                        },
                    )
                })
                .collect(),
            provider_errors: None,
        };
        let mut state = Snapshot {
            connected: true,
            ..Default::default()
        };
        assert!(
            ListSessions::new(Default::default())
                .apply(&mut state, root)
                .is_empty()
        );
        assert_eq!(state.project_threads.len(), 5);
        assert!(state.threads.as_ref().unwrap().data.is_empty());
        let (mut state, effects) = reduce_intent(
            &state,
            Intent::SetProjectExpanded {
                project_id: "p0".into(),
                expanded: true,
            },
        );
        assert!(effects.is_empty(), "initial pages are already received");
        let received = state.project_threads["p1"].clone();
        ListProjectSessions {
            project_id: "p0".into(),
            limit: 25,
            search_term: String::new(),
        }
        .apply(
            &mut state,
            ThreadList {
                revision: 0,
                data: vec![row("p0")],
                projects: vec![],
                limit: 25,
                has_more: false,
                has_more_projects: false,
                project_pages: Default::default(),
                provider_errors: None,
            },
        );
        let mut query = (*state.list_query).clone();
        query.project_limit = 15;
        query.part = Some(ListPart::Projects);
        let mut more = ListSessions::new(query);
        more.prepare(&mut state).unwrap();
        assert_eq!(
            more.capture(&state).unwrap().project_limits,
            Some(Default::default())
        );
        more.apply(
            &mut state,
            ThreadList {
                revision: 0,
                data: vec![],
                projects: (0..15).map(|i| project(&format!("p{i}"))).collect(),
                limit: 5,
                has_more: false,
                has_more_projects: true,
                project_pages: Default::default(),
                provider_errors: None,
            },
        );
        assert!(Arc::ptr_eq(&received, &state.project_threads["p1"]));
        assert!(!state.project_threads.contains_key("p14"));
        let mut refresh = ListSessions::new((*state.list_query).clone());
        refresh.prepare(&mut state).unwrap();
        assert_eq!(
            refresh.capture(&state).unwrap().project_limits,
            Some([("p0".into(), 25)].into())
        );
        let (state, _) = reduce_intent(
            &state,
            Intent::SetProjectExpanded {
                project_id: "p0".into(),
                expanded: false,
            },
        );
        let (state, effects) = reduce_intent(
            &state,
            Intent::SetProjectExpanded {
                project_id: "p0".into(),
                expanded: true,
            },
        );
        assert!(effects.is_empty());
        assert_eq!(
            state.expanded_projects["p0"], 25,
            "short pages preserve their requested count"
        );
        let (_, effects) = reduce_intent(
            &state,
            Intent::SetProjectExpanded {
                project_id: "p14".into(),
                expanded: true,
            },
        );
        assert_eq!(
            effects.len(),
            1,
            "only an unread older project needs its first page"
        );
    }

    #[tokio::test]
    async fn project_reads_survive_task_navigation_and_report_failure_only_in_their_section() {
        tokio::time::timeout(std::time::Duration::from_secs(30), async {
            use serde_json::json;
            async fn read(reader: &mut host_fixture::Reader) -> host_fixture::Request {
                tokio::time::timeout(std::time::Duration::from_secs(30), reader.read_request())
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap()
            }
            let initial = Snapshot {
                list_query: Arc::new(crate::models::ListQuery {
                    search_term: "Archive".into(),
                    ..Default::default()
                }),
                ..Default::default()
            };
            let (peer, mut reader, writer) = host_fixture::connect(&initial).await;
            let store = crate::store::Store::new(peer, initial);
            let page =
                json!({"data":[],"projects":[{"id":"old","name":"Old","roots":[]}],"hasMore":true,"hasMoreProjects":false,"projectPages":{},"limit":5});
            for _ in 0..4 {
                let request = read(&mut reader).await;
                let result = match request["method"].as_str().unwrap() {
                    "host/session/list" => page.clone(),
                    "host/model/list" => json!({"data":[],"nextCursor":null}),
                    "host/account/list" => json!({"accounts":[],"selected":{}}),
                    "host/taskActivity/read" => json!({"revision":0,"statuses":[],"unread":[],"display":agent_protocol::live_activity::TaskActivitySummary::default().display()}),
                    method => panic!("unexpected {method}"),
                };
                writer
                    .reply(&request, json!({"result":result}))
                    .await
                    .unwrap();
            }
            let mut updates = store.subscribe();
            while {
                let snapshot = updates.borrow_and_update();
                snapshot.threads.is_none()
                    || snapshot.operations.keys().any(|key| !matches!(key, OperationKey::ProjectList { .. } | OperationKey::SessionPage { .. } | OperationKey::ListDecorations { .. }))
            } {
                updates.changed().await.unwrap();
            }
            assert!(store.snapshot().thread_list().unwrap().projects[0].expanded);
            let request = read(&mut reader).await;
            assert_eq!(request["method"], "host/project/sessions");
            assert_eq!(request["params"], json!({"projectId":"old","limit":5,"searchTerm":"Archive"}));
            assert!(store.snapshot().thread_list().unwrap().projects[0].loading);
            assert!(
                !store
                    .snapshot()
                    .operations
                    .contains_key(&OperationKey::SessionList)
            );
            let recent = store.snapshot().threads.clone().unwrap();
            store.dispatch(Intent::ShowThreadList).await.unwrap();
            writer
                .reply(&request, json!({"result":page}))
                .await
                .unwrap();
            while {
                let snapshot = updates.borrow_and_update();
                !snapshot.project_threads.contains_key("old")
            }
            {
                updates.changed().await.unwrap();
            }
            assert!(Arc::ptr_eq(
                &recent,
                store.snapshot().threads.as_ref().unwrap()
            ));
            store
                .dispatch(Intent::RefreshProject {
                    project_id: "old".into(),
                })
                .await
                .unwrap();
            let request = read(&mut reader).await;
            assert_eq!(request["params"]["searchTerm"], "Archive");
            writer.reply(&request,json!({"error":{"code":"provider_failed","message":"fixture unavailable","delivery":"notSent"}})).await.unwrap();
            while {
                let snapshot = updates.borrow_and_update();
                snapshot.thread_list().unwrap().projects[0].loading
            } {
                updates.changed().await.unwrap();
            }
            assert!(store.snapshot().error.is_none());
            assert!(
                store.snapshot().thread_list().unwrap().projects[0]
                    .error
                    .as_deref()
                    .unwrap()
                    .contains("fixture unavailable")
            );
            store
                .dispatch(Intent::SetProjectExpanded {
                    project_id: "old".into(),
                    expanded: false,
                })
                .await
                .unwrap();
            let received = store.snapshot().project_threads["old"].clone();
            assert!(
                store.snapshot().thread_list().unwrap().projects[0]
                    .error
                    .is_none()
            );
            store.dispatch(Intent::SetProjectExpanded {
                project_id: "old".into(), expanded: true,
            }).await.unwrap();
            assert!(Arc::ptr_eq(&received, &store.snapshot().project_threads["old"]));
            assert!(!store.snapshot().thread_list().unwrap().projects[0].loading);
            assert!(tokio::time::timeout(std::time::Duration::from_millis(50), reader.read_request()).await.is_err(),
                "a received page must not be read again on reopening");
            store.close().await.unwrap();
        })
        .await
        .expect("project lifecycle deadline");
    }

    #[tokio::test]
    async fn fleet_reads_coalesce_and_do_not_block_the_root_list_or_publish_after_closing() {
        async fn read(reader: &mut host_fixture::Reader) -> host_fixture::Request {
            tokio::time::timeout(std::time::Duration::from_secs(30), reader.read_request())
                .await
                .expect("fleet request was not sent")
                .unwrap()
                .unwrap()
        }
        use serde_json::json;
        let parent = SessionRef::new(ProviderKind::Codex, "parent".into()).unwrap();
        let snapshot = Snapshot {
            navigation: Arc::new(Navigation {
                thread_id: Some(parent.clone()),
                ..Default::default()
            }),
            conversations: Arc::new(
                [(
                    parent.clone(),
                    Arc::new(Thread {
                        id: Some(parent.clone()),
                        ..Default::default()
                    }),
                )]
                .into(),
            ),
            ..Default::default()
        };
        let (peer, mut reader, writer) = host_fixture::connect(&snapshot).await;
        let store = crate::store::Store::new(peer, snapshot);
        let empty_list = json!({"data":[],"projects":[],"hasMore":false,"hasMoreProjects":false,"projectPages":{},"limit":5});
        for _ in 0..5 {
            let request = read(&mut reader).await;
            let result = match request["method"].as_str().unwrap() {
                "host/session/list" => empty_list.clone(),
                "host/account/list" => json!({"accounts":[],"selected":{}}),
                "host/taskActivity/read" => {
                    json!({"revision":0,"statuses":[],"unread":[],"display":agent_protocol::live_activity::TaskActivitySummary::default().display()})
                }
                "host/model/list" => json!({"data":[],"nextCursor":null}),
                "host/session/open" => json!({"thread":{"id":parent}}),
                method => panic!("unexpected initial request {method}"),
            };
            writer
                .reply(&request, json!({"result":result}))
                .await
                .unwrap();
        }
        let mut updates = store.subscribe();
        loop {
            let ready = {
                let snapshot = updates.borrow_and_update();
                snapshot.threads.is_some()
                    && snapshot.account.accounts.is_some()
                    && snapshot.subscriptions.contains_key(&parent)
                    && snapshot.operations.is_empty()
            };
            if ready {
                break;
            }
            updates.changed().await.unwrap();
        }
        let before = store.snapshot();
        let first = store.dispatch(Intent::WatchAgents {
            thread_id: Some(parent.clone()),
        });
        let request = read(&mut reader).await;
        assert_eq!(request["method"], "host/session/agents");
        assert_eq!(request["params"], json!({"threadId":parent}));
        let mut receipts = Vec::new();
        for _ in 0..20 {
            store
                .dispatch(Intent::WatchAgents { thread_id: None })
                .await
                .unwrap();
            receipts.push(store.dispatch(Intent::WatchAgents {
                thread_id: Some(parent.clone()),
            }));
        }
        let listing = store.dispatch(Intent::ListSessions(ListSessions::new(Default::default())));
        let list_request = read(&mut reader).await;
        assert_eq!(
            list_request["method"], "host/session/list",
            "fleet requests must coalesce while the first response is pending"
        );
        writer
            .reply(&list_request, json!({"result":empty_list}))
            .await
            .unwrap();
        listing.await.unwrap();
        writer.reply(&request, json!({"result":[{"id":{"provider":"codex","id":"obsolete"},"parentId":parent,"name":"Old","status":"running"}]})).await.unwrap();
        first.await.unwrap();
        assert!(
            store.snapshot().agent_panel().is_empty(),
            "a superseded fleet response must not publish stale rows"
        );
        let mut latest = read(&mut reader).await;
        if latest["method"] == "host/taskActivity/read" {
            writer.reply(&latest, json!({"result":{"revision":0,"statuses":[],"unread":[],"display":agent_protocol::live_activity::TaskActivitySummary::default().display()}})).await.unwrap();
            latest = read(&mut reader).await;
        }
        assert_eq!(latest["method"], "host/session/agents");
        store
            .dispatch(Intent::WatchAgents { thread_id: None })
            .await
            .unwrap();
        writer.reply(&latest, json!({"result":[{"id":{"provider":"codex","id":"child"},"parentId":parent,"name":"Review","status":"running"}]})).await.unwrap();
        for receipt in receipts {
            receipt.await.unwrap();
        }
        assert!(
            store.snapshot().agent_panel().is_empty(),
            "closing the panel discards an in-flight fleet response"
        );
        let watching = store.dispatch(Intent::WatchAgents {
            thread_id: Some(parent.clone()),
        });
        let latest = read(&mut reader).await;
        writer.reply(&latest, json!({"result":[{"id":{"provider":"codex","id":"child"},"parentId":parent,"name":"Review","status":"running"}]})).await.unwrap();
        watching.await.unwrap();
        let after = store.snapshot();
        assert_eq!(after.agent_panel()[0].title, "Review");
        assert!(after.agent_panel()[0].active);
        assert_eq!(after.navigation, before.navigation);
        assert_eq!(after.drafts, before.drafts);
        store
            .dispatch(Intent::WatchAgents { thread_id: None })
            .await
            .unwrap();
        let first = store.dispatch(Intent::WatchAgents {
            thread_id: Some(parent.clone()),
        });
        let request = read(&mut reader).await;
        store
            .dispatch(Intent::WatchAgents { thread_id: None })
            .await
            .unwrap();
        let queued = store.dispatch(Intent::WatchAgents {
            thread_id: Some(parent),
        });
        store
            .dispatch(Intent::WatchAgents { thread_id: None })
            .await
            .unwrap();
        writer.reply(&request, json!({"result":[]})).await.unwrap();
        first.await.unwrap();
        queued.await.unwrap();
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), reader.read_request())
                .await
                .is_err(),
            "closing the panel must also discard the queued read before sending it"
        );
        store.close().await.unwrap();
    }

    #[rstest::rstest]
    #[case(0)]
    #[case(5)]
    #[case(15)]
    fn project_disclosure_keeps_received_rows_and_finishes_reads_while_closed(
        #[case] count: usize,
    ) {
        let root = |title: &str| {
            serde_json::from_value::<crate::models::ThreadList>(serde_json::json!({
                "data":[{"id":{"provider":"codex","id":title},"name":title}],
                "projects":[{"id":"old","name":"Old","roots":[]}],
                "hasMore":false,"hasMoreProjects":false,"projectPages":{},"limit":count.max(5) as u32
            }))
            .unwrap()
        };
        let project = |title: &str| {
            serde_json::from_value::<crate::models::ThreadList>(serde_json::json!({
                "data":(0..count).map(|index| serde_json::json!({"id":{"provider":"codex","id":format!("{title}-{index}")},"projectId":"old","name":title})).collect::<Vec<_>>(),
                "projects":[{"id":"old","name":"Old","roots":[]}],
                "hasMore":false,"hasMoreProjects":false,"projectPages":{},"limit":count.max(5) as u32
            }))
            .unwrap()
        };
        let mut snapshot = Snapshot {
            connected: true,
            ..Default::default()
        };
        ListSessions::new(Default::default()).apply(&mut snapshot, root("chat"));
        let selected = SessionRef::new(ProviderKind::Codex, "selected".into()).unwrap();
        Arc::make_mut(&mut snapshot.navigation).thread_id = Some(selected.clone());
        Arc::make_mut(&mut snapshot.drafts).insert(
            selected.clone().into(),
            Arc::new(Draft {
                text: "Keep this draft".into(),
                ..Default::default()
            }),
        );
        let (opened, effects) = reduce_intent(
            &snapshot,
            Intent::SetProjectExpanded {
                project_id: "old".into(),
                expanded: true,
            },
        );
        assert_eq!(effects.len(), 1);
        let mut published = opened;
        let root_before = published.threads.clone().unwrap();
        let refresh_effects =
            ListSessions::new(Default::default()).apply(&mut published, root("new-chat"));
        assert_eq!(refresh_effects.len(), 1);
        assert!(!Arc::ptr_eq(
            &root_before,
            published.threads.as_ref().unwrap()
        ));
        if count > 5 {
            (published, _) = reduce_intent(
                &published,
                Intent::ExpandThreadList {
                    project_id: Some("old".into()),
                },
            );
        }
        let limit = published.expanded_projects["old"];
        ListProjectSessions {
            project_id: "old".into(),
            limit,
            search_term: String::new(),
        }
        .apply(&mut published, project("old-task"));
        assert_eq!(published.project_threads["old"].data.len(), count);
        let received = published.project_threads["old"].clone();
        let navigation = published.navigation.clone();
        let drafts = published.drafts.clone();
        let (mut closed, effects) = reduce_intent(
            &published,
            Intent::SetProjectExpanded {
                project_id: "old".into(),
                expanded: false,
            },
        );
        assert!(effects.is_empty());
        ListProjectSessions {
            project_id: "old".into(),
            limit,
            search_term: String::new(),
        }
        .apply(&mut closed, project("late-task"));
        assert!(!Arc::ptr_eq(&received, &closed.project_threads["old"]));
        assert!(
            closed.project_threads["old"]
                .data
                .iter()
                .all(|row| row.name.as_deref() == Some("late-task"))
        );
        assert!(closed.thread_list().unwrap().projects[0].threads.is_empty());
        let (reopened, effects) = reduce_intent(
            &closed,
            Intent::SetProjectExpanded {
                project_id: "old".into(),
                expanded: true,
            },
        );
        assert!(effects.is_empty());
        assert_eq!(
            reopened.thread_list().unwrap().projects[0].threads.len(),
            count
        );
        assert_eq!(reopened.expanded_projects["old"], count.max(5) as u32);
        assert!(!reopened.thread_list().unwrap().projects[0].loading);
        assert_eq!(closed.navigation, navigation);
        assert_eq!(closed.drafts, drafts);
        ListSessions::new(crate::models::ListQuery {
            search_term: "other".into(),
            ..Default::default()
        })
        .prepare(&mut closed)
        .unwrap();
        assert!(closed.project_threads.is_empty());
    }

    #[test]
    fn searching_root_auto_expands_visible_projects_and_queues_scoped_reads() {
        let page = || {
            serde_json::from_value::<crate::models::ThreadList>(serde_json::json!({
                "data":[],
                "projects":[{"id":"matching","name":"Matching","roots":[]}],
                "hasMore":false,"hasMoreProjects":false,"projectPages":{},"limit":5
            }))
            .unwrap()
        };
        let mut snapshot = Snapshot {
            connected: true,
            ..Default::default()
        };
        let mut search = ListSessions::new(crate::models::ListQuery {
            search_term: "needle".into(),
            ..Default::default()
        });
        search.prepare(&mut snapshot).unwrap();
        let effects = search.apply(&mut snapshot, page());
        assert_eq!(snapshot.expanded_projects["matching"], 5);
        assert_eq!(effects.len(), 1);
        assert!(snapshot.thread_list().unwrap().projects[0].expanded);

        let mut clear = ListSessions::new(Default::default());
        clear.prepare(&mut snapshot).unwrap();
        clear.apply(&mut snapshot, page());
        assert!(snapshot.expanded_projects.is_empty());
        assert!(!snapshot.thread_list().unwrap().projects[0].expanded);
    }

    #[test]
    fn root_pagination_keeps_loaded_project_pages_without_refetching_them() {
        let root_page = |title: &str| {
            serde_json::from_value::<crate::models::ThreadList>(serde_json::json!({
                "data":[{"id":{"provider":"codex","id":title},"name":title}],
                "projects":[{"id":"project","name":"Project","roots":[]}],
                "hasMore":false,"hasMoreProjects":false,"projectPages":{},"limit":5
            }))
            .unwrap()
        };
        let project_page = || {
            serde_json::from_value::<crate::models::ThreadList>(serde_json::json!({
                "data":[{"id":{"provider":"codex","id":"project-task"},"projectId":"project","name":"Project task"}],
                "projects":[{"id":"project","name":"Project","roots":[]}],
                "hasMore":true,"hasMoreProjects":false,"projectPages":{},"limit":5
            }))
            .unwrap()
        };
        let mut snapshot = Snapshot {
            connected: true,
            ..Default::default()
        };
        ListSessions::new(Default::default()).apply(&mut snapshot, root_page("initial"));
        let (opened, effects) = reduce_intent(
            &snapshot,
            Intent::SetProjectExpanded {
                project_id: "project".into(),
                expanded: true,
            },
        );
        assert_eq!(effects.len(), 1);
        let mut opened = opened;
        ListProjectSessions {
            project_id: "project".into(),
            limit: 5,
            search_term: String::new(),
        }
        .apply(&mut opened, project_page());
        let loaded = opened.project_threads["project"].clone();
        let (mut paginating, effects) =
            reduce_intent(&opened, Intent::ExpandThreadList { project_id: None });
        assert_eq!(effects.len(), 1);
        let root_query = (*paginating.list_query).clone();
        let root_effects = ListSessions::new(root_query).apply(&mut paginating, root_page("next"));
        assert!(root_effects.is_empty());
        assert!(Arc::ptr_eq(&loaded, &paginating.project_threads["project"]));
        assert_eq!(
            paginating.project_threads["project"].data[0]
                .name
                .as_deref(),
            Some("Project task")
        );
    }

    #[test]
    fn late_root_page_keeps_project_pagination_and_its_pending_request() {
        let mut snapshot = Snapshot::default();
        let page: crate::models::ThreadList = serde_json::from_value(serde_json::json!({
            "data":[{"id":{"provider":"codex","id":"old"},"projectId":"project"}],
            "projects":[{"id":"project","name":"Project","roots":[]}],
            "hasMore":false,"hasMoreProjects":false,"limit":5,
            "projectPages":{"project":{"limit":5,"hasMore":true}}
        }))
        .unwrap();
        let loaded = Arc::new(crate::models::ThreadList {
            revision: 0,
            limit: 15,
            data: vec![crate::models::Thread {
                id: Some(SessionRef::new(ProviderKind::Codex, "new".into()).unwrap()),
                project_id: crate::models::ProjectMembership::Assigned("project".into()),
                ..Default::default()
            }],
            projects: Vec::new(),
            has_more: false,
            has_more_projects: false,
            project_pages: Default::default(),
            provider_errors: None,
        });
        Arc::make_mut(&mut snapshot.project_threads).insert("project".into(), loaded.clone());
        let key = OperationKey::ProjectList {
            project_id: "project".into(),
        };
        Arc::make_mut(&mut snapshot.operations).insert(
            key.clone(),
            OperationState {
                generation: 1,
                phase: OperationPhase::Running,
            },
        );
        ListSessions::new(Default::default()).apply(&mut snapshot, page.clone());
        assert!(
            Arc::ptr_eq(&loaded, &snapshot.project_threads["project"]),
            "A delayed five-row root response cannot replace the fifteen-row project page"
        );
        assert!(
            snapshot.operations.contains_key(&key),
            "Only the project request can finish its own pending pagination"
        );
        let mut refresh = page;
        refresh.project_pages.get_mut("project").unwrap().limit = 15;
        ListSessions::new(Default::default()).apply(&mut snapshot, refresh);
        assert_eq!(
            snapshot.project_threads["project"].data[0]
                .id
                .as_ref()
                .unwrap()
                .id,
            "old",
            "Refreshing the same requested count must replace stale rows"
        );
        assert!(snapshot.operations.contains_key(&key));
    }

    #[test]
    fn agents_are_loaded_only_while_the_selected_fleet_is_observed() {
        let parent = SessionRef::new(ProviderKind::Codex, "parent".into()).unwrap();
        let other = SessionRef::new(ProviderKind::Codex, "other".into()).unwrap();
        let mut snapshot = Snapshot {
            connected: true,
            ..Default::default()
        };
        Arc::make_mut(&mut snapshot.navigation).thread_id = Some(parent.clone());
        Arc::make_mut(&mut snapshot.drafts).insert(
            parent.clone().into(),
            Arc::new(Draft {
                text: "Keep my draft".into(),
                ..Default::default()
            }),
        );
        let (_, effects) = reduce(&snapshot, Event::Connected);
        assert_eq!(
            effects.len(),
            5,
            "reconnecting loads the task aggregate without fetching an unobserved fleet"
        );
        let (ignored, effects) = reduce_intent(
            &snapshot,
            Intent::WatchAgents {
                thread_id: Some(other),
            },
        );
        assert!(effects.is_empty());
        assert!(ignored.observed_agents.is_none());
        let (watching, effects) = reduce_intent(
            &snapshot,
            Intent::WatchAgents {
                thread_id: Some(parent.clone()),
            },
        );
        assert_eq!(effects.len(), 1);
        assert_eq!(watching.observed_agents, Some(parent.clone()));
        assert_eq!(watching.navigation, snapshot.navigation);
        assert_eq!(watching.drafts, snapshot.drafts);
        let (_, effects) = reduce_intent(
            &watching,
            Intent::WatchAgents {
                thread_id: Some(parent.clone()),
            },
        );
        assert!(
            effects.is_empty(),
            "rendering the same panel must not refetch"
        );
        let (_, effects) = reduce(&watching, Event::Connected);
        assert_eq!(
            effects.len(),
            6,
            "reconnect refreshes only the observed fleet"
        );
        let (closed, effects) = reduce_intent(&watching, Intent::WatchAgents { thread_id: None });
        assert!(effects.is_empty());
        assert!(closed.observed_agents.is_none());
        let mut offline = closed.clone();
        offline.connected = false;
        let (offline, effects) = reduce_intent(
            &offline,
            Intent::WatchAgents {
                thread_id: Some(parent),
            },
        );
        assert!(effects.is_empty());
        assert!(
            offline.observed_agents.is_some(),
            "offline observations survive until reconnect"
        );
    }

    #[test]
    fn fleet_metadata_preserves_history_and_ignores_results_after_closing() {
        let parent = SessionRef::new(ProviderKind::Codex, "parent".into()).unwrap();
        let child = SessionRef::new(ProviderKind::Codex, "child".into()).unwrap();
        let history = Arc::new(Thread {
            id: Some(child.clone()),
            turns: Some(vec![Arc::new(crate::models::Turn {
                id: "history".into(),
                ..Default::default()
            })]),
            ..Default::default()
        });
        let mut snapshot = Snapshot {
            observed_agents: Some(parent.clone()),
            conversations: Arc::new([(child.clone(), history.clone())].into()),
            ..Default::default()
        };
        let observations = vec![agent_protocol::models::AgentObservation {
            id: child.clone(),
            parent_id: parent.clone(),
            name: Some("Review".into()),
            status: crate::models::SessionStatus::Running,
        }];
        ListAgents {
            thread_id: parent.clone(),
        }
        .apply(&mut snapshot, observations.clone());
        let thread = &snapshot.conversations[&child];
        assert_eq!(thread.parent_id, Some(parent.clone()));
        assert_eq!(thread.name.as_deref(), Some("Review"));
        assert_eq!(thread.status, crate::models::SessionStatus::Running);
        assert_eq!(thread.turns, history.turns);
        snapshot.observed_agents = None;
        let conversations = snapshot.conversations.clone();
        ListAgents { thread_id: parent }.apply(&mut snapshot, observations);
        assert!(Arc::ptr_eq(&snapshot.conversations, &conversations));
    }
}
