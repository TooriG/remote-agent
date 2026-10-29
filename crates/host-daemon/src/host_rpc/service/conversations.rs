use super::*;
use crate::host_rpc::conversations::{Binding, Conversation, Delivery, Handoff, inline_context};
use agent_protocol::{
    handoff,
    session::{HistoryPage, SessionRef},
};

#[cfg(test)]
mod tests;

impl HostRpcService {
    pub(super) async fn read_handoff_history(
        &self,
        params: &agent_protocol::handoff::ReadHistory,
    ) -> Result<agent_protocol::handoff::Page, Failure> {
        let history = if let Some(row) = self.inner.conversations.get(&params.session)? {
            self.conversation_history(&row, true).await?
        } else {
            self.native_history(&params.session, true).await?
        };
        let messages = history
            .thread
            .turns
            .iter()
            .flatten()
            .flat_map(|turn| agent_protocol::handoff::Message::from_turn(turn))
            .collect::<Vec<_>>();
        Ok(agent_protocol::handoff::page(
            &messages,
            params.offset,
            params.limit,
            params.text_offset,
        ))
    }

    pub(super) async fn fork_conversation(
        &self,
        row: &Conversation,
        turn_id: &agent_protocol::ids::TurnId,
    ) -> Result<ThreadResponse, Failure> {
        use crate::host_rpc::conversations::Segment;
        let (native, turn) = row.source(turn_id)?;
        if !row.segments.iter().any(|segment| segment.session == native) {
            return Err(Failure::new(
                "invalid_turn",
                "The fork source is outside this conversation.",
            ));
        }
        let agent = self.inner.providers.get(native.provider)?;
        if !agent.capabilities().fork {
            return Err(Failure::new(
                "unsupported_operation",
                "The source provider cannot fork this response.",
            ));
        }
        let history = self.native_history(&native, false).await?;
        let boundary = history
            .thread
            .turns
            .as_deref()
            .unwrap_or_default()
            .iter()
            .rposition(|candidate| candidate.id.as_str() == turn)
            .ok_or_else(|| Failure::new("invalid_turn", "Fork source turn is unavailable."))?
            + 1;
        let segment_index = row
            .segments
            .iter()
            .rposition(|segment| {
                segment.session == native
                    && segment.from < boundary
                    && segment.end.is_none_or(|end| boundary <= end)
            })
            .ok_or_else(|| {
                Failure::new(
                    "invalid_turn",
                    "Fork source is outside the saved history range.",
                )
            })?;
        let response = self
            .start_thread(agent.as_ref(), |browser| {
                agent.fork(&native.id, &turn, browser)
            })
            .await?;
        let mut fork = Conversation::new(&response.thread)?;
        let target = fork.id.clone();
        let count = self
            .native_history(&target, false)
            .await?
            .thread
            .turns
            .as_ref()
            .map_or(0, Vec::len);
        fork.segments = row.segments[..=segment_index].to_vec();
        fork.segments.last_mut().expect("fork source segment").end = Some(boundary);
        fork.bindings[0].seen = fork.segments.len();
        fork.handoffs = row
            .handoffs
            .iter()
            .filter(|handoff| {
                fork.segments
                    .iter()
                    .any(|segment| segment.session == handoff.session)
            })
            .cloned()
            .map(|mut handoff| {
                handoff.receipt = None;
                handoff.submission = agent_protocol::session::SubmissionDelivery::Rejected;
                handoff
            })
            .collect();
        fork.segments.push(Segment {
            session: target,
            from: count,
            end: None,
        });
        self.inner.conversations.save(fork.clone())?;
        Self::conversation_page(self.conversation_history(&fork, false).await?, None, 5)
    }
    pub fn set_history_scope(&self, directory: std::path::PathBuf, isolated: bool) {
        let _ = self.inner.history_scope.set((directory, isolated));
    }
    /// Native paging is consumed on the Host. Never export just the client's
    /// five visible turns as if they were the entire conversation.
    pub(super) async fn native_history(
        &self,
        session: &SessionRef,
        hydrate: bool,
    ) -> Result<ThreadResponse, Failure> {
        let agent = self.inner.providers.get(session.provider)?;
        let mut response = agent.open(&session.id, 100, true).await?;
        if response
            .thread
            .history_read_state
            .as_ref()
            .is_some_and(|state| {
                matches!(
                    state.kind,
                    agent_protocol::session::HistoryReadKind::Incomplete
                        | agent_protocol::session::HistoryReadKind::Unavailable
                )
            })
        {
            return Err(Failure::new(
                "incomplete_history",
                "The provider could not read the source history completely.",
            ));
        }
        if response.thread.id.as_ref() != Some(session) {
            return Err(Failure::new(
                "invalid_session",
                "native session identity changed",
            ));
        }
        let mut seen = std::collections::HashSet::new();
        while let Some(cursor) = response.thread.history_cursor.clone() {
            if !seen.insert(cursor.clone()) {
                return Err(Failure::new("invalid_history", "history cursor repeated"));
            }
            agent
                .read_history(&session.id, &cursor, true)
                .await?
                .prepend_to(&mut response.thread);
        }
        if response.thread.history_has_more == Some(true) {
            return Err(Failure::new(
                "incomplete_history",
                "The provider did not supply complete history.",
            ));
        }
        if hydrate {
            for turn in response.thread.turns.iter_mut().flatten() {
                let turn = Arc::make_mut(turn);
                if turn.items_summary {
                    turn.items = Some(agent.read_turn_items(&session.id, &turn.id).await?);
                    turn.items_summary = false;
                }
                for item in turn.items.iter_mut().flatten() {
                    // Handoffs deliberately omit reasoning and native tool
                    // state. Their deferred bodies need not be recovered.
                    if item.is_deferred()
                        && matches!(
                            item.body(),
                            agent_protocol::items::ItemBody::UserMessage { .. }
                                | agent_protocol::items::ItemBody::AssistantText { .. }
                                | agent_protocol::items::ItemBody::CommandExecution { .. }
                                | agent_protocol::items::ItemBody::FileChange { .. }
                                | agent_protocol::items::ItemBody::Plan { .. }
                        )
                    {
                        let output = agent
                            .read_item(&op::ReadItem {
                                thread_id: session.clone(),
                                turn_id: turn.id.clone(),
                                item_id: item.id.clone(),
                            })
                            .await?;
                        if output.item.is_deferred() {
                            return Err(Failure::new(
                                "incomplete_history",
                                "Historical item could not be read.",
                            ));
                        }
                        *item = Arc::new(output.item);
                    }
                }
            }
        }
        Ok(response)
    }

