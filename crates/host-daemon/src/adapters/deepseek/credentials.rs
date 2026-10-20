//! Own the current DSH credential file and expose secret-free route status.
use super::{Failure, KeyProvider, failure};
use agent_protocol::dsh::{CredentialRoute, CredentialSource, Credentials};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};
use tokio::io::AsyncReadExt;
use zeroize::{Zeroize, Zeroizing};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ManagedKeys {
    version: u8,
    refs: BTreeMap<String, String>,
}
impl Default for ManagedKeys {
    fn default() -> Self {
        Self {
            version: 1,
            refs: BTreeMap::new(),
        }
    }
}
impl Drop for ManagedKeys {
    fn drop(&mut self) {
        self.refs.values_mut().for_each(Zeroize::zeroize);
    }
}
impl ManagedKeys {
    pub async fn load(directory: &Path) -> Result<Self, Failure> {
        let file = match tokio::fs::File::open(directory.join("dsh-keys.json")).await {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(failure(error)),
        };
        let mut bytes = Zeroizing::new(Vec::new());
        file.take(64 * 1024 + 1)
            .read_to_end(&mut bytes)
            .await
            .map_err(failure)?;
        if bytes.len() > 64 * 1024 {
            return Err(failure("DSH APIキー設定が大きすぎます。"));
        }
        let keys: Self = serde_json::from_slice(&bytes)
            .map_err(|_| failure("DSH APIキー設定を読み込めません。"))?;
        if keys.version != 1
            || keys.refs.keys().any(|reference| {
                !KeyProvider::ALL
                    .iter()
                    .any(|provider| provider.reference() == reference)
            })
        {
            return Err(failure("DSH APIキー設定の形式が不正です。"));
        }
        Ok(keys)
    }
    pub fn get(&self, provider: KeyProvider) -> Option<&str> {
        self.refs.get(provider.reference()).map(String::as_str)
    }
    pub fn is_empty(&self) -> bool {
        self.refs.is_empty()
    }
    pub fn set(&mut self, provider: KeyProvider, key: Option<&str>) {
        if let Some(mut previous) = self.refs.remove(provider.reference()) {
            previous.zeroize();
        }
        if let Some(key) = key {
            self.refs
                .insert(provider.reference().into(), key.to_owned());
        }
    }
    pub async fn save(&self, directory: &Path) -> Result<(), Failure> {
        // Retire generated data before committing the authoritative file. A failed
        // replacement leaves the previous credentials authoritative and regenerable.
        match tokio::fs::remove_file(directory.join("dsh-runtime-credentials.json")).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(failure(error)),
        }
        crate::platform::save_private_json(&directory.join("dsh-keys.json"), self).map_err(failure)
    }
    pub fn status(&self, external: bool) -> Credentials {
        Credentials {
            routes: KeyProvider::ALL
                .into_iter()
                .map(|provider| {
                    let key = self.get(provider);
                    CredentialRoute {
                        provider,
                        source: if key.is_some() {
                            CredentialSource::Managed
                        } else if provider == KeyProvider::DeepSeek && external {
                            CredentialSource::External
                        } else {
                            CredentialSource::Missing
                        },
                        needs_attention: key.is_some_and(|key| misplaced_key(provider, key)),
                    }
                })
                .collect(),
        }
    }
}
pub(super) fn misplaced_key(provider: KeyProvider, key: &str) -> bool {
    provider != KeyProvider::OpenRouter && key.starts_with("sk-or-")
}
pub(super) fn validate_key(provider: KeyProvider, key: Option<&str>) -> Result<(), Failure> {
    let Some(key) = key else {
        return Ok(());
    };
    if key.is_empty()
        || key.len() > 4096
        || key
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err(failure("有効なAPIキーを入力してください。"));
    }
    if misplaced_key(provider, key) {
        return Err(Failure::new(
            "dsh_key_provider_mismatch",
            "OpenRouterのキーです。接続先にOpenRouterを選んで保存してください。",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn valid_key_boundaries_are_accepted(provider in 0usize..9, length in 1usize..=4096) {
            let key = "k".repeat(length);
            prop_assert!(validate_key(KeyProvider::ALL[provider], Some(&key)).is_ok());
            prop_assert!(validate_key(KeyProvider::ALL[provider], None).is_ok());
        }
    }
    #[test]
    fn maximum_key_length_is_accepted_and_larger_values_are_rejected() {
        assert!(validate_key(KeyProvider::OpenRouter, Some(&"k".repeat(4096))).is_ok());
        assert!(validate_key(KeyProvider::OpenRouter, Some(&"k".repeat(4097))).is_err());
    }
    #[test]
    fn routed_key_validation_and_metadata_agree() {
        for provider in KeyProvider::ALL {
            let mut keys = ManagedKeys::default();
            keys.set(provider, Some("sk-or-test-only"));
            let route = keys
                .status(false)
                .routes
                .into_iter()
                .find(|route| route.provider == provider)
                .unwrap();
            assert_eq!(route.needs_attention, provider != KeyProvider::OpenRouter);
            assert_eq!(
                validate_key(provider, Some("sk-or-test-only")).is_ok(),
                provider == KeyProvider::OpenRouter
            );
        }
    }
}
