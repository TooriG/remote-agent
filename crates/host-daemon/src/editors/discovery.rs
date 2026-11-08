use super::{
    EditorId, FileOpenCapabilities, LaunchStyle, LocatedEditor, Platform, base_args, commands,
    launch_style,
};
use agent_protocol::editors::FileManagerRevealKind;
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

pub(super) struct SearchPaths {
    pub(super) platform: Platform,
    path: Vec<PathBuf>,
    extensions: Vec<OsString>,
    home: Option<PathBuf>,
    /// The Mac's application folders.
    applications: Vec<PathBuf>,
    roots: Vec<PathBuf>,
    local_app_data: Option<PathBuf>,
    data_home: Option<PathBuf>,
    gui: bool,
    wsl: Option<String>,
}

impl SearchPaths {
    pub(super) fn read() -> Self {
        let variable = |name| std::env::var_os(name).filter(|value| !value.is_empty());
        let platform = Platform::current();
        let path = variable("PATH")
            .or_else(|| variable("Path"))
            .or_else(|| variable("path"));
        let wsl = (platform == Platform::Linux
            && std::env::var_os("SSH_CONNECTION").is_none()
            && std::env::var_os("SSH_TTY").is_none()
            && std::env::var_os("container").is_none())
        .then(|| variable("WSL_DISTRO_NAME"))
        .flatten()
        .and_then(|value| value.into_string().ok());
        let local_app_data = variable("LOCALAPPDATA").map(PathBuf::from);
        let mut roots = local_app_data
            .as_ref()
            .map(|root| root.join("Programs"))
            .into_iter()
            .collect::<Vec<_>>();
        roots.extend(
            ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"]
                .into_iter()
                .filter_map(variable)
                .map(PathBuf::from),
        );
        let home = variable("HOME").map(PathBuf::from);
        let search = Self {
            platform,
            path: path
                .map(|path| std::env::split_paths(&path).collect())
                .unwrap_or_default(),
            extensions: variable("PATHEXT")
                .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".into())
                .to_string_lossy()
                .split(';')
                .map(OsString::from)
                .collect(),
            applications: home
                .iter()
                .map(|home| home.join("Applications"))
                .chain([PathBuf::from("/Applications")])
                .collect(),
            home,
            roots,
            local_app_data,
            data_home: variable("XDG_DATA_HOME").map(PathBuf::from),
            gui: ["DISPLAY", "WAYLAND_DISPLAY"].into_iter().any(|name| {
                variable(name).is_some_and(|value| !value.to_string_lossy().trim().is_empty())
            }),
            wsl,
        };
        // A Host that tests its clients looks for editors in one directory, so
        // the machine's own editors stay out of its menus and launches.
        match variable("AGENT_EDITOR_DIRECTORY") {
            Some(directory) => search.only(PathBuf::from(directory)),
            None => search,
        }
    }

    /// Editors' commands, and on the Mac their applications, only in `directory`.
    fn only(self, directory: PathBuf) -> Self {
        Self {
            path: vec![directory.clone()],
            applications: vec![directory],
            home: None,
            roots: Vec::new(),
            local_app_data: None,
            data_home: None,
            ..self
        }
    }

    pub(super) fn command(&self, name: &str) -> Option<PathBuf> {
        let candidates = if Path::new(name).is_absolute() {
            vec![PathBuf::from(name)]
        } else {
            self.path.iter().map(|root| root.join(name)).collect()
        };
        candidates.into_iter().find_map(|path| {
            if self.platform == Platform::Windows {
                if executable(&path) {
                    return Some(path);
                }
                self.extensions
                    .iter()
                    .map(|extension| {
                        PathBuf::from(format!(
                            "{}{}",
                            path.display(),
                            extension.to_string_lossy().to_ascii_lowercase()
                        ))
                    })
                    .find(|candidate| executable(candidate))
            } else {
                executable(&path).then_some(path)
            }
        })
    }

