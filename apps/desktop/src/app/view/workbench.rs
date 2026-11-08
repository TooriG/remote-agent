use super::*;
use agent_core::panels::{PanelState, PanelTool};
use agent_core::presentation::agents::AgentSummary;
use agent_core::presentation::files::{EXPLORER_CONTENT_MIN_WIDTH, EXPLORER_MIN_WIDTH};
use errors::ErrorRow;

#[derive(PartialEq)]
struct WorkbenchModel {
    standalone: bool,
    details_popover: bool,
    workspace: Entity<workspace::WorkspaceView>,
    panel: PanelState,
    browsers: Vec<(u64, Entity<crate::browser::Browser>)>,
    sidebar: bool,
    side_chat_mode: bool,
    tools: Vec<PanelTool>,
    errors: Vec<ErrorRow>,
    tree: Option<files::FileBrowserModel>,
    explorer_state: Entity<gpui_kit::component::resizable::ResizableState>,
    explorer_width: f32,
    explorer_max: f32,
    explorer_available: f32,
    explorer_focus: FocusHandle,
    cwd: String,
    review_files: Vec<String>,
    editor: FileEditorModel,
    review: Option<(u64, u64, usize)>,
    agents: Arc<Vec<AgentSummary>>,
    resource: Option<AnyView>,
    resource_message: &'static str,
    file_search: Entity<InputState>,
    focus: FocusHandle,
}

#[derive(PartialEq)]
struct FileEditorModel {
    path: Option<String>,
    target: Option<String>,
    failed: bool,
    input: Entity<gpui_kit::component::input::EditorState>,
    saving: bool,
    truncated: bool,
    draft: DraftKey,
}

pub(in crate::app) enum WorkbenchAction {
    Activate(u64),
    Close(u64),
    Open(Panel),
    Layout(PanelLayout),
    Hide,
    Download(String),
    Save,
    Reload(String),
    RefreshReview,
    Review(String),
    Notice(errors::NoticeAction),
    Tree(
        agent_core::presentation::files::FileTreeKey,
        agent_core::presentation::files::FileTreeAction,
    ),
    TreeSelect(
        agent_core::presentation::files::FileTreeKey,
        String,
        bool,
        bool,
    ),
    TreeMenu(
        agent_core::presentation::files::FileTreeKey,
        String,
        Point<Pixels>,
    ),
    TreeBounds(Bounds<Pixels>),
    ExplorerFit(f32),
    ExplorerResize(f32),
    ExplorerCommit,
}
impl EventEmitter<WorkbenchAction> for WorkbenchView {}

pub(in crate::app) struct WorkbenchView {
    desktop: WeakEntity<Desktop>,
    pane: Option<Arc<agent_core::store::Pane>>,
    _explorer: Subscription,
    model: Option<WorkbenchModel>,
    titles: BTreeMap<u64, String>,
    _browsers: Vec<Subscription>,
    #[cfg(test)]
    pub(in crate::app) renders: usize,
}

impl WorkbenchView {
    pub(in crate::app) fn new(
        desktop: &Entity<Desktop>,
        explorer: &Entity<gpui_kit::component::resizable::ResizableState>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe_global::<crate::localization::Language>(|_, cx| cx.notify())
            .detach();
        Self {
            desktop: desktop.downgrade(),
            pane: None,
            _explorer: cx.observe(explorer, |_, _, cx| cx.notify()),
            model: None,
            titles: BTreeMap::new(),
            _browsers: Vec::new(),
            #[cfg(test)]
            renders: 0,
        }
    }

    pub(super) fn dispatch(&self, intent: Intent) {
        if let Some(pane) = &self.pane {
            drop(pane.dispatch(intent));
        }
    }

    fn set_model(&mut self, model: Option<WorkbenchModel>, cx: &mut Context<Self>) {
        if self.model == model {
            return;
        }
        if self.model.as_ref().map(|model| &model.browsers)
            != model.as_ref().map(|model| &model.browsers)
        {
            self.titles = model
                .as_ref()
                .into_iter()
                .flat_map(|model| &model.browsers)
                .map(|(id, browser)| {
                    (
                        *id,
                        browser.read(cx).title(crate::localization::language(cx)),
                    )
                })
                .collect();
            self._browsers = model
                .as_ref()
                .into_iter()
                .flat_map(|model| &model.browsers)
                .map(|(id, browser)| {
                    let id = *id;
                    cx.subscribe(
                        browser,
                        move |view, _, title: &crate::browser::BrowserTitle, cx| {
                            if view.titles.get(&id) != Some(&title.0) {
                                view.titles.insert(id, title.0.clone());
                                cx.notify();
                            }
                        },
                    )
                })
                .collect();
        }
        self.model = model;
        cx.notify();
    }
}

impl Render for WorkbenchView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        {
            self.renders += 1;
        }
        self.model
            .as_ref()
            .map(|model| Self::render_workbench(model, &self.titles, &self.desktop, window, cx))
            .unwrap_or_else(|| div().into_any_element())
    }
}

