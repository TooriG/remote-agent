use super::*;
use crate::{
    models::{Thread, Turn, TurnStatus},
    session::SessionRef,
    state::{Attachment, Navigation, operations::OperationState},
};

fn session() -> SessionRef {
    SessionRef::new(ProviderKind::Codex, "chat".into()).unwrap()
}

fn snapshot() -> Snapshot {
    let mut snapshot = Snapshot::default();
    snapshot.connection.connected = true;
    Arc::make_mut(&mut snapshot.pane).navigation = Arc::new(Navigation {
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
    ComposerPresentation::from_snapshot(Arc::new(snapshot.clone()))
}

#[test]
fn submission_needs_content_or_recording_and_an_available_provider() {
    let mut snapshot = snapshot();
    assert!(!presentation(&snapshot).can_submit(false));
    assert!(presentation(&snapshot).can_submit(true));
    crate::state::set_draft_text(&mut snapshot, session().into(), " \n\t".into());
    assert!(!presentation(&snapshot).can_submit(false));
    crate::state::set_draft_text(&mut snapshot, session().into(), "日本語 👋".into());
    assert!(presentation(&snapshot).can_submit(false));

    let draft = Arc::make_mut(&mut snapshot.input.drafts)
        .get_mut(&session().into())
        .unwrap();
    let draft = Arc::make_mut(draft);
    draft.text.clear();
    draft.attachments.push(Attachment {
        path: "/uploaded/image.png".into(),
        name: "image.png".into(),
        is_image: true,
    });
    assert!(presentation(&snapshot).can_submit(false));
    snapshot.connection.connected = false;
    assert!(!presentation(&snapshot).can_submit(false));
    assert!(!presentation(&snapshot).can_submit(true));
    snapshot.connection.connected = true;
    Arc::make_mut(&mut snapshot.pane).navigation = Arc::default();
    assert!(
        !presentation(&snapshot).can_submit(true),
        "recording does not choose an unavailable provider"
    );
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
        snapshot.operations.insert(
            key.clone(),
            OperationState {
                generation: 1,
                phase: OperationPhase::Running,
            },
        );
        let busy = presentation(&snapshot);
        assert!(!busy.can_submit(false));
        assert!(!busy.can_submit(true));
        assert_eq!(
            busy.controls.transferring,
            matches!(key, OperationKey::Attachment { .. })
        );
        snapshot.operations.get_mut(&key).unwrap().phase = OperationPhase::Failed {
            message: "retry".into(),
        };
        assert!(presentation(&snapshot).can_submit(false));
        snapshot.operations.remove(&key);
        assert!(presentation(&snapshot).can_submit(false));
    }
    snapshot.operations.insert(
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
        presentation(&snapshot).can_submit(false),
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
    snapshot.operations.insert(
        key.clone(),
        OperationState {
            generation: 1,
            phase: OperationPhase::Running,
        },
    );
    assert!(!presentation(&snapshot).can_advance_question());
    snapshot.operations.get_mut(&key).unwrap().phase = OperationPhase::Failed {
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
    assert!(!presentation(&snapshot).can_submit(true));
    let key = OperationKey::Interrupt {
        session: session(),
        turn: "turn".into(),
    };
    snapshot.operations.insert(
        key.clone(),
        OperationState {
            generation: 1,
            phase: OperationPhase::Running,
        },
    );
    assert!(!presentation(&snapshot).can_interrupt());
    snapshot.operations.get_mut(&key).unwrap().phase = OperationPhase::Failed {
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
        Arc::make_mut(&mut Arc::make_mut(&mut snapshot.pane).workspace).review =
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
    assert_eq!(snapshot().composer_review(false), None);
    assert_eq!(with_files(0).composer_review(true), None);

    let three = with_files(3).composer_review(false).unwrap();
    assert_eq!((three.files, three.additions, three.deletions), (3, 7, 2));
    assert_eq!(three.rows.len(), 3);
    assert_eq!(three.folded, None, "three files need no toggle");

    let four = with_files(4);
    let folded = four.composer_review(false).unwrap();
    assert_eq!(
        paths(&folded),
        ["src/日本語-0.rs", "src/日本語-1.rs", "src/日本語-2.rs"]
    );
    assert_eq!(folded.folded, Some(1));
    let expanded = four.composer_review(true).unwrap();
    assert_eq!(expanded.rows.len(), 4);
    assert_eq!(
        expanded.folded,
        Some(1),
        "the toggle stays to fold it again"
    );

    let mut draft = four;
    Arc::make_mut(&mut draft.pane).navigation = Arc::new(Navigation {
        thread_id: None,
        draft_key: "draft".into(),
        cwd: "/project".into(),
    });
    assert_eq!(
        draft.composer_review(false),
        None,
        "a new draft has no card"
    );
}

#[test]
fn an_answered_turn_offers_its_background_work_and_stop_while_the_composer_sends() {
    let mut snapshot = snapshot();
    let waiting = |background| {
        Arc::new(Thread {
            id: Some(session()),
            turns: Some(vec![Arc::new(Turn {
                id: "turn".into(),
                status: TurnStatus::Completed,
                ..Default::default()
            })]),
            background,
            ..Default::default()
        })
    };
    Arc::make_mut(&mut snapshot.sessions.conversations).insert(session(), waiting(Vec::new()));
    assert_eq!(presentation(&snapshot).controls.background, None);
    Arc::make_mut(&mut snapshot.sessions.conversations).insert(
        session(),
        waiting(vec![agent_protocol::execution::BackgroundTask {
            id: "shell".into(),
            kind: agent_protocol::execution::BackgroundTaskKind::Monitor,
            description: Some("Watch the build".into()),
        }]),
    );
    crate::state::set_draft_text(&mut snapshot, session().into(), "next".into());
    let composer = presentation(&snapshot);
    let work = composer.controls.background.as_ref().unwrap();
    assert!(work.waiting);
    assert_eq!(
        work.title(crate::localization::UiLanguage::English),
        "Waiting on monitor Watch the build"
    );
    assert!(composer.can_interrupt(), "Stop ends the work");
    assert_eq!(composer.controls.running_turn, Some("turn".into()));
    assert!(composer.can_submit(false), "a message can follow the work");
}
