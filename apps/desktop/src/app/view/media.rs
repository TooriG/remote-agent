mod asset;
use super::*;
use asset::{ImageAsset, ImageId};
use asset::{ImageKey, ImagePreview};

#[derive(Default)]
pub(in crate::app) struct MediaSource {
    pub(in crate::app) host: String,
    pub(in crate::app) cwd: String,
    pub(in crate::app) remote: bool,
    pub(in crate::app) session: Option<SessionRef>,
}

pub(in crate::app) enum MediaEvent {
    Visibility,
    Error(ErrorSource),
}

pub(in crate::app) struct Media {
    source: MediaSource,
    store: Option<Arc<agent_core::store::Store>>,
    pane: Option<Arc<agent_core::store::Pane>>,
    handle: tokio::runtime::Handle,
    images: HashMap<ImageId, Entity<ImageAsset>>,
    pub(in crate::app) gallery: Option<ImageGallery>,
    image_dir: Arc<tempfile::TempDir>,
    side_chat_mode: bool,
}
impl EventEmitter<MediaEvent> for Media {}

pub(in crate::app) struct ImageGallery {
    zoom: f32,
    id: uuid::Uuid,
    entries: Vec<(Arc<String>, bool)>,
    initial: (Arc<String>, bool),
    origin: ImageKey,
    selected: Option<usize>,
    list: ListState,
    loading: bool,
    saving: bool,
    saved: bool,
    error: Option<ErrorSource>,
    image: Option<(Entity<ImageAsset>, Subscription)>,
}
impl ImageGallery {
    pub(in crate::app) fn current_image(&self) -> &(Arc<String>, bool) {
        self.selected
            .and_then(|index| self.entries.get(index))
            .unwrap_or(&self.initial)
    }
    fn current_key(&self) -> ImageKey {
        let (source, encoded) = self.current_image();
        ImageKey {
            source: source.clone(),
            encoded: *encoded,
            ..self.origin.clone()
        }
    }
}

