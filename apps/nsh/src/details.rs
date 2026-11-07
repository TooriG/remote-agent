use crate::{Connection, native, shared::Shared};
use agent_core::{
    presentation::{
        agents::AgentSummary,
        diff::{WorkspaceDiffFile, diff_files},
    },
    state::{Intent, operations as op},
};
use nsh_core::prelude::*;
use serde_json::{Value, json};

// The Swift decoder and these encoders share fixtures to guard the native boundary.
fn diff_data(files: &[WorkspaceDiffFile]) -> Value {
    json!({"diff": {"files": files}})
}

fn agent_data(rows: &[AgentSummary]) -> Value {
    json!({"agents": {"rows": rows}})
}

#[cfg(any(target_os = "macos", test))]
pub(crate) fn workspace_data(
    p: &agent_core::presentation::view::Presentation,
    local: bool,
) -> Value {
    use agent_core::presentation::editors;
    use agent_protocol::editors::FileManagerRevealKind;
    let cwd = &p.composer.controls.navigation.cwd;
    let review = p.snapshot.pane.workspace.review.as_ref();
    let capabilities = p.snapshot.files.file_open_capabilities.as_ref();
    let available = capabilities
        .map(|c| c.editors.as_slice())
        .unwrap_or_default();
    let selected = editors::preferred_editor(p.snapshot.files.preferred_editor, available);
    let manager = capabilities
        .and_then(|c| c.reveal)
        .unwrap_or(FileManagerRevealKind::Finder);
    let sources = p
        .conversation
        .as_ref()
        .into_iter()
        .flat_map(|c| c.source_paths.iter())
        .chain(p.composer.draft.attachments.iter().map(|a| &a.path))
        .collect::<std::collections::BTreeSet<_>>();
    json!({"workspace": {"summary": {
        "directory": if cwd.is_empty() { "フォルダを選択" } else { agent_core::presentation::markdown::file_basename(cwd) },
        "enabled": p.composer.controls.connected,
        "hasDirectory": !cwd.is_empty() && !p.context.selected_directory().is_empty(),
        "branch": review.map(|r| r.branch.as_str()).unwrap_or_default(),
        "additions": review.map_or(0, |r| r.additions), "deletions": review.map_or(0, |r| r.deletions),
        "editors": if local { available.iter().map(|id| json!({"id":id, "title":editors::editor_label(*id,manager), "selected":Some(*id)==selected})).collect::<Vec<_>>() } else { Vec::new() },
        "editor": selected,
        "editorLabel": selected.map(|id| format!("{}で開く", editors::editor_label(id,manager))).unwrap_or_else(|| "エディターで開く".into()),
        "sources": sources.into_iter().map(|path| json!({"path":path,"title":agent_core::presentation::markdown::file_basename(path)})).collect::<Vec<_>>()
    }}})
}

pub(crate) fn diff(cwd: String, host: String) -> impl IntoElement {
    let connection = use_context::<Connection>().unwrap();
    let cwd = stored(cwd);
    let host = stored(host);
    let review = computed(move || {
        connection.publication.with(|p| {
            let workspace = &p.snapshot.pane.workspace;
            (workspace.review_cwd.as_ref() == Some(&cwd.get()))
                .then(|| workspace.review.clone().map(Shared))
                .flatten()
        })
    });
    let projected = signal(None::<Result<Value, String>>);
    effect(move || {
        let review = review.get();
        projected.set(None);
        if let Some(review) = review {
            // Parsing large diffs does not block the native UI. The effect's task
            // is cancelled when a newer review or a different surface replaces it.
            spawn(async move {
                projected.set(Some(
                    blocking(move || diff_data(&diff_files(&review)))
                        .await
                        .map_err(|reason| reason.to_string()),
                ));
            });
        }
    });
    let load = move || {
        connection.client.with(|client| {
            client.dispatch(
                &host.get(),
                None,
                Intent::ReviewWorkspace(op::ReviewWorkspace { cwd: cwd.get() }),
            )
        })
    };
    effect(move || {
        load();
    });
    let pending = computed(move || {
        connection.publication.with(|p| {
            p.snapshot
                .operation_running(op::OperationKey::WorkspaceReview)
        })
    });
    let failed = computed(move || {
        connection
            .publication
            .with(|p| p.context.workspace_error().is_some())
    });
    view()
        .flex_1()
        .min_h_0()
        .child(
            view()
                .flex_row()
                .items_center()
                .p_2()
                .child(
                    text(move || cwd.get())
                        .text_size(px(12.0))
                        .truncate()
                        .selectable(true)
                        .flex_1()
                        .min_w_0(),
                )
                .child(
                    native::button("arrow.clockwise", "差分を更新", load)
                        .prop("enabled", move || {
                            json!(
                                !pending.get()
                                    && connection.publication.with(|p| p.settings.connected())
                            )
                        })
                        .w(px(44.0))
                        .h(px(44.0)),
                ),
        )
        .child(when(
            move || !failed.get(),
            move || {
                native("bex.details")
                    .prop("data", move || {
                        if pending.get() {
                            return json!({"loading": {}});
                        }
                        projected.with(|result| match result {
                            Some(Ok(data)) => data.clone(),
                            Some(Err(message)) => json!({"error": {"message": message}}),
                            None => json!({"loading": {}}),
                        })
                    })
                    .flex_1()
                    .min_h_0()
            },
        ))
}

