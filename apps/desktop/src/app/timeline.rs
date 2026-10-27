//! The open conversation's rows and render caches. Rows derive from the core
//! snapshot; everything here can be dropped and rebuilt from it.
use super::*;
use crate::diff::DiffView;

pub(super) struct Question {
    pub(super) definition: agent_protocol::requests::Question,
    pub(super) input: Entity<InputState>,
    pub(super) selected: HashSet<String>,
}
pub(super) struct RequestInputs {
    pub(super) questions: Vec<Question>,
    pub(super) response: Entity<TextareaState>,
    pub(super) sent: bool,
}
pub(super) struct ImageGallery {
    pub(super) zoom: f32,
    pub(super) id: uuid::Uuid,
    pub(super) entries: Vec<(Arc<String>, bool)>,
    pub(super) initial: (Arc<String>, bool),
    pub(super) selected: Option<usize>,
    pub(super) list: ListState,
    pub(super) loading: bool,
    pub(super) saving: bool,
    pub(super) saved: bool,
    pub(super) error: String,
}
impl ImageGallery {
    pub(super) fn current_image(&self) -> &(Arc<String>, bool) {
        self.selected
            .and_then(|index| self.entries.get(index))
            .unwrap_or(&self.initial)
    }
}
pub(super) struct ImageState {
    pub(super) source: Arc<String>,
    pub(super) path: Option<ImageSource>,
    pub(super) error: Option<String>,
}
pub(super) struct MarkdownContent {
    pub(super) source: SharedString,
    pub(super) rendered: SharedString,
    pub(super) images: Rc<[String]>,
    pub(super) code_headers: Arc<HashMap<usize, String>>,
    pub(super) diagrams: HashMap<String, Arc<RenderImage>>,
    pub(super) wrap_code: bool,
    pub(super) compact_tables: bool,
}
#[derive(Clone)]
pub(super) enum ConversationRow {
    History(agent_core::presentation::conversation::HistoryControl),
    Turn {
        turn: Arc<agent_core::presentation::conversation::RenderedTurn>,
        index: usize,
        last: bool,
    },
    Pending(String, Arc<PendingSubmission>),
    Request(Box<agent_core::presentation::conversation::Request>),
}
impl ConversationRow {
    pub(super) fn same_identity(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::History(_), Self::History(_)) => true,
            (
                Self::Turn {
                    turn: a, index: ai, ..
                },
                Self::Turn {
                    turn: b, index: bi, ..
                },
            ) => a.source.id == b.source.id && a.rows[*ai].id == b.rows[*bi].id,
            (Self::Pending(a, _), Self::Pending(b, _)) => a == b,
            (Self::Request(a), Self::Request(b)) => a.id == b.id,
            _ => false,
        }
    }
    pub(super) fn unchanged(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::History(a), Self::History(b)) => a == b,
            (
                Self::Turn {
                    turn: a,
                    index: ai,
                    last: al,
                },
                Self::Turn {
                    turn: b,
                    index: bi,
                    last: bl,
                },
            ) => {
                let a_row = &a.rows[*ai];
                let b_row = &b.rows[*bi];
                if al != bl || a_row.timestamp_ms != b_row.timestamp_ms {
                    return false;
                }
                match (&a_row.content, &b_row.content) {
                    (
                        ConversationRowContent::User { item: a },
                        ConversationRowContent::User { item: b },
                    )
                    | (
                        ConversationRowContent::Activity { item: a, .. },
                        ConversationRowContent::Activity { item: b, .. },
                    ) => Arc::ptr_eq(a, b),
                    (
                        ConversationRowContent::Response {
                            item: a,
                            fork_turn_id: af,
                        },
                        ConversationRowContent::Response {
                            item: b,
                            fork_turn_id: bf,
                        },
                    ) => Arc::ptr_eq(a, b) && af == bf,
                    (
                        ConversationRowContent::ActivityHeader { activity: x },
                        ConversationRowContent::ActivityHeader { activity: y },
                    ) => {
                        x == y
                            && (!x.is_in_progress || a.work_progress(true) == b.work_progress(true))
                    }
                    (
                        ConversationRowContent::InProgress { .. },
                        ConversationRowContent::InProgress { .. },
                    ) => a.work_progress(false) == b.work_progress(false),
                    (
                        ConversationRowContent::ProviderHandoff { text: a },
                        ConversationRowContent::ProviderHandoff { text: b },
                    ) => a == b,
                    (
                        ConversationRowContent::PendingRequest { request: a },
                        ConversationRowContent::PendingRequest { request: b },
                    ) => a == b,
                    (
                        ConversationRowContent::Error { error: a },
                        ConversationRowContent::Error { error: b },
                    ) => a == b,
                    _ => false,
                }
            }
            (Self::Pending(a, x), Self::Pending(b, y)) => a == b && Arc::ptr_eq(x, y),
            (Self::Request(a), Self::Request(b)) => a == b,
            _ => false,
        }
    }
}

