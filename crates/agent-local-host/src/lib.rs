//! Files the Host and its local clients share on one machine: private state
//! files, the local client identity, discovery of the running Host and the
//! application bundle layout.
mod files;
mod registry;
pub mod update;

pub use files::{create_state_directory, private_file_options, save_private_json};
pub use registry::{HostLease, LocalHost, LocalHostRegistry, LocalHostState};

use agent_transport::transport::Identity;
use anyhow::{Result, ensure};
use std::path::{Path, PathBuf};

/// The Host provisions both identity keys here; local clients read only theirs.
pub fn identity_path(directory: &Path) -> PathBuf {
    directory.join("identity.keys")
}

/// Read only the local identity already provisioned by the Host.
/// File access must run on a blocking worker.
pub fn load_local_identity(directory: &Path) -> Result<Identity> {
    let bytes = match std::fs::read(identity_path(directory)) {
        Ok(bytes) => zeroize::Zeroizing::new(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            anyhow::bail!("local Host credentials are not provisioned")
        }
        Err(error) => return Err(error.into()),
    };
    ensure!(
        bytes.len() == 64,
        "saved Host keys must contain exactly 64 bytes"
    );
    Ok(Identity::from_bytes(bytes[32..].try_into().unwrap()))
}

/// The Mac runtime has its own background-only identity instead of inheriting
/// the desktop app's foreground identity when npm or an MCP tool uses it.
pub fn bundled_node_path(executable: &Path) -> Option<PathBuf> {
    let directory = executable.parent()?;
    directory.ends_with("Contents/MacOS").then(|| {
        directory
            .parent()
            .expect("Contents exists")
            .join("Helpers/Bex Node.app/Contents/MacOS/node")
    })
}
