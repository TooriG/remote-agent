//! Host-owned transcripts. Native provider history is read once to import a
//! conversation; afterwards the Host records each turn as it finishes and
//! never returns to native storage for reads. Rows keep every item inline;
//! readers summarize or defer bodies for transfer.
use agent_protocol::{
    execution::TurnStatus,
    ids::TurnId,
    models::{SessionStatus, ThreadResponse, Turn},
    protocol,
    session::SessionRef,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::Mutex,
};

/// A window stops growing past this many encoded bytes, keeping at least one turn.
const WINDOW_BYTES: usize = 1 << 20;

pub(crate) struct Transcripts {
    // A store that failed to open stays failed; reads must not fall back to
    // an empty transcript that would look like a conversation without turns.
    state: Mutex<Result<Connection, String>>,
}

pub(crate) struct Window {
    pub response: ThreadResponse,
    /// Position of the first returned turn. Earlier turns precede it.
    pub start: usize,
}

fn failure(error: impl std::fmt::Display) -> String {
    error.to_string()
}

/// Within a turn ID's candidates, shared item IDs identify the same occurrence.
fn same_occurrence(stored: &Turn, live: &Turn) -> bool {
    let stored_items = stored.items.as_deref().unwrap_or_default();
    let live_items = live.items.as_deref().unwrap_or_default();
    if stored_items.is_empty() || live_items.is_empty() {
        return true;
    }
    let stored_ids: HashSet<_> = stored_items.iter().map(|item| &item.id).collect();
    live_items.iter().any(|item| stored_ids.contains(&item.id))
}

fn turns_by_id<'a>(turns: impl Iterator<Item = &'a Turn>) -> HashMap<&'a TurnId, Vec<&'a Turn>> {
    let mut by_id: HashMap<_, Vec<_>> = HashMap::new();
    for turn in turns {
        by_id.entry(&turn.id).or_default().push(turn);
    }
    by_id
}

fn decode_turn(body: &[u8]) -> Result<Turn, String> {
    protocol::decode(body).map_err(failure)
}

