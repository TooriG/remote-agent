//! T3's session metadata cache: 60 seconds, 1,024 paths and 64 paths per RPC.
//! Requests are automatic, so network paths and oversized input never reach the Host.
use crate::state::Snapshot;
use agent_protocol::file_metadata::{FilePathMetadata, ReadFileMetadata};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

const CAPACITY: usize = 1_024;
const TTL_MS: u64 = 60_000;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Entry {
    value: Option<FilePathMetadata>,
    checked_at: u64,
    pending: Option<u64>,
    used: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileMetadataCache {
    entries: Arc<BTreeMap<String, Entry>>,
    sequence: u64,
}

impl FileMetadataCache {
    pub fn get(&self, path: &str) -> Option<&FilePathMetadata> {
        self.entries.get(path)?.value.as_ref()
    }

    pub(crate) fn ensure(&mut self, paths: Vec<String>, now_ms: u64) -> (u64, Vec<String>) {
        self.sequence += 1;
        let generation = self.sequence;
        let paths = paths
            .into_iter()
            .filter(|path| {
                !agent_protocol::paths::is_network_host_path(path)
                    && ReadFileMetadata {
                        paths: vec![path.clone()],
                    }
                    .validate()
                    .is_ok()
            })
            .collect::<BTreeSet<_>>();
        let mut requests = Vec::new();
        let entries = Arc::make_mut(&mut self.entries);
        for path in paths.into_iter().take(CAPACITY) {
            if !entries.contains_key(&path) && entries.len() == CAPACITY {
                let oldest = entries
                    .iter()
                    .min_by_key(|(_, entry)| entry.used)
                    .map(|(path, _)| path.clone())
                    .unwrap();
                entries.remove(&oldest);
            }
            let entry = entries.entry(path.clone()).or_default();
            entry.used = generation;
            if entry.pending.is_some()
                || (entry.value.is_some() && now_ms.saturating_sub(entry.checked_at) < TTL_MS)
            {
                continue;
            }
            entry.pending = Some(generation);
            requests.push(path);
        }
        (generation, requests)
    }

    pub(crate) fn received(
        &mut self,
        generation: u64,
        paths: Vec<String>,
        values: Option<Vec<FilePathMetadata>>,
        now_ms: u64,
    ) {
        let mut values = values
            .filter(|values| values.len() == paths.len())
            .into_iter()
            .flatten();
        let entries = Arc::make_mut(&mut self.entries);
        for path in paths {
            let value = values.next().unwrap_or_else(|| FilePathMetadata {
                path: path.clone(),
                metadata: None,
            });
            if let Some(entry) = entries.get_mut(&path)
                && entry.pending == Some(generation)
            {
                entry.value = Some(value);
                entry.checked_at = now_ms;
                entry.pending = None;
            }
        }
    }

    pub(crate) fn invalidate(&mut self, path: &str) {
        Arc::make_mut(&mut self.entries).remove(path);
    }

    pub(crate) fn disconnected(&mut self) {
        for entry in Arc::make_mut(&mut self.entries).values_mut() {
            entry.pending = None;
        }
    }
}

#[cfg_attr(feature = "bindings", uniffi::export)]
impl Snapshot {
    pub fn file_metadata(&self, path: String) -> Option<FilePathMetadata> {
        self.workspace.file_metadata.get(&path).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn value(path: &str) -> FilePathMetadata {
        FilePathMetadata {
            path: path.into(),
            metadata: None,
        }
    }
    #[test]
    fn requests_deduplicate_and_refresh_at_expiry_without_losing_visible_metadata() {
        let mut cache = FileMetadataCache::default();
        let (generation, paths) = cache.ensure(vec!["~/a".into(), "~/a".into()], 0);
        assert_eq!(paths, ["~/a"]);
        assert!(cache.ensure(paths.clone(), 10).1.is_empty());
        let previous = cache.clone();
        cache.received(generation, paths, Some(vec![value("/host/a")]), 100);
        assert!(
            previous.get("~/a").is_none(),
            "previous snapshots are immutable"
        );
        assert_eq!(cache.get("~/a"), Some(&value("/host/a")));
        assert!(cache.ensure(vec!["~/a".into()], 60_099).1.is_empty());
        assert_eq!(cache.ensure(vec!["~/a".into()], 60_100).1, ["~/a"]);
        assert_eq!(cache.get("~/a"), Some(&value("/host/a")));
    }

    #[test]
    fn invalidation_disconnect_and_eviction_do_not_accept_old_results() {
        let mut cache = FileMetadataCache::default();
        let (old, paths) = cache.ensure(vec!["/a".into(), "/b".into()], 0);
        cache.invalidate("/a");
        cache.disconnected();
        let (new, _) = cache.ensure(paths.clone(), 1);
        cache.received(old, paths.clone(), Some(vec![value("old"); 2]), 2);
        assert!(cache.get("/a").is_none());
        cache.received(new, paths.clone(), None, 3);
        assert_eq!(cache.get("/a"), Some(&value("/a")));
        assert!(
            cache.ensure(paths.clone(), 4).1.is_empty(),
            "failed reads are cached too"
        );
        cache.ensure((0..CAPACITY).map(|n| format!("/new/{n}")).collect(), 5);
        assert_eq!(cache.entries.len(), CAPACITY);
        assert!(cache.get("/a").is_none());
        cache.received(new, paths, Some(vec![value("old"); 2]), 6);
        assert!(!cache.entries.contains_key("/a"));
    }

    #[test]
    fn automatic_reads_are_bounded_and_never_contact_network_paths() {
        let mut cache = FileMetadataCache::default();
        let mut paths = vec![
            "relative".into(),
            "//server/a".into(),
            r"\\server\a".into(),
            format!("/{}", "😀".repeat(256)),
        ];
        paths.extend((0..2_000).map(|n| format!("/file/{n}")));
        let (_, requests) = cache.ensure(paths, 0);
        assert_eq!(requests.len(), CAPACITY);
        assert!(requests.iter().all(|path| path.starts_with("/file/")));
        assert_eq!(cache.entries.len(), CAPACITY);
    }
}
