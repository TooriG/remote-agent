use super::*;
use crate::client::ClientExt;

#[cfg_attr(feature = "bindings", derive(uniffi::Record))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListAccounts {}
rpc::rpc_method!(ListAccounts, ListAccounts, |self| crate::models::Empty {});

impl Operation for ListAccounts {
    fn key(&self) -> Option<OperationKey> {
        Some(OperationKey::Accounts)
    }
    fn scheduling(&self) -> Scheduling {
        Scheduling::LatestHostState(OperationKey::Accounts)
    }
    rpc_operation!();
    fn apply(
        self,
        snapshot: &mut Snapshot,
        _pane_id: &str,
        mut output: Self::Output,
    ) -> Vec<Effect> {
        let effects = output
            .accounts
            .iter_mut()
            .map(|account| {
                account.usage = snapshot
                    .catalog
                    .account
                    .accounts
                    .as_ref()
                    .and_then(|previous| {
                        previous.accounts.iter().find(|old| {
                            old.provider == account.provider
                                && old.id == account.id
                                && old.email == account.email
                        })
                    })
                    .and_then(|old| old.usage.clone());
                Effect::execute(ReadAccountUsage {
                    provider: account.provider,
                    id: account.id.clone(),
                    email: account.email.clone(),
                })
            })
            .collect();
        Arc::make_mut(&mut snapshot.catalog.account).accounts = Some(Arc::new(output));
        snapshot.normalize_model_drafts();
        effects
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadAccountUsage {
    pub provider: crate::session::ProviderKind,
    #[serde(rename = "accountId")]
    pub id: String,
    // The native profile can change identity while a usage read is in flight.
    #[serde(skip)]
    email: Option<String>,
}

rpc::rpc_method!(ReadAccountUsage, ReadAccountUsage, |self| {
    agent_protocol::operations::ReadAccountUsage {
        provider: self.provider,
        id: self.id.clone(),
    }
});

impl Operation for ReadAccountUsage {
    rpc_operation!();
    fn apply(self, snapshot: &mut Snapshot, _pane_id: &str, output: Self::Output) -> Vec<Effect> {
        if let Some(accounts) = &mut Arc::make_mut(&mut snapshot.catalog.account).accounts
            && let Some(account) = Arc::make_mut(accounts).accounts.iter_mut().find(|account| {
                account.provider == self.provider
                    && account.id == self.id
                    && account.email == self.email
            })
        {
            account.usage = Some(output);
        }
        Vec::new()
    }
}

pub use agent_protocol::operations::SelectAccount;

impl Operation for SelectAccount {
    fn key(&self) -> Option<OperationKey> {
        Some(OperationKey::Accounts)
    }
    rpc_operation!();
    fn apply(self, snapshot: &mut Snapshot, pane_id: &str, output: Self::Output) -> Vec<Effect> {
        apply_selection(snapshot, pane_id, output);
        vec![
            Effect::execute(ListAccounts {}),
            Effect::execute(LoadModels {
                provider: self.provider,
            }),
        ]
    }
}

fn apply_selection(snapshot: &mut Snapshot, pane_id: &str, output: rpc::AccountSelection) {
    snapshot.catalog.composer_catalogs = Arc::default();
    let rpc::AccountSelection {
        selected_id,
        provider,
        persistence_error,
    } = output;
    if let Some(accounts) = &mut Arc::make_mut(&mut snapshot.catalog.account).accounts {
        let accounts = Arc::make_mut(accounts);
        accounts.selected.insert(provider, selected_id);
    }
    if let Some(error) = persistence_error
        && let Some(pane) = snapshot.panes.get_mut(pane_id)
    {
        Arc::make_mut(pane).error = Some(error);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(uniffi::Record))]
pub struct SelectAccountForDraft {
    pub provider: crate::session::ProviderKind,
    pub id: String,
    pub thread_id: DraftKey,
}

impl Operation for SelectAccountForDraft {
    fn key(&self) -> Option<OperationKey> {
        Some(OperationKey::Accounts)
    }
    no_input!();
    type Output = (rpc::AccountSelection, Result<rpc::ModelPage, PeerError>);

    async fn run(
        &self,
        _: Self::Input,
        context: &mut Execution<'_>,
    ) -> Result<Self::Output, PeerError> {
        let selection = context
            .call(&SelectAccount {
                provider: self.provider,
                id: self.id.clone(),
            })
            .await?;
        Ok((selection, context.client.models(self.provider).await))
    }

    fn apply(
        self,
        snapshot: &mut Snapshot,
        pane_id: &str,
        (selection, catalog): Self::Output,
    ) -> Vec<Effect> {
        let previous_provider = snapshot.model_provider_for_draft(self.thread_id.clone());
        let mut draft = snapshot
            .input
            .drafts
            .get(&self.thread_id)
            .map(|draft| (**draft).clone())
            .unwrap_or_default();
        let provider = selection.provider;
        apply_selection(snapshot, pane_id, selection);
        match catalog {
            Ok(catalog) => {
                LoadModels { provider }.apply(snapshot, pane_id, catalog);
                if snapshot
                    .catalog
                    .models
                    .iter()
                    .any(|model| model.model.provider == provider)
                {
                    if previous_provider != Some(provider) {
                        let selection =
                            snapshot.catalog.model_preferences.selections.get(&provider);
                        draft.model = selection.map(|selection| selection.model.clone());
                        draft.effort = selection.and_then(|selection| selection.effort.clone());
                        draft.service_tier =
                            selection.and_then(|selection| selection.service_tier.clone());
                    }
                    let (model, effort, tier) = supported_settings(
                        draft.model.as_ref(),
                        draft.effort.as_deref(),
                        draft.service_tier.as_deref(),
                        Some(provider),
                        &snapshot.catalog.models,
                        &snapshot.catalog.model_errors,
                        snapshot.allows_custom_codex_models(),
                    );
                    let settings = (
                        model.cloned(),
                        effort.map(str::to_owned),
                        tier.map(str::to_owned),
                    );
                    (draft.model, draft.effort, draft.service_tier) = settings;
                    Arc::make_mut(&mut snapshot.input.drafts)
                        .insert(self.thread_id, Arc::new(draft));
                }
            }
            Err(error) => {
                if let Some(pane) = snapshot.panes.get_mut(pane_id) {
                    Arc::make_mut(pane).error = Some(error.to_string());
                }
            }
        }
        vec![Effect::execute(ListAccounts {})]
    }
}

pub use agent_protocol::operations::SubmitAccountLogin;

/// A pending login is confirmed by the Store; screens only start, answer, or cancel it.
const LOGIN_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

/// Reads the pending login after the poll interval, while one is pending.
pub(in crate::state) fn poll_account_login(snapshot: &Snapshot) -> Option<Effect> {
    let login = snapshot.catalog.account.login.as_ref()?;
    Some(Effect::execute(PollAccountLogin(ReadAccountLogin {
        provider: login.provider,
        id: login.login_id.clone(),
    })))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(uniffi::Record))]
pub struct StartAccountLogin {
    pub provider: crate::session::ProviderKind,
    /// The draft that adopts the new account; otherwise the provider's selection changes.
    pub thread_id: Option<DraftKey>,
}
rpc::rpc_method!(StartAccountLogin, StartAccountLogin, |self| {
    agent_protocol::operations::StartAccountLogin {
        provider: self.provider,
    }
});

impl Operation for StartAccountLogin {
    fn key(&self) -> Option<OperationKey> {
        Some(OperationKey::AccountLogin {
            provider: self.provider,
        })
    }
    rpc_operation!();
    fn apply(self, snapshot: &mut Snapshot, _pane_id: &str, login: Self::Output) -> Vec<Effect> {
        let account = Arc::make_mut(&mut snapshot.catalog.account);
        account.login = Some(Arc::new(login));
        account.login_draft = self.thread_id;
        poll_account_login(snapshot).into_iter().collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(uniffi::Record))]
pub struct ReadAccountLogin {
    pub provider: crate::session::ProviderKind,
    pub id: String,
}
rpc::rpc_method!(ReadAccountLogin, ReadAccountLogin, |self| {
    agent_protocol::operations::ReadAccountLogin {
        provider: self.provider,
        id: self.id.clone(),
    }
});

impl Operation for ReadAccountLogin {
    fn key(&self) -> Option<OperationKey> {
        Some(OperationKey::AccountLogin {
            provider: self.provider,
        })
    }
    no_input!();
    type Output = Result<rpc::AccountLoginStatus, PeerError>;

