use crate::ui::Enabled;
use crate::{Connection, native};
use agent_core::{
    presentation::{permissions::PERMISSION_CHOICES, view::Presentation},
    state::{DraftKey, Intent, operations as op},
};
use agent_protocol::session::ProviderKind;
use nsh_core::prelude::*;
use serde_json::{Value, json};

#[derive(Clone, PartialEq)]
struct Picker {
    id: String,
    host: String,
    storage: String,
    draft: DraftKey,
    provider: Option<ProviderKind>,
    query: String,
}

impl Picker {
    fn owns(&self, host: &str, storage: &str, draft: &DraftKey) -> bool {
        self.host == host && self.storage == storage && &self.draft == draft
    }
}

pub(crate) fn controls() -> impl Element {
    let connection = use_context::<Connection>().unwrap();
    let publication = connection.publication;
    let picker = signal(None::<Picker>);
    effect(move || {
        let host = connection.host.get();
        publication.with(|p| {
            let draft = p.settings.draft_key();
            if picker.with_untracked(|picker| {
                picker.as_ref().is_some_and(|picker| {
                    !picker.owns(&host, &p.snapshot.connection.storage_scope, &draft)
                })
            }) {
                picker.set(None);
            }
            if let Some(request) = p.snapshot.permission_control(&draft).load_request {
                connection.dispatch(Intent::ReadPermissionSettings(request));
            }
        });
    });
    let label = computed(move || {
        publication.with(|p| {
            p.settings
                .selected_model_name()
                .unwrap_or_else(|| "モデル".into())
        })
    });
    view()
        .flex_row()
        .items_center()
        .child(
            crate::ui::text_button(
                move || SharedString::from(label.get()),
                move || {
                    let host = connection.host.get_untracked();
                    picker.set(Some(publication.with_untracked(|p| Picker {
                        id: uuid::Uuid::new_v4().to_string(),
                        host,
                        storage: p.snapshot.connection.storage_scope.clone(),
                        draft: p.settings.draft_key(),
                        provider: p.settings.provider(),
                        query: String::new(),
                    })));
                    connection.dispatch(Intent::ReadProviderSettings(op::ReadProviderSettings {}));
                    connection.dispatch(Intent::ListAccounts(op::ListAccounts {}));
                },
            )
            .enabled(move || {
                publication.with(|p| p.settings.connected() && !p.composer.controls.sending)
            }),
        )
        .child(
            native("bex.details")
                .prop("data", move || {
                    json!({"modelPicker": {"data": picker.with(|picker| {
                        picker.as_ref().map(|picker| publication.with(|p| picker_data(p, picker)))
                    })}})
                })
                .on_native_event(move |event: &NativeEvent| {
                    let Some(current) = picker.get_untracked().filter(|picker| {
                        event.data["token"].as_str() == Some(&picker.id)
                            && publication.with_untracked(|p| {
                                picker.owns(
                                    &connection.host.get_untracked(),
                                    &p.snapshot.connection.storage_scope,
                                    &p.settings.draft_key(),
                                )
                            })
                    }) else {
                        return;
                    };
                    let action = event.data["action"].as_str().unwrap_or_default();
                    let value = event.data["value"].as_str().unwrap_or_default();
                    match action {
                        "close" => picker.set(None),
                        "search" => picker.update(|picker| {
                            if let Some(picker) = picker {
                                picker.query = value.into();
                            }
                        }),
                        "accounts" => {
                            if let Some(provider) = current.provider {
                                picker.set(None);
                                connection.application.with(|app| {
                                    app.navigate(crate::Navigation::Provider(Some(provider)))
                                });
                            }
                        }
                        "provider"
                            if publication.with_untracked(|p| {
                                p.settings.connected() && !p.composer.controls.sending
                            }) =>
                        {
                            if let Ok(provider) =
                                serde_json::from_value::<ProviderKind>(json!(value))
                            {
                                picker.update(|picker| {
                                    if let Some(picker) = picker {
                                        picker.provider = Some(provider);
                                        picker.query.clear();
                                    }
                                });
                                if let Some(model) = publication
                                    .with_untracked(|p| p.settings.model_for_provider(provider))
                                {
                                    connection.dispatch(Intent::SelectModel {
                                        thread_id: current.draft,
                                        model,
                                    });
                                }
                            }
                        }
                        _ => {
                            if let Some(intent) = publication
                                .with_untracked(|p| picker_intent(p, &current, action, value))
                            {
                                connection.dispatch(intent);
                            }
                        }
                    }
                })
                .w(px(0.0))
                .h(px(0.0)),
        )
}

