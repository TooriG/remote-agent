//! Started providers, providers that failed to start, and reads fanned out
//! across all of them. One provider's failure never hides the others.
use super::{Agent, Backend, Failure, Identity};
use crate::dictation::Dictation;
use agent_protocol::{composer::ComposerCatalog, operations as op, session::ProviderKind};
use std::{collections::HashMap, path::Path, path::PathBuf, sync::Arc};

// Finish discovery before the client's request deadline, even when a native
// provider is still initializing. Healthy providers must remain usable.
const MODEL_LIST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

pub(crate) struct Registry {
    agents: HashMap<ProviderKind, Arc<dyn Agent>>,
    startup_errors: HashMap<ProviderKind, Failure>,
}

impl Registry {
    /// Also returns the dictation backend supplied by the backends, if any.
    pub(crate) fn new(backends: impl IntoIterator<Item = Backend>) -> (Self, Dictation) {
        let mut agents = HashMap::new();
        let mut startup_errors = HashMap::new();
        let mut dictation = Dictation::default();
        for backend in backends {
            match backend.agent {
                Ok(agent) => {
                    agents.insert(backend.provider, agent);
                }
                Err(error) => {
                    startup_errors.insert(backend.provider, error);
                }
            }
            if let Some(backend) = backend.dictation {
                dictation = backend;
            }
        }
        (
            Self {
                agents,
                startup_errors,
            },
            dictation,
        )
    }

    pub(crate) fn get(&self, provider: ProviderKind) -> Result<Arc<dyn Agent>, Failure> {
        self.agents.get(&provider).cloned().ok_or_else(|| {
            self.startup_errors
                .get(&provider)
                .cloned()
                .unwrap_or_else(|| {
                    Failure::new(
                        "provider_unavailable",
                        format!("{provider:?} is unavailable"),
                    )
                })
        })
    }

    /// Started providers in provider order.
    pub(crate) fn all(&self) -> Vec<(ProviderKind, Arc<dyn Agent>)> {
        let mut agents: Vec<_> = self.agents.iter().map(|(p, a)| (*p, a.clone())).collect();
        agents.sort_by_key(|(p, _)| *p);
        agents
    }

    pub(crate) fn startup_errors(&self) -> impl Iterator<Item = (ProviderKind, &Failure)> {
        self.startup_errors
            .iter()
            .map(|(provider, error)| (*provider, error))
    }

    pub(crate) async fn accounts(&self) -> op::Accounts {
        let mut combined = op::Accounts {
            accounts: Vec::new(),
            selected: HashMap::new(),
            error: None,
        };
        let mut errors = self
            .startup_errors
            .values()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        for (_, agent) in self.all() {
            match Identity::list(agent.as_ref()).await {
                Ok(accounts) => {
                    combined.accounts.extend(accounts.accounts);
                    combined.selected.extend(accounts.selected);
                    errors.extend(accounts.error);
                }
                Err(error) => errors.push(error.to_string()),
            }
        }
        combined.error = (!errors.is_empty()).then(|| errors.join("\n"));
        combined
    }

    pub(crate) async fn catalog(&self, cwd: &str) -> ComposerCatalog {
        let results = futures_util::future::join_all(
            self.all()
                .into_iter()
                .map(|(_, agent)| async move { agent.catalog(cwd).await }),
        )
        .await;
        let mut catalog = ComposerCatalog {
            cwd: cwd.to_owned(),
            ..Default::default()
        };
        for (provider, error) in self.startup_errors.iter() {
            catalog.errors.insert(*provider, vec![error.to_string()]);
        }
        for result in results {
            catalog.candidates.extend(result.candidates);
            catalog.errors.extend(result.errors);
        }
        catalog.candidates.sort_by(|a, b| {
            a.invocation
                .name
                .to_lowercase()
                .cmp(&b.invocation.name.to_lowercase())
                .then(a.invocation.path.cmp(&b.invocation.path))
                .then(a.invocation.provider.cmp(&b.invocation.provider))
        });
        catalog
            .candidates
            .dedup_by(|a, b| a.invocation == b.invocation);
        catalog
    }

    /// The cursor records each provider's own cursor; exhausted providers drop out.
    pub(crate) async fn models(&self, params: &op::ListModels) -> Result<op::ModelPage, Failure> {
        let cursors: std::collections::BTreeMap<ProviderKind, Option<String>> = params
            .cursor
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?
            .unwrap_or_else(|| {
                self.all()
                    .into_iter()
                    .map(|(provider, _)| provider)
                    .chain(self.startup_errors.keys().copied())
                    .map(|provider| (provider, None))
                    .collect()
            });
        let mut page = op::ModelPage {
            data: Vec::new(),
            next_cursor: None,
            provider_errors: None,
        };
        let mut next = std::collections::BTreeMap::new();
        let listings = futures_util::future::join_all(cursors.into_iter().map(
            |(provider, cursor)| async move {
                let result = match self.get(provider) {
                    Ok(agent) => tokio::time::timeout(
                        MODEL_LIST_TIMEOUT,
                        agent.models(&op::ListModels {
                            cursor,
                            limit: params.limit,
                        }),
                    )
                    .await
                    .unwrap_or_else(|_| {
                        Err(Failure::new(
                            "provider_timeout",
                            "モデル一覧の取得がタイムアウトしました。もう一度お試しください。",
                        ))
                    }),
                    Err(error) => Err(error),
                };
                (provider, result)
            },
        ))
        .await;
        for (provider, result) in listings {
            match result {
                Ok(result) => {
                    page.data.extend(result.data);
                    if let Some(cursor) = result.next_cursor {
                        next.insert(provider, Some(cursor));
                    }
                }
                Err(error) => {
                    page.provider_errors
                        .get_or_insert_default()
                        .insert(provider.key().to_owned(), serde_json::to_value(error)?);
                }
            }
        }
        if !next.is_empty() {
            page.next_cursor = Some(serde_json::to_string(&next)?);
        }
        Ok(page)
    }

