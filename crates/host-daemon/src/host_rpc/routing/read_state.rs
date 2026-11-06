//! Completion receipts belong to the Host, including while clients are offline.
use super::*;
use crate::host_rpc::database::{Database, append_event, ensure_thread, failure, session_ref};
use agent_protocol::ids::TurnId;
use rusqlite::params;
use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq, Serialize, Deserialize)]
struct Completion {
    session: SessionRef,
    turn_id: TurnId,
    unread: bool,
}

pub(super) struct ReadState {
    database: Option<Arc<Database>>,
    completions: Result<Vec<Completion>, String>,
}
impl Default for ReadState {
    fn default() -> Self {
        Self {
            database: None,
            completions: Ok(Vec::new()),
        }
    }
}
impl ReadState {
    pub(super) fn new(database: Arc<Database>) -> Self {
        let completions = database.with(|connection| {
            let mut rows = connection.prepare("SELECT provider, id, completed_turn_id, unread FROM threads WHERE completed_turn_id IS NOT NULL ORDER BY provider, id").map_err(failure)?;
            rows.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, bool>(3)?)))
                .map_err(failure)?.map(|row| {
                    let (provider, id, turn, unread) = row.map_err(failure)?;
                    Ok(Completion { session: session_ref(provider, id)?, turn_id: turn.into(), unread })
                }).collect()
        });
        Self {
            database: Some(database),
            completions,
        }
    }
    #[cfg(test)]
    fn load(path: std::path::PathBuf) -> Self {
        Self::new(Arc::new(Database::open(&path)))
    }
    pub(super) fn check(&self) -> Result<(), Failure> {
        self.completions
            .as_ref()
            .map(|_| ())
            .map_err(|error| Failure::new("read_state_storage_failed", error))
    }
    pub(super) fn unread(&self) -> Vec<(SessionRef, TurnId)> {
        self.completions
            .as_ref()
            .into_iter()
            .flatten()
            .filter(|completion| completion.unread)
            .map(|completion| (completion.session.clone(), completion.turn_id.clone()))
            .collect()
    }
    fn save(&self, completions: &[Completion]) -> Result<(), Failure> {
        let Some(database) = &self.database else {
            return Ok(());
        };
        database.write(|transaction| {
            // Compare durable rows, so a later successful write retries live facts
            // kept through a temporary storage failure.
            let stored = {
                let mut rows = transaction.prepare("SELECT provider, id FROM threads WHERE completed_turn_id IS NOT NULL").map_err(failure)?;
                rows.query_map([], |row| Ok((row.get(0)?, row.get(1)?))).map_err(failure)?
                    .map(|row| { let (provider, id) = row.map_err(failure)?; session_ref(provider, id) }).collect::<Result<Vec<_>, String>>()?
            };
            for old in stored.iter().filter(|old| !completions.iter().any(|next| next.session == **old)) {
                transaction.execute("UPDATE threads SET completed_turn_id = NULL, unread = 0 WHERE provider = ?1 AND id = ?2", params![old.provider.key(), old.id]).map_err(failure)?;
                append_event(transaction, old, "thread.completion-forgotten", &())?;
            }
            for next in completions {
                ensure_thread(transaction, &next.session)?;
                let changed = transaction.execute("UPDATE threads SET completed_turn_id = ?3, unread = ?4 WHERE provider = ?1 AND id = ?2 AND (completed_turn_id IS NOT ?3 OR unread != ?4)", params![next.session.provider.key(), next.session.id, next.turn_id.as_str(), next.unread]).map_err(failure)?;
                if changed > 0 { append_event(transaction, &next.session, "thread.read-state", &(next.turn_id.as_str(), next.unread))?; }
            }
            Ok(())
        }).map_err(|error| Failure::new("read_state_storage_failed", error))
    }

    pub(super) fn record(
        &mut self,
        session: &SessionRef,
        active: bool,
        completed: Option<&TurnId>,
    ) -> bool {
        let Ok(completions) = &self.completions else {
            return false;
        };
        let previous = completions
            .iter()
            .find(|completion| &completion.session == session);
        if !completed.is_some_and(|turn| previous.is_none_or(|old| &old.turn_id != turn))
            && !previous.is_some_and(|old| active && old.unread)
        {
            return false;
        }
        let completion = Completion {
            session: session.clone(),
            turn_id: completed
                .or_else(|| previous.map(|old| &old.turn_id))
                .unwrap()
                .clone(),
            unread: !active,
        };
        let mut next = completions.clone();
        if let Some(previous) = next.iter_mut().find(|old| &old.session == session) {
            *previous = completion;
        } else {
            next.push(completion);
        }
        self.update(next);
        true
    }
    fn update(&mut self, next: Vec<Completion>) {
        // Live facts survive temporary disk failures; read receipts only clear
        // a badge after their write succeeds.
        if let Err(error) = self.save(&next) {
            tracing::warn!(target: "agent", operation = "host.read_state.record", message = %error);
        }
        self.completions = Ok(next);
    }
    pub(super) fn mark_read(
        &mut self,
        session: &SessionRef,
        turn_id: &TurnId,
    ) -> Result<bool, Failure> {
        self.check()?;
        let mut next = self.completions.as_ref().unwrap().clone();
        let Some(completion) = next.iter_mut().find(|completion| {
            &completion.session == session && &completion.turn_id == turn_id && completion.unread
        }) else {
            return Ok(false);
        };
        completion.unread = false;
        self.save(&next)?;
        self.completions = Ok(next);
        Ok(true)
    }
}

