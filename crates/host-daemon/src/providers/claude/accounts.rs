//! Credentials stay in Claude Code's own storage. Only account labels and the
//! selection are persisted here. Conversation settings and transcripts stay in
//! the native home; only secure credential storage changes with the account.
use crate::providers::{AccountCommand, AccountReply};
use agent_protocol::{
    models::Empty,
    operations::{Account, AccountLogin, AccountLoginStatus, AccountSelection},
    session::ProviderKind,
};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::{Child, ChildStdin},
    sync::oneshot,
    task::JoinHandle,
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Registry {
    accounts: Vec<Account>,
    selected_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Info {
    email: Option<String>,
    subscription_type: Option<String>,
    token_source: Option<String>,
    api_key_source: Option<String>,
}

impl Info {
    fn account(self, id: String) -> Option<Account> {
        let plan = self.subscription_type.filter(|plan| !plan.is_empty())?;
        if self.token_source.as_deref() == Some("none") {
            return None;
        }
        Some(Account {
            id,
            provider: ProviderKind::Claude,
            email: self.email,
            plan_type: Some(plan),
            usage: None,
        })
    }

    fn logged_out(&self) -> bool {
        self.token_source.as_deref() == Some("none")
            && self.api_key_source.as_deref() == Some("none")
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            accounts: Vec::new(),
            selected_id: Some("claude:desktop".into()),
        }
    }
}

pub(crate) struct Accounts {
    program: PathBuf,
    directory: PathBuf,
    native_home: PathBuf,
    native_inherited: bool,
    registry: Registry,
    revision: u64,
    native_checked_at: Option<Instant>,
    login: Option<Login>,
    usage: crate::account_usage::UsageCache,
}
struct Login {
    started: Instant,
    id: String,
    home: PathBuf,
    process: Cli,
}
struct Cli {
    child: Child,
    input: Option<ChildStdin>,
    output: JoinHandle<Result<(), String>>,
}
impl Drop for Cli {
    fn drop(&mut self) {
        // EOF tells the supervisor to terminate the whole process group.
        self.input.take();
        self.output.abort();
    }
}

