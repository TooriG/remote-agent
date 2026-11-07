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
    pub text: ErrorText,
    pub recovery_label: Option<String>,
}

/// UI copy is written in the source language, Japanese. Host, transport and
/// provider diagnostics are not, so they stay verbatim in `details` instead
/// of mixing into translated copy.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "bindings", derive(uniffi::Record))]
pub struct ErrorText {
    pub message: String,
    pub details: Option<String>,
}

/// Single-string consumers show the copy first and the cause on its own line.
impl std::fmt::Display for ErrorText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)?;
        match &self.details {
            Some(details) => write!(f, "\n{details}"),
            None => Ok(()),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ErrorRecovery {
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
        ProviderSettings => (Settings, "プロバイダーの設定を変更できませんでした"),
        HostSoftware => (Settings, "ホストのバージョン情報を取得できませんでした"),
        FileOpenCapabilities => (Settings, "エディターを検出できませんでした"),
        Models { .. } | ModelSettings { .. } => (Settings, "モデルを読み込めませんでした"),
        Accounts => (Settings, "アカウントの操作に失敗しました"),
        WorktreeSettings | Worktrees => (Settings, "ワークツリーの操作に失敗しました"),
    })
}

impl Snapshot {
    pub(crate) fn error_recovery(&self, id: ErrorId) -> Option<ErrorRecovery> {
        use crate::state::{Intent, operations as op};
        let ErrorId::Operation { key, generation } = id else {
            return None;
        };
        let state = self.operations.get(&key)?;
        if state.generation != generation || !matches!(state.phase, OperationPhase::Failed { .. }) {
            return None;
        }
        let (label, intent) = match key {
            OperationKey::FileOpenCapabilities => (
                "再読み込み",
                Intent::ReadFileOpenCapabilities(op::ReadFileOpenCapabilities {}),
            ),
            OperationKey::Models { provider } => (
                "再読み込み",
                Intent::LoadModels(op::LoadModels { provider }),
            ),
            OperationKey::ModelSettings { model } => (
                "再読み込み",
                Intent::ReadModelSettings(op::ReadModelSettings { model }),
            ),
            OperationKey::Accounts => (
                "アカウントを再読み込み",
                Intent::ListAccounts(op::ListAccounts {}),
            ),
            OperationKey::ProviderSettings => (
                "設定を再読み込み",
                Intent::ReadProviderSettings(op::ReadProviderSettings {}),
            ),
            OperationKey::HostSoftware => (
                "再読み込み",
                Intent::ReadHostSoftware(op::ReadHostSoftware {}),
            ),
            OperationKey::Worktrees => ("再読み込み", Intent::ListWorktrees(op::ListWorktrees {})),
            OperationKey::WorktreeSettings => (
                "設定を再読み込み",
                Intent::ReadWorktreeSettings(op::ReadWorktreeSettings {}),
            ),
            OperationKey::ProjectScan => ("再検索", Intent::ScanProjects(op::ScanProjects {})),
            OperationKey::SessionList | OperationKey::SessionPage { .. } => (
                "会話一覧を再読み込み",
                Intent::ListSessions(op::ListSessions::new((*self.lists.list_query).clone())),
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
}

#[cfg_attr(feature = "bindings", uniffi::export)]
impl Snapshot {
    pub fn error_notices(&self, scope: ErrorScope) -> Vec<ErrorNotice> {
        let mut notices = Vec::new();
        if scope == ErrorScope::Application {
            let application = [
                (
                    &self.connection.connection_failure,
                    "接続",
                    "ホストに接続できません",
                ),
                (&self.pane.error, "操作", "操作に失敗しました"),
            ];
            for (error, context, title) in application {
                if let Some(error) = error {
                    notices.push(ErrorNotice {
                        id: ErrorId::Application {
                            message: error.clone(),
                        },
                        context: context.into(),
                        title: title.into(),
                        text: error_text(error),
                        recovery_label: None,
                    });
                }
            }
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
                    text: error_text(message),
                    recovery_label: self
                        .error_recovery(ErrorId::Operation {
                            key: key.clone(),
                            generation: state.generation,
                        })
                        .map(|recovery| recovery.label),
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

const FAILED: &str = "エラーが発生しました。";
const HOST_FAILED: &str = "ホストで操作に失敗しました。";
const NO_DETAILS: &str = "ホストで操作に失敗しました。エラーの詳細を取得できませんでした。";

#[cfg_attr(feature = "bindings", uniffi::export)]
pub fn error_text(error: &str) -> ErrorText {
    if let Some(raw) = error.strip_prefix("remote RPC error: ") {
        return host_failure(raw);
    }
    // Local failures arrive as `PeerError` and `TransportError` text.
    let (fallback, text) = if let Some(text) = error.strip_prefix("invalid RPC message: ") {
        (FAILED, text)
    } else if let Some(text) = error.strip_prefix("connection closed: ") {
        ("ホストとの接続が切れました。", text)
    } else if error.starts_with("iroh transport failed: ") {
        (
            "ネットワーク接続とホストの起動状態を確認してください。",
            error,
        )
    } else if error.starts_with("request ") && error.ends_with(" timed out") {
        ("ホストから応答がありませんでした。", error)
    } else if error.starts_with("Invalid ") && error.contains(" response: ") {
        ("ホストの応答を読み取れませんでした。", error)
    } else {
        (FAILED, error)
    };
    let mut text = split(text);
    if text.message.is_empty() {
        text = ErrorText {
            message: fallback.into(),
            details: (!error.trim().is_empty()).then(|| error.into()),
        };
    }
    text
}

fn host_failure(raw: &str) -> ErrorText {
    let copy = |message: &str| ErrorText {
        message: message.into(),
        details: None,
    };
    let Ok(value) = serde_json::from_str::<Value>(raw) else {
        return copy(NO_DETAILS);
    };
    match value["code"].as_str() {
        Some("file_not_found") => return copy("ファイルまたはフォルダが見つかりません。"),
        Some("file_access_denied") => {
            return copy("ファイルまたはフォルダにアクセスする権限がありません。");
        }
        Some("thread_in_use") => return copy(agent_protocol::error::THREAD_IN_USE_MESSAGE),
        _ => {}
    }
    let dictation = value["code"] == "dictation_failed";
    let mut text = match value["message"]
        .as_str()
        .filter(|message| !message.trim().is_empty())
    {
        Some(message) => split(&crate::diagnostics::sanitize(message)),
        None if dictation => copy(""),
        None => return copy(NO_DETAILS),
    };
    if dictation {
        let reason = match text.message.as_str() {
            "" => "もう一度録音してください。",
            reason => reason,
        };
        text.message = format!("音声を文字起こしできませんでした。{reason}");
    } else if text.message.is_empty() {
        text.message = HOST_FAILED.into();
    }
    text
}

/// Host failures join context and cause as `context: cause`, so a Japanese
/// line ending in a non-Japanese cause is split into copy and details.
fn split(error: &str) -> ErrorText {
    let copy_text = |text: &str| {
        text.chars().any(|c| {
            matches!(c, '\u{3040}'..='\u{30ff}' | '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}')
        })
    };
    let (mut copy, mut details) = (Vec::new(), Vec::new());
    for line in error.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if !copy_text(line) {
            details.push(line);
            continue;
        }
        match line
            .match_indices(": ")
            .find(|(index, _)| !copy_text(&line[index + 2..]))
        {
            Some((index, _)) => {
                copy.push(&line[..index]);
                details.push(&line[index + 2..]);
            }
            None => copy.push(line),
        }
    }
    ErrorText {
        message: copy.join("\n"),
        details: (!details.is_empty()).then(|| details.join("\n")),
    }
}

#[cfg(test)]
mod error_tests {
    use super::{ErrorText, error_text};
    use crate::peer::PeerError;
    use proptest::prelude::*;

    fn japanese(text: &str) -> bool {
        text.chars()
            .any(|c| matches!(c, '\u{3040}'..='\u{30ff}' | '\u{4e00}'..='\u{9fff}'))
    }

    #[test]
    fn writer_conflicts_explain_explicit_fork_recovery() {
        let text = error_text(
            r#"remote RPC error: {"code":"thread_in_use","message":"thread private-id already has an active writer","delivery":"notSent"}"#,
        );
        assert!(text.message.contains("分岐") && text.message.contains("未送信"));
        assert!(!text.message.contains("private-id") && !text.message.contains('{'));
        assert_eq!(text.details, None);
    }

    #[test]
    fn known_file_failures_show_recovery_copy_without_host_payloads() {
        for (code, message) in [
            ("file_not_found", "ファイルまたはフォルダが見つかりません。"),
            (
                "file_access_denied",
                "ファイルまたはフォルダにアクセスする権限がありません。",
            ),
        ] {
            let raw = format!(
                "remote RPC error: {}",
                serde_json::json!({"code": code, "message": "host file failure", "data": {"path": "private"}})
            );
            assert_eq!(
                error_text(&raw),
                ErrorText {
                    message: message.into(),
                    details: None
                }
            );
        }
    }

    #[test]
    fn provider_cause_stays_in_details_without_exposing_payloads() {
        let raw = r#"remote RPC error: {"delivery":"unknown","code":"provider_failed","message":"thread example already has an active writer","execution":{"providerCode":"-32600"}}"#;
        let text = error_text(raw);
        assert_eq!(text.message, "ホストで操作に失敗しました。");
        assert_eq!(
            text.details.as_deref(),
            Some("thread example already has an active writer")
        );
        assert_eq!(crate::diagnostics::sanitize(raw), text.details.unwrap());
        for raw in [
            "remote RPC error: invalid",
            r#"remote RPC error: {"message":" "}"#,
        ] {
            let text = error_text(raw);
            assert!(text.message.contains("詳細を取得できませんでした"));
            assert!(!text.message.contains("もう一度"));
            assert_eq!(text.details, None);
        }
    }

    #[test]
    fn host_copy_keeps_its_language_and_moves_the_cause_to_details() {
        let host = |message: &str| {
            error_text(&format!(
                "remote RPC error: {}",
                serde_json::json!({"code": "workspace_review_failed", "message": message})
            ))
        };
        assert_eq!(
            host("ワークツリーを確認できません: fatal: not a git repository"),
            ErrorText {
                message: "ワークツリーを確認できません".into(),
                details: Some("fatal: not a git repository".into()),
            }
        );
        assert_eq!(
            host(
                "Codex CLIの更新に失敗しました: exit status 1\nインストール元の方法で更新してください。"
            ),
            ErrorText {
                message: "Codex CLIの更新に失敗しました\nインストール元の方法で更新してください。"
                    .into(),
                details: Some("exit status 1".into()),
            }
        );
        let warning = "稼働状況を確認できないため削除できません: 未保存の変更があります。";
        assert_eq!(host(warning).message, warning);
        assert_eq!(host(warning).details, None);
        assert_eq!(
            error_text(r#"remote RPC error: {"code":-1,"message":"permission denied"}"#),
            ErrorText {
                message: "ホストで操作に失敗しました。".into(),
                details: Some("permission denied".into()),
            }
        );
    }

    #[test]
    fn dictation_errors_explain_recovery_without_rendering_rpc_payloads() {
        let text = error_text(
            r#"remote RPC error: {"code":"dictation_failed","message":"provider failed","data":{"audio":"private"}}"#,
        );
        assert_eq!(
            text.message,
            "音声を文字起こしできませんでした。もう一度録音してください。"
        );
        assert_eq!(text.details.as_deref(), Some("provider failed"));
        assert_eq!(
            error_text(r#"remote RPC error: {"code":"dictation_failed","message":" "}"#),
            ErrorText {
                message: "音声を文字起こしできませんでした。もう一度録音してください。".into(),
                details: None,
            }
        );
        assert_eq!(
            error_text(r#"remote RPC error: {"code":"dictation_failed","message":"Codexにログインしてください。"}"#).message,
            "音声を文字起こしできませんでした。Codexにログインしてください。"
        );
        assert_eq!(
            error_text("マイクへのアクセスを許可してください"),
            ErrorText {
                message: "マイクへのアクセスを許可してください".into(),
                details: None,
            }
        );
    }

    #[test]
    fn local_transport_failures_explain_the_failure_and_keep_the_cause() {
        let busy = "操作が混み合っています。少し待って再試行してください。";
        assert_eq!(
            error_text(&PeerError::InvalidMessage(busy.into()).to_string()),
            ErrorText {
                message: busy.into(),
                details: None,
            }
        );
        let cases = [
            (
                PeerError::ConnectionClosed("store is closed".into()).to_string(),
                "ホストとの接続が切れました。",
            ),
            (
                PeerError::RequestTimeout {
                    method: "turn/start".into(),
                }
                .to_string(),
                "ホストから応答がありませんでした。",
            ),
            (
                PeerError::InvalidResponse {
                    method: "thread/read".into(),
                    reason: "missing field".into(),
                    raw: String::new(),
                    sequence: None,
                }
                .to_string(),
                "ホストの応答を読み取れませんでした。",
            ),
            (
                crate::transport::TransportError::Connection("timed out".into()).to_string(),
                "ネットワーク接続とホストの起動状態を確認してください。",
            ),
            (
                PeerError::InvalidMessage("digest mismatch".into()).to_string(),
                "エラーが発生しました。",
            ),
            ("save failed".into(), "エラーが発生しました。"),
            (
                "request turn/start rejected".into(),
                "エラーが発生しました。",
            ),
            ("read operation timed out".into(), "エラーが発生しました。"),
            ("Invalid argument".into(), "エラーが発生しました。"),
            (
                "unexpected response: missing field".into(),
                "エラーが発生しました。",
            ),
        ];
        for (raw, message) in cases {
            assert_eq!(
                error_text(&raw),
                ErrorText {
                    message: message.into(),
                    details: Some(raw.clone()),
                }
            );
        }
        assert_eq!(
            error_text(""),
            ErrorText {
                message: "エラーが発生しました。".into(),
                details: None
            }
        );
    }

    proptest! {
        #[test]
        fn copy_is_always_japanese_and_details_never_are(
            lines in proptest::collection::vec(
                proptest::sample::select(vec![
                    "ファイルを保存できません", "timed out", "fatal: not a git repository",
                    "ホストに接続してください。", "", "  ", "a: b", "保存先: /tmp/x",
                ]),
                0..5,
            ),
            prefix in proptest::sample::select(vec![
                "", "invalid RPC message: ", "connection closed: ", "iroh transport failed: ",
            ]),
        ) {
            let raw = format!("{prefix}{}", lines.join("\n"));
            let text = error_text(&raw);
            prop_assert!(japanese(&text.message));
            prop_assert!(!text.details.as_deref().is_some_and(japanese));
            let details = text.details.clone().unwrap_or_default();
            for line in lines.iter().map(|line| line.trim()).filter(|line| !line.is_empty() && !japanese(line)) {
                prop_assert!(details.contains(line));
            }
        }
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
        snapshot.operations.insert(
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
            connection: crate::state::Connection {
                connection_failure: Some("connection lost".into()),
                ..Default::default()
            },
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
        assert_eq!(global[0].title, "ホストに接続できません");
        let (recovered, _) = reduce(&snapshot, Event::Connected);
        assert!(recovered.error_notices(ErrorScope::Application).is_empty());
        assert_eq!(local.len(), 1);
        assert_eq!(local[0].context, "この会話");
        assert_eq!(local[0].title, "メッセージを送信できませんでした");
        assert_eq!(local[0].text.details.as_deref(), Some("send rejected"));
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
        assert!(next.error_notices(ErrorScope::Application).is_empty());
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
            snapshot
                .history_control(session)
                .unwrap()
                .error
                .and_then(|error| error.details)
                .as_deref(),
            Some("history unavailable")
        );
        let settings = snapshot.error_notices(ErrorScope::Settings);
        assert_eq!(settings.len(), 1);
        assert_eq!(
            settings[0].text.details.as_deref(),
            Some("account unavailable")
        );
    }

    #[test]
    fn validation_failure_preserves_connection_error_and_belongs_to_workspace() {
        let snapshot = Snapshot {
            connection: crate::state::Connection {
                connection_failure: Some("connection lost".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        let (next, effects) = reduce(
            &snapshot,
            Event::Intent(Intent::ListFiles(ListFiles {
                path: "relative".into(),
            })),
        );
        assert!(effects.is_empty());
        assert_eq!(
            next.error_notices(ErrorScope::Application),
            snapshot.error_notices(ErrorScope::Application)
        );
        let notices = next.error_notices(ErrorScope::Workspace);
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].text.message, "絶対パスを入力してください。");
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
        Arc::make_mut(&mut snapshot.lists.list_query).search_term = "keep filter".into();
        let cases = [
            (
                OperationKey::Models {
                    provider: agent_protocol::session::ProviderKind::Codex,
                },
                Intent::LoadModels(op::LoadModels {
                    provider: agent_protocol::session::ProviderKind::Codex,
                }),
            ),
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
                Intent::ListSessions(op::ListSessions::new((*snapshot.lists.list_query).clone())),
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
            snapshot.operations.get_mut(&key).unwrap().phase = OperationPhase::Running;
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
            connection: crate::state::Connection {
                connected: true,
                ..Default::default()
            },
            pane: Arc::new(crate::state::PaneState {
                error: Some("save failed".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let notice = snapshot
            .error_notices(ErrorScope::Application)
            .pop()
            .unwrap();
        assert_eq!(notice.title, "操作に失敗しました");
        let newer = Snapshot {
            connection: crate::state::Connection {
                ..snapshot.connection.clone()
            },
            pane: Arc::new(crate::state::PaneState {
                error: Some("connection lost".into()),
                ..Default::default()
            }),
            ..snapshot
        };
        let (next, _) = reduce(
            &newer,
            Event::Intent(Intent::DismissError { id: notice.id }),
        );
        assert_eq!(next.pane.error, newer.pane.error);
        let (next, _) = reduce(&next, Event::Connected);
        assert!(next.error_notices(ErrorScope::Application).is_empty());
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
                .dispatch(Intent::LoadModels(crate::state::operations::LoadModels {
                    provider: crate::session::ProviderKind::Codex
                }))
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
        let key = OperationKey::Models {
            provider: agent_protocol::session::ProviderKind::Codex,
        };
        let mut snapshot = Snapshot::default();
        snapshot.operations.insert(
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
            prop_assert_eq!(snapshot.error_notices(conversation(&other))[0].text.details.clone(), Some("other failure".into()));
            snapshot.operations.insert(key.clone(), OperationState {
                generation: newer,
                phase: if running { OperationPhase::Running } else { OperationPhase::Failed { message: message.clone() } },
            });
            let (next, _) = reduce(&snapshot, Event::Intent(Intent::DismissError { id: dismissed.id }));
            prop_assert_eq!(next.operations.get(&key), snapshot.operations.get(&key));
            prop_assert_eq!(next.error_notices(conversation(&other)), snapshot.error_notices(conversation(&other)));
        }
    }
}
