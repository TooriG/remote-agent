//! Browser profile parsing and exact HTTPS password scopes.
pub const MAX_IMPORT_BYTES: usize = 16 * 1024 * 1024;
/// Passwords stay scoped to one HTTPS origin, including its port. Never broaden to a parent domain.
pub fn password_origin(input: &str) -> Result<String, String> {
    let url = url::Url::parse(input).map_err(|_| "ログイン先の URL が不正です。")?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || input.chars().any(char::is_control)
    {
        return Err("パスワードは HTTPS のサイトでのみ利用できます。".into());
    }
    Ok(url.origin().ascii_serialization())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChromeProfile {
    pub name: String,
    pub directory: String,
}
pub fn parse_chrome_profiles(bytes: &[u8]) -> Result<Vec<ChromeProfile>, String> {
    if bytes.len() > MAX_IMPORT_BYTES {
        return Err("Chromeのプロファイル情報が大きすぎます".into());
    }
    let state: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "Chromeのプロファイル情報を読み取れません")?;
    let profiles = state
        .pointer("/profile/info_cache")
        .and_then(|value| value.as_object())
        .ok_or("Chromeのプロファイル情報がありません")?;
    Ok(profiles
        .iter()
        .filter(|(directory, _)| {
            !directory.is_empty()
                // Win32 normalizes trailing dots/spaces before opening a path.
                && !directory.ends_with(['.', ' '])
                && !directory
                    .chars()
                    .any(|character| matches!(character, '/' | '\\' | ':' | '\0'))
        })
        .map(|(directory, info)| ChromeProfile {
            directory: directory.clone(),
            name: info
                .get("name")
                .and_then(|value| value.as_str())
                .unwrap_or(directory)
                .into(),
        })
        .collect())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn password_scopes_never_expand_beyond_an_https_origin() {
        assert_eq!(
            password_origin("https://Example.test:8443/login?x=1").unwrap(),
            "https://example.test:8443"
        );
        assert_eq!(
            password_origin("https://example.test:443/login").unwrap(),
            "https://example.test"
        );
        for input in [
            "http://example.test",
            "https://user:pass@example.test",
            "https://user@example.test",
            "https://:pass@example.test",
            "file:///private",
            "https://example.test\n.attacker.test",
            "data:text/html,test",
        ] {
            assert!(password_origin(input).is_err());
        }
        assert_ne!(
            password_origin("https://example.test").unwrap(),
            password_origin("https://example.test.attacker.test").unwrap()
        );
        assert_ne!(
            password_origin("https://example.test").unwrap(),
            password_origin("https://sub.example.test").unwrap()
        );
    }

    #[test]
    fn profile_paths_cannot_escape_the_browser_root() {
        let profiles = parse_chrome_profiles(br#"{"profile":{"info_cache":{"Default":{"name":"Main"},"../outside":{},"C:\\outside":{},"..":{},".. ":{},"...":{},"Default ":{},"":{}}}}"#).unwrap();
        assert_eq!(
            profiles,
            vec![ChromeProfile {
                name: "Main".into(),
                directory: "Default".into()
            }]
        );
    }

    #[test]
    fn import_byte_limit_is_inclusive_and_checked_before_parsing() {
        let mut bytes = br#"{"profile":{"info_cache":{}}}"#.to_vec();
        bytes.resize(MAX_IMPORT_BYTES, b' ');
        assert!(parse_chrome_profiles(&bytes).is_ok());
        bytes.push(b' ');
        assert!(parse_chrome_profiles(&bytes).is_err());
    }
    proptest::proptest! {
        #[test]
        fn arbitrary_input_does_not_panic(bytes in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..4096)) {
            let _ = parse_chrome_profiles(&bytes);
        }
    }
}
