use super::*;
use crate::{
    models::{Item, ItemBody, ItemStatus, Turn},
    session::{ProviderKind, SessionRef, SubmissionDelivery},
};

fn pending(draft_key: DraftKey) -> Arc<PendingSubmission> {
    Arc::new(PendingSubmission {
        sequence: 0,
        submitted_at_ms: None,
        draft_key,
        draft: Arc::default(),
        turn_id: None,
        after_item_id: None,
        accepted: false,
        delivery_unknown: true,
        queued: false,
    })
}

fn echo(native: &str, client: Option<&str>, user: bool) -> Arc<Item> {
    let mut item = Item::new(
        native.into(),
        ItemStatus::Unknown,
        if user {
            ItemBody::UserMessage {
                text: None,
                content: vec![],
            }
        } else {
            ItemBody::AssistantText {
                text: String::new(),
                phase: Default::default(),
            }
        },
    );
    item.client_input_id = client.map(Into::into);
    Arc::new(item)
}

#[test]
fn native_echoes_remove_only_inputs_owned_by_the_reconciled_session() {
    let session = SessionRef::new(ProviderKind::Codex, "shared".into()).unwrap();
    let other = SessionRef::new(ProviderKind::Claude, "shared".into()).unwrap();
    let own = DraftKey::from(&session);
    let mut state = Snapshot {
        sessions: crate::state::Sessions {
            conversations: Arc::new(
                [(
                    session.clone(),
                    Arc::new(Thread {
                        turns: Some(vec![
                            Arc::new(Turn {
                                id: "reused".into(),
                                items: Some(vec![
                                    echo("first", Some("one"), true),
                                    echo("duplicate", Some("one"), true),
                                    echo("assistant", Some("not-user"), false),
                                ]),
                                ..Default::default()
                            }),
                            Arc::new(Turn {
                                id: "reused".into(),
                                items: Some(vec![
                                    echo("second", Some("two"), true),
                                    echo("foreign", Some("other"), true),
                                    echo("local", Some("local"), true),
                                    echo("untracked", Some("untracked"), true),
                                    echo("without-id", None, true),
                                ]),
                                ..Default::default()
                            }),
                        ]),
                        ..Default::default()
                    }),
                )]
                .into(),
            ),
            pending_submissions: Arc::new(
                [
                    ("one".into(), pending(own.clone())),
                    ("two".into(), pending(own.clone())),
                    ("not-user".into(), pending(own.clone())),
                    ("no-echo".into(), pending(own)),
                    ("other".into(), pending(other.into())),
                    ("local".into(), pending("shared".into())),
                ]
                .into(),
            ),
            ..Default::default()
        },
        ..Default::default()
    };
    let previous = state.sessions.pending_submissions.clone();
    reconcile_pending(&mut state, &session);
    assert_eq!(state.sessions.pending_submissions.len(), 4);
    for id in ["not-user", "no-echo", "other", "local"] {
        assert!(Arc::ptr_eq(
            &state.sessions.pending_submissions[id],
            &previous[id]
        ));
    }
    assert_eq!(previous.len(), 6);
}

#[test]
fn pending_snapshot_is_shared_until_its_own_native_echo_arrives() {
    let session = SessionRef::new(ProviderKind::Codex, "task".into()).unwrap();
    let mut state = Snapshot {
        sessions: crate::state::Sessions {
            pending_submissions: Arc::new(
                [("pending".into(), pending(session.clone().into()))].into(),
            ),
            ..Default::default()
        },
        ..Default::default()
    };
    let previous = state.sessions.pending_submissions.clone();
    reconcile_pending(&mut state, &session);
    assert!(Arc::ptr_eq(&state.sessions.pending_submissions, &previous));
    Arc::make_mut(&mut state.sessions.conversations).insert(session.clone(), Arc::default());
    reconcile_pending(&mut state, &session);
    assert!(Arc::ptr_eq(&state.sessions.pending_submissions, &previous));
    let other = SessionRef::new(ProviderKind::Claude, "task".into()).unwrap();
    Arc::make_mut(&mut state.sessions.conversations).insert(
        other.clone(),
        Arc::new(Thread {
            turns: Some(vec![Arc::new(Turn {
                items: Some(vec![echo("foreign", Some("pending"), true)]),
                ..Default::default()
            })]),
            ..Default::default()
        }),
    );
    reconcile_pending(&mut state, &other);
    assert!(Arc::ptr_eq(&state.sessions.pending_submissions, &previous));
    Arc::make_mut(
        Arc::make_mut(&mut state.sessions.conversations)
            .get_mut(&session)
            .unwrap(),
    )
    .turns = Some(vec![Arc::new(Turn {
        items: Some(vec![
            echo("untracked", Some("untracked"), true),
            echo("confirmed", Some("pending"), true),
            echo("duplicate", Some("pending"), true),
        ]),
        ..Default::default()
    })]);
    reconcile_pending(&mut state, &session);
    assert!(state.sessions.pending_submissions.is_empty());
    assert_eq!(previous.len(), 1);
}

#[test]
fn delivery_receipts_are_scoped_before_mutating_pending_input() {
    let session = SessionRef::new(ProviderKind::Codex, "task".into()).unwrap();
    let other = SessionRef::new(ProviderKind::Claude, "task".into()).unwrap();
    let mut owned = pending(session.clone().into());
    Arc::make_mut(&mut owned).turn_id = Some("previous".into());
    Arc::make_mut(&mut owned).after_item_id = Some("anchor".into());
    let mut state = Snapshot {
        sessions: crate::state::Sessions {
            conversations: Arc::new(
                [(
                    session.clone(),
                    Arc::new(Thread {
                        submissions: [
                            (
                                "owned".into(),
                                SubmissionDelivery::Accepted {
                                    turn_id: Some("native".into()),
                                },
                            ),
                            ("foreign".into(), SubmissionDelivery::Rejected),
                        ]
                        .into(),
                        ..Default::default()
                    }),
                )]
                .into(),
            ),
            pending_submissions: Arc::new(
                [
                    ("owned".into(), owned),
                    ("foreign".into(), pending(other.into())),
                ]
                .into(),
            ),
            ..Default::default()
        },
        ..Default::default()
    };
    let previous = state.sessions.pending_submissions.clone();
    reconcile_pending(&mut state, &session);
    let owned = &state.sessions.pending_submissions["owned"];
    assert!(owned.accepted && !owned.delivery_unknown);
    assert_eq!(owned.turn_id.as_deref(), Some("native"));
    assert!(owned.after_item_id.is_none());
    assert!(!previous["owned"].accepted);
    assert!(Arc::ptr_eq(
        &state.sessions.pending_submissions["foreign"],
        &previous["foreign"]
    ));
}
