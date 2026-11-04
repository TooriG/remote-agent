use super::*;
use crate::state::Snapshot;
use icu_collator::{
    Collator, CollatorBorrowed, CollatorPreferences,
    options::{CollatorOptions, Strength},
    preferences::CollationNumericOrdering,
};
use std::collections::HashSet;

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

#[derive(Clone, Copy)]
pub(super) struct EntryRef<'a> {
    pub(super) path: &'a str,
    pub(super) kind: EntryKind,
    pub(super) ignored: bool,
}

impl<'a> From<&'a TreeEntry> for EntryRef<'a> {
    fn from(entry: &'a TreeEntry) -> Self {
        Self {
            path: &entry.path,
            kind: entry.kind,
            ignored: entry.ignored,
        }
    }
}

impl Tree {
    pub(super) fn entries(&self) -> BTreeMap<&str, EntryRef<'_>> {
        let mut entries: BTreeMap<_, _> = self
            .discoveries
            .iter()
            .map(|(path, entry)| (path.as_str(), EntryRef::from(entry)))
            .collect();
        let mut pending = vec![""];
        // Remembered directories can own cached children before their parent is loaded.
        pending.extend(
            entries
                .values()
                .filter(|entry| entry.kind == EntryKind::Directory)
                .map(|entry| entry.path),
        );
        let mut visited = BTreeSet::new();
        while let Some(path) = pending.pop() {
            if !visited.insert(path) {
                continue;
            }
            if let Some(children) = self
                .directories
                .get(path)
                .and_then(|directory| directory.entries.as_deref())
            {
                for entry in children {
                    entries.insert(entry.path.as_str(), EntryRef::from(entry));
                    if entry.kind == EntryKind::Directory {
                        pending.push(entry.path.as_str());
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
                    .entry(entry.path.as_str())
                    .or_insert(EntryRef::from(entry));
                let mut path = entry.path.as_str();
                while let Some((parent, _)) = path.rsplit_once('/') {
                    entries.entry(parent).or_insert(EntryRef {
                        path: parent,
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
        let all_expanded = if self.desktop_search() {
            all_expanded(&entries, &visible)
        } else {
            all_expanded(&entries, &self.expanded)
        };
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
        let can_expand_all = !self.host_folder
            && entries
                .values()
                .any(|entry| entry.kind == EntryKind::Directory);
        let mut children: BTreeMap<&str, Vec<EntryRef<'_>>> = BTreeMap::new();
        for entry in entries.into_values() {
            let parent = entry.path.rsplit_once('/').map_or("", |(parent, _)| parent);
            children.entry(parent).or_default().push(entry);
        }
        let rows = self.rows(children, searching, &visible, collator.as_ref());
        let focused_path = super::input::focused_path(&rows, self.focused.as_deref(), &self.query);
        FileTreeView {
            focused_path,
            can_expand_all,
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
        mut children: BTreeMap<&str, Vec<EntryRef<'_>>>,
        searching: bool,
        visible: &BTreeSet<&str>,
        collator: Option<&CollatorBorrowed<'_>>,
    ) -> Vec<FileTreeRow> {
        // Keep the common small selection allocation-free. Large selections
        // must not rescan every selected path for every visible row.
        let selected = (self.selected.len() >= 64).then(|| {
            self.selected
                .iter()
                .map(String::as_str)
                .collect::<HashSet<_>>()
        });
        let visible = searching.then_some(visible);
        let mut pending =
            ordered_children(children.remove("").unwrap_or_default(), collator, visible)
                .into_iter()
                .rev()
                .map(|entry| (entry, 0))
                .collect::<Vec<_>>();
        let mut rows = Vec::new();
        while let Some((mut entry, depth)) = pending.pop() {
            let mut label = basename(entry.path).to_owned();
            if self.surface == FileTreeSurface::Desktop {
                while entry.kind == EntryKind::Directory
                    && let Some(nested) = children.get(entry.path)
                    && nested.len() == 1
                    && nested[0].kind == EntryKind::Directory
                {
                    entry = nested[0];
                    label.push_str(" / ");
                    label.push_str(basename(entry.path));
                }
            }
            let expanded = (searching && self.surface == FileTreeSurface::Desktop)
                || self.expanded.contains(entry.path);
            rows.push(FileTreeRow {
                path: entry.path.to_owned(),
                label,
                kind: entry.kind,
                depth,
                expanded,
                selected: selected.as_ref().map_or_else(
                    || self.selected.iter().any(|path| path == entry.path),
                    |selected| selected.contains(entry.path),
                ),
                ignored: entry.ignored,
                loading: self.running.contains(entry.path)
                    || self.queue.iter().any(|path| path == entry.path),
            });
            if entry.kind == EntryKind::Directory
                && (expanded || searching)
                && let Some(nested) = children.remove(entry.path)
            {
                pending.extend(
                    ordered_children(nested, collator, visible)
                        .into_iter()
                        .rev()
                        .map(|entry| (entry, depth + 1)),
                );
            }
        }
        rows
    }
    pub(super) fn search_paths<'a>(
        &'a self,
        entries: &BTreeMap<&'a str, EntryRef<'a>>,
    ) -> BTreeSet<&'a str> {
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
                        .map(|entry| entry.path.as_str())
                        .collect()
                })
        } else {
            let query = super::input::desktop_query(&self.query);
            entries
                .values()
                .filter(|entry| super::input::matches_query_path(entry.path, entry.kind, &query))
                .map(|entry| entry.path)
                .collect()
        };
        let mut visible = BTreeSet::new();
        for path in paths {
            let mut parent = path;
            while let Some((ancestor, _)) = parent.rsplit_once('/') {
                visible.insert(ancestor);
                parent = ancestor;
            }
            visible.insert(path);
        }
        visible
    }
}

fn ordered_children<'a>(
    mut entries: Vec<EntryRef<'a>>,
    collator: Option<&CollatorBorrowed<'_>>,
    visible: Option<&BTreeSet<&str>>,
) -> Vec<EntryRef<'a>> {
    if let Some(visible) = visible {
        entries.retain(|entry| visible.contains(entry.path));
    }
    if let Some(collator) = collator {
        entries.sort_by(|left, right| {
            (right.kind == EntryKind::Directory)
                .cmp(&(left.kind == EntryKind::Directory))
                .then_with(|| collator.compare(basename(left.path), basename(right.path)))
        });
    } else {
        entries.sort_by_cached_key(|entry| {
            (
                entry.kind != EntryKind::Directory,
                sort::Key::new(basename(entry.path)),
            )
        });
    }
    entries
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
