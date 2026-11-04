//! Shared restoration and ordered persistence. Platform adapters only read and
//! atomically replace opaque payloads; device preferences have one shared owner.
use super::{AgentError, error};
use crate::{
    persistence::{EditorPreferences, merge_model_preferences},
    state::{ModelPreferences, Snapshot},
};
use futures_util::{
    FutureExt,
    future::{BoxFuture, Shared},
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tokio::sync::{mpsc, oneshot, watch};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "bindings", derive(uniffi::Enum))]
pub enum StorageKey {
    LocalState,
    ModelPreferences,
    EditorPreferences,
}

/// Runs on blocking workers. Each adapter is scoped to one Host/window; the
/// preference keys always address the device-wide locations on that platform.
#[cfg_attr(feature = "bindings", uniffi::export(rust, foreign))]
pub trait ClientStorage: Send + Sync {
    fn load(&self, key: StorageKey) -> Result<Vec<u8>, AgentError>;
    fn save(&self, key: StorageKey, bytes: Vec<u8>) -> Result<(), AgentError>;
}

#[derive(Debug, thiserror::Error)]
pub enum RestoreError {
    #[error("{0}")]
    InvalidState(serde_json::Error),
    #[error(transparent)]
    Storage(#[from] AgentError),
}

#[derive(Clone, Default)]
struct Preferences {
    models: ModelPreferences,
    editor: EditorPreferences,
}
impl Preferences {
    fn capture(snapshot: &Snapshot) -> Self {
        Self {
            models: snapshot.model_preferences.clone(),
            editor: EditorPreferences::capture(snapshot),
        }
    }
    fn apply(&self, snapshot: &mut Snapshot) {
        snapshot.model_preferences = self.models.clone();
        snapshot.preferred_editor = self.editor.preferred;
    }
}
struct DeviceState {
    current: Preferences,
    saved_models: Option<ModelPreferences>,
    saved_editor: Option<EditorPreferences>,
}

/// Share this owner between simultaneous views and successive Host sessions.
/// The last disk payload is tracked separately, so failed writes remain dirty
/// without reapplying an obsolete session's edits over another window's work.
#[derive(Default)]
pub struct DevicePreferences {
    state: Mutex<Option<DeviceState>>,
}
impl DevicePreferences {
    fn load(&self, storage: &dyn ClientStorage) -> Result<Preferences, AgentError> {
        let mut state = self.state.lock().unwrap();
        let state = Self::initialize(&mut state, storage)?;
        Ok(state.current.clone())
    }
    fn initialize<'a>(
        state: &'a mut Option<DeviceState>,
        storage: &dyn ClientStorage,
    ) -> Result<&'a mut DeviceState, AgentError> {
        if state.is_none() {
            let models = storage.load(StorageKey::ModelPreferences)?;
            let editor = storage.load(StorageKey::EditorPreferences)?;
            let saved_models: Option<ModelPreferences> = (!models.is_empty())
                .then(|| serde_json::from_slice(&models).map_err(error))
                .transpose()?;
            let saved_editor: Option<EditorPreferences> = (!editor.is_empty())
                .then(|| serde_json::from_slice(&editor).map_err(error))
                .transpose()?;
            *state = Some(DeviceState {
                current: Preferences {
                    models: saved_models.clone().unwrap_or_default(),
                    editor: saved_editor.clone().unwrap_or_default(),
                },
                saved_models,
                saved_editor,
            });
        }
        Ok(state.as_mut().unwrap())
    }
    fn save(
        &self,
        storage: &dyn ClientStorage,
        before: Preferences,
        after: Preferences,
    ) -> (Preferences, Result<(), AgentError>) {
        let mut state = self.state.lock().unwrap();
        let state = match Self::initialize(&mut state, storage) {
            Ok(state) => state,
            Err(reason) => return (before, Err(reason)),
        };
        state.current.models =
            merge_model_preferences(&state.current.models, &before.models, &after.models);
        if before.editor != after.editor {
            state.current.editor = after.editor.clone();
        }
        let result = state.persist(storage);
        (after, result)
    }
}

impl DeviceState {
    fn persist(&mut self, storage: &dyn ClientStorage) -> Result<(), AgentError> {
        if self.saved_models.as_ref() != Some(&self.current.models) {
            storage.save(
                StorageKey::ModelPreferences,
                serde_json::to_vec(&self.current.models).map_err(error)?,
            )?;
            self.saved_models = Some(self.current.models.clone());
        }
        if self.saved_editor.as_ref() != Some(&self.current.editor) {
            storage.save(
                StorageKey::EditorPreferences,
                serde_json::to_vec(&self.current.editor).map_err(error)?,
            )?;
            self.saved_editor = Some(self.current.editor.clone());
        }
        Ok(())
    }
}

pub(crate) type Completion<T> = Shared<BoxFuture<'static, Result<T, AgentError>>>;
struct Save {
    snapshot: Arc<Snapshot>,
    complete: oneshot::Sender<Result<(), AgentError>>,
}

struct Restored {
    preferences: Preferences,
    conversations: Option<PathBuf>,
}

