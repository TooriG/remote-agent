//! Conversation lists: native titles merged with registered projects and Host conversations.
use super::*;

impl HostRpcService {
    pub(super) async fn list_sessions(
        &self,
        params: &op::ListSessions,
    ) -> Result<agent_protocol::models::ThreadList, Failure> {
        let started = std::time::Instant::now();
        let result = self
            .host_title_list(
                &params.query.search_term,
                crate::projects::titles::TitleQuery::Root {
                    limit: params.query.limit,
                    project_limit: params.query.project_limit,
                    project_limits: params.query.project_limits.as_ref(),
                    part: params.query.part,
                    searching: !params.query.search_term.trim().is_empty(),
                },
            )
            .await;
        tracing::info!(target: "agent", operation = "host.thread.list.performance",
            message = %format_args!("elapsed_ms={} success={}", started.elapsed().as_millis(), result.is_ok()));
        result
    }

    pub(super) async fn list_project_sessions(
        &self,
        params: &op::ListProjectSessions,
    ) -> Result<agent_protocol::models::ThreadList, Failure> {
        self.host_title_list(
            &params.search_term,
            crate::projects::titles::TitleQuery::Project {
                id: &params.project_id,
                limit: params.limit,
                searching: !params.search_term.trim().is_empty(),
            },
        )
        .await
    }

