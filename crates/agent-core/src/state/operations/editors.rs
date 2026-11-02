use super::*;
pub use agent_protocol::editors::{OpenInEditor, ReadFileOpenCapabilities};

impl Operation for ReadFileOpenCapabilities {
    rpc_operation!();
    fn key(&self) -> Option<OperationKey> {
        Some(OperationKey::FileOpenCapabilities)
    }
    fn scheduling(&self) -> Scheduling {
        Scheduling::LatestHostState(OperationKey::FileOpenCapabilities)
    }
    fn apply(self, snapshot: &mut Snapshot, output: Self::Output) -> Vec<Effect> {
        snapshot.file_open_capabilities = Some(Arc::new(output));
        Vec::new()
    }
}

impl Operation for OpenInEditor {
    rpc_operation!();
    fn prepare(&mut self, _: &mut Snapshot) -> Result<(), String> {
        self.validate().map_err(str::to_owned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_protocol::editors::EditorId;

    #[test]
    fn file_menu_launches_and_reveals_do_not_change_the_preferred_editor() {
        let (mut snapshot, effects) = crate::state::reduce(
            &Snapshot::default(),
            Event::Intent(Intent::SetPreferredEditor {
                editor: EditorId::Zed,
            }),
        );
        assert!(effects.is_empty());
        let mut request = OpenInEditor {
            path: "~/a.rs".into(),
            line: Some(12),
            column: None,
            editor: EditorId::Cursor,
            reveal: false,
        };
        request.prepare(&mut snapshot).unwrap();
        assert_eq!(snapshot.preferred_editor, Some(EditorId::Zed));
        request.editor = EditorId::FileManager;
        request.reveal = true;
        request.prepare(&mut snapshot).unwrap();
        assert_eq!(snapshot.preferred_editor, Some(EditorId::Zed));
        request.editor = EditorId::VsCode;
        assert!(request.prepare(&mut snapshot).is_err());
        assert_eq!(snapshot.preferred_editor, Some(EditorId::Zed));
    }

    #[test]
    fn discovery_is_host_state_and_does_not_replace_user_preferences() {
        let mut snapshot = Snapshot {
            preferred_editor: Some(EditorId::Zed),
            ..Default::default()
        };
        ReadFileOpenCapabilities {}.apply(
            &mut snapshot,
            agent_protocol::editors::FileOpenCapabilities {
                editors: vec![EditorId::VsCode],
                reveal: None,
            },
        );
        assert_eq!(snapshot.preferred_editor, Some(EditorId::Zed));
        assert_eq!(
            snapshot.file_open_capabilities.as_ref().unwrap().editors,
            [EditorId::VsCode]
        );
    }
}
