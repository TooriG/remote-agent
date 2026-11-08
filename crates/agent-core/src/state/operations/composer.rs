use super::*;

use agent_protocol::composer::{ComposerCandidate, ComposerCatalog};
pub use agent_protocol::operations::LoadComposerCatalog;
use agent_protocol::session::ProviderKind;

pub(crate) fn prefetch_composer_catalog<'a>(
    connected: bool,
    catalogs: &mut Arc<BTreeMap<String, Arc<ComposerCatalog>>>,
    panes: impl Iterator<Item = (&'a str, &'a Navigation)>,
) -> Vec<Effect> {
    if !connected {
        return Vec::new();
    }
    let mut effects = Vec::new();
    for (pane, navigation) in panes {
        if navigation.draft_key.is_empty() || catalogs.contains_key(&navigation.cwd) {
            continue;
        }
        prepare_catalog(catalogs, &navigation.cwd);
        effects.push(
            Effect::execute(LoadComposerCatalog {
                cwd: navigation.cwd.clone(),
            })
            .in_pane(pane),
        );
    }
    effects
}
fn prepare_catalog(catalogs: &mut Arc<BTreeMap<String, Arc<ComposerCatalog>>>, cwd: &str) {
    let candidates = catalogs
        .get(cwd)
        .map(|catalog| catalog.candidates.clone())
        .unwrap_or_default();
    Arc::make_mut(catalogs).insert(
        cwd.into(),
        Arc::new(ComposerCatalog {
            cwd: cwd.into(),
            loading: true,
            candidates,
            ..Default::default()
        }),
    );
}
impl Operation for LoadComposerCatalog {
    type Input = Arc<ComposerCatalog>;
    fn capture(&self, snapshot: &Snapshot, _pane_id: &str) -> Result<Self::Input, PeerError> {
        snapshot
            .catalog
            .composer_catalogs
            .get(&self.cwd)
            .cloned()
            .ok_or_else(|| PeerError::InvalidMessage("catalog load is not prepared".into()))
    }
    const BACKGROUND: bool = true;
    type Output = (Arc<ComposerCatalog>, ComposerCatalog);
    const STALE_POLICY: StalePolicy = StalePolicy::Apply;
    fn prepare(&mut self, snapshot: &mut Snapshot, _pane_id: &str) -> Result<(), String> {
        prepare_catalog(&mut snapshot.catalog.composer_catalogs, &self.cwd);
        Ok(())
    }
    async fn run(
        &self,
        source: Self::Input,
        context: &mut Execution<'_>,
    ) -> Result<Self::Output, PeerError> {
        let catalog = context
            .call(self)
            .await
            .unwrap_or_else(|_| unavailable_catalog(&self.cwd, &source.candidates));
        Ok((source, catalog))
    }
    fn rejected_output(&self, source: Self::Input) -> Option<Self::Output> {
        let catalog = unavailable_catalog(&self.cwd, &source.candidates);
        Some((source, catalog))
    }
    fn apply(
        self,
        snapshot: &mut Snapshot,
        _pane_id: &str,
        (source, catalog): Self::Output,
    ) -> Vec<Effect> {
        if snapshot
            .catalog
            .composer_catalogs
            .get(&self.cwd)
            .is_some_and(|current| Arc::ptr_eq(current, &source))
        {
            Arc::make_mut(&mut snapshot.catalog.composer_catalogs)
                .insert(self.cwd, Arc::new(catalog));
        }
        Vec::new()
    }
}

fn unavailable_catalog(cwd: &str, candidates: &[ComposerCandidate]) -> ComposerCatalog {
    ComposerCatalog {
        cwd: cwd.into(),
        candidates: candidates.to_vec(),
        errors: [ProviderKind::Codex, ProviderKind::Claude]
            .into_iter()
            .map(|provider| {
                (
                    provider,
                    vec!["候補を取得できませんでした。再度 @ または / を入力してください。".into()],
                )
            })
            .collect(),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    proptest::proptest! {
           #[test]
           fn catalog_replies_belong_to_their_loading_source(
               same_source in proptest::bool::ANY,
               same_cwd in proptest::bool::ANY,
               connected in proptest::bool::ANY,
           ) {
               let source = Arc::new(ComposerCatalog { cwd: "/first".into(), loading: true, ..Default::default() });
               let current = if same_source { source.clone() } else { Arc::new((*source).clone()) };
               let mut snapshot = Snapshot {
                   connection: crate::state::Connection {
                       connected,
                       ..Default::default()
                   },
                   catalog: crate::state::Catalog {
                       composer_catalogs: std::sync::Arc::new((if connected { Some(current.clone()) } else { None }).into_iter().map(|catalog| (catalog.cwd.clone(), catalog)).collect()),
                       ..Default::default()
                   },
    panes: crate::state::PaneStates::with_primary(std::sync::Arc::new(crate::state::PaneState { navigation: Arc::new(Navigation { cwd: if same_cwd { "/first" } else { "/second" }.into(), ..Default::default() }), ..Default::default() })),
                   ..Default::default()
               };
               let before = snapshot.catalog.composer_catalogs.clone();
               let output = ComposerCatalog { cwd: "/first".into(), ..Default::default() };
               // A reply survives unrelated navigation epochs only while the
               // exact loading catalog still owns it. Account/reconnect resets
               // replace that owner even when all catalog values are identical.
               LoadComposerCatalog { cwd: "/first".into() }.complete(&mut snapshot, "", (source, output), false).unwrap();
               if same_source && connected {
                   proptest::prop_assert!(!snapshot.catalog.composer_catalogs["/first"].loading);
               } else {
                   proptest::prop_assert_eq!(snapshot.catalog.composer_catalogs, before);
               }
           }
       }
}
