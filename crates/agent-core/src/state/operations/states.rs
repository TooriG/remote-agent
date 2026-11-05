//! One operation registry. Only operations whose result is a pane selection
//! carry a pane address; session and Host resources retain their shared keys.
use super::{OperationKey, OperationState};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct OperationStates {
    entries: Arc<BTreeMap<(String, OperationKey), OperationState>>,
    pane: String,
}
impl OperationKey {
    pub(crate) fn address(&self, pane: &str) -> (String, Self) {
        (
            if self.pane_local() {
                pane.into()
            } else {
                String::new()
            },
            self.clone(),
        )
    }
    fn pane_local(&self) -> bool {
        matches!(self, Self::Directory | Self::File | Self::WorkspaceReview)
    }
}
impl OperationStates {
    fn address(&self, key: &OperationKey) -> (String, OperationKey) {
        key.address(&self.pane)
    }
    pub(crate) fn for_pane(&self, pane: &str) -> Self {
        Self {
            entries: self.entries.clone(),
            pane: pane.into(),
        }
    }
    pub(crate) fn close_pane(&mut self, pane: &str) {
        Arc::make_mut(&mut self.entries).retain(|(owner, _), _| owner != pane);
    }
    pub fn get(&self, key: &OperationKey) -> Option<&OperationState> {
        self.entries.get(&self.address(key))
    }
    pub fn get_mut(&mut self, key: &OperationKey) -> Option<&mut OperationState> {
        {
            let address = self.address(key);
            Arc::make_mut(&mut self.entries).get_mut(&address)
        }
    }
    pub fn contains_key(&self, key: &OperationKey) -> bool {
        self.get(key).is_some()
    }
    pub fn insert(&mut self, key: OperationKey, value: OperationState) -> Option<OperationState> {
        {
            let address = self.address(&key);
            Arc::make_mut(&mut self.entries).insert(address, value)
        }
    }
    pub fn remove(&mut self, key: &OperationKey) -> Option<OperationState> {
        {
            let address = self.address(key);
            Arc::make_mut(&mut self.entries).remove(&address)
        }
    }
    pub fn iter(&self) -> impl Iterator<Item = (&OperationKey, &OperationState)> {
        self.entries.iter().filter_map(|((pane, key), value)| {
            (pane.as_str()
                == if key.pane_local() {
                    self.pane.as_str()
                } else {
                    ""
                })
            .then_some((key, value))
        })
    }
    pub fn keys(&self) -> impl Iterator<Item = &OperationKey> {
        self.iter().map(|(key, _)| key)
    }
    pub fn values(&self) -> impl Iterator<Item = &OperationState> {
        self.iter().map(|(_, value)| value)
    }
    pub fn is_empty(&self) -> bool {
        self.iter().next().is_none()
    }
    pub fn retain(&mut self, mut keep: impl FnMut(&OperationKey, &mut OperationState) -> bool) {
        let selected = self.pane.as_str();
        Arc::make_mut(&mut self.entries).retain(|(pane, key), value| {
            (pane.as_str() != if key.pane_local() { selected } else { "" }) || keep(key, value)
        });
    }
}
impl FromIterator<(OperationKey, OperationState)> for OperationStates {
    fn from_iter<T: IntoIterator<Item = (OperationKey, OperationState)>>(iter: T) -> Self {
        Self {
            entries: Arc::new(
                iter.into_iter()
                    .map(|(key, value)| ((String::new(), key), value))
                    .collect(),
            ),
            pane: String::new(),
        }
    }
}
impl<const N: usize> From<[(OperationKey, OperationState); N]> for OperationStates {
    fn from(value: [(OperationKey, OperationState); N]) -> Self {
        value.into_iter().collect()
    }
}

impl crate::state::Same for OperationStates {
    fn same(&self, other: &Self) -> bool {
        self.pane == other.pane && Arc::ptr_eq(&self.entries, &other.entries)
    }
}
