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

#[cfg(not(target_os = "macos"))]
#[derive(Default)]
struct ActionOutput {
    page_text: String,
    value: String,
    credentials: Option<agent_protocol::browser::BrowserCredentials>,
}
