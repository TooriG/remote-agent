//! Provider-neutral observations of execution and failure.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TurnStatus {
    Running,
    Completed,
    Failed,
    Interrupted,
    #[default]
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ItemStatus {
    Running,
    Completed,
    Failed,
    Declined,
    Interrupted,
    #[default]
    #[serde(other)]
    Unknown,
}
impl TurnStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
            Self::Unknown => "unknown",
        }
    }
}
impl ItemStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Declined => "declined",
            Self::Interrupted => "interrupted",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCategory {
    RateLimited,
    UsageLimit,
    Overloaded,
    ContextLimit,
    SessionLimit,
    Auth,
    Network,
    Policy,
    InvalidInput,
    Sandbox,
    Rollback,
    InputUnavailable,
    Internal,
    Provider(serde_json::Value),
    #[default]
    Other,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionError {
    pub category: ErrorCategory,
    pub message: String,
    pub details: Option<String>,
    pub provider_code: Option<String>,
    pub retry: Option<RetryEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryEvidence {
    pub retrying: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionStatus {
    Preparing,
    Running,
    Idle,
    Unavailable,
    #[default]
    #[serde(other)]
    Unknown,
}

/// Work an agent left running after it answered, which it reports on later,
/// as T3 Code's pending background task. A conversation waits on its roster.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundTask {
    pub id: String,
    pub kind: BackgroundTaskKind,
    /// The work's name: a subagent's title, a command's or a monitor's description.
    pub description: Option<String>,
}

/// Declared in the order a reader groups the work: agents first, loose tasks last.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub enum BackgroundTaskKind {
    Subagent,
    Command,
    Monitor,
    /// Work the provider cannot name, including kinds this build does not know.
    #[default]
    #[serde(other)]
    BackgroundTask,
}

impl BackgroundTaskKind {
    /// Subagents and monitors wake the agent, which continues; a command such
    /// as a dev server can run for hours after the agent is done. Unnamed work
    /// holds as the conservative choice.
    pub fn holds_completion(self) -> bool {
        self != Self::Command
    }
}

/// How background work ended, or what it reported, when the agent woke for it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BackgroundOutcome {
    Completed,
    Failed,
    Cancelled,
    /// A monitor reported new output and keeps running.
    Updated,
    #[default]
    #[serde(other)]
    Unknown,
}

/// One piece of background work a notification names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundReport {
    pub kind: BackgroundTaskKind,
    pub label: Option<String>,
    pub outcome: BackgroundOutcome,
    /// How a command exited, when the provider reports it.
    pub exit_code: Option<i32>,
}

/// Whether a roster keeps its conversation waiting rather than done.
pub fn background_holds_completion(tasks: &[BackgroundTask]) -> bool {
    tasks.iter().any(|task| task.kind.holds_completion())
}

#[cfg(test)]
mod background_tests {
    use super::*;

    #[test]
    fn only_work_that_wakes_the_agent_holds_its_completion() {
        let task = |kind| BackgroundTask {
            id: "task".into(),
            kind,
            description: None,
        };
        assert!(!background_holds_completion(&[]));
        assert!(!background_holds_completion(&[task(
            BackgroundTaskKind::Command
        )]));
        for kind in [
            BackgroundTaskKind::Subagent,
            BackgroundTaskKind::Monitor,
            BackgroundTaskKind::BackgroundTask,
        ] {
            assert!(background_holds_completion(&[
                task(BackgroundTaskKind::Command),
                task(kind)
            ]));
        }
        assert_eq!(
            serde_json::from_str::<BackgroundTaskKind>("\"futureWork\"").unwrap(),
            BackgroundTaskKind::BackgroundTask
        );
    }
}
