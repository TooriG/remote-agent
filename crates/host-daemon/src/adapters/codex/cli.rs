use agent_protocol::operations::{CodexCliInstallation, CodexCliStatus};
use codex_app_server::CodexAppServer;
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};

pub(super) struct Updater {
    program: PathBuf,
    home: Option<PathBuf>,
    state: Arc<tokio::sync::Mutex<State>>,
}

#[derive(Default)]
struct State {
    status: CodexCliStatus,
    update_error: Option<String>,
}

pub(super) fn active_installation(server: &CodexAppServer) -> CodexCliInstallation {
    CodexCliInstallation {
        executable: server.executable().to_string_lossy().into_owned(),
        version: server
            .initialize_response()
            .version()
            .map(|version| format!("codex-cli {version}")),
    }
}

impl Updater {
    pub(super) fn new(
        program: PathBuf,
        home: Option<PathBuf>,
        active: Option<CodexCliInstallation>,
    ) -> Self {
        Self {
            program,
            home,
            state: Arc::new(tokio::sync::Mutex::new(State {
                status: CodexCliStatus {
                    active,
                    ..Default::default()
                },
                ..Default::default()
            })),
        }
    }

    // Installation survives RPC deadlines and reconnects. Hold admission while
    // probing so another client cannot start a simultaneous update.
    pub(super) async fn request(&self, update: bool) -> CodexCliStatus {
        let mut state = self.state.lock().await;
        if state.status.running {
            return state.status.clone();
        }
        if !update {
            state.status = status(
                state.status.active.clone(),
                installation(&self.program, self.home.as_deref()).await,
                state.update_error.clone(),
            );
            return state.status.clone();
        }
        state.status.running = true;
        state.status.error = None;
        state.update_error = None;
        let result = state.status.clone();
        let state = self.state.clone();
        let program = self.program.clone();
        let home = self.home.clone();
        tokio::spawn(async move {
            let update_error = async {
                let executable = codex_app_server::resolve_executable(&program).map_err(|error| error.to_string())?;
                run(&executable, home.as_deref(), "update", Duration::from_secs(300)).await
            }.await.err().map(|error| {
                format!("Codex CLIの更新に失敗しました: {error}\nこのCLIが自動更新に対応していない場合は、インストール元の方法で更新してください。")
            });
            // An installer can repoint the launcher and then fail (for example
            // while updating shell settings). Probe even after a failed update.
            let installed = installation(&program, home.as_deref()).await;
            let mut state = state.lock().await;
            state.status = status(state.status.active.clone(), installed, update_error.clone());
            state.update_error = update_error;
        });
        result
    }
}

fn status(
    active: Option<CodexCliInstallation>,
    installed: Result<CodexCliInstallation, String>,
    update_error: Option<String>,
) -> CodexCliStatus {
    let (installed, error) = match installed {
        Ok(installed) => (Some(installed), update_error),
        Err(error) => (
            None,
            Some(match update_error {
                Some(update) => format!("{update}\n{error}"),
                None => error,
            }),
        ),
    };
    let restart_required = installed
        .as_ref()
        .is_some_and(|installed| active.as_ref() != Some(installed));
    CodexCliStatus {
        running: false,
        active,
        installed,
        restart_required,
        error,
    }
}

async fn installation(program: &Path, home: Option<&Path>) -> Result<CodexCliInstallation, String> {
    let executable =
        codex_app_server::resolve_executable(program).map_err(|error| error.to_string())?;
    let version = run(&executable, home, "--version", Duration::from_secs(10)).await?;
    if !version.starts_with("codex-cli ") {
        return Err("Codex CLI version could not be verified".into());
    }
    Ok(CodexCliInstallation {
        executable: executable.to_string_lossy().into_owned(),
        version: Some(version),
    })
}

async fn run(
    executable: &Path,
    home: Option<&Path>,
    argument: &str,
    timeout: Duration,
) -> Result<String, String> {
    let mut command = bex_process::command(executable).map_err(|error| error.to_string())?;
    if let Some(home) = home {
        command.env("CODEX_HOME", home);
    }
    command.arg(argument);
    collect(command, timeout).await
}

