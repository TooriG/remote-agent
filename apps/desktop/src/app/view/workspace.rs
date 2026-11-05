use super::*;

#[derive(PartialEq)]
pub(in crate::app) struct WorkspaceModel {
    connected: bool,
    cwd: String,
    directory: String,
    review: Option<Arc<agent_protocol::models::WorkspaceReview>>,
    local_shell: bool,
    editors: Vec<agent_protocol::editors::EditorId>,
    preferred_editor: Option<agent_protocol::editors::EditorId>,
    sources: Arc<Vec<String>>,
    attachments: Vec<String>,
    errors: Vec<errors::ErrorRow>,
}

pub(in crate::app) enum WorkspaceAction {
    Download(String),
    RefreshReview,
    Review,
    Attach,
    PickFolder,
    Browse(String),
    OpenEditor {
        path: String,
        editor: agent_protocol::editors::EditorId,
    },
    Notice(errors::NoticeAction),
}

pub(in crate::app) struct WorkspaceView {
    model: Option<WorkspaceModel>,
    source_paths: Vec<String>,
}
impl EventEmitter<WorkspaceAction> for WorkspaceView {}
impl WorkspaceView {
    pub(in crate::app) fn new(cx: &mut Context<Self>) -> Self {
        cx.observe_global::<crate::localization::Language>(|_, cx| cx.notify())
            .detach();
        Self {
            model: None,
            source_paths: Vec::new(),
        }
    }
    pub(in crate::app) fn set_model(&mut self, model: WorkspaceModel, cx: &mut Context<Self>) {
        if self.model.as_ref() == Some(&model) {
            return;
        }
        if self
            .model
            .as_ref()
            .is_none_or(|old| old.sources != model.sources || old.attachments != model.attachments)
        {
            self.source_paths = model
                .sources
                .iter()
                .chain(&model.attachments)
                .cloned()
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
        }
        self.model = Some(model);
        cx.notify();
    }
    fn workspace_card(&self, model: &WorkspaceModel, cx: &Context<Self>) -> AnyElement {
        let language = crate::localization::language(cx);
        let enabled = model.connected && !model.cwd.is_empty();
        let review_enabled = enabled && !model.directory.is_empty();
        let review = model.review.as_ref();
        let (additions, deletions) =
            review.map_or((0, 0), |review| (review.additions, review.deletions));
        let mut sources = v_flex().gap_1();
        for (i, path) in self.source_paths.iter().enumerate() {
            let path = path.clone();
            sources = sources.child(
                button(
                    format!("source-{i}"),
                    file_name(&path),
                    cx,
                    move |_, _, cx| cx.emit(WorkspaceAction::Download(path.clone())),
                )
                .icon(IconName::FileText)
                .w_full(),
            );
        }
        if self.source_paths.is_empty() {
            sources = sources.child(
                div()
                    .text_sm()
                    .text_color(rgb(appearance::MUTED))
                    .py_2()
                    .child(language.text("添付ファイルがここに表示されます")),
            );
        }
        v_flex()
            .id("workspace-card")
            .child(errors::error_list(
                &model.errors,
                ErrorScope::Workspace,
                None,
                cx,
                |_, action, _, cx| cx.emit(WorkspaceAction::Notice(action)),
            ))
            .w_full()
            .h_full()
            .p_4()
            .gap_4()
            .overflow_y_scroll()
            .child(
                v_flex()
                    .gap_2()
                    .p_4()
                    .rounded(px(18.))
                    .bg(rgb(appearance::RAISED))
                    .child(
                        h_flex()
                            .child(
                                div()
                                    .flex_1()
                                    .text_sm()
                                    .text_color(rgb(appearance::MUTED))
                                    .child(language.text("ワークスペース")),
                            )
                            .child(
                                icon_button(
                                    "refresh-context",
                                    IconName::RotateCw,
                                    language.text("変更を更新"),
                                    cx,
                                    |_, _, cx| cx.emit(WorkspaceAction::RefreshReview),
                                )
                                .disabled(!review_enabled),
                            ),
                    )
                    .child(
                        button(
                            "context-files",
                            if model.cwd.is_empty() {
                                language.text("フォルダを選択").into()
                            } else if model.directory.is_empty() {
                                language.text("チャット").into()
                            } else {
                                file_name(&model.cwd)
                            },
                            cx,
                            |s, _, cx| {
                                let cwd = &s.model.as_ref().unwrap().cwd;
                                if cwd.is_empty() {
                                    cx.emit(WorkspaceAction::PickFolder);
                                } else {
                                    cx.emit(WorkspaceAction::Browse(cwd.clone()));
                                }
                            },
                        )
                        .icon(IconName::FolderOpen)
                        .w_full(),
                    )
                    .when(model.local_shell && model.connected, |body| {
                        body.child(self.open_in_editor_row(model, cx))
                    })
                    .when_some(
                        review.filter(|review| !review.branch.is_empty()),
                        |body, review| {
                            body.child(
                                div()
                                    .pl_2()
                                    .text_sm()
                                    .text_color(rgb(appearance::MUTED))
                                    .child(review.branch.clone()),
                            )
                        },
                    )
                    .child(
                        button(
                            "context-changes",
                            language.text("変更"),
                            cx,
                            |_, _, cx| cx.emit(WorkspaceAction::Review),
                        )
                        .accessibility_label(agent_core::localization::localized_ui_format(
                            language,
                            "変更  +{0}  −{1}".into(),
                            vec![additions.to_string(), deletions.to_string()],
                        ))
                        .child(review_counts(Some(additions), Some(deletions), language))
                        .icon(IconName::Replace)
                        .w_full()
                        .disabled(!enabled),
                    )
                    .child(div().h(px(1.)).my_2().bg(rgb(appearance::BORDER)))
                    .child(
                        h_flex()
                            .child(
                                div()
                                    .flex_1()
                                    .text_sm()
                                    .text_color(rgb(appearance::MUTED))
                                    .child(language.text("ソース")),
                            )
                            .child(
                                icon_button(
                                    "context-attach",
                                    IconName::Plus,
                                    language.text("ソースを追加"),
                                    cx,
                                    |_, _, cx| cx.emit(WorkspaceAction::Attach),
                                )
                                .disabled(!enabled),
                            ),
                    )
                    .child(sources)
                    .child(
                        button(
                            "all-files",
                            language.text("すべてのファイル"),
                            cx,
                            |view, _, cx| {
                                cx.emit(WorkspaceAction::Browse(
                                    view.model.as_ref().unwrap().cwd.clone(),
                                ))
                            },
                        )
                        .icon(IconName::Folder)
                        .w_full()
                        .disabled(!enabled),
                    ),
            )
            .into_any_element()
    }
    fn open_in_editor_row(&self, model: &WorkspaceModel, cx: &Context<Self>) -> AnyElement {
        use agent_core::presentation::editors::{editor_label, preferred_editor};
        use agent_protocol::editors::FileManagerRevealKind;
        let file_manager = if cfg!(target_os = "macos") {
            FileManagerRevealKind::Finder
        } else if cfg!(target_os = "windows") {
            FileManagerRevealKind::FileExplorer
        } else {
            FileManagerRevealKind::Files
        };
        let options = model.editors.clone();
        let selected = preferred_editor(model.preferred_editor, &options);
        let no_cwd = model.cwd.is_empty();
        let language = crate::localization::language(cx);
        let label = match selected {
            Some(editor) => agent_core::localization::localized_ui_format(
                language,
                "{0}で開く".into(),
                vec![language.text(editor_label(editor, file_manager)).into()],
            ),
            None => language.text("エディターで開く").into(),
        };
        let owner = cx.entity().downgrade();
        h_flex()
            .w_full()
            .child(
                button("open-in-editor", label, cx, move |view, _, cx| {
                    if let Some(editor) = selected {
                        cx.emit(WorkspaceAction::OpenEditor {
                            path: view.model.as_ref().unwrap().cwd.clone(),
                            editor,
                        });
                    }
                })
                .icon(IconName::ExternalLink)
                .flex_1()
                .disabled(selected.is_none() || no_cwd),
            )
            .child(
                Button::new("open-in-editor-picker")
                    .icon(IconName::ChevronDown)
                    .small()
                    .ghost()
                    .tooltip(language.text("エディターを選択"))
                    .accessibility_label(language.text("エディターを選択"))
                    .disabled(no_cwd)
                    .dropdown_menu(move |mut menu, _, _| {
                        if options.is_empty() {
                            menu = menu.item(
                                PopupMenuItem::new(
                                    language.text("インストール済みのエディターが見つかりません"),
                                )
                                .disabled(true),
                            );
                        }
                        for editor in &options {
                            let editor = *editor;
                            let owner = owner.clone();
                            menu = menu.item(
                                PopupMenuItem::new(
                                    language.text(editor_label(editor, file_manager)),
                                )
                                .on_click(move |_, _, cx| {
                                    let _ = owner.update(cx, |view, cx| {
                                        cx.emit(WorkspaceAction::OpenEditor {
                                            path: view.model.as_ref().unwrap().cwd.clone(),
                                            editor,
                                        });
                                    });
                                }),
                            );
                        }
                        menu
                    }),
            )
            .into_any_element()
    }
}

