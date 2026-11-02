use super::*;

/// Native input produces one shared state action and any resulting UI operation.
pub(super) struct FileTreeInput {
    pub(super) actions: Vec<FileTreeAction>,
    pub(super) menu_path: Option<String>,
}

pub(super) fn focused_path(
    rows: &[FileTreeRow],
    focused: Option<&str>,
    query: &str,
) -> Option<String> {
    let query = desktop_query(query);
    if !query.is_empty() {
        return rows
            .iter()
            .filter(|row| matches_query_path(&row.path, row.kind, &query))
            .find(|row| Some(row.path.as_str()) == focused)
            .or_else(|| {
                rows.iter()
                    .find(|row| matches_query_path(&row.path, row.kind, &query))
            })
            .map(|row| row.path.clone());
    }
    rows.iter()
        .find(|row| Some(row.path.as_str()) == focused)
        .or_else(|| {
            rows.iter()
                .rev()
                .find(|row| focused.is_some_and(|path| path.starts_with(&format!("{}/", row.path))))
        })
        .or_else(|| rows.first())
        .map(|row| row.path.clone())
}
pub(super) fn desktop_query(query: &str) -> String {
    query.trim().replace('\\', "/").to_lowercase()
}
pub(super) fn matches_query_path(path: &str, kind: EntryKind, query: &str) -> bool {
    let suffix = if kind == EntryKind::Directory {
        "/"
    } else {
        ""
    };
    format!("{}{suffix}", path.to_lowercase()).contains(query)
}

impl FileTreeView {
    /// A compressed directory chain shares a row with its terminal directory.
    pub fn index_for_path(&self, path: &str) -> Option<usize> {
        self.rows
            .iter()
            .position(|row| row.path == path)
            .or_else(|| {
                self.rows
                    .iter()
                    .position(|row| row.path.starts_with(&format!("{path}/")))
            })
    }
    pub(super) fn desktop_keyboard_input(
        &self,
        key: &str,
        additive: bool,
        shift: bool,
        anchor: Option<&str>,
    ) -> Option<FileTreeInput> {
        if !self.query.trim().is_empty() {
            return self.search_input(key);
        }
        let focused = self.focused_path.as_deref()?;
        let index = self.rows.iter().position(|row| row.path == focused)?;
        let row = &self.rows[index];
        let next = match key {
            "down" => (index + 1).min(self.rows.len() - 1),
            "up" => index.saturating_sub(1),
            "home" => 0,
            "end" => self.rows.len() - 1,
            _ => index,
        };
        let mut menu_path = None;
        let action = match key {
            "down" | "up" if shift => {
                let mut paths = self.selected_paths.clone();
                if next != index {
                    let next_path = &self.rows[next].path;
                    if paths.contains(&row.path) && paths.contains(next_path) {
                        paths.retain(|path| path != &row.path);
                    } else if !paths.contains(next_path) {
                        paths.push(next_path.clone());
                    }
                }
                FileTreeAction::SelectPaths {
                    paths,
                    anchor: anchor.map(str::to_owned).or_else(|| Some(row.path.clone())),
                    focused: Some(self.rows[next].path.clone()),
                }
            }
            "down" | "up" | "home" | "end" => FileTreeAction::Focus {
                path: self.rows[next].path.clone(),
            },
            "right" if row.kind == EntryKind::Directory && !row.expanded => {
                FileTreeAction::Toggle {
                    path: row.path.clone(),
                }
            }
            "right" => FileTreeAction::Focus {
                path: self.rows[(index + 1).min(self.rows.len() - 1)].path.clone(),
            },
            "left" if row.kind == EntryKind::Directory && row.expanded => FileTreeAction::Toggle {
                path: row.path.clone(),
            },
            "left" => {
                let parent = self.rows[..index]
                    .iter()
                    .rev()
                    .find(|parent| row.path.starts_with(&format!("{}/", parent.path)))?;
                FileTreeAction::Focus {
                    path: parent.path.clone(),
                }
            }
            "a" if additive => {
                let paths: Vec<_> = self.rows.iter().map(|row| row.path.clone()).collect();
                FileTreeAction::SelectPaths {
                    paths,
                    anchor: self
                        .focused_path
                        .clone()
                        .or_else(|| anchor.map(str::to_owned)),
                    focused: self.focused_path.clone(),
                }
            }
            "enter" | "space" => {
                if key == "space" && additive {
                    FileTreeAction::Select {
                        path: focused.into(),
                        additive: true,
                        range: false,
                    }
                } else {
                    FileTreeAction::Click {
                        path: focused.into(),
                        additive,
                        range: shift,
                    }
                }
            }
            "contextmenu" => {
                menu_path = Some(focused.into());
                FileTreeAction::Focus {
                    path: focused.into(),
                }
            }
            _ => return None,
        };
        Some(FileTreeInput {
            actions: vec![action],
            menu_path,
        })
    }
    fn search_input(&self, key: &str) -> Option<FileTreeInput> {
        let action = match key {
            "escape" => FileTreeAction::Query {
                text: String::new(),
            },
            "enter" => {
                let path = self.focused_path.clone()?;
                return Some(FileTreeInput {
                    actions: vec![
                        FileTreeAction::Select {
                            path,
                            additive: false,
                            range: false,
                        },
                        FileTreeAction::Query {
                            text: String::new(),
                        },
                    ],
                    menu_path: None,
                });
            }
            "up" | "down" => {
                let query = desktop_query(&self.query);
                let matches: Vec<_> = self
                    .rows
                    .iter()
                    .filter(|row| matches_query_path(&row.path, row.kind, &query))
                    .collect();
                if matches.is_empty() {
                    return None;
                }
                let index = matches
                    .iter()
                    .position(|row| Some(&row.path) == self.focused_path.as_ref())
                    .unwrap_or_default();
                let next = if key == "down" {
                    (index + 1).min(matches.len() - 1)
                } else {
                    index.saturating_sub(1)
                };
                FileTreeAction::Focus {
                    path: matches[next].path.clone(),
                }
            }
            _ => return None,
        };
        Some(FileTreeInput {
            actions: vec![action],
            menu_path: None,
        })
    }
}

