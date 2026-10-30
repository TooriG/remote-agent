//! Runs outside the app and Host so neither process owns its own replacement.
use agent_local_host::{
    LocalHostRegistry, LocalHostState,
    update::{ContinuationDelivery, RECOVERY_FILE, Recovery},
};
use agent_protocol::operations::{HostUpdate, PrepareHostUpdate};
use agent_transport::transport::{Endpoint, Relays};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub const RESULT_FILE: &str = "app-update-result.json";
pub const READY_FILE: &str = "app-update-ready.json";

#[derive(Clone, Serialize, Deserialize)]
pub struct Plan {
    pub build: u64,
    pub payload: PathBuf,
    pub destination: PathBuf,
    pub backup: PathBuf,
    pub executable: PathBuf,
    pub directory: PathBuf,
    pub isolated: bool,
}

#[derive(Serialize, Deserialize)]
pub struct UpdateResult {
    pub build: u64,
    pub error: Option<String>,
}

pub fn cleanup_workers(directory: &Path) {
    let Ok(lock) = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(directory.join("app-update.lock"))
    else {
        return;
    };
    if lock.try_lock().is_err() {
        return;
    }
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with("bex-updater-")
            && entry.file_type().is_ok_and(|kind| kind.is_dir())
        {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

fn run(command: &mut Command) -> Result<()> {
    ensure!(
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?
            .success(),
        "更新ファイルを準備できませんでした。"
    );
    Ok(())
}

pub fn stage(
    download: &agent_core::updates::Download,
    archive: &Path,
    directory: PathBuf,
    isolated: bool,
) -> Result<Plan> {
    let executable = std::env::current_exe()?;
    let destination = if cfg!(target_os = "macos") {
        ensure!(
            executable
                .parent()
                .is_some_and(|p| p.ends_with("Contents/MacOS")),
            "ApplicationsにインストールしたBexから更新してください。"
        );
        executable
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_owned()
    } else {
        executable
            .parent()
            .context("application directory unavailable")?
            .to_owned()
    };
    let parent = destination
        .parent()
        .context("application parent unavailable")?;
    // Stage beside the destination. This also checks write permission before
    // quitting and keeps the final rename on one filesystem.
    let staging = tempfile::Builder::new()
        .prefix(".bex-update-")
        .tempdir_in(parent)
        .context(
            "アプリの保存先に書き込めません。書き込み可能な場所へBexをインストールしてください。",
        )?;
    let payload = if cfg!(target_os = "macos") {
        run(Command::new("/usr/bin/ditto")
            .args(["-x", "-k"])
            .arg(archive)
            .arg(staging.path()))?;
        let bundle = staging.path().join("bex.app");
        run(Command::new("/usr/bin/codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(&bundle))?;
        run(Command::new("/usr/sbin/spctl")
            .args(["--assess", "--type", "execute"])
            .arg(&bundle))?;
        let team = |path: &Path| -> Result<String> {
            let output = Command::new("/usr/bin/codesign")
                .args(["-d", "--verbose=4"])
                .arg(path)
                .output()?;
            ensure!(output.status.success(), "アプリの署名を確認できません。");
            let text = String::from_utf8(output.stderr)?;
            text.lines()
                .find_map(|line| line.strip_prefix("TeamIdentifier="))
                .filter(|team| *team != "not set")
                .map(str::to_owned)
                .context("署名の開発元を確認できません。")
        };
        ensure!(
            team(&bundle)? == team(&destination)?,
            "更新ファイルの開発元が一致しません。"
        );
        let output = Command::new("/usr/libexec/PlistBuddy")
            .args(["-c", "Print :CFBundleIdentifier"])
            .arg(bundle.join("Contents/Info.plist"))
            .output()?;
        ensure!(
            output.status.success()
                && String::from_utf8(output.stdout)?.trim() == "app.bex.desktop",
            "更新ファイルがBexではありません。"
        );
        verify_build(&bundle.join("Contents/MacOS/bex"), download.build)?;
        bundle
    } else {
        let payload = staging.path().join(&download.file_name);
        fs::copy(archive, &payload)?;
        payload
    };
    let backup = staging.path().join("previous");
    let plan = Plan {
        build: download.build,
        payload,
        destination,
        backup,
        executable,
        directory,
        isolated,
    };
    agent_local_host::save_private_json(&plan.directory.join(READY_FILE), &plan)?;
    let _ = staging.keep();
    Ok(plan)
}

fn verify_build(executable: &Path, build: u64) -> Result<()> {
    let output = Command::new(executable)
        .arg("--bex-build-number")
        .stdin(Stdio::null())
        .output()?;
    ensure!(
        output.status.success() && String::from_utf8(output.stdout)?.trim() == build.to_string(),
        "更新後のバージョンを確認できません。更新は適用されませんでした。"
    );
    Ok(())
}

/// The caller retains the file until GPUI finishes saving/closing every window.
pub fn launch(plan: &Plan) -> Result<File> {
    let cache = tempfile::Builder::new()
        .prefix("bex-updater-")
        .tempdir_in(&plan.directory)?;
    let path = cache.path().join("plan.json");
    agent_local_host::save_private_json(&path, plan)?;
    let lease = File::create(cache.path().join("app.lock"))?;
    lease.lock()?;
    let worker = cache
        .path()
        .join(format!("updater{}", std::env::consts::EXE_SUFFIX));
    fs::copy(std::env::current_exe()?, &worker)?;
    let mut command = Command::new(worker);
    command.arg("--bex-apply-update").arg(&path);
    detached(&mut command);
    command.spawn()?;
    let deadline = Instant::now() + Duration::from_secs(10);
    while !cache.path().join("ready").is_file() {
        ensure!(
            Instant::now() < deadline,
            "更新用プロセスを起動できません。もう一度お試しください。"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    fs::write(cache.path().join("apply"), b"apply")?;
    let _ = cache.keep();
    Ok(lease)
}

fn detached(command: &mut Command) {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000 | 0x00000200);
    }
}

pub async fn apply(path: &Path) -> Result<()> {
    let plan: Plan = serde_json::from_slice(&fs::read(path)?)?;
    let job = path.parent().context("updater directory unavailable")?;
    let update_lock = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(plan.directory.join("app-update.lock"))?;
    update_lock.try_lock().context("別の更新が実行中です。")?;
    let lease = File::options()
        .read(true)
        .write(true)
        .open(job.join("app.lock"))?;
    fs::write(job.join("ready"), b"ready")?;
    let deadline = Instant::now() + Duration::from_secs(60);
    while lease.try_lock().is_err() {
        ensure!(
            Instant::now() < deadline,
            "アプリを終了できませんでした。更新は適用されませんでした。"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    ensure!(job.join("apply").is_file(), "更新は開始されませんでした。");
    let registry = if plan.isolated {
        LocalHostRegistry::new(plan.directory.clone())
    } else {
        LocalHostRegistry::for_user()?
    };
    let mut result = apply_to_host(&plan, &registry, job).await;
    // Always restore the Host, including after a failed replacement. Its exact
    // launch configuration was recorded before stopping it.
    let restart = async {
        if let Ok(bytes) = fs::read(job.join("host.json")) {
            let host = registry.resolve(&plan.directory)?;
            if matches!(host.state, LocalHostState::Stopped) {
                let launch: HostUpdate = serde_json::from_slice(&bytes)?;
                let program = if verify_build(&plan.executable, plan.build).is_ok() {
                    plan.executable
                        .with_file_name(format!("host-daemon{}", std::env::consts::EXE_SUFFIX))
                } else {
                    PathBuf::from(launch.executable)
                };
                let mut command = Command::new(program);
                command.args(launch.arguments);
                detached(&mut command);
                command.spawn()?;
            }
            wait_for_host(&registry, &host.directory, true).await?;
            wait_for_recovery(&host.directory).await?;
        }
        Ok::<_, anyhow::Error>(())
    }
    .await;
    if let Err(error) = restart {
        result = Err(error.context("Hostの復旧を確認できませんでした。"));
    }
    let _ = fs::remove_file(plan.directory.join(READY_FILE));
    agent_local_host::save_private_json(
        &plan.directory.join(RESULT_FILE),
        &UpdateResult {
            build: plan.build,
            error: result.as_ref().err().map(|e| format!("{e:#}")),
        },
    )?;
    let mut command = Command::new(&plan.executable);
    detached(&mut command);
    command.spawn()?;
    if result.is_ok() {
        if plan.backup.exists() {
            fs::remove_dir_all(&plan.backup)?;
        }
        if let Some(staging) = plan.backup.parent() {
            fs::remove_dir_all(staging)?;
        }
    }
    result
}

async fn apply_to_host(plan: &Plan, registry: &LocalHostRegistry, job: &Path) -> Result<()> {
    let host = registry.resolve(&plan.directory)?;
    match host.state {
        LocalHostState::Ready(ticket) => {
            let endpoint = Endpoint::bind(
                agent_local_host::load_local_identity(&host.directory)?,
                Relays::Loopback,
            )
            .await?;
            let connection = endpoint.connect(&ticket).await?;
            let (peer, _events) = connection.open_peer(Duration::from_secs(30), 8).await?;
            let prepared = peer.call(&PrepareHostUpdate {}).await?;
            agent_local_host::save_private_json(&job.join("host.json"), &prepared)?;
            peer.close().await;
            connection.close();
            endpoint.close().await;
            wait_for_host(registry, &host.directory, false).await?;
        }
        LocalHostState::Starting => bail!("Hostが起動中です。起動後にもう一度更新してください。"),
        LocalHostState::Stopped => {}
    }
    replace_and_verify(plan)
}

fn replace_and_verify(plan: &Plan) -> Result<()> {
    let result = (|| {
        fs::rename(&plan.destination, &plan.backup)?;
        if cfg!(target_os = "macos") {
            fs::rename(&plan.payload, &plan.destination)?;
        } else {
            run(Command::new(&plan.payload)
                .args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART"])
                .arg(format!("/DIR={}", plan.destination.display())))?;
        }
        verify_build(&plan.executable, plan.build)
    })();
    if result.is_err() && plan.backup.exists() {
        if plan.destination.exists() {
            fs::remove_dir_all(&plan.destination)?;
        }
        fs::rename(&plan.backup, &plan.destination)?;
    }
    result
}

async fn wait_for_host(registry: &LocalHostRegistry, directory: &Path, ready: bool) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let host = registry.resolve(directory)?;
        if if ready {
            matches!(host.state, LocalHostState::Ready(_))
        } else {
            matches!(host.state, LocalHostState::Stopped)
        } {
            return Ok(());
        }
        ensure!(
            Instant::now() < deadline,
            "Hostの再起動を確認できませんでした。"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn wait_for_recovery(directory: &Path) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let recovery: Recovery = serde_json::from_slice(&fs::read(directory.join(RECOVERY_FILE))?)?;
        if recovery.tasks.iter().all(|task| {
            matches!(
                task.delivery,
                ContinuationDelivery::Accepted(_) | ContinuationDelivery::Completed
            )
        }) {
            return Ok(());
        }
        ensure!(
            !recovery
                .tasks
                .iter()
                .any(|task| matches!(task.delivery, ContinuationDelivery::Failed(_))),
            "更新は適用されましたが、一部の会話を再開できませんでした。会話を開いて確認してください。"
        );
        ensure!(
            Instant::now() < deadline,
            "中断した会話の再開を確認できませんでした。会話を開いて確認してください。"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(directory: &Path) -> Plan {
        let destination = directory.join("installed");
        fs::create_dir(&destination).unwrap();
        fs::write(destination.join("previous-version"), b"old app").unwrap();
        Plan {
            build: 95,
            payload: directory.join("payload"),
            backup: directory.join("backup"),
            executable: destination.join("app"),
            destination,
            directory: directory.to_owned(),
            isolated: true,
        }
    }

    #[test]
    fn a_failed_replacement_restores_the_previous_app() {
        let directory = tempfile::tempdir().unwrap();
        let plan = plan(directory.path());
        assert!(replace_and_verify(&plan).is_err());
        assert_eq!(
            fs::read(plan.destination.join("previous-version")).unwrap(),
            b"old app"
        );
        assert!(!plan.backup.exists());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_wrong_installed_version_rolls_back_and_a_verified_version_keeps_its_backup() {
        use std::os::unix::fs::PermissionsExt;
        for (version, success) in [(94, false), (95, true)] {
            let directory = tempfile::tempdir().unwrap();
            let plan = plan(directory.path());
            fs::create_dir(&plan.payload).unwrap();
            let app = plan.payload.join("app");
            fs::write(&app, format!("#!/bin/sh\nprintf '%s\\n' {version}\n")).unwrap();
            fs::set_permissions(app, fs::Permissions::from_mode(0o700)).unwrap();
            assert_eq!(replace_and_verify(&plan).is_ok(), success);
            assert_eq!(plan.backup.exists(), success);
            assert_eq!(plan.destination.join("previous-version").exists(), !success);
        }
    }
}
