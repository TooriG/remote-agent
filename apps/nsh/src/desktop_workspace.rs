//! Native workspace controls consume Core publications and keep Host operations in Core.
use crate::details::WorkspaceSummary;
use crate::l10n::tr;
use crate::{Connection, workbench::Workbench};
use agent_core::{
    panels::PanelKind,
    presentation::composer::ComposerReview,
    state::{Intent, operations as op},
};
use agent_protocol::{editors::EditorId, models::ChangedFile};
use nsh_core::prelude::*;
use serde_json::json;

/// Starts a new conversation in a folder the user picks: in a WSL distro's
/// folder on that distro's Host, from which the dialog starts while it is chosen.
pub(crate) fn choose_folder(connection: Connection) {
    let dialog = nsh_core::file_dialog::OpenDialog::folders();
    let start =
        agent_local_host::wsl::host_distro(&connection.host.get_untracked()).and_then(|distro| {
            let cwd = connection
                .composer
                .with_untracked(|c| c.controls.navigation.cwd.clone());
            let cwd = if cwd.is_empty() {
                connection
                    .context
                    .with_untracked(|c| c.home())
                    .unwrap_or_else(|| "/home".into())
            } else {
                cwd
            };
            agent_local_host::wsl::to_unc(distro, &cwd).ok()
        });
    let choice = pick_paths(match start {
        Some(start) => dialog.directory(start),
        None => dialog,
    });
    spawn(async move {
        let picked = match choice.await {
            Ok(Some(paths)) => paths.into_iter().next(),
            Ok(None) => None,
            Err(reason) => return crate::native::message(tr("フォルダー"), reason),
        };
        let Some(path) = picked else {
            return;
        };
        match connection
            .application
            .with(|application| application.open_wsl_folder(&path, |cwd| Intent::NewChat { cwd }))
        {
            Ok(true) => {}
            Ok(false) => connection.dispatch(Intent::NewChat {
                cwd: path.to_string_lossy().into_owned(),
            }),
            Err(reason) => crate::native::message(tr("フォルダー"), reason),
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
            tr("ワークスペース"),
            ui::icon_button(Icon::REFRESH, tr("変更を更新"), move || act(Action::Refresh))
                .enabled(move || enabled() && has_directory()),
        ))
        .child(
            row(Icon::FOLDER_OPEN, move || summary.with(|s| s.directory.clone()))
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
                        row(Icon::EXTERNAL_LINK, move || {
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
                        ui::icon_button(Icon::CHEVRON_DOWN, tr("エディターを選択"), move || {
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
            row(Icon::REPLACE, || tr("変更").to_owned())
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
            tr("ソース"),
            ui::icon_button(Icon::PLUS, tr("ソースを追加"), move || act(Action::Attach))
                .enabled(enabled),
        ))
        .child(when(
            move || summary.with(|s| s.sources.is_empty()),
            || {
                text(tr("添付ファイルがここに表示されます"))
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
                row(Icon::FILE_TEXT, move || title.clone())
                    .on_press(move || act(Action::Source(path.clone())))
            },
        ))
        .child(
            row(Icon::FOLDER, || tr("すべてのファイル").to_owned())
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

/// The conversation's changed files above its composer, as T3 Code's card: the
/// totals and the first files, each opening the diff.
pub(crate) fn review_card() -> impl IntoElement {
    use crate::ui::colors;
    let connection = use_context::<Connection>().unwrap();
    let workspace = use_context::<Workbench>().unwrap();
    let expanded = signal(false);
    let review = computed(move || {
        let expanded = expanded.get();
        connection
            .publication
            .with(|p| p.snapshot.composer_review(expanded))
    });
    let connected = move || connection.composer.with(|p| p.controls.connected);
    let open = move || {
        let cwd = connection
            .composer
            .with_untracked(|p| p.controls.navigation.cwd.clone());
        workspace.open(PanelKind::Diff, Some(cwd), None);
    };
    let folded = move || review.with(|r| r.as_ref().and_then(|r| r.folded));
    when(
        move || review.with(Option::is_some),
        move || {
            view()
                .w_full()
                .rounded(px(12.0))
                .border_1()
                .border_color(colors::BORDER)
                .overflow_hidden()
                .bg(colors::RAISED)
                .text_color(colors::FOREGROUND)
                .child(review_summary(review, connected, open))
                .child(review_files(review, open))
                .child(when(
                    move || folded().is_some(),
                    move || review_toggle(expanded, folded),
                ))
        },
    )
}

/// The card's heading: how many files changed and their lines, and the review.
fn review_summary(
    review: Memo<Option<ComposerReview>>,
    connected: impl Fn() -> bool + Copy + 'static,
    open: impl Fn() + Copy + 'static,
) -> nsh_base::ButtonElement {
    use crate::{
        l10n::tr_format,
        ui::{self, Enabled, Icon, colors},
    };
    use agent_core::presentation::conversation::ActionTone;
    let total =
        move |count: fn(&ComposerReview) -> u64| move || review.with(|r| r.as_ref().map(count));
    nsh_base::button()
        .accessibility_label(Some(SharedString::from(tr("変更を表示"))))
        .flex_row()
        .items_center()
        .gap_3()
        .p_3()
        .border_b_1()
        .border_b_color(colors::BORDER)
        .on_press(open)
        .child(view().p_2().child(ui::icon(Icon::REPLACE, 16.0)))
        .child(
            view()
                .flex_1()
                .min_w_0()
                .gap_1()
                .child(
                    text(move || {
                        let files = review.with(|r| r.as_ref().map_or(0, |r| r.files));
                        tr_format("{0} 件のファイルを変更", [&files])
                    })
                    .text_size(px(14.0)),
                )
                .child(counts(total(|r| r.additions), total(|r| r.deletions))),
        )
        .child(
            ui::action_button(tr("レビューする"), ActionTone::Secondary, open)
                .rounded(px(8.0))
                .enabled(connected),
        )
}

/// The listed files, each with its lines; any of them opens the diff.
fn review_files(
    review: Memo<Option<ComposerReview>>,
    open: impl Fn() + Copy + 'static,
) -> impl IntoElement {
    use crate::ui::colors;
    scroll().max_h(px(252.0)).child(view().py_1().child(each(
        move || review.with(|r| r.as_ref().map(|r| r.rows.clone()).unwrap_or_default()),
        |file: &ChangedFile| file.path.clone(),
        move |file, _| {
            let ChangedFile {
                path,
                additions,
                deletions,
                ..
            } = file.get_untracked();
            nsh_base::button()
                .accessibility_label(Some(SharedString::from(path.clone())))
                .flex_row()
                .items_center()
                .flex_shrink_0()
                .gap_3()
                .px_3()
                .h(px(36.0))
                .hover(|s| s.bg(colors::GHOST_HOVER))
                .on_press(open)
                .child(
                    text(path)
                        .flex_1()
                        .min_w_0()
                        .text_size(px(14.0))
                        .number_of_lines(1),
                )
                .child(counts(move || additions, move || deletions))
        },
    )))
}

/// Shows the files past the first ones, or folds them away again.
fn review_toggle(
    expanded: Signal<bool>,
    folded: impl Fn() -> Option<usize> + Copy + 'static,
) -> View {
    use crate::{
        l10n::tr_format,
        ui::{self, Icon, colors},
    };
    let label = move || {
        if expanded.get() {
            tr("折りたたむ").to_owned()
        } else {
            tr_format(
                "あと {0} 個のファイルを表示",
                [&folded().unwrap_or_default()],
            )
        }
    };
    view().px_2().py_1().child(
        nsh_base::button()
            .accessibility_label(nsh_core::element::Prop::from(
                (move || Some(SharedString::from(label()))).into_reactive(),
            ))
            .flex_row()
            .items_center()
            .self_start()
            .gap_2()
            .h(px(28.0))
            .px_2()
            .rounded(px(ui::RADIUS))
            .hover(|s| s.bg(colors::GHOST_HOVER))
            .on_press(move || expanded.update(|expanded| *expanded = !*expanded))
            .child(when(expanded, || ui::icon(Icon::CHEVRON_UP, 14.0)))
            .child(when(
                move || !expanded.get(),
                || ui::icon(Icon::CHEVRON_DOWN, 14.0),
            ))
            .child(text(label).text_size(px(13.0))),
    )
}

/// A file's added and removed lines, as Git counts them; a binary file has none.
fn counts(
    additions: impl Fn() -> Option<u64> + Copy + 'static,
    deletions: impl Fn() -> Option<u64> + Copy + 'static,
) -> View {
    use crate::ui::colors;
    let lines = move || additions().zip(deletions());
    view()
        .flex_row()
        .gap_1()
        .child(when(
            move || lines().is_some(),
            move || {
                view()
                    .flex_row()
                    .gap_1()
                    .child(
                        text(move || format!("+{}", additions().unwrap_or_default()))
                            .text_size(px(12.0))
                            .text_color(nsh_core::element::rgb(0x37cf77)),
                    )
                    .child(
                        text(move || format!("−{}", deletions().unwrap_or_default()))
                            .text_size(px(12.0))
                            .text_color(nsh_core::element::rgb(0xff6259)),
                    )
            },
        ))
        .child(when(
            move || lines().is_none(),
            || {
                text(tr("バイナリ"))
                    .text_size(px(12.0))
                    .text_color(colors::MUTED)
            },
        ))
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
        items.push(json!({"id": CHOOSE_FOLDER, "title": tr("フォルダを選択…")}));
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
                // The project menu takes the template's slot, wherever the language puts it.
                let headline = tr("{0}で何を作りますか？");
                let (before, after) = headline.split_once("{0}").unwrap_or((headline, ""));
                view()
                    .flex_row()
                    .items_center()
                    .justify_center()
                    .max_w(relative(1.0))
                    .child(text(before).text_size(px(30.0)))
                    .child(project_menu(project, 30.0, colors::FOREGROUND, choose))
                    .child(text(after).text_size(px(30.0)))
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
                    .child(text(tr("プロジェクトなしで始める")).text_size(px(14.0)))
            },
        ))
        .child(when(chat, || {
            text(tr("何に取り組みますか？")).text_size(px(30.0))
        }))
        .child(when(chat, move || {
            project_menu(
                || tr("プロジェクトなし").to_owned(),
                14.0,
                colors::MUTED,
                choose,
            )
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
        .accessibility_label(Some(SharedString::from(tr("プロジェクトを選択"))))
        .on_press(choose)
        .child(
            text(label)
                .text_size(px(size))
                .font_weight(FontWeight::MEDIUM)
                .number_of_lines(1)
                .min_w_0(),
        )
        .child(ui::icon(Icon::CHEVRON_DOWN, (size * 0.5).max(12.0)))
}
