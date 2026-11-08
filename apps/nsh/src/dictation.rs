//! Capture is the platform's (AVFAudio, WASAPI); preparation, transcription
//! and draft updates are core's.
use crate::l10n::tr;
use crate::ui::Enabled;
use crate::{Connection, native};
use agent_core::{client::DictationPreparation, state::DraftKey};
use nsh_core::prelude::*;
use std::sync::Arc;

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod avfaudio;
#[cfg(any(target_os = "macos", target_os = "ios"))]
use avfaudio::Capture;
#[cfg(windows)]
mod wasapi;
#[cfg(windows)]
use wasapi::Capture;

struct Recording {
    capture: Capture,
    preparation: Arc<DictationPreparation>,
    host: String,
    pane: Option<u64>,
    draft: DraftKey,
}

#[derive(Clone, Copy)]
pub(crate) struct Dictation {
    recording: Stored<Option<Recording>>,
    pub active: Signal<bool>,
    pub requesting: Signal<bool>,
    generation: Signal<u64>,
    target: Memo<(String, DraftKey)>,
}

impl Dictation {
    pub fn new() -> Self {
        let connection = use_context::<Connection>().unwrap();
        let this = Self {
            recording: stored(None),
            active: signal(false),
            requesting: signal(false),
            generation: signal(0),
            target: computed(move || {
                (
                    connection.host.get(),
                    connection
                        .composer
                        .with(|p| p.controls.navigation.draft_key.clone()),
                )
            }),
        };
        effect(move || {
            this.target.get();
            this.cancel();
        });
        // The iPhone stops recording when it leaves the foreground; the Mac goes
        // on while the user is in another app, as its desktop client did.
        effect(move || {
            if !native::in_use() || !connection.composer.with(|p| p.controls.connected) {
                this.cancel();
            }
        });
        this
    }

    pub fn cancel(self) {
        self.generation.update(|value| *value += 1);
        self.recording.replace(None);
        self.active.set(false);
        self.requesting.set(false);
    }

    pub fn finish(self, client: Arc<crate::client::Client>, send: bool) {
        if let Some(recording) = self.recording.replace(None) {
            self.active.set(false);
            match recording.capture.finish() {
                Ok(audio) => client.dictate(
                    recording.host,
                    recording.pane,
                    recording.draft,
                    recording.preparation,
                    audio,
                    send,
                ),
                Err(reason) => native::message(tr("音声入力"), reason),
            }
        }
    }

    pub fn button(self) -> impl Element {
        let connection = use_context::<Connection>().unwrap();
        view()
            .flex_row()
            .items_center()
            .child(
                crate::ui::icon_button_sized(
                    crate::ui::Icon::MICROPHONE,
                    tr("音声をCodexで文字起こし"),
                    40.0,
                    20.0,
                    move || {
                        if self.active.get_untracked() {
                            self.finish(connection.client.get(), false);
                        } else {
                            let (host, draft) = self.target.get_untracked();
                            let preparation = match connection
                                .client
                                .with(|client| client.prepare_dictation(&host, connection.pane))
                            {
                                Ok(preparation) => preparation,
                                Err(reason) => {
                                    native::message(tr("音声入力"), reason.to_string());
                                    return;
                                }
                            };
                            self.requesting.set(true);
                            let current = self.generation.get_untracked();
                            spawn(async move {
                                let permission = Capture::request_permission().await;
                                if current != self.generation.get_untracked()
                                    || self.target.get_untracked() != (host.clone(), draft.clone())
                                {
                                    return;
                                }
                                self.requesting.set(false);
                                let result = permission.and_then(|()| Capture::start());
                                match result {
                                    Ok(capture) => {
                                        self.recording.replace(Some(Recording {
                                            capture,
                                            preparation,
                                            host,
                                            pane: connection.pane,
                                            draft,
                                        }));
                                        self.active.set(true);
                                    }
                                    Err(reason) => native::message(tr("音声入力"), reason),
                                }
                            });
                        }
                    },
                )
                .enabled(move || {
                    !self.requesting.get()
                        && !connection.uploading()
                        && connection.composer.with(|p| p.can_dictate())
                })
                .style_when(
                    move || self.active.get(),
                    |s| s.bg(crate::ui::colors::SELECTED),
                ),
            )
            .child(when(
                move || self.active.get(),
                move || {
                    crate::ui::icon_button_sized(
                        crate::ui::Icon::CLOSE,
                        tr("録音を取り消す"),
                        40.0,
                        20.0,
                        move || self.cancel(),
                    )
                },
            ))
            .child(when(
                move || self.requesting.get(),
                || text(tr("マイクの許可を確認中…")),
            ))
            .child(when(
                move || connection.composer.with(|p| p.controls.transcribing),
                || text(tr("文字起こし中…")),
            ))
    }
}
