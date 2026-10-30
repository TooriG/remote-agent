use crate::state::{
    DraftKey, Snapshot,
    operations::{ErrorId, OperationKey, OperationPhase},
};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "bindings", derive(uniffi::Enum))]
pub enum ErrorScope {
    Application,
    Conversation { draft_key: DraftKey },
    Settings,
    Workspace,
    Projects,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "bindings", derive(uniffi::Record))]
pub struct ErrorNotice {
    pub id: ErrorId,
    pub context: String,
    pub title: String,
    pub message: String,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "bindings", derive(uniffi::Record))]
pub struct ErrorRecovery {
    pub label: String,
    pub intent: crate::state::Intent,
}

impl ErrorScope {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Application => "アプリ全体",
            Self::Conversation { .. } => "この会話",
            Self::Settings => "設定",
            Self::Workspace => "作業スペース",
            Self::Projects => "プロジェクト",
        }
    }
}

// Errors already rendered beside a retry control stay with that control.
fn operation_notice(key: &OperationKey) -> Option<(ErrorScope, &'static str)> {
    use ErrorScope::*;
    use OperationKey::*;
    Some(match key {
        History { .. }
        | Request { .. }
        | AccountLogin { .. }
        | DshCredentials
        | Permissions { .. }
        | Fork { .. }
        | TurnItems { .. }
        | ProjectList { .. }
        | ListDecorations { .. }
        | TaskActivity
        | LiveActivity { .. } => return None,
        Item { item } => (
            Conversation {
                draft_key: (&item.thread_id).into(),
            },
            "詳細を読み込めませんでした",
        ),
        Submission { draft_key } => (
            Conversation {
                draft_key: draft_key.clone(),
            },
            "メッセージを送信できませんでした",
        ),
        Dictation { draft_key } => (
            Conversation {
                draft_key: draft_key.clone(),
            },
            "音声入力に失敗しました",
        ),
        Attachment { draft_key } => (
            Conversation {
                draft_key: draft_key.clone(),
            },
            "添付ファイルを追加できませんでした",
        ),
        Interrupt { session, .. } => (
            Conversation {
                draft_key: session.into(),
            },
            "作業を停止できませんでした",
        ),
        Agents { session } => (
            Conversation {
                draft_key: session.into(),
            },
            "エージェントを読み込めませんでした",
        ),
        SessionList | SessionPage { .. } => (Projects, "会話一覧を読み込めませんでした"),
        ProjectScan => (Projects, "プロジェクトを検索できませんでした"),
        ProjectImport => (Projects, "プロジェクトを取り込めませんでした"),
        Directory => (Workspace, "フォルダを読み込めませんでした"),
        File => (Workspace, "ファイルを読み込めませんでした"),
        SaveFile { .. } => (Workspace, "ファイルを保存できませんでした"),
        WorkspaceReview => (Workspace, "変更を読み込めませんでした"),
        CodexCli => (Settings, "Codex CLIの操作に失敗しました"),
        Models | ModelSettings { .. } => (Settings, "モデルを読み込めませんでした"),
        Accounts => (Settings, "アカウントの操作に失敗しました"),
        WorktreeSettings | Worktrees => (Settings, "ワークツリーの操作に失敗しました"),
    })
}

#[cfg_attr(feature = "bindings", uniffi::export)]
impl Snapshot {
    pub fn error_recovery(&self, id: ErrorId) -> Option<ErrorRecovery> {
        use crate::state::{Intent, operations as op};
        let ErrorId::Operation { key, generation } = id else {
            return None;
        };
        let state = self.operations.get(&key)?;
        if state.generation != generation || !matches!(state.phase, OperationPhase::Failed { .. }) {
            return None;
        }
        let (label, intent) = match key {
            OperationKey::Models => ("再読み込み", Intent::LoadModels(op::LoadModels {})),
            OperationKey::ModelSettings { model } => (
                "再読み込み",
                Intent::ReadModelSettings(op::ReadModelSettings { model }),
            ),
            OperationKey::Accounts => (
                "アカウントを再読み込み",
                Intent::ListAccounts(op::ListAccounts {}),
            ),
            OperationKey::Worktrees => ("再読み込み", Intent::ListWorktrees(op::ListWorktrees {})),
            OperationKey::WorktreeSettings => (
                "設定を再読み込み",
                Intent::ReadWorktreeSettings(op::ReadWorktreeSettings {}),
            ),
            OperationKey::ProjectScan => ("再検索", Intent::ScanProjects(op::ScanProjects {})),
            OperationKey::SessionList | OperationKey::SessionPage { .. } => (
                "会話一覧を再読み込み",
                Intent::ListSessions(op::ListSessions::new((*self.list_query).clone())),
            ),
            OperationKey::SaveFile { path } => {
                ("保存を再試行", Intent::SaveFile(op::SaveFile { path }))
            }
            _ => return None,
        };
        Some(ErrorRecovery {
            label: label.into(),
            intent,
        })
    }

