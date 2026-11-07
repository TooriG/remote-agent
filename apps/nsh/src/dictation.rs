//! Capture is AVFAudio's; preparation, transcription and draft updates are core's.
use crate::ui::Enabled;
use crate::{Connection, native};
use agent_core::{client::DictationPreparation, state::DraftKey};
use block2::RcBlock;
use nsh_core::{platform::Lifecycle, prelude::*};
use objc2::{AnyThread, rc::Retained, runtime::Bool};
use objc2_avf_audio::*;
use objc2_foundation::{NSString, NSURL};
use std::sync::Arc;

struct Capture {
    recorder: Retained<AVAudioRecorder>,
    file: tempfile::TempPath,
    #[cfg(target_os = "ios")]
    _session: AudioSession,
}

impl Capture {
    async fn request_permission() -> Result<(), String> {
        let (send, receive) = async_channel::bounded(1);
        let permission = RcBlock::new(move |allowed: Bool| {
            let _ = send.try_send(allowed.as_bool());
        });
        // AVAudioApplication copies the callback; the result resumes on NSH's UI executor.
        unsafe {
            AVAudioApplication::requestRecordPermissionWithCompletionHandler(&permission);
        }
        if !receive.recv().await.map_err(|reason| reason.to_string())? {
            return Err("設定アプリでBexのマイクへのアクセスを許可してください。".into());
        }
        Ok(())
    }

    fn start() -> Result<Self, String> {
        #[cfg(target_os = "ios")]
        let session = AudioSession::record()?;
        let file = tempfile::Builder::new()
            .prefix("bex-dictation-")
            .suffix(".wav")
            .tempfile()
            .map_err(|reason| reason.to_string())?
            .into_temp_path();
        let url = NSURL::fileURLWithPath(&NSString::from_str(&file.to_string_lossy()));
        // A mono 24 kHz signed 16-bit PCM format matches core's transcription contract.
        let recorder = unsafe {
            let format = AVAudioFormat::initWithCommonFormat_sampleRate_channels_interleaved(
                AVAudioFormat::alloc(),
                AVAudioCommonFormat::PCMFormatInt16,
                24_000.0,
                1,
                true,
            )
            .ok_or("録音形式を作成できませんでした。")?;
            AVAudioRecorder::initWithURL_format_error(AVAudioRecorder::alloc(), &url, &format)
                .map_err(|reason| reason.localizedDescription().to_string())?
        };
        let capture = Self {
            recorder,
            file,
            #[cfg(target_os = "ios")]
            _session: session,
        };
        if !unsafe { capture.recorder.record() } {
            return Err("録音を開始できませんでした。".into());
        }
        Ok(capture)
    }

    fn finish(self) -> Result<Vec<u8>, String> {
        // The recorder and buffer stay owned here until the sample copy completes.
        unsafe {
            if !self.recorder.isRecording() {
                return Err("録音が中断されました。もう一度録音してください。".into());
            }
            self.recorder.stop();
            let url = NSURL::fileURLWithPath(&NSString::from_str(&self.file.to_string_lossy()));
            let file = AVAudioFile::initForReading_commonFormat_interleaved_error(
                AVAudioFile::alloc(),
                &url,
                AVAudioCommonFormat::PCMFormatInt16,
                true,
            )
            .map_err(|reason| reason.localizedDescription().to_string())?;
            let length = u32::try_from(file.length())
                .ok()
                .filter(|length| *length > 0)
                .ok_or("音声を録音できませんでした。")?;
            let buffer = AVAudioPCMBuffer::initWithPCMFormat_frameCapacity(
                AVAudioPCMBuffer::alloc(),
                &file.processingFormat(),
                length,
            )
            .ok_or("録音データを読み込めませんでした。")?;
            file.readIntoBuffer_error(&buffer)
                .map_err(|reason| reason.localizedDescription().to_string())?;
            let channels = buffer.int16ChannelData();
            if channels.is_null() {
                return Err("録音データを読み込めませんでした。".into());
            }
            let samples =
                std::slice::from_raw_parts((*channels).as_ptr(), buffer.frameLength() as usize);
            Ok(samples
                .iter()
                .flat_map(|sample| sample.to_le_bytes())
                .collect())
        }
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        unsafe {
            self.recorder.stop();
        }
    }
}

#[cfg(target_os = "ios")]
struct AudioSession {
    session: Retained<AVAudioSession>,
    category: Retained<NSString>,
    mode: Retained<NSString>,
    options: AVAudioSessionCategoryOptions,
}
#[cfg(target_os = "ios")]
impl AudioSession {
    fn record() -> Result<Self, String> {
        // Retain the previous configuration before making any fallible change.
        unsafe {
            let session = AVAudioSession::sharedInstance();
            let previous = Self {
                category: session.category(),
                mode: session.mode(),
                options: session.categoryOptions(),
                session,
            };
            previous
                .session
                .setCategory_mode_options_error(
                    AVAudioSessionCategoryRecord.ok_or("録音を利用できません。")?,
                    AVAudioSessionModeMeasurement.ok_or("録音を利用できません。")?,
                    AVAudioSessionCategoryOptions::empty(),
                )
                .map_err(|reason| reason.localizedDescription().to_string())?;
            previous
                .session
                .setActive_error(true)
                .map_err(|reason| reason.localizedDescription().to_string())?;
            Ok(previous)
        }
    }
}
#[cfg(target_os = "ios")]
impl Drop for AudioSession {
    fn drop(&mut self) {
        unsafe {
            let _ = self.session.setActive_withOptions_error(
                false,
                AVAudioSessionSetActiveOptions::NotifyOthersOnDeactivation,
            );
            let _ = self.session.setCategory_mode_options_error(
                &self.category,
                &self.mode,
                self.options,
            );
        }
    }
}

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
        effect(move || {
            if lifecycle() != Lifecycle::Active
                || !connection.composer.with(|p| p.controls.connected)
            {
                this.cancel();
            }
        });
        this
    }

    fn cancel(self) {
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
                Err(reason) => native::message("音声入力", reason),
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
                    crate::ui::Icon::Microphone,
                    "音声をCodexで文字起こし",
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
                                    native::message("音声入力", reason.to_string());
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
                                    Err(reason) => native::message("音声入力", reason),
                                }
                            });
                        }
                    },
                )
                .enabled(move || {
                    !self.requesting.get()
                        && !connection.uploading()
                        && connection.composer.with(|p| {
                            p.controls.connected
                                && !p.controls.sending
                                && !p.controls.transferring
                                && !p.controls.transcribing
                        })
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
                        crate::ui::Icon::Close,
                        "録音を取り消す",
                        40.0,
                        20.0,
                        move || self.cancel(),
                    )
                },
            ))
            .child(when(
                move || self.requesting.get(),
                || text("マイクの許可を確認中…"),
            ))
            .child(when(
                move || connection.composer.with(|p| p.controls.transcribing),
                || text("文字起こし中…"),
            ))
    }
}
