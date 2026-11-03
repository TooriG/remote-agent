//! Conversation reads from Host transcripts, and their one-time native import.
use super::*;
use crate::host_rpc::conversations::Conversation;
use agent_protocol::session::SessionRef;
use conversations::HistoryUse;

/// Every transcript window, including older pages, holds at most this many turns.
pub(super) const WINDOW_TURNS: usize = 10;

pub(super) enum Import {
    Running,
    Failed,
}

pub(super) fn storage_failure(error: String) -> Failure {
    Failure::new("transcript_storage_failed", error)
}

// Saved conversations already paged by numeric start positions. Other
// conversations mark positions so that native cursors never alias them.
fn cursor(row: bool, start: usize) -> String {
    if row {
        start.to_string()
    } else {
        format!("t{start}")
    }
}

/// The start position a transcript cursor names, or None for a native cursor.
pub(super) fn position(row: bool, cursor: &str) -> Option<usize> {
    if row {
        cursor.parse().ok()
    } else {
        cursor.strip_prefix('t')?.parse().ok()
    }
}

impl HostRpcService {
    /// A stored window with Host-owned metadata applied, or None before import.
    pub(super) fn stored_history(
        &self,
        target: &SessionRef,
        row: Option<&Conversation>,
        end: Option<usize>,
        limit: usize,
        include_activity: bool,
    ) -> Result<Option<ThreadResponse>, Failure> {
        if row.is_some_and(|row| row.startup.is_some()) {
            return Ok(None);
        }
        let Some(window) = self
            .inner
            .transcripts
            .window(target, end, limit)
            .map_err(storage_failure)?
        else {
            return Ok(None);
        };
        let mut response = window.response;
        let native = row.map_or(target, |row| &row.active);
        response.thread.capabilities = Some(match row {
            Some(row) => {
                row.describe(&mut response.thread);
                self.conversation_capabilities(
                    row.active.provider,
                    row.segments.iter().map(|segment| segment.session.provider),
                )
            }
            None => self
                .inner
                .providers
                .get(target.provider)
                .map(|agent| agent.capabilities())
                .unwrap_or_default(),
        });
        // Accepted turns update Host selections; the imported snapshot only
        // describes the conversation before the Host ran it.
        let saved = self
            .inner
            .model_selections
            .resolve(native, None, None, None)?;
        response.model = saved.model.or(response.model);
        response.effort = saved.effort.or(response.effort);
        response.service_tier = saved.service_tier.or(response.service_tier);
        if !include_activity {
            for turn in response.thread.turns.iter_mut().flatten() {
                let provider = turn.provider.unwrap_or(target.provider);
                let Ok(agent) = self.inner.providers.get(provider) else {
                    continue;
                };
                if let Some(items) = agent.summary_items(turn.items.as_deref().unwrap_or_default())
                {
                    let turn = Arc::make_mut(turn);
                    turn.items = Some(items);
                    turn.items_summary = true;
                }
            }
        }
        let more = window.start > 0;
        response.thread.history_cursor = more.then(|| cursor(row.is_some(), window.start));
        response.thread.history_has_more = Some(more);
        response.thread.history_read_state = Some(agent_protocol::session::HistoryReadState::new(
            if more {
                agent_protocol::session::HistoryReadKind::Partial
            } else {
                agent_protocol::session::HistoryReadKind::Complete
            },
            Vec::new(),
        ));
        Ok(Some(response))
    }

    /// Copy native history in the background, once per conversation at a time.
    /// A failed import waits for a re-import or a Host restart; an unavailable
    /// provider, often one still starting, lets the next open try again.
    pub(super) fn import_transcript(&self, target: SessionRef, retry: bool) {
        {
            let mut imports = self.inner.imports.lock().unwrap();
            match imports.get(&target) {
                Some(Import::Running) => return,
                Some(Import::Failed) if !retry => return,
                _ => {}
            }
            imports.insert(target.clone(), Import::Running);
        }
        let service = self.clone();
        tokio::spawn(async move {
            let started = std::time::Instant::now();
            let result = match service.import_history(&target).await {
                Ok(response) => {
                    let transcripts = service.inner.transcripts.clone();
                    let key = target.clone();
                    tokio::task::spawn_blocking(move || transcripts.import(&key, response))
                        .await
                        .map_err(|error| error.to_string())
                        .and_then(|result| result)
                        .map_err(storage_failure)
                }
                Err(error) => Err(error),
            };
            let mut imports = service.inner.imports.lock().unwrap();
            match result {
                Ok(()) => {
                    imports.remove(&target);
                    tracing::info!(target: "agent", operation = "transcript.import",
                        message = %format_args!("provider={} total_ms={}", target.provider.key(), started.elapsed().as_millis()));
                }
                Err(error) => {
                    if error.code == "provider_unavailable" {
                        imports.remove(&target);
                    } else {
                        imports.insert(target, Import::Failed);
                    }
                    tracing::warn!(target: "agent", operation = "transcript.import", message = %error.message);
                }
            }
        });
    }

    /// History in public identity, keeping the text of every user and
    /// assistant message.
    async fn import_history(&self, target: &SessionRef) -> Result<ThreadResponse, Failure> {
        let mut response = if let Some(row) = self.inner.conversations.get(target)? {
            if row.startup.is_some() {
                return Err(Failure::new(
                    "preparation_active",
                    "A conversation is imported after its preparation finishes.",
                ));
            }
            self.conversation_history(&row, HistoryUse::Import).await?
        } else {
            self.native_history(target, HistoryUse::Import).await?
        };
        // Past activity is not imported; turns the Host runs keep every item.
        for turn in response.thread.turns.iter_mut().flatten() {
            if let Some(items) = &mut Arc::make_mut(turn).items {
                items.retain(|item| text(item.body()));
            }
        }
        Ok(response)
    }
}

/// The conversation's text: what an import copies from native history.
pub(super) fn text(body: &agent_protocol::items::ItemBody) -> bool {
    matches!(
        body,
        agent_protocol::items::ItemBody::UserMessage { .. }
            | agent_protocol::items::ItemBody::AssistantText { .. }
    )
}