    /// Identifies the native storage the providers read, independent of path aliases.
    pub(crate) fn storage_scope(&self) -> Result<String, serde_json::Error> {
        storage_scope(self.all().into_iter().filter_map(|(p, a)| {
            a.history_scope_directory()
                .map(|path| (p, canonical_storage_path(path)))
        }))
    }

    pub(crate) fn data_recipients(&self) -> Vec<String> {
        self.all()
            .into_iter()
            .filter(|(_, a)| a.availability().is_ok())
            .map(|(_, agent)| agent.data_recipient().into())
            .collect()
    }

    pub(crate) fn errors(&self) -> serde_json::Value {
        let mut errors: serde_json::Map<_, _> = self
            .all()
            .into_iter()
            .filter_map(|(p, a)| {
                a.availability().err().map(|e| {
                    (
                        p.key().to_owned(),
                        serde_json::to_value(e).expect("failure serializes"),
                    )
                })
            })
            .collect();
        errors.extend(self.startup_errors.iter().map(|(provider, error)| {
            (
                provider.key().to_owned(),
                serde_json::to_value(error).expect("failure serializes"),
            )
        }));
        serde_json::Value::Object(errors)
    }

    pub(crate) async fn discard_workspace_processes(
        &self,
        directory: &Path,
    ) -> Result<(), Failure> {
        for (_, agent) in self.all() {
            agent.discard_workspace_processes(directory).await?;
        }
        Ok(())
    }

    pub(crate) async fn shutdown(&self) {
        for (_, agent) in self.all() {
            agent.shutdown().await;
        }
    }
}

fn storage_scope(
    areas: impl IntoIterator<Item = (ProviderKind, PathBuf)>,
) -> Result<String, serde_json::Error> {
    let areas: std::collections::BTreeMap<_, _> = areas
        .into_iter()
        .map(|(provider, path)| (provider.key().to_owned(), path))
        .collect();
    let bytes = serde_json::to_vec(&areas)?;
    let digest = ring::digest::digest(&ring::digest::SHA256, &bytes);
    Ok(digest
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

// Resolve existing ancestors too: a newly created native directory must not
// change a scope merely because /var is a symlink to /private/var on macOS.
fn canonical_storage_path(path: &Path) -> PathBuf {
    if let Ok(path) = dunce::canonicalize(path) {
        return path;
    }
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) if !parent.as_os_str().is_empty() => {
            canonical_storage_path(parent).join(name)
        }
        _ => std::env::current_dir().unwrap_or_default().join(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_scope_has_a_fixed_encoding() {
        let areas = [
            (ProviderKind::Codex, "/fixtures/codex".into()),
            (ProviderKind::Claude, "/fixtures/claude".into()),
        ];
        assert_eq!(
            storage_scope(areas).unwrap(),
            "8170d6192a00957e1b3b4a3da16963137015f8d36f1efaa8b8b347090f78476b",
        );
    }

    proptest::proptest! {
        #[test]
        fn storage_scope_ignores_input_order_but_tracks_provider_paths(
            codex in "[a-zA-Z0-9/_ .あ界é-]{1,64}",
            claude in "[a-zA-Z0-9/_ .あ界é-]{1,64}",
        ) {
            let codex = std::path::PathBuf::from(codex);
            let claude = std::path::PathBuf::from(claude);
            let original = storage_scope([
                (ProviderKind::Codex, codex.clone()),
                (ProviderKind::Claude, claude.clone()),
            ]).unwrap();
            proptest::prop_assert_eq!(&original, &storage_scope([
                (ProviderKind::Claude, claude.clone()),
                (ProviderKind::Codex, codex.clone()),
            ]).unwrap());
            for areas in [
                [(ProviderKind::Codex, codex.join("different")), (ProviderKind::Claude, claude.clone())],
                [(ProviderKind::Codex, codex.clone()), (ProviderKind::Claude, claude.join("different"))],
            ] {
                proptest::prop_assert_ne!(&original, &storage_scope(areas).unwrap());
            }
            proptest::prop_assert_ne!(&original, &storage_scope([
                (ProviderKind::Codex, codex),
            ]).unwrap());
        }
    }

    #[test]
    fn creating_native_storage_does_not_change_its_identity() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("native").join("projects");
        let before = canonical_storage_path(&path);
        std::fs::create_dir_all(&path).unwrap();
        let scope = storage_scope([(ProviderKind::Codex, before)]).unwrap();
        assert_eq!(
            scope,
            storage_scope([(ProviderKind::Codex, canonical_storage_path(&path)),]).unwrap()
        );
        #[cfg(unix)]
        {
            let alias = root.path().join("alias");
            std::os::unix::fs::symlink(root.path(), &alias).unwrap();
            assert_eq!(
                scope,
                storage_scope([(
                    ProviderKind::Codex,
                    canonical_storage_path(&alias.join("native/projects"))
                ),])
                .unwrap()
            );
        }
    }
}
