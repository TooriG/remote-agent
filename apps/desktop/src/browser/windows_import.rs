//! Windows browser import. Chrome performs password export with its own OS auth;
//! BEX never extracts Chrome's app-bound keys or writes plaintext credentials.
use super::*;
use agent_core::browser_import::{self, BrowserBookmark, BrowserLogin, MAX_IMPORT_BYTES};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use windows::{
    Win32::{
        Foundation::{HLOCAL, LocalFree},
        Security::Cryptography::{
            CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
        },
    },
    core::w,
};
use zeroize::{Zeroize, Zeroizing};

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct BrowserData {
    logins: Vec<BrowserLogin>,
    bookmarks: Vec<BrowserBookmark>,
}

fn data_path() -> Result<PathBuf, String> {
    directories::ProjectDirs::from("app", "bex", "BEX")
        .map(|dirs| dirs.data_local_dir().join("browser-import.dpapi"))
        .ok_or_else(|| "ブラウザの保存先を取得できません".into())
}

fn bounded_read(path: &Path, limit: usize) -> Result<Zeroizing<Vec<u8>>, String> {
    let file =
        fs::File::open(path).map_err(|_| "ファイルを開けません。アクセス権を確認してください")?;
    let mut bytes = Zeroizing::new(Vec::new());
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "ファイルを読み取れません")?;
    if bytes.len() > limit {
        return Err("ファイルが大きすぎます".into());
    }
    Ok(bytes)
}

fn crypt(bytes: &[u8], encrypt: bool) -> Result<Zeroizing<Vec<u8>>, String> {
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes
            .len()
            .try_into()
            .map_err(|_| "保存データが大きすぎます")?,
        pbData: bytes.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    // DPAPI defaults to the current Windows user; never use LOCAL_MACHINE.
    unsafe {
        let result = if encrypt {
            CryptProtectData(
                &input,
                w!("BEX browser data"),
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        result.map_err(|_| "ブラウザの保存データを暗号化・復号できません。同じWindowsユーザーで再試行してください")?;
        let buffer = std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize);
        let copied = Zeroizing::new(buffer.to_vec());
        buffer.zeroize();
        let _ = LocalFree(Some(HLOCAL(output.pbData.cast())));
        Ok(copied)
    }
}

fn load(path: &Path) -> Result<BrowserData, String> {
    match fs::metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(BrowserData::default());
        }
        Err(_) => return Err("ブラウザの保存データを確認できません".into()),
        Ok(_) => {}
    }
    let encrypted = bounded_read(path, MAX_IMPORT_BYTES * 8)?;
    let clear = crypt(&encrypted, false)?;
    serde_json::from_slice(&clear).map_err(|_| "ブラウザの保存データを読み取れません".into())
}

fn update(
    path: &Path,
    change: impl FnOnce(&mut BrowserData) -> Result<(), String>,
) -> Result<(), String> {
    let parent = path.parent().ok_or("保存先が正しくありません")?;
    fs::create_dir_all(parent).map_err(|_| "保存フォルダを作成できません")?;
    // Multiple windows/processes must merge against the most recent vault.
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path.with_extension("lock"))
        .map_err(|_| "保存データをロックできません")?;
    lock.lock().map_err(|_| "保存データをロックできません")?;
    let mut data = load(path)?;
    change(&mut data)?;
    let clear =
        Zeroizing::new(serde_json::to_vec(&data).map_err(|_| "保存データを変換できません")?);
    if clear.len() > MAX_IMPORT_BYTES * 4 {
        return Err("保存データが大きすぎます".into());
    }
    let encrypted = crypt(&clear, true)?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|_| "保存ファイルを作成できません")?;
    temporary
        .write_all(&encrypted)
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|_| "ブラウザデータを保存できません")?;
    temporary
        .persist(path)
        .map_err(|_| "ブラウザデータを置き換えられません")?;
    Ok(())
}

