use super::*;
use agent_core::{presentation::error::error_message, store::Store};

impl Desktop {
    pub(super) fn download(&mut self, source: String) {
        let Some(store) = self.session.as_ref().map(|session| session.store.clone()) else {
            self.set_error("Host に接続していません".into());
            return;
        };
        self.busy += 1;
        let scope = self.local_error_scope();
        self.effect(
            async move {
                let name = source
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap_or(&source)
                    .to_owned();
                let destination =
                    tokio::task::spawn_blocking(move || platform::choose_destination(&name))
                        .await
                        .map_err(|error| format!("保存先を選択できません: {error}"))?;
                if let Some(destination) = destination {
                    download_to(&store, source, destination).await?;
                }
                Ok(())
            },
            move |result| Update::LocalCompleted {
                scope,
                result: result.map(|()| None),
            },
        );
    }

    pub(super) fn open_file_with_system(&mut self, source: String) {
        let Some(store) = self.session.as_ref().map(|session| session.store.clone()) else {
            self.set_error("Host に接続していません".into());
            return;
        };
        let remote = self.remote.as_ref().map(|host| host.ticket.clone());
        let scope = self.local_error_scope();
        self.busy += 1;
        self.effect(
            async move {
                if platform::host_is_local(remote.as_deref()).await {
                    let path = PathBuf::from(source);
                    let metadata = tokio::fs::metadata(&path)
                        .await
                        .map_err(|error| format!("ファイルを開けません: {error}"))?;
                    if !metadata.is_file() {
                        return Err("ファイルを開けません: 通常のファイルではありません".into());
                    }
                    return Ok(path);
                }
                fetch_system_file(&store, source, platform::downloads::cache_directory()?).await
            },
            move |result| Update::LocalCompleted {
                scope,
                result: result.map(Some),
            },
        );
    }
}

async fn fetch_system_file(
    store: &Store,
    source: String,
    cache: PathBuf,
) -> Result<PathBuf, String> {
    let name = source.clone();
    let (directory, path) =
        tokio::task::spawn_blocking(move || platform::downloads::prepare(&cache, &name))
            .await
            .map_err(|error| format!("一時ファイルを作成できません: {error}"))?
            .map_err(|error| format!("一時ファイルを作成できません: {error}"))?;
    download_to(store, source, path.clone()).await?;
    // The viewer reads the file after this returns; retention cleanup removes it.
    let _ = directory.keep();
    Ok(path)
}

