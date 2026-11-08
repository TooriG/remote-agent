use super::*;
use crate::host_rpc::service::conversations::tests::{NativeAgent, call};
use agent_protocol::{
    execution::TurnStatus,
    models::{Thread, Turn},
    protocol::Call,
    session::{HistoryPage, OpenSession, OpenedSession, ReadHistory},
};

#[tokio::test]
async fn first_older_page_starts_before_the_initial_window_without_overlap() {
    let root = tempfile::tempdir().unwrap();
    let native = NativeAgent::new(ProviderKind::Codex, root.path().join("native"));
    let target = SessionRef::new(ProviderKind::Codex, "thirty-six-turns".into()).unwrap();
    native.records.lock().unwrap().insert(
        target.id.clone(),
        ThreadResponse {
            thread: Thread {
                id: Some(target.clone()),
                turns: Some(
                    (0..36)
                        .map(|index| {
                            Arc::new(Turn {
                                id: format!("turn-{index}").into(),
                                status: TurnStatus::Completed,
                                items: Some(Vec::new()),
                                ..Default::default()
                            })
                        })
                        .collect(),
                ),
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let service = HostRpcService::new(
        [native.backend()],
        ProjectStore::new(root.path().join("worktrees.json")),
    );
    let session = service.open_session();
    let opened: OpenedSession = call(
        &service,
        session.id(),
        Call::OpenSession(OpenSession {
            session: target.clone(),
            limit: 10,
            include_activity: false,
            known_version: None,
        }),
    )
    .await
    .unwrap();
    let mut thread = opened.response.thread;
    let ids = |thread: &Thread| {
        thread
            .turns
            .as_ref()
            .unwrap()
            .iter()
            .map(|turn| turn.id.to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        ids(&thread),
        (26..36)
            .map(|index| format!("turn-{index}"))
            .collect::<Vec<_>>()
    );
    // Opening imports native history first. Its continuation names the oldest
    // displayed transcript position, not a provider's native five-turn cursor.
    assert_eq!(thread.history_cursor.as_deref(), Some("t26"));
    let older: HistoryPage = call(
        &service,
        session.id(),
        Call::ReadHistory(ReadHistory {
            session: target,
            cursor: thread.history_cursor.clone().unwrap(),
            include_activity: false,
        }),
    )
    .await
    .unwrap();
    assert_eq!(older.turns.len(), WINDOW_TURNS);
    assert_eq!(older.next_cursor.as_deref(), Some("t16"));
    older.prepend_to(&mut thread);
    assert_eq!(
        ids(&thread),
        (16..36)
            .map(|index| format!("turn-{index}"))
            .collect::<Vec<_>>()
    );
}