    pub fn error_notices(&self, scope: ErrorScope) -> Vec<ErrorNotice> {
        let mut notices = Vec::new();
        if scope == ErrorScope::Application
            && (self.connected || self.connection_attempt.is_none())
            && let Some(error) = &self.error
        {
            notices.push(ErrorNotice {
                id: ErrorId::Application {
                    message: error_message(error),
                },
                context: if self.connected { "操作" } else { "接続" }.into(),
                title: if self.connected {
                    "操作に失敗しました"
                } else {
                    "Hostに接続できません"
                }
                .into(),
                message: error_message(error),
            });
        }
        for (key, state) in self.operations.iter() {
            let OperationPhase::Failed { message } = &state.phase else {
                continue;
            };
            let Some((target, title)) = operation_notice(key) else {
                continue;
            };
            if target == scope {
                notices.push(ErrorNotice {
                    id: ErrorId::Operation {
                        key: key.clone(),
                        generation: state.generation,
                    },
                    context: scope.label().into(),
                    title: title.into(),
                    message: error_message(message),
                });
            }
        }
        notices.sort_by_key(|notice| {
            std::cmp::Reverse(match &notice.id {
                ErrorId::Operation { generation, .. } => *generation,
                ErrorId::Application { .. } => 0,
            })
        });
        notices
    }
}

pub fn error_message(error: &str) -> String {
    let Some(raw) = error.strip_prefix("remote RPC error: ") else {
        return error.into();
    };
    let Ok(value) = serde_json::from_str::<Value>(raw) else {
        return "接続先で操作に失敗しました。エラーの詳細を取得できませんでした。".into();
    };
    if value["code"] == "thread_in_use" {
        return agent_protocol::error::THREAD_IN_USE_MESSAGE.into();
    }
    if value["code"] == "dictation_failed" {
        let reason = value["message"]
            .as_str()
            .filter(|message| !message.trim().is_empty())
            .map(crate::diagnostics::sanitize)
            .unwrap_or_else(|| "もう一度録音してください。".into());
        return format!("音声を文字起こしできませんでした。{reason}");
    }
    value["message"]
        .as_str()
        .filter(|message| !message.trim().is_empty())
        .map(crate::diagnostics::sanitize)
        .unwrap_or_else(|| {
            "接続先で操作に失敗しました。エラーの詳細を取得できませんでした。".into()
        })
}

#[cfg(test)]
mod error_tests {
    use super::error_message;
    #[test]
    fn writer_conflicts_explain_explicit_fork_recovery() {
        let message = error_message(
            r#"remote RPC error: {"code":"thread_in_use","message":"thread private-id already has an active writer","delivery":"notSent"}"#,
        );
        assert!(message.contains("分岐") && message.contains("未送信"));
        assert!(!message.contains("private-id") && !message.contains('{'));
    }

    #[test]
    fn provider_cause_survives_host_envelope_without_exposing_payloads() {
        let raw = r#"remote RPC error: {"delivery":"unknown","code":"provider_failed","message":"thread example already has an active writer","execution":{"providerCode":"-32600"}}"#;
        assert_eq!(
            error_message(raw),
            "thread example already has an active writer"
        );
        assert_eq!(crate::diagnostics::sanitize(raw), error_message(raw));
        for raw in [
            "remote RPC error: invalid",
            r#"remote RPC error: {"message":" "}"#,
        ] {
            let message = error_message(raw);
            assert!(message.contains("詳細を取得できませんでした"));
            assert!(!message.contains("もう一度"));
        }
    }

