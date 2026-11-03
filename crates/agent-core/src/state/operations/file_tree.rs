use super::*;
use crate::presentation::files::{FileTreeAction, FileTreeKey, tree::TreeWork};
use agent_protocol::file_tree::{ListTreeEntries, SearchTreeEntries, TreeEntries};

pub(crate) fn file_tree_action(
    snapshot: &mut Snapshot,
    key: FileTreeKey,
    action: FileTreeAction,
) -> Vec<Effect> {
    if (ListTreeEntries {
        root: key.root.clone(),
        directory: String::new(),
    })
    .validate()
    .is_err()
    {
        return Vec::new();
    }
    let connection = snapshot.connection_generation;
    let trees = &mut Arc::make_mut(&mut snapshot.workspace).file_trees;
    let work = trees.action(key, action, connection);
    if snapshot.connected {
        effects(work)
    } else {
        trees.disconnected();
        Vec::new()
    }
}
pub(crate) fn refresh_file_trees(snapshot: &mut Snapshot, path: &str) -> Vec<Effect> {
    if !snapshot.connected {
        return Vec::new();
    }
    effects(
        Arc::make_mut(&mut snapshot.workspace)
            .file_trees
            .refresh_matching(path, snapshot.connection_generation),
    )
}

fn effects(work: Vec<TreeWork>) -> Vec<Effect> {
    work.into_iter()
        .map(|work| match work {
            TreeWork::Directory {
                key,
                path,
                generation,
            } => Effect::execute(ReadTreeDirectory {
                key,
                path,
                generation,
            }),
            TreeWork::Search {
                key,
                query,
                generation,
            } => Effect::execute(DebounceSearchTree(SearchTree {
                key,
                query,
                generation,
            })),
        })
        .collect()
}

#[derive(Debug)]
struct ReadTreeDirectory {
    key: FileTreeKey,
    path: String,
    generation: u64,
}
impl Operation for ReadTreeDirectory {
    no_input!();
    type Output = Result<TreeEntries, String>;
    const STALE_POLICY: StalePolicy = StalePolicy::Apply;
    async fn run(&self, _: (), context: &mut Execution<'_>) -> Result<Self::Output, PeerError> {
        Ok(context
            .call(&ListTreeEntries {
                root: self.key.root.clone(),
                directory: self.path.clone(),
            })
            .await
            .map_err(|error| crate::presentation::error::error_message(&error.to_string())))
    }
    fn rejected_output(&self, _: ()) -> Option<Self::Output> {
        Some(Err("Unable to load folder.".into()))
    }
    fn apply(self, snapshot: &mut Snapshot, result: Self::Output) -> Vec<Effect> {
        effects(Arc::make_mut(&mut snapshot.workspace).file_trees.directory(
            &self.key,
            self.path,
            self.generation,
            result,
        ))
    }
}

#[derive(Debug)]
struct DebounceSearchTree(SearchTree);
impl Operation for DebounceSearchTree {
    no_input!();
    type Output = Result<(), String>;
    const STALE_POLICY: StalePolicy = StalePolicy::Apply;
    async fn run(&self, _: (), _: &mut Execution<'_>) -> Result<Self::Output, PeerError> {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        Ok(Ok(()))
    }
    fn rejected_output(&self, _: ()) -> Option<Self::Output> {
        Some(Err("Unable to search files.".into()))
    }
    fn apply(self, snapshot: &mut Snapshot, result: Self::Output) -> Vec<Effect> {
        if let Err(error) = result {
            return self.0.apply(snapshot, Err(error));
        }
        // The timer and the RPC have separate lifetimes. New input must start
        // its own debounce while a prior RPC is running, and stale timers must
        // never issue an obsolete Host search.
        if snapshot
            .workspace
            .file_trees
            .search_current(&self.0.key, self.0.generation)
        {
            vec![Effect::continuation(self.0)]
        } else {
            Vec::new()
        }
    }
}