async fn collect(
    mut command: tokio::process::Command,
    timeout: Duration,
) -> Result<String, String> {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    // The supervisor's stdin is its lifetime pipe; keep it open while waiting.
    // Dropping it on timeout terminates the entire owned process tree.
    let lifetime = child.stdin.take();
    let output = tokio::time::timeout(timeout, child.wait_with_output()).await;
    drop(lifetime);
    let output = output
        .map_err(|_| "CLI command timed out".to_owned())?
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        let detail = if detail.trim().is_empty() {
            String::from_utf8_lossy(&output.stdout)
        } else {
            detail
        };
        return Err(format!(
            "{}: {}",
            output.status,
            detail.chars().take(2000).collect::<String>().trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(script: &str) -> tokio::process::Command {
        #[cfg(windows)]
        {
            let program = std::env::var_os("COMSPEC").unwrap();
            let mut command = bex_process::command(std::path::Path::new(&program)).unwrap();
            command.args(["/D", "/C", script]);
            command
        }
        #[cfg(unix)]
        {
            let mut command = bex_process::command(std::path::Path::new("/bin/sh")).unwrap();
            command.args(["-c", script]);
            command
        }
    }

    async fn wait_for_update(updater: &Updater) -> CodexCliStatus {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let status = updater.request(false).await;
                if !status.running {
                    break status;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("CLI update deadline")
    }

    #[tokio::test]
    async fn supervised_commands_retain_lifetime_report_errors_and_time_out() {
        assert_eq!(
            collect(shell("echo codex-cli fixture"), Duration::from_secs(10))
                .await
                .unwrap(),
            "codex-cli fixture"
        );
        #[cfg(windows)]
        let failed = "echo update-denied 1>&2 & exit /b 7";
        #[cfg(unix)]
        let failed = "echo update-denied >&2; exit 7";
        let error = collect(shell(failed), Duration::from_secs(10))
            .await
            .unwrap_err();
        assert!(error.contains("update-denied"), "{error}");
        #[cfg(windows)]
        let slow = {
            let system = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap());
            let mut command = bex_process::command(&system.join("System32/ping.exe")).unwrap();
            // GUI-launched Hosts may not inherit a PATH containing System32.
            // The timeout fixture must still start rather than fail immediately.
            command.env("PATH", "");
            command.args(["-n", "30", "127.0.0.1"]);
            command
        };
        #[cfg(unix)]
        let slow = shell("sleep 30");
        assert_eq!(
            collect(slow, Duration::from_millis(100)).await.unwrap_err(),
            "CLI command timed out"
        );
    }

    #[tokio::test]
    async fn missing_executable_finishes_with_error_and_can_be_retried() {
        let root = tempfile::tempdir().unwrap();
        let updater = Updater::new(root.path().join("missing-codex"), None, None);
        let (_, update) = tokio::join!(updater.request(false), updater.request(true));
        assert!(update.running);
        let finished = wait_for_update(&updater).await;
        assert!(
            finished
                .error
                .as_deref()
                .unwrap()
                .contains("Codex CLIの更新に失敗")
        );
        assert!(!finished.restart_required);
        assert!(finished.installed.is_none());
        assert!(updater.request(true).await.running);
    }

    #[tokio::test]
    async fn status_rechecks_the_executable_even_with_a_cached_version() {
        let root = tempfile::tempdir().unwrap();
        let updater = Updater::new(root.path().join("missing-codex"), None, None);
        updater.state.lock().await.status.installed = Some(CodexCliInstallation {
            executable: "cached".into(),
            version: Some("codex-cli cached".into()),
        });
        let status = updater.request(false).await;
        assert!(status.error.is_some(), "must probe the current executable");
        assert!(!status.running);
        assert!(!status.restart_required);
    }

    #[tokio::test]
    async fn concurrent_request_does_not_restart_running_update() {
        let updater = Updater::new(PathBuf::from("codex"), None, None);
        updater.state.lock().await.status.running = true;
        let (first, second) = tokio::join!(updater.request(true), updater.request(true));
        assert_eq!(first, second);
        assert!(first.running);
    }

    #[cfg(unix)]
    fn write_cli(path: &Path, version: &str, update: &str) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(
            path,
            format!(
                r#"#!/bin/sh
case "$1" in
  --version) printf '%s\n' 'codex-cli {version}' ;;
  update) {update} ;;
  *) exit 1 ;;
esac
"#
            ),
        )
        .unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn external_replacement_detects_path_and_version_changes_without_relabeling_the_running_cli()
     {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join("old");
        let new = root.path().join("new");
        let launcher = root.path().join("codex");
        write_cli(&old, "1.0.0", "exit 0");
        write_cli(&new, "1.0.0", "exit 0");
        symlink(&old, &launcher).unwrap();
        let active = installation(&launcher, None).await.unwrap();
        let updater = Updater::new(launcher.clone(), None, Some(active.clone()));
        let initial = updater.request(false).await;
        assert_eq!(initial.active, Some(active.clone()));
        assert!(!initial.restart_required);
        std::fs::remove_file(&launcher).unwrap();
        symlink(&new, &launcher).unwrap();
        let moved = updater.request(false).await;
        assert_eq!(moved.active, Some(active.clone()));
        assert_eq!(
            moved.installed.as_ref().unwrap().executable,
            new.canonicalize().unwrap().to_string_lossy()
        );
        assert!(
            moved.restart_required,
            "a different installation matters even at the same version"
        );
        std::fs::remove_file(&launcher).unwrap();
        symlink(&old, &launcher).unwrap();
        assert!(
            !updater.request(false).await.restart_required,
            "restoring the active installation clears the mismatch"
        );
        write_cli(&old, "2.0.0", "exit 0");
        let replaced = updater.request(false).await;
        assert_eq!(replaced.active, Some(active));
        assert_eq!(
            replaced.installed.unwrap().version.as_deref(),
            Some("codex-cli 2.0.0")
        );
        assert!(
            replaced.restart_required,
            "in-place replacements also need restart"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn failed_installer_still_reports_repointed_launcher_and_a_noop_update_needs_no_restart()
    {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join("old");
        let new = root.path().join("new");
        let launcher = root.path().join("old.launcher");
        write_cli(
            &old,
            "1.0.0",
            "ln -sfn \"$0.next\" \"$0.launcher\"; echo profile-denied >&2; exit 7",
        );
        write_cli(&new, "2.0.0", "exit 0");
        symlink(&old, &launcher).unwrap();
        symlink(&new, old.with_extension("next")).unwrap();
        let active = installation(&launcher, None).await.unwrap();
        let updater = Updater::new(launcher.clone(), None, Some(active.clone()));
        assert!(updater.request(true).await.running);
        let finished = wait_for_update(&updater).await;
        assert_eq!(finished.active, Some(active));
        assert_eq!(
            finished.installed.as_ref().unwrap().version.as_deref(),
            Some("codex-cli 2.0.0")
        );
        assert!(finished.restart_required);
        assert!(
            finished
                .error
                .as_deref()
                .unwrap()
                .contains("profile-denied")
        );
        assert_eq!(
            updater.request(false).await.error,
            finished.error,
            "polling retains the installer failure"
        );
        let restarted = Updater::new(launcher, None, finished.installed);
        assert!(restarted.request(true).await.running);
        let noop = wait_for_update(&restarted).await;
        assert!(
            !noop.restart_required,
            "an up-to-date installer does not change the running CLI"
        );
        assert!(noop.error.is_none());
    }

    proptest::proptest! {
        #[test]
        fn installed_version_drift_preserves_active_identity_and_install_errors(
            version in 1u32..10000, delta in 1u32..10000, failed in proptest::bool::ANY,
        ) {
            let active = CodexCliInstallation { executable: "/cli".into(), version: Some(format!("codex-cli {version}")) };
            let installed = CodexCliInstallation { executable: "/cli".into(), version: Some(format!("codex-cli {}", version + delta)) };
            let result = status(Some(active.clone()), Ok(installed.clone()), failed.then(|| "install failed".into()));
            proptest::prop_assert_eq!(result.active, Some(active));
            proptest::prop_assert_eq!(result.installed, Some(installed));
            proptest::prop_assert!(result.restart_required);
            proptest::prop_assert!(!result.running);
            proptest::prop_assert_eq!(result.error.as_deref(), failed.then_some("install failed"));
        }
    }
}
