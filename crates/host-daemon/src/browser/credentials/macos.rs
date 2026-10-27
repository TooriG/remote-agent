use super::{Cookie, Prepared};
use agent_protocol::browser::{BrowserAction, BrowserCredentials, BrowserImportProfile};
use agent_protocol::browser_import::password_origin;
#[path = "passwords.rs"]
mod passwords;
type StoragePassword = fn() -> Result<Vec<u8>, String>;
struct ImportSource {
    name: &'static str,
    root: std::path::PathBuf,
    password: StoragePassword,
}
fn keychain_secret(service: &str, account: &str) -> Result<Vec<u8>, String> {
    security_framework::passwords::get_generic_password(service, account).map_err(|_| {
        "ブラウザのKeychainにアクセスできません。アクセスを許可して再試行してください".into()
    })
}

fn import_sources() -> Vec<ImportSource> {
    let Some(dirs) = directories::BaseDirs::new() else {
        return Vec::new();
    };
    let root = dirs.home_dir().join("Library/Application Support");
    let definitions: [(&str, &str, StoragePassword); 6] = [
        ("Chrome", "Google/Chrome", || {
            keychain_secret("Chrome Safe Storage", "Chrome")
        }),
        ("Microsoft Edge", "Microsoft Edge", || {
            keychain_secret("Microsoft Edge Safe Storage", "Microsoft Edge")
        }),
        ("Brave", "BraveSoftware/Brave-Browser", || {
            keychain_secret("Brave Safe Storage", "Brave")
        }),
        ("Arc", "Arc/User Data", || {
            keychain_secret("Arc Safe Storage", "Arc")
        }),
        ("Vivaldi", "Vivaldi", || {
            keychain_secret("Vivaldi Safe Storage", "Vivaldi")
        }),
        ("Opera", "com.operasoftware.Opera", || {
            keychain_secret("Opera Safe Storage", "Opera")
        }),
    ];
    definitions
        .into_iter()
        .filter_map(|(name, directory, password)| {
            let root = root.join(directory);
            root.is_dir().then_some(ImportSource {
                name,
                root,
                password,
            })
        })
        .collect()
}

fn chromium_profiles(root: &std::path::Path) -> Result<Vec<(String, std::path::PathBuf)>, String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(root.join("Local State"))
        .and_then(|file| {
            file.take(agent_protocol::browser_import::MAX_IMPORT_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
        })
        .map_err(|_| {
            "ブラウザのプロファイルが見つかりません。取り込み元のブラウザを一度起動してください"
                .to_string()
        })?;
    let result: Vec<_> = agent_protocol::browser_import::parse_chrome_profiles(&bytes)?
        .into_iter()
        .filter_map(|profile| {
            let path = root.join(&profile.directory);
            (chrome_cookie_path(&path).is_some() || path.join("Login Data").is_file())
                .then(|| (format!("{} ({})", profile.name, profile.directory), path))
        })
        .collect();
    if result.is_empty() {
        return Err("取り込めるブラウザプロファイルが見つかりません".into());
    }
    Ok(result)
}

fn chrome_cookie_path(profile: &std::path::Path) -> Option<std::path::PathBuf> {
    ["Network/Cookies", "Cookies"]
        .into_iter()
        .map(|name| profile.join(name))
        .find(|path| path.is_file())
}