    #[test]
    fn dictation_errors_explain_recovery_without_rendering_rpc_payloads() {
        let message = error_message(
            r#"remote RPC error: {"code":"dictation_failed","message":"provider failed","data":{"audio":"private"}}"#,
        );
        assert!(message.contains("音声を文字起こしできませんでした。provider failed"));
        assert!(!message.contains("private") && !message.contains("{"));
        assert!(error_message(r#"remote RPC error: {"code":"dictation_failed","message":"Codexにログインしてください。"}"#).contains("Codexにログインしてください。"));
        assert_eq!(
            error_message(r#"remote RPC error: {"code":-1,"message":"permission denied"}"#),
            "permission denied"
        );
        assert_eq!(
            error_message("マイクへのアクセスを許可してください"),
            "マイクへのアクセスを許可してください"
        );
    }
}

#[cfg(test)]
mod notice_tests {
    use super::*;
    use crate::state::{
        Event, Intent,
        operations::{ListFiles, OperationState},
        reduce,
    };
    use proptest::prelude::*;
    use std::sync::Arc;

    fn failed(snapshot: &mut Snapshot, key: OperationKey, generation: u64, message: &str) {
        Arc::make_mut(&mut snapshot.operations).insert(
            key,
            OperationState {
                generation,
                phase: OperationPhase::Failed {
                    message: message.into(),
                },
            },
        );
    }

    fn conversation(key: &DraftKey) -> ErrorScope {
        ErrorScope::Conversation {
            draft_key: key.clone(),
        }
    }

    #[test]
    fn connection_and_operation_failures_have_separate_context_and_dismissal() {
        let draft = DraftKey::default();
        let mut snapshot = Snapshot {
            error: Some("connection lost".into()),
            ..Default::default()
        };
        failed(
            &mut snapshot,
            OperationKey::Submission {
                draft_key: draft.clone(),
            },
            1,
            r#"remote RPC error: {"message":"send rejected","data":{"secret":"private"}}"#,
        );
        let global = snapshot.error_notices(ErrorScope::Application);
        let local = snapshot.error_notices(conversation(&draft));
        assert_eq!(global.len(), 1);
        assert_eq!(global[0].context, "接続");
        assert_eq!(global[0].title, "Hostに接続できません");
        assert_eq!(local.len(), 1);
        assert_eq!(local[0].context, "この会話");
        assert_eq!(local[0].title, "メッセージを送信できませんでした");
        assert_eq!(local[0].message, "send rejected");
        let (next, effects) = reduce(
            &snapshot,
            Event::Intent(Intent::DismissError {
                id: local[0].id.clone(),
            }),
        );
        assert!(effects.is_empty());
        assert!(next.error_notices(conversation(&draft)).is_empty());
        assert_eq!(next.error_notices(ErrorScope::Application), global);
        let (next, _) = reduce(
            &next,
            Event::Intent(Intent::DismissError {
                id: global[0].id.clone(),
            }),
        );
        assert!(next.error.is_none());
    }

    #[test]
    fn history_failures_stay_with_their_retry_control_and_settings_stay_out_of_chat() {
        let session =
            crate::session::SessionRef::new(crate::session::ProviderKind::Codex, "thread".into())
                .unwrap();
        let draft = DraftKey::from(&session);
        let mut snapshot = Snapshot::default();
        failed(
            &mut snapshot,
            OperationKey::History {
                session: session.clone(),
            },
            1,
            "history unavailable",
        );
        failed(
            &mut snapshot,
            OperationKey::Accounts,
            2,
            "account unavailable",
        );
        assert!(snapshot.error_notices(conversation(&draft)).is_empty());
        assert!(snapshot.error_notices(ErrorScope::Application).is_empty());
        assert_eq!(
            snapshot.history_control(session).unwrap().error.as_deref(),
            Some("history unavailable")
        );
        let settings = snapshot.error_notices(ErrorScope::Settings);
        assert_eq!(settings.len(), 1);
        assert_eq!(settings[0].message, "account unavailable");
    }

    #[test]
    fn validation_failure_preserves_connection_error_and_belongs_to_workspace() {
        let snapshot = Snapshot {
            error: Some("connection lost".into()),
            ..Default::default()
        };
        let (next, effects) = reduce(
            &snapshot,
            Event::Intent(Intent::ListFiles(ListFiles {
                path: "relative".into(),
            })),
        );
        assert!(effects.is_empty());
        assert_eq!(next.error, snapshot.error);
        let notices = next.error_notices(ErrorScope::Workspace);
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].message, "絶対パスを入力してください。");
        assert!(matches!(
            notices[0].id,
            ErrorId::Operation { generation: 1, .. }
        ));
    }

    #[test]
    fn recovery_reloads_the_owned_resource_and_never_resubmits_a_message() {
        use crate::state::operations as op;
        let draft = DraftKey::from("draft");
        let model = crate::models::ModelRef {
            provider: crate::session::ProviderKind::Codex,
            id: "model".into(),
        };
        let mut snapshot = Snapshot::default();
        Arc::make_mut(&mut snapshot.list_query).search_term = "keep filter".into();
        let cases = [
            (OperationKey::Models, Intent::LoadModels(op::LoadModels {})),
            (
                OperationKey::ModelSettings {
                    model: model.clone(),
                },
                Intent::ReadModelSettings(op::ReadModelSettings { model }),
            ),
            (
                OperationKey::Accounts,
                Intent::ListAccounts(op::ListAccounts {}),
            ),
            (
                OperationKey::Worktrees,
                Intent::ListWorktrees(op::ListWorktrees {}),
            ),
            (
                OperationKey::WorktreeSettings,
                Intent::ReadWorktreeSettings(op::ReadWorktreeSettings {}),
            ),
            (
                OperationKey::ProjectScan,
                Intent::ScanProjects(op::ScanProjects {}),
            ),
            (
                OperationKey::SessionList,
                Intent::ListSessions(op::ListSessions::new((*snapshot.list_query).clone())),
            ),
            (
                OperationKey::SaveFile {
                    path: "/file".into(),
                },
                Intent::SaveFile(op::SaveFile {
                    path: "/file".into(),
                }),
            ),
        ];
        for (key, expected) in cases {
            failed(&mut snapshot, key.clone(), 1, "unavailable");
            let id = ErrorId::Operation {
                key: key.clone(),
                generation: 1,
            };
            let recovery = snapshot.error_recovery(id.clone()).unwrap();
            assert!(!recovery.label.is_empty());
            assert_eq!(
                serde_json::to_value(recovery.intent).unwrap(),
                serde_json::to_value(expected).unwrap()
            );
            assert!(
                snapshot
                    .error_recovery(ErrorId::Operation {
                        key: key.clone(),
                        generation: 0
                    })
                    .is_none()
            );
            Arc::make_mut(&mut snapshot.operations)
                .get_mut(&key)
                .unwrap()
                .phase = OperationPhase::Running;
            assert!(snapshot.error_recovery(id).is_none());
        }
        for key in [
            OperationKey::Submission {
                draft_key: draft.clone(),
            },
            OperationKey::Dictation { draft_key: draft },
        ] {
            failed(&mut snapshot, key.clone(), 1, "delivery unknown");
            assert!(
                snapshot
                    .error_recovery(ErrorId::Operation { key, generation: 1 })
                    .is_none()
            );
        }
    }

    #[test]
    fn closing_an_old_global_notice_preserves_a_newer_error_and_recovery_clears_it() {
        let snapshot = Snapshot {
            connected: true,
            error: Some("save failed".into()),
            ..Default::default()
        };
        let notice = snapshot
            .error_notices(ErrorScope::Application)
            .pop()
            .unwrap();
        assert_eq!(notice.title, "操作に失敗しました");
        let newer = Snapshot {
            error: Some("connection lost".into()),
            ..snapshot
        };
        let (next, _) = reduce(
            &newer,
            Event::Intent(Intent::DismissError { id: notice.id }),
        );
        assert_eq!(next.error, newer.error);
        let (next, _) = reduce(&next, Event::Connected);
        assert!(next.error_notices(ErrorScope::Application).is_empty());
    }

    #[test]
    fn superseded_connection_checks_cannot_reveal_a_previous_disconnect_error() {
        let previous = Snapshot {
            error: Some("background timeout".into()),
            ..Default::default()
        };
        let first = uuid::Uuid::new_v4();
        let second = uuid::Uuid::new_v4();
        let (checking, _) = reduce(&previous, Event::Connecting(first));
        let (checking, _) = reduce(&checking, Event::Connecting(second));
        let (checking, _) = reduce(&checking, Event::ConnectionFinished(first));
        assert_eq!(checking.connection_attempt, Some(second));
        assert!(checking.error_notices(ErrorScope::Application).is_empty());
        let (finished, _) = reduce(&checking, Event::ConnectionFinished(second));
        assert!(finished.connection_attempt.is_none());
        assert_eq!(
            finished.error_notices(ErrorScope::Application),
            previous.error_notices(ErrorScope::Application)
        );
        let connected = Snapshot {
            connected: true,
            error: Some("save failed".into()),
            ..checking
        };
        assert_eq!(
            connected.error_notices(ErrorScope::Application)[0].message,
            "save failed"
        );
    }

    #[tokio::test]
    async fn offline_operation_failure_stays_with_its_operation() {
        let store = crate::store::Store::offline(Snapshot::default());
        assert!(
            store
                .snapshot()
                .error_notices(ErrorScope::Application)
                .is_empty()
        );
        assert!(
            store
                .dispatch(Intent::LoadModels(crate::state::operations::LoadModels {}))
                .await
                .is_err()
        );
        assert!(
            store
                .snapshot()
                .error_notices(ErrorScope::Application)
                .is_empty()
        );
        assert_eq!(
            store.snapshot().error_notices(ErrorScope::Settings).len(),
            1
        );
        store.close().await.unwrap();
    }

    #[test]
    fn navigation_clears_workspace_failures_but_keeps_other_owners() {
        let mut snapshot = Snapshot::default();
        for key in [
            OperationKey::File,
            OperationKey::Directory,
            OperationKey::WorkspaceReview,
            OperationKey::SaveFile {
                path: "/file".into(),
            },
        ] {
            failed(&mut snapshot, key, 1, "workspace unavailable");
        }
        failed(
            &mut snapshot,
            OperationKey::Accounts,
            2,
            "accounts unavailable",
        );
        let (next, _) = reduce(
            &snapshot,
            Event::Intent(Intent::NewChat {
                cwd: "/other".into(),
            }),
        );
        assert!(next.error_notices(ErrorScope::Workspace).is_empty());
        assert_eq!(
            next.error_notices(ErrorScope::Settings),
            snapshot.error_notices(ErrorScope::Settings)
        );
    }

    #[test]
    fn dismissal_does_not_cancel_a_running_operation() {
        let key = OperationKey::Models;
        let mut snapshot = Snapshot::default();
        Arc::make_mut(&mut snapshot.operations).insert(
            key.clone(),
            OperationState {
                generation: 1,
                phase: OperationPhase::Running,
            },
        );
        let (next, _) = reduce(
            &snapshot,
            Event::Intent(Intent::DismissError {
                id: ErrorId::Operation {
                    key: key.clone(),
                    generation: 1,
                },
            }),
        );
        assert_eq!(next.operations.get(&key), snapshot.operations.get(&key));
    }

    proptest! {
        #[test]
        fn dismissal_cannot_remove_a_retry_or_another_conversations_error(
            message in "[a-z]{1,30}", newer in 2u64..100, running in any::<bool>(),
        ) {
            let first = DraftKey::default();
            let other = DraftKey::from("other");
            let key = OperationKey::Submission { draft_key: first.clone() };
            let mut snapshot = Snapshot::default();
            failed(&mut snapshot, key.clone(), 1, &message);
            failed(&mut snapshot, OperationKey::Submission { draft_key: other.clone() }, 100, "other failure");
            let dismissed = snapshot.error_notices(conversation(&first)).pop().unwrap();
            prop_assert_eq!(snapshot.error_notices(conversation(&other))[0].message.clone(), "other failure");
            Arc::make_mut(&mut snapshot.operations).insert(key.clone(), OperationState {
                generation: newer,
                phase: if running { OperationPhase::Running } else { OperationPhase::Failed { message: message.clone() } },
            });
            let (next, _) = reduce(&snapshot, Event::Intent(Intent::DismissError { id: dismissed.id }));
            prop_assert_eq!(next.operations.get(&key), snapshot.operations.get(&key));
            prop_assert_eq!(next.error_notices(conversation(&other)), snapshot.error_notices(conversation(&other)));
        }
    }
}
