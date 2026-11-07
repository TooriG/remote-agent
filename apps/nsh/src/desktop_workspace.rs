//! Native workspace controls consume Core publications and keep Host operations in Core.
use crate::details::WorkspaceSummary;
use crate::{Connection, workbench::Workbench};
use agent_core::{
    panels::PanelKind,
    state::{Intent, operations as op},
};
use agent_protocol::editors::EditorId;
use nsh_core::prelude::*;
use serde_json::json;

pub(crate) fn card() -> impl IntoElement {
    let connection = use_context::<Connection>().unwrap();
    let workspace = use_context::<Workbench>().unwrap();
    let files = use_context::<crate::files::Browser>().unwrap();
    let target = computed(move || {
        connection.composer.with(|p| {
            (
                connection.host.get(),
                p.controls.connected,
                p.controls.navigation.cwd.clone(),
            )
        })
    });
    effect(move || {
        let (host, connected, cwd) = target.get();
        if connected {
            if !cwd.is_empty() {
                connection.dispatch(Intent::ReviewWorkspace(op::ReviewWorkspace { cwd }));
            }
            if host == "local"
                && !connection
                    .publication
                    .with_untracked(|p| p.snapshot.files.file_open_capabilities_requested)
            {
                connection.dispatch(Intent::ReadFileOpenCapabilities(
                    op::ReadFileOpenCapabilities {},
                ));
            }
        }
    });
    let summary = computed(move || {
        connection
            .publication
            .with(|p| crate::details::workspace_summary(p, connection.host.with(|h| h == "local")))
    });
    let act = move |action: Action| {
        let (cwd, home) = connection
            .publication
            .with_untracked(|p| (p.composer.controls.navigation.cwd.clone(), p.context.home()));
        match action {
            Action::Refresh => {
                connection.dispatch(Intent::ReviewWorkspace(op::ReviewWorkspace { cwd }))
            }
            Action::Diff => workspace.open(PanelKind::Diff, Some(cwd), None),
            Action::Files if !cwd.is_empty() => workspace.open(PanelKind::Files, Some(cwd), None),
            Action::Files => {
                let choice = pick_paths(nsh_core::file_dialog::OpenDialog::folders());
                spawn(async move {
                    match choice.await {
                        Ok(Some(paths)) => {
                            if let Some(path) = paths.into_iter().next() {
                                connection.dispatch(Intent::NewChat {
                                    cwd: path.to_string_lossy().into_owned(),
                                });
                            }
                        }
                        Ok(None) => {}
                        Err(reason) => crate::native::message("フォルダー", reason),
                    }
                });
            }
            Action::Attach => crate::composer::pick_files(
                connection.application.get(),
                connection.host.get_untracked(),
                connection.pane,
                connection
                    .composer
                    .with_untracked(|p| p.controls.navigation.draft_key.clone()),
            ),
            Action::Source(path) => files.open(
                path,
                cwd,
                home,
                connection.host.get_untracked(),
                connection.client.get(),
            ),
            Action::PreferEditor(editor) => {
                connection.dispatch(Intent::SetPreferredEditor { editor })
            }
            Action::OpenEditor(editor) if !cwd.is_empty() => {
                connection.dispatch(Intent::OpenInEditor(op::OpenInEditor {
                    path: cwd,
                    line: None,
                    column: None,
                    editor,
                    reveal: false,
                }))
            }
            Action::OpenEditor(_) => {}
        }
    };
    // The desktop client keeps its (empty) notice list above the card.
    view()
        .w(px(300.0))
        .h_full()
        .p_4()
        .pt(px(32.0))
        .child(summary_card(summary, act))
}

enum Action {
    Refresh,
    Diff,
    Files,
    Attach,
    Source(String),
    OpenEditor(EditorId),
    PreferEditor(EditorId),
}

