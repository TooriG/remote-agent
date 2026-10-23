//! Availability is derived from the connected Host's catalog and authentication.
use crate::{models::Model, session::ProviderKind, state::Snapshot};
use agent_protocol::operations::Accounts;
use serde_json::{Map, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "bindings", derive(uniffi::Enum))]
pub enum AgentAvailability {
    Checking,
    Ready,
    LoginRequired,
    Unavailable,
}

#[derive(Clone, Debug)]
#[cfg_attr(feature = "bindings", derive(uniffi::Record))]
pub struct ConnectionAgent {
    pub provider: ProviderKind,
    pub name: String,
    pub availability: AgentAvailability,
    pub label: String,
}

#[derive(Clone, Debug)]
#[cfg_attr(feature = "bindings", derive(uniffi::Record))]
pub struct ConnectionSetup {
    pub agents: Vec<ConnectionAgent>,
    pub can_start: bool,
}

fn availability(
    connected: bool,
    provider: ProviderKind,
    models: &[Model],
    accounts: Option<&Accounts>,
    errors: &Map<String, Value>,
) -> AgentAvailability {
    if !connected {
        return AgentAvailability::Checking;
    }
    let key = provider.key();
    if errors.contains_key(key) {
        return AgentAvailability::Unavailable;
    }
    let Some(accounts) = accounts else {
        return AgentAvailability::Checking;
    };
    let has_model = models.iter().any(|model| model.model.provider == provider);
    let authenticated = accounts
        .accounts
        .iter()
        .any(|account| account.provider == provider && accounts.is_selected(account));
    match (has_model, authenticated) {
        (true, true) => AgentAvailability::Ready,
        (true, false) => AgentAvailability::LoginRequired,
        (false, true) => AgentAvailability::Checking,
        (false, false) => AgentAvailability::Unavailable,
    }
}

#[cfg_attr(feature = "bindings", uniffi::export)]
impl Snapshot {
    pub fn connection_setup(&self) -> ConnectionSetup {
        use crate::state::operations::{OperationKey, OperationPhase};
        let checks_failed = self.connected
            && [OperationKey::Models, OperationKey::Accounts]
                .iter()
                .any(|key| {
                    self.operations
                        .get(key)
                        .is_some_and(|state| matches!(state.phase, OperationPhase::Failed { .. }))
                });
        let agents: Vec<_> = [
            (ProviderKind::Codex, "Codex"),
            (ProviderKind::Claude, "Claude Code"),
            (ProviderKind::DeepSeek, "DeepSeek Harness"),
        ]
        .into_iter()
        .map(|(provider, name)| {
            let availability = availability(
                self.connected,
                provider,
                &self.models,
                self.account.accounts.as_deref(),
                &self.model_errors,
            );
            let availability = match availability {
                AgentAvailability::Checking if checks_failed => AgentAvailability::Unavailable,
                availability => availability,
            };
            ConnectionAgent {
                provider,
                name: name.into(),
                availability,
                label: match availability {
                    AgentAvailability::Checking => "確認中…",
                    AgentAvailability::Ready => "利用可能",
                    AgentAvailability::LoginRequired => "未ログイン",
                    AgentAvailability::Unavailable => "設定が必要",
                }
                .into(),
            }
        })
        .collect();
        ConnectionSetup {
            can_start: agents
                .iter()
                .any(|agent| agent.availability == AgentAvailability::Ready),
            agents,
        }
    }
}