pub(super) struct Timeline {
    pub(super) list: ListState,
    pub(super) rows: Vec<ConversationRow>,
    pub(super) rendered: Option<Arc<agent_core::presentation::conversation::RenderedConversation>>,
    pub(super) hovered_marker: Option<usize>,
    pub(super) diffs: HashMap<String, Entity<DiffView>>,
    pub(super) markdown: HashMap<String, MarkdownContent>,
    pub(super) expanded_items: HashSet<String>,
    pub(super) expanded_work: HashMap<String, ActivityExpansion>,
    pub(super) requests: HashMap<RequestId, RequestInputs>,
    pub(super) images: HashMap<String, ImageState>,
    pub(super) gallery: Option<ImageGallery>,
    pub(super) image_dir: tempfile::TempDir,
}

impl Timeline {
    pub(super) fn new() -> Self {
        let list = ListState::new(0, ListAlignment::Bottom, px(600.));
        list.set_follow_mode(FollowMode::Tail);
        Self {
            list,
            rows: Vec::new(),
            rendered: None,
            hovered_marker: None,
            diffs: HashMap::new(),
            markdown: HashMap::new(),
            expanded_items: HashSet::new(),
            expanded_work: HashMap::new(),
            requests: HashMap::new(),
            images: HashMap::new(),
            gallery: None,
            image_dir: tempfile::Builder::new()
                .prefix("agent-images-")
                .tempdir()
                .expect("image temporary directory"),
        }
    }

    /// Drops what was rendered from another Host's snapshot.
    pub(super) fn clear(&mut self) {
        self.images.clear();
        self.gallery = None;
        self.rows.clear();
        self.list.reset(0);
        self.forget_rendering();
        self.requests.clear();
    }

    /// Drops rendering derived from the previously open conversation.
    pub(super) fn forget_rendering(&mut self) {
        self.rendered = None;
        self.diffs.clear();
        self.markdown.clear();
    }

