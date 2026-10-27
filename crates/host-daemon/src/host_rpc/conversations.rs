//! A conversation keeps its first native identity as its stable public address.
//! Provider bindings and ordered native-history ranges belong to the Host. Text
//! remains in native transcripts; only the exact delivered handoff is copied.
use crate::providers::{AgentChange, Failure};
use agent_protocol::{
    handoff::Context, ids::ClientInputId, models::*, operations::SubmissionReceipt, session::*,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex},
};

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Binding {
    pub session: SessionRef,
    pub seen: usize,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Segment {
    pub session: SessionRef,
    pub from: usize,
    pub end: Option<usize>,
}

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
pub(super) enum Delivery {
    Ready,
    Pending,
    Injected,
    Inline,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Handoff {
    pub session: SessionRef,
    pub input_id: ClientInputId,
    pub context: Context,
    pub delivery: Delivery,
    pub receipt: Option<SubmissionReceipt>,
    pub submission: SubmissionDelivery,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Conversation {
    pub id: SessionRef,
    pub active: SessionRef,
    pub cwd: String,
    pub name: Option<String>,
    pub preview: Option<String>,
    pub bindings: Vec<Binding>,
    pub segments: Vec<Segment>,
    pub handoffs: Vec<Handoff>,
}

impl Conversation {
    /// Preserve IDs that existed before switching. Other provider sessions,
    /// including replacement sessions of the same AI, have separate namespaces.
    pub fn identity(&self, session: &SessionRef, id: &str) -> String {
        if session == &self.id {
            id.into()
        } else {
            qualified(session, id)
        }
    }

    pub fn source(&self, id: &str) -> Result<(SessionRef, String), Failure> {
        if id.starts_with("host:") {
            source(id)
        } else {
            Ok((self.id.clone(), id.into()))
        }
    }
    pub fn new(thread: &Thread) -> Result<Self, Failure> {
        let id = thread
            .id
            .clone()
            .ok_or_else(|| Failure::new("invalid_thread", "session ID is missing"))?;
        Ok(Self {
            active: id.clone(),
            cwd: thread
                .cwd
                .clone()
                .ok_or_else(|| Failure::new("invalid_thread", "working directory is missing"))?,
            name: thread.name.clone(),
            preview: thread.preview.clone(),
            bindings: vec![Binding {
                session: id.clone(),
                seen: 0,
            }],
            segments: vec![Segment {
                session: id.clone(),
                from: 0,
                end: None,
            }],
            id,
            handoffs: Vec::new(),
        })
    }

    pub fn binding(&self, provider: ProviderKind) -> Option<&Binding> {
        self.bindings
            .iter()
            .rev()
            .find(|binding| binding.session.provider == provider)
    }

    pub fn seal(&mut self, count: usize) -> Result<(), Failure> {
        if let Some(segment) = self
            .segments
            .last_mut()
            .filter(|segment| segment.end.is_none())
        {
            if count < segment.from {
                return Err(Failure::new(
                    "history_changed",
                    "Native history was shortened. The conversation cannot be switched safely.",
                ));
            }
            segment.end = Some(count);
        }
        if let Some(binding) = self
            .bindings
            .iter_mut()
            .find(|binding| binding.session == self.active)
        {
            binding.seen = self.segments.len();
        }
        Ok(())
    }

    pub fn activate(&mut self, session: SessionRef, count: usize) {
        if let Some(binding) = self
            .bindings
            .iter_mut()
            .find(|binding| binding.session == session)
        {
            binding.seen = self.segments.len();
        }
        self.segments.push(Segment {
            session: session.clone(),
            from: count,
            end: None,
        });
        self.active = session;
    }

    pub fn describe(&self, thread: &mut Thread) {
        thread.id = Some(self.id.clone());
        thread.active_provider = Some(self.active.provider);
        thread.cwd = Some(self.cwd.clone());
        thread.name = self.name.clone();
        thread.preview = self.preview.clone();
    }

    pub fn restore_source(&mut self, session: SessionRef) {
        if self
            .segments
            .last()
            .is_some_and(|segment| segment.end.is_none())
        {
            self.segments.pop();
        }
        let count = self
            .segments
            .iter()
            .rev()
            .find(|segment| segment.session == session)
            .and_then(|segment| segment.end)
            .unwrap_or(0);
        self.active = session.clone();
        self.segments.push(Segment {
            session,
            from: count,
            end: None,
        });
    }

    pub fn strip_context(&self, session: &SessionRef, item: &mut Item) {
        let body = match &mut item.body {
            ItemContent::Inline { body } => body.as_mut(),
            ItemContent::Deferred { summary } => summary.as_mut(),
        };
        let ItemBody::UserMessage { text, content } = body else {
            return;
        };
        for handoff in self.handoffs.iter().rev().filter(|handoff| {
            &handoff.session == session
                && handoff.delivery != Delivery::Injected
                && (item.client_input_id.as_ref() == Some(&handoff.input_id)
                    || item.id.as_str() == handoff.input_id.as_str())
        }) {
            // A refused submission may have been retried with the same input
            // identity and newer context. Strip only the exact delivered packet.
            let prefix = inline_context(&handoff.context);
            if let Some(rest) = text
                .as_deref()
                .and_then(|value| value.strip_prefix(&prefix))
            {
                *text = (!rest.is_empty()).then(|| rest.to_owned());
            }
            if let Some(MessagePart::Text { text }) = content.first_mut() {
                if let Some(rest) = text.strip_prefix(&prefix) {
                    *text = rest.into();
                }
                if text.is_empty() {
                    content.remove(0);
                }
            }
        }
    }

    pub fn turn(&self, session: &SessionRef, mut turn: Turn) -> Turn {
        turn.provider = Some(session.provider);
        turn.id = self.identity(session, &turn.id).into();
        if let Some(items) = &mut turn.items {
            for item in items {
                self.item(session, Arc::make_mut(item));
            }
        }
        turn
    }

    pub fn item(&self, session: &SessionRef, item: &mut Item) {
        self.strip_context(session, item);
        item.id = self.identity(session, &item.id).into();
        let body = match &mut item.body {
            ItemContent::Inline { body } => body.as_mut(),
            ItemContent::Deferred { summary } => summary.as_mut(),
        };
        if let ItemBody::Subagent {
            sender: Some(sender),
            ..
        } = body
            && self
                .bindings
                .iter()
                .any(|binding| binding.session == *sender)
        {
            *sender = self.id.clone();
        }
    }
}

pub(super) fn inline_context(context: &Context) -> String {
    format!("{}\n\nUser message:\n", context.render())
}

pub(super) fn qualified(session: &SessionRef, id: &str) -> String {
    format!(
        "host:{}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&(session, id)).expect("identity serializes"))
    )
}

pub(super) fn source(id: &str) -> Result<(SessionRef, String), Failure> {
    let value = id
        .strip_prefix("host:")
        .and_then(|id| URL_SAFE_NO_PAD.decode(id).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    value.ok_or_else(|| {
        Failure::new(
            "invalid_history_reference",
            "The item does not belong to a provider conversation.",
        )
    })
}

pub(super) struct Conversations {
    path: PathBuf,
    // Loading errors remain errors; a damaged registry must never create a
    // second logical conversation or redirect an input to an old native session.
    state: Mutex<Result<Vec<Conversation>, String>>,
}

impl Conversations {
    pub fn new(path: PathBuf) -> Self {
        let state = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice::<Vec<Conversation>>(&bytes)
                .map_err(|error| error.to_string())
                .and_then(|mut rows| {
                    validate(&rows)?;
                    for row in &mut rows {
                        for handoff in &mut row.handoffs {
                            if handoff.submission == SubmissionDelivery::Sending {
                                handoff.submission = SubmissionDelivery::Unknown;
                            }
                        }
                        if row
                            .segments
                            .last()
                            .is_some_and(|segment| segment.end.is_some())
                        {
                            row.restore_source(row.active.clone());
                        }
                    }
                    Ok(rows)
                }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(error.to_string()),
        };
        Self {
            path,
            state: Mutex::new(state),
        }
    }

    pub fn get(&self, id: &SessionRef) -> Result<Option<Conversation>, Failure> {
        let state = self.state.lock().unwrap();
        let rows = state
            .as_ref()
            .map_err(|error| Failure::new("conversation_storage_failed", error))?;
        if let Some(row) = rows.iter().find(|row| &row.id == id) {
            return Ok(Some(row.clone()));
        }
        if let Some(row) = owner(rows, id) {
            return Err(Failure::new(
                "native_session_bound",
                format!(
                    "Use the public conversation {} instead of its provider session.",
                    row.id
                ),
            ));
        }
        Ok(None)
    }

    pub fn summary(&self, mut thread: Thread) -> Result<Option<Thread>, Failure> {
        let state = self.state.lock().unwrap();
        let rows = state
            .as_ref()
            .map_err(|error| Failure::new("conversation_storage_failed", error))?;
        if let Some(id) = &thread.id
            && let Some(row) = owner(rows, id)
        {
            if *id != row.active {
                return Ok(None);
            }
            row.describe(&mut thread);
        }
        if let Some(parent) = &thread.parent_id
            && let Some(row) = owner(rows, parent)
        {
            thread.parent_id = Some(row.id.clone());
        }
        Ok(Some(thread))
    }

    pub fn active_bindings(&self) -> Result<Vec<(SessionRef, ProviderKind)>, Failure> {
        let state = self.state.lock().unwrap();
        let rows = state
            .as_ref()
            .map_err(|error| Failure::new("conversation_storage_failed", error))?;
        Ok(rows
            .iter()
            .map(|row| (row.id.clone(), row.active.provider))
            .collect())
    }

    pub fn search_targets(&self, search: &str) -> Result<Vec<SessionRef>, Failure> {
        let state = self.state.lock().unwrap();
        let rows = state
            .as_ref()
            .map_err(|error| Failure::new("conversation_storage_failed", error))?;
        let search = search.trim().to_lowercase();
        Ok(rows
            .iter()
            .filter(|row| {
                row.name
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&search)
                    || row
                        .preview
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&search)
            })
            .map(|row| row.active.clone())
            .collect())
    }

    pub fn save(&self, conversation: Conversation) -> Result<(), Failure> {
        let mut state = self.state.lock().unwrap();
        let mut next = state
            .as_ref()
            .cloned()
            .map_err(|error| Failure::new("conversation_storage_failed", error))?;
        if let Some(row) = next.iter_mut().find(|row| row.id == conversation.id) {
            *row = conversation;
        } else {
            next.push(conversation);
        }
        validate(&next).map_err(|error| Failure::new("invalid_conversation", error))?;
        let bytes = serde_json::to_vec(&next)?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| Failure::new("conversation_storage_failed", error))?;
        }
        atomicwrites::AtomicFile::new(&self.path, atomicwrites::OverwriteBehavior::AllowOverwrite)
            .write(|file| {
                file.write_all(&bytes)?;
                file.sync_all()
            })
            .map_err(|error| Failure::new("conversation_storage_failed", error))?;
        *state = Ok(next);
        Ok(())
    }

    pub fn route(
        &self,
        event: AgentChange,
        supports_fork: bool,
    ) -> Result<Option<AgentChange>, Failure> {
        let state = self.state.lock().unwrap();
        let rows = state
            .as_ref()
            .map_err(|error| Failure::new("conversation_storage_failed", error))?;
        let event = match event {
            AgentChange::Session {
                session,
                mut change,
            } => {
                let Some(row) = owner(rows, &session) else {
                    return Ok(Some(AgentChange::Session { session, change }));
                };
                // Closed ranges are read from native history. Late callbacks
                // from a released runtime cannot change the active execution.
                if session != row.active {
                    return Ok(None);
                }
                namespace_change(row, &session, &mut change, supports_fork);
                AgentChange::Session {
                    session: row.id.clone(),
                    change,
                }
            }
            AgentChange::TaskParent { session, parent } => AgentChange::TaskParent {
                session,
                parent: owner(rows, &parent).map_or_else(|| parent.clone(), |row| row.id.clone()),
            },
            AgentChange::Request {
                session,
                origin,
                mut request,
            } => {
                let root = if let Some(row) = owner(rows, &session) {
                    if session != row.active {
                        return Err(Failure::new(
                            "inactive_provider_session",
                            "The provider session has been released.",
                        ));
                    }
                    namespace_request(row, &session, &mut request);
                    row.id.clone()
                } else {
                    session
                };
                AgentChange::Request {
                    session: root,
                    origin,
                    request,
                }
            }
            AgentChange::Renamed { session, name } => {
                if owner(rows, &session).is_some() {
                    return Ok(None);
                }
                AgentChange::Renamed { session, name }
            }
            other => other,
        };
        Ok(Some(event))
    }
}

