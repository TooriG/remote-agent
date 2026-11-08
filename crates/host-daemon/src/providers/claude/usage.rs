use agent_protocol::operations::{UsagePeriod, UsageWindow};
use serde_json::Value;

pub(crate) fn windows(value: &Value) -> Vec<UsageWindow> {
    value["rate_limits"]["limits"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|window| {
            let period = match window["kind"].as_str()? {
                "session" => 300,
                "weekly_all" | "weekly_scoped" => 10080,
                _ => return None,
            };
            let scope = &window["scope"];
            let name = scope["model"]["display_name"]
                .as_str()
                .or_else(|| scope["surface"]["display_name"].as_str());
            let name = name.filter(|name| !name.is_empty()).map(str::to_owned);
            let resets = window["resets_at"]
                .as_str()
                .and_then(|date| chrono::DateTime::parse_from_rfc3339(date).ok())
                .map(|date| date.timestamp());
            UsageWindow::from_used(
                UsagePeriod::Minutes { minutes: period },
                name,
                window["percent"].as_f64()?,
                resets,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn claude_windows_preserve_server_order_and_model_and_surface_scopes() {
        let windows = windows(&json!({"rate_limits": {"limits": [
            {"kind":"session","percent":72,"resets_at":"2033-05-18T03:33:20Z","scope":null},
            {"kind":"weekly_all","percent":39,"resets_at":"invalid"},
            {"kind":"weekly_scoped","percent":34,"scope":{"model":{"display_name":"Fable"}}},
            {"kind":"weekly_scoped","percent":0,"scope":{"surface":{"display_name":"アプリ"}}},
            {"kind":"weekly_scoped","percent":100,"scope":{"model":{"display_name":"Sonnet"}}},
            {"kind":"weekly_all","percent":null},
            {"kind":"unknown","percent":10},
            null
        ]}}));
        assert_eq!(
            windows,
            vec![
                UsageWindow {
                    period: agent_protocol::operations::UsagePeriod::Minutes { minutes: 300 },
                    name: None,
                    remaining_percent: 28,
                    resets_at: Some(2000000000)
                },
                UsageWindow {
                    period: agent_protocol::operations::UsagePeriod::Minutes { minutes: 10080 },
                    name: None,
                    remaining_percent: 61,
                    resets_at: None
                },
                UsageWindow {
                    period: agent_protocol::operations::UsagePeriod::Minutes { minutes: 10080 },
                    name: Some("Fable".into()),
                    remaining_percent: 66,
                    resets_at: None
                },
                UsageWindow {
                    period: agent_protocol::operations::UsagePeriod::Minutes { minutes: 10080 },
                    name: Some("アプリ".into()),
                    remaining_percent: 100,
                    resets_at: None
                },
                UsageWindow {
                    period: agent_protocol::operations::UsagePeriod::Minutes { minutes: 10080 },
                    name: Some("Sonnet".into()),
                    remaining_percent: 0,
                    resets_at: None
                },
            ]
        );
        assert!(self::windows(&json!({"rate_limits":null})).is_empty());
        assert!(self::windows(&json!({"rate_limits":{"limits":[]}})).is_empty());
    }
}