fn picker_data(p: &Presentation, picker: &Picker) -> Value {
    let settings = &p.settings;
    let selected = settings.selected_model();
    let controls = settings.quick_controls();
    let permissions = p.snapshot.permission_control(&picker.draft);
    let account = settings.accounts().and_then(|accounts| {
        accounts
            .accounts
            .iter()
            .find(|account| {
                Some(account.provider) == picker.provider && accounts.is_selected(account)
            })
            .cloned()
    });
    json!({
        "id": picker.id, "provider": picker.provider, "query": picker.query,
        "disabled": !settings.connected() || p.composer.controls.sending,
        "providers": settings.connection_setup().agents.iter().map(|agent| json!({"id": agent.provider, "title": agent.name})).collect::<Vec<_>>(),
        "models": settings.models_matching(picker.provider, picker.query.clone()).iter().map(|model| json!({"id": model.id, "title": model.display_name})).collect::<Vec<_>>(),
        "selectedModel": settings.current_model().map(|model| model.id),
        "loading": settings.models_loading(picker.provider),
        "errors": settings.model_error_messages(picker.provider),
        "notices": settings.notices().iter().map(|notice| json!({
            "id": serde_json::to_string(&notice.id).expect("Core error IDs are serializable"),
            "title": notice.title, "text": notice.text.to_string(), "recovery": notice.recovery_label,
        })).collect::<Vec<_>>(),
        "customModel": p.snapshot.custom_model_choice(picker.provider, &picker.query).map(|model| model.id),
        "quick": (selected.as_ref().map(|model| model.provider) == picker.provider).then(|| json!({
            "efforts": controls.efforts, "effort": controls.effort,
            "speeds": controls.speed_choices.iter().map(|choice| json!({"id": choice.id, "title": choice.label})).collect::<Vec<_>>(),
            "speed": controls.speed_choices.iter().find(|choice| choice.selected).map(|choice| &choice.id),
        })),
        "permissions": (permissions.provider == picker.provider && permissions.version.is_some()).then(|| json!({
            "selected": permissions.mode.and_then(|mode| PERMISSION_CHOICES.iter().position(|choice| choice.0 == mode)).map(|index| index.to_string()),
            "choices": PERMISSION_CHOICES.iter().enumerate().map(|(index, (_, title, _))| json!({"id": index.to_string(), "title": title})).collect::<Vec<_>>(),
        })),
        "account": account.as_ref().map(|account| json!({"title": account.email.as_ref().unwrap_or(&account.id), "asset": crate::accounts::icon(account.provider),
            "usage": settings.account_weekly_usage(account.provider, account.id.clone()),
        })),
    })
}