    async fn run(
        &self,
        _: Self::Input,
        context: &mut Execution<'_>,
    ) -> Result<Self::Output, PeerError> {
        Ok(context.call(self).await)
    }

    fn complete(
        self,
        snapshot: &mut Snapshot,
        _pane_id: &str,
        output: Self::Output,
        current: bool,
    ) -> Result<Vec<Effect>, PeerError> {
        if !current {
            return Ok(Vec::new());
        }
        let status = output.inspect_err(|error| {
            Arc::make_mut(&mut snapshot.catalog.account).discard_ended_login(
                self.provider,
                &self.id,
                error,
            );
        })?;
        if !status.completed {
            return Ok(poll_account_login(snapshot).into_iter().collect());
        }
        let draft =
            Arc::make_mut(&mut snapshot.catalog.account).clear_login(self.provider, &self.id);
        let Some(id) = status.account_id else {
            return Ok(vec![
                Effect::execute(ListAccounts {}),
                Effect::execute(LoadModels {
                    provider: self.provider,
                }),
            ]);
        };
        // The Host owns the login: a pane closed meanwhile still gets the new account.
        Ok(vec![match draft {
            Some(thread_id) => Effect::execute(SelectAccountForDraft {
                provider: self.provider,
                id,
                thread_id,
            }),
            None => Effect::execute(SelectAccount {
                provider: self.provider,
                id,
            }),
        }])
    }
}

/// The Store's own confirmation of a pending login; it shares the login's key,
/// so starting, answering, or cancelling the login supersedes it.
#[derive(Debug)]
pub(in crate::state) struct PollAccountLogin(ReadAccountLogin);

impl Operation for PollAccountLogin {
    fn key(&self) -> Option<OperationKey> {
        self.0.key()
    }
    no_input!();
    type Output = <ReadAccountLogin as Operation>::Output;
    async fn run(
        &self,
        _: Self::Input,
        context: &mut Execution<'_>,
    ) -> Result<Self::Output, PeerError> {
        tokio::time::sleep(LOGIN_POLL_INTERVAL).await;
        Ok(context.call(&self.0).await)
    }
    fn complete(
        self,
        snapshot: &mut Snapshot,
        pane_id: &str,
        output: Self::Output,
        current: bool,
    ) -> Result<Vec<Effect>, PeerError> {
        self.0.complete(snapshot, pane_id, output, current)
    }
}

pub use agent_protocol::operations::CancelAccountLogin;

impl Operation for CancelAccountLogin {
    fn key(&self) -> Option<OperationKey> {
        Some(OperationKey::AccountLogin {
            provider: self.provider,
        })
    }
    rpc_operation!();
    fn apply(self, snapshot: &mut Snapshot, _pane_id: &str, _output: Self::Output) -> Vec<Effect> {
        Arc::make_mut(&mut snapshot.catalog.account).clear_login(self.provider, &self.id);
        // Cancelling an obsolete form supersedes the poll of the login that continues.
        poll_account_login(snapshot).into_iter().collect()
    }
}

pub use agent_protocol::operations::LogoutAccount;

impl Operation for LogoutAccount {
    fn invalidates(&self) -> bool {
        true
    }
    fn key(&self) -> Option<OperationKey> {
        Some(OperationKey::Accounts)
    }
    rpc_operation!();
    fn apply(self, snapshot: &mut Snapshot, _pane_id: &str, _output: Self::Output) -> Vec<Effect> {
        Arc::make_mut(&mut snapshot.catalog.account).accounts = None;
        let mut effects = vec![
            Effect::execute(ListAccounts {}),
            Effect::execute(LoadModels {
                provider: self.provider,
            }),
        ];
        if self.provider == crate::session::ProviderKind::DeepSeek {
            Arc::make_mut(&mut snapshot.catalog.account).dsh_credentials = None;
            effects.push(Effect::execute(ReadDshCredentials {}));
        }
        effects
    }
}

impl Operation for SubmitAccountLogin {
    fn key(&self) -> Option<OperationKey> {
        Some(OperationKey::AccountLogin {
            provider: self.provider,
        })
    }
    no_input!();
    type Output = Result<crate::models::Empty, PeerError>;

