use super::*;
use crate::{
    models::{Thread, Turn, TurnStatus},
    session::SessionRef,
    state::{Navigation, operations::OperationState},
};

fn session() -> SessionRef {
    SessionRef::new(ProviderKind::Codex, "chat".into()).unwrap()
}

fn snapshot() -> Snapshot {
    let mut snapshot = Snapshot::default();
    snapshot.connection.connected = true;
    Arc::make_mut(snapshot.panes.primary_mut()).navigation = Arc::new(Navigation {
        thread_id: Some(session()),
        draft_key: session().into(),
        cwd: "/project".into(),
    });
    Arc::make_mut(&mut snapshot.sessions.conversations).insert(
        session(),
        Arc::new(Thread {
            id: Some(session()),
            ..Default::default()
        }),
    );
    snapshot
}

fn presentation(snapshot: &Snapshot) -> Arc<ComposerPresentation> {
    ComposerPresentation::project(snapshot, snapshot.panes.primary(), None)
}

#[test]
fn a_conversation_still_opening_or_failed_to_open_keeps_its_draft_but_sends_nothing() {
    let mut snapshot = snapshot();
    crate::state::set_draft_text(&mut snapshot, session().into(), "下書き".into());
    assert!(presentation(&snapshot).controls.loaded);
    assert!(presentation(&snapshot).can_send(true));

    Arc::make_mut(&mut snapshot.sessions.conversations).remove(&session());
    let opening = presentation(&snapshot);
    assert!(!opening.controls.loaded);
    assert_eq!(opening.draft.text, "下書き");
    assert!(!opening.can_send(true));
    assert!(!opening.can_attach(), "nor an attachment");

    // A new chat has no conversation to wait for.
    Arc::make_mut(snapshot.panes.primary_mut()).navigation = Arc::new(Navigation {
        thread_id: None,
        draft_key: session().into(),
        cwd: "/project".into(),
    });
    assert!(presentation(&snapshot).controls.loaded);
}

#[test]
fn another_agent_waits_for_the_working_conversation_and_says_why() {
    let mut snapshot = snapshot();
    crate::state::set_draft_text(&mut snapshot, session().into(), "切り替えて送ります".into());
    Arc::make_mut(
        Arc::make_mut(&mut snapshot.input.drafts)
            .get_mut(&session().into())
            .unwrap(),
    )
    .model = Some(ModelRef {
        provider: ProviderKind::Claude,
        id: "sonnet".into(),
    });
    let status = |snapshot: &mut Snapshot, status| {
        Arc::make_mut(
            Arc::make_mut(&mut snapshot.sessions.conversations)
                .get_mut(&session())
                .unwrap(),
        )
        .status = status;
    };
    status(&mut snapshot, crate::models::SessionStatus::Running);
    let held = presentation(&snapshot);
    assert!(!held.can_send(true));
    assert!(
        held.controls
            .unavailable
            .as_deref()
            .is_some_and(|reason| reason.starts_with("AIを切り替えて送信するには")),
        "a held send names its reason"
    );
    status(&mut snapshot, crate::models::SessionStatus::Idle);
    let ready = presentation(&snapshot);
    assert!(ready.can_send(true));
    assert_eq!(ready.controls.unavailable, None);
}

#[test]
fn active_draft_operations_block_submission_until_completion_or_retry() {
    let mut snapshot = snapshot();
    crate::state::set_draft_text(&mut snapshot, session().into(), "送信".into());
    for key in [
        OperationKey::Submission {
            draft_key: session().into(),
        },
        OperationKey::Attachment {
            draft_key: session().into(),
        },
        OperationKey::Dictation {
            draft_key: session().into(),
        },
    ] {
        Arc::make_mut(&mut snapshot.operations).insert(
            key.clone(),
            OperationState {
                generation: 1,
                phase: OperationPhase::Running,
            },
        );
        let busy = presentation(&snapshot);
        assert!(!busy.can_send(true));
        assert!(!busy.can_attach() && !busy.can_dictate());
        assert_eq!(
            busy.controls.transferring,
            matches!(key, OperationKey::Attachment { .. })
        );
        Arc::make_mut(&mut snapshot.operations)
            .get_mut(&key)
            .unwrap()
            .phase = OperationPhase::Failed {
            message: "retry".into(),
        };
        assert!(presentation(&snapshot).can_send(true));
        Arc::make_mut(&mut snapshot.operations).remove(&key);
        assert!(presentation(&snapshot).can_send(true));
    }
    Arc::make_mut(&mut snapshot.operations).insert(
        OperationKey::Attachment {
            draft_key: SessionRef::new(ProviderKind::Codex, "other".into())
                .unwrap()
                .into(),
        },
        OperationState {
            generation: 1,
            phase: OperationPhase::Running,
        },
    );
    assert!(
        presentation(&snapshot).can_send(true),
        "another conversation's transfer does not block this draft"
    );
}

