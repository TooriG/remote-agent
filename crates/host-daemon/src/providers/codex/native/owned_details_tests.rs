use super::*;
use proptest::prelude::*;
use serde_json::json;

#[test]
fn file_changes_preserve_every_kind_and_move_paths_diffs_and_output() {
    let value = json!({"id":"item","type":"fileChange","output":"native output","changes":[
        {"path":"/add","kind":"add","diff":"+added"},
        {"path":"/delete","kind":{"type":"delete","movePath":"ignored"},"diff":null},
        {"path":"/update","kind":{"type":"update","movePath":"/moved"},"diff":"patch"},
        {"path":"/simple","kind":"update","diff":""},
        {"path":"/future","kind":{"type":"future","movePath":"ignored"},"diff":1},
        {"path":1,"kind":{"type":"update","movePath":true}},
        null, 4, "ignored", []
    ]});
    let path_pointer = value["changes"][0]["path"].as_str().unwrap().as_ptr();
    let diff_pointer = value["changes"][0]["diff"].as_str().unwrap().as_ptr();
    let move_pointer = value["changes"][2]["kind"]["movePath"]
        .as_str()
        .unwrap()
        .as_ptr();
    let output_pointer = value["output"].as_str().unwrap().as_ptr();
    let item = parse_item(value).unwrap();
    let ItemBody::FileChange { changes, output } = item.body() else {
        panic!("file change body")
    };
    let mut expected: Vec<_> = [
        ("/add", FileChangeKind::Add, Some("+added")),
        ("/delete", FileChangeKind::Delete, None),
        (
            "/update",
            FileChangeKind::Update {
                move_path: Some("/moved".into()),
            },
            Some("patch"),
        ),
        (
            "/simple",
            FileChangeKind::Update { move_path: None },
            Some(""),
        ),
        ("/future", FileChangeKind::Unknown, None),
        ("", FileChangeKind::Update { move_path: None }, None),
    ]
    .into_iter()
    .map(|(path, kind, diff)| FileChange {
        path: path.into(),
        kind,
        diff: diff.map(str::to_owned),
        proposal: None,
    })
    .collect();
    expected.extend(vec![
        FileChange {
            path: String::new(),
            kind: FileChangeKind::Unknown,
            diff: None,
            proposal: None
        };
        4
    ]);
    assert_eq!(changes, &expected);
    assert_eq!(output, "native output");
    assert_eq!(changes[0].path.as_ptr(), path_pointer);
    assert_eq!(changes[0].diff.as_ref().unwrap().as_ptr(), diff_pointer);
    assert!(
        matches!(&changes[2].kind, FileChangeKind::Update { move_path: Some(path) } if path.as_ptr() == move_pointer)
    );
    assert_eq!(output.as_ptr(), output_pointer);
}

#[test]
fn file_change_optional_values_keep_missing_empty_and_invalid_shapes_distinct() {
    for (value, output) in [
        (Value::Null, ""),
        (json!("invalid"), "invalid"),
        (json!({"path":"ignored"}), ""),
        (json!(1), ""),
    ] {
        assert_eq!(
            parse_item(json!({"id":"item","type":"fileChange","changes":value,"output":value}))
                .unwrap()
                .body(),
            &ItemBody::FileChange {
                changes: vec![],
                output: output.into()
            }
        );
    }
    assert_eq!(
        parse_item(json!({"id":"item","type":"fileChange"}))
            .unwrap()
            .body(),
        &ItemBody::FileChange {
            changes: vec![],
            output: String::new()
        }
    );
    for (value, expected) in [
        (Value::Null, None),
        (json!(false), None),
        (json!(1), None),
        (json!([]), None),
        (json!({}), None),
        (json!(""), Some("")),
    ] {
        let item = parse_item(json!({"id":"item","type":"fileChange","changes":[{"kind":{"type":"update","movePath":value},"diff":value}]})).unwrap();
        let expected = expected.map(str::to_owned);
        assert_eq!(
            item.body(),
            &ItemBody::FileChange {
                changes: vec![FileChange {
                    path: String::new(),
                    kind: FileChangeKind::Update {
                        move_path: expected.clone()
                    },
                    diff: expected,
                    proposal: None,
                }],
                output: String::new(),
            }
        );
    }
}