fn read_chrome_cookies(
    profile: &std::path::Path,
    now: i64,
    password: impl FnOnce() -> Result<Vec<u8>, String>,
) -> Result<(Vec<Cookie>, usize), String> {
    use sha2::{Digest, Sha256};
    use zeroize::Zeroizing;
    let path = chrome_cookie_path(profile).ok_or("ブラウザのCookieファイルが見つかりません")?;
    let mut db =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|_| {
                "ブラウザのCookieを開けません。ファイルのアクセス権を確認してください".to_string()
            })?;
    db.busy_timeout(std::time::Duration::from_secs(3))
        .map_err(|_| "Cookieの読み込み設定に失敗しました")?;
    // One read transaction includes committed WAL rows without copying or modifying Chrome's DB.
    let transaction = db
        .transaction()
        .map_err(|_| "ブラウザのCookieを読み取れません。再試行してください")?;
    let schema: i64 = transaction
        .query_row("SELECT value FROM meta WHERE key = 'version'", [], |row| {
            row.get::<_, String>(0)?
                .parse::<i64>()
                .map_err(|_| rusqlite::Error::InvalidQuery)
        })
        .map_err(|_| "ブラウザのCookie形式を確認できません")?;
    // The selected columns and cipher are checked below. Schema 24 and later
    // bind decrypted values to the host; a later additive schema is compatible.
    if schema < 23 {
        return Err("このブラウザのCookie形式にはまだ対応していません".into());
    }
    let mut statement = transaction.prepare("SELECT host_key, name, value, encrypted_value, path, is_secure, is_httponly, samesite, expires_utc, has_expires, top_frame_site_key FROM cookies")
        .map_err(|_| "ブラウザのCookie形式を読み取れません")?;
    let rows = statement
        .query_map([], |row| {
            let expires: i64 = row.get(8)?;
            Ok((
                Cookie {
                    domain: row.get(0)?,
                    name: row.get(1)?,
                    value: Zeroizing::new(row.get(2)?),
                    path: row.get(4)?,
                    secure: row.get(5)?,
                    http_only: row.get(6)?,
                    same_site: row.get(7)?,
                    expires: if row.get::<_, bool>(9)? {
                        Some(expires / 1_000_000 - 11_644_473_600)
                    } else {
                        None
                    },
                },
                row.get::<_, Vec<u8>>(3)?,
                row.get::<_, String>(10)?,
            ))
        })
        .map_err(|_| "ブラウザのCookieを読み取れません")?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "ブラウザのCookieに読み取れないデータがあります")?;
    drop(statement);
    drop(transaction);
    drop(db);
    let mut password = Some(password);
    let mut key = None;
    let mut cookies = Vec::new();
    let mut skipped = 0;
    let mut identities = std::collections::HashSet::new();
    for (mut cookie, encrypted, partition) in rows {
        if !partition.is_empty() || cookie.expires.is_some_and(|expires| expires <= now) {
            skipped += 1;
            continue;
        }
        if !encrypted.is_empty() {
            if !cookie.value.is_empty() {
                return Err("ブラウザのCookieに矛盾する値があります".into());
            }
            let encrypted = chromium_payload(&encrypted)?;
            if key.is_none() {
                let secret = Zeroizing::new(password.take().expect("key requested once")()?);
                key = Some(chromium_key(&secret));
            }
            let key = key.as_ref().expect("derived key");
            let clear = decrypt_chromium_payload(encrypted, key)?;
            let value = if schema >= 24 {
                if clear.len() < 32 || clear[..32] != Sha256::digest(cookie.domain.as_bytes())[..] {
                    return Err(
                        "ブラウザのCookieのドメイン検証に失敗しました。取り込みは行いませんでした"
                            .into(),
                    );
                }
                &clear[32..]
            } else {
                &clear[..]
            };
            cookie.value = Zeroizing::new(
                std::str::from_utf8(value)
                    .map_err(|_| "ブラウザのCookieの文字コードに対応していません")?
                    .to_owned(),
            );
        }
        if !matches!(cookie.same_site, -1..=2)
            || cookie.domain.is_empty()
            || !cookie.path.starts_with('/')
            || cookie
                .name
                .bytes()
                .any(|b| b <= 0x20 || b >= 0x7f || b"()<>@,;:\\\"/[]?={}".contains(&b))
            || cookie
                .value
                .bytes()
                .any(|b| b < 0x20 || b == 0x7f || b == b';')
            || url::Host::parse(cookie.domain.trim_start_matches('.')).is_err()
        {
            return Err(
                "ブラウザのCookieに未対応の属性があります。取り込みは行いませんでした".into(),
            );
        }
        if !identities.insert((
            cookie.domain.clone(),
            cookie.name.clone(),
            cookie.path.clone(),
        )) {
            return Err("Cookieが重複しています。取り込みは行いませんでした".into());
        }
        cookies.push(cookie);
    }
    Ok((cookies, skipped))
}

