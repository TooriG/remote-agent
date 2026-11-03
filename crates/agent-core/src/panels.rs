//! Conversation-owned surface tabs. Native views own the resources addressed by ids.
use crate::state::{DraftKey, Snapshot};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

#[cfg_attr(feature = "bindings", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PanelScope {
    pub host: String,
    pub storage: String,
    pub draft: DraftKey,
}

#[cfg_attr(feature = "bindings", derive(uniffi::Enum))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelKind {
    Home,
    Browser,
    Terminal,
    Files,
    File,
    Diff,
    SideChat,
    Agents,
}
#[cfg_attr(feature = "bindings", derive(uniffi::Enum))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PanelLayout {
    #[default]
    Right,
    Bottom,
    Expanded,
}
#[cfg_attr(feature = "bindings", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelSurface {
    pub id: u64,
    pub kind: PanelKind,
    pub path: Option<String>,
}
#[cfg_attr(feature = "bindings", derive(uniffi::Record))]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PanelState {
    pub open: bool,
    pub layout: PanelLayout,
    pub surfaces: Vec<PanelSurface>,
    pub active: Option<u64>,
}
impl PanelState {
    pub fn selected(&self) -> Option<&PanelSurface> {
        self.surfaces
            .iter()
            .find(|surface| Some(surface.id) == self.active)
    }
    pub fn kind(&self) -> PanelKind {
        self.selected()
            .map_or(PanelKind::Home, |surface| surface.kind)
    }
}
#[cfg_attr(feature = "bindings", derive(uniffi::Record))]
#[derive(PartialEq)]
pub struct PanelTool {
    pub kind: PanelKind,
    pub key: String,
    pub label: String,
    pub shortcut: String,
    pub enabled: bool,
}

#[cfg_attr(feature = "bindings", uniffi::export)]
pub fn panel_tools(connected: bool, has_directory: bool) -> Vec<PanelTool> {
    [
        (PanelKind::Browser, "browser", "ブラウザ", "B"),
        (PanelKind::Terminal, "terminal", "ターミナル", "T"),
        (PanelKind::Files, "files", "ファイル", "F"),
        (PanelKind::Diff, "diff", "差分", "D"),
        (PanelKind::SideChat, "sideChat", "サイドチャット", "S"),
        (PanelKind::Agents, "agents", "エージェント", "A"),
    ]
    .into_iter()
    .map(|(kind, key, label, shortcut)| PanelTool {
        kind,
        key: key.into(),
        label: label.into(),
        shortcut: shortcut.into(),
        enabled: matches!(kind, PanelKind::Browser | PanelKind::Agents)
            || (connected && has_directory),
    })
    .collect()
}

#[cfg_attr(feature = "bindings", uniffi::export)]
pub fn panel_title(kind: PanelKind) -> String {
    panel_tools(false, false)
        .into_iter()
        .find(|tool| {
            tool.kind
                == if kind == PanelKind::File {
                    PanelKind::Files
                } else {
                    kind
                }
        })
        .map_or_else(|| "機能を開く".into(), |tool| tool.label)
}

#[derive(Debug, Default)]
struct PanelCollection {
    threads: BTreeMap<PanelScope, PanelState>,
    next_id: u64,
}

/// Owns conversation-scoped tab state for every native client. Views own resources.
#[cfg_attr(feature = "bindings", derive(uniffi::Object))]
#[derive(Debug, Default)]
pub struct Panels {
    state: Mutex<PanelCollection>,
}

#[cfg_attr(feature = "bindings", uniffi::export)]
impl Panels {
    #[cfg_attr(feature = "bindings", uniffi::constructor)]
    pub fn new() -> Arc<Self> {
        Arc::default()
    }