impl Desktop {
    pub(in crate::app) fn workbench_action(
        &mut self,
        action: &WorkbenchAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use WorkbenchAction::*;
        match action {
            Activate(id) => self.activate_surface(*id, window, cx),
            Close(id) => self.close_surface(*id, window, cx),
            Open(panel) => self.open_panel(*panel, window, cx),
            Layout(layout) => {
                self.panels = self.panels.clone().set_layout(self.panel_scope(), *layout)
            }
            Hide => self.panels = self.panels.clone().set_open(self.panel_scope(), false),
            Download(path) => self.download(path.clone()),
            Save => self.save_file(),
            Reload(path) => {
                self.open_file(path.clone(), None, true, cx);
            }
            RefreshReview => self.refresh_review(),
            Review(path) => self.open_review(Some(path), cx),
            Notice(action) => self.notice_action(action.clone(), window, cx),
            Tree(key, action) => {
                if !self
                    .file_browser
                    .as_ref()
                    .is_some_and(|browser| &browser.model.key == key)
                {
                    return;
                }
                self.dispatch(Intent::FileTree {
                    key: key.clone(),
                    action: action.clone(),
                });
                if matches!(
                    action,
                    agent_core::presentation::files::FileTreeAction::Refresh
                ) {
                    let panel = self.panels.get(self.panel_scope());
                    if panel.kind() == Panel::File
                        && let Some(path) =
                            panel.selected().and_then(|surface| surface.path.clone())
                    {
                        self.dispatch(Intent::ReadFile(op::ReadFile {
                            path,
                            discard_draft: false,
                        }));
                    }
                }
            }
            TreeSelect(key, path, multiple, extend) => {
                self.select_file_tree_row(key, path, *multiple, *extend, cx)
            }
            TreeMenu(key, path, point) => self.show_tree_file_menu(key, path, *point, window, cx),
            TreeBounds(bounds) => {
                if let Some(browser) = &mut self.file_browser {
                    browser.bounds = *bounds;
                }
                return;
            }
            ExplorerFit(available) => self.fit_file_explorer(*available, window, cx),
            ExplorerResize(width) => self.resize_file_explorer(*width, window, cx),
            ExplorerCommit => self.commit_file_explorer_width(window, cx),
        }
        cx.notify();
    }

    fn workbench_visible(&self, open: bool, cx: &App) -> bool {
        self.visible
            && open
            && self.tab == Tab::Chat
            && self.project_editor.is_none()
            && self.media.read(cx).gallery.is_none()
            && !self.onboarding
            && !self.import_projects
    }

    // Resource ownership and visibility are reconciled before drawing, including
    // Desktop state changes and window resizes. Rendering only reads the result.
    pub(in crate::app) fn sync_surfaces(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let scope = self.panel_scope();
        let panel = self.panels.get(scope.clone());
        let panel_open = panel.open;
        let panel_kind = panel.kind();
        let panel_layout = panel.layout;
        let active_surface = panel.active;
        let active = self.workbench_visible(panel_open, cx);
        let thread_id = (active && panel_kind == Panel::Agents)
            .then(|| self.publication.pane.navigation.thread_id.clone())
            .flatten();
        if self.publication.pane.observed_agents != thread_id {
            self.dispatch(Intent::WatchAgents { thread_id });
        }
        let composer_visible = self.visible
            && self.project_editor.is_none()
            && self.media.read(cx).gallery.is_none()
            && self.tab == Tab::Chat
            && (!panel_open
                || (!self.side_chat_mode
                    && panel_layout != PanelLayout::Expanded
                    && (panel_layout == PanelLayout::Bottom
                        || window.viewport_size().width >= px(1080.))));
        if !composer_visible && self.dictation.is_some() {
            self.cancel_recording();
            self.sync_composer(cx);
        }
        if active
            && let Some(id) = active_surface
            && let Some(connection) = self.session.as_ref()
            && let Some(PanelScope {
                draft: DraftKey::Session { session: thread },
                ..
            }) = self.panels.scope(id)
            && let Some(SurfaceResource::Pending { url }) = self.surface_resources.get(&id)
        {
            let browser = crate::browser::Browser::new(
                connection.host.store.clone(),
                thread,
                url.clone(),
                window,
                cx,
            );
            self.surface_resources
                .insert(id, SurfaceResource::Browser(browser));
        }
        if active
            && panel_kind == Panel::Diff
            && let Some(id) = active_surface
        {
            self.workspace_diff(id, cx);
        }
        for (id, resource) in &self.surface_resources {
            match resource {
                SurfaceResource::Browser(view) => view.update(cx, |view, cx| {
                    view.set_visible(active && Some(*id) == active_surface, cx);
                }),
                SurfaceResource::SideChat(chat) => {
                    chat.update(cx, |chat, cx| {
                        chat.set_visible(active && Some(*id) == active_surface, window, cx)
                    });
                }
                SurfaceResource::Terminal(view)
                    if self.snapshot.connection.connected
                        && self.panels.scope(*id).is_some_and(|owner| {
                            owner.host == scope.host && owner.storage == scope.storage
                        }) =>
                {
                    if let Some(session) = &self.session {
                        view.update(cx, |view, cx| {
                            view.connect(session.host.store.clone(), window, cx)
                        });
                    }
                }
                _ => (),
            }
        }
        self.sync_file_browser(
            active && matches!(panel_kind, Panel::Files | Panel::File),
            window,
            cx,
        );
        // Like T3, a details popover closes once the card can dock again, so
        // it does not reopen unasked when the window narrows later.
        if self.details_popover && details_inline(panel_open, window.viewport_size().width) {
            self.details_popover = false;
            cx.notify();
        }
        let model = self.workbench_model(window.viewport_size().width, cx);
        self.workbench_view.update(cx, |view, cx| {
            view.pane = self.session.as_ref().map(|session| session.pane.clone());
            view.set_model(model, cx);
        });
    }

