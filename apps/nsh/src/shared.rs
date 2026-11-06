//! Core publications use immutable Arc identity; NSH memos require PartialEq.
use std::{ops::Deref, sync::Arc};
pub(crate) struct Shared<T>(pub Arc<T>);
impl<T> Clone for Shared<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<T> PartialEq for Shared<T> {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl<T> Deref for Shared<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}
