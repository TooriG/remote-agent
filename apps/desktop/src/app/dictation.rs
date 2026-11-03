use super::*;
use agent_core::state::operations as op;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Phase {
    Preparing,
    #[cfg(target_os = "macos")]
    Permission,
    Recording,
    Transcribing,
}

pub(super) struct Dictation {
    pub(super) id: uuid::Uuid,
    key: DraftKey,
    generation: u64,
    pub(super) phase: Phase,
    send: bool,
    control: Option<platform::Recording>,
    preparation: Option<agent_core::client::DictationPreparation>,
    pub(super) levels: std::collections::VecDeque<f32>,
}
impl Desktop {
    pub(super) fn dictation_bar(
        phase: Option<Phase>,
        levels: &std::collections::VecDeque<f32>,
        connected: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        let language = crate::localization::language(cx);
        let Some(phase) = phase else {
            return div().into_any_element();
        };
        let recording = phase == Phase::Recording;
        h_flex()
            .h(px(88.))
            .px_2()
            .gap_3()
            .child(
                icon_button(
                    "cancel-dictation",
                    IconName::Close,
                    language.text("録音を取り消す (Esc)"),
                    cx,
                    |s, _, _| s.cancel_recording(),
                )
                .disabled(phase == Phase::Transcribing),
            )
            .child(if recording {
                h_flex()
                    .id("recording-waveform")
                    .flex_1()
                    .min_w_0()
                    .h(px(48.))
                    .items_center()
                    .justify_center()
                    .gap(px(3.))
                    .overflow_hidden()
                    .children(levels.iter().map(|level| {
                        div()
                            .w(px(3.))
                            .flex_shrink_0()
                            .h(px(3. + level.sqrt() * 45.))
                            .rounded_full()
                            .bg(rgb(appearance::FOREGROUND))
                    }))
                    .into_any_element()
            } else {
                h_flex()
                    .flex_1()
                    .gap_2()
                    .child(crate::activity::indicator())
                    .child(match phase {
                        Phase::Preparing => language.text("録音を準備中…"),
                        #[cfg(target_os = "macos")]
                        Phase::Permission => language.text("マイクの許可を確認中…"),
                        _ => language.text("文字起こし中…"),
                    })
                    .into_any_element()
            })
            .child(
                icon_button(
                    "stop-dictation",
                    Icon::default().path("icons/stop.svg"),
                    language.text("録音を終了して文字起こし"),
                    cx,
                    |s, _, _| s.finish_dictation(false),
                )
                .disabled(!recording),
            )
            .child(
                icon_button(
                    "send-dictation",
                    IconName::ArrowUp,
                    language.text("文字起こしして送信"),
                    cx,
                    |s, _, cx| s.send(cx),
                )
                .primary()
                .large()
                .rounded_full()
                .w(px(44.))
                .h(px(44.))
                .disabled(!recording || !connected),
            )
            .into_any_element()
    }
    pub(super) fn start_dictation(&mut self) {
        if self.dictation.is_some() || !self.snapshot.connected || self.busy > 0 {
            return;
        }
        let (events, incoming) = async_channel::unbounded();
        match platform::start_recording(events) {
            Ok(control) => {
                let id = uuid::Uuid::new_v4();
                let updates = self.updates.clone();
                let epoch = self.epoch;
                self.runtime.handle.spawn(async move {
                    while let Ok(event) = incoming.recv().await {
                        if updates
                            .send((epoch, Update::Recording(id, event)))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                });
                self.dictation = Some(Dictation {
                    id,
                    key: self.draft_key().clone(),
                    generation: self.snapshot.epoch,
                    phase: Phase::Preparing,
                    send: false,
                    control: Some(control),
                    preparation: None,
                    levels: std::collections::VecDeque::from([0.; 40]),
                });
                self.local_errors.remove(&self.local_error_scope());
            }
            Err(error) => self.set_error(error),
        }
    }
    pub(super) fn finish_dictation(&mut self, send: bool) {
        let Some(state) = self
            .dictation
            .as_mut()
            .filter(|state| state.phase == Phase::Recording)
        else {
            return;
        };
        if let Err(error) = state.control.as_ref().expect("recording control").finish() {
            self.set_error(error);
            self.dictation = None;
            return;
        }
        state.phase = Phase::Transcribing;
        state.send = send;
    }
    pub(super) fn cancel_recording(&mut self) {
        if let Some(state) = self.dictation.as_mut() {
            if state.phase == Phase::Transcribing {
                state.send = false;
            } else {
                self.dictation = None;
            }
        }
    }
    pub(super) fn recording_update(&mut self, id: uuid::Uuid, event: platform::RecordingEvent) {
        let Some(state) = self.dictation.as_mut().filter(|state| state.id == id) else {
            return;
        };
        match event {
            #[cfg(target_os = "macos")]
            platform::RecordingEvent::RequestingPermission => state.phase = Phase::Permission,
            #[cfg(target_os = "macos")]
            platform::RecordingEvent::Preparing => state.phase = Phase::Preparing,
            platform::RecordingEvent::Started => {
                state.phase = Phase::Recording;
                state.preparation = self
                    .session
                    .as_ref()
                    .map(|session| session.store.prepare_dictation());
            }
            platform::RecordingEvent::Level(level) => {
                if state.phase == Phase::Recording && level.is_finite() {
                    state.levels.pop_front();
                    state.levels.push_back(level.clamp(0., 1.));
                }
            }
            platform::RecordingEvent::Finished(Err(error)) => {
                self.dictation = None;
                self.set_error(error);
            }
            platform::RecordingEvent::Finished(Ok(audio)) => {
                state.control = None;
                let intent = Intent::Transcribe(op::Dictate {
                    draft_key: state.key.clone(),
                    preparation: state
                        .preparation
                        .as_ref()
                        .map(|preparation| preparation.id()),
                    audio,
                    send: state.send && state.generation == self.snapshot.epoch,
                    client_user_message_id: uuid::Uuid::new_v4().to_string().into(),
                });
                self.perform(intent, OperationCompletion::Dictation(id));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use std::{cell::Cell, rc::Rc};

    #[gpui::test]
    fn recording_levels_only_redraw_the_composer_and_failures_restore_the_normal_ui(
        cx: &mut TestAppContext,
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        cx.update(|cx| {
            crate::appearance::init(cx);
            cx.set_global(Runtime {
                handle: runtime.handle().clone(),
                connections: Arc::default(),
                closing: tokio_util::task::TaskTracker::new(),
                logging_error: None,
            });
        });
        let id = uuid::Uuid::new_v4();
        let (view, window) = cx.add_window_view(|window, cx| {
            let mut view = Desktop::new(Mode::Main, window, cx);
            view.onboarding = false;
            Arc::make_mut(&mut view.snapshot).connected = true;
            view.panels.open(view.panel_scope(), Panel::Files, None);
            view.dictation = Some(Dictation {
                id,
                key: view.draft_key().clone(),
                generation: view.snapshot.epoch,
                phase: Phase::Recording,
                send: false,
                control: None,
                preparation: None,
                levels: std::collections::VecDeque::from([0.; 40]),
            });
            view
        });
        window.simulate_resize(size(px(1600.), px(900.)));
        window.run_until_parked();
        let counts = |view: &Desktop, cx: &App| {
            [
                view.sidebar_view.as_ref().unwrap().read(cx).renders,
                view.conversation.read(cx).renders,
                view.workbench_view.read(cx).renders,
                view.composer_view.read(cx).content.read(cx).renders,
            ]
        };
        let before = view.read_with(window, counts);
        let notifications = Rc::new(Cell::new(0));
        let _observation = window.update(|_, cx| {
            let count = notifications.clone();
            cx.observe(&view, move |_, _| count.set(count.get() + 1))
        });
        let receive = |event, window: &mut VisualTestContext| {
            window.update(|window, cx| {
                view.update(cx, |view, cx| {
                    view.receive((view.epoch, Update::Recording(id, event)), window, cx);
                })
            });
            window.run_until_parked();
        };
        receive(platform::RecordingEvent::Level(0.25), window);
        let after = view.read_with(window, counts);
        assert_eq!(
            &after[..3],
            &before[..3],
            "audio samples must reuse the conversation and panels"
        );
        assert!(after[3] > before[3], "the visible waveform must update");
        assert_eq!(
            notifications.get(),
            0,
            "audio samples must not broadcast Desktop notifications"
        );
        view.read_with(window, |view, _| {
            assert_eq!(view.dictation.as_ref().unwrap().levels.back(), Some(&0.25))
        });
        receive(platform::RecordingEvent::Level(f32::NAN), window);
        assert_eq!(
            view.read_with(window, counts),
            after,
            "invalid samples must not change the waveform"
        );
        receive(
            platform::RecordingEvent::Finished(Err("recording failed".into())),
            window,
        );
        assert!(
            notifications.get() > 0,
            "recording failures must update the surrounding UI"
        );
        view.read_with(window, |view, _| assert!(view.dictation.is_none()));
    }
}
