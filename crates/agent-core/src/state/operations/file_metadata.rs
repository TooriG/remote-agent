use super::*;
use agent_protocol::file_metadata::{FileMetadataEntries, METADATA_BATCH_LIMIT, ReadFileMetadata};

pub(crate) fn ensure_file_metadata(
    snapshot: &mut Snapshot,
    paths: Vec<String>,
    now_ms: u64,
) -> Vec<Effect> {
    if !snapshot.connected {
        return Vec::new();
    }
    let (generation, paths) = Arc::make_mut(&mut snapshot.workspace)
        .file_metadata
        .ensure(paths, now_ms);
    paths
        .chunks(METADATA_BATCH_LIMIT)
        .map(|paths| {
            Effect::execute(ReadMetadata {
                paths: paths.to_vec(),
                generation,
                connection: snapshot.connection_generation,
                requested_at: now_ms,
            })
        })
        .collect()
}

#[derive(Debug)]
struct ReadMetadata {
    paths: Vec<String>,
    generation: u64,
    connection: u64,
    requested_at: u64,
}
impl Operation for ReadMetadata {
    no_input!();
    type Output = (u64, Option<FileMetadataEntries>);
    const BACKGROUND: bool = true;
    const STALE_POLICY: StalePolicy = StalePolicy::Apply;
    async fn run(&self, _: (), context: &mut Execution<'_>) -> Result<Self::Output, PeerError> {
        let output = context
            .call(&ReadFileMetadata {
                paths: self.paths.clone(),
            })
            .await
            .ok();
        let checked_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        Ok((checked_at, output))
    }
    fn rejected_output(&self, _: ()) -> Option<Self::Output> {
        Some((self.requested_at, None))
    }
    fn apply(self, snapshot: &mut Snapshot, (checked_at, output): Self::Output) -> Vec<Effect> {
        if snapshot.connected && snapshot.connection_generation == self.connection {
            Arc::make_mut(&mut snapshot.workspace)
                .file_metadata
                .received(
                    self.generation,
                    self.paths,
                    output.map(|output| output.entries),
                    checked_at,
                );
        }
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn automatic_metadata_work_is_batched_and_disconnected_clients_issue_no_reads() {
        let mut snapshot = Snapshot::default();
        let paths: Vec<_> = (0..130).map(|n| format!("/a/{n}")).collect();
        assert!(ensure_file_metadata(&mut snapshot, paths.clone(), 0).is_empty());
        snapshot.connected = true;
        let effects = ensure_file_metadata(&mut snapshot, paths.clone(), 0);
        assert_eq!(effects.len(), 3);
        assert!(ensure_file_metadata(&mut snapshot, paths, 1).is_empty());
    }

    #[test]
    fn stale_navigation_can_complete_but_a_previous_connection_cannot_publish_metadata() {
        let mut snapshot = Snapshot {
            connected: true,
            connection_generation: 2,
            ..Default::default()
        };
        let paths = vec!["/a".into()];
        let (generation, _) = Arc::make_mut(&mut snapshot.workspace)
            .file_metadata
            .ensure(paths.clone(), 0);
        let request = |connection| ReadMetadata {
            paths: paths.clone(),
            generation,
            connection,
            requested_at: 0,
        };
        request(1).stale(&mut snapshot, (1, None));
        assert!(snapshot.workspace.file_metadata.get("/a").is_none());
        request(2).stale(&mut snapshot, (2, None));
        assert!(snapshot.workspace.file_metadata.get("/a").is_some());
    }
}
