use super::*;
use crate::{models::Thread, presentation::conversation::project_conversation, state::DraftKey};
use agent_protocol::{
    execution::TurnStatus,
    requests::{Request, RequestBody, RequestTarget},
    session::RequestDelivery,
};
use serde_json::json;

fn scope() -> PanelScope {
    PanelScope {
        host: "host".into(),
        storage: "account".into(),
        draft: DraftKey::from("draft"),
    }
}

fn source(answer: bool) -> Arc<Thread> {
    let mut items = vec![
        json!({"id":"command", "status":"running", "body":{"inline":{"body":{
            "commandExecution":{"command":"pwd", "cwd":null, "output":"first output", "exitCode":null}
        }}}}),
    ];
    if answer {
        items.push(json!({"id":"answer", "body":{"inline":{"body":{
            "assistantText":{"text":"answer", "phase":"unknown"}
        }}}}));
    }
    Arc::new(
        serde_json::from_value(json!({
            "id":{"provider":"codex", "id":"thread"}, "capabilities":{"fork":true},
            "turns":[{"id":"turn", "status":"running", "startedAtMs":1000, "items":items}]
        }))
        .unwrap(),
    )
}

fn render(
    source: Arc<Thread>,
    previous: Option<Arc<RenderedConversation>>,
) -> Arc<RenderedConversation> {
    project_conversation(
        crate::localization::UiLanguage::Japanese,
        &scope().draft,
        &Arc::default(),
        source,
        &previous,
    )
}

fn project(
    source: Arc<RenderedConversation>,
    policy: ConversationRowPolicy,
    expansion: HashMap<String, String>,
    previous: Option<Arc<ConversationRows>>,
) -> Arc<ConversationRows> {
    ConversationRows::project(source, scope(), expansion, policy, previous)
}

fn header(rows: &ConversationRows) -> &Arc<VisibleConversationRow> {
    rows.rows
        .iter()
        .find(|row| {
            matches!(
                row.data.content,
                ConversationRowContent::ActivityHeader { .. }
            )
        })
        .unwrap()
}

fn expand(row: &VisibleConversationRow) -> HashMap<String, String> {
    let ConversationRowContent::ActivityHeader { activity } = &row.data.content else {
        panic!("activity header")
    };
    [(row.id.clone(), activity.status.clone())].into()
}

const TIMELINE: ConversationRowPolicy = ConversationRowPolicy {
    requests: false,
    progress: ProgressRows::UnlessHeader,
};

#[test]
fn request_and_progress_placement_is_explicit_and_source_counts_ignore_visibility() {
    let mut thread = source(false);
    for (id, target) in [
        (
            "turn-request",
            RequestTarget::Turn {
                turn_id: "turn".into(),
                item_id: None,
            },
        ),
        ("session-request", RequestTarget::Session),
    ] {
        let request = Request {
            id: id.into(),
            target,
            delivery: RequestDelivery::Awaiting,
            body: RequestBody::ToolExecution {
                tool: "tool".into(),
                namespace: None,
                arguments: json!({}),
            },
        };
        Arc::make_mut(&mut thread)
            .requests
            .insert(request.id.clone(), Arc::new(request));
    }
    let raw = render(thread, None);
    let inline = project(
        raw.clone(),
        ConversationRowPolicy {
            requests: true,
            progress: ProgressRows::Always,
        },
        HashMap::new(),
        None,
    );
    let docked = project(
        raw.clone(),
        ConversationRowPolicy {
            requests: false,
            progress: ProgressRows::Hidden,
        },
        HashMap::new(),
        None,
    );
    let timeline = project(raw.clone(), TIMELINE, HashMap::new(), None);
    assert_eq!(
        header(&timeline)
            .progress
            .as_ref()
            .unwrap()
            .label(crate::localization::UiLanguage::Japanese, 3.0),
        "2秒 作業中 · pwd"
    );
    let requests = |rows: &ConversationRows| {
        rows.rows
            .iter()
            .filter(|row| {
                matches!(
                    row.data.content,
                    ConversationRowContent::PendingRequest { .. }
                )
            })
            .count()
    };
    let progress = |rows: &ConversationRows| {
        rows.rows
            .iter()
            .filter(|row| matches!(row.data.content, ConversationRowContent::InProgress { .. }))
            .count()
    };
    assert_eq!(requests(&inline), 2);
    assert_eq!(requests(&docked), 0);
    assert_eq!(requests(&timeline), 0);
    assert_eq!(progress(&inline), 1);
    assert_eq!(progress(&docked), 0);
    assert_eq!(
        progress(&timeline),
        0,
        "hidden activity and requests leave the working header adjacent"
    );
    assert_eq!(
        inline.source_item_count(),
        3,
        "two requests plus the hidden command"
    );
    assert_eq!(docked.source_item_count(), inline.source_item_count());
    assert_eq!(timeline.source_item_count(), inline.source_item_count());
    let opened = project(
        raw,
        TIMELINE,
        expand(header(&timeline)),
        Some(timeline.clone()),
    );
    assert_eq!(
        progress(&opened),
        1,
        "a displayed activity separates the working header from progress"
    );
    assert_eq!(opened.source_item_count(), timeline.source_item_count());
    assert_eq!(
        header(&opened)
            .progress
            .as_ref()
            .unwrap()
            .label(crate::localization::UiLanguage::Japanese, 3.0),
        "2秒 作業中…"
    );
    let progress_row = opened
        .rows
        .iter()
        .find(|row| matches!(row.data.content, ConversationRowContent::InProgress { .. }))
        .unwrap();
    assert_eq!(
        progress_row
            .progress
            .as_ref()
            .unwrap()
            .label(crate::localization::UiLanguage::Japanese, 3.0),
        "2秒 作業中…"
    );
    assert!(
        opened
            .rows
            .iter()
            .any(|row| matches!(row.data.content, ConversationRowContent::Activity { .. }))
    );
    assert!(
        !timeline
            .rows
            .iter()
            .any(|row| matches!(row.data.content, ConversationRowContent::Activity { .. }))
    );
}

