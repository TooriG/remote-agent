//! The list owns its native project editor; Core owns project decisions and persistence.
use crate::{client::Client, native, shared::Shared};
use agent_core::{
    presentation::{
        list::ProjectSummary,
        project_menu::{ProjectAction, ProjectMenuItem},
        view::Presentation,
    },
    state::{Intent, operations as op},
};
use nsh_core::prelude::*;
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(serde::Serialize, serde::Deserialize)]
struct Command {
    host: String,
    storage: String,
    action: ProjectAction,
}

pub(crate) fn menu(
    project: &ProjectSummary,
    sections: &[String],
    host: &str,
    storage: &str,
    local: bool,
) -> Value {
    encode(
        agent_core::presentation::project_menu::project_menu(
            project,
            sections,
            local,
            "フォルダを開く",
        ),
        host,
        storage,
    )
}

fn encode(items: Vec<ProjectMenuItem>, host: &str, storage: &str) -> Value {
    json!(items.into_iter().map(|item| match item {
            ProjectMenuItem::Separator => json!({"separator": true}),
            ProjectMenuItem::Submenu { title, items } => json!({"title": title, "children": encode(items, host, storage)}),
            ProjectMenuItem::Action { title, action, checked, enabled } => json!({
                "title": title, "selected": checked, "enabled": enabled,
                "id": serde_json::to_string(&Command { host: host.into(), storage: storage.into(), action }).expect("project menu commands serialize"),
            }),
        }).collect::<Vec<_>>())
}

#[derive(Clone, PartialEq)]
struct Editor {
    id: String,
    host: String,
    storage: String,
    project: String,
    name: String,
    section: String,
    error: Option<String>,
}

#[derive(Clone, Copy)]
pub(crate) struct Projects {
    editor: Signal<Option<Editor>>,
    client: Stored<Arc<Client>>,
    host: Memo<String>,
    publication: Memo<Shared<Presentation>>,
}

impl Projects {
    pub fn new(
        client: Stored<Arc<Client>>,
        host: Memo<String>,
        publication: Memo<Shared<Presentation>>,
    ) -> Self {
        Self {
            editor: signal(None),
            client,
            host,
            publication,
        }
    }

    fn same_scope(self, host: &str, storage: &str) -> bool {
        self.host.get_untracked() == host
            && self
                .publication
                .with_untracked(|p| p.snapshot.connection.storage_scope == storage)
    }

