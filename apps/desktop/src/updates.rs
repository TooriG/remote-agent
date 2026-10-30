mod install;

use agent_core::updates::{Download, RELEASES_URL, select_download};
use futures_util::AsyncReadExt;
use gpui_kit::http_client::{AsyncBody, HttpClient, HttpRequestExt, Request};
use gpui_kit::{
    component::{
        button::{Button, ButtonVariants},
        *,
    },
    prelude::FluentBuilder,
    *,
};
use std::{path::Path, sync::Arc, time::Duration};
use tokio::io::AsyncWriteExt;

enum State {
    Checking,
    Current,
    Available(Download),
    Downloading(u8),
    Ready(install::Plan),
    Installing,
    Failed(String),
}

pub struct Updates {
    state: State,
}

pub fn build_number() -> u64 {
    option_env!("AGENT_BUILD_NUMBER")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0)
}

/// Handle updater workers before starting GPUI or connecting to the Host.
pub fn run_command() -> bool {
    let mut args = std::env::args_os().skip(1);
    match args.next().as_deref().and_then(|s| s.to_str()) {
        Some("--bex-build-number") => {
            println!("{}", build_number());
            true
        }
        Some("--bex-apply-update") => {
            if let Some(path) = args.next() {
                let runtime = tokio::runtime::Runtime::new().expect("start updater runtime");
                if let Err(error) = runtime.block_on(install::apply(Path::new(&path))) {
                    eprintln!(
                        "Bex update: {}",
                        agent_transport::diagnostics::sanitize(&format!("{error:#}"))
                    );
                }
            }
            true
        }
        _ => false,
    }
}

