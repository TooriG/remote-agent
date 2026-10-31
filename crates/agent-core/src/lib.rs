//! Client state, workflows, presentation, and optional native bindings.
use agent_protocol::{browser, models, protocol, session};
use agent_transport::{diagnostics, peer, transfers, transport};
#[cfg(test)]
extern crate self as agent_core;

pub mod browser_view;
pub mod client;
pub mod composer;
pub mod panels;
pub mod persistence;
pub mod presentation;
pub mod privacy;
pub mod questions;
pub mod state;
pub mod store;
pub mod updates;

#[cfg(feature = "bindings")]
pub mod bindings;
#[cfg(feature = "bindings")]
uniffi::setup_scaffolding!();
