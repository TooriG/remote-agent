use super::*;
use proptest::prelude::*;

fn text(id: &str, status: ItemStatus, value: &str) -> Arc<Item> {
    Arc::new(Item::new(
        id.into(),
        status,
        ItemBody::AssistantText {
            text: value.into(),
            phase: AssistantPhase::Unknown,
        },
    ))
}

#[test]
fn message_matching_keeps_exact_running_and_fallback_priority_in_input_order() {
    let items = [
        text("message2:ignored", ItemStatus::Running, "match"),
        text("message:done", ItemStatus::Completed, "match"),
        text("message:running", ItemStatus::Running, "other"),
        text("message:later", ItemStatus::Running, "match"),
        text("message:equal", ItemStatus::Running, "match"),
    ];
    assert_eq!(
        message_item_id(&items, "message", 99, 1, Some("text"), Some("match")),
        "message:later"
    );
    assert_eq!(
        message_item_id(&items[..3], "message", 99, 1, Some("text"), Some("match")),
        "message:done"
    );
    for value in [Some("other"), Some("missing"), None] {
        assert_eq!(
            message_item_id(&items, "message", 99, 1, Some("text"), value),
            "message:running"
        );
    }
    for (kind, value, count) in [
        (Some("future"), Some("match"), 1),
        (None, Some("match"), 1),
        (Some("text"), Some("match"), 0),
        (Some("text"), Some("match"), 2),
    ] {
        assert_eq!(
            message_item_id(&items, "message", 99, count, kind, value),
            "message:99"
        );
    }
    let failed = [
        text("message:failed", ItemStatus::Failed, "match"),
        text("message:done", ItemStatus::Completed, "match"),
    ];
    assert_eq!(
        message_item_id(&failed, "message", 99, 1, Some("text"), Some("match")),
        "message:failed"
    );
}

#[test]
fn message_prefixes_and_empty_reasoning_preserve_exact_text_boundaries() {
    for message in ["", "メッセージ🌿", "nested:message"] {
        let id = format!("{message}:kept");
        let items = [
            text(&format!("{message}x:ignored"), ItemStatus::Running, "内容"),
            text(&id, ItemStatus::Completed, "内容"),
        ];
        assert_eq!(
            message_item_id(&items, message, 99, 1, Some("text"), Some("内容")),
            id
        );
    }
    for content in [
        vec![],
        vec![String::new()],
        vec![String::new(), String::new()],
    ] {
        let items = [Arc::new(Item::new(
            "message:empty".into(),
            ItemStatus::Completed,
            ItemBody::Reasoning {
                content,
                summary: vec![],
            },
        ))];
        assert_eq!(
            message_item_id(&items, "message", 99, 1, Some("thinking"), Some("")),
            "message:empty"
        );
        assert_eq!(
            message_item_id(&items, "message", 99, 1, Some("thinking"), None),
            "message:99"
        );
        assert_eq!(
            message_item_id(&items, "message", 99, 1, Some("text"), Some("")),
            "message:99"
        );
    }
}

proptest! {
    #[test]
    fn fragmented_reasoning_matches_its_full_native_text_without_normalizing_it(
        parts in proptest::collection::vec(".{0,40}",0..12), native in ".{0,80}"
    ) {
        let joined=parts.concat();
        let extended=format!("{joined}🚀");
        let items=[Arc::new(Item::new("message:original".into(),ItemStatus::Completed,ItemBody::Reasoning {content:parts,summary:vec![]}))];
        for value in [Some(joined.as_str()),Some(native.as_str()),Some(extended.as_str()),None] {
            let expected=if value==Some(joined.as_str()) {"message:original"}else{"message:99"};
            prop_assert_eq!(message_item_id(&items,"message",99,1,Some("thinking"),value),expected);
        }
    }
}
