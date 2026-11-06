use super::*;

impl Database {
    /// List revisions come from committed events, including after a Host restart.
    pub(in crate::host_rpc) fn update_metadata(
        &self,
        session: &SessionRef,
        cwd: Option<&str>,
        name: Option<&str>,
        preview: Option<&str>,
    ) -> Result<(u64, bool), String> {
        self.write(|transaction| {
            ensure_thread(transaction, session)?;
            let settled = transaction.query_row(
                "SELECT settled FROM threads WHERE provider = ?1 AND id = ?2",
                params![session.provider.key(), session.id],
                |row| row.get(0),
            ).map_err(failure)?;
            let sequence = append_event(transaction, session, "thread.updated", &serde_json::json!({"cwd":cwd,"name":name,"preview":preview}))?;
            transaction.execute(
                "UPDATE threads SET cwd = ?3,
                    name = CASE WHEN name_override THEN name ELSE ?4 END,
                    preview = CASE WHEN preview_override THEN preview ELSE ?5 END, list_revision = ?6
                 WHERE provider = ?1 AND id = ?2",
                params![session.provider.key(), session.id, cwd, name, preview, sequence as i64],
            ).map_err(failure)?;
            Ok((sequence, settled))
        })
    }

    pub(in crate::host_rpc) fn rename(
        &self,
        session: &SessionRef,
        name: Option<&str>,
    ) -> Result<u64, String> {
        self.write(|transaction| {
            ensure_thread(transaction, session)?;
            let sequence = append_event(transaction, session, "thread.renamed", &name)?;
            transaction.execute(
                "UPDATE threads SET name = ?3, list_revision = ?4 WHERE provider = ?1 AND id = ?2",
                params![session.provider.key(), session.id, name, sequence as i64],
            ).map_err(failure)?;
            advance(transaction, session.provider.key(), &session.id)?;
            Ok(sequence)
        })
    }
}

/// A Host-wide transcript version; list metadata has its own event sequence.
pub(in crate::host_rpc) fn advance(
    transaction: &Transaction,
    provider: &str,
    id: &str,
) -> Result<(), String> {
    transaction.execute(
        "INSERT INTO versions (provider, id, version) VALUES (?1, ?2, (SELECT COALESCE(MAX(version), 0) + 1 FROM versions))
         ON CONFLICT (provider, id) DO UPDATE SET version = excluded.version",
        params![provider, id],
    ).map_err(failure)?;
    Ok(())
}
