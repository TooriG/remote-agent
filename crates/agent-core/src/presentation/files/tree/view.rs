use super::*;
use crate::state::Snapshot;
use icu_collator::{
    Collator, CollatorPreferences,
    options::{CollatorOptions, Strength},
    preferences::CollationNumericOrdering,
};

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "bindings", derive(uniffi::Record))]
pub struct FileTreeRow {
    pub path: String,
    pub label: String,
    pub kind: EntryKind,
    pub depth: u32,
    pub expanded: bool,
    pub selected: bool,
    pub ignored: bool,
    pub loading: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "bindings", derive(uniffi::Record))]
pub struct FileTreeView {
    pub rows: Vec<FileTreeRow>,
    pub selected_paths: Vec<String>,
    pub open_file: Option<FileTreeRequest>,
    pub context_menu: Option<FileTreeRequest>,
    pub reveal_id: u64,
    pub focused_path: Option<String>,
    pub query: String,
    pub ready: bool,
    pub pending: bool,
    pub refreshing: bool,
    pub all_expanded: bool,
    pub toggle_all_label: String,
    pub can_expand_all: bool,
    pub truncated: bool,
    pub error: Option<crate::presentation::error::ErrorText>,
}

impl Tree {
    pub(super) fn entries(&self) -> BTreeMap<String, TreeEntry> {
        let mut entries = self.discoveries.clone();
        let mut pending = vec![String::new()];
        pending.extend(
            entries
                .values()
                .filter(|entry| entry.kind == EntryKind::Directory)
                .map(|entry| entry.path.clone()),
        );
        let mut visited = BTreeSet::new();
        while let Some(path) = pending.pop() {
            if !visited.insert(path.clone()) {
                continue;
            }
            if let Some(children) = self
                .directories
                .get(&path)
                .and_then(|directory| directory.entries.as_deref())
            {
                for entry in children {
                    entries.insert(entry.path.clone(), entry.clone());
                    if entry.kind == EntryKind::Directory {
                        pending.push(entry.path.clone());
                    }
                }
            }
        }
        if !self.query.trim().is_empty()
            && let Some(Ok(search)) = &self.search
        {
            for entry in &search.entries {
                if !valid_tree_path(&entry.path) {
                    continue;
                }
                entries
                    .entry(entry.path.clone())
                    .or_insert_with(|| entry.clone());
                let mut path = entry.path.as_str();
                while let Some((parent, _)) = path.rsplit_once('/') {
                    entries.entry(parent.into()).or_insert_with(|| TreeEntry {
                        path: parent.into(),
                        kind: EntryKind::Directory,
                        ignored: false,
                    });
                    path = parent;
                }
            }
        }
        entries
    }
    pub(super) fn view(&self) -> FileTreeView {
        let entries = self.entries();
        let searching = !self.query.trim().is_empty();
        let visible = if searching {
            self.search_paths(&entries)
        } else {
            BTreeSet::new()
        };
        let all_expanded = all_expanded(
            &entries,
            if self.desktop_search() {
                &visible
            } else {
                &self.expanded
            },
        );
        let collator = (self.surface == FileTreeSurface::Mobile).then(|| {
            let mut preferences: CollatorPreferences = self
                .locale
                .replace('_', "-")
                .parse::<icu_locale_core::Locale>()
                .map(Into::into)
                .unwrap_or_default();
            preferences.numeric_ordering = Some(CollationNumericOrdering::True);
            let mut options = CollatorOptions::default();
            options.strength = Some(Strength::Primary);
            Collator::try_new(preferences, options).expect("compiled ICU collation data")
        });
        let sort_keys = entries
            .keys()
            .filter(|_| self.surface == FileTreeSurface::Desktop)
            .map(|path| (path.as_str(), sort::Key::new(basename(path))))
            .collect::<BTreeMap<_, _>>();
        let mut children: BTreeMap<&str, Vec<&TreeEntry>> = BTreeMap::new();
        for entry in entries.values() {
            let parent = entry.path.rsplit_once('/').map_or("", |(parent, _)| parent);
            children.entry(parent).or_default().push(entry);
        }
        for entries in children.values_mut() {
            entries.sort_by(|left, right| {
                let directory = |entry: &TreeEntry| entry.kind == EntryKind::Directory;
                directory(right)
                    .cmp(&directory(left))
                    .then_with(|| match self.surface {
                        FileTreeSurface::Desktop => {
                            sort_keys[left.path.as_str()].compare(&sort_keys[right.path.as_str()])
                        }
                        FileTreeSurface::Mobile => collator
                            .as_ref()
                            .unwrap()
                            .compare(basename(&left.path), basename(&right.path)),
                    })
            });
        }
        let rows = self.rows(&children, searching, &visible);
        let focused_path = super::input::focused_path(&rows, self.focused.as_deref(), &self.query);
        FileTreeView {
            focused_path,
            can_expand_all: !self.host_folder
                && entries
                    .values()
                    .any(|entry| entry.kind == EntryKind::Directory),
            rows,
            selected_paths: self.selected.clone(),
            open_file: self.open_file.clone(),
            context_menu: self.context_menu.clone(),
            reveal_id: self.reveal_id,
            query: self.query.clone(),
            ready: self
                .directories
                .get("")
                .is_some_and(|directory| directory.entries.is_some()),
            pending: !self.running.is_empty()
                || !self.queue.is_empty()
                || (searching && !self.search_query().trim().is_empty() && self.search.is_none()),
            refreshing: self
                .directories
                .get("")
                .is_some_and(|directory| directory.entries.is_some())
                && (self.running.contains("") || self.queue.iter().any(|path| path.is_empty())),
            all_expanded,
            toggle_all_label: if self.expand_all || all_expanded {
                "すべてのフォルダを折りたたむ"
            } else {
                "すべてのフォルダを展開"
            }
            .into(),
            truncated: searching
                && self
                    .search
                    .as_ref()
                    .is_some_and(|result| result.as_ref().is_ok_and(|search| search.truncated)),
            error: self
                .directories
                .values()
                .find_map(|directory| directory.error.as_deref())
                .or_else(|| {
                    self.search
                        .as_ref()
                        .and_then(|search| search.as_ref().err())
                        .map(String::as_str)
                })
                .map(crate::presentation::error::error_text),
        }
    }
    fn rows(
        &self,
        children: &BTreeMap<&str, Vec<&TreeEntry>>,
        searching: bool,
        visible: &BTreeSet<String>,
    ) -> Vec<FileTreeRow> {
        let mut pending = children
            .get("")
            .into_iter()
            .flatten()
            .rev()
            .map(|entry| (*entry, 0))
            .collect::<Vec<_>>();
        let mut rows = Vec::new();
        while let Some((mut entry, depth)) = pending.pop() {
            if searching && !visible.contains(&entry.path) {
                continue;
            }
            let mut label = basename(&entry.path).to_owned();
            if self.surface == FileTreeSurface::Desktop {
                while entry.kind == EntryKind::Directory
                    && let Some(nested) = children.get(entry.path.as_str())
                    && nested.len() == 1
                    && nested[0].kind == EntryKind::Directory
                {
                    entry = nested[0];
                    label.push_str(" / ");
                    label.push_str(basename(&entry.path));
                }
            }
            let expanded = (searching && self.surface == FileTreeSurface::Desktop)
                || self.expanded.contains(&entry.path);
            rows.push(FileTreeRow {
                path: entry.path.clone(),
                label,
                kind: entry.kind,
                depth,
                expanded,
                selected: self.selected.contains(&entry.path),
                ignored: entry.ignored,
                loading: self.running.contains(&entry.path) || self.queue.contains(&entry.path),
            });
            if entry.kind == EntryKind::Directory
                && (expanded || searching)
                && let Some(nested) = children.get(entry.path.as_str())
            {
                pending.extend(nested.iter().rev().map(|entry| (*entry, depth + 1)));
            }
        }
        rows
    }
    pub(super) fn search_paths(&self, entries: &BTreeMap<String, TreeEntry>) -> BTreeSet<String> {
        // Mobile filters with the server result set. Pierre on desktop instead
        // merges those paths into its model, then does a literal substring match.
        let paths = if self.surface == FileTreeSurface::Mobile {
            self.search
                .as_ref()
                .and_then(|result| result.as_ref().ok())
                .map_or_else(Vec::new, |search| {
                    search
                        .entries
                        .iter()
                        .map(|entry| entry.path.clone())
                        .collect()
                })
        } else {
            let query = super::input::desktop_query(&self.query);
            entries
                .values()
                .filter(|entry| super::input::matches_query_path(&entry.path, entry.kind, &query))
                .map(|entry| entry.path.clone())
                .collect()
        };
        let mut visible = BTreeSet::new();
        for path in paths {
            let mut parent = path.as_str();
            while let Some((ancestor, _)) = parent.rsplit_once('/') {
                visible.insert(ancestor.into());
                parent = ancestor;
            }
            visible.insert(path);
        }
        visible
    }
}
fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

#[cfg_attr(feature = "bindings", uniffi::export)]
impl Snapshot {
    pub fn file_tree(&self, key: FileTreeKey) -> FileTreeView {
        self.workspace.file_trees.view(&key)
    }
}