impl Updates {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let directory = crate::platform::state_dir().ok();
        let result = directory
            .as_ref()
            .and_then(|dir| std::fs::read(dir.join(install::RESULT_FILE)).ok())
            .and_then(|bytes| serde_json::from_slice::<install::UpdateResult>(&bytes).ok());
        let ready = directory
            .as_ref()
            .and_then(|dir| std::fs::read(dir.join(install::READY_FILE)).ok())
            .and_then(|bytes| serde_json::from_slice::<install::Plan>(&bytes).ok())
            .filter(|plan| {
                plan.build > build_number()
                    && plan.payload.exists()
                    && std::env::current_exe().ok().as_ref() == Some(&plan.executable)
            });
        let state = match result {
            Some(install::UpdateResult {
                build,
                error: Some(error),
            }) if build >= build_number() => State::Failed(error),
            Some(result) if result.build > build_number() => State::Failed(
                "更新が適用されず、以前のバージョンで起動しました。もう一度お試しください。".into(),
            ),
            _ => ready.map_or(State::Current, State::Ready),
        };
        cx.spawn(async move |view, cx| {
            cx.background_executor()
                .timer(Duration::from_secs(10))
                .await;
            if let Some(directory) = directory {
                cx.background_executor()
                    .spawn(async move { install::cleanup_workers(&directory) })
                    .await;
            }
            loop {
                if view
                    .update(cx, |view, cx| {
                        if build_number() != 0
                            && matches!(view.state, State::Current | State::Available(_))
                        {
                            view.check(cx);
                        }
                    })
                    .is_err()
                {
                    break;
                }
                cx.background_executor()
                    .timer(Duration::from_secs(3600))
                    .await;
            }
        })
        .detach();
        Self { state }
    }

    fn check(&mut self, cx: &mut Context<Self>) {
        if let Ok(directory) = crate::platform::state_dir() {
            let _ = std::fs::remove_file(directory.join(install::RESULT_FILE));
        }
        if build_number() == 0 {
            self.state = State::Failed(
                "開発版ではアプリ内更新を利用できません。配布版のBexから更新してください。".into(),
            );
            cx.notify();
            return;
        }
        self.state = State::Checking;
        cx.notify();
        let http = cx.http_client();
        let work = cx.global::<crate::Runtime>().handle.spawn(async move {
            let mut response = http
                .send(
                    Request::builder()
                        .uri(RELEASES_URL)
                        .header("User-Agent", "Bex-Updater")
                        .header("Accept", "application/vnd.github+json")
                        .timeout(Duration::from_secs(30))
                        .body(AsyncBody::empty())?,
                )
                .await?;
            anyhow::ensure!(
                response.status().is_success(),
                "更新の確認に失敗しました (HTTP {})。もう一度お試しください。",
                response.status()
            );
            let mut bytes = Vec::new();
            response
                .body_mut()
                .take(4 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .await?;
            anyhow::ensure!(bytes.len() <= 4 * 1024 * 1024, "更新情報が大きすぎます。");
            select_download(
                &bytes,
                build_number(),
                std::env::consts::OS,
                std::env::consts::ARCH,
            )
            .map_err(anyhow::Error::msg)
        });
        cx.spawn(async move |view, cx| {
            let result = work.await.map_err(anyhow::Error::from).and_then(|r| r);
            let _ = view.update(cx, |view, cx| {
                view.state = match result {
                    Ok(Some(download)) => State::Available(download),
                    Ok(None) => State::Current,
                    Err(error) => State::Failed(format!("{error:#}")),
                };
                cx.notify();
            });
        })
        .detach();
    }

    fn download(&mut self, release: Download, cx: &mut Context<Self>) {
        self.state = State::Downloading(0);
        cx.notify();
        let http = cx.http_client();
        let (progress, receiver) = async_channel::bounded(1);
        let work = cx.global::<crate::Runtime>().handle.spawn(async move {
            let directory = crate::platform::state_dir().map_err(anyhow::Error::msg)?;
            agent_local_host::create_state_directory(&directory)?;
            let cache = tempfile::Builder::new()
                .prefix("bex-download-")
                .tempdir_in(&directory)?;
            let archive = cache.path().join(&release.file_name);
            download(http, &release, &archive, progress).await?;
            tokio::task::spawn_blocking(move || {
                install::stage(
                    &release,
                    &archive,
                    directory,
                    crate::platform::isolated_host()?,
                )
            })
            .await?
        });
        cx.spawn(async move |view, cx| {
            while let Ok(percent) = receiver.recv().await {
                if view
                    .update(cx, |view, cx| {
                        view.state = State::Downloading(percent);
                        cx.notify();
                    })
                    .is_err()
                {
                    return;
                }
            }
            let result = work.await.map_err(anyhow::Error::from).and_then(|r| r);
            let _ = view.update(cx, |view, cx| {
                view.state = match result {
                    Ok(plan) => State::Ready(plan),
                    Err(error) => State::Failed(format!("{error:#}")),
                };
                cx.notify();
            });
        })
        .detach();
    }

    fn activate(&mut self, cx: &mut Context<Self>) {
        match &self.state {
            State::Available(download) => self.download(download.clone(), cx),
            State::Ready(plan) => {
                let plan = plan.clone();
                self.state = State::Installing;
                cx.notify();
                let work = cx
                    .background_executor()
                    .spawn(async move { install::launch(&plan) });
                cx.spawn(async move |view, cx| {
                    let result = work.await;
                    let _ = view.update(cx, |view, cx| match result {
                        Ok(lease) => {
                            // GPUI drops views before main finishes shutting down.
                            // Keep this single lock open until the OS exits the
                            // process, so the worker also waits for Windows to
                            // release the installed executable and directory.
                            std::mem::forget(lease);
                            cx.quit();
                        }
                        Err(error) => {
                            view.state = State::Failed(format!("{error:#}"));
                            cx.notify();
                        }
                    });
                })
                .detach();
            }
            State::Current | State::Failed(_) => self.check(cx),
            _ => {}
        }
    }
}

impl Render for Updates {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (label, tooltip, disabled) = match &self.state {
            State::Checking => (
                "更新を確認中…".into(),
                "最新版を確認しています。".into(),
                true,
            ),
            State::Current => (
                "更新を確認".into(),
                "Bexの最新版を確認します。".into(),
                false,
            ),
            State::Available(download) => (
                "更新をダウンロード".into(),
                format!("Bex {}をダウンロードします。", download.build),
                false,
            ),
            State::Downloading(percent) => (
                format!("ダウンロード中 {percent}%"),
                "ダウンロードと検証が完了するまで、このまま作業できます。".into(),
                true,
            ),
            State::Ready(plan) => (
                "再起動して更新".into(),
                format!(
                    "Bex {}に更新します。中断した会話は同じ会話で再開します。",
                    plan.build
                ),
                false,
            ),
            State::Installing => (
                "再起動の準備中…".into(),
                "会話と設定を保存しています。".into(),
                true,
            ),
            State::Failed(error) => ("更新を再試行".into(), error.clone(), false),
        };
        v_flex()
            .w_full()
            .gap_1()
            .child(
                Button::new("app-update")
                    .debug_selector(|| "app-update".into())
                    .small()
                    .ghost()
                    .w_full()
                    .icon(IconName::RotateCw)
                    .label(label.clone())
                    .accessibility_label(label)
                    .tooltip(tooltip)
                    .disabled(disabled)
                    .on_click(cx.listener(|view, _, _, cx| view.activate(cx))),
            )
            .when_some(
                match &self.state {
                    State::Failed(error) => Some(error.clone()),
                    _ => None,
                },
                |view, error| view.child(div().text_xs().px_2().child(error)),
            )
    }
}

