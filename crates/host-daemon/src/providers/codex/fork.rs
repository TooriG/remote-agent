use super::Failure;
use serde_json::{Value, json};
use std::future::Future;

/// An explicit terminal boundary can disagree with Codex's persisted index
/// after interruption. Latest snapshots are supported even with an active
/// writer and do not resume or interrupt the source conversation.
pub(super) async fn request<F, Fut>(mut params: Value, mut call: F) -> Result<Value, Failure>
where
    F: FnMut(&'static str, Value) -> Fut,
    Fut: Future<Output = Result<Value, Failure>>,
{
    let failure = match call("thread/fork", params.clone()).await {
        Ok(value) => return Ok(value),
        Err(failure) => failure,
    };
    let turn = params["lastTurnId"].as_str().unwrap_or_default().to_owned();
    if turn.is_empty()
        || !failure.execution.as_ref().is_some_and(|error| {
            error.provider_code.as_deref() == Some("-32600")
                && error.message == format!("lastTurnId '{turn}' identifies an in-progress turn")
        })
    {
        return Err(failure);
    }
    let page = call(
        "thread/turns/list",
        json!({"threadId":params["threadId"],"limit":1,"sortDirection":"desc","itemsView":"summary"}),
    )
    .await?;
    if page["data"]
        .as_array()
        .is_none_or(|turns| turns.len() != 1 || turns[0]["id"].as_str() != Some(turn.as_str()))
    {
        return Err(Failure::new(
            "fork_turn_in_progress",
            "この返答はCodexで処理中のため、まだ分岐できません。完了後に再試行してください。",
        ));
    }
    params
        .as_object_mut()
        .expect("fork parameters")
        .remove("lastTurnId");
    let fork = call("thread/fork", params).await?;
    // Another writer may append a turn between the latest read and snapshot.
    // Never navigate to a fork containing turns beyond the selected boundary.
    if fork["thread"]["turns"]
        .as_array()
        .and_then(|turns| turns.last())
        .and_then(|turn| turn["id"].as_str())
        != Some(turn.as_str())
    {
        if let Some(id) = fork["thread"]["id"].as_str() {
            call("thread/archive", json!({"threadId":id})).await?;
        }
        return Err(Failure::new(
            "fork_boundary_changed",
            "分岐中に会話が更新されました。履歴を確認して、もう一度分岐してください。",
        ));
    }
    Ok(fork)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn busy() -> Failure {
        super::super::native_failure(
            &serde_json::value::to_raw_value(&json!({"code":-32600,"message":"lastTurnId 'turn' identifies an in-progress turn"})).unwrap(),
        )
    }

    async fn script(
        steps: Vec<(&'static str, Value, Result<Value, Failure>)>,
    ) -> Result<Value, Failure> {
        let mut steps = steps.into_iter();
        let result = request(params(), |method, params| {
            let (expected_method, expected_params, result) = steps.next().expect("extra request");
            assert_eq!(method, expected_method);
            assert_eq!(params, expected_params);
            std::future::ready(result)
        })
        .await;
        assert!(steps.next().is_none(), "missing request");
        result
    }

    fn params() -> Value {
        json!({"threadId":"source","lastTurnId":"turn","excludeTurns":false,"config":{"browser":"preserved"}})
    }
    fn latest_params() -> Value {
        let mut params = params();
        params.as_object_mut().unwrap().remove("lastTurnId");
        params
    }
    fn read_params() -> Value {
        json!({"threadId":"source","limit":1,"sortDirection":"desc","itemsView":"summary"})
    }

    #[tokio::test]
    async fn latest_in_progress_boundary_uses_snapshot_without_changing_source() {
        let fork = json!({"thread":{"id":"fork","turns":[{"id":"turn"}]}});
        assert_eq!(
            script(vec![
                ("thread/fork", params(), Err(busy())),
                (
                    "thread/turns/list",
                    read_params(),
                    Ok(json!({"data":[{"id":"turn","status":"inProgress"}]}))
                ),
                ("thread/fork", latest_params(), Ok(fork.clone())),
            ])
            .await
            .unwrap(),
            fork
        );
    }

    #[tokio::test]
    async fn historical_or_missing_boundary_never_falls_back_to_latest() {
        for page in [
            json!({"data":[{"id":"newer"}]}),
            json!({"data":[]}),
            json!({}),
        ] {
            assert_eq!(
                script(vec![
                    ("thread/fork", params(), Err(busy())),
                    ("thread/turns/list", read_params(), Ok(page)),
                ])
                .await
                .unwrap_err()
                .code,
                "fork_turn_in_progress"
            );
        }
    }

    #[tokio::test]
    async fn concurrent_new_turn_archives_only_the_unwanted_fork() {
        assert_eq!(
            script(vec![
                ("thread/fork", params(), Err(busy())),
                (
                    "thread/turns/list",
                    read_params(),
                    Ok(json!({"data":[{"id":"turn"}]}))
                ),
                (
                    "thread/fork",
                    latest_params(),
                    Ok(json!({"thread":{"id":"fork","turns":[{"id":"turn"},{"id":"newer"}]}}))
                ),
                ("thread/archive", json!({"threadId":"fork"}), Ok(json!({}))),
            ])
            .await
            .unwrap_err()
            .code,
            "fork_boundary_changed"
        );
    }

    #[tokio::test]
    async fn successful_exact_boundary_and_unrelated_errors_are_not_retried() {
        let fork = json!({"thread":{"id":"exact"}});
        assert_eq!(
            script(vec![("thread/fork", params(), Ok(fork.clone()))])
                .await
                .unwrap(),
            fork
        );
        assert_eq!(
            script(vec![(
                "thread/fork",
                params(),
                Err(Failure::new("other", "error"))
            )])
            .await
            .unwrap_err()
            .code,
            "other"
        );
    }
}
