//! Password import and exact-origin matching shared by native browsers.
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

pub const MAX_IMPORT_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_PASSWORDS: usize = 20_000;

#[derive(Serialize, Deserialize)]
pub struct BrowserBookmark {
    pub name: String,
    pub url: String,
}

pub fn parse_chrome_bookmarks(bytes: &[u8]) -> Result<Vec<BrowserBookmark>, String> {
    if bytes.len() > MAX_IMPORT_BYTES {
        return Err("ブックマークは16MB以下にしてください".into());
    }
    let data: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "Chromeのブックマークを読み取れません")?;
    let roots = data
        .get("roots")
        .and_then(|v| v.as_object())
        .ok_or("Chromeのブックマーク形式ではありません")?;
    let mut pending: Vec<_> = roots.values().collect();
    let mut result = Vec::new();
    while let Some(node) = pending.pop() {
        if let Some(children) = node.get("children").and_then(|v| v.as_array()) {
            pending.extend(children.iter().rev());
        }
        if node.get("type").and_then(|v| v.as_str()) == Some("url")
            && let Some(url) = node.get("url").and_then(|v| v.as_str())
            && let Ok(parsed) = url::Url::parse(url)
            && matches!(parsed.scheme(), "https" | "http")
            && parsed.host_str().is_some()
            && parsed.username().is_empty()
            && parsed.password().is_none()
        {
            result.push(BrowserBookmark {
                name: node
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or(url)
                    .to_owned(),
                url: parsed.to_string(),
            });
        }
    }
    if result.len() > MAX_PASSWORDS {
        return Err("ブックマークは20,000件までです".into());
    }
    Ok(result)
}

// Never derive Debug: these values must not appear in diagnostics.
#[derive(Serialize, Deserialize)]
pub struct BrowserLogin {
    pub origin: String,
    pub username: String,
    pub password: String,
}

impl Drop for BrowserLogin {
    fn drop(&mut self) {
        self.username.zeroize();
        self.password.zeroize();
    }
}

pub fn login_origin(value: &str) -> Option<String> {
    let url = url::Url::parse(value).ok()?;
    // Insecure pages may only receive credentials on loopback during development.
    let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if (url.scheme() != "https" && !(url.scheme() == "http" && loopback))
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    Some(url.origin().ascii_serialization())
}

pub struct PasswordImport {
    pub logins: Vec<BrowserLogin>,
    pub skipped: usize,
}

/// Chrome's exported CSV; malformed files fail atomically without echoing a row.
pub fn parse_chrome_passwords(bytes: &[u8]) -> Result<PasswordImport, String> {
    if bytes.len() > MAX_IMPORT_BYTES {
        return Err("CSVは16MB以下にしてください".into());
    }
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    let mut reader = csv::ReaderBuilder::new().from_reader(bytes);
    let headers = reader
        .headers()
        .map_err(|_| "CSVのヘッダーを読み取れません")?;
    let column = |name| {
        let mut indices = headers.iter().enumerate().filter(|(_, h)| *h == name);
        let index = indices.next().map(|(i, _)| i);
        if indices.next().is_some() {
            None
        } else {
            index
        }
    };
    let (Some(url), Some(username), Some(password)) =
        (column("url"), column("username"), column("password"))
    else {
        return Err(
            "Chromeから書き出したCSV（url, username, password列）を選択してください".into(),
        );
    };
    let mut result = PasswordImport {
        logins: Vec::new(),
        skipped: 0,
    };
    for (index, record) in reader.records().enumerate() {
        if index >= MAX_PASSWORDS {
            return Err("パスワードは一度に20,000件まで取り込めます".into());
        }
        let row =
            record.map_err(|_| "CSVの形式が正しくありません。Chromeから再度書き出してください")?;
        let Some(origin) = login_origin(&row[url]) else {
            result.skipped += 1;
            continue;
        };
        if row[password].is_empty() {
            result.skipped += 1;
            continue;
        }
        result.logins.push(BrowserLogin {
            origin,
            username: row[username].to_owned(),
            password: row[password].to_owned(),
        });
    }
    Ok(result)
}

/// Reimport updates the same account while preserving other saved accounts.
pub fn merge_logins(
    saved: &mut Vec<BrowserLogin>,
    imported: Vec<BrowserLogin>,
) -> Result<(), String> {
    let mut merged = std::collections::BTreeMap::new();
    // Check capacity before modifying either input or persisted state.
    for login in saved.iter().chain(&imported) {
        merged.insert((login.origin.as_str(), login.username.as_str()), ());
    }
    if merged.len() > MAX_PASSWORDS {
        return Err("保存できるパスワードは20,000件までです".into());
    }
    drop(merged);
    let mut merged = std::collections::BTreeMap::new();
    for login in saved.drain(..).chain(imported) {
        merged.insert((login.origin.clone(), login.username.clone()), login);
    }
    *saved = merged.into_values().collect();
    Ok(())
}