    /// Reconciles rows with the snapshot, keeping the visible row in place.
    pub(super) fn sync(&mut self, snapshot: &Snapshot, reset: bool) {
        self.rendered = snapshot.conversation_thread().map(|thread| {
            agent_core::presentation::conversation::project_conversation(
                snapshot,
                thread,
                &self.rendered,
            )
        });
        let mut rows = conversation_rows(&self.rendered, &self.expanded_work);
        if let Some(control) = snapshot
            .navigation
            .thread_id
            .clone()
            .and_then(|session| snapshot.history_control(session))
        {
            rows.insert(0, ConversationRow::History(control));
        }
        if reset {
            self.list.reset(rows.len());
            self.list.scroll_to_end();
            self.rows = rows;
            return;
        }
        let old = &self.rows;
        let prefix = old
            .iter()
            .zip(&rows)
            .take_while(|(a, b)| a.same_identity(b))
            .count();
        let suffix = old[prefix..]
            .iter()
            .rev()
            .zip(rows[prefix..].iter().rev())
            .take_while(|(a, b)| a.same_identity(b))
            .count();
        let structure_changed = prefix + suffix != old.len() || old.len() != rows.len();
        let anchor = self.list.logical_scroll_top();
        // Match T3's maintainVisibleContentPosition: retain a visible row by
        // identity, even when a preceding history/work header is replaced.
        let visible_anchor = (structure_changed && !self.list.is_following_tail())
            .then(|| {
                let viewport = self.list.viewport_bounds();
                for (index, row) in old.iter().enumerate().skip(anchor.item_ix) {
                    let bounds = self.list.bounds_for_item(index)?;
                    if bounds.top() >= viewport.bottom() {
                        break;
                    }
                    match row {
                        ConversationRow::History(_) => continue,
                        ConversationRow::Turn { turn, index, .. }
                            if matches!(
                                turn.rows[*index].content,
                                ConversationRowContent::ActivityHeader { .. }
                                    | ConversationRowContent::InProgress { .. }
                            ) =>
                        {
                            continue;
                        }
                        _ => (),
                    }
                    if bounds.bottom() > viewport.top()
                        && let Some(item_ix) = rows.iter().position(|next| row.same_identity(next))
                    {
                        return Some(ListOffset {
                            item_ix,
                            offset_in_item: viewport.top() - bounds.top(),
                        });
                    }
                }
                None
            })
            .flatten();
        if structure_changed {
            self.list
                .splice(prefix..old.len() - suffix, rows.len() - prefix - suffix);
        }
        for index in 0..prefix {
            if !old[index].unchanged(&rows[index]) {
                self.list.remeasure_items(index..index + 1);
            }
        }
        for offset in 0..suffix {
            let index = rows.len() - suffix + offset;
            if !old[old.len() - suffix + offset].unchanged(&rows[index]) {
                self.list.remeasure_items(index..index + 1);
            }
        }
        self.rows = rows;
        if let Some(anchor) = visible_anchor {
            self.list.scroll_to(anchor);
        }
    }
    pub(super) fn pause_tail(&self) {
        if self.list.logical_scroll_top().item_ix == self.list.item_count() {
            self.list
                .scroll_by(-self.list.viewport_bounds().size.height);
        }
        self.list.pause_following_tail();
    }
    pub(super) fn remeasure_item(&self, id: &str) {
        for (index, row) in self.rows.iter().enumerate() {
            if let ConversationRow::Turn {
                turn,
                index: row_index,
                ..
            } = row
                && match &turn.rows[*row_index].content {
                    ConversationRowContent::User { item }
                    | ConversationRowContent::Activity { item, .. }
                    | ConversationRowContent::Response { item, .. } => {
                        item.data
                            .native_id
                            .as_ref()
                            .map_or(item.data.id.as_str(), |native| native.as_str())
                            == id
                    }
                    _ => false,
                }
            {
                self.list.remeasure_items(index..index + 1);
            }
        }
    }
}

fn conversation_rows(
    rendered: &Option<Arc<agent_core::presentation::conversation::RenderedConversation>>,
    expanded_work: &HashMap<String, agent_core::presentation::conversation::ActivityExpansion>,
) -> Vec<ConversationRow> {
    let mut rows = Vec::new();
    if let Some(rendered) = rendered {
        // T3's timeline virtualizes messages and work rows, never an entire
        // provider turn. A long-running turn must not mount its offscreen text.
        for turn in &rendered.turns {
            let start = rows.len();
            let mut expanded = false;
            for (index, row) in turn.rows.iter().enumerate() {
                match &row.content {
                    ConversationRowContent::ActivityHeader { activity } => {
                        expanded = agent_core::presentation::conversation::activity_is_expanded(
                            activity,
                            expanded_work.get(&activity.id).cloned(),
                        );
                    }
                    ConversationRowContent::Activity { .. } if !expanded => continue,
                    _ => (),
                }
                rows.push(ConversationRow::Turn {
                    turn: turn.clone(),
                    index,
                    last: false,
                });
            }
            if rows.len() > start
                && let Some(ConversationRow::Turn { last, .. }) = rows.last_mut()
            {
                *last = true;
            }
        }
        rows.extend(rendered.queued.iter().filter_map(|item| {
            if let agent_core::presentation::conversation::ItemSource::Pending(id, pending) =
                &item.source
            {
                Some(ConversationRow::Pending(id.clone(), pending.clone()))
            } else {
                None
            }
        }));
    }
    rows.extend(
        rendered
            .iter()
            .flat_map(|conversation| &conversation.request_rows)
            .filter_map(|row| {
                if let ConversationRowContent::PendingRequest { request } = &row.content {
                    Some(ConversationRow::Request(request.clone()))
                } else {
                    None
                }
            }),
    );
    rows
}

