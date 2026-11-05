use super::*;
use crate::{
    session::{ProviderKind, SessionRef},
    state::{
        DraftKey, Intent, Navigation,
        operations::ReadThread,
        projects::{PlacedChat, ProjectPreferences},
    },
    store::Store,
};

fn session(id: &str) -> SessionRef {
    SessionRef::new(ProviderKind::Codex, id.into()).unwrap()
}

fn snapshot() -> Snapshot {
    let mut state = Snapshot::default();
    Arc::make_mut(&mut state.pane).navigation = Arc::new(Navigation {
        thread_id: Some(session("a")),
        draft_key: session("a").into(),
        cwd: "/a".into(),
    });
    state.sessions.conversations = Arc::new(
        ["a", "b"]
            .map(|id| {
                let id = session(id);
                (
                    id.clone(),
                    Arc::new(crate::models::Thread {
                        id: Some(id),
                        name: Some("Title".into()),
                        ..Default::default()
                    }),
                )
            })
            .into(),
    );
    state.lists.threads = Some(Arc::new(
        serde_json::from_value(serde_json::json!({
            "data":[],"projects":[{"id":"project","name":"Project","roots":[]}],
            "hasMore":false,"hasMoreProjects":false,"projectPages":{},"limit":5
        }))
        .unwrap(),
    ));
    state.lists.expanded_projects = Arc::new([("project".into(), 5)].into());
    state.lists.project_preferences = Arc::new(
        [(
            "".into(),
            [(
                "project".into(),
                ProjectPreferences {
                    placed_chats: vec![PlacedChat {
                        id: session("a"),
                        title: "Saved title".into(),
                    }],
                    ..Default::default()
                },
            )]
            .into(),
        )]
        .into(),
    );
    state
}

#[test]
fn body_deltas_reuse_the_list_but_shortcut_metadata_updates_it() {
    let initial = Presentation::project(Arc::new(snapshot()), None);
    let mut state = initial.snapshot.as_ref().clone();
    Arc::make_mut(
        Arc::make_mut(&mut state.sessions.conversations)
            .get_mut(&session("a"))
            .unwrap(),
    )
    .turns = Some(Vec::new());
    let streamed = Presentation::project(Arc::new(state), Some(&initial));
    assert!(Arc::ptr_eq(&streamed.composer, &initial.composer));
    assert!(Arc::ptr_eq(&streamed.context, &initial.context));
    assert!(Arc::ptr_eq(
        streamed.list.as_ref().unwrap(),
        initial.list.as_ref().unwrap()
    ));
    assert!(!Arc::ptr_eq(
        streamed.conversation.as_ref().unwrap(),
        initial.conversation.as_ref().unwrap()
    ));
    let mut state = streamed.snapshot.as_ref().clone();
    Arc::make_mut(
        Arc::make_mut(&mut state.sessions.conversations)
            .get_mut(&session("a"))
            .unwrap(),
    )
    .name = Some("Renamed".into());
    let renamed = Presentation::project(Arc::new(state), Some(&streamed));
    assert!(!Arc::ptr_eq(
        renamed.list.as_ref().unwrap(),
        streamed.list.as_ref().unwrap()
    ));
    assert_eq!(
        renamed.list.as_ref().unwrap().projects[0].threads[0].title,
        "Renamed"
    );
    assert_eq!(
        initial.list.as_ref().unwrap().projects[0].threads[0].title,
        "Title"
    );
}

#[test]
fn only_the_selected_draft_changes_its_composer() {
    let initial = Presentation::project(Arc::new(snapshot()), None);
    let mut state = initial.snapshot.as_ref().clone();
    crate::state::set_draft_text(&mut state, session("b").into(), "other pane".into());
    let other = Presentation::project(Arc::new(state.clone()), Some(&initial));
    assert!(Arc::ptr_eq(&other.composer, &initial.composer));
    assert!(Arc::ptr_eq(&other.context, &initial.context));
    crate::state::set_draft_text(&mut state, session("a").into(), "this pane".into());
    let selected = Presentation::project(Arc::new(state), Some(&other));
    assert!(!Arc::ptr_eq(&selected.composer, &other.composer));
    assert!(Arc::ptr_eq(&selected.context, &other.context));
    assert!(Arc::ptr_eq(
        selected.conversation.as_ref().unwrap(),
        other.conversation.as_ref().unwrap()
    ));
    assert_eq!(selected.composer.draft.text, "this pane");
}

#[test]
fn rebasing_retains_a_stream_prefix_and_drops_another_selection_or_host() {
    let initial = Presentation::project(Arc::new(snapshot()), None);
    let mut state = initial.snapshot.as_ref().clone();
    crate::state::set_draft_text(&mut state, session("a").into(), "typed".into());
    let current = Presentation::current(Arc::new(state.clone()), &initial);
    assert_eq!(
        current.snapshot.input.drafts[&DraftKey::from(session("a"))].text,
        "typed"
    );
    assert!(Arc::ptr_eq(
        current.conversation.as_ref().unwrap(),
        initial.conversation.as_ref().unwrap()
    ));
    Arc::make_mut(&mut Arc::make_mut(&mut state.pane).navigation).thread_id = Some(session("b"));
    assert!(
        Presentation::current(Arc::new(state.clone()), &initial)
            .conversation
            .is_none()
    );
    assert!(!initial.same_context(&state));
    state = initial.snapshot.as_ref().clone();
    state.connection.storage_scope = "another host".into();
    let replaced = Presentation::current(Arc::new(state), &initial);
    assert!(replaced.list.is_none() && replaced.conversation.is_none());
}

async fn presented(store: &Store, id: &SessionRef) -> Arc<Presentation> {
    let mut updates = store.subscribe_presentation();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let value = updates.borrow_and_update().clone();
            if value
                .conversation
                .as_ref()
                .is_some_and(|conversation| conversation.source.id.as_ref() == Some(id))
            {
                return value;
            }
            updates.changed().await.unwrap();
        }
    })
    .await
    .expect("presentation did not follow navigation")
}

#[tokio::test]
async fn the_native_stream_keeps_acknowledgments_and_selected_history_consistent() {
    let store = Store::offline(snapshot());
    let initial = presented(&store, &session("a")).await;
    store
        .dispatch(Intent::SetDraftText {
            thread_id: session("a").into(),
            text: "latest draft".into(),
        })
        .await
        .unwrap();
    let acknowledged = store.presentation();
    assert_eq!(
        acknowledged.snapshot.input.drafts[&DraftKey::from(session("a"))].text,
        "latest draft"
    );
    assert!(Arc::ptr_eq(
        acknowledged.conversation.as_ref().unwrap(),
        initial.conversation.as_ref().unwrap()
    ));
    assert!(
        store
            .dispatch(Intent::ReadThread(ReadThread::open(session("b"))))
            .await
            .is_err()
    );
    let immediate = store.presentation();
    assert_eq!(
        immediate.snapshot.pane.navigation.thread_id,
        Some(session("b"))
    );
    assert!(
        immediate
            .conversation
            .as_ref()
            .is_none_or(|conversation| conversation.source.id == Some(session("b")))
    );
    let selected = presented(&store, &session("b")).await;
    assert_eq!(
        selected.snapshot.pane.navigation.thread_id,
        Some(session("b"))
    );
    assert_eq!(
        selected.snapshot.input.drafts[&DraftKey::from(session("a"))].text,
        "latest draft"
    );
    store.close().await.unwrap();
}
