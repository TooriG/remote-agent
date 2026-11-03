//! Native ABI adapters; client workflows live in client modules.
mod json;
mod protocol;
mod snapshot;

use crate::client::native::{AgentError, error};

#[uniffi::export]
pub fn browser_address_url(input: String) -> Result<String, AgentError> {
    crate::browser_view::address_url(&input).map_err(error)
}

#[uniffi::export]
pub fn browser_fit_action(
    width: f64,
    height: f64,
    current_width: u32,
    current_height: u32,
) -> Option<crate::browser::BrowserAction> {
    crate::browser_view::fit_action(width, height, current_width, current_height)
}

#[uniffi::export]
pub fn browser_input_focus(
    action: crate::browser::BrowserAction,
    input_kind: Option<crate::browser::BrowserInputKind>,
) -> Option<bool> {
    crate::browser_view::input_focus(&action, input_kind)
}

#[uniffi::export]
pub fn account_error_message(message: String) -> String {
    crate::presentation::error::error_message(&message)
}

#[uniffi::export]
pub fn apply_model_preferences(
    persisted: Vec<u8>,
    preferences: Vec<u8>,
) -> Result<Vec<u8>, AgentError> {
    crate::persistence::apply_model_preferences(&persisted, &preferences).map_err(error)
}
