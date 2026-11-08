//! Host paths use core's authenticated transfer; native views receive local copies.
use crate::l10n::{localized, tr, tr_format};
use crate::{
    Connection,
    client::{Client, LocalFile},
    native,
    shared::Shared,
};
use agent_core::presentation::markdown::{MarkdownFileSurface, markdown_file_target};
use nsh_core::{ImageSource, prelude::*};
use std::sync::Arc;

#[derive(Clone, Copy)]
pub(crate) struct Preview {
    state: Signal<PreviewState>,
    owner: Owner,
}

#[derive(Clone, PartialEq)]
enum PreviewState {
    Closed,
    Loading(uuid::Uuid),
    Ready(Shared<LocalFile>),
}

impl Preview {
    pub fn new() -> Self {
        Self {
            state: signal(PreviewState::Closed),
            owner: Owner::current(),
        }
    }

    fn show(self, file: Shared<LocalFile>) {
        self.state.set(PreviewState::Ready(file));
    }

    pub fn begin(self) -> uuid::Uuid {
        let request = uuid::Uuid::new_v4();
        self.state.set(PreviewState::Loading(request));
        request
    }

    pub fn is_pending(self, request: uuid::Uuid) -> bool {
        self.state.get_untracked() == PreviewState::Loading(request)
    }

    pub fn load(self, transfer: impl Future<Output = Result<Shared<LocalFile>, String>> + 'static) {
        let request = self.begin();
        // Preview ownership is independent of recycled conversation rows.
        self.owner.run(move || {
            spawn(async move {
                let result = transfer.await;
                if !self.is_pending(request) {
                    return;
                }
                match result {
                    Ok(file) => self.show(file),
                    Err(reason) => {
                        self.state.set(PreviewState::Closed);
                        native::message(tr("ファイル"), reason);
                    }
                }
            })
        });
    }

    pub fn close(self) {
        self.state.set(PreviewState::Closed);
    }
}

pub(crate) fn preview() -> impl Element {
    let preview = use_context::<Preview>().unwrap();
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    return drawn_preview(preview);
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    view()
        .child(native("bex.preview")
            .h(px(0.0))
            .prop("path", move || serde_json::json!(preview.state.with(|file| match file {
                PreviewState::Ready(file) => file.path.to_string_lossy().into_owned(),
                _ => String::new(),
            })))
            .on_native_event(move |event: &NativeEvent| {
                if &*event.name == "closed" && preview.state.with_untracked(|file| matches!(file, PreviewState::Ready(file) if event.data.as_str() == file.path.to_str())) {
                    preview.state.set(PreviewState::Closed);
                } else if &*event.name == "error" {
                    preview.state.set(PreviewState::Closed);
                    native::message(tr("プレビュー"), tr("このファイルをプレビューできませんでした。"));
                }
            }))
        .child(when(move || matches!(preview.state.get(), PreviewState::Loading(_)), || text(tr("ファイルを読み込み中…"))))
}

/// The preview where QuickLook is not: an image in NSH's own viewer over the
/// window; another file in the app the system opens it with, while a bar keeps
/// its local copy until it is dismissed.
#[cfg(not(any(target_os = "macos", target_os = "ios")))]
fn drawn_preview(preview: Preview) -> View {
    use crate::ui::{self, Icon, colors};
    use nsh_core::element::{overlay, rgba};
    let file = computed(move || match preview.state.get() {
        PreviewState::Ready(file) => Some(file),
        _ => None,
    });
    let image = move || {
        file.with(|file| {
            file.as_ref()
                .is_some_and(|file| native::image_file(&file.path))
        })
    };
    let path = move || {
        file.with(|file| {
            file.as_ref()
                .map(|file| file.path.to_string_lossy().into_owned())
                .unwrap_or_default()
        })
    };
    effect(move || {
        let Some(file) = file.get().filter(|file| !native::image_file(&file.path)) else {
            return;
        };
        let opened = open_path(file.path.clone());
        spawn(async move {
            if let Err(reason) = opened.await {
                preview.state.set(PreviewState::Closed);
                native::message(tr("プレビュー"), reason);
            }
        });
    });
    view()
        .child(when(image, move || {
            overlay()
                .absolute()
                .inset_0()
                .items_center()
                .justify_center()
                .p_8()
                .bg(rgba(0x000000cc))
                .on_press(move || preview.close())
                .child(
                    nsh_core::element::image(path)
                        .w_full()
                        .h_full()
                        .object_fit(ResizeMode::Contain)
                        .label(tr("プレビュー"))
                        .on_error(move |_: &str| {
                            preview.close();
                            native::message(
                                tr("プレビュー"),
                                tr("このファイルをプレビューできませんでした。"),
                            );
                        }),
                )
                .child(
                    view()
                        .absolute()
                        .top(px(16.0))
                        .right(px(16.0))
                        .child(ui::icon_button(Icon::CLOSE, tr("閉じる"), move || {
                            preview.close()
                        })),
                )
        }))
        .child(when(
            move || file.with(Option::is_some) && !image(),
            move || {
                view()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_b_color(colors::BORDER)
                    .child(
                        text(move || {
                            file.with(|file| {
                                file.as_ref()
                                    .and_then(|file| file.path.file_name())
                                    .map(|name| name.to_string_lossy().into_owned())
                                    .unwrap_or_default()
                            })
                        })
                        .flex_1()
                        .min_w_0()
                        .number_of_lines(1),
                    )
                    .child(ui::text_button(tr("閉じる"), move || preview.close()))
            },
        ))
        .child(when(
            move || matches!(preview.state.get(), PreviewState::Loading(_)),
            || text(tr("ファイルを読み込み中…")),
        ))
}

