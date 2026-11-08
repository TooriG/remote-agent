//! The background work Claude left running, named as T3 Code's Waiting roster:
//! https://github.com/pingdotgg/t3code/blob/dac8b11ddc25bd5da62a367fdb6c3de777ae2986/apps/server/src/orchestration-v2/Adapters/ClaudeAdapterV2.ts#L1777
use agent_protocol::execution::{BackgroundTask, BackgroundTaskKind};
use serde_json::Value;
use std::collections::HashSet;

#[derive(Default)]
pub(super) struct Roster {
    /// In the order Claude started them.
    tasks: Vec<BackgroundTask>,
    /// Monitor calls whose task has not started yet.
    monitor_calls: HashSet<String>,
    /// Claude runs a Monitor as a `local_bash` task, so only the Monitor call
    /// that started it tells it apart from a background command.
    monitors: HashSet<String>,
}

/// Task runs are named by `run_id`, or `task_id` when the SDK has no run ID.
pub(in crate::providers::claude) fn task_key(value: &Value) -> Option<&str> {
    value["run_id"].as_str().or(value["task_id"].as_str())
}

/// Silent tasks never reach the transcript, so nothing waits on them.
fn silent(value: &Value) -> bool {
    value["ambient"] == true || value["skip_transcript"] == true
}

impl Roster {
    pub(super) fn tasks(&self) -> &[BackgroundTask] {
        &self.tasks
    }

    pub(super) fn observe(&mut self, message: &Value) {
        match (message["type"].as_str(), message["subtype"].as_str()) {
            (Some("assistant"), _) => {
                self.monitor_calls.extend(
                    blocks(message)
                        .filter(|block| block["type"] == "tool_use" && block["name"] == "Monitor")
                        .filter_map(|block| block["id"].as_str().map(str::to_owned)),
                );
            }
            (Some("user"), _) => {
                // A call's result without a started task ends the call.
                for block in blocks(message).filter(|block| block["type"] == "tool_result") {
                    if let Some(id) = block["tool_use_id"].as_str() {
                        self.monitor_calls.remove(id);
                    }
                }
            }
            (Some("system"), Some("task_started")) => {
                let Some(id) = task_key(message) else { return };
                if let Some(call) = message["tool_use_id"].as_str()
                    && self.monitor_calls.remove(call)
                {
                    self.monitors.insert(id.to_owned());
                }
                // A foreground task blocks its tool call, so nothing waits on it
                // after the answer; one moved to the background arrives in a level.
                if silent(message) || message["is_backgrounded"] == false {
                    return;
                }
                let task = self.task(id, message);
                match self.tasks.iter_mut().find(|known| known.id == task.id) {
                    Some(known) => *known = task,
                    None => self.tasks.push(task),
                }
            }
            (Some("system"), Some("task_notification")) => {
                if let Some(id) = task_key(message) {
                    self.tasks.retain(|task| task.id != id);
                    self.monitors.remove(id);
                }
            }
            (Some("system"), Some("background_tasks_changed")) => {
                // Undeclared SDK subtype: the full level of live background tasks.
                let Some(level) = message["tasks"].as_array() else {
                    return;
                };
                let tasks: Vec<_> = level
                    .iter()
                    .filter(|entry| !silent(entry))
                    .filter_map(|entry| Some(self.task(task_key(entry)?, entry)))
                    .collect();
                let listed: HashSet<_> = level.iter().filter_map(task_key).collect();
                self.monitors.retain(|id| listed.contains(id.as_str()));
                self.tasks = tasks;
            }
            _ => {}
        }
    }

    fn task(&self, id: &str, value: &Value) -> BackgroundTask {
        let kind = match value["task_type"].as_str() {
            Some("local_bash") if self.monitors.contains(id) => BackgroundTaskKind::Monitor,
            Some("local_bash") => BackgroundTaskKind::Command,
            Some("local_agent" | "remote_agent") => BackgroundTaskKind::Subagent,
            _ => BackgroundTaskKind::BackgroundTask,
        };
        let description = value["description"]
            .as_str()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
            // A level can omit what the task's start named.
            .or_else(|| {
                self.tasks
                    .iter()
                    .find(|task| task.id == id)
                    .and_then(|task| task.description.clone())
            });
        BackgroundTask {
            id: id.to_owned(),
            kind,
            description,
        }
    }
}

