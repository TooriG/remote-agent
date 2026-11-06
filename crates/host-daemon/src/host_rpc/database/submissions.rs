use super::*;
use agent_protocol::{operations::SessionSettlement, session::SubmissionDelivery};
use rusqlite::OptionalExtension;
use std::collections::BTreeMap;

fn delivery(value: String, turn: Option<String>) -> Result<SubmissionDelivery, String> {
    match value.as_str() {
        "sending" => Ok(SubmissionDelivery::Sending),
        "accepted" => Ok(SubmissionDelivery::Accepted {
            turn_id: turn.map(Into::into),
        }),
        "unknown" => Ok(SubmissionDelivery::Unknown),
        "rejected" => Ok(SubmissionDelivery::Rejected),
        _ => Err("Invalid stored submission delivery".into()),
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct InputPayload<'a> {
    input: &'a [agent_protocol::operations::Input],
    model: Option<&'a agent_protocol::models::ModelRef>,
    effort: Option<&'a str>,
    service_tier: Option<&'a str>,
}

impl Database {
    pub(in crate::host_rpc) fn submission_delivery(
        &self,
        session: &SessionRef,
        id: &str,
    ) -> Result<Option<SubmissionDelivery>, String> {
        self.with(|connection| {
            connection.query_row("SELECT delivery, turn_id FROM command_receipts WHERE provider = ?1 AND id = ?2 AND command_id = ?3 AND kind = 'submission'", params![session.provider.key(), session.id, id], |row| Ok((row.get(0)?, row.get(1)?)))
                .optional().map_err(failure)?.map(|(state, turn)| delivery(state, turn)).transpose()
        })
    }

    pub(in crate::host_rpc) fn pending_submissions(
        &self,
        session: &SessionRef,
    ) -> Result<BTreeMap<agent_protocol::ids::ClientInputId, SubmissionDelivery>, String> {
        self.with(|connection| {
            let mut statement = connection.prepare("SELECT command_id, delivery, turn_id FROM command_receipts WHERE provider = ?1 AND id = ?2 AND kind = 'submission' AND resolved = 0 AND delivery != 'rejected'").map_err(failure)?;
            statement.query_map(params![session.provider.key(), session.id], |row| Ok((row.get::<_, String>(0)?, row.get(1)?, row.get(2)?)))
                .map_err(failure)?.map(|row| {
                    let (id, state, turn) = row.map_err(failure)?;
                    let state = delivery(state, turn)?;
                    Ok((id.into(), if state == SubmissionDelivery::Sending { SubmissionDelivery::Unknown } else { state }))
                }).collect()
        })
    }

    /// Admission and returning the thread to Active are one durable decision.
    pub(in crate::host_rpc) fn admit_submission(
        &self,
        session: &SessionRef,
        id: &str,
        input: Option<&agent_protocol::operations::Submission>,
    ) -> Result<Option<SessionSettlement>, String> {
        let payload = InputPayload {
            input: input.map_or(&[], |input| input.input.as_slice()),
            model: input.and_then(|input| input.model.as_ref()),
            effort: input.and_then(|input| input.effort.as_deref()),
            service_tier: input.and_then(|input| input.service_tier.as_deref()),
        };
        self.write(|transaction| {
            ensure_thread(transaction, session)?;
            let previous: Option<String> = transaction.query_row("SELECT delivery FROM command_receipts WHERE provider = ?1 AND id = ?2 AND command_id = ?3", params![session.provider.key(), session.id, id], |row| row.get(0)).optional().map_err(failure)?;
            if previous.is_some_and(|state| state != "rejected") { return Err("submission is already admitted; read its outcome before sending again".into()); }
            let settled: bool = transaction.query_row("SELECT settled FROM threads WHERE provider = ?1 AND id = ?2", params![session.provider.key(), session.id], |row| row.get(0)).map_err(failure)?;
            let revision = append_event(transaction, session, "input.admitted", &serde_json::json!({"commandId": id, "input": payload}))?;
            transaction.execute("INSERT INTO command_receipts (provider, id, command_id, kind, delivery, result_sequence, payload_json) VALUES (?1, ?2, ?3, 'submission', 'sending', ?4, ?5)
                ON CONFLICT(provider, id, command_id) DO UPDATE SET delivery = 'sending', resolved = 0, turn_id = NULL, result_sequence = excluded.result_sequence, payload_json = excluded.payload_json",
                params![session.provider.key(), session.id, id, revision as i64, serde_json::to_string(&payload).map_err(failure)?]).map_err(failure)?;
            transaction.execute("UPDATE threads SET settled = 0, list_revision = ?3 WHERE provider = ?1 AND id = ?2", params![session.provider.key(), session.id, revision as i64]).map_err(failure)?;
            Ok(settled.then(|| SessionSettlement { session: session.clone(), settled: false, revision }))
        })
    }

    /// Provider history can confirm an initial input whose acknowledgement was lost.
    pub(in crate::host_rpc) fn confirm_submission(
        &self,
        session: &SessionRef,
        id: &str,
        turn: &agent_protocol::ids::TurnId,
    ) -> Result<(), String> {
        self.write(|transaction| {
            let sequence = append_event(transaction, session, "input.confirmed", &(id, turn))?;
            transaction.execute(
                "INSERT INTO command_receipts (provider, id, command_id, kind, delivery, turn_id, resolved, result_sequence, payload_json)
                 VALUES (?1, ?2, ?3, 'submission', 'accepted', ?4, 1, ?5, '{}')
                 ON CONFLICT(provider, id, command_id) DO UPDATE SET delivery = 'accepted', turn_id = excluded.turn_id, resolved = 1, result_sequence = excluded.result_sequence",
                params![session.provider.key(), session.id, id, turn.as_str(), sequence as i64],
            ).map_err(failure)?;
            Ok(())
        })
    }

    pub(in crate::host_rpc) fn finish_submission(
        &self,
        session: &SessionRef,
        id: &str,
        outcome: &SubmissionDelivery,
    ) -> Result<(), String> {
        let (state, turn) = match outcome {
            SubmissionDelivery::Sending => ("sending", None),
            SubmissionDelivery::Accepted { turn_id } => {
                ("accepted", turn_id.as_ref().map(|id| id.as_str()))
            }
            SubmissionDelivery::Unknown => ("unknown", None),
            SubmissionDelivery::Rejected => ("rejected", None),
        };
        self.write(|transaction| {
            let changed = transaction.execute("UPDATE command_receipts SET turn_id = CASE WHEN resolved = 1 AND delivery = 'accepted' THEN turn_id ELSE COALESCE(?5, turn_id) END, delivery = ?4, resolved = (resolved OR ?4 = 'rejected')
                WHERE provider = ?1 AND id = ?2 AND command_id = ?3 AND NOT (resolved = 1 AND delivery = 'accepted' AND ?4 != 'accepted')",
                params![session.provider.key(), session.id, id, state, turn]).map_err(failure)?;
            if changed > 0 {
                let sequence = append_event(transaction, session, "input.delivery", &(id, outcome))?;
                transaction.execute("UPDATE command_receipts SET result_sequence = ?4 WHERE provider = ?1 AND id = ?2 AND command_id = ?3", params![session.provider.key(), session.id, id, sequence as i64]).map_err(failure)?;
            }
            Ok(())
        })
    }
}
