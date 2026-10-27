//! Worktree listing and removal. Removal waits until no provider can be running there.
use super::*;

impl HostRpcService {
    pub(super) async fn worktree_list(
        &self,
    ) -> Result<Vec<agent_protocol::models::Worktree>, Failure> {
        let mut worktrees = self
            .inner
            .worktrees
            .list()
            .await
            .map_err(|error| Failure::new("worktree_list_failed", error))?;
        if worktrees.is_empty() {
            return Ok(worktrees);
        }
        for (_, error) in self.inner.providers.startup_errors() {
            for worktree in &mut worktrees {
                worktree.blocked_reason =
                    Some(format!("稼働状況を確認できないため削除できません: {error}"));
            }
        }
        let mut threads = Vec::new();
        for (_, agent) in self.inner.providers.all() {
            let pages = session_pages(
                agent.as_ref(),
                "",
                SessionListScope::All,
                100,
                100,
                None,
                None,
            );
            futures_util::pin_mut!(pages);
            while let Some(result) = pages.next().await {
                match result {
                    Ok(page) => threads.extend(page.into_iter().map(|summary| summary.thread)),
                    Err(error) if error.code == "invalid_session_list" => return Err(error),
                    Err(error) => {
                        for w in &mut worktrees {
                            w.blocked_reason =
                                Some(format!("稼働状況を確認できないため削除できません: {error}"));
                        }
                        break;
                    }
                }
            }
        }
        // Providers can omit a first, still-running turn from their history list.
        // Resolve every retained execution before allowing any checkout removal.
        for target in self.inner.router.execution_targets() {
            if threads
                .iter()
                .any(|thread| thread.id.as_ref() == Some(&target))
            {
                continue;
            }
            let response = self
                .inner
                .providers
                .get(target.provider)?
                .state(&target.id)
                .await?
                .response;
            if response.thread.id.as_ref() != Some(&target) {
                return Err(Failure::new(
                    "invalid_thread",
                    "native session identity changed",
                ));
            }
            threads.push(response.thread);
        }
        for thread in threads {
            let thread = match thread.id.clone() {
                Some(id) => self.inner.router.overlay_execution(&id, thread),
                None => thread,
            };
            let active = worktree_active(
                thread.status,
                thread.turns.as_deref().unwrap_or_default(),
                !thread.requests.is_empty(),
                thread.submissions.values(),
            );
            let Some(cwd) = thread.cwd.as_deref().filter(|cwd| !cwd.trim().is_empty()) else {
                if active {
                    return Err(Failure::new(
                        "worktree_activity_unknown",
                        "実行中の会話の作業場所を確認できないため削除できません。",
                    ));
                }
                continue;
            };
            let cwd = tokio::fs::canonicalize(cwd)
                .await
                .unwrap_or_else(|_| std::path::PathBuf::from(cwd));
            let cwd = dunce::simplified(&cwd);
            let matching: Vec<_> = worktrees
                .iter()
                .enumerate()
                .filter_map(|(index, tree)| cwd.starts_with(&tree.path).then_some(index))
                .collect();
            if matching.is_empty() {
                continue;
            }
            // Resolve provider-specific uncertainty once per matching session,
            // reusing the history scan instead of rereading it for every tree.
            let idle_warning = if !active && let Some(id) = &thread.id {
                self.inner
                    .providers
                    .get(id.provider)?
                    .workspace_idle_warning(&id.id)
                    .await
            } else {
                None
            };
            let display = self.inner.conversations.summary(thread)?;
            for index in matching {
                let worktree = &mut worktrees[index];
                if active {
                    worktree.blocked_reason = Some("このワークツリーで作業を実行中です。完了または停止してから削除してください。".into());
                } else if let Some(warning) = idle_warning {
                    worktree.blocked_reason = Some(format!(
                        "稼働状況を確認できないため削除できません: {warning}"
                    ));
                }
                if let Some(thread) = &display
                    && let Some(id) = &thread.id
                {
                    worktree
                        .threads
                        .push(agent_protocol::models::WorktreeThread {
                            id: id.clone(),
                            name: thread
                                .name
                                .clone()
                                .filter(|name| !name.is_empty())
                                .or_else(|| {
                                    thread
                                        .preview
                                        .as_deref()
                                        .map(|preview| preview.chars().take(120).collect())
                                })
                                .unwrap_or_else(|| "新しいチャット".into()),
                            active,
                        });
                }
            }
        }
        for worktree in &mut worktrees {
            if self
                .inner
                .terminals
                .in_use(std::path::Path::new(&worktree.path))
            {
                worktree.blocked_reason =
                    Some("このワークツリーのターミナルを閉じてから削除してください。".into());
            }
        }
        Ok(worktrees)
    }
    pub(super) async fn remove_worktree(&self, path: String) -> Result<(), Failure> {
        let _exclusive = self.inner.worktree_access.write().await;
        let entries = self.worktree_list().await?;
        let entry = entries
            .iter()
            .find(|entry| entry.path == path)
            .ok_or_else(|| {
                Failure::new(
                    "worktree_remove_failed",
                    "Bexが作成したワークツリーではありません。",
                )
            })?;
        if let Some(reason) = &entry.blocked_reason {
            return Err(Failure::new("worktree_remove_failed", reason));
        }
        self.inner
            .worktrees
            .remove(path, false)
            .await
            .map_err(|error| Failure::new("worktree_remove_failed", error))
    }
    pub(crate) async fn cleanup_merged_worktrees(&self) -> anyhow::Result<()> {
        if !self.inner.worktrees.settings(None).await?.delete_merged {
            return Ok(());
        }
        let _exclusive = self.inner.worktree_access.write().await;
        let entries = self.worktree_list().await?;
        let statuses = crate::worktrees::directory_statuses(
            entries
                .iter()
                .filter(|entry| entry.blocked_reason.is_none())
                .map(|entry| crate::worktrees::StatusSource {
                    cwd: entry.path.clone(),
                    checkout: None,
                    repository: None,
                    branch: None,
                })
                .collect(),
        )
        .await?;
        for (source, status) in statuses {
            if status == agent_protocol::models::WorktreeStatus::Merged
                && let Err(error) = self.inner.worktrees.remove(source.cwd, true).await
            {
                tracing::warn!(target: "agent", operation = "host.worktree.cleanup", message = %error);
            }
        }
        Ok(())
    }
}

