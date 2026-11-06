use super::*;
use crate::host_rpc::database::{Database, append_event, failure, session_ref};
use rusqlite::params;

/// Indexed identity and scalar metadata live in columns, never in this payload.
#[derive(Serialize, Deserialize)]
struct Payload {
    segments: Vec<Segment>,
    handoffs: Vec<Handoff>,
    startup: Option<Startup>,
}

pub(super) fn load(database: &Database) -> Result<Vec<Conversation>, String> {
    database.with(|connection| {
        let mut statement = connection
            .prepare(
                "SELECT provider, id, cwd, CASE WHEN name_override THEN name END, CASE WHEN preview_override THEN preview END, active_provider, active_id,
                    json_extract(payload_json, '$.conversation') FROM threads WHERE registered = 1",
            )
            .map_err(failure)?;
        let mut rows = statement.query([]).map_err(failure)?;
        let mut result = Vec::new();
        while let Some(row) = rows.next().map_err(failure)? {
            let id = session_ref(row.get(0).map_err(failure)?, row.get(1).map_err(failure)?)?;
            let provider: Option<String> = row.get(5).map_err(failure)?;
            let active = match provider {
                Some(provider) => session_ref(provider, row.get(6).map_err(failure)?)?,
                None => id.clone(),
            };
            let payload: Payload = serde_json::from_str(&row.get::<_, String>(7).map_err(failure)?)
                .map_err(failure)?;
            let bindings = load_bindings(connection, &id)?;
            result.push(Conversation {
                id,
                active,
                cwd: row.get(2).map_err(failure)?,
                name: row.get(3).map_err(failure)?,
                preview: row.get(4).map_err(failure)?,
                bindings,
                segments: payload.segments,
                handoffs: payload.handoffs,
                startup: payload.startup,
            });
        }
        Ok(result)
    })
}

fn load_bindings(
    connection: &rusqlite::Connection,
    id: &SessionRef,
) -> Result<Vec<Binding>, String> {
    let mut statement = connection.prepare(
        "SELECT provider, id, covered FROM provider_threads WHERE thread_provider = ?1 AND thread_id = ?2 ORDER BY ordinal",
    ).map_err(failure)?;
    statement
        .query_map(params![id.provider.key(), id.id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<i64>>(2)?,
            ))
        })
        .map_err(failure)?
        .map(|row| {
            let (provider, id, covered) = row.map_err(failure)?;
            Ok(Binding {
                session: session_ref(provider, id)?,
                covered: covered.map(|value| value as usize),
            })
        })
        .collect()
}

pub(super) fn save(database: &Database, row: &Conversation) -> Result<(), String> {
    let payload = Payload {
        segments: row.segments.clone(),
        handoffs: row.handoffs.clone(),
        startup: row.startup.clone(),
    };
    let encoded = serde_json::to_string(&payload).map_err(failure)?;
    database.write(|transaction| {
        let active = (!row.bindings.is_empty()).then_some(&row.active);
        let bindings_changed = load_bindings(transaction, &row.id)? != row.bindings;
        let changed = transaction.execute(
            "INSERT INTO threads (provider, id, registered, cwd, name, preview, active_provider, active_id, payload_json, name_override, preview_override)
             VALUES (?1, ?2, 1, ?3, ?4, ?5, ?6, ?7, json_object('conversation', json(?8)), ?4 IS NOT NULL, ?5 IS NOT NULL)
             ON CONFLICT (provider, id) DO UPDATE SET registered = 1, cwd = excluded.cwd,
                 name = COALESCE(excluded.name, threads.name), preview = COALESCE(excluded.preview, threads.preview),
                 name_override = excluded.name_override, preview_override = excluded.preview_override,
                 active_provider = excluded.active_provider, active_id = excluded.active_id,
                 payload_json = json_set(threads.payload_json, '$.conversation', json(?8))
             WHERE registered = 0 OR cwd IS NOT excluded.cwd OR name_override != excluded.name_override
                 OR preview_override != excluded.preview_override OR (excluded.name_override AND name IS NOT excluded.name)
                 OR (excluded.preview_override AND preview IS NOT excluded.preview)
                 OR active_provider IS NOT excluded.active_provider OR active_id IS NOT excluded.active_id
                 OR json_extract(threads.payload_json, '$.conversation') IS NOT json(?8)",
            params![row.id.provider.key(), row.id.id, row.cwd, row.name, row.preview,
                active.map(|id| id.provider.key()), active.map(|id| &id.id), encoded],
        ).map_err(failure)?;
        if bindings_changed {
            transaction.execute(
                "DELETE FROM provider_threads WHERE thread_provider = ?1 AND thread_id = ?2",
                params![row.id.provider.key(), row.id.id],
            ).map_err(failure)?;
            for (ordinal, binding) in row.bindings.iter().enumerate() {
                transaction.execute(
                    "INSERT INTO provider_threads (provider, id, thread_provider, thread_id, ordinal, covered) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![binding.session.provider.key(), binding.session.id, row.id.provider.key(), row.id.id, ordinal as i64, binding.covered.map(|value| value as i64)],
                ).map_err(failure)?;
            }
        }
        if changed > 0 || bindings_changed {
            append_event(transaction, &row.id, "thread.registered", &serde_json::json!({
                "cwd": row.cwd, "name": row.name, "preview": row.preview, "active": row.active,
                "bindings": row.bindings, "context": payload,
            }))?;
        }
        Ok(())
    })
}