pub(crate) fn agents(host: String) -> impl IntoElement {
    let connection = use_context::<Connection>().unwrap();
    let host = stored(host);
    let thread_id = connection
        .publication
        .with_untracked(|p| p.context.navigation().thread_id);
    connection
        .client
        .with(|client| client.dispatch(&host.get(), None, Intent::WatchAgents { thread_id }));
    cleanup(move || {
        connection.client.with(|client| {
            client.dispatch(&host.get(), None, Intent::WatchAgents { thread_id: None })
        })
    });
    let agents = computed(move || connection.publication.with(|p| Shared(p.agents.clone())));
    native("bex.details")
        .prop("data", move || agents.with(|rows| agent_data(rows)))
        .flex_1()
        .min_h_0()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_contract_uses_host_editor_labels_and_preserves_sources() {
        use agent_core::{
            presentation::view::Presentation,
            state::{Attachment, Draft, Snapshot},
        };
        use agent_protocol::{
            editors::{EditorId, FileManagerRevealKind, FileOpenCapabilities},
            models::WorkspaceReview,
        };
        use std::sync::Arc;
        let mut snapshot = Snapshot::default();
        snapshot.connection.connected = true;
        let pane = Arc::make_mut(&mut snapshot.pane);
        Arc::make_mut(&mut pane.navigation).cwd = "/fixture/日本語 👋".into();
        Arc::make_mut(&mut pane.workspace).review = Some(Arc::new(WorkspaceReview {
            branch: "feature/native".into(),
            additions: 7,
            deletions: 2,
            files: Vec::new(),
            diff: String::new(),
        }));
        snapshot.files.file_open_capabilities = Some(Arc::new(FileOpenCapabilities {
            editors: vec![EditorId::Cursor, EditorId::FileManager],
            reveal: Some(FileManagerRevealKind::Finder),
        }));
        snapshot.files.preferred_editor = Some(EditorId::Cursor);
        Arc::make_mut(&mut snapshot.input.drafts).insert(
            pane.navigation.draft_key.clone(),
            Arc::new(Draft {
                attachments: vec![Attachment {
                    path: "/fixture/日本語.txt".into(),
                    name: "日本語.txt".into(),
                    is_image: false,
                }],
                ..Default::default()
            }),
        );
        let p = Presentation::project(Arc::new(snapshot), None);
        let expected: Value = serde_json::from_str(include_str!(
            "../apple/Tests/BexNshNativeTests/Fixtures/workspace.json"
        ))
        .unwrap();
        assert_eq!(workspace_data(&p, true), expected);
        assert_eq!(
            workspace_data(&p, false)["workspace"]["summary"]["editors"],
            json!([])
        );
    }

    #[test]
    fn agents_match_the_native_display_contract() {
        let rows = [
            AgentSummary {
                key: "root/active".into(),
                title: "日本語の確認 👋".into(),
                status: "実行中".into(),
                active: true,
                unread: true,
                model: Some("fixture-model".into()),
                detail: Some("進捗 1/2".into()),
                depth: 2,
            },
            AgentSummary {
                key: "root/complete".into(),
                title: "Finished".into(),
                status: "完了".into(),
                active: false,
                unread: false,
                model: None,
                detail: None,
                depth: 0,
            },
        ];
        let expected: Value = serde_json::from_str(include_str!(
            "../apple/Tests/BexNshNativeTests/Fixtures/agents.json"
        ))
        .unwrap();
        assert_eq!(agent_data(&rows), expected);
    }

    #[test]
    fn diffs_match_the_native_display_contract() {
        use agent_core::presentation::diff::DiffRow;
        let files = [
            WorkspaceDiffFile {
                path: "日本語 👋.txt".into(),
                additions: Some(1),
                deletions: Some(1),
                rows: vec![
                    DiffRow {
                        text: "@@ -1 +1 @@".into(),
                        old: None,
                        new: None,
                        kind: "@".into(),
                        file: 1,
                    },
                    DiffRow {
                        text: "-before".into(),
                        old: Some(1),
                        new: None,
                        kind: "-".into(),
                        file: 1,
                    },
                    DiffRow {
                        text: "+変更 👋".into(),
                        old: None,
                        new: Some(1),
                        kind: "+".into(),
                        file: 1,
                    },
                ],
            },
            WorkspaceDiffFile {
                path: "image.png".into(),
                additions: None,
                deletions: None,
                rows: vec![],
            },
        ];
        let expected: Value = serde_json::from_str(include_str!(
            "../apple/Tests/BexNshNativeTests/Fixtures/diff.json"
        ))
        .unwrap();
        assert_eq!(diff_data(&files), expected);
    }
}