pub(super) fn worktree_active<'a>(
    status: agent_protocol::models::SessionStatus,
    turns: &[Arc<agent_protocol::models::Turn>],
    has_requests: bool,
    mut submissions: impl Iterator<Item = &'a agent_protocol::session::SubmissionDelivery>,
) -> bool {
    use agent_protocol::{
        execution::TurnStatus, models::SessionStatus, session::SubmissionDelivery,
    };
    status == SessionStatus::Running
        || has_requests
        || turns.iter().any(|turn| turn.status == TurnStatus::Running)
        || submissions.any(|delivery| match delivery {
            SubmissionDelivery::Rejected => false,
            SubmissionDelivery::Accepted { turn_id: Some(id) } => {
                // Completed receipts can remain for replay after interruption.
                // Only a known finished turn proves that input no longer owns work.
                !turns.iter().any(|turn| {
                    &turn.id == id
                        && matches!(
                            turn.status,
                            TurnStatus::Completed | TurnStatus::Failed | TurnStatus::Interrupted
                        )
                })
            }
            _ => true,
        })
}

#[cfg(test)]
mod tests {
    #[test]
    fn worktree_activity_requires_finished_delivery_and_no_other_live_work() {
        use super::worktree_active;
        use agent_protocol::{
            execution::TurnStatus, models::SessionStatus, session::SubmissionDelivery,
        };
        let receipt = SubmissionDelivery::Accepted {
            turn_id: Some("accepted".into()),
        };
        for status in [
            TurnStatus::Running,
            TurnStatus::Unknown,
            TurnStatus::Completed,
            TurnStatus::Failed,
            TurnStatus::Interrupted,
        ] {
            let turns = [std::sync::Arc::new(agent_protocol::models::Turn {
                id: "accepted".into(),
                status,
                ..Default::default()
            })];
            let busy = |session, requests, deliveries: &[SubmissionDelivery]| {
                worktree_active(session, &turns, requests, deliveries.iter())
            };
            assert_eq!(
                busy(SessionStatus::Idle, false, &[]),
                status == TurnStatus::Running
            );
            assert_eq!(
                busy(SessionStatus::Idle, false, std::slice::from_ref(&receipt)),
                matches!(status, TurnStatus::Running | TurnStatus::Unknown)
            );
            assert!(busy(SessionStatus::Running, false, &[]));
            assert!(busy(SessionStatus::Idle, true, &[]));
            for unresolved in [
                SubmissionDelivery::Sending,
                SubmissionDelivery::Unknown,
                SubmissionDelivery::Accepted { turn_id: None },
                SubmissionDelivery::Accepted {
                    turn_id: Some("other".into()),
                },
            ] {
                assert!(busy(
                    SessionStatus::Idle,
                    false,
                    &[receipt.clone(), unresolved]
                ));
            }
        }
        assert!(!worktree_active(
            SessionStatus::Idle,
            &[],
            false,
            [SubmissionDelivery::Rejected].iter()
        ));
        assert!(worktree_active(
            SessionStatus::Idle,
            &[],
            false,
            [receipt].iter()
        ));
    }

