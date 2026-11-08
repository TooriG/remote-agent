use super::*;
const ROOT: &str = "/workspace";
fn key(root: &str) -> FileTreeKey {
    FileTreeKey {
        owner: "browser".into(),
        root: root.into(),
    }
}

fn entry(path: &str, kind: EntryKind) -> TreeEntry {
    TreeEntry {
        path: path.into(),
        kind,
        ignored: false,
    }
}
fn open(trees: &mut FileTrees, root: &str, surface: FileTreeSurface) -> Vec<TreeWork> {
    trees.action(
        key(root),
        FileTreeAction::Open {
            surface,
            locale: "en-US".into(),
            workspace_root: root.into(),
        },
        1,
    )
}
fn action(trees: &mut FileTrees, action: FileTreeAction) -> Vec<TreeWork> {
    trees.action(key(ROOT), action, 1)
}
fn complete(trees: &mut FileTrees, work: &TreeWork, entries: Vec<TreeEntry>) -> Vec<TreeWork> {
    let TreeWork::Directory {
        key,
        path,
        generation,
    } = work
    else {
        panic!("directory work")
    };
    trees.directory(
        key,
        path.clone(),
        *generation,
        Ok(TreeEntries {
            entries,
            truncated: false,
        }),
    )
}
fn paths(trees: &FileTrees) -> Vec<String> {
    trees
        .view(&key(ROOT))
        .rows
        .into_iter()
        .map(|row| row.path)
        .collect()
}
fn toggle(trees: &mut FileTrees, path: &str) -> Vec<TreeWork> {
    action(trees, FileTreeAction::Toggle { path: path.into() })
}

#[test]
fn collapsing_before_an_expansion_finishes_rejects_late_visibility() {
    let mut trees = FileTrees::default();
    let root = open(&mut trees, ROOT, FileTreeSurface::Desktop);
    complete(
        &mut trees,
        &root[0],
        vec![entry("src", EntryKind::Directory)],
    );
    let pending = toggle(&mut trees, "src");
    toggle(&mut trees, "src");
    complete(
        &mut trees,
        &pending[0],
        vec![entry("src/file.rs", EntryKind::File)],
    );
    assert_eq!(paths(&trees), ["src"]);
    assert!(!trees.view(&key(ROOT)).rows[0].expanded);
}