impl Media {
    pub(in crate::app) fn new(
        handle: tokio::runtime::Handle,
        side_chat_mode: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe_global::<crate::localization::Language>(|_, cx| cx.notify())
            .detach();

        Self {
            source: MediaSource::default(),
            store: None,
            pane: None,
            handle,
            images: HashMap::new(),
            gallery: None,
            side_chat_mode,
            image_dir: Arc::new(
                tempfile::Builder::new()
                    .prefix("agent-images-")
                    .tempdir()
                    .expect("image temporary directory"),
            ),
        }
    }
    pub(in crate::app) fn sync(
        &mut self,
        source: MediaSource,
        store: Option<Arc<agent_core::store::Store>>,
        pane: Option<Arc<agent_core::store::Pane>>,
        cx: &mut Context<Self>,
    ) {
        if self.source.host != source.host {
            self.reset(cx);
        }
        self.source = source;
        self.store = store;
        self.pane = pane;
    }
    pub(in crate::app) fn reset(&mut self, cx: &mut Context<Self>) {
        self.images.clear();
        self.close_gallery(cx);
    }
    fn key(&self, source: &str, encoded: bool) -> ImageKey {
        ImageKey {
            host: self.source.host.clone(),
            cwd: self.source.cwd.clone(),
            source: Arc::new(source.to_owned()),
            encoded,
        }
    }
    #[cfg(test)]
    pub(in crate::app) fn fixture_image(
        &mut self,
        source: &str,
        path: ImageSource,
        cx: &mut Context<Self>,
    ) {
        let key = self.key(source, false);
        let image = cx.new(|_| ImageAsset::fixture(key.clone(), path, self.image_dir.clone()));
        self.images.insert(key.id(), image);
    }
    #[cfg(test)]
    pub(in crate::app) fn asset(
        &mut self,
        source: &str,
        encoded: bool,
        cx: &mut Context<Self>,
    ) -> Entity<ImageAsset> {
        self.asset_key(self.key(source, encoded), cx)
    }
    fn asset_key(&mut self, key: ImageKey, cx: &mut Context<Self>) -> Entity<ImageAsset> {
        self.images
            .entry(key.id())
            .or_insert_with(|| {
                cx.new(|cx| {
                    ImageAsset::new(
                        key,
                        self.source.remote,
                        self.store.clone(),
                        self.image_dir.clone(),
                        &self.handle,
                        cx,
                    )
                })
            })
            .clone()
    }
    pub(in crate::app) fn preview(
        media: &Entity<Self>,
        id: impl Into<SharedString>,
        source: &str,
        encoded: bool,
        height: f32,
        clickable: bool,
        cx: &mut App,
    ) -> AnyElement {
        media.update(cx, |media, cx| {
            // Cache identity contains a digest, not the image payload. Repainting
            // an existing image must not allocate/copy a base64 source string.
            let key = ImageId::new(&media.source.host, &media.source.cwd, source, encoded);
            let asset = media
                .images
                .get(&key)
                .cloned()
                .unwrap_or_else(|| media.asset_key(media.key(source, encoded), cx));
            media.preview_asset(id, asset, height, clickable, cx)
        })
    }
    fn preview_asset(
        &self,
        id: impl Into<SharedString>,
        asset: Entity<ImageAsset>,
        height: f32,
        clickable: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        ImagePreview {
            id: id.into(),
            media: cx.entity().downgrade(),
            asset,
            session: self.source.session.clone(),
            height,
            clickable,
        }
        .into_any_element()
    }
    #[cfg(test)]
    pub(in crate::app) fn open(&mut self, source: &str, encoded: bool, cx: &mut Context<Self>) {
        self.open_gallery(self.key(source, encoded), self.source.session.clone(), cx);
    }
    fn open_gallery(
        &mut self,
        origin: ImageKey,
        session: Option<SessionRef>,
        cx: &mut Context<Self>,
    ) {
        if origin.host != self.source.host {
            return;
        }
        let was_open = self.gallery.is_some();
        let id = uuid::Uuid::new_v4();
        self.gallery = Some(ImageGallery {
            zoom: 1.,
            id,
            entries: Vec::new(),
            initial: (origin.source.clone(), origin.encoded),
            origin,
            selected: None,
            list: ListState::new(0, ListAlignment::Top, px(160.)),
            loading: session.is_some(),
            saving: false,
            saved: false,
            error: None,
            image: None,
        });
        if let Some(thread_id) = session
            && let Some(pane) = &self.pane
        {
            let receipt = pane.dispatch(Intent::LoadSessionImages(op::LoadSessionImages {
                thread_id,
            }));
            let load = self
                .handle
                .spawn(async move { receipt.await.map_err(|error| error.to_string()) });
            cx.spawn(async move |media, cx| {
                let result = load.await.unwrap_or_else(|error| Err(error.to_string()));
                let _ = media.update(cx, |media, cx| media.gallery_loaded(id, result, cx));
            })
            .detach();
        }
        if !was_open {
            cx.emit(MediaEvent::Visibility);
        }
        cx.notify();
    }
    pub(super) fn close_gallery(&mut self, cx: &mut Context<Self>) {
        if self.gallery.take().is_some() {
            cx.emit(MediaEvent::Visibility);
            cx.notify();
        }
    }
    fn gallery_loaded(
        &mut self,
        id: uuid::Uuid,
        result: Result<Outcome, String>,
        cx: &mut Context<Self>,
    ) {
        let Some(gallery) = self.gallery.as_mut().filter(|gallery| gallery.id == id) else {
            return;
        };
        gallery.loading = false;
        match result {
            Ok(Outcome::SessionImages { images }) => {
                let initial = gallery.current_image().clone();
                let entries: Vec<_> = images
                    .into_iter()
                    .map(|image| (Arc::new(image.source), image.encoded))
                    .collect();
                gallery.selected = entries.iter().position(|entry| entry == &initial);
                gallery
                    .list
                    .splice(0..gallery.list.item_count(), entries.len());
                gallery.entries = entries;
                if let Some(index) = gallery.selected {
                    gallery.list.scroll_to_reveal_item(index);
                }
            }
            Err(error) => {
                tracing::error!(target: "agent", operation = "gallery.load", message = %error);
                gallery.error = Some(error.into());
            }
            _ => unreachable!("session image outcome"),
        }
        cx.notify();
    }
    fn gallery_thumbnail(&mut self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let language = crate::localization::language(cx);
        let Some(gallery) = &self.gallery else {
            return div().into_any_element();
        };
        let Some((source, encoded)) = gallery.entries.get(index).cloned() else {
            return div().into_any_element();
        };
        let selected = gallery.selected == Some(index);
        let key = ImageKey {
            source,
            encoded,
            ..gallery.origin.clone()
        };
        let asset = self.asset_key(key, cx);
        let image = self.preview_asset(format!("gallery-thumbnail-{index}"), asset, 64., false, cx);
        Button::new(format!("gallery-thumbnail-{index}"))
            .accessibility_label(agent_core::localization::localized_ui_format(
                language,
                "生成画像 {0}".into(),
                vec![(index + 1).to_string()],
            ))
            .ghost()
            .child(image)
            .w(px(80.))
            .h(px(80.))
            .p_2()
            .selected(selected)
            .on_click(cx.listener(move |s, _, _, cx| {
                if let Some(gallery) = s.gallery.as_mut().filter(|gallery| !gallery.saving) {
                    gallery.selected = Some(index);
                    gallery.zoom = 1.;
                    gallery.saved = false;
                    gallery.error = None;
                    cx.notify();
                }
            }))
            .into_any_element()
    }

