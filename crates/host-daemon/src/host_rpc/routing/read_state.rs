//! Completion receipts belong to the Host, including while clients are offline.
use super::*;
use agent_protocol::ids::TurnId;
use serde::{Deserialize, Serialize};
use std::{io::Write, path::PathBuf};

#[derive(Clone, Serialize, Deserialize)]
struct Completion {
    session: SessionRef,
    turn_id: TurnId,
    unread: bool,
}

pub(super) struct ReadState {
    path: Option<PathBuf>,
    completions: Result<Vec<Completion>, String>,
}
impl Default for ReadState {
    fn default() -> Self {
        Self {
            path: None,
            completions: Ok(Vec::new()),
        }
    }
}
impl ReadState {
    pub(super) fn load(path: PathBuf) -> Self {
        let completions = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| error.to_string()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(error.to_string()),
        };
        Self {
            path: Some(path),
            completions,
        }
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
        let Some(path) = &self.path else {
            return Ok(());
        };
        let bytes = serde_json::to_vec(completions)?;
        let write = || -> anyhow::Result<()> {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            atomicwrites::AtomicFile::new(path, atomicwrites::AllowOverwrite).write(|file| {
                file.write_all(&bytes)?;
                file.sync_all()
            })?;
            Ok(())
        };
        write().map_err(|error| Failure::new("read_state_storage_failed", error))
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
        // Keep live completion facts even if disk is temporarily unavailable.
        // Receipts, in contrast, only clear a badge after their write succeeds.
        if let Err(error) = self.save(&next) {
            tracing::warn!(target: "agent", operation = "host.read_state.record", message = %error);
        }
        self.completions = Ok(next);
        true
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
    pub(in crate::host_rpc) fn with_read_state(self, path: PathBuf) -> Self {
        lock_state(&self.state).reads = ReadState::load(path);
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
        let path = directory.path().join("reads.json");
        let router = SessionRouter::new().with_read_state(path.clone());
        let id = SessionRef::new(ProviderKind::Codex, "task".into()).unwrap();
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
        let path = directory.path().join("reads.json");
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
    fn a_failed_receipt_write_preserves_unread_and_can_be_retried() {
        let directory = tempfile::tempdir().unwrap();
        let parent = directory.path().join("state");
        let path = parent.join("reads.json");
        let id = SessionRef::new(ProviderKind::Claude, "task".into()).unwrap();
        let turn = "completed".into();
        let mut state = ReadState::load(path.clone());
        assert!(state.record(&id, false, Some(&turn)));
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&parent).unwrap();
        std::fs::write(&parent, b"blocked").unwrap();
        assert!(state.mark_read(&id, &turn).is_err());
        assert_eq!(state.unread(), vec![(id.clone(), turn.clone())]);
        std::fs::remove_file(&parent).unwrap();
        assert!(state.mark_read(&id, &turn).unwrap());
        assert!(ReadState::load(path).unread().is_empty());
    }

    #[test]
    fn corrupted_receipts_are_reported_and_never_overwritten() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("reads.json");
        std::fs::write(&path, b"damaged").unwrap();
        let mut state = ReadState::load(path.clone());
        let id = SessionRef::new(ProviderKind::Codex, "task".into()).unwrap();
        assert!(state.check().is_err());
        assert!(state.mark_read(&id, &"turn".into()).is_err());
        assert!(!state.record(&id, false, Some(&"turn".into())));
        assert_eq!(std::fs::read(path).unwrap(), b"damaged");
    }
}
