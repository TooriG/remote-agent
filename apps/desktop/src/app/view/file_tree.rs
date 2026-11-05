use super::workbench::WorkbenchAction;
use super::*;
use agent_core::presentation::files::{
    FileTreeAction, FileTreeKey, FileTreeRow, file_tree_mention,
};
use agent_protocol::file_tree::EntryKind;

#[derive(Clone)]
pub(in crate::app) struct FileTreeDrag {
    pub mentions: String,
    label: String,
    primary: String,
    key: FileTreeKey,
    paths: Vec<String>,
    owner: WeakEntity<WorkbenchView>,
}
impl Render for FileTreeDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .bg(rgb(appearance::RAISED))
            .border_1()
            .border_color(rgb(appearance::BORDER))
            .text_xs()
            .child(self.label.clone())
    }
}

pub(super) fn render_tree(
    model: Option<&files::FileBrowserModel>,
    search: &Entity<InputState>,
    focus: &FocusHandle,
    cwd: &str,
    cx: &Context<WorkbenchView>,
) -> AnyElement {
    let Some(model) = model else {
        return div().into_any_element();
    };
    let tree = &model.tree;
    let language = crate::localization::language(cx);
    let refresh = model.key.clone();
    let clear = model.key.clone();
    let clear_search = search.clone();
    let clear_focus = focus.clone();
    let toggle = model.key.clone();
    let retry = model.key.clone();
    let keyboard = model.key.clone();
    let keyboard_tree = tree.clone();
    let cwd = cwd.to_owned();
    let row_focus = focus.clone();
    let mut header = h_flex()
        .h(px(40.))
        .min_h(px(40.))
        .px_2()
        .gap_1()
        .border_b_1()
        .border_color(rgb(appearance::BORDER))
        .child(icon_button(
            "refresh-files",
            IconName::RefreshCw,
            language.text(if tree.pending {
                "更新中…"
            } else {
                "ファイル一覧を更新"
            }),
            cx,
            move |_, _, cx| {
                cx.emit(WorkbenchAction::Tree(
                    refresh.clone(),
                    FileTreeAction::Refresh,
                ))
            },
        ))
        .child(
            div().flex_1().min_w_0().child(
                Input::new(search)
                    .small()
                    .appearance(false)
                    .aria_label(language.text("ファイルを検索")),
            ),
        )
        .on_key_down(cx.listener(move |_, event: &KeyDownEvent, window, cx| {
            if event.keystroke.key == "escape" {
                cx.emit(WorkbenchAction::Tree(
                    clear.clone(),
                    FileTreeAction::Query {
                        text: String::new(),
                    },
                ));
                clear_search.update(cx, |input, cx| input.set_value("", window, cx));
                clear_focus.focus(window, cx);
                cx.stop_propagation();
            }
        }));
    if tree.can_expand_all {
        header = header.child(icon_button(
            "expand-files",
            if tree.all_expanded {
                Icon::default().path("icons/chevrons-down-up.svg")
            } else {
                Icon::new(IconName::ChevronsUpDown)
            },
            agent_core::localization::localized_ui_text(language, tree.toggle_all_label.clone()),
            cx,
            move |_, _, cx| {
                cx.emit(WorkbenchAction::Tree(
                    toggle.clone(),
                    FileTreeAction::ToggleAll,
                ))
            },
        ));
    }
    let rows = model.tree.clone();
    let key = model.key.clone();
    let owner = cx.entity().downgrade();
    let list = uniform_list("file-tree-rows", tree.rows.len(), {
        let owner = owner.clone();
        move |range, window, cx| {
            owner
                .update(cx, |_, cx| {
                    range
                        .map(|index| {
                            file_tree_row(&key, &rows, &row_focus, &cwd, index, window, cx)
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        }
    })
    .track_scroll(&model.scroll)
    .px(px(14.4))
    .size_full();
    v_flex()
        .size_full()
        .min_h_0()
        .min_w_0()
        .child(header)
        .when_some(tree.error.as_ref(), |body, error| {
            body.child(
                v_flex().child(error_view(error, language)).child(
                    button(
                        "file-tree-error",
                        language.text("再試行"),
                        cx,
                        move |_, _, cx| {
                            cx.emit(WorkbenchAction::Tree(
                                retry.clone(),
                                FileTreeAction::Refresh,
                            ))
                        },
                    )
                    .text_xs()
                    .w_full(),
                ),
            )
        })
        .when(
            tree.truncated && !tree.pending && !tree.query.trim().is_empty(),
            |body| {
                body.child(
                    div()
                        .px_3()
                        .py_1()
                        .text_xs()
                        .text_color(rgb(appearance::MUTED))
                        .child(
                            language.text(
                                "ほかにも一致する項目があります。検索語を絞り込んでください。",
                            ),
                        ),
                )
            },
        )
        .when(tree.rows.is_empty(), |body| {
            body.child(
                div()
                    .px_3()
                    .py_4()
                    .text_xs()
                    .text_color(rgb(appearance::MUTED))
                    .child(language.text(if !tree.ready || tree.pending {
                        "ファイルを読み込み中…"
                    } else {
                        "一致するファイルはありません"
                    })),
            )
        })
        .child(
            div()
                .flex_1()
                .min_h_0()
                .min_w_0()
                .id("file-tree-navigation")
                .role(Role::Tree)
                .aria_label("Files")
                .track_focus(focus)
                .on_key_down(cx.listener(move |_, event: &KeyDownEvent, _, cx| {
                    let key = if event.keystroke.key == "f10" && event.keystroke.modifiers.shift {
                        "contextmenu"
                    } else {
                        event.keystroke.key.as_str()
                    };
                    let additive =
                        event.keystroke.modifiers.control || event.keystroke.modifiers.platform;
                    let shift = event.keystroke.modifiers.shift;
                    if keyboard_tree.handles_desktop_key(key, additive, shift) {
                        cx.emit(WorkbenchAction::Tree(
                            keyboard.clone(),
                            FileTreeAction::Keyboard {
                                key: key.into(),
                                additive,
                                shift,
                            },
                        ));
                        cx.stop_propagation();
                    }
                }))
                .on_prepaint({
                    let owner = owner.clone();
                    move |bounds, _, cx| {
                        let _ =
                            owner.update(cx, |_, cx| cx.emit(WorkbenchAction::TreeBounds(bounds)));
                    }
                })
                .child(list),
        )
        .into_any_element()
}

fn file_tree_row(
    key: &FileTreeKey,
    tree: &agent_core::presentation::files::FileTreeView,
    focus: &FocusHandle,
    cwd: &str,
    index: usize,
    window: &mut Window,
    cx: &Context<WorkbenchView>,
) -> AnyElement {
    let row = &tree.rows[index];
    let directory = row.kind == EntryKind::Directory;
    let focused = focus.is_focused(window) && tree.focused_path.as_ref() == Some(&row.path);
    let click_focus = focus.clone();
    let menu_focus = focus.clone();
    let path = row.path.clone();
    let menu_path = path.clone();
    let menu_key = key.clone();
    let click_key = key.clone();
    let drag = file_tree_drag(key, tree, cwd, row, cx);
    let search_match = tree.is_search_match(row);
    let icon = if directory {
        let icon = Icon::default()
            .path(if row.expanded {
                "icons/file-icons/chevron-down.svg"
            } else {
                "icons/file-icons/chevron-right.svg"
            })
            .size(px(16.));
        if row.expanded && row.loading {
            Icon::default()
                .path("icons/file-icons/loading.svg")
                .size(px(16.))
                .when(row.ignored, |icon| {
                    icon.text_color(super::file_icon::ignored_ink(cx.theme().is_dark()))
                })
                .opacity(0.6)
                .with_animation(
                    "loading-folder",
                    Animation::new(std::time::Duration::from_secs(1)).repeat(),
                    |icon, delta| icon.transform(Transformation::rotate(percentage(delta))),
                )
                .into_any_element()
        } else {
            icon.into_any_element()
        }
    } else {
        super::file_icon::file_icon(&row.path, true, row.ignored, cx)
    };
    h_flex()
        .relative()
        .when(focused, |row| {
            row.child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(rgb(0x009fff)),
            )
        })
        .id(SharedString::from(format!("file-tree-{}", row.path)))
        .debug_selector(move || format!("workspace-file-{index}"))
        .role(Role::TreeItem)
        .aria_label(row.label.clone())
        .aria_selected(row.selected)
        .when(directory, |element| element.aria_expanded(row.expanded))
        .h(px(24.))
        .w_full()
        .min_w_0()
        .pl(px(6.4 + row.depth as f32 * 18.7))
        .pr(px(6.4))
        .mx(px(1.6))
        .gap(px(4.8))
        .rounded(px(6.))
        .text_size(px(12.))
        .cursor_pointer()
        .text_color(if row.ignored {
            super::file_icon::ignored_ink(cx.theme().is_dark())
        } else {
            rgb(appearance::FOREGROUND).into()
        })
        .when(search_match, |row| row.font_semibold())
        .when(row.selected, |element| {
            element.bg(rgb(appearance::FOREGROUND).opacity(0.12))
        })
        .when(!row.selected, |element| {
            element.hover(|style| style.bg(rgb(appearance::FOREGROUND).opacity(0.07)))
        })
        .child(
            div()
                .size(px(16.))
                .flex_shrink_0()
                .text_color(rgb(if row.selected {
                    appearance::FOREGROUND
                } else {
                    0x84848a
                }))
                .when(row.ignored, |icon| icon.opacity(0.5))
                .child(icon),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_ellipsis()
                .child(row.label.clone()),
        )
        .on_click(cx.listener(move |_, event: &ClickEvent, window, cx| {
            click_focus.focus(window, cx);
            cx.emit(WorkbenchAction::TreeSelect(
                click_key.clone(),
                path.clone(),
                event.modifiers().platform || event.modifiers().control,
                event.modifiers().shift,
            ));
        }))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |_, event: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                menu_focus.focus(window, cx);
                cx.emit(WorkbenchAction::Tree(
                    menu_key.clone(),
                    FileTreeAction::Focus {
                        path: menu_path.clone(),
                    },
                ));
                cx.emit(WorkbenchAction::TreeMenu(
                    menu_key.clone(),
                    menu_path.clone(),
                    event.position,
                ));
            }),
        )
        .when_some(drag, |row, drag| {
            row.on_drag(drag, |drag, _, _, cx| {
                cx.stop_propagation();
                let drag = drag.clone();
                let _ = drag.owner.update(cx, |view, _| {
                    view.dispatch(Intent::FileTree {
                        key: drag.key.clone(),
                        action: FileTreeAction::DragStart {
                            path: drag.primary.clone(),
                        },
                    });
                });
                cx.new(|cx| {
                    let owner = drag.owner.clone();
                    let key = drag.key.clone();
                    let paths = drag.paths.clone();
                    cx.on_release(move |_, cx| {
                        cx.defer(move |cx| {
                            let _ = owner.update(cx, |view, _| {
                                view.dispatch(Intent::FileTree {
                                    key,
                                    action: FileTreeAction::DragEnd { paths },
                                });
                            });
                        })
                    })
                    .detach();
                    drag
                })
            })
        })
        .into_any_element()
}
fn file_tree_drag(
    key: &FileTreeKey,
    tree: &agent_core::presentation::files::FileTreeView,
    cwd: &str,
    row: &FileTreeRow,
    cx: &Context<WorkbenchView>,
) -> Option<FileTreeDrag> {
    let paths = tree.drag_paths(&row.path);
    if paths.is_empty() {
        return None;
    }
    let mentions = paths
        .iter()
        .map(|path| file_tree_mention(&key.root, cwd, path))
        .collect::<Vec<_>>()
        .join(" ");
    Some(FileTreeDrag {
        mentions,
        primary: row.path.clone(),
        label: if paths.len() == 1 {
            row.label.clone()
        } else {
            format!("{} items", paths.len())
        },
        key: key.clone(),
        paths,
        owner: cx.entity().downgrade(),
    })
}
