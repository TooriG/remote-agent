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

fn versioned(id: Option<SessionRef>, name: &str, revision: u64) -> Thread {
    Thread {
        id,
        name: Some(name.into()),
        preview: Some(format!("preview-{name}")),
        cwd: Some(format!("/repo/{name}")),
        status: crate::models::SessionStatus::Idle,
        list_revision: revision,
        ..Default::default()
    }
}

fn unavailable(row: Thread) -> Thread {
    Thread {
        status: crate::models::SessionStatus::Unknown,
        list_stale: Some(true),
        ..row
    }
}

#[test]
fn failed_reads_restore_only_missing_active_provider_rows_once_in_cached_order() {
    for count in [16, 127, 128, 129, 512] {
        let existing = Some(session(ProviderKind::Codex, "existing"));
        let shared_codex = Some(session(ProviderKind::Codex, "shared"));
        let received = vec![
            versioned(existing.clone(), "current", 0),
            versioned(
                Some(session(ProviderKind::Claude, "shared")),
                "claude-current",
                0,
            ),
        ];
        let first = versioned(Some(session(ProviderKind::Codex, "missing")), "first", 0);
        let mut handoff = versioned(Some(session(ProviderKind::Claude, "handoff")), "handoff", 0);
        handoff.active_provider = Some(ProviderKind::Codex);
        let mut healthy = versioned(Some(session(ProviderKind::Codex, "healthy")), "healthy", 0);
        healthy.active_provider = Some(ProviderKind::Claude);
        let mut skipped = versioned(Some(session(ProviderKind::Codex, "later")), "skipped", 0);
        skipped.active_provider = Some(ProviderKind::Claude);
        let eligible = versioned(Some(session(ProviderKind::Codex, "later")), "eligible", 0);
        let shared = versioned(shared_codex, "codex-shared", 0);
        let mut anonymous = versioned(None, "anonymous", 0);
        anonymous.active_provider = Some(ProviderKind::Codex);
        let mut cached = vec![
            versioned(existing, "stale", 0),
            first.clone(),
            versioned(first.id.clone(), "duplicate", 1),
            anonymous,
            healthy,
            handoff.clone(),
            skipped,
            eligible.clone(),
            shared.clone(),
        ];
        let mut expected = received.clone();
        expected.extend([first, handoff, eligible, shared].map(unavailable));
        // Six initial rows qualify, including the existing row and duplicate.
        for index in 6..count {
            let row = versioned(
                Some(session(ProviderKind::Codex, &format!("extra{index}"))),
                "extra",
                0,
            );
            expected.push(unavailable(row.clone()));
            cached.push(row);
        }
        let mut input = list(received);
        let errors: serde_json::Map<String, serde_json::Value> =
            [("codex".into(), serde_json::json!("offline"))]
                .into_iter()
                .collect();
        input.provider_errors = Some(errors.clone());
        let result = merge_received(input, Some(&list(cached)));
        assert_eq!(result.data, expected);
        assert_eq!(result.provider_errors, Some(errors));
    }
}

proptest! {
    #[test]
    fn failed_reads_preserve_received_rows_and_restore_only_unavailable_sources(
        cases in proptest::collection::vec((0u8..3, any::<bool>()), 0..512),
    ) {
        let mut cached = Vec::new();
        let mut received = Vec::new();
        let mut restored = Vec::new();
        for (index, &(active, returned)) in cases.iter().enumerate() {
            let provider = if index % 2 == 0 { ProviderKind::Codex } else { ProviderKind::Claude };
            let id = Some(session(provider, &format!("task{}", index / 2)));
            let mut row = versioned(id.clone(), "cached", 0);
            row.active_provider = match active { 1 => Some(ProviderKind::Codex), 2 => Some(ProviderKind::Claude), _ => None };
            if returned {
                received.push(versioned(id, "received", 0));
            } else if active == 1 || (active == 0 && provider == ProviderKind::Codex) {
                restored.push(unavailable(row.clone()));
            }
            cached.push(row);
        }
        received.reverse();
        let mut expected = received.clone();
        expected.extend(restored);
        let mut input = list(received);
        input.provider_errors = Some([("codex".into(), serde_json::json!("offline"))].into_iter().collect());
        let result = merge_received(input, Some(&list(cached)));
        prop_assert_eq!(result.data, expected);
    }
}

#[test]
fn newer_notices_keep_last_values_first_positions_and_duplicate_received_rows() {
    for count in [16, 127, 128, 129, 512] {
        let existing = Some(session(ProviderKind::Codex, "existing"));
        let a = Some(session(ProviderKind::Codex, "a"));
        let b = Some(session(ProviderKind::Codex, "b"));
        let last = versioned(existing.clone(), "last", 2);
        let last_anonymous = versioned(None, "anonymous-last", 2);
        let appended_a = versioned(a.clone(), "a-last", 2);
        let appended_b = versioned(b, "b", 2);
        let duplicate = versioned(existing.clone(), "duplicate", 0);
        let anonymous_duplicate = versioned(None, "anonymous-duplicate", 0);
        let mut received = vec![
            versioned(existing.clone(), "received", 0),
            duplicate.clone(),
            versioned(None, "anonymous", 0),
            anonymous_duplicate.clone(),
        ];
        let mut expected = vec![
            last.clone(),
            duplicate,
            last_anonymous.clone(),
            anonymous_duplicate,
        ];
        let mut previous = vec![
            versioned(a, "a-first", 2),
            versioned(existing, "first", 3),
            appended_b.clone(),
            appended_a.clone(),
            last,
            versioned(None, "anonymous-first", 4),
            last_anonymous,
        ];
        while previous.len() < count {
            let id = Some(session(
                ProviderKind::Codex,
                &format!("padding{}", previous.len()),
            ));
            let cached = versioned(id.clone(), "cached-padding", 2);
            received.push(versioned(id, "received-padding", 0));
            expected.push(cached.clone());
            previous.push(cached);
        }
        previous.push(versioned(
            Some(session(ProviderKind::Codex, "ignored")),
            "old",
            1,
        ));
        expected.extend([appended_a, appended_b]);
        assert_eq!(
            merge_received(list(received), Some(&list(previous))).data,
            expected
        );
    }
}

proptest! {
    #[test]
    fn only_notices_newer_than_the_read_replace_each_provider_scoped_row(
        fresh in proptest::collection::vec(any::<bool>(), 0..512),
    ) {
        let mut previous = Vec::new();
        let mut received = Vec::new();
        let mut expected = Vec::new();
        for (index, fresh) in fresh.into_iter().enumerate() {
            let provider = if index % 2 == 0 { ProviderKind::Codex } else { ProviderKind::Claude };
            let id = Some(session(provider, &format!("task{}", index / 2)));
            let cached = versioned(id.clone(), "cached", if fresh { 2 } else { 1 });
            let read = versioned(id, "received", 0);
            expected.push(if fresh { cached.clone() } else { read.clone() });
            previous.push(cached);
            received.push(read);
        }
        received.reverse();
        expected.reverse();
        let merged = merge_received(list(received), Some(&list(previous)));
        prop_assert_eq!(merged.data, expected);
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