impl FileTreeView {
    /// Pierre suppresses dragging during search and drags the whole selection
    /// only when the grabbed row already belongs to it.
    pub fn drag_paths(&self, path: &str) -> Vec<String> {
        if !self.query.trim().is_empty() {
            Vec::new()
        } else if self.selected_paths.iter().any(|selected| selected == path) {
            self.selected_paths.clone()
        } else {
            vec![path.into()]
        }
    }
    pub fn handles_desktop_key(&self, key: &str, additive: bool, shift: bool) -> bool {
        self.desktop_keyboard_input(key, additive, shift, None)
            .is_some()
    }
}

pub(super) fn selection(
    rows: &[FileTreeRow],
    current: &[String],
    anchor: Option<&str>,
    path: &str,
    additive: bool,
    range: bool,
) -> (Vec<String>, Option<String>) {
    let mut selected = if additive {
        current.to_vec()
    } else {
        Vec::new()
    };
    if range
        && let Some(anchor) = anchor
        && let (Some(first), Some(last)) = (
            rows.iter().position(|row| row.path == anchor),
            rows.iter().position(|row| row.path == path),
        )
    {
        for row in &rows[first.min(last)..=first.max(last)] {
            if !selected.contains(&row.path) {
                selected.push(row.path.clone());
            }
        }
        return (selected, Some(anchor.into()));
    }
    if let Some(index) = selected.iter().position(|selected| selected == path) {
        if !range {
            selected.remove(index);
        }
    } else {
        selected.push(path.into());
    }
    (selected, Some(path.into()))
}
