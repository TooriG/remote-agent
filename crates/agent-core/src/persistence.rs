//! Device storage is an explicit subset of runtime state. Host results and
//! connection authority are fetched again, never restored from this format.
//!
//! Desktop, Swift and Kotlin use this codec; Snapshot inspection JSON is not a
//! storage format. Preserve user-owned drafts, attachments, pending sends, edits,
//! preferences and navigation across Host storage scopes. Reject damaged required
//! fields rather than silently replacing user work with empty state; ignore
//! unrelated runtime fields. Empty bytes represent a new installation.
//! Native clients own private atomic writes and lifecycle flushes. Restoration
//! marks unconfirmed sends as delivery-unknown until Host evidence reconciles
//! them, preserving their content and order without automatically resending.
use crate::state::{
    Draft, FileDraft, ModelDefaults, ModelDefaultsScope, Navigation, PendingSubmission, ScopedData,
    Snapshot,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

/// One device's editor selection, shared across Host snapshots. Discovery is never persisted.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EditorPreferences {
    pub preferred: Option<agent_protocol::editors::EditorId>,
}

impl EditorPreferences {
    pub fn capture(snapshot: &Snapshot) -> Self {
        Self {
            preferred: snapshot.preferred_editor,
        }
    }
}

#[cfg(test)]
mod editor_tests {
    use super::*;
    use agent_protocol::editors::EditorId;

    #[test]
    fn editor_preferences_are_device_owned_and_never_serialize_host_discovery() {
        let snapshot = Snapshot {
            preferred_editor: Some(EditorId::Cursor),
            file_open_capabilities: Some(Arc::new(agent_protocol::editors::FileOpenCapabilities {
                editors: vec![EditorId::Cursor, EditorId::FileManager],
                reveal: None,
            })),
            ..Default::default()
        };
        let bytes = serde_json::to_vec(&EditorPreferences::capture(&snapshot)).unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
            serde_json::json!({"preferred":"cursor"})
        );
        assert_eq!(
            serde_json::from_slice::<EditorPreferences>(&bytes)
                .unwrap()
                .preferred,
            Some(EditorId::Cursor)
        );
        assert!(
            decode(&encode(&snapshot).unwrap())
                .unwrap()
                .file_open_capabilities
                .is_none()
        );
    }
}

/// Required user-owned fields prevent a damaged document from silently becoming
/// an empty draft. Runtime Snapshot fields cannot change the storage contract.
#[derive(Serialize, Deserialize)]
pub struct PersistedState {
    model_defaults: ModelDefaults,
    #[serde(with = "entries")]
    scoped_model_defaults: Arc<BTreeMap<ModelDefaultsScope, ModelDefaults>>,
    #[serde(default)]
    project_preferences: Arc<BTreeMap<String, BTreeMap<String, crate::state::ProjectPreferences>>>,
    storage_scope: String,
    archived_scopes: Arc<BTreeMap<String, Arc<ScopedData>>>,
    #[serde(with = "entries")]
    drafts: Arc<BTreeMap<crate::state::DraftKey, Arc<Draft>>>,
    pending_submissions: Arc<BTreeMap<agent_protocol::ids::ClientInputId, Arc<PendingSubmission>>>,
    file_drafts: Arc<BTreeMap<String, Arc<FileDraft>>>,
    navigation: Arc<Navigation>,
}

impl PersistedState {
    pub fn capture(snapshot: &Snapshot) -> Self {
        Self {
            model_defaults: snapshot.model_defaults.clone(),
            scoped_model_defaults: snapshot.scoped_model_defaults.clone(),
            project_preferences: snapshot.project_preferences.clone(),
            storage_scope: snapshot.storage_scope.clone(),
            archived_scopes: snapshot.archived_scopes.clone(),
            drafts: snapshot.drafts.clone(),
            pending_submissions: snapshot.pending_submissions.clone(),
            file_drafts: snapshot.file_drafts.clone(),
            navigation: snapshot.navigation.clone(),
        }
    }
}

pub fn encode(snapshot: &Snapshot) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(&PersistedState::capture(snapshot))
}

