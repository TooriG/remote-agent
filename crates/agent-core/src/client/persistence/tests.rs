use super::*;
use crate::state::{Draft, Event, Intent, ModelDefaultsScope, ProviderModelDefaults, reduce};
use agent_protocol::{editors::EditorId, session::ProviderKind};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[derive(Default)]
struct DeviceDisk {
    models: Mutex<Vec<u8>>,
    editor: Mutex<Vec<u8>>,
    writes: AtomicUsize,
    fail_models: AtomicBool,
}
#[derive(Default)]
struct Storage {
    local: Mutex<Vec<u8>>,
    device: Arc<DeviceDisk>,
    fail_local: AtomicBool,
    saved: tokio::sync::Notify,
}
impl ClientStorage for Storage {
    fn load(&self, key: StorageKey) -> Result<Vec<u8>, AgentError> {
        Ok(match key {
            StorageKey::LocalState => self.local.lock().unwrap().clone(),
            StorageKey::ModelPreferences => self.device.models.lock().unwrap().clone(),
            StorageKey::EditorPreferences => self.device.editor.lock().unwrap().clone(),
        })
    }
    fn save(&self, key: StorageKey, bytes: Vec<u8>) -> Result<(), AgentError> {
        match key {
            StorageKey::LocalState => {
                if self.fail_local.load(Ordering::SeqCst) {
                    return Err(error("disk unavailable"));
                }
                *self.local.lock().unwrap() = bytes;
                self.saved.notify_one();
            }
            StorageKey::ModelPreferences => {
                if self.device.fail_models.load(Ordering::SeqCst) {
                    return Err(error("preferences unavailable"));
                }
                *self.device.models.lock().unwrap() = bytes;
            }
            StorageKey::EditorPreferences => *self.device.editor.lock().unwrap() = bytes,
        }
        self.device.writes.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
fn draft(snapshot: &mut Snapshot, text: &str) {
    Arc::make_mut(&mut snapshot.drafts).insert(
        "draft".into(),
        Arc::new(Draft {
            text: text.into(),
            ..Default::default()
        }),
    );
}
fn edit(snapshot: &Snapshot, intent: Intent) -> Snapshot {
    reduce(snapshot, Event::Intent(intent)).0
}
fn settings(snapshot: &Snapshot) -> &ProviderModelDefaults {
    &snapshot.model_defaults.providers[&ProviderKind::Codex]
}
fn session(storage: Arc<Storage>, device: Arc<DevicePreferences>) -> Arc<Persistence> {
    Persistence::new(storage, device)
}

#[tokio::test]
async fn device_preferences_apply_across_hosts_without_replacing_their_work() {
    let mut defaults = Snapshot::default();
    defaults.model_defaults.providers.insert(
        ProviderKind::Codex,
        ProviderModelDefaults {
            effort: Some("high".into()),
            service_tier: Some("priority".into()),
            ..Default::default()
        },
    );
    defaults.preferred_editor = Some(EditorId::Zed);
    let disk = Arc::new(DeviceDisk::default());
    *disk.models.lock().unwrap() =
        serde_json::to_vec(&ModelPreferences::capture(&defaults)).unwrap();
    *disk.editor.lock().unwrap() =
        serde_json::to_vec(&EditorPreferences::capture(&defaults)).unwrap();
    let device = Arc::new(DevicePreferences::default());
    for host in ["first", "second"] {
        let storage = Arc::new(Storage {
            device: disk.clone(),
            ..Default::default()
        });
        let mut saved = Snapshot {
            storage_scope: host.into(),
            ..Default::default()
        };
        draft(&mut saved, &format!("{host}'s draft"));
        *storage.local.lock().unwrap() = crate::persistence::encode(&saved).unwrap();
        let restored = session(storage, device.clone())
            .restore(false, None)
            .await
            .unwrap();
        assert_eq!(restored.drafts, saved.drafts);
        assert_eq!(restored.navigation, saved.navigation);
        assert_eq!(restored.storage_scope, host);
        assert_eq!(restored.model_defaults, defaults.model_defaults);
        assert_eq!(restored.preferred_editor, Some(EditorId::Zed));
    }
    assert_eq!(disk.writes.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn local_presets_initialize_device_preferences_without_losing_existing_work() {
    let storage = Arc::new(Storage::default());
    let mut saved = edit(
        &Snapshot::default(),
        Intent::SelectDefaultEffort {
            scope: ModelDefaultsScope::Global,
            provider: ProviderKind::Codex,
            effort: Some("high".into()),
        },
    );
    saved = edit(
        &saved,
        Intent::SelectDefaultServiceTier {
            scope: ModelDefaultsScope::Environment { id: "first".into() },
            provider: ProviderKind::Claude,
            service_tier: Some("fast".into()),
        },
    );
    draft(&mut saved, "existing Android draft");
    *storage.local.lock().unwrap() = crate::persistence::encode(&saved).unwrap();
    let persistence = session(storage.clone(), Arc::default());
    let restored = persistence.restore(false, None).await.unwrap();
    assert_eq!(restored.model_defaults, saved.model_defaults);
    assert_eq!(restored.scoped_model_defaults, saved.scoped_model_defaults);
    assert_eq!(restored.drafts, saved.drafts);
    persistence.start(&restored);
    persistence
        .finish(Arc::new(restored))
        .unwrap()
        .await
        .unwrap();
    let other = session(
        Arc::new(Storage {
            device: storage.device.clone(),
            ..Default::default()
        }),
        Arc::default(),
    )
    .restore(false, None)
    .await
    .unwrap();
    assert_eq!(other.model_defaults, saved.model_defaults);
    assert_eq!(other.scoped_model_defaults, saved.scoped_model_defaults);
    assert!(other.drafts.is_empty());
}

#[tokio::test]
async fn simultaneous_views_keep_independent_preference_edits_when_closed_in_reverse_order() {
    let disk = Arc::new(DeviceDisk::default());
    let device = Arc::new(DevicePreferences::default());
    let main_storage = Arc::new(Storage {
        device: disk.clone(),
        ..Default::default()
    });
    let side_storage = Arc::new(Storage {
        device: disk.clone(),
        ..Default::default()
    });
    let main = session(main_storage.clone(), device.clone());
    let side = session(side_storage.clone(), device.clone());
    let mut first = main.restore(false, None).await.unwrap();
    let mut second = side.restore(false, None).await.unwrap();
    main.start(&first);
    side.start(&second);
    let starting_model = agent_protocol::models::ModelRef {
        provider: ProviderKind::Codex,
        id: "preferred".into(),
    };
    first = edit(
        &first,
        Intent::SelectNewChatModel {
            scope: ModelDefaultsScope::Global,
            model: Some(starting_model.clone()),
        },
    );
    for scope in [
        ModelDefaultsScope::Global,
        ModelDefaultsScope::Environment { id: "host".into() },
    ] {
        first = edit(
            &first,
            Intent::SelectDefaultEffort {
                scope: scope.clone(),
                provider: ProviderKind::Codex,
                effort: Some("high".into()),
            },
        );
        second = edit(
            &second,
            Intent::SelectDefaultServiceTier {
                scope,
                provider: ProviderKind::Codex,
                service_tier: Some("priority".into()),
            },
        );
    }
    draft(&mut first, "main draft");
    draft(&mut second, "side draft");
    main.enqueue(Arc::new(first.clone()))
        .unwrap()
        .await
        .unwrap();
    side.enqueue(Arc::new(second.clone()))
        .unwrap()
        .await
        .unwrap();
    side.finish(Arc::new(second)).unwrap().await.unwrap();
    main.finish(Arc::new(first)).unwrap().await.unwrap();
    let restored = session(
        Arc::new(Storage {
            device: disk,
            ..Default::default()
        }),
        Arc::default(),
    )
    .restore(false, None)
    .await
    .unwrap();
    assert_eq!(settings(&restored).effort.as_deref(), Some("high"));
    assert_eq!(restored.model_defaults.new_chat_model, Some(starting_model));
    assert_eq!(
        settings(&restored).service_tier.as_deref(),
        Some("priority")
    );
    let scoped = &restored.scoped_model_defaults
        [&ModelDefaultsScope::Environment { id: "host".into() }]
        .providers[&ProviderKind::Codex];
    assert_eq!(scoped.effort.as_deref(), Some("high"));
    assert_eq!(scoped.service_tier.as_deref(), Some("priority"));
    let main = crate::persistence::decode(&main_storage.local.lock().unwrap()).unwrap();
    let side = crate::persistence::decode(&side_storage.local.lock().unwrap()).unwrap();
    assert_eq!(main.drafts[&"draft".into()].text, "main draft");
    assert_eq!(side.drafts[&"draft".into()].text, "side draft");
    assert!(restored.drafts.is_empty());
}

#[tokio::test]
async fn retrying_a_failed_local_save_does_not_reapply_obsolete_device_edits() {
    let disk = Arc::new(DeviceDisk::default());
    let device = Arc::new(DevicePreferences::default());
    let storage = Arc::new(Storage {
        device: disk.clone(),
        ..Default::default()
    });
    let first = session(storage.clone(), device.clone());
    let second = session(
        Arc::new(Storage {
            device: disk.clone(),
            ..Default::default()
        }),
        device,
    );
    let initial = first.restore(false, None).await.unwrap();
    let other = second.restore(false, None).await.unwrap();
    first.start(&initial);
    second.start(&other);
    let mut high = edit(
        &initial,
        Intent::SelectDefaultEffort {
            scope: ModelDefaultsScope::Global,
            provider: ProviderKind::Codex,
            effort: Some("high".into()),
        },
    );
    draft(&mut high, "recover this draft");
    storage.fail_local.store(true, Ordering::SeqCst);
    assert!(
        first
            .enqueue(Arc::new(high.clone()))
            .unwrap()
            .await
            .is_err()
    );
    assert!(first.failure().unwrap().contains("disk unavailable"));
    let low = edit(
        &other,
        Intent::SelectDefaultEffort {
            scope: ModelDefaultsScope::Global,
            provider: ProviderKind::Codex,
            effort: Some("low".into()),
        },
    );
    second.finish(Arc::new(low)).unwrap().await.unwrap();
    storage.fail_local.store(false, Ordering::SeqCst);
    first.finish(Arc::new(high)).unwrap().await.unwrap();
    assert!(first.failure().is_none());
    let restored = session(storage, Arc::default())
        .restore(false, None)
        .await
        .unwrap();
    assert_eq!(settings(&restored).effort.as_deref(), Some("low"));
    assert_eq!(restored.drafts[&"draft".into()].text, "recover this draft");
}

#[tokio::test]
async fn corrupted_state_or_preferences_are_rejected_without_writing() {
    for key in [
        StorageKey::LocalState,
        StorageKey::ModelPreferences,
        StorageKey::EditorPreferences,
    ] {
        let storage = Arc::new(Storage::default());
        match key {
            StorageKey::LocalState => *storage.local.lock().unwrap() = b"{}".to_vec(),
            StorageKey::ModelPreferences => {
                *storage.device.models.lock().unwrap() = b"invalid".to_vec()
            }
            StorageKey::EditorPreferences => {
                *storage.device.editor.lock().unwrap() = b"invalid".to_vec()
            }
        }
        assert!(
            session(storage.clone(), Arc::default())
                .restore(false, None)
                .await
                .is_err()
        );
        assert_eq!(storage.device.writes.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn failed_device_writes_remain_dirty_when_another_session_saves() {
    let disk = Arc::new(DeviceDisk::default());
    let device = Arc::new(DevicePreferences::default());
    let first = session(
        Arc::new(Storage {
            device: disk.clone(),
            ..Default::default()
        }),
        device.clone(),
    );
    let second = session(
        Arc::new(Storage {
            device: disk.clone(),
            ..Default::default()
        }),
        device,
    );
    let baseline = first.restore(false, None).await.unwrap();
    let other = second.restore(false, None).await.unwrap();
    first.start(&baseline);
    second.start(&other);
    let high = edit(
        &baseline,
        Intent::SelectDefaultEffort {
            scope: ModelDefaultsScope::Global,
            provider: ProviderKind::Codex,
            effort: Some("high".into()),
        },
    );
    disk.fail_models.store(true, Ordering::SeqCst);
    assert!(
        first
            .enqueue(Arc::new(high.clone()))
            .unwrap()
            .await
            .is_err()
    );
    assert!(disk.models.lock().unwrap().is_empty());
    disk.fail_models.store(false, Ordering::SeqCst);
    let other = edit(
        &other,
        Intent::SetPreferredEditor {
            editor: EditorId::Zed,
        },
    );
    second.finish(Arc::new(other)).unwrap().await.unwrap();
    first.finish(Arc::new(high)).unwrap().await.unwrap();
    let restored = session(
        Arc::new(Storage {
            device: disk,
            ..Default::default()
        }),
        Arc::default(),
    )
    .restore(false, None)
    .await
    .unwrap();
    assert_eq!(settings(&restored).effort.as_deref(), Some("high"));
    assert_eq!(restored.preferred_editor, Some(EditorId::Zed));
}

#[tokio::test]
async fn final_save_seals_the_queue_even_if_earlier_waiters_are_cancelled() {
    let storage = Arc::new(Storage::default());
    let persistence = session(storage.clone(), Arc::default());
    let mut snapshot = persistence.restore(false, None).await.unwrap();
    persistence.start(&snapshot);
    draft(&mut snapshot, "earlier");
    drop(persistence.enqueue(Arc::new(snapshot.clone())).unwrap());
    draft(&mut snapshot, "final");
    let final_save = persistence.finish(Arc::new(snapshot.clone())).unwrap();
    assert!(persistence.enqueue(Arc::new(Snapshot::default())).is_err());
    final_save.await.unwrap();
    assert_eq!(
        crate::persistence::decode(&storage.local.lock().unwrap())
            .unwrap()
            .drafts,
        snapshot.drafts
    );
}

#[tokio::test]
async fn autosave_observes_core_changes_without_ui_notifications() {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        let storage = Arc::new(Storage::default());
        let persistence = session(storage.clone(), Arc::default());
        let restored = persistence.restore(false, None).await.unwrap();
        let store = crate::store::Store::offline(restored);
        persistence.start(&store.snapshot());
        let stop = CancellationToken::new();
        let observe = tokio::spawn(persistence.clone().observe(store.subscribe(), stop.clone()));
        store
            .dispatch(Intent::SetDraftText {
                thread_id: "draft".into(),
                text: "no UI save".into(),
            })
            .await
            .unwrap();
        loop {
            storage.saved.notified().await;
            let snapshot = crate::persistence::decode(&storage.local.lock().unwrap()).unwrap();
            if snapshot
                .drafts
                .get(&"draft".into())
                .is_some_and(|draft| draft.text == "no UI save")
            {
                break;
            }
        }
        stop.cancel();
        observe.await.unwrap();
        store.close().await.unwrap();
        persistence.finish(store.snapshot()).unwrap().await.unwrap();
    })
    .await
    .expect("autosave stalled");
}

#[test]
fn model_replacement_resets_its_options_and_inheritance_removes_only_its_scope() {
    use agent_protocol::models::ModelRef;
    let scope = ModelDefaultsScope::Environment { id: "host".into() };
    let mut before = Snapshot::default();
    for provider in [ProviderKind::Codex, ProviderKind::Claude] {
        before = edit(
            &before,
            Intent::SelectDefaultModel {
                scope: scope.clone(),
                provider,
                model: Some(ModelRef {
                    provider,
                    id: "old".into(),
                }),
            },
        );
        before = edit(
            &before,
            Intent::SelectDefaultEffort {
                scope: scope.clone(),
                provider,
                effort: Some("high".into()),
            },
        );
    }
    let mut current = before.clone();
    current.preferred_editor = Some(EditorId::Zed);
    current = edit(
        &current,
        Intent::SelectDefaultServiceTier {
            scope: ModelDefaultsScope::Global,
            provider: ProviderKind::Claude,
            service_tier: Some("fast".into()),
        },
    );
    let after = edit(
        &before,
        Intent::SelectDefaultModel {
            scope: scope.clone(),
            provider: ProviderKind::Codex,
            model: Some(ModelRef {
                provider: ProviderKind::Codex,
                id: "new".into(),
            }),
        },
    );
    let mut merged = current.clone();
    ModelPreferences::capture(&current)
        .merge(
            &ModelPreferences::capture(&before),
            &ModelPreferences::capture(&after),
        )
        .apply(&mut merged);
    let scoped = &merged.scoped_model_defaults[&scope];
    assert_eq!(
        scoped.providers[&ProviderKind::Codex]
            .model
            .as_ref()
            .unwrap()
            .id,
        "new"
    );
    assert!(scoped.providers[&ProviderKind::Codex].effort.is_none());
    assert_eq!(
        scoped.providers[&ProviderKind::Claude].effort.as_deref(),
        Some("high")
    );
    assert_eq!(merged.model_defaults, current.model_defaults);
    let inherited = edit(
        &after,
        Intent::InheritModelDefaults {
            scope: scope.clone(),
        },
    );
    ModelPreferences::capture(&merged)
        .merge(
            &ModelPreferences::capture(&after),
            &ModelPreferences::capture(&inherited),
        )
        .apply(&mut merged);
    assert!(!merged.scoped_model_defaults.contains_key(&scope));
    assert_eq!(merged.model_defaults, current.model_defaults);
}

proptest::proptest! {
    #[test]
    fn independent_option_edits_commute(
        original_effort in proptest::option::of("[a-z]{1,8}"),
        original_tier in proptest::option::of("[a-z]{1,8}"),
        effort in proptest::option::of("[a-z]{1,8}"),
        tier in proptest::option::of("[a-z]{1,8}"),
    ) {
        let mut before = Snapshot::default();
        before = edit(&before, Intent::SelectDefaultEffort { scope: ModelDefaultsScope::Global,
            provider: ProviderKind::Codex, effort: original_effort.clone() });
        before = edit(&before, Intent::SelectDefaultServiceTier { scope: ModelDefaultsScope::Global,
            provider: ProviderKind::Codex, service_tier: original_tier.clone() });
        let first = edit(&before, Intent::SelectDefaultEffort { scope: ModelDefaultsScope::Global,
            provider: ProviderKind::Codex, effort: effort.clone() });
        let second = edit(&before, Intent::SelectDefaultServiceTier { scope: ModelDefaultsScope::Global,
            provider: ProviderKind::Codex, service_tier: tier.clone() });
        let baseline = ModelPreferences::capture(&before);
        let first = ModelPreferences::capture(&first);
        let second = ModelPreferences::capture(&second);
        let forward = first.merge(&baseline, &second);
        let backward = second.merge(&baseline, &first);
        proptest::prop_assert_eq!(&forward, &backward);
        let mut restored = Snapshot::default();
        forward.apply(&mut restored);
        let actual = restored.model_defaults.providers.get(&ProviderKind::Codex).cloned().unwrap_or_default();
        proptest::prop_assert_eq!(actual.effort, effort);
        proptest::prop_assert_eq!(actual.service_tier, tier);
    }
}
