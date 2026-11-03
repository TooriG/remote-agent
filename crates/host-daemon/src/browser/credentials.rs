//! Import directly into the Host's browser. Clients receive profile names and usernames only.
use agent_protocol::browser::{BrowserAction, BrowserCredentials};
#[cfg(any(target_os = "macos", test))]
use zeroize::Zeroizing;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(any(target_os = "macos", test))]
pub(super) use agent_protocol::browser_native::BrowserCookie as Cookie;

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
