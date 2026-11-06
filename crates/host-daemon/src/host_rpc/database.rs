//! One transaction owner for conversation state, command receipts and committed events.
use agent_protocol::session::SessionRef;
use rusqlite::{Connection, Transaction, TransactionBehavior, params};
use std::{path::Path, sync::Mutex};
mod metadata;
mod submissions;
pub(super) use metadata::advance;

pub(super) struct Database {
    connection: Mutex<Result<Connection, String>>,
}

pub(super) fn failure(error: impl std::fmt::Display) -> String {
    error.to_string()
}

impl Default for Database {
    fn default() -> Self {
        Self::open(Path::new(":memory:"))
    }
}

impl Database {
    pub(super) fn open(path: &Path) -> Self {
        let connection = (|| {
            if let Some(parent) = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                std::fs::create_dir_all(parent).map_err(failure)?;
            }
            let connection = Connection::open(path).map_err(failure)?;
            connection
                .busy_timeout(std::time::Duration::from_secs(10))
                .map_err(failure)?;
            connection
                .execute_batch(include_str!("database/schema.sql"))
                .map_err(failure)?;
            Ok(connection)
        })();
        Self {
            connection: Mutex::new(connection),
        }
    }

    pub(super) fn with<T>(
        &self,
        work: impl FnOnce(&mut Connection) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut connection = self
            .connection
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        work(connection.as_mut().map_err(|error| error.clone())?)
    }

    pub(super) fn write<T>(
        &self,
        work: impl FnOnce(&Transaction) -> Result<T, String>,
    ) -> Result<T, String> {
        self.with(|connection| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(failure)?;
            let result = work(&transaction)?;
            transaction.commit().map_err(failure)?;
            Ok(result)
        })
    }
}

pub(super) fn ensure_thread(connection: &Connection, session: &SessionRef) -> Result<(), String> {
    session.validate().map_err(str::to_owned)?;
    connection
        .execute(
            "INSERT OR IGNORE INTO threads (provider, id) VALUES (?1, ?2)",
            params![session.provider.key(), session.id],
        )
        .map_err(failure)?;
    Ok(())
}

pub(super) fn append_event(
    connection: &Connection,
    session: &SessionRef,
    kind: &str,
    payload: &impl serde::Serialize,
) -> Result<u64, String> {
    ensure_thread(connection, session)?;
    connection
        .execute(
            "INSERT INTO events (provider, id, kind, payload_json) VALUES (?1, ?2, ?3, ?4)",
            params![
                session.provider.key(),
                session.id,
                kind,
                serde_json::to_string(payload).map_err(failure)?
            ],
        )
        .map_err(failure)?;
    Ok(connection.last_insert_rowid() as u64)
}

/// Decode the SQL identity using the same validation as the RPC boundary.
pub(super) fn session_ref(provider: String, id: String) -> Result<SessionRef, String> {
    let provider = serde_json::from_value(serde_json::Value::String(provider)).map_err(failure)?;
    SessionRef::new(provider, id).map_err(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_protocol::session::ProviderKind;

    #[test]
    fn provider_bindings_cannot_activate_another_public_conversations_runtime() {
        let database = Database::default();
        let first = SessionRef::new(ProviderKind::Codex, "same".into()).unwrap();
        let second = SessionRef::new(ProviderKind::Claude, "same".into()).unwrap();
        database.write(|tx| {
            ensure_thread(tx, &first)?;
            ensure_thread(tx, &second)?;
            tx.execute("INSERT INTO provider_threads (provider, id, thread_provider, thread_id, ordinal) VALUES ('codex', 'native', 'codex', 'same', 0)", []).map_err(failure)?;
            Ok(())
        }).unwrap();
        assert!(database.write(|tx| {
            tx.execute("UPDATE threads SET active_provider = 'codex', active_id = 'native' WHERE provider = 'claude'", []).map_err(failure)?;
            Ok(())
        }).is_err());
        let active: Option<String> = database
            .with(|db| {
                db.query_row(
                    "SELECT active_id FROM threads WHERE provider = 'claude'",
                    [],
                    |row| row.get(0),
                )
                .map_err(failure)
            })
            .unwrap();
        assert!(
            active.is_none(),
            "the deferred ownership constraint rolls back the entire transaction"
        );
        assert!(
            database
                .with(|db| db
                    .execute("UPDATE threads SET unread = 1 WHERE provider = 'codex'", [])
                    .map_err(failure))
                .is_err()
        );
        assert!(
            database
                .with(|db| db
                    .execute(
                        "UPDATE threads SET settled = 'invalid' WHERE provider = 'codex'",
                        []
                    )
                    .map_err(failure))
                .is_err()
        );
    }
}
