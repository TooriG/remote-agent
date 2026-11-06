//! Host-owned organization for public conversation identities, including native-only sessions.
use super::*;
use crate::host_rpc::database::{append_event, ensure_thread, failure, session_ref};
use agent_protocol::operations::SessionSettlement;
use rusqlite::params;
use std::collections::BTreeSet;

impl SessionRouter {
    pub(in crate::host_rpc) fn settlement_snapshot(
        &self,
    ) -> Result<(u64, BTreeSet<SessionRef>), Failure> {
        let state = lock_state(&self.state);
        let sessions = state
            .database
            .with(|connection| {
                let mut rows = connection
                    .prepare(
                        "SELECT provider, id FROM threads WHERE settled = 1 ORDER BY provider, id",
                    )
                    .map_err(failure)?;
                rows.query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(failure)?
                .map(|row| {
                    let (provider, id) = row.map_err(failure)?;
                    session_ref(provider, id)
                })
                .collect::<Result<BTreeSet<_>, String>>()
            })
            .map_err(|error| Failure::new("settlement_storage_failed", error))?;
        Ok((state.list_revision, sessions))
    }

    pub(super) fn settlement_state(
        &self,
        session: &SessionRef,
    ) -> Result<SessionSettlement, Failure> {
        use rusqlite::OptionalExtension;
        lock_state(&self.state).database.with(|connection| {
            let (settled, revision): (bool, i64) = connection.query_row("SELECT settled, list_revision FROM threads WHERE provider = ?1 AND id = ?2", params![session.provider.key(), session.id], |row| Ok((row.get(0)?, row.get(1)?))).optional().map_err(failure)?.unwrap_or((false, 0));
            Ok(SessionSettlement { session: session.clone(), settled, revision: revision as u64 })
        }).map_err(|error| Failure::new("settlement_storage_failed", error))
    }

    #[cfg(test)]
    pub(in crate::host_rpc) fn is_settled(&self, session: &SessionRef) -> Result<bool, Failure> {
        self.settlement_state(session).map(|state| state.settled)
    }

    /// Callers serialize this with input admission using the conversation's submission lock.
    pub(in crate::host_rpc) fn set_settled(
        &self,
        session: &SessionRef,
        settled: bool,
    ) -> Result<SessionSettlement, Failure> {
        let mut state = lock_state(&self.state);
        let revision = state.database.write(|transaction| {
            ensure_thread(transaction, session)?;
            let (previous, revision): (bool, i64) = transaction.query_row(
                "SELECT settled, list_revision FROM threads WHERE provider = ?1 AND id = ?2", params![session.provider.key(), session.id], |row| Ok((row.get(0)?, row.get(1)?)),
            ).map_err(failure)?;
            if previous == settled { return Ok(revision as u64); }
            let revision = append_event(transaction, session, "thread.settled", &settled)?;
            transaction.execute("UPDATE threads SET settled = ?3, list_revision = ?4 WHERE provider = ?1 AND id = ?2", params![session.provider.key(), session.id, settled, revision as i64]).map_err(failure)?;
            Ok(revision)
        }).map_err(|error| Failure::new("settlement_storage_failed", error))?;
        state.list_revision = state.list_revision.max(revision);
        let change = SessionSettlement {
            session: session.clone(),
            settled,
            revision,
        };
        // Enqueue under the owning lock so two clients observe the same revision order.
        let frame = protocol::encode(Notification::SessionSettlement {
            change: change.clone(),
        })
        .expect("settlement encodes");
        let failed = state
            .sessions
            .iter()
            .filter_map(|(id, output)| output.try_send(*id, frame.clone()).is_err().then_some(*id))
            .collect();
        drop(state);
        self.close_failed(failed);
        Ok(change)
    }