    fn image_gallery_view(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let language = crate::localization::language(cx);
        let gallery = self.gallery.as_ref().unwrap();
        let (source, encoded) = gallery.current_image().clone();
        let list_state = gallery.list.clone();
        let zoom = gallery.zoom;
        let saving = gallery.saving;
        let saved = gallery.saved;
        let loading = gallery.loading;
        let label = gallery
            .selected
            .map(|index| format!("{} / {}", index + 1, gallery.entries.len()))
            .unwrap_or_else(|| language.text("画像").into());
        let error = gallery
            .error
            .as_ref()
            .map(|error| error.flat_text(language));
        let key = ImageKey {
            source,
            encoded,
            ..gallery.origin.clone()
        };
        let asset = self.asset_key(key.clone(), cx);
        let ready = asset.read(cx).path.is_some();
        let gallery = self.gallery.as_mut().unwrap();
        if gallery
            .image
            .as_ref()
            .is_none_or(|(previous, _)| previous != &asset)
        {
            gallery.image = Some((asset.clone(), cx.observe(&asset, |_, _, cx| cx.notify())));
        }
        let image = self.preview_asset(
            "gallery-main",
            asset,
            (f32::from(window.viewport_size().height) - 200.).max(120.) * zoom,
            false,
            cx,
        );
        let entity = cx.entity().downgrade();
        let thumbnails = list(list_state, move |index, _, cx| {
            entity
                .update(cx, |s, cx| s.gallery_thumbnail(index, cx))
                .unwrap_or_else(|_| div().into_any_element())
        })
        .size_full();
        v_flex()
            .size_full()
            .p_4()
            .pt(px(if cfg!(target_os = "windows") && !self.side_chat_mode {
                16.
            } else {
                44.
            }))
            .gap_3()
            .child(
                h_flex()
                    .justify_end()
                    .gap_3()
                    .child(div().flex_1().child(label))
                    .when(loading, |row| {
                        row.child(language.text("画像一覧を読み込み中…"))
                    })
                    .child(
                        icon_button(
                            "gallery-save",
                            if saved {
                                Icon::new(IconName::Check)
                            } else {
                                Icon::default().path("icons/download.svg")
                            },
                            if saved {
                                language.text("保存済み")
                            } else {
                                language.text("保存")
                            },
                            cx,
                            |s, _, cx| s.save_gallery_image(cx),
                        )
                        .disabled(!ready || saving || saved),
                    )
                    .child(
                        icon_button(
                            "gallery-close",
                            IconName::Close,
                            language.text("閉じる"),
                            cx,
                            |s, _, cx| s.close_gallery(cx),
                        )
                        .disabled(saving),
                    ),
            )
            .when_some(error, |view, error| {
                view.child(div().text_color(rgb(0xff8e86)).child(error))
            })
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .gap_4()
                    .child(div().w(px(88.)).h_full().child(thumbnails))
                    .child(
                        div()
                            .id("gallery-viewport")
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_scroll()
                            .child(
                                div()
                                    .w(px((f32::from(window.viewport_size().width) - 152.)
                                        .max(120.)
                                        * zoom))
                                    .child(image),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .justify_center()
                    .gap_3()
                    .child(
                        button("gallery-zoom-out", "−", cx, |s, _, _| {
                            if let Some(gallery) = &mut s.gallery {
                                gallery.zoom = (gallery.zoom - 0.25).max(0.25);
                            }
                        })
                        .accessibility_label(language.text("縮小"))
                        .disabled(zoom <= 0.25),
                    )
                    .child(
                        button(
                            "gallery-zoom-reset",
                            format!("{:.0}%", zoom * 100.),
                            cx,
                            |s, _, _| {
                                if let Some(gallery) = &mut s.gallery {
                                    gallery.zoom = 1.;
                                }
                            },
                        )
                        .accessibility_label(language.text("倍率をリセット")),
                    )
                    .child(
                        button("gallery-zoom-in", "+", cx, |s, _, _| {
                            if let Some(gallery) = &mut s.gallery {
                                gallery.zoom = (gallery.zoom + 0.25).min(4.);
                            }
                        })
                        .accessibility_label(language.text("拡大"))
                        .disabled(zoom >= 4.),
                    ),
            )
            .into_any_element()
    }

    fn save_gallery_image(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self
            .gallery
            .as_ref()
            .filter(|gallery| !gallery.saving)
            .map(|gallery| gallery.current_key())
        else {
            return;
        };
        self.save_image(&key, cx);
    }

    fn save_image(&mut self, key: &ImageKey, cx: &mut Context<Self>) {
        let Some(ImageSource::Resource(resource)) = self
            .images
            .get(&key.id())
            .and_then(|image| image.read(cx).path.clone())
        else {
            return;
        };
        let id = if let Some(gallery) = self
            .gallery
            .as_mut()
            .filter(|gallery| gallery.current_key() == *key)
        {
            if gallery.saving {
                return;
            }
            gallery.saving = true;
            gallery.error = None;
            Some(gallery.id)
        } else {
            None
        };
        let http = cx.http_client();
        cx.spawn(async move |view, cx| {
            enum Original {
                File(std::sync::Arc<Path>),
                Bytes(Vec<u8>),
            }
            let result = async {
                let original = match resource {
                    Resource::Path(path) => Original::File(path),
                    Resource::Uri(uri) => {
                        use futures_util::AsyncReadExt;
                        let mut response = http
                            .get(
                                uri.as_ref(),
                                gpui_kit::http_client::AsyncBody::empty(),
                                true,
                            )
                            .await
                            .map_err(|error| error.to_string())?;
                        if !response.status().is_success() {
                            return Err(ErrorSource::formatted("画像の取得に失敗しました: {0}", vec![response.status().to_string()]));
                        }
                        let mut bytes = Vec::new();
                        response
                            .body_mut()
                            .read_to_end(&mut bytes)
                            .await
                            .map_err(|error| error.to_string())?;
                        Original::Bytes(bytes)
                    }
                    _ => return Err(ErrorSource::from("保存できない画像です")),
                };
                cx.background_executor()
                    .spawn(async move {
                        let format = match &original {
                            Original::File(path) => image::ImageReader::open(path)
                                .and_then(|reader| reader.with_guessed_format())
                                .map_err(|error| error.to_string())?
                                .format()
                                .ok_or("画像形式が不明です")?,
                            Original::Bytes(bytes) => {
                                image::guess_format(bytes).map_err(|error| error.to_string())?
                            }
                        };
                        let mode = format!("image.{}", format.extensions_str()[0]);
                        let Some(destination) = platform::choose_destination(&mode) else {
                            return Ok(false);
                        };
                        match original {
                            Original::File(path) => {
                                let source = std::fs::canonicalize(&path)
                                    .map_err(|error| error.to_string())?;
                                if std::fs::canonicalize(&destination).ok().as_deref()
                                    != Some(source.as_path())
                                {
                                    std::fs::copy(source, destination)
                                        .map_err(|error| error.to_string())?;
                                }
                            }
                            Original::Bytes(bytes) => std::fs::write(destination, bytes)
                                .map_err(|error| error.to_string())?,
                        }
                        Ok::<_, ErrorSource>(true)
                    })
                    .await
            }
            .await;
            let _ = view.update(cx, |s, cx| {
                if let Some(gallery) = s.gallery.as_mut().filter(|gallery| Some(gallery.id) == id) {
                    gallery.saving = false;
                    match result {
                        Ok(saved) => gallery.saved = saved,
                        Err(error) => {
                            tracing::error!(target: "agent", operation = "gallery.save", message = %error);
                            gallery.error = Some(error);
                        }
                    }
                } else if let Err(error) = result {
                    tracing::error!(target: "agent", operation = "image.save", message = %error);
                    cx.emit(MediaEvent::Error(error));
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

impl Render for Media {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.gallery.is_some() {
            self.image_gallery_view(window, cx)
        } else {
            div().into_any_element()
        }
    }
}

impl Desktop {
    pub(super) fn click_conversation_link(
        &mut self,
        source: &str,
        control: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if source.starts_with("https://") || source.starts_with("http://") {
            if control {
                self.open_browser(self.panel_scope(), source, cx);
            } else {
                cx.open_url(source);
            }
            return;
        }
        if let Some((file, lookup_path)) =
            agent_core::presentation::markdown::desktop_file_reference(
                source,
                &self.publication.pane.navigation.cwd,
                self.snapshot.connection.host_home_directory.as_deref(),
            )
        {
            self.activate_reference(
                &file,
                lookup_path.as_deref(),
                false,
                window.mouse_position(),
                window,
                cx,
            );
        }
    }
}

#[cfg(test)]
mod ownership_tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_kit as gpui;
    use std::cell::Cell;

    #[gpui::test]
    fn completions_and_zoom_notify_only_media_owners(cx: &mut TestAppContext) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        cx.update(|cx| {
            crate::appearance::init(cx);
            cx.set_global(Runtime {
                handle: runtime.handle().clone(),
                sessions: Default::default(),
                connections: Default::default(),
                closing: Default::default(),
                logging_error: None,
            });
        });
        let mut desktop = None;
        let _window = cx.open_window(size(px(700.), px(700.)), |window, cx| {
            let view = cx.new(|cx| {
                Desktop::new(
                    Mode::Fixture {
                        remote: None,
                        cwd: "/fixture".into(),
                    },
                    window,
                    cx,
                )
            });
            desktop = Some(view.clone());
            crate::activity::ActivityWindow::new(view, window, cx)
        });
        cx.run_until_parked();
        let desktop = desktop.unwrap();
        let notifications = Rc::new(Cell::new(0));
        let count = notifications.clone();
        cx.update(|cx| {
            cx.observe(&desktop, move |_, _| {
                count.set(count.get() + 1);
            })
            .detach();
        });
        let (media, asset) = desktop.update(cx, |view, cx| {
            let media = view.media.clone();
            let asset = media.update(cx, |media, cx| media.asset("picture.png", false, cx));
            (media, asset)
        });
        asset.update(cx, |asset, cx| {
            asset.complete(Ok("/fixture/picture.png".into()), cx)
        });
        cx.run_until_parked();
        assert_eq!(
            notifications.get(),
            0,
            "loading one image must not notify Desktop"
        );
        media.update(cx, |media, cx| media.open("picture.png", false, cx));
        cx.run_until_parked();
        let before = notifications.get();
        assert!(before > 0, "opening the gallery changes the window layout");
        media.update(cx, |media, cx| {
            media.gallery.as_mut().unwrap().zoom = 2.;
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(notifications.get(), before, "zoom belongs to the gallery");
    }

    #[gpui::test]
    fn gallery_and_image_completions_cannot_replace_a_new_mount_or_host(cx: &mut TestAppContext) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let media = cx.new(|cx| Media::new(runtime.handle().clone(), false, cx));
        media.update(cx, |media, cx| {
            media.sync(
                MediaSource {
                    host: "one".into(),
                    cwd: "/work".into(),
                    ..Default::default()
                },
                None,
                None,
                cx,
            );
            let old_asset = media.asset("image.png", false, cx);
            media.open("image.png", false, cx);
            let old_gallery = media.gallery.as_ref().unwrap().id;
            media.open("next.png", false, cx);
            media.gallery_loaded(old_gallery, Err("stale".into()), cx);
            assert!(media.gallery.as_ref().unwrap().error.is_none());
            assert_eq!(
                media.gallery.as_ref().unwrap().current_image().0.as_str(),
                "next.png"
            );
            media.sync(
                MediaSource {
                    host: "two".into(),
                    cwd: "/work".into(),
                    ..Default::default()
                },
                None,
                None,
                cx,
            );
            assert!(media.gallery.is_none());
            let current = media.asset("image.png", false, cx);
            assert_ne!(current, old_asset);
            old_asset.update(cx, |asset, cx| asset.complete(Ok("/old.png".into()), cx));
            assert!(current.read(cx).path.is_none());
            media.gallery_loaded(old_gallery, Err("stale".into()), cx);
            assert!(media.gallery.is_none());
        });
    }
}

#[cfg(test)]
mod file_panel_tests {
    use super::*;
    use core::prelude::v1::test;

    fn resolved(path: &str, directory: bool) -> Outcome {
        Outcome::FileReferenceResolved {
            path: path.into(),
            directory,
        }
    }
    fn reference_request(view: &Desktop, source: &str, epoch: u64) -> files::ReferenceRequest {
        let (file, lookup_path) = agent_core::presentation::markdown::desktop_file_reference(
            source,
            &view.snapshot.panes.primary().navigation.cwd,
            view.snapshot.connection.host_home_directory.as_deref(),
        )
        .unwrap();
        files::ReferenceRequest {
            epoch,
            target: view.reference_target(&file, lookup_path.as_deref()),
            action: files::ReferenceAction::Open(view.file_open_generation),
        }
    }
    fn start_reference(
        view: &mut Desktop,
        source: &str,
        window: &mut Window,
        cx: &mut Context<Desktop>,
    ) -> files::ReferenceRequest {
        Arc::make_mut(&mut view.snapshot).connection.connected = true;
        let request = reference_request(view, source, view.snapshot.connection.epoch);
        view.open_reference(request.target, window, cx);
        reference_request(view, source, view.snapshot.connection.epoch)
    }

    #[gpui::test]
    fn text_panels_and_native_files_keep_their_draft_and_resolution_boundaries(
        cx: &mut TestAppContext,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("large.txt");
        let cwd = directory.path().to_string_lossy().into_owned();
        let source = path.to_string_lossy().into_owned();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        cx.update(|cx| {
            crate::appearance::init(cx);
            cx.set_global(crate::Runtime {
                handle: runtime.handle().clone(),
                sessions: Default::default(),
                connections: Arc::new(crate::platform::Connections::default()),
                closing: tokio_util::task::TaskTracker::new(),
                logging_error: None,
            });
        });
        let mut desktop = None;
        let (_, window) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| {
                Desktop::new(
                    Mode::Fixture {
                        remote: None,
                        cwd: cwd.clone(),
                    },
                    window,
                    cx,
                )
            });
            desktop = Some(view.clone());
            Root::new(view, window, cx)
        });
        let desktop = desktop.unwrap();
        window.update(|window, cx| {
            desktop.update(cx, |view, cx| {
                let snapshot = Arc::make_mut(&mut view.snapshot);
                Arc::make_mut(&mut Arc::make_mut(snapshot.panes.primary_mut()).navigation).cwd =
                    cwd.clone();
                view.composer.input.update(cx, |input, cx| {
                    input.set_value("Keep this draft", window, cx)
                });
                view.open_surface(view.panel_scope(), Panel::Files, None);
                view.sync_file_browser(true, window, cx);
                view.file_browser.as_mut().unwrap().model.tree =
                    Arc::new(agent_core::presentation::files::FileTreeView {
                        rows: vec![agent_core::presentation::files::FileTreeRow {
                            path: "large.txt".into(),
                            label: "large.txt".into(),
                            kind: agent_protocol::file_tree::EntryKind::File,
                            depth: 0,
                            expanded: false,
                            selected: false,
                            ignored: false,
                            loading: false,
                        }],
                        ready: true,
                        ..Default::default()
                    });
                view.panels = view.panels.clone().set_open(view.panel_scope(), true);
                cx.notify();
            });
        });
        window.run_until_parked();
        let entry = window
            .debug_bounds("workspace-file-0")
            .expect("image file entry");
        window.simulate_click(entry.center(), Modifiers::default());
        window.run_until_parked();
        window.update(|_, cx| {
            desktop.update(cx, |view, cx| {
                let key = view.file_browser.as_ref().unwrap().model.key.clone();
                view.open_file_from_tree(
                    &key,
                    &agent_core::presentation::files::FileTreeRequest {
                        id: 1,
                        path: "large.txt".into(),
                    },
                    cx,
                );
            })
        });
        window.run_until_parked();
        assert!(
            window.debug_bounds("download-file").is_some(),
            "unsupported files keep an explicit save action"
        );
        window.update(|window, cx| {
            desktop.update(cx, |view, cx| {
                assert!(
                    view.media.read(cx).gallery.is_none(),
                    "text references must not open a media preview"
                );
                assert_eq!(view.panels.get(view.panel_scope()).kind(), Panel::File);
                assert_eq!(
                    view.panels
                        .get(view.panel_scope())
                        .selected()
                        .unwrap()
                        .path
                        .as_deref(),
                    Some(source.as_str())
                );
                assert_eq!(
                    view.composer.input.read(cx).value().as_ref(),
                    "Keep this draft"
                );
                let key = view.file_browser.as_ref().unwrap().model.key.clone();
                let duplicate = agent_core::presentation::files::FileTreeRequest {
                    id: 1,
                    path: "another.txt".into(),
                };
                view.open_file_from_tree(&key, &duplicate, cx);
                let mut closed_owner = key;
                closed_owner.owner.push_str("-previous-mount");
                view.open_file_from_tree(
                    &closed_owner,
                    &agent_core::presentation::files::FileTreeRequest { id: 2, ..duplicate },
                    cx,
                );
                assert_eq!(
                    view.panels
                        .get(view.panel_scope())
                        .selected()
                        .unwrap()
                        .path
                        .as_deref(),
                    Some(source.as_str()),
                    "duplicate projections and closed tree owners cannot reopen a file"
                );
                for name in [
                    "large.txt",
                    "installer.EXE",
                    "readme.txt",
                    "source.rs",
                    "index.html",
                    "notebook.ipynb",
                ] {
                    let target = directory.path().join(name).to_string_lossy().into_owned();
                    view.panels = view.panels.clone().set_open(view.panel_scope(), false);
                    let request = start_reference(view, &target, window, cx);
                    // The fixture is offline; deliver the Host's resolution
                    // through the same production completion used by links.
                    view.operation_completed(
                        OperationCompletion::FileReference(Box::new(request)),
                        Ok(resolved(&target, false)),
                        window,
                        cx,
                    );
                    let panel = view.panels.get(view.panel_scope());
                    assert!(panel.open);
                    assert_eq!(panel.kind(), Panel::File, "{name}");
                    assert_eq!(
                        panel.selected().unwrap().path.as_deref(),
                        Some(target.as_str())
                    );
                    assert!(view.media.read(cx).gallery.is_none());
                    assert_eq!(
                        view.local_busy, 0,
                        "{name} must not start an implicit download or OS launch"
                    );
                    assert_eq!(
                        view.composer.input.read(cx).value().as_ref(),
                        "Keep this draft"
                    );
                }
                let before = view.panels.get(view.panel_scope());
                let stale = start_reference(view, "older.rs", window, cx);
                let latest = start_reference(view, "newer.rs", window, cx);
                view.operation_completed(
                    OperationCompletion::FileReference(Box::new(stale)),
                    Ok(resolved("/stale.rs", false)),
                    window,
                    cx,
                );
                assert_eq!(
                    view.panels.get(view.panel_scope()),
                    before,
                    "a slower lookup cannot replace a newer request"
                );
                let winner = format!("{cwd}/newer.rs");
                view.operation_completed(
                    OperationCompletion::FileReference(Box::new(latest)),
                    Ok(resolved(&winner, false)),
                    window,
                    cx,
                );
                assert_eq!(
                    view.panels
                        .get(view.panel_scope())
                        .selected()
                        .unwrap()
                        .path
                        .as_deref(),
                    Some(winner.as_str())
                );
                let stale = start_reference(view, "older.rs", window, cx);
                view.open_file(source.clone(), None, false, cx);
                view.operation_completed(
                    OperationCompletion::FileReference(Box::new(stale)),
                    Ok(resolved("/stale.rs", false)),
                    window,
                    cx,
                );
                assert_eq!(
                    view.panels
                        .get(view.panel_scope())
                        .selected()
                        .unwrap()
                        .path
                        .as_deref(),
                    Some(source.as_str()),
                    "tree and explicit editor opens supersede pending lookups"
                );
                let mut copy =
                    reference_request(view, "missing.rs:12:3", view.snapshot.connection.epoch);
                copy.action = files::ReferenceAction::Menu(
                    agent_core::presentation::files::FileAction::CopyRelativePath,
                );
                let found = format!("{cwd}/nested/missing.rs");
                view.operation_completed(
                    OperationCompletion::FileReference(Box::new(copy)),
                    Ok(resolved(&found, false)),
                    window,
                    cx,
                );
                assert_eq!(
                    cx.read_from_clipboard()
                        .and_then(|item| item.text())
                        .as_deref(),
                    Some("nested/missing.rs:12:3")
                );
                // The existing gallery remains available to its explicit image
                // entry points; only file-reference routing is being replaced.
                let image = directory
                    .path()
                    .join("explicit.png")
                    .to_string_lossy()
                    .into_owned();
                view.media
                    .update(cx, |media, cx| media.open(&image, false, cx));
                assert_eq!(
                    view.media
                        .read(cx)
                        .gallery
                        .as_ref()
                        .unwrap()
                        .current_image()
                        .0
                        .as_str(),
                    image
                );
                view.media.update(cx, |media, cx| media.close_gallery(cx));
                let request = start_reference(view, "~/Documents", window, cx);
                view.operation_completed(
                    OperationCompletion::FileReference(Box::new(request)),
                    Ok(resolved(&cwd, true)),
                    window,
                    cx,
                );
                assert_eq!(view.panels.get(view.panel_scope()).kind(), Panel::Files);
                let before = view.panels.get(view.panel_scope());
                view.operation_completed(
                    OperationCompletion::FileReference(Box::new(reference_request(
                        view,
                        &source,
                        view.snapshot.connection.epoch + 1,
                    ))),
                    Ok(resolved(&source, false)),
                    window,
                    cx,
                );
                assert!(
                    view.media.read(cx).gallery.is_none(),
                    "a stale Host response must not open a file"
                );
                assert_eq!(view.panels.get(view.panel_scope()), before);
            });
        });
        for (index, name) in [
            "picture.PNG",
            "clip.MP4",
            "song.mp3",
            "document.pdf",
            "notes.docx",
            "archive.zip",
            "font.ttf",
        ]
        .iter()
        .enumerate()
        {
            let target = directory.path().join(name);
            window.update(|window, cx| {
                desktop.update(cx, |view, cx| {
                    let before = view.panels.get(view.panel_scope());
                    let request = start_reference(view, target.to_str().unwrap(), window, cx);
                    view.operation_completed(
                        OperationCompletion::FileReference(Box::new(request)),
                        Ok(resolved(target.to_str().unwrap(), false)),
                        window,
                        cx,
                    );
                    assert_eq!(
                        view.panels.get(view.panel_scope()),
                        before,
                        "native files must not open a text panel"
                    );
                    assert_eq!(
                        view.composer.input.read(cx).value().as_ref(),
                        "Keep this draft"
                    );
                    assert_eq!(view.local_busy, 0, "local files do not require downloading");
                })
            });
            assert_eq!(
                window.opened_url(),
                Some(url::Url::from_file_path(&target).unwrap().into())
            );
            let name = format!("tree {name}");
            let target = directory.path().join(&name);
            window.update(|_, cx| {
                desktop.update(cx, |view, cx| {
                    let key = view.file_browser.as_ref().unwrap().model.key.clone();
                    view.open_file_from_tree(
                        &key,
                        &agent_core::presentation::files::FileTreeRequest {
                            id: index as u64 + 10,
                            path: name,
                        },
                        cx,
                    );
                })
            });
            assert_eq!(
                window.opened_url(),
                Some(url::Url::from_file_path(target).unwrap().into())
            );
        }
    }

    #[gpui::test]
    fn side_chat_links_and_changes_are_visible_and_return_to_the_same_draft(
        cx: &mut TestAppContext,
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        cx.update(|cx| {
            crate::appearance::init(cx);
            cx.set_global(crate::Runtime {
                handle: runtime.handle().clone(),
                sessions: Default::default(),
                connections: Arc::new(crate::platform::Connections::default()),
                closing: tokio_util::task::TaskTracker::new(),
                logging_error: None,
            });
        });
        let mut desktop = None;
        let (_, window) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| {
                Desktop::new(
                    Mode::Fixture {
                        remote: Some(RemoteHost {
                            id: "fixture".into(),
                            name: "fixture".into(),
                            ticket: "invalid-fixture-ticket".into(),
                        }),
                        cwd: "/fixture".into(),
                    },
                    window,
                    cx,
                )
            });
            desktop = Some(view.clone());
            Root::new(view, window, cx)
        });
        let desktop = desktop.unwrap();
        window.update(|window, cx| {
            desktop.update(cx, |view, cx| {
                Arc::make_mut(
                    &mut Arc::make_mut(Arc::make_mut(&mut view.snapshot).panes.primary_mut())
                        .navigation,
                )
                .cwd = "/fixture".into();
                view.composer.input.update(cx, |input, cx| {
                    input.set_value("Keep this side draft", window, cx)
                });
                let request = start_reference(view, "readme.txt:1", window, cx);
                view.operation_completed(
                    OperationCompletion::FileReference(Box::new(request)),
                    Ok(resolved("/fixture/readme.txt", false)),
                    window,
                    cx,
                );
                assert!(
                    view.panels.get(view.panel_scope()).open
                        && view.panels.get(view.panel_scope()).kind() == Panel::File
                );
                #[cfg(target_os = "windows")]
                {
                    view.click_conversation_link("https://example.com/first", true, window, cx);
                    assert!(
                        view.panels.get(view.panel_scope()).open
                            && view.panels.get(view.panel_scope()).kind() == Panel::Browser
                    );
                    assert_eq!(
                        view.panels.get(view.panel_scope()).active.and_then(|id| {
                            match view.surface_resources.get(&id) {
                                Some(SurfaceResource::Pending { url }) => url.as_deref(),
                                _ => None,
                            }
                        }),
                        Some("https://example.com/first")
                    );
                    view.click_conversation_link(
                        "http://localhost:3000/next?q=1#detail",
                        true,
                        window,
                        cx,
                    );
                    assert_eq!(
                        view.panels.get(view.panel_scope()).active.and_then(|id| {
                            match view.surface_resources.get(&id) {
                                Some(SurfaceResource::Pending { url }) => url.as_deref(),
                                _ => None,
                            }
                        }),
                        Some("http://localhost:3000/next?q=1#detail")
                    );
                    let request = start_reference(view, "readme.txt:1", window, cx);
                    view.operation_completed(
                        OperationCompletion::FileReference(Box::new(request)),
                        Ok(resolved("/fixture/readme.txt", false)),
                        window,
                        cx,
                    );
                    assert!(view.panels.get(view.panel_scope()).kind() == Panel::File);
                    assert_eq!(
                        view.composer.input.read(cx).value().as_ref(),
                        "Keep this side draft"
                    );
                }
                cx.notify();
            })
        });
        window.run_until_parked();
        assert!(
            window.debug_bounds("conversation-file").is_some(),
            "file links must show the file panel"
        );
        let back = window
            .debug_bounds("back-side-chat")
            .expect("return to side chat");
        window.simulate_click(back.center(), Modifiers::default());
        window.update(|_, cx| {
            desktop.update(cx, |view, cx| {
                assert_eq!(
                    view.composer.input.read(cx).value().as_ref(),
                    "Keep this side draft"
                );
                assert!(!view.panels.get(view.panel_scope()).open);
                view.open_review(None, cx);
                cx.notify();
            })
        });
        window.run_until_parked();
        assert!(
            window.debug_bounds("diff-file-navigation").is_some(),
            "change summaries must show the diff panel"
        );
        let back = window.debug_bounds("back-side-chat").unwrap();
        window.simulate_click(back.center(), Modifiers::default());
        window.update(|_, cx| {
            desktop.update(cx, |view, cx| {
                assert_eq!(
                    view.composer.input.read(cx).value().as_ref(),
                    "Keep this side draft"
                );
                assert!(!view.panels.get(view.panel_scope()).open);
            })
        });
    }
}

