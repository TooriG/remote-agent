//! This PC's Host as a phone reaches it: the invitation it scans or pastes.
use agent_protocol::models::Invitation;
use std::fmt::Write;

/// The invitation as a phone reads it from a QR code or a paste: its JSON in
/// ASCII. The QR encoder declares no character set, so camera scanners need no
/// UTF-8 guess; JSON escapes keep names and recipients, surrogate pairs included.
pub fn invitation_code(invitation: &Invitation) -> String {
    ascii_json(&serde_json::to_string(invitation).expect("an invitation serializes"))
}

fn ascii_json(json: &str) -> String {
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
    use super::*;
    use proptest::prelude::*;
    use serde_json::{Value, json};

    #[test]
    fn the_code_keeps_every_invitation_field_in_ascii() {
        let invitation = Invitation {
            endpoint: "endpoint-test".into(),
            invitation: uuid::Uuid::new_v4(),
            expires_at: 1794329560,
            host_name: "PC \u{65e5}\u{672c}\u{8a9e} \u{1f4bb}".into(),
            ai_recipients: vec![
                "OpenAI".into(),
                "DeepSeek Harness\u{ff08}\u{9078}\u{629e}\u{ff09}".into(),
            ],
            transcription_recipient: None,
        };
        let code = invitation_code(&invitation);
        assert!(code.is_ascii());
        assert!(serde_json::from_str::<Invitation>(&code).unwrap() == invitation);
    }

    #[test]
    fn existing_escapes_stay_unchanged() {
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
