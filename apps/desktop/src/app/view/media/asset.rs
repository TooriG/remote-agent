use super::*;
use base64::Engine;
use gpui_kit::component::menu::ContextMenuExt;

#[derive(Clone, PartialEq, Eq)]
pub(in crate::app) struct ImageKey {
    pub(super) host: String,
    pub(super) cwd: String,
    pub(super) source: Arc<String>,
    pub(super) encoded: bool,
}

#[derive(PartialEq, Eq, Hash)]
pub(super) struct ImageId {
    host: String,
    cwd: String,
    source: [u8; 32],
    encoded: bool,
}
impl ImageId {
    pub(super) fn new(host: &str, cwd: &str, source: &str, encoded: bool) -> Self {
        Self {
            host: host.into(),
            cwd: cwd.into(),
            encoded,
            source: ring::digest::digest(&ring::digest::SHA256, source.as_bytes())
                .as_ref()
                .try_into()
                .expect("SHA-256 digest length"),
        }
    }
}
impl ImageKey {
    pub(super) fn id(&self) -> ImageId {
        ImageId::new(&self.host, &self.cwd, &self.source, self.encoded)
    }
}

pub(in crate::app) struct ImageAsset {
    pub(super) key: ImageKey,
    pub(in crate::app) path: Option<ImageSource>,
    error: Option<ErrorSource>,
    _loading: Task<()>,
    _directory: Arc<tempfile::TempDir>,
}

impl ImageAsset {
    #[cfg(test)]
    pub(super) fn fixture(
        key: ImageKey,
        path: ImageSource,
        directory: Arc<tempfile::TempDir>,
    ) -> Self {
        Self {
            key,
            path: Some(path),
            error: None,
            _loading: Task::ready(()),
            _directory: directory,
        }
    }

    pub(super) fn new(
        key: ImageKey,
        remote: bool,
        store: Option<Arc<agent_core::store::Store>>,
        directory: Arc<tempfile::TempDir>,
        handle: &tokio::runtime::Handle,
        cx: &mut Context<Self>,
    ) -> Self {
        let destination = directory.path().join(uuid::Uuid::new_v4().to_string());
        let source = key.source.clone();
        let cwd = key.cwd.clone();
        let encoded = key.encoded;
        let directory_lifetime = directory.clone();
        let load = handle.spawn(async move {
            let _directory = directory_lifetime;
            if encoded || source.starts_with("data:image/") {
                let data = if encoded {
                    source.as_str()
                } else {
                    let (header, data) = source.split_once(',').ok_or("画像データが不正です")?;
                    if !header.ends_with(";base64") {
                        return Err("未対応の画像データです".into());
                    }
                    data
                };
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(data)
                    .map_err(|error| error.to_string())?;
                tokio::fs::write(&destination, bytes)
                    .await
                    .map_err(|error| error.to_string())?;
                return Ok(destination.to_string_lossy().into_owned());
            }
            if source.starts_with("https://") || source.starts_with("http://") {
                return Ok(source.as_ref().clone());
            }
            let path = conversation_file_path(&source, &cwd)?;
            if !remote && path.is_absolute() {
                return Ok(path.to_string_lossy().into_owned());
            }
            let store = store.ok_or("ホストに接続していません")?;
            if !remote {
                let result = store
                    .dispatch(Intent::ResolveFile(op::ResolveFile {
                        path: path.to_string_lossy().into_owned(),
                    }))
                    .await
                    .map_err(|error| error.to_string())?;
                let Outcome::ResolvedFile { entry } = result else {
                    unreachable!("resolved file outcome")
                };
                return Ok(entry.path);
            }
            files::download_to(
                &store,
                path.into_os_string()
                    .into_string()
                    .map_err(|_| "download source is not UTF-8")?,
                destination.clone(),
            )
            .await?;
            Ok(destination.to_string_lossy().into_owned())
        });
        Self {
            key,
            path: None,
            error: None,
            _directory: directory,
            _loading: cx.spawn(async move |asset, cx| {
                let result = load
                    .await
                    .unwrap_or_else(|error| Err(ErrorSource::from(error.to_string())));
                let _ = asset.update(cx, |asset, cx| asset.complete(result, cx));
            }),
        }
    }

    pub(super) fn complete(&mut self, result: Result<String, ErrorSource>, cx: &mut Context<Self>) {
        match result {
            Ok(path) => {
                self.path = Some(if Path::new(&path).is_absolute() {
                    PathBuf::from(path).into()
                } else {
                    ImageSource::from(path)
                });
                self.error = None;
            }
            Err(error) => {
                tracing::error!(target: "agent", operation = "image.load", message = %error);
                self.error = Some(error);
            }
        }
        cx.notify();
    }
}

