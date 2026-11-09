//! A rollback journal joins the transcript transaction and binding-file update.
//! Until the journal is durably removed, restart restores both original values.
use super::*;
use crate::host_rpc::transcripts::Transcripts;
use std::{io::Write, path::Path};

#[derive(Serialize, Deserialize)]
struct Backup {
    previous: Option<Conversation>,
    history: ThreadResponse,
}

fn storage(error: impl std::fmt::Display) -> Failure {
    Failure::new("conversation_storage_failed", error)
}

pub(super) fn save_json(path: &Path, value: &impl Serialize) -> Result<(), Failure> {
    let bytes = serde_json::to_vec(value)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(storage)?;
    }
    atomicwrites::AtomicFile::new(path, atomicwrites::AllowOverwrite)
        .write_with_options(
            |file| file.write_all(&bytes),
            agent_local_host::private_file_options(),
        )
        .map_err(storage)
}

fn remove_journal(path: &Path) -> Result<(), Failure> {
    std::fs::remove_file(path).map_err(storage)?;
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        std::fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(storage)?;
    }
    Ok(())
}

impl Conversations {
    fn rewind_journal(&self) -> PathBuf {
        self.path.with_extension("rewind")
    }

    fn restore_rewind(
        &self,
        state: &mut Result<Vec<Conversation>, String>,
        transcripts: &Transcripts,
        backup: Backup,
    ) -> Result<(), Failure> {
        let id = backup
            .history
            .thread
            .id
            .clone()
            .ok_or_else(|| storage("missing rewind identity"))?;
        let rows = state.as_ref().map_err(storage)?.clone();
        let restored = match backup.previous {
            Some(previous) => replacing(rows, previous),
            None => rows.into_iter().filter(|row| row.id != id).collect(),
        };
        transcripts.replace(&id, backup.history).map_err(storage)?;
        self.save_rows(state, restored)?;
        remove_journal(&self.rewind_journal())
    }

