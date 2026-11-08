use super::*;

fn action(
    label: impl Into<SharedString>,
    owner: WeakEntity<Desktop>,
    host: String,
    intent: Intent,
) -> PopupMenuItem {
    PopupMenuItem::new(label).on_click(move |_, _, cx| {
        let _ = owner.update(cx, |s, cx| {
            s.dispatch_host(&host, intent.clone(), cx);
            cx.notify();
        });
    })
}

pub(super) fn project_menu_button(
    host: String,
    id: String,
    owner: WeakEntity<Desktop>,
) -> impl IntoElement {
    Button::new("project-menu")
        .icon(IconName::Ellipsis)
        .xsmall()
        .ghost()
        .accessibility_label("プロジェクトの操作")
        .tooltip("プロジェクトの操作")
        .dropdown_menu(move |mut menu, window, cx| {
            let Some(entity) = owner.upgrade() else {
                return menu;
            };
            let state = entity.read(cx);
            let local = host == "local" || (host.is_empty() && state.remote.is_none());
            let snapshot = match state.connection_store(&host, cx) {
                Some(store) => store.snapshot(),
                None if host.is_empty() => state.publication.snapshot.clone(),
                None => return menu,
            };
            let Some(list) = snapshot.thread_list() else {
                return menu;
            };
            let Some(project) = list.projects.iter().find(|p| p.id == id) else {
                return menu;
            };
            menu = menu.item(action(
                if project.pinned {
                    "ピン留めを解除"
                } else {
                    "ピン留め"
                },
                owner.clone(),
                host.clone(),
                Intent::SetProjectPinned {
                    project_id: id.clone(),
                    pinned: !project.pinned,
                },
            ));
            let target = owner.clone();
            let edit_id = id.clone();
            let edit_host = host.clone();
            menu = menu
                .item(PopupMenuItem::new("編集").on_click(move |_, window, cx| {
                    let _ = target.update(cx, |s, cx| {
                        s.edit_project(&edit_host, &edit_id, window, cx);
                        cx.notify();
                    });
                }))
                .separator();
            let mut sections = list
                .projects
                .iter()
                .filter_map(|p| p.section.clone())
                .collect::<Vec<_>>();
            sections.sort();
            sections.dedup();
            let target = owner.clone();
            let section_id = id.clone();
            let current = project.section.clone();
            let section_host = host.clone();
            menu = menu.submenu("セクション", window, cx, move |mut menu, _, _| {
                menu = menu.item(
                    action(
                        "未分類",
                        target.clone(),
                        section_host.clone(),
                        Intent::SetProjectSection {
                            project_id: section_id.clone(),
                            section: String::new(),
                        },
                    )
                    .checked(current.is_none()),
                );
                for section in &sections {
                    menu = menu.item(
                        action(
                            section.clone(),
                            target.clone(),
                            section_host.clone(),
                            Intent::SetProjectSection {
                                project_id: section_id.clone(),
                                section: section.clone(),
                            },
                        )
                        .checked(current.as_ref() == Some(section)),
                    );
                }
                let target = target.clone();
                let edit_id = section_id.clone();
                let edit_host = section_host.clone();
                menu.item(
                    PopupMenuItem::new("新しいセクション…").on_click(move |_, window, cx| {
                        let _ = target.update(cx, |s, cx| {
                            s.edit_project(&edit_host, &edit_id, window, cx);
                            cx.notify();
                        });
                    }),
                )
            });
            let roots = project
                .roots
                .iter()
                .map(|r| r.path.clone())
                .collect::<Vec<_>>();
            let target = owner.clone();
            let folder_host = host.clone();
            if roots.len() == 1 {
                let path = roots[0].clone();
                menu = menu.item(
                    PopupMenuItem::new(if cfg!(target_os = "windows") {
                        "エクスプローラーで開く"
                    } else {
                        "フォルダを開く"
                    })
                    .disabled(!local)
                    .on_click(move |_, _, cx| {
                        let _ = target.update(cx, |s, cx| {
                            s.open_project_folder(&folder_host, &path);
                            cx.notify();
                        });
                    }),
                );
            } else {
                menu = menu.submenu(
                    "フォルダを開く",
                    window,
                    cx,
                    move |mut menu, _, _| {
                        for path in &roots {
                            let target = target.clone();
                            let path = path.clone();
                            let folder_host = folder_host.clone();
                            menu = menu.item(
                                PopupMenuItem::new(path.clone()).disabled(!local).on_click(
                                    move |_, _, cx| {
                                        let _ = target.update(cx, |s, cx| {
                                            s.open_project_folder(&folder_host, &path);
                                            cx.notify();
                                        });
                                    },
                                ),
                            );
                        }
                        menu
                    },
                );
            }
            menu = menu.separator().item(action(
                "チャットをアーカイブ",
                owner.clone(),
                host.clone(),
                Intent::ArchiveProjectChats {
                    project_id: id.clone(),
                    before: chrono::Utc::now().timestamp(),
                },
            ));
            if project.chats_archived {
                menu = menu.item(action(
                    "アーカイブしたチャットを戻す",
                    owner.clone(),
                    host.clone(),
                    Intent::RestoreProjectChats {
                        project_id: id.clone(),
                    },
                ));
            }
            menu.separator().item(action(
                "プロジェクトを一覧から削除",
                owner.clone(),
                host.clone(),
                Intent::SetProjectHidden {
                    project_id: id.clone(),
                    hidden: true,
                },
            ))
        })
}