fn owner<'a>(rows: &'a [Conversation], native: &SessionRef) -> Option<&'a Conversation> {
    rows.iter().find(|row| {
        row.bindings
            .iter()
            .any(|binding| &binding.session == native)
    })
}

fn validate(rows: &[Conversation]) -> Result<(), String> {
    let mut identities = std::collections::HashSet::new();
    for row in rows {
        row.id.validate().map_err(str::to_owned)?;
        if row
            .bindings
            .first()
            .is_none_or(|binding| binding.session != row.id)
            || !row
                .bindings
                .iter()
                .any(|binding| binding.session == row.active)
            || row
                .segments
                .last()
                .is_none_or(|segment| segment.session != row.active)
            || row.cwd.trim().is_empty()
        {
            return Err("Invalid conversation identity or workspace".into());
        }
        for binding in &row.bindings {
            binding.session.validate().map_err(str::to_owned)?;
            if !identities.insert(binding.session.clone()) || binding.seen > row.segments.len() {
                return Err("Duplicate native owner or invalid history coverage".into());
            }
        }
        for (index, segment) in row.segments.iter().enumerate() {
            segment.session.validate().map_err(str::to_owned)?;
            if segment.end.is_some_and(|end| end < segment.from)
                || (segment.end.is_none() && index + 1 != row.segments.len())
            {
                return Err("Invalid native history range".into());
            }
        }
    }
    Ok(())
}

