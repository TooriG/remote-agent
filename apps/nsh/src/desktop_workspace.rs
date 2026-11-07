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

/// Starts a new conversation in a folder the user picks.
pub(crate) fn choose_folder(connection: Connection) {
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
            Action::Files => choose_folder(connection),
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

/// Picks a folder rather than one of the projects in the draft's menu.
const CHOOSE_FOLDER: &str = "choose-folder";

/// A new conversation's headline above the centered composer, as T3 Code's
/// draft hero: the project is a menu of this Host's projects.
pub(crate) fn draft_hero() -> View {
    use crate::ui::{self, colors};
    let connection = use_context::<Connection>().unwrap();
    let directory = computed(move || connection.context.with(|p| p.selected_directory()));
    let projects = computed(move || {
        let host = connection.host.get();
        connection.state.with(|state| {
            state
                .list_inputs()
                .into_iter()
                .filter(|(list_host, _)| *list_host == host)
                .flat_map(|(_, list)| {
                    list.projects
                        .iter()
                        .filter_map(|project| {
                            Some((project.name.clone(), project.roots.first()?.path.clone()))
                        })
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        })
    });
    let project = move || {
        let directory = directory.get();
        projects.with(|projects| {
            projects
                .iter()
                .find(|(_, root)| *root == directory)
                .map(|(name, _)| name.clone())
                .unwrap_or_else(|| {
                    agent_core::presentation::markdown::file_basename(&directory).to_owned()
                })
        })
    };
    let choose = move || {
        let selected = directory.get_untracked();
        let mut items = projects
            .get_untracked()
            .into_iter()
            .map(|(name, root)| json!({"id": root, "title": name, "selected": root == selected}))
            .collect::<Vec<_>>();
        if !items.is_empty() {
            items.push(json!({"separator": true}));
        }
        items.push(json!({"id": CHOOSE_FOLDER, "title": "フォルダを選択…"}));
        ui::popup(&json!(items), move |id| {
            if id == CHOOSE_FOLDER {
                choose_folder(connection);
            } else if id != selected {
                connection.dispatch(Intent::NewChat { cwd: id.to_owned() });
            }
        });
    };
    let chat = move || directory.with(String::is_empty);
    view()
        .w_full()
        .items_center()
        .pb_4()
        .gap_2()
        .child(when(
            move || !chat(),
            move || {
                view()
                    .flex_row()
                    .items_center()
                    .justify_center()
                    .max_w(relative(1.0))
                    .child(project_menu(project, 30.0, colors::FOREGROUND, choose))
                    .child(text("で何を作りますか？").text_size(px(30.0)))
            },
        ))
        .child(when(
            move || !chat(),
            move || {
                nsh_base::button()
                    .px_1()
                    .text_color(colors::MUTED)
                    .hover(|s| s.text_color(colors::FOREGROUND))
                    .on_press(move || connection.dispatch(Intent::NewChat { cwd: String::new() }))
                    .child(text("プロジェクトなしで始める").text_size(px(14.0)))
            },
        ))
        .child(when(chat, || text("何に取り組みますか？").text_size(px(30.0))))
        .child(when(chat, move || {
            project_menu(|| "プロジェクトなし".to_owned(), 14.0, colors::MUTED, choose)
        }))
}

/// The draft's project, as a menu button in the headline's type.
fn project_menu(
    label: impl Fn() -> String + Copy + 'static,
    size: f32,
    color: Rgba,
    choose: impl Fn() + 'static,
) -> nsh_base::ButtonElement {
    use crate::ui::{self, Icon, colors};
    nsh_base::button()
        .flex_row()
        .items_center()
        .gap_1()
        .min_w_0()
        .px_1()
        .rounded(px(ui::RADIUS))
        .text_color(color)
        .hover(|s| s.bg(colors::GHOST_HOVER))
        .accessibility_label(Some(SharedString::from("プロジェクトを選択")))
        .on_press(choose)
        .child(
            text(label)
                .text_size(px(size))
                .font_weight(FontWeight::MEDIUM)
                .number_of_lines(1)
                .min_w_0(),
        )
        .child(ui::icon(Icon::ChevronDown, (size * 0.5).max(12.0)))
}
