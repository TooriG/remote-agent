use super::*;

fn panel_icon(kind: Panel) -> IconName {
    match kind {
        Panel::Browser => IconName::Globe,
        Panel::Terminal => IconName::SquareTerminal,
        Panel::Files | Panel::File => IconName::Folder,
        Panel::Diff => IconName::Replace,
        Panel::SideChat => IconName::Bot,
        Panel::Agents => IconName::Network,
        Panel::Home => IconName::Plus,
    }
}

impl Desktop {
    fn panel_tools(&self) -> Vec<agent_core::panels::PanelTool> {
        agent_core::panels::panel_tools(
            self.snapshot.connected,
            !self.snapshot.navigation.cwd.is_empty(),
        )
    }
    pub(super) fn open_review(&mut self, path: Option<&str>, cx: &mut Context<Self>) {
        let id = self.panels.open(self.panel_scope(), Panel::Diff, None);
        self.tab = Tab::Chat;
        let diff = self.workspace_diff(id, cx);
        if let Some(path) = path {
            diff.update(cx, |diff, cx| diff.reveal_path(path, cx));
        }
        self.refresh_review();
        cx.notify();
    }

    pub(super) fn review_card(&self, cx: &Context<Self>) -> AnyElement {
        let review = self.snapshot.workspace.review.as_deref();
        let files = review
            .map(|review| review.files.as_slice())
            .unwrap_or_default();
        let visible = if self.review_expanded {
            files.len()
        } else {
            files.len().min(3)
        };
        v_flex()
            .w_full()
            .rounded(px(12.))
            .border_1()
            .border_color(rgb(appearance::BORDER))
            .overflow_hidden()
            .bg(rgb(appearance::RAISED))
            .child(
                h_flex()
                    .id("review-summary")
                    .cursor_pointer()
                    .on_click(cx.listener(|s, _, _, cx| {
                        s.open_review(None, cx);
                        cx.notify();
                    }))
                    .gap_3()
                    .p_3()
                    .bg(rgb(appearance::RAISED))
                    .border_b_1()
                    .border_color(rgb(appearance::BORDER))
                    .child(
                        div()
                            .p_2()
                            .rounded(px(9.))
                            .bg(rgb(appearance::RAISED))
                            .child(Icon::new(IconName::Replace).size_4()),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child(
                                div()
                                    .text_sm()
                                    .child(format!("{} 件のファイルを変更", files.len())),
                            )
                            .child(review_counts(
                                Some(review.map_or(0, |review| review.additions)),
                                Some(review.map_or(0, |review| review.deletions)),
                            )),
                    )
                    .child(
                        self.button("review-changes", "レビューする", cx, |s, _, cx| {
                            cx.stop_propagation();
                            s.open_review(None, cx);
                        })
                        .border_1()
                        .rounded(px(8.))
                        .disabled(!self.snapshot.connected),
                    ),
            )
            .child(
                v_flex()
                    .id("review-file-list")
                    .max_h(px(252.))
                    .overflow_y_scroll()
                    .py_1()
                    .children(files.iter().take(visible).map(|file| {
                        let path = file.path.clone();
                        h_flex()
                            .id(SharedString::from(format!("review-file-{}", file.path)))
                            .cursor_pointer()
                            .on_click(cx.listener(move |s, _, _, cx| {
                                s.open_review(Some(&path), cx);
                                cx.notify();
                            }))
                            .px_3()
                            .h(px(36.))
                            .flex_shrink_0()
                            .gap_3()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_sm()
                                    .text_ellipsis()
                                    .child(file.path.clone()),
                            )
                            .child(review_counts(file.additions, file.deletions))
                    })),
            )
            .when(files.len() > 3, |card| {
                card.child(
                    div().px_2().py_1().bg(rgb(appearance::RAISED)).child(
                        self.button(
                            "expand-review-files",
                            if self.review_expanded {
                                "折りたたむ".to_owned()
                            } else {
                                format!("あと {} 個のファイルを表示", files.len() - 3)
                            },
                            cx,
                            |s, _, _| s.review_expanded = !s.review_expanded,
                        )
                        .icon(if self.review_expanded {
                            IconName::ChevronUp
                        } else {
                            IconName::ChevronDown
                        }),
                    ),
                )
            })
            .into_any_element()
    }
    pub(super) fn files(&self, cx: &Context<Self>) -> AnyElement {
        let mut entries = v_flex().gap_1();
        for (i, entry) in self
            .snapshot
            .workspace
            .directory
            .as_ref()
            .map(|directory| directory.entries.as_slice())
            .unwrap_or_default()
            .iter()
            .enumerate()
        {
            let path = entry.path.clone();
            let download = path.clone();
            let directory = entry.directory;
            entries = entries.child(
                h_flex()
                    .w_full()
                    .gap_1()
                    .child(
                        self.button(
                            format!("file-{i}"),
                            entry.name.clone(),
                            cx,
                            move |s, _, cx| {
                                if directory {
                                    s.browse(path.clone());
                                } else {
                                    s.open_file(PathBuf::from(&path), cx);
                                }
                            },
                        )
                        .icon(if directory {
                            IconName::Folder
                        } else {
                            IconName::FileText
                        })
                        .flex_1()
                        .debug_selector(move || format!("workspace-file-{i}")),
                    )
                    .when(!directory, |row| {
                        row.child(
                            Button::new(SharedString::from(format!("download-{i}")))
                                .icon(IconName::ArrowDown)
                                .small()
                                .ghost()
                                .tooltip("ダウンロード")
                                .accessibility_label("ダウンロード")
                                .on_click(cx.listener(move |s, _, _, cx| {
                                    s.download(download.clone());
                                    cx.notify();
                                })),
                        )
                    }),
            );
        }
        v_flex()
            .size_full()
            .min_h_0()
            .p_4()
            .gap_3()
            .child(
                h_flex()
                    .gap_2()
                    .child(Self::icon_button(
                        "parent-directory",
                        IconName::ArrowUp,
                        "親フォルダ",
                        cx,
                        |s, _, cx| {
                            let path = s.path.read(cx).value();
                            s.browse(
                                Path::new(path.as_ref())
                                    .parent()
                                    .unwrap_or(Path::new("/"))
                                    .to_string_lossy()
                                    .into_owned(),
                            );
                        },
                    ))
                    .child(Input::new(&self.path).small().aria_label("フォルダのパス"))
                    .child(Self::icon_button(
                        "browse",
                        IconName::ArrowRight,
                        "フォルダを開く",
                        cx,
                        |s, _, cx| s.browse(s.path.read(cx).value().to_string()),
                    )),
            )
            .child(
                div()
                    .id("file-list")
                    .debug_selector(|| "conversation-files".into())
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(entries),
            )
            .into_any_element()
    }
    fn file_editor(&self, cx: &Context<Self>) -> AnyElement {
        let mut editor = v_flex()
            .debug_selector(|| "conversation-file".into())
            .flex_1()
            .min_h_0()
            .min_w_0()
            .gap_2();
        let panel = self.panels.get(self.panel_scope());
        let selected = panel.selected().and_then(|surface| surface.path.as_deref());
        if let Some(file) = self
            .snapshot
            .workspace
            .file
            .as_ref()
            .filter(|file| selected == Some(file.path.as_str()))
        {
            editor = editor
                .child(
                    h_flex().gap_2().child(IconName::FileText).child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .text_ellipsis()
                            .child(file.path.clone()),
                    ),
                )
                .child(
                    div().flex_1().min_h_0().child(
                        Editor::new(&self.editor_input)
                            .h_full()
                            .aria_label("ファイル編集"),
                    ),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .child(
                            self.button("save-file", "保存", cx, |s, _, _| s.save_file())
                                .primary()
                                .disabled(self.busy > 0),
                        )
                        .child(Self::icon_button(
                            "reload-file",
                            IconName::RotateCw,
                            "再読み込み・下書きを破棄",
                            cx,
                            |s, _, _| {
                                if let Some(file) = &s.snapshot.workspace.file {
                                    s.edit(file.path.clone(), true);
                                }
                            },
                        ))
                        .child(
                            self.button("ai-edit", "AI に編集を依頼", cx, |s, _, _| {
                                if let Some(file) = &s.snapshot.workspace.file {
                                    s.dispatch(Intent::SetDraftText {
                                        thread_id: s.draft_key().clone(),
                                        text: format!(
                                            "ファイル {} を編集してください。\n\n",
                                            file.path
                                        ),
                                    });
                                }
                                s.tab = Tab::Chat;
                            })
                            .icon(IconName::Bot),
                        ),
                );
        } else {
            editor = editor
                .items_center()
                .justify_center()
                .gap_3()
                .child(
                    Icon::new(IconName::FileText)
                        .size_8()
                        .text_color(rgb(appearance::MUTED)),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(appearance::MUTED))
                        .child("ファイルを読み込み中…"),
                );
        }
        editor.p_4().into_any_element()
    }
    pub(super) fn workbench(&mut self, standalone: bool, cx: &mut Context<Self>) -> AnyElement {
        let panel = self.panels.get(self.panel_scope());
        let layout = panel.layout;
        let active = panel.active;
        let kind = panel.kind();
        let tabs = panel
            .surfaces
            .iter()
            .map(|surface| {
                let id = surface.id;
                let title = match self.surface_resources.get(&id) {
                    Some(SurfaceResource::Browser { view, .. }) => view.read(cx).title(),
                    _ => surface.path.as_ref().map_or_else(
                        || agent_core::panels::panel_title(surface.kind),
                        |path| file_name(path),
                    ),
                };
                h_flex()
                    .gap_1()
                    .rounded_md()
                    .when(Some(id) == active, |tab| tab.bg(rgb(appearance::RAISED)))
                    .child(
                        Button::new(("surface-tab", id as usize))
                            .label(title)
                            .icon(panel_icon(surface.kind))
                            .small()
                            .ghost()
                            .selected(Some(id) == active)
                            .debug_selector(move || format!("surface-tab-{id}"))
                            .on_click(cx.listener(move |view, _, window, cx| {
                                view.activate_surface(id, window, cx)
                            })),
                    )
                    .child(
                        Self::icon_button(
                            format!("close-surface-{id}"),
                            IconName::Close,
                            "タブを閉じる",
                            cx,
                            move |view, window, cx| view.close_surface(id, window, cx),
                        )
                        .debug_selector(move || {
                            if Some(id) == active {
                                "active-surface-close".into()
                            } else {
                                format!("close-surface-{id}")
                            }
                        }),
                    )
            })
            .collect::<Vec<_>>();
        let owner = cx.entity().downgrade();
        let menu_owner = owner.clone();
        let add = Button::new("panel-add")
            .debug_selector(|| "panel-add".into())
            .icon(IconName::Plus)
            .small()
            .ghost()
            .tooltip("機能を開く")
            .accessibility_label("機能を開く")
            .dropdown_menu(move |mut menu, _, cx| {
                let tools = menu_owner
                    .upgrade()
                    .map(|view| view.read(cx).panel_tools())
                    .unwrap_or_default();
                for tool in tools {
                    let owner = menu_owner.clone();
                    menu = menu.item(
                        PopupMenuItem::new(tool.label)
                            .icon(panel_icon(tool.kind))
                            .disabled(!tool.enabled)
                            .on_click(move |_, window, cx| {
                                let _ = owner
                                    .update(cx, |view, cx| view.open_panel(tool.kind, window, cx));
                            }),
                    );
                }
                menu
            });
        let toolbar = h_flex()
            .debug_selector(|| "workbench-header".into())
            .h(px(crate::WINDOW_HEADER_HEIGHT))
            .px_2()
            .gap_1()
            .flex_shrink_0()
            .when(standalone && !self.sidebar, |bar| {
                bar.pl_0().child(sidebar_header(false, cx))
            })
            .child(
                h_flex()
                    .id("surface-tabs")
                    .flex_1()
                    .min_w_0()
                    .overflow_x_scroll()
                    .gap_1()
                    .children(tabs)
                    .child(add),
            )
            .when(!self.side_chat_mode, |bar| {
                bar.child(
                    Self::icon_button(
                        "panel-expand",
                        if layout == PanelLayout::Expanded {
                            IconName::Minimize
                        } else {
                            IconName::Maximize
                        },
                        "パネルを拡大・縮小",
                        cx,
                        |view, _, _| {
                            let key = view.panel_scope();
                            let layout =
                                if view.panels.get(key.clone()).layout == PanelLayout::Expanded {
                                    PanelLayout::Right
                                } else {
                                    PanelLayout::Expanded
                                };
                            view.panels.set_layout(key, layout);
                        },
                    )
                    .debug_selector(|| "panel-expand".into()),
                )
                .child(
                    Self::icon_button(
                        "panel-bottom",
                        IconName::PanelBottom,
                        "下・右の配置を切り替え",
                        cx,
                        move |view, _, _| {
                            let layout = if layout == PanelLayout::Bottom {
                                PanelLayout::Right
                            } else {
                                PanelLayout::Bottom
                            };
                            view.panels.set_layout(view.panel_scope(), layout);
                        },
                    )
                    .debug_selector(|| "panel-bottom".into())
                    .selected(layout == PanelLayout::Bottom),
                )
                .child(Self::panel_toggle(true, cx))
            })
            .when(self.side_chat_mode, |bar| {
                bar.child(
                    Self::icon_button(
                        "back-side-chat",
                        IconName::ArrowLeft,
                        "サイドチャットに戻る",
                        cx,
                        |view, _, _| view.panels.set_open(view.panel_scope(), false),
                    )
                    .debug_selector(|| "back-side-chat".into()),
                )
            });
        let body = match kind {
            Panel::Home => {
                let mut chooser = v_flex()
                    .w_full()
                    .max_w(px(300.))
                    .gap_1()
                    .child(div().text_sm().mb_3().text_center().child("機能を開く"));
                for (ix, tool) in self.panel_tools().into_iter().enumerate() {
                    let panel = tool.kind;
                    chooser = chooser.child(
                        self.button(
                            format!("tool-{ix}"),
                            tool.label,
                            cx,
                            move |view, window, cx| {
                                view.open_panel(panel, window, cx);
                            },
                        )
                        .debug_selector(move || format!("surface-{ix}"))
                        .icon(panel_icon(panel))
                        .w_full()
                        .h_8()
                        .justify_start()
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(appearance::MUTED))
                                .child(tool.shortcut),
                        )
                        .disabled(!tool.enabled),
                    );
                }
                v_flex()
                    .id("surface-launcher")
                    .debug_selector(|| "surface-launcher".into())
                    .size_full()
                    .p_6()
                    .items_center()
                    .justify_center()
                    .track_focus(&self.panel_focus)
                    .on_key_down(cx.listener(|view, event: &KeyDownEvent, window, cx| {
                        if event.keystroke.modifiers.control
                            || event.keystroke.modifiers.platform
                            || event.keystroke.modifiers.alt
                        {
                            return;
                        }
                        let Some(tool) = view.panel_tools().into_iter().find(|tool| {
                            tool.shortcut
                                .eq_ignore_ascii_case(event.keystroke.key.as_str())
                        }) else {
                            return;
                        };
                        if tool.enabled {
                            view.open_panel(tool.kind, window, cx);
                            cx.stop_propagation();
                        }
                    }))
                    .child(chooser)
                    .into_any_element()
            }
            Panel::Agents => self.agents_panel(),
            Panel::Files => self.files(cx),
            Panel::File => self.file_editor(cx),
            Panel::Diff => v_flex()
                .size_full()
                .min_h_0()
                .p_4()
                .gap_3()
                .child(
                    h_flex()
                        .child(div().flex_1().child(format!(
                                "変更  +{} −{}",
                                self.snapshot
                                    .workspace
                                    .review
                                    .as_ref()
                                    .map_or(0, |review| review.additions),
                                self.snapshot
                                    .workspace
                                    .review
                                    .as_ref()
                                    .map_or(0, |review| review.deletions)
                            )))
                        .child(Self::icon_button(
                            "refresh-diff",
                            IconName::RotateCw,
                            "変更を更新",
                            cx,
                            |s, _, _| s.refresh_review(),
                        )),
                )
                .child(self.diff_file_menu(cx))
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .child(self.workspace_diff(active.unwrap(), cx)),
                )
                .into_any_element(),
            Panel::SideChat | Panel::Terminal | Panel::Browser => {
                match active.and_then(|id| self.surface_resources.get(&id)) {
                    Some(SurfaceResource::SideChat(view)) => view.clone().into_any_element(),
                    Some(SurfaceResource::Terminal(view)) => view.clone().into_any_element(),
                    Some(SurfaceResource::Diff(view)) => view.clone().into_any_element(),
                    Some(SurfaceResource::Browser { view, .. }) => view.clone().into_any_element(),
                    Some(SurfaceResource::Pending { .. }) => {
                        div().p_4().child("ブラウザを起動中…").into_any_element()
                    }
                    None => div()
                        .p_4()
                        .child("機能を開けませんでした。「＋」から再試行してください。")
                        .into_any_element(),
                }
            }
        };
        v_flex()
            .size_full()
            .min_h_0()
            .min_w_0()
            .child(toolbar)
            .child(div().flex_1().min_h_0().flex().child(body))
            .into_any_element()
    }
}