impl Render for WorkspaceView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.model
            .as_ref()
            .map(|model| self.workspace_card(model, cx))
            .unwrap_or_else(|| div().into_any_element())
    }
}

/// T3's thread details toggle: it shows or hides the docked card, or opens
/// the same card as a popover when the card cannot dock.
pub(in crate::app) fn details_toggle(
    inline: bool,
    open: bool,
    render_owner: EntityId,
    desktop: &WeakEntity<Desktop>,
    workspace: Entity<workspace::WorkspaceView>,
    cx: &App,
) -> AnyElement {
    let label = crate::localization::language(cx).text("スレッド詳細を切り替え");
    let icon = Icon::default().path("icons/square-menu.svg");
    if inline {
        let owner = desktop.clone();
        return Button::new("thread-details-toggle")
            .icon(icon)
            .small()
            .ghost()
            .tooltip(label)
            .accessibility_label(label)
            .on_click(move |_, _, cx| {
                let _ = owner.update(cx, |view, cx| {
                    view.details = !open;
                    cx.notify();
                });
            })
            .debug_selector(|| "thread-details-toggle".into())
            .selected(open)
            .into_any_element();
    }
    let owner = desktop.clone();
    popover::Popover::new("thread-details")
        .appearance(false)
        .anchor(Anchor::TopRight)
        .open(open)
        .on_open_change(move |open, _, cx| {
            let _ = owner.update(cx, |view, cx| {
                view.details_popover = *open;
                cx.notify();
            });
        })
        .trigger(
            Button::new("thread-details-toggle")
                .debug_selector(|| "thread-details-toggle".into())
                .icon(icon)
                .small()
                .ghost()
                .selected(open)
                .tooltip(label)
                .accessibility_label(label),
        )
        .content(move |_, window, cx| {
            observe_popup(&workspace.downgrade(), render_owner, window, cx);
            v_flex()
                .debug_selector(|| "thread-details-popover".into())
                .w(px(300.))
                .child(workspace.clone())
                .into_any_element()
        })
        .into_any_element()
}

impl Desktop {
    pub(in crate::app) fn publish_workspace(&self, cx: &mut Context<Self>) {
        let model = WorkspaceModel {
            connected: self.snapshot.connection.connected,
            cwd: self.snapshot.pane.navigation.cwd.clone(),
            directory: self.snapshot.selected_directory(),
            review: self.snapshot.pane.workspace.review.clone(),
            local_shell: self.local_shell,
            editors: self
                .snapshot
                .files
                .file_open_capabilities
                .as_ref()
                .map(|c| c.editors.clone())
                .unwrap_or_default(),
            preferred_editor: self.snapshot.files.preferred_editor,
            sources: self
                .publication
                .conversation
                .as_ref()
                .map(|c| c.source_paths.clone())
                .unwrap_or_default(),
            attachments: self
                .draft()
                .attachments
                .iter()
                .map(|a| a.path.clone())
                .collect(),
            errors: self.error_rows(&ErrorScope::Workspace),
        };
        self.workspace_view
            .update(cx, |view, cx| view.set_model(model, cx));
    }
}
