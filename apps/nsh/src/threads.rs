use crate::application::{ListRequest, ListScope, ListSource};
use crate::shared::Shared;
use crate::{Connection, native};
use agent_core::{
    presentation::{
        list::{ThreadList, ThreadSummary},
        view::Presentation,
    },
    state::{Intent, operations as op},
};
use nsh_core::prelude::*;
use serde_json::json;

#[derive(Clone, PartialEq, serde::Serialize)]
enum Action {
    Open {
        thread: agent_protocol::session::SessionRef,
        project: Option<String>,
    },
    Expand(String),
    New {
        project: String,
        cwd: String,
    },
    More(Option<String>),
    Projects,
    ProjectHeader,
    Heading(Option<String>),
}
#[derive(Clone, PartialEq)]
struct Row {
    scope: Option<ListScope>,
    title: String,
    symbol: &'static str,
    png: Option<Vec<u8>>,
    level: u32,
    disclosure: bool,
    expanded: bool,
    action: Action,
    compose: Option<Action>,
    menu: serde_json::Value,
    indicators: Option<native::RowIndicators>,
}
impl Row {
    fn heading(
        title: String,
        action: Action,
        menu: serde_json::Value,
        scope: Option<ListScope>,
    ) -> Self {
        Self {
            scope,
            title,
            action,
            compose: None,
            menu,
            symbol: "",
            png: None,
            level: 0,
            disclosure: false,
            expanded: false,
            indicators: None,
        }
    }
}
fn thread_row(thread: &ThreadSummary, level: u32, project: Option<&str>, scope: &ListScope) -> Row {
    Row {
        scope: Some(scope.clone()),
        title: thread.title.clone(),
        symbol: "",
        png: None,
        level,
        disclosure: false,
        expanded: false,
        action: Action::Open {
            thread: thread.id.clone(),
            project: project.map(str::to_owned),
        },
        compose: None,
        menu: serde_json::Value::Null,
        indicators: {
            let activity = agent_core::presentation::thread_status::thread_activity(
                thread.waiting,
                thread.active,
                thread.unread,
            );
            (activity.is_some() || thread.worktree_status.is_some()).then_some(
                native::RowIndicators {
                    activity,
                    worktree: thread.worktree_status,
                },
            )
        },
    }
}
fn rows(
    list: Option<&ThreadList>,
    dark: bool,
    sources: &[ListSource],
    selected_host: &str,
    desktop: bool,
) -> Vec<Row> {
    let mut rows = Vec::new();
    if desktop {
        rows.push(Row::heading(
            "プロジェクト".into(),
            Action::ProjectHeader,
            crate::projects::header_menu(
                list.map_or(&[], |list| list.hidden_projects.as_slice()),
                desktop,
                selected_host,
                sources,
            ),
            None,
        ));
    }
    let Some(list) = list else {
        return rows;
    };
    let more_title = if desktop {
        "もっと表示する"
    } else {
        "さらに表示"
    };
    let sections = list
        .projects
        .iter()
        .filter_map(|project| project.section.clone())
        .collect::<Vec<_>>();
    let headings = agent_core::presentation::list::project_headings(
        list.projects
            .iter()
            .map(|project| (project.pinned, project.section.as_deref())),
        "プロジェクト",
        "ピン留め",
    );
    for (project, heading) in list.projects.iter().zip(headings) {
        let Some(source) = sources
            .iter()
            .find(|source| source.scope.host == project.host_id)
        else {
            continue;
        };
        if let Some(heading) = heading.filter(|_| desktop) {
            rows.push(Row::heading(
                heading.into(),
                Action::Heading(Some(project.id.clone())),
                serde_json::Value::Null,
                Some(source.scope.clone()),
            ));
        }
        rows.push(Row {
            scope: Some(source.scope.clone()),
            title: project.name.clone(),
            symbol: "folder",
            png: project.icon.as_ref().map(|icon| {
                if dark {
                    &icon.dark_png
                } else {
                    &icon.light_png
                }
                .clone()
            }),
            level: 0,
            disclosure: true,
            expanded: project.expanded,
            action: Action::Expand(project.id.clone()),
            compose: project
                .roots
                .first()
                .filter(|_| desktop)
                .map(|root| Action::New {
                    project: project.id.clone(),
                    cwd: root.path.clone(),
                }),
            menu: crate::projects::menu(
                project,
                &sections,
                &source.scope,
                desktop && source.scope.host == "local",
            ),
            indicators: None,
        });
        if project.expanded {
            for thread in &project.threads {
                rows.push(thread_row(thread, 1, Some(&project.id), &source.scope));
            }
            if project.has_more {
                rows.push(Row {
                    scope: Some(source.scope.clone()),
                    title: more_title.into(),
                    symbol: if desktop { "" } else { "ellipsis" },
                    png: None,
                    level: 1,
                    disclosure: false,
                    expanded: false,
                    action: Action::More(Some(project.id.clone())),
                    compose: None,
                    menu: serde_json::Value::Null,
                    indicators: None,
                });
            }
            for root in project.roots.iter().filter(|_| !desktop) {
                rows.push(Row {
                    scope: Some(source.scope.clone()),
                    title: "新しいチャット".into(),
                    symbol: "plus",
                    png: None,
                    level: 1,
                    disclosure: false,
                    expanded: false,
                    action: Action::New {
                        project: project.id.clone(),
                        cwd: root.path.clone(),
                    },
                    compose: None,
                    menu: serde_json::Value::Null,
                    indicators: None,
                });
            }
        }
    }
    if list.has_more_projects {
        rows.push(Row {
            scope: None,
            title: if desktop {
                "もっとプロジェクトを表示"
            } else {
                "プロジェクトをさらに表示"
            }
            .into(),
            symbol: "folder.badge.plus",
            png: None,
            level: 0,
            disclosure: false,
            expanded: false,
            action: Action::Projects,
            compose: None,
            menu: serde_json::Value::Null,
            indicators: None,
        });
    }
    if desktop && !list.threads.is_empty() {
        rows.push(Row::heading(
            "チャット".into(),
            Action::Heading(None),
            serde_json::Value::Null,
            None,
        ));
    }
    for thread in &list.threads {
        if let Some(source) = sources
            .iter()
            .find(|source| source.scope.host == thread.host_id)
        {
            rows.push(thread_row(thread, 0, None, &source.scope));
        }
    }
    if list.has_more {
        rows.push(Row {
            scope: None,
            title: more_title.into(),
            symbol: if desktop { "" } else { "ellipsis" },
            png: None,
            level: 0,
            disclosure: false,
            expanded: false,
            action: Action::More(None),
            compose: None,
            menu: serde_json::Value::Null,
            indicators: None,
        });
    }
    rows
}

