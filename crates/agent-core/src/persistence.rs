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
    Draft, FileDraft, ModelPreferences, Navigation, PendingSubmission, ScopedData, Snapshot,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};

/// One device's editor selection, shared across Host snapshots. Discovery is never persisted.
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
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
    #[serde(default)]
    model_preferences: ModelPreferences,
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
            model_preferences: snapshot.model_preferences.clone(),
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

pub fn encode_model_preferences(snapshot: &Snapshot) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(&snapshot.model_preferences)
}

pub fn apply_model_preferences(
    persisted: &[u8],
    preferences: &[u8],
) -> Result<Vec<u8>, serde_json::Error> {
    let mut saved = if persisted.is_empty() {
        PersistedState::capture(&Snapshot::default())
    } else {
        serde_json::from_slice::<PersistedState>(persisted)?
    };
    let preferences: ModelPreferences = if preferences.is_empty() {
        ModelPreferences::default()
    } else {
        serde_json::from_slice(preferences)?
    };
    saved.model_preferences = preferences;
    serde_json::to_vec(&saved)
}

pub fn decode(bytes: &[u8]) -> Result<Snapshot, serde_json::Error> {
    if bytes.is_empty() {
        return Ok(Snapshot::default());
    }
    let saved: PersistedState = serde_json::from_slice(bytes)?;
    Ok(Snapshot {
        model_preferences: saved.model_preferences,
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
