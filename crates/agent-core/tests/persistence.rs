use agent_core::{
    persistence,
    state::{Draft, Snapshot},
};
use serde_json::json;
use std::sync::Arc;

#[test]
fn remembered_models_apply_across_hosts_without_replacing_their_user_work() {
    let selected = agent_core::state::ModelPreferences {
        provider: Some(agent_protocol::session::ProviderKind::Claude),
        selections: [(
            agent_protocol::session::ProviderKind::Claude,
            agent_core::state::ModelSelection {
                model: agent_protocol::models::ModelRef {
                    provider: agent_protocol::session::ProviderKind::Claude,
                    id: "sonnet".into(),
                },
                effort: Some("high".into()),
                service_tier: Some("fast".into()),
            },
        )]
        .into(),
    };
    let preferences = persistence::encode_model_preferences(&Snapshot {
        model_preferences: selected.clone(),
        ..Default::default()
    })
    .unwrap();
    for host in ["first", "second"] {
        let mut snapshot = Snapshot {
            storage_scope: host.into(),
            ..Default::default()
        };
        Arc::make_mut(&mut snapshot.drafts).insert(
            "existing".into(),
            Arc::new(Draft {
                text: format!("{host}'s draft"),
                effort: Some("medium".into()),
                ..Default::default()
            }),
        );
        let saved = persistence::encode(&snapshot).unwrap();
        let restored = persistence::decode(
            &persistence::apply_model_preferences(&saved, &preferences).unwrap(),
        )
        .unwrap();
        assert_eq!(
            restored,
            Snapshot {
                model_preferences: selected.clone(),
                ..snapshot.clone()
            }
        );
        assert_eq!(
            persistence::decode(&persistence::apply_model_preferences(&saved, &[]).unwrap())
                .unwrap(),
            snapshot
        );
    }
    assert_eq!(
        persistence::decode(&persistence::apply_model_preferences(&[], &preferences).unwrap())
            .unwrap()
            .model_preferences,
        selected
    );
    assert!(persistence::apply_model_preferences(b"{}", &preferences).is_err());
    assert!(persistence::apply_model_preferences(&[], b"invalid").is_err());
}

#[test]
fn absent_model_preferences_preserve_user_work_but_invalid_preferences_fail() {
    let mut snapshot = Snapshot::default();
    Arc::make_mut(&mut snapshot.drafts).insert(
        "unsent".into(),
        Arc::new(Draft {
            text: "keep this request".into(),
            ..Default::default()
        }),
    );
    let mut saved: serde_json::Value =
        serde_json::from_slice(&persistence::encode(&snapshot).unwrap()).unwrap();
    saved.as_object_mut().unwrap().remove("model_preferences");
    assert_eq!(
        persistence::decode(&serde_json::to_vec(&saved).unwrap()).unwrap(),
        snapshot
    );
    saved["model_preferences"] = json!(17);
    assert!(persistence::decode(&serde_json::to_vec(&saved).unwrap()).is_err());
}

#[test]
fn runtime_fields_cannot_override_restored_user_work() {
    let mut snapshot = Snapshot::default();
    Arc::make_mut(&mut snapshot.drafts).insert(
        agent_protocol::session::SessionRef {
            provider: agent_protocol::session::ProviderKind::Codex,
            id: "thread".into(),
        }
        .into(),
        Arc::new(Draft {
            text: "keep this draft".into(),
            ..Default::default()
        }),
    );
    let mut saved: serde_json::Value =
        serde_json::from_slice(&persistence::encode(&snapshot).unwrap()).unwrap();
    saved["conversations"] = json!("invalid discardable history");
    saved["workspace"] = json!(42);
    saved["connected"] = json!(true);
    saved["epoch"] = json!(99);
    saved["requests"] = json!({"untrusted": "approval"});
    saved["activity"] = json!(17);
    let restored = persistence::decode(&serde_json::to_vec(&saved).unwrap()).unwrap();
    assert_eq!(restored, snapshot);
}

#[test]
fn missing_or_invalid_user_work_is_not_silently_discarded() {
    let saved: serde_json::Value =
        serde_json::from_slice(&persistence::encode(&Snapshot::default()).unwrap()).unwrap();
    for field in [
        "storage_scope",
        "archived_scopes",
        "drafts",
        "pending_submissions",
        "file_drafts",
        "navigation",
    ] {
        let mut missing = saved.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert!(
            persistence::decode(&serde_json::to_vec(&missing).unwrap()).is_err(),
            "missing {field}"
        );
        let mut damaged = saved.clone();
        damaged[field] = json!(17);
        assert!(
            persistence::decode(&serde_json::to_vec(&damaged).unwrap()).is_err(),
            "damaged {field}"
        );
    }
    assert!(persistence::decode(b"{").is_err());
    assert_eq!(persistence::decode(b"").unwrap(), Snapshot::default());
}