    pub(super) fn locate(&self, editor: EditorId) -> Option<LocatedEditor> {
        if editor == EditorId::FileManager {
            return None;
        }
        let prefer_bundle = self.platform == Platform::Mac && editor == EditorId::Zed;
        let prefer_zeditor = self.platform == Platform::Linux && editor == EditorId::Zed;
        if !prefer_bundle && !prefer_zeditor {
            for command in commands(editor) {
                if let Some(command) = self.command(command) {
                    return Some(LocatedEditor {
                        command,
                        base_args: base_args(editor),
                    });
                }
            }
        }
        for command in self.candidates(editor) {
            if executable(&command) {
                let base_args = if editor == EditorId::Kiro && self.platform != Platform::Linux {
                    Vec::new()
                } else {
                    base_args(editor)
                };
                return Some(LocatedEditor { command, base_args });
            }
        }
        if prefer_bundle {
            for command in commands(editor) {
                if let Some(command) = self.command(command) {
                    return Some(LocatedEditor {
                        command,
                        base_args: base_args(editor),
                    });
                }
            }
        }
        None
    }

    fn candidates(&self, editor: EditorId) -> Vec<PathBuf> {
        match self.platform {
            Platform::Mac => self.mac_candidates(editor),
            Platform::Windows => self.windows_candidates(editor),
            Platform::Linux => self.linux_candidates(editor),
        }
    }

    fn mac_candidates(&self, editor: EditorId) -> Vec<PathBuf> {
        let mut candidates = Vec::new();
        for root in &self.applications {
            let mut bundles = install_names(editor)
                .into_iter()
                .map(|name| format!("{name}.app"))
                .collect::<Vec<_>>();
            for entry in directory_names(root) {
                if install_names(editor)
                    .iter()
                    .any(|name| versioned_bundle(&entry, name))
                    && !bundles.contains(&entry)
                {
                    bundles.push(entry);
                }
            }
            for bundle in bundles {
                let contents = root.join(bundle).join("Contents");
                if launch_style(editor) == LaunchStyle::LineColumn || editor == EditorId::Zed {
                    candidates.push(contents.join("MacOS").join(if editor == EditorId::Zed {
                        "cli"
                    } else {
                        commands(editor)[0]
                    }));
                } else {
                    candidates.push(contents.join("Resources/app/bin").join(commands(editor)[0]));
                    candidates.push(contents.join("Resources/app/bin/code"));
                }
            }
        }
        if launch_style(editor) == LaunchStyle::LineColumn
            && let Some(home) = &self.home
        {
            candidates.push(
                home.join("Library/Application Support/JetBrains/Toolbox/scripts")
                    .join(commands(editor)[0]),
            );
        }
        candidates
    }

    fn windows_candidates(&self, editor: EditorId) -> Vec<PathBuf> {
        let mut candidates = Vec::new();
        let command = commands(editor)[0];
        if launch_style(editor) == LaunchStyle::LineColumn {
            if let Some(root) = &self.local_app_data {
                candidates.push(
                    root.join("JetBrains/Toolbox/scripts")
                        .join(format!("{command}.cmd")),
                );
            }
            for root in self
                .roots
                .iter()
                .flat_map(|root| [root.clone(), root.join("JetBrains")])
            {
                for entry in directory_names(&root) {
                    if install_names(editor)
                        .iter()
                        .any(|name| entry == *name || entry.starts_with(&format!("{name} ")))
                    {
                        candidates.push(
                            root.join(&entry)
                                .join("bin")
                                .join(format!("{command}64.exe")),
                        );
                        candidates
                            .push(root.join(&entry).join("bin").join(format!("{command}.exe")));
                    }
                }
            }
        } else {
            let name = match editor {
                EditorId::VsCode => "Microsoft VS Code",
                EditorId::VsCodeInsiders => "Microsoft VS Code Insiders",
                _ => editor.label(),
            };
            for root in &self.roots {
                for directory in ["resources/app/bin", "bin"] {
                    candidates.push(
                        root.join(name)
                            .join(directory)
                            .join(format!("{command}.cmd")),
                    );
                    candidates.push(root.join(name).join(directory).join("code.cmd"));
                }
                if editor == EditorId::Zed {
                    candidates.push(root.join(name).join("bin/zed.exe"));
                    candidates.push(root.join(name).join("zed.exe"));
                }
            }
        }
        candidates
    }