fn namespace_change(
    row: &Conversation,
    session: &SessionRef,
    change: &mut SessionChange,
    supports_fork: bool,
) {
    match change {
        SessionChange::Turn { turn, .. } => {
            *turn = row.turn(session, turn.clone());
            turn.supports_fork = Some(supports_fork);
        }
        SessionChange::Item { turn_id, item } => {
            *turn_id = row.identity(session, turn_id).into();
            row.item(session, Arc::make_mut(item));
        }
        SessionChange::TurnItems { turn_id, items } => {
            *turn_id = row.identity(session, turn_id).into();
            for item in items {
                row.item(session, Arc::make_mut(item));
            }
        }
        SessionChange::Text {
            turn_id, item_id, ..
        }
        | SessionChange::RemoveItem { turn_id, item_id }
        | SessionChange::ReasoningPart {
            turn_id, item_id, ..
        } => {
            *turn_id = row.identity(session, turn_id).into();
            *item_id = row.identity(session, item_id).into();
        }
        SessionChange::Error { turn_id, .. } => *turn_id = row.identity(session, turn_id).into(),
        SessionChange::Request { request } => namespace_request(row, session, request),
        SessionChange::Submission {
            delivery:
                SubmissionDelivery::Accepted {
                    turn_id: Some(turn_id),
                },
            ..
        } => *turn_id = row.identity(session, turn_id).into(),
        _ => {}
    }
}