/// SSH receives a destination argument, never shell code or additional options.
pub fn validate_ssh_destination(destination: &str) -> Result<&str, &'static str> {
    let destination = destination.trim();
    if destination.is_empty()
        || destination.starts_with('-')
        || !destination
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-@:[]".contains(&byte))
    {
        return Err("SSH接続先に user@server またはSSHのホスト名を入力してください。");
    }
    Ok(destination)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_checks_stop_spinning_and_retries_keep_cached_ready_agents() {
        use crate::state::operations::{OperationKey, OperationPhase, OperationState};
        use std::sync::Arc;
        for key in [OperationKey::Models, OperationKey::Accounts] {
            let mut snapshot = Snapshot {
                connected: true,
                ..Default::default()
            };
            Arc::make_mut(&mut snapshot.operations).insert(
                key.clone(),
                OperationState {
                    generation: 1,
                    phase: OperationPhase::Failed {
                        message: "request timed out".into(),
                    },
                },
            );
            let setup = snapshot.connection_setup();
            assert!(!setup.can_start);
            assert!(
                setup
                    .agents
                    .iter()
                    .all(|agent| agent.availability == AgentAvailability::Unavailable)
            );
            Arc::make_mut(&mut snapshot.account).accounts = Some(Arc::new(
                serde_json::from_value(serde_json::json!({
                    "accounts":[{"id":"native","provider":"codex"}],
                    "selected":{"codex":"native"}
                }))
                .unwrap(),
            ));
            assert_eq!(
                snapshot.connection_setup().agents[0].availability,
                AgentAvailability::Unavailable
            );
            Arc::make_mut(&mut snapshot.operations)
                .get_mut(&key)
                .unwrap()
                .phase = OperationPhase::Running;
            assert_eq!(
                snapshot.connection_setup().agents[0].availability,
                AgentAvailability::Checking
            );
            snapshot.models = Arc::new(
                serde_json::from_value(serde_json::json!([{
                    "id":"gpt","model":{"provider":"codex","id":"gpt"},"displayName":"GPT",
                    "settings":{"defaultReasoningEffort":"","supportedReasoningEfforts":[]}
                }]))
                .unwrap(),
            );
            assert!(snapshot.connection_setup().can_start);
            Arc::make_mut(&mut snapshot.operations)
                .get_mut(&key)
                .unwrap()
                .phase = OperationPhase::Failed {
                message: "refresh failed".into(),
            };
            assert!(
                snapshot.connection_setup().can_start,
                "cached ready providers survive a failed refresh"
            );
            snapshot.connected = false;
            let setup = snapshot.connection_setup();
            assert!(!setup.can_start);
            assert!(
                setup
                    .agents
                    .iter()
                    .all(|agent| agent.availability == AgentAvailability::Checking)
            );
        }
    }

    proptest::proptest! {
        #[test]
        fn cached_catalog_and_accounts_never_prove_a_disconnected_agent_ready(
            connected in proptest::bool::ANY,
            authenticated in proptest::bool::ANY,
            catalog in proptest::bool::ANY,
            failed in proptest::bool::ANY,
            provider in proptest::sample::select(vec![ProviderKind::Codex, ProviderKind::Claude, ProviderKind::DeepSeek]),
        ) {
            let key = provider.key();
            let models: Vec<Model> = if catalog {
                serde_json::from_value(serde_json::json!([{
                    "id":"same-native-id", "model":{"provider":key,"id":"same-native-id"},
                    "displayName":"Agent", "settings": { "defaultReasoningEffort":"", "supportedReasoningEfforts":[]
                }}])).unwrap()
            } else { Vec::new() };
            let accounts: Accounts = serde_json::from_value(serde_json::json!({
                "accounts": if authenticated { vec![serde_json::json!({"id":"selected","provider":key})] } else { Vec::new() },
                "selected":{(key):"selected"},
                "error":null
            })).unwrap();
            let errors = if failed { Map::from_iter([(key.into(), Value::Null)]) } else { Map::new() };
            let actual = availability(connected, provider, &models, Some(&accounts), &errors);
            proptest::prop_assert_eq!(actual == AgentAvailability::Ready, connected && authenticated && catalog && !failed);
        }

        #[test]
        fn ssh_targets_cannot_inject_options_or_shell_syntax(value in ".{0,120}") {
            if let Ok(target) = validate_ssh_destination(&value) {
                proptest::prop_assert!(!target.is_empty());
                proptest::prop_assert!(!target.starts_with('-'));
                proptest::prop_assert!(!target.chars().any(|c| c.is_whitespace() || ";|&$`\\\"'".contains(c)));
            }
        }
    }

    #[test]
    fn one_ready_provider_can_start_without_configuring_the_other() {
        let mut snapshot = Snapshot {
            connected: true,
            models: std::sync::Arc::new(serde_json::from_value(serde_json::json!([
                {"id":"gpt", "model":{"provider":"codex","id":"gpt"}, "displayName":"GPT", "settings": { "defaultReasoningEffort":"", "supportedReasoningEfforts":[]}},
                {"id":"sonnet", "model":{"provider":"claude","id":"sonnet"}, "displayName":"Sonnet", "settings": { "defaultReasoningEffort":"", "supportedReasoningEfforts":[]}}
            ])).unwrap()),
            ..Default::default()
        };
        std::sync::Arc::make_mut(&mut snapshot.account).accounts = Some(std::sync::Arc::new(
            serde_json::from_value(serde_json::json!({
                "accounts":[{"id":"native", "provider":"codex"}, {"id":"native", "provider":"claude"}],
                "selected":{"codex":"native"}
            })).unwrap()
        ));
        let setup = snapshot.connection_setup();
        assert!(setup.can_start);
        assert_eq!(setup.agents[0].availability, AgentAvailability::Ready);
        assert_eq!(
            setup.agents[1].availability,
            AgentAvailability::LoginRequired
        );
        assert_eq!(setup.agents[0].label, "利用可能");
        assert_eq!(setup.agents[1].label, "未ログイン");
        snapshot.connected = false;
        let setup = snapshot.connection_setup();
        assert!(!setup.can_start);
        assert!(
            setup
                .agents
                .iter()
                .all(|agent| agent.availability == AgentAvailability::Checking)
        );
    }

    #[test]
    fn incomplete_checks_and_authentication_remain_distinct() {
        let model: Model = serde_json::from_value(serde_json::json!({
            "id":"gpt", "model":{"provider":"codex","id":"gpt"},
            "displayName":"GPT", "settings": { "defaultReasoningEffort":"", "supportedReasoningEfforts":[]
        }}))
        .unwrap();
        let accounts = Accounts {
            accounts: Vec::new(),
            selected: Default::default(),
            error: None,
        };
        assert_eq!(
            availability(
                true,
                ProviderKind::Codex,
                std::slice::from_ref(&model),
                None,
                &Map::new()
            ),
            AgentAvailability::Checking
        );
        assert_eq!(
            availability(
                true,
                ProviderKind::Codex,
                &[model],
                Some(&accounts),
                &Map::new()
            ),
            AgentAvailability::LoginRequired
        );
        assert_eq!(
            availability(true, ProviderKind::Codex, &[], Some(&accounts), &Map::new()),
            AgentAvailability::Unavailable
        );
        let authenticated = serde_json::from_value(serde_json::json!({
            "accounts":[{"id":"native", "provider":"codex"}], "selected":{"codex":"native"}
        }))
        .unwrap();
        assert_eq!(
            availability(
                true,
                ProviderKind::Codex,
                &[],
                Some(&authenticated),
                &Map::new()
            ),
            AgentAvailability::Checking
        );
    }

    #[test]
    fn ssh_accepts_destinations_and_rejects_options_and_commands() {
        assert_eq!(
            validate_ssh_destination(" user@server \n"),
            Ok("user@server")
        );
        for target in ["user@server", "dev-server", "user@[::1]"] {
            assert_eq!(validate_ssh_destination(target), Ok(target));
        }
        for target in [
            "",
            "-oProxyCommand=anything",
            "user@server; command",
            "user@$(command)",
        ] {
            assert!(validate_ssh_destination(target).is_err());
        }
    }
}