    async fn run(
        &self,
        _: Self::Input,
        context: &mut Execution<'_>,
    ) -> Result<Self::Output, PeerError> {
        Ok(context.call(self).await)
    }

    fn complete(
        self,
        snapshot: &mut Snapshot,
        _pane_id: &str,
        output: Self::Output,
        current: bool,
    ) -> Result<Vec<Effect>, PeerError> {
        if !current {
            return Ok(Vec::new());
        }
        output.inspect_err(|error| {
            Arc::make_mut(&mut snapshot.catalog.account).discard_ended_login(
                self.provider,
                &self.id,
                error,
            );
        })?;
        Ok(poll_account_login(snapshot).into_iter().collect())
    }
}

impl AccountState {
    /// Ends this login unless a different one is pending, returning the draft
    /// that was waiting to adopt the new account.
    fn clear_login(
        &mut self,
        provider: crate::session::ProviderKind,
        id: &str,
    ) -> Option<DraftKey> {
        if self
            .login
            .as_ref()
            .is_some_and(|login| login.provider != provider || login.login_id != id)
        {
            return None;
        }
        self.login = None;
        self.login_draft.take()
    }

    fn discard_ended_login(
        &mut self,
        provider: crate::session::ProviderKind,
        id: &str,
        error: &PeerError,
    ) {
        if let PeerError::Remote { error, .. } = error
            && serde_json::from_str::<agent_protocol::error::RpcFailure>(error)
                .is_ok_and(|failure| failure.code == agent_protocol::error::ACCOUNT_LOGIN_ENDED)
        {
            self.clear_login(provider, id);
        }
    }
}

pub use agent_protocol::dsh::{
    KeyUpdate as UpdateDshCredential, ReadCredentials as ReadDshCredentials,
};
impl Operation for ReadDshCredentials {
    fn key(&self) -> Option<OperationKey> {
        Some(OperationKey::DshCredentials)
    }
    rpc_operation!();
    fn apply(
        self,
        snapshot: &mut Snapshot,
        _pane_id: &str,
        credentials: Self::Output,
    ) -> Vec<Effect> {
        Arc::make_mut(&mut snapshot.catalog.account).dsh_credentials = Some(Arc::new(credentials));
        Vec::new()
    }
}
impl Operation for UpdateDshCredential {
    fn invalidates(&self) -> bool {
        true
    }
    fn key(&self) -> Option<OperationKey> {
        Some(OperationKey::DshCredentials)
    }
    rpc_operation!();
    fn apply(
        self,
        snapshot: &mut Snapshot,
        _pane_id: &str,
        credentials: Self::Output,
    ) -> Vec<Effect> {
        Arc::make_mut(&mut snapshot.catalog.account).dsh_credentials = Some(Arc::new(credentials));
        vec![
            Effect::execute(ListAccounts {}),
            Effect::execute(LoadModels {
                provider: crate::session::ProviderKind::DeepSeek,
            }),
        ]
    }
}

#[cfg(test)]
mod account_model_tests {
    use super::*;
    use serde_json::json;