#[test]
fn completion_refreshes_actions_without_replacing_the_body_and_stable_rows_reuse_arcs() {
    let mut thread = source(true);
    // A response alone keeps its last-row position across completion, isolating the actions.
    Arc::make_mut(&mut Arc::make_mut(&mut thread).turns.as_mut().unwrap()[0])
        .items
        .as_mut()
        .unwrap()
        .remove(0);
    let raw = render(thread.clone(), None);
    let policy = ConversationRowPolicy {
        requests: false,
        progress: ProgressRows::Hidden,
    };
    let live = project(raw.clone(), policy, HashMap::new(), None);
    let repeated = project(raw.clone(), policy, HashMap::new(), Some(live.clone()));
    assert!(Arc::ptr_eq(&live, &repeated));
    let before = live
        .rows
        .iter()
        .find(|row| matches!(row.data.content, ConversationRowContent::Response { .. }))
        .unwrap();
    assert!(before.last);
    let ConversationRowContent::Response {
        item: before_item,
        show_actions: false,
        ..
    } = &before.data.content
    else {
        panic!("live response")
    };
    Arc::make_mut(&mut Arc::make_mut(&mut thread).turns.as_mut().unwrap()[0]).status =
        TurnStatus::Completed;
    let done = project(
        render(thread.clone(), Some(raw)),
        policy,
        HashMap::new(),
        Some(live.clone()),
    );
    let after = done
        .rows
        .iter()
        .find(|row| matches!(row.data.content, ConversationRowContent::Response { .. }))
        .unwrap();
    let ConversationRowContent::Response {
        item: after_item,
        show_actions: true,
        fork_turn_id: Some(id),
    } = &after.data.content
    else {
        panic!("completed response actions")
    };
    assert_eq!(id.as_str(), "turn");
    assert!(Arc::ptr_eq(before_item, after_item));
    assert!(after.last);
    assert!(!Arc::ptr_eq(before, after));
    assert!(
        before != after,
        "native equality must observe changed action flags"
    );
    let repeated = project(
        done.source.clone(),
        policy,
        HashMap::new(),
        Some(done.clone()),
    );
    assert!(Arc::ptr_eq(&done, &repeated));
    Arc::make_mut(&mut thread)
        .capabilities
        .as_mut()
        .unwrap()
        .fork = false;
    let no_fork = project(
        render(thread, Some(done.source.clone())),
        policy,
        HashMap::new(),
        Some(done.clone()),
    );
    let response = no_fork
        .rows
        .iter()
        .find(|row| matches!(row.data.content, ConversationRowContent::Response { .. }))
        .unwrap();
    let ConversationRowContent::Response {
        item,
        show_actions: true,
        fork_turn_id: None,
    } = &response.data.content
    else {
        panic!("completed copy action without fork support")
    };
    assert!(Arc::ptr_eq(after_item, item));
    assert!(
        after != response,
        "removing fork support must update the row even with copy still available"
    );
}

