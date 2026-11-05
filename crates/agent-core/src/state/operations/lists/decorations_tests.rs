use super::*;
use crate::{
    models::{Project, Thread, ThreadList, WorktreeStatus},
    session::{ProviderKind, SessionRef},
};
use agent_protocol::operations::{ListDecorationScope, ListDecorations};

fn row(provider: ProviderKind, id: &str, cwd: Option<&str>, branch: Option<&str>) -> Thread {
    Thread {
        id: Some(SessionRef::new(provider, id.into()).unwrap()),
        cwd: cwd.map(str::to_owned),
        git_branch: branch.map(str::to_owned),
        worktree_status: Some(WorktreeStatus::Unmerged),
        ..Default::default()
    }
}

fn list(data: Vec<Thread>, projects: Vec<Project>) -> Arc<ThreadList> {
    Arc::new(ThreadList {
        revision: 0,
        limit: 5,
        data,
        projects,
        has_more: false,
        has_more_projects: false,
        project_pages: Default::default(),
        provider_errors: None,
    })
}

fn read(threads: &[Thread]) -> ReadListDecorations {
    ReadListDecorations {
        scope: ListDecorationScope::Root { part: None },
        threads: threads
            .iter()
            .filter_map(|thread| {
                Some((
                    thread.id.clone()?,
                    thread.cwd.clone()?,
                    thread.git_branch.clone(),
                ))
            })
            .collect(),
    }
}

#[test]
fn project_headers_need_no_reads_and_worktree_status_requires_a_thread_directory() {
    let current = row(ProviderKind::Codex, "task", Some("/repo"), Some("main"));
    for (threads, expected) in [
        (Vec::new(), Vec::new()),
        (
            vec![Thread {
                cwd: Some("/repo".into()),
                ..Default::default()
            }],
            Vec::new(),
        ),
        (
            vec![row(ProviderKind::Codex, "task", None, Some("main"))],
            Vec::new(),
        ),
        (
            vec![current.clone()],
            vec![Effect::execute(read(std::slice::from_ref(&current)))],
        ),
    ] {
        let mut state = Snapshot::default();
        let effects = ListSessions::new(Default::default()).apply(
            &mut state,
            Arc::unwrap_or_clone(list(
                threads,
                vec![Project {
                    id: "p".into(),
                    name: "Project".into(),
                    roots: vec![crate::models::ProjectRoot {
                        path: "/repo".into(),
                    }],
                    ..Default::default()
                }],
            )),
        );
        assert_eq!(format!("{effects:?}"), format!("{expected:?}"));
        assert_eq!(state.thread_list().unwrap().projects[0].name, "Project");
    }
}

#[test]
fn decorations_match_captured_sources_across_root_and_project_pages() {
    let current = row(ProviderKind::Codex, "task", Some("/repo"), Some("main"));
    let cleared = row(ProviderKind::Claude, "clear", Some("/repo"), None);
    let mut operation = read(&[current.clone(), cleared.clone()]);
    // The original lookup uses the first captured source for a session.
    operation.threads.push((
        current.id.clone().unwrap(),
        "/elsewhere".into(),
        Some("main".into()),
    ));
    let stale = vec![
        row(
            ProviderKind::Codex,
            "task",
            Some("/elsewhere"),
            Some("main"),
        ),
        row(ProviderKind::Codex, "task", Some("/repo"), Some("next")),
        row(ProviderKind::Codex, "task", Some("/repo"), None),
        row(ProviderKind::Codex, "task", None, Some("main")),
        row(ProviderKind::Claude, "task", Some("/repo"), Some("main")),
        row(
            ProviderKind::Codex,
            "unrequested",
            Some("/repo"),
            Some("main"),
        ),
        Thread::default(),
    ];
    let project = Project {
        id: "p".into(),
        ..Default::default()
    };
    let mut state = Snapshot {
        threads: Some(list(vec![current.clone(), cleared.clone()], vec![project])),
        project_threads: Arc::new(
            [
                ("changed".into(), list(vec![current.clone()], Vec::new())),
                ("stale".into(), list(stale, Vec::new())),
            ]
            .into(),
        ),
        ..Default::default()
    };
    let previous = state.clone();
    let output = ListDecorations {
        statuses: vec![
            (current.id.clone().unwrap(), None),
            (current.id.clone().unwrap(), Some(WorktreeStatus::Merged)),
            (cleared.id.clone().unwrap(), None),
            (
                SessionRef::new(ProviderKind::Codex, "unrequested".into()).unwrap(),
                Some(WorktreeStatus::Merged),
            ),
        ],
    };
    assert!(operation.apply(&mut state, output).is_empty());
    let root = state.threads.as_ref().unwrap();
    assert_eq!(root.data[0].worktree_status, Some(WorktreeStatus::Merged));
    assert_eq!(root.data[1].worktree_status, None);
    assert_eq!(root.projects, previous.threads.as_ref().unwrap().projects);
    assert_eq!(
        state.project_threads["changed"].data[0].worktree_status,
        Some(WorktreeStatus::Merged)
    );
    assert!(Arc::ptr_eq(
        &state.project_threads["stale"],
        &previous.project_threads["stale"]
    ));
    assert_eq!(
        previous.threads.as_ref().unwrap().data[0].worktree_status,
        Some(WorktreeStatus::Unmerged)
    );
}

#[test]
fn unchanged_decorations_preserve_shared_lists() {
    let current = row(ProviderKind::Codex, "task", Some("/repo"), None);
    let project = Project {
        id: "p".into(),
        ..Default::default()
    };
    let original = Snapshot {
        threads: Some(list(vec![current.clone()], vec![project])),
        project_threads: Arc::new([("p".into(), list(vec![current.clone()], Vec::new()))].into()),
        ..Default::default()
    };
    for statuses in [
        Vec::new(),
        vec![(current.id.clone().unwrap(), current.worktree_status)],
    ] {
        let mut state = original.clone();
        read(std::slice::from_ref(&current)).apply(&mut state, ListDecorations { statuses });
        assert!(Arc::ptr_eq(
            state.threads.as_ref().unwrap(),
            original.threads.as_ref().unwrap()
        ));
        assert!(Arc::ptr_eq(
            &state.project_threads,
            &original.project_threads
        ));
    }
}

#[test]
fn project_decorations_do_not_require_a_root_list() {
    let current = row(ProviderKind::Codex, "task", Some("/repo"), None);
    let original = Snapshot {
        project_threads: Arc::new([("p".into(), list(vec![current.clone()], Vec::new()))].into()),
        ..Default::default()
    };
    let output = ListDecorations {
        statuses: vec![(current.id.clone().unwrap(), Some(WorktreeStatus::Merged))],
    };
    let mut state = original.clone();
    read(&[]).apply(&mut state, output.clone());
    assert!(Arc::ptr_eq(
        &state.project_threads,
        &original.project_threads
    ));
    read(&[current]).apply(&mut state, output);
    assert!(state.threads.is_none());
    assert_eq!(
        state.project_threads["p"].data[0].worktree_status,
        Some(WorktreeStatus::Merged)
    );
    assert_eq!(
        original.project_threads["p"].data[0].worktree_status,
        Some(WorktreeStatus::Unmerged)
    );
}
