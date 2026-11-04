use std::fmt::Write;

/// The QR encoder emits byte segments without a character-set declaration.
/// Keep the invitation JSON in ASCII so camera scanners need no UTF-8 guessing.
/// JSON escapes preserve names and recipient disclosures, including surrogate pairs.
pub(super) fn ascii_json(json: &str) -> String {
    let mut text = String::with_capacity(json.len());
    for unit in json.encode_utf16() {
        if unit <= 0x7f {
            text.push(unit as u8 as char);
        } else {
            write!(text, "\\u{unit:04x}").expect("writing to a String cannot fail");
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::ascii_json;
    use proptest::prelude::*;
    use serde_json::{Value, json};

    #[test]
    fn camera_text_preserves_invitation_fields_and_escapes_unicode() {
        let invitation = json!({
            "endpoint": "endpoint-test",
            "invitation": "229696e9-145b-45a1-a5d0-7673ff6109f1",
            "expiresAt": 1794329560_u64,
            "hostName": "PC \u{65e5}\u{672c}\u{8a9e} \u{1f4bb}",
            "aiRecipients": ["OpenAI", "DeepSeek Harness\u{ff08}\u{9078}\u{629e}\u{ff09}"],
            "transcriptionRecipient": null
        });
        let text = ascii_json(&serde_json::to_string(&invitation).unwrap());
        assert!(text.is_ascii());
        assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), invitation);
    }

    #[test]
    fn ascii_json_including_existing_escapes_is_unchanged() {
        let text = r#"{"hostName":"PC \"quoted\"\\path\n\ud83d\udcbb","expiresAt":123}"#;
        assert_eq!(ascii_json(text), text);
        assert!(serde_json::from_str::<Value>(text).is_ok());
    }

    proptest! {
        #[test]
        fn every_unicode_name_round_trips_as_ascii(name in any::<String>()) {
            let invitation = json!({"hostName": name, "aiRecipients": [name]});
            let text = ascii_json(&serde_json::to_string(&invitation).unwrap());
            prop_assert!(text.is_ascii());
            prop_assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), invitation);
        }
    }
}
