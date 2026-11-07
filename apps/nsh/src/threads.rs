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
    fn heading(title: String, action: Action, menu: serde_json::Value) -> Self {
        Self {
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
fn thread_row(thread: &ThreadSummary, level: u32, project: Option<&str>) -> Row {
    Row {
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
    host: &str,
    storage: &str,
    desktop: bool,
    connected: bool,
) -> Vec<Row> {
    let mut rows = Vec::new();
    if desktop {
        rows.push(Row::heading(
            "プロジェクト".into(),
            Action::ProjectHeader,
            crate::projects::header_menu(
                list.map_or(&[], |list| list.hidden_projects.as_slice()),
                desktop,
                host,
                storage,
                connected,
            ),
        ));
    }
    let Some(list) = list else {
        return rows;
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
        if let Some(heading) = heading.filter(|_| desktop) {
            rows.push(Row::heading(
                heading.into(),
                Action::Heading(Some(project.id.clone())),
                serde_json::Value::Null,
            ));
        }
        rows.push(Row {
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
                host,
                storage,
                desktop && host == "local",
            ),
            indicators: None,
        });
        if project.expanded {
            for thread in &project.threads {
                rows.push(thread_row(thread, 1, Some(&project.id)));
            }
            if project.has_more {
                rows.push(Row {
                    title: "さらに表示".into(),
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
            title: "プロジェクトをさらに表示".into(),
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
        ));
    }
    for thread in &list.threads {
        rows.push(thread_row(thread, 0, None));
    }
    if list.has_more {
        rows.push(Row {
            title: "さらに表示".into(),
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
    let projects = crate::projects::Projects::new(connection.client, connection.host, publication);
    let connected = computed(move || publication.with(|p| p.composer.controls.connected));
    effect(move || {
        let search_term = query.get();
        connection.host.get();
        if connected.get() {
            let mut current =
                publication.with_untracked(|p| (*p.snapshot.lists.list_query).clone());
            if current.search_term != search_term {
                current.search_term = search_term;
                connection.dispatch(Intent::ListSessions(op::ListSessions::new(current)));
            }
        }
    });
    let entries = computed(move || {
        let host = connection.host.get();
        publication.with(|p| {
            rows(
                p.list.as_deref(),
                dark(),
                &host,
                &p.snapshot.connection.storage_scope,
                is_desktop(),
                p.composer.controls.connected,
            )
        })
    });
    let table = native("bex.list").prop("items", move || entries.with(|entries| json!(entries.iter().map(|row| json!({"id": row_id(&row.action), "title": row.title, "symbol": row.symbol, "asset": null, "png": row.png, "level": row.level, "disclosure": row.disclosure, "group": matches!(&row.action, Action::Heading(_) | Action::ProjectHeader), "expanded": row.expanded, "menu": row.menu, "indicators": row.indicators, "compose": row.compose.as_ref().map(|action| json!({"id": row_id(action), "enabled": connected.get()}))})).collect::<Vec<_>>())));
    let table = if is_desktop() {
        table
            .prop("sidebar", json!(true))
            .prop("selected", move || {
                publication.with(|p| {
                    let navigation = &p.composer.controls.navigation;
                    json!(entries.with(|rows| selected_rows(rows, navigation.thread_id.as_ref(),)))
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
                        (row_id(action) == id).then(|| (action.clone(), row.expanded))
                    })
                })
            {
                let intent = match action.0 {
                    Action::Open { thread, .. } => {
                        open();
                        Intent::ReadThread(op::ReadThread::open(thread))
                    }
                    Action::Expand(id) => Intent::SetProjectExpanded {
                        project_id: id,
                        expanded: event
                            .data
                            .get("expanded")
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(!action.1),
                    },
                    Action::New { cwd, .. } => {
                        open();
                        Intent::NewChat { cwd }
                    }
                    Action::More(project_id) => Intent::ExpandThreadList { project_id },
                    Action::Projects => Intent::ExpandProjects,
                    Action::Heading(_) | Action::ProjectHeader => return,
                };
                connection.dispatch(intent);
            }
        })
        .flex_1();
    view()
        .flex_1()
        .child(when(|| !is_desktop(), move || projects.mobile_header()))
        .child(table)
        .child(projects.view())
}

fn selected_rows(
    rows: &[Row],
    thread: Option<&agent_protocol::session::SessionRef>,
) -> Vec<String> {
    rows.iter()
        .filter(|row| match &row.action {
            Action::Open {
                thread: candidate, ..
            } => thread == Some(candidate),
            _ => false,
        })
        .map(|row| row_id(&row.action))
        .collect()
}

fn row_id(action: &Action) -> String {
    serde_json::to_string(action).expect("native row actions serialize")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_header_keeps_folder_addition_local_and_available_without_a_loaded_list() {
        for (host, connected, enabled) in [
            ("local", false, false),
            ("local", true, true),
            ("remote", true, false),
        ] {
            let rows = rows(None, false, host, "scope", true, connected);
            assert_eq!(rows.len(), 1);
            assert!(matches!(rows[0].action, Action::ProjectHeader));
            assert_eq!(rows[0].menu[0]["id"], "add");
            assert_eq!(rows[0].menu[0]["enabled"], enabled);
        }
        assert!(rows(None, false, "local", "scope", false, true).is_empty());
    }

    #[test]
    fn native_item_identity_is_stable_and_unique_across_projects() {
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
        let mut list = ThreadList {
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
        };
        let collapsed = rows(Some(&list), false, "host", "scope", false, true);
        assert_eq!(collapsed.len(), 1);
        assert_eq!(collapsed[0].png, Some(vec![1, 2]));
        let dark = rows(Some(&list), true, "host", "scope", false, true);
        assert_eq!(dark[0].png, Some(vec![3, 4]));
        assert_eq!(row_id(&collapsed[0].action), row_id(&dark[0].action));
        list.projects[0].expanded = true;
        let expanded = rows(Some(&list), false, "host", "scope", false, true);
        assert_eq!(row_id(&collapsed[0].action), row_id(&expanded[0].action));
        assert!(expanded[0].expanded);
        let selected = row_id(&expanded[1].action);
        let mut new_thread = thread;
        new_thread.id = serde_json::from_value(json!({"provider": "codex", "id": "new"})).unwrap();
        list.projects[0].threads.insert(0, new_thread);
        let inserted = rows(Some(&list), false, "host", "scope", false, true);
        assert_eq!(selected, row_id(&inserted[2].action));
        assert_ne!(selected, row_id(&inserted[1].action));

        list.threads.push(list.projects[0].threads[1].clone());
        let placed = rows(Some(&list), false, "host", "scope", false, true);
        let ids = placed
            .iter()
            .map(|row| row_id(&row.action))
            .collect::<Vec<_>>();
        assert_eq!(
            ids.len(),
            ids.iter().collect::<std::collections::HashSet<_>>().len()
        );
        let candidates = selected_rows(&placed, Some(&list.threads[0].id));
        assert_eq!(candidates.len(), 2);
        assert_ne!(candidates[0], candidates[1]);

        let desktop = rows(Some(&list), false, "host", "scope", true, true);
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
                .map(|row| row_id(&row.action))
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
        let paged = rows(Some(&list), false, "host", "scope", true, true);
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
            !rows(Some(&list), false, "host", "scope", true, true)
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
        let ids = rows(Some(&list), false, "host", "scope", false, true)
            .iter()
            .map(|row| row_id(&row.action))
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
            let rendered = rows(Some(&list), false, host, "scope", desktop, true);
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
                    .filter_map(|row| row.compose.as_ref())
                    .map(row_id)
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
            assert!(selected_rows(&rendered, None).is_empty());
            assert!(selected_rows(&rendered, Some(&open_thread)).is_empty());
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
        let row = thread_row(&thread, 1, Some("project"));
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
            thread_row(&thread, 0, None).indicators.unwrap().worktree,
            Some(WorktreeStatus::Unmerged)
        );
        thread.worktree_status = None;
        assert!(thread_row(&thread, 0, None).indicators.is_none());
    }
}
