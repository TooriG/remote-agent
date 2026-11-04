use anyhow::{Context as _, Result, anyhow};
use std::{
    path::Path,
    process::{Command, Output},
};

pub(crate) fn text(cwd: &Path, args: &[&str]) -> Result<String> {
    let output = output(cwd, args)?;
    String::from_utf8(output.stdout).context("git returned non-UTF-8 output")
}

pub(crate) fn output(cwd: &Path, args: &[&str]) -> Result<Output> {
    let output = Command::new("git")
        // Background status reads must not compete with checkout writes.
        .arg("--no-optional-locks")
        .args(args)
        .current_dir(cwd)
        .output()
        .context("failed to run git")?;
    if output.status.success() {
        return Ok(output);
    }
    Err(anyhow!(failure(&output)))
}

/// Bound commands that can wait on the network or a busy filesystem; the
/// timed-out process is killed. Callers run on a blocking worker.
pub(crate) fn bounded(cwd: &Path, args: &[&str], limit: std::time::Duration) -> Result<Output> {
    use std::io::Read as _;
    let mut child = Command::new("git")
        .arg("--no-optional-locks")
        .args(args)
        .current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("failed to run git")?;
    // Drain both pipes off-thread so a full pipe cannot stall the deadline.
    let drain = |pipe: Option<Box<dyn std::io::Read + Send>>| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut bytes);
            }
            bytes
        })
    };
    let stdout = drain(child.stdout.take().map(|pipe| Box::new(pipe) as _));
    let stderr = drain(child.stderr.take().map(|pipe| Box::new(pipe) as _));
    let deadline = std::time::Instant::now() + limit;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(anyhow!("git {} timed out", args.join(" ")));
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    let output = Output {
        status,
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    };
    if output.status.success() {
        return Ok(output);
    }
    Err(anyhow!(failure(&output)))
}

pub(crate) fn failure(output: &Output) -> String {
    let message = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if message.is_empty() {
        "git command failed".to_owned()
    } else {
        message
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        time::{Duration, SystemTime},
    };

    #[test]
    fn status_reads_preserve_the_index_while_observing_file_changes() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        text(root, &["init", "--quiet", "--initial-branch=main"]).unwrap();
        let path = root.join("tracked.txt");
        fs::write(&path, "tracked\n").unwrap();
        text(root, &["add", "tracked.txt"]).unwrap();
        text(
            root,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--quiet",
                "-m",
                "fixture",
            ],
        )
        .unwrap();
        let index_path = root.join(".git/index");
        let index = fs::read(&index_path).unwrap();
        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(
                fs::FileTimes::new().set_modified(SystemTime::now() + Duration::from_secs(60)),
            )
            .unwrap();
        assert!(
            text(root, &["status", "--porcelain=v1"])
                .unwrap()
                .is_empty()
        );
        assert_eq!(fs::read(&index_path).unwrap(), index);
        fs::write(&path, "changed\n").unwrap();
        assert_eq!(
            text(root, &["status", "--porcelain=v1"]).unwrap(),
            " M tracked.txt\n"
        );
        assert_eq!(fs::read(index_path).unwrap(), index);
    }
}