#[cfg(test)]
mod tests {
    use super::{
        Arc, ConversationRow, ConversationRowContent, Snapshot, Thread, conversation_rows,
    };

    #[test]
    fn row_projection_keeps_repeated_turns_and_scopes_requests_without_changing_input() {
        let mut source: Arc<Thread> = Arc::new(serde_json::from_value(serde_json::json!({"id":{"provider":"codex","id":"selected"},"turns":[{"id":"repeated","items":[{"id":"first","status":"unknown","clientInputId":null,"body":{"inline":{"body":{"assistantText":{"text":"first answer","phase":"unknown"}}}}}],"status":"unknown"},{"id":"repeated","items":[{"id":"second","status":"unknown","clientInputId":null,"body":{"inline":{"body":{"assistantText":{"text":"second answer","phase":"unknown"}}}}}],"status":"unknown"}]})).unwrap());
        let mut snapshot = Snapshot::default();
        Arc::make_mut(&mut snapshot.navigation).thread_id =
            Some(agent_protocol::session::SessionRef {
                provider: agent_protocol::session::ProviderKind::Codex,
                id: "selected".into(),
            });
        let request = Arc::new(agent_protocol::requests::Request {
            id: "global".into(),
            target: agent_protocol::requests::RequestTarget::Session,
            delivery: agent_protocol::session::RequestDelivery::Awaiting,
            body: agent_protocol::requests::RequestBody::Elicitation {
                server: "fixture".into(),
                message: "input".into(),
                input: agent_protocol::requests::ElicitationInput::Form { fields: vec![] },
            },
        });
        Arc::make_mut(&mut source)
            .requests
            .insert(request.id.clone(), request.clone());
        let mut other = (*source).clone();
        let request = Arc::make_mut(other.requests.get_mut("global").unwrap());
        request.id = "other".into();
        other.id = Some(agent_protocol::session::SessionRef {
            provider: agent_protocol::session::ProviderKind::Codex,
            id: "other".into(),
        });
        Arc::make_mut(&mut snapshot.conversations)
            .insert(other.id.clone().unwrap(), Arc::new(other));
        let before = snapshot.clone();
        let rendered = Some(
            agent_core::presentation::conversation::project_conversation(
                &snapshot,
                source.clone(),
                &None,
            ),
        );
        let rows = conversation_rows(&rendered, &Default::default());
        let repeated = conversation_rows(&rendered, &Default::default());
        let messages: Vec<_> = rows
            .iter()
            .filter_map(|row| {
                let ConversationRow::Turn { turn, index, .. } = row else {
                    return None;
                };
                let ConversationRowContent::Response { item, .. } = &turn.rows[*index].content
                else {
                    return None;
                };
                Some((&turn.source, item.data.body.as_deref()))
            })
            .collect();
        assert_eq!(messages.len(), 2);
        for ((row, body), (turn, expected)) in messages.iter().zip(
            source
                .turns
                .as_ref()
                .unwrap()
                .iter()
                .zip(["first answer", "second answer"]),
        ) {
            assert!(Arc::ptr_eq(row, turn));
            assert_eq!(*body, Some(expected));
        }
        let requests: Vec<_> = rows
            .iter()
            .filter_map(|row| match row {
                ConversationRow::Request(request) => Some(request.id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(requests, ["global"]);
        assert!(rows.iter().zip(&repeated).all(|(a, b)| a.unchanged(b)));
        assert_eq!(snapshot, before);
    }
}