async fn download(
    http: Arc<dyn HttpClient>,
    release: &Download,
    path: &Path,
    progress: async_channel::Sender<u8>,
) -> anyhow::Result<()> {
    let mut response = http
        .send(
            Request::builder()
                .uri(&release.url)
                .header("User-Agent", "Bex-Updater")
                .follow_redirects(gpui_kit::http_client::RedirectPolicy::FollowLimit(5))
                .timeout(Duration::from_secs(15 * 60))
                .body(AsyncBody::empty())?,
        )
        .await?;
    anyhow::ensure!(
        response.status().is_success(),
        "更新ファイルをダウンロードできません (HTTP {})。",
        response.status()
    );
    let mut file = tokio::fs::File::create(path).await?;
    let mut hash = ring::digest::Context::new(&ring::digest::SHA256);
    let mut buffer = vec![0; 64 * 1024];
    let mut received = 0;
    loop {
        let count = response.body_mut().read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        received += count as u64;
        anyhow::ensure!(
            received <= release.size,
            "更新ファイルのサイズが一致しません。"
        );
        hash.update(&buffer[..count]);
        file.write_all(&buffer[..count]).await?;
        let _ = progress.try_send((received.saturating_mul(100) / release.size) as u8);
    }
    let digest: String = hash
        .finish()
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    anyhow::ensure!(
        received == release.size && digest == release.sha256,
        "更新ファイルの検証に失敗しました。もう一度ダウンロードしてください。"
    );
    file.sync_all().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Download, State, Updates, download};
    use gpui_kit as gpui;
    use gpui_kit::{
        Modifiers,
        http_client::{AsyncBody, FakeHttpClient, Response},
    };

    #[tokio::test]
    async fn downloads_are_checked_before_they_can_be_installed_and_can_be_retried() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("update.zip");
        let release = Download {
            build: 95,
            file_name: "update.zip".into(),
            url: "https://example.test/update.zip".into(),
            size: 3,
            sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into(),
        };
        for (body, status, success) in [
            ("ab", 200, false),
            ("abcd", 200, false),
            ("bad", 200, false),
            ("abc", 500, false),
            ("abc", 200, true),
        ] {
            let http = FakeHttpClient::create(move |request| async move {
                assert_eq!(request.uri(), "https://example.test/update.zip");
                Ok(Response::builder()
                    .status(status)
                    .body(AsyncBody::from(body))
                    .unwrap())
            });
            let (progress, _receiver) = async_channel::bounded(1);
            assert_eq!(
                download(http, &release, &path, progress).await.is_ok(),
                success
            );
        }
        assert_eq!(std::fs::read(path).unwrap(), b"abc");
    }

    #[gpui::test]
    fn progress_buttons_ignore_repeated_clicks(cx: &mut gpui_kit::TestAppContext) {
        cx.update(crate::appearance::init);
        let (view, window) = cx.add_window_view(|_, _| Updates {
            state: State::Downloading(50),
        });
        window.run_until_parked();
        let bounds = window.debug_bounds("app-update").unwrap();
        window.simulate_click(bounds.center(), Modifiers::default());
        window.run_until_parked();
        view.update(window, |view, cx| {
            assert!(matches!(view.state, State::Downloading(50)));
            view.state = State::Installing;
            cx.notify();
        });
        window.run_until_parked();
        let bounds = window.debug_bounds("app-update").unwrap();
        window.simulate_click(bounds.center(), Modifiers::default());
        window.run_until_parked();
        view.update(window, |view, _| {
            assert!(matches!(view.state, State::Installing));
        });
    }
}
