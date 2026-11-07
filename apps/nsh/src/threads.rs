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
    Heading(String),
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
    menu: serde_json::Value,
}
fn thread_row(thread: &ThreadSummary, level: u32, project: Option<&str>) -> Row {
    Row {
        title: thread.title.clone(),
        symbol: if thread.waiting {
            "hand.raised"
        } else if thread.active {
            "ellipsis.bubble"
        } else if thread.unread {
            "bubble.left.fill"
        } else {
            "bubble.left"
        },
        png: None,
        level,
        disclosure: false,
        expanded: false,
        action: Action::Open {
            thread: thread.id.clone(),
            project: project.map(str::to_owned),
        },
        menu: serde_json::Value::Null,
    }
}
fn rows(
    list: Option<&ThreadList>,
    dark: bool,
    host: &str,
    storage: &str,
    desktop: bool,
) -> Vec<Row> {
    let Some(list) = list else {
        return Vec::new();
    };
    let mut rows = Vec::new();
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
            rows.push(Row {
                title: heading.into(),
                symbol: "",
                png: None,
                level: 0,
                disclosure: false,
                expanded: false,
                action: Action::Heading(project.id.clone()),
                menu: serde_json::Value::Null,
            });
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
            menu: crate::projects::menu(
                project,
                &sections,
                host,
                storage,
                desktop && host == "local",
            ),
        });
        if project.expanded {
            for thread in &project.threads {
                rows.push(thread_row(thread, 1, Some(&project.id)));
            }
            if project.has_more {
                rows.push(Row {
                    title: "さらに表示".into(),
                    symbol: "ellipsis",
                    png: None,
                    level: 1,
                    disclosure: false,
                    expanded: false,
                    action: Action::More(Some(project.id.clone())),
                    menu: serde_json::Value::Null,
                });
            }
            for root in &project.roots {
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
                    menu: serde_json::Value::Null,
                });
            }
        }
    }
    for thread in &list.threads {
        rows.push(thread_row(thread, 0, None));
    }
    if list.has_more {
        rows.push(Row {
            title: "さらに表示".into(),
            symbol: "ellipsis",
            png: None,
            level: 0,
            disclosure: false,
            expanded: false,
            action: Action::More(None),
            menu: serde_json::Value::Null,
        });
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
            menu: serde_json::Value::Null,
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
            )
        })
    });
    let table = native("bex.list").prop("items", move || entries.with(|entries| json!(entries.iter().map(|row| json!({"id": row_id(&row.action), "title": row.title, "symbol": row.symbol, "asset": null, "png": row.png, "level": row.level, "disclosure": row.disclosure, "group": matches!(&row.action, Action::Heading(_)), "expanded": row.expanded, "menu": row.menu})).collect::<Vec<_>>())));
    let table = if is_desktop() {
        table
            .prop("sidebar", json!(true))
            .prop("selected", move || {
                publication.with(|p| {
                    let navigation = &p.composer.controls.navigation;
                    json!(entries.with(|rows| selected_rows(
                        rows,
                        navigation.thread_id.as_ref(),
                        &navigation.cwd
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
            if matches!(&*event.name, "select" | "expand")
                && let Some(id) = if &*event.name == "expand" {
                    event.data["id"].as_str()
                } else {
                    event.data.as_str()
                }
                && let Some(action) = entries.with_untracked(|rows| {
                    rows.iter()
                        .find(|row| row_id(&row.action) == id)
                        .map(|row| (row.action.clone(), row.expanded))
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
                    Action::Heading(_) => return,
                };
                connection.dispatch(intent);
            }
        })
        .flex_1();
    view()
        .flex_1()
        .child(projects.header())
        .child(table)
        .child(projects.view())
}

fn selected_rows(
    rows: &[Row],
    thread: Option<&agent_protocol::session::SessionRef>,
    cwd: &str,
) -> Vec<String> {
    rows.iter()
        .filter(|row| match &row.action {
            Action::Open {
                thread: candidate, ..
            } => thread == Some(candidate),
            Action::New { cwd: candidate, .. } => {
                thread.is_none() && !candidate.is_empty() && candidate == cwd
            }
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
        let collapsed = rows(Some(&list), false, "host", "scope", false);
        assert_eq!(collapsed.len(), 1);
        assert_eq!(collapsed[0].png, Some(vec![1, 2]));
        let dark = rows(Some(&list), true, "host", "scope", false);
        assert_eq!(dark[0].png, Some(vec![3, 4]));
        assert_eq!(row_id(&collapsed[0].action), row_id(&dark[0].action));
        list.projects[0].expanded = true;
        let expanded = rows(Some(&list), false, "host", "scope", false);
        assert_eq!(row_id(&collapsed[0].action), row_id(&expanded[0].action));
        assert!(expanded[0].expanded);
        let selected = row_id(&expanded[1].action);
        let mut new_thread = thread;
        new_thread.id = serde_json::from_value(json!({"provider": "codex", "id": "new"})).unwrap();
        list.projects[0].threads.insert(0, new_thread);
        let inserted = rows(Some(&list), false, "host", "scope", false);
        assert_eq!(selected, row_id(&inserted[2].action));
        assert_ne!(selected, row_id(&inserted[1].action));

        list.threads.push(list.projects[0].threads[1].clone());
        let placed = rows(Some(&list), false, "host", "scope", false);
        let ids = placed
            .iter()
            .map(|row| row_id(&row.action))
            .collect::<Vec<_>>();
        assert_eq!(
            ids.len(),
            ids.iter().collect::<std::collections::HashSet<_>>().len()
        );
        let candidates = selected_rows(&placed, Some(&list.threads[0].id), "/project");
        assert_eq!(candidates.len(), 2);
        assert_ne!(candidates[0], candidates[1]);
        list.threads.clear();

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
        let ids = rows(Some(&list), false, "host", "scope", false)
            .iter()
            .map(|row| row_id(&row.action))
            .collect::<Vec<_>>();
        assert_eq!(ids.len(), 4);
        assert_eq!(
            ids.iter().collect::<std::collections::HashSet<_>>().len(),
            4
        );
    }
}
