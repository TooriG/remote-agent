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
    /// Other input already waits in the queue, which this input does not overtake.
    pub ahead: bool,
}

/// Steering joins the running turn at once, as in T3 Code; a provider that
/// cannot steer, or a turn whose ID is not known yet, queues the input. Input
/// to a conversation with no running turn starts one, behind input already
/// queued: a provider whose answered turn keeps background work running (as
/// Claude's does) takes it into that process as its next turn.
pub(super) fn submission_route<'a>(
    execution: Execution<'a>,
    follow_up: FollowUp,
    cwd: Option<&'a str>,
) -> Result<Route<'a>, &'static str> {
    if execution.status == SessionStatus::Running {
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

    proptest::proptest! {
        #[test]
        fn running_input_never_starts_a_second_turn(
            steer in proptest::bool::ANY,
            steers in proptest::bool::ANY,
            ahead in proptest::bool::ANY,
            turn in proptest::option::of("[a-z]{1,24}"),
        ) {
            let follow_up = if steer { FollowUp::Steer } else { FollowUp::Queue };
            let execution = Execution {
                status: SessionStatus::Running,
                running_turn: turn.as_deref(),
                steers,
                ahead,
            };
            match submission_route(execution, follow_up, Some("/project")) {
                Ok(Route::Provider(SubmissionTarget::Steer(id))) => {
                    proptest::prop_assert!(steer && steers);
                    proptest::prop_assert_eq!(Some(id), turn.as_deref());
                }
                Ok(Route::Queue) => {
                    let waits = !steer || !steers || turn.is_none();
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
