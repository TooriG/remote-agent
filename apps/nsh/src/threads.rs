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
    Open(agent_protocol::session::SessionRef),
    Expand(String),
    New { project: String, cwd: String },
    More(Option<String>),
    Projects,
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
}
fn thread_row(thread: &ThreadSummary, level: u32) -> Row {
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
        action: Action::Open(thread.id.clone()),
    }
}
fn rows(list: Option<&ThreadList>, dark: bool) -> Vec<Row> {
    let Some(list) = list else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    for project in &list.projects {
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
        });
        if project.expanded {
            for thread in &project.threads {
                rows.push(thread_row(thread, 1));
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
                });
            }
        }
    }
    for thread in &list.threads {
        rows.push(thread_row(thread, 0));
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
    let entries = computed(move || publication.with(|p| rows(p.list.as_deref(), dark())));
    let table = native("bex.list").prop("items", move || entries.with(|entries| json!(entries.iter().map(|row| json!({"id": row_id(&row.action), "title": row.title, "symbol": row.symbol, "asset": null, "png": row.png, "level": row.level, "disclosure": row.disclosure, "group": row.level == 0 && row.disclosure, "expanded": row.expanded})).collect::<Vec<_>>())));
    let table = if is_desktop() {
        table
            .prop("sidebar", json!(true))
            .prop("selected", move || {
                publication.with(|p| {
                    let navigation = &p.composer.controls.navigation;
                    let action = navigation.thread_id.clone().map(Action::Open).or_else(|| {
                        entries.with(|rows| {
                            rows.iter().find_map(|row| match &row.action {
                                Action::New { cwd, .. }
                                    if !cwd.is_empty() && *cwd == navigation.cwd =>
                                {
                                    Some(row.action.clone())
                                }
                                _ => None,
                            })
                        })
                    });
                    json!(action.as_ref().map(row_id))
                })
            })
    } else {
        table
    };
    table
        .on_native_event(move |event: &NativeEvent| {
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
                    Action::Open(id) => {
                        open();
                        Intent::ReadThread(op::ReadThread::open(id))
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
                };
                connection.dispatch(intent);
            }
        })
        .flex_1()
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
        let collapsed = rows(Some(&list), false);
        assert_eq!(collapsed.len(), 1);
        assert_eq!(collapsed[0].png, Some(vec![1, 2]));
        let dark = rows(Some(&list), true);
        assert_eq!(dark[0].png, Some(vec![3, 4]));
        assert_eq!(row_id(&collapsed[0].action), row_id(&dark[0].action));
        list.projects[0].expanded = true;
        let expanded = rows(Some(&list), false);
        assert_eq!(row_id(&collapsed[0].action), row_id(&expanded[0].action));
        assert!(expanded[0].expanded);
        let selected = row_id(&expanded[1].action);
        let mut new_thread = thread;
        new_thread.id = serde_json::from_value(json!({"provider": "codex", "id": "new"})).unwrap();
        list.projects[0].threads.insert(0, new_thread);
        let inserted = rows(Some(&list), false);
        assert_eq!(selected, row_id(&inserted[2].action));
        assert_ne!(selected, row_id(&inserted[1].action));

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
        let ids = rows(Some(&list), false)
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
