//! Pane selection is a view into one shared snapshot. Swapping the selected
//! pane is pure and never changes another pane or opens another connection.
use super::*;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PaneStates(pub Arc<BTreeMap<String, Arc<PaneState>>>);
impl Same for PaneStates {
    fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
            || self.0.len() == other.0.len()
                && self
                    .0
                    .iter()
                    .zip(other.0.iter())
                    .all(|((id, pane), (other_id, other))| {
                        id == other_id && Arc::ptr_eq(pane, other)
                    })
    }
}
impl Snapshot {
    pub fn for_pane(&self, id: &str) -> Option<Self> {
        if id == self.pane_id {
            return Some(self.clone());
        }
        let selected = self.panes.0.get(id)?.clone();
        let mut next = self.clone();
        let panes = Arc::make_mut(&mut next.panes.0);
        panes.remove(id);
        panes.insert(self.pane_id.clone(), self.pane.clone());
        next.pane = selected;
        next.pane_id = id.into();
        next.operations = self.operations.for_pane(id);
        Some(next)
    }
    pub(crate) fn open_pane(&mut self, id: String, cwd: String) {
        let draft_key = DraftKey::Local {
            key: format!("new:{id}:{cwd}"),
        };
        Arc::make_mut(&mut self.panes.0).insert(
            id,
            Arc::new(PaneState {
                navigation: Arc::new(Navigation {
                    cwd,
                    draft_key,
                    ..Default::default()
                }),
                ..Default::default()
            }),
        );
    }
    pub(crate) fn close_pane(&mut self, id: &str) {
        if id.is_empty() || Arc::make_mut(&mut self.panes.0).remove(id).is_none() {
            return;
        }
        self.operations.close_pane(id);
        let retained: BTreeSet<_> = self
            .panes
            .0
            .values()
            .chain(std::iter::once(&self.pane))
            .filter_map(|pane| pane.navigation.thread_id.clone())
            .collect();
        let conversations = &self.sessions.conversations;
        let active = &self.lists.activity.active;
        Arc::make_mut(&mut self.sessions.subscriptions).retain(|id, _| {
            retained.contains(id)
                || active.get(id) == Some(&true)
                || conversations
                    .get(id)
                    .is_some_and(|thread| !thread.requests.is_empty())
        });
    }
    pub(crate) fn selected_elsewhere(&self, id: &crate::session::SessionRef) -> bool {
        self.panes
            .0
            .values()
            .any(|pane| pane.navigation.thread_id.as_ref() == Some(id))
    }
}
