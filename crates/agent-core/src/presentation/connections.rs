//! Availability is derived from the connected Host's catalog and authentication.
use crate::{localization::UiLanguage, models::Model, session::ProviderKind, state::Snapshot};
use agent_protocol::operations::Accounts;
use serde_json::{Map, Value};

#[cfg_attr(feature = "bindings", uniffi::export)]
pub fn connection_status_label(language: UiLanguage, connected: bool, enabled: bool) -> String {
    language
        .text(if !enabled {
            "オフ"
        } else if connected {
            "接続済み"
        } else {
            "未接続"
        })
        .into()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "bindings", derive(uniffi::Enum))]
pub enum AgentAvailability {
    Checking,
    Ready,
    LoginRequired,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "bindings", derive(uniffi::Record))]
pub struct ConnectionAgent {
    pub provider: ProviderKind,
    pub enabled: bool,
    pub name: String,
    pub availability: AgentAvailability,
    pub label: String,
    pub error: Option<super::error::ErrorText>,
}

#[derive(Clone, Debug, PartialEq)]
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
    if errors.contains_key(key)
        || accounts.is_some_and(|accounts| accounts.errors.contains_key(&provider))
    {
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
    pub fn connection_enabled(&self) -> bool {
        !self.connection.connection_disabled
    }

    pub fn provider_enabled(&self, provider: ProviderKind) -> bool {
        self.catalog
            .provider_settings
            .as_ref()
            .is_none_or(|settings| settings.enabled(provider))
    }

    pub fn provider_settings_loaded(&self) -> bool {
        self.catalog.provider_settings.is_some()
    }

    pub fn connection_setup(&self) -> ConnectionSetup {
        use crate::state::operations::{OperationKey, OperationPhase};
        let agents: Vec<_> = [
            (ProviderKind::Codex, "Codex"),
            (ProviderKind::Claude, "Claude Code"),
            (ProviderKind::DeepSeek, "DeepSeek Harness"),
        ]
        .into_iter()
        .map(|(provider, name)| {
            let enabled = self.provider_enabled(provider);
            let checks_failed = self.connection.connected
                && [OperationKey::Models { provider }, OperationKey::Accounts]
                    .iter()
                    .any(|key| {
                        self.operations.get(key).is_some_and(|state| {
                            matches!(state.phase, OperationPhase::Failed { .. })
                        })
                    });

            let availability = availability(
                self.connection.connected,
                provider,
                &self.catalog.models,
                self.catalog.account.accounts.as_deref(),
                &self.catalog.model_errors,
            );
            let availability = match availability {
                _ if !enabled => AgentAvailability::Unavailable,
                AgentAvailability::Checking if checks_failed => AgentAvailability::Unavailable,
                availability => availability,
            };
            ConnectionAgent {
                provider,
                enabled,
                name: name.into(),
                error: (enabled && availability == AgentAvailability::Unavailable)
                    .then(|| {
                        self.catalog
                            .model_errors
                            .get(provider.key())
                            .and_then(|error| error.get("message"))
                            .and_then(Value::as_str)
                            .map(super::error::error_text)
                            .or_else(|| {
                                self.catalog
                                    .account
                                    .accounts
                                    .as_ref()?
                                    .errors
                                    .get(&provider)
                                    .map(|error| super::error::error_text(error))
                            })
                            .or_else(|| self.operation_error(OperationKey::Models { provider }))
                            .or_else(|| self.operation_error(OperationKey::Accounts))
                    })
                    .flatten(),
                availability,
                label: self
                    .catalog
                    .ui_language
                    .text(if !enabled {
                        "オフ"
                    } else {
                        match availability {
                            AgentAvailability::Checking => "確認中…",
                            AgentAvailability::Ready => "利用可能",
                            AgentAvailability::LoginRequired => "未ログイン",
                            AgentAvailability::Unavailable => "設定が必要",
                        }
                    })
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
    fn setup_retains_the_failed_providers_reason_without_blaming_other_agents() {
        let mut snapshot = Snapshot {
            connection: crate::state::Connection {
                connected: true,
                ..Default::default()
            },
            ..Default::default()
        };
        snapshot.catalog.model_errors = std::sync::Arc::new(
            serde_json::from_value(serde_json::json!({
                "codex": {"code":"provider_timeout", "message":"Codex model discovery timed out"}
            }))
            .unwrap(),
        );
        let agents = snapshot.connection_setup().agents;
        assert_eq!(
            agents[0]
                .error
                .as_ref()
                .and_then(|error| error.details.as_deref()),
            Some("Codex model discovery timed out")
        );
        assert!(agents[1].error.is_none());
        assert!(agents[2].error.is_none());
        snapshot.connection.connected = false;
        assert!(
            snapshot
                .connection_setup()
                .agents
                .iter()
                .all(|agent| agent.error.is_none())
        );
    }

    #[test]
    fn account_probe_failures_are_scoped_and_recover_without_logging_out() {
        use std::sync::Arc;
        let mut snapshot = Snapshot {
            connection: crate::state::Connection {
                connected: true,
                ..Default::default()
            },
            catalog: crate::state::Catalog {
                models: Arc::new(serde_json::from_value(serde_json::json!([
                    {"id":"gpt", "model":{"provider":"codex","id":"gpt"}, "displayName":"GPT", "settings":{"defaultReasoningEffort":"","supportedReasoningEfforts":[]}},
                    {"id":"sonnet", "model":{"provider":"claude","id":"sonnet"}, "displayName":"Sonnet", "settings":{"defaultReasoningEffort":"","supportedReasoningEfforts":[]}}
                ])).unwrap()),
                ..Default::default()
            },
            ..Default::default()
        };
        let accounts: Accounts = serde_json::from_value(serde_json::json!({
            "accounts":[{"id":"native", "provider":"codex"}, {"id":"claude:desktop", "provider":"claude"}],
            "selected":{"codex":"native", "claude":"claude:desktop"},
            "errors":{"claude":"executable is missing: claude"}
        })).unwrap();
        Arc::make_mut(&mut snapshot.catalog.account).accounts = Some(Arc::new(accounts));
        let setup = snapshot.connection_setup();
        assert!(setup.can_start);
        assert_eq!(setup.agents[0].availability, AgentAvailability::Ready);
        assert!(setup.agents[0].error.is_none());
        assert_eq!(setup.agents[1].availability, AgentAvailability::Unavailable);
        assert_eq!(
            setup.agents[1].error.as_ref().unwrap().details.as_deref(),
            Some("executable is missing: claude")
        );
        Arc::make_mut(
            Arc::make_mut(&mut snapshot.catalog.account)
                .accounts
                .as_mut()
                .unwrap(),
        )
        .errors
        .clear();
        let setup = snapshot.connection_setup();
        assert_eq!(setup.agents[1].availability, AgentAvailability::Ready);
        assert!(setup.agents[1].error.is_none());
    }

    #[test]
    fn failed_checks_stop_spinning_and_retries_keep_cached_ready_agents() {
        use crate::state::operations::{OperationKey, OperationPhase, OperationState};
        use std::sync::Arc;
        for key in [
            OperationKey::Models {
                provider: agent_protocol::session::ProviderKind::Codex,
            },
            OperationKey::Accounts,
        ] {
            let mut snapshot = Snapshot {
                connection: crate::state::Connection {
                    connected: true,
                    ..Default::default()
                },
                ..Default::default()
            };
            snapshot.operations.insert(
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
            assert!(setup.agents.iter().all(|agent| agent.availability
                == if matches!(key, OperationKey::Models { provider } if provider != agent.provider)
                {
                    AgentAvailability::Checking
                } else {
                    AgentAvailability::Unavailable
                }));
            Arc::make_mut(&mut snapshot.catalog.account).accounts = Some(Arc::new(
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
            snapshot.operations.get_mut(&key).unwrap().phase = OperationPhase::Running;
            assert_eq!(
                snapshot.connection_setup().agents[0].availability,
                AgentAvailability::Checking
            );
            snapshot.catalog.models = Arc::new(
                serde_json::from_value(serde_json::json!([{
                    "id":"gpt","model":{"provider":"codex","id":"gpt"},"displayName":"GPT",
                    "settings":{"defaultReasoningEffort":"","supportedReasoningEfforts":[]}
                }]))
                .unwrap(),
            );
            assert!(snapshot.connection_setup().can_start);
            snapshot.operations.get_mut(&key).unwrap().phase = OperationPhase::Failed {
                message: "refresh failed".into(),
            };
            assert!(
                snapshot.connection_setup().can_start,
                "cached ready providers survive a failed refresh"
            );
            snapshot.connection.connected = false;
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
            account_failed in proptest::bool::ANY,
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
                "errors": if account_failed { serde_json::json!({(key):"account probe failed"}) } else { serde_json::json!({}) }
            })).unwrap();
            let errors = if failed { Map::from_iter([(key.into(), Value::Null)]) } else { Map::new() };
            let actual = availability(connected, provider, &models, Some(&accounts), &errors);
            proptest::prop_assert_eq!(actual == AgentAvailability::Ready, connected && authenticated && catalog && !failed && !account_failed);
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
            connection: crate::state::Connection {
                connected: true,
                ..Default::default()
            },
            catalog: crate::state::Catalog {
                models: std::sync::Arc::new(serde_json::from_value(serde_json::json!([
                    {"id":"gpt", "model":{"provider":"codex","id":"gpt"}, "displayName":"GPT", "settings": { "defaultReasoningEffort":"", "supportedReasoningEfforts":[]}},
                    {"id":"sonnet", "model":{"provider":"claude","id":"sonnet"}, "displayName":"Sonnet", "settings": { "defaultReasoningEffort":"", "supportedReasoningEfforts":[]}}
                ])).unwrap()),
                ..Default::default()
            },
            ..Default::default()
        };
        std::sync::Arc::make_mut(&mut snapshot.catalog.account).accounts = Some(std::sync::Arc::new(
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
        snapshot.connection.connected = false;
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
            errors: Default::default(),
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

#[cfg(test)]
mod status_tests {
    use super::*;
    #[test]
    fn connection_copy_uses_current_language_and_the_saved_enabled_flag() {
        for (connected, enabled, japanese, english) in [
            (false, true, "未接続", "Disconnected"),
            (true, true, "接続済み", "Connected"),
            (false, false, "オフ", "Off"),
            (true, false, "オフ", "Off"),
        ] {
            assert_eq!(
                connection_status_label(UiLanguage::Japanese, connected, enabled),
                japanese
            );
            assert_eq!(
                connection_status_label(UiLanguage::English, connected, enabled),
                english
            );
        }
    }
}