#[derive(Debug)]
struct SearchTree {
    key: FileTreeKey,
    query: String,
    generation: u64,
}
impl Operation for SearchTree {
    no_input!();
    type Output = Result<TreeEntries, String>;
    const STALE_POLICY: StalePolicy = StalePolicy::Apply;
    async fn run(&self, _: (), context: &mut Execution<'_>) -> Result<Self::Output, PeerError> {
        Ok(context
            .call(&SearchTreeEntries {
                root: self.key.root.clone(),
                query: self.query.clone(),
                limit: 200,
                files_only: false,
            })
            .await
            .map_err(|error| crate::presentation::error::error_message(&error.to_string())))
    }
    fn rejected_output(&self, _: ()) -> Option<Self::Output> {
        Some(Err("Unable to search files.".into()))
    }
    fn apply(self, snapshot: &mut Snapshot, result: Self::Output) -> Vec<Effect> {
        Arc::make_mut(&mut snapshot.workspace).file_trees.searched(
            &self.key,
            self.generation,
            result,
        );
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presentation::files::FileTreeSurface;
    fn key(root: &str) -> FileTreeKey {
        FileTreeKey {
            owner: "browser".into(),
            root: root.into(),
        }
    }

    #[test]
    fn only_the_latest_debounce_can_start_a_host_search() {
        let mut snapshot = Snapshot::default();
        let trees = &mut Arc::make_mut(&mut snapshot.workspace).file_trees;
        trees.action(
            key("/workspace"),
            FileTreeAction::Open {
                surface: FileTreeSurface::Mobile,
                locale: "en-US".into(),
                workspace_root: "/workspace".into(),
            },
            1,
        );
        let mut query = |text: &str| {
            trees
                .action(
                    key("/workspace"),
                    FileTreeAction::Query { text: text.into() },
                    1,
                )
                .into_iter()
                .find_map(|work| {
                    let TreeWork::Search {
                        key,
                        query,
                        generation,
                    } = work
                    else {
                        return None;
                    };
                    Some(DebounceSearchTree(SearchTree {
                        key,
                        query,
                        generation,
                    }))
                })
                .unwrap()
        };
        let old = query("old");
        let current = query("current");
        assert!(old.apply(&mut snapshot, Ok(())).is_empty());
        assert_eq!(current.apply(&mut snapshot, Ok(())).len(), 1);
        let trees = &mut Arc::make_mut(&mut snapshot.workspace).file_trees;
        let pending = trees.action(
            key("/workspace"),
            FileTreeAction::Query {
                text: "pending".into(),
            },
            1,
        );
        let TreeWork::Search {
            key: pending_key,
            query,
            generation,
        } = pending.into_iter().next().unwrap()
        else {
            panic!()
        };
        trees.action(
            key("/workspace"),
            FileTreeAction::Query {
                text: String::new(),
            },
            1,
        );
        assert!(
            DebounceSearchTree(SearchTree {
                key: pending_key,
                query,
                generation
            })
            .apply(&mut snapshot, Ok(()))
            .is_empty()
        );
    }

    #[test]
    fn reducer_owns_tree_updates_without_mutating_published_snapshots() {
        let mut before = Snapshot {
            connected: true,
            ..Default::default()
        };
        Arc::make_mut(&mut before.navigation).cwd = "/another-thread".into();
        let (mut next, effects) = crate::state::reduce(
            &before,
            crate::state::Event::Intent(Intent::FileTree {
                key: key("/workspace"),
                action: FileTreeAction::Open {
                    surface: FileTreeSurface::Mobile,
                    locale: "en-US".into(),
                    workspace_root: "/workspace".into(),
                },
            }),
        );
        assert_eq!(effects.len(), 1);
        assert!(!before.file_tree(key("/workspace")).pending);
        assert!(next.file_tree(key("/workspace")).pending);
        assert!(!next.file_tree(key("/workspace")).ready);
        assert_eq!(
            file_tree_action(
                &mut next,
                key("/workspace"),
                FileTreeAction::Query {
                    text: "main".into()
                }
            )
            .len(),
            1,
            "the tree searches its owning workspace even when a side chat changed Store navigation"
        );
        // Invalid roots never reach a Host operation or create client state.
        assert!(file_tree_action(&mut next, key("relative"), FileTreeAction::Refresh).is_empty());
    }

    #[tokio::test]
    async fn disconnected_directory_failure_settles_in_the_tree_and_can_be_retried() {
        let store = crate::store::Store::offline(Snapshot::default());
        let open = Intent::FileTree {
            key: key("/workspace"),
            action: FileTreeAction::Open {
                surface: FileTreeSurface::Mobile,
                locale: "en-US".into(),
                workspace_root: "/workspace".into(),
            },
        };
        let _ = store.dispatch(open.clone()).await;
        let mut updates = store.subscribe();
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            updates.wait_for(|snapshot| {
                let tree = snapshot.file_tree(key("/workspace"));
                !tree.pending && tree.error.is_some()
            }),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(
            store
                .snapshot()
                .error_notices(crate::presentation::error::ErrorScope::Workspace)
                .is_empty()
        );
        let _ = store.dispatch(open).await;
        updates
            .wait_for(|snapshot| !snapshot.file_tree(key("/workspace")).pending)
            .await
            .unwrap();
        assert!(
            store
                .snapshot()
                .file_tree(key("/workspace"))
                .error
                .is_some()
        );
        store.close().await.unwrap();
    }
}