fn chrome_profiles() -> Result<Vec<(String, PathBuf)>, String> {
    let root = directories::BaseDirs::new()
        .ok_or("Chromeの保存先を取得できません")?
        .data_local_dir()
        .join("Google/Chrome/User Data");
    let state = bounded_read(&root.join("Local State"), MAX_IMPORT_BYTES)?;
    let state: serde_json::Value =
        serde_json::from_slice(&state).map_err(|_| "Chromeのプロファイルを読み取れません")?;
    let profiles = state
        .pointer("/profile/info_cache")
        .and_then(|v| v.as_object())
        .ok_or("Chromeのプロファイルがありません")?;
    let mut result = Vec::new();
    for (directory, info) in profiles {
        let mut components = Path::new(directory).components();
        if !matches!(components.next(), Some(std::path::Component::Normal(_)))
            || components.next().is_some()
        {
            continue;
        }
        let path = root.join(directory).join("Bookmarks");
        if path.is_file() {
            let name = info
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or(directory);
            result.push((format!("{name} ({directory})"), path));
        }
    }
    if result.is_empty() {
        return Err("ブックマークのあるChromeプロファイルがありません".into());
    }
    Ok(result)
}

fn import_passwords(path: &Path, destination: &Path) -> Result<String, String> {
    let bytes = bounded_read(path, MAX_IMPORT_BYTES)?;
    let imported = browser_import::parse_chrome_passwords(&bytes)?;
    let count = imported.logins.len();
    let skipped = imported.skipped;
    if count == 0 {
        return Err(format!(
            "取り込めるパスワードがありません（未対応URL・空欄など{skipped}件）"
        ));
    }
    update(destination, |data| {
        browser_import::merge_logins(&mut data.logins, imported.logins)
    })?;
    Ok(format!(
        "パスワード{count}件を取り込みました（除外{skipped}件）。サイトを開き「ログインを入力」から選択できます。書き出したCSVは削除してください。"
    ))
}

fn open_chrome_password_export() -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let chrome = ["LOCALAPPDATA", "ProgramFiles", "ProgramFiles(x86)"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(|root| PathBuf::from(root).join("Google/Chrome/Application/chrome.exe"))
        .find(|path| path.is_file())
        .ok_or("Chromeが見つかりません")?;
    std::process::Command::new(chrome)
        .arg("chrome://password-manager/settings")
        .creation_flags(0x08000000)
        .spawn()
        .map_err(|_| "Chromeを開けません")?;
    Ok(())
}