/// The desktop client's workspace card: the folder, its editor, branch and
/// changes, and the conversation's sources.
fn summary_card(
    summary: Memo<WorkspaceSummary>,
    act: impl Fn(Action) + Copy + 'static,
) -> impl IntoElement {
    use crate::ui::{self, Enabled, Icon, colors};
    let enabled = move || summary.with(|s| s.enabled);
    let has_directory = move || summary.with(|s| s.has_directory);
    let heading = move |title: &'static str, control: ui::Control| {
        view()
            .flex_row()
            .items_center()
            .child(
                text(title)
                    .flex_1()
                    .text_size(px(14.0))
                    .text_color(colors::MUTED),
            )
            .child(control)
    };
    view()
        .gap_2()
        .p_4()
        .rounded(px(18.0))
        .bg(colors::RAISED)
        .text_color(colors::FOREGROUND)
        .child(heading(
            "ワークスペース",
            ui::icon_button(Icon::Refresh, "変更を更新", move || act(Action::Refresh))
                .enabled(move || enabled() && has_directory()),
        ))
        .child(
            row(Icon::FolderOpen, move || summary.with(|s| s.directory.clone()))
                .on_press(move || act(Action::Files))
                .disabled(move || !enabled()),
        )
        .child(when(
            move || summary.with(|s| !s.editors.is_empty()),
            move || {
                view()
                    .flex_row()
                    .items_center()
                    .child(
                        row(Icon::ExternalLink, move || {
                            summary.with(|s| s.editor_label.clone())
                        })
                        .flex_1()
                        .on_press(move || {
                            if let Some(editor) = summary.with_untracked(|s| s.editor) {
                                act(Action::OpenEditor(editor));
                            }
                        })
                        .disabled(move || !has_directory() || summary.with(|s| s.editor.is_none())),
                    )
                    .child(
                        ui::icon_button(Icon::ChevronDown, "エディターを選択", move || {
                            let editors = summary.with_untracked(|s| {
                                s.editors
                                    .iter()
                                    .map(|(id, title)| {
                                        json!({"id": id, "title": title, "selected": Some(*id) == s.editor})
                                    })
                                    .collect::<Vec<_>>()
                            });
                            ui::popup(&json!(editors), move |id: &str| {
                                if let Ok(editor) = serde_json::from_value(json!(id)) {
                                    act(Action::PreferEditor(editor));
                                }
                            })
                        })
                        .enabled(has_directory),
                    )
            },
        ))
        .child(when(
            move || summary.with(|s| !s.branch.is_empty()),
            move || {
                text(move || summary.with(|s| s.branch.clone()))
                    .pl_2()
                    .text_size(px(14.0))
                    .text_color(colors::MUTED)
            },
        ))
        .child(
            row(Icon::Replace, || "変更".to_owned())
                .on_press(move || act(Action::Diff))
                .disabled(move || !enabled() || !has_directory())
                .child(
                    text(move || format!("+{}", summary.with(|s| s.additions)))
                        .text_size(px(14.0))
                        .text_color(nsh_core::element::rgb(0x22c55e)),
                )
                .child(
                    text(move || format!("−{}", summary.with(|s| s.deletions)))
                        .text_size(px(14.0))
                        .text_color(nsh_core::element::rgb(0xef4444)),
                ),
        )
        .child(view().h(px(1.0)).my_2().bg(colors::BORDER))
        .child(heading(
            "ソース",
            ui::icon_button(Icon::Plus, "ソースを追加", move || act(Action::Attach))
                .enabled(enabled),
        ))
        .child(when(
            move || summary.with(|s| s.sources.is_empty()),
            || {
                text("添付ファイルがここに表示されます")
                    .py_2()
                    .text_size(px(14.0))
                    .line_height(px(20.0))
                    .text_color(colors::MUTED)
            },
        ))
        .child(each(
            move || summary.with(|s| s.sources.clone()),
            |(path, _): &(String, String)| path.clone(),
            move |source, _| {
                let (path, title) = source.get_untracked();
                row(Icon::FileText, move || title.clone())
                    .on_press(move || act(Action::Source(path.clone())))
            },
        ))
        .child(
            row(Icon::Folder, || "すべてのファイル".to_owned())
                .on_press(move || act(Action::Files))
                .disabled(move || !enabled()),
        )
}

/// A row of the card: an icon and a label, 24 pt, a fill under the pointer.
fn row(glyph: crate::ui::Icon, title: impl Fn() -> String + 'static) -> nsh_base::ButtonElement {
    use crate::ui::{self, colors};
    nsh_base::button()
        .h(px(24.0))
        .px_2()
        .gap_2()
        .flex_row()
        .items_center()
        .rounded(px(ui::RADIUS))
        .hover(|s| s.bg(colors::GHOST_HOVER))
        .disabled_style(|s| s.opacity(0.5))
        .child(ui::icon(glyph, 14.0))
        .child(
            text(title)
                .flex_1()
                .min_w_0()
                .text_size(px(14.0))
                .number_of_lines(1),
        )
}
