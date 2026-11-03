//! A private renderer address beside the Host state. The lease prevents a
//! second desktop from replacing the browser used by already connected clients.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
pub struct Renderer {
    pub id: String,
    pub socket: PathBuf,
}

pub struct RendererLease {
    state: PathBuf,
    pub renderer: Renderer,
    _directory: tempfile::TempDir,
    _lock: File,
}

impl RendererLease {
    pub fn state_directory(&self) -> &Path {
        &self.state
    }
    pub fn acquire(state: &Path) -> Result<Self> {
        crate::create_state_directory(state)?;
        let lock = crate::files::open_lock(&state.join("browser-renderer.lock"))?;
        lock.try_lock()
            .context("Bexの内蔵ブラウザは別のウィンドウで起動しています。")?;
        let directory = tempfile::Builder::new()
            .prefix("bex-renderer-")
            .tempdir_in("/tmp")?;
        crate::create_state_directory(directory.path())?;
        Ok(Self {
            state: state.to_owned(),
            renderer: Renderer {
                id: uuid::Uuid::new_v4().to_string(),
                socket: directory.path().join("browser.sock"),
            },
            _directory: directory,
            _lock: lock,
        })
    }

    pub fn publish(&self) -> Result<()> {
        crate::save_private_json(&self.state.join("browser-renderer.json"), &self.renderer)
    }
}

impl Drop for RendererLease {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.state.join("browser-renderer.json"));
        let _ = self._lock.unlock();
    }
}

pub fn read_renderer(state: &Path) -> Result<Renderer> {
    let renderer: Renderer =
        serde_json::from_slice(&std::fs::read(state.join("browser-renderer.json"))?)?;
    ensure!(
        !renderer.id.is_empty() && renderer.id.len() <= 256 && renderer.socket.is_absolute(),
        "invalid browser registration"
    );
    Ok(renderer)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn one_renderer_owns_registration_until_it_stops() {
        let state = tempfile::tempdir().unwrap();
        let first = RendererLease::acquire(state.path()).unwrap();
        first.publish().unwrap();
        assert_eq!(read_renderer(state.path()).unwrap().id, first.renderer.id);
        assert!(RendererLease::acquire(state.path()).is_err());
        drop(first);
        assert!(read_renderer(state.path()).is_err());
        let next = RendererLease::acquire(state.path()).unwrap();
        next.publish().unwrap();
        assert_eq!(read_renderer(state.path()).unwrap().id, next.renderer.id);
    }
}
