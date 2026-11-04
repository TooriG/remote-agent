use super::*;
use proptest::prelude::*;
use serde_json::json;

fn native(kind: &str, content: Value) -> Value {
    json!({"id":"item","type":kind,"status":"completed","content":content})
}

#[test]
fn taking_native_fields_preserves_other_values_without_inserting_missing_keys() {
    let mut value = json!({"payload":[{"text":"opaque"}],"other":{"keep":[1,2]}});
    let pointer = value["payload"].as_array().unwrap().as_ptr();
    let payload = take_value(&mut value, "payload");
    assert_eq!(payload, json!([{"text":"opaque"}]));
    assert_eq!(payload.as_array().unwrap().as_ptr(), pointer);
    let expected = json!({"payload":null,"other":{"keep":[1,2]}});
    assert_eq!(value, expected);
    assert!(take_value(&mut value, "missing").is_null());
    assert_eq!(value, expected);
    for mut value in [Value::Null, json!(1), json!([])] {
        let original = value.clone();
        assert!(take_value(&mut value, "missing").is_null());
        assert_eq!(value, original);
    }
}

#[test]
fn reasoning_moves_string_and_object_parts_and_the_summary() {
    let mut value = native(
        "reasoning",
        json!(["first", {"text":"second"}, "", {"text":""}, {"text":1}, false, null, ["ignored"]]),
    );
    value["summary"] = json!("summary");
    let first = value["content"][0].as_str().unwrap().as_ptr();
    let second = value["content"][1]["text"].as_str().unwrap().as_ptr();
    let summary_pointer = value["summary"].as_str().unwrap().as_ptr();
    let item = parse_item(value).unwrap();
    let ItemBody::Reasoning { content, summary } = item.body() else {
        panic!("reasoning body")
    };
    assert_eq!(content, &["first", "second", "", ""]);
    assert_eq!(summary, &["summary"]);
    assert_eq!(content[0].as_ptr(), first);
    assert_eq!(content[1].as_ptr(), second);
    assert_eq!(summary[0].as_ptr(), summary_pointer);
    let value = native("reasoning", json!("single"));
    let pointer = value["content"].as_str().unwrap().as_ptr();
    let item = parse_item(value).unwrap();
    let ItemBody::Reasoning { content, .. } = item.body() else {
        panic!("reasoning body")
    };
    assert_eq!(content, &["single"]);
    assert_eq!(content[0].as_ptr(), pointer);
}

#[test]
fn reasoning_falls_back_to_text_only_for_missing_or_null_content() {
    for (content, expected) in [
        (None, vec!["fallback"]),
        (Some(Value::Null), vec!["fallback"]),
        (Some(json!("")), vec![""]),
        (Some(json!([])), vec![]),
        (Some(json!(7)), vec![]),
        (Some(json!({"text":"ignored"})), vec![]),
    ] {
        let mut value = json!({"id":"item","type":"reasoning","text":"fallback","summary":["summary",{"text":"detail"}]});
        if let Some(content) = content {
            value["content"] = content;
        }
        assert_eq!(
            parse_item(value).unwrap().body(),
            &ItemBody::Reasoning {
                content: expected.into_iter().map(str::to_owned).collect(),
                summary: vec!["summary".into(), "detail".into()],
            }
        );
    }
}

