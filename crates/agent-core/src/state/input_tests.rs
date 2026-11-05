use super::*;

#[rstest::rstest]
#[case("", "音声🎤", "音声🎤")]
#[case("入力", "音声🎤", "入力\n音声🎤")]
#[case("入力\n", "音声🎤", "入力\n音声🎤")]
#[case("入力 ", "音声🎤", "入力 音声🎤")]
#[case("入力", "", "入力")]
fn transcript_appends_to_the_latest_target_without_changing_selection(
    #[case] existing: &str,
    #[case] spoken: &str,
    #[case] expected: &str,
) {
    let target = DraftKey::from("recorded");
    let selected = DraftKey::from("selected");
    let mut previous = Snapshot::default();
    set_draft_text(&mut previous, target.clone(), existing.into());
    set_draft_text(&mut previous, selected.clone(), "keep this draft".into());
    Arc::make_mut(&mut Arc::make_mut(&mut previous.pane).navigation).draft_key = selected.clone();
    let (next, effects) = reduce(
        &previous,
        Event::Intent(Intent::AppendTranscript {
            thread_id: target.clone(),
            text: spoken.into(),
        }),
    );
    assert_eq!(
        next.input
            .drafts
            .get(&target)
            .map(|draft| draft.text.as_str())
            .unwrap_or_default(),
        expected
    );
    assert_eq!(
        previous
            .input
            .drafts
            .get(&target)
            .map(|draft| draft.text.as_str())
            .unwrap_or_default(),
        existing
    );
    assert!(Arc::ptr_eq(
        &next.pane.navigation,
        &previous.pane.navigation
    ));
    assert!(Arc::ptr_eq(
        &next.input.drafts[&selected],
        &previous.input.drafts[&selected]
    ));
    assert!(effects.is_empty());
}

#[test]
fn transcript_uses_the_utf8_end_when_refreshing_completions() {
    let mut snapshot = Snapshot::default();
    snapshot.connection.connected = true;
    let key = DraftKey::from("draft");
    Arc::make_mut(&mut snapshot.pane).navigation = Arc::new(Navigation {
        draft_key: key.clone(),
        cwd: "/code".into(),
        ..Default::default()
    });
    Arc::make_mut(&mut snapshot.catalog.composer_catalogs).insert(
        "/code".into(),
        Arc::new(agent_protocol::composer::ComposerCatalog {
            cwd: "/code".into(),
            loading: false,
            candidates: Vec::new(),
            errors: Default::default(),
        }),
    );
    set_draft_text(&mut snapshot, key.clone(), "日本語".into());
    let (next, effects) = reduce(
        &snapshot,
        Event::Intent(Intent::AppendTranscript {
            thread_id: key.clone(),
            text: "/".into(),
        }),
    );
    assert_eq!(next.input.drafts[&key].text, "日本語\n/");
    assert!(
        next.catalog
            .composer_catalogs
            .get(&next.pane.navigation.cwd)
            .unwrap()
            .loading
    );
    assert_eq!(effects.len(), 1);
}

#[test]
fn a_recovery_click_cannot_retry_a_replaced_operation() {
    let mut snapshot = Snapshot::default();
    snapshot.operations.insert(
        op::OperationKey::Models {
            provider: crate::session::ProviderKind::Codex,
        },
        op::OperationState {
            generation: 9,
            phase: op::OperationPhase::Failed {
                message: "offline".into(),
            },
        },
    );
    let recover = |generation| {
        Event::Intent(Intent::RecoverError {
            id: op::ErrorId::Operation {
                key: op::OperationKey::Models {
                    provider: crate::session::ProviderKind::Codex,
                },
                generation,
            },
        })
    };
    let (stale, effects) = reduce(&snapshot, recover(8));
    assert!(effects.is_empty());
    assert!(stale.operations.same(&snapshot.operations));
    let (_, effects) = reduce(&snapshot, recover(9));
    assert_eq!(effects.len(), 1);
}