#[test]
fn repeated_turn_and_item_ids_have_independent_expansion_and_reuse() {
    let mut thread = source(true);
    let first = thread.turns.as_ref().unwrap()[0].clone();
    Arc::make_mut(&mut thread)
        .turns
        .as_mut()
        .unwrap()
        .push(first);
    let raw = render(thread, None);
    assert_ne!(raw.turns[0].key(), raw.turns[1].key());
    let collapsed = project(raw.clone(), TIMELINE, HashMap::new(), None);
    let headers: Vec<_> = collapsed
        .rows
        .iter()
        .filter(|row| {
            matches!(
                row.data.content,
                ConversationRowContent::ActivityHeader { .. }
            )
        })
        .collect();
    assert_eq!(headers.len(), 2);
    assert_ne!(headers[0].id, headers[1].id);
    let opened = project(raw, TIMELINE, expand(headers[0]), Some(collapsed.clone()));
    assert_eq!(
        opened
            .rows
            .iter()
            .filter(|row| matches!(row.data.content, ConversationRowContent::Activity { .. }))
            .count(),
        1
    );
    let second = opened
        .rows
        .iter()
        .find(|row| row.id == headers[1].id)
        .unwrap();
    assert!(!second.expanded);
    assert!(
        headers[1].as_ref() == second.as_ref(),
        "native wrappers share the same row identity"
    );
    for before in collapsed
        .rows
        .iter()
        .filter(|row| row.origin == headers[1].origin)
    {
        let after = opened.rows.iter().find(|row| row.id == before.id).unwrap();
        assert!(
            Arc::ptr_eq(before, after),
            "unaffected rows retain their render values"
        );
    }
    let unique: std::collections::HashSet<_> = opened.rows.iter().map(|row| &row.id).collect();
    assert_eq!(unique.len(), opened.rows.len());
}

#[test]
fn scopes_and_panes_do_not_share_expansion_choices_or_mutate_previous_results() {
    let raw = render(source(false), None);
    let collapsed = project(raw.clone(), TIMELINE, HashMap::new(), None);
    let choices = expand(header(&collapsed));
    let opened = project(
        raw.clone(),
        TIMELINE,
        choices.clone(),
        Some(collapsed.clone()),
    );
    assert!(header(&opened).expanded);
    assert!(!header(&collapsed).expanded);
    let other_pane = project(raw.clone(), TIMELINE, HashMap::new(), Some(opened.clone()));
    assert!(!header(&other_pane).expanded);
    assert!(header(&opened).expanded);
    for key in [
        PanelScope {
            host: "other".into(),
            ..scope()
        },
        PanelScope {
            storage: "other".into(),
            ..scope()
        },
        PanelScope {
            draft: DraftKey::from("other"),
            ..scope()
        },
    ] {
        let other = ConversationRows::project(
            raw.clone(),
            key,
            choices.clone(),
            TIMELINE,
            Some(opened.clone()),
        );
        assert!(!header(&other).expanded);
        assert_ne!(header(&opened).id, header(&other).id);
    }
}

#[test]
fn collapsed_body_updates_are_visible_on_reopen_and_status_changes_close_activity() {
    let mut thread = source(false);
    let raw = render(thread.clone(), None);
    let collapsed = project(raw.clone(), TIMELINE, HashMap::new(), None);
    let choices = expand(header(&collapsed));
    let native = Arc::make_mut(&mut Arc::make_mut(&mut thread).turns.as_mut().unwrap()[0])
        .items
        .as_mut()
        .unwrap();
    let command = Arc::make_mut(&mut native[0]);
    command.body = serde_json::from_value(json!({"inline":{"body":{"commandExecution":{
        "command":"pwd", "cwd":null, "output":"latest hidden output", "exitCode":null
    }}}}))
    .unwrap();
    let updated_raw = render(thread.clone(), Some(raw));
    let updated = project(
        updated_raw.clone(),
        TIMELINE,
        HashMap::new(),
        Some(collapsed.clone()),
    );
    assert!(
        Arc::ptr_eq(header(&collapsed), header(&updated)),
        "unchanged summary and progress stay cached"
    );
    let reopened = project(
        updated_raw.clone(),
        TIMELINE,
        choices.clone(),
        Some(updated),
    );
    let activity = reopened
        .rows
        .iter()
        .find_map(|row| match &row.data.content {
            ConversationRowContent::Activity { item, .. } => Some(item),
            _ => None,
        })
        .unwrap();
    assert!(activity.expanded_body().contains("latest hidden output"));
    Arc::make_mut(&mut Arc::make_mut(&mut thread).turns.as_mut().unwrap()[0]).status =
        TurnStatus::Completed;
    let completed = project(
        render(thread, Some(updated_raw)),
        TIMELINE,
        choices,
        Some(reopened),
    );
    assert!(!header(&completed).expanded);
    assert!(
        !completed
            .rows
            .iter()
            .any(|row| matches!(row.data.content, ConversationRowContent::Activity { .. }))
    );
}

