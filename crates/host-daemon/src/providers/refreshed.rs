//! Provider metadata that is slow to discover answers from the last result.
//! A read older than the freshness window refreshes it in the background, so
//! the next read sees changes without any read waiting for discovery again.
use std::{
    collections::HashMap,
    future::Future,
    hash::Hash,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// Discovery runs again in the background once a result is this old.
pub(crate) const DISCOVERY_FRESH: Duration = Duration::from_secs(30);
/// Bounds metadata kept for directories that are no longer opened.
const ENTRIES: usize = 64;

pub(crate) struct Refreshed<K, V> {
    fresh: Duration,
    entries: Arc<Mutex<HashMap<K, Entry<V>>>>,
}

struct Entry<V> {
    value: V,
    at: Instant,
    refreshing: bool,
}

impl<K, V> Refreshed<K, V>
where
    K: Eq + Hash + Clone + Send + 'static,
    V: Clone + Send + 'static,
{
    pub(crate) fn new(fresh: Duration) -> Self {
        Self {
            fresh,
            entries: Default::default(),
        }
    }

    /// Only the first read of a key waits for `load`; failures are not kept.
    pub(crate) async fn read<L>(&self, key: K, load: impl FnOnce() -> L) -> Result<V, String>
    where
        L: Future<Output = Result<V, String>> + Send + 'static,
    {
        let cached = {
            let mut entries = self.entries.lock().unwrap();
            entries.get_mut(&key).map(|entry| {
                let stale = !entry.refreshing && entry.at.elapsed() >= self.fresh;
                entry.refreshing |= stale;
                (entry.value.clone(), stale)
            })
        };
        let Some((value, stale)) = cached else {
            let value = load().await?;
            self.store(key, value.clone());
            return Ok(value);
        };
        if stale {
            let (entries, refresh) = (self.entries.clone(), load());
            tokio::spawn(async move {
                let refreshed = refresh.await;
                let mut entries = entries.lock().unwrap();
                if let Some(entry) = entries.get_mut(&key) {
                    entry.refreshing = false;
                    if let Ok(value) = refreshed {
                        (entry.value, entry.at) = (value, Instant::now());
                    }
                }
            });
        }
        Ok(value)
    }

    fn store(&self, key: K, value: V) {
        let mut entries = self.entries.lock().unwrap();
        if entries.len() >= ENTRIES && !entries.contains_key(&key) {
            entries.clear();
        }
        entries.insert(
            key,
            Entry {
                value,
                at: Instant::now(),
                refreshing: false,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stale_metadata_answers_at_once_and_the_next_read_sees_the_refresh() {
        let cache = Refreshed::new(Duration::from_millis(20));
        assert!(
            cache
                .read("key", || async { Err("discovery failed".into()) })
                .await
                .is_err()
        );
        assert_eq!(cache.read("key", || async { Ok(1) }).await, Ok(1));
        let fresh = cache.read("key", || async {
            panic!("fresh metadata must not be discovered again")
        });
        assert_eq!(fresh.await, Ok(1));
        tokio::time::sleep(Duration::from_millis(30)).await;
        let release = Arc::new(tokio::sync::Notify::new());
        let gate = release.clone();
        let stale = cache.read("key", || async move {
            gate.notified().await;
            Ok(2)
        });
        assert_eq!(
            stale.await,
            Ok(1),
            "a stale read must not wait for discovery"
        );
        // One refresh at a time: a read during it neither waits nor starts another.
        let during = cache.read("key", || async { panic!("refresh is already running") });
        assert_eq!(during.await, Ok(1));
        release.notify_one();
        tokio::time::timeout(Duration::from_secs(1), async {
            while cache.read("key", || async { Ok(3) }).await != Ok(2) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the background refresh must replace the stale value");

        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(
            cache.read("key", || async { Err("offline".into()) }).await,
            Ok(2)
        );
        tokio::time::timeout(Duration::from_secs(1), async {
            while cache.entries.lock().unwrap()["key"].refreshing {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            cache.read("key", || async { Err("offline".into()) }).await,
            Ok(2),
            "a failed refresh keeps serving the last value"
        );
    }
}