    pub fn get(&self, key: PanelScope) -> PanelState {
        self.state
            .lock()
            .unwrap()
            .threads
            .get(&key)
            .cloned()
            .unwrap_or_default()
    }
    pub fn scope(&self, id: u64) -> Option<PanelScope> {
        self.state
            .lock()
            .unwrap()
            .threads
            .iter()
            .find(|(_, panel)| panel.surfaces.iter().any(|s| s.id == id))
            .map(|(key, _)| key.clone())
    }
    pub fn open(&self, key: PanelScope, kind: PanelKind, path: Option<String>) -> u64 {
        let mut state = self.state.lock().unwrap();
        let id = state.next_id;
        let panel = state.threads.entry(key).or_default();
        panel.open = true;
        if !matches!(
            kind,
            PanelKind::Browser | PanelKind::Terminal | PanelKind::SideChat
        ) && let Some(surface) = panel
            .surfaces
            .iter_mut()
            .find(|s| s.kind == kind && (kind == PanelKind::Files || s.path == path))
        {
            if path.is_some() || kind != PanelKind::Files {
                surface.path = path;
            }
            panel.active = Some(surface.id);
            return surface.id;
        }
        panel.surfaces.push(PanelSurface { id, kind, path });
        panel.active = Some(id);
        state.next_id += 1;
        id
    }
    pub fn activate(&self, key: PanelScope, id: u64) -> bool {
        let mut state = self.state.lock().unwrap();
        let Some(panel) = state.threads.get_mut(&key) else {
            return false;
        };
        if !panel.surfaces.iter().any(|s| s.id == id) {
            return false;
        }
        panel.active = Some(id);
        panel.open = true;
        true
    }
    pub fn close(&self, key: PanelScope, id: u64) {
        let mut state = self.state.lock().unwrap();
        let Some(panel) = state.threads.get_mut(&key) else {
            return;
        };
        let Some(index) = panel.surfaces.iter().position(|s| s.id == id) else {
            return;
        };
        panel.surfaces.remove(index);
        if panel.active == Some(id) {
            panel.active = panel
                .surfaces
                .get(index.min(panel.surfaces.len().saturating_sub(1)))
                .map(|s| s.id);
        }
    }
    pub fn set_open(&self, key: PanelScope, open: bool) {
        self.state
            .lock()
            .unwrap()
            .threads
            .entry(key)
            .or_default()
            .open = open;
    }
    pub fn set_layout(&self, key: PanelScope, layout: PanelLayout) {
        self.state
            .lock()
            .unwrap()
            .threads
            .entry(key)
            .or_default()
            .layout = layout;
    }
    pub fn rekey(&self, before: PanelScope, after: PanelScope) {
        let mut state = self.state.lock().unwrap();
        if let Some(panel) = state.threads.remove(&before) {
            state.threads.insert(after, panel);
        }
    }
}

