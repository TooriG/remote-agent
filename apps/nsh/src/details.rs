use crate::l10n::{localized, tr};
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

/// The desktop workspace card's content: the folder, its editors, review and sources.
#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct WorkspaceSummary {
    pub directory: String,
    pub enabled: bool,
    pub has_directory: bool,
    pub branch: String,
    pub additions: u64,
    pub deletions: u64,
    /// The Host's editors and their labels; only the local Host's open here.
    pub editors: Vec<(agent_protocol::editors::EditorId, String)>,
    pub editor: Option<agent_protocol::editors::EditorId>,
    pub editor_label: String,
    /// Each source's path and file name.
    pub sources: Vec<(String, String)>,
}

#[cfg(any(target_os = "macos", test))]
pub(crate) fn workspace_summary(
    p: &agent_core::presentation::view::Presentation,
    local: bool,
) -> WorkspaceSummary {
    use crate::l10n::tr_format;
    use agent_core::presentation::{editors, markdown::file_basename};
    use agent_protocol::editors::FileManagerRevealKind;
    let cwd = &p.composer.controls.navigation.cwd;
    let review = p.snapshot.pane.workspace.review.as_ref();
    let capabilities = p.snapshot.files.file_open_capabilities.as_ref();
    let available = capabilities
        .map(|c| c.editors.as_slice())
        .unwrap_or_default();
    let editor = editors::preferred_editor(p.snapshot.files.preferred_editor, available);
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
    WorkspaceSummary {
        directory: if cwd.is_empty() {
            tr("フォルダを選択").into()
        } else {
            file_basename(cwd).into()
        },
        enabled: p.composer.controls.connected,
        has_directory: !cwd.is_empty() && !p.context.selected_directory().is_empty(),
        branch: review.map(|r| r.branch.clone()).unwrap_or_default(),
        additions: review.map_or(0, |r| r.additions),
        deletions: review.map_or(0, |r| r.deletions),
        editors: if local {
            available
                .iter()
                .map(|id| (*id, localized(editors::editor_label(*id, manager).into())))
                .collect()
        } else {
            Vec::new()
        },
        editor,
        editor_label: editor
            .map(|id| {
                tr_format(
                    "{0}で開く",
                    [&localized(editors::editor_label(id, manager).into())],
                )
            })
            .unwrap_or_else(|| tr("エディターで開く").into()),
        sources: sources
            .into_iter()
            .map(|path| (path.clone(), file_basename(path).into()))
            .collect(),
    }
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
    let failed = computed(move || connection.context.with(|p| p.workspace_error().is_some()));
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
                    native::button("arrow.clockwise", tr("差分を更新"), load)
                        .prop("enabled", move || {
                            json!(!pending.get() && connection.settings.with(|p| p.connected()))
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
        .context
        .with_untracked(|p| p.navigation().thread_id);
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
        .prop("data", move || {
            agents.with(|rows| {
                let rows = rows.iter().cloned().map(|row| AgentSummary {
                    status: localized(row.status),
                    detail: row.detail.map(localized),
                    ..row
                });
                agent_data(&rows.collect::<Vec<_>>())
            })
        })
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
        assert_eq!(
            workspace_summary(&p, true),
            WorkspaceSummary {
                directory: "日本語 👋".into(),
                enabled: true,
                has_directory: true,
                branch: "feature/native".into(),
                additions: 7,
                deletions: 2,
                editors: vec![
                    (EditorId::Cursor, "Cursor".into()),
                    (EditorId::FileManager, "Finder".into()),
                ],
                editor: Some(EditorId::Cursor),
                editor_label: "Cursorで開く".into(),
                sources: vec![("/fixture/日本語.txt".into(), "日本語.txt".into())],
            }
        );
        assert!(workspace_summary(&p, false).editors.is_empty());
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