fn chromium_key(secret: &[u8]) -> zeroize::Zeroizing<[u8; 16]> {
    let mut key = zeroize::Zeroizing::new([0u8; 16]);
    pbkdf2::pbkdf2_hmac::<sha1::Sha1>(secret, b"saltysalt", 1003, key.as_mut());
    key
}

fn chromium_payload(encrypted: &[u8]) -> Result<&[u8], String> {
    let payload = encrypted
        .strip_prefix(b"v10")
        .ok_or("未対応のブラウザ暗号化方式です。取り込みは行いませんでした。")?;
    if payload.is_empty() || !payload.len().is_multiple_of(16) || payload.len() > 64 * 1024 {
        return Err("ブラウザの暗号化データが不正です。".into());
    }
    Ok(payload)
}

fn decrypt_chromium_payload(
    encrypted: &[u8],
    key: &[u8; 16],
) -> Result<zeroize::Zeroizing<Vec<u8>>, String> {
    use aes::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};
    cbc::Decryptor::<aes::Aes128>::new(key.into(), (&[b' '; 16]).into())
        .decrypt_padded_vec_mut::<Pkcs7>(encrypted)
        .map(zeroize::Zeroizing::new)
        .map_err(|_| "ブラウザの保存データを復号できません。取り込みは行いませんでした。".into())
}

