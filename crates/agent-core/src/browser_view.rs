//! Shared browser presentation and input decisions.
use crate::localization::{UiLanguage, localized_ui_format};
use agent_protocol::browser::BrowserNotice;

/// Native browser surfaces retain facts so language changes do not re-read the Host.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "bindings", derive(uniffi::Enum))]
pub enum BrowserStatus {
    Importing { remote: bool },
    Notice { notice: BrowserNotice },
    Value { value: String },
}

#[cfg_attr(feature = "bindings", uniffi::export)]
pub fn browser_status_label(language: UiLanguage, status: BrowserStatus) -> String {
    let (key, arguments) = match status {
        BrowserStatus::Importing { remote } => (
            if remote {
                "ブラウザデータを取り込み中… 接続先のMacでOSのアクセス確認が出たら許可してください。"
            } else {
                "ブラウザデータを取り込み中… OSのアクセス確認が出たら許可してください。"
            },
            vec![],
        ),
        BrowserStatus::Value { value } => return value,
        BrowserStatus::Notice { notice } => match notice {
            BrowserNotice::CookiesImported { count: 0, skipped } => (
                "取り込めるCookieがありません（期限切れなど{0}件を除外）。",
                vec![skipped.to_string()],
            ),
            BrowserNotice::CookiesImported { count, skipped } => (
                "Cookieを{0}件取り込みました（期限切れなど{1}件を除外）。ログイン状態をサイトで確認してください。",
                vec![count.to_string(), skipped.to_string()],
            ),
            BrowserNotice::PasswordsImported { count, skipped } => (
                "パスワードを{0}件Keychainに保存しました（HTTPS以外など{1}件を除外）。入力はサイトごとに選択してください。",
                vec![count.to_string(), skipped.to_string()],
            ),
            BrowserNotice::PasswordsDeleted => {
                ("このサイトの保存パスワードを削除しました。", vec![])
            }
            BrowserNotice::PasswordFilled => (
                "パスワードを入力しました。内容を確認してからログインしてください。",
                vec![],
            ),
        },
    };
    localized_ui_format(language, key.into(), arguments)
}

pub fn address_url(input: &str) -> Result<String, String> {
    let input = input.trim();
    if input.is_empty() || input.len() > 8192 {
        return Err("URL または検索語を入力してください。".into());
    }
    if input == "about:blank" || input.contains("://") {
        return agent_protocol::browser::browser_url(input);
    }
    let authority = input.split(['/', '?', '#']).next().unwrap_or_default();
    let bare_port = authority
        .rsplit_once(':')
        .is_some_and(|(_, port)| !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()));
    if authority.contains(':') && !bare_port && !authority.starts_with('[') {
        return Err("http または https の URL を入力してください。".into());
    }
    if !input.chars().any(char::is_whitespace)
        && (authority.contains('.')
            || authority == "localhost"
            || authority.starts_with('[')
            || bare_port)
    {
        return agent_protocol::browser::browser_url(input);
    }
    let mut url = url::Url::parse("https://duckduckgo.com/").unwrap();
    url.query_pairs_mut().append_pair("q", input);
    Ok(url.into())
}

/// Follow a native view's logical size without repeatedly resizing an unchanged page.
pub fn fit_action(
    width: f64,
    height: f64,
    current_width: u32,
    current_height: u32,
) -> Option<agent_protocol::browser::BrowserAction> {
    use agent_protocol::browser::{BrowserAction, MAX_HEIGHT, MAX_WIDTH, MIN_HEIGHT, MIN_WIDTH};
    if !width.is_finite() || !height.is_finite() || width <= 0. || height <= 0. {
        return None;
    }
    let width = width
        .round()
        .clamp(f64::from(MIN_WIDTH), f64::from(MAX_WIDTH)) as u32;
    let height = height
        .round()
        .clamp(f64::from(MIN_HEIGHT), f64::from(MAX_HEIGHT)) as u32;
    (width != current_width || height != current_height)
        .then_some(BrowserAction::Resize { width, height })
}