#[test]
fn timestamps_last_row_and_progress_are_part_of_render_identity() {
    let mut thread = source(false);
    Arc::make_mut(&mut Arc::make_mut(&mut thread).turns.as_mut().unwrap()[0])
        .items.as_mut().unwrap().insert(0, Arc::new(serde_json::from_value(json!({
            "id":"user", "body":{"inline":{"body":{"userMessage":{"text":"question", "content":[]}}}}
        })).unwrap()));
    let raw = render(thread.clone(), None);
    let initial = project(raw.clone(), TIMELINE, HashMap::new(), None);
    let user = |rows: &ConversationRows| {
        rows.rows
            .iter()
            .find(|row| matches!(row.data.content, ConversationRowContent::User { .. }))
            .unwrap()
            .clone()
    };
    let before = user(&initial);
    assert_eq!(before.data.timestamp_ms, Some(1000));
    assert!(header(&initial).last);

    let mut timed = thread.clone();
    Arc::make_mut(&mut Arc::make_mut(&mut timed).turns.as_mut().unwrap()[0]).started_at_ms =
        Some(2000);
    let retimed = project(
        render(timed, Some(raw.clone())),
        TIMELINE,
        HashMap::new(),
        Some(initial.clone()),
    );
    let after = user(&retimed);
    assert_eq!(after.data.timestamp_ms, Some(2000));
    let (
        ConversationRowContent::User { item: old_item },
        ConversationRowContent::User { item: new_item },
    ) = (&before.data.content, &after.data.content)
    else {
        panic!("user rows")
    };
    assert!(Arc::ptr_eq(old_item, new_item), "only row timing changed");
    assert!(before != after);
    assert_eq!(
        header(&retimed)
            .progress
            .as_ref()
            .unwrap()
            .label(crate::localization::UiLanguage::Japanese, 3.0),
        "1秒 作業中 · pwd"
    );
    assert!(!Arc::ptr_eq(header(&initial), header(&retimed)));

    let mut answered = thread.clone();
    let answer = source(true).turns.as_ref().unwrap()[0]
        .items
        .as_ref()
        .unwrap()[1]
        .clone();
    Arc::make_mut(&mut Arc::make_mut(&mut answered).turns.as_mut().unwrap()[0])
        .items
        .as_mut()
        .unwrap()
        .push(answer);
    let moved = project(
        render(answered, Some(raw.clone())),
        TIMELINE,
        HashMap::new(),
        Some(initial.clone()),
    );
    assert!(!header(&moved).last);
    assert!(
        !Arc::ptr_eq(header(&initial), header(&moved)),
        "a new final response changes the preceding row padding"
    );

    Arc::make_mut(&mut Arc::make_mut(&mut thread).turns.as_mut().unwrap()[0]).started_at =
        Some(7.0);
    let legacy_timed = project(
        render(thread, Some(raw)),
        TIMELINE,
        HashMap::new(),
        Some(initial),
    );
    let fallback = user(&legacy_timed);
    assert_eq!(fallback.data.timestamp_ms, before.data.timestamp_ms);
    assert!(
        before != fallback,
        "native timestamp fallbacks cannot retain stale turn timing"
    );
}

#[test]
fn toggling_returns_choices_and_loads_without_mutating_the_frame_or_accepting_stale_ids() {
    let mut thread = source(true);
    let turn = Arc::make_mut(&mut Arc::make_mut(&mut thread).turns.as_mut().unwrap()[0]);
    turn.items.as_mut().unwrap().remove(0);
    turn.items_summary = true;
    turn.status = TurnStatus::Completed;
    let raw = render(thread, None);
    let collapsed = project(raw.clone(), TIMELINE, HashMap::new(), None);
    let id = header(&collapsed).id.clone();
    let change = collapsed.toggle_activity(id.clone()).unwrap();
    let load = change.load_items.unwrap();
    assert_eq!(load.thread_id.id, "thread");
    assert_eq!(load.turn_id.as_str(), "turn");
    assert!(collapsed.expansion.is_empty());
    assert!(!header(&collapsed).expanded);
    let opened = project(
        raw.clone(),
        TIMELINE,
        change.expansion,
        Some(collapsed.clone()),
    );
    assert!(header(&opened).expanded);
    let close = opened.toggle_activity(id.clone()).unwrap();
    assert!(!close.expansion.contains_key(&id));
    assert!(close.load_items.is_none(), "closing never loads details");
    let response = opened
        .rows
        .iter()
        .find(|row| matches!(row.data.content, ConversationRowContent::Response { .. }))
        .unwrap();
    assert!(opened.toggle_activity(response.id.clone()).is_none());
    assert!(opened.toggle_activity("missing".into()).is_none());
    let other = ConversationRows::project(
        raw,
        PanelScope {
            host: "new host".into(),
            ..scope()
        },
        HashMap::new(),
        TIMELINE,
        Some(opened),
    );
    assert!(
        other.toggle_activity(id).is_none(),
        "retired host/window actions cannot load into another scope"
    );
}