fn profiles() -> Vec<BrowserImportProfile> {
    let mut result = Vec::new();
    for source in import_sources() {
        match chromium_profiles(&source.root) {
            Ok(profiles) => {
                result.extend(
                    profiles
                        .into_iter()
                        .map(|(name, path)| BrowserImportProfile {
                            id: format!(
                                "{}/{}",
                                source.name,
                                path.file_name().unwrap().to_string_lossy()
                            ),
                            label: format!("{} · {name}", source.name),
                        }),
                )
            }
            Err(error) => result.push(BrowserImportProfile {
                id: String::new(),
                label: format!("{}: {error}", source.name),
            }),
        }
    }
    result
}
fn selected_profile(id: &str) -> Result<(std::path::PathBuf, StoragePassword), String> {
    for source in import_sources() {
        if let Ok(profiles) = chromium_profiles(&source.root) {
            for (_, path) in profiles {
                if id
                    == format!(
                        "{}/{}",
                        source.name,
                        path.file_name().unwrap().to_string_lossy()
                    )
                {
                    return Ok((path, source.password));
                }
            }
        }
    }
    Err("取り込み元のプロファイルが見つかりません。もう一度一覧を開いてください。".into())
}
pub(super) fn prepare(action: BrowserAction, origin: Option<String>) -> Result<Prepared, String> {
    match action {
        BrowserAction::Credentials => {
            let (accounts, password_error) = match origin.as_deref().map(passwords::accounts) {
                Some(Ok(accounts)) => (accounts, None),
                Some(Err(error)) => (Vec::new(), Some(error)),
                None => (Vec::new(), None),
            };
            Ok(Prepared::Metadata(BrowserCredentials {
                profiles: profiles(),
                origin,
                accounts,
                password_error,
            }))
        }
        BrowserAction::ImportCookies { profile } => {
            let (path, password) = selected_profile(&profile)?;
            let (cookies, skipped) = read_chrome_cookies(
                &path,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|_| "現在時刻を確認できません。")?
                    .as_secs() as i64,
                password,
            )?;
            Ok(Prepared::Cookies { cookies, skipped })
        }
        BrowserAction::ImportPasswords { profile } => {
            let (path, password) = selected_profile(&profile)?;
            passwords::import(&path, password).map(Prepared::Notice)
        }
        BrowserAction::FillPassword { origin, username } => {
            let script = passwords::script(&origin, &username)?;
            Ok(Prepared::Fill { origin, script })
        }
        BrowserAction::DeletePasswords { origin } => {
            passwords::delete(&origin).map(Prepared::Notice)
        }
        _ => unreachable!("credential action checked before preparing"),
    }
}
#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    pub(super) fn fixture() -> (tempfile::TempDir, Connection) {
        let directory = tempfile::tempdir().unwrap();
        let db = Connection::open(directory.path().join("Cookies")).unwrap();
        db.execute_batch(include_str!("../../../tests/fixtures/chrome-cookies.sql"))
            .unwrap();
        (directory, db)
    }
    pub(super) fn password() -> Result<Vec<u8>, String> {
        Ok(b"fixture-password".to_vec())
    }

    #[test]
    fn decrypts_chrome_v24_and_preserves_cookie_attributes() {
        use super::read_chrome_cookies;
        let (directory, db) = fixture();
        let original = std::fs::read(directory.path().join("Cookies")).unwrap();
        let (cookies, skipped) =
            read_chrome_cookies(directory.path(), 1_700_000_000, password).unwrap();
        assert_eq!((cookies.len(), skipped), (1, 0));
        let cookie = &cookies[0];
        assert_eq!(&**cookie.value, "fixture-login");
        assert_eq!(cookie.expires, Some(1_800_000_000));
        let params = cookie.params();
        assert_eq!(params["domain"], ".example.test");
        assert_eq!(params["name"], "login");
        assert_eq!(params["value"], "fixture-login");
        assert_eq!(params["path"], "/account");
        assert_eq!(params["httpOnly"], true);
        assert_eq!(params["secure"], true);
        assert_eq!(params["sameSite"], "Lax");
        assert_eq!(params["expires"], 1_800_000_000_i64);
        assert_eq!(
            std::fs::read(directory.path().join("Cookies")).unwrap(),
            original
        );
        assert_eq!(
            db.query_row("SELECT count(*) FROM cookies", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
    }

    #[test]
    fn reads_committed_wal_session_cookies_without_reading_keychain_or_uncommitted_rows() {
        use super::read_chrome_cookies;
        let (directory, db) = fixture();
        db.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;
            UPDATE cookies SET value='session', encrypted_value=X'', host_key='example.test',
            has_expires=0, is_persistent=0, expires_utc=0, is_secure=0, is_httponly=0, samesite=2;
            BEGIN IMMEDIATE; UPDATE cookies SET value='uncommitted';",
        )
        .unwrap();
        let (cookies, skipped) = read_chrome_cookies(directory.path(), 1_900_000_000, || {
            panic!("plaintext needs no Keychain")
        })
        .unwrap();
        assert_eq!((cookies.len(), skipped), (1, 0));
        let params = cookies[0].params();
        assert_eq!(params["value"], "session");
        assert_eq!(params["domain"], "example.test");
        assert_eq!(params["httpOnly"], false);
        assert_eq!(params["secure"], false);
        assert!(params.get("expires").is_none());
        assert_eq!(params["sameSite"], "Strict");
        db.execute_batch("ROLLBACK;").unwrap();
    }

    #[test]
    fn supports_v23_ciphertext_and_empty_values_and_samesite_none() {
        use super::read_chrome_cookies;
        let (directory, db) = fixture();
        db.execute_batch("UPDATE meta SET value='23' WHERE key='version';
            UPDATE cookies SET encrypted_value=X'7631309d53da233d3900ecc985316cd65cae4a', samesite=0;").unwrap();
        let (cookies, _) = read_chrome_cookies(directory.path(), 0, password).unwrap();
        assert_eq!(&**cookies[0].value, "fixture-login");
        assert_eq!(cookies[0].params()["sameSite"], "None");
        db.execute_batch("UPDATE cookies SET encrypted_value=X'', value='', samesite=-1;")
            .unwrap();
        let (cookies, _) =
            read_chrome_cookies(directory.path(), 0, || panic!("empty plaintext")).unwrap();
        assert_eq!(cookies.len(), 1);
        assert!(cookies[0].value.is_empty());
        assert!(cookies[0].params().get("sameSite").is_none());
    }

    #[test]
    fn excludes_expired_and_partitioned_cookies_without_decrypting_them() {
        use super::read_chrome_cookies;
        let (directory, db) = fixture();
        let (cookies, skipped) =
            read_chrome_cookies(directory.path(), 1_800_000_000, || panic!("expired")).unwrap();
        assert!(cookies.is_empty());
        assert_eq!(skipped, 1);
        db.execute_batch("UPDATE cookies SET top_frame_site_key='https://other.test';")
            .unwrap();
        let (cookies, skipped) =
            read_chrome_cookies(directory.path(), 0, || panic!("partitioned")).unwrap();
        assert!(cookies.is_empty());
        assert_eq!(skipped, 1);
    }

    #[test]
    fn rejects_wrong_key_domain_hash_and_unknown_format_without_exposing_values() {
        use super::read_chrome_cookies;
        let (directory, db) = fixture();
        for password in [b"wrong".to_vec(), Vec::new()] {
            let error = read_chrome_cookies(directory.path(), 0, || Ok(password))
                .err()
                .unwrap();
            assert!(!error.contains("fixture-login"));
        }
        db.execute_batch("UPDATE cookies SET host_key='.elsewhere.test';")
            .unwrap();
        assert!(
            read_chrome_cookies(directory.path(), 0, password)
                .err()
                .unwrap()
                .contains("ドメイン検証")
        );
        db.execute_batch("UPDATE cookies SET encrypted_value=X'763230001122';")
            .unwrap();
        assert!(read_chrome_cookies(directory.path(), 0, || panic!("unknown cipher")).is_err());
        db.execute_batch("UPDATE meta SET value='22' WHERE key='version';")
            .unwrap();
        assert!(read_chrome_cookies(directory.path(), 0, || panic!("unknown schema")).is_err());
    }

    #[test]
    fn later_cookie_schemas_still_require_the_domain_binding() {
        use super::read_chrome_cookies;
        let (directory, db) = fixture();
        db.execute_batch("UPDATE meta SET value='25' WHERE key='version';")
            .unwrap();
        let (cookies, skipped) = read_chrome_cookies(directory.path(), 0, password).unwrap();
        assert_eq!((cookies.len(), skipped), (1, 0));
        assert_eq!(&**cookies[0].value, "fixture-login");
        db.execute_batch("UPDATE cookies SET host_key='.elsewhere.test';")
            .unwrap();
        assert!(read_chrome_cookies(directory.path(), 0, password).is_err());
    }

    #[test]
    fn can_retry_after_denied_keychain_and_missing_database() {
        use super::read_chrome_cookies;
        let directory = tempfile::tempdir().unwrap();
        assert!(read_chrome_cookies(directory.path(), 0, password).is_err());
        assert!(!directory.path().join("Cookies").exists());
        let db = Connection::open(directory.path().join("Cookies")).unwrap();
        db.execute_batch(include_str!("../../../tests/fixtures/chrome-cookies.sql"))
            .unwrap();
        assert_eq!(
            read_chrome_cookies(directory.path(), 0, || Err("denied".into()))
                .err()
                .unwrap(),
            "denied"
        );
        let (cookies, skipped) = read_chrome_cookies(directory.path(), 0, password).unwrap();
        assert_eq!((cookies.len(), skipped), (1, 0));
    }

    #[test]
    fn lists_named_profiles_with_both_cookie_locations_and_rejects_path_traversal() {
        use super::chromium_profiles;
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(directory.path().join("Default/Network")).unwrap();
        std::fs::create_dir_all(directory.path().join("Profile 2")).unwrap();
        std::fs::write(directory.path().join("Default/Network/Cookies"), []).unwrap();
        std::fs::write(directory.path().join("Profile 2/Cookies"), []).unwrap();
        std::fs::write(directory.path().join("Local State"), r#"{"profile":{"info_cache":{"Default":{"name":"個人用"},"Profile 2":{"name":"仕事用"},"Missing":{"name":"removed"},"../escape":{"name":"invalid"}}}}"#).unwrap();
        let profiles = chromium_profiles(directory.path()).unwrap();
        assert_eq!(profiles.len(), 2);
        assert_eq!(
            profiles[0],
            ("個人用 (Default)".into(), directory.path().join("Default"))
        );
        assert_eq!(
            profiles[1],
            (
                "仕事用 (Profile 2)".into(),
                directory.path().join("Profile 2")
            )
        );
        std::fs::write(directory.path().join("Local State"), "broken").unwrap();
        assert!(chromium_profiles(directory.path()).is_err());
    }
}