/// Device-owned presets shared by every saved Host snapshot.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelPreferences {
    #[serde(flatten)]
    defaults: ModelDefaults,
    #[serde(with = "entries")]
    scoped: Arc<BTreeMap<ModelDefaultsScope, ModelDefaults>>,
}
impl ModelPreferences {
    pub fn capture(snapshot: &Snapshot) -> Self {
        Self {
            defaults: snapshot.model_defaults.clone(),
            scoped: snapshot.scoped_model_defaults.clone(),
        }
    }

    pub(crate) fn apply(&self, snapshot: &mut Snapshot) {
        snapshot.model_defaults = self.defaults.clone();
        snapshot.scoped_model_defaults = self.scoped.clone();
    }

    /// Apply only this session's edits to the device's current preferences.
    /// Another window's unchanged fields must never be replaced by a stale copy.
    pub(crate) fn merge(&self, before: &Self, after: &Self) -> Self {
        let mut scoped = self.scoped.clone();
        for scope in before
            .scoped
            .keys()
            .chain(after.scoped.keys())
            .collect::<BTreeSet<_>>()
        {
            let (before, after) = (before.scoped.get(scope), after.scoped.get(scope));
            if before == after {
                continue;
            }
            match after {
                Some(after) => {
                    let value = merge_defaults(
                        scoped.get(scope).unwrap_or(&ModelDefaults::default()),
                        before.unwrap_or(&ModelDefaults::default()),
                        after,
                    );
                    Arc::make_mut(&mut scoped).insert(scope.clone(), value);
                }
                None => {
                    Arc::make_mut(&mut scoped).remove(scope);
                }
            }
        }
        Self {
            defaults: merge_defaults(&self.defaults, &before.defaults, &after.defaults),
            scoped,
        }
    }
}

fn merge_defaults(
    current: &ModelDefaults,
    before: &ModelDefaults,
    after: &ModelDefaults,
) -> ModelDefaults {
    let mut merged = current.clone();
    if before.new_chat_model != after.new_chat_model {
        merged.new_chat_model = after.new_chat_model.clone();
    }
    for provider in before
        .providers
        .keys()
        .chain(after.providers.keys())
        .collect::<BTreeSet<_>>()
    {
        let empty = crate::state::ProviderModelDefaults::default();
        let before = before.providers.get(provider).unwrap_or(&empty);
        let after = after.providers.get(provider).unwrap_or(&empty);
        if before == after {
            continue;
        }
        let mut settings = merged.providers.get(provider).cloned().unwrap_or_default();
        if before.model != after.model {
            settings = after.clone();
        } else {
            if before.effort != after.effort {
                settings.effort = after.effort.clone();
            }
            if before.service_tier != after.service_tier {
                settings.service_tier = after.service_tier.clone();
            }
        }
        if settings == empty {
            merged.providers.remove(provider);
        } else {
            merged.providers.insert(*provider, settings);
        }
    }
    merged
}
pub fn decode(bytes: &[u8]) -> Result<Snapshot, serde_json::Error> {
    if bytes.is_empty() {
        return Ok(Snapshot::default());
    }
    let saved: PersistedState = serde_json::from_slice(bytes)?;
    Ok(Snapshot {
        model_defaults: saved.model_defaults,
        scoped_model_defaults: saved.scoped_model_defaults,
        project_preferences: saved.project_preferences,
        storage_scope: saved.storage_scope,
        archived_scopes: saved.archived_scopes,
        drafts: saved.drafts,
        pending_submissions: saved.pending_submissions,
        file_drafts: saved.file_drafts,
        navigation: saved.navigation,
        ..Default::default()
    })
}

/// Structured domain keys are stored as entries, without encoding keys into strings.
pub(crate) mod entries {
    use serde::{Deserialize, Serialize};
    use std::{collections::BTreeMap, sync::Arc};
    pub fn serialize<S: serde::Serializer, K: Serialize, V: Serialize>(
        values: &BTreeMap<K, V>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(values.iter())
    }
    pub fn deserialize<
        'de,
        D: serde::Deserializer<'de>,
        K: Deserialize<'de> + Ord,
        V: Deserialize<'de>,
    >(
        deserializer: D,
    ) -> Result<Arc<BTreeMap<K, V>>, D::Error> {
        let entries = Vec::<(K, V)>::deserialize(deserializer)?;
        let mut values = BTreeMap::new();
        for (key, value) in entries {
            if values.insert(key, value).is_some() {
                return Err(serde::de::Error::custom("duplicate domain key"));
            }
        }
        Ok(Arc::new(values))
    }
}