fn picker_intent(p: &Presentation, picker: &Picker, action: &str, value: &str) -> Option<Intent> {
    if matches!(action, "recover" | "dismiss") {
        let id = p
            .settings
            .notices()
            .into_iter()
            .find(|notice| serde_json::to_string(&notice.id).ok().as_deref() == Some(value))?
            .id;
        return Some(if action == "recover" {
            Intent::RecoverError { id }
        } else {
            Intent::DismissError { id }
        });
    }
    if !p.settings.connected() || p.composer.controls.sending {
        return None;
    }
    let thread_id = picker.draft.clone();
    Some(match action {
        "effort" | "speed"
            if p.settings.selected_model().map(|model| model.provider) != picker.provider =>
        {
            return None;
        }
        "model" => Intent::SelectModel {
            thread_id,
            model: p
                .settings
                .models_matching(picker.provider, picker.query.clone())
                .into_iter()
                .find(|model| model.id == value)?
                .model,
        },
        "custom" => Intent::SelectModel {
            thread_id,
            model: p
                .snapshot
                .custom_model_choice(picker.provider, &picker.query)?,
        },
        "effort" => Intent::SelectEffort {
            thread_id,
            effort: value.into(),
        },
        "speed" => Intent::SelectServiceTier {
            thread_id,
            service_tier: value.into(),
        },
        "permission" => {
            let control = p.snapshot.permission_control(&thread_id);
            if control.provider != picker.provider {
                return None;
            }
            let (mode, _, _) = PERMISSION_CHOICES.get(value.parse::<usize>().ok()?)?;
            Intent::UpdatePermissionSettings(op::UpdatePermissionSettings {
                provider: control.provider?,
                mode: *mode,
                version: control.version?.into(),
            })
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::state::{Draft, Snapshot};
    use std::sync::Arc;

    fn fixture() -> (Snapshot, Picker) {
        let mut state = Snapshot::default();
        state.connection.connected = true;
        state.connection.storage_scope = "storage".into();
        state.catalog.models = Arc::new(serde_json::from_value(json!([
            {"id":"codex/native","model":{"provider":"codex","id":"native"},"displayName":"日本語モデル 👋", "settings": {
                "defaultReasoningEffort":"medium", "supportedReasoningEfforts":[{"reasoningEffort":"low"},{"reasoningEffort":"medium"}],
                "serviceTiers":[{"id":"priority","name":"高速","fast":true}]
            }},
            {"id":"claude/native","model":{"provider":"claude","id":"native"},"displayName":"別のモデル"}
        ])).unwrap());
        Arc::make_mut(&mut state.catalog.account).accounts = Some(Arc::new(serde_json::from_value(json!({
            "selected":{"codex":"shared", "claude":"shared"}, "accounts":[
                {"id":"shared","provider":"claude","email":"other@example.invalid"},
                {"id":"shared","provider":"codex","email":"native@example.invalid", "usage":{"fetchedAt":1,"windows":[
                    {"label":"5時間枠", "remainingPercent":72}, {"label":"週間枠","remainingPercent":42}
                ]}}
            ]
        })).unwrap()));
        let draft = state.pane.navigation.draft_key.clone();
        Arc::make_mut(&mut state.input.drafts).insert(
            draft.clone(),
            Arc::new(Draft {
                model: Some(
                    serde_json::from_value(json!({"provider":"codex","id":"native"})).unwrap(),
                ),
                effort: Some("medium".into()),
                service_tier: Some("priority".into()),
                ..Default::default()
            }),
        );
        let picker = Picker {
            id: "native-picker".into(),
            host: "host".into(),
            storage: "storage".into(),
            draft,
            provider: Some(ProviderKind::Codex),
            query: "日本語".into(),
        };
        (state, picker)
    }

    #[test]
    fn native_model_form_uses_core_search_settings_and_provider_scoped_account_limits() {
        let (state, picker) = fixture();
        let p = Presentation::project(Arc::new(state), None);
        let data = picker_data(&p, &picker);
        let fixture: Value = serde_json::from_str(include_str!(
            "../apple/Tests/BexNshNativeTests/Fixtures/model-picker.json"
        ))
        .unwrap();
        assert_eq!(json!({"modelPicker": {"data": data}}), fixture);
        assert!(
            matches!(picker_intent(&p, &picker, "model", "codex/native"), Some(Intent::SelectModel {model, thread_id}) if model.provider == ProviderKind::Codex && model.id == "native" && thread_id == picker.draft)
        );
        assert!(picker_intent(&p, &picker, "model", "claude/native").is_none());
        assert!(picker_intent(&p, &picker, "model", "missing").is_none());
        assert!(
            matches!(picker_intent(&p, &picker, "custom", ""), Some(Intent::SelectModel {model, thread_id}) if model.provider == ProviderKind::Codex && model.id == "日本語" && thread_id == picker.draft)
        );
        assert!(
            matches!(picker_intent(&p, &picker, "effort", "low"), Some(Intent::SelectEffort {thread_id, effort}) if thread_id == picker.draft && effort == "low")
        );
        assert!(
            matches!(picker_intent(&p, &picker, "speed", "default"), Some(Intent::SelectServiceTier {thread_id, service_tier}) if thread_id == picker.draft && service_tier == "default")
        );
    }

    #[test]
    fn permissions_use_the_current_provider_and_version_and_invalid_actions_are_ignored() {
        use agent_protocol::permissions::{PermissionMode, PermissionSettings};
        let (mut state, mut picker) = fixture();
        state.catalog.permission_settings = Arc::new(
            [(
                ProviderKind::Codex,
                Arc::new(op::PermissionSettingsState {
                    result: Some(Ok(PermissionSettings {
                        mode: Some(PermissionMode::Ask),
                        version: "revision-2".into(),
                    })),
                }),
            )]
            .into(),
        );
        let p = Presentation::project(Arc::new(state), None);
        let data = picker_data(&p, &picker);
        assert_eq!(data["permissions"]["selected"], "0");
        assert_eq!(data["permissions"]["choices"][2]["title"], "フルアクセス");
        assert!(
            matches!(picker_intent(&p, &picker, "permission", "1"), Some(Intent::UpdatePermissionSettings(request)) if request.provider == ProviderKind::Codex && request.version == "revision-2" && request.mode == PermissionMode::Auto)
        );
        for (action, value) in [
            ("unknown", ""),
            ("permission", "9"),
            ("permission", "not-an-index"),
            ("recover", "old-error"),
            ("dismiss", "old-error"),
        ] {
            assert!(picker_intent(&p, &picker, action, value).is_none());
        }
        picker.provider = Some(ProviderKind::Claude);
        let data = picker_data(&p, &picker);
        assert!(data["permissions"].is_null());
        assert!(data["quick"].is_null());
        assert_eq!(data["account"]["title"], "other@example.invalid");
        for (action, value) in [
            ("permission", "1"),
            ("effort", "low"),
            ("speed", "default"),
            ("custom", ""),
        ] {
            assert!(picker_intent(&p, &picker, action, value).is_none());
        }
    }

    #[test]
    fn a_disconnected_model_form_keeps_error_recovery_and_dismissal_available() {
        let (mut state, picker) = fixture();
        state.connection.connected = false;
        state.operations.insert(
            op::OperationKey::Accounts,
            op::OperationState {
                generation: 7,
                phase: op::OperationPhase::Failed {
                    message: "account lookup failed".into(),
                },
            },
        );
        let p = Presentation::project(Arc::new(state), None);
        let id = op::ErrorId::Operation {
            key: op::OperationKey::Accounts,
            generation: 7,
        };
        let encoded = serde_json::to_string(&id).unwrap();
        assert_eq!(picker_data(&p, &picker)["notices"][0]["id"], encoded);
        assert!(
            matches!(picker_intent(&p, &picker, "recover", &encoded), Some(Intent::RecoverError {id: actual}) if actual == id)
        );
        assert!(
            matches!(picker_intent(&p, &picker, "dismiss", &encoded), Some(Intent::DismissError {id: actual}) if actual == id)
        );
        assert!(picker_intent(&p, &picker, "dismiss", "obsolete-error").is_none());
    }

    #[test]
    fn a_model_form_cannot_apply_to_a_replaced_host_account_draft_or_busy_submission() {
        let (mut state, picker) = fixture();
        assert!(picker.owns("host", "storage", &picker.draft));
        assert!(!picker.owns("other-host", "storage", &picker.draft));
        assert!(!picker.owns("host", "other-account", &picker.draft));
        assert!(!picker.owns("host", "storage", &DraftKey::from("other-draft")));
        for disconnected in [true, false] {
            state.connection.connected = !disconnected;
            if !disconnected {
                state.operations.insert(
                    op::OperationKey::Submission {
                        draft_key: picker.draft.clone(),
                    },
                    op::OperationState {
                        generation: 1,
                        phase: op::OperationPhase::Running,
                    },
                );
            }
            let p = Presentation::project(Arc::new(state.clone()), None);
            assert_eq!(picker_data(&p, &picker)["disabled"], true);
            for (action, value) in [
                ("model", "codex/native"),
                ("effort", "low"),
                ("speed", "default"),
                ("permission", "0"),
            ] {
                assert!(picker_intent(&p, &picker, action, value).is_none());
            }
        }
    }
}