pub(super) async fn download_to(
    store: &Store,
    source: String,
    destination: PathBuf,
) -> Result<(), String> {
    store
        .dispatch(Intent::DownloadFile(op::DownloadFile {
            source,
            destination: destination
                .into_os_string()
                .into_string()
                .map_err(|_| "保存先のパスを読み取れません")?,
        }))
        .await
        .map_err(|error| {
            format!(
                "ファイルを取得・保存できません: {}",
                error_message(&error.to_string())
            )
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_kit as gpui;
    use std::time::Duration;

    #[tokio::test]
    async fn failed_system_downloads_are_removed_and_saved_files_keep_the_failure_reason() {
        let cache = tempfile::tempdir().unwrap();
        let store = Store::offline(Snapshot::default());
        let error = fetch_system_file(&store, "/movies/video.mp4".into(), cache.path().into())
            .await
            .unwrap_err();
        assert!(
            error.starts_with("ファイルを取得・保存できません: "),
            "{error}"
        );
        assert!(error.len() > "ファイルを取得・保存できません: ".len());
        assert_eq!(std::fs::read_dir(cache.path()).unwrap().count(), 0);

        let destination = cache.path().join("saved.zip");
        std::fs::write(&destination, b"existing file").unwrap();
        let saved_error = download_to(&store, "/bundle.zip".into(), destination.clone())
            .await
            .unwrap_err();
        assert_eq!(saved_error, error);
        assert_eq!(std::fs::read(&destination).unwrap(), b"existing file");
    }

    #[tokio::test]
    async fn temporary_storage_failures_are_reported_without_starting_a_transfer() {
        let cache = tempfile::NamedTempFile::new().unwrap();
        let store = Store::offline(Snapshot::default());
        let error = fetch_system_file(&store, "/video.mp4".into(), cache.path().into())
            .await
            .unwrap_err();
        assert!(
            error.starts_with("一時ファイルを作成できません: "),
            "{error}"
        );
        assert!(store.snapshot().operations.is_empty());
    }

    #[gpui::test]
    fn system_files_use_the_host_path_and_report_errors_in_the_original_conversation(
        cx: &mut TestAppContext,
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let app_runtime = Runtime {
            handle: runtime.handle().clone(),
            connections: Arc::default(),
            closing: tokio_util::task::TaskTracker::new(),
            logging_error: None,
        };
        cx.update(|cx| {
            crate::appearance::init(cx);
            cx.set_global(app_runtime.clone());
        });
        let view = cx.add_window(|window, cx| {
            Desktop::new(
                Mode::SideChat {
                    remote: Some(RemoteHost {
                        id: "fixture".into(),
                        name: "fixture".into(),
                        ticket: "invalid".into(),
                    }),
                    cwd: "/fixture".into(),
                },
                window,
                cx,
            )
        });
        let session = runtime.block_on(async {
            let (updates, incoming) = async_channel::unbounded();
            let publish = tokio::spawn(StoreSession::publish(
                Ok(Arc::new(Store::offline(Snapshot::default()))),
                app_runtime,
                updates,
                |session| session,
                |_| Err("snapshot".into()),
            ));
            let session = incoming.recv().await.unwrap().unwrap();
            publish.abort();
            session
        });
        let (updates, incoming) = async_channel::unbounded();
        view.update(cx, |view, _, _| {
            view.epoch += 1; // Ignore the fixture's initial invalid connection.
            view.remote = None;
            view.session = Some(session);
            view.updates = updates;
        })
        .unwrap();
        let directory = tempfile::tempdir().unwrap();
        for (extension, resolved_link) in [("MP4", false), ("pdf", true), ("mp3", true)] {
            let path = directory.path().join(format!("media.{extension}"));
            std::fs::write(&path, b"media").unwrap();
            view.update(cx, |view, window, cx| {
                view.panels.set_open(view.panel_scope(), false);
                if resolved_link {
                    view.operation_completed(
                        OperationCompletion::OpenFile(view.snapshot.epoch),
                        Ok(Outcome::ResolvedFile {
                            entry: agent_protocol::models::FileEntry {
                                name: format!("media.{extension}"),
                                path: path.to_str().unwrap().into(),
                                directory: false,
                                size: 5,
                            },
                        }),
                        window,
                        cx,
                    );
                } else {
                    // The file list calls this same entry point.
                    view.open_file(path.clone(), cx);
                }
                assert!(view.media.gallery.is_none());
                assert!(!view.panels.get(view.panel_scope()).open);
                assert_eq!(view.busy, 1);
            })
            .unwrap();
            let update = runtime.block_on(async {
                tokio::time::timeout(Duration::from_secs(5), incoming.recv())
                    .await
                    .unwrap()
                    .unwrap()
            });
            view.update(cx, |view, window, cx| view.receive(update, window, cx))
                .unwrap();
            assert_eq!(cx.opened_with_system().last(), Some(&path));
        }
        view.update(cx, |view, window, cx| {
            let scope = view.local_error_scope();
            view.open_file(directory.path().join("missing.mp4"), cx);
            // A later conversation must not take ownership of this error.
            runtime
                .block_on(
                    view.session
                        .as_ref()
                        .unwrap()
                        .store
                        .dispatch(Intent::NewChat {
                            cwd: "/other".into(),
                        }),
                )
                .unwrap();
            view.accept_snapshot(window, cx);
            assert_ne!(scope, view.local_error_scope());
            view.receive(
                (
                    view.epoch - 1,
                    Update::LocalCompleted {
                        scope,
                        result: Ok(Some(directory.path().join("stale.mp4"))),
                    },
                ),
                window,
                cx,
            );
        })
        .unwrap();
        let update = runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(5), incoming.recv())
                .await
                .unwrap()
                .unwrap()
        });
        view.update(cx, |view, window, cx| {
            view.receive(update, window, cx);
            assert_eq!(view.busy, 0);
            assert!(
                view.local_errors
                    .values()
                    .any(|error| error.starts_with("ファイルを開けません: "))
            );
            assert!(!view.local_errors.contains_key(&view.local_error_scope()));
        })
        .unwrap();
        assert_eq!(cx.opened_with_system().len(), 3);
    }
}