fn namespace_request(
    row: &Conversation,
    session: &SessionRef,
    request: &mut agent_protocol::requests::Request,
) {
    if let agent_protocol::requests::RequestTarget::Turn { turn_id, item_id } = &mut request.target
    {
        *turn_id = row.identity(session, turn_id).into();
        if let Some(item_id) = item_id {
            *item_id = row.identity(session, item_id).into();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conversation() -> Conversation {
        Conversation::new(&Thread {
            id: Some(SessionRef {
                provider: ProviderKind::Codex,
                id: "original".into(),
            }),
            cwd: Some("/workspace".into()),
            ..Default::default()
        })
        .unwrap()
    }

    #[test]
    fn persistence_recovers_preparation_and_does_not_commit_failed_writes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("conversations.json");
        let store = Conversations::new(path.clone());
        let mut row = conversation();
        row.seal(3).unwrap();
        store.save(row.clone()).unwrap();
        let recovered = Conversations::new(path.clone())
            .get(&row.id)
            .unwrap()
            .unwrap();
        assert_eq!(recovered.segments.len(), 2);
        assert_eq!(recovered.segments[0].end, Some(3));
        assert_eq!(recovered.segments[1].from, 3);
        assert_eq!(recovered.segments[1].end, None);
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        row.name = Some("uncommitted".into());
        assert!(store.save(row.clone()).is_err());
        assert_eq!(store.get(&row.id).unwrap().unwrap().name, None);
        std::fs::remove_dir(&path).unwrap();
        std::fs::write(&path, "broken").unwrap();
        assert!(Conversations::new(path).get(&row.id).is_err());
    }

    #[test]
    fn released_provider_callbacks_cannot_change_the_active_conversation() {
        let directory = tempfile::tempdir().unwrap();
        let store = Conversations::new(directory.path().join("conversations.json"));
        let mut row = conversation();
        let target = SessionRef {
            provider: ProviderKind::Claude,
            id: "target".into(),
        };
        row.seal(1).unwrap();
        row.bindings.push(Binding {
            session: target.clone(),
            seen: 0,
        });
        row.activate(target.clone(), 0);
        store.save(row.clone()).unwrap();
        let change = || SessionChange::Turn {
            turn: Turn {
                id: "same-id".into(),
                ..Default::default()
            },
            completed: true,
        };
        assert!(
            store
                .route(
                    AgentChange::Session {
                        session: row.id.clone(),
                        change: change()
                    },
                    true
                )
                .unwrap()
                .is_none()
        );
        let Some(AgentChange::Session {
            session,
            change: SessionChange::Turn { turn, .. },
        }) = store
            .route(
                AgentChange::Session {
                    session: target.clone(),
                    change: change(),
                },
                false,
            )
            .unwrap()
        else {
            panic!("active event was not routed")
        };
        assert_eq!(session, row.id);
        assert_eq!(row.source(&turn.id).unwrap(), (target, "same-id".into()));
        assert_eq!(turn.supports_fork, Some(false));
    }

    #[test]
    fn inline_cleanup_matches_the_exact_retry_packet_and_preserves_user_whitespace() {
        let mut row = conversation();
        let packet = |coverage: &str| Context {
            messages: vec![],
            coverage: coverage.into(),
            omitted: 0,
        };
        for context in [packet("old history"), packet("new history")] {
            row.handoffs.push(Handoff {
                session: row.id.clone(),
                input_id: "same-input".into(),
                context,
                delivery: Delivery::Inline,
                receipt: None,
                submission: SubmissionDelivery::Rejected,
            });
        }
        let mut item = Item::new(
            "user".into(),
            agent_protocol::execution::ItemStatus::Completed,
            ItemBody::UserMessage {
                text: Some(format!(
                    "{}\n\nkeep spacing",
                    inline_context(&row.handoffs[0].context)
                )),
                content: vec![],
            },
        );
        item.client_input_id = Some("same-input".into());
        row.strip_context(&row.id, &mut item);
        assert!(
            matches!(item.body(), ItemBody::UserMessage { text: Some(text), .. } if text == "\n\nkeep spacing")
        );
        let prefix = inline_context(&row.handoffs[0].context);
        let mut separate = Item::new(
            "same-input".into(),
            agent_protocol::execution::ItemStatus::Completed,
            ItemBody::UserMessage {
                text: Some(prefix.clone()),
                content: vec![
                    MessagePart::Text { text: prefix },
                    MessagePart::Text {
                        text: "\n\nkeep spacing".into(),
                    },
                ],
            },
        );
        row.strip_context(&row.id, &mut separate);
        assert!(
            matches!(separate.body(), ItemBody::UserMessage { text: None, content }
            if content == &[MessagePart::Text { text: "\n\nkeep spacing".into() }])
        );
    }
}