#[test]
fn user_parts_keep_every_kind_and_move_text_names_paths_and_images() {
    let mut value = native(
        "userMessage",
        json!([
            {"type":"localImage","path":"/repo/image.png"},
            {"type":"image","url":"data:image/png;base64,image"},
            {"type":"skill","name":"skill","path":"/repo/skill"},
            {"type":"mention","name":"plugin","path":"plugin://plugin"},
            {"type":"mention","name":"app","path":"app://app"},
            {"type":"mention","name":"file","path":"/repo/file"},
            {"type":"future","text":"fallback text"},
            {"text":""},
            {"type":"localImage","path":4},
            {"type":"mention","name":false,"path":null},
            {"type":"mention","path":"Plugin://case-sensitive"},
            {"type":"future","text":4}, "ignored", null, []
        ]),
    );
    value["text"] = json!("legacy text");
    let text_pointer = value["text"].as_str().unwrap().as_ptr();
    let image_pointer = value["content"][1]["url"].as_str().unwrap().as_ptr();
    let name_pointer = value["content"][2]["name"].as_str().unwrap().as_ptr();
    let path_pointer = value["content"][2]["path"].as_str().unwrap().as_ptr();
    let fallback_pointer = value["content"][6]["text"].as_str().unwrap().as_ptr();
    let item = parse_item(value).unwrap();
    let ItemBody::UserMessage { text, content } = item.body() else {
        panic!("user body")
    };
    assert_eq!(text.as_deref(), Some("legacy text"));
    assert_eq!(text.as_ref().unwrap().as_ptr(), text_pointer);
    assert_eq!(
        content,
        &[
            MessagePart::Image {
                source: "/repo/image.png".into()
            },
            MessagePart::Image {
                source: "data:image/png;base64,image".into()
            },
            MessagePart::Invocation {
                name: "skill".into(),
                path: "/repo/skill".into()
            },
            MessagePart::Invocation {
                name: "plugin".into(),
                path: "plugin://plugin".into()
            },
            MessagePart::Invocation {
                name: "app".into(),
                path: "app://app".into()
            },
            MessagePart::Attachment {
                name: "file".into(),
                path: "/repo/file".into()
            },
            MessagePart::Text {
                text: "fallback text".into()
            },
            MessagePart::Text { text: "".into() },
            MessagePart::Image { source: "".into() },
            MessagePart::Attachment {
                name: "".into(),
                path: "".into()
            },
            MessagePart::Attachment {
                name: "".into(),
                path: "Plugin://case-sensitive".into()
            },
        ]
    );
    assert!(
        matches!(&content[1], MessagePart::Image { source } if source.as_ptr() == image_pointer)
    );
    assert!(
        matches!(&content[2], MessagePart::Invocation { name, path } if name.as_ptr() == name_pointer && path.as_ptr() == path_pointer)
    );
    assert!(matches!(&content[6], MessagePart::Text { text } if text.as_ptr() == fallback_pointer));
}

#[test]
fn invalid_user_text_still_fails_and_unknown_items_keep_their_complete_payload() {
    for invalid in [json!(1), json!(false), json!([]), json!({})] {
        let mut value = native("userMessage", json!([]));
        value["text"] = invalid;
        assert!(parse_item(value).is_err());
    }
    for content in [
        Value::Null,
        json!("ignored"),
        json!({"text":"ignored"}),
        json!(1),
    ] {
        assert_eq!(
            parse_item(native("userMessage", content)).unwrap().body(),
            &ItemBody::UserMessage {
                text: None,
                content: vec![]
            }
        );
    }
    let value = json!({"id":"item","type":"future","clientId":"client","text":"opaque","content":[{"text":"keep"}],"summary":"keep","other":{"a":[1,2]}});
    assert!(
        matches!(parse_item(value.clone()).unwrap().body(), ItemBody::Custom { value: body, .. } if body == &value)
    );
}

proptest! {
    #[test]
    fn reasoning_arrays_keep_only_textual_entries_in_input_order(
        entries in proptest::collection::vec((0u8..4, any::<String>()), 0..128),
    ) {
        let mut values = Vec::new();
        let mut expected = Vec::new();
        for (kind, text) in entries {
            match kind {
                0 => { values.push(json!(text)); expected.push(text); }
                1 => { values.push(json!({"text":text})); expected.push(text); }
                2 => values.push(json!({"text":null,"other":text})),
                _ => values.push(json!([text])),
            }
        }
        let item = parse_item(native("reasoning", json!(values))).unwrap();
        prop_assert_eq!(item.body(), &ItemBody::Reasoning { content: expected, summary: vec![] });
    }
}
