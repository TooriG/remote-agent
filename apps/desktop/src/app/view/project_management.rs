use super::*;
use agent_core::presentation::project_menu::{ProjectAction, ProjectMenuItem};
use gpui_kit::component::menu::PopupMenu;

fn populate(
    mut menu: PopupMenu,
    entries: Vec<ProjectMenuItem>,
    host: String,
    storage: String,
    owner: WeakEntity<Desktop>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    for entry in entries {
        menu = match entry {
            ProjectMenuItem::Separator => menu.separator(),
            ProjectMenuItem::Submenu { title, items } => {
                let owner = owner.clone();
                let host = host.clone();
                let storage = storage.clone();
                menu.submenu(title, window, cx, move |menu, window, cx| {
                    populate(
                        menu,
                        items.clone(),
                        host.clone(),
                        storage.clone(),
                        owner.clone(),
                        window,
                        cx,
                    )
                })
            }
            ProjectMenuItem::Action {
                title,
                action,
                checked,
                enabled,
            } => {
                let owner = owner.clone();
                let host = host.clone();
                let storage = storage.clone();
                menu.item(
                    PopupMenuItem::new(title)
                        .checked(checked)
                        .disabled(!enabled)
                        .on_click(move |_, window, cx| {
                            let _ = owner.update(cx, |state, cx| {
                                if !state.project_scope_matches(&host, &storage, cx) {
                                    return;
                                }
                                match action.clone().into_intent(chrono::Utc::now().timestamp()) {
                                    Ok(intent) => {
                                        state.dispatch_host(&host, intent, cx);
                                    }
                                    Err(ProjectAction::Edit(id)) => {
                                        state.edit_project(&host, &id, window, cx)
                                    }
                                    Err(ProjectAction::OpenFolder(path)) => {
                                        state.open_project_folder(&host, &path)
                                    }
                                    Err(_) => unreachable!("project operations resolve in Core"),
                                }
                                cx.notify();
                            });
                        }),
                )
            }
        };
    }
    menu
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
        .dropdown_menu(move |menu, window, cx| {
            let Some(entity) = owner.upgrade() else {
                return menu;
            };
            let state = entity.read(cx);
            let local = host == "local" || (host.is_empty() && state.remote.is_none());
            let snapshot = match state.connection_store(&host, cx) {
                Some(store) => store.snapshot(),
                None if host.is_empty() => state.snapshot.clone(),
                None => return menu,
            };
            let Some(list) = snapshot.thread_list() else {
                return menu;
            };
            let Some(project) = list.projects.iter().find(|project| project.id == id) else {
                return menu;
            };
            let sections = list
                .projects
                .iter()
                .filter_map(|project| project.section.clone())
                .collect::<Vec<_>>();
            let label = if cfg!(target_os = "windows") {
                "エクスプローラーで開く"
            } else {
                "フォルダを開く"
            };
            let items = agent_core::presentation::project_menu::project_menu(
                project, &sections, local, label,
            );
            populate(
                menu,
                items,
                host.clone(),
                snapshot.connection.storage_scope.clone(),
                owner.clone(),
                window,
                cx,
            )
        })
}

impl Desktop {
    fn project_scope_matches(&self, host: &str, storage: &str, cx: &App) -> bool {
        match self.connection_store(host, cx) {
            Some(store) => storage == store.snapshot().connection.storage_scope,
            None => host.is_empty() && storage == self.snapshot.connection.storage_scope,
        }
    }

    fn edit_project(&mut self, host: &str, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let language = crate::localization::language(cx);
        let snapshot = match self.connection_store(host, cx) {
            Some(store) => store.snapshot(),
            None if host.is_empty() => self.snapshot.clone(),
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
                    if !s.project_scope_matches(&editor.host, &editor.scope, cx) {
                        s.set_error(language.text("接続先が変わりました。プロジェクトを開き直してください").into());
                        s.project_editor = None; cx.notify(); return;
                    }
                    let name = editor.name.read(cx).value().to_string();
                    let section = editor.section.read(cx).value().to_string();
                    if let Err(reason) = agent_core::state::projects::validate_project_labels(&name, &section) {
                        s.set_error(language.text(reason).into());
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
            self.set_error("接続先のフォルダはこの端末では開けません".into());
            return;
        }
        let path = Path::new(path);
        if !path.is_absolute() || !path.is_dir() {
            self.set_error("プロジェクトのフォルダが見つかりません".into());
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
            self.set_error(format!("フォルダを開けません: {error}"));
        }
    }
}
