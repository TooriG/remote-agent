//! Only OS storage. Core owns restoration, persistence scheduling and recovery.
use agent_core::client::{
    AgentError,
    persistence::{ClientStorage, StorageKey},
    session::ClientPlatform,
};
use serde::{Deserialize, Serialize};
use std::{io, path::PathBuf};

pub(crate) fn error(reason: impl std::fmt::Display) -> AgentError {
    AgentError::Failed {
        reason: reason.to_string(),
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Host {
    pub id: String,
    pub name: String,
    pub ticket: String,
    pub local_directory: Option<PathBuf>,
}

pub(crate) struct Storage {
    pub root: PathBuf,
    pub host: Host,
}

impl Storage {
    fn host_directory(&self) -> PathBuf {
        self.root.join("hosts").join(
            uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, self.host.id.as_bytes()).to_string(),
        )
    }
    fn path(&self, key: StorageKey) -> PathBuf {
        match key {
            StorageKey::LocalState => self.host_directory().join("state.json"),
            StorageKey::ModelPreferences => self.root.join("model-preferences.json"),
            StorageKey::EditorPreferences => self.root.join("editor-preferences.json"),
        }
    }
}

pub(crate) fn read(path: &std::path::Path) -> Result<Vec<u8>, AgentError> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(bytes),
        Err(reason) if reason.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(reason) => Err(error(reason)),
    }
}

impl ClientStorage for Storage {
    fn load(&self, key: StorageKey) -> Result<Vec<u8>, AgentError> {
        read(&self.path(key))
    }
    fn save(&self, key: StorageKey, bytes: Vec<u8>) -> Result<(), AgentError> {
        agent_local_host::save_private_bytes(&self.path(key), &bytes).map_err(error)
    }
}

impl ClientPlatform for Storage {
    fn identity(&self) -> Result<Vec<u8>, AgentError> {
        if let Some(directory) = &self.host.local_directory {
            return agent_local_host::load_local_identity(directory)
                .map(|identity| identity.to_bytes().to_vec())
                .map_err(error);
        }
        identity(&self.host.id)
    }
    fn diagnostics_directory(&self) -> Result<Option<String>, AgentError> {
        Ok(Some(
            self.host_directory()
                .join("diagnostics")
                .to_string_lossy()
                .into_owned(),
        ))
    }
    fn conversation_directory(&self) -> Result<Option<String>, AgentError> {
        Ok(Some(
            self.host_directory()
                .join("conversations")
                .to_string_lossy()
                .into_owned(),
        ))
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn identity(host: &str) -> Result<Vec<u8>, AgentError> {
    use security_framework::passwords::{PasswordOptions, generic_password, set_generic_password};
    const SERVICE: &str = "com.bex.nsh-preview.identity";
    match generic_password(PasswordOptions::new_generic_password(SERVICE, host)) {
        Ok(bytes) => Ok(bytes),
        Err(reason) if reason.code() == -25300 => {
            let bytes = agent_core::client::native::generate_identity();
            set_generic_password(SERVICE, host, &bytes).map_err(error)?;
            Ok(bytes)
        }
        Err(reason) => Err(error(reason)),
    }
}

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
fn identity(_: &str) -> Result<Vec<u8>, AgentError> {
    Err(error("This client requires Apple Keychain."))
}

pub(crate) fn directory() -> Result<PathBuf, AgentError> {
    if let Some(directory) = std::env::var_os("AGENT_STATE_DIR") {
        return Ok(PathBuf::from(directory).join("nsh-client"));
    }
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        use objc2_foundation::{
            NSSearchPathDirectory, NSSearchPathDomainMask, NSSearchPathForDirectoriesInDomains,
        };
        let paths = NSSearchPathForDirectoriesInDomains(
            NSSearchPathDirectory::ApplicationSupportDirectory,
            NSSearchPathDomainMask::UserDomainMask,
            true,
        );
        let path = paths
            .firstObject()
            .ok_or_else(|| error("Application Support directory is unavailable"))?;
        Ok(PathBuf::from(path.to_string()).join("com.bex.nsh-preview"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    {
        Err(error(
            "This client requires an Apple application data directory.",
        ))
    }
}

pub(crate) fn local_host() -> Result<Option<Host>, AgentError> {
    #[cfg(target_os = "macos")]
    {
        use agent_local_host::{LocalHostRegistry, LocalHostState};
        let registry = if std::env::var_os("AGENT_ISOLATED_HOST").as_deref()
            == Some(std::ffi::OsStr::new("1"))
        {
            LocalHostRegistry::new(PathBuf::from(
                std::env::var_os("AGENT_STATE_DIR")
                    .ok_or_else(|| error("AGENT_STATE_DIR is required for an isolated Host"))?,
            ))
        } else {
            LocalHostRegistry::for_user().map_err(error)?
        };
        let local = registry.resolve(registry.directory()).map_err(error)?;
        if let LocalHostState::Ready(ticket) = local.state {
            return Ok(Some(Host {
                id: "local".into(),
                name: "This PC".into(),
                ticket: ticket.to_string(),
                local_directory: Some(local.directory),
            }));
        }
    }
    Ok(None)
}
