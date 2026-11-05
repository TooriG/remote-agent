//! Host authorization stays here; the Mac desktop owns its embedded WebKit views.
#[cfg(not(target_os = "macos"))]
mod cdp;
#[cfg(not(target_os = "macos"))]
mod chrome;
mod credentials;
pub mod mcp;
#[cfg(target_os = "macos")]
mod native;
#[cfg(not(target_os = "macos"))]
pub use chrome::Browser;
#[cfg(target_os = "macos")]
pub use native::Browser;

impl Drop for Browser {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl Browser {
    pub fn provider_config(&self, thread: &str) -> Result<serde_json::Value, String> {
        Ok(serde_json::json!({
            "command": std::env::current_exe().map_err(|error| error.to_string())?,
            "args": ["browser-mcp", "--socket", self.socket(), "--thread", thread],
        }))
    }

    pub(super) fn socket(&self) -> std::path::PathBuf {
        self.bridge_directory.path().join("bridge.sock")
    }
}

fn validate_agent_action(action: &agent_protocol::browser::BrowserAction) -> Result<(), String> {
    action.validate()?;
    if credentials::manages_credentials(action) {
        return Err(
            "ログイン状態と保存パスワードは、ブラウザのメニューから取り込んでください。".into(),
        );
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
#[derive(Default)]
struct ActionOutput {
    page_text: String,
    value: String,
    credentials: Option<agent_protocol::browser::BrowserCredentials>,
}
