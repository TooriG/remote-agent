//! The Host's private connection to the native browser in its desktop app.
//! Credentials travel only over local IPC; this contract is not an agent tool.
use crate::browser::{BrowserAction, BrowserFrame};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

#[derive(Clone, Serialize, Deserialize)]
pub struct BrowserCookie {
    pub domain: String,
    pub name: String,
    pub value: Zeroizing<String>,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    pub same_site: i64,
    pub expires: Option<i64>,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub enum Capture {
    None,
    Viewer,
    Snapshot,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct DisplayedPage {
    pub tab_id: String,
    pub generation: u64,
}

#[derive(Clone, Serialize, Deserialize)]
pub enum NativeAction {
    Browser(BrowserAction),
    ImportCookies(Vec<BrowserCookie>),
    FillPassword {
        origin: String,
        script: Zeroizing<String>,
    },
}

#[derive(Clone, Serialize, Deserialize)]
pub struct NativeRequest {
    pub scope: String,
    pub displayed: Option<DisplayedPage>,
    pub image_id: String,
    pub renderer_id: String,
    pub capture: Capture,
    pub action: NativeAction,
}

pub type NativeReply = Result<BrowserFrame, String>;

impl NativeRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.scope.is_empty()
            || self.scope.len() > 8192
            || self.image_id.len() > 256
            || self.renderer_id.len() > 256
            || self
                .displayed
                .as_ref()
                .is_some_and(|p| p.tab_id.len() > 256)
        {
            return Err("ブラウザ接続情報が不正です。".into());
        }
        match &self.action {
            NativeAction::Browser(action) => action.validate(),
            NativeAction::ImportCookies(cookies) if cookies.len() <= 100_000 => Ok(()),
            NativeAction::FillPassword { origin, script }
                if script.len() <= 1024 * 1024
                    && crate::browser_import::password_origin(origin).as_deref() == Ok(origin) =>
            {
                Ok(())
            }
            _ => Err("ブラウザ操作の値が範囲外です。".into()),
        }
    }
}
