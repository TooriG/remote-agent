use agent_protocol::models::Project;
use anyhow::Context;
use std::{
    collections::{HashMap, HashSet},
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

pub(crate) mod icons;
pub(crate) mod state;
pub(crate) mod titles;

/// Keep the primary provider's project roots when catalogs overlap. Resolve each
/// path once per snapshot, rather than doing filesystem I/O for every root pair.
pub(crate) fn merge_catalogs(
    mut primary: Vec<Project>,
    supplemental: Vec<Project>,
) -> Vec<Project> {
    let mut resolved = HashMap::new();
    for root in primary
        .iter()
        .chain(&supplemental)
        .flat_map(|project| &project.roots)
    {
        resolved.entry(root.path.clone()).or_insert_with(|| {
            dunce::canonicalize(&root.path).unwrap_or_else(|_| root.path.clone().into())
        });
    }
    let mut targets: HashSet<_> = primary
        .iter()
        .flat_map(|project| &project.roots)
        .map(|root| &resolved[&root.path])
        .collect();
    for mut project in supplemental {
        let native_group = project.roots.is_empty();
        project
            .roots
            .retain(|root| !targets.contains(&resolved[&root.path]));
        if native_group || !project.roots.is_empty() {
            targets.extend(project.roots.iter().map(|root| &resolved[&root.path]));
            primary.push(project);
        }
    }
    primary
}

/// BEX registrations supplement the read-only native provider project catalog.
#[derive(Debug, Clone)]
pub struct ProjectStore {
    path: PathBuf,
    registration: Arc<tokio::sync::Mutex<()>>,
}
impl ProjectStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            registration: Arc::default(),
        }
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub(crate) fn chat_directory(&self) -> PathBuf {
        self.path.with_file_name("bex-chats")
    }

    async fn read_projects(&self) -> anyhow::Result<Vec<Project>> {
        match tokio::fs::read(self.path.with_file_name("bex-projects.json")).await {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(error.into()),
        }
    }
    pub(crate) async fn register(&self, root: &Path) -> anyhow::Result<String> {
        anyhow::ensure!(root.is_absolute(), "project directory must be absolute");
        let root = tokio::fs::canonicalize(root).await?;
        let root = dunce::simplified(&root);
        anyhow::ensure!(
            tokio::fs::metadata(root).await?.is_dir(),
            "project path must be a directory"
        );
        let _registration = self.registration.lock().await;
        let mut projects = self.read_projects().await?;
        let path = root.to_str().context("project path is not UTF-8")?;
        for registered in projects.iter().flat_map(|project| &project.roots) {
            if registered.path == path
                || tokio::fs::canonicalize(&registered.path)
                    .await
                    .ok()
                    .is_some_and(|path| dunce::simplified(&path) == root)
            {
                return Ok(path.into());
            }
        }
        projects.push(Project {
            list_revision: 0,
            id: uuid::Uuid::new_v4().to_string(),
            name: root
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.into()),
            roots: vec![agent_protocol::models::ProjectRoot { path: path.into() }],
            favicon_png: None,
        });
        let file = self.path.with_file_name("bex-projects.json");
        let bytes = serde_json::to_vec(&projects)?;
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            if let Some(parent) = file.parent() {
                std::fs::create_dir_all(parent)?;
            }
            atomicwrites::AtomicFile::new(&file, atomicwrites::AllowOverwrite).write(|f| {
                use std::io::Write;
                f.write_all(&bytes)
            })?;
            Ok(())
        })
        .await??;
        Ok(path.into())
    }
    #[cfg(test)]
    pub(crate) async fn load(&self) -> anyhow::Result<state::Snapshot> {
        self.load_with_projects(Vec::new()).await
    }
    pub(crate) async fn load_with_projects(
        &self,
        mut native: Vec<Project>,
    ) -> anyhow::Result<state::Snapshot> {
        for mut project in self.read_projects().await? {
            if native.iter().any(|entry| entry.id == project.id) {
                continue;
            }
            project
                .roots
                .retain(|root| !native.iter().any(|entry| entry.roots.contains(root)));
            if !project.roots.is_empty() {
                native.push(project);
            }
        }
        let mut snapshot = state::Snapshot {
            chat_directory: match tokio::fs::canonicalize(self.chat_directory()).await {
                Ok(path) => Some(dunce::simplified(&path).to_owned()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(error) => return Err(error.into()),
            },
            ..Default::default()
        };
        for mut project in native {
            let native_group = project.roots.is_empty();
            for root in std::mem::take(&mut project.roots) {
                if Path::new(&root.path).is_absolute() {
                    match tokio::fs::canonicalize(&root.path).await {
                        Ok(path) => {
                            let path = dunce::simplified(&path);
                            if snapshot.chat_directory.as_deref() == Some(path) {
                                continue;
                            }
                            snapshot
                                .resolved_roots
                                .insert(root.path.clone(), path.to_owned());
                        }
                        Err(error)
                            if matches!(
                                error.kind(),
                                io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied
                            ) => {}
                        Err(error) => return Err(error.into()),
                    }
                }
                project.roots.push(root);
            }
            if native_group || !project.roots.is_empty() {
                snapshot.projects.push(project);
            }
        }
        snapshot.worktree_roots = crate::worktrees::workspace_roots(&self.path).await?;
        Ok(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_merge_preserves_project_priority_and_native_groups() {
        let directory = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(directory.path()).unwrap();
        let project = |id: &str, paths: &[&str]| Project {
            id: id.into(),
            roots: paths
                .iter()
                .map(|path| agent_protocol::models::ProjectRoot {
                    path: root.join(path).to_string_lossy().into_owned(),
                })
                .collect(),
            ..Default::default()
        };
        // Exact missing paths must also deduplicate; resolving them fails.
        let primary = project("primary", &["missing"]);
        let mixed = project("mixed", &["missing", "other"]);
        let group = project("native-group", &[]);
        assert_eq!(
            merge_catalogs(
                vec![primary.clone()],
                vec![
                    project("duplicate", &["missing"]),
                    mixed,
                    project("later-duplicate", &["other"]),
                    group.clone(),
                ],
            ),
            vec![primary, project("mixed", &["other"]), group],
        );
    }

    #[cfg(unix)]
    #[test]
    fn catalog_merge_compares_resolved_roots_and_keeps_distinct_native_ids() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let workspace = root.join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let alias = root.join("alias");
        std::os::unix::fs::symlink(&workspace, &alias).unwrap();
        let project = |id: &str, path: &Path| Project {
            id: id.into(),
            roots: vec![agent_protocol::models::ProjectRoot {
                path: path.to_string_lossy().into_owned(),
            }],
            ..Default::default()
        };
        let primary = vec![project("one", &workspace), project("two", &workspace)];
        assert_eq!(
            merge_catalogs(primary.clone(), vec![project("alias", &alias)]),
            primary,
        );
        assert_eq!(
            merge_catalogs(
                Vec::new(),
                vec![project("one", &workspace), project("alias", &alias)]
            ),
            vec![project("one", &workspace)],
        );
    }

    #[tokio::test]
    async fn standalone_chat_directory_does_not_become_a_project() {
        let directory = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(directory.path()).unwrap();
        let store = ProjectStore::new(root.join("bex-worktrees.json"));
        let chats = store.chat_directory();
        std::fs::create_dir(&chats).unwrap();
        store.register(&chats).await.unwrap();
        let workspace = root.join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let projects = serde_json::from_value(serde_json::json!([
            {"id":"chats","name":"Chats","roots":[{"path":chats}]},
            {"id":"mixed","name":"Workspace","roots":[{"path":chats},{"path":workspace}]},
            {"id":"virtual","name":"Native group","roots":[]}
        ]))
        .unwrap();
        let snapshot = store.load_with_projects(projects).await.unwrap();
        assert_eq!(snapshot.projects.len(), 2);
        assert_eq!(snapshot.projects[0].id, "mixed");
        assert_eq!(snapshot.projects[0].roots.len(), 1);
        assert_eq!(Path::new(&snapshot.projects[0].roots[0].path), workspace);
        assert_eq!(snapshot.projects[1].id, "virtual");
        assert_eq!(
            snapshot.project_for_workspace(chats.to_str().unwrap()),
            None
        );
        assert!(
            !snapshot
                .resolved_roots
                .contains_key(chats.to_str().unwrap())
        );
        assert!(store.load().await.unwrap().projects.is_empty());
    }
    #[tokio::test]
    async fn native_projects_refresh_and_take_precedence_over_duplicate_bex_roots() {
        let directory = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(directory.path()).unwrap();
        let project = root.join("project");
        std::fs::create_dir(&project).unwrap();
        let store = ProjectStore::new(root.join("bex-worktrees.json"));
        store.register(&project).await.unwrap();
        let native = Project {
            id: "native-project".into(),
            name: "Codex project name".into(),
            roots: vec![agent_protocol::models::ProjectRoot {
                path: project.to_str().unwrap().into(),
            }],
            ..Default::default()
        };
        let snapshot = store
            .load_with_projects(vec![native.clone()])
            .await
            .unwrap();
        assert_eq!(snapshot.projects, [native]);
        assert_eq!(
            snapshot.project_for_workspace(project.to_str().unwrap()),
            Some("native-project")
        );
        assert_eq!(store.load().await.unwrap().projects.len(), 1);
        assert!(!root.join(".codex-global-state.json").exists());
    }
    #[tokio::test]
    async fn registration_is_durable_idempotent_and_refreshes_worktree_roots() {
        let directory = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(directory.path()).unwrap();
        let project = root.join("project");
        std::fs::create_dir(&project).unwrap();
        let store = ProjectStore::new(root.join("bex-worktrees.json"));
        assert!(store.load().await.unwrap().projects.is_empty());
        store.register(&project).await.unwrap();
        store.register(&project).await.unwrap();
        let reopened = ProjectStore::new(store.path());
        let snapshot = reopened.load().await.unwrap();
        assert_eq!(snapshot.projects.len(), 1);
        assert_eq!(
            snapshot.project_for_workspace(project.to_str().unwrap()),
            Some(snapshot.projects[0].id.as_str())
        );
        let id = snapshot.projects[0].id.clone();
        let checkout = root.join("checkout");
        std::fs::write(
            store.path(),
            serde_json::to_vec(
                &serde_json::json!({"workspaceRoots":{checkout.to_str().unwrap():project}}),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            reopened
                .load()
                .await
                .unwrap()
                .project_for_workspace(checkout.to_str().unwrap()),
            Some(id.as_str())
        );
        std::fs::remove_file(store.path()).unwrap();
        assert_eq!(
            reopened
                .load()
                .await
                .unwrap()
                .project_for_workspace(checkout.to_str().unwrap()),
            None
        );
    }
}