    pub(in crate::host_rpc) fn settle_if_idle(
        &self,
        session: &SessionRef,
        status: agent_protocol::models::SessionStatus,
        running_turn: bool,
        pending_request: bool,
        pending_input: bool,
    ) -> Result<SessionSettlement, Failure> {
        let actor = self.actor(session);
        let owned = lock_state(&actor);
        let stored_pending = lock_state(&self.state)
            .database
            .pending_submissions(session)
            .map_err(|error| Failure::new("settlement_storage_failed", error))?;
        let live = &owned.timeline;
        let status = if live.status == agent_protocol::models::SessionStatus::Unknown {
            status
        } else {
            live.status
        };
        let reason = agent_protocol::session::settlement_unavailable_reason(
            status,
            running_turn
                || live
                    .turns
                    .iter()
                    .flatten()
                    .any(|turn| turn.status == agent_protocol::models::TurnStatus::Running),
            pending_request || !live.requests.is_empty(),
            pending_input
                || !stored_pending.is_empty()
                || live.submissions.values().any(|delivery| {
                    *delivery != agent_protocol::session::SubmissionDelivery::Rejected
                }),
        );
        if let Some(reason) = reason {
            return Err(Failure::new("settlement_unavailable", reason));
        }
        self.set_settled(session, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_rpc::{database::Database, transcripts::Transcripts};
    use agent_protocol::{execution::SessionStatus, session::SubmissionDelivery};

    #[tokio::test]
    async fn commit_precedes_all_client_notices_and_failed_writes_publish_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.sqlite");
        let database = Arc::new(Database::open(&path));
        let router =
            SessionRouter::new().with_transcripts(Arc::new(Transcripts::new(database.clone())));
        let id = SessionRef::new(ProviderKind::Codex, "thread".into()).unwrap();
        let mut first = router.open_session();
        let mut second = router.open_session();
        let settled = router.set_settled(&id, true).unwrap();
        for client in [&mut first, &mut second] {
            let notice: Notification = protocol::decode(&client.recv().await.unwrap()).unwrap();
            assert!(
                matches!(notice, Notification::SessionSettlement { change } if change == settled)
            );
        }
        let reopened = SessionRouter::new().with_transcripts(Arc::new(Transcripts::open(&path)));
        assert!(reopened.is_settled(&id).unwrap());
        assert_eq!(reopened.settlement_snapshot().unwrap().0, settled.revision);
        database.with(|db| db.execute_batch("CREATE TRIGGER fail_settle BEFORE UPDATE ON threads BEGIN SELECT RAISE(ABORT, 'failed'); END").map_err(failure)).unwrap();
        assert!(router.set_settled(&id, false).is_err());
        assert!(router.is_settled(&id).unwrap());
        assert_eq!(router.settlement_snapshot().unwrap().0, settled.revision);
        for client in [&first, &second] {
            assert_eq!(client.queued_bytes.load(Relaxed), 0);
        }
        database
            .with(|db| {
                db.execute_batch("DROP TRIGGER fail_settle")
                    .map_err(failure)
            })
            .unwrap();
        let active = router.set_settled(&id, false).unwrap();
        assert!(active.revision > settled.revision);
    }

    #[test]
    fn restart_preserves_receipts_and_unknown_inputs_block_settlement() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.sqlite");
        let id = SessionRef::new(ProviderKind::Claude, "thread".into()).unwrap();
        let router = SessionRouter::new().with_transcripts(Arc::new(Transcripts::open(&path)));
        router.set_settled(&id, true).unwrap();
        router.begin_submission(&id, "command", None).unwrap();
        assert!(!router.is_settled(&id).unwrap());
        drop(router);
        let router = SessionRouter::new().with_transcripts(Arc::new(Transcripts::open(&path)));
        assert!(router.begin_submission(&id, "command", None).is_err());
        assert_eq!(
            lock_state(&router.actor(&id)).timeline.submissions["command"],
            SubmissionDelivery::Unknown
        );
        assert!(
            router
                .settle_if_idle(&id, SessionStatus::Idle, false, false, false)
                .is_err()
        );
        router
            .finish_submission(&id, "command", SubmissionDelivery::Rejected)
            .unwrap();
        router
            .settle_if_idle(&id, SessionStatus::Idle, false, false, false)
            .unwrap();
        router.begin_submission(&id, "retry", None).unwrap();
        router
            .finish_submission(
                &id,
                "retry",
                SubmissionDelivery::Accepted {
                    turn_id: Some("turn".into()),
                },
            )
            .unwrap();
        let mut user = agent_protocol::models::Item::new(
            "user".into(),
            agent_protocol::execution::ItemStatus::Completed,
            agent_protocol::items::ItemBody::UserMessage {
                text: Some("retry".into()),
                content: Vec::new(),
            },
        );
        user.client_input_id = Some("retry".into());
        let observed = agent_protocol::models::Turn {
            id: "observed".into(),
            status: agent_protocol::execution::TurnStatus::Completed,
            items: Some(vec![Arc::new(user)]),
            ..Default::default()
        };
        router.confirm_submission(&id, "retry", &observed).unwrap();
        router
            .finish_submission(
                &id,
                "retry",
                SubmissionDelivery::Accepted {
                    turn_id: Some("stale-ack".into()),
                },
            )
            .unwrap();
        router
            .settle_if_idle(&id, SessionStatus::Idle, false, false, false)
            .unwrap();
        drop(router);
        let router = SessionRouter::new().with_transcripts(Arc::new(Transcripts::open(&path)));
        assert_eq!(
            router
                .submission_receipt(&id, "retry")
                .unwrap()
                .unwrap()
                .turn_id
                .unwrap()
                .as_str(),
            "observed"
        );
    }