#[cfg_attr(feature = "bindings", uniffi::export)]
impl Snapshot {
    pub fn panel_scope(&self, host: String, draft: crate::state::DraftKey) -> PanelScope {
        PanelScope {
            host,
            storage: self.storage_scope.clone(),
            draft,
        }
    }
    pub fn submitted_session(
        &self,
        draft: crate::state::DraftKey,
    ) -> Option<crate::session::SessionRef> {
        self.draft_sessions.get(&draft).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scope(draft: impl Into<DraftKey>) -> PanelScope {
        PanelScope {
            host: "host".into(),
            storage: "storage".into(),
            draft: draft.into(),
        }
    }
    #[test]
    fn a_new_conversation_starts_at_the_launcher_without_starting_a_tool() {
        let panels = Panels::default();
        let key = scope("new");
        panels.set_open(key.clone(), true);
        let empty = panels.get(key.clone());
        assert!(empty.open);
        assert!(empty.surfaces.is_empty());
        assert_eq!(empty.kind(), PanelKind::Home);
        assert_eq!(empty.active, None);
        let terminal = panels.open(key.clone(), PanelKind::Terminal, None);
        assert!(
            empty.surfaces.is_empty(),
            "Reading state must return an immutable snapshot"
        );
        assert_eq!(panels.get(key).active, Some(terminal));
    }

    #[test]
    fn the_launcher_and_add_menu_share_order_titles_shortcuts_and_availability() {
        for connected in [false, true] {
            for has_directory in [false, true] {
                let tools = panel_tools(connected, has_directory);
                assert_eq!(
                    tools
                        .iter()
                        .map(|tool| tool.key.as_str())
                        .collect::<Vec<_>>(),
                    ["browser", "terminal", "files", "diff", "sideChat", "agents"]
                );
                assert_eq!(
                    tools
                        .iter()
                        .map(|tool| tool.shortcut.as_str())
                        .collect::<Vec<_>>(),
                    ["B", "T", "F", "D", "S", "A"]
                );
                for tool in tools {
                    let available = match tool.kind {
                        PanelKind::Browser | PanelKind::Agents => true,
                        _ => connected && has_directory,
                    };
                    assert_eq!(tool.enabled, available, "{}", tool.key);
                    assert_eq!(panel_title(tool.kind), tool.label);
                }
            }
        }
        assert_eq!(panel_title(PanelKind::Home), "機能を開く");
        assert_eq!(panel_title(PanelKind::File), "ファイル");
    }

    #[test]
    fn the_same_session_id_on_different_hosts_and_storage_scopes_has_independent_panels() {
        let panels = Panels::default();
        let a = scope("session");
        let b = PanelScope {
            host: "other-host".into(),
            ..a.clone()
        };
        let c = PanelScope {
            storage: "replacement-host-storage".into(),
            ..a.clone()
        };
        let browser = panels.open(a.clone(), PanelKind::Browser, None);
        assert!(panels.get(b.clone()).surfaces.is_empty());
        assert!(panels.get(c.clone()).surfaces.is_empty());
        panels.open(b.clone(), PanelKind::Terminal, None);
        assert_eq!(panels.get(a.clone()).active, Some(browser));
        assert_ne!(panels.get(a.clone()).active, panels.get(b.clone()).active);
        assert!(!panels.activate(b.clone(), browser));
    }
    #[test]
    fn explorer_navigation_reuses_its_surface_and_invalid_closes_leave_the_selection_alone() {
        let panels = Panels::default();
        let key = scope("a");
        let directory = panels.open(key.clone(), PanelKind::Files, Some("/project".into()));
        assert_eq!(
            panels.open(key.clone(), PanelKind::Files, Some("/project/src".into())),
            directory
        );
        assert_eq!(
            panels.get(key.clone()).selected().unwrap().path.as_deref(),
            Some("/project/src")
        );
        assert_eq!(panels.open(key.clone(), PanelKind::Files, None), directory);
        assert_eq!(
            panels.get(key.clone()).selected().unwrap().path.as_deref(),
            Some("/project/src")
        );
        let browser = panels.open(key.clone(), PanelKind::Browser, None);
        panels.close(key.clone(), directory);
        assert_eq!(panels.get(key.clone()).active, Some(browser));
        let before = panels.get(key.clone());
        panels.close(key.clone(), u64::MAX);
        panels.close(scope("missing"), browser);
        assert_eq!(panels.get(key.clone()), before);
        assert!(!panels.activate(scope("missing"), browser));
        assert_eq!(panels.scope(u64::MAX), None);
    }

    proptest::proptest! {
        #[test]
        fn changes_to_one_conversation_never_change_another(actions in proptest::collection::vec((0u8..5, 0u8..20), 0..100)) {
            let panels = Panels::default();
            let a = scope("a");
            let b = scope("b");
            panels.open(b.clone(), PanelKind::Terminal, None);
            let expected = panels.get(b.clone());
            for (action, value) in actions {
                match action {
                    0 => { panels.open(a.clone(), PanelKind::Browser, None); }
                    1 => panels.set_open(a.clone(), value % 2 == 0),
                    2 => panels.close(a.clone(), u64::from(value)),
                    3 => { panels.activate(a.clone(), u64::from(value)); }
                    _ => panels.set_layout(a.clone(), PanelLayout::Expanded),
                }
                proptest::prop_assert_eq!(panels.get(b.clone()), expected.clone());
                let panel = panels.get(a.clone());
                proptest::prop_assert_eq!(panel.active.is_none(), panel.surfaces.is_empty());
                if let Some(id) = panel.active { proptest::prop_assert!(panel.surfaces.iter().any(|s| s.id == id)); }
            }
        }
    }
    #[test]
    fn conversations_keep_their_surfaces_selection_visibility_and_layout() {
        let panels = Panels::default();
        let a = scope("draft-a");
        let b = scope("draft-b");
        let browser = panels.open(a.clone(), PanelKind::Browser, None);
        let terminal = panels.open(a.clone(), PanelKind::Terminal, None);
        panels.set_layout(a.clone(), PanelLayout::Bottom);
        panels.set_open(a.clone(), false);
        let other = panels.open(b.clone(), PanelKind::SideChat, None);
        assert_eq!(panels.get(a.clone()).active, Some(terminal));
        assert_eq!(panels.get(a.clone()).layout, PanelLayout::Bottom);
        assert!(!panels.get(a.clone()).open);
        assert_eq!(panels.get(b.clone()).active, Some(other));
        assert!(!panels.activate(b.clone(), browser));
        assert_eq!(panels.get(b.clone()).surfaces.len(), 1);
        assert!(panels.activate(a.clone(), browser));
        assert!(panels.get(a.clone()).open);
        assert_eq!(panels.get(a.clone()).surfaces.len(), 2);
    }
    #[test]
    fn closing_a_surface_selects_its_neighbor_and_last_close_keeps_the_launcher_open() {
        let panels = Panels::default();
        let key = scope("a");
        let a = panels.open(key.clone(), PanelKind::Terminal, None);
        let b = panels.open(key.clone(), PanelKind::Terminal, None);
        let c = panels.open(key.clone(), PanelKind::Browser, None);
        panels.activate(key.clone(), b);
        panels.close(key.clone(), b);
        assert_eq!(panels.get(key.clone()).active, Some(c));
        panels.close(key.clone(), c);
        assert_eq!(panels.get(key.clone()).active, Some(a));
        panels.close(key.clone(), a);
        assert!(panels.get(key.clone()).open);
        assert_eq!(panels.get(key.clone()).kind(), PanelKind::Home);
    }
    #[test]
    fn files_deduplicate_by_path_and_submission_moves_the_entire_panel() {
        let panels = Panels::default();
        let draft = scope("local");
        let session = scope(crate::session::SessionRef {
            provider: crate::session::ProviderKind::Codex,
            id: "session".into(),
        });
        let file = panels.open(draft.clone(), PanelKind::File, Some("/a".into()));
        assert_eq!(
            panels.open(draft.clone(), PanelKind::File, Some("/a".into())),
            file
        );
        assert_ne!(
            panels.open(draft.clone(), PanelKind::File, Some("/b".into())),
            file
        );
        panels.rekey(draft.clone(), session.clone());
        assert!(panels.get(draft.clone()).surfaces.is_empty());
        assert_eq!(panels.get(session.clone()).surfaces.len(), 2);
        assert_eq!(panels.scope(file), Some(session.clone()));
    }
}