    pub(super) async fn conversation_history(
        &self,
        row: &Conversation,
        hydrate: bool,
    ) -> Result<ThreadResponse, Failure> {
        let mut native = HashMap::new();
        for segment in &row.segments {
            if !native.contains_key(&segment.session) {
                native.insert(
                    segment.session.clone(),
                    self.native_history(&segment.session, hydrate).await?,
                );
            }
        }
        let mut response = if let Some(response) = native.get(&row.active) {
            response.clone()
        } else {
            self.inner
                .providers
                .get(row.active.provider)?
                .open(&row.active.id, 1, false)
                .await?
        };
        let mut turns = Vec::new();
        (response.model, response.effort, response.service_tier) =
            self.inner.model_selections.resolve(
                &row.active,
                response.model.as_ref(),
                response.effort.as_deref(),
                response.service_tier.as_deref(),
            )?;
        for segment in &row.segments {
            let source = native.get(&segment.session).expect("segment loaded");
            let source = source.thread.turns.as_deref().unwrap_or_default();
            let end = segment.end.unwrap_or(source.len());
            let slice = source.get(segment.from..end).ok_or_else(|| {
                Failure::new(
                    "history_changed",
                    "Native history was shortened. Saved conversation boundaries are unavailable.",
                )
            })?;
            let supports_fork = self
                .inner
                .providers
                .get(segment.session.provider)?
                .capabilities()
                .fork;
            turns.extend(slice.iter().map(|turn| {
                let mut turn = row.turn(&segment.session, (**turn).clone());
                turn.supports_fork = Some(supports_fork);
                Arc::new(turn)
            }));
        }
        row.describe(&mut response.thread);
        response.thread.turns = Some(turns);
        let capabilities = self
            .inner
            .providers
            .get(row.active.provider)?
            .capabilities();
        response.thread.capabilities = Some(agent_protocol::session::Capabilities {
            rename: true,
            fork: row.segments.iter().any(|segment| {
                self.inner
                    .providers
                    .get(segment.session.provider)
                    .is_ok_and(|agent| agent.capabilities().fork)
            }),
            ..capabilities
        });
        response.thread.history_cursor = None;
        response.thread.history_has_more = Some(false);
        Ok(response)
    }