impl Desktop {
    fn workspace_diff(&mut self, id: u64, cx: &mut Context<Self>) -> Entity<DiffView> {
        let source = self
            .snapshot
            .workspace
            .review
            .as_ref()
            .map_or("", |review| review.diff.as_str());
        if let Some(SurfaceResource::Diff(view)) = self.surface_resources.get(&id) {
            view.update(cx, |view, cx| view.set_source(source, cx));
            return view.clone();
        }
        let view = cx.new(|_| DiffView::new(source.to_owned().into(), true));
        self.surface_resources
            .insert(id, SurfaceResource::Diff(view.clone()));
        view
    }
    fn diff_file_menu(&self, cx: &Context<Self>) -> AnyElement {
        let entity = cx.entity().downgrade();
        Button::new("diff-file-navigation")
            .debug_selector(|| "diff-file-navigation".into())
            .label(format!(
                "ファイルを選択 ({})",
                self.snapshot
                    .workspace
                    .review
                    .as_ref()
                    .map_or(0, |review| review.files.len())
            ))
            .accessibility_label("差分のファイルを選択")
            .dropdown_caret(true)
            .ghost()
            .dropdown_menu(move |mut menu, _, cx| {
                if let Some(owner) = entity.upgrade()
                    && let Some(review) = &owner.read(cx).snapshot.workspace.review
                {
                    for file in &review.files {
                        let path = file.path.clone();
                        let entity = entity.clone();
                        menu = menu.item(PopupMenuItem::new(path.clone()).on_click(
                            move |_, _, cx| {
                                let _ = entity.update(cx, |s, cx| s.open_review(Some(&path), cx));
                            },
                        ));
                    }
                }
                menu
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[gpui::test]
    fn surface_tabs_and_layout_follow_the_conversation_and_last_close_returns_to_the_launcher(
        cx: &mut TestAppContext,
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        cx.update(|cx| {
            crate::appearance::init(cx);
            cx.set_global(Runtime {
                handle: runtime.handle().clone(),
                connections: Arc::default(),
                closing: tokio_util::task::TaskTracker::new(),
                logging_error: None,
            });
        });
        let (view, window) = cx.add_window_view(|window, cx| {
            let mut desktop = Desktop::new(Mode::Main, window, cx);
            desktop.onboarding = false;
            desktop.panels.set_open(desktop.panel_scope(), true);
            desktop
        });
        window.simulate_resize(size(px(1600.), px(900.)));
        window.run_until_parked();
        assert!(window.debug_bounds("surface-launcher").is_some());
        let chat = window.debug_bounds("conversation-chat").unwrap();
        let title = window.debug_bounds("conversation-header").unwrap();
        let tools = window.debug_bounds("workbench-header").unwrap();
        assert_eq!(title.bottom(), chat.top());
        assert_eq!(tools.top(), title.top());
        assert_eq!(tools.size.height, title.size.height);
        assert!(title.right() <= chat.right());
        assert!(tools.left() >= chat.right());
        let toggle = window.debug_bounds("panel-toggle").unwrap();
        assert!(toggle.left() >= tools.left() && toggle.right() <= tools.right());
        assert!(toggle.left() > window.debug_bounds("panel-bottom").unwrap().right());
        window.simulate_click(toggle.center(), Modifiers::default());
        window.run_until_parked();
        assert!(window.debug_bounds("workbench-header").is_none());
        let title = window.debug_bounds("conversation-header").unwrap();
        assert_eq!(title.right(), tools.right());
        let toggle = window.debug_bounds("panel-toggle").unwrap();
        assert!(toggle.left() >= title.left() && toggle.right() <= title.right());
        window.simulate_click(toggle.center(), Modifiers::default());
        window.run_until_parked();
        assert!(
            window.debug_bounds("panel-add").unwrap().center().x
                < window.debug_bounds("surface-launcher").unwrap().center().x,
            "the empty panel's add button belongs on the left"
        );
        let agents = window.debug_bounds("surface-5").unwrap().center();
        window.simulate_click(agents, Modifiers::default());
        window.run_until_parked();
        assert!(window.debug_bounds("agents-panel").is_some());
        let (a, diff) = view.update(window, |view, cx| {
            let a = view.panel_scope();
            view.open_review(None, cx);
            (a, view.panels.get(view.panel_scope()).active.unwrap())
        });
        window.run_until_parked();
        assert!(window.debug_bounds("diff-file-navigation").is_some());
        let bottom = window.debug_bounds("panel-bottom").unwrap().center();
        window.simulate_click(bottom, Modifiers::default());
        window.run_until_parked();
        let chat = window.debug_bounds("conversation-chat").unwrap();
        let title = window.debug_bounds("conversation-header").unwrap();
        let tools = window.debug_bounds("workbench-header").unwrap();
        assert_eq!(title.bottom(), chat.top());
        assert!(tools.top() >= chat.bottom());
        let diff_menu = window.debug_bounds("diff-file-navigation").unwrap();
        assert!(
            diff_menu.top() >= chat.bottom(),
            "bottom placement must put the surface below the chat"
        );
        view.update(window, |view, cx| {
            assert_eq!(view.panels.get(a.clone()).layout, PanelLayout::Bottom);
            let b = PanelScope {
                draft: DraftKey::from("other-conversation"),
                ..view.panel_scope()
            };
            Arc::make_mut(&mut Arc::make_mut(&mut view.snapshot).navigation).draft_key =
                b.draft.clone();
            assert!(!view.panels.get(b.clone()).open);
            view.panels.set_open(b, true);
            cx.notify();
        });
        window.run_until_parked();
        assert!(window.debug_bounds("surface-launcher").is_some());
        assert!(window.debug_bounds("diff-file-navigation").is_none());
        view.update(window, |view, cx| {
            Arc::make_mut(&mut Arc::make_mut(&mut view.snapshot).navigation).draft_key =
                a.draft.clone();
            assert_eq!(view.panels.get(a.clone()).active, Some(diff));
            assert_eq!(view.panels.get(a.clone()).surfaces.len(), 2);
            cx.notify();
        });
        window.run_until_parked();
        assert!(window.debug_bounds("diff-file-navigation").is_some());
        let bottom = window.debug_bounds("panel-bottom").unwrap().center();
        window.simulate_click(bottom, Modifiers::default());
        window.run_until_parked();
        let chat = window.debug_bounds("conversation-chat").unwrap();
        let title = window.debug_bounds("conversation-header").unwrap();
        let tools = window.debug_bounds("workbench-header").unwrap();
        assert_eq!(tools.top(), title.top());
        assert!(tools.left() >= chat.right());
        let expand = window.debug_bounds("panel-expand").unwrap().center();
        window.simulate_click(expand, Modifiers::default());
        window.run_until_parked();
        assert!(window.debug_bounds("conversation-chat").is_none());
        assert!(window.debug_bounds("conversation-header").is_none());
        assert_eq!(
            window.debug_bounds("workbench-header").unwrap().top(),
            px(0.)
        );
        let toggle = window.debug_bounds("panel-toggle").unwrap().center();
        window.simulate_click(toggle, Modifiers::default());
        window.run_until_parked();
        assert!(window.debug_bounds("workbench-header").is_none());
        assert!(window.debug_bounds("conversation-header").is_some());
        let toggle = window.debug_bounds("panel-toggle").unwrap().center();
        window.simulate_click(toggle, Modifiers::default());
        window.run_until_parked();
        assert!(window.debug_bounds("conversation-header").is_none());
        assert!(window.debug_bounds("diff-file-navigation").is_some());
        let restore = window.debug_bounds("panel-expand").unwrap().center();
        window.simulate_click(restore, Modifiers::default());
        window.run_until_parked();
        assert!(window.debug_bounds("conversation-chat").is_some());
        let close = window
            .debug_bounds("active-surface-close")
            .unwrap()
            .center();
        window.simulate_click(close, Modifiers::default());
        window.run_until_parked();
        assert!(window.debug_bounds("agents-panel").is_some());
        let close = window
            .debug_bounds("active-surface-close")
            .unwrap()
            .center();
        window.simulate_click(close, Modifiers::default());
        window.run_until_parked();
        assert!(window.debug_bounds("surface-launcher").is_some());
        let collapse = window.debug_bounds("collapse-sidebar").unwrap().center();
        window.simulate_click(collapse, Modifiers::default());
        window.run_until_parked();
        let title = window.debug_bounds("conversation-header").unwrap();
        let sidebar = window.debug_bounds("expand-sidebar").unwrap();
        assert!(sidebar.left() >= title.left() && sidebar.right() <= title.right());
        let expand = window.debug_bounds("panel-expand").unwrap().center();
        window.simulate_click(expand, Modifiers::default());
        window.run_until_parked();
        assert!(window.debug_bounds("conversation-header").is_none());
        let tools = window.debug_bounds("workbench-header").unwrap();
        let sidebar = window.debug_bounds("expand-sidebar").unwrap();
        assert!(sidebar.left() >= tools.left() && sidebar.right() <= tools.right());
        let restore = window.debug_bounds("panel-expand").unwrap().center();
        window.simulate_click(restore, Modifiers::default());
        window.simulate_resize(size(px(900.), px(900.)));
        window.run_until_parked();
        assert!(window.debug_bounds("conversation-header").is_none());
        let tools = window.debug_bounds("workbench-header").unwrap();
        let sidebar = window.debug_bounds("expand-sidebar").unwrap();
        assert!(sidebar.left() >= tools.left() && sidebar.right() <= tools.right());
        let close = window.debug_bounds("panel-toggle").unwrap().center();
        window.simulate_click(close, Modifiers::default());
        window.run_until_parked();
        assert!(window.debug_bounds("conversation-header").is_some());
        assert!(window.debug_bounds("workbench-header").is_none());
        let sidebar = window.debug_bounds("expand-sidebar").unwrap().center();
        window.simulate_click(sidebar, Modifiers::default());
        window.run_until_parked();
        assert!(window.debug_bounds("desktop-sidebar-shell").is_some());
    }
}