/// Filled only after an explicit account selection, in the top-level document.
/// Recheck the actual origin inside the script to handle navigation races.
pub fn fill_login_script(login: &BrowserLogin) -> String {
    let origin = serde_json::to_string(&login.origin).expect("string");
    let username = serde_json::to_string(&login.username).expect("string");
    let password = serde_json::to_string(&login.password).expect("string");
    format!(
        r#"(() => {{
      if (window.top !== window || location.origin !== {origin}) return;
      const visible = e => !e.disabled && !e.readOnly && e.getClientRects().length > 0;
      const passwords = [...document.querySelectorAll('input[type="password"]')]
        .filter(e => visible(e) && e.autocomplete !== 'new-password');
      if (passwords.length !== 1) return;
      const p = passwords[0], form = p.form;
      if (form && form.action && new URL(form.action, location.href).origin !== location.origin) return;
      const inputs = [...(form || document).querySelectorAll('input')].filter(visible);
      const u = inputs.find(e => e.autocomplete === 'username') ||
        inputs.filter(e => ['email','text'].includes(e.type) &&
          (e.compareDocumentPosition(p) & Node.DOCUMENT_POSITION_FOLLOWING)).pop();
      const set = (e,v) => {{
        Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(e,v);
        e.dispatchEvent(new Event('input',{{bubbles:true}}));
        e.dispatchEvent(new Event('change',{{bubbles:true}}));
      }};
      if (u) set(u,{username});
      set(p,{password});
    }})()"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bookmarks_include_nested_web_links_and_exclude_executable_urls() {
        let bookmarks = parse_chrome_bookmarks(br#"{"roots":{"bookmark_bar":{"type":"folder","children":[{"type":"url","name":"Safe","url":"https://example.test/a"},{"type":"folder","children":[{"type":"url","name":"Local","url":"http://localhost:8888/"},{"type":"url","name":"Bad","url":"javascript:alert(1)"},{"type":"url","url":"file:///C:/private"},{"type":"url","url":"https://user:pass@example.test"}]}]}}}"#).unwrap();
        assert_eq!(bookmarks.len(), 2);
        assert_eq!(bookmarks[0].name, "Safe");
        assert_eq!(bookmarks[1].url, "http://localhost:8888/");
        assert!(parse_chrome_bookmarks(b"{}").is_err());
    }
    #[test]
    fn oversized_imports_fail_before_parsing() {
        let bytes = vec![b'a'; MAX_IMPORT_BYTES + 1];
        assert!(parse_chrome_passwords(&bytes).is_err());
        assert!(parse_chrome_bookmarks(&bytes).is_err());
    }
    #[test]
    fn chrome_csv_handles_bom_quotes_unicode_newlines_and_skips_unsafe_rows() {
        let data = "\u{feff}name,url,username,password,note\r\nx,https://example.com/login,名前,\"a,\"\"b\n秘密\",\r\nx,http://example.com,u,p,\r\nx,android://app,u,p,\r\nx,https://example.com,u,,\r\n";
        let result = parse_chrome_passwords(data.as_bytes()).unwrap();
        assert_eq!(result.logins.len(), 1);
        assert_eq!(result.skipped, 3);
        assert_eq!(result.logins[0].origin, "https://example.com");
        assert_eq!(result.logins[0].username, "名前");
        assert_eq!(result.logins[0].password, "a,\"b\n秘密");
    }
    #[test]
    fn malformed_csv_does_not_echo_secrets() {
        for text in [
            "url,username\nhttps://example.com,secret",
            "url,username,password\nhttps://example.com,u,secret,extra",
            "url,url,username,password\na,b,u,secret",
        ] {
            let error = parse_chrome_passwords(text.as_bytes()).err().unwrap();
            assert!(!error.contains("secret"));
        }
    }
    #[test]
    fn reimport_updates_only_matching_account() {
        let parse = |s: &str| {
            parse_chrome_passwords(format!("url,username,password\n{s}").as_bytes())
                .unwrap()
                .logins
        };
        let mut saved = parse("https://a.test,u,old\nhttps://a.test,v,keep");
        merge_logins(
            &mut saved,
            parse("https://a.test/login,u,new\nhttps://b.test,u,other"),
        )
        .unwrap();
        assert_eq!(saved.len(), 3);
        assert_eq!(saved[0].password, "new");
        assert_eq!(saved[1].password, "keep");
    }
    #[test]
    fn origin_matching_never_expands_to_other_hosts_schemes_or_ports() {
        assert_eq!(
            login_origin("https://EXAMPLE.com:443/a").as_deref(),
            Some("https://example.com")
        );
        for url in [
            "http://example.com",
            "file:///a",
            "javascript:alert(1)",
            "https://user:pass@example.com",
        ] {
            assert!(login_origin(url).is_none());
        }
        let origin = login_origin("https://example.com").unwrap();
        for url in [
            "https://evil.example.com",
            "https://example.com.evil.test",
            "https://example.com:444",
        ] {
            assert_ne!(login_origin(url).unwrap(), origin);
        }
    }
    proptest::proptest! {
        #[test]
        fn arbitrary_csv_never_panics(input in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..4096)) {
            let _ = parse_chrome_passwords(&input);
        }
    }
}