    pub(super) fn conversation_page(
        mut response: ThreadResponse,
        end: Option<usize>,
        limit: usize,
    ) -> Result<ThreadResponse, Failure> {
        let turns = response.thread.turns.take().unwrap_or_default();
        let end = end.unwrap_or(turns.len());
        let start = end.saturating_sub(limit);
        response.thread.turns = Some(
            turns
                .get(start..end)
                .ok_or_else(|| {
                    Failure::new(
                        "invalid_history_cursor",
                        "conversation history cursor is no longer valid",
                    )
                })?
                .to_vec(),
        );
        response.thread.history_cursor = (start > 0).then(|| start.to_string());
        response.thread.history_has_more = Some(start > 0);
        response.thread.history_read_state = Some(agent_protocol::session::HistoryReadState::new(
            if start > 0 {
                agent_protocol::session::HistoryReadKind::Partial
            } else {
                agent_protocol::session::HistoryReadKind::Complete
            },
            vec![],
        ));
        response.thread.history_limit = Some(limit as u64);
        agent_protocol::models::defer_item_details(
            response.thread.turns.as_mut().unwrap(),
            agent_protocol::models::MAX_INLINE_ITEM_BYTES,
        );
        Ok(response)
    }

    pub(super) async fn switch_provider(
        &self,
        root: &SessionRef,
        input: &op::Submission,
    ) -> Result<op::SubmissionReceipt, Failure> {
        let previous = self.inner.conversations.get(root)?;
        let active = previous
            .as_ref()
            .map_or_else(|| root.clone(), |row| row.active.clone());
        let source = self.native_history(&active, false).await?;
        let selected = input
            .model
            .as_ref()
            .or(source.model.as_ref())
            .ok_or_else(|| {
                Failure::new("model_required", "Select a model for the target provider.")
            })?;
        let observed = self
            .inner
            .router
            .overlay_execution(root, source.thread.clone());
        let unfinished = observed
            .submissions
            .iter()
            .filter(|(id, _)| *id != &input.client_user_message_id)
            .map(|(_, delivery)| delivery);
        if worktree_active(
            observed.status,
            observed.turns.as_deref().unwrap_or_default(),
            !observed.requests.is_empty(),
            unfinished,
        ) {
            return Err(Failure::new(
                "provider_switch_busy",
                "現在の応答・確認・送信が確定してからAIを切り替えてください。",
            ));
        }
        if source.thread.can_accept_direct_input == Some(false)
            || (source.thread.parent_id.is_some()
                && source.thread.can_accept_direct_input != Some(true))
        {
            return Err(Failure::new(
                "provider_switch_unavailable",
                "サブエージェントのAIは切り替えられません。親の会話から指示してください。",
            ));
        }
        let source_agent = self.inner.providers.get(active.provider)?;
        let deadline = tokio::time::Instant::now() + SESSION_LIST_TIMEOUT;
        let children = session_pages(
            source_agent.as_ref(),
            "",
            SessionListScope::Descendants(&active.id),
            100,
            100,
            Some(deadline),
            None,
        )
        .try_collect::<Vec<_>>()
        .await?;
        for child in children.into_iter().flatten() {
            let thread = if let Some(id) = &child.thread.id {
                self.inner
                    .router
                    .overlay_execution(id, child.thread.clone())
            } else {
                child.thread
            };
            if worktree_active(
                thread.status,
                thread.turns.as_deref().unwrap_or_default(),
                !thread.requests.is_empty(),
                thread.submissions.values(),
            ) {
                return Err(Failure::new(
                    "provider_switch_busy",
                    "サブエージェントの応答・確認が終わってからAIを切り替えてください。",
                ));
            }
        }
        let agent = self.inner.providers.get(selected.provider)?;
        agent.validate_create()?;
        let mut row = previous.unwrap_or(Conversation::new(&source.thread)?);
        row.seal(source.thread.turns.as_ref().map_or(0, Vec::len))?;
        let mut target = row
            .binding(selected.provider)
            .map(|binding| binding.session.clone());
        // An ambiguous history injection must never be replayed into the same
        // native session. Recover the logical history into a fresh binding.
        if target.as_ref().is_some_and(|target| {
            row.handoffs
                .iter()
                .any(|handoff| &handoff.session == target && handoff.delivery == Delivery::Pending)
        }) {
            target = None;
        }
        let mut target_history = if let Some(target) = &target {
            match self.native_history(target, false).await {
                Ok(history) => history,
                Err(_) => self.create_handoff_target(&row, selected).await?,
            }
        } else {
            self.create_handoff_target(&row, selected).await?
        };
        let target = target_history
            .thread
            .id
            .clone()
            .ok_or_else(|| Failure::new("invalid_session", "target session ID is missing"))?;
        if target_history.thread.status == agent_protocol::models::SessionStatus::Running
            || !target_history.thread.requests.is_empty()
        {
            return Err(Failure::new(
                "provider_switch_busy",
                "切り替え先のAIがまだ実行中です。完了を待ってください。",
            ));
        }
        if !row.bindings.iter().any(|binding| binding.session == target) {
            row.bindings.push(Binding {
                session: target.clone(),
                seen: 0,
            });
        }
        // Persist the native binding before attempting delivery; events and
        // reconnection must resolve it to the same public conversation.
        self.inner.conversations.save(row.clone())?;
        let last_seen = row
            .bindings
            .iter()
            .find(|binding| binding.session == target)
            .expect("binding saved")
            .seen;
        let mut history_row = row.clone();
        history_row.segments = row.segments.iter().skip(last_seen).cloned().collect();
        let history = self.conversation_history(&history_row, true).await?;
        let messages = history
            .thread
            .turns
            .iter()
            .flatten()
            .flat_map(|turn| handoff::Message::from_turn(turn))
            .collect::<Vec<_>>();
        let coverage = format!(
            "Provider context handoff. Conversation: {root}. Covered history segments: {last_seen}..{}.\nRead omitted history on this Host with the BEX history command shown below.\n{}",
            row.segments.len(),
            self.history_command(root)
        );
        // Bound only the history we add. Display history includes material the
        // provider has already compacted; it cannot measure live model context.
        // The native provider owns token accounting and automatic compaction.
        let context = handoff::select(&messages, &coverage, handoff::DEFAULT_BUDGET)
            .map_err(|error| Failure::new("handoff_context_limit", error))?;
        source_agent
            .unload(&active.id)
            .await
            .map_err(Failure::before_submission)?;
        let index = row.handoffs.len();
        row.handoffs.push(Handoff {
            session: target.clone(),
            input_id: input.client_user_message_id.clone(),
            context,
            delivery: Delivery::Pending,
            receipt: None,
            submission: agent_protocol::session::SubmissionDelivery::Rejected,
        });
        self.inner.conversations.save(row.clone())?;
        let injected = agent
            .inject_history(&target.id, &row.handoffs[index].context)
            .await
            .map_err(Failure::before_submission)?;
        if injected {
            row.handoffs[index].delivery = Delivery::Injected;
            let seen = row.segments.len();
            row.bindings
                .iter_mut()
                .find(|binding| binding.session == target)
                .expect("target binding")
                .seen = seen;
            self.inner.conversations.save(row.clone())?;
            target_history = self.native_history(&target, false).await?;
        }
        let from = target_history.thread.turns.as_ref().map_or(0, Vec::len);
        row.activate(target.clone(), from);
        self.inner.conversations.save(row.clone())?;
        let mut native_input = input.clone();
        native_input.thread_id = target.clone();
        if !injected {
            native_input.input.insert(
                0,
                op::Input::Text {
                    text: inline_context(&row.handoffs[index].context),
                },
            );
        }
        row.handoffs[index].submission = agent_protocol::session::SubmissionDelivery::Sending;
        self.inner.conversations.save(row.clone())?;
        let result = self.submit_native(root, &target, &native_input).await;
        match result {
            Ok(mut receipt) => {
                if !injected {
                    row.handoffs[index].delivery = Delivery::Inline;
                }
                receipt.turn_id = receipt.turn_id.map(|id| row.identity(&target, &id).into());
                row.handoffs[index].receipt = Some(receipt.clone());
                row.handoffs[index].submission =
                    agent_protocol::session::SubmissionDelivery::Accepted {
                        turn_id: receipt.turn_id.clone(),
                    };
                self.inner.conversations.save(row)?;
                Ok(receipt)
            }
            Err(error) => {
                // A refused prompt has no tool effects. Keep delivered native
                // history, restore the source binding, and retain its open range.
                if error.delivery == agent_protocol::error::Delivery::NotSent {
                    row.restore_source(active.clone());
                    if !injected {
                        row.handoffs[index].delivery = Delivery::Ready;
                        row.bindings
                            .iter_mut()
                            .find(|binding| binding.session == target)
                            .expect("target binding")
                            .seen = last_seen;
                    }
                    row.handoffs[index].submission =
                        agent_protocol::session::SubmissionDelivery::Rejected;
                    self.inner.conversations.save(row)?;
                } else {
                    row.handoffs[index].submission =
                        agent_protocol::session::SubmissionDelivery::Unknown;
                    self.inner.conversations.save(row)?;
                }
                Err(error)
            }
        }
    }

