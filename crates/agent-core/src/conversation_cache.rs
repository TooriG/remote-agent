//! Device copies of opened conversations, one directory per Host, so that a
//! conversation opened before shows at once while the Host confirms it.
//!
//! A copy is the newest window of a settled conversation: running and
//! expanded conversations keep their last settled copy. Writes follow the
//! first change after half a second, then at most every ten seconds, keeping
//! only the latest state. Copies have no count or size limit. A damaged or
//! older copy is removed when read; removing a Host removes its directory.
//! Copies are best effort: failed reads and writes leave the Host as the source.
use crate::{
    models::{SessionStatus, Thread, TurnStatus},
    session::{HistoryReadKind, SessionRef},
    state::{Snapshot, operations::HISTORY_WINDOW_TURNS},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::watch;

/// Copies restored at startup, most recently saved first.
const RESTORED: usize = 20;
/// Copies in another format are discarded instead of migrated.
const FORMAT: u32 = 1;

#[derive(Serialize, Deserialize)]
struct Copy {
    format: u32,
    thread: Thread,
}

fn file(directory: &Path, id: &SessionRef) -> PathBuf {
    let name: String = id.id.bytes().map(|byte| format!("{byte:02x}")).collect();
    directory.join(format!("{}-{name}.bin", id.provider.key()))
}

fn savable(thread: &Thread) -> bool {
    thread.id.is_some()
        && thread.preparation.is_none()
        && thread.turns.as_ref().is_some_and(|turns| {
            turns.len() <= HISTORY_WINDOW_TURNS as usize
                && turns.iter().all(|turn| turn.status != TurnStatus::Running)
        })
        && thread.history_read_state.as_ref().is_none_or(|state| {
            !matches!(
                state.kind,
                HistoryReadKind::Unavailable | HistoryReadKind::Incomplete
            )
        })
}

/// Live execution and pending input belong to the Host, not to a copy.
fn copy(thread: &Thread) -> Thread {
    Thread {
        requests: Default::default(),
        submissions: Default::default(),
        status: SessionStatus::Unknown,
        list_stale: None,
        ..thread.clone()
    }
}

fn read(path: &Path) -> Option<Thread> {
    let copy = std::fs::read(path)
        .ok()
        .and_then(|bytes| agent_protocol::protocol::decode::<Copy>(&bytes).ok())
        .filter(|copy| copy.format == FORMAT && copy.thread.id.is_some());
    if copy.is_none() {
        let _ = std::fs::remove_file(path);
    }
    copy.map(|copy| copy.thread)
}

fn write(directory: &Path, thread: &Thread) -> std::io::Result<()> {
    let id = thread.id.as_ref().expect("savable threads have IDs");
    let bytes = agent_protocol::protocol::encode(Copy {
        format: FORMAT,
        thread: copy(thread),
    })?;
    std::fs::create_dir_all(directory)?;
    let path = file(directory, id);
    let pending = path.with_extension("pending");
    std::fs::write(&pending, bytes)?;
    std::fs::rename(pending, path)
}

/// Add the most recently saved copies that `snapshot` does not hold yet.
pub fn restore(directory: &Path, snapshot: &mut Snapshot) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let mut files: Vec<_> = entries
        .flatten()
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "bin")
        })
        .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
        .collect();
    files.sort_by(|left, right| right.0.cmp(&left.0));
    let conversations = Arc::make_mut(&mut snapshot.conversations);
    for (_, path) in files.into_iter().take(RESTORED) {
        if let Some(thread) = read(&path) {
            let id = thread.id.clone().expect("read copies have IDs");
            conversations.entry(id).or_insert_with(|| Arc::new(thread));
        }
    }
}

/// Remove a Host's copies, such as when it is paired again or removed.
pub fn clear(directory: &Path) {
    let _ = std::fs::remove_dir_all(directory);
}