pub(crate) async fn receive(
    transfer: async_channel::Receiver<Result<Arc<LocalFile>, agent_core::client::AgentError>>,
) -> Result<Shared<LocalFile>, String> {
    transfer
        .recv()
        .await
        .map_err(|reason| reason.to_string())?
        .map(Shared)
        .map_err(|reason| reason.to_string())
}

pub(crate) async fn load_image(
    client: Arc<Client>,
    host: String,
    cwd: String,
    source: String,
) -> Result<Shared<LocalFile>, String> {
    let transfer = if source.starts_with("data:image/") {
        let mime = source
            .strip_prefix("data:")
            .and_then(|value| value.split_once(';'))
            .map(|(mime, _)| mime)
            .ok_or(tr("画像を読み取れませんでした。"))?;
        let extension =
            native::image_extension(mime).ok_or(tr("この画像形式を利用できません。"))?;
        let data = ImageSource {
            uri: source.into(),
            ..Default::default()
        }
        .inline()
        .data
        .ok_or(tr("画像を読み取れませんでした。"))?;
        client.cache_image(data.to_vec(), extension)
    } else if source.starts_with("https://") || source.starts_with("http://") {
        let response = fetch(source).await?;
        if !response.ok() {
            return Err(tr_format(
                "画像を取得できませんでした (HTTP {0})。",
                [&response.status()],
            ));
        }
        let extension = response
            .header("content-type")
            .and_then(|mime| native::image_extension(mime.split(';').next().unwrap_or(mime)))
            .unwrap_or_else(|| "png".into());
        client.cache_image(response.body, extension)
    } else {
        let file = markdown_file_target(source, cwd, MarkdownFileSurface::Mobile)
            .ok_or(tr("画像の場所を読み取れませんでした。"))?;
        client.download(host, file.path)
    };
    receive(transfer).await
}

pub(crate) fn image_view(
    connection: Connection,
    preview: Preview,
    source: impl Fn() -> String + 'static,
    width: f32,
    height: f32,
) -> impl Element {
    let decode_error = signal(None::<String>);
    let key = computed(move || {
        (
            connection.host.get(),
            connection
                .composer
                .with(|p| p.controls.navigation.cwd.clone()),
            source(),
        )
    });
    let loaded = resource(
        move || Some(key.get()),
        move |(host, cwd, source), _| load_image(connection.client.get(), host, cwd, source),
    );
    effect(move || {
        key.get();
        decode_error.set(None);
    });
    let failed = move || loaded.error().is_some() || decode_error.get().is_some();
    // TextView's image wrapper has intrinsic width; a percentage alone collapses it.
    // A failed image keeps only its message's height, not the image's.
    view()
        .w(px(width))
        .style_when(move || !failed(), |s| s.min_h(px(height)))
        .max_w(relative(1.0))
        .gap_2()
        .child(when(
            move || loaded.get().is_some(),
            move || {
                image(move || {
                    loaded.with(|file| {
                        file.map(|file| file.path.to_string_lossy().into_owned())
                            .unwrap_or_default()
                    })
                })
                .w_full()
                .h(px(height))
                .object_fit(ResizeMode::Contain)
                .label(tr("画像を拡大"))
                .role(AccessibilityRole::ImageButton)
                .on_error(move |reason: &str| decode_error.set(Some(reason.into())))
                .on_press(move || {
                    if let Some(file) = loaded.get() {
                        preview.show(file);
                    }
                })
            },
        ))
        .child(when(
            move || loaded.loading(),
            || text(tr("画像を読み込み中…")),
        ))
        .child(when(failed, move || {
            view()
                .gap_2()
                .child(text(move || {
                    loaded
                        .error()
                        .or_else(|| decode_error.get())
                        .map(localized)
                        .unwrap_or_default()
                }))
                .child(view().flex_row().child(crate::ui::text_button(
                    tr("再試行"),
                    move || {
                        decode_error.set(None);
                        loaded.refetch()
                    },
                )))
        }))
}
