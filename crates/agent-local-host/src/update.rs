//! The private handover journal is shared by the Host and the detached updater.
use agent_protocol::{ids::TurnId, operations::Submission, session::SessionRef};
use serde::{Deserialize, Serialize};

pub const RECOVERY_FILE: &str = "host-update-recovery.json";

#[derive(Serialize, Deserialize)]
pub struct Recovery {
    pub tasks: Vec<InterruptedTask>,
}

#[derive(Serialize, Deserialize)]
pub struct InterruptedTask {
    pub native_session: SessionRef,
    pub submission: Submission,
    pub cwd: Option<String>,
    pub last_turn: Option<TurnId>,
    pub delivery: ContinuationDelivery,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub enum ContinuationDelivery {
    Pending,
    /// Persisted before sending. A crash cannot cause an automatic duplicate.
    Sending,
    /// The saved turn completed while the previous Host was stopping.
    Completed,
    Accepted(TurnId),
    Failed(String),
}
