//! One native browser view of a Host tab: its actions go to the Host in order,
//! and the page is read again between them while the view is shown. The view
//! drives the session by awaiting `next` until it returns nothing. Native task
//! cancellation does not stop an awaited call, so `next` runs one call at a time
//! and hiding the view ends a waiting call before it takes the next action.
use super::Store;
use crate::{
    browser::{BrowserAction, BrowserFrame, BrowserRequest},
    session::SessionRef,
};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::Notify;

/// The page is read this often; after a failed read the Host is retried more slowly.
const READ_INTERVAL: Duration = Duration::from_millis(150);
const RETRY_INTERVAL: Duration = Duration::from_millis(1500);
/// Input beyond this backlog is dropped instead of being replayed late.
const MAX_QUEUED: usize = 64;

#[derive(Clone)]
#[cfg_attr(feature = "bindings", derive(uniffi::Enum))]
#[allow(
    clippy::large_enum_variant,
    reason = "UniFFI enums cannot box a payload, and one update exists at a time"
)]
pub enum BrowserUpdate {
    /// The Host page after `action`.
    Frame {
        frame: BrowserFrame,
        action: BrowserAction,
    },
    /// The page could not be read; reading continues after the retry interval.
    ReadFailed { message: String },
    ActionFailed {
        action: BrowserAction,
        message: String,
    },
}

#[derive(Default)]
struct State {
    renderer_id: String,
    /// Tab, image, and generation of the latest page; actions refer to the page the user saw.
    tab_id: String,
    image_id: String,
    generation: u64,
    queued: VecDeque<BrowserRequest>,
    active: bool,
    /// The pause before the next request: the read interval, or the retry interval after a failed read.
    wait: Duration,
}

#[cfg_attr(feature = "bindings", derive(uniffi::Object))]
pub struct BrowserSession {
    store: Arc<Store>,
    thread: SessionRef,
    state: Mutex<State>,
    /// Held for a whole `next` call.
    driving: tokio::sync::Mutex<()>,
    hidden: Notify,
}

impl BrowserSession {
    pub fn new(store: Arc<Store>, thread: SessionRef) -> Arc<Self> {
        Arc::new(Self {
            store,
            thread,
            state: Mutex::default(),
            driving: tokio::sync::Mutex::new(()),
            hidden: Notify::new(),
        })
    }

    fn request(&self, state: &State, action: BrowserAction) -> BrowserRequest {
        BrowserRequest {
            thread_id: self.thread.clone(),
            tab_id: state.tab_id.clone(),
            image_id: state.image_id.clone(),
            generation: state.generation,
            renderer_id: state.renderer_id.clone(),
            action,
        }
    }

    /// The next queued action, or a read while the view is shown.
    fn next_request(&self) -> Option<BrowserRequest> {
        let mut state = self.state.lock().unwrap();
        if let Some(mut request) = state.queued.pop_front() {
            // The native renderer can change between queuing and sending.
            request.renderer_id = state.renderer_id.clone();
            return Some(request);
        }
        state
            .active
            .then(|| self.request(&state, BrowserAction::Read))
    }

    /// Actions that have not been sent yet; native views observe results instead.
    pub fn pending_actions(&self) -> usize {
        self.state.lock().unwrap().queued.len()
    }

    fn update(
        &self,
        action: BrowserAction,
        result: Result<BrowserFrame, crate::peer::PeerError>,
    ) -> BrowserUpdate {
        let mut state = self.state.lock().unwrap();
        let reading = matches!(action, BrowserAction::Read);
        state.wait = READ_INTERVAL;
        match result {
            Ok(mut frame) => {
                state.tab_id = frame.tab_id.clone();
                state.image_id = frame.image_id.clone();
                state.generation = frame.generation;
                // A queued answer closes the dialog; an earlier read must not reopen it.
                if reading
                    && state
                        .queued
                        .iter()
                        .any(|request| matches!(request.action, BrowserAction::Dialog { .. }))
                {
                    frame.dialog = None;
                }
                BrowserUpdate::Frame { frame, action }
            }
            Err(error) => {
                let crate::client::AgentError::Failed { reason: message } =
                    crate::client::error(error);
                if reading {
                    state.wait = RETRY_INTERVAL;
                    BrowserUpdate::ReadFailed { message }
                } else {
                    BrowserUpdate::ActionFailed { action, message }
                }
            }
        }
    }
}

#[cfg_attr(feature = "bindings", uniffi::export(async_runtime = "tokio"))]
impl BrowserSession {
    /// Queues `action` against the page currently shown.
    pub fn send(&self, action: BrowserAction) {
        let mut state = self.state.lock().unwrap();
        if state.queued.len() < MAX_QUEUED {
            let request = self.request(&state, action);
            state.queued.push_back(request);
        }
    }

    /// Reads the page only while the view is shown; queued actions are sent either way.
    pub fn set_active(&self, active: bool) {
        self.state.lock().unwrap().active = active;
        if !active {
            self.hidden.notify_waiters();
        }
    }

    /// Identifies a native renderer that already displays the page.
    pub fn set_renderer(&self, renderer_id: String) {
        self.state.lock().unwrap().renderer_id = renderer_id;
    }

    /// Drops actions that have not been sent.
    pub fn clear(&self) {
        self.state.lock().unwrap().queued.clear();
    }

    /// Sends the next action, or a read while shown, after the interval and
    /// returns its result. Nothing remains once the view is hidden and every
    /// action is sent. One caller drives a session at a time.
    pub async fn next(&self) -> Option<BrowserUpdate> {
        let _driving = self.driving.lock().await;
        let wait = self.state.lock().unwrap().wait;
        tokio::select! {
            () = tokio::time::sleep(wait) => {}
            // The next reader of a shown view sends the queued actions.
            () = self.hidden.notified() => return None,
        }
        let request = self.next_request()?;
        let action = request.action.clone();
        let result = self.store.browser(request).await;
        Some(self.update(action, result))
    }
}
