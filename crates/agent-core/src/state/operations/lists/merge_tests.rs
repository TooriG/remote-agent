use super::*;
use crate::{
    models::{Thread, ThreadList, WorktreeStatus},
    session::{ProviderKind, SessionRef},
};
use proptest::prelude::*;

fn session(provider: ProviderKind, id: &str) -> SessionRef {
    SessionRef::new(provider, id.into()).unwrap()
}

fn row(
    id: Option<SessionRef>,
    cwd: Option<&str>,
    branch: Option<&str>,
    status: Option<WorktreeStatus>,
) -> Thread {
    Thread {
        id,
        cwd: cwd.map(str::to_owned),
        git_branch: branch.map(str::to_owned),
        worktree_status: status,
        ..Default::default()
    }
}

fn list(data: Vec<Thread>) -> ThreadList {
    ThreadList {
        revision: 1,
        limit: data.len() as u32,
        data,
        projects: Vec::new(),
        has_more: false,
        has_more_projects: false,
        project_pages: Default::default(),
        provider_errors: None,
    }
}

#[test]
fn received_lists_keep_the_first_exact_source_including_absent_fields() {
    for count in [16, 63, 64, 65, 127, 128, 129, 512] {
        let codex = Some(session(ProviderKind::Codex, "same"));
        let claude = Some(session(ProviderKind::Claude, "same"));
        let merged = Some(WorktreeStatus::Merged);
        let unmerged = Some(WorktreeStatus::Unmerged);
        let mut previous = vec![
            row(codex.clone(), Some("/repo"), Some("main"), merged),
            row(codex.clone(), Some("/repo"), Some("main"), None),
            row(codex.clone(), Some("/other"), Some("main"), None),
            row(codex.clone(), Some("/repo"), None, None),
            row(claude.clone(), Some("/repo"), Some("main"), None),
            row(None, None, None, merged),
            row(None, None, None, None),
        ];
        let mut received = vec![
            row(codex.clone(), Some("/repo"), Some("main"), unmerged),
            row(codex.clone(), Some("/other"), Some("main"), unmerged),
            row(codex.clone(), Some("/repo"), Some("changed"), unmerged),
            row(codex.clone(), Some("/repo"), None, unmerged),
            row(codex, None, Some("main"), unmerged),
            row(claude, Some("/repo"), Some("main"), unmerged),
            row(None, None, None, unmerged),
        ];
        while previous.len() < count {
            previous.push(row(
                Some(session(
                    ProviderKind::Codex,
                    &format!("old{}", previous.len()),
                )),
                None,
                None,
                None,
            ));
        }
        while received.len() < count {
            received.push(row(
                Some(session(
                    ProviderKind::Codex,
                    &format!("new{}", received.len()),
                )),
                None,
                None,
                unmerged,
            ));
        }
        let expected_ids: Vec<_> = received.iter().map(|thread| thread.id.clone()).collect();
        let result = merge_received(list(received), Some(&list(previous)));
        assert_eq!(
            result
                .data
                .iter()
                .map(|thread| thread.id.clone())
                .collect::<Vec<_>>(),
            expected_ids
        );
        assert_eq!(
            result.data[..7]
                .iter()
                .map(|thread| thread.worktree_status)
                .collect::<Vec<_>>(),
            [merged, None, unmerged, None, unmerged, None, merged]
        );
        assert!(
            result.data[7..]
                .iter()
                .all(|thread| thread.worktree_status == unmerged)
        );
    }
}

proptest! {
    #[test]
    fn refreshed_metadata_never_crosses_a_changed_source(
        cases in proptest::collection::vec((0u8..4, 0u8..3), 0..256),
    ) {
        let mut previous = Vec::new();
        let mut received = Vec::new();
        let mut expected = Vec::new();
        for (index, &(change, status)) in cases.iter().enumerate() {
            let id = format!("task{index}");
            let old = match status {
                0 => None,
                1 => Some(WorktreeStatus::Merged),
                _ => Some(WorktreeStatus::Unmerged),
            };
            previous.push(row(Some(session(ProviderKind::Codex, &id)), Some("/repo"), Some("main"), old));
            let provider = if change == 3 { ProviderKind::Claude } else { ProviderKind::Codex };
            received.push(row(
                Some(session(provider, &id)),
                Some(if change == 1 { "/other" } else { "/repo" }),
                Some(if change == 2 { "other" } else { "main" }),
                Some(WorktreeStatus::Unmerged),
            ));
            expected.push(if change == 0 { old } else { Some(WorktreeStatus::Unmerged) });
        }
        received.reverse();
        expected.reverse();
        let result = merge_received(list(received), Some(&list(previous)));
        prop_assert_eq!(result.data.iter().map(|thread| thread.worktree_status).collect::<Vec<_>>(), expected);
    }
}
