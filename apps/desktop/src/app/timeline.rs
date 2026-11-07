//! The open conversation's rows and render caches. Rows derive from the core
//! snapshot; everything here can be dropped and rebuilt from it.
use super::*;
use crate::diff::DiffView;

pub(super) struct RequestInputs {
    pub(super) response: Entity<TextareaState>,
    pub(super) sent: bool,
}
pub(super) struct MarkdownContent {
    pub(super) source: SharedString,
    pub(super) rendered: SharedString,
    pub(super) definitions: Arc<HashMap<String, String>>,
    pub(super) code_headers: Arc<HashMap<usize, String>>,
    pub(super) file_chips: super::view::markdown_file::FileChips,
    pub(super) parser_revision: u64,
    pub(super) diagrams: HashMap<String, Arc<RenderImage>>,
    pub(super) wrap_code: bool,
    pub(super) compact_tables: bool,
}
#[derive(Clone)]
pub(super) enum ConversationRow {
    History(agent_core::presentation::conversation::HistoryControl),
    Content(Arc<agent_core::presentation::conversation::rows::VisibleConversationRow>),
}
impl ConversationRow {
    pub(super) fn same_identity(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::History(_), Self::History(_)) => true,
            (Self::Content(a), Self::Content(b)) => a.id == b.id,
            _ => false,
        }
    }
    pub(super) fn unchanged(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::History(a), Self::History(b)) => a == b,
            (Self::Content(a), Self::Content(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

pub(super) struct Timeline {
    pub(super) list: ListState,
    pub(super) rows: Vec<ConversationRow>,
    pub(super) rendered: Option<Arc<agent_core::presentation::conversation::RenderedConversation>>,
    pub(super) projected:
        Option<Arc<agent_core::presentation::conversation::rows::ConversationRows>>,
    pub(super) hovered_marker: Option<usize>,
    pub(super) diffs: HashMap<String, Entity<DiffView>>,
    pub(super) markdown: HashMap<String, MarkdownContent>,
    pub(super) expanded_items: HashSet<String>,
    pub(super) expanded_work: HashMap<String, String>,
    pub(super) requests: HashMap<RequestId, RequestInputs>,
}

impl Timeline {
    pub(super) fn new() -> Self {
        let list = ListState::new(0, ListAlignment::Bottom, px(600.));
        list.set_follow_mode(FollowMode::Tail);
        Self {
            list,
            rows: Vec::new(),
            rendered: None,
            projected: None,
            hovered_marker: None,
            diffs: HashMap::new(),
            markdown: HashMap::new(),
            expanded_items: HashSet::new(),
            expanded_work: HashMap::new(),
            requests: HashMap::new(),
        }
    }

    /// Drops what was rendered from another Host's snapshot.
    pub(super) fn clear(&mut self) {
        self.rows.clear();
        self.list.reset(0);
        self.forget_rendering();
        self.requests.clear();
    }

    /// Drops rendering derived from the previously open conversation.
    pub(super) fn forget_rendering(&mut self) {
        self.rendered = None;
        self.projected = None;
        self.diffs.clear();
        self.markdown.clear();
    }

    /// Reconciles rows with the snapshot, keeping the visible row in place.
    pub(super) fn sync(
        &mut self,
        rendered: Option<Arc<agent_core::presentation::conversation::RenderedConversation>>,
        scope: &PanelScope,
        history: Option<agent_core::presentation::conversation::HistoryControl>,
        reset: bool,
    ) {
        self.rendered = rendered;
        self.rebuild_rows(scope, history, reset);
    }

    pub(super) fn rebuild_rows(
        &mut self,
        scope: &PanelScope,
        history: Option<agent_core::presentation::conversation::HistoryControl>,
        reset: bool,
    ) {
        use agent_core::presentation::conversation::rows::{
            ConversationRowPolicy, ConversationRows, ProgressRows,
        };
        self.projected = self.rendered.as_ref().map(|rendered| {
            ConversationRows::project(
                rendered.clone(),
                scope.clone(),
                self.expanded_work.clone(),
                ConversationRowPolicy {
                    requests: false,
                    progress: ProgressRows::UnlessHeader,
                },
                self.projected.clone(),
            )
        });
        let mut rows: Vec<_> = self
            .projected
            .as_ref()
            .into_iter()
            .flat_map(|projected| &projected.rows)
            .cloned()
            .map(ConversationRow::Content)
            .collect();
        if let Some(control) = history {
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
                        ConversationRow::Content(row)
                            if matches!(
                                row.data.content,
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
            if let ConversationRow::Content(row) = row
                && match &row.data.content {
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
