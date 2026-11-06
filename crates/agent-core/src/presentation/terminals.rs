//! Terminal display reads only terminal resources, independent of chat updates.
use crate::state::{Terminal, TerminalPhase, TerminalView};
use std::{collections::BTreeMap, sync::Arc};

/// Native views can briefly report a zero grid while being laid out.
#[cfg_attr(feature = "bindings", uniffi::export)]
pub fn terminal_grid(cols: i32, rows: i32) -> Option<agent_protocol::operations::TerminalSize> {
    if cols <= 0 || rows <= 0 {
        return None;
    }
    Some(agent_protocol::operations::TerminalSize {
        cols: cols.min(500) as u16,
        rows: rows.min(250) as u16,
    })
}

#[cfg_attr(feature = "bindings", derive(uniffi::Object))]
pub struct Terminals {
    pub(crate) states: Arc<BTreeMap<String, Arc<Terminal>>>,
}

#[cfg_attr(feature = "bindings", uniffi::export)]
impl Terminals {
    #[cfg_attr(feature = "bindings", uniffi::constructor)]
    pub fn empty() -> Arc<Self> {
        Arc::new(Self {
            states: Arc::default(),
        })
    }
    pub fn view(&self, handle: String) -> Option<TerminalView> {
        self.states.get(&handle).map(|terminal| view(terminal))
    }
}

pub(crate) fn view(terminal: &Terminal) -> TerminalView {
    TerminalView {
        status: match terminal.phase {
            TerminalPhase::Starting | TerminalPhase::Running => None,
            _ => Some(terminal.phase.label()),
        },
        loading: matches!(terminal.phase, TerminalPhase::Starting),
        accepts_input: terminal.phase.accepts_input(),
        output: terminal
            .output
            .iter()
            .map(|chunk| (**chunk).clone())
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Snapshot, TerminalOutput};

    #[test]
    fn native_grids_ignore_unlaid_out_views_and_fit_host_limits() {
        for (cols, rows) in [(0, 1), (1, 0), (-1, 1), (1, -1), (i32::MIN, i32::MAX)] {
            assert!(terminal_grid(cols, rows).is_none());
        }
        for (cols, rows, expected_cols, expected_rows) in [
            (1, 1, 1, 1),
            (500, 250, 500, 250),
            (501, 20, 500, 20),
            (80, 251, 80, 250),
            (i32::MAX, i32::MAX, 500, 250),
        ] {
            let size = terminal_grid(cols, rows).unwrap();
            assert_eq!((size.cols, size.rows), (expected_cols, expected_rows));
        }
    }
    #[test]
    fn terminal_resources_retain_their_output_and_state_across_other_updates() {
        let mut state = Snapshot::default();
        Arc::make_mut(&mut state.terminals).insert(
            "shell".into(),
            Arc::new(Terminal {
                cwd: "/project".into(),
                size: agent_protocol::operations::TerminalSize { cols: 80, rows: 24 },
                phase: TerminalPhase::Starting,
                output: [Arc::new(TerminalOutput {
                    sequence: 1,
                    data: b"ready\r\n".to_vec(),
                    reset_size: None,
                })]
                .into(),
                sequence: 1,
            }),
        );
        let before = state.terminals();
        assert!(before.view("shell".into()).unwrap().loading);
        assert!(state.terminal_accepts_input("shell".into()));
        assert!(before.view("missing".into()).is_none());
        Arc::make_mut(&mut state.input.drafts).insert(Default::default(), Arc::default());
        assert!(Arc::ptr_eq(&before.states, &state.terminals));
        Arc::make_mut(
            Arc::make_mut(&mut state.terminals)
                .get_mut("shell")
                .unwrap(),
        )
        .phase = TerminalPhase::Suspended;
        let suspended = state.terminals().view("shell".into()).unwrap();
        assert!(!suspended.loading && !suspended.accepts_input);
        assert!(!state.terminal_accepts_input("shell".into()));
        assert_eq!(suspended.status.as_deref(), Some("再接続を待っています"));
        assert_eq!(suspended.output[0].data, b"ready\r\n");
        let retained = before.view("shell".into()).unwrap();
        assert!(retained.loading && retained.accepts_input);
        assert_eq!(retained.output[0].sequence, 1);
    }
}