#[test]
fn replacing_a_directory_prunes_only_its_removed_or_changed_descendants() {
    let mut tree = Tree::default();
    let directories = ["src/keep", "src/change", "src/gone", "src2"];
    for path in directories {
        tree.directories.insert(path.into(), Directory::default());
        tree.expanded.insert(path.into());
        tree.discoveries
            .insert(path.into(), entry(path, EntryKind::Directory));
    }
    tree.running = ["src/keep".into(), "src/gone".into()].into();
    tree.queue = ["src/change".into(), "src2".into()].into();
    tree.selected = [
        "src/keep/a.rs",
        "src/change",
        "src/change/nested.rs",
        "src/gone/a.rs",
        "src2/a.rs",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    tree.anchor = Some("src/gone/a.rs".into());
    tree.focused = Some("src/change".into());
    tree.prune(
        "src",
        &[
            entry("src/keep", EntryKind::Directory),
            entry("src/change", EntryKind::File),
        ],
    );
    let remaining = ["src/keep", "src2"];
    assert!(tree.directories.keys().map(String::as_str).eq(remaining));
    assert!(tree.expanded.iter().map(String::as_str).eq(remaining));
    assert!(tree.running.iter().map(String::as_str).eq(["src/keep"]));
    assert!(tree.queue.iter().map(String::as_str).eq(["src2"]));
    assert!(tree.discoveries.keys().map(String::as_str).eq(remaining));
    assert_eq!(tree.selected, ["src/keep/a.rs", "src/change", "src2/a.rs"]);
    assert!(tree.anchor.is_none());
    assert_eq!(tree.focused.as_deref(), Some("src/change"));
}

#[test]
fn cached_children_remain_visible_while_a_parent_listing_is_unavailable() {
    let mut tree = Tree::default();
    tree.discoveries
        .insert("src".into(), entry("src", EntryKind::Directory));
    tree.expanded.insert("src".into());
    tree.directories.insert(
        "src".into(),
        Directory {
            entries: Some(Arc::new(vec![
                entry("src/file10.rs", EntryKind::File),
                entry("src/file2.rs", EntryKind::File),
            ])),
            ..Default::default()
        },
    );
    let view = tree.view();
    assert!(!view.ready);
    assert_eq!(
        view.rows
            .iter()
            .map(|row| row.path.as_str())
            .collect::<Vec<_>>(),
        ["src", "src/file2.rs", "src/file10.rs"]
    );
}

#[test]
fn mobile_order_keeps_primary_collation_ties_in_source_path_order() {
    let mut trees = FileTrees::default();
    let root = open(&mut trees, ROOT, FileTreeSurface::Mobile);
    let mut entries = ["ä", "a10", "a2", "á", "a01", "A1", "a", "A"]
        .into_iter()
        .map(|path| entry(path, EntryKind::File))
        .collect::<Vec<_>>();
    entries.push(entry("z", EntryKind::Directory));
    complete(&mut trees, &root[0], entries);
    assert_eq!(
        paths(&trees),
        ["z", "A", "a", "á", "ä", "A1", "a01", "a2", "a10"]
    );
}

#[test]
fn range_selection_keeps_first_occurrences_and_existing_order() {
    let mut trees = FileTrees::default();
    let root = open(&mut trees, ROOT, FileTreeSurface::Desktop);
    complete(
        &mut trees,
        &root[0],
        ["a", "b", "c", "d"]
            .into_iter()
            .map(|path| entry(path, EntryKind::File))
            .collect(),
    );
    let mut rows = trees.view(&key(ROOT)).rows;
    rows.insert(2, rows[1].clone());
    let current = ["b".into(), "a".into(), "b".into()];
    for (anchor, target) in [("a", "d"), ("d", "a")] {
        let (selected, retained_anchor) =
            super::input::selection(&rows, &current, Some(anchor), target, true, true);
        assert_eq!(selected, ["b", "a", "b", "c", "d"]);
        assert_eq!(retained_anchor.as_deref(), Some(anchor));
        let (selected, _) =
            super::input::selection(&rows, &current, Some(anchor), target, false, true);
        assert_eq!(selected, ["a", "b", "c", "d"]);
    }
}

#[test]
fn shift_selection_can_cover_a_large_tree_in_display_order() {
    let mut trees = FileTrees::default();
    let root = open(&mut trees, ROOT, FileTreeSurface::Desktop);
    let paths: Vec<_> = (0..1024).map(|id| format!("file{id}.rs")).collect();
    complete(
        &mut trees,
        &root[0],
        paths
            .iter()
            .rev()
            .map(|path| entry(path, EntryKind::File))
            .collect(),
    );
    action(
        &mut trees,
        FileTreeAction::Select {
            path: paths[0].clone(),
            additive: false,
            range: false,
        },
    );
    action(
        &mut trees,
        FileTreeAction::Select {
            path: paths.last().unwrap().clone(),
            additive: false,
            range: true,
        },
    );
    let view = trees.view(&key(ROOT));
    assert_eq!(view.selected_paths, paths);
    assert!(view.rows.iter().all(|row| row.selected));
    action(
        &mut trees,
        FileTreeAction::Select {
            path: paths[1].clone(),
            additive: false,
            range: false,
        },
    );
    assert_eq!(trees.view(&key(ROOT)).selected_paths, [paths[1].clone()]);
    assert_eq!(
        view.selected_paths, paths,
        "previous projections keep their own selection"
    );
}

#[test]
fn large_selections_keep_input_order_and_exact_row_membership() {
    let mut trees = FileTrees::default();
    let root = open(&mut trees, ROOT, FileTreeSurface::Desktop);
    complete(
        &mut trees,
        &root[0],
        (0..256)
            .map(|id| entry(&format!("file{id}.rs"), EntryKind::File))
            .collect(),
    );
    for count in [0, 1, 15, 16, 63, 64, 128] {
        let selected: Vec<_> = (0..count)
            .rev()
            .map(|id| format!("file{}.rs", id * 2))
            .collect();
        action(
            &mut trees,
            FileTreeAction::SelectPaths {
                paths: selected.clone(),
                anchor: None,
                focused: None,
            },
        );
        let view = trees.view(&key(ROOT));
        assert_eq!(view.selected_paths, selected);
        let expected: BTreeSet<_> = selected.iter().collect();
        assert_eq!(view.rows.iter().filter(|row| row.selected).count(), count);
        for row in &view.rows {
            assert_eq!(row.selected, expected.contains(&row.path));
        }
    }
}

#[test]
fn lazy_tree_sorts_directories_first_and_desktop_flattens_single_directory_chains() {
    let mut trees = FileTrees::default();
    let root = open(&mut trees, ROOT, FileTreeSurface::Desktop);
    assert!(trees.view(&key(ROOT)).pending);
    assert!(!trees.view(&key(ROOT)).ready);
    complete(
        &mut trees,
        &root[0],
        vec![
            entry("file10", EntryKind::File),
            entry("src", EntryKind::Directory),
            entry("file2", EntryKind::File),
        ],
    );
    assert_eq!(paths(&trees), ["src", "file2", "file10"]);
    let src = toggle(&mut trees, "src");
    assert!(toggle(&mut trees, "src").is_empty());
    assert!(toggle(&mut trees, "src").is_empty());
    complete(
        &mut trees,
        &src[0],
        vec![
            entry("src/nested", EntryKind::Directory),
            entry("wrong/parent", EntryKind::File),
        ],
    );
    let view = trees.view(&key(ROOT));
    assert_eq!(view.rows[0].label, "src / nested");
    assert_eq!(view.rows[0].path, "src/nested");
    assert!(!view.rows[0].expanded);
    let nested = toggle(&mut trees, "src/nested");
    complete(
        &mut trees,
        &nested[0],
        vec![entry("src/nested/main.rs", EntryKind::File)],
    );
    assert_eq!(trees.view(&key(ROOT)).rows[1].depth, 1);
    let mobile = open(&mut trees, ROOT, FileTreeSurface::Mobile);
    assert!(!mobile.is_empty());
    assert_eq!(
        &paths(&trees)[..3],
        ["src", "src/nested", "src/nested/main.rs"]
    );
    assert_eq!(trees.view(&key(ROOT)).rows[2].depth, 2);
}

#[test]
fn directory_batches_deduplicate_in_order_and_retry_only_missing_or_failed_entries() {
    for count in [0, 1, 31, 32, 63, 64, 128] {
        let mut tree = Tree::default();
        tree.request(["running".into()], false);
        let started = tree.start(&key(ROOT));
        assert_eq!(started.len(), 1);
        tree.request(["queued".into()], false);
        for (path, error) in [("loaded", None), ("failed", Some("old failure".into()))] {
            tree.directories.insert(
                path.into(),
                Directory {
                    entries: Some(Arc::default()),
                    error,
                    used: 7,
                },
            );
        }
        let names = (0..count).map(|i| format!("new{i}")).collect::<Vec<_>>();
        let requested = ["running", "queued", "loaded", "failed"]
            .into_iter()
            .map(str::to_owned)
            .chain(names.iter().flat_map(|path| [path.clone(), path.clone()]))
            .collect::<Vec<_>>();
        tree.request(requested.clone(), false);
        let mut expected = ["queued", "failed"]
            .into_iter()
            .map(str::to_owned)
            .chain(names)
            .collect::<Vec<_>>();
        assert!(tree.queue.iter().eq(&expected));
        assert!(tree.directories["failed"].error.is_none());
        assert_eq!(tree.directories["loaded"].used, 7);
        tree.request(requested.clone(), false);
        assert!(tree.queue.iter().eq(&expected));
        tree.request(requested, true);
        expected.push("loaded".into());
        assert!(tree.queue.iter().eq(&expected));
        assert_eq!(tree.start(&key(ROOT)).len(), expected.len().min(3));
        assert!(tree.running.len() <= 4);
    }
}

#[test]
fn large_expansion_keeps_queued_and_running_rows_loading_and_starts_work_in_order() {
    let mut trees = FileTrees::default();
    let root = open(&mut trees, ROOT, FileTreeSurface::Desktop);
    let mut entries = (0..130)
        .map(|i| entry(&format!("dir{i:03}"), EntryKind::Directory))
        .collect::<Vec<_>>();
    entries.push(entry("file.txt", EntryKind::File));
    complete(&mut trees, &root[0], entries);
    let running = action(&mut trees, FileTreeAction::ToggleAll);
    assert_eq!(running.len(), 4);
    for row in trees.view(&key(ROOT)).rows {
        assert_eq!(row.loading, row.kind == EntryKind::Directory);
    }
    let next = complete(&mut trees, &running[0], Vec::new());
    assert!(matches!(next.as_slice(), [TreeWork::Directory { path, .. }] if path == "dir004"));
    let view = trees.view(&key(ROOT));
    assert!(
        !view
            .rows
            .iter()
            .find(|row| row.path == "dir000")
            .unwrap()
            .loading
    );
    assert!(
        view.rows
            .iter()
            .find(|row| row.path == "dir004")
            .unwrap()
            .loading
    );
    assert!(
        view.rows
            .iter()
            .find(|row| row.path == "dir129")
            .unwrap()
            .loading
    );
}

#[test]
fn expand_all_limits_concurrency_and_skips_ignored_folders_until_manual_open() {
    let mut trees = FileTrees::default();
    let root = open(&mut trees, ROOT, FileTreeSurface::Mobile);
    let mut entries = (0..7)
        .map(|n| entry(&format!("dir{n}"), EntryKind::Directory))
        .collect::<Vec<_>>();
    entries.push(TreeEntry {
        ignored: true,
        ..entry("node_modules", EntryKind::Directory)
    });
    complete(&mut trees, &root[0], entries);
    let work = action(&mut trees, FileTreeAction::ToggleAll);
    assert_eq!(work.len(), 4);
    let pending = trees.view(&key(ROOT));
    assert!(
        pending
            .rows
            .iter()
            .filter(|row| !row.ignored)
            .all(|row| row.loading)
    );
    assert!(
        pending
            .rows
            .iter()
            .filter(|row| row.ignored)
            .all(|row| !row.loading)
    );
    assert!(trees.view(&key(ROOT)).all_expanded);
    assert!(
        !trees
            .view(&key(ROOT))
            .rows
            .iter()
            .find(|row| row.path == "node_modules")
            .unwrap()
            .expanded
    );
    assert_eq!(complete(&mut trees, &work[0], vec![]).len(), 1);
    let next = complete(
        &mut trees,
        &work[1],
        vec![entry("dir1/child", EntryKind::Directory)],
    );
    assert_eq!(next.len(), 1);
    assert!(
        trees
            .view(&key(ROOT))
            .rows
            .iter()
            .find(|row| row.path == "dir1/child")
            .unwrap()
            .expanded
    );
    action(&mut trees, FileTreeAction::ToggleAll);
    assert!(trees.view(&key(ROOT)).rows.iter().all(|row| !row.expanded));
    toggle(&mut trees, "node_modules");
    assert!(
        trees
            .tree_mut(&key(ROOT))
            .unwrap()
            .queue
            .iter()
            .any(|path| path == "node_modules")
    );
}

#[test]
fn expand_all_control_tracks_manual_expansion_and_never_counts_ignored_only_folders() {
    let mut trees = FileTrees::default();
    let root = open(&mut trees, ROOT, FileTreeSurface::Desktop);
    complete(
        &mut trees,
        &root[0],
        vec![
            entry("src", EntryKind::Directory),
            TreeEntry {
                ignored: true,
                ..entry("vendor", EntryKind::Directory)
            },
        ],
    );
    assert!(!trees.view(&key(ROOT)).all_expanded);
    toggle(&mut trees, "vendor");
    assert!(!trees.view(&key(ROOT)).all_expanded);
    toggle(&mut trees, "src");
    assert!(trees.view(&key(ROOT)).all_expanded);
    action(&mut trees, FileTreeAction::ToggleAll);
    assert!(trees.view(&key(ROOT)).rows.iter().all(|row| !row.expanded));
    let refresh = action(&mut trees, FileTreeAction::Refresh);
    let root = refresh
        .iter()
        .find(|work| matches!(work, TreeWork::Directory { path, .. } if path.is_empty()))
        .unwrap();
    complete(
        &mut trees,
        root,
        vec![TreeEntry {
            ignored: true,
            ..entry("vendor", EntryKind::Directory)
        }],
    );
    assert!(!trees.view(&key(ROOT)).all_expanded);
    toggle(&mut trees, "vendor");
    assert!(!trees.view(&key(ROOT)).all_expanded);
}

#[test]
fn refresh_and_reconnection_reject_stale_results_without_discarding_rows_or_selection() {
    let mut trees = FileTrees::default();
    let root = open(&mut trees, ROOT, FileTreeSurface::Mobile);
    complete(
        &mut trees,
        &root[0],
        vec![entry("src", EntryKind::Directory)],
    );
    let old = toggle(&mut trees, "src");
    complete(
        &mut trees,
        &old[0],
        vec![entry("src/main.rs", EntryKind::File)],
    );
    action(
        &mut trees,
        FileTreeAction::Select {
            path: "src/main.rs".into(),
            additive: false,
            range: false,
        },
    );
    let refresh = action(&mut trees, FileTreeAction::Refresh);
    assert_eq!(refresh.len(), 2);
    assert_eq!(trees.view(&key(ROOT)).selected_paths, ["src/main.rs"]);
    assert!(complete(&mut trees, &old[0], vec![]).is_empty());
    assert!(paths(&trees).contains(&"src/main.rs".into()));
    let reconnect = trees.action(
        key(ROOT),
        FileTreeAction::Open {
            surface: FileTreeSurface::Mobile,
            locale: "en".into(),
            workspace_root: "/workspace".into(),
        },
        2,
    );
    assert_eq!(reconnect.len(), 2);
    for work in refresh {
        complete(&mut trees, &work, vec![]);
    }
    assert!(paths(&trees).contains(&"src/main.rs".into()));
    let current_root = reconnect
        .iter()
        .find(|work| matches!(work, TreeWork::Directory { path, .. } if path.is_empty()))
        .unwrap();
    complete(
        &mut trees,
        current_root,
        vec![entry("src", EntryKind::File)],
    );
    assert_eq!(paths(&trees), ["src"]);
    assert!(trees.view(&key(ROOT)).selected_paths.is_empty());
    assert!(
        !trees
            .tree_mut(&key(ROOT))
            .unwrap()
            .directories
            .contains_key("src")
    );
}

#[test]
fn directory_failures_keep_previous_data_and_reopening_retries() {
    let mut trees = FileTrees::default();
    let root = open(&mut trees, ROOT, FileTreeSurface::Mobile);
    complete(&mut trees, &root[0], vec![entry("kept", EntryKind::File)]);
    let work = action(&mut trees, FileTreeAction::Refresh);
    let TreeWork::Directory { generation, .. } = work[0] else {
        panic!()
    };
    trees.directory(
        &key(ROOT),
        String::new(),
        generation,
        Err("Disconnected".into()),
    );
    assert_eq!(paths(&trees), ["kept"]);
    assert_eq!(
        trees
            .view(&key(ROOT))
            .error
            .and_then(|error| error.details)
            .as_deref(),
        Some("Disconnected")
    );
    let retry = open(&mut trees, ROOT, FileTreeSurface::Mobile);
    assert_eq!(retry.len(), 1);
    assert!(trees.view(&key(ROOT)).error.is_none());
    complete(&mut trees, &retry[0], vec![]);
    assert!(paths(&trees).is_empty());
}

#[test]
fn disconnect_settles_pending_work_and_rejects_completions_from_the_old_connection() {
    let mut trees = FileTrees::default();
    let root = open(&mut trees, ROOT, FileTreeSurface::Mobile);
    let search = action(
        &mut trees,
        FileTreeAction::Query {
            text: "main".into(),
        },
    );
    let TreeWork::Search { generation, .. } = search[0] else {
        panic!()
    };
    let published = trees.clone();
    trees.disconnected();
    assert!(!trees.view(&key(ROOT)).pending);
    assert!(trees.view(&key(ROOT)).error.is_some());
    assert!(published.view(&key(ROOT)).pending);
    complete(&mut trees, &root[0], vec![entry("old", EntryKind::File)]);
    trees.searched(
        &key(ROOT),
        generation,
        Ok(TreeEntries {
            entries: vec![entry("main", EntryKind::File)],
            truncated: false,
        }),
    );
    assert!(paths(&trees).is_empty());
    let retry = open(&mut trees, ROOT, FileTreeSurface::Mobile);
    assert_eq!(retry.len(), 2);
    assert!(trees.view(&key(ROOT)).pending);
    assert!(trees.view(&key(ROOT)).error.is_none());
}

#[test]
fn mobile_uses_backend_matches_while_desktop_uses_pierre_literal_paths() {
    let mut trees = FileTrees::default();
    let root = open(&mut trees, ROOT, FileTreeSurface::Mobile);
    complete(
        &mut trees,
        &root[0],
        vec![entry("src", EntryKind::Directory)],
    );
    let search = action(
        &mut trees,
        FileTreeAction::Query {
            text: "s/mrs".into(),
        },
    );
    assert!(paths(&trees).is_empty());
    let TreeWork::Search { generation, .. } = search[0] else {
        panic!()
    };
    trees.searched(
        &key(ROOT),
        generation,
        Ok(TreeEntries {
            entries: vec![entry("src/main.rs", EntryKind::File)],
            truncated: true,
        }),
    );
    assert_eq!(paths(&trees), ["src", "src/main.rs"]);
    assert!(trees.view(&key(ROOT)).truncated);
    trees.tree_mut(&key(ROOT)).unwrap().surface = FileTreeSurface::Desktop;
    assert!(paths(&trees).is_empty());
    action(
        &mut trees,
        FileTreeAction::Query {
            text: "SRC\\main".into(),
        },
    );
    // A new query removes the old server result until its own response arrives.
    assert!(paths(&trees).is_empty());
    let generation = trees.tree_mut(&key(ROOT)).unwrap().search_generation;
    trees.searched(
        &key(ROOT),
        generation,
        Ok(TreeEntries {
            entries: vec![entry("src/main.rs", EntryKind::File)],
            truncated: false,
        }),
    );
    assert_eq!(paths(&trees), ["src", "src/main.rs"]);
    assert!(trees.view(&key(ROOT)).rows[0].expanded);
    action(
        &mut trees,
        FileTreeAction::Reveal {
            path: Some("src/main.rs".into()),
            directory: false,
        },
    );
    assert!(trees.view(&key(ROOT)).query.is_empty());
    assert!(paths(&trees).contains(&"src/main.rs".into()));
}

#[test]
fn desktop_search_preserves_normal_expansion_and_defers_expand_all_until_search_closes() {
    let mut trees = FileTrees::default();
    let root = open(&mut trees, ROOT, FileTreeSurface::Desktop);
    complete(
        &mut trees,
        &root[0],
        vec![
            entry("a", EntryKind::Directory),
            entry("b", EntryKind::Directory),
        ],
    );
    let a = toggle(&mut trees, "a");
    complete(&mut trees, &a[0], vec![entry("a/main.rs", EntryKind::File)]);
    action(
        &mut trees,
        FileTreeAction::Query {
            text: "main".into(),
        },
    );
    assert!(trees.view(&key(ROOT)).rows[0].expanded);
    assert!(toggle(&mut trees, "a").is_empty());
    assert!(trees.view(&key(ROOT)).rows[0].expanded);
    assert!(!trees.view(&key(ROOT)).all_expanded);
    assert_eq!(
        trees.view(&key(ROOT)).toggle_all_label,
        "すべてのフォルダを展開"
    );
    assert!(action(&mut trees, FileTreeAction::ToggleAll).is_empty());
    assert_eq!(
        trees.view(&key(ROOT)).toggle_all_label,
        "すべてのフォルダを折りたたむ"
    );
    assert!(!trees.view(&key(ROOT)).all_expanded);
    let pending = action(
        &mut trees,
        FileTreeAction::Query {
            text: String::new(),
        },
    );
    assert!(
        pending
            .iter()
            .any(|work| matches!(work, TreeWork::Directory { path, .. } if path == "b"))
    );
    assert!(trees.view(&key(ROOT)).all_expanded);
    action(
        &mut trees,
        FileTreeAction::Query {
            text: "main".into(),
        },
    );
    action(&mut trees, FileTreeAction::ToggleAll);
    action(
        &mut trees,
        FileTreeAction::Query {
            text: String::new(),
        },
    );
    assert!(
        trees.view(&key(ROOT)).all_expanded,
        "closing Pierre search restores the pre-search expansion"
    );
}

#[test]
fn search_generations_query_clear_and_workspace_eviction_reject_old_responses() {
    let mut trees = FileTrees::default();
    let old_root = open(&mut trees, ROOT, FileTreeSurface::Mobile);
    let work = action(&mut trees, FileTreeAction::Query { text: "old".into() });
    let TreeWork::Search {
        generation: old, ..
    } = work[0]
    else {
        panic!()
    };
    let work = action(
        &mut trees,
        FileTreeAction::Query {
            text: String::new(),
        },
    );
    assert!(work.is_empty());
    trees.searched(
        &key(ROOT),
        old,
        Ok(TreeEntries {
            entries: vec![entry("old", EntryKind::File)],
            truncated: false,
        }),
    );
    assert!(paths(&trees).is_empty());
    for index in 0..8 {
        open(
            &mut trees,
            &format!("/root{index}"),
            FileTreeSurface::Mobile,
        );
    }
    assert_eq!(trees.trees.len(), 8);
    let new_root = open(&mut trees, ROOT, FileTreeSurface::Mobile);
    complete(
        &mut trees,
        &old_root[0],
        vec![entry("old", EntryKind::File)],
    );
    assert!(paths(&trees).is_empty());
    complete(
        &mut trees,
        &new_root[0],
        vec![entry("new", EntryKind::File)],
    );
    assert_eq!(paths(&trees), ["new"]);
}

#[test]
fn query_bounds_and_host_folder_controls_follow_each_surface() {
    let mut trees = FileTrees::default();
    let root = open(&mut trees, ROOT, FileTreeSurface::Mobile);
    complete(
        &mut trees,
        &root[0],
        vec![entry("loaded", EntryKind::Directory)],
    );
    let work = action(
        &mut trees,
        FileTreeAction::Query {
            text: format!("  {}  ", "🦀".repeat(140)),
        },
    );
    assert!(matches!(&work[0], TreeWork::Search { query, .. } if query == &"🦀".repeat(128)));
    trees.tree_mut(&key(ROOT)).unwrap().surface = FileTreeSurface::Desktop;
    let work = action(
        &mut trees,
        FileTreeAction::Query {
            text: format!(" {}", "🦀".repeat(140)),
        },
    );
    assert!(
        matches!(&work[0], TreeWork::Search { query, .. } if query.encode_utf16().count() == 256 && !query.starts_with(' '))
    );
    trees.action(
        key(ROOT),
        FileTreeAction::Open {
            surface: FileTreeSurface::Desktop,
            locale: "en-US".into(),
            workspace_root: "/other-workspace".into(),
        },
        1,
    );
    let work = trees.action(
        key(ROOT),
        FileTreeAction::Query {
            text: "load".into(),
        },
        1,
    );
    assert!(work.is_empty());
    assert_eq!(paths(&trees), ["loaded"]);
    assert!(!trees.view(&key(ROOT)).can_expand_all);
    assert!(
        trees
            .action(key(ROOT), FileTreeAction::ToggleAll, 1)
            .is_empty()
    );
    assert!(!trees.tree_mut(&key(ROOT)).unwrap().expand_all);
    assert_eq!(
        file_tree_path("C:\\workspace", "nested/a b.rs"),
        "C:\\workspace\\nested\\a b.rs"
    );
    assert_eq!(
        file_tree_path("/workspace/", "a.rs:12"),
        "/workspace/a.rs:12"
    );
}

#[test]
fn selection_ranges_follow_display_order() {
    let mut trees = FileTrees::default();
    let root = open(&mut trees, ROOT, FileTreeSurface::Desktop);
    complete(
        &mut trees,
        &root[0],
        ["a", "b", "c", "d"]
            .map(|path| entry(path, EntryKind::File))
            .into(),
    );
    action(
        &mut trees,
        FileTreeAction::Select {
            path: "b".into(),
            additive: false,
            range: false,
        },
    );
    action(
        &mut trees,
        FileTreeAction::Select {
            path: "d".into(),
            additive: false,
            range: true,
        },
    );
    assert_eq!(trees.view(&key(ROOT)).selected_paths, ["b", "c", "d"]);
}

#[test]
fn hidden_range_anchors_never_toggle_selected_rows() {
    let mut trees = FileTrees::default();
    let root = open(&mut trees, ROOT, FileTreeSurface::Desktop);
    complete(
        &mut trees,
        &root[0],
        ["a", "b", "c", "d"]
            .map(|path| entry(path, EntryKind::File))
            .into(),
    );
    for path in ["a", "d"] {
        action(
            &mut trees,
            FileTreeAction::Click {
                path: path.into(),
                additive: true,
                range: false,
            },
        );
    }
    action(&mut trees, FileTreeAction::Query { text: "a".into() });
    action(
        &mut trees,
        FileTreeAction::Click {
            path: "a".into(),
            additive: true,
            range: true,
        },
    );
    assert_eq!(trees.view(&key(ROOT)).selected_paths, ["a", "d"]);
}

#[test]
fn desktop_row_clicks_close_search_keep_selected_ancestors_open_and_use_the_last_item() {
    let mut trees = FileTrees::default();
    let root = open(&mut trees, ROOT, FileTreeSurface::Desktop);
    complete(
        &mut trees,
        &root[0],
        vec![
            entry("src", EntryKind::Directory),
            entry("a.rs", EntryKind::File),
        ],
    );
    let search = action(
        &mut trees,
        FileTreeAction::Query {
            text: "NESTED".into(),
        },
    );
    let TreeWork::Search { generation, .. } = search[0] else {
        panic!()
    };
    let before = trees.revision();
    trees.searched(
        &key(ROOT),
        generation,
        Ok(TreeEntries {
            entries: vec![entry("src/nested.rs", EntryKind::File)],
            truncated: false,
        }),
    );
    assert!(trees.revision() > before);
    assert_eq!(trees.view(&key(ROOT)).query, "nested");
    let work = action(
        &mut trees,
        FileTreeAction::Click {
            path: "src/nested.rs".into(),
            additive: false,
            range: false,
        },
    );
    let first = trees.view(&key(ROOT)).open_file.unwrap();
    assert_eq!(first.path, "src/nested.rs");
    assert!(trees.view(&key(ROOT)).query.is_empty());
    assert!(
        trees
            .view(&key(ROOT))
            .rows
            .iter()
            .any(|row| row.path == "src" && row.expanded)
    );
    let src = work
        .iter()
        .find(|work| matches!(work, TreeWork::Directory { path, .. } if path == "src"))
        .unwrap();
    complete(
        &mut trees,
        src,
        vec![entry("src/nested.rs", EntryKind::File)],
    );
    assert!(
        trees
            .view(&key(ROOT))
            .rows
            .iter()
            .any(|row| row.path == "src/nested.rs" && row.selected)
    );
    action(
        &mut trees,
        FileTreeAction::Click {
            path: "src".into(),
            additive: true,
            range: false,
        },
    );
    assert_eq!(
        trees.view(&key(ROOT)).open_file.unwrap().id,
        first.id,
        "a final folder must not emit another file request"
    );
    action(
        &mut trees,
        FileTreeAction::Click {
            path: "src".into(),
            additive: true,
            range: false,
        },
    );
    assert!(trees.view(&key(ROOT)).open_file.unwrap().id > first.id);
    assert!(
        trees
            .view(&key(ROOT))
            .rows
            .iter()
            .any(|row| row.path == "src" && row.expanded)
    );
    action(
        &mut trees,
        FileTreeAction::Click {
            path: "src".into(),
            additive: false,
            range: false,
        },
    );
    assert!(
        trees
            .view(&key(ROOT))
            .rows
            .iter()
            .any(|row| row.path == "src" && !row.expanded)
    );
    action(
        &mut trees,
        FileTreeAction::Reveal {
            path: Some("src".into()),
            directory: true,
        },
    );
    assert!(
        trees
            .view(&key(ROOT))
            .rows
            .iter()
            .any(|row| row.path == "src" && row.expanded)
    );
    assert!(trees.view(&key(ROOT)).open_file.is_none());
}

#[test]
fn desktop_keyboard_moves_focus_without_opening_and_matches_pierre_selection_commands() {
    let mut trees = FileTrees::default();
    let root = open(&mut trees, ROOT, FileTreeSurface::Desktop);
    complete(
        &mut trees,
        &root[0],
        vec![
            entry("dir", EntryKind::Directory),
            entry("a.rs", EntryKind::File),
            entry("b.rs", EntryKind::File),
        ],
    );
    let press = |trees: &mut FileTrees, input_key: &str, additive, shift| {
        let before = trees.view(&key(ROOT)).open_file;
        action(
            trees,
            FileTreeAction::Keyboard {
                key: input_key.into(),
                additive,
                shift,
            },
        );
        trees
            .view(&key(ROOT))
            .open_file
            .filter(|request| Some(request.id) != before.map(|request| request.id))
            .map(|request| request.path)
    };
    assert_eq!(press(&mut trees, "down", false, false), None);
    assert_eq!(trees.view(&key(ROOT)).focused_path.as_deref(), Some("a.rs"));
    assert!(trees.view(&key(ROOT)).selected_paths.is_empty());
    assert_eq!(
        press(&mut trees, "enter", false, false).as_deref(),
        Some("a.rs")
    );
    assert_eq!(press(&mut trees, "enter", false, false), None);
    assert_eq!(
        press(&mut trees, "down", false, true).as_deref(),
        Some("b.rs")
    );
    assert_eq!(trees.view(&key(ROOT)).selected_paths, ["a.rs", "b.rs"]);
    assert_eq!(
        press(&mut trees, "up", false, true).as_deref(),
        Some("a.rs")
    );
    assert_eq!(trees.view(&key(ROOT)).selected_paths, ["a.rs"]);
    assert_eq!(press(&mut trees, "a", true, false).as_deref(), Some("b.rs"));
    assert_eq!(
        trees.view(&key(ROOT)).selected_paths,
        ["dir", "a.rs", "b.rs"]
    );
    press(&mut trees, "contextmenu", false, false);
    assert_eq!(trees.view(&key(ROOT)).context_menu.unwrap().path, "a.rs");
    action(
        &mut trees,
        FileTreeAction::Query {
            text: "b.rs".into(),
        },
    );
    assert_eq!(trees.view(&key(ROOT)).focused_path.as_deref(), Some("b.rs"));
    assert!(!trees.view(&key(ROOT)).handles_desktop_key("a", true, false));
    press(&mut trees, "enter", false, false);
    assert!(trees.view(&key(ROOT)).query.is_empty());
    assert_eq!(trees.view(&key(ROOT)).selected_paths, ["b.rs"]);
    // Navigation and selection are reduced against the owner's latest state,
    // even if no intermediate snapshot has reached the native view.
    press(&mut trees, "home", false, false);
    action(
        &mut trees,
        FileTreeAction::Keyboard {
            key: "down".into(),
            additive: false,
            shift: false,
        },
    );
    action(
        &mut trees,
        FileTreeAction::Keyboard {
            key: "down".into(),
            additive: false,
            shift: false,
        },
    );
    assert_eq!(trees.view(&key(ROOT)).focused_path.as_deref(), Some("b.rs"));
    let open_id = trees.view(&key(ROOT)).open_file.unwrap().id;
    assert!(
        trees.view(&key(ROOT)).context_menu.is_some(),
        "menu and file requests cannot overwrite one another"
    );
    action(
        &mut trees,
        FileTreeAction::Reveal {
            path: None,
            directory: false,
        },
    );
    assert!(trees.view(&key(ROOT)).open_file.is_none());
    assert!(trees.view(&key(ROOT)).context_menu.is_none());
    assert_eq!(trees.view(&key(ROOT)).selected_paths, ["b.rs"]);
    press(&mut trees, "space", true, false);
    press(&mut trees, "space", true, false);
    assert!(trees.view(&key(ROOT)).open_file.unwrap().id > open_id);
    let reopened = open(&mut trees, ROOT, FileTreeSurface::Desktop);
    assert!(!reopened.is_empty());
    assert!(trees.view(&key(ROOT)).open_file.is_none());
}

#[test]
fn evicted_expanded_children_reload_after_reopening_their_parent() {
    let mut trees = FileTrees::default();
    let root = open(&mut trees, ROOT, FileTreeSurface::Mobile);
    complete(
        &mut trees,
        &root[0],
        vec![entry("parent", EntryKind::Directory)],
    );
    // Mobile retains expansion after evicting a collapsed folder's cache.
    trees
        .tree_mut(&key(ROOT))
        .unwrap()
        .expanded
        .insert("parent/child".into());
    let parent = toggle(&mut trees, "parent");
    let child = complete(
        &mut trees,
        &parent[0],
        vec![
            TreeEntry {
                ignored: true,
                ..entry("parent/child", EntryKind::Directory)
            },
            entry("parent/closed", EntryKind::Directory),
            entry("parent/file.txt", EntryKind::File),
        ],
    );
    assert!(
        matches!(child.as_slice(), [TreeWork::Directory { path, .. }] if path == "parent/child")
    );
    assert!(
        complete(
            &mut trees,
            &child[0],
            vec![entry("parent/child/found.txt", EntryKind::File)]
        )
        .is_empty()
    );
    assert!(paths(&trees).contains(&"parent/child/found.txt".into()));
}

#[test]
fn mobile_bounds_inactive_directory_cache_without_losing_expansion_or_selection() {
    let mut tree = Tree {
        surface: FileTreeSurface::Mobile,
        ..Default::default()
    };
    tree.directories.insert(String::new(), Directory::default());
    for index in 0..80 {
        let path = format!("d{index}");
        tree.directories.insert(
            path.clone(),
            Directory {
                used: index,
                ..Default::default()
            },
        );
        if index < 5 {
            tree.expanded.insert(path);
        }
    }
    tree.selected.push("d79/file".into());
    tree.bound(true);
    assert_eq!(tree.directories.len(), 70);
    assert!(tree.directories.contains_key("d0"));
    assert!(!tree.directories.contains_key("d5"));
    tree.bound(false);
    assert_eq!(tree.directories.len(), 65);
    assert_eq!(tree.expanded.len(), 5);
    assert_eq!(tree.selected, ["d79/file"]);
}

#[test]
fn independent_browsers_do_not_share_requests_selection_or_mount_lifetimes() {
    let mut trees = FileTrees::default();
    let main = key(ROOT);
    let side = FileTreeKey {
        owner: "side-chat".into(),
        root: ROOT.into(),
    };
    let main_work = open(&mut trees, ROOT, FileTreeSurface::Desktop);
    let side_work = trees.action(
        side.clone(),
        FileTreeAction::Open {
            surface: FileTreeSurface::Desktop,
            locale: "en-US".into(),
            workspace_root: ROOT.into(),
        },
        1,
    );
    complete(
        &mut trees,
        &main_work[0],
        vec![entry("main.rs", EntryKind::File)],
    );
    complete(
        &mut trees,
        &side_work[0],
        vec![entry("side.rs", EntryKind::File)],
    );
    action(
        &mut trees,
        FileTreeAction::Select {
            path: "main.rs".into(),
            additive: false,
            range: false,
        },
    );
    assert_eq!(trees.view(&main).selected_paths, ["main.rs"]);
    assert!(trees.view(&side).selected_paths.is_empty());
    assert_eq!(trees.view(&side).rows[0].path, "side.rs");
    trees.action(main.clone(), FileTreeAction::Close, 1);
    assert!(!trees.view(&main).ready);
    assert!(trees.view(&side).ready);
    let reopened = open(&mut trees, ROOT, FileTreeSurface::Desktop);
    complete(
        &mut trees,
        &main_work[0],
        vec![entry("old.rs", EntryKind::File)],
    );
    assert!(!trees.view(&main).ready);
    complete(
        &mut trees,
        &reopened[0],
        vec![entry("new.rs", EntryKind::File)],
    );
    assert_eq!(paths(&trees), ["new.rs"]);
    open(&mut trees, "/other", FileTreeSurface::Desktop);
    assert!(
        !trees.view(&main).ready,
        "desktop replaces the model when its root key changes"
    );
    assert!(trees.view(&side).ready);
    for index in 0..9 {
        open(
            &mut trees,
            &format!("/mobile{index}"),
            FileTreeSurface::Mobile,
        );
    }
    assert_eq!(
        trees
            .trees
            .iter()
            .filter(|(key, _)| key.owner == "browser")
            .count(),
        8
    );
    assert!(
        trees.view(&side).ready,
        "mobile cache eviction belongs to its own browser"
    );
}

#[test]
fn pull_refresh_indicator_tracks_the_loaded_root_and_not_child_or_search_requests() {
    let mut trees = FileTrees::default();
    let work = open(&mut trees, ROOT, FileTreeSurface::Mobile);
    assert!(!trees.view(&key(ROOT)).refreshing);
    complete(
        &mut trees,
        &work[0],
        vec![entry("src", EntryKind::Directory)],
    );
    let child = toggle(&mut trees, "src");
    assert!(trees.view(&key(ROOT)).pending);
    assert!(!trees.view(&key(ROOT)).refreshing);
    complete(&mut trees, &child[0], vec![]);
    let refresh = action(&mut trees, FileTreeAction::Refresh);
    assert!(trees.view(&key(ROOT)).refreshing);
    for work in refresh {
        complete(&mut trees, &work, vec![]);
    }
    assert!(!trees.view(&key(ROOT)).refreshing);
    action(
        &mut trees,
        FileTreeAction::Query {
            text: "main".into(),
        },
    );
    assert!(trees.view(&key(ROOT)).pending);
    assert!(!trees.view(&key(ROOT)).refreshing);
}

#[test]
fn workspace_mutations_refresh_only_matching_active_roots_and_preserve_selection() {
    let mut trees = FileTrees::default();
    let old = open(&mut trees, "/old", FileTreeSurface::Mobile);
    let root = open(&mut trees, ROOT, FileTreeSurface::Mobile);
    complete(&mut trees, &root[0], vec![entry("a.rs", EntryKind::File)]);
    action(
        &mut trees,
        FileTreeAction::Select {
            path: "a.rs".into(),
            additive: false,
            range: false,
        },
    );
    assert!(trees.refresh_matching("/old/a.rs", 1).is_empty());
    assert!(trees.refresh_matching("/elsewhere", 1).is_empty());
    let refresh = trees.refresh_matching("/workspace/a.rs", 1);
    assert_eq!(refresh.len(), 1);
    assert!(trees.view(&key(ROOT)).refreshing);
    assert_eq!(trees.view(&key(ROOT)).selected_paths, ["a.rs"]);
    assert!(complete(&mut trees, &old[0], vec![]).is_empty());
    complete(
        &mut trees,
        &refresh[0],
        vec![entry("b.rs", EntryKind::File)],
    );
    assert_eq!(paths(&trees), ["b.rs"]);
}

#[test]
fn mobile_folder_taps_expand_load_and_collapse() {
    let mut trees = FileTrees::default();
    let root = open(&mut trees, ROOT, FileTreeSurface::Mobile);
    complete(
        &mut trees,
        &root[0],
        vec![
            entry("src", EntryKind::Directory),
            entry("a.rs", EntryKind::File),
        ],
    );
    action(
        &mut trees,
        FileTreeAction::Click {
            path: "a.rs".into(),
            additive: false,
            range: false,
        },
    );
    let selected = trees.view(&key(ROOT)).open_file;
    let click = || FileTreeAction::Click {
        path: "src".into(),
        additive: false,
        range: false,
    };
    let work = action(&mut trees, click());
    assert_eq!(work.len(), 1);
    assert!(trees.view(&key(ROOT)).rows[0].expanded);
    complete(
        &mut trees,
        &work[0],
        vec![entry("src/main.rs", EntryKind::File)],
    );
    assert_eq!(paths(&trees), ["src", "src/main.rs", "a.rs"]);
    assert!(action(&mut trees, click()).is_empty());
    assert_eq!(paths(&trees), ["src", "a.rs"]);
    assert_eq!(trees.view(&key(ROOT)).selected_paths, ["a.rs"]);
    assert_eq!(trees.view(&key(ROOT)).open_file, selected);
    assert!(!trees.view(&key(ROOT)).rows[0].expanded);
}

#[test]
fn mobile_file_taps_reopen_the_same_selection_with_a_fresh_request() {
    for surface in [FileTreeSurface::Mobile, FileTreeSurface::Desktop] {
        let mut trees = FileTrees::default();
        let root = open(&mut trees, ROOT, surface);
        complete(&mut trees, &root[0], vec![entry("a.rs", EntryKind::File)]);
        let click = || FileTreeAction::Click {
            path: "a.rs".into(),
            additive: false,
            range: false,
        };
        action(&mut trees, click());
        let first = trees.view(&key(ROOT)).open_file.unwrap();
        action(&mut trees, click());
        let second = trees.view(&key(ROOT)).open_file.unwrap();
        assert_eq!(first.path, second.path);
        assert_eq!(first.id != second.id, surface == FileTreeSurface::Mobile);
    }
}
