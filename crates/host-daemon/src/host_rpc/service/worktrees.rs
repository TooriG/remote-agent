//! Worktree listing and removal. Removal waits until no provider can be running there.
use super::*;

#[derive(Default)]
struct WorktreeBlockers {
    running: bool,
    terminal: bool,
    warnings: std::collections::BTreeSet<String>,
}

impl WorktreeBlockers {
    fn reason(self) -> Option<String> {
        // Known occupancy precedes uncertainty; warnings are sorted and deduplicated.
        let mut reasons = Vec::new();
        if self.running {
            reasons.push(
                "このワークツリーで作業を実行中です。完了または停止してから削除してください。"
                    .to_owned(),
            );
        }
        if self.terminal {
            reasons.push("このワークツリーのターミナルを閉じてから削除してください。".to_owned());
        }
        reasons.extend(
            self.warnings
                .into_iter()
                .map(|warning| format!("稼働状況を確認できないため削除できません: {warning}")),
        );
        (!reasons.is_empty()).then(|| reasons.join("\n"))
    }
}

impl HostRpcService {
    pub(super) async fn worktree_list(
        &self,
    ) -> Result<Vec<agent_protocol::models::Worktree>, Failure> {
        let worktrees = self
            .inner
            .worktrees
            .list()
            .await
            .map_err(|error| Failure::new("worktree_list_failed", error))?;
        if worktrees.is_empty() {
            return Ok(worktrees);
        }
        let (threads, warning) = self.worktree_threads().await?;
        let mut worktrees = worktrees
            .into_iter()
            .map(|tree| {
                (
                    tree,
                    WorktreeBlockers {
                        warnings: warning.iter().cloned().collect(),
                        ..Default::default()
                    },
                )
            })
            .collect::<Vec<_>>();
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
                .filter_map(|(index, (tree, _))| cwd.starts_with(&tree.path).then_some(index))
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
                let (worktree, blockers) = &mut worktrees[index];
                blockers.running |= active;
                if let Some(warning) = idle_warning {
                    blockers.warnings.insert(warning.into());
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
        Ok(worktrees
            .into_iter()
            .map(|(mut worktree, mut blockers)| {
                blockers.terminal = self
                    .inner
                    .terminals
                    .in_use(std::path::Path::new(&worktree.path));
                worktree.blocked_reason = blockers.reason();
                worktree
            })
            .collect())
    }
    async fn worktree_threads(
        &self,
    ) -> Result<(Vec<agent_protocol::models::Thread>, Option<String>), Failure> {
        let mut warnings = std::collections::BTreeMap::new();
        let mut threads = Vec::new();
        for (provider, agent) in self.inner.providers.all() {
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
                        warnings.insert(provider, error.to_string());
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
        Ok((threads, self.inner.providers.error_summary(warnings)))
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
    fn repository(directory: &std::path::Path) -> std::path::PathBuf {
        let repository = directory.join("repository");
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
        repository
    }

    #[tokio::test]
    async fn worktree_keeps_idle_uncertainty_and_provider_failures_alongside_running_work() {
        use super::*;
        use crate::host_rpc::service::conversations::tests::{NativeAgent, call};
        let directory = tempfile::tempdir().unwrap();
        let repository = repository(directory.path());
        let mut agent = NativeAgent::new(ProviderKind::Codex, directory.path().join("codex"));
        Arc::get_mut(&mut agent).unwrap().idle_warning = Some("native idle warning");
        let host = HostRpcService::new(
            [
                agent.backend(),
                Backend::unavailable(ProviderKind::Claude, "claude startup failed"),
            ],
            ProjectStore::new(directory.path().join("worktrees.json")),
        );
        host.inner
            .worktrees
            .settings(Some(agent_protocol::models::WorktreeSettings {
                create_on_new_session: true,
                ..Default::default()
            }))
            .await
            .unwrap();
        let checkout = host
            .inner
            .worktrees
            .prepare(repository.to_str())
            .await
            .unwrap()
            .unwrap();
        agent
            .create(checkout.to_str().unwrap(), None, None)
            .await
            .unwrap();
        let running = agent
            .create(checkout.to_str().unwrap(), None, None)
            .await
            .unwrap()
            .thread
            .id
            .unwrap();
        agent
            .records
            .lock()
            .unwrap()
            .get_mut(&running.id)
            .unwrap()
            .thread
            .status = agent_protocol::models::SessionStatus::Running;
        let client = host.open_session();
        let worktrees: Vec<agent_protocol::models::Worktree> = call(
            &host,
            client.id(),
            Call::ListWorktrees(agent_protocol::models::Empty {}),
        )
        .await
        .unwrap();
        assert_eq!(worktrees.len(), 1);
        assert_eq!(worktrees[0].threads.len(), 2);
        assert!(worktrees[0].threads.iter().any(|thread| thread.active));
        assert!(worktrees[0].threads.iter().any(|thread| !thread.active));
        assert_eq!(
            worktrees[0].blocked_reason.as_deref(),
            Some(
                "このワークツリーで作業を実行中です。完了または停止してから削除してください。\n稼働状況を確認できないため削除できません: claude startup failed\n稼働状況を確認できないため削除できません: native idle warning"
            )
        );
    }

    #[tokio::test]
    async fn worktree_block_reasons_keep_every_provider_failure_in_provider_order() {
        for listing_fails in [false, true] {
            use super::*;
            use crate::host_rpc::service::conversations::tests::NativeAgent;
            use std::sync::atomic::Ordering;
            let directory = tempfile::tempdir().unwrap();
            let repository = repository(directory.path());
            for order in [
                [
                    ProviderKind::DeepSeek,
                    ProviderKind::Claude,
                    ProviderKind::Codex,
                ],
                [
                    ProviderKind::Claude,
                    ProviderKind::Codex,
                    ProviderKind::DeepSeek,
                ],
                [
                    ProviderKind::Codex,
                    ProviderKind::DeepSeek,
                    ProviderKind::Claude,
                ],
            ] {
                let a = NativeAgent::new(ProviderKind::Codex, directory.path().join("codex"));
                let host = HostRpcService::new(
                    order.map(|provider| {
                        if listing_fails && provider == ProviderKind::Codex {
                            a.backend()
                        } else {
                            Backend::unavailable(
                                provider,
                                format!("{} startup failed", provider.key()),
                            )
                        }
                    }),
                    ProjectStore::new(directory.path().join("worktrees.json")),
                );
                host.inner
                    .worktrees
                    .settings(Some(agent_protocol::models::WorktreeSettings {
                        create_on_new_session: true,
                        ..Default::default()
                    }))
                    .await
                    .unwrap();
                host.inner
                    .worktrees
                    .prepare(repository.to_str())
                    .await
                    .unwrap()
                    .unwrap();
                let expected = if listing_fails {
                    "稼働状況を確認できないため削除できません: codex list failed\nclaude startup failed\ndeepseek startup failed"
                } else {
                    "稼働状況を確認できないため削除できません: codex startup failed\nclaude startup failed\ndeepseek startup failed"
                };

                a.fail_list.store(1, Ordering::SeqCst);
                let entries = host.worktree_list().await.unwrap();
                assert!(!entries.is_empty());
                assert!(
                    entries
                        .iter()
                        .all(|entry| entry.blocked_reason.as_deref() == Some(expected))
                );
            }
        }
    }

    #[test]
    fn blockers_keep_all_occupancy_and_uncertainty_reasons_in_a_fixed_order() {
        assert_eq!(super::WorktreeBlockers::default().reason(), None);
        for warnings in [
            ["native idle warning", "codex startup failed"],
            ["codex startup failed", "native idle warning"],
        ] {
            let blockers = super::WorktreeBlockers {
                running: true,
                terminal: true,
                warnings: warnings.into_iter().map(str::to_owned).collect(),
            };
            assert_eq!(
                blockers.reason().as_deref(),
                Some(
                    "このワークツリーで作業を実行中です。完了または停止してから削除してください。\nこのワークツリーのターミナルを閉じてから削除してください。\n稼働状況を確認できないため削除できません: codex startup failed\n稼働状況を確認できないため削除できません: native idle warning"
                )
            );
        }
    }

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
