//! The list owns its native project editor; Core owns project decisions and persistence.
use crate::application::{Application, ListScope, ListSource};
use crate::l10n::{localized, tr};
use crate::{native, shared::Shared};
use agent_core::{
    presentation::{
        list::{ProjectSummary, ThreadList},
        project_menu::{ProjectAction, ProjectMenuItem},
    },
    state::{Intent, operations as op},
};
use nsh_core::prelude::*;
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(serde::Serialize, serde::Deserialize)]
struct Command {
    #[serde(flatten)]
    scope: ListScope,
    action: ProjectAction,
}

pub(crate) fn menu(
    project: &ProjectSummary,
    sections: &[String],
    scope: &ListScope,
    local: bool,
) -> Value {
    json!(encode(
        agent_core::presentation::project_menu::project_menu(
            project,
            sections,
            local,
            tr("フォルダを開く"),
        ),
        scope,
    ))
}

fn encode(items: Vec<ProjectMenuItem>, scope: &ListScope) -> Vec<Value> {
    items.into_iter().map(|item| match item {
            ProjectMenuItem::Separator => json!({"separator": true}),
            ProjectMenuItem::Submenu { title, items } => json!({"title": localized(title), "children": encode(items, scope)}),
            ProjectMenuItem::Action { title, action, checked, enabled } => json!({
                "title": localized(title), "selected": checked, "enabled": enabled,
                "id": serde_json::to_string(&Command { scope: scope.clone(), action }).expect("project menu commands serialize"),
            }),
        }).collect()
}

pub(crate) fn header_menu(
    hidden: &[ProjectSummary],
    desktop: bool,
    selected_host: &str,
    sources: &[ListSource],
) -> Value {
    let mut items = Vec::new();
    if desktop {
        items.push(json!({"id": "add", "title": tr("フォルダをプロジェクトに追加…"), "enabled": sources.iter().any(|source| source.scope.host == selected_host && source.scope.host == "local" && source.connected)}));
    }
    if !hidden.is_empty() {
        let hidden: Vec<_> = sources
            .iter()
            .flat_map(|source| {
                encode(
                    hidden
                        .iter()
                        .filter(|project| project.host_id == source.scope.host)
                        .map(|project| ProjectMenuItem::Action {
                            title: project.name.clone(),
                            checked: false,
                            enabled: true,
                            action: ProjectAction::Dispatch(Box::new(Intent::SetProjectHidden {
                                project_id: project.id.clone(),
                                hidden: false,
                            })),
                        })
                        .collect(),
                    &source.scope,
                )
            })
            .collect();
        items.push(json!({"title": tr("非表示のプロジェクトを戻す"), "children": hidden}));
    }
    json!(items)
}

#[derive(Clone, PartialEq)]
struct Editor {
    id: String,
    scope: ListScope,
    project: String,
    name: String,
    section: String,
    error: Option<String>,
}

#[derive(Clone, Copy)]
pub(crate) struct Projects {
    editor: Signal<Option<Editor>>,
    application: Stored<Arc<Application>>,
    host: Memo<String>,
    sources: Memo<Vec<ListSource>>,
    list: Memo<Option<Shared<ThreadList>>>,
}

impl Projects {
    pub fn new(
        application: Stored<Arc<Application>>,
        host: Memo<String>,
        sources: Memo<Vec<ListSource>>,
        list: Memo<Option<Shared<ThreadList>>>,
    ) -> Self {
        Self {
            editor: signal(None),
            application,
            host,
            sources,
            list,
        }
    }

    fn same_scope(self, scope: &ListScope) -> bool {
        self.application
            .with(|application| application.snapshot().list_scope_matches(scope))
    }

    pub fn select(self, command: &str) {
        if command == "add" {
            self.pick_folder();
            return;
        }
        let Ok(command) = serde_json::from_str::<Command>(command) else {
            return;
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let action = match command.action.into_intent(now) {
            Ok(intent) => {
                self.application
                    .with(|application| application.list_action(&command.scope, intent, false));
                return;
            }
            Err(action) => action,
        };
        if !self.same_scope(&command.scope) {
            return;
        }
        match action {
            ProjectAction::Edit(id) => {
                let project = self.list.with_untracked(|list| {
                    list.as_ref()?
                        .projects
                        .iter()
                        .find(|project| project.id == id && project.host_id == command.scope.host)
                        .map(|project| (project.name.clone(), project.section.clone()))
                });
                if let Some((name, section)) = project {
                    self.editor.set(Some(Editor {
                        id: uuid::Uuid::new_v4().to_string(),
                        scope: command.scope,
                        project: id,
                        name,
                        section: section.unwrap_or_default(),
                        error: None,
                    }));
                }
            }
            ProjectAction::OpenFolder(path) if command.scope.host == "local" => {
                #[cfg(target_os = "macos")]
                native::macos::open_directory(&path);
                #[cfg(not(target_os = "macos"))]
                let _ = path;
            }
            ProjectAction::OpenFolder(_) => native::message(
                tr("フォルダを開く"),
                tr("接続先のフォルダはこの端末では開けません"),
            ),
            _ => unreachable!("project operations resolve in Core"),
        }
    }

    fn pick_folder(self) {
        if !is_desktop() || self.host.get_untracked() != "local" {
            return;
        }
        let Some(scope) = self.sources.with_untracked(|sources| {
            sources
                .iter()
                .find(|source| source.scope.host == self.host.get_untracked())
                .map(|source| source.scope.clone())
        }) else {
            return;
        };
        let choice = pick_paths(nsh_core::file_dialog::OpenDialog::folders());
        spawn(async move {
            match choice.await {
                Ok(Some(paths)) => {
                    if let Some(path) = paths.into_iter().next() {
                        self.application.with(|application| {
                            application.list_action(
                                &scope,
                                Intent::AddProject(op::AddProject {
                                    cwd: path.to_string_lossy().into_owned(),
                                }),
                                false,
                            )
                        });
                    }
                }
                Ok(_) => {}
                Err(reason) => native::message(tr("プロジェクトを追加"), reason),
            }
        });
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
                if !self.same_scope(&editor.scope) {
                    self.editor.set(None);
                    native::message(
                        tr("プロジェクトを編集"),
                        tr("接続先が変わりました。プロジェクトを開き直してください"),
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
                            editor.error = Some(localized(reason.into()));
                        }
                    });
                    return;
                }
                self.application.with(|application| {
                    application.list_action(
                        &editor.scope,
                        Intent::EditProject {
                            project_id: editor.project,
                            name: name.into(),
                            section: section.into(),
                        },
                        false,
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
            scope: ListScope {
                host: "local".into(),
                storage: "account".into(),
            },
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