    #[test]
    fn admission_rollback_keeps_settlement_without_a_receipt_or_event() {
        let router = SessionRouter::new();
        let id = SessionRef::new(ProviderKind::Codex, "thread".into()).unwrap();
        let settled = router.set_settled(&id, true).unwrap();
        let database = lock_state(&router.state).database.clone();
        database.with(|db| db.execute_batch("CREATE TRIGGER fail_receipt BEFORE INSERT ON command_receipts BEGIN SELECT RAISE(ABORT, 'failed'); END").map_err(failure)).unwrap();
        assert!(router.begin_submission(&id, "command", None).is_err());
        assert!(router.is_settled(&id).unwrap());
        assert!(
            database
                .submission_delivery(&id, "command")
                .unwrap()
                .is_none()
        );
        let sequence = database
            .with(|db| {
                db.query_row("SELECT MAX(sequence) FROM events", [], |row| {
                    row.get::<_, i64>(0)
                })
                .map_err(failure)
            })
            .unwrap();
        assert_eq!(sequence as u64, settled.revision);
    }
    #[test]
    fn completion_read_receipts_and_organization_remain_independent() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.sqlite");
        let router = SessionRouter::new().with_transcripts(Arc::new(Transcripts::open(&path)));
        let id = SessionRef::new(ProviderKind::Codex, "thread".into()).unwrap();
        router.task_activity(
            vec![Thread {
                id: Some(id.clone()),
                status: SessionStatus::Idle,
                ..Default::default()
            }],
            0,
        );
        router.session_change(
            &id,
            SessionChange::Turn {
                turn: agent_protocol::models::Turn {
                    id: "turn".into(),
                    status: agent_protocol::execution::TurnStatus::Completed,
                    ..Default::default()
                },
                completed: true,
            },
        );
        assert!(!router.is_settled(&id).unwrap());
        router
            .settle_if_idle(&id, SessionStatus::Idle, false, false, false)
            .unwrap();
        assert_eq!(router.task_activity(Vec::new(), 0).unread.len(), 1);
        router
            .mark_session_read(&agent_protocol::live_activity::MarkSessionRead {
                session: id.clone(),
                turn_id: "turn".into(),
            })
            .unwrap();
        assert!(router.is_settled(&id).unwrap());
        let restarted = SessionRouter::new().with_transcripts(Arc::new(Transcripts::open(&path)));
        assert!(restarted.is_settled(&id).unwrap());
        assert!(restarted.task_activity(Vec::new(), 0).unread.is_empty());
    }
    #[test]
    fn provider_completion_remains_live_when_transcript_storage_fails() {
        let database = Arc::new(Database::default());
        let router =
            SessionRouter::new().with_transcripts(Arc::new(Transcripts::new(database.clone())));
        let id = SessionRef::new(ProviderKind::Codex, "thread".into()).unwrap();
        router.task_activity(
            vec![Thread {
                id: Some(id.clone()),
                status: SessionStatus::Idle,
                ..Default::default()
            }],
            0,
        );
        database.with(|db| db.execute_batch("CREATE TRIGGER fail_turn BEFORE INSERT ON turns BEGIN SELECT RAISE(ABORT, 'failed'); END").map_err(failure)).unwrap();
        router.session_change(
            &id,
            SessionChange::Turn {
                turn: agent_protocol::models::Turn {
                    id: "completed".into(),
                    status: agent_protocol::execution::TurnStatus::Completed,
                    ..Default::default()
                },
                completed: true,
            },
        );
        assert_eq!(
            router.task_activity(Vec::new(), 0).unread,
            [(id, "completed".into())]
        );
    }
}