#[cfg(test)]
mod tests {
    use agent_core::presentation::markdown::{
        MarkdownBlock, markdown_blocks, markdown_presentation,
    };
    use gpui_kit as gpui;
    use gpui_kit::{AppContext, TestAppContext, component::text::TextViewState};

    #[gpui::test]
    fn markdown_tables_keep_every_shared_cell_in_desktop_selection(cx: &mut TestAppContext) {
        cx.update(crate::appearance::init);
        let source = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../crates/agent-core/tests/fixtures/markdown/table.md"
        ));
        let rendered = markdown_presentation(source, "", None).source;
        let state = cx.new(|cx| TextViewState::markdown(&rendered, cx));
        cx.run_until_parked();
        state.update(cx, |state, cx| state.select_all(cx));
        let selected = state.read_with(cx, |state, _| state.selected_text());
        let MarkdownBlock::Table { rows, .. } = &markdown_blocks(source.into())[0] else {
            panic!("expected shared table")
        };
        let expected = rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|cell| {
                        cell.runs
                            .iter()
                            .map(|run| run.text.as_str())
                            .collect::<String>()
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(selected.trim_end(), expected);
        state.update(cx, |state, cx| state.clear_selection(cx));
        assert_eq!(state.read_with(cx, |state, _| state.selected_text()), "");
    }
}
