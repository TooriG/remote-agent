use agent_protocol::operations::{UsagePeriod, UsageWindow};
use serde_json::Value;

pub(crate) fn windows(value: &Value) -> Vec<UsageWindow> {
    let buckets: Vec<&Value> = match value["rateLimitsByLimitId"].as_object() {
        Some(buckets) => buckets.values().collect(),
        None => vec![&value["rateLimits"]],
    };
    buckets
        .into_iter()
        .flat_map(|bucket| {
            ["primary", "secondary"].into_iter().filter_map(move |key| {
                let window = &bucket[key];
                let duration = window["windowDurationMins"].as_u64()?;
                if duration == 0 {
                    return None;
                }
                let name = bucket["limitName"]
                    .as_str()
                    .or(bucket["limitId"].as_str())
                    .filter(|name| *name != "codex" && !name.is_empty())
                    .map(str::to_owned);
                UsageWindow::from_used(
                    UsagePeriod::Minutes { minutes: duration },
                    name,
                    window["usedPercent"].as_f64()?,
                    window["resetsAt"].as_i64(),
                )
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn codex_windows_preserve_missing_values_and_normalize_remaining_usage() {
        let windows = windows(
            &json!({"rateLimitsByLimitId":{"codex":{"primary":{"usedPercent":28.2,"windowDurationMins":300,"resetsAt":1900000000},"secondary":null},"other":{"limitName":"Extra","primary":{"usedPercent":110,"windowDurationMins":10080}}}}),
        );
        assert_eq!(windows[0].remaining_percent, 71);
        assert_eq!(windows[0].period, UsagePeriod::Minutes { minutes: 300 });
        assert_eq!(windows[0].name, None);
        assert_eq!(windows[1].remaining_percent, 0);
        assert_eq!(windows[1].period, UsagePeriod::Minutes { minutes: 10080 });
        assert_eq!(windows[1].name.as_deref(), Some("Extra"));
    }
}
