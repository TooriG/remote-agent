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
use tokio::sync::OnceCell;

/// Discovery runs again in the background once a result is this old.
pub(crate) const DISCOVERY_FRESH: Duration = Duration::from_secs(30);
/// Bounds metadata kept for directories that are no longer opened.
const ENTRIES: usize = 64;

pub(crate) struct Refreshed<K, V> {
    fresh: Duration,
    entries: Mutex<HashMap<K, Arc<Entry<V>>>>,
}

struct Entry<V> {
    ready: OnceCell<Mutex<Cached<V>>>,
}

struct Cached<V> {
    value: V,
    at: Instant,
    refreshing: bool,
}

impl<K, V> Refreshed<K, V>
where
    K: Eq + Hash,
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
        let entry = {
            let mut entries = self.entries.lock().unwrap();
            if entries.len() >= ENTRIES && !entries.contains_key(&key) {
                entries.clear();
            }
            entries
                .entry(key)
                .or_insert_with(|| {
                    Arc::new(Entry {
                        ready: OnceCell::new(),
                    })
                })
                .clone()
        };
        let Some(cached) = entry.ready.get() else {
            // Concurrent first readers share one discovery. Cancelling its
            // caller releases OnceCell's initializer so another reader can retry.
            return entry
                .ready
                .get_or_try_init(|| async {
                    load().await.map(|value| {
                        Mutex::new(Cached {
                            value,
                            at: Instant::now(),
                            refreshing: false,
                        })
                    })
                })
                .await
                .map(|cached| cached.lock().unwrap().value.clone());
        };
        let (value, stale) = {
            let mut cached = cached.lock().unwrap();
            let stale = !cached.refreshing && cached.at.elapsed() >= self.fresh;
            cached.refreshing |= stale;
            (cached.value.clone(), stale)
        };
        if stale {
            let refresh = load();
            tokio::spawn(async move {
                let refreshed = refresh.await;
                let mut cached = entry.ready.get().unwrap().lock().unwrap();
                cached.refreshing = false;
                if let Ok(value) = refreshed {
                    (cached.value, cached.at) = (value, Instant::now());
                }
            });
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn concurrent_first_reads_share_discovery_without_blocking_other_keys() {
        let cache = Arc::new(Refreshed::new(Duration::from_secs(60)));
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let first = {
            let (cache, started, release) = (cache.clone(), started.clone(), release.clone());
            tokio::spawn(async move {
                cache
                    .read("key", || async move {
                        started.notify_one();
                        release.notified().await;
                        Ok(7)
                    })
                    .await
            })
        };
        tokio::time::timeout(Duration::from_secs(3), started.notified())
            .await
            .unwrap();
        let mut second = Box::pin(cache.read("key", || async {
            panic!("the first discovery is already running")
        }));
        assert!(futures_util::poll!(&mut second).is_pending());
        let mut other = Box::pin(cache.read("other", || async { Ok(11) }));
        assert!(matches!(
            futures_util::poll!(&mut other),
            std::task::Poll::Ready(Ok(11))
        ));
        release.notify_one();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(3), first)
                .await
                .unwrap()
                .unwrap(),
            Ok(7)
        );
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(3), second)
                .await
                .unwrap(),
            Ok(7)
        );
    }

    #[tokio::test]
    async fn a_cancelled_first_reader_leaves_discovery_retryable() {
        let cache = Arc::new(Refreshed::new(Duration::from_secs(60)));
        let started = Arc::new(tokio::sync::Notify::new());
        let first = {
            let (cache, started) = (cache.clone(), started.clone());
            tokio::spawn(async move {
                cache
                    .read("key", || async move {
                        started.notify_one();
                        std::future::pending::<Result<u32, String>>().await
                    })
                    .await
            })
        };
        tokio::time::timeout(Duration::from_secs(3), started.notified())
            .await
            .unwrap();
        first.abort();
        assert!(first.await.unwrap_err().is_cancelled());
        assert_eq!(cache.read("key", || async { Ok(7) }).await, Ok(7));
    }

    #[tokio::test]
    async fn full_cache_keeps_existing_keys_until_another_key_is_discovered() {
        let cache = Refreshed::new(Duration::from_secs(60));
        for key in 0..ENTRIES {
            assert_eq!(cache.read(key, || async move { Ok(key) }).await, Ok(key));
        }
        assert_eq!(
            cache
                .read(0, || async { Err("fresh entry was lost".into()) })
                .await,
            Ok(0)
        );
        assert_eq!(
            cache.read(ENTRIES, || async { Ok(ENTRIES) }).await,
            Ok(ENTRIES)
        );
        assert_eq!(cache.read(0, || async { Ok(123) }).await, Ok(123));
    }

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
        let mut duplicated = false;
        let during = cache.read("key", || {
            duplicated = true;
            async { Ok(3) }
        });
        assert_eq!(during.await, Ok(1));
        assert!(!duplicated, "refresh is already running");
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
            while cache.entries.lock().unwrap()["key"]
                .ready
                .get()
                .unwrap()
                .lock()
                .unwrap()
                .refreshing
            {
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
