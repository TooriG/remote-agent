//! Host-owned transcripts. Native provider history is read once to import a
//! conversation; afterwards the Host records each turn as it finishes and
//! never returns to native storage for reads. Rows keep every item inline;
//! readers summarize or defer bodies for transfer.
use super::database::{Database, advance, append_event, ensure_thread};
use agent_protocol::{
    execution::TurnStatus,
    ids::{ItemId, TurnId},
    models::{Item, SessionStatus, ThreadResponse, Turn},
    protocol,
    session::SessionRef,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use std::{collections::HashMap, sync::Arc};

/// A window stops growing past this many encoded bytes, keeping at least one turn.
const WINDOW_BYTES: usize = 1 << 20;

pub(crate) struct Transcripts {
    pub(super) database: Arc<Database>,
}

pub(crate) struct Window {
    pub response: ThreadResponse,
    /// Position of the first returned turn. Earlier turns precede it.
    pub start: usize,
}

fn failure(error: impl std::fmt::Display) -> String {
    error.to_string()
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

fn encode_metadata(response: ThreadResponse) -> Result<String, String> {
    let mut value = serde_json::to_value(response).map_err(failure)?;
    let thread = value["thread"].as_object_mut().expect("thread record");
    for field in [
        "id",
        "cwd",
        "name",
        "preview",
        "settled",
        "listRevision",
        "transcriptVersion",
    ] {
        thread.remove(field);
    }
    serde_json::to_string(&value).map_err(failure)
}

fn read_metadata(
    connection: &Connection,
    session: &SessionRef,
) -> Result<Option<ThreadResponse>, String> {
    connection.query_row(
        "SELECT json_extract(payload_json, '$.history'), cwd, name, preview, settled FROM threads WHERE provider = ?1 AND id = ?2 AND imported = 1",
        params![session.provider.key(), session.id],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, Option<String>>(2)?, row.get::<_, Option<String>>(3)?, row.get::<_, bool>(4)?)),
    ).optional().map_err(failure)?.map(|(payload, cwd, name, preview, settled)| {
        let mut response: ThreadResponse = serde_json::from_str(&payload).map_err(failure)?;
        response.thread.id = Some(session.clone());
        response.thread.cwd = cwd;
        response.thread.name = name;
        response.thread.preview = preview;
        response.thread.settled = settled;
        Ok(response)
    }).transpose()
}

impl Transcripts {
    pub(super) fn new(database: Arc<Database>) -> Self {
        Self { database }
    }