    fn workbench_model(&self, width: Pixels, cx: &App) -> Option<WorkbenchModel> {
        let language = crate::localization::language(cx);
        let panel = self.panels.get(self.panel_scope());
        if !self.workbench_visible(panel.open, cx) {
            return None;
        }
        let kind = panel.kind();
        let resource = panel
            .active
            .and_then(|id| self.surface_resources.get(&id))
            .and_then(|resource| match resource {
                SurfaceResource::Browser(view) => Some(view.clone().into()),
                SurfaceResource::Terminal(view) => Some(view.clone().into()),
                SurfaceResource::SideChat(view) => Some(view.clone().into()),
                SurfaceResource::Diff { view, .. } => Some(view.clone().into()),
                SurfaceResource::Pending { .. } => None,
            });
        let pending = panel.active.is_some_and(|id| {
            matches!(
                self.surface_resources.get(&id),
                Some(SurfaceResource::Pending { .. })
            )
        });
        let browsers: Vec<_> = panel
            .surfaces
            .iter()
            .filter_map(|surface| match self.surface_resources.get(&surface.id) {
                Some(SurfaceResource::Browser(view)) => Some((surface.id, view.clone())),
                _ => None,
            })
            .collect();
        let file = (kind == Panel::File)
            .then(|| self.publication.pane.workspace.file.as_ref())
            .flatten()
            .filter(|file| {
                panel.selected().and_then(|surface| surface.path.as_deref())
                    == Some(file.path.as_str())
            })
            .map(|file| file.path.clone());
        let target = panel.selected().and_then(|surface| surface.path.clone());
        Some(WorkbenchModel {
            standalone: !self.side_chat_mode
                && (panel.layout == PanelLayout::Expanded
                    || (panel.layout == PanelLayout::Right && width < px(1080.))),
            details_popover: self.details_popover,
            workspace: self.workspace_view.clone(),
            panel,
            browsers,
            resource,
            resource_message: if pending {
                if matches!(self.draft_key(), DraftKey::Session { .. }) {
                    language.text("ブラウザを起動中…")
                } else {
                    language.text("会話を始めると、AIと同じブラウザを利用できます。")
                }
            } else {
                language.text("機能を開けませんでした。「＋」から再試行してください。")
            },
            sidebar: self.sidebar,
            side_chat_mode: self.side_chat_mode,
            tools: self.panel_tools(),
            errors: self.error_rows(&ErrorScope::Workspace),
            tree: self
                .file_browser
                .as_ref()
                .map(|browser| browser.model.clone()),
            explorer_state: self.file_explorer.state.clone(),
            explorer_width: agent_core::presentation::files::explorer_width(
                self.file_explorer.preferred,
                self.file_explorer.available,
            ),
            explorer_available: self.file_explorer.available,
            explorer_focus: self.file_explorer.focus.clone(),
            cwd: self.publication.pane.navigation.cwd.clone(),
            review_files: self
                .publication
                .pane
                .workspace
                .review
                .as_ref()
                .map(|review| review.files.iter().map(|file| file.path.clone()).collect())
                .unwrap_or_default(),
            explorer_max: agent_core::presentation::files::explorer_max_width(
                self.file_explorer.available,
            ),
            editor: FileEditorModel {
                target,
                draft: self.draft_key().clone(),
                input: self.editor.input.clone(),
                truncated: file.is_some()
                    && self
                        .publication
                        .pane
                        .workspace
                        .file
                        .as_ref()
                        .is_some_and(|file| file.truncated),
                path: file,
                saving: kind == Panel::File && self.busy(),
                failed: self
                    .snapshot
                    .operation_error(op::OperationKey::File {
                        pane: self.publication.pane_id.clone(),
                    })
                    .is_some(),
            },
            review: (kind == Panel::Diff)
                .then(|| self.publication.pane.workspace.review.as_ref())
                .flatten()
                .map(|review| (review.additions, review.deletions, review.files.len())),
            agents: if kind == Panel::Agents {
                self.publication.agents.clone()
            } else {
                Arc::default()
            },
            file_search: self.file_search.clone(),
            focus: self.panel_focus.clone(),
        })
    }

    pub(super) fn workbench(&self) -> AnyElement {
        self.workbench_view
            .clone()
            .cached(StyleRefinement::default().size_full())
            .into_any_element()
    }
}

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
            self.snapshot.connection.connected,
            !self.publication.pane.navigation.cwd.is_empty(),
        )
    }
    pub(in crate::app) fn open_review(&mut self, path: Option<&str>, cx: &mut Context<Self>) {
        let id = self.open_surface(self.panel_scope(), Panel::Diff, None);
        self.tab = Tab::Chat;
        let diff = self.workspace_diff(id, cx);
        if let Some(path) = path {
            diff.update(cx, |diff, cx| diff.reveal_path(path, cx));
        }
        self.refresh_review();
        cx.notify();
    }
}