    /// Recover before provider event pumps or client requests can use a binding.
    /// A failed recovery stays failed, just like an unreadable registry.
    pub fn recover_rewind(&self, transcripts: &Transcripts) {
        let path = self.rewind_journal();
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) => {
                *self.state.lock().unwrap() = Err(error.to_string());
                return;
            }
        };
        let mut state = self.state.lock().unwrap();
        let result = serde_json::from_slice(&bytes)
            .map_err(storage)
            .and_then(|backup| self.restore_rewind(&mut state, transcripts, backup));
        if let Err(error) = result {
            *state = Err(error.to_string());
        }
    }

    /// Registry writes are serialized through the complete two-resource update.
    pub fn rewind(
        &self,
        transcripts: &Transcripts,
        row: Conversation,
        before: ThreadResponse,
        after: ThreadResponse,
    ) -> Result<u64, Failure> {
        let mut state = self.state.lock().unwrap();
        let rows = state.as_ref().map_err(storage)?.clone();
        let id = row.id.clone();
        let previous = rows.iter().find(|previous| previous.id == id).cloned();
        let next = replacing(rows, row);
        let backup = Backup {
            previous,
            history: before,
        };
        save_json(&self.rewind_journal(), &backup)?;
        let result = transcripts
            .replace(&id, after)
            .map_err(storage)
            .and_then(|version| {
                self.save_rows(&mut state, next)?;
                remove_journal(&self.rewind_journal())?;
                Ok(version)
            });
        match result {
            Err(error) => {
                if let Err(recovery) = self.restore_rewind(&mut state, transcripts, backup) {
                    *state = Err(recovery.to_string());
                }
                Err(error)
            }
            Ok(version) => Ok(version),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values() -> (Conversation, Conversation, ThreadResponse, ThreadResponse) {
        let thread = |id: &str| Thread {
            id: Some(SessionRef::new(ProviderKind::Codex, id.into()).unwrap()),
            cwd: Some("/workspace".into()),
            ..Default::default()
        };
        let original = Conversation::new(&thread("original")).unwrap();
        let mut replacement = Conversation::new(&thread("replacement")).unwrap();
        replacement.id = original.id.clone();
        let mut before = ThreadResponse {
            thread: thread("original"),
            ..Default::default()
        };
        before.thread.turns = Some(
            ["first", "removed"]
                .into_iter()
                .map(|id| {
                    Arc::new(Turn {
                        id: id.into(),
                        status: TurnStatus::Completed,
                        ..Default::default()
                    })
                })
                .collect(),
        );
        let mut after = before.clone();
        after.thread.turns.as_mut().unwrap().truncate(1);
        (original, replacement, before, after)
    }

    #[test]
    fn restart_rolls_back_every_incomplete_rewind_boundary() {
        for (managed, boundary) in [false, true]
            .into_iter()
            .flat_map(|managed| (0..3).map(move |boundary| (managed, boundary)))
        {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("conversations.json");
            let transcripts = Transcripts::open(&directory.path().join("history.sqlite"));
            let (original, replacement, before, after) = values();
            let store = Conversations::new(path.clone());
            if managed {
                store.save(original.clone()).unwrap();
            }
            transcripts
                .import(
                    &original.id,
                    transcripts.version(&original.id).unwrap(),
                    before.clone(),
                )
                .unwrap();
            save_json(
                &store.rewind_journal(),
                &Backup {
                    previous: managed.then(|| original.clone()),
                    history: before,
                },
            )
            .unwrap();
            if boundary >= 1 {
                transcripts.replace(&original.id, after).unwrap();
            }
            if boundary >= 2 {
                store.save(replacement).unwrap();
            }
            drop(store);
            let restored = Conversations::new(path);
            restored.recover_rewind(&transcripts);
            assert_eq!(
                restored.get(&original.id).unwrap().map(|row| row.active),
                managed.then(|| original.active.clone())
            );
            assert_eq!(
                transcripts
                    .history(&original.id)
                    .unwrap()
                    .unwrap()
                    .thread
                    .turns
                    .unwrap()
                    .len(),
                2
            );
            assert!(!restored.rewind_journal().exists());
        }
    }

    #[test]
    fn native_import_captured_before_rewind_cannot_restore_removed_turns() {
        for managed in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let transcripts = Transcripts::open(&directory.path().join("history.sqlite"));
            let store = Conversations::new(directory.path().join("conversations.json"));
            let (original, replacement, before, after) = values();
            if managed {
                store.save(original.clone()).unwrap();
                transcripts
                    .import(&original.id, None, before.clone())
                    .unwrap();
            }
            assert_eq!(
                transcripts
                    .history(&original.id)
                    .unwrap()
                    .map(|history| history.thread.turns.unwrap().len()),
                managed.then_some(2)
            );
            let captured = transcripts.version(&original.id).unwrap();
            let committed = store
                .rewind(&transcripts, replacement.clone(), before.clone(), after)
                .unwrap();
            // A queued blocking writer completes after the binding and history switch.
            transcripts.import(&original.id, captured, before).unwrap();
            assert_eq!(transcripts.version(&original.id).unwrap(), Some(committed));
            assert_eq!(
                store.get(&original.id).unwrap().unwrap().active,
                replacement.active
            );
            let history = transcripts.history(&original.id).unwrap().unwrap();
            assert_eq!(
                history
                    .thread
                    .turns
                    .unwrap()
                    .iter()
                    .map(|turn| turn.id.as_str())
                    .collect::<Vec<_>>(),
                ["first"]
            );
        }
    }

    #[test]
    fn unreadable_or_corrupt_journal_fails_closed_without_altering_the_binding() {
        for directory_journal in [true, false] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("conversations.json");
            let transcripts = Transcripts::open(&directory.path().join("history.sqlite"));
            let (original, _, before, _) = values();
            let store = Conversations::new(path.clone());
            store.save(original.clone()).unwrap();
            transcripts
                .import(
                    &original.id,
                    transcripts.version(&original.id).unwrap(),
                    before,
                )
                .unwrap();
            let binding = std::fs::read(&path).unwrap();
            let journal = store.rewind_journal();
            if directory_journal {
                std::fs::create_dir(&journal).unwrap();
            } else {
                std::fs::write(&journal, b"broken journal").unwrap();
            }
            store.recover_rewind(&transcripts);
            assert!(store.get(&original.id).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), binding);
            if directory_journal {
                std::fs::remove_dir(journal).unwrap();
            } else {
                assert_eq!(std::fs::read(&journal).unwrap(), b"broken journal");
                std::fs::remove_file(journal).unwrap();
            }
            let repaired = Conversations::new(path);
            repaired.recover_rewind(&transcripts);
            assert_eq!(
                repaired.get(&original.id).unwrap().unwrap().active,
                original.active
            );
        }
    }

    #[test]
    fn failed_binding_write_preserves_history_and_recovers_before_retry() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("conversations.json");
        let transcripts = Transcripts::open(&directory.path().join("history.sqlite"));
        let (original, replacement, before, after) = values();
        let store = Conversations::new(path.clone());
        store.save(original.clone()).unwrap();
        transcripts
            .import(
                &original.id,
                transcripts.version(&original.id).unwrap(),
                before.clone(),
            )
            .unwrap();
        let saved = path.with_extension("saved");
        std::fs::rename(&path, &saved).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(
            store
                .rewind(
                    &transcripts,
                    replacement.clone(),
                    before.clone(),
                    after.clone()
                )
                .is_err()
        );
        assert_eq!(
            transcripts
                .history(&original.id)
                .unwrap()
                .unwrap()
                .thread
                .turns
                .unwrap()
                .len(),
            2
        );
        assert!(
            store.get(&original.id).is_err(),
            "a failed recovery must not allow submissions"
        );
        std::fs::remove_dir(&path).unwrap();
        std::fs::rename(saved, &path).unwrap();
        let restored = Conversations::new(path);
        restored.recover_rewind(&transcripts);
        assert_eq!(
            restored.get(&original.id).unwrap().unwrap().active,
            original.active
        );
        restored
            .rewind(&transcripts, replacement.clone(), before, after)
            .unwrap();
        assert_eq!(
            restored.get(&original.id).unwrap().unwrap().active,
            replacement.active
        );
        assert_eq!(
            transcripts
                .history(&original.id)
                .unwrap()
                .unwrap()
                .thread
                .turns
                .unwrap()
                .len(),
            1
        );
    }
}
