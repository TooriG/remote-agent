use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Unreported,
    AwaitingResponse,
    Responding,
    Complete,
    Ignored,
}

/// SDK task levels and completion bookends can arrive in either order. A task
/// leaving the level is not proof that Claude has answered its completion.
#[derive(Default)]
pub(super) struct Reports {
    // Keys identify task runs (run_id, or task_id when the SDK has no run_id).
    // Terminal records prevent late edges from requiring the same report again.
    tasks: HashMap<String, Phase>,
}

impl Reports {
    pub(super) fn observe(mut self, tasks: impl IntoIterator<Item = (String, bool)>) -> Self {
        for (id, needs_report) in tasks {
            let phase = self.tasks.entry(id).or_insert(Phase::Unreported);
            if *phase == Phase::Complete {
                continue;
            }
            if !needs_report {
                *phase = Phase::Ignored;
            } else if *phase == Phase::Ignored {
                *phase = Phase::Unreported;
            }
        }
        self
    }

    pub(super) fn notified(mut self, id: &str, ambient: bool) -> Self {
        let phase = self
            .tasks
            .entry(id.into())
            .or_insert(Phase::AwaitingResponse);
        if ambient {
            *phase = Phase::Complete;
        } else if !matches!(*phase, Phase::Complete | Phase::Responding | Phase::Ignored) {
            *phase = Phase::AwaitingResponse;
        }
        self
    }

    pub(super) fn response_started(mut self, run: Option<&str>) -> Self {
        if let Some(id) = run {
            let phase = self.tasks.entry(id.into()).or_insert(Phase::Responding);
            if !matches!(*phase, Phase::Complete | Phase::Ignored) {
                *phase = Phase::Responding;
            }
        } else {
            for phase in self.tasks.values_mut() {
                if *phase == Phase::AwaitingResponse {
                    *phase = Phase::Responding;
                }
            }
        }
        self
    }

    pub(super) fn response_finished(mut self) -> Self {
        for phase in self.tasks.values_mut() {
            if *phase == Phase::Responding {
                *phase = Phase::Complete;
            }
        }
        self
    }

    pub(super) fn pending(&self) -> bool {
        self.tasks.values().any(|phase| {
            matches!(
                phase,
                Phase::Unreported | Phase::AwaitingResponse | Phase::Responding
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn running() -> Reports {
        Reports::default().observe(HashMap::from([("run".into(), true)]))
    }

    #[test]
    fn old_foreground_results_do_not_answer_pending_background_reports() {
        let work = running().observe(HashMap::new()).response_finished();
        assert!(work.pending());
        let work = work.notified("run", false).response_finished();
        assert!(work.pending());
        let work = work.response_started(None).response_finished();
        assert!(!work.pending());
        assert!(!work.notified("run", false).pending());
    }

    #[test]
    fn late_level_changes_do_not_restart_an_answered_report() {
        let work = running()
            .notified("run", false)
            .response_started(None)
            .observe(HashMap::new());
        assert!(work.pending());
        let work = work
            .response_finished()
            .observe(HashMap::from([("run".into(), true)]))
            .observe(HashMap::new());
        assert!(!work.pending());
        let work = work.observe(HashMap::from([("resumed-run".into(), true)]));
        assert!(work.notified("run", false).pending());
    }

    #[test]
    fn ambient_changes_do_not_require_a_report() {
        let work = running().observe(HashMap::from([("run".into(), false)]));
        assert!(!work.pending());
        assert!(
            work.observe(HashMap::from([("run".into(), true)]))
                .pending()
        );
    }

    #[test]
    fn consumed_reports_finish_only_their_own_task_run() {
        let work = Reports::default()
            .notified("one", false)
            .notified("two", false)
            .response_started(Some("one"))
            .response_finished();
        assert!(work.pending());
        assert!(
            !work
                .response_started(Some("two"))
                .response_finished()
                .pending()
        );
    }

    #[test]
    fn sdk_agents_and_silent_completions_do_not_need_another_report() {
        let work = Reports::default()
            .observe([("agent".into(), false)])
            .notified("agent", false);
        assert!(!work.pending());
        assert!(
            !work
                .observe(HashMap::from([("agent".into(), false)]))
                .observe(HashMap::new())
                .pending()
        );
        assert!(
            !running()
                .observe(HashMap::new())
                .notified("run", true)
                .pending()
        );
    }

    proptest! {
        #[test]
        fn only_reports_available_to_the_new_response_can_finish(
            notices in prop::collection::vec(any::<bool>(), 1..8),
            level_first in any::<bool>(),
        ) {
            let level = notices.iter().enumerate().map(|(i, _)| (i.to_string(), true));
            let mut work = Reports::default().observe(level);
            if level_first { work = work.observe(HashMap::new()); }
            for (i, noticed) in notices.iter().enumerate() {
                if *noticed { work = work.notified(&i.to_string(), false); }
            }
            work = work.response_started(None);
            if !level_first { work = work.observe(HashMap::new()); }
            work = work.response_finished();
            prop_assert_eq!(work.pending(), notices.iter().any(|noticed| !noticed));
        }

        #[test]
        fn late_metadata_cannot_require_an_answer_for_a_completed_run(
            changes in prop::collection::vec((any::<bool>(), any::<bool>(), any::<bool>()), 0..16),
        ) {
            let mut work = running().notified("run", false).response_started(None)
                .response_finished().observe(HashMap::new());
            for (ambient, independent, notification) in changes {
                work = if notification && independent { work.observe([("run".into(), false)]) }
                    else if notification { work.notified("run", ambient) }
                    else { work.observe(HashMap::from([("run".into(), !ambient && independent)])) };
            }
            prop_assert!(!work.observe(HashMap::new()).pending());
        }
    }
}
