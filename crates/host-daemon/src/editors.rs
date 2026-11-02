//! The Host owns discovery and external processes. Client UI never launches a Host path.
mod discovery;
mod launch;

use agent_protocol::editors::{EditorId, FileOpenCapabilities, OpenInEditor};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, watch};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Platform {
    Mac,
    Windows,
    Linux,
}

impl Platform {
    fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Mac
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Linux
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LaunchStyle {
    Direct,
    Goto,
    LineColumn,
}

fn commands(editor: EditorId) -> &'static [&'static str] {
    use EditorId::*;
    match editor {
        Cursor => &["cursor"],
        Trae => &["trae"],
        Kiro => &["kiro"],
        VsCode => &["code"],
        VsCodeInsiders => &["code-insiders"],
        VsCodium => &["codium"],
        Zed => &["zed", "zeditor"],
        Antigravity => &["antigravity-ide", "agy-ide"],
        Idea => &["idea"],
        Aqua => &["aqua"],
        Clion => &["clion"],
        Datagrip => &["datagrip"],
        Dataspell => &["dataspell"],
        Goland => &["goland"],
        Phpstorm => &["phpstorm"],
        Pycharm => &["pycharm"],
        Rider => &["rider"],
        Rubymine => &["rubymine"],
        Rustrover => &["rustrover"],
        Webstorm => &["webstorm"],
        FileManager => &[],
    }
}

fn launch_style(editor: EditorId) -> LaunchStyle {
    use EditorId::*;
    match editor {
        Cursor | Trae | Kiro | VsCode | VsCodeInsiders | VsCodium | Antigravity => {
            LaunchStyle::Goto
        }
        Zed | FileManager => LaunchStyle::Direct,
        _ => LaunchStyle::LineColumn,
    }
}

#[derive(Clone, Debug)]
struct LocatedEditor {
    command: PathBuf,
    base_args: Vec<String>,
}

fn base_args(editor: EditorId) -> Vec<String> {
    match editor {
        EditorId::Cursor => vec!["--classic".into()],
        EditorId::Kiro => vec!["ide".into()],
        _ => Vec::new(),
    }
}

type Scan = Option<(Instant, Result<FileOpenCapabilities, String>)>;

fn scan_expired(value: &Scan, closed: bool, now: Instant) -> bool {
    value.as_ref().map_or(closed, |(completed, result)| {
        result.is_err() || now.saturating_duration_since(*completed) >= Duration::from_secs(60)
    })
}

#[derive(Default)]
pub(crate) struct Editors {
    cache: Mutex<Option<watch::Receiver<Scan>>>,
}

impl Editors {
    pub(crate) async fn capabilities(&self) -> Result<FileOpenCapabilities, String> {
        let mut cache = self.cache.lock().await;
        let expired = cache.as_ref().is_none_or(|scan| {
            scan_expired(&scan.borrow(), scan.has_changed().is_err(), Instant::now())
        });
        if expired {
            let (complete, scan) = watch::channel(None);
            *cache = Some(scan);
            // A cancelled caller cannot cancel the scan or leave the next caller waiting forever.
            tokio::spawn(async move {
                let result = discovery::capabilities().await;
                let _ = complete.send(Some((Instant::now(), result)));
            });
        }
        let mut scan = cache.as_ref().unwrap().clone();
        drop(cache);
        loop {
            if let Some((_, result)) = scan.borrow().as_ref() {
                return result.clone();
            }
            scan.changed()
                .await
                .map_err(|_| "Editor discovery stopped".to_owned())?;
        }
    }

    pub(crate) async fn open(&self, request: OpenInEditor) -> Result<(), String> {
        request.validate().map_err(str::to_owned)?;
        let path = crate::workspace_files::absolute_path(&request.path)
            .map_err(|error| error.to_string())?;
        let capabilities = self.capabilities().await?;
        if !capabilities.editors.contains(&request.editor) {
            return Err(format!(
                "{} is unavailable on this Host",
                request.editor.label()
            ));
        }
        if request.reveal && capabilities.reveal.is_none() {
            return Err("Reveal in file manager is unavailable on this Host".into());
        }
        let discovery = discovery::SearchPaths::read();
        let platform = discovery.platform;
        let file_manager = if request.reveal
            && capabilities.reveal == Some(agent_protocol::editors::FileManagerRevealKind::Files)
        {
            discovery.command("xdg-open")
        } else {
            discovery.file_manager()
        };
        if request.editor == EditorId::FileManager {
            let path = if request.reveal {
                path
            } else {
                // T3 passes a position-bearing editor target unchanged even to file-manager.
                PathBuf::from(
                    launch::editor_args(request.editor, &path, request.line, request.column, &[])?
                        .remove(0),
                )
            };
            return launch::file_manager(&path, request.reveal, platform, file_manager.as_deref())
                .await;
        }
        let editor = request.editor;
        let located = tokio::task::spawn_blocking(move || discovery.locate(editor))
            .await
            .map_err(|error| error.to_string())?;
        let located =
            located.ok_or_else(|| format!("{} command not found", request.editor.label()))?;
        let arguments = launch::editor_args(
            request.editor,
            &path,
            request.line,
            request.column,
            &located.base_args,
        )?;
        launch::spawn(&located.command, &arguments).await
    }
}

#[cfg(test)]
mod tests;
