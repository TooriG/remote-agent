//! Response boundaries and input ownership are independent of timeline IO.
mod background;
use super::native;
use agent_protocol::ids::ItemId;
use background::Reports;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InputDelivery {
    NewTurn,
    CurrentTurn,
    Background,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MessageDisposition {
    Forward,
    Applied,
    Deferred,
    Failed,
    Interrupted,
}

pub(super) struct Observation<'a> {
    pub echoes: Vec<(&'a str, bool)>,
    pub disposition: MessageDisposition,
}

#[derive(Default)]
pub(super) struct ResponseState {
    interrupted: bool,
    pending_inputs: HashMap<ItemId, bool>,
    result_received: bool,
    idle: bool,
    background: Reports,
}

impl ResponseState {
    pub(super) fn interrupted(&self) -> bool {
        self.interrupted
    }

    pub(super) fn interrupt(&mut self) {
        self.interrupted = true;
    }

    /// A refused stop leaves the response running.
    pub(super) fn interrupt_result(&mut self, error: Option<String>) -> Result<(), String> {
        self.interrupted = error.is_none();
        error.map_or(Ok(()), Err)
    }

    pub(super) fn answered(&self) -> bool {
        self.result_received && self.idle && self.pending_inputs.is_empty()
    }

    pub(super) fn finished(&self) -> bool {
        self.answered() && (!self.background.pending() || self.interrupted)
    }

    pub(super) fn delivery(&self) -> InputDelivery {
        if self.background.pending() {
            InputDelivery::Background
        } else if self.answered() && !self.interrupted {
            InputDelivery::NewTurn
        } else {
            InputDelivery::CurrentTurn
        }
    }

    pub(super) fn accept(&mut self, id: ItemId, delivery: InputDelivery) {
        self.pending_inputs
            .insert(id, delivery == InputDelivery::Background);
    }

    pub(super) fn observe<'a>(&mut self, message: &'a Value) -> Result<Observation<'a>, String> {
        let background_level = self.observe_background(message)?;
        let echoes = self.consume_echoes(message);
        let disposition = if background_level {
            MessageDisposition::Applied
        } else {
            self.transition(message, !echoes.is_empty())
        };
        Ok(Observation {
            echoes,
            disposition,
        })
    }

    fn consume_echoes<'a>(&mut self, message: &'a Value) -> Vec<(&'a str, bool)> {
        if !message["parent_tool_use_id"].is_null() {
            return Vec::new();
        }
        // Reply frames can echo a batch before the individual user
        // replays. Consume every UUID in SDK consumption order.
        let echoes: Vec<_> = if message["type"] == "user" {
            message["uuid"].as_str().into_iter().collect()
        } else if let Some(ids) = message["user_message_uuids"].as_array() {
            ids.iter().filter_map(Value::as_str).collect()
        } else {
            message["user_message_uuid"].as_str().into_iter().collect()
        };
        let consumed: Vec<_> = echoes
            .into_iter()
            .filter_map(|id| {
                self.pending_inputs
                    .remove(id)
                    .map(|background| (id, background))
            })
            .collect();
        let opens_turn = self.result_received
            && self.idle
            && !self.interrupted
            && consumed.iter().any(|(_, background)| *background);
        consumed
            .into_iter()
            .enumerate()
            .map(|(index, (id, _))| (id, opens_turn && index == 0))
            .collect()
    }

    fn observe_background(&mut self, message: &Value) -> Result<bool, String> {
        if message["type"] != "system" {
            return Ok(false);
        }
        match message["subtype"].as_str() {
            Some("task_notification") => {
                let id = message["run_id"]
                    .as_str()
                    .or(message["task_id"].as_str())
                    .ok_or("Claude task notification ID is missing")?;
                self.background.notified(
                    id,
                    message["ambient"] == true || message["skip_transcript"] == true,
                );
            }
            Some("task_started") => {
                let id = message["run_id"]
                    .as_str()
                    .or(message["task_id"].as_str())
                    .ok_or("Claude task start ID is missing")?;
                let report = native::needs_background_report(
                    message["task_type"].as_str(),
                    message["is_backgrounded"] != false,
                    message["ambient"] == true || message["skip_transcript"] == true,
                );
                self.background.observe([(id.to_owned(), report)]);
            }
            Some("background_tasks_changed") => {
                let tasks = message["tasks"]
                    .as_array()
                    .ok_or("Claude background task level is missing")?
                    .iter()
                    .map(|task| {
                        let id = task["run_id"]
                            .as_str()
                            .or(task["task_id"].as_str())
                            .ok_or("Claude background task ID is missing")?;
                        Ok::<_, String>((
                            id.to_owned(),
                            native::needs_background_report(
                                task["task_type"].as_str(),
                                true,
                                task["ambient"] == true,
                            ),
                        ))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                self.background.observe(tasks);
                return Ok(true);
            }
            _ => {}
        }
        Ok(false)
    }

    fn transition(&mut self, message: &Value, consumed: bool) -> MessageDisposition {
        let kind = message["type"].as_str().unwrap_or_default();
        match kind {
            "system" if message["subtype"] == "session_state_changed" => {
                if self.idle && message["state"] == "running" && self.result_received {
                    if !self.pending_inputs.is_empty() {
                        // A turn-start level can precede the input echo.
                        // Keep the prior terminal boundary until its owner is known.
                        return MessageDisposition::Deferred;
                    }
                    self.background.response_started(None);
                }
                self.idle = message["state"] == "idle";
            }
            "result" => {
                // Claude Code ends a stopped response, including one awaiting
                // an answer, with an execution error. The stop is its outcome,
                // as in T3 Code.
                if message["is_error"] == true && !self.interrupted {
                    return MessageDisposition::Failed;
                }
                if message["origin"]["kind"] == "task-notification" {
                    self.background
                        .response_started(message["origin"]["runId"].as_str());
                }
                self.background.response_finished();
                self.result_received = true;
                if self.interrupted {
                    return MessageDisposition::Interrupted;
                }
            }
            _ => {
                let notification =
                    kind == "user" && message["origin"]["kind"] == "task-notification";
                let started = message["parent_tool_use_id"].is_null()
                    && (kind == "assistant"
                        || (kind == "system"
                            && message["subtype"] == "init"
                            && self.pending_inputs.is_empty())
                        || (kind == "stream_event" && message["event"]["type"] == "message_start"));
                if notification {
                    self.background
                        .response_started(message["origin"]["runId"].as_str());
                } else if started && !consumed && self.result_received {
                    self.background.response_started(None);
                }
                if consumed || started || (notification && message["parent_tool_use_id"].is_null())
                {
                    self.result_received = false;
                    self.idle = false;
                }
                return MessageDisposition::Forward;
            }
        }
        MessageDisposition::Applied
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn observe(response: &mut ResponseState, message: Value) -> MessageDisposition {
        response.observe(&message).unwrap().disposition
    }

    fn settle(response: &mut ResponseState) {
        assert_eq!(
            observe(response, json!({"type":"result"})),
            MessageDisposition::Applied
        );
        assert_eq!(
            observe(
                response,
                json!({"type":"system","subtype":"session_state_changed","state":"idle"})
            ),
            MessageDisposition::Applied
        );
    }

    #[test]
    fn pending_input_keeps_the_prior_boundary_until_its_echo() {
        let mut response = ResponseState::default();
        observe(
            &mut response,
            json!({"type":"system","subtype":"background_tasks_changed","tasks":[{"run_id":"shell","task_type":"local_bash"}]}),
        );
        settle(&mut response);
        let delivery = response.delivery();
        assert_eq!(delivery, InputDelivery::Background);
        response.accept("queued".into(), delivery);
        assert!(!response.answered());
        assert_eq!(
            observe(
                &mut response,
                json!({"type":"system","subtype":"session_state_changed","state":"running"})
            ),
            MessageDisposition::Deferred
        );
        let echo = json!({"type":"user","uuid":"queued"});
        let observation = response.observe(&echo).unwrap();
        assert_eq!(observation.echoes, [("queued", true)]);
        assert_eq!(observation.disposition, MessageDisposition::Forward);
        assert!(!response.answered());
        settle(&mut response);
        assert!(response.answered());
        assert!(!response.finished());
    }

    #[test]
    fn parent_responses_and_nested_activity_have_distinct_boundaries() {
        for (message, stays_settled) in [
            (json!({"type":"assistant"}), false),
            (
                json!({"type":"assistant","parent_tool_use_id":"child"}),
                true,
            ),
            (
                json!({"type":"stream_event","event":{"type":"message_start"}}),
                false,
            ),
            (
                json!({"type":"stream_event","event":{"type":"content_block_delta"}}),
                true,
            ),
            (
                json!({"type":"user","origin":{"kind":"task-notification"}}),
                false,
            ),
            (
                json!({"type":"user","origin":{"kind":"task-notification"},"parent_tool_use_id":"child"}),
                true,
            ),
            (json!({"type":"user"}), true),
        ] {
            let mut response = ResponseState::default();
            settle(&mut response);
            assert_eq!(response.delivery(), InputDelivery::NewTurn);
            assert_eq!(observe(&mut response, message), MessageDisposition::Forward);
            assert_eq!(response.answered(), stays_settled);
        }
    }

    #[test]
    fn background_work_needs_its_own_report_and_errors_do_not_finish_it() {
        let mut response = ResponseState::default();
        assert_eq!(response.delivery(), InputDelivery::CurrentTurn);
        observe(
            &mut response,
            json!({"type":"system","subtype":"background_tasks_changed","tasks":[{"run_id":"shell","task_type":"local_bash"}]}),
        );
        settle(&mut response);
        assert!(!response.finished());
        assert_eq!(
            observe(
                &mut response,
                json!({"type":"result","is_error":true,"origin":{"kind":"task-notification","runId":"shell"}})
            ),
            MessageDisposition::Failed
        );
        assert!(!response.finished());
        assert_eq!(
            observe(
                &mut response,
                json!({"type":"result","origin":{"kind":"task-notification","runId":"shell"}})
            ),
            MessageDisposition::Applied
        );
        assert!(response.finished());
        response.accept("pending".into(), InputDelivery::CurrentTurn);
        assert!(
            response
                .interrupt_result(Some("not interrupted".into()))
                .is_err()
        );
        assert!(!response.interrupted());
        response.interrupt_result(None).unwrap();
        assert!(response.interrupted());
        assert_eq!(
            observe(&mut response, json!({"type":"result"})),
            MessageDisposition::Interrupted
        );
    }

    #[test]
    fn a_requested_stop_owns_the_error_result_unless_it_is_refused() {
        let mut stopped = ResponseState::default();
        stopped.interrupt();
        assert_eq!(
            observe(&mut stopped, json!({"type":"result","is_error":true})),
            MessageDisposition::Interrupted
        );
        let mut refused = ResponseState::default();
        refused.interrupt();
        assert!(refused.interrupt_result(Some("refused".into())).is_err());
        assert!(!refused.interrupted());
        assert_eq!(
            observe(&mut refused, json!({"type":"result","is_error":true})),
            MessageDisposition::Failed
        );
    }

    #[test]
    fn background_notices_are_answered_by_the_next_foreground_response() {
        let mut response = ResponseState::default();
        observe(
            &mut response,
            json!({"type":"system","subtype":"task_started","task_id":"shell","task_type":"local_bash"}),
        );
        assert_eq!(response.delivery(), InputDelivery::Background);
        // The foreground result settles before Claude answers a task notice.
        settle(&mut response);
        observe(
            &mut response,
            json!({"type":"system","subtype":"task_notification","task_id":"shell"}),
        );
        observe(&mut response, json!({"type":"assistant"}));
        settle(&mut response);
        assert!(response.finished());
        assert!(
            response
                .observe(&json!({"type":"system","subtype":"task_started"}))
                .is_err()
        );
    }

    #[test]
    fn delivery_waits_for_echoes_and_interruption_keeps_input_in_the_current_turn() {
        let mut response = ResponseState::default();
        response.accept("input".into(), response.delivery());
        settle(&mut response);
        assert_eq!(response.delivery(), InputDelivery::CurrentTurn);
        observe(&mut response, json!({"type":"user","uuid":"input"}));
        settle(&mut response);
        assert_eq!(response.delivery(), InputDelivery::NewTurn);
        response.interrupt();
        assert_eq!(response.delivery(), InputDelivery::CurrentTurn);
    }

    #[test]
    fn a_batch_consumes_each_pending_echo_once_in_sdk_order_and_opens_one_turn() {
        let mut response = ResponseState::default();
        observe(
            &mut response,
            json!({"type":"system","subtype":"task_started","task_id":"shell","task_type":"local_bash"}),
        );
        settle(&mut response);
        response.accept("first".into(), response.delivery());
        response.accept("second".into(), response.delivery());
        let message =
            json!({"type":"assistant","user_message_uuids":["unknown","second","first","second"]});
        let observation = response.observe(&message).unwrap();
        assert_eq!(observation.echoes, [("second", true), ("first", false)]);
        assert_eq!(observation.disposition, MessageDisposition::Forward);
        assert!(!response.answered());
    }

    #[test]
    fn interrupted_background_input_never_opens_a_turn_and_does_not_owe_a_report() {
        let mut response = ResponseState::default();
        observe(
            &mut response,
            json!({"type":"system","subtype":"task_started","task_id":"shell","task_type":"local_bash"}),
        );
        settle(&mut response);
        response.accept("queued".into(), response.delivery());
        response.interrupt();
        let message = json!({"type":"user","uuid":"queued"});
        assert_eq!(
            response.observe(&message).unwrap().echoes,
            [("queued", false)]
        );
        assert_eq!(
            observe(&mut response, json!({"type":"result"})),
            MessageDisposition::Interrupted
        );
        assert!(!response.finished());
        observe(
            &mut response,
            json!({"type":"system","subtype":"session_state_changed","state":"idle"}),
        );
        assert!(response.finished());
    }

    #[test]
    fn running_without_pending_input_starts_the_response_to_a_task_notice() {
        let mut response = ResponseState::default();
        observe(
            &mut response,
            json!({"type":"system","subtype":"task_started","task_id":"shell","task_type":"local_bash"}),
        );
        settle(&mut response);
        observe(
            &mut response,
            json!({"type":"system","subtype":"task_notification","task_id":"shell"}),
        );
        assert_eq!(
            observe(
                &mut response,
                json!({"type":"system","subtype":"session_state_changed","state":"running"})
            ),
            MessageDisposition::Applied
        );
        assert!(!response.answered());
        settle(&mut response);
        assert!(response.finished());
    }

    #[test]
    fn silent_task_notices_finish_without_another_response() {
        for flag in ["ambient", "skip_transcript"] {
            let mut response = ResponseState::default();
            observe(
                &mut response,
                json!({"type":"system","subtype":"task_started","task_id":"shell","task_type":"local_bash"}),
            );
            settle(&mut response);
            assert!(!response.finished());
            let mut notice =
                json!({"type":"system","subtype":"task_notification","task_id":"shell"});
            notice[flag] = true.into();
            observe(&mut response, notice);
            assert!(response.finished());
        }
    }

    #[test]
    fn silent_and_foreground_tasks_do_not_require_a_background_report() {
        for message in [
            json!({"type":"system","subtype":"task_started","task_id":"shell","task_type":"local_bash","ambient":true}),
            json!({"type":"system","subtype":"task_started","task_id":"shell","task_type":"local_bash","skip_transcript":true}),
            json!({"type":"system","subtype":"task_started","task_id":"shell","task_type":"local_bash","is_backgrounded":false}),
            json!({"type":"system","subtype":"background_tasks_changed","tasks":[{"run_id":"shell","task_type":"local_bash","ambient":true}]}),
        ] {
            let mut response = ResponseState::default();
            observe(&mut response, message);
            settle(&mut response);
            assert!(response.finished());
            assert_eq!(response.delivery(), InputDelivery::NewTurn);
        }
    }

    #[test]
    fn a_queued_human_reply_does_not_answer_a_task_notice() {
        let mut response = ResponseState::default();
        observe(
            &mut response,
            json!({"type":"system","subtype":"task_started","task_id":"shell","task_type":"local_bash"}),
        );
        settle(&mut response);
        observe(
            &mut response,
            json!({"type":"system","subtype":"task_notification","task_id":"shell"}),
        );
        // Idle is a repeated level, not the start of the task's response.
        observe(
            &mut response,
            json!({"type":"system","subtype":"session_state_changed","state":"idle"}),
        );
        response.accept("human".into(), response.delivery());
        let reply = json!({"type":"assistant","user_message_uuid":"human"});
        assert_eq!(response.observe(&reply).unwrap().echoes, [("human", true)]);
        settle(&mut response);
        assert!(!response.finished());
    }

    #[test]
    fn init_starts_a_response_when_no_input_is_waiting_for_its_echo() {
        let mut response = ResponseState::default();
        settle(&mut response);
        assert_eq!(
            observe(&mut response, json!({"type":"system","subtype":"init"})),
            MessageDisposition::Forward
        );
        assert!(!response.answered());
    }

    #[test]
    fn init_preserves_a_pending_inputs_terminal_boundary_until_its_echo() {
        let mut response = ResponseState::default();
        observe(
            &mut response,
            json!({"type":"system","subtype":"task_started","task_id":"shell","task_type":"local_bash"}),
        );
        settle(&mut response);
        response.accept("queued".into(), response.delivery());
        observe(&mut response, json!({"type":"system","subtype":"init"}));
        let echo = json!({"type":"user","uuid":"queued"});
        assert_eq!(response.observe(&echo).unwrap().echoes, [("queued", true)]);
    }
}