    #[tokio::test]
    async fn worktree_listing_blocks_only_checkouts_with_unverified_claude_activity() {
        use super::*;
        let root = tempfile::tempdir().unwrap();
        let repository = root.path().join("repository");
        std::fs::create_dir(&repository).unwrap();
        crate::git::text(&repository, &["init", "--quiet", "--initial-branch=main"]).unwrap();
        crate::git::text(
            &repository,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--allow-empty",
                "--quiet",
                "-m",
                "fixture",
            ],
        )
        .unwrap();
        let native = root.path().join("native");
        let project = native.join("projects/example");
        std::fs::create_dir_all(&project).unwrap();
        let claude = crate::providers::Claude::load(
            root.path().join("unused-cli"),
            root.path().join("state"),
            Some(native),
        )
        .await
        .unwrap();
        let service = HostRpcService::new(
            [claude.into()],
            ProjectStore::new(root.path().join("projects.json")),
        );
        service
            .inner
            .worktrees
            .settings(Some(agent_protocol::models::WorktreeSettings {
                create_on_new_session: true,
                ..Default::default()
            }))
            .await
            .unwrap();
        let first = service
            .inner
            .worktrees
            .prepare(repository.to_str())
            .await
            .unwrap()
            .unwrap();
        let other = service
            .inner
            .worktrees
            .prepare(repository.to_str())
            .await
            .unwrap()
            .unwrap();
        let id = "12345678-1234-4234-8234-123456789abc";
        let transcript = project.join(format!("{id}.jsonl"));
        let history = include_str!("../../../tests/fixtures/claude-2.1.266.jsonl")
            .lines()
            .map(|line| {
                let mut entry: serde_json::Value = serde_json::from_str(line).unwrap();
                entry["cwd"] = serde_json::json!(first);
                entry.to_string()
            })
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&transcript, history).unwrap();
        let entries = service.worktree_list().await.unwrap();
        let first_entry = entries
            .iter()
            .find(|entry| std::path::Path::new(&entry.path) == first)
            .unwrap();
        assert_eq!(first_entry.threads[0].id.id, id);
        assert!(!first_entry.threads[0].active);
        assert!(
            first_entry
                .blocked_reason
                .as_ref()
                .unwrap()
                .contains("native transcript does not expose external process activity")
        );
        let other_entry = entries
            .iter()
            .find(|entry| std::path::Path::new(&entry.path) == other)
            .unwrap();
        assert!(other_entry.blocked_reason.is_none());
        assert!(
            service
                .remove_worktree(first_entry.path.clone())
                .await
                .is_err()
        );
        assert!(first.is_dir());
        std::fs::remove_file(transcript).unwrap();
        assert!(
            service
                .worktree_list()
                .await
                .unwrap()
                .iter()
                .all(|entry| entry.blocked_reason.is_none())
        );
    }
}
