use super::*;
use proptest::prelude::*;

fn hunk(lines: Value) -> Value {
    json!({"oldStart":4,"oldLines":7,"newStart":8,"newLines":9,"lines":lines})
}

#[test]
fn patch_headers_line_prefixes_and_hunk_order_remain_exact() {
    let value = json!([
        hunk(json!([" unchanged 日本語", "-old\ttext", "+new\"text", "\\ No newline at end of file"])),
        {"oldStart":u64::MAX,"oldLines":0,"newStart":0,"newLines":u64::MAX,"lines":[],"future":{"nested":[null,1,"keep"]}}
    ]);
    let original = value.clone();
    assert_eq!(
        structured_patch(&value).as_deref(),
        Some(
            "@@ -4,7 +8,9 @@\n unchanged 日本語\n-old\ttext\n+new\"text\n\\ No newline at end of file\n@@ -18446744073709551615,0 +0,18446744073709551615 @@\n"
        )
    );
    assert_eq!(value, original);
    let hunks: Vec<NativePatchHunk<'_>> = serde::Deserialize::deserialize(&value).unwrap();
    assert_eq!(
        hunks[0].lines[0].as_ptr(),
        value[0]["lines"][0].as_str().unwrap().as_ptr()
    );
}

#[test]
fn malformed_patches_are_rejected_as_a_whole() {
    let valid = hunk(json!(["+valid"]));
    for value in [
        Value::Null,
        json!("patch"),
        json!({}),
        json!([]),
        json!([null]),
        json!([1]),
    ] {
        assert!(structured_patch(&value).is_none(), "{value}");
    }
    for lines in [
        Value::Null,
        json!("+line"),
        json!([null]),
        json!([1]),
        json!([""]),
        json!(["invalid"]),
        json!(["@header"]),
        json!(["+valid", false]),
    ] {
        let invalid = hunk(lines);
        assert!(structured_patch(&json!([invalid.clone()])).is_none());
        assert!(structured_patch(&json!([valid.clone(), invalid])).is_none());
    }
    for key in ["oldStart", "oldLines", "newStart", "newLines"] {
        let mut missing = valid.clone();
        missing.as_object_mut().unwrap().remove(key);
        assert!(structured_patch(&json!([missing])).is_none());
        for value in [Value::Null, json!(-1), json!(1.0), json!("1"), json!(true)] {
            let mut invalid = valid.clone();
            invalid[key] = value;
            assert!(structured_patch(&json!([invalid])).is_none(), "{key}");
        }
    }
}

#[test]
fn invalid_observed_patch_keeps_the_previous_diff_and_proposal() {
    let change = FileChange {
        path: "/repo/file".into(),
        kind: FileChangeKind::Update { move_path: None },
        diff: Some("previous diff".into()),
        proposal: Some(FileProposal::Edit {
            old_text: "before".into(),
            new_text: "after".into(),
            replace_all: false,
        }),
    };
    let body = ItemBody::FileChange {
        changes: vec![change.clone()],
        output: "old output".into(),
    };
    let original = body.clone();
    assert_eq!(
        tool_result_body(
            &body,
            &json!("current output"),
            &json!({"type":"update","structuredPatch":[hunk(json!(["unmarked"]))]})
        ),
        ItemBody::FileChange {
            changes: vec![change],
            output: "current output".into()
        }
    );
    assert_eq!(body, original);
}

proptest! {
    #[test]
    fn marked_patch_lines_render_without_changing_their_text(
        lines in proptest::collection::vec((0u8..4, any::<String>()), 0..64),
    ) {
        let lines: Vec<_> = lines.into_iter().map(|(prefix, text)| format!("{}{text}", [' ', '+', '-', '\\'][usize::from(prefix)])).collect();
        let mut expected = String::from("@@ -4,7 +8,9 @@\n");
        if !lines.is_empty() {
            expected.push_str(&lines.join("\n"));
            expected.push('\n');
        }
        prop_assert_eq!(structured_patch(&json!([hunk(json!(lines))])), Some(expected));
    }
}
