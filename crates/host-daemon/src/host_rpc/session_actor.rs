//! Live execution and unconfirmed input delivery. Native history is never retained here.
use agent_protocol::models::Thread;

#[derive(Default)]
pub(super) struct SessionActor {
    pub(super) timeline: agent_protocol::session::Timeline,
    pub(super) request_origins:
        std::collections::BTreeMap<agent_protocol::ids::RequestId, crate::providers::RequestOrigin>,
    pub(super) leases: usize,
    pub(super) submission_lock: std::sync::Arc<tokio::sync::Mutex<()>>,
    pub(super) subscriptions: std::collections::HashMap<
        uuid::Uuid,
        (super::routing::SessionId, super::routing::Outbound),
    >,
}
impl SessionActor {
    /// Reads and submissions may race completion. Keep completed turns until leases finish,
    /// then retain only live work and delivery evidence. Never adopt native history.
    pub(super) fn release(&mut self) -> bool {
        if self.leases > 0 {
            return true;
        }
        // Retire evidence only once the provider has echoed the input in a
        // finished turn. Completion alone says nothing about a queued input or
        // an RPC still in flight on another client.
        self.timeline.submissions.retain(|id, delivery| {
            matches!(
                delivery,
                agent_protocol::session::SubmissionDelivery::Sending
            ) || !self
                .timeline
                .turns
                .iter()
                .flatten()
                .filter(|turn| turn.status != agent_protocol::execution::TurnStatus::Running)
                .flat_map(|turn| turn.items.iter().flatten())
                .any(|item| {
                    matches!(
                        item.body(),
                        agent_protocol::items::ItemBody::UserMessage { .. }
                    ) && item.client_input_id.as_ref() == Some(id)
                })
        });
        if let Some(turns) = &mut self.timeline.turns {
            turns.retain(|turn| turn.status == agent_protocol::execution::TurnStatus::Running || self.timeline.submissions.values().any(|delivery| matches!(delivery, agent_protocol::session::SubmissionDelivery::Accepted {turn_id:Some(id)} if id == &turn.id)));
        }
        if self.timeline.turns.as_ref().is_some_and(Vec::is_empty) {
            self.timeline.turns = None;
        }
        if self.timeline.turns.is_none()
            && !agent_protocol::models::task_active(None, self.timeline.status)
        {
            self.timeline.status = agent_protocol::models::SessionStatus::Unknown;
        }
        !self.timeline.submissions.is_empty()
            || self
                .timeline
                .preparation
                .as_ref()
                .is_some_and(Option::is_some)
            || !self.timeline.requests.is_empty()
            || self.timeline.turns.is_some()
    }

    pub(super) fn overlay(&self, mut thread: Thread) -> Thread {
        if let Some(cwd) = &self.timeline.cwd {
            thread.cwd = Some(cwd.clone());
        }
        for turn in self.timeline.turns.iter().flatten() {
            // Native transcripts do not identify a still-running local process.
            // Its owned turn is authoritative, even if persisted messages look complete.
            let turns = thread.turns.get_or_insert_default();
            // Some providers reuse a turn ID. Shared item IDs identify the
            // owned occurrence before falling back to the latest matching ID.
            let index = turns
                .iter()
                .enumerate()
                .filter(|(_, old)| old.id == turn.id)
                .max_by_key(|(_, old)| {
                    old.items.iter().flatten().any(|item| {
                        turn.items
                            .iter()
                            .flatten()
                            .any(|current| current.id == item.id)
                    })
                })
                .map(|(index, _)| index);
            if let Some(index) = index {
                turns[index] = turn.clone();
            } else {
                turns.push(turn.clone());
            }
        }
        if self.timeline.status != agent_protocol::models::SessionStatus::Unknown {
            thread.status = self.timeline.status;
        }
        thread.requests = self.timeline.requests.clone();
        thread.submissions = self.timeline.submissions.clone();
        if let Some(preparation) = &self.timeline.preparation {
            thread.preparation = preparation.clone();
        }
        thread
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_protocol::session::SubmissionDelivery;

    #[test]
    fn live_response_overlays_its_own_items_when_turn_ids_repeat() {
        use agent_protocol::models::Turn;
        use std::sync::Arc;
        let turn = |item: &str| -> Arc<Turn> {
            Arc::new(
                serde_json::from_value(serde_json::json!({
                    "id":"repeated", "status":"completed", "items":[{
                        "id":item, "status":"completed", "body":{"inline":{"body":{
                            "assistantText":{"text":item,"phase":"final"}
                        }}}
                    }]
                }))
                .unwrap(),
            )
        };
        let persisted = turn("current");
        let mut current = persisted.as_ref().clone();
        current.status = agent_protocol::execution::TurnStatus::Running;
        let current = Arc::new(current);
        let previous = turn("previous");
        let actor = SessionActor {
            timeline: agent_protocol::session::Timeline {
                turns: Some(vec![current.clone()]),
                ..Default::default()
            },
            ..Default::default()
        };
        for (history, expected) in [
            (
                vec![persisted.clone(), previous.clone()],
                vec![current.clone(), previous.clone()],
            ),
            (
                vec![previous.clone(), persisted.clone()],
                vec![previous.clone(), current.clone()],
            ),
        ] {
            let projected = actor.overlay(Thread {
                turns: Some(history),
                ..Default::default()
            });
            assert_eq!(projected.turns.as_ref().unwrap(), &expected);
        }
    }

    #[test]
    fn finished_echo_retires_only_confirmed_input_and_stale_execution_status() {
        let mut actor = SessionActor {
            timeline: agent_protocol::session::Timeline {
                status: agent_protocol::models::SessionStatus::Idle,
                turns: Some(serde_json::from_value(serde_json::json!([{"id":"done","status":"completed","items":[{"id":"echo","status":"unknown","clientInputId":"accepted","body":{"inline":{"body":{"userMessage":{"text":null,"content":[]}}}}}]}])).unwrap()),
                ..Default::default()
            },
            ..Default::default()
        };
        actor.timeline.submissions.insert(
            "accepted".into(),
            SubmissionDelivery::Accepted {
                turn_id: Some("done".into()),
            },
        );
        actor
            .timeline
            .submissions
            .insert("waiting".into(), SubmissionDelivery::Sending);
        actor
            .timeline
            .submissions
            .insert("uncertain".into(), SubmissionDelivery::Unknown);
        assert!(actor.release());
        assert!(!actor.timeline.submissions.contains_key("accepted"));
        assert_eq!(actor.timeline.submissions.len(), 2);
        assert!(actor.timeline.turns.is_none());
        assert!(
            actor.timeline.status == agent_protocol::models::SessionStatus::Unknown,
            "retained delivery evidence must not override newer provider execution state"
        );
    }
}
