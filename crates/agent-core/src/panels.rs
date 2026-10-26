//! Conversation-owned surface tabs. Native views own the resources addressed by ids.
use crate::state::DraftKey;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Scope {
    pub host: String,
    pub storage: String,
    pub draft: DraftKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Home,
    Browser,
    Terminal,
    Files,
    File,
    Diff,
    SideChat,
    Agents,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Layout {
    #[default]
    Right,
    Bottom,
    Expanded,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Surface {
    pub id: u64,
    pub kind: Kind,
    pub path: Option<String>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThreadPanel {
    pub open: bool,
    pub layout: Layout,
    pub surfaces: Vec<Surface>,
    pub active: Option<u64>,
}
impl ThreadPanel {
    pub fn selected(&self) -> Option<&Surface> {
        self.surfaces
            .iter()
            .find(|surface| Some(surface.id) == self.active)
    }
    pub fn kind(&self) -> Kind {
        self.selected().map_or(Kind::Home, |surface| surface.kind)
    }
}
static EMPTY: ThreadPanel = ThreadPanel {
    open: false,
    layout: Layout::Right,
    surfaces: Vec::new(),
    active: None,
};

#[derive(Debug, Default)]
pub struct Panels {
    threads: BTreeMap<Scope, ThreadPanel>,
    next_id: u64,
}
impl Panels {
    pub fn get(&self, key: &Scope) -> &ThreadPanel {
        self.threads.get(key).unwrap_or(&EMPTY)
    }
    pub fn scope(&self, id: u64) -> Option<&Scope> {
        self.threads
            .iter()
            .find(|(_, panel)| panel.surfaces.iter().any(|s| s.id == id))
            .map(|(key, _)| key)
    }
    pub fn open(&mut self, key: Scope, kind: Kind, path: Option<String>) -> u64 {
        let panel = self.threads.entry(key).or_default();
        panel.open = true;
        if !matches!(kind, Kind::Browser | Kind::Terminal | Kind::SideChat)
            && let Some(surface) = panel
                .surfaces
                .iter_mut()
                .find(|s| s.kind == kind && (kind == Kind::Files || s.path == path))
        {
            if path.is_some() || kind != Kind::Files {
                surface.path = path;
            }
            panel.active = Some(surface.id);
            return surface.id;
        }
        let id = self.next_id;
        self.next_id += 1;
        panel.surfaces.push(Surface { id, kind, path });
        panel.active = Some(id);
        id
    }
    pub fn activate(&mut self, key: &Scope, id: u64) -> bool {
        let Some(panel) = self.threads.get_mut(key) else {
            return false;
        };
        if !panel.surfaces.iter().any(|s| s.id == id) {
            return false;
        }
        panel.active = Some(id);
        panel.open = true;
        true
    }
    pub fn close(&mut self, key: &Scope, id: u64) {
        let Some(panel) = self.threads.get_mut(key) else {
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
    pub fn set_open(&mut self, key: Scope, open: bool) {
        self.threads.entry(key).or_default().open = open;
    }
    pub fn set_layout(&mut self, key: Scope, layout: Layout) {
        self.threads.entry(key).or_default().layout = layout;
    }
    pub fn rekey(&mut self, before: &Scope, after: Scope) {
        if let Some(panel) = self.threads.remove(before) {
            self.threads.insert(after, panel);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scope(draft: impl Into<DraftKey>) -> Scope {
        Scope {
            host: "host".into(),
            storage: "storage".into(),
            draft: draft.into(),
        }
    }
    #[test]
    fn the_same_session_id_on_different_hosts_and_storage_scopes_has_independent_panels() {
        let mut panels = Panels::default();
        let a = scope("session");
        let b = Scope {
            host: "other-host".into(),
            ..a.clone()
        };
        let c = Scope {
            storage: "replacement-host-storage".into(),
            ..a.clone()
        };
        let browser = panels.open(a.clone(), Kind::Browser, None);
        assert!(panels.get(&b).surfaces.is_empty());
        assert!(panels.get(&c).surfaces.is_empty());
        panels.open(b.clone(), Kind::Terminal, None);
        assert_eq!(panels.get(&a).active, Some(browser));
        assert_ne!(panels.get(&a).active, panels.get(&b).active);
        assert!(!panels.activate(&b, browser));
    }
    #[test]
    fn explorer_navigation_reuses_its_surface_and_invalid_closes_leave_the_selection_alone() {
        let mut panels = Panels::default();
        let key = scope("a");
        let directory = panels.open(key.clone(), Kind::Files, Some("/project".into()));
        assert_eq!(
            panels.open(key.clone(), Kind::Files, Some("/project/src".into())),
            directory
        );
        assert_eq!(
            panels.get(&key).selected().unwrap().path.as_deref(),
            Some("/project/src")
        );
        assert_eq!(panels.open(key.clone(), Kind::Files, None), directory);
        assert_eq!(
            panels.get(&key).selected().unwrap().path.as_deref(),
            Some("/project/src")
        );
        let browser = panels.open(key.clone(), Kind::Browser, None);
        panels.close(&key, directory);
        assert_eq!(panels.get(&key).active, Some(browser));
        let before = panels.get(&key).clone();
        panels.close(&key, u64::MAX);
        panels.close(&scope("missing"), browser);
        assert_eq!(panels.get(&key), &before);
        assert!(!panels.activate(&scope("missing"), browser));
        assert_eq!(panels.scope(u64::MAX), None);
    }

    proptest::proptest! {
        #[test]
        fn changes_to_one_conversation_never_change_another(actions in proptest::collection::vec((0u8..5, 0u8..20), 0..100)) {
            let mut panels = Panels::default();
            let a = scope("a");
            let b = scope("b");
            panels.open(b.clone(), Kind::Terminal, None);
            let expected = panels.get(&b).clone();
            for (action, value) in actions {
                match action {
                    0 => { panels.open(a.clone(), Kind::Browser, None); }
                    1 => panels.set_open(a.clone(), value % 2 == 0),
                    2 => panels.close(&a, u64::from(value)),
                    3 => { panels.activate(&a, u64::from(value)); }
                    _ => panels.set_layout(a.clone(), Layout::Expanded),
                }
                proptest::prop_assert_eq!(panels.get(&b), &expected);
                let panel = panels.get(&a);
                proptest::prop_assert_eq!(panel.active.is_none(), panel.surfaces.is_empty());
                if let Some(id) = panel.active { proptest::prop_assert!(panel.surfaces.iter().any(|s| s.id == id)); }
            }
        }
    }
    #[test]
    fn conversations_keep_their_surfaces_selection_visibility_and_layout() {
        let mut panels = Panels::default();
        let a = scope("draft-a");
        let b = scope("draft-b");
        let browser = panels.open(a.clone(), Kind::Browser, None);
        let terminal = panels.open(a.clone(), Kind::Terminal, None);
        panels.set_layout(a.clone(), Layout::Bottom);
        panels.set_open(a.clone(), false);
        let other = panels.open(b.clone(), Kind::SideChat, None);
        assert_eq!(panels.get(&a).active, Some(terminal));
        assert_eq!(panels.get(&a).layout, Layout::Bottom);
        assert!(!panels.get(&a).open);
        assert_eq!(panels.get(&b).active, Some(other));
        assert!(!panels.activate(&b, browser));
        assert_eq!(panels.get(&b).surfaces.len(), 1);
        assert!(panels.activate(&a, browser));
        assert!(panels.get(&a).open);
        assert_eq!(panels.get(&a).surfaces.len(), 2);
    }
    #[test]
    fn closing_a_surface_selects_its_neighbor_and_last_close_keeps_the_launcher_open() {
        let mut panels = Panels::default();
        let key = scope("a");
        let a = panels.open(key.clone(), Kind::Terminal, None);
        let b = panels.open(key.clone(), Kind::Terminal, None);
        let c = panels.open(key.clone(), Kind::Browser, None);
        panels.activate(&key, b);
        panels.close(&key, b);
        assert_eq!(panels.get(&key).active, Some(c));
        panels.close(&key, c);
        assert_eq!(panels.get(&key).active, Some(a));
        panels.close(&key, a);
        assert!(panels.get(&key).open);
        assert_eq!(panels.get(&key).kind(), Kind::Home);
    }
    #[test]
    fn files_deduplicate_by_path_and_submission_moves_the_entire_panel() {
        let mut panels = Panels::default();
        let draft = scope("local");
        let session = scope(crate::session::SessionRef {
            provider: crate::session::ProviderKind::Codex,
            id: "session".into(),
        });
        let file = panels.open(draft.clone(), Kind::File, Some("/a".into()));
        assert_eq!(
            panels.open(draft.clone(), Kind::File, Some("/a".into())),
            file
        );
        assert_ne!(
            panels.open(draft.clone(), Kind::File, Some("/b".into())),
            file
        );
        panels.rekey(&draft, session.clone());
        assert!(panels.get(&draft).surfaces.is_empty());
        assert_eq!(panels.get(&session).surfaces.len(), 2);
        assert_eq!(panels.scope(file), Some(&session));
    }
}