pub(crate) fn list(
    publication: Memo<Shared<Presentation>>,
    query: Signal<String>,
    open: impl Fn() + Copy + 'static,
) -> impl IntoElement {
    let connection = use_context::<Connection>().unwrap();
    let sources = computed(move || connection.state.with(|state| state.list_sources()));
    let inputs = computed(move || connection.state.with(|state| state.list_inputs()));
    let list = computed(move || {
        inputs.with(|inputs| {
            agent_core::presentation::connection_list::combine_connection_lists(
                inputs
                    .iter()
                    .map(|(host, value)| (host.clone(), (**value).clone())),
            )
            .map(|list| Shared(std::sync::Arc::new(list)))
        })
    });
    let projects =
        crate::projects::Projects::new(connection.application, connection.host, sources, list);
    effect(move || {
        let search_term = query.get();
        sources.with(|_| ());
        connection
            .application
            .with(|application| application.search_list(&search_term));
    });
    let entries = computed(move || {
        let host = connection.host.get();
        list.with(|list| rows(list.as_deref(), dark(), &sources.get(), &host, is_desktop()))
    });
    let table = native("bex.list").prop("items", move || {
        entries.with(|entries| native_rows(entries, &sources.get()))
    });
    let table = if is_desktop() {
        table
            .prop("sidebar", json!(true))
            .prop("selected", move || {
                publication.with(|p| {
                    let navigation = &p.composer.controls.navigation;
                    let scope = ListScope {
                        host: connection.host.get(),
                        storage: p.snapshot.connection.storage_scope.clone(),
                    };
                    json!(entries.with(|rows| selected_rows(
                        rows,
                        &scope,
                        navigation.thread_id.as_ref()
                    )))
                })
            })
    } else {
        table
    };
    let table = table
        .on_native_event(move |event: &NativeEvent| {
            if &*event.name == "menu" {
                if let Some(command) = event.data.as_str() {
                    projects.select(command);
                }
                return;
            }
            if matches!(&*event.name, "select" | "expand" | "compose")
                && let Some(id) = if &*event.name == "expand" {
                    event.data["id"].as_str()
                } else {
                    event.data.as_str()
                }
                && let Some(action) = entries.with_untracked(|rows| {
                    rows.iter().find_map(|row| {
                        let action = if &*event.name == "compose" {
                            row.compose.as_ref()
                        } else {
                            Some(&row.action)
                        }?;
                        (row_id(row.scope.as_ref(), action) == id)
                            .then(|| (action.clone(), row.expanded, row.scope.clone()))
                    })
                })
            {
                let global = match &action.0 {
                    Action::Projects => Some(ListRequest::MoreProjects),
                    Action::More(None) => Some(ListRequest::MoreChats),
                    _ => None,
                };
                if let Some(request) = global {
                    connection
                        .application
                        .with(|application| application.request_list(request));
                    return;
                }
                let Some(scope) = action.2.as_ref() else {
                    return;
                };
                let navigate = matches!(&action.0, Action::Open { .. } | Action::New { .. });
                let intent = match action.0 {
                    Action::Open { thread, .. } => Intent::ReadThread(op::ReadThread::open(thread)),
                    Action::Expand(id) => Intent::SetProjectExpanded {
                        project_id: id,
                        expanded: event
                            .data
                            .get("expanded")
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(!action.1),
                    },
                    Action::New { cwd, .. } => Intent::NewChat { cwd },
                    Action::More(project_id) => Intent::ExpandThreadList { project_id },
                    Action::Projects => return,
                    Action::Heading(_) | Action::ProjectHeader => return,
                };
                let accepted = connection
                    .application
                    .with(|application| application.list_action(scope, intent, navigate));
                if navigate && accepted {
                    open();
                }
            }
        })
        .flex_1();
    view()
        .flex_1()
        .child(when(|| !is_desktop(), move || projects.mobile_header()))
        .child(table)
        .child(projects.view())
}

