//! Client-owned lazy file trees. The reducer owns changes; apps send input and
//! render rows. Directory and search effects carry generations, never UI objects.
use agent_protocol::file_tree::{EntryKind, TreeEntries, TreeEntry, valid_tree_path};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::Arc,
};
mod input;
mod sort;
#[cfg(test)]
mod tests;
mod view;
pub use view::{FileTreeRow, FileTreeView};

/// One mounted browser can visit several roots. Independent browsers (including
/// a side chat) must never share expansion, search, selection or request lifetimes.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "bindings", derive(uniffi::Record))]
pub struct FileTreeKey {
    pub owner: String,
    pub root: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "bindings", derive(uniffi::Record))]
pub struct FileTreeRequest {
    pub id: u64,
    pub path: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "bindings", derive(uniffi::Enum))]
pub enum FileTreeSurface {
    #[default]
    Desktop,
    Mobile,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "bindings", derive(uniffi::Enum))]
pub enum FileTreeAction {
    Close,
    Open {
        surface: FileTreeSurface,
        locale: String,
        workspace_root: String,
    },
    Toggle {
        path: String,
    },
    Focus {
        path: String,
    },
    SelectPaths {
        paths: Vec<String>,
        anchor: Option<String>,
        focused: Option<String>,
    },
    Keyboard {
        key: String,
        additive: bool,
        shift: bool,
    },
    Click {
        path: String,
        additive: bool,
        range: bool,
    },
    Select {
        path: String,
        additive: bool,
        range: bool,
    },
    Reveal {
        path: Option<String>,
        directory: bool,
    },
    Query {
        text: String,
    },
    ToggleAll,
    Refresh,
    DragStart {
        path: String,
    },
    DragEnd {
        paths: Vec<String>,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Directory {
    entries: Option<Arc<Vec<TreeEntry>>>,
    error: Option<String>,
    used: u64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Tree {
    connection: u64,
    host_folder: bool,
    surface: FileTreeSurface,
    locale: String,
    generation: u64,
    directories: BTreeMap<String, Directory>,
    discoveries: BTreeMap<String, TreeEntry>,
    expanded: BTreeSet<String>,
    queue: VecDeque<String>,
    running: BTreeSet<String>,
    selected: Vec<String>,
    anchor: Option<String>,
    focused: Option<String>,
    open_file: Option<FileTreeRequest>,
    context_menu: Option<FileTreeRequest>,
    reveal_id: u64,
    query: String,
    search_generation: u64,
    search: Option<Result<TreeEntries, String>>,
    expand_all: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TreeWork {
    Directory {
        key: FileTreeKey,
        path: String,
        generation: u64,
    },
    Search {
        key: FileTreeKey,
        query: String,
        generation: u64,
    },
}

/// Client-owned directory state, bounded by mobile's eight-workspace cache.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileTrees {
    trees: VecDeque<(FileTreeKey, Arc<Tree>)>,
    sequence: u64,
}

impl FileTrees {
    pub(crate) fn disconnected(&mut self) {
        if self.trees.is_empty() {
            return;
        }
        self.sequence += 1;
        for (_, tree) in &mut self.trees {
            let tree = Arc::make_mut(tree);
            tree.generation = self.sequence;
            tree.search_generation = self.sequence;
            let pending = std::mem::take(&mut tree.running)
                .into_iter()
                .chain(tree.queue.drain(..));
            for path in pending {
                tree.directories.entry(path).or_default().error =
                    Some("Host not connected.".into());
            }
            if !tree.search_query().trim().is_empty() && tree.search.is_none() {
                tree.search = Some(Err("Host not connected.".into()));
            }
        }
    }
    fn active_keys(&self) -> Vec<FileTreeKey> {
        let mut owners = BTreeSet::new();
        self.trees
            .iter()
            .rev()
            .filter(|(key, _)| owners.insert(key.owner.clone()))
            .map(|(key, _)| key.clone())
            .collect()
    }
    pub(crate) fn reconnect(&mut self, connection: u64) -> Vec<TreeWork> {
        self.active_keys()
            .into_iter()
            .flat_map(|key| self.action(key, FileTreeAction::Refresh, connection))
            .collect()
    }
    /// Refresh the active root of each mounted browser after a workspace mutation.
    pub(crate) fn refresh_matching(&mut self, path: &str, connection: u64) -> Vec<TreeWork> {
        use crate::presentation::markdown::file_workspace_relative_path;
        let keys = self
            .active_keys()
            .into_iter()
            .filter(|key| {
                file_workspace_relative_path(path, &key.root).is_some()
                    || file_workspace_relative_path(&key.root, path).is_some()
            })
            .collect::<Vec<_>>();
        keys.into_iter()
            .flat_map(|key| self.action(key, FileTreeAction::Refresh, connection))
            .collect()
    }
    fn tree_mut(&mut self, key: &FileTreeKey) -> Option<&mut Tree> {
        self.trees
            .iter_mut()
            .find(|(path, _)| path == key)
            .map(|(_, tree)| Arc::make_mut(tree))
    }
    pub fn revision(&self) -> u64 {
        self.sequence
    }
    pub fn view(&self, key: &FileTreeKey) -> FileTreeView {
        self.trees
            .iter()
            .find(|(path, _)| path == key)
            .map_or_else(FileTreeView::default, |(_, tree)| tree.view())
    }
    pub(crate) fn action(
        &mut self,
        key: FileTreeKey,
        action: FileTreeAction,
        connection: u64,
    ) -> Vec<TreeWork> {
        self.sequence += 1;
        let ticket = self.sequence;
        if matches!(action, FileTreeAction::Close) {
            self.trees.retain(|(current, _)| current.owner != key.owner);
            return Vec::new();
        }
        if let FileTreeAction::Open { surface, .. } = &action {
            if let Some((previous, tree)) = self
                .trees
                .iter_mut()
                .rev()
                .find(|(current, _)| current.owner == key.owner)
                && previous != &key
            {
                let tree = Arc::make_mut(tree);
                tree.generation = ticket;
                tree.search_generation = ticket;
                tree.running.clear();
                tree.queue.clear();
                tree.bound(false);
            }
            // Desktop keys its model by root. Mobile retains eight visited
            // workspaces within the same mounted browser.
            if *surface == FileTreeSurface::Desktop {
                self.trees
                    .retain(|(current, _)| current.owner != key.owner || current == &key);
            }
            let tree = self
                .trees
                .iter()
                .position(|(path, _)| path == &key)
                .and_then(|index| self.trees.remove(index))
                .unwrap_or_else(|| (key.clone(), Arc::default()));
            self.trees.push_back(tree);
            if self
                .trees
                .iter()
                .filter(|(current, _)| current.owner == key.owner)
                .count()
                > 8
            {
                let oldest = self
                    .trees
                    .iter()
                    .position(|(current, _)| current.owner == key.owner)
                    .unwrap();
                self.trees.remove(oldest);
            }
        }
        let Some(tree) = self.tree_mut(&key) else {
            return Vec::new();
        };
        let reconnect = tree.connection != connection;
        let changed_source = if let FileTreeAction::Open { workspace_root, .. } = &action {
            let host_folder = crate::presentation::markdown::file_workspace_relative_path(
                &key.root,
                workspace_root,
            )
            .is_none();
            let changed = tree.host_folder != host_folder;
            tree.host_folder = host_folder;
            changed
        } else {
            false
        };
        let mut search = changed_source.then(|| tree.restart_search());
        let opening = matches!(action, FileTreeAction::Open { .. });
        if reconnect || opening || matches!(action, FileTreeAction::Refresh) {
            tree.generation = ticket;
        }
        if reconnect || opening {
            tree.connection = connection;
            tree.running.clear();
            tree.queue.clear();
            if reconnect && !opening && !matches!(action, FileTreeAction::Refresh) {
                search = tree.action(FileTreeAction::Refresh, ticket).or(search);
            }
            if !tree.query.trim().is_empty() {
                search = Some(tree.restart_search());
            }
        }
        search = tree.action(action, ticket).or(search);
        let mut effects = Vec::new();
        if let Some(query) = search {
            tree.search_generation = ticket;
            if !query.trim().is_empty() {
                effects.push(TreeWork::Search {
                    key: key.clone(),
                    query,
                    generation: ticket,
                });
            }
        }
        effects.extend(tree.start(&key));
        effects
    }
    pub(crate) fn directory(
        &mut self,
        key: &FileTreeKey,
        path: String,
        generation: u64,
        result: Result<TreeEntries, String>,
    ) -> Vec<TreeWork> {
        self.sequence += 1;
        let ticket = self.sequence;
        let Some(tree) = self.tree_mut(key) else {
            return Vec::new();
        };
        if tree.generation != generation || !tree.running.remove(&path) {
            return Vec::new();
        }
        match result {
            Ok(mut listing) => {
                listing.entries.retain(|entry| {
                    valid_tree_path(&entry.path)
                        && entry.path.rsplit_once('/').map_or("", |(parent, _)| parent) == path
                });
                tree.prune(&path, &listing.entries);
                tree.directories.insert(
                    path,
                    Directory {
                        entries: Some(Arc::new(listing.entries)),
                        error: None,
                        used: ticket,
                    },
                );
                if tree.expand_all {
                    tree.expand_available();
                }
                tree.load_expanded();
            }
            Err(error) => tree.directories.entry(path).or_default().error = Some(error),
        }
        tree.bound(true);
        tree.start(key)
    }
    pub(crate) fn searched(
        &mut self,
        key: &FileTreeKey,
        generation: u64,
        result: Result<TreeEntries, String>,
    ) {
        if self.search_current(key, generation) {
            self.sequence += 1;
            self.tree_mut(key).unwrap().search = Some(result);
        }
    }
    pub(crate) fn search_current(&self, key: &FileTreeKey, generation: u64) -> bool {
        self.trees
            .iter()
            .any(|(path, tree)| path == key && tree.search_generation == generation)
    }
}

impl Tree {
    fn request(&mut self, path: String, refresh: bool) {
        if self.running.contains(&path) || self.queue.contains(&path) {
            return;
        }
        let directory = self.directories.entry(path.clone()).or_default();
        if !refresh && directory.entries.is_some() && directory.error.is_none() {
            return;
        }
        directory.error = None;
        self.queue.push_back(path);
    }
    fn start(&mut self, key: &FileTreeKey) -> Vec<TreeWork> {
        let mut work = Vec::new();
        while self.running.len() < 4
            && let Some(path) = self.queue.pop_front()
        {
            self.running.insert(path.clone());
            work.push(TreeWork::Directory {
                key: key.clone(),
                path,
                generation: self.generation,
            });
        }
        work
    }
    fn action(&mut self, action: FileTreeAction, ticket: u64) -> Option<String> {
        let click = matches!(action, FileTreeAction::Click { .. });
        match action {
            FileTreeAction::Open {
                surface, locale, ..
            } => {
                self.open_file = None;
                self.context_menu = None;
                self.surface = surface;
                self.locale = locale;
                self.request(String::new(), true);
                let visible = self
                    .expanded
                    .iter()
                    .filter(|path| self.visible(path))
                    .cloned()
                    .chain(
                        self.directories
                            .iter()
                            .filter(|(_, value)| value.error.is_some())
                            .map(|(path, _)| path.clone()),
                    )
                    .collect::<BTreeSet<_>>();
                for path in visible {
                    self.request(path, true);
                }
                self.retain_discoveries();
            }
            FileTreeAction::Keyboard {
                key,
                additive,
                shift,
            } => {
                let view = self.view();
                if let Some(input) =
                    view.desktop_keyboard_input(&key, additive, shift, self.anchor.as_deref())
                {
                    let mut search = None;
                    for action in input.actions {
                        search = self.action(action, ticket).or(search);
                    }
                    if let Some(path) = input.menu_path {
                        self.context_menu = Some(FileTreeRequest { id: ticket, path });
                    }
                    return search;
                }
            }
            FileTreeAction::Toggle { path } if valid_tree_path(&path) => self.toggle(path),
            FileTreeAction::Focus { path } if valid_tree_path(&path) => self.focused = Some(path),
            FileTreeAction::SelectPaths {
                paths,
                anchor,
                focused,
            } => {
                let before = self.selected.clone();
                self.selected = paths
                    .into_iter()
                    .filter(|path| valid_tree_path(path))
                    .collect();
                self.request_selected_file(&before, ticket);
                self.anchor = anchor.filter(|path| valid_tree_path(path));
                self.focused = focused.filter(|path| valid_tree_path(path));
                self.retain_discoveries();
            }
            FileTreeAction::Select {
                path,
                additive,
                range,
            }
            | FileTreeAction::Click {
                path,
                additive,
                range,
            } if valid_tree_path(&path) => {
                return self.select_row(path, additive, range, click, ticket);
            }

            FileTreeAction::Reveal { path, directory } => {
                self.reveal_id = ticket;
                self.open_file = None;
                self.context_menu = None;
                let path = path.filter(|path| valid_tree_path(path))?;
                self.load_ancestors(&path, true);
                if directory {
                    self.expanded.insert(path.clone());
                    self.request(path.clone(), false);
                }
                self.focused = Some(path.clone());
                self.selected = vec![path];
                self.retain_discoveries();
                return self.query(String::new());
            }
            FileTreeAction::Query { text } => return self.query(text),
            FileTreeAction::ToggleAll if !self.host_folder => {
                if self.expand_all || self.all_expanded() {
                    self.expand_all = false;
                    if !self.desktop_search() {
                        self.expanded.clear();
                    }
                } else {
                    self.expand_all = true;
                    self.expand_available();
                }
            }
            FileTreeAction::Refresh => {
                self.running.clear();
                self.queue.clear();
                let paths = self.directories.keys().cloned().collect::<Vec<_>>();
                for path in paths {
                    self.request(path, true);
                }
                if !self.query.trim().is_empty() {
                    return Some(self.restart_search());
                }
            }
            FileTreeAction::DragStart { path }
                if !self.desktop_search() && valid_tree_path(&path) =>
            {
                if !self.selected.contains(&path) {
                    self.selected = vec![path.clone()];
                    self.anchor = Some(path.clone());
                }
                self.focused = Some(path);
            }
            FileTreeAction::DragEnd { paths } => {
                self.selected.retain(|path| !paths.contains(path));
                self.retain_discoveries();
            }
            _ => {}
        }
        None
    }
    /// Row interaction differs by surface; programmatic selection remains separate.
    fn select_row(
        &mut self,
        path: String,
        additive: bool,
        range: bool,
        click: bool,
        ticket: u64,
    ) -> Option<String> {
        let directory = self
            .entries()
            .get(&path)
            .is_some_and(|entry| entry.kind == EntryKind::Directory);
        self.load_ancestors(&path, self.surface == FileTreeSurface::Mobile);
        self.focused = Some(path.clone());
        // A mobile folder press changes expansion, preserving the open file's selection.
        if self.surface == FileTreeSurface::Desktop || !directory {
            let before = self.selected.clone();
            self.select(path.clone(), additive, range);
            let previous = if click && self.surface == FileTreeSurface::Mobile {
                &[][..]
            } else {
                &before
            };
            self.request_selected_file(previous, ticket);
        }
        if click && directory && !(additive || range) {
            self.toggle(path);
        }
        self.retain_discoveries();
        if click && self.surface == FileTreeSurface::Desktop {
            self.query(String::new())
        } else {
            None
        }
    }
    fn request_selected_file(&mut self, previous: &[String], ticket: u64) {
        if previous != self.selected
            && let Some(path) = self.selected.last()
            && self
                .entries()
                .get(path)
                .is_some_and(|entry| entry.kind == EntryKind::File)
        {
            self.open_file = Some(FileTreeRequest {
                id: ticket,
                path: path.clone(),
            });
        }
    }
    fn toggle(&mut self, path: String) {
        // Pierre re-expands search matches after every tree mutation.
        // The normal expansion is restored when search closes.
        if self.desktop_search() {
            return;
        }
        if !self.expanded.remove(&path) {
            self.load_ancestors(&path, true);
            self.expanded.insert(path.clone());
            self.request(path, true);
        } else {
            self.expand_all = false;
            self.bound(true);
        }
        self.retain_discoveries();
    }
    fn load_ancestors(&mut self, path: &str, expand: bool) {
        let mut parent = String::new();
        let mut parts = path.split('/').peekable();
        while let Some(part) = parts.next() {
            if parts.peek().is_none() {
                break;
            }
            if !parent.is_empty() {
                parent.push('/');
            }
            parent.push_str(part);
            if expand {
                self.expanded.insert(parent.clone());
            }
            self.request(parent.clone(), false);
        }
    }
    fn select(&mut self, path: String, additive: bool, range: bool) {
        let rows = if range { self.view().rows } else { Vec::new() };
        (self.selected, self.anchor) = input::selection(
            &rows,
            &self.selected,
            self.anchor.as_deref(),
            &path,
            additive,
            range,
        );
    }
    fn query(&mut self, text: String) -> Option<String> {
        let text = if self.surface == FileTreeSurface::Desktop {
            input::desktop_query(&text)
        } else {
            text
        };
        let previous = self.search_query();
        if self.desktop_search() && text.trim().is_empty() {
            for path in self.selected.clone() {
                self.load_ancestors(&path, true);
            }
        }
        self.query = text;
        if self.query.trim().is_empty() && self.expand_all {
            self.expand_available();
        }
        if previous == self.search_query() {
            return None;
        }
        Some(self.restart_search())
    }
    fn search_query(&self) -> String {
        if self.host_folder {
            return String::new();
        }
        let text = if self.surface == FileTreeSurface::Mobile {
            self.query.trim()
        } else {
            &self.query
        };
        String::from_utf16_lossy(&text.encode_utf16().take(256).collect::<Vec<_>>())
    }
    fn restart_search(&mut self) -> String {
        self.search = None;
        self.search_query()
    }
    fn expand_available(&mut self) {
        if self.desktop_search() {
            return;
        }
        let paths = self
            .entries()
            .into_values()
            .filter(|entry| entry.kind == EntryKind::Directory && !entry.ignored)
            .map(|entry| entry.path)
            .collect::<Vec<_>>();
        for path in paths {
            self.expanded.insert(path.clone());
            if self.query.trim().is_empty() {
                self.request(path, false);
            }
        }
    }
    fn load_expanded(&mut self) {
        if self.surface == FileTreeSurface::Desktop && !self.query.trim().is_empty() {
            return;
        }
        let missing = self
            .entries()
            .into_values()
            .filter(|entry| {
                entry.kind == EntryKind::Directory
                    && self.expanded.contains(&entry.path)
                    && self.visible(&entry.path)
                    && !self.directories.contains_key(&entry.path)
            })
            .map(|entry| entry.path)
            .collect::<Vec<_>>();
        for path in missing {
            self.request(path, false);
        }
    }
    fn all_expanded(&self) -> bool {
        let entries = self.entries();
        let search = self.desktop_search().then(|| self.search_paths(&entries));
        all_expanded(&entries, search.as_ref().unwrap_or(&self.expanded))
    }
    fn desktop_search(&self) -> bool {
        self.surface == FileTreeSurface::Desktop && !self.query.trim().is_empty()
    }
    fn visible(&self, path: &str) -> bool {
        let mut parent = path;
        loop {
            if parent.is_empty() {
                return true;
            }
            if !self.expanded.contains(parent) {
                return false;
            }
            parent = parent.rsplit_once('/').map_or("", |(ancestor, _)| ancestor);
        }
    }
    fn bound(&mut self, preserve_visible: bool) {
        if self.surface != FileTreeSurface::Mobile {
            return;
        }
        let mut inactive = self
            .directories
            .iter()
            .filter(|(path, _)| !path.is_empty() && (!preserve_visible || !self.visible(path)))
            .map(|(path, directory)| (directory.used, path.clone()))
            .collect::<Vec<_>>();
        inactive.sort();
        let count = inactive.len().saturating_sub(64);
        for (_, path) in inactive.into_iter().take(count) {
            self.directories.remove(&path);
        }
    }
    fn retain_discoveries(&mut self) {
        let mut owned = BTreeSet::new();
        for path in self
            .directories
            .keys()
            .chain(&self.expanded)
            .chain(&self.selected)
        {
            let mut path = path.as_str();
            while !path.is_empty() {
                owned.insert(path.to_owned());
                path = path.rsplit_once('/').map_or("", |(parent, _)| parent);
            }
        }
        self.discoveries = self
            .entries()
            .into_iter()
            .filter(|(path, _)| owned.contains(path))
            .collect();
    }
    fn prune(&mut self, directory: &str, entries: &[TreeEntry]) {
        let prefix = if directory.is_empty() {
            String::new()
        } else {
            format!("{directory}/")
        };
        let children = entries
            .iter()
            .map(|entry| (entry.path.as_str(), entry.kind))
            .collect::<BTreeMap<_, _>>();
        let retained = |path: &str, directory: bool| {
            let Some(relative) = path.strip_prefix(&prefix) else {
                return true;
            };
            if relative.is_empty() {
                return true;
            }
            let (child, nested) = relative
                .split_once('/')
                .map_or((relative, false), |(child, _)| (child, true));
            children
                .get(format!("{prefix}{child}").as_str())
                .is_some_and(|kind| !(directory || nested) || *kind == EntryKind::Directory)
        };
        self.directories.retain(|path, _| retained(path, true));
        self.expanded.retain(|path| retained(path, true));
        self.queue.retain(|path| retained(path, true));
        self.running.retain(|path| retained(path, true));
        self.discoveries
            .retain(|path, entry| retained(path, entry.kind == EntryKind::Directory));
        self.selected.retain(|path| retained(path, false));
        self.anchor = self.anchor.take().filter(|path| retained(path, false));
        self.focused = self.focused.take().filter(|path| retained(path, false));
    }
}

fn all_expanded(entries: &BTreeMap<String, TreeEntry>, expanded: &BTreeSet<String>) -> bool {
    let mut directories = entries
        .values()
        .filter(|entry| entry.kind == EntryKind::Directory && !entry.ignored)
        .peekable();
    directories.peek().is_some() && directories.all(|entry| expanded.contains(&entry.path))
}

#[cfg_attr(feature = "bindings", uniffi::export)]
pub fn file_tree_path(root: &str, relative: &str) -> String {
    let separator = if agent_protocol::paths::is_windows_absolute_host_path(root) {
        '\\'
    } else {
        '/'
    };
    let base = if root.ends_with(separator) {
        root.into()
    } else {
        format!("{root}{separator}")
    };
    base + &relative.replace('/', &separator.to_string())
}