    #[rstest::rstest]
    fn catalogs_and_accounts_can_arrive_in_either_order(#[values(true, false)] models_first: bool) {
        let mut snapshot = Snapshot::default();
        let key = snapshot.panes.primary().navigation.draft_key.clone();
        Arc::make_mut(&mut snapshot.input.drafts).insert(
            key.clone(),
            Arc::new(Draft {
                text: "keep this input".into(),
                ..Default::default()
            }),
        );
        let explicit_key = DraftKey::from("explicit");
        let explicit = Arc::new(Draft {
            model: Some(crate::models::ModelRef {
                provider: crate::session::ProviderKind::Codex,
                id: "gpt".into(),
            }),
            effort: Some("high".into()),
            service_tier: Some("default".into()),
            ..Default::default()
        });
        Arc::make_mut(&mut snapshot.input.drafts).insert(explicit_key.clone(), explicit.clone());
        let accounts: rpc::Accounts = serde_json::from_value(json!({
            "accounts":[{"id":"b","provider":"claude"}], "selected":{"claude":"b"}
        }))
        .unwrap();
        let models: rpc::ModelPage = serde_json::from_value(json!({"data":[
            {"id":"gpt","model":{"provider":"codex","id":"gpt"},"displayName":"GPT","isDefault":true, "settings": {"defaultReasoningEffort":"high","supportedReasoningEfforts":[{"reasoningEffort":"high"}]}},
            {"id":"sonnet","model":{"provider":"claude","id":"sonnet"},"displayName":"Sonnet","isDefault":true, "settings": {"defaultReasoningEffort":"","supportedReasoningEfforts":[]}}
        ]})).unwrap();
        if models_first {
            crate::test_models::apply_catalog(
                &mut snapshot,
                rpc::ModelPage {
                    data: models.data.clone(),
                    next_cursor: None,
                    provider_errors: None,
                },
            );
        } else {
            ListAccounts {}.apply(&mut snapshot, "", accounts.clone());
        }
        assert!(snapshot.model_provider_for_draft(key.clone()).is_none());
        assert_eq!(snapshot.input.drafts[&explicit_key], explicit);
        if models_first {
            ListAccounts {}.apply(&mut snapshot, "", accounts);
        } else {
            crate::test_models::apply_catalog(&mut snapshot, models);
        }
        assert_eq!(
            snapshot.model_provider_for_draft(key.clone()),
            Some(crate::session::ProviderKind::Claude)
        );
        assert_eq!(snapshot.input.drafts[&key].text, "keep this input");
        assert_eq!(snapshot.input.drafts[&explicit_key], explicit);
    }