impl Desktop {
    fn edit_project(&mut self, host: &str, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let language = crate::localization::language(cx);
        let snapshot = match self.connection_store(host, cx) {
            Some(store) => store.snapshot(),
            None if host.is_empty() => self.publication.snapshot.clone(),
            None => return,
        };
        let Some(list) = snapshot.thread_list() else {
            return;
        };
        let Some(project) = list.projects.iter().find(|p| p.id == id) else {
            return;
        };
        self.project_editor = Some(ProjectEditor {
            id: id.into(),
            host: host.into(),
            scope: snapshot.connection.storage_scope.clone(),
            name: cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(project.name.clone())
                    .placeholder(language.text("表示名"))
            }),
            section: cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(project.section.clone().unwrap_or_default())
                    .placeholder(language.text("空欄で未分類"))
            }),
        });
    }
    pub(super) fn project_editor_view(&self, cx: &Context<Self>) -> AnyElement {
        let language = crate::localization::language(cx);
        let editor = self.project_editor.as_ref().unwrap();
        v_flex().flex_1().min_w_0().p_8().gap_4()
            .child(section_heading(language.text("プロジェクトを編集"), language.text("表示名とセクションはこの端末・接続先ごとに保存されます。フォルダや会話履歴は残ります。")))
            .child(language.text("表示名")).child(Input::new(&editor.name).aria_label(language.text("プロジェクトの表示名")))
            .child(language.text("セクション")).child(Input::new(&editor.section).aria_label(language.text("プロジェクトのセクション")))
            .child(h_flex().gap_2()
                .child(Button::new("save-project").label(language.text("保存")).primary().on_click(cx.listener(move |s, _, _, cx| {
                    let editor = s.project_editor.as_ref().unwrap();
                    let same_scope = match s.connection_store(&editor.host, cx) {
                        Some(store) => editor.scope == store.snapshot().connection.storage_scope,
                        None => editor.host.is_empty() && editor.scope == s.publication.snapshot.connection.storage_scope,
                    };
                    if !same_scope {
                        s.set_error("接続先が変わりました。プロジェクトを開き直してください");
                        s.project_editor = None; cx.notify(); return;
                    }
                    let name = editor.name.read(cx).value().to_string();
                    let section = editor.section.read(cx).value().to_string();
                    if name.trim().is_empty() || name.trim().chars().count()>100 || section.trim().chars().count()>60 {
                        s.set_error("表示名は1〜100文字、セクションは60文字以内で入力してください");
                    } else {
                        s.dispatch_host(&editor.host, Intent::EditProject { project_id: editor.id.clone(), name, section }, cx);
                        s.project_editor = None;
                    }
                    cx.notify();
                })))
                .child(Button::new("cancel-project").label(language.text("キャンセル")).on_click(cx.listener(move |s, _, _, cx| {
                    s.project_editor = None; cx.notify();
                }))))
            .child(self.error_notices(ErrorScope::Projects, cx))
            .into_any_element()
    }
    fn open_project_folder(&mut self, host: &str, path: &str) {
        if host != "local" && (!host.is_empty() || self.remote.is_some()) {
            self.set_error("接続先のフォルダはこの端末では開けません");
            return;
        }
        let path = Path::new(path);
        if !path.is_absolute() || !path.is_dir() {
            self.set_error("プロジェクトのフォルダが見つかりません");
            return;
        }
        #[cfg(target_os = "windows")]
        let program = "explorer.exe";
        #[cfg(target_os = "macos")]
        let program = "open";
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        let program = "xdg-open";
        let mut command = std::process::Command::new(program);
        command.arg(path);
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        if let Err(error) = command.spawn() {
            self.set_error(ErrorSource::diagnostic(
                "フォルダを開けません",
                error.to_string(),
            ));
        }
    }
}
