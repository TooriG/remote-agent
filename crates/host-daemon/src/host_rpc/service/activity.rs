//! Task activity shared with every client, Live Activities and APNs.
use super::*;

impl HostRpcService {
    pub(super) fn register_live_activity(
        &self,
        session: SessionId,
        params: &agent_protocol::live_activity::RegisterLiveActivity,
    ) -> Result<agent_protocol::live_activity::LiveActivityRegistration, Failure> {
        params
            .validate()
            .map_err(|error| Failure::new("invalid_params", error))?;
        let enabled = self
            .inner
            .router
            .apns()
            .is_some_and(|apns| apns.environment() == params.environment);
        if enabled {
            self.inner
                .router
                .register_live_activity(session, params)
                .map_err(|error| Failure::new("live_activity_failed", error))?;
        }
        Ok(agent_protocol::live_activity::LiveActivityRegistration { enabled })
    }

    pub(super) fn unregister_live_activity(
        &self,
        session: SessionId,
        params: &agent_protocol::live_activity::UnregisterLiveActivity,
    ) -> Result<agent_protocol::models::Empty, Failure> {
        let principal = self
            .inner
            .router
            .principal(session)
            .map_err(|error| Failure::new("connection_closed", error))?;
        if let Some(apns) = self.inner.router.apns() {
            apns.unregister(&principal, &params.activity_id);
        }
        Ok(agent_protocol::models::Empty {})
    }

    pub(super) async fn read_task_activity(
        &self,
    ) -> Result<agent_protocol::live_activity::TaskActivityState, Failure> {
        let revision = self.inner.router.task_activity_revision();
        let agents = self.inner.providers.all();
        let has_sources =
            !agents.is_empty() || !self.inner.providers.startup_errors().next().is_none();
        let listings =
            futures_util::future::join_all(agents.into_iter().map(|(_, agent)| async move {
                tokio::time::timeout(
                    SESSION_LIST_TIMEOUT,
                    session_pages(
                        agent.as_ref(),
                        "",
                        SessionListScope::Loaded,
                        100,
                        100,
                        None,
                        None,
                    )
                    .try_collect::<Vec<_>>(),
                )
                .await
            }))
            .await;
        let mut native = Vec::new();
        let mut successful = false;
        let mut complete = self.inner.providers.startup_errors().next().is_none();
        for result in listings {
            if let Ok(Ok(pages)) = result {
                let fresh = pages
                    .iter()
                    .flatten()
                    .all(|summary| summary.thread.list_stale != Some(true));
                successful |= fresh;
                complete &= fresh;
                for summary in pages.into_iter().flatten() {
                    if let Some(thread) = self.inner.conversations.summary(summary.thread)? {
                        native.push(thread);
                    }
                }
            } else {
                complete = false;
            }
        }
        let state = self.inner.router.task_activity(native, revision);
        if !successful && has_sources {
            return Err(Failure::new(
                "task_activity_unavailable",
                "タスクの状態を取得できません。",
            ));
        }
        if complete && let Some(apns) = self.inner.router.apns() {
            apns.activate(state.display.clone());
        }
        Ok(state)
    }
    pub async fn enable_apns(
        &self,
        path: &std::path::Path,
        host_name: &str,
        host_id: &str,
    ) -> anyhow::Result<()> {
        self.inner
            .router
            .set_apns(crate::apns::Apns::load(path, host_name, host_id).await?);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn live_activity_registration_falls_back_without_apns_and_rejects_malformed_tokens() {
        use super::*;
        use agent_protocol::live_activity::{
            LiveActivityRegistration, PushEnvironment, RegisterLiveActivity,
        };
        let directory = tempfile::tempdir().unwrap();
        let service = HostRpcService::new(
            [crate::providers::codex::Codex::new(Err("provider is offline".into())).into()],
            ProjectStore::new(directory.path().join("projects.json")),
        );
        let connection = service.open_session();
        let mut params = RegisterLiveActivity {
            activity_id: Some("activity".into()),
            allow_start: false,
            token: vec![1; 32],
            environment: PushEnvironment::Sandbox,
        };
        for configured in [false, true] {
            if configured {
                service
                    .inner
                    .router
                    .set_apns(Some(crate::apns::Apns::testing()));
                params.environment = PushEnvironment::Production;
            }
            let response = service
                .dispatch(connection.id(), &Call::RegisterLiveActivity(params.clone()))
                .await
                .unwrap();
            let Response::Success { result } = agent_protocol::protocol::decode::<
                Response<LiveActivityRegistration>,
            >(&response.initial)
            .unwrap() else {
                panic!("unconfigured or mismatched APNs must keep local updates available");
            };
            assert!(!result.enabled);
        }
        params.token.clear();
        let response = service
            .dispatch(connection.id(), &Call::RegisterLiveActivity(params))
            .await
            .unwrap();
        assert!(matches!(
            agent_protocol::protocol::decode::<Response<LiveActivityRegistration>>(
                &response.initial
            )
            .unwrap(),
            Response::Failure { .. }
        ));
    }
}