    pub(super) async fn project_snapshot(
        &self,
    ) -> Result<crate::projects::state::Snapshot, Failure> {
        self.inner
            .projects
            .load()
            .await
            .map_err(|error| Failure::new("project_state_unavailable", error))
    }
    pub(super) async fn host_title_list(
        &self,
        search: &str,
        query: crate::projects::titles::TitleQuery<'_>,
    ) -> Result<agent_protocol::models::ThreadList, Failure> {
        let revision = self.inner.router.list_revision();
        let agents = self.inner.providers.all();
        // One title walk supplies each requested quota plus lookahead.
        let (first_page_size, later_page_size) = match query {
            crate::projects::titles::TitleQuery::Root {
                limit,
                project_limits,
                part,
                ..
            } => {
                let chats = if part == Some(agent_protocol::models::ListPart::Projects) {
                    0
                } else {
                    limit.max(1).saturating_add(1)
                };
                let projects = project_limits.map_or(30, |limits| {
                    limits.values().fold(0u32, |sum, limit| {
                        sum.saturating_add(limit.max(&1).saturating_add(1))
                    })
                });
                ((chats.saturating_add(projects)).clamp(1, 100), 100)
            }
            crate::projects::titles::TitleQuery::Project { .. } => (100, 100),
        };
        let started = std::time::Instant::now();
        let scope = if search.trim().is_empty() {
            SessionListScope::Roots
        } else {
            SessionListScope::All
        };
        let ((snapshot, projects_elapsed), (mut first_pages, first_pages_elapsed)) = tokio::join!(
            async { (self.project_snapshot().await, started.elapsed()) },
            async {
                let started = tokio::time::Instant::now();
                let deadline = started + SESSION_LIST_TIMEOUT;
                let pages = futures_util::future::join_all(agents.iter().map(|(_, agent)| {
                    read_session_page(
                        Agent::list(agent.as_ref(), search, None, scope, first_page_size),
                        deadline,
                    )
                }))
                .await;
                (pages, started.elapsed())
            },
        );
        let snapshot = snapshot?;
        // Project discovery must not consume the native page-read budget.
        // Reuse the first pages and give subsequent reads only the time left.
        let deadline =
            tokio::time::Instant::now() + SESSION_LIST_TIMEOUT.saturating_sub(first_pages_elapsed);
        if !search.trim().is_empty() {
            let targets = self.inner.conversations.search_targets(search)?;
            let found =
                futures_util::future::join_all(targets.into_iter().map(|target| async move {
                    let response = tokio::time::timeout_at(deadline, async {
                        self.inner
                            .providers
                            .get(target.provider)?
                            .open(&target.id, 1, false)
                            .await
                    })
                    .await;
                    (target.provider, response)
                }))
                .await;
            for (provider, result) in found {
                if let Ok(Ok(response)) = result
                    && let Some(index) = agents
                        .iter()
                        .position(|(candidate, _)| *candidate == provider)
                    && let Ok(page) = &mut first_pages[index]
                    && !page
                        .data
                        .iter()
                        .any(|summary| summary.thread.id == response.thread.id)
                {
                    page.data.push(crate::providers::SessionSummary {
                        branch: response.thread.git_branch.clone(),
                        thread: response.thread,
                    });
                    page.data.sort_by(|left, right| {
                        right
                            .thread
                            .updated_at
                            .unwrap_or_default()
                            .total_cmp(&left.thread.updated_at.unwrap_or_default())
                    });
                }
            }
        }
        let initial_elapsed = started.elapsed();
        let mut listings = futures_util::future::join_all(agents.iter().zip(first_pages).map(
            |((provider, agent), page)| async move {
                let mut threads = session_pages(
                    agent.as_ref(),
                    search,
                    scope,
                    first_page_size,
                    later_page_size,
                    Some(deadline),
                    Some(page),
                )
                .map_ok(|page| futures_util::stream::iter(page.into_iter().map(Ok::<_, Failure>)))
                .try_flatten()
                .boxed();
                let head = threads.try_next().await;
                (*provider, agent.capabilities(), threads, head)
            },
        ))
        .await;
        let mut titles = crate::projects::titles::TitleList::new(&snapshot.projects, query);
        let mut provider_errors = self
            .provider_errors()
            .as_object()
            .cloned()
            .unwrap_or_default();
        let successful = listings.iter().any(|(_, _, _, head)| head.is_ok());
        while let Some(newest) = listings
            .iter()
            .filter_map(|(_, _, _, head)| head.as_ref().ok()?.as_ref())
            .map(|summary| summary.thread.updated_at.unwrap_or_default())
            .max_by(f64::total_cmp)
        {
            // Never scan additional pages merely to exhaust a timestamp tie in
            // the requested root window. Order ties within that window by ID.
            let maximum = titles.remaining_chats().unwrap_or(usize::MAX).max(1);
            let mut group = Vec::new();
            for (_, capabilities, threads, head) in &mut listings {
                while group.len() < maximum
                    && head.as_ref().is_ok_and(|thread| {
                        thread.as_ref().is_some_and(|summary| {
                            summary
                                .thread
                                .updated_at
                                .unwrap_or_default()
                                .total_cmp(&newest)
                                == std::cmp::Ordering::Equal
                        })
                    })
                {
                    group.push((head.as_mut().unwrap().take().unwrap(), *capabilities));
                    if group.len() < maximum {
                        *head = threads.try_next().await;
                    }
                }
            }
            group.sort_by(|(a, _), (b, _)| a.thread.id.cmp(&b.thread.id));
            for (summary, capabilities) in group {
                let Some(mut thread) = self.inner.conversations.summary(summary.thread)? else {
                    continue;
                };
                thread.git_branch = summary.branch;
                describe_thread(&mut thread, capabilities, &snapshot);
                titles.push(thread);
                if titles.done() {
                    break;
                }
            }
            if titles.done() {
                break;
            }
            // A group can fill its allowance with assigned rows that are not
            // retained. Continue that provider only if the visible page still
            // needs rows; do not start another read after its lookahead.
            for (_, _, threads, head) in &mut listings {
                if matches!(head, Ok(None)) {
                    *head = threads.try_next().await;
                }
            }
        }
        for (provider, _, _, head) in listings {
            if let Err(error) = head {
                provider_errors.insert(provider.key().to_owned(), serde_json::to_value(error)?);
            }
        }
        if !successful && !provider_errors.is_empty() {
            return Err(Failure::new(
                "sessions_unavailable",
                serde_json::to_value(provider_errors)?,
            ));
        }
        let mut page = titles.finish();
        page.revision = revision;
        for thread in &mut page.data {
            thread.list_revision = revision;
        }
        for project in &mut page.projects {
            project.list_revision = revision;
        }
        if !provider_errors.is_empty() {
            page.provider_errors = Some(provider_errors);
        }
        tracing::info!(target: "agent", operation = "host.thread.list.stages",
            message = %format_args!("projects_ms={} titles_ms={} titles={}", projects_elapsed.as_millis(), (first_pages_elapsed + started.elapsed().saturating_sub(initial_elapsed)).as_millis(), page.data.len()));
        Ok(page)
    }
    pub(super) async fn list_decorations(
        &self,
        params: &op::ReadListDecorations,
    ) -> Result<op::ListDecorations, Failure> {
        let projects = params.projects.clone();
        let icons = tokio::task::spawn_blocking(move || {
            projects
                .into_iter()
                .map(|(id, roots)| {
                    let icon = roots.iter().find_map(|root| {
                        crate::projects::icons::resolve(std::path::Path::new(root))
                    });
                    (id, icon)
                })
                .collect()
        });
        let worktrees = if params.threads.is_empty() {
            Default::default()
        } else {
            crate::worktrees::workspace_roots(self.inner.projects.path())
                .await
                .map_err(|error| Failure::new("worktree_status_failed", error))?
        };
        let sources: Vec<_> = params
            .threads
            .iter()
            .map(|(id, cwd, branch)| {
                let mapping =
                    crate::projects::state::worktree_mapping(&worktrees, std::path::Path::new(cwd));
                (
                    id.clone(),
                    crate::worktrees::StatusSource {
                        cwd: cwd.clone(),
                        checkout: mapping.map(|(checkout, _)| checkout.to_owned()),
                        repository: mapping.map(|(_, repository)| repository.to_owned()),
                        branch: branch.clone(),
                    },
                )
            })
            .collect();
        let (icons, statuses) = tokio::join!(
            icons,
            crate::worktrees::directory_statuses(
                sources.iter().map(|(_, source)| source.clone()).collect()
            )
        );
        let icons = icons.map_err(|error| Failure::new("project_icons_unavailable", error))?;
        let statuses = statuses.map_err(|error| Failure::new("worktree_status_failed", error))?;
        Ok(op::ListDecorations {
            icons,
            statuses: sources
                .into_iter()
                .map(|(id, source)| (id, statuses.get(&source).copied()))
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn prefetched_project_titles_continue_past_the_first_native_page() {
        use super::*;
        let root = tempfile::tempdir().unwrap();
        let cwd = dunce::canonicalize(root.path()).unwrap();
        let native = root.path().join("native");
        let transcripts = native.join("projects/shared");
        std::fs::create_dir_all(&transcripts).unwrap();
        let mut expected = std::collections::HashSet::new();
        for _ in 0..102 {
            let id = uuid::Uuid::new_v4();
            expected.insert(id.to_string());
            let user = serde_json::json!({"type":"user","sessionId":id,"cwd":cwd,"message":{"content":"hello"}});
            std::fs::write(transcripts.join(format!("{id}.jsonl")), format!("{user}\n")).unwrap();
        }
        let agent = crate::providers::Claude::load(
            root.path().join("missing-cli"),
            root.path().join("state"),
            Some(native),
        )
        .await
        .unwrap();
        let projects = ProjectStore::new(root.path().join("agent-worktrees.json"));
        projects.register(&cwd).await.unwrap();
        let service = HostRpcService::new([agent.into()], projects);
        let snapshot = service.project_snapshot().await.unwrap();
        let page = service
            .host_title_list(
                "",
                crate::projects::titles::TitleQuery::Project {
                    id: &snapshot.projects[0].id,
                    limit: 101,
                    searching: false,
                },
            )
            .await
            .unwrap();
        assert_eq!(page.data.len(), 101, "{:?}", page.provider_errors);
        assert!(page.has_more);
        let ids: std::collections::HashSet<_> = page
            .data
            .into_iter()
            .map(|thread| thread.id.unwrap().id)
            .collect();
        assert_eq!(ids.len(), 101);
        assert!(ids.is_subset(&expected));
    }

    #[tokio::test]
    async fn list_decorations_use_the_listed_roots_without_reloading_project_catalogs() {
        use super::*;
        let directory = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(directory.path()).unwrap();
        std::fs::write(
            root.join("favicon.svg"),
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><path fill="red" d="M0 0h8v8H0z"/></svg>"#,
        )
        .unwrap();
        let state = root.join("agent-worktrees.json");
        std::fs::write(state.with_file_name("agent-projects.json"), "{").unwrap();
        let service = HostRpcService::new(
            [crate::providers::codex::Codex::new(Err("provider is offline".into())).into()],
            ProjectStore::new(state),
        );
        let params = op::ReadListDecorations {
            scope: op::ListDecorationScope::Root { part: None },
            projects: vec![(
                "listed-project".into(),
                vec![root.to_string_lossy().into_owned()],
            )],
            threads: Vec::new(),
        };
        let result = service.list_decorations(&params).await.unwrap();
        assert!(result.icons["listed-project"].is_some());
        assert!(result.statuses.is_empty());
    }

    #[tokio::test]
    async fn native_project_metadata_does_not_change_registered_membership() {
        use super::*;
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let workspace = dunce::canonicalize(&workspace).unwrap();
        let projects = ProjectStore::new(root.path().join("agent-worktrees.json"));
        projects.register(&workspace).await.unwrap();
        let registered = projects.load().await.unwrap().projects[0].id.clone();
        let native = root.path().join("native");
        let transcripts = native.join("projects/shared");
        std::fs::create_dir_all(&transcripts).unwrap();
        let mut expected = Vec::new();
        for cwd in [&workspace, &root.path().join("outside")] {
            let id = uuid::Uuid::new_v4();
            let metadata = serde_json::json!({"type":"attachment","attachment":{
                "type":"prompt_snapshot","systemPrompt":[
                    "## Where you are\n\nProject: \"Native\" (id: `chan_native`)\n"
                ]
            }});
            let user = serde_json::json!({"type":"user","sessionId":id,"cwd":cwd,"message":{"content":"hello"}});
            std::fs::write(
                transcripts.join(format!("{id}.jsonl")),
                format!("{metadata}\n{user}\n"),
            )
            .unwrap();
            expected.push(id.to_string());
        }
        std::fs::write(native.join(".claude.json"), "{").unwrap();
        let claude = crate::providers::Claude::load(
            root.path().join("missing-cli"),
            root.path().join("state"),
            Some(native.clone()),
        )
        .await
        .unwrap();
        let service = HostRpcService::new([claude.into()], projects);
        let session = service.open_session();
        for repaired in [false, true] {
            if repaired {
                std::fs::write(native.join(".claude.json"), "{}").unwrap();
            }
            let response = service
                .dispatch(
                    session.id(),
                    &Call::ListSessions(op::ListSessions {
                        query: Default::default(),
                    }),
                )
                .await
                .unwrap();
            let Response::Success { result } = agent_protocol::protocol::decode::<
                Response<agent_protocol::models::ThreadList>,
            >(&response.initial)
            .unwrap() else {
                panic!("project metadata failure must not fail session listing")
            };
            assert!(
                result
                    .projects
                    .iter()
                    .any(|project| project.id == registered)
            );
            assert_eq!(result.data.len(), 2);
            for id in &expected {
                assert!(
                    result
                        .data
                        .iter()
                        .any(|thread| thread.id.as_ref().is_some_and(|thread| &thread.id == id))
                );
            }
            assert_eq!(result.projects.len(), 1);
            assert!(
                result
                    .data
                    .iter()
                    .any(|thread| thread.project_id.as_deref() == Some(registered.as_str()))
            );
            assert!(result.data.iter().any(|thread| thread.project_id.is_none()));
        }
    }

    #[tokio::test]
    async fn registered_roots_determine_membership_for_both_providers() {
        use super::*;
        use agent_protocol::models::ProjectMembership;
        let directory = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(directory.path()).unwrap();
        let project = root.join("project");
        std::fs::create_dir(&project).unwrap();
        let store = ProjectStore::new(root.join("agent-worktrees.json"));
        store.register(&project).await.unwrap();
        let projects = store.load().await.unwrap();
        let registered = projects.projects[0].id.as_str();
        for provider in [ProviderKind::Codex, ProviderKind::Claude] {
            for (cwd, membership, expected) in [
                (
                    project.clone(),
                    ProjectMembership::Unassigned {},
                    Some(registered),
                ),
                (
                    project.clone(),
                    ProjectMembership::Unknown {},
                    Some(registered),
                ),
                (
                    project.clone(),
                    ProjectMembership::Assigned("native-other".into()),
                    Some(registered),
                ),
                (
                    root.join("outside"),
                    ProjectMembership::Assigned("native-other".into()),
                    None,
                ),
            ] {
                let mut thread = Thread {
                    id: Some(agent_protocol::session::SessionRef {
                        provider,
                        id: "thread".into(),
                    }),
                    cwd: Some(cwd.to_str().unwrap().into()),
                    project_id: membership,
                    ..Default::default()
                };
                describe_thread(&mut thread, Default::default(), &projects);
                assert_eq!(thread.project_id.as_deref(), expected);
            }
        }
    }

    #[tokio::test]
    async fn task_list_refreshes_branding_for_all_project_headers() {
        use super::*;
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("remote-agent");
        let workspaces = std::iter::once(workspace.clone())
            .chain((0..5).map(|index| root.path().join(format!("empty-project-{index}"))))
            .collect::<Vec<_>>();
        let native = root.path().join("native");
        for workspace in &workspaces {
            std::fs::create_dir(workspace).unwrap();
            std::fs::write(workspace.join("favicon.svg"),
                r##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><rect width="32" height="32" fill="#ff0000"/></svg>"##).unwrap();
        }
        let transcript = native.join("projects/p");
        std::fs::create_dir_all(&transcript).unwrap();
        let id = "12345678-1234-4234-8234-123456789abc";
        std::fs::write(transcript.join(format!("{id}.jsonl")),serde_json::json!({"type":"user","sessionId":id,"cwd":dunce::canonicalize(&workspace).unwrap(),"message":{"content":"Branding task"}}).to_string()+"\n").unwrap();
        let projects = ProjectStore::new(root.path().join("agent-worktrees.json"));
        for workspace in &workspaces {
            projects.register(workspace).await.unwrap();
        }
        let claude = crate::providers::Claude::load(
            root.path().join("does-not-exist"),
            root.path().join("state"),
            Some(native),
        )
        .await
        .unwrap();
        let service = HostRpcService::new(
            [
                Backend::unavailable(ProviderKind::Codex, "unavailable"),
                claude.into(),
            ],
            projects,
        );
        let session = service.open_session();
        for has_icon in [true, false] {
            if !has_icon {
                for workspace in &workspaces {
                    std::fs::remove_file(workspace.join("favicon.svg")).unwrap();
                }
            }
            for project_limit in [5, 15] {
                let call = Call::ListSessions(op::ListSessions {
                    query: agent_protocol::models::ListQuery {
                        project_limit,
                        ..Default::default()
                    },
                });
                let response = service.dispatch(session.id(), &call).await.unwrap();
                let reply = agent_protocol::protocol::decode::<
                    Response<agent_protocol::models::ThreadList>,
                >(&response.initial)
                .unwrap();
                let Response::Success { result } = reply else {
                    panic!("task list failed: {reply:?}")
                };
                assert_eq!(result.data.len(), 1);
                assert!(result.data[0].turns.is_none() && result.data[0].preview.is_none());
                let decoration = op::ReadListDecorations {
                    scope: op::ListDecorationScope::Root { part: None },
                    projects: result
                        .projects
                        .iter()
                        .map(|project| {
                            (
                                project.id.clone(),
                                project.roots.iter().map(|root| root.path.clone()).collect(),
                            )
                        })
                        .collect(),
                    threads: Vec::new(),
                };
                let decorated = service
                    .dispatch(session.id(), &Call::ReadListDecorations(decoration))
                    .await
                    .unwrap();
                let Response::Success { result: decorated } = agent_protocol::protocol::decode::<
                    Response<op::ListDecorations>,
                >(&decorated.initial)
                .unwrap() else {
                    panic!("decoration failed")
                };
                let mut result = result;
                for project in &mut result.projects {
                    project.favicon_png = decorated.icons[&project.id].clone();
                }
                assert_eq!(result.projects.len(), (project_limit as usize).min(6));
                assert_eq!(result.has_more_projects, project_limit == 5);
                let project_id = result.projects[0].id.clone();
                let response = service
                    .dispatch(
                        session.id(),
                        &Call::ListProjectSessions(op::ListProjectSessions {
                            project_id: project_id.clone(),
                            limit: 5,
                            search_term: String::new(),
                        }),
                    )
                    .await
                    .unwrap();
                let Response::Success { result: page } = agent_protocol::protocol::decode::<
                    Response<agent_protocol::models::ThreadList>,
                >(&response.initial)
                .unwrap() else {
                    panic!("project list failed")
                };
                let snapshot = agent_core::state::Snapshot {
                    threads: Some(Arc::new(result)),
                    expanded_projects: Arc::new([(project_id.clone(), 5)].into()),
                    project_threads: Arc::new([(project_id, Arc::new(page))].into()),
                    ..Default::default()
                };
                let list = snapshot.thread_list().unwrap();
                assert_eq!(list.projects.len(), (project_limit as usize).min(6));
                assert!(list.threads.is_empty());
                assert_eq!(list.projects[0].threads.len(), 1);
                assert_eq!(
                    list.projects[0].threads[0].project_id.as_ref(),
                    Some(&list.projects[0].id)
                );
                assert!(
                    list.projects[1..]
                        .iter()
                        .all(|project| project.threads.is_empty())
                );
                assert_eq!(list.projects[0].monogram, "RA");
                for project in &list.projects {
                    assert_eq!(project.icon_png.is_some(), has_icon, "{}", project.name);
                    if let Some(png) = &project.icon_png {
                        let image = image::load_from_memory(png).unwrap().into_rgba8();
                        assert_eq!(image.dimensions(), (64, 64));
                        assert_eq!(image.get_pixel(32, 32).0, [255, 0, 0, 255]);
                    }
                }
            }
        }
    }
}