/// A pointer click or Tab explicitly chooses a page editor. Polling must not
/// reopen a keyboard the user dismissed, or steal focus from the address bar.
pub fn input_focus(
    action: &agent_protocol::browser::BrowserAction,
    input_kind: Option<agent_protocol::browser::BrowserInputKind>,
) -> Option<bool> {
    use agent_protocol::browser::{BrowserAction, BrowserKey};
    match action {
        BrowserAction::Click { .. }
        | BrowserAction::Key {
            key: BrowserKey::Tab,
        } => Some(input_kind.is_some()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_outcomes_use_current_language_and_page_values_stay_verbatim() {
        use BrowserNotice::*;
        for (notice, japanese, english) in [
            (
                CookiesImported {
                    count: 0,
                    skipped: 3,
                },
                "取り込めるCookieがありません（期限切れなど3件を除外）。",
                "No cookies to import (3 expired or otherwise excluded).",
            ),
            (
                CookiesImported {
                    count: 12,
                    skipped: 3,
                },
                "Cookieを12件取り込みました（期限切れなど3件を除外）。ログイン状態をサイトで確認してください。",
                "Imported 12 cookies (3 expired or otherwise excluded). Check your login status on the site.",
            ),
            (
                PasswordsImported {
                    count: 4,
                    skipped: 2,
                },
                "パスワードを4件Keychainに保存しました（HTTPS以外など2件を除外）。入力はサイトごとに選択してください。",
                "Saved 4 passwords in Keychain (2 non-HTTPS or otherwise excluded). Select passwords to fill for each site.",
            ),
            (
                PasswordsDeleted,
                "このサイトの保存パスワードを削除しました。",
                "Deleted saved passwords for this site.",
            ),
            (
                PasswordFilled,
                "パスワードを入力しました。内容を確認してからログインしてください。",
                "Filled the password. Check the contents before signing in.",
            ),
        ] {
            let status = BrowserStatus::Notice { notice };
            assert_eq!(
                browser_status_label(UiLanguage::Japanese, status.clone()),
                japanese
            );
            assert_eq!(browser_status_label(UiLanguage::English, status), english);
        }
        for language in [UiLanguage::Japanese, UiLanguage::English] {
            for value in ["設定", "残り 42%", "設定\n{0}"] {
                assert_eq!(
                    browser_status_label(
                        language,
                        BrowserStatus::Value {
                            value: value.into()
                        }
                    ),
                    value
                );
            }
        }
        assert_eq!(
            browser_status_label(
                UiLanguage::English,
                BrowserStatus::Importing { remote: false }
            ),
            "Importing browser data… Allow access if prompted by the OS."
        );
        assert_eq!(
            browser_status_label(
                UiLanguage::English,
                BrowserStatus::Importing { remote: true }
            ),
            "Importing browser data… Allow access if prompted on the remote Mac."
        );
    }

    #[test]
    fn only_explicit_editor_selection_changes_keyboard_focus() {
        use agent_protocol::browser::{BrowserAction, BrowserInputKind, BrowserKey};
        for (kind, opens_keyboard) in [
            (None, false),
            (Some(BrowserInputKind::Text), true),
            (Some(BrowserInputKind::Password), true),
        ] {
            for action in [
                BrowserAction::Click { x: 10., y: 20. },
                BrowserAction::Key {
                    key: BrowserKey::Tab,
                },
            ] {
                assert_eq!(input_focus(&action, kind), Some(opens_keyboard));
            }
            for action in [
                BrowserAction::Read,
                BrowserAction::Type {
                    text: "日本語".into(),
                },
                BrowserAction::Key {
                    key: BrowserKey::Enter,
                },
                BrowserAction::Key {
                    key: BrowserKey::Backspace,
                },
            ] {
                assert_eq!(input_focus(&action, kind), None);
            }
        }
    }

    #[test]
    fn follows_the_view_and_preserves_protocol_limits() {
        use agent_protocol::browser::{BrowserAction::Resize, MAX_HEIGHT, MAX_WIDTH};
        for (width, height, expected) in [
            (540.3, 852.6, (540, 853)),
            (20., 30., (320, 240)),
            (10000., 10000., (MAX_WIDTH, MAX_HEIGHT)),
        ] {
            let Some(Resize {
                width: actual_width,
                height: actual_height,
            }) = fit_action(width, height, 1280, 800)
            else {
                panic!("expected resize");
            };
            assert_eq!((actual_width, actual_height), expected);
            assert!(fit_action(width, height, actual_width, actual_height).is_none());
        }
        for invalid in [0., -1., f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(fit_action(invalid, 800., 1280, 800).is_none());
            assert!(fit_action(1280., invalid, 1280, 800).is_none());
        }
        assert!(fit_action(1280., 800., 1280, 800).is_none());
        assert!(matches!(
            fit_action(1280., 801., 1280, 800),
            Some(Resize {
                width: 1280,
                height: 801
            })
        ));
        assert!(matches!(
            fit_action(1281., 800., 1280, 800),
            Some(Resize {
                width: 1281,
                height: 800
            })
        ));
    }

    proptest::proptest! {
        #[test]
        fn resized_viewports_are_valid_and_stable(width in 0.01f64..20000., height in 0.01f64..20000.) {
            let action = fit_action(width, height, 0, 0).unwrap();
            proptest::prop_assert!(action.validate().is_ok());
            if let agent_protocol::browser::BrowserAction::Resize { width: actual_width, height: actual_height } = action {
                proptest::prop_assert!(fit_action(width, height, actual_width, actual_height).is_none());
            }
        }
    }

    #[test]
    fn addresses_search_and_host_local_development_are_distinct() {
        for (input, expected) in [
            ("example.com", "https://example.com/"),
            ("localhost:3000/path", "http://localhost:3000/path"),
            ("[::1]:4000", "http://[::1]:4000/"),
            (
                "日本語 search",
                "https://duckduckgo.com/?q=%E6%97%A5%E6%9C%AC%E8%AA%9E+search",
            ),
            (
                "query&secret=other",
                "https://duckduckgo.com/?q=query%26secret%3Dother",
            ),
        ] {
            assert_eq!(address_url(input).unwrap(), expected);
        }
        for input in [
            "",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "https://user:pass@example.com",
        ] {
            assert!(address_url(input).is_err());
        }
    }
}
