mod install;

use agent_core::updates::{Download, RELEASES_URL, select_download};
use futures_util::AsyncReadExt;
use gpui_kit::http_client::{AsyncBody, HttpClient, HttpRequestExt, Request};
use gpui_kit::{
    component::{
        button::{Button, ButtonVariants},
        progress::ProgressCircle,
        spinner::Spinner,
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

pub struct RefreshThreads;
impl EventEmitter<RefreshThreads> for Updates {}

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
                        if matches!(view.state, State::Current | State::Available(_)) {
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
        if build_number() == 0 {
            return;
        }
        if let Ok(directory) = crate::platform::state_dir() {
            let _ = std::fs::remove_file(directory.join(install::RESULT_FILE));
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
            State::Current | State::Failed(_) => {
                cx.emit(RefreshThreads);
                self.check(cx);
            }
            _ => {}
        }
    }
}

impl Render for Updates {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let language = crate::localization::language(cx);
        let (icon, tooltip, disabled) = match &self.state {
            State::Checking => (
                Icon::new(IconName::RotateCw),
                language.text("更新を確認中…").into(),
                true,
            ),
            State::Current => (
                Icon::new(IconName::RotateCw),
                language.text("会話を更新").into(),
                false,
            ),
            State::Available(download) => (
                Icon::default().path("icons/download.svg"),
                agent_core::localization::localized_ui_format(
                    language,
                    "Bex {0}をダウンロードします。".into(),
                    vec![download.build.to_string()],
                ),
                false,
            ),
            State::Downloading(percent) => (
                Icon::default().path("icons/download.svg"),
                agent_core::localization::localized_ui_format(
                    language,
                    "ダウンロード中 {0}%".into(),
                    vec![percent.to_string()],
                ),
                true,
            ),
            State::Ready(plan) => (
                Icon::new(IconName::RotateCw),
                agent_core::localization::localized_ui_format(
                    language,
                    "再起動してBex {0}に更新します。中断した会話は同じ会話で再開します。".into(),
                    vec![plan.build.to_string()],
                ),
                false,
            ),
            State::Installing => (
                Icon::new(IconName::RotateCw),
                language.text("再起動の準備中…").into(),
                true,
            ),
            State::Failed(error) => (
                Icon::new(IconName::RotateCw),
                agent_core::localization::localized_ui_format(
                    language,
                    "更新を再試行: {0}".into(),
                    vec![agent_core::localization::localized_ui_text(
                        language,
                        error.to_string(),
                    )],
                ),
                false,
            ),
        };
        let icon = if matches!(self.state, State::Checking | State::Installing) {
            Spinner::new().icon(icon).small().into_any_element()
        } else {
            icon.small().into_any_element()
        };
        let foreground = cx.theme().foreground;
        let background = cx.theme().sidebar;
        let content = h_flex()
            .relative()
            .size_6()
            .justify_center()
            .child(icon)
            .when(matches!(self.state, State::Available(_)), |content| {
                content.child(
                    div()
                        .absolute()
                        .top(px(2.))
                        .right(px(2.))
                        .size(px(5.))
                        .rounded_full()
                        .bg(foreground),
                )
            })
            .when(matches!(self.state, State::Ready(_)), |content| {
                content.child(
                    h_flex()
                        .absolute()
                        .bottom_0()
                        .right_0()
                        .size(px(9.))
                        .justify_center()
                        .rounded_full()
                        .bg(foreground)
                        .text_color(background)
                        .child(Icon::new(IconName::Check).size(px(7.))),
                )
            })
            .when_some(
                match self.state {
                    State::Downloading(percent) => Some(percent),
                    _ => None,
                },
                |content, percent| {
                    content.child(
                        ProgressCircle::new("update-download-progress")
                            .absolute()
                            .inset_0()
                            .size_full()
                            .color(foreground)
                            .value(f32::from(percent))
                            .accessibility_label(language.text("更新のダウンロード進捗")),
                    )
                },
            );
        Button::new("refresh-threads")
            .debug_selector(|| "refresh-threads".into())
            .small()
            .ghost()
            .size_6()
            .p_0()
            .child(content)
            .accessibility_label(tooltip.clone())
            .tooltip(tooltip)
            .disabled(disabled)
            .on_click(cx.listener(|view, _, _, cx| view.activate(cx)))
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
    use super::{Download, RefreshThreads, State, Updates, build_number, download, install};
    use gpui_kit as gpui;
    use gpui_kit::{
        Modifiers,
        http_client::{AsyncBody, FakeHttpClient, Response},
    };
    use std::{cell::Cell, rc::Rc, sync::Arc};

    fn release() -> Download {
        Download {
            build: 95,
            file_name: "update.zip".into(),
            url: "https://example.test/update.zip".into(),
            size: 3,
            sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into(),
        }
    }

    fn plan(directory: std::path::PathBuf) -> install::Plan {
        install::Plan {
            build: 95,
            payload: "payload".into(),
            destination: "destination".into(),
            backup: "backup".into(),
            executable: "executable".into(),
            directory,
            isolated: true,
        }
    }

    #[tokio::test]
    async fn downloads_are_checked_before_they_can_be_installed_and_can_be_retried() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("update.zip");
        let release = release();
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
    fn the_refresh_control_keeps_its_size_through_update_states(cx: &mut gpui::TestAppContext) {
        cx.update(crate::appearance::init);
        let (view, window) = cx.add_window_view(|_, _| Updates {
            state: State::Current,
        });
        window.run_until_parked();
        let original = window.debug_bounds("refresh-threads").unwrap();
        assert_eq!(original.size, gpui::size(gpui::px(24.), gpui::px(24.)));
        for state in [
            State::Checking,
            State::Available(release()),
            State::Downloading(50),
            State::Ready(plan("directory".into())),
            State::Installing,
            State::Failed("Download failed".into()),
        ] {
            view.update(window, |view, cx| {
                view.state = state;
                cx.notify();
            });
            window.run_until_parked();
            assert_eq!(window.debug_bounds("refresh-threads").unwrap(), original);
            assert!(window.debug_bounds("app-update").is_none());
        }
    }

    #[gpui::test]
    fn the_same_button_refreshes_chats_then_downloads_an_available_update(
        cx: &mut gpui::TestAppContext,
    ) {
        // Keep network work parked; this test exercises click dispatch only.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        cx.update(|cx| {
            crate::appearance::init(cx);
            cx.set_global(crate::Runtime {
                handle: runtime.handle().clone(),
                connections: Arc::default(),
                closing: tokio_util::task::TaskTracker::new(),
                logging_error: None,
            });
        });
        let (view, window) = cx.add_window_view(|_, _| Updates {
            state: State::Current,
        });
        let refreshes = Rc::new(Cell::new(0));
        let received = refreshes.clone();
        let _subscription = window.update(|_, cx| {
            cx.subscribe(&view, move |_, _: &RefreshThreads, _| {
                received.set(received.get() + 1);
            })
        });
        window.run_until_parked();
        let bounds = window.debug_bounds("refresh-threads").unwrap();
        window.simulate_click(bounds.center(), Modifiers::default());
        window.run_until_parked();
        assert_eq!(refreshes.get(), 1);
        if build_number() == 0 {
            view.update(window, |view, _| {
                assert!(matches!(view.state, State::Current))
            });
        }
        view.update(window, |view, cx| {
            view.state = State::Failed("Connection interrupted".into());
            cx.notify();
        });
        window.run_until_parked();
        window.simulate_click(bounds.center(), Modifiers::default());
        window.run_until_parked();
        assert_eq!(refreshes.get(), 2);

        view.update(window, |view, cx| {
            view.state = State::Available(release());
            cx.notify();
        });
        window.run_until_parked();
        window.simulate_click(bounds.center(), Modifiers::default());
        window.run_until_parked();
        view.update(window, |view, _| {
            assert!(matches!(view.state, State::Downloading(0)));
        });
        assert_eq!(refreshes.get(), 2);

        // A failed restart leaves the same control usable for a retry. The
        // missing directory prevents launching a worker or touching an app.
        let directory = tempfile::tempdir().unwrap();
        view.update(window, |view, cx| {
            view.state = State::Ready(plan(directory.path().join("missing")));
            cx.notify();
        });
        window.run_until_parked();
        window.simulate_click(bounds.center(), Modifiers::default());
        window.run_until_parked();
        view.update(window, |view, _| {
            assert!(matches!(view.state, State::Failed(_)));
        });
        assert_eq!(window.debug_bounds("refresh-threads").unwrap(), bounds);
        assert_eq!(refreshes.get(), 2);
    }

    #[gpui::test]
    fn progress_buttons_ignore_repeated_clicks(cx: &mut gpui_kit::TestAppContext) {
        cx.update(crate::appearance::init);
        let (view, window) = cx.add_window_view(|_, _| Updates {
            state: State::Downloading(50),
        });
        window.run_until_parked();
        let bounds = window.debug_bounds("refresh-threads").unwrap();
        window.simulate_click(bounds.center(), Modifiers::default());
        window.run_until_parked();
        view.update(window, |view, cx| {
            assert!(matches!(view.state, State::Downloading(50)));
            view.state = State::Installing;
            cx.notify();
        });
        window.run_until_parked();
        let bounds = window.debug_bounds("refresh-threads").unwrap();
        window.simulate_click(bounds.center(), Modifiers::default());
        window.run_until_parked();
        view.update(window, |view, _| {
            assert!(matches!(view.state, State::Installing));
        });
    }
}