    pub fn select(self, command: &str) {
        if command == "add" {
            self.pick_folder();
            return;
        }
        let Ok(command) = serde_json::from_str::<Command>(command) else {
            return;
        };
        if !self.same_scope(&command.host, &command.storage) {
            return;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        match command.action.into_intent(now) {
            Ok(intent) => self
                .client
                .with(|client| client.dispatch(&command.host, None, intent)),
            Err(ProjectAction::Edit(id)) => {
                let project = self.publication.with_untracked(|p| {
                    p.list
                        .as_ref()?
                        .projects
                        .iter()
                        .find(|project| project.id == id)
                        .map(|project| (project.name.clone(), project.section.clone()))
                });
                if let Some((name, section)) = project {
                    self.editor.set(Some(Editor {
                        id: uuid::Uuid::new_v4().to_string(),
                        host: command.host,
                        storage: command.storage,
                        project: id,
                        name,
                        section: section.unwrap_or_default(),
                        error: None,
                    }));
                }
            }
            Err(ProjectAction::OpenFolder(path)) if command.host == "local" => {
                #[cfg(target_os = "macos")]
                native::macos::open_directory(&path);
                #[cfg(not(target_os = "macos"))]
                let _ = path;
            }
            Err(ProjectAction::OpenFolder(_)) => {
                native::message("フォルダを開く", "接続先のフォルダはこの端末では開けません")
            }
            Err(_) => unreachable!("project operations resolve in Core"),
        }
    }

    fn pick_folder(self) {
        if !is_desktop() || self.host.get_untracked() != "local" {
            return;
        }
        let host = self.host.get_untracked();
        let storage = self
            .publication
            .with_untracked(|p| p.snapshot.connection.storage_scope.clone());
        let choice = pick_paths(nsh_core::file_dialog::OpenDialog::folders());
        spawn(async move {
            match choice.await {
                Ok(Some(paths)) if self.same_scope(&host, &storage) => {
                    if let Some(path) = paths.into_iter().next() {
                        self.client.with(|client| {
                            client.dispatch(
                                &host,
                                None,
                                Intent::AddProject(op::AddProject {
                                    cwd: path.to_string_lossy().into_owned(),
                                }),
                            )
                        });
                    }
                }
                Ok(_) => {}
                Err(reason) => native::message("プロジェクトを追加", reason),
            }
        });
    }

    pub fn header(self) -> impl IntoElement {
        view().flex_row().items_center().px_2()
            .child(text("プロジェクト").text_size(px(12.0)).text_color(move || nsh_base::theme().muted_foreground).flex_1())
            .child(when(move || is_desktop() || self.publication.with(|p| p.list.as_ref().is_some_and(|list| !list.hidden_projects.is_empty())), move || native::menu("plus", "プロジェクトを管理", move || {
                let host = self.host.get();
                self.publication.with(|p| {
                    let mut items = Vec::new();
                    if is_desktop() {
                        items.push(json!({"id": "add", "title": "フォルダをプロジェクトに追加…", "enabled": host == "local" && p.composer.controls.connected}));
                    }
                    if let Some(list) = &p.list {
                        let hidden = list.hidden_projects.iter().map(|project| ProjectMenuItem::Action {
                            title: project.name.clone(), checked: false, enabled: true,
                            action: ProjectAction::Dispatch(Box::new(Intent::SetProjectHidden { project_id: project.id.clone(), hidden: false })),
                        }).collect::<Vec<_>>();
                        if !hidden.is_empty() {
                            items.push(json!({"title": "非表示のプロジェクトを戻す", "children": encode(hidden, &host, &p.snapshot.connection.storage_scope)}));
                        }
                    }
                    json!(items)
                })
            }, move |command| self.select(command))
                .prop("appearance", json!("plain"))
                .w(px(if is_desktop() { 24.0 } else { 44.0 }))
                .h(px(if is_desktop() { 24.0 } else { 44.0 }))))
    }

    pub fn view(self) -> impl IntoElement {
        native("bex.details")
            .prop("data", move || {
                self.editor.with(|editor| editor_data(editor.as_ref()))
            })
            .on_native_event(move |event: &NativeEvent| {
                let Some(editor) = self
                    .editor
                    .get_untracked()
                    .filter(|editor| event.data["token"].as_str() == Some(editor.id.as_str()))
                else {
                    return;
                };
                if event.data["action"] == "cancel" {
                    self.editor.set(None);
                    return;
                }
                if event.data["action"] != "save" {
                    return;
                }
                if !self.same_scope(&editor.host, &editor.storage) {
                    self.editor.set(None);
                    native::message(
                        "プロジェクトを編集",
                        "接続先が変わりました。プロジェクトを開き直してください",
                    );
                    return;
                }
                let (Some(name), Some(section)) =
                    (event.data["name"].as_str(), event.data["section"].as_str())
                else {
                    return;
                };
                if let Err(reason) =
                    agent_core::state::projects::validate_project_labels(name, section)
                {
                    self.editor.update(|editor| {
                        if let Some(editor) = editor {
                            editor.error = Some(reason.into());
                        }
                    });
                    return;
                }
                self.client.with(|client| {
                    client.dispatch(
                        &editor.host,
                        None,
                        Intent::EditProject {
                            project_id: editor.project,
                            name: name.into(),
                            section: section.into(),
                        },
                    )
                });
                self.editor.set(None);
            })
            .w(px(0.0))
            .h(px(0.0))
    }
}

fn editor_data(editor: Option<&Editor>) -> Value {
    json!({"projectEditor": {"editor": editor.map(|editor| json!({
        "id": editor.id, "name": editor.name, "section": editor.section, "error": editor.error,
    }))}})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_editor_contract_preserves_input_and_validation_without_exposing_routing_state() {
        let editor = Editor {
            id: "fixture-editor".into(),
            host: "local".into(),
            storage: "account".into(),
            project: "project".into(),
            name: "日本語のプロジェクト 👋".into(),
            section: "個人".into(),
            error: Some("表示名は1〜100文字、セクションは60文字以内で入力してください".into()),
        };
        let expected: Value = serde_json::from_str(include_str!(
            "../apple/Tests/BexNshNativeTests/Fixtures/project-editor.json"
        ))
        .unwrap();
        assert_eq!(editor_data(Some(&editor)), expected);
        assert_eq!(
            editor_data(None),
            json!({"projectEditor": {"editor": null}})
        );
    }
}
