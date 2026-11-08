//! The desktop client's session metadata cache: 60 seconds, 1,024 paths and 64 paths per RPC.
//! Requests are automatic, so network paths and oversized input never reach the Host.
use crate::state::Snapshot;
use agent_protocol::file_metadata::{FilePathMetadata, valid_metadata_path};
use std::{collections::BTreeMap, sync::Arc};

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
        let mut paths = paths;
        paths.retain(|path| {
            !agent_protocol::paths::is_network_host_path(path) && valid_metadata_path(path)
        });
        paths.sort_unstable();
        paths.dedup();
        paths.truncate(CAPACITY);
        let mut requests = Vec::new();
        if paths.is_empty() {
            return (generation, requests);
        }
        let entries = Arc::make_mut(&mut self.entries);
        let mut evictions = 0;
        let mut eviction_order = Vec::new().into_iter();
        for path in paths {
            if !entries.contains_key(&path) && entries.len() == CAPACITY {
                let oldest = if evictions < 16 {
                    evictions += 1;
                    entries
                        .iter()
                        .min_by_key(|(_, entry)| entry.used)
                        .map(|(path, _)| path.clone())
                        .unwrap()
                } else {
                    // Small misses need no index. Bulk replacement sorts once,
                    // retaining the path tie-break of the map's original scan.
                    if eviction_order.len() == 0 {
                        let mut order = entries
                            .iter()
                            .map(|(path, entry)| (entry.used, path.clone()))
                            .collect::<Vec<_>>();
                        order.sort_unstable();
                        eviction_order = order.into_iter();
                    }
                    // At most CAPACITY distinct paths are touched in a batch,
                    // so an untouched older entry always remains for eviction.
                    eviction_order
                        .find(|(_, path)| {
                            entries
                                .get(path)
                                .is_some_and(|entry| entry.used < generation)
                        })
                        .map(|(_, path)| path)
                        .unwrap()
                };
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
        for path in paths {
            let value = values.next();
            if !self
                .entries
                .get(&path)
                .is_some_and(|entry| entry.pending == Some(generation))
            {
                continue;
            }
            let entry = Arc::make_mut(&mut self.entries).get_mut(&path).unwrap();
            entry.value = Some(value.unwrap_or_else(|| FilePathMetadata {
                path: path.clone(),
                metadata: None,
            }));
            entry.checked_at = now_ms;
            entry.pending = None;
        }
    }

    pub(crate) fn invalidate(&mut self, path: &str) {
        if self.entries.contains_key(path) {
            Arc::make_mut(&mut self.entries).remove(path);
        }
    }

    pub(crate) fn disconnected(&mut self) {
        if self.entries.values().all(|entry| entry.pending.is_none()) {
            return;
        }
        for entry in Arc::make_mut(&mut self.entries).values_mut() {
            entry.pending = None;
        }
    }
}

#[cfg_attr(feature = "bindings", uniffi::export)]
impl Snapshot {
    pub fn file_metadata(&self, path: String) -> Option<FilePathMetadata> {
        self.files.workspace.file_metadata.get(&path).cloned()
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

    #[test]
    fn bulk_eviction_matches_ordered_individual_requests_and_preserves_recent_entries() {
        let mut bulk = FileMetadataCache::default();
        let (generation, paths) = bulk.ensure(
            (0..CAPACITY).map(|i| format!("/cached/{i:04}")).collect(),
            0,
        );
        let values = paths.iter().map(|path| value(path)).collect();
        bulk.received(generation, paths, Some(values), 0);
        assert!(
            bulk.ensure((0..128).map(|i| format!("/cached/{i:04}")).collect(), 1)
                .1
                .is_empty()
        );
        let mut individual = bulk.clone();
        let ordered = (0..512)
            .map(|i| format!("/ahead/{i:04}"))
            .chain((0..512).map(|i| format!("/cached/{:04}", i * 2)))
            .collect::<Vec<_>>();
        let expected_requests = ordered
            .iter()
            .flat_map(|path| individual.ensure(vec![path.clone()], 2).1)
            .collect::<Vec<_>>();
        let (_, requests) = bulk.ensure(
            ordered
                .into_iter()
                .rev()
                .flat_map(|path| [path.clone(), path])
                .collect(),
            2,
        );
        assert_eq!(requests, expected_requests);
        assert!(bulk.entries.keys().eq(individual.entries.keys()));
        for path in bulk.entries.keys() {
            assert_eq!(bulk.get(path), individual.get(path));
        }
        assert_eq!(bulk.get("/cached/0000"), Some(&value("/cached/0000")));
        assert_eq!(bulk.entries.len(), CAPACITY);
    }

    #[test]
    fn replacing_the_entire_cache_keeps_batch_ties_in_path_order() {
        let mut cache = FileMetadataCache::default();
        cache.ensure((0..CAPACITY).map(|i| format!("/old/{i:04}")).collect(), 0);
        let paths = (0..CAPACITY)
            .map(|i| format!("/new/{i:04}"))
            .collect::<Vec<_>>();
        let (_, requests) = cache.ensure(paths.clone(), 1);
        assert_eq!(requests, paths);
        assert!(cache.entries.keys().eq(paths.iter()));
        cache.ensure(vec!["/last".into()], 2);
        assert!(!cache.entries.contains_key("/new/0000"));
        assert!(cache.entries.contains_key("/new/0001"));
        assert!(cache.entries.contains_key("/last"));
        assert_eq!(cache.entries.len(), CAPACITY);
    }

    #[test]
    fn rejected_paths_leave_shared_metadata_untouched() {
        let mut cache = FileMetadataCache::default();
        cache.ensure(vec!["/cached".into()], 0);
        let previous = cache.clone();
        assert!(
            cache
                .ensure(vec!["relative".into(), "//server/share".into()], 1)
                .1
                .is_empty()
        );
        assert!(Arc::ptr_eq(&cache.entries, &previous.entries));
    }

    #[test]
    fn stale_or_empty_updates_keep_shared_entries_and_match_values_by_request_position() {
        let mut cache = FileMetadataCache::default();
        let (generation, paths) = cache.ensure(vec!["/a".into(), "/b".into()], 0);
        cache.invalidate("/a");
        let previous = cache.clone();
        cache.received(generation + 1, paths.clone(), None, 1);
        cache.invalidate("/missing");
        assert!(Arc::ptr_eq(&cache.entries, &previous.entries));
        cache.received(
            generation,
            paths,
            Some(vec![value("resolved a"), value("resolved b")]),
            2,
        );
        assert!(cache.get("/a").is_none());
        assert_eq!(cache.get("/b"), Some(&value("resolved b")));
        assert!(previous.get("/b").is_none());
        let previous = cache.clone();
        cache.disconnected();
        assert!(Arc::ptr_eq(&cache.entries, &previous.entries));
        let (next, paths) = cache.ensure(vec!["/b".into()], TTL_MS + 2);
        cache.disconnected();
        cache.received(next, paths, Some(vec![value("stale")]), TTL_MS + 3);
        assert_eq!(cache.get("/b"), Some(&value("resolved b")));
        assert_eq!(cache.ensure(vec!["/b".into()], TTL_MS + 4).1, ["/b"]);
    }
}
