//! Host input-routing decisions. Clients say what input sent while a turn runs
//! should do; the Host decides from live execution what it does.
use crate::providers::SubmissionTarget;
use agent_protocol::{models::SessionStatus, operations::FollowUp};

/// Where input goes: to the provider, or into the Host's queue to start once
/// the running turn and the input queued before it have finished.
#[derive(Debug, PartialEq)]
pub(super) enum Route<'a> {
    Provider(SubmissionTarget<'a>),
    Queue,
}

/// What the Host knows about a conversation's execution when input arrives.
#[derive(Debug, Clone, Copy)]
pub(super) struct Execution<'a> {
    pub status: SessionStatus,
    pub running_turn: Option<&'a str>,
    /// The provider can take input into its running turn.
    pub steers: bool,
    /// The running turn answered and only background work remains, so its
    /// process takes the next prompt now; T3 Code's run has ended there.
    pub answered: bool,
    /// Other input already waits in the queue, which this input does not overtake.
    pub ahead: bool,
}

/// Steering joins the running turn at once, as in T3 Code; a provider that
/// cannot steer, or a turn whose ID is not known yet, queues the input.
pub(super) fn submission_route<'a>(
    execution: Execution<'a>,
    follow_up: FollowUp,
    cwd: Option<&'a str>,
) -> Result<Route<'a>, &'static str> {
    let running = execution.status == SessionStatus::Running;
    if running && !execution.answered {
        if follow_up == FollowUp::Steer
            && execution.steers
            && let Some(turn) = execution.running_turn
        {
            return Ok(Route::Provider(SubmissionTarget::Steer(turn)));
        }
        return Ok(Route::Queue);
    }
    if execution.ahead {
        return Ok(Route::Queue);
    }
    if running {
        return Ok(Route::Provider(SubmissionTarget::Next));
    }
    Ok(Route::Provider(SubmissionTarget::Start {
        cwd: cwd
            .filter(|cwd| !cwd.trim().is_empty())
            .ok_or("thread working directory is unknown")?,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    const IDLE: [SessionStatus; 3] = [
        SessionStatus::Idle,
        SessionStatus::Unknown,
        SessionStatus::Unavailable,
    ];
    const FOLLOW_UPS: [FollowUp; 2] = [FollowUp::Queue, FollowUp::Steer];

    fn execution(status: SessionStatus, running_turn: Option<&str>) -> Execution<'_> {
        Execution {
            status,
            running_turn,
            steers: true,
            answered: false,
            ahead: false,
        }
    }

    #[test]
    fn idle_input_starts_unless_queued_input_waits_before_it() {
        for status in IDLE {
            for follow_up in FOLLOW_UPS {
                for steers in [false, true] {
                    let route = |ahead, cwd| {
                        let execution = Execution {
                            steers,
                            ahead,
                            ..execution(status, Some("historical"))
                        };
                        submission_route(execution, follow_up, cwd)
                    };
                    assert_eq!(
                        route(false, Some("/project")).unwrap(),
                        Route::Provider(SubmissionTarget::Start { cwd: "/project" })
                    );
                    assert!(route(false, Some(" ")).is_err());
                    assert_eq!(route(true, Some("/project")).unwrap(), Route::Queue);
                }
            }
        }
    }

    #[test]
    fn running_input_steers_only_when_asked_supported_and_the_turn_is_known() {
        for (follow_up, steers, turn, expected) in [
            (
                FollowUp::Steer,
                true,
                Some("live"),
                Route::Provider(SubmissionTarget::Steer("live")),
            ),
            (FollowUp::Steer, true, None, Route::Queue),
            (FollowUp::Steer, false, Some("live"), Route::Queue),
            (FollowUp::Queue, true, Some("live"), Route::Queue),
            (FollowUp::Queue, false, None, Route::Queue),
        ] {
            for ahead in [false, true] {
                let execution = Execution {
                    steers,
                    ahead,
                    ..execution(SessionStatus::Running, turn)
                };
                assert_eq!(
                    submission_route(execution, follow_up, None).unwrap(),
                    expected
                );
            }
        }
    }

    #[test]
    fn an_answered_turn_takes_the_next_prompt_behind_queued_input() {
        for follow_up in FOLLOW_UPS {
            for (ahead, expected) in [
                (false, Route::Provider(SubmissionTarget::Next)),
                (true, Route::Queue),
            ] {
                let execution = Execution {
                    answered: true,
                    ahead,
                    ..execution(SessionStatus::Running, Some("live"))
                };
                assert_eq!(
                    submission_route(execution, follow_up, None).unwrap(),
                    expected
                );
            }
        }
    }

    proptest::proptest! {
        #[test]
        fn running_input_never_starts_a_second_turn(
            steer in proptest::bool::ANY,
            steers in proptest::bool::ANY,
            answered in proptest::bool::ANY,
            ahead in proptest::bool::ANY,
            turn in proptest::option::of("[a-z]{1,24}"),
        ) {
            let follow_up = if steer { FollowUp::Steer } else { FollowUp::Queue };
            let execution = Execution {
                status: SessionStatus::Running,
                running_turn: turn.as_deref(),
                steers,
                answered,
                ahead,
            };
            match submission_route(execution, follow_up, Some("/project")) {
                Ok(Route::Provider(SubmissionTarget::Steer(id))) => {
                    proptest::prop_assert!(steer && steers && !answered);
                    proptest::prop_assert_eq!(Some(id), turn.as_deref());
                }
                Ok(Route::Provider(SubmissionTarget::Next)) => {
                    proptest::prop_assert!(answered && !ahead)
                }
                Ok(Route::Queue) => {
                    let waits = if answered { ahead } else { !steer || !steers || turn.is_none() };
                    proptest::prop_assert!(waits)
                }
                Ok(Route::Provider(SubmissionTarget::Start { .. })) => {
                    proptest::prop_assert!(false, "running execution must not start another turn")
                }
                Err(_) => proptest::prop_assert!(false, "running execution accepts follow-up input"),
            }
        }
    }
}