fn blocks(message: &Value) -> impl Iterator<Item = &Value> {
    message["message"]["content"]
        .as_array()
        .into_iter()
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn roster(messages: &[Value]) -> Vec<(String, BackgroundTaskKind, Option<String>)> {
        let mut roster = Roster::default();
        for message in messages {
            roster.observe(message);
        }
        roster
            .tasks()
            .iter()
            .map(|task| (task.id.clone(), task.kind, task.description.clone()))
            .collect()
    }

    fn monitor_call(id: &str) -> Value {
        json!({"type":"assistant","message":{"content":[{"type":"tool_use","id":id,"name":"Monitor","input":{}}]}})
    }

    fn started(id: &str, kind: &str, call: &str, description: &str) -> Value {
        json!({"type":"system","subtype":"task_started","task_id":id,"task_type":kind,"tool_use_id":call,"description":description})
    }

    #[test]
    fn task_messages_name_commands_monitors_and_subagents() {
        use BackgroundTaskKind::*;
        let messages = [
            monitor_call("watch-call"),
            started("shell", "local_bash", "bash-call", " Start the dev server "),
            started("watch", "local_bash", "watch-call", "Watch the build"),
            started("agent", "local_agent", "agent-call", "Review src/math.ts"),
            started("flow", "local_workflow", "flow-call", ""),
        ];
        assert_eq!(
            roster(&messages),
            [
                ("shell".into(), Command, Some("Start the dev server".into())),
                ("watch".into(), Monitor, Some("Watch the build".into())),
                ("agent".into(), Subagent, Some("Review src/math.ts".into())),
                ("flow".into(), BackgroundTask, None),
            ]
        );
        let mut finished = messages.to_vec();
        finished.push(json!({"type":"system","subtype":"task_notification","task_id":"watch"}));
        assert_eq!(
            roster(&finished)
                .into_iter()
                .map(|task| task.0)
                .collect::<Vec<_>>(),
            ["shell", "agent", "flow"]
        );
    }

    #[test]
    fn a_level_replaces_the_roster_and_keeps_names_and_monitors_it_lists() {
        use BackgroundTaskKind::*;
        let level = |tasks: Value| json!({"type":"system","subtype":"background_tasks_changed","tasks":tasks});
        let messages = [
            monitor_call("watch-call"),
            started("watch", "local_bash", "watch-call", "Watch the build"),
            started("shell", "local_bash", "bash-call", "Run tests"),
            level(json!([
                {"task_id":"watch","task_type":"local_bash"},
                {"run_id":"agent","task_type":"local_agent","description":"Explore"},
                {"task_id":"ambient","task_type":"monitor","ambient":true}
            ])),
        ];
        assert_eq!(
            roster(&messages),
            [
                ("watch".into(), Monitor, Some("Watch the build".into())),
                ("agent".into(), Subagent, Some("Explore".into())),
            ]
        );
        let mut emptied = messages.to_vec();
        emptied.push(level(json!([])));
        emptied.push(level(json!([{"task_id":"watch","task_type":"local_bash"}])));
        assert_eq!(
            roster(&emptied),
            [("watch".into(), Command, None)],
            "a task that left the level is no longer a monitor"
        );
    }

    #[test]
    fn foreground_and_silent_tasks_and_ended_calls_are_not_waited_on() {
        let ended = [
            monitor_call("call"),
            json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"call","content":"failed"}]}}),
            started("shell", "local_bash", "call", "after the call ended"),
        ];
        assert_eq!(roster(&ended)[0].1, BackgroundTaskKind::Command);
        for flag in ["ambient", "skip_transcript"] {
            let mut task = started("shell", "local_bash", "call", "silent");
            task[flag] = true.into();
            assert!(roster(&[task]).is_empty());
        }
        let mut foreground = started("shell", "local_bash", "call", "foreground");
        foreground["is_backgrounded"] = false.into();
        assert!(roster(&[foreground]).is_empty());
    }
}
