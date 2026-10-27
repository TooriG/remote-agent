//! Import directly into the Host's browser. Clients receive profile names and usernames only.
use agent_protocol::browser::{BrowserAction, BrowserCredentials};
#[cfg(any(target_os = "macos", test))]
use zeroize::Zeroizing;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(any(target_os = "macos", test))]
pub(super) struct Cookie {
    pub(super) domain: String,
    pub(super) name: String,
    pub(super) value: Zeroizing<String>,
    pub(super) path: String,
    pub(super) secure: bool,
    pub(super) http_only: bool,
    pub(super) same_site: i64,
    pub(super) expires: Option<i64>,
}
#[cfg(any(target_os = "macos", test))]
impl Cookie {
    pub(super) fn params(&self) -> serde_json::Value {
        let mut value = serde_json::json!({"domain":self.domain,"name":self.name,
            "value":self.value.as_str(),"path":self.path,"secure":self.secure,"httpOnly":self.http_only});
        if let Some(expires) = self.expires {
            value["expires"] = expires.into();
        }
        if let Some(policy) = match self.same_site {
            0 => Some("None"),
            1 => Some("Lax"),
            2 => Some("Strict"),
            _ => None,
        } {
            value["sameSite"] = policy.into();
        }
        value
    }
}

pub(super) enum Prepared {
    Metadata(BrowserCredentials),
    #[cfg(any(target_os = "macos", test))]
    Cookies {
        cookies: Vec<Cookie>,
        skipped: usize,
    },
    #[cfg(target_os = "macos")]
    Notice(String),
    #[cfg(target_os = "macos")]
    Fill {
        origin: String,
        script: Zeroizing<String>,
    },
}

pub(super) fn manages_credentials(action: &BrowserAction) -> bool {
    matches!(
        action,
        BrowserAction::Credentials
            | BrowserAction::ImportCookies { .. }
            | BrowserAction::ImportPasswords { .. }
            | BrowserAction::FillPassword { .. }
            | BrowserAction::DeletePasswords { .. }
    )
}

pub(super) async fn prepare(
    action: BrowserAction,
    origin: Option<String>,
) -> Result<Prepared, String> {
    tokio::task::spawn_blocking(move || {
        #[cfg(target_os = "macos")]
        {
            macos::prepare(action, origin)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = origin;
            if matches!(action, BrowserAction::Credentials) {
                Ok(Prepared::Metadata(BrowserCredentials {
                    profiles: Vec::new(),
                    origin: None,
                    accounts: Vec::new(),
                    password_error: None,
                }))
            } else {
                Err(
                    "ログイン状態と保存パスワードの取り込みは、現在MacのHostで利用できます。"
                        .into(),
                )
            }
        }
    })
    .await
    .map_err(|_| "ブラウザの取り込み処理が中断されました。".to_owned())?
}
