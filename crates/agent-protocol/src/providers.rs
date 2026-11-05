//! Host-owned provider enablement. Disabling new work retains native history and accounts.
use crate::session::ProviderKind;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderSettings {
    pub revision: u64,
    pub disabled: Vec<ProviderKind>,
}

impl ProviderSettings {
    pub fn enabled(&self, provider: ProviderKind) -> bool {
        !self.disabled.contains(&provider)
    }

    pub fn with_enabled(&self, provider: ProviderKind, enabled: bool) -> Self {
        if self.enabled(provider) == enabled {
            return self.clone();
        }
        let mut disabled = self.disabled.clone();
        disabled.retain(|value| *value != provider);
        if !enabled {
            disabled.push(provider);
            disabled.sort();
        }
        Self {
            revision: self.revision.saturating_add(1),
            disabled,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadProviderSettings {}
crate::rpc_method!(ReadProviderSettings, ReadProviderSettings, |self| {
    crate::models::Empty {}
});

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetProviderEnabled {
    pub provider: ProviderKind,
    pub enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enablement_is_idempotent_and_changes_only_the_named_provider() {
        let initial = ProviderSettings::default();
        assert_eq!(initial.with_enabled(ProviderKind::Codex, true), initial);
        let disabled = initial.with_enabled(ProviderKind::Codex, false);
        assert!(!disabled.enabled(ProviderKind::Codex));
        assert!(disabled.enabled(ProviderKind::Claude));
        assert!(initial.enabled(ProviderKind::Codex));
        assert_eq!(disabled.revision, 1);
        assert_eq!(disabled.with_enabled(ProviderKind::Codex, false), disabled);
        let enabled = disabled.with_enabled(ProviderKind::Codex, true);
        assert!(enabled.disabled.is_empty());
        assert_eq!(enabled.revision, 2);
    }
}
