//! T3 Code's Waiting roster for Codex: the commands and subagents a completed
//! turn left running. Codex reports their ends as item updates; Stop ends the
//! commands through their background terminals.
//! https://github.com/pingdotgg/t3code/blob/dac8b11ddc25bd5da62a367fdb6c3de777ae2986/packages/shared/src/orchestrationV2PendingBackgroundWork.ts
use crate::providers::AgentChange;
use agent_protocol::{
    execution::{BackgroundTask, BackgroundTaskKind, ItemStatus, TurnStatus},
    ids::ItemId,
    items::{Item, ItemBody},
    session::{SessionChange, SessionRef},
};
use serde_json::Value;
use std::{collections::HashMap, sync::Mutex};

#[derive(Default)]
pub(super) struct Rosters(Mutex<HashMap<SessionRef, Roster>>);

#[derive(Default)]
struct Roster {
    running: bool,
    work: Vec<Work>,
    published: Vec<BackgroundTask>,
}

struct Work {
    item: ItemId,
    task: BackgroundTask,
    /// The background terminal a command runs in.
    process: Option<String>,
}

impl Roster {
    /// Only an answered conversation waits; while a turn runs, it works.
    fn waiting(&self) -> Vec<BackgroundTask> {
        if self.running {
            Vec::new()
        } else {
            self.work.iter().map(|work| work.task.clone()).collect()
        }
    }

    fn track(&mut self, item: &Item, native: &Value) {
        let known = self
            .work
            .iter()
            .position(|work| work.item == item.id)
            .map(|index| self.work.remove(index));
        if item.status != ItemStatus::Running {
            return;
        }
        let (kind, description) = match item.body() {
            ItemBody::CommandExecution { command, .. } => {
                (BackgroundTaskKind::Command, Some(command.as_str()))
            }
            ItemBody::Subagent { prompt, tool, .. } => (
                BackgroundTaskKind::Subagent,
                prompt.as_deref().or(Some(tool.as_str())),
            ),
            _ => return,
        };
        self.work.push(Work {
            item: item.id.clone(),
            task: BackgroundTask {
                id: item.id.to_string(),
                kind,
                description: description
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(str::to_owned),
            },
            // A turn's summary of its items can omit what their start named.
            process: native["processId"]
                .as_str()
                .map(str::to_owned)
                .or(known.and_then(|work| work.process)),
        });
    }
}

impl Rosters {
    /// The roster change a native change implies for its conversation.
    pub(super) fn observe(&self, change: &AgentChange, params: &Value) -> Option<AgentChange> {
        let AgentChange::Session { session, change } = change else {
            return None;
        };
        let mut rosters = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        let roster = rosters.entry(session.clone()).or_default();
        match change {
            SessionChange::Turn { turn, completed } => {
                roster.running = !completed;
                for item in turn.items.iter().flatten() {
                    roster.track(item, &Value::Null);
                }
                // A stopped or failed turn leaves nothing it will report on.
                if *completed && turn.status != TurnStatus::Completed {
                    roster.work.clear();
                }
            }
            SessionChange::Item { item, .. } => roster.track(item, &params["item"]),
            SessionChange::Status { status }
                if !agent_protocol::models::task_active(None, *status) =>
            {
                roster.running = false;
            }
            _ => return None,
        }
        let waiting = roster.waiting();
        if waiting == roster.published {
            return None;
        }
        roster.published = waiting.clone();
        Some(AgentChange::Session {
            session: session.clone(),
            change: SessionChange::Background { tasks: waiting },
        })
    }

    /// The background terminals Stop ends while the conversation waits.
    pub(super) fn terminals(&self, session: &SessionRef) -> Vec<String> {
        let rosters = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        rosters
            .get(session)
            .filter(|roster| !roster.running)
            .into_iter()
            .flat_map(|roster| roster.work.iter().filter_map(|work| work.process.clone()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_protocol::{models::Turn, session::ProviderKind};
    use serde_json::json;
    use std::sync::Arc;

    fn session() -> SessionRef {
        SessionRef::new(ProviderKind::Codex, "thread".into()).unwrap()
    }

    fn item(id: &str, status: ItemStatus, body: ItemBody) -> AgentChange {
        AgentChange::Session {
            session: session(),
            change: SessionChange::Item {
                turn_id: "turn".into(),
                item: Arc::new(Item::new(id.into(), status, body)),
            },
        }
    }

    fn command() -> ItemBody {
        ItemBody::CommandExecution {
            command: "npm run dev".into(),
            cwd: None,
            output: String::new(),
            exit_code: None,
        }
    }

    fn turn(status: TurnStatus) -> AgentChange {
        AgentChange::Session {
            session: session(),
            change: SessionChange::Turn {
                turn: Turn {
                    id: "turn".into(),
                    status,
                    ..Default::default()
                },
                completed: status != TurnStatus::Running,
            },
        }
    }

    fn roster(change: Option<AgentChange>) -> Option<Vec<(String, BackgroundTaskKind)>> {
        match change? {
            AgentChange::Session {
                change: SessionChange::Background { tasks },
                ..
            } => Some(
                tasks
                    .into_iter()
                    .map(|task| (task.description.unwrap_or_default(), task.kind))
                    .collect(),
            ),
            _ => None,
        }
    }

    #[test]
    fn a_completed_turn_waits_on_the_commands_and_subagents_it_left_running() {
        let rosters = Rosters::default();
        assert_eq!(
            roster(rosters.observe(&turn(TurnStatus::Running), &Value::Null)),
            None
        );
        let started = json!({"item":{"processId":"process-1"}});
        assert_eq!(
            roster(rosters.observe(&item("server", ItemStatus::Running, command()), &started)),
            None,
            "a running turn is working, not waiting"
        );
        rosters.observe(
            &item(
                "agent",
                ItemStatus::Running,
                ItemBody::Subagent {
                    tool: "spawnAgent".into(),
                    prompt: Some("Review the change".into()),
                    model: None,
                    effort: None,
                    sender: None,
                    receivers: vec![],
                    states: vec![],
                    agent_id: None,
                    result: None,
                },
            ),
            &Value::Null,
        );
        rosters.observe(&item("done", ItemStatus::Running, command()), &Value::Null);
        rosters.observe(
            &item("done", ItemStatus::Completed, command()),
            &Value::Null,
        );
        assert_eq!(
            roster(rosters.observe(&turn(TurnStatus::Completed), &Value::Null)),
            Some(vec![
                ("npm run dev".into(), BackgroundTaskKind::Command),
                ("Review the change".into(), BackgroundTaskKind::Subagent),
            ])
        );
        assert_eq!(rosters.terminals(&session()), ["process-1"]);
        assert_eq!(
            roster(rosters.observe(&item("server", ItemStatus::Failed, command()), &Value::Null)),
            Some(vec![(
                "Review the change".into(),
                BackgroundTaskKind::Subagent
            )])
        );
        assert!(rosters.terminals(&session()).is_empty());
        assert_eq!(
            roster(rosters.observe(&turn(TurnStatus::Interrupted), &Value::Null)),
            Some(vec![]),
            "a stopped turn reports on nothing"
        );
    }
}