impl super::composer::ComposerContent {
    pub(super) fn review_card(
        review: &agent_protocol::models::WorkspaceReview,
        expanded: bool,
        connected: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        let language = crate::localization::language(cx);
        let files = &review.files;
        let visible = if expanded {
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
                    .on_click(cx.listener(|_, _, _, cx| {
                        cx.emit(super::composer::ComposerAction::Review(None));
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
                            .child(div().text_sm().child(
                                agent_core::localization::localized_ui_format(
                                    language,
                                    "{0} 件のファイルを変更".into(),
                                    vec![files.len().to_string()],
                                ),
                            ))
                            .child(review_counts(
                                Some(review.additions),
                                Some(review.deletions),
                                language,
                            )),
                    )
                    .child(
                        button(
                            "review-changes",
                            language.text("レビューする"),
                            cx,
                            |_, _, cx| {
                                cx.stop_propagation();
                                cx.emit(super::composer::ComposerAction::Review(None));
                            },
                        )
                        .border_1()
                        .rounded(px(8.))
                        .disabled(!connected),
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
                            .on_click(cx.listener(move |_, _, _, cx| {
                                cx.emit(super::composer::ComposerAction::Review(Some(
                                    path.clone(),
                                )));
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
                            .child(review_counts(file.additions, file.deletions, language))
                    })),
            )
            .when(files.len() > 3, |card| {
                card.child(
                    div().px_2().py_1().bg(rgb(appearance::RAISED)).child(
                        button(
                            "expand-review-files",
                            if expanded {
                                language.text("折りたたむ").to_owned()
                            } else {
                                agent_core::localization::localized_ui_format(
                                    language,
                                    "あと {0} 個のファイルを表示".into(),
                                    vec![(files.len() - 3).to_string()],
                                )
                            },
                            cx,
                            |_, _, cx| cx.emit(super::composer::ComposerAction::ToggleReview),
                        )
                        .icon(if expanded {
                            IconName::ChevronUp
                        } else {
                            IconName::ChevronDown
                        }),
                    ),
                )
            })
            .into_any_element()
    }
}

impl WorkbenchView {
    fn explorer_handle(model: &WorkbenchModel, active: bool, cx: &Context<Self>) -> AnyElement {
        let focus = model.explorer_focus.clone();
        div()
            .id("resize-file-explorer")
            .size_full()
            .track_focus(&focus)
            .tab_index(0)
            .role(Role::Splitter)
            .aria_orientation(Orientation::Vertical)
            .aria_label(crate::localization::language(cx).text("ファイル一覧の幅を変更"))
            .aria_min_numeric_value(EXPLORER_MIN_WIDTH as f64)
            .aria_max_numeric_value(model.explorer_max as f64)
            .when_some(
                model.explorer_state.read(cx).sizes().get(1),
                |handle, width| handle.aria_numeric_value(f32::from(*width) as f64),
            )
            .border_l_1()
            .border_color(rgb(appearance::BORDER))
            .when(active, |handle| {
                handle.border_color(rgb(appearance::ACCENT))
            })
            .focus_visible(|handle| handle.border_color(rgb(appearance::ACCENT)))
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                focus.focus(window, cx)
            })
            .on_key_down(cx.listener(|view, event: &KeyDownEvent, _, cx| {
                let modifiers = event.keystroke.modifiers;
                if modifiers.alt || modifiers.control || modifiers.platform {
                    return;
                }
                let Some(model) = &view.model else {
                    return;
                };
                let Some(current) = model.explorer_state.read(cx).sizes().get(1).copied() else {
                    return;
                };
                if let Some(width) = agent_core::presentation::files::explorer_key_width(
                    &event.keystroke.key,
                    current.into(),
                    model.explorer_available,
                ) {
                    cx.emit(WorkbenchAction::ExplorerResize(width));
                    cx.stop_propagation();
                }
            }))
            .into_any_element()
    }

    fn file_editor(model: &FileEditorModel, cx: &Context<Self>) -> AnyElement {
        let path = model.path.as_deref();
        let target = model.target.as_deref();
        let failed = model.failed;
        let input = &model.input;
        let saving = model.saving;
        let truncated = model.truncated;
        let draft = &model.draft;
        let language = crate::localization::language(cx);
        let reload = path.map(str::to_owned);
        let draft = draft.clone();
        let edit = path.map(|path| {
            agent_core::localization::localized_ui_format(
                language,
                "ファイル {0} を編集してください。\n\n".into(),
                vec![path.to_owned()],
            )
        });
        let mut editor = v_flex()
            .debug_selector(|| "conversation-file".into())
            .size_full()
            .min_h_0()
            .min_w_0()
            .gap_2();
        if let Some(path) = target {
            let download = path.to_owned();
            editor = editor.child(
                h_flex()
                    .gap_2()
                    .child(super::file_icon::file_icon(path, false, false, cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .text_ellipsis()
                            .child(path.to_owned()),
                    )
                    .child(
                        icon_button(
                            "download-file",
                            IconName::ArrowDown,
                            language.text("ダウンロード"),
                            cx,
                            move |_, _, cx| {
                                cx.emit(WorkbenchAction::Download(download.clone()));
                                cx.notify();
                            },
                        )
                        .debug_selector(|| "download-file".into()),
                    ),
            );
        }
        if path.is_some() {
            editor = editor
                .when(truncated, |editor| {
                    editor.child(
                        div()
                            .text_xs()
                            .child(language.text("file.preview.truncated").to_owned()),
                    )
                })
                .child(
                    div().flex_1().min_h_0().child(
                        Editor::new(input)
                            .readonly(truncated)
                            .h_full()
                            .aria_label(language.text("ファイル編集")),
                    ),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .child(
                            button("save-file", language.text("保存"), cx, |_, _, cx| {
                                cx.emit(WorkbenchAction::Save)
                            })
                            .primary()
                            .disabled(saving || truncated),
                        )
                        .child(icon_button(
                            "reload-file",
                            IconName::RotateCw,
                            language.text("再読み込み・下書きを破棄"),
                            cx,
                            move |_, _, cx| {
                                if let Some(path) = &reload {
                                    cx.emit(WorkbenchAction::Reload(path.clone()));
                                }
                            },
                        ))
                        .child(
                            button(
                                "ai-edit",
                                language.text("AI に編集を依頼"),
                                cx,
                                move |view, _, _| {
                                    if let Some(text) = &edit {
                                        view.dispatch(Intent::SetDraftText {
                                            thread_id: draft.clone(),
                                            text: text.clone(),
                                        });
                                    }
                                },
                            )
                            .icon(IconName::Bot),
                        ),
                );
        } else {
            editor =
                editor
                    .items_center()
                    .justify_center()
                    .gap_3()
                    .child(
                        Icon::new(IconName::FileText)
                            .size_8()
                            .text_color(rgb(appearance::MUTED)),
                    )
                    .child(div().text_sm().text_color(rgb(appearance::MUTED)).child(
                        language.text(if failed {
                            "このファイルを表示できません"
                        } else {
                            "ファイルを読み込み中…"
                        }),
                    ));
        }
        editor.p_4().into_any_element()
    }
    fn render_workbench(
        model: &WorkbenchModel,
        titles: &BTreeMap<u64, String>,
        desktop: &WeakEntity<Desktop>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let language = crate::localization::language(cx);
        let render_owner = cx.entity_id();
        let panel = &model.panel;
        let layout = panel.layout;
        let active = panel.active;
        let kind = panel.kind();
        let tabs = panel
            .surfaces
            .iter()
            .map(|surface| {
                let id = surface.id;
                let title = titles.get(&id).cloned().unwrap_or_else(|| {
                    surface.path.as_ref().map_or_else(
                        || agent_core::panels::panel_title(surface.kind),
                        |path| file_name(path),
                    )
                });
                h_flex()
                    .gap_1()
                    .rounded_md()
                    .when(Some(id) == active, |tab| tab.bg(rgb(appearance::RAISED)))
                    .child(
                        Button::new(("surface-tab", id as usize))
                            .label(title.clone())
                            .icon(panel_icon(surface.kind))
                            .small()
                            .ghost()
                            .selected(Some(id) == active)
                            .debug_selector(move || format!("surface-tab-{id}"))
                            .on_click(cx.listener(move |_, _, _, cx| {
                                cx.emit(WorkbenchAction::Activate(id))
                            })),
                    )
                    .child(
                        icon_button(
                            format!("close-surface-{id}"),
                            IconName::Close,
                            language.text("タブを閉じる"),
                            cx,
                            move |_, _, cx| cx.emit(WorkbenchAction::Close(id)),
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
            .tooltip(language.text("機能を開く"))
            .accessibility_label(language.text("機能を開く"))
            .dropdown_menu(move |mut menu, _, cx| {
                let tools = menu_owner
                    .upgrade()
                    .and_then(|view| {
                        view.read(cx)
                            .model
                            .as_ref()
                            .map(|model| model.tools.clone())
                    })
                    .unwrap_or_default();
                for tool in tools {
                    let owner = menu_owner.clone();
                    menu = menu.item(
                        PopupMenuItem::new(tool.label)
                            .icon(panel_icon(tool.kind))
                            .disabled(!tool.enabled)
                            .on_click(move |_, _, cx| {
                                let _ = owner
                                    .update(cx, |_, cx| cx.emit(WorkbenchAction::Open(tool.kind)));
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
            .when(model.standalone && !model.sidebar, |bar| {
                bar.pl_0().child(sidebar_header(false, desktop, cx))
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
            .when(!model.side_chat_mode, |bar| {
                bar.child(
                    icon_button(
                        "panel-expand",
                        if layout == PanelLayout::Expanded {
                            IconName::Minimize
                        } else {
                            IconName::Maximize
                        },
                        language.text("パネルを拡大・縮小"),
                        cx,
                        move |_, _, cx| {
                            cx.emit(WorkbenchAction::Layout(
                                if layout == PanelLayout::Expanded {
                                    PanelLayout::Right
                                } else {
                                    PanelLayout::Expanded
                                },
                            ))
                        },
                    )
                    .debug_selector(|| "panel-expand".into()),
                )
                .child(
                    icon_button(
                        "panel-bottom",
                        IconName::PanelBottom,
                        language.text("下・右の配置を切り替え"),
                        cx,
                        move |_, _, cx| {
                            cx.emit(WorkbenchAction::Layout(if layout == PanelLayout::Bottom {
                                PanelLayout::Right
                            } else {
                                PanelLayout::Bottom
                            }))
                        },
                    )
                    .debug_selector(|| "panel-bottom".into())
                    .selected(layout == PanelLayout::Bottom),
                )
                // The panel covers the conversation header, so it carries the toggle.
                .when(model.standalone, |bar| {
                    bar.child(workspace::details_toggle(
                        false,
                        model.details_popover,
                        render_owner,
                        desktop,
                        model.workspace.clone(),
                        cx,
                    ))
                })
                .child(Desktop::panel_toggle(true, desktop, cx))
            })
            .when(model.side_chat_mode, |bar| {
                bar.child(
                    icon_button(
                        "back-side-chat",
                        IconName::ArrowLeft,
                        language.text("サイドチャットに戻る"),
                        cx,
                        |_, _, cx| cx.emit(WorkbenchAction::Hide),
                    )
                    .debug_selector(|| "back-side-chat".into()),
                )
            });
        let toolbar = toolbar.when(
            cfg!(target_os = "windows")
                && !model.side_chat_mode
                && (model.standalone || layout == PanelLayout::Right),
            |toolbar| crate::window_chrome::header(toolbar, window),
        );
        let body = match kind {
            Panel::Home => {
                let mut chooser = v_flex().w_full().max_w(px(300.)).gap_1().child(
                    div()
                        .text_sm()
                        .mb_3()
                        .text_center()
                        .child(language.text("機能を開く")),
                );
                for (ix, tool) in model.tools.iter().enumerate() {
                    let panel = tool.kind;
                    chooser = chooser.child(
                        button(
                            format!("tool-{ix}"),
                            tool.label.clone(),
                            cx,
                            move |_, _, cx| {
                                cx.emit(WorkbenchAction::Open(panel));
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
                                .child(tool.shortcut.clone()),
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
                    .track_focus(&model.focus)
                    .on_key_down(cx.listener(|view, event: &KeyDownEvent, _, cx| {
                        if event.keystroke.modifiers.control
                            || event.keystroke.modifiers.platform
                            || event.keystroke.modifiers.alt
                        {
                            return;
                        }
                        let Some(tool) = view
                            .model
                            .as_ref()
                            .into_iter()
                            .flat_map(|model| model.tools.iter())
                            .find(|tool| {
                                tool.shortcut
                                    .eq_ignore_ascii_case(event.keystroke.key.as_str())
                            })
                        else {
                            return;
                        };
                        if tool.enabled {
                            cx.emit(WorkbenchAction::Open(tool.kind));
                            cx.stop_propagation();
                        }
                    }))
                    .child(chooser)
                    .into_any_element()
            }
            Panel::Agents => Desktop::agents_panel(&model.agents, language),
            Panel::Files => super::file_tree::render_tree(
                model.tree.as_ref(),
                &model.file_search,
                &model.focus,
                &model.cwd,
                cx,
            ),
            Panel::File => div()
                .size_full()
                .on_prepaint({
                    let owner = cx.entity().downgrade();
                    move |bounds, window, cx| {
                        let available = f32::from(bounds.size.width);
                        if let Some(owner) = owner.upgrade()
                            && owner
                                .read(cx)
                                .model
                                .as_ref()
                                .is_some_and(|model| model.explorer_available != available)
                        {
                            window.defer(cx, move |_, cx| {
                                owner.update(cx, |_, cx| {
                                    cx.emit(WorkbenchAction::ExplorerFit(available))
                                })
                            });
                        }
                    }
                })
                .child(
                    h_resizable("file-explorer-split")
                        .with_state(&model.explorer_state)
                        .with_handle_appearance(Rc::new({
                            let owner = cx.entity().downgrade();
                            move |handle, _, cx| {
                                owner
                                    .update(cx, |view, cx| {
                                        view.model.as_ref().map(|model| {
                                            Self::explorer_handle(model, handle.is_active(), cx)
                                        })
                                    })
                                    .ok()
                                    .flatten()
                            }
                        }))
                        .on_resize({
                            let owner = cx.entity().downgrade();
                            move |_, _, cx| {
                                let _ = owner
                                    .update(cx, |_, cx| cx.emit(WorkbenchAction::ExplorerCommit));
                            }
                        })
                        .child(
                            resizable_panel()
                                .size_range(px(EXPLORER_CONTENT_MIN_WIDTH)..Pixels::MAX)
                                .child(Self::file_editor(&model.editor, cx)),
                        )
                        .child(
                            resizable_panel()
                                .size(px(model.explorer_width))
                                .size_range(px(EXPLORER_MIN_WIDTH)..px(model.explorer_max))
                                .fixed()
                                .child(super::file_tree::render_tree(
                                    model.tree.as_ref(),
                                    &model.file_search,
                                    &model.focus,
                                    &model.cwd,
                                    cx,
                                )),
                        ),
                )
                .into_any_element(),
            Panel::Diff => {
                v_flex()
                    .size_full()
                    .min_h_0()
                    .p_4()
                    .gap_3()
                    .child(
                        h_flex()
                            .child(div().flex_1().child(
                                agent_core::localization::localized_ui_format(
                                    language,
                                    "変更  +{0} −{1}".into(),
                                    vec![
                                        model.review.map_or(0, |review| review.0).to_string(),
                                        model.review.map_or(0, |review| review.1).to_string(),
                                    ],
                                ),
                            ))
                            .child(icon_button(
                                "refresh-diff",
                                IconName::RotateCw,
                                language.text("変更を更新"),
                                cx,
                                |_, _, cx| cx.emit(WorkbenchAction::RefreshReview),
                            )),
                    )
                    .child(Self::diff_file_menu(&model.review_files, cx))
                    .child(
                        div().flex_1().min_h_0().children(
                            model
                                .resource
                                .clone()
                                .map(|view| view.cached(StyleRefinement::default().size_full())),
                        ),
                    )
                    .into_any_element()
            }
            Panel::SideChat | Panel::Terminal | Panel::Browser => {
                if let Some(view) = &model.resource {
                    view.clone()
                        .cached(StyleRefinement::default().size_full())
                        .into_any_element()
                } else {
                    div().p_4().child(model.resource_message).into_any_element()
                }
            }
        };
        v_flex()
            .size_full()
            .min_h_0()
            .min_w_0()
            .child(toolbar)
            .child(errors::error_list(
                &model.errors,
                ErrorScope::Workspace,
                None,
                cx,
                |_, action, _, cx| cx.emit(WorkbenchAction::Notice(action)),
            ))
            .child(div().flex_1().min_h_0().flex().child(body))
            .into_any_element()
    }
}

impl Desktop {
    fn workspace_diff(&mut self, id: u64, cx: &mut Context<Self>) -> Entity<DiffView> {
        let review = &self.publication.pane.workspace.review;
        let source = review.as_ref().map_or("", |review| review.diff.as_str());
        if let Some(SurfaceResource::Diff {
            view,
            source: previous,
        }) = self.surface_resources.get_mut(&id)
        {
            if previous.as_ref().map(Weak::as_ptr) != review.as_ref().map(Arc::as_ptr) {
                view.update(cx, |view, cx| view.set_source(source, cx));
                *previous = review.as_ref().map(Arc::downgrade);
            }
            return view.clone();
        }
        let view = cx.new(|_| DiffView::new(source.to_owned().into(), true));
        self.surface_resources.insert(
            id,
            SurfaceResource::Diff {
                view: view.clone(),
                source: review.as_ref().map(Arc::downgrade),
            },
        );
        view
    }
}

impl WorkbenchView {
    fn diff_file_menu(files: &[String], cx: &Context<Self>) -> AnyElement {
        let language = crate::localization::language(cx);
        let entity = cx.entity().downgrade();
        let files = files.to_vec();
        Button::new("diff-file-navigation")
            .debug_selector(|| "diff-file-navigation".into())
            .label(agent_core::localization::localized_ui_format(
                language,
                "ファイルを選択 ({0})".into(),
                vec![files.len().to_string()],
            ))
            .accessibility_label(language.text("差分のファイルを選択"))
            .dropdown_caret(true)
            .ghost()
            .dropdown_menu(move |mut menu, _, _| {
                for path in &files {
                    let path = path.clone();
                    let entity = entity.clone();
                    menu = menu.item(PopupMenuItem::new(path.clone()).on_click(move |_, _, cx| {
                        let _ = entity
                            .update(cx, |_, cx| cx.emit(WorkbenchAction::Review(path.clone())));
                    }));
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
    fn side_workbench_reads_its_own_review_and_file_editability(cx: &mut TestAppContext) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        cx.update(|cx| {
            crate::appearance::init(cx);
            cx.set_global(Runtime {
                handle: runtime.handle().clone(),
                sessions: Default::default(),
                connections: Arc::default(),
                closing: tokio_util::task::TaskTracker::new(),
                logging_error: None,
            });
        });
        let (store, pane) = runtime.block_on(async {
            let store = Arc::new(agent_core::store::Store::offline(Snapshot::default(), None));
            let pane = store.open_pane("/side".into()).unwrap();
            (store, pane)
        });
        for truncated in [false, true] {
            let mut snapshot = store.snapshot().as_ref().clone();
            for (id, path, truncated) in [
                ("", "/primary/file", !truncated),
                (pane.id(), "/side/file", truncated),
            ] {
                let workspace = Arc::make_mut(
                    &mut Arc::make_mut(snapshot.panes.get_mut(id).unwrap()).workspace,
                );
                workspace.file = Some(Arc::new(agent_protocol::models::FileContent {
                    path: path.into(),
                    revision: "base".into(),
                    text: "text".into(),
                    size: 4,
                    truncated,
                }));
                workspace.review = Some(Arc::new(agent_protocol::models::WorkspaceReview {
                    branch: "main".into(),
                    additions: 1,
                    deletions: 0,
                    diff: String::new(),
                    files: vec![agent_protocol::models::ChangedFile {
                        path: path.into(),
                        status: "M".into(),
                        additions: Some(1),
                        deletions: Some(0),
                    }],
                }));
            }
            let snapshot = Arc::new(snapshot);
            let (_, _) = cx.add_window_view(|window, cx| {
                let mut desktop = Desktop::new(
                    Mode::Fixture {
                        remote: None,
                        cwd: String::new(),
                    },
                    window,
                    cx,
                );
                desktop.publication = agent_core::presentation::view::Presentation::project(
                    snapshot.clone(),
                    pane.id(),
                    None,
                )
                .unwrap();
                desktop.snapshot = snapshot.clone();
                desktop.open_surface(
                    desktop.panel_scope(),
                    Panel::File,
                    Some("/side/file".into()),
                );
                let model = desktop.workbench_model(px(1400.), cx).unwrap();
                assert_eq!(model.review_files, ["/side/file"]);
                assert_eq!(model.editor.path.as_deref(), Some("/side/file"));
                assert_eq!(model.editor.truncated, truncated);
                desktop
            });
        }
        runtime.block_on(store.close()).unwrap();
    }

    #[gpui::test]
    fn hidden_workbench_defers_resources_and_reconciles_before_drawing(cx: &mut TestAppContext) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        cx.update(|cx| {
            crate::appearance::init(cx);
            cx.set_global(Runtime {
                handle: runtime.handle().clone(),
                sessions: Default::default(),
                connections: Arc::default(),
                closing: tokio_util::task::TaskTracker::new(),
                logging_error: None,
            });
        });
        let review = Arc::new(agent_protocol::models::WorkspaceReview {
            branch: "main".into(),
            additions: 1,
            deletions: 0,
            files: Vec::new(),
            diff: "+first".into(),
        });
        let (desktop, window) = cx.add_window_view(|window, cx| {
            let mut desktop = Desktop::new(Mode::Main, window, cx);
            desktop.onboarding = false;
            desktop.tab = Tab::Settings;
            desktop.open_surface(desktop.panel_scope(), Panel::Diff, None);
            Arc::make_mut(
                &mut Arc::make_mut(Arc::make_mut(&mut desktop.snapshot).panes.primary_mut())
                    .workspace,
            )
            .review = Some(review.clone());
            desktop.project_fixture();
            desktop
        });
        window.simulate_resize(size(px(1600.), px(900.)));
        window.run_until_parked();
        desktop.read_with(window, |desktop, cx| {
            assert!(
                desktop.surface_resources.is_empty(),
                "settings must not prepare a hidden Diff"
            );
            assert!(desktop.workbench_view.read(cx).model.is_none());
        });
        desktop.update(window, |desktop, cx| {
            desktop.tab = Tab::Chat;
            cx.notify();
        });
        let (diff, workbench) = desktop.read_with(window, |desktop, cx| {
            let id = desktop.panels.get(desktop.panel_scope()).active.unwrap();
            let SurfaceResource::Diff { view, source } = &desktop.surface_resources[&id] else {
                panic!("Diff must be prepared before drawing")
            };
            assert!(Arc::ptr_eq(
                &source.as_ref().unwrap().upgrade().unwrap(),
                &review
            ));
            assert!(
                desktop
                    .workbench_view
                    .read(cx)
                    .model
                    .as_ref()
                    .unwrap()
                    .resource
                    .is_some()
            );
            (view.clone(), desktop.workbench_view.clone())
        });
        window.run_until_parked();
        window.simulate_resize(size(px(800.), px(900.)));
        assert!(workbench.read_with(window, |view, _| view.model.as_ref().unwrap().standalone));
        window.simulate_resize(size(px(1600.), px(900.)));
        assert!(!workbench.read_with(window, |view, _| view.model.as_ref().unwrap().standalone));
        let changes = Rc::new(std::cell::Cell::new(0));
        let _observation = window.update(|_, cx| {
            let changes = changes.clone();
            cx.observe(&workbench, move |_, _| changes.set(changes.get() + 1))
        });
        let diff_changes = Rc::new(std::cell::Cell::new(0));
        let _diff_observation = window.update(|_, cx| {
            let changes = diff_changes.clone();
            cx.observe(&diff, move |_, _| changes.set(changes.get() + 1))
        });
        desktop.update(window, |desktop, cx| {
            let workspace = Arc::make_mut(
                &mut Arc::make_mut(Arc::make_mut(&mut desktop.snapshot).panes.primary_mut())
                    .workspace,
            );
            Arc::make_mut(workspace.review.as_mut().unwrap()).diff = "+second".into();
            desktop.project_fixture();
            cx.notify();
        });
        window.run_until_parked();
        assert_eq!(
            changes.get(),
            0,
            "Diff text must not invalidate the toolbar model"
        );
        assert_eq!(
            diff_changes.get(),
            1,
            "the Diff itself must receive its changed source"
        );
        desktop.update(window, |desktop, cx| {
            desktop.tab = Tab::Settings;
            cx.notify();
        });
        window.run_until_parked();
        let notifications = diff_changes.get();
        desktop.update(window, |desktop, cx| {
            let workspace = Arc::make_mut(
                &mut Arc::make_mut(Arc::make_mut(&mut desktop.snapshot).panes.primary_mut())
                    .workspace,
            );
            Arc::make_mut(workspace.review.as_mut().unwrap()).diff = "+third".into();
            desktop.project_fixture();
            cx.notify();
        });
        window.run_until_parked();
        assert_eq!(
            diff_changes.get(),
            notifications,
            "hidden resources must not synchronize"
        );
        desktop.update(window, |desktop, cx| {
            desktop.tab = Tab::Chat;
            cx.notify();
        });
        assert_eq!(
            diff_changes.get(),
            notifications + 1,
            "reopening must synchronize before drawing"
        );
        window.run_until_parked();
        assert!(window.debug_bounds("diff-file-navigation").is_some());
        window.update(|window, cx| {
            desktop.update(cx, |desktop, cx| desktop.set_visible(false, window, cx))
        });
        let before = diff_changes.get();
        desktop.update(window, |desktop, cx| {
            let workspace = Arc::make_mut(
                &mut Arc::make_mut(Arc::make_mut(&mut desktop.snapshot).panes.primary_mut())
                    .workspace,
            );
            Arc::make_mut(workspace.review.as_mut().unwrap()).diff = "+hidden side chat".into();
            desktop.project_fixture();
            cx.notify();
        });
        window.run_until_parked();
        assert_eq!(
            diff_changes.get(),
            before,
            "a hidden side chat must stay inactive during its own updates"
        );
        window.update(|window, cx| {
            desktop.update(cx, |desktop, cx| desktop.set_visible(true, window, cx))
        });
        assert_eq!(
            diff_changes.get(),
            before + 1,
            "restoring a side chat must resume its surface"
        );
    }

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
                sessions: Default::default(),
                connections: Arc::default(),
                closing: tokio_util::task::TaskTracker::new(),
                logging_error: None,
            });
        });
        let (view, window) = cx.add_window_view(|window, cx| {
            let mut desktop = Desktop::new(Mode::Main, window, cx);
            desktop.onboarding = false;
            desktop.panels = desktop.panels.clone().set_open(desktop.panel_scope(), true);
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
            Arc::make_mut(
                &mut Arc::make_mut(Arc::make_mut(&mut view.snapshot).panes.primary_mut())
                    .navigation,
            )
            .draft_key = b.draft.clone();
            view.project_fixture();
            assert!(!view.panels.get(b.clone()).open);
            view.panels = view.panels.clone().set_open(b, true);
            cx.notify();
        });
        window.run_until_parked();
        assert!(window.debug_bounds("surface-launcher").is_some());
        assert!(window.debug_bounds("diff-file-navigation").is_none());
        view.update(window, |view, cx| {
            Arc::make_mut(
                &mut Arc::make_mut(Arc::make_mut(&mut view.snapshot).panes.primary_mut())
                    .navigation,
            )
            .draft_key = a.draft.clone();
            view.project_fixture();
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