#[derive(IntoElement)]
pub(super) struct ImagePreview {
    pub(super) id: SharedString,
    pub(super) media: WeakEntity<Media>,
    pub(super) asset: Entity<ImageAsset>,
    pub(super) session: Option<SessionRef>,
    pub(super) height: f32,
    pub(super) clickable: bool,
}

impl RenderOnce for ImagePreview {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        // Retain only this mount's display state. Observing the asset here via
        // use_keyed_state would forward completion to the conversation's owner.
        let view = window.with_global_id(self.id.into(), |id, window| {
            window.with_element_state::<Entity<ImageView>, _>(id, |retained, _| {
                let view = retained
                    .filter(|view| view.read(cx).asset == self.asset)
                    .unwrap_or_else(|| {
                        cx.new(|cx| {
                            cx.observe_global::<crate::localization::Language>(|_, cx| cx.notify())
                                .detach();
                            ImageView {
                                _loaded: cx.observe(&self.asset, |_, _, cx| cx.notify()),
                                asset: self.asset,
                                media: self.media,
                                session: self.session.clone(),
                                height: self.height,
                                clickable: self.clickable,
                                #[cfg(test)]
                                renders: 0,
                            }
                        })
                    });
                view.update(cx, |view, cx| {
                    if view.height != self.height
                        || view.clickable != self.clickable
                        || view.session != self.session
                    {
                        view.height = self.height;
                        view.clickable = self.clickable;
                        view.session = self.session;
                        cx.notify();
                    }
                });
                (view.clone(), view)
            })
        });
        view.cached(
            StyleRefinement::default()
                .w_full()
                .h(px(self.height))
                .flex_shrink_0(),
        )
    }
}

struct ImageView {
    asset: Entity<ImageAsset>,
    media: WeakEntity<Media>,
    session: Option<SessionRef>,
    height: f32,
    clickable: bool,
    _loaded: Subscription,
    #[cfg(test)]
    renders: usize,
}

impl Render for ImageView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        {
            self.renders += 1;
        }
        let language = crate::localization::language(cx);
        let state = self.asset.read(cx);
        let Some(path) = &state.path else {
            return match &state.error {
                Some(error) => div()
                    .text_sm()
                    .text_color(rgb(appearance::MUTED))
                    .child(error.flat_text(language))
                    .into_any_element(),
                None => image_skeleton(self.height, cx.theme().secondary).into_any_element(),
            };
        };
        let key = state.key.clone();
        let background = cx.theme().secondary;
        let height = self.height;
        let image = img(path.clone())
            .id("image")
            .w_full()
            .min_w_0()
            .h(px(height))
            .min_h(px(height))
            .max_h(px(height))
            .object_fit(ObjectFit::Contain)
            .debug_selector(|| "chat-image".into())
            .with_loading(move || image_skeleton(height, background).into_any_element())
            .with_fallback(move || {
                div()
                    .text_sm()
                    .text_color(rgb(appearance::MUTED))
                    .child(language.text("画像を表示できません"))
                    .into_any_element()
            });
        let media = self.media.clone();
        let menu_key = key.clone();
        let session = self.session.clone();
        let menu_owner = media.clone();
        div()
            .id("image-menu-boundary")
            .w_full()
            .h(px(height))
            .flex_shrink_0()
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_mouse_up(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .id("image-menu")
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(self.clickable, |element| {
                        element.cursor_pointer().on_click(move |_, _, cx| {
                            let _ = media.update(cx, |media, cx| {
                                media.open_gallery(key.clone(), session.clone(), cx)
                            });
                        })
                    })
                    .child(image)
                    .context_menu(move |menu, _, cx| {
                        let saving = menu_owner
                            .read_with(cx, |media, _| {
                                media.gallery.as_ref().is_some_and(|g| g.saving)
                            })
                            .unwrap_or(true);
                        let media = menu_owner.clone();
                        let key = menu_key.clone();
                        menu.item(
                            PopupMenuItem::new(language.text("画像を名前を付けて保存…"))
                                .icon(Icon::default().path("icons/download.svg"))
                                .disabled(saving)
                                .on_click(move |_, _, cx| {
                                    let _ =
                                        media.update(cx, |media, cx| media.save_image(&key, cx));
                                }),
                        )
                    }),
            )
            .into_any_element()
    }
}