impl SessionRouter {
    #[cfg(test)]
    pub(in crate::host_rpc) fn with_read_state(self, path: std::path::PathBuf) -> Self {
        let database = Arc::new(Database::open(&path));
        let mut state = lock_state(&self.state);
        state.reads = ReadState::new(database.clone());
        state.database = database;
        drop(state);
        self
    }
    pub(in crate::host_rpc) fn check_read_state(&self) -> Result<(), Failure> {
        lock_state(&self.state).reads.check()
    }
    pub(in crate::host_rpc) fn mark_session_read(
        &self,
        params: &agent_protocol::live_activity::MarkSessionRead,
    ) -> Result<(), Failure> {
        let mut state = lock_state(&self.state);
        if !state.reads.mark_read(&params.session, &params.turn_id)? {
            return Ok(());
        }
        state.task_revision += 1;
        let activity =
            activity::task_activity_state(state.task_revision, &state.tasks, state.reads.unread());
        let failed =
            activity::publish_task_activity(state.apns.as_deref(), &state.sessions, &activity);
        drop(state);
        self.close_failed(failed);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_fresh_running_list_clears_a_saved_completion() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.sqlite");
        let router = SessionRouter::new().with_read_state(path.clone());
        let id = SessionRef::new(ProviderKind::Codex, "task".into()).unwrap();
        router.task_activity(
            vec![Thread {
                id: Some(id.clone()),
                ..Default::default()
            }],
            0,
        );
        let complete = |turn: &str| {
            router.session_change(
                &id,
                SessionChange::Turn {
                    turn: agent_protocol::models::Turn {
                        id: turn.into(),
                        status: agent_protocol::execution::TurnStatus::Completed,
                        ..Default::default()
                    },
                    completed: true,
                },
            )
        };
        complete("first");
        let revision = router.task_activity_revision();
        complete("second");
        let running = Thread {
            id: Some(id.clone()),
            status: agent_protocol::models::SessionStatus::Running,
            ..Default::default()
        };
        let late = router.task_activity(vec![running.clone()], revision);
        assert_eq!(late.unread, vec![(id.clone(), "second".into())]);
        let fresh = router.task_activity(vec![running], late.revision);
        assert!(fresh.unread.is_empty());
        assert!(ReadState::load(path).unread().is_empty());
    }