#[test]
fn queued_inputs_keep_their_footer_between_turns_and_session_rows_without_inflating_source_counts()
{
    use crate::state::{Draft, PendingSubmission};
    let mut thread = source(false);
    Arc::make_mut(&mut thread).requests.insert(
        "session".into(),
        Arc::new(Request {
            id: "session".into(),
            target: RequestTarget::Session,
            delivery: RequestDelivery::Awaiting,
            body: RequestBody::ToolExecution {
                tool: "tool".into(),
                namespace: None,
                arguments: json!({}),
            },
        }),
    );
    let mut pending = [(
        "pending".into(),
        Arc::new(PendingSubmission {
            sequence: 0,
            submitted_at_ms: Some(12_000),
            draft_key: thread.id.as_ref().unwrap().into(),
            draft: Arc::new(Draft {
                text: "unsent footer".into(),
                ..Default::default()
            }),
            turn_id: Some("unloaded".into()),
            after_item_id: None,
            accepted: false,
            delivery_unknown: true,
        }),
    )]
    .into_iter()
    .collect();
    let raw = project_conversation(
        crate::localization::UiLanguage::Japanese,
        &scope().draft,
        &pending,
        thread.clone(),
        &None,
    );
    let policy = ConversationRowPolicy {
        requests: true,
        progress: ProgressRows::Always,
    };
    let rows = project(raw.clone(), policy, HashMap::new(), None);
    let queue = rows.queued_rows();
    assert_eq!(queue.len(), 1);
    assert_eq!(queue[0].data.timestamp_ms, Some(12_000));
    assert!(queue[0].last);
    let ConversationRowContent::User { item } = &queue[0].data.content else {
        panic!("queued input")
    };
    assert_eq!(item.data.body.as_deref(), Some("unsent footer"));
    assert_eq!(item.unknown_submission_id().as_deref(), Some("pending"));
    assert_eq!(
        rows.source_item_count(),
        2,
        "the source command and session request exclude queued local input"
    );
    let grouped: Vec<_> = rows
        .turn_rows()
        .into_iter()
        .chain(rows.queued_rows())
        .chain(rows.session_rows())
        .map(|row| row.id.clone())
        .collect();
    assert_eq!(
        grouped,
        rows.rows
            .iter()
            .map(|row| row.id.clone())
            .collect::<Vec<_>>()
    );
    Arc::make_mut(pending.get_mut("pending").unwrap()).submitted_at_ms = Some(13_000);
    let updated = project(
        project_conversation(
            crate::localization::UiLanguage::Japanese,
            &scope().draft,
            &pending,
            thread,
            &Some(raw),
        ),
        policy,
        HashMap::new(),
        Some(rows),
    );
    assert_eq!(updated.queued_rows()[0].data.timestamp_ms, Some(13_000));
    assert!(!Arc::ptr_eq(&queue[0], &updated.queued_rows()[0]));
}

#[test]
fn scope_components_are_unambiguous_even_when_they_contain_key_delimiters() {
    let raw = render(source(false), None);
    let before_scope = PanelScope {
        host: "a:b".into(),
        storage: "c".into(),
        draft: "draft:[]".into(),
    };
    let first =
        ConversationRows::project(raw.clone(), before_scope, HashMap::new(), TIMELINE, None);
    let choices = expand(header(&first));
    let other_scope = PanelScope {
        host: "a".into(),
        storage: "b:c".into(),
        draft: "draft:[]".into(),
    };
    let other = ConversationRows::project(raw, other_scope, choices, TIMELINE, Some(first.clone()));
    assert_ne!(header(&first).id, header(&other).id);
    assert!(!header(&other).expanded);
}