#[test]
fn review_text_moves_without_changing_the_entering_phase_or_invalid_defaults() {
    for (kind, entering) in [("enteredReviewMode", true), ("exitedReviewMode", false)] {
        let value = json!({"id":"item","type":kind,"review":"review text"});
        let pointer = value["review"].as_str().unwrap().as_ptr();
        let item = parse_item(value).unwrap();
        assert_eq!(
            item.body(),
            &ItemBody::Review {
                entering,
                text: "review text".into()
            }
        );
        assert!(matches!(item.body(), ItemBody::Review { text, .. } if text.as_ptr() == pointer));
        for review in [Value::Null, json!(1), json!([]), json!({})] {
            assert_eq!(
                parse_item(json!({"id":"item","type":kind,"review":review}))
                    .unwrap()
                    .body(),
                &ItemBody::Review {
                    entering,
                    text: String::new()
                }
            );
        }
        assert_eq!(
            parse_item(json!({"id":"item","type":kind})).unwrap().body(),
            &ItemBody::Review {
                entering,
                text: String::new()
            }
        );
    }
}

#[test]
fn hook_fragments_move_only_object_texts_including_empty_strings() {
    let value = json!({"id":"item","type":"hookPrompt","fragments":[{"text":"first"},"ignored",{"text":""},{"text":"last"},{"text":null},null,{},[],1]});
    let first = value["fragments"][0]["text"].as_str().unwrap().as_ptr();
    let last = value["fragments"][3]["text"].as_str().unwrap().as_ptr();
    let item = parse_item(value).unwrap();
    let ItemBody::Hook { fragments } = item.body() else {
        panic!("hook body")
    };
    assert_eq!(fragments, &["first", "", "last"]);
    assert_eq!(fragments[0].as_ptr(), first);
    assert_eq!(fragments[2].as_ptr(), last);
    for fragments in [
        Value::Null,
        json!("ignored"),
        json!({"text":"ignored"}),
        json!(1),
    ] {
        assert_eq!(
            parse_item(json!({"id":"item","type":"hookPrompt","fragments":fragments}))
                .unwrap()
                .body(),
            &ItemBody::Hook { fragments: vec![] }
        );
    }
    assert_eq!(
        parse_item(json!({"id":"item","type":"hookPrompt"}))
            .unwrap()
            .body(),
        &ItemBody::Hook { fragments: vec![] }
    );
}

proptest! {
    #[test]
    fn file_change_kinds_and_optional_text_survive_native_representation_variants(
        rows in proptest::collection::vec((0u8..4, any::<String>(), proptest::option::of(any::<String>()), proptest::option::of(any::<String>()), any::<bool>()), 0..64),
    ) {
        let mut changes = Vec::new();
        let mut expected = Vec::new();
        for (kind, path, diff, moved, object) in rows {
            let tag = ["add", "delete", "update", "future"][usize::from(kind)];
            let native_kind = if object { json!({"type":tag,"movePath":moved}) } else { json!(tag) };
            changes.push(json!({"path":path,"kind":native_kind,"diff":diff}));
            expected.push(FileChange {
                path, diff, proposal: None,
                kind: match kind {
                    0 => FileChangeKind::Add,
                    1 => FileChangeKind::Delete,
                    2 => FileChangeKind::Update { move_path: if object { moved } else { None } },
                    _ => FileChangeKind::Unknown,
                },
            });
        }
        let item = parse_item(json!({"id":"item","type":"fileChange","changes":changes})).unwrap();
        prop_assert_eq!(item.body(), &ItemBody::FileChange { changes: expected, output: String::new() });
    }
}