    #[cfg(test)]
    pub(crate) fn open(path: &std::path::Path) -> Self {
        Self::new(Arc::new(Database::open(path)))
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
        let cwd = response.thread.cwd.take();
        let name = response.thread.name.take();
        let preview = response.thread.preview.take();
        let metadata = encode_metadata(response)?;
        let key = params![conversation.provider.key(), conversation.id];
        self.database.write(|transaction| {
            ensure_thread(transaction, conversation)?;
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
                        .find(|turn| native.matches_occurrence(turn));
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
                                .any(|native| native.matches_occurrence(turn))
                        })
                        .map(std::borrow::Cow::Borrowed),
                )
                .collect();
            transaction
                .execute("DELETE FROM turns WHERE provider = ?1 AND id = ?2", key)
                .map_err(failure)?;
            for (position, turn) in turns.iter().enumerate() {
                write_turn(transaction, conversation, position as i64, turn)?;
            }
            transaction
                .execute(
                    "UPDATE threads SET imported = 1, cwd = ?3, name = CASE WHEN name_override THEN name ELSE ?4 END, preview = CASE WHEN preview_override THEN preview ELSE ?5 END,
                        payload_json = json_set(payload_json, '$.history', json(?6)) WHERE provider = ?1 AND id = ?2",
                    params![conversation.provider.key(), conversation.id, cwd, name, preview, metadata],
                )
                .map_err(failure)?;
            advance(transaction, conversation.provider.key(), &conversation.id)
        })
    }

    /// Replace the stored occurrence of a live turn, or append it. Conversations
    /// that are not imported yet keep these turns for their import.
    /// A turn observed without items keeps the items already stored for it.
    /// The removed item identifies the occurrence even after its last item is gone.
    pub(crate) fn record(
        &self,
        conversation: &SessionRef,
        turn: &Turn,
        removed_item: Option<&ItemId>,
    ) -> Result<(), String> {
        let key = params![conversation.provider.key(), conversation.id];
        self.database.write(|transaction| {
            let stored = find_turn(transaction, conversation, &turn.id, false, |stored| {
                removed_item
                    .map_or_else(
                        || stored.shares_items(turn),
                        |id| stored.items.iter().flatten().any(|item| &item.id == id),
                    )
                    .then_some(stored)
            })?;
            let stored = match stored {
                None if removed_item.is_none() => {
                    find_turn(transaction, conversation, &turn.id, false, |stored| {
                        stored.matches_occurrence(turn).then_some(stored)
                    })?
                }
                stored => stored,
            };
            let mut turn = std::borrow::Cow::Borrowed(turn);
            let position = match stored {
                Some((position, stored)) => {
                    if turn.items.is_none() {
                        turn.to_mut().items = stored.items;
                    }
                    position
                }
                None => turn_count(transaction, conversation.provider.key(), &conversation.id)?,
            };
            transaction
                .execute(
                    "INSERT OR IGNORE INTO threads (provider, id) VALUES (?1, ?2)",
                    key,
                )
                .map_err(failure)?;
            write_turn(transaction, conversation, position, &turn)?;
            advance(transaction, conversation.provider.key(), &conversation.id)
        })
    }

    /// Change the stored occurrence containing the item, or the latest occurrence
    /// for changes that do not name an item.
    pub(crate) fn update_turn(
        &self,
        conversation: &SessionRef,
        turn_id: &TurnId,
        item_id: Option<&ItemId>,
        change: impl FnOnce(Turn) -> Option<Turn>,
    ) -> Result<(), String> {
        self.database.write(|transaction| {
            let Some((position, turn)) =
                find_turn(transaction, conversation, turn_id, false, |turn| {
                    item_id
                        .is_none_or(|id| turn.items.iter().flatten().any(|item| &item.id == id))
                        .then_some(turn)
                })?
            else {
                return Ok(());
            };
            let Some(turn) = change(turn) else {
                return Ok(());
            };
            write_turn(transaction, conversation, position, &turn)?;
            advance(transaction, conversation.provider.key(), &conversation.id)
        })
    }

    /// No turn runs across a Host restart.
    pub(crate) fn interrupt_running(&self) -> Result<(), String> {
        self.database.write(|transaction| {
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
        self.database.with(|connection| {
            let key = params![conversation.provider.key(), conversation.id];
            let Some(mut response) = read_metadata(connection, conversation)? else {
                return Ok(None);
            };
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
        self.database.with(|connection| {
            let Some(mut response) = read_metadata(connection, conversation)? else {
                return Ok(None);
            };
            let count =
                turn_count(connection, conversation.provider.key(), &conversation.id)? as usize;
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
        self.database.with(|connection| {
            let mut rows = connection
                .prepare("SELECT provider, id, cwd FROM threads WHERE imported = 1")
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
        self.database.with(|connection| {
            find_turn(connection, conversation, turn_id, true, Some)
                .map(|found| found.map(|(_, turn)| turn))
        })
    }

    pub(crate) fn item(
        &self,
        conversation: &SessionRef,
        turn_id: &TurnId,
        item_id: &ItemId,
        imported_only: bool,
    ) -> Result<Option<std::sync::Arc<Item>>, String> {
        self.database.with(|connection| {
            find_turn(connection, conversation, turn_id, imported_only, |turn| {
                turn.items?.into_iter().find(|item| &item.id == item_id)
            })
            .map(|found| found.map(|(_, item)| item))
        })
    }
}

fn find_turn<T>(
    connection: &Connection,
    conversation: &SessionRef,
    turn_id: &TurnId,
    imported_only: bool,
    select: impl Fn(Turn) -> Option<T>,
) -> Result<Option<(i64, T)>, String> {
    let mut statement = connection
        .prepare_cached(
            "SELECT position, body FROM turns JOIN threads USING (provider, id)
         WHERE provider = ?1 AND id = ?2 AND turn_id = ?3 AND (?4 = 0 OR imported = 1)
         ORDER BY position DESC",
        )
        .map_err(failure)?;
    let mut rows = statement
        .query(params![
            conversation.provider.key(),
            conversation.id,
            turn_id.as_str(),
            imported_only
        ])
        .map_err(failure)?;
    while let Some(row) = rows.next().map_err(failure)? {
        let turn = decode_turn(&row.get::<_, Vec<u8>>(1).map_err(failure)?)?;
        if let Some(selected) = select(turn) {
            return Ok(Some((row.get(0).map_err(failure)?, selected)));
        }
    }
    Ok(None)
}

/// Imports enumerate positions, and live writes only replace or append. The
/// final indexed position therefore gives the count without scanning the history.
fn turn_count(connection: &Connection, provider: &str, id: &str) -> Result<i64, String> {
    connection
        .prepare_cached(
            "SELECT COALESCE(MAX(position) + 1, 0) FROM turns WHERE provider = ?1 AND id = ?2",
        )
        .map_err(failure)?
        .query_row(params![provider, id], |row| row.get(0))
        .map_err(failure)
}

fn write_turn(
    transaction: &Transaction,
    conversation: &SessionRef,
    position: i64,
    turn: &Turn,
) -> Result<(), String> {
    ensure_thread(transaction, conversation)?;
    let sequence = append_event(transaction, conversation, "turn.updated", turn)?;
    for item in turn.items.iter().flatten() {
        if let Some(id) = &item.client_input_id
            && matches!(
                item.body(),
                agent_protocol::items::ItemBody::UserMessage { .. }
            )
        {
            transaction.execute("UPDATE command_receipts SET delivery = 'accepted', resolved = 1, turn_id = ?4, result_sequence = ?5 WHERE provider = ?1 AND id = ?2 AND command_id = ?3 AND kind = 'submission'", params![conversation.provider.key(), conversation.id, id.as_str(), turn.id.as_str(), sequence as i64]).map_err(failure)?;
        }
    }
    transaction
        .prepare_cached(
            "INSERT INTO turns (provider, id, position, turn_id, running, body)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (provider, id, position) DO UPDATE SET
                 turn_id = excluded.turn_id, running = excluded.running, body = excluded.body",
        )
        .map_err(failure)?
        .execute(params![
            conversation.provider.key(),
            conversation.id,
            position,
            turn.id.as_str(),
            turn.status == TurnStatus::Running,
            protocol::encode(turn).map_err(failure)?
        ])
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
            .record(
                &target,
                &turn("shared", &["live"], TurnStatus::Completed),
                None,
            )
            .unwrap();
        store
            .record(
                &target,
                &turn("later", &["after"], TurnStatus::Completed),
                None,
            )
            .unwrap();
        assert!(store.window(&target, None, 10).unwrap().is_none());
        assert!(store.turn(&target, &"later".into()).unwrap().is_none());
        assert!(
            store
                .item(&target, &"later".into(), &"after".into(), true)
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .item(&target, &"later".into(), &"after".into(), false)
                .unwrap()
                .is_some()
        );

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
                None,
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
    fn failed_cached_writes_roll_back_and_recover_without_gaps_after_reopening() {
        let (root, store) = store();
        let target = conversation();
        store.import(&target, history(vec![])).unwrap();
        assert_eq!(store.window(&target, None, 10).unwrap().unwrap().start, 0);
        store
            .record(&target, &turn("first", &["a"], TurnStatus::Completed), None)
            .unwrap();
        let version = store
            .window(&target, None, 10)
            .unwrap()
            .unwrap()
            .response
            .thread
            .transcript_version;
        store
            .database
            .with(|connection| {
                connection
                    .execute_batch(
                        "CREATE TRIGGER reject_turn BEFORE INSERT ON turns
                         WHEN NEW.turn_id = 'rejected' OR NEW.running = 1
                         BEGIN SELECT RAISE(ABORT, 'rejected by test'); END;",
                    )
                    .map_err(failure)
            })
            .unwrap();
        assert!(
            store
                .record(
                    &target,
                    &turn("rejected", &["b"], TurnStatus::Completed),
                    None
                )
                .is_err()
        );
        assert!(
            store
                .record(&target, &turn("first", &["a"], TurnStatus::Running), None)
                .is_err()
        );
        assert!(
            store
                .update_turn(&target, &"first".into(), None, |mut turn| {
                    turn.status = TurnStatus::Running;
                    Some(turn)
                })
                .is_err()
        );
        let unchanged = store.window(&target, None, 10).unwrap().unwrap();
        assert_eq!(texts(&unchanged), ["first/a"]);
        assert_eq!(
            unchanged.response.thread.turns.as_ref().unwrap()[0].status,
            TurnStatus::Completed
        );
        assert_eq!(unchanged.response.thread.transcript_version, version);
        store
            .record(
                &target,
                &turn("second", &["b"], TurnStatus::Completed),
                None,
            )
            .unwrap();
        drop(store);
        let store = Transcripts::open(&root.path().join("state/transcripts.sqlite"));
        let latest = store.window(&target, None, 1).unwrap().unwrap();
        assert_eq!(latest.start, 1);
        assert_eq!(texts(&latest), ["second/b"]);
        // Replacing an earlier occurrence must not move the next append position.
        store
            .record(&target, &turn("first", &["a"], TurnStatus::Failed), None)
            .unwrap();
        store
            .record(&target, &turn("third", &["c"], TurnStatus::Completed), None)
            .unwrap();
        let window = store.window(&target, None, 10).unwrap().unwrap();
        assert_eq!(window.start, 0);
        assert_eq!(texts(&window), ["first/a", "second/b", "third/c"]);
        store
            .database
            .with(|connection| {
                let counted: i64 = connection
                    .query_row(
                        "SELECT COUNT(*) FROM turns WHERE provider = ?1 AND id = ?2",
                        params![target.provider.key(), target.id],
                        |row| row.get(0),
                    )
                    .map_err(failure)?;
                assert_eq!(
                    turn_count(connection, target.provider.key(), &target.id)?,
                    counted
                );
                Ok(())
            })
            .unwrap();
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
            .record(
                &target,
                &turn("repeated", &["new"], TurnStatus::Running),
                None,
            )
            .unwrap();
        store
            .record(
                &target,
                &turn("repeated", &["new", "more"], TurnStatus::Completed),
                None,
            )
            .unwrap();
        let mut bare = turn("repeated", &[], TurnStatus::Failed);
        bare.items = None;
        store.record(&target, &bare, None).unwrap();
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
        store
            .record(
                &target,
                &turn("repeated", &["old", "late"], TurnStatus::Completed),
                None,
            )
            .unwrap();
        let window = store.window(&target, None, 10).unwrap().unwrap();
        assert_eq!(window.response.thread.turns.as_ref().unwrap().len(), 2);
        assert_eq!(
            texts(&window),
            [
                "repeated/old",
                "repeated/late",
                "repeated/new",
                "repeated/more"
            ]
        );
        assert_eq!(
            window.response.thread.turns.as_ref().unwrap()[1].status,
            TurnStatus::Failed
        );
    }

    #[test]
    fn known_items_identify_older_occurrences_before_empty_later_turns() {
        let (_root, store) = store();
        let target = conversation();
        store
            .import(
                &target,
                history(vec![
                    turn("repeated", &["old"], TurnStatus::Completed),
                    turn("repeated", &[], TurnStatus::Completed),
                ]),
            )
            .unwrap();
        store
            .record(
                &target,
                &turn("repeated", &["old", "late"], TurnStatus::Completed),
                None,
            )
            .unwrap();
        let window = store.window(&target, None, 10).unwrap().unwrap();
        assert_eq!(texts(&window), ["repeated/old", "repeated/late"]);
        let turns = window.response.thread.turns.unwrap();
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].items.as_ref().unwrap().len(), 2);
        assert!(turns[1].items.as_ref().unwrap().is_empty());
    }

    #[test]
    fn removing_an_unrecorded_occurrence_does_not_replace_an_older_one() {
        let (_root, store) = store();
        let target = conversation();
        store
            .import(
                &target,
                history(vec![turn("repeat", &["old"], TurnStatus::Completed)]),
            )
            .unwrap();
        // A completion can fail to persist before its last item is removed.
        store
            .record(
                &target,
                &turn("repeat", &[], TurnStatus::Completed),
                Some(&"unrecorded".into()),
            )
            .unwrap();
        let window = store.window(&target, None, 10).unwrap().unwrap();
        assert_eq!(texts(&window), ["repeat/old"]);
        let turns = window.response.thread.turns.unwrap();
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].items.as_ref().unwrap()[0].id.as_str(), "old");
        assert!(turns[1].items.as_ref().unwrap().is_empty());
    }

    #[test]
    fn restarts_interrupt_running_turns_and_renames_follow_imports() {
        let (_root, store) = store();
        let target = conversation();
        store
            .database
            .rename(&target, Some("ignored before import"))
            .unwrap();
        store
            .import(
                &target,
                history(vec![turn("running", &["partial"], TurnStatus::Running)]),
            )
            .unwrap();
        store.database.rename(&target, Some("renamed")).unwrap();
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
            .update_turn(&target, &"done".into(), None, |mut turn| {
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
            .update_turn(&target, &"missing".into(), None, |_| unreachable!())
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
            .record(&target, &turn("t", &["i"], TurnStatus::Running), None)
            .unwrap();
        let recorded = version(&target);
        assert!(recorded > version(&other));
        store.database.rename(&target, Some("renamed")).unwrap();
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
        store.record(&target, &first, None).unwrap();
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
                .record(
                    &conversation(),
                    &turn("t", &["i"], TurnStatus::Completed),
                    None
                )
                .is_err()
        );
    }
}
