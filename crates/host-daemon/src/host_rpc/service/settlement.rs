use super::*;

impl HostRpcService {
    pub(super) async fn set_session_settled(
        &self,
        params: &op::SetSessionSettled,
    ) -> Result<op::SessionSettlement, Failure> {
        params
            .thread_id
            .validate()
            .map_err(|error| Failure::new("invalid_params", error))?;
        if params.settled {
            let row = self.inner.conversations.get(&params.thread_id)?;
            let native = row.as_ref().map_or(&params.thread_id, |row| &row.active);
            let mut thread = if row.as_ref().is_some_and(|row| row.bindings.is_empty()) {
                Thread::default()
            } else {
                let thread = self
                    .inner
                    .providers
                    .get(native.provider)?
                    .state(&native.id)
                    .await?
                    .response
                    .thread;
                if thread.id.as_ref() != Some(native) {
                    return Err(Failure::new(
                        "invalid_thread_state",
                        "Provider returned another conversation's state",
                    ));
                }
                thread
            };
            if let Some(row) = &row {
                row.describe(&mut thread);
            }
            let unsafe_input =
                thread.submissions.values().any(|delivery| {
                    *delivery != agent_protocol::session::SubmissionDelivery::Rejected
                }) || row.as_ref().is_some_and(|row| {
                    row.handoffs.iter().any(|handoff| {
                        matches!(
                            handoff.submission,
                            agent_protocol::session::SubmissionDelivery::Sending
                                | agent_protocol::session::SubmissionDelivery::Unknown
                        )
                    })
                });
            return self.inner.router.settle_if_idle(
                &params.thread_id,
                thread.status,
                thread.active_turn_id().is_some(),
                !thread.requests.is_empty(),
                unsafe_input
                    || thread.preparation.as_ref().is_some_and(|preparation| {
                        preparation.status.is_active()
                            || preparation.status
                                == agent_protocol::session::PreparationStatus::Unknown
                    }),
            );
        }
        self.inner
            .router
            .set_settled(&params.thread_id, params.settled)
    }
}