    fn linux_candidates(&self, editor: EditorId) -> Vec<PathBuf> {
        let mut roots = self
            .home
            .as_ref()
            .map(|home| home.join(".local/bin"))
            .into_iter()
            .collect::<Vec<_>>();
        roots.extend(["/usr/local/bin", "/usr/bin", "/snap/bin"].map(PathBuf::from));
        if launch_style(editor) == LaunchStyle::LineColumn
            && let Some(data) = self
                .data_home
                .clone()
                .or_else(|| self.home.as_ref().map(|home| home.join(".local/share")))
        {
            roots.push(data.join("JetBrains/Toolbox/scripts"));
        }
        if editor == EditorId::Zed {
            ["zeditor", "zed"]
                .into_iter()
                .flat_map(|command| {
                    self.command(command)
                        .into_iter()
                        .chain(roots.iter().map(move |root| root.join(command)))
                })
                .collect()
        } else {
            roots
                .iter()
                .flat_map(|root| {
                    commands(editor)
                        .iter()
                        .map(move |command| root.join(command))
                })
                .collect()
        }
    }

    pub(super) fn file_manager(&self) -> Option<PathBuf> {
        let command = match self.platform {
            Platform::Mac => Some("open"),
            Platform::Windows => Some("explorer"),
            Platform::Linux if self.wsl.is_some() => Some("explorer.exe"),
            Platform::Linux if self.gui => Some("xdg-open"),
            _ => None,
        };
        command
            .and_then(|command| self.command(command))
            .or_else(|| {
                (self.platform == Platform::Linux && self.gui)
                    .then(|| self.command("xdg-open"))
                    .flatten()
            })
    }
}

fn executable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn directory_names(path: &Path) -> Vec<String> {
    std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect()
}

fn install_names(editor: EditorId) -> Vec<&'static str> {
    match editor {
        EditorId::VsCode => vec!["Visual Studio Code"],
        EditorId::VsCodeInsiders => vec!["Visual Studio Code - Insiders"],
        EditorId::Antigravity => vec!["Antigravity IDE"],
        EditorId::Idea => vec![
            "IntelliJ IDEA",
            "IntelliJ IDEA CE",
            "IntelliJ IDEA Ultimate",
        ],
        EditorId::Pycharm => vec!["PyCharm", "PyCharm CE"],
        EditorId::Rider => vec!["Rider", "JetBrains Rider"],
        _ => vec![editor.label()],
    }
}

fn versioned_bundle(entry: &str, name: &str) -> bool {
    entry
        .strip_prefix(&format!("{name} "))
        .and_then(|version| version.strip_suffix(".app"))
        .is_some_and(|version| {
            version.as_bytes().first().is_some_and(u8::is_ascii_digit)
                && version
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || byte == b'.')
        })
}

pub(super) async fn capabilities() -> Result<FileOpenCapabilities, String> {
    let search = SearchPaths::read();
    let (mut editors, manager, platform) = tokio::task::spawn_blocking(move || {
        let editors = EditorId::ALL
            .iter()
            .copied()
            .filter(|editor| search.locate(*editor).is_some())
            .collect::<Vec<_>>();
        (editors, search.file_manager(), search.platform)
    })
    .await
    .map_err(|error| error.to_string())?;
    let mut reveal = None;
    if let Some(manager) = manager {
        let usable =
            manager.file_name().is_none_or(|name| name != "xdg-open") || directory_handler().await;
        if usable {
            editors.push(EditorId::FileManager);
            reveal = match platform {
                Platform::Mac => Some(FileManagerRevealKind::Finder),
                Platform::Windows => executable(&super::launch::powershell_path())
                    .then_some(FileManagerRevealKind::FileExplorer),
                Platform::Linux
                    if manager
                        .file_name()
                        .is_some_and(|name| name == "explorer.exe") =>
                {
                    let search = SearchPaths::read();
                    if search.command("powershell.exe").is_some() {
                        Some(FileManagerRevealKind::FileExplorer)
                    } else if search.gui
                        && search.command("xdg-open").is_some()
                        && directory_handler().await
                    {
                        Some(FileManagerRevealKind::Files)
                    } else {
                        None
                    }
                }
                Platform::Linux => Some(FileManagerRevealKind::Files),
            };
        }
    }
    Ok(FileOpenCapabilities { editors, reveal })
}

