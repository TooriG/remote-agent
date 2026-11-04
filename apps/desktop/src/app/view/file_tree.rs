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
    owner: WeakEntity<Desktop>,
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

impl Desktop {
    pub(super) fn files_tree(
        model: Option<&files::FileBrowserModel>,
        search: &Entity<InputState>,
        focus: &FocusHandle,
        cx: &Context<Self>,
    ) -> AnyElement {
        let Some(model) = model else {
            return div().into_any_element();
        };
        let tree = &model.tree;
        let language = crate::localization::language(cx);
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
                |view, _, cx| {
                    view.file_tree_action(FileTreeAction::Refresh);
                    let panel = view.panels.get(view.panel_scope());
                    if panel.kind() == Panel::File
                        && let Some(path) =
                            panel.selected().and_then(|surface| surface.path.clone())
                    {
                        view.dispatch(Intent::ReadFile(op::ReadFile {
                            path,
                            discard_draft: false,
                        }));
                    }
                    cx.notify();
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
            .on_key_down(cx.listener(|view, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    view.file_tree_action(FileTreeAction::Query {
                        text: String::new(),
                    });
                    view.file_search
                        .update(cx, |input, cx| input.set_value("", window, cx));
                    view.panel_focus.focus(window, cx);
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
                agent_core::localization::localized_ui_text(
                    language,
                    tree.toggle_all_label.clone(),
                ),
                cx,
                |view, _, cx| {
                    view.file_tree_action(FileTreeAction::ToggleAll);
                    cx.notify();
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
                    .update(cx, |view, cx| {
                        range
                            .map(|index| {
                                view.file_tree_row(&key, &rows.rows[index], index, window, cx)
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
                            |view, _, cx| {
                                view.file_tree_action(FileTreeAction::Refresh);
                                cx.notify();
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
                            .child(language.text(
                                "ほかにも一致する項目があります。検索語を絞り込んでください。",
                            )),
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
                    .on_key_down(cx.listener(Self::file_tree_key))
                    .on_prepaint({
                        let owner = owner.clone();
                        move |bounds, _, cx| {
                            let _ = owner.update(cx, |view, _| {
                                if let Some(browser) = &mut view.file_browser {
                                    browser.bounds = bounds;
                                }
                            });
                        }
                    })
                    .child(list),
            )
            .into_any_element()
    }

    fn file_tree_row(
        &self,
        key: &FileTreeKey,
        row: &FileTreeRow,
        index: usize,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let directory = row.kind == EntryKind::Directory;
        let focused = self.panel_focus.is_focused(window)
            && self
                .file_browser
                .as_ref()
                .is_some_and(|browser| browser.model.tree.focused_path.as_ref() == Some(&row.path));
        let path = row.path.clone();
        let menu_path = path.clone();
        let menu_key = key.clone();
        let click_key = key.clone();
        let drag = self.file_tree_drag(key, row, cx);
        let search_match = self
            .file_browser
            .as_ref()
            .is_some_and(|browser| browser.model.tree.is_search_match(row));
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
            .on_click(cx.listener(move |view, event: &ClickEvent, window, cx| {
                view.panel_focus.focus(window, cx);
                view.select_file_tree_row(
                    &click_key,
                    &path,
                    event.modifiers().platform || event.modifiers().control,
                    event.modifiers().shift,
                    cx,
                );
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |view, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    view.panel_focus.focus(window, cx);
                    view.file_tree_action(FileTreeAction::Focus {
                        path: menu_path.clone(),
                    });
                    view.show_tree_file_menu(&menu_key, &menu_path, event.position, window, cx);
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
                                let _ = owner.update(cx, |view, cx| {
                                    view.dispatch(Intent::FileTree {
                                        key,
                                        action: FileTreeAction::DragEnd { paths },
                                    });
                                    cx.notify();
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
        &self,
        key: &FileTreeKey,
        row: &FileTreeRow,
        cx: &Context<Self>,
    ) -> Option<FileTreeDrag> {
        let paths = self.file_browser.as_ref()?.model.tree.drag_paths(&row.path);
        if paths.is_empty() {
            return None;
        }
        let mentions = paths
            .iter()
            .map(|path| file_tree_mention(&key.root, &self.snapshot.navigation.cwd, path))
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
}