    #[test]
    fn usage_updates_only_its_account_and_survives_list_refresh() {
        let mut snapshot = Snapshot::default();
        let accounts: rpc::Accounts = serde_json::from_value(json!({
            "accounts": [{"id":"a","provider":"codex","email":"a@example.invalid"}, {"id":"b","provider":"codex"}, {"id":"a","provider":"claude","email":"a@example.invalid"}],
            "selected":{"codex":"b","claude":"a"}
        })).unwrap();
        let effects = ListAccounts {}.apply(&mut snapshot, "", accounts.clone());
        assert_eq!(effects.len(), 3);
        let usage = rpc::AccountUsage {
            windows: vec![],
            fetched_at: 1,
            error: Some("unavailable".into()),
        };
        ReadAccountUsage {
            provider: crate::session::ProviderKind::Codex,
            id: "a".into(),
            email: Some("a@example.invalid".into()),
        }
        .apply(&mut snapshot, "", usage.clone());
        ListAccounts {}.apply(&mut snapshot, "", accounts);
        let listed = snapshot.catalog.account.accounts.as_ref().unwrap();
        assert_eq!(
            listed
                .selected
                .get(&crate::session::ProviderKind::Codex)
                .map(String::as_str),
            Some("b")
        );
        assert_eq!(listed.accounts[0].usage.as_ref(), Some(&usage));
        assert!(listed.accounts[1].usage.is_none());
        assert!(listed.accounts[2].usage.is_none());
        assert!(!listed.is_selected(&listed.accounts[0]));
        assert!(listed.is_selected(&listed.accounts[1]));
        assert!(listed.is_selected(&listed.accounts[2]));
        let mut changed = (**listed).clone();
        changed.accounts[0].email = Some("different@example.invalid".into());
        ListAccounts {}.apply(&mut snapshot, "", changed);
        ReadAccountUsage {
            provider: crate::session::ProviderKind::Codex,
            id: "a".into(),
            email: Some("a@example.invalid".into()),
        }
        .apply(&mut snapshot, "", usage.clone());
        assert!(
            snapshot.catalog.account.accounts.as_ref().unwrap().accounts[0]
                .usage
                .is_none()
        );
        ListAccounts {}.apply(
            &mut snapshot,
            "",
            serde_json::from_value(json!({"accounts":[],"selected":{}})).unwrap(),
        );
        ReadAccountUsage {
            provider: crate::session::ProviderKind::Codex,
            id: "a".into(),
            email: Some("a@example.invalid".into()),
        }
        .apply(&mut snapshot, "", usage);
        assert!(
            snapshot
                .catalog
                .account
                .accounts
                .as_ref()
                .unwrap()
                .accounts
                .is_empty()
        );
    }