impl Transcripts {
    pub(crate) fn open(path: &Path) -> Self {
        let state = (|| {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(failure)?;
            }
            let connection = Connection::open(path).map_err(failure)?;
            // A restarting Host can briefly overlap its predecessor.
            connection
                .busy_timeout(std::time::Duration::from_secs(10))
                .map_err(failure)?;
            connection
                .execute_batch(
                    "PRAGMA journal_mode = WAL;
                     PRAGMA synchronous = NORMAL;
                     CREATE TABLE IF NOT EXISTS conversations (
                         provider TEXT NOT NULL,
                         id TEXT NOT NULL,
                         cwd TEXT,
                         metadata BLOB,
                         PRIMARY KEY (provider, id)
                     ) WITHOUT ROWID;
                     CREATE TABLE IF NOT EXISTS turns (
                         provider TEXT NOT NULL,
                         id TEXT NOT NULL,
                         position INTEGER NOT NULL,
                         turn_id TEXT NOT NULL,
                         running INTEGER NOT NULL,
                         body BLOB NOT NULL,
                         PRIMARY KEY (provider, id, position)
                     ) WITHOUT ROWID;
                     CREATE INDEX IF NOT EXISTS turn_ids ON turns (provider, id, turn_id, position);
                     -- Host-wide order of each conversation's latest change.
                     CREATE TABLE IF NOT EXISTS versions (
                         provider TEXT NOT NULL,
                         id TEXT NOT NULL,
                         version INTEGER NOT NULL,
                         PRIMARY KEY (provider, id)
                     ) WITHOUT ROWID;",
                )
                .map_err(failure)?;
            Ok(connection)
        })();
        Self {
            state: Mutex::new(state),
        }
    }

    fn with<T>(
        &self,
        work: impl FnOnce(&mut Connection) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        work(state.as_mut().map_err(|error| error.clone())?)
    }

    fn write<T>(&self, work: impl FnOnce(&Transaction) -> Result<T, String>) -> Result<T, String> {
        self.with(|connection| {
            let transaction = connection.transaction().map_err(failure)?;
            let value = work(&transaction)?;
            transaction.commit().map_err(failure)?;
            Ok(value)
        })
    }

    /// Native history fills in what the Host did not run. A turn the Host
    /// recorded keeps its recorded items, which an import does not copy in
    /// full; recorded turns missing from native history follow it in order.
    pub(crate) fn import(
        &self,
        conversation: &SessionRef,
        mut response: ThreadResponse,
    ) -> Result<(), String> {
        let native = response.thread.turns.take().unwrap_or_default();
        // Live execution supplies status; a stored observation would go stale.
        response.thread.status = SessionStatus::Unknown;
        response.thread.history_cursor = None;
        response.thread.history_has_more = None;
        response.thread.history_read_state = None;
        let metadata = protocol::encode(&response).map_err(failure)?;
        let key = params![conversation.provider.key(), conversation.id];
        self.write(|transaction| {
            let mut recorded = Vec::new();
            {
                let mut rows = transaction
                    .prepare("SELECT body FROM turns WHERE provider = ?1 AND id = ?2 ORDER BY position")
                    .map_err(failure)?;
                let mut rows = rows.query(key).map_err(failure)?;
                while let Some(row) = rows.next().map_err(failure)? {
                    recorded.push(decode_turn(&row.get::<_, Vec<u8>>(0).map_err(failure)?)?);
                }
            }
            // A turn recorded before its conversation was saved lacks the
            // provider attribution native reads add.
            let recorded_by_id = turns_by_id(recorded.iter());
            let native_by_id = turns_by_id(native.iter().map(AsRef::as_ref));
            let turns: Vec<std::borrow::Cow<Turn>> = native
                .iter()
                .map(|native| {
                    let stored = recorded_by_id.get(&native.id).into_iter().flatten().copied()
                        .find(|turn| same_occurrence(native, turn));
                    match stored {
                        Some(turn) => std::borrow::Cow::Owned(Turn {
                            provider: turn.provider.or(native.provider),
                            supports_fork: turn.supports_fork.or(native.supports_fork),
                            ..turn.clone()
                        }),
                        None => std::borrow::Cow::Borrowed(native.as_ref()),
                    }
                })
                .chain(
                    recorded
                        .iter()
                        .filter(|turn| {
                            !native_by_id.get(&turn.id).into_iter().flatten()
                                .any(|native| same_occurrence(native, turn))
                        })
                        .map(std::borrow::Cow::Borrowed),
                )
                .collect();
            transaction
                .execute("DELETE FROM turns WHERE provider = ?1 AND id = ?2", key)
                .map_err(failure)?;
            for (position, turn) in turns.iter().enumerate() {
                insert(transaction, conversation, position as i64, turn)?;
            }
            transaction
                .execute(
                    "INSERT INTO conversations (provider, id, cwd, metadata) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT (provider, id) DO UPDATE SET cwd = excluded.cwd, metadata = excluded.metadata",
                    params![conversation.provider.key(), conversation.id, response.thread.cwd, metadata],
                )
                .map_err(failure)?;
            advance(transaction, conversation.provider.key(), &conversation.id)
        })
    }

    /// Replace the stored occurrence of a live turn, or append it. Conversations
    /// that are not imported yet keep these turns for their import.
    /// A turn observed without items keeps the items already stored for it.
    pub(crate) fn record(&self, conversation: &SessionRef, turn: &Turn) -> Result<(), String> {
        let key = params![conversation.provider.key(), conversation.id];
        self.write(|transaction| {
            let latest = transaction
                .query_row(
                    "SELECT position, body FROM turns WHERE provider = ?1 AND id = ?2 AND turn_id = ?3
                     ORDER BY position DESC LIMIT 1",
                    params![conversation.provider.key(), conversation.id, turn.id.as_str()],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?)),
                )
                .optional()
                .map_err(failure)?;
            let mut turn = std::borrow::Cow::Borrowed(turn);
            let stored = latest
                .map(|(position, body)| Ok::<_, String>((position, decode_turn(&body)?)))
                .transpose()?
                .filter(|(_, stored)| same_occurrence(stored, &turn));
            let position = match stored {
                Some((position, stored)) => {
                    if turn.items.is_none() {
                        turn.to_mut().items = stored.items;
                    }
                    position
                }
                None => transaction
                    .query_row(
                        "SELECT COUNT(*) FROM turns WHERE provider = ?1 AND id = ?2",
                        key,
                        |row| row.get::<_, i64>(0),
                    )
                    .map_err(failure)?,
            };
            transaction
                .execute(
                    "INSERT OR IGNORE INTO conversations (provider, id) VALUES (?1, ?2)",
                    key,
                )
                .map_err(failure)?;
            transaction
                .execute(
                    "DELETE FROM turns WHERE provider = ?1 AND id = ?2 AND position = ?3",
                    params![conversation.provider.key(), conversation.id, position],
                )
                .map_err(failure)?;
            insert(transaction, conversation, position, &turn)?;
            advance(transaction, conversation.provider.key(), &conversation.id)
        })
    }

    /// Change the latest stored occurrence of a turn, such as with an update
    /// a provider sent after the turn finished.
    pub(crate) fn update_turn(
        &self,
        conversation: &SessionRef,
        turn_id: &TurnId,
        change: impl FnOnce(Turn) -> Option<Turn>,
    ) -> Result<(), String> {
        self.write(|transaction| {
            let Some((position, body)) = transaction
                .query_row(
                    "SELECT position, body FROM turns WHERE provider = ?1 AND id = ?2 AND turn_id = ?3
                     ORDER BY position DESC LIMIT 1",
                    params![conversation.provider.key(), conversation.id, turn_id.as_str()],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?)),
                )
                .optional()
                .map_err(failure)?
            else {
                return Ok(());
            };
            let Some(turn) = change(decode_turn(&body)?) else {
                return Ok(());
            };
            transaction
                .execute(
                    "DELETE FROM turns WHERE provider = ?1 AND id = ?2 AND position = ?3",
                    params![conversation.provider.key(), conversation.id, position],
                )
                .map_err(failure)?;
            insert(transaction, conversation, position, &turn)?;
            advance(transaction, conversation.provider.key(), &conversation.id)
        })
    }

    /// No turn runs across a Host restart.
    pub(crate) fn interrupt_running(&self) -> Result<(), String> {
        self.write(|transaction| {
            let mut stopped = Vec::new();
            {
                let mut rows = transaction
                    .prepare("SELECT provider, id, position, body FROM turns WHERE running = 1")
                    .map_err(failure)?;
                let mut rows = rows.query([]).map_err(failure)?;
                while let Some(row) = rows.next().map_err(failure)? {
                    let mut turn = decode_turn(&row.get::<_, Vec<u8>>(3).map_err(failure)?)?;
                    turn.status = TurnStatus::Interrupted;
                    stopped.push((
                        row.get::<_, String>(0).map_err(failure)?,
                        row.get::<_, String>(1).map_err(failure)?,
                        row.get::<_, i64>(2).map_err(failure)?,
                        protocol::encode(&turn).map_err(failure)?,
                    ));
                }
            }
            for (provider, id, position, body) in stopped {
                transaction
                    .execute(
                        "UPDATE turns SET running = 0, body = ?4
                         WHERE provider = ?1 AND id = ?2 AND position = ?3",
                        params![provider, id, position, body],
                    )
                    .map_err(failure)?;
                advance(transaction, &provider, &id)?;
            }
            Ok(())
        })
    }

    /// Handoffs and forks read the entire committed transcript, without the UI
    /// window's byte limit. Recorded turns alone do not prove a complete import.
    pub(crate) fn history(
        &self,
        conversation: &SessionRef,
    ) -> Result<Option<ThreadResponse>, String> {
        self.with(|connection| {
            let key = params![conversation.provider.key(), conversation.id];
            let Some(metadata) = connection
                .query_row(
                    "SELECT metadata FROM conversations WHERE provider = ?1 AND id = ?2",
                    key,
                    |row| row.get::<_, Option<Vec<u8>>>(0),
                )
                .optional()
                .map_err(failure)?
                .flatten()
            else {
                return Ok(None);
            };
            let mut response: ThreadResponse = protocol::decode(&metadata).map_err(failure)?;
            let mut statement = connection
                .prepare("SELECT body FROM turns WHERE provider = ?1 AND id = ?2 ORDER BY position")
                .map_err(failure)?;
            let mut rows = statement.query(key).map_err(failure)?;
            let mut turns = Vec::new();
            while let Some(row) = rows.next().map_err(failure)? {
                turns.push(std::sync::Arc::new(decode_turn(
                    &row.get::<_, Vec<u8>>(0).map_err(failure)?,
                )?));
            }
            response.thread.turns = Some(turns);
            Ok(Some(response))
        })
    }

    /// The newest turns before `end`, or None until the conversation is imported.
    pub(crate) fn window(
        &self,
        conversation: &SessionRef,
        end: Option<usize>,
        limit: usize,
    ) -> Result<Option<Window>, String> {
        let key = params![conversation.provider.key(), conversation.id];
        self.with(|connection| {
            let Some(metadata) = connection
                .query_row(
                    "SELECT metadata FROM conversations WHERE provider = ?1 AND id = ?2",
                    key,
                    |row| row.get::<_, Option<Vec<u8>>>(0),
                )
                .optional()
                .map_err(failure)?
                .flatten()
            else {
                return Ok(None);
            };
            let count = connection
                .query_row(
                    "SELECT COUNT(*) FROM turns WHERE provider = ?1 AND id = ?2",
                    key,
                    |row| row.get::<_, i64>(0),
                )
                .map_err(failure)? as usize;
            let end = end.unwrap_or(count);
            if end > count {
                return Err("conversation history cursor is no longer valid".into());
            }
            let mut rows = connection
                .prepare(
                    "SELECT body FROM turns WHERE provider = ?1 AND id = ?2 AND position < ?3
                     ORDER BY position DESC LIMIT ?4",
                )
                .map_err(failure)?;
            let mut rows = rows
                .query(params![
                    conversation.provider.key(),
                    conversation.id,
                    end as i64,
                    i64::try_from(limit).unwrap_or(i64::MAX)
                ])
                .map_err(failure)?;
            let (mut turns, mut bytes) = (Vec::new(), 0);
            while let Some(row) = rows.next().map_err(failure)? {
                let body = row.get::<_, Vec<u8>>(0).map_err(failure)?;
                if !turns.is_empty() && bytes + body.len() > WINDOW_BYTES {
                    break;
                }
                bytes += body.len();
                turns.push(std::sync::Arc::new(decode_turn(&body)?));
            }
            turns.reverse();
            let mut response: ThreadResponse = protocol::decode(&metadata).map_err(failure)?;
            let start = end - turns.len();
            response.thread.turns = Some(turns);
            response.thread.transcript_version = connection
                .query_row(
                    "SELECT version FROM versions WHERE provider = ?1 AND id = ?2",
                    key,
                    |row| row.get::<_, i64>(0),
                )
                .optional()
                .map_err(failure)?
                .map(|version| version as u64);
            Ok(Some(Window { response, start }))
        })
    }

    /// Imported conversations and their working directories.
    pub(crate) fn imported(&self) -> Result<Vec<(SessionRef, Option<String>)>, String> {
        self.with(|connection| {
            let mut rows = connection
                .prepare("SELECT provider, id, cwd FROM conversations WHERE metadata IS NOT NULL")
                .map_err(failure)?;
            let mut rows = rows.query([]).map_err(failure)?;
            let mut imported = Vec::new();
            while let Some(row) = rows.next().map_err(failure)? {
                let provider =
                    serde_json::from_value(serde_json::Value::String(row.get(0).map_err(failure)?))
                        .map_err(failure)?;
                imported.push((
                    SessionRef::new(provider, row.get(1).map_err(failure)?).map_err(failure)?,
                    row.get(2).map_err(failure)?,
                ));
            }
            Ok(imported)
        })
    }

    /// The latest stored occurrence of a turn of an imported conversation.
    pub(crate) fn turn(
        &self,
        conversation: &SessionRef,
        turn_id: &TurnId,
    ) -> Result<Option<Turn>, String> {
        self.with(|connection| {
            connection
                .query_row(
                    "SELECT body FROM turns JOIN conversations USING (provider, id)
                     WHERE provider = ?1 AND id = ?2 AND turn_id = ?3 AND metadata IS NOT NULL
                     ORDER BY position DESC LIMIT 1",
                    params![
                        conversation.provider.key(),
                        conversation.id,
                        turn_id.as_str()
                    ],
                    |row| row.get::<_, Vec<u8>>(0),
                )
                .optional()
                .map_err(failure)?
                .map(|body| decode_turn(&body))
                .transpose()
        })
    }

    /// Names change on the Host; the imported snapshot must follow them.
    pub(crate) fn rename(
        &self,
        conversation: &SessionRef,
        name: Option<&str>,
    ) -> Result<(), String> {
        let key = params![conversation.provider.key(), conversation.id];
        self.write(|transaction| {
            let Some(metadata) = transaction
                .query_row(
                    "SELECT metadata FROM conversations WHERE provider = ?1 AND id = ?2",
                    key,
                    |row| row.get::<_, Option<Vec<u8>>>(0),
                )
                .optional()
                .map_err(failure)?
                .flatten()
            else {
                return Ok(());
            };
            let mut response: ThreadResponse = protocol::decode(&metadata).map_err(failure)?;
            response.thread.name = name.map(Into::into);
            transaction
                .execute(
                    "UPDATE conversations SET metadata = ?3 WHERE provider = ?1 AND id = ?2",
                    params![
                        conversation.provider.key(),
                        conversation.id,
                        protocol::encode(&response).map_err(failure)?
                    ],
                )
                .map_err(failure)?;
            advance(transaction, conversation.provider.key(), &conversation.id)
        })
    }
}