impl Accounts {
    pub(super) async fn load(
        program: PathBuf,
        directory: PathBuf,
        native_home: PathBuf,
        native_inherited: bool,
    ) -> anyhow::Result<Self> {
        agent_local_host::create_state_directory(&directory)?;
        let registry = match tokio::fs::read(directory.join("accounts.json")).await {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Registry::default(),
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            program,
            directory,
            native_home,
            native_inherited,
            registry,
            revision: 0,
            native_checked_at: None,
            login: None,
            usage: Default::default(),
        })
    }

    pub(super) fn revision(&self) -> u64 {
        self.revision
    }

    pub(super) fn selected_home(&self) -> Result<Option<PathBuf>, String> {
        match self.registry.selected_id.as_deref() {
            Some("claude:desktop") => Ok(Some(self.native_home.clone())),
            Some(id) => self.account_home(id).map(Some),
            None => Ok(None),
        }
    }

    fn account_home(&self, id: &str) -> Result<PathBuf, String> {
        if !self
            .registry
            .accounts
            .iter()
            .any(|account| account.id == id)
        {
            return Err("Claude アカウントが見つかりません。".into());
        }
        if id == "claude:desktop" {
            return Ok(self.native_home.clone());
        }
        let uuid = id
            .strip_prefix("claude:")
            .and_then(|id| uuid::Uuid::parse_str(id).ok())
            .ok_or("Claude アカウントIDが無効です。")?;
        Ok(self.directory.join(uuid.to_string()))
    }

    async fn save(&self) -> Result<(), String> {
        let path = self.directory.join("accounts.json");
        let value = self.registry.clone();
        tokio::task::spawn_blocking(move || agent_local_host::save_private_json(&path, &value))
            .await
            .map_err(|_| "Claude アカウントを保存できません。")?
            .map_err(|_| "Claude アカウントを保存できません。".into())
    }

    async fn info(&self, home: &Path) -> Result<Info, String> {
        let cwd = tempfile::tempdir_in(&self.directory).map_err(|error| error.to_string())?;
        let probe = super::process::Process::probe(
            &self.program,
            super::process::homes(&self.native_home, self.native_inherited, home)
                .map(|(_, home)| (home, home)),
            cwd.path(),
        )
        .await?;
        serde_json::from_value(probe.initialized["account"].clone())
            .map_err(|_| "Claude の認証状態を読み取れません。".into())
    }

    pub(crate) async fn list(&mut self) -> Result<(Vec<Account>, Option<String>), String> {
        // Opening settings must not launch a CLI for every refresh. Selection
        // still verifies credentials, and native logout invalidates this label cache.
        if self
            .native_checked_at
            .is_none_or(|at| at.elapsed() >= Duration::from_secs(60))
        {
            let native = self
                .info(&self.native_home)
                .await?
                .account("claude:desktop".into());
            let previous = self
                .registry
                .accounts
                .iter()
                .find(|account| account.id == "claude:desktop");
            if previous.and_then(|account| account.email.as_deref())
                != native.as_ref().and_then(|account| account.email.as_deref())
            {
                self.usage.remove("claude:desktop");
            }
            self.registry
                .accounts
                .retain(|account| account.id != "claude:desktop");
            if let Some(account) = native {
                self.registry.accounts.insert(0, account);
            }
            self.native_checked_at = Some(Instant::now());
        }
        let selected = self
            .registry
            .selected_id
            .as_ref()
            .filter(|id| {
                self.registry
                    .accounts
                    .iter()
                    .any(|account| &account.id == *id)
            })
            .cloned();
        Ok((self.registry.accounts.clone(), selected))
    }

    pub(crate) fn usage_request(
        &mut self,
        id: &str,
    ) -> Result<
        impl std::future::Future<Output = agent_protocol::operations::AccountUsage> + use<>,
        String,
    > {
        let home = self.account_home(id)?;
        let program = self.program.clone();
        let homes = super::process::homes(&self.native_home, self.native_inherited, &home)
            .map(|(config, credentials)| (config.to_owned(), credentials.to_owned()));
        let directory = self.directory.clone();
        let cache = self.usage.entry(id.to_owned()).or_default().clone();
        Ok(async move {
            cache
                .read(async {
                    super::process::Process::probe(
                        &program,
                        homes
                            .as_ref()
                            .map(|(config, credentials)| (config.as_path(), credentials.as_path())),
                        &directory,
                    )
                    .await?
                    .usage
                    .map(|usage| super::usage::windows(&usage))
                })
                .await
        })
    }

    pub(crate) async fn request(
        &mut self,
        command: AccountCommand,
    ) -> Result<AccountReply, String> {
        match command {
            AccountCommand::Select { id } => {
                let home = self.account_home(&id)?;
                self.info(&home)
                    .await?
                    .account(id.clone())
                    .ok_or("Claude に再ログインしてください。")?;
                let previous = self.registry.selected_id.replace(id.clone());
                if let Err(error) = self.save().await {
                    self.registry.selected_id = previous;
                    return Err(error);
                }
                self.revision += 1;
                Ok(AccountSelection {
                    provider: ProviderKind::Claude,
                    selected_id: id,
                    persistence_error: None,
                }
                .into())
            }
            AccountCommand::Logout { id } => {
                if id == "claude:desktop" {
                    self.native_checked_at = None;
                }
                self.usage.remove(&id);
                let home = self.account_home(&id)?;
                if self.registry.selected_id.as_ref() == Some(&id) {
                    self.registry.selected_id = None;
                    if let Err(error) = self.save().await {
                        self.registry.selected_id = Some(id);
                        return Err(error);
                    }
                }
                self.revision += 1;
                let mut process = Cli::start(
                    &self.program,
                    super::process::homes(&self.native_home, self.native_inherited, &home)
                        .map(|(_, home)| home),
                    &["auth", "logout"],
                    None,
                )?;
                if !process.finish().await? {
                    return Err("Claude のログアウトに失敗しました。".into());
                }
                self.registry.accounts.retain(|account| account.id != id);
                self.save().await?;
                Ok(Empty {}.into())
            }
            AccountCommand::StartLogin => {
                self.cancel().await?;
                let id = format!("claude:{}", uuid::Uuid::new_v4());
                let home = self.directory.join(id.strip_prefix("claude:").unwrap());
                agent_local_host::create_state_directory(&home)
                    .map_err(|_| "Claude の保存先を作成できません。")?;
                let (sender, url) = oneshot::channel();
                let process = Cli::start(
                    &self.program,
                    Some(&home),
                    &["auth", "login", "--claudeai"],
                    Some(sender),
                )?;
                self.login = Some(Login {
                    started: Instant::now(),
                    id: id.clone(),
                    home,
                    process,
                });
                let result = tokio::time::timeout(Duration::from_secs(30), url)
                    .await
                    .map_err(|_| "Claude のログイン開始がタイムアウトしました。".to_owned())
                    .and_then(|result| {
                        result.map_err(|_| "Claude のログインを開始できません。".to_owned())
                    });
                let verification_url = match result {
                    Ok(url) => url,
                    Err(error) => {
                        self.cancel().await?;
                        return Err(error);
                    }
                };
                Ok(AccountLogin {
                    provider: ProviderKind::Claude,
                    login_id: id,
                    user_code: String::new(),
                    verification_url,
                    requires_code_submission: true,
                }
                .into())
            }
            AccountCommand::SubmitLogin { id, code } => {
                let login = self
                    .login
                    .as_mut()
                    .filter(|login| login.id == id)
                    .ok_or("ログイン手続きが一致しません。")?;
                let code = zeroize::Zeroizing::new(code);
                let code = code.trim();
                if code.is_empty() || code.len() > 4096 || code.contains(['\r', '\n']) {
                    return Err("認証コードが無効です。".into());
                }
                let input = login
                    .process
                    .input
                    .as_mut()
                    .ok_or("ログインが終了しました。")?;
                input
                    .write_all(code.as_bytes())
                    .await
                    .map_err(|_| "認証コードを送信できません。")?;
                input
                    .write_all(b"\n")
                    .await
                    .map_err(|_| "認証コードを送信できません。")?;
                input
                    .flush()
                    .await
                    .map_err(|_| "認証コードを送信できません。")?;
                Ok(Empty {}.into())
            }
            AccountCommand::ReadLogin { id } => {
                if self
                    .registry
                    .accounts
                    .iter()
                    .any(|account| account.id == id)
                {
                    return Ok(AccountLoginStatus {
                        completed: true,
                        account_id: Some(id),
                    }
                    .into());
                }
                let login = self
                    .login
                    .as_mut()
                    .filter(|login| login.id == id)
                    .ok_or("ログイン手続きが一致しません。")?;
                if login.started.elapsed() > Duration::from_secs(600) {
                    self.cancel().await?;
                    return Err(
                        "Claude のログインが期限切れになりました。もう一度お試しください。".into(),
                    );
                }
                let Some(status) = login
                    .process
                    .child
                    .try_wait()
                    .map_err(|_| "Claude の認証状態を確認できません。")?
                else {
                    return Ok(AccountLoginStatus {
                        completed: false,
                        account_id: None,
                    }
                    .into());
                };
                if !status.success() {
                    self.cancel().await?;
                    return Err("Claude のログインに失敗しました。もう一度お試しください。".into());
                }
                let home = login.home.clone();
                let account = self
                    .info(&home)
                    .await?
                    .account(id.clone())
                    .ok_or("Claude のログインが完了していません。")?;
                self.registry.accounts.push(account);
                if let Err(error) = self.save().await {
                    self.registry.accounts.pop();
                    return Err(error);
                }
                self.login = None;
                Ok(AccountLoginStatus {
                    completed: true,
                    account_id: Some(id),
                }
                .into())
            }
            AccountCommand::CancelLogin { id } => {
                if self.login.as_ref().is_some_and(|login| login.id != id) {
                    return Err("ログイン手続きが一致しません。".into());
                }
                self.cancel().await?;
                Ok(Empty {}.into())
            }
        }
    }

    pub(super) async fn cancel(&mut self) -> Result<(), String> {
        if let Some(login) = self.login.as_mut() {
            login.process.input.take();
            let _ = login.process.child.wait().await;
            let home = login.home.clone();
            // A completion can race cancellation; let the CLI remove its credentials.
            let mut process = Cli::start(&self.program, Some(&home), &["auth", "logout"], None)?;
            // A failed logout is harmless only when the SDK explicitly confirms
            // there are no credentials. Keep the login available for retry on
            // every other failure, including an unreadable status response.
            if !process.finish().await? && !self.info(&home).await?.logged_out() {
                return Err("Claude の認証手続きを破棄できません。".into());
            }
            tokio::fs::remove_dir_all(&home)
                .await
                .map_err(|_| "Claude の一時設定を削除できません。")?;
            self.login = None;
        }
        Ok(())
    }
}