pub struct Persistence {
    storage: Arc<dyn ClientStorage>,
    device: Arc<DevicePreferences>,
    restored: Mutex<Option<Restored>>,
    queue: Mutex<Option<mpsc::UnboundedSender<Save>>>,
    failure: watch::Sender<Option<String>>,
}
impl Persistence {
    pub fn new(storage: Arc<dyn ClientStorage>, device: Arc<DevicePreferences>) -> Arc<Self> {
        Arc::new(Self {
            storage,
            device,
            restored: Mutex::default(),
            queue: Mutex::default(),
            failure: watch::channel(None).0,
        })
    }
    pub fn failure(&self) -> Option<String> {
        self.failure.borrow().clone()
    }
    #[cfg(feature = "bindings")]
    pub(crate) fn report_failure(&self, reason: String) {
        self.failure.send_replace(Some(reason));
    }
    pub fn failures(&self) -> watch::Receiver<Option<String>> {
        self.failure.subscribe()
    }
    pub async fn restore(
        self: &Arc<Self>,
        new_pairing: bool,
        conversations: Option<PathBuf>,
    ) -> Result<Snapshot, RestoreError> {
        let persistence = self.clone();
        tokio::task::spawn_blocking(move || {
            let storage = persistence.storage.as_ref();
            let bytes = if new_pairing {
                Vec::new()
            } else {
                storage.load(StorageKey::LocalState)?
            };
            let mut snapshot =
                crate::persistence::decode(&bytes).map_err(RestoreError::InvalidState)?;
            let preferences = persistence.device.load(storage)?;
            preferences.apply(&mut snapshot);
            if let Some(directory) = &conversations {
                if new_pairing {
                    crate::conversation_cache::clear(directory);
                }
                crate::conversation_cache::restore(directory, &mut snapshot);
            }
            *persistence.restored.lock().unwrap() = Some(Restored {
                preferences,
                conversations,
            });
            Ok::<_, RestoreError>(snapshot)
        })
        .await
        .map_err(error)?
    }
    /// Prepare the ordered writer and conversation copies without entering a
    /// runtime. The caller must spawn the returned task on its own executor.
    pub fn start(
        &self,
        snapshots: watch::Receiver<Arc<Snapshot>>,
    ) -> Option<impl Future<Output = ()> + Send + use<>> {
        let mut queue = self.queue.lock().unwrap();
        if queue.is_some() {
            return None;
        }
        let (send, mut receive) = mpsc::unbounded_channel::<Save>();
        *queue = Some(send);
        let Restored {
            preferences: mut baseline,
            conversations,
        } = self
            .restored
            .lock()
            .unwrap()
            .take()
            .unwrap_or_else(|| Restored {
                preferences: Preferences::capture(&snapshots.borrow()),
                conversations: None,
            });
        let storage = self.storage.clone();
        let device = self.device.clone();
        let failure = self.failure.clone();
        let saving = async move {
            while let Some(Save { snapshot, complete }) = receive.recv().await {
                let storage = storage.clone();
                let device = device.clone();
                let saved = tokio::task::spawn_blocking(move || {
                    let (baseline, result) =
                        device.save(storage.as_ref(), baseline, Preferences::capture(&snapshot));
                    let result = result.and_then(|()| {
                        storage.save(
                            StorageKey::LocalState,
                            crate::persistence::encode(&snapshot).map_err(error)?,
                        )
                    });
                    (baseline, result)
                })
                .await;
                let result = match saved {
                    Ok((previous, result)) => {
                        baseline = previous;
                        result
                    }
                    Err(reason) => {
                        let reason = error(reason);
                        failure.send_replace(Some(reason.to_string()));
                        let _ = complete.send(Err(reason));
                        break;
                    }
                };
                failure.send_replace(result.as_ref().err().map(ToString::to_string));
                let _ = complete.send(result);
            }
        };
        Some(async move {
            let copies = async move {
                if let Some(directory) = conversations {
                    crate::conversation_cache::run(directory, snapshots).await;
                }
            };
            tokio::join!(saving, copies);
        })
    }
    pub fn enqueue(&self, snapshot: Arc<Snapshot>) -> Result<Completion<()>, AgentError> {
        self.send(snapshot, false)
    }
    /// Seal the queue before publishing the final snapshot. An autosave racing
    /// shutdown can no longer enqueue an older snapshot behind this write.
    pub fn finish(&self, snapshot: Arc<Snapshot>) -> Result<Completion<()>, AgentError> {
        self.send(snapshot, true)
    }
    fn send(
        &self,
        snapshot: Arc<Snapshot>,
        final_save: bool,
    ) -> Result<Completion<()>, AgentError> {
        let (complete, result) = oneshot::channel();
        let mut queue = self.queue.lock().unwrap();
        queue
            .as_ref()
            .ok_or_else(|| error("session is not ready"))?
            .send(Save { snapshot, complete })
            .map_err(error)?;
        if final_save {
            queue.take();
        }
        Ok(async move { result.await.map_err(error)? }.boxed().shared())
    }
    pub async fn observe(
        self: Arc<Self>,
        mut snapshots: watch::Receiver<Arc<Snapshot>>,
        stop: CancellationToken,
    ) {
        let mut previous = snapshots.borrow_and_update().clone();
        let mut dirty = true;
        loop {
            let next = tokio::select! {
                biased;
                _ = stop.cancelled() => return,
                result = snapshots.changed() => match result { Ok(()) => Some(snapshots.borrow_and_update().clone()), Err(_) => return },
                _ = tokio::time::sleep(std::time::Duration::from_millis(250)), if dirty => None,
            };
            if let Some(next) = next {
                let immediate = !next.list_unchanged(previous.clone());
                previous = next;
                dirty = true;
                if !immediate {
                    continue;
                }
            }
            if let Ok(saved) = self.enqueue(previous.clone()) {
                let _ = saved.await;
            }
            dirty = false;
        }
    }
}

#[cfg(test)]
mod tests;