/// Give the conversation the next Host-wide version.
fn advance(transaction: &Transaction, provider: &str, id: &str) -> Result<(), String> {
    transaction
        .execute(
            "INSERT INTO versions (provider, id, version)
             VALUES (?1, ?2, (SELECT COALESCE(MAX(version), 0) + 1 FROM versions))
             ON CONFLICT (provider, id) DO UPDATE SET version = excluded.version",
            params![provider, id],
        )
        .map_err(failure)?;
    Ok(())
}

fn insert(
    transaction: &Transaction,
    conversation: &SessionRef,
    position: i64,
    turn: &Turn,
) -> Result<(), String> {
    transaction
        .execute(
            "INSERT INTO turns (provider, id, position, turn_id, running, body)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                conversation.provider.key(),
                conversation.id,
                position,
                turn.id.as_str(),
                turn.status == TurnStatus::Running,
                protocol::encode(turn).map_err(failure)?
            ],
        )
        .map_err(failure)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_protocol::{
        items::{AssistantPhase, ItemBody},
        models::{Item, Thread},
        session::ProviderKind,
    };
    use std::sync::Arc;

    fn conversation() -> SessionRef {
        SessionRef::new(ProviderKind::Codex, "conversation".into()).unwrap()
    }

    fn turn(id: &str, items: &[&str], status: TurnStatus) -> Turn {
        Turn {
            id: id.into(),
            status,
            items: Some(
                items
                    .iter()
                    .map(|item| {
                        Arc::new(Item::new(
                            (*item).into(),
                            Default::default(),
                            ItemBody::AssistantText {
                                text: format!("{id}/{item}"),
                                phase: AssistantPhase::Final,
                            },
                        ))
                    })
                    .collect(),
            ),
            ..Default::default()
        }
    }

    fn history(turns: Vec<Turn>) -> ThreadResponse {
        ThreadResponse {
            thread: Thread {
                id: Some(conversation()),
                name: Some("native".into()),
                status: SessionStatus::Running,
                turns: Some(turns.into_iter().map(Arc::new).collect()),
                ..Default::default()
            },
            model: None,
            effort: None,
            service_tier: None,
        }
    }

    fn store() -> (tempfile::TempDir, Transcripts) {
        let root = tempfile::tempdir().unwrap();
        let store = Transcripts::open(&root.path().join("state/transcripts.sqlite"));
        (root, store)
    }

    fn texts(window: &Window) -> Vec<String> {
        window
            .response
            .thread
            .turns
            .iter()
            .flatten()
            .flat_map(|turn| turn.items.iter().flatten())
            .map(|item| match item.body() {
                ItemBody::AssistantText { text, .. } => text.clone(),
                _ => unreachable!(),
            })
            .collect()
    }

    #[test]
    fn imports_match_repeated_turn_ids_by_items_and_keep_unmatched_occurrences() {
        let (_root, store) = store();
        let target = conversation();
        store
            .import(
                &target,
                history(vec![
                    turn("reused", &["a"], TurnStatus::Completed),
                    turn("reused", &["b"], TurnStatus::Failed),
                    turn("other", &["tail"], TurnStatus::Completed),
                ]),
            )
            .unwrap();
        let mut shared = turn("reused", &["b", "native-only"], TurnStatus::Running);
        shared.provider = Some(ProviderKind::Codex);
        shared.supports_fork = Some(true);
        store
            .import(
                &target,
                history(vec![
                    shared,
                    turn("reused", &["new"], TurnStatus::Completed),
                ]),
            )
            .unwrap();
        let saved = store
            .history(&target)
            .unwrap()
            .unwrap()
            .thread
            .turns
            .unwrap();
        assert_eq!(
            saved
                .iter()
                .map(|turn| turn.id.as_str())
                .collect::<Vec<_>>(),
            ["reused", "reused", "reused", "other"]
        );
        assert_eq!(saved[0].status, TurnStatus::Failed);
        assert_eq!(saved[0].provider, Some(ProviderKind::Codex));
        assert_eq!(saved[0].supports_fork, Some(true));
        assert_eq!(
            saved
                .iter()
                .flat_map(|turn| turn.items.iter().flatten())
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            ["b", "new", "a", "tail"]
        );

        // An itemless native observation matches the first stored occurrence
        // and suppresses all stored occurrences covered by that observation.
        store
            .import(
                &target,
                history(vec![turn("reused", &[], TurnStatus::Running)]),
            )
            .unwrap();
        let saved = store
            .history(&target)
            .unwrap()
            .unwrap()
            .thread
            .turns
            .unwrap();
        assert_eq!(saved.len(), 2);
        assert_eq!(saved[0].items.as_ref().unwrap()[0].id.as_str(), "b");
        assert_eq!(saved[1].id.as_str(), "other");
    }

    #[test]
    fn recorded_turns_stay_hidden_until_import_and_keep_their_items() {
        let (_root, store) = store();
        let target = conversation();
        store
            .record(&target, &turn("shared", &["live"], TurnStatus::Completed))
            .unwrap();
        store
            .record(&target, &turn("later", &["after"], TurnStatus::Completed))
            .unwrap();
        assert!(store.window(&target, None, 10).unwrap().is_none());
        assert!(store.turn(&target, &"later".into()).unwrap().is_none());

        let mut native = turn("shared", &["live"], TurnStatus::Completed);
        Arc::make_mut(&mut native.items.as_mut().unwrap()[0]).body =
            agent_protocol::items::ItemContent::Inline {
                body: Box::new(ItemBody::AssistantText {
                    text: "native".into(),
                    phase: AssistantPhase::Final,
                }),
            };
        store
            .import(
                &target,
                history(vec![turn("first", &["a"], TurnStatus::Completed), native]),
            )
            .unwrap();
        let window = store.window(&target, None, 10).unwrap().unwrap();
        assert_eq!(texts(&window), ["first/a", "shared/live", "later/after"]);

        // A turn that finished while its native history was read keeps the
        // recorded result instead of the unfinished read.
        let running = SessionRef::new(ProviderKind::Codex, "running".into()).unwrap();
        store
            .record(
                &running,
                &turn("racing", &["start", "end"], TurnStatus::Completed),
            )
            .unwrap();
        store
            .import(
                &running,
                history(vec![turn("racing", &["start"], TurnStatus::Running)]),
            )
            .unwrap();
        let window = store.window(&running, None, 10).unwrap().unwrap();
        assert_eq!(texts(&window), ["racing/start", "racing/end"]);
        assert_eq!(
            window.response.thread.turns.unwrap()[0].status,
            TurnStatus::Completed
        );
        assert_eq!(window.start, 0);
        assert_eq!(window.response.thread.name.as_deref(), Some("native"));
        assert_eq!(window.response.thread.status, SessionStatus::Unknown);
    }

    #[test]
    fn windows_page_backwards_and_stop_at_the_byte_budget() {
        let (_root, store) = store();
        let target = conversation();
        let turns = (0..12)
            .map(|index| turn(&index.to_string(), &["x"], TurnStatus::Completed))
            .collect();
        store.import(&target, history(turns)).unwrap();
        let newest = store.window(&target, None, 10).unwrap().unwrap();
        assert_eq!(
            (
                newest.start,
                newest.response.thread.turns.as_ref().unwrap().len()
            ),
            (2, 10)
        );
        let older = store
            .window(&target, Some(newest.start), 10)
            .unwrap()
            .unwrap();
        assert_eq!(texts(&older), ["0/x", "1/x"]);
        assert!(store.window(&target, Some(13), 10).is_err());

        let large = |id: &str| {
            let mut turn = turn(id, &["x"], TurnStatus::Completed);
            Arc::make_mut(&mut turn.items.as_mut().unwrap()[0]).body =
                agent_protocol::items::ItemContent::Inline {
                    body: Box::new(ItemBody::AssistantText {
                        text: "x".repeat(WINDOW_BYTES / 2 + 1),
                        phase: AssistantPhase::Final,
                    }),
                };
            turn
        };
        let target = SessionRef::new(ProviderKind::Codex, "large".into()).unwrap();
        store
            .import(&target, history(vec![large("a"), large("b")]))
            .unwrap();
        let window = store.window(&target, None, 10).unwrap().unwrap();
        assert_eq!(
            window.start, 1,
            "a window keeps one turn past the budget, never two"
        );
    }

    #[test]
    fn recording_replaces_the_same_occurrence_and_keeps_known_items() {
        let (_root, store) = store();
        let target = conversation();
        store
            .import(
                &target,
                history(vec![turn("repeated", &["old"], TurnStatus::Completed)]),
            )
            .unwrap();
        // A reused turn ID with new items is a new occurrence.
        store
            .record(&target, &turn("repeated", &["new"], TurnStatus::Running))
            .unwrap();
        store
            .record(
                &target,
                &turn("repeated", &["new", "more"], TurnStatus::Completed),
            )
            .unwrap();
        let mut bare = turn("repeated", &[], TurnStatus::Failed);
        bare.items = None;
        store.record(&target, &bare).unwrap();
        let window = store.window(&target, None, 10).unwrap().unwrap();
        let turns = window.response.thread.turns.as_ref().unwrap();
        assert_eq!(
            texts(&window),
            ["repeated/old", "repeated/new", "repeated/more"]
        );
        assert_eq!(turns[1].status, TurnStatus::Failed);
        assert_eq!(
            store
                .turn(&target, &"repeated".into())
                .unwrap()
                .unwrap()
                .status,
            TurnStatus::Failed
        );
    }

    #[test]
    fn restarts_interrupt_running_turns_and_renames_follow_imports() {
        let (_root, store) = store();
        let target = conversation();
        store
            .rename(&target, Some("ignored before import"))
            .unwrap();
        store
            .import(
                &target,
                history(vec![turn("running", &["partial"], TurnStatus::Running)]),
            )
            .unwrap();
        store.rename(&target, Some("renamed")).unwrap();
        store.interrupt_running().unwrap();
        let window = store.window(&target, None, 10).unwrap().unwrap();
        assert_eq!(window.response.thread.name.as_deref(), Some("renamed"));
        assert_eq!(
            window.response.thread.turns.as_ref().unwrap()[0].status,
            TurnStatus::Interrupted
        );
    }

    #[test]
    fn late_updates_change_the_stored_turn_and_its_version() {
        let (_root, store) = store();
        let target = conversation();
        store
            .import(
                &target,
                history(vec![turn("done", &["answer"], TurnStatus::Completed)]),
            )
            .unwrap();
        let before = store
            .window(&target, None, 10)
            .unwrap()
            .unwrap()
            .response
            .thread
            .transcript_version;
        store
            .update_turn(&target, &"done".into(), |mut turn| {
                turn.items.as_mut()?.push(Arc::new(Item::new(
                    "late".into(),
                    Default::default(),
                    ItemBody::AssistantText {
                        text: "done/late".into(),
                        phase: AssistantPhase::Final,
                    },
                )));
                Some(turn)
            })
            .unwrap();
        store
            .update_turn(&target, &"missing".into(), |_| unreachable!())
            .unwrap();
        let window = store.window(&target, None, 10).unwrap().unwrap();
        assert_eq!(texts(&window), ["done/answer", "done/late"]);
        assert!(window.response.thread.transcript_version > before);
    }

    #[test]
    fn every_change_gives_its_conversation_a_newer_version() {
        let (_root, store) = store();
        let target = conversation();
        let other = SessionRef::new(ProviderKind::Codex, "other".into()).unwrap();
        let version = |target: &SessionRef| {
            store
                .window(target, None, 10)
                .unwrap()
                .unwrap()
                .response
                .thread
                .transcript_version
                .unwrap()
        };
        store.import(&target, history(vec![])).unwrap();
        store.import(&other, history(vec![])).unwrap();
        let imported = version(&target);
        assert!(version(&other) > imported);
        store
            .record(&target, &turn("t", &["i"], TurnStatus::Running))
            .unwrap();
        let recorded = version(&target);
        assert!(recorded > version(&other));
        store.rename(&target, Some("renamed")).unwrap();
        let renamed = version(&target);
        assert!(renamed > recorded);
        store.interrupt_running().unwrap();
        assert!(version(&target) > renamed);
        assert_eq!(version(&target), version(&target), "reads keep the version");
    }

    #[test]
    fn complete_history_requires_an_import_and_is_not_limited_to_a_display_window() {
        let root = tempfile::tempdir().unwrap();
        let store = Transcripts::open(&root.path().join("transcripts.sqlite"));
        let target = conversation();
        let mut first = turn("first", &["large"], TurnStatus::Completed);
        Arc::make_mut(&mut first.items.as_mut().unwrap()[0]).body =
            agent_protocol::items::ItemContent::Inline {
                body: Box::new(ItemBody::AssistantText {
                    text: "x".repeat(WINDOW_BYTES),
                    phase: AssistantPhase::Final,
                }),
            };
        store.record(&target, &first).unwrap();
        assert!(store.history(&target).unwrap().is_none());
        let second = turn("second", &["last"], TurnStatus::Completed);
        store.import(&target, history(vec![first, second])).unwrap();
        assert!(store.window(&target, None, 10).unwrap().unwrap().start > 0);
        let complete = store.history(&target).unwrap().unwrap();
        assert_eq!(
            complete
                .thread
                .turns
                .unwrap()
                .iter()
                .map(|turn| turn.id.as_str())
                .collect::<Vec<_>>(),
            ["first", "second"]
        );
    }

    #[test]
    fn an_unopened_store_fails_instead_of_reporting_empty_transcripts() {
        let root = tempfile::tempdir().unwrap();
        let store = Transcripts::open(root.path());
        assert!(store.window(&conversation(), None, 10).is_err());
        assert!(
            store
                .record(&conversation(), &turn("t", &["i"], TurnStatus::Completed))
                .is_err()
        );
    }
}