#[test]
fn answers_cannot_be_advanced_twice_while_the_response_is_in_flight() {
    use agent_protocol::{
        requests::{Question, Request, RequestBody, RequestTarget},
        session::RequestDelivery,
    };
    let mut snapshot = snapshot();
    Arc::make_mut(
        Arc::make_mut(&mut snapshot.sessions.conversations)
            .get_mut(&session())
            .unwrap(),
    )
    .requests
    .insert(
        "question".into(),
        Arc::new(Request {
            id: "question".into(),
            target: RequestTarget::Session,
            delivery: RequestDelivery::Awaiting,
            body: RequestBody::Question {
                questions: vec![Question {
                    id: "answer".into(),
                    header: "質問".into(),
                    prompt: "何を作る？".into(),
                    secret: false,
                    allow_free_text: true,
                    multiple: false,
                    choices: Vec::new(),
                }],
            },
        }),
    );
    assert!(!presentation(&snapshot).can_advance_question());
    crate::questions::act(
        &mut snapshot,
        "question".into(),
        crate::questions::QuestionAction::Edit {
            text: "iPhoneアプリ".into(),
        },
    );
    assert!(presentation(&snapshot).can_advance_question());
    let key = OperationKey::Request {
        id: "question".into(),
    };
    Arc::make_mut(&mut snapshot.operations).insert(
        key.clone(),
        OperationState {
            generation: 1,
            phase: OperationPhase::Running,
        },
    );
    assert!(!presentation(&snapshot).can_advance_question());
    Arc::make_mut(&mut snapshot.operations)
        .get_mut(&key)
        .unwrap()
        .phase = OperationPhase::Failed {
        message: "retry".into(),
    };
    assert!(presentation(&snapshot).can_advance_question());
    snapshot.connection.connected = false;
    assert!(!presentation(&snapshot).can_advance_question());
    assert!(!presentation(&Snapshot::default()).can_advance_question());
}

#[test]
fn stopping_retries_failure_and_does_not_depend_on_message_input_availability() {
    let mut snapshot = snapshot();
    assert!(!presentation(&snapshot).can_interrupt());
    let thread = Arc::make_mut(
        Arc::make_mut(&mut snapshot.sessions.conversations)
            .get_mut(&session())
            .unwrap(),
    );
    thread.can_accept_direct_input = Some(false);
    thread.turns = Some(vec![Arc::new(Turn {
        id: "turn".into(),
        status: TurnStatus::Running,
        ..Default::default()
    })]);
    assert!(presentation(&snapshot).can_interrupt());
    assert!(!presentation(&snapshot).can_send(true));
    let key = OperationKey::Interrupt {
        session: session(),
        turn: "turn".into(),
    };
    Arc::make_mut(&mut snapshot.operations).insert(
        key.clone(),
        OperationState {
            generation: 1,
            phase: OperationPhase::Running,
        },
    );
    assert!(!presentation(&snapshot).can_interrupt());
    Arc::make_mut(&mut snapshot.operations)
        .get_mut(&key)
        .unwrap()
        .phase = OperationPhase::Failed {
        message: "retry".into(),
    };
    assert!(presentation(&snapshot).can_interrupt());
    snapshot.connection.connected = false;
    assert!(!presentation(&snapshot).can_interrupt());
}

#[test]
fn the_review_card_folds_past_three_files_and_needs_a_conversation_with_changes() {
    use agent_protocol::models::{ChangedFile, WorkspaceReview};
    let with_files = |count: usize| {
        let mut snapshot = snapshot();
        Arc::make_mut(&mut Arc::make_mut(snapshot.panes.primary_mut()).workspace).review =
            Some(Arc::new(WorkspaceReview {
                branch: "main".into(),
                additions: 7,
                deletions: 2,
                files: (0..count)
                    .map(|index| ChangedFile {
                        path: format!("src/日本語-{index}.rs"),
                        status: "M".into(),
                        additions: Some(1),
                        deletions: None,
                    })
                    .collect(),
                diff: String::new(),
            }));
        snapshot
    };
    let paths = |review: &ComposerReview| {
        review
            .rows
            .iter()
            .map(|file| file.path.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(snapshot().panes.primary().composer_review(false), None);
    assert_eq!(with_files(0).panes.primary().composer_review(true), None);

    let three = with_files(3)
        .panes
        .primary()
        .composer_review(false)
        .unwrap();
    assert_eq!((three.files, three.additions, three.deletions), (3, 7, 2));
    assert_eq!(three.rows.len(), 3);
    assert_eq!(three.folded, None, "three files need no toggle");

    let four = with_files(4);
    let folded = four.panes.primary().composer_review(false).unwrap();
    assert_eq!(
        paths(&folded),
        ["src/日本語-0.rs", "src/日本語-1.rs", "src/日本語-2.rs"]
    );
    assert_eq!(folded.folded, Some(1));
    let expanded = four.panes.primary().composer_review(true).unwrap();
    assert_eq!(expanded.rows.len(), 4);
    assert_eq!(
        expanded.folded,
        Some(1),
        "the toggle stays to fold it again"
    );

    let mut draft = four;
    Arc::make_mut(draft.panes.primary_mut()).navigation = Arc::new(Navigation {
        thread_id: None,
        draft_key: "draft".into(),
        cwd: "/project".into(),
    });
    assert_eq!(
        draft.panes.primary().composer_review(false),
        None,
        "a new draft has no card"
    );
}

#[test]
fn the_review_action_is_named_from_a_24rem_card() {
    assert!(super::review_action_labelled(384.));
    assert!(super::review_action_labelled(720.));
    assert!(!super::review_action_labelled(383.9));
    assert!(!super::review_action_labelled(0.));
}