fn native_rows(entries: &[Row], sources: &[ListSource]) -> serde_json::Value {
    json!(entries.iter().map(|row| json!({"id": row_id(row.scope.as_ref(), &row.action), "title": row.title, "symbol": row.symbol, "asset": null, "png": row.png, "level": row.level, "disclosure": row.disclosure, "group": matches!(&row.action, Action::Heading(_) | Action::ProjectHeader), "expanded": row.expanded, "menu": row.menu, "indicators": row.indicators, "compose": row.compose.as_ref().map(|action| json!({"id": row_id(row.scope.as_ref(), action), "enabled": sources.iter().any(|source| Some(&source.scope) == row.scope.as_ref() && source.connected)}))})).collect::<Vec<_>>())
}

fn selected_rows(
    rows: &[Row],
    scope: &ListScope,
    thread: Option<&agent_protocol::session::SessionRef>,
) -> Vec<String> {
    rows.iter()
        .filter(|row| match &row.action {
            Action::Open {
                thread: candidate, ..
            } => row.scope.as_ref() == Some(scope) && thread == Some(candidate),
            _ => false,
        })
        .map(|row| row_id(row.scope.as_ref(), &row.action))
        .collect()
}

fn row_id(scope: Option<&ListScope>, action: &Action) -> String {
    serde_json::to_string(&(scope, action)).expect("native row actions serialize")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(host: &str, storage: &str) -> ListScope {
        ListScope {
            host: host.into(),
            storage: storage.into(),
        }
    }

    fn fixture_rows(
        list: Option<&ThreadList>,
        dark: bool,
        host: &str,
        storage: &str,
        desktop: bool,
        connected: bool,
    ) -> Vec<Row> {
        let list = agent_core::presentation::connection_list::combine_connection_lists(
            list.into_iter().map(|list| (host.into(), list.clone())),
        );
        super::rows(
            list.as_ref(),
            dark,
            &[ListSource {
                scope: scope(host, storage),
                connected,
            }],
            host,
            desktop,
        )
    }

    #[test]
    fn desktop_header_keeps_folder_addition_local_and_available_without_a_loaded_list() {
        for (host, connected, enabled) in [
            ("local", false, false),
            ("local", true, true),
            ("remote", true, false),
        ] {
            let rows = fixture_rows(None, false, host, "scope", true, connected);
            assert_eq!(rows.len(), 1);
            assert!(matches!(rows[0].action, Action::ProjectHeader));
            assert_eq!(rows[0].menu[0]["id"], "add");
            assert_eq!(rows[0].menu[0]["enabled"], enabled);
        }
        assert!(fixture_rows(None, false, "local", "scope", false, true).is_empty());
    }

    fn fixture_list() -> ThreadList {
        let thread = ThreadSummary {
            host_id: "host".into(),
            id: serde_json::from_value(json!({"provider": "codex", "id": "chat"})).unwrap(),
            title: "選択中の会話".into(),
            project_id: Some("project".into()),
            active: false,
            waiting: false,
            unread: false,
            worktree_status: None,
        };
        ThreadList {
            loading_projects: false,
            loading_chats: false,
            notice: None,
            threads: Vec::new(),
            projects: vec![agent_core::presentation::list::ProjectSummary {
                host_id: "host".into(),
                id: "project".into(),
                name: "プロジェクト".into(),
                coordinator_url: None,
                roots: Vec::new(),
                icon: Some(agent_core::presentation::list::ProjectIconImages {
                    light_png: vec![1, 2],
                    dark_png: vec![3, 4],
                }),
                monogram: "PR".into(),
                icon_color: 0,
                pinned: false,
                section: None,
                chats_archived: false,
                expanded: false,
                loading: false,
                error: None,
                threads: vec![thread.clone()],
                has_more: false,
            }],
            hidden_projects: Vec::new(),
            has_more: false,
            has_more_projects: false,
        }
    }

    #[test]
    fn native_item_identity_is_stable_and_unique_across_projects() {
        let mut list = fixture_list();
        let thread = list.projects[0].threads[0].clone();
        let collapsed = fixture_rows(Some(&list), false, "host", "scope", false, true);
        assert_eq!(collapsed.len(), 1);
        assert_eq!(collapsed[0].png, Some(vec![1, 2]));
        let dark = fixture_rows(Some(&list), true, "host", "scope", false, true);
        assert_eq!(dark[0].png, Some(vec![3, 4]));
        assert_eq!(
            row_id(collapsed[0].scope.as_ref(), &collapsed[0].action),
            row_id(dark[0].scope.as_ref(), &dark[0].action)
        );
        list.projects[0].expanded = true;
        let expanded = fixture_rows(Some(&list), false, "host", "scope", false, true);
        assert_eq!(
            row_id(collapsed[0].scope.as_ref(), &collapsed[0].action),
            row_id(expanded[0].scope.as_ref(), &expanded[0].action)
        );
        assert!(expanded[0].expanded);
        let selected = row_id(expanded[1].scope.as_ref(), &expanded[1].action);
        let mut new_thread = thread;
        new_thread.id = serde_json::from_value(json!({"provider": "codex", "id": "new"})).unwrap();
        list.projects[0].threads.insert(0, new_thread);
        let inserted = fixture_rows(Some(&list), false, "host", "scope", false, true);
        assert_eq!(
            selected,
            row_id(inserted[2].scope.as_ref(), &inserted[2].action)
        );
        assert_ne!(
            selected,
            row_id(inserted[1].scope.as_ref(), &inserted[1].action)
        );

        list.threads.push(list.projects[0].threads[1].clone());
        let placed = fixture_rows(Some(&list), false, "host", "scope", false, true);
        let ids = placed
            .iter()
            .map(|row| row_id(row.scope.as_ref(), &row.action))
            .collect::<Vec<_>>();
        assert_eq!(
            ids.len(),
            ids.iter().collect::<std::collections::HashSet<_>>().len()
        );
        let candidates = selected_rows(&placed, &scope("host", "scope"), Some(&list.threads[0].id));
        assert_eq!(candidates.len(), 2);
        assert_ne!(candidates[0], candidates[1]);

        let desktop = fixture_rows(Some(&list), false, "host", "scope", true, true);
        assert!(matches!(desktop[0].action, Action::ProjectHeader));
        assert!(!desktop[0].disclosure);
        assert_eq!(desktop[0].level, 0);
        assert_eq!(desktop[1].level, 0);
        assert_eq!(desktop[2].level, 1);
        let recent_heading = desktop
            .iter()
            .position(|row| row.action == Action::Heading(None))
            .unwrap();
        assert_eq!(desktop[recent_heading].title, "チャット");
        assert_eq!(desktop[recent_heading].level, 0);
        assert!(!desktop[recent_heading].disclosure);
        assert_eq!(desktop[recent_heading + 1].level, 0);
        assert_eq!(desktop[recent_heading + 1].title, list.threads[0].title);
        assert_eq!(
            desktop
                .iter()
                .map(|row| row_id(row.scope.as_ref(), &row.action))
                .collect::<std::collections::HashSet<_>>()
                .len(),
            desktop.len()
        );
        assert!(
            !placed
                .iter()
                .any(|row| matches!(row.action, Action::Heading(_)))
        );
        list.has_more_projects = true;
        let paged = fixture_rows(Some(&list), false, "host", "scope", true, true);
        assert!(
            paged
                .iter()
                .position(|row| row.action == Action::Projects)
                .unwrap()
                < paged
                    .iter()
                    .position(|row| row.action == Action::Heading(None))
                    .unwrap()
        );
        list.has_more_projects = false;
        list.threads.clear();
        assert!(
            !fixture_rows(Some(&list), false, "host", "scope", true, true)
                .iter()
                .any(|row| row.action == Action::Heading(None))
        );

        // Two provider projects can refer to the same Host folder. They must
        // remain distinct native items even though both start a chat there.
        list.projects[0].threads.clear();
        list.projects[0]
            .roots
            .push(agent_protocol::models::ProjectRoot {
                path: "/project".into(),
            });
        let mut other = list.projects[0].clone();
        other.id = "other-provider-project".into();
        list.projects.push(other);
        let ids = fixture_rows(Some(&list), false, "host", "scope", false, true)
            .iter()
            .map(|row| row_id(row.scope.as_ref(), &row.action))
            .collect::<Vec<_>>();
        assert_eq!(ids.len(), 4);
        assert_eq!(
            ids.iter().collect::<std::collections::HashSet<_>>().len(),
            4
        );
        list.projects[0].has_more = true;
        let open_thread = agent_protocol::session::SessionRef::new(
            agent_protocol::session::ProviderKind::Codex,
            "chat".into(),
        )
        .unwrap();
        for (host, desktop, folder_enabled) in [
            ("local", true, true),
            ("remote", true, false),
            ("local", false, false),
            ("remote", false, false),
        ] {
            let rendered = fixture_rows(Some(&list), false, host, "scope", desktop, true);
            let project = rendered
                .iter()
                .find(|row| row.action == Action::Expand("project".into()))
                .unwrap();
            let folder = project
                .menu
                .as_array()
                .unwrap()
                .iter()
                .find(|item| item["title"] == "フォルダを開く")
                .unwrap();
            assert_eq!(folder["enabled"], folder_enabled);
            let more = rendered
                .iter()
                .find(|row| row.action == Action::More(Some("project".into())))
                .unwrap();
            assert_eq!(more.level, 1);
            if desktop {
                let identities = rendered
                    .iter()
                    .filter_map(|row| {
                        row.compose
                            .as_ref()
                            .map(|action| row_id(row.scope.as_ref(), action))
                    })
                    .collect::<Vec<_>>();
                assert_eq!(identities.len(), 2);
                assert_eq!(
                    identities
                        .iter()
                        .collect::<std::collections::HashSet<_>>()
                        .len(),
                    2
                );
                assert!(
                    matches!(&project.compose, Some(Action::New {project, cwd}) if project == "project" && cwd == "/project")
                );
                assert!(
                    !rendered
                        .iter()
                        .any(|row| matches!(&row.action, Action::New { .. }))
                );
            } else {
                let compose = rendered.iter().find(|row| matches!(&row.action, Action::New { project, .. } if project == "project")).unwrap();
                assert_eq!(compose.level, 1);
                assert!(project.compose.is_none());
            }
            assert!(selected_rows(&rendered, &scope(host, "scope"), None).is_empty());
            assert!(selected_rows(&rendered, &scope(host, "scope"), Some(&open_thread)).is_empty());
        }
    }

    #[test]
    fn conversation_row_preserves_activity_and_worktree_independently() {
        use agent_core::presentation::thread_status::ThreadActivity;
        use agent_protocol::models::WorktreeStatus;
        let mut thread = ThreadSummary {
            host_id: "host".into(),
            id: serde_json::from_value(json!({"provider": "codex", "id": "chat"})).unwrap(),
            title: "会話".into(),
            project_id: None,
            active: true,
            waiting: true,
            unread: true,
            worktree_status: Some(WorktreeStatus::Unmerged),
        };
        let row = thread_row(&thread, 1, Some("project"), &scope("host", "scope"));
        assert!(row.symbol.is_empty());
        let indicators = row.indicators.unwrap();
        assert_eq!(indicators.activity, Some(ThreadActivity::Waiting));
        assert_eq!(indicators.worktree, Some(WorktreeStatus::Unmerged));
        let encoded = serde_json::to_value(&indicators).unwrap();
        assert_eq!(
            encoded,
            json!({"activity": "waiting", "worktree": "unmerged"})
        );
        thread.active = false;
        thread.waiting = false;
        thread.unread = false;
        assert_eq!(
            thread_row(&thread, 0, None, &scope("host", "scope"))
                .indicators
                .unwrap()
                .worktree,
            Some(WorktreeStatus::Unmerged)
        );
        thread.worktree_status = None;
        assert!(
            thread_row(&thread, 0, None, &scope("host", "scope"))
                .indicators
                .is_none()
        );
    }

    #[test]
    fn identical_host_rows_and_menus_keep_their_owner_and_account_scope() {
        let mut list = fixture_list();
        list.projects[0].expanded = true;
        list.projects[0]
            .roots
            .push(agent_protocol::models::ProjectRoot {
                path: "/same".into(),
            });
        list.threads = list.projects[0].threads.clone();
        let mut hidden = list.projects[0].clone();
        hidden.id = "hidden".into();
        list.hidden_projects.push(hidden);
        let combined = agent_core::presentation::connection_list::combine_connection_lists([
            ("one".into(), list.clone()),
            ("two".into(), list),
        ])
        .unwrap();
        let sources = vec![
            ListSource {
                scope: scope("one", "first-account"),
                connected: true,
            },
            ListSource {
                scope: scope("two", "second-account"),
                connected: false,
            },
        ];
        let rendered = rows(Some(&combined), false, &sources, "one", true);
        let native = native_rows(&rendered, &sources);
        let ids: std::collections::HashSet<_> = rendered
            .iter()
            .map(|row| row_id(row.scope.as_ref(), &row.action))
            .collect();
        assert_eq!(ids.len(), rendered.len());
        let mut compose = std::collections::HashSet::new();
        for row in rendered.iter().filter(|row| row.disclosure) {
            let source = row.scope.as_ref().unwrap();
            compose.insert(row_id(Some(source), row.compose.as_ref().unwrap()));
            let item = native
                .as_array()
                .unwrap()
                .iter()
                .find(|item| item["id"] == row_id(Some(source), &row.action))
                .unwrap();
            assert_eq!(item["compose"]["enabled"], source.host == "one");
            for item in row
                .menu
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|item| item["id"].as_str())
            {
                let command: serde_json::Value = serde_json::from_str(item).unwrap();
                assert_eq!(command["host"], source.host);
                assert_eq!(command["storage"], source.storage);
            }
        }
        assert_eq!(compose.len(), 2);
        let selected_one =
            selected_rows(&rendered, &sources[0].scope, Some(&combined.threads[0].id));
        let selected_two =
            selected_rows(&rendered, &sources[1].scope, Some(&combined.threads[1].id));
        assert_eq!(selected_one.len(), 2);
        assert_eq!(selected_two.len(), 2);
        assert!(selected_one.iter().all(|id| !selected_two.contains(id)));
        let hidden = rendered[0].menu[1]["children"].as_array().unwrap();
        let owners: Vec<_> = hidden
            .iter()
            .map(|item| {
                let command: serde_json::Value =
                    serde_json::from_str(item["id"].as_str().unwrap()).unwrap();
                (
                    command["host"].as_str().unwrap().to_owned(),
                    command["storage"].as_str().unwrap().to_owned(),
                )
            })
            .collect();
        assert_eq!(
            owners,
            [
                ("one".into(), "first-account".into()),
                ("two".into(), "second-account".into())
            ]
        );
        let mut replaced = sources.clone();
        replaced[0].scope.storage = "new-account".into();
        let new_rows = rows(Some(&combined), false, &replaced, "one", true);
        let new_selection =
            selected_rows(&new_rows, &replaced[0].scope, Some(&combined.threads[0].id));
        assert!(selected_one.iter().all(|id| !new_selection.contains(id)));
        assert_eq!(
            selected_rows(&new_rows, &sources[1].scope, Some(&combined.threads[1].id)),
            selected_two
        );
    }
}