impl Browser {
    fn run_import(
        &mut self,
        work: impl FnOnce() -> Result<String, String> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        if self.importing {
            return;
        }
        self.importing = true;
        self.error.clear();
        self.import_notice = "取り込み中…".into();
        let task = cx.background_executor().spawn(async move { work() });
        cx.spawn(async move |owner, cx| {
            let result = task.await;
            let _ = owner.update(cx, |s, cx| {
                s.importing = false;
                match result {
                    Ok(notice) => s.import_notice = notice,
                    Err(error) => {
                        s.import_notice.clear();
                        s.error = error;
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn pick_password_csv(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.importing {
            return;
        }
        // Keep the native child behind the OS picker, restoring the panel state on cancel.
        self.webview.update(cx, |v, _| v.hide());
        let path = rfd::FileDialog::new()
            .set_title("Chromeから書き出したパスワードCSVを選択")
            .add_filter("Chrome passwords", &["csv"])
            .pick_file();
        self.set_visible(self.visible, cx);
        window.activate_window();
        if let Some(path) = path {
            self.run_import(move || import_passwords(&path, &data_path()?), cx);
        }
    }

    pub(super) fn windows_import_toolbar(&self, cx: &Context<Self>) -> impl IntoElement {
        let owner = cx.entity().downgrade();
        let import_owner = owner.clone();
        let visibility = owner.clone();
        let fill_owner = owner.clone();
        let fill_visibility = owner.clone();
        let bookmark_owner = owner.clone();
        let bookmark_visibility = owner;
        h_flex().px_2().pb_2().gap_1().flex_wrap()
            .child(Button::new("browser-import").label("Chromeから取り込む").small().ghost()
                .disabled(self.importing).dropdown_caret(true)
                .dropdown_menu(move |mut menu, _, _| {
                    let owner = import_owner.clone();
                    menu = menu.item(PopupMenuItem::new("パスワードCSVを取り込む…").on_click(move |_, window, cx| {
                        let _ = owner.update(cx, |s, cx| s.pick_password_csv(window, cx));
                    }));
                    let owner = import_owner.clone();
                    menu = menu.item(PopupMenuItem::new("Chromeでパスワードを書き出す…").on_click(move |_, _, cx| {
                        // OS opens Chrome; no browsing page gets a privileged browser bridge.
                        let opened = open_chrome_password_export();
                        let _ = owner.update(cx, |s, cx| {
                            s.import_notice = if opened.is_ok() { "Chromeの「パスワードをダウンロード」でCSVを書き出し、「パスワードCSVを取り込む」から選択してください。".into() } else { "Chromeで chrome://password-manager/settings を開き、「パスワードをダウンロード」でCSVを書き出してください。".into() };
                            cx.notify();
                        });
                    }));
                    match chrome_profiles() {
                        Ok(profiles) => for (name, path) in profiles {
                            let owner = import_owner.clone();
                            menu = menu.item(PopupMenuItem::new(format!("ブックマーク: {name}")).on_click(move |_, _, cx| {
                                let path = path.clone();
                                let _ = owner.update(cx, |s, cx| s.run_import(move || {
                                    let bookmarks = browser_import::parse_chrome_bookmarks(&bounded_read(&path, MAX_IMPORT_BYTES)?)?;
                                    let count = bookmarks.len();
                                    update(&data_path()?, |data| {
                                        let mut merged: std::collections::BTreeMap<_, _> = data.bookmarks.drain(..).map(|b| (b.url.clone(), b)).collect();
                                        for bookmark in bookmarks { merged.insert(bookmark.url.clone(), bookmark); }
                                        if merged.len() > browser_import::MAX_PASSWORDS { return Err("ブックマークは20,000件までです".into()); }
                                        data.bookmarks = merged.into_values().collect();
                                        Ok(())
                                    })?;
                                    Ok(format!("ブックマーク{count}件を取り込みました。「ブックマーク」から開けます。"))
                                }, cx));
                            }));
                        },
                        Err(_) => menu = menu.item(PopupMenuItem::new("Chromeのブックマークが見つかりません").disabled(true)),
                    }
                    let owner = import_owner.clone();
                    menu.item(PopupMenuItem::new("取り込んだデータをすべて削除").on_click(move |_, _, cx| {
                        let _ = owner.update(cx, |s, cx| s.run_import(|| {
                            update(&data_path()?, |data| { *data = BrowserData::default(); Ok(()) })?;
                            Ok("BEXに取り込んだパスワードとブックマークを削除しました。".into())
                        }, cx));
                    }))
                })
                .on_open_change(move |open, _, cx| {
                    let _ = visibility.update(cx, |s, cx| { s.profile_menu_open = *open; s.set_visible(s.visible, cx); cx.notify(); });
                }))
            .child(Button::new("browser-fill-login").label("ログインを入力").small().ghost().dropdown_caret(true)
                .dropdown_menu(move |mut menu, _, cx| {
                    let origin = fill_owner.update(cx, |s, cx| s.webview.read(cx).raw().url().ok().and_then(|u| browser_import::login_origin(&u))).ok().flatten();
                    let result = data_path().and_then(|p| load(&p));
                    match result {
                        Ok(data) => {
                            let matching: Vec<_> = data.logins.into_iter().filter(|l| Some(&l.origin) == origin.as_ref()).collect();
                            if matching.is_empty() { return menu.item(PopupMenuItem::new("このサイトの保存済みログインはありません").disabled(true)); }
                            for login in matching {
                                let owner = fill_owner.clone();
                                let label = if login.username.is_empty() { "ユーザー名なし".to_owned() } else { login.username.clone() };
                                menu = menu.item(PopupMenuItem::new(label).on_click(move |_, _, cx| {
                                    let _ = owner.update(cx, |s, cx| {
                                        let current = s.webview.read(cx).raw().url().ok().and_then(|u| browser_import::login_origin(&u));
                                        if current.as_deref() != Some(&login.origin) { s.error = "サイトが変わりました。ログインを選び直してください".into(); }
                                        else {
                                            let script = Zeroizing::new(browser_import::fill_login_script(&login));
                                            if s.webview.read(cx).raw().evaluate_script(&script).is_err() { s.error = "ログインを入力できませんでした".into(); }
                                            else { s.error.clear(); s.import_notice = "表示中のログインフォームに入力します。入力結果を確認してログインしてください。".into(); }
                                        }
                                        cx.notify();
                                    });
                                }));
                            }
                        }
                        Err(error) => { menu = menu.item(PopupMenuItem::new(error).disabled(true)); }
                    }
                    menu
                })
                .on_open_change(move |open, _, cx| { let _ = fill_visibility.update(cx, |s, cx| { s.profile_menu_open = *open; s.set_visible(s.visible, cx); cx.notify(); }); }))
            .child(Button::new("browser-bookmarks").label("ブックマーク").small().ghost().dropdown_caret(true)
                .dropdown_menu(move |mut menu, _, _| {
                    match data_path().and_then(|p| load(&p)) {
                        Ok(data) if !data.bookmarks.is_empty() => for bookmark in data.bookmarks {
                            let owner = bookmark_owner.clone();
                            menu = menu.item(PopupMenuItem::new(bookmark.name).on_click(move |_, window, cx| {
                                let _ = owner.update(cx, |s, cx| s.open_url(&bookmark.url, window, cx));
                            }));
                        },
                        Ok(_) => menu = menu.item(PopupMenuItem::new("取り込んだブックマークはありません").disabled(true)),
                        Err(error) => menu = menu.item(PopupMenuItem::new(error).disabled(true)),
                    }
                    menu
                })
                .on_open_change(move |open, _, cx| { let _ = bookmark_visibility.update(cx, |s, cx| { s.profile_menu_open = *open; s.set_visible(s.visible, cx); cx.notify(); }); }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn import_persists_encrypted_and_survives_reopen_and_failed_retry() {
        let dir = tempfile::tempdir().unwrap();
        let csv = dir.path().join("export.csv");
        let vault = dir.path().join("browser.dpapi");
        fs::write(
            &csv,
            "name,url,username,password,note\nx,https://example.test/login,user,fixture-secret,\n",
        )
        .unwrap();
        let original = fs::read(&csv).unwrap();
        assert!(import_passwords(&csv, &vault).unwrap().contains("1件"));
        assert_eq!(load(&vault).unwrap().logins[0].password, "fixture-secret");
        assert!(
            !fs::read(&vault)
                .unwrap()
                .windows(14)
                .any(|s| s == b"fixture-secret")
        );
        assert_eq!(fs::read(&csv).unwrap(), original);
        let before = fs::read(&vault).unwrap();
        fs::write(&csv, "wrong,header\nsecret,password\n").unwrap();
        assert!(import_passwords(&csv, &vault).is_err());
        assert_eq!(fs::read(&vault).unwrap(), before);
        fs::write(
            &csv,
            "url,username,password\nhttps://example.test,user,replaced\n",
        )
        .unwrap();
        import_passwords(&csv, &vault).unwrap();
        let data = load(&vault).unwrap();
        assert_eq!(data.logins.len(), 1);
        assert_eq!(data.logins[0].password, "replaced");
        update(&vault, |data| {
            *data = BrowserData::default();
            Ok(())
        })
        .unwrap();
        assert!(load(&vault).unwrap().logins.is_empty());
    }
    #[test]
    fn corrupt_vault_is_never_silently_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("browser.dpapi");
        fs::write(&path, b"corrupt").unwrap();
        assert!(update(&path, |_| Ok(())).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"corrupt");
    }
}
