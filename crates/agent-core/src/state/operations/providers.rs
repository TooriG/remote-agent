use super::*;
pub use agent_protocol::providers::{ReadProviderSettings, SetProviderEnabled};

impl Operation for ReadProviderSettings {
    rpc_operation!();
    fn key(&self) -> Option<OperationKey> {
        Some(OperationKey::ProviderSettings)
    }
    fn scheduling(&self) -> Scheduling {
        Scheduling::LatestHostState(OperationKey::ProviderSettings)
    }
    fn apply(self, snapshot: &mut Snapshot, settings: Self::Output) -> Vec<Effect> {
        snapshot.accept_provider_settings(settings);
        let mut effects = refresh_models();
        effects.push(Effect::execute(ListAccounts {}));
        effects
    }
}

impl Operation for SetProviderEnabled {
    rpc_operation!();
    fn key(&self) -> Option<OperationKey> {
        Some(OperationKey::ProviderSettings)
    }
    fn scheduling(&self) -> Scheduling {
        Scheduling::Control
    }
    fn apply(self, snapshot: &mut Snapshot, settings: Self::Output) -> Vec<Effect> {
        if snapshot.accept_provider_settings(settings) {
            refresh_models()
        } else {
            Vec::new()
        }
    }
}

impl Snapshot {
    pub(crate) fn accept_provider_settings(
        &mut self,
        settings: agent_protocol::providers::ProviderSettings,
    ) -> bool {
        if self
            .catalog
            .provider_settings
            .as_ref()
            .is_some_and(|current| current.revision >= settings.revision)
        {
            return false;
        }
        self.catalog.provider_settings = Some(Arc::new(settings));
        let settings = self.catalog.provider_settings.as_ref().unwrap();
        Arc::make_mut(&mut self.catalog.models)
            .retain(|model| settings.enabled(model.model.provider));
        self.catalog.composer_catalogs = Arc::default();
        self.normalize_model_drafts();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_protocol::{providers::ProviderSettings, session::ProviderKind};

    #[test]
    fn disabled_providers_stay_out_of_the_picker_when_an_old_catalog_finishes() {
        let mut snapshot = Snapshot {
            connection: crate::state::Connection {
                connected: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let catalog = || {
            serde_json::from_value(serde_json::json!({
                "data":[{"id":"gpt","model":{"provider":"codex","id":"gpt"},"displayName":"GPT"}]
            }))
            .unwrap()
        };
        LoadModels {
            provider: ProviderKind::Codex,
        }
        .apply(&mut snapshot, catalog());
        snapshot.accept_provider_settings(
            ProviderSettings::default().with_enabled(ProviderKind::Codex, false),
        );
        LoadModels {
            provider: ProviderKind::Codex,
        }
        .apply(&mut snapshot, catalog());
        assert!(
            snapshot
                .models_matching(Some(ProviderKind::Codex), String::new())
                .is_empty()
        );
        assert!(
            snapshot
                .custom_model_choice(Some(ProviderKind::Codex), "custom")
                .is_none()
        );
        let setup = snapshot.connection_setup();
        let codex = setup
            .agents
            .iter()
            .find(|agent| agent.provider == ProviderKind::Codex)
            .unwrap();
        assert!(!codex.enabled);
        assert_eq!(codex.label, "オフ");
        assert!(codex.error.is_none());
        assert!(!setup.can_start);
        snapshot.accept_provider_settings(
            snapshot
                .catalog
                .provider_settings
                .as_ref()
                .unwrap()
                .with_enabled(ProviderKind::Codex, true),
        );
        LoadModels {
            provider: ProviderKind::Codex,
        }
        .apply(&mut snapshot, catalog());
        assert_eq!(
            snapshot
                .models_matching(Some(ProviderKind::Codex), String::new())
                .len(),
            1
        );
    }

    #[test]
    fn stale_reads_cannot_undo_a_newer_provider_toggle() {
        let mut snapshot = Snapshot::default();
        let initial = ProviderSettings::default();
        let disabled = initial.with_enabled(ProviderKind::Codex, false);
        assert!(snapshot.accept_provider_settings(disabled.clone()));
        assert!(!snapshot.provider_enabled(ProviderKind::Codex));
        assert!(snapshot.provider_enabled(ProviderKind::Claude));
        assert!(!snapshot.accept_provider_settings(initial));
        assert!(!snapshot.accept_provider_settings(disabled.clone()));
        assert!(!snapshot.provider_enabled(ProviderKind::Codex));
        snapshot.accept_provider_settings(disabled.with_enabled(ProviderKind::Codex, true));
        assert!(snapshot.provider_enabled(ProviderKind::Codex));
    }
}