impl Cli {
    fn start(
        program: &Path,
        home: Option<&Path>,
        args: &[&str],
        url: Option<oneshot::Sender<String>>,
    ) -> Result<Self, String> {
        let mut command =
            bex_process::command(program).map_err(|_| "Claude の認証処理を起動できません。")?;
        super::process::set_homes(&mut command, home.map(|home| (home, home)));
        command
            .args(args)
            .env_remove("ANTHROPIC_API_KEY")
            .env_remove("ANTHROPIC_AUTH_TOKEN")
            .env_remove("CLAUDE_CODE_OAUTH_TOKEN")
            .env_remove("CLAUDE_CODE_OAUTH_REFRESH_TOKEN")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        // The browser is opened by the requesting client, including an iPhone.
        bex_process::suppress_browser(&mut command)
            .map_err(|_| "Claude の認証処理を起動できません。")?;
        let mut child = command
            .spawn()
            .map_err(|_| "Claude の認証処理を起動できません。")?;
        let output = tokio::spawn(read_output(child.stdout.take().unwrap(), url));
        let input = child.stdin.take();
        Ok(Self {
            child,
            input,
            output,
        })
    }

    async fn finish(&mut self) -> Result<bool, String> {
        let status = tokio::time::timeout(Duration::from_secs(30), self.child.wait())
            .await
            .map_err(|_| "Claude の認証処理がタイムアウトしました。")?
            .map_err(|_| "Claude の認証処理が終了しました。")?;
        self.input.take();
        (&mut self.output)
            .await
            .map_err(|_| "Claude の認証応答を読み取れません。")??;
        Ok(status.success())
    }
}
async fn read_output(
    mut reader: impl AsyncRead + Unpin,
    mut url: Option<oneshot::Sender<String>>,
) -> Result<(), String> {
    let mut output = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        let length = reader
            .read(&mut buffer)
            .await
            .map_err(|_| "Claude の認証応答を読み取れません。")?;
        if length == 0 {
            break;
        }
        if output.len() + length > 64 * 1024 {
            return Err("Claude の認証応答が長すぎます。".into());
        }
        output.extend_from_slice(&buffer[..length]);
        if url.is_some() {
            let text = String::from_utf8_lossy(&output);
            let complete = text
                .rfind(char::is_whitespace)
                .map_or("", |end| &text[..end]);
            for word in complete.split_whitespace() {
                if let Ok(parsed) = reqwest::Url::parse(word)
                    && parsed.scheme() == "https"
                    && matches!(
                        parsed.host_str(),
                        Some(
                            "claude.com"
                                | "claude.ai"
                                | "console.anthropic.com"
                                | "platform.claude.com"
                        )
                    )
                    && parsed.query_pairs().any(|(key, _)| key == "state")
                {
                    let _ = url.take().unwrap().send(word.into());
                    break;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sdk_identity_requires_a_subscription_and_explicit_logout_requires_no_credentials() {
        for (value, subscribed, logged_out) in [
            (
                json!({"email":"account@example.invalid","subscriptionType":"max","tokenSource":"oauth","apiKeySource":"none"}),
                true,
                false,
            ),
            (json!({"subscriptionType":"max"}), true, false),
            (
                json!({"subscriptionType":"max","tokenSource":"none","apiKeySource":"none"}),
                false,
                true,
            ),
            (
                json!({"tokenSource":"none","apiKeySource":"none"}),
                false,
                true,
            ),
            (
                json!({"subscriptionType":"","tokenSource":"oauth"}),
                false,
                false,
            ),
            (
                json!({"tokenSource":"none","apiKeySource":"ANTHROPIC_API_KEY"}),
                false,
                false,
            ),
            (json!({"tokenSource":"none"}), false, false),
            (json!({}), false, false),
        ] {
            let info: Info = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(info.logged_out(), logged_out, "{value}");
            let account = info.account("claude:test".into());
            assert_eq!(account.is_some(), subscribed, "{value}");
            if let Some(account) = account {
                assert_eq!(account.id, "claude:test");
                assert_eq!(account.provider, ProviderKind::Claude);
                assert_eq!(account.email.as_deref(), value["email"].as_str());
                assert_eq!(account.plan_type.as_deref(), Some("max"));
                assert!(account.usage.is_none());
            }
        }
        for invalid in [json!(null), json!([]), json!({"tokenSource":false})] {
            assert!(serde_json::from_value::<Info>(invalid).is_err());
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn sdk_status_waits_for_usage_and_keeps_identity_when_usage_fails() {
        use std::os::unix::fs::PermissionsExt;
        tokio::time::timeout(Duration::from_secs(30), async {
            let directory = tempfile::tempdir().unwrap();
            let native = directory.path().join("native");
            std::fs::create_dir(&native).unwrap();
            let program = directory.path().join("claude.mjs");
            std::fs::write(&program, r#"#!/usr/bin/env node
import assert from 'node:assert/strict';
import { createInterface } from 'node:readline';
import { appendFileSync, existsSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
assert.ok(!process.argv.includes('auth'));
assert.equal(process.env.CLAUDE_CONFIG_DIR, process.env.CLAUDE_SECURESTORAGE_CONFIG_DIR);
const home = process.env.CLAUDE_CONFIG_DIR;
for await (const line of createInterface({ input: process.stdin })) {
  const message = JSON.parse(line);
  assert.equal(message.type, 'control_request');
  const operation = message.request.subtype;
  appendFileSync(join(home, 'operations'), `${operation}\n`);
  let response;
  if (operation === 'initialize') {
    response = { account: existsSync(join(home, 'invalid-account')) ? null : {
      email: 'sdk@example.invalid', subscriptionType: 'max', tokenSource: 'oauth', apiKeySource: 'none'
    } };
  } else {
    assert.equal(operation, 'get_usage');
    assert.equal(message.request.skip_behaviors, true);
    writeFileSync(join(home, 'usage-requested'), '');
    while (!existsSync(join(home, 'usage-release'))) await new Promise(resolve => setTimeout(resolve, 10));
    response = {};
  }
  const error = operation === 'get_usage' && existsSync(join(home, 'usage-error'));
  process.stdout.write(`${JSON.stringify({ type: 'control_response', response: {
    request_id: message.request_id, subtype: error ? 'error' : 'success',
    ...(error ? { error: 'usage unavailable' } : { response })
  } })}\n`);
}
"#).unwrap();
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
            let mut accounts = Accounts::load(program, directory.path().join("accounts"), native.clone(), false).await.unwrap();
            let mut info = Box::pin(accounts.info(&native));
            tokio::select! {
                _ = &mut info => panic!("status returned before usage completed"),
                _ = async {
                    while !native.join("usage-requested").exists() {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                } => {}
            }
            std::fs::write(native.join("usage-release"), "").unwrap();
            let account = info.await.unwrap().account("claude:desktop".into()).unwrap();
            assert_eq!(account.email.as_deref(), Some("sdk@example.invalid"));
            assert_eq!(account.plan_type.as_deref(), Some("max"));
            assert_eq!(std::fs::read_to_string(native.join("operations")).unwrap(), "initialize\nget_usage\n");

            std::fs::write(native.join("usage-error"), "").unwrap();
            let (listed, selected) = accounts.list().await.unwrap();
            assert_eq!(listed, vec![account]);
            assert_eq!(selected.as_deref(), Some("claude:desktop"));
            std::fs::write(native.join("invalid-account"), "").unwrap();
            accounts.native_checked_at = None;
            assert!(accounts.list().await.is_err());
            assert_eq!(accounts.registry.accounts, listed);
        }).await.unwrap();
    }

    #[tokio::test]
    async fn repeated_listing_reuses_native_identity_but_expiration_rechecks_it() {
        let directory = tempfile::tempdir().unwrap();
        let mut accounts = Accounts::load(
            directory.path().join("missing-claude"),
            directory.path().join("accounts"),
            directory.path().join("native"),
            false,
        )
        .await
        .unwrap();
        accounts.registry.accounts.push(Account {
            id: "claude:desktop".into(),
            provider: ProviderKind::Claude,
            email: Some("native@example.invalid".into()),
            plan_type: None,
            usage: None,
        });
        accounts.native_checked_at = Some(Instant::now());
        let (listed, selected) = accounts.list().await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(selected.as_deref(), Some("claude:desktop"));
        accounts.native_checked_at = Some(Instant::now() - Duration::from_secs(61));
        assert!(accounts.list().await.is_err());
        accounts.native_checked_at = Some(Instant::now());
        assert!(
            accounts
                .request(AccountCommand::Logout {
                    id: "claude:desktop".into()
                })
                .await
                .is_err()
        );
        assert!(accounts.native_checked_at.is_none());
    }
}