    async fn create_handoff_target(
        &self,
        row: &Conversation,
        model: &agent_protocol::models::ModelRef,
    ) -> Result<ThreadResponse, Failure> {
        // Reuse the exact workspace; creating a second worktree would sever the
        // relationship between the transferred conversation and its files.
        self.inner
            .providers
            .get(model.provider)?
            .create(
                &row.cwd,
                Some(&model.id),
                self.browser_config(&row.id.to_string())?,
            )
            .await
    }

    fn history_command(&self, root: &SessionRef) -> String {
        let program = std::env::current_exe().unwrap_or_else(|_| "host-daemon".into());
        let quote = |text: &str| {
            format!(
                "'{}'",
                text.replace('\'', if cfg!(windows) { "''" } else { "'\\''" })
            )
        };
        let scope = self
            .inner
            .history_scope
            .get()
            .map(|(path, isolated)| {
                format!(
                    "{} --state-dir {}",
                    if *isolated { " --isolated" } else { "" },
                    quote(&path.to_string_lossy())
                )
            })
            .unwrap_or_default();
        format!(
            "{}{}{} history {} --offset 0 --limit 20\nContinue with --offset next_offset, or --offset position --limit 1 --text-offset next_text_offset to read a long item.",
            if cfg!(windows) { "& " } else { "" },
            quote(&program.to_string_lossy()),
            scope,
            quote(&root.to_string())
        )
    }

    pub(super) async fn read_conversation_page(
        &self,
        row: &Conversation,
        cursor: &str,
    ) -> Result<HistoryPage, Failure> {
        let end = cursor.parse().map_err(|_| {
            Failure::new(
                "invalid_history_cursor",
                "invalid conversation history cursor",
            )
        })?;
        let page =
            Self::conversation_page(self.conversation_history(row, false).await?, Some(end), 5)?;
        Ok(HistoryPage {
            turns: page.thread.turns.unwrap_or_default(),
            next_cursor: page.thread.history_cursor,
        })
    }
}
