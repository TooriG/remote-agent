use crate::execution::SessionStatus;

/// Organization cannot hide work whose completion or input delivery is still uncertain.
pub fn settlement_unavailable_reason(
    status: SessionStatus,
    running_turn: bool,
    pending_request: bool,
    pending_input: bool,
) -> Option<&'static str> {
    if pending_request {
        return Some("確認待ちの要求に回答してから片づけてください。");
    }
    if running_turn || matches!(status, SessionStatus::Preparing | SessionStatus::Running) {
        return Some("実行中のタスクを停止してから片づけてください。");
    }
    if pending_input {
        return Some("送信結果を確認してから片づけてください。");
    }
    (status != SessionStatus::Idle).then_some("実行状態を確認できるまで片づけられません。")
}

#[cfg(test)]
mod settlement_tests {
    use super::*;

    #[test]
    fn only_known_idle_work_without_pending_input_or_requests_can_settle() {
        for status in [
            SessionStatus::Preparing,
            SessionStatus::Running,
            SessionStatus::Idle,
            SessionStatus::Unavailable,
            SessionStatus::Unknown,
        ] {
            for running in [false, true] {
                for request in [false, true] {
                    for input in [false, true] {
                        assert_eq!(
                            settlement_unavailable_reason(status, running, request, input)
                                .is_none(),
                            status == SessionStatus::Idle && !running && !request && !input
                        );
                    }
                }
            }
        }
    }
}