    #[test]
    fn account_selection_preserves_supported_settings_and_normalizes_new_catalog() {
        let mut snapshot = Snapshot::default();
        Arc::make_mut(&mut snapshot.catalog.account).accounts = Some(Arc::new(serde_json::from_value(json!({
            "accounts":[{"id":"a","provider":"codex"},{"id":"b","provider":"codex"},{"id":"claude:c","provider":"claude"}],
            "selected":{"codex":"a","claude":"claude:c"}
        })).unwrap()));
        let codex: Model = serde_json::from_value(json!({"id":"gpt","model":{"provider": "codex", "id": "gpt"},"displayName":"GPT", "settings": {"defaultReasoningEffort":"medium","supportedReasoningEfforts":[{"reasoningEffort":"medium"},{"reasoningEffort":"high"}],"serviceTiers":[{"id":"fast","fast":true}]}})).unwrap();
        let claude: Model = serde_json::from_value(json!({"id":"claude:sonnet","model":{"provider": "claude", "id": "sonnet"},"displayName":"Sonnet", "settings": {"defaultReasoningEffort":"high","supportedReasoningEfforts":[{"reasoningEffort":"high"}]}})).unwrap();
        snapshot.catalog.models = Arc::new(vec![codex.clone(), claude.clone()]);
        Arc::make_mut(&mut snapshot.input.drafts).insert(
            "draft".into(),
            Arc::new(Draft {
                model: Some(agent_protocol::models::ModelRef {
                    provider: agent_protocol::session::ProviderKind::Codex,
                    id: "gpt".into(),
                }),
                effort: Some("medium".into()),
                service_tier: Some("fast".into()),
                ..Default::default()
            }),
        );
        let select = |snapshot: &mut Snapshot, id: &str, provider, data| {
            SelectAccountForDraft {
                provider,
                id: id.into(),
                thread_id: "draft".into(),
            }
            .apply(
                snapshot,
                "",
                (
                    rpc::AccountSelection {
                        selected_id: id.into(),
                        provider,
                        persistence_error: None,
                    },
                    Ok(rpc::ModelPage {
                        data,
                        next_cursor: None,
                        provider_errors: None,
                    }),
                ),
            );
        };
        select(
            &mut snapshot,
            "b",
            crate::session::ProviderKind::Codex,
            vec![codex.clone(), claude.clone()],
        );
        assert_eq!(
            snapshot.input.drafts[&crate::state::DraftKey::from("draft")]
                .model
                .as_ref(),
            Some(&agent_protocol::models::ModelRef {
                provider: agent_protocol::session::ProviderKind::Codex,
                id: "gpt".into()
            })
        );
        assert_eq!(
            snapshot.input.drafts[&crate::state::DraftKey::from("draft")]
                .effort
                .as_deref(),
            Some("medium")
        );
        assert_eq!(
            snapshot.input.drafts[&crate::state::DraftKey::from("draft")]
                .service_tier
                .as_deref(),
            Some("fast")
        );
        select(
            &mut snapshot,
            "claude:c",
            crate::session::ProviderKind::Claude,
            vec![codex.clone(), claude.clone()],
        );
        let draft = &snapshot.input.drafts[&crate::state::DraftKey::from("draft")];
        assert_eq!(
            draft.model.as_ref(),
            Some(&agent_protocol::models::ModelRef {
                provider: agent_protocol::session::ProviderKind::Claude,
                id: "sonnet".into()
            })
        );
        assert_eq!(draft.effort.as_deref(), Some("high"));
        assert_eq!(draft.service_tier.as_deref(), Some("default"));
        assert_eq!(
            snapshot
                .catalog
                .account
                .accounts
                .as_ref()
                .unwrap()
                .selected
                .get(&crate::session::ProviderKind::Claude)
                .map(String::as_str),
            Some("claude:c")
        );
        crate::test_models::apply_catalog(
            &mut snapshot,
            rpc::ModelPage {
                data: vec![codex.clone()],
                next_cursor: None,
                provider_errors: None,
            },
        );
        assert_eq!(
            snapshot.input.drafts[&crate::state::DraftKey::from("draft")]
                .model
                .as_ref(),
            Some(&agent_protocol::models::ModelRef {
                provider: agent_protocol::session::ProviderKind::Claude,
                id: "sonnet".into()
            })
        );
        let mut restricted = codex;
        restricted
            .settings
            .as_mut()
            .unwrap()
            .supported_reasoning_efforts
            .truncate(1);
        restricted.settings.as_mut().unwrap().service_tiers = None;
        select(
            &mut snapshot,
            "b",
            crate::session::ProviderKind::Codex,
            vec![restricted.clone(), claude],
        );
        assert_eq!(
            snapshot
                .catalog
                .account
                .accounts
                .as_ref()
                .unwrap()
                .selected
                .get(&crate::session::ProviderKind::Codex)
                .map(String::as_str),
            Some("b")
        );
        assert_eq!(
            crate::presentation::view::Presentation::from_snapshot(
                Arc::new(snapshot.clone()),
                "".into()
            )
            .unwrap()
            .settings()
            .models_matching(Some(crate::session::ProviderKind::Codex), String::new()),
            vec![restricted]
        );
        assert_eq!(
            snapshot.input.drafts[&crate::state::DraftKey::from("draft")]
                .effort
                .as_deref(),
            Some("medium")
        );
        assert_eq!(
            snapshot.input.drafts[&crate::state::DraftKey::from("draft")]
                .service_tier
                .as_deref(),
            Some("default")
        );
    }
}
