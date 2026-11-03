use super::*;
use crate::state::{Draft, Intent, ModelSelection};
use agent_protocol::{editors::EditorId, models::ModelRef, session::ProviderKind};
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
fn selection(
    provider: ProviderKind,
    id: &str,
    effort: Option<String>,
    tier: Option<String>,
) -> ModelSelection {
    ModelSelection {
        model: ModelRef {
            provider,
            id: id.into(),
        },
        effort,
        service_tier: tier,
    }
}
fn remember(snapshot: &mut Snapshot, selection: ModelSelection) {
    let provider = selection.model.provider;
    snapshot.model_preferences.provider = Some(provider);
    snapshot
        .model_preferences
        .selections
        .insert(provider, selection);
}
fn selected(snapshot: &Snapshot) -> &ModelSelection {
    &snapshot.model_preferences.selections[&ProviderKind::Codex]
}
fn session(storage: Arc<Storage>, device: Arc<DevicePreferences>) -> Arc<Persistence> {
    Persistence::new(storage, device)
}
fn with_disk(disk: Arc<DeviceDisk>) -> Arc<Storage> {
    Arc::new(Storage {
        device: disk,
        ..Default::default()
    })
}

#[tokio::test]
async fn device_preferences_apply_across_hosts_without_replacing_their_work() {
    let mut defaults = Snapshot::default();
    remember(
        &mut defaults,
        selection(
            ProviderKind::Codex,
            "chosen",
            Some("high".into()),
            Some("priority".into()),
        ),
    );
    defaults.preferred_editor = Some(EditorId::Zed);
    let disk = Arc::new(DeviceDisk::default());
    *disk.models.lock().unwrap() = serde_json::to_vec(&defaults.model_preferences).unwrap();
    *disk.editor.lock().unwrap() =
        serde_json::to_vec(&EditorPreferences::capture(&defaults)).unwrap();
    let device = Arc::new(DevicePreferences::default());
    for host in ["first", "second"] {
        let storage = with_disk(disk.clone());
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
        assert_eq!(restored.model_preferences, defaults.model_preferences);
        assert_eq!(restored.preferred_editor, Some(EditorId::Zed));
    }
    assert_eq!(disk.writes.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn local_choices_initialize_device_preferences_without_losing_existing_work() {
    let storage = Arc::new(Storage::default());
    let mut saved = Snapshot::default();
    remember(
        &mut saved,
        selection(ProviderKind::Codex, "chosen", Some("high".into()), None),
    );
    remember(
        &mut saved,
        selection(ProviderKind::Claude, "other", None, Some("fast".into())),
    );
    draft(&mut saved, "existing Android draft");
    *storage.local.lock().unwrap() = crate::persistence::encode(&saved).unwrap();
    let persistence = session(storage.clone(), Arc::default());
    let restored = persistence.restore(false, None).await.unwrap();
    assert_eq!(restored.model_preferences, saved.model_preferences);
    assert_eq!(restored.drafts, saved.drafts);
    persistence.start(&restored);
    persistence
        .finish(Arc::new(restored))
        .unwrap()
        .await
        .unwrap();
    let other = session(with_disk(storage.device.clone()), Arc::default())
        .restore(false, None)
        .await
        .unwrap();
    assert_eq!(other.model_preferences, saved.model_preferences);
    assert!(other.drafts.is_empty());
}

#[tokio::test]
async fn simultaneous_views_keep_each_providers_choice_when_closed_in_reverse_order() {
    let disk = Arc::new(DeviceDisk::default());
    let device = Arc::new(DevicePreferences::default());
    let main_storage = with_disk(disk.clone());
    let side_storage = with_disk(disk.clone());
    let main = session(main_storage.clone(), device.clone());
    let side = session(side_storage.clone(), device);
    let mut first = main.restore(false, None).await.unwrap();
    let mut second = side.restore(false, None).await.unwrap();
    main.start(&first);
    side.start(&second);
    let codex = selection(ProviderKind::Codex, "preferred", Some("high".into()), None);
    let claude = selection(ProviderKind::Claude, "other", None, Some("fast".into()));
    remember(&mut first, codex.clone());
    remember(&mut second, claude.clone());
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
    let restored = session(with_disk(disk), Arc::default())
        .restore(false, None)
        .await
        .unwrap();
    assert_eq!(
        restored.model_preferences.provider,
        Some(ProviderKind::Claude)
    );
    assert_eq!(
        restored.model_preferences.selections[&ProviderKind::Codex],
        codex
    );
    assert_eq!(
        restored.model_preferences.selections[&ProviderKind::Claude],
        claude
    );
    let main = crate::persistence::decode(&main_storage.local.lock().unwrap()).unwrap();
    let side = crate::persistence::decode(&side_storage.local.lock().unwrap()).unwrap();
    assert_eq!(main.drafts[&"draft".into()].text, "main draft");
    assert_eq!(side.drafts[&"draft".into()].text, "side draft");
    assert!(restored.drafts.is_empty());
}

#[tokio::test]
async fn simultaneous_option_edits_do_not_overwrite_each_other() {
    let disk = Arc::new(DeviceDisk::default());
    let device = Arc::new(DevicePreferences::default());
    let mut initial = Snapshot::default();
    remember(
        &mut initial,
        selection(ProviderKind::Codex, "chosen", None, None),
    );
    *disk.models.lock().unwrap() = serde_json::to_vec(&initial.model_preferences).unwrap();
    let main = session(with_disk(disk.clone()), device.clone());
    let side = session(with_disk(disk.clone()), device);
    let mut first = main.restore(false, None).await.unwrap();
    let mut second = side.restore(false, None).await.unwrap();
    main.start(&first);
    side.start(&second);
    first
        .model_preferences
        .selections
        .get_mut(&ProviderKind::Codex)
        .unwrap()
        .effort = Some("high".into());
    second
        .model_preferences
        .selections
        .get_mut(&ProviderKind::Codex)
        .unwrap()
        .service_tier = Some("priority".into());
    main.enqueue(Arc::new(first.clone()))
        .unwrap()
        .await
        .unwrap();
    side.finish(Arc::new(second)).unwrap().await.unwrap();
    main.finish(Arc::new(first)).unwrap().await.unwrap();
    let restored = session(with_disk(disk), Arc::default())
        .restore(false, None)
        .await
        .unwrap();
    assert_eq!(selected(&restored).effort.as_deref(), Some("high"));
    assert_eq!(
        selected(&restored).service_tier.as_deref(),
        Some("priority")
    );
}

#[tokio::test]
async fn retrying_a_failed_local_save_does_not_reapply_obsolete_device_edits() {
    let disk = Arc::new(DeviceDisk::default());
    let device = Arc::new(DevicePreferences::default());
    let storage = with_disk(disk.clone());
    let first = session(storage.clone(), device.clone());
    let second = session(with_disk(disk), device);
    let mut high = first.restore(false, None).await.unwrap();
    let mut low = second.restore(false, None).await.unwrap();
    first.start(&high);
    second.start(&low);
    remember(
        &mut high,
        selection(ProviderKind::Codex, "chosen", Some("high".into()), None),
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
    remember(
        &mut low,
        selection(ProviderKind::Codex, "chosen", Some("low".into()), None),
    );
    second.finish(Arc::new(low)).unwrap().await.unwrap();
    storage.fail_local.store(false, Ordering::SeqCst);
    first.finish(Arc::new(high)).unwrap().await.unwrap();
    assert!(first.failure().is_none());
    let restored = session(storage, Arc::default())
        .restore(false, None)
        .await
        .unwrap();
    assert_eq!(selected(&restored).effort.as_deref(), Some("low"));
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
    let first = session(with_disk(disk.clone()), device.clone());
    let second = session(with_disk(disk.clone()), device);
    let mut high = first.restore(false, None).await.unwrap();
    let mut other = second.restore(false, None).await.unwrap();
    first.start(&high);
    second.start(&other);
    remember(
        &mut high,
        selection(ProviderKind::Codex, "chosen", Some("high".into()), None),
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
    other.preferred_editor = Some(EditorId::Zed);
    second.finish(Arc::new(other)).unwrap().await.unwrap();
    first.finish(Arc::new(high)).unwrap().await.unwrap();
    let restored = session(with_disk(disk), Arc::default())
        .restore(false, None)
        .await
        .unwrap();
    assert_eq!(selected(&restored).effort.as_deref(), Some("high"));
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
fn model_replacement_and_removal_keep_other_providers_choices() {
    let mut before = Snapshot::default();
    remember(
        &mut before,
        selection(ProviderKind::Claude, "other", Some("medium".into()), None),
    );
    remember(
        &mut before,
        selection(
            ProviderKind::Codex,
            "old",
            Some("high".into()),
            Some("priority".into()),
        ),
    );
    let mut current = before.model_preferences.clone();
    current
        .selections
        .get_mut(&ProviderKind::Claude)
        .unwrap()
        .service_tier = Some("fast".into());
    let mut after = before.model_preferences.clone();
    after.selections.insert(
        ProviderKind::Codex,
        selection(ProviderKind::Codex, "new", None, None),
    );
    let merged = merge_model_preferences(&current, &before.model_preferences, &after);
    assert_eq!(
        merged.selections[&ProviderKind::Codex],
        after.selections[&ProviderKind::Codex]
    );
    assert_eq!(
        merged.selections[&ProviderKind::Claude],
        current.selections[&ProviderKind::Claude]
    );
    let before = after.clone();
    after.selections.remove(&ProviderKind::Codex);
    after.provider = None;
    let inherited = merge_model_preferences(&merged, &before, &after);
    assert!(!inherited.selections.contains_key(&ProviderKind::Codex));
    assert_eq!(inherited.provider, None);
    assert_eq!(
        inherited.selections[&ProviderKind::Claude],
        current.selections[&ProviderKind::Claude]
    );
}

#[test]
fn stale_model_options_never_attach_to_a_different_models_selection() {
    let before = ModelPreferences {
        provider: Some(ProviderKind::Codex),
        selections: [(
            ProviderKind::Codex,
            selection(ProviderKind::Codex, "old", None, None),
        )]
        .into(),
    };
    let mut current = before.clone();
    current.selections.insert(
        ProviderKind::Codex,
        selection(ProviderKind::Codex, "new", None, Some("priority".into())),
    );
    let mut after = before.clone();
    after
        .selections
        .get_mut(&ProviderKind::Codex)
        .unwrap()
        .effort = Some("high".into());
    let merged = merge_model_preferences(&current, &before, &after);
    assert_eq!(
        merged.selections[&ProviderKind::Codex],
        after.selections[&ProviderKind::Codex]
    );
}

proptest::proptest! {
    #[test]
    fn independent_option_edits_commute(
        original_effort in proptest::option::of("[a-z]{1,8}"),
        original_tier in proptest::option::of("[a-z]{1,8}"),
        effort in proptest::option::of("[a-z]{1,8}"),
        tier in proptest::option::of("[a-z]{1,8}"),
    ) {
        let baseline = ModelPreferences { provider: Some(ProviderKind::Codex), selections: [(ProviderKind::Codex,
            selection(ProviderKind::Codex, "chosen", original_effort, original_tier))].into() };
        let mut first = baseline.clone();
        first.selections.get_mut(&ProviderKind::Codex).unwrap().effort = effort.clone();
        let mut second = baseline.clone();
        second.selections.get_mut(&ProviderKind::Codex).unwrap().service_tier = tier.clone();
        let forward = merge_model_preferences(&first, &baseline, &second);
        let backward = merge_model_preferences(&second, &baseline, &first);
        proptest::prop_assert_eq!(&forward, &backward);
        proptest::prop_assert_eq!(&forward.selections[&ProviderKind::Codex].effort, &effort);
        proptest::prop_assert_eq!(&forward.selections[&ProviderKind::Codex].service_tier, &tier);
    }
}
