use super::{EditorId, LaunchStyle, Platform, launch_style};
use base64::Engine;
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

pub(super) fn editor_args(
    editor: EditorId,
    path: &Path,
    line: Option<u32>,
    column: Option<u32>,
    base: &[String],
) -> Result<Vec<String>, String> {
    let path = path.to_str().ok_or("Editor path is not UTF-8")?;
    let mut arguments = base.to_vec();
    match (launch_style(editor), line) {
        (LaunchStyle::LineColumn, Some(line)) => {
            arguments.extend(["--line".into(), line.to_string()]);
            if let Some(column) = column {
                arguments.extend(["--column".into(), column.saturating_sub(1).to_string()]);
            }
            arguments.push(path.into());
        }
        (style, Some(line)) => {
            if style == LaunchStyle::Goto {
                arguments.push("--goto".into());
            }
            arguments.push(format!(
                "{path}:{line}{}",
                column
                    .map(|column| format!(":{column}"))
                    .unwrap_or_default()
            ));
        }
        (_, None) => arguments.push(path.into()),
    }
    Ok(arguments)
}

pub(super) async fn spawn(command: &Path, arguments: &[String]) -> Result<(), String> {
    if cfg!(target_os = "windows")
        && command.extension().is_some_and(|extension| {
            extension.as_encoded_bytes().eq_ignore_ascii_case(b"cmd")
                || extension.as_encoded_bytes().eq_ignore_ascii_case(b"bat")
        })
        && arguments.iter().any(|argument| {
            argument
                .chars()
                .any(|char| char <= '\u{1f}' || char == '\u{7f}' || char == '"')
        })
    {
        return Err(
            "This editor launcher cannot open paths containing line breaks or double quotes".into(),
        );
    }
    tokio::process::Command::new(command)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(false)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Unable to launch {}: {error}", command.display()))
}

pub(super) fn powershell_path() -> PathBuf {
    PathBuf::from(
        std::env::var_os("SYSTEMROOT")
            .or_else(|| std::env::var_os("windir"))
            .unwrap_or_else(|| r"C:\Windows".into()),
    )
    .join("System32/WindowsPowerShell/v1.0/powershell.exe")
}

fn literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn powershell_args(source: &str) -> Vec<String> {
    let bytes = source
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    vec![
        "-NoProfile".into(),
        "-NonInteractive".into(),
        "-ExecutionPolicy".into(),
        "Bypass".into(),
        "-EncodedCommand".into(),
        base64::engine::general_purpose::STANDARD.encode(bytes),
    ]
}

fn reveal_source(target: &str, parent: &str) -> String {
    format!(
        "$ProgressPreference = 'SilentlyContinue'; $folderVerb = (Get-ItemProperty -LiteralPath 'Registry::HKEY_CLASSES_ROOT\\Directory\\shell' -ErrorAction SilentlyContinue).'(default)'; if ($folderVerb -and $folderVerb -ne 'none') {{ Invoke-Item -LiteralPath {} }} else {{ Start-Process 'explorer.exe' -ArgumentList ('/select,\"' + {} + '\"') }}",
        literal(parent),
        literal(target)
    )
}

fn wsl_path(target: &str, distro: &str) -> String {
    let relative = target.trim_start_matches('/').replace('/', "\\");
    format!(
        "\\\\wsl.localhost\\{distro}{}",
        if relative.is_empty() {
            String::new()
        } else {
            format!("\\{relative}")
        }
    )
}

pub(super) async fn file_manager(
    path: &Path,
    reveal: bool,
    platform: Platform,
    command: Option<&Path>,
) -> Result<(), String> {
    let command = command.ok_or("File manager is unavailable")?;
    let target = path.to_str().ok_or("File path is not UTF-8")?;
    let parent = path.parent().unwrap_or(path).to_string_lossy();
    match platform {
        Platform::Mac => {
            spawn(
                command,
                &if reveal {
                    vec!["-R".into(), target.into()]
                } else {
                    vec![target.into()]
                },
            )
            .await
        }
        Platform::Windows => {
            let target = target.replace('/', "\\");
            if reveal {
                spawn(
                    &powershell_path(),
                    &powershell_args(&reveal_source(&target, &parent.replace('/', "\\"))),
                )
                .await
            } else {
                spawn(command, &[target]).await
            }
        }
        Platform::Linux
            if command
                .file_name()
                .is_some_and(|name| name == "explorer.exe") =>
        {
            let distro =
                std::env::var("WSL_DISTRO_NAME").map_err(|_| "WSL distribution is unavailable")?;
            let target = wsl_path(target, &distro);
            if !reveal {
                return spawn(command, &[target]).await;
            }
            let parent = wsl_path(&parent, &distro);
            let source = if target.contains('"') {
                format!("Invoke-Item -LiteralPath {}", literal(&parent))
            } else {
                reveal_source(&target, &parent)
            };
            match spawn(Path::new("powershell.exe"), &powershell_args(&source)).await {
                Ok(()) => Ok(()),
                Err(_) => spawn(command, &[parent]).await,
            }
        }
        Platform::Linux => {
            if reveal && reveal_linux(path).await {
                return Ok(());
            }
            spawn(
                command,
                &[if reveal {
                    parent.into_owned()
                } else {
                    target.into()
                }],
            )
            .await
        }
    }
}

async fn reveal_linux(path: &Path) -> bool {
    let Ok(uri) = url::Url::from_file_path(path) else {
        return false;
    };
    let mut command = tokio::process::Command::new("gdbus");
    command
        .args([
            "call",
            "--session",
            "--dest",
            "org.freedesktop.FileManager1",
            "--object-path",
            "/org/freedesktop/FileManager1",
            "--method",
            "org.freedesktop.FileManager1.ShowItems",
            &format!("[\"{uri}\"]"),
            "",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    matches!(tokio::time::timeout(Duration::from_secs(2), command.status()).await, Ok(Ok(status)) if status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn powershell_source_preserves_quotes_and_selects_the_original_path() {
        let source = reveal_source(r"C:\資料\a'b.txt", r"C:\資料");
        assert!(source.contains("'C:\\資料\\a''b.txt'"));
        assert!(source.contains("Invoke-Item -LiteralPath 'C:\\資料'"));
        let args = powershell_args(&source);
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(args.last().unwrap())
            .unwrap();
        let units = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        assert_eq!(String::from_utf16(&units).unwrap(), source);
    }

    #[test]
    fn wsl_reveal_uses_the_distribution_share() {
        assert_eq!(
            wsl_path("/home/alice/資料.rs", "Ubuntu"),
            r"\\wsl.localhost\Ubuntu\home\alice\資料.rs"
        );
        assert_eq!(wsl_path("/", "Ubuntu"), r"\\wsl.localhost\Ubuntu");
    }
}