    #[test]
    fn receipts_survive_restart_and_do_not_hide_newer_or_other_provider_completions() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.sqlite");
        let codex = SessionRef::new(ProviderKind::Codex, "same".into()).unwrap();
        let claude = SessionRef::new(ProviderKind::Claude, "same".into()).unwrap();
        let first = "first".into();
        let second = "second".into();
        let mut state = ReadState::load(path.clone());
        assert!(state.record(&codex, false, Some(&first)));
        assert!(state.record(&claude, false, Some(&first)));
        assert!(state.mark_read(&codex, &first).unwrap());
        assert!(
            !state.record(&codex, false, Some(&first)),
            "duplicate completion must stay read"
        );
        assert_eq!(
            ReadState::load(path.clone()).unread(),
            vec![(claude.clone(), first.clone())]
        );
        assert!(state.record(&codex, false, Some(&second)));
        assert!(
            !state.mark_read(&codex, &first).unwrap(),
            "a delayed receipt cannot clear new work"
        );
        assert_eq!(ReadState::load(path.clone()).unread().len(), 2);
        assert!(state.mark_read(&claude, &first).unwrap());
        assert_eq!(
            ReadState::load(path.clone()).unread(),
            vec![(codex.clone(), second.clone())]
        );
        assert!(
            state.record(&codex, true, None),
            "starting new work clears the old badge"
        );
        assert!(ReadState::load(path).unread().is_empty());
    }

    #[test]
    fn subagent_completions_never_survive_as_standalone_unread_results() {
        use agent_protocol::{execution::TurnStatus, models::SessionStatus};
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.sqlite");
        let router = SessionRouter::new().with_read_state(path.clone());
        let root = SessionRef::new(ProviderKind::Codex, "root".into()).unwrap();
        let child = SessionRef::new(ProviderKind::Codex, "child".into()).unwrap();
        let conversation = Thread {
            id: Some(root.clone()),
            status: SessionStatus::Idle,
            ..Default::default()
        };
        router.task_activity(vec![conversation.clone()], 0);
        for session in [&root, &child] {
            router.session_change(
                session,
                SessionChange::Turn {
                    turn: agent_protocol::models::Turn {
                        id: "completed".into(),
                        status: TurnStatus::Completed,
                        ..Default::default()
                    },
                    completed: true,
                },
            );
        }
        let expected = vec![(root, "completed".into())];
        assert_eq!(router.task_activity(Vec::new(), 0).unread, expected);
        assert_eq!(ReadState::load(path.clone()).unread(), expected);
        let restarted = SessionRouter::new().with_read_state(path);
        let state = restarted.task_activity(vec![conversation], 0);
        assert_eq!(state.unread, expected);
        assert_eq!(state.display.current.total, 1);
        assert!(!state.display.ongoing);
    }

    #[test]
    fn a_failed_receipt_write_preserves_unread_and_can_be_retried() {
        let directory = tempfile::tempdir().unwrap();
        let parent = directory.path().join("state");
        let path = parent.join("state.sqlite");
        let id = SessionRef::new(ProviderKind::Claude, "task".into()).unwrap();
        let turn = "completed".into();
        let mut state = ReadState::load(path.clone());
        assert!(state.record(&id, false, Some(&turn)));
        state.database.as_ref().unwrap().with(|db| db.execute_batch("CREATE TRIGGER fail_read BEFORE UPDATE ON threads BEGIN SELECT RAISE(ABORT, 'injected failure'); END").map_err(failure)).unwrap();
        assert!(state.mark_read(&id, &turn).is_err());
        assert_eq!(state.unread(), vec![(id.clone(), turn.clone())]);
        state
            .database
            .as_ref()
            .unwrap()
            .with(|db| db.execute_batch("DROP TRIGGER fail_read").map_err(failure))
            .unwrap();
        assert!(state.mark_read(&id, &turn).unwrap());
        assert!(ReadState::load(path).unread().is_empty());
    }

    #[test]
    fn corrupted_receipts_are_reported_and_never_overwritten() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.sqlite");
        std::fs::write(&path, b"damaged").unwrap();
        let mut state = ReadState::load(path.clone());
        let id = SessionRef::new(ProviderKind::Codex, "task".into()).unwrap();
        assert!(state.check().is_err());
        assert!(state.mark_read(&id, &"turn".into()).is_err());
        assert!(!state.record(&id, false, Some(&"turn".into())));
        assert_eq!(std::fs::read(path).unwrap(), b"damaged");
    }
    #[test]
    fn later_writes_retry_live_completions_kept_through_a_storage_failure() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.sqlite");
        let mut reads = ReadState::load(path.clone());
        let database = reads.database.clone().unwrap();
        database.with(|db| db.execute_batch("CREATE TRIGGER fail_completion BEFORE UPDATE ON threads BEGIN SELECT RAISE(ABORT, 'failed'); END").map_err(failure)).unwrap();
        let first = SessionRef::new(ProviderKind::Codex, "first".into()).unwrap();
        let second = SessionRef::new(ProviderKind::Claude, "second".into()).unwrap();
        assert!(reads.record(&first, false, Some(&"turn".into())));
        assert_eq!(reads.unread().len(), 1);
        assert!(ReadState::load(path.clone()).unread().is_empty());
        database
            .with(|db| {
                db.execute_batch("DROP TRIGGER fail_completion")
                    .map_err(failure)
            })
            .unwrap();
        assert!(reads.record(&second, false, Some(&"other".into())));
        assert_eq!(ReadState::load(path).unread().len(), 2);
    }
}