async fn directory_handler() -> bool {
    let mut command = tokio::process::Command::new("xdg-mime");
    command
        .args(["query", "default", "inode/directory"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    matches!(tokio::time::timeout(Duration::from_secs(2), command.output()).await, Ok(Ok(output)) if output.status.success() && !output.stdout.iter().all(u8::is_ascii_whitespace))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn search(root: &Path, platform: Platform) -> SearchPaths {
        SearchPaths {
            platform,
            path: vec![root.join("bin")],
            extensions: vec![".EXE".into(), ".CMD".into()],
            home: Some(root.into()),
            applications: vec![root.join("Applications")],
            roots: vec![root.join("Programs")],
            local_app_data: Some(root.join("Local")),
            data_home: None,
            gui: false,
            wsl: None,
        }
    }

    fn launcher(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"fixture").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    #[test]
    fn stable_bundles_are_not_confused_with_other_editions() {
        assert!(versioned_bundle(
            "IntelliJ IDEA 2026.2.app",
            "IntelliJ IDEA"
        ));
        assert!(!versioned_bundle(
            "Visual Studio Code - Insiders.app",
            "Visual Studio Code"
        ));
        assert!(!versioned_bundle("IntelliJ IDEA CE.app", "IntelliJ IDEA"));
    }

    #[test]
    fn mac_bundle_discovery_prefers_zed_over_the_unrelated_path_command() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("bin/zed");
        let app = root.path().join("Applications/Zed.app/Contents/MacOS/cli");
        launcher(&path);
        launcher(&app);
        assert_eq!(
            search(root.path(), Platform::Mac)
                .locate(EditorId::Zed)
                .unwrap()
                .command,
            app
        );
    }

    #[test]
    fn a_host_confined_to_one_directory_finds_editors_only_there() {
        let root = tempfile::tempdir().unwrap();
        launcher(&root.path().join("bin/cursor"));
        launcher(&root.path().join("Applications/Zed.app/Contents/MacOS/cli"));
        let only = root.path().join("editors");
        launcher(&only.join("code"));
        launcher(&only.join("Kiro.app/Contents/Resources/app/bin/kiro"));
        let search = search(root.path(), Platform::Mac).only(only.clone());
        assert_eq!(
            search.locate(EditorId::VsCode).unwrap().command,
            only.join("code")
        );
        assert_eq!(
            search.locate(EditorId::Kiro).unwrap().command,
            only.join("Kiro.app/Contents/Resources/app/bin/kiro")
        );
        assert!(search.locate(EditorId::Cursor).is_none());
        assert!(search.locate(EditorId::Zed).is_none());
        assert_eq!(search.file_manager(), None);
    }

    #[test]
    fn kiro_bundles_omit_the_standalone_cli_subcommand() {
        let root = tempfile::tempdir().unwrap();
        let app = root
            .path()
            .join("Applications/Kiro.app/Contents/Resources/app/bin/kiro");
        launcher(&app);
        let located = search(root.path(), Platform::Mac)
            .locate(EditorId::Kiro)
            .unwrap();
        assert_eq!(located.command, app);
        assert!(located.base_args.is_empty());
    }

    #[test]
    fn windows_discovers_per_user_vscode_and_toolbox_scripts() {
        let root = tempfile::tempdir().unwrap();
        let code = root.path().join("Programs/Microsoft VS Code/bin/code.cmd");
        let idea = root.path().join("Local/JetBrains/Toolbox/scripts/idea.cmd");
        launcher(&code);
        launcher(&idea);
        let search = search(root.path(), Platform::Windows);
        assert_eq!(search.locate(EditorId::VsCode).unwrap().command, code);
        assert_eq!(search.locate(EditorId::Idea).unwrap().command, idea);
    }

    #[test]
    fn linux_prefers_zeditor_and_headless_sessions_have_no_file_manager() {
        let root = tempfile::tempdir().unwrap();
        let zed = root.path().join("bin/zed");
        let zeditor = root.path().join("bin/zeditor");
        launcher(&zed);
        launcher(&zeditor);
        let search = search(root.path(), Platform::Linux);
        assert_eq!(search.locate(EditorId::Zed).unwrap().command, zeditor);
        assert!(search.file_manager().is_none());
    }
}