/// Save changed, settled conversations until the Store stops publishing.
pub async fn run(directory: PathBuf, mut updates: watch::Receiver<Arc<Snapshot>>) {
    // The state published before this task started is written as a change.
    let mut saved = BTreeMap::<SessionRef, Arc<Thread>>::new();
    updates.mark_changed();
    while updates.changed().await.is_ok() {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let conversations = updates.borrow_and_update().conversations.clone();
        let changed: Vec<_> = conversations
            .iter()
            .filter(|(id, thread)| {
                saved
                    .get(*id)
                    .is_none_or(|previous| !Arc::ptr_eq(previous, thread))
                    && savable(thread)
            })
            .map(|(id, thread)| (id.clone(), thread.clone()))
            .collect();
        if !changed.is_empty() {
            let target = directory.clone();
            let pending = changed.clone();
            let written = tokio::task::spawn_blocking(move || {
                pending
                    .iter()
                    .map(|(_, thread)| write(&target, thread).is_ok())
                    .collect::<Vec<_>>()
            })
            .await
            .unwrap_or_default();
            for ((id, thread), written) in changed.into_iter().zip(written) {
                if written {
                    saved.insert(id, thread);
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Turn;
    use crate::session::ProviderKind;

    fn thread(id: &str, turns: usize, status: TurnStatus) -> Thread {
        Thread {
            id: Some(SessionRef::new(ProviderKind::Claude, id.into()).unwrap()),
            name: Some(id.into()),
            status: SessionStatus::Running,
            transcript_version: Some(7),
            turns: Some(
                (0..turns)
                    .map(|index| {
                        Arc::new(Turn {
                            id: format!("{id}-{index}").into(),
                            status,
                            items: Some(vec![]),
                            ..Default::default()
                        })
                    })
                    .collect(),
            ),
            ..Default::default()
        }
    }

    fn snapshot(threads: impl IntoIterator<Item = Thread>) -> Arc<Snapshot> {
        Arc::new(Snapshot {
            conversations: Arc::new(
                threads
                    .into_iter()
                    .map(|thread| (thread.id.clone().unwrap(), Arc::new(thread)))
                    .collect(),
            ),
            ..Default::default()
        })
    }

    fn restored(directory: &Path) -> BTreeMap<SessionRef, Arc<Thread>> {
        let mut snapshot = Snapshot::default();
        restore(directory, &mut snapshot);
        snapshot.conversations.as_ref().clone()
    }

    fn name(directory: &Path) -> Option<String> {
        restored(directory).values().next()?.name.clone()
    }

    /// Writes run on blocking threads, outside the paused clock.
    async fn written(directory: &Path, expected: &str) {
        for _ in 0..1000 {
            if name(directory).as_deref() == Some(expected) {
                return;
            }
            tokio::task::yield_now().await;
            std::thread::sleep(Duration::from_millis(2));
        }
        panic!("{expected} was not written");
    }

    #[tokio::test(start_paused = true)]
    async fn settled_windows_are_saved_after_a_pause_and_restored() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("host:a/b");
        let (updates, receiver) = watch::channel(snapshot([]));
        let saving = tokio::spawn(run(directory.clone(), receiver));
        updates.send_replace(snapshot([
            thread("settled", 2, TurnStatus::Completed),
            thread("running", 2, TurnStatus::Running),
            thread(
                "expanded",
                HISTORY_WINDOW_TURNS as usize + 1,
                TurnStatus::Completed,
            ),
        ]));
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert!(
            restored(&directory).is_empty(),
            "the first write waits half a second"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
        written(&directory, "settled").await;
        let copies = restored(&directory);
        assert_eq!(copies.len(), 1);
        let copy = copies.values().next().unwrap();
        assert_eq!(copy.name.as_deref(), Some("settled"));
        assert_eq!(copy.transcript_version, Some(7));
        assert_eq!(
            copy.status,
            SessionStatus::Unknown,
            "live status is not a copy"
        );

        // Later changes coalesce into one write per ten seconds.
        let mut renamed = thread("settled", 2, TurnStatus::Completed);
        renamed.name = Some("renamed".into());
        updates.send_replace(snapshot([renamed]));
        tokio::time::sleep(Duration::from_secs(5)).await;
        assert_eq!(name(&directory).as_deref(), Some("settled"));
        tokio::time::sleep(Duration::from_secs(6)).await;
        written(&directory, "renamed").await;
        drop(updates);
        saving.await.unwrap();
    }

    #[test]
    fn restoring_keeps_held_conversations_and_removes_damaged_copies() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path();
        write(directory, &thread("held", 1, TurnStatus::Completed)).unwrap();
        write(directory, &thread("saved", 1, TurnStatus::Completed)).unwrap();
        let damaged = directory.join("claude-00.bin");
        std::fs::write(&damaged, b"not a copy").unwrap();
        let mut snapshot = Snapshot::default();
        let held = thread("held", 3, TurnStatus::Completed);
        Arc::make_mut(&mut snapshot.conversations).insert(held.id.clone().unwrap(), Arc::new(held));
        restore(directory, &mut snapshot);
        let conversations = &snapshot.conversations;
        assert_eq!(conversations.len(), 2);
        assert_eq!(
            conversations
                .values()
                .find(|thread| thread.name.as_deref() == Some("held"))
                .unwrap()
                .turns
                .as_ref()
                .unwrap()
                .len(),
            3,
            "memory is newer than a copy"
        );
        assert!(!damaged.exists());
        clear(directory);
        assert!(restored(directory).is_empty());
    }
}
