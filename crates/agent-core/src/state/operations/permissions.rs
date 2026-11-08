use super::*;
use agent_protocol::permissions::PermissionSettings;
pub use agent_protocol::permissions::{ReadPermissionSettings, UpdatePermissionSettings};

/// Connection-local read result; settings are saved only by the provider.
#[derive(Debug, Clone, PartialEq)]
pub struct PermissionSettingsState {
    pub result: Option<Result<PermissionSettings, String>>,
}
macro_rules! permission_operation {
    ($ty:ty) => {
        impl Operation for $ty {
            const NAVIGATION_BOUND: bool = false;
            fn key(&self) -> Option<OperationKey> {
                Some(OperationKey::Permissions {
                    provider: self.provider,
                })
            }
            no_input!();
            type Output = Result<PermissionSettings, String>;
            fn prepare(&mut self, snapshot: &mut Snapshot, _pane_id: &str) -> Result<(), String> {
                Arc::make_mut(&mut snapshot.catalog.permission_settings).insert(
                    self.provider,
                    Arc::new(PermissionSettingsState { result: None }),
                );
                Ok(())
            }
            async fn run(
                &self,
                _: (),
                context: &mut Execution<'_>,
            ) -> Result<Self::Output, PeerError> {
                Ok(context.call(self).await.map_err(|error| error.to_string()))
            }
            fn apply(
                self,
                snapshot: &mut Snapshot,
                _pane_id: &str,
                output: Self::Output,
            ) -> Vec<Effect> {
                if snapshot
                    .catalog
                    .permission_settings
                    .contains_key(&self.provider)
                {
                    Arc::make_mut(&mut snapshot.catalog.permission_settings).insert(
                        self.provider,
                        Arc::new(PermissionSettingsState {
                            result: Some(output),
                        }),
                    );
                }
                Vec::new()
            }
        }
    };
}
permission_operation!(ReadPermissionSettings);
permission_operation!(UpdatePermissionSettings);
