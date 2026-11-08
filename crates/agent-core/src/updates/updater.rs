//! The desktop app's update flow: the selected channel, checking its releases,
//! downloading and verifying one, and restarting into it. The installed app is
//! replaced by `agent_local_host::install`; views observe [`UpdateState`].
use super::{Channel, Download, select_download};
use agent_local_host::{
    install::{self, Plan, UpdateResult},
    launch::HostScope,
};
use anyhow::{Context, Result, ensure};
use std::{
    path::Path,
    sync::{Arc, Mutex, Weak},
    time::Duration,
};
use tokio::{io::AsyncWriteExt, sync::watch};

const CHANNEL_FILE: &str = "update-channel.json";
const RELEASES_LIMIT: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub enum UpdatePhase {
    /// Source builds carry no release number and are never replaced.
    Development,
    Checking,
    Current,
    Available(Download),
    Downloading(u8),
    /// A verified build is staged; restarting applies it.
    Ready(u64),
    Installing,
    Failed(String),
}

impl UpdatePhase {
    /// Work in progress; the channel and the action wait for it.
    pub fn busy(&self) -> bool {
        matches!(
            self,
            Self::Checking | Self::Downloading(_) | Self::Installing
        )
    }

    /// The phase a launch starts in, from the previous update's outcome and a
    /// still staged build.
    fn restored(build: u64, result: Option<UpdateResult>, ready: Option<u64>) -> Self {
        if build == 0 {
            return Self::Development;
        }
        match result {
            Some(UpdateResult {
                build: attempted,
                error: Some(error),
            }) if attempted >= build => Self::Failed(error),
            Some(result) if result.build > build => Self::Failed(
                "更新が適用されず、以前のバージョンで起動しました。もう一度お試しください。".into(),
            ),
            _ => ready.map_or(Self::Current, Self::Ready),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct UpdateState {
    pub channel: Channel,
    pub phase: UpdatePhase,
}

/// Owns the update state of one app process. Dropping the last handle stops
/// its periodic checks.
pub struct Updater {
    build: u64,
    scope: HostScope,
    state: watch::Sender<UpdateState>,
    plan: Mutex<Option<Plan>>,
    runtime: tokio::runtime::Handle,
}

impl Updater {
    /// Restores the previous outcome, then checks shortly after launch and hourly.
    pub fn start(scope: HostScope, build: u64, runtime: tokio::runtime::Handle) -> Arc<Self> {
        let directory = &scope.directory;
        let channel = std::fs::read(directory.join(CHANNEL_FILE))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let plan = install::ready_plan(directory, build);
        let phase = UpdatePhase::restored(
            build,
            install::last_result(directory),
            plan.as_ref().map(|plan| plan.build),
        );
        let updater = Arc::new(Self {
            build,
            plan: Mutex::new(plan.filter(|_| matches!(phase, UpdatePhase::Ready(_)))),
            state: watch::channel(UpdateState { channel, phase }).0,
            scope,
            runtime,
        });
        if build > 0 {
            updater.runtime.spawn(periodic(Arc::downgrade(&updater)));
        }
        updater
    }

    pub fn subscribe(&self) -> watch::Receiver<UpdateState> {
        self.state.subscribe()
    }

    pub fn select_channel(self: &Arc<Self>, channel: Channel) {
        if self.state.borrow().phase.busy() || self.build == 0 {
            return;
        }
        let directory = &self.scope.directory;
        if let Err(error) =
            agent_local_host::save_private_json(&directory.join(CHANNEL_FILE), &channel)
        {
            self.state
                .send_modify(|state| state.phase = UpdatePhase::Failed(error.to_string()));
            return;
        }
        // A build staged from the previous channel is never applied.
        install::discard_ready(directory);
        self.plan.lock().unwrap().take();
        self.state.send_modify(|state| {
            state.channel = channel;
            state.phase = UpdatePhase::Current;
        });
        self.check();
    }

    /// Looks for a newer release on the selected channel.
    pub fn check(self: &Arc<Self>) {
        let mut channel = Channel::default();
        let started = self.state.send_if_modified(|state| {
            if !matches!(
                state.phase,
                UpdatePhase::Current | UpdatePhase::Available(_) | UpdatePhase::Failed(_)
            ) {
                return false;
            }
            channel = state.channel;
            state.phase = UpdatePhase::Checking;
            true
        });
        if !started {
            return;
        }
        install::clear_result(&self.scope.directory);
        let owner = self.clone();
        self.runtime.spawn(async move {
            let phase = match releases(channel).await.and_then(|bytes| {
                select_download(
                    &bytes,
                    owner.build,
                    std::env::consts::OS,
                    std::env::consts::ARCH,
                    channel,
                )
                .map_err(anyhow::Error::msg)
            }) {
                Ok(Some(download)) => UpdatePhase::Available(download),
                Ok(None) => UpdatePhase::Current,
                Err(error) => UpdatePhase::Failed(format!("{error:#}")),
            };
            owner.state.send_if_modified(|state| {
                let current = state.channel == channel && state.phase == UpdatePhase::Checking;
                if current {
                    state.phase = phase;
                }
                current
            });
        });
    }

    /// Downloads, verifies and stages the available release.
    pub fn download(self: &Arc<Self>) {
        let mut release = None;
        self.state.send_if_modified(|state| {
            let UpdatePhase::Available(download) = &state.phase else {
                return false;
            };
            release = Some(download.clone());
            state.phase = UpdatePhase::Downloading(0);
            true
        });
        let Some(release) = release else {
            return;
        };
        let owner = self.clone();
        self.runtime.spawn(async move {
            let progress = |percent| {
                owner.state.send_if_modified(|state| {
                    let current = matches!(state.phase, UpdatePhase::Downloading(previous) if previous != percent);
                    if current {
                        state.phase = UpdatePhase::Downloading(percent);
                    }
                    current
                });
            };
            let staged = owner.stage(&release, progress).await;
            let phase = match staged {
                Ok(plan) => {
                    let build = plan.build;
                    *owner.plan.lock().unwrap() = Some(plan);
                    UpdatePhase::Ready(build)
                }
                Err(error) => UpdatePhase::Failed(format!("{error:#}")),
            };
            owner.state.send_modify(|state| state.phase = phase);
        });
    }

    async fn stage(&self, release: &Download, progress: impl Fn(u8)) -> Result<Plan> {
        let directory = self.scope.directory.clone();
        agent_local_host::create_state_directory(&directory)?;
        let cache = tempfile::Builder::new()
            .prefix("bex-download-")
            .tempdir_in(&directory)?;
        let archive = cache.path().join(&release.file_name);
        download(release, &archive, progress).await?;
        let (build, isolated) = (release.build, self.scope.isolated);
        tokio::task::spawn_blocking(move || install::stage(build, &archive, directory, isolated))
            .await?
    }

    /// Starts the worker that replaces the app after this process exits. The
    /// caller keeps the returned lock open until exit, then quits; `None`
    /// leaves the failure in the state.
    pub async fn restart(&self) -> Option<std::fs::File> {
        let plan = self.plan.lock().unwrap().clone()?;
        let started = self.state.send_if_modified(|state| {
            let ready = state.phase == UpdatePhase::Ready(plan.build);
            if ready {
                state.phase = UpdatePhase::Installing;
            }
            ready
        });
        if !started {
            return None;
        }
        let result = tokio::task::spawn_blocking(move || install::launch(&plan))
            .await
            .map_err(anyhow::Error::from)
            .and_then(|result| result);
        match result {
            Ok(lease) => Some(lease),
            Err(error) => {
                self.state
                    .send_modify(|state| state.phase = UpdatePhase::Failed(format!("{error:#}")));
                None
            }
        }
    }
}

async fn periodic(updater: Weak<Updater>) {
    tokio::time::sleep(Duration::from_secs(10)).await;
    let Some(directory) = updater.upgrade().map(|owner| owner.scope.directory.clone()) else {
        return;
    };
    let _ = tokio::task::spawn_blocking(move || install::cleanup_workers(&directory)).await;
    loop {
        let Some(owner) = updater.upgrade() else {
            return;
        };
        if matches!(
            owner.state.borrow().phase,
            UpdatePhase::Current | UpdatePhase::Available(_)
        ) {
            owner.check();
        }
        drop(owner);
        tokio::time::sleep(Duration::from_secs(3600)).await;
    }
}

fn http() -> Result<reqwest::Client> {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    Ok(reqwest::Client::builder()
        .user_agent("Bex-Updater")
        .build()?)
}

async fn releases(channel: Channel) -> Result<Vec<u8>> {
    let mut response = http()?
        .get(channel.releases_url())
        .header("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(30))
        .send()
        .await?;
    ensure!(
        response.status().is_success(),
        "更新の確認に失敗しました (HTTP {})。もう一度お試しください。",
        response.status()
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        bytes.extend_from_slice(&chunk);
        ensure!(bytes.len() <= RELEASES_LIMIT, "更新情報が大きすぎます。");
    }
    Ok(bytes)
}

async fn download(release: &Download, path: &Path, progress: impl Fn(u8)) -> Result<()> {
    let mut response = http()?
        .get(&release.url)
        .timeout(Duration::from_secs(15 * 60))
        .send()
        .await?;
    ensure!(
        response.status().is_success(),
        "更新ファイルをダウンロードできません (HTTP {})。",
        response.status()
    );
    let mut file = Verified::create(release, path).await?;
    while let Some(chunk) = response.chunk().await? {
        file.write(&chunk).await?;
        progress(file.percent());
    }
    file.finish().await
}

/// A download that matches its published size and digest before it is kept.
struct Verified<'a> {
    release: &'a Download,
    file: tokio::fs::File,
    hash: ring::digest::Context,
    received: u64,
}

impl<'a> Verified<'a> {
    async fn create(release: &'a Download, path: &Path) -> Result<Self> {
        Ok(Self {
            release,
            file: tokio::fs::File::create(path)
                .await
                .context("更新ファイルを保存できません。")?,
            hash: ring::digest::Context::new(&ring::digest::SHA256),
            received: 0,
        })
    }

    async fn write(&mut self, bytes: &[u8]) -> Result<()> {
        self.received += bytes.len() as u64;
        ensure!(
            self.received <= self.release.size,
            "更新ファイルのサイズが一致しません。"
        );
        self.hash.update(bytes);
        self.file.write_all(bytes).await?;
        Ok(())
    }

    fn percent(&self) -> u8 {
        (self.received.saturating_mul(100) / self.release.size.max(1)) as u8
    }

    async fn finish(self) -> Result<()> {
        let digest: String = self
            .hash
            .finish()
            .as_ref()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        ensure!(
            self.received == self.release.size && digest == self.release.sha256,
            "更新ファイルの検証に失敗しました。もう一度ダウンロードしてください。"
        );
        self.file.sync_all().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release() -> Download {
        Download {
            channel: Channel::Production,
            build: 95,
            file_name: "update.zip".into(),
            url: "https://example.test/update.zip".into(),
            size: 3,
            // SHA-256 of "abc".
            sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into(),
        }
    }

    #[tokio::test]
    async fn downloads_are_kept_only_with_the_published_size_and_digest() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("update.zip");
        let release = release();
        for (chunks, success) in [
            (vec!["ab"], false),
            (vec!["ab", "cd"], false),
            (vec!["bad"], false),
            (vec!["a", "bc"], true),
        ] {
            let mut file = Verified::create(&release, &path).await.unwrap();
            let mut result = Ok(());
            for chunk in chunks {
                result = file.write(chunk.as_bytes()).await;
                if result.is_err() {
                    break;
                }
            }
            let result = match result {
                Ok(()) => file.finish().await,
                Err(error) => Err(error),
            };
            assert_eq!(result.is_ok(), success);
        }
        assert_eq!(std::fs::read(path).unwrap(), b"abc");
    }

    #[test]
    fn a_launch_reports_the_previous_outcome_before_offering_a_staged_build() {
        let result = |build, error: Option<&str>| {
            Some(UpdateResult {
                build,
                error: error.map(Into::into),
            })
        };
        assert_eq!(
            UpdatePhase::restored(0, result(95, Some("failed")), Some(95)),
            UpdatePhase::Development
        );
        assert_eq!(
            UpdatePhase::restored(94, result(95, Some("failed")), Some(95)),
            UpdatePhase::Failed("failed".into())
        );
        assert_eq!(
            UpdatePhase::restored(95, result(95, Some("conversations")), None),
            UpdatePhase::Failed("conversations".into()),
            "an applied update still reports what it could not resume"
        );
        assert!(matches!(
            UpdatePhase::restored(94, result(95, None), Some(95)),
            UpdatePhase::Failed(_)
        ));
        assert_eq!(
            UpdatePhase::restored(95, result(95, None), None),
            UpdatePhase::Current
        );
        assert_eq!(
            UpdatePhase::restored(95, result(94, Some("old")), Some(96)),
            UpdatePhase::Ready(96)
        );
        assert_eq!(UpdatePhase::restored(95, None, None), UpdatePhase::Current);
    }

    #[tokio::test]
    async fn source_builds_never_check_and_a_channel_choice_is_restored() {
        let directory = tempfile::tempdir().unwrap();
        let scope = HostScope {
            directory: directory.path().into(),
            isolated: true,
        };
        let runtime = tokio::runtime::Handle::current();
        let development = Updater::start(scope.clone(), 0, runtime.clone());
        development.check();
        development.select_channel(Channel::Nightly);
        assert_eq!(
            *development.subscribe().borrow(),
            UpdateState {
                channel: Channel::Production,
                phase: UpdatePhase::Development
            }
        );
        agent_local_host::save_private_json(
            &directory.path().join(CHANNEL_FILE),
            &Channel::Nightly,
        )
        .unwrap();
        let release = Updater::start(scope, 95, runtime);
        let state = release.subscribe().borrow().clone();
        assert_eq!(state.channel, Channel::Nightly);
        assert_eq!(state.phase, UpdatePhase::Current);
        // Without a staged build there is nothing to restart into.
        assert!(release.restart().await.is_none());
        release.download();
        assert_eq!(release.subscribe().borrow().phase, UpdatePhase::Current);
    }
}
