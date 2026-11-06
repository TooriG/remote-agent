//! Host-owned routing for authenticated clients and independent providers.
mod conversations;
mod database;
pub(crate) mod routing;
pub(crate) mod service;
mod session_actor;
pub(crate) mod submission;
mod transcripts;

pub use routing::{HostSession, SessionId};
pub use service::HostRpcService;
