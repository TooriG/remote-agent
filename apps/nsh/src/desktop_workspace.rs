//! Native workspace controls consume Core publications and keep Host operations in Core.
use crate::{Connection, native, workbench::Workbench};
use agent_core::{
    panels::PanelKind,
    state::{Intent, operations as op},
};
use agent_protocol::editors::EditorId;
use nsh_core::prelude::*;

pub(crate) fn card() -> impl IntoElement {
    let connection = use_context::<Connection>().unwrap();
    let workspace = use_context::<Workbench>().unwrap();
    let files = use_context::<crate::files::Browser>().unwrap();
    let target = computed(move || {
        connection.composer.with(|p| {
            (
                connection.host.get(),
                p.controls.connected,
                p.controls.navigation.cwd.clone(),
            )
        })
    });
    effect(move || {
        let (host, connected, cwd) = target.get();
        if connected {
            if !cwd.is_empty() {
                connection.dispatch(Intent::ReviewWorkspace(op::ReviewWorkspace { cwd }));
            }
            if host == "local"
                && !connection
                    .publication
                    .with_untracked(|p| p.snapshot.files.file_open_capabilities_requested)
            {
                connection.dispatch(Intent::ReadFileOpenCapabilities(
                    op::ReadFileOpenCapabilities {},
                ));
            }
        }
    });
    let card = native("bex.details")
        .prop("data", move || {
            connection
                .publication
                .with(|p| crate::details::workspace_data(p, connection.host.with(|h| h == "local")))
        })
        .on_native_event(move |event: &NativeEvent| {
            let (cwd, home) = connection
                .publication
                .with_untracked(|p| (p.composer.controls.navigation.cwd.clone(), p.context.home()));
            match event.data["action"].as_str() {
                Some("refresh") => {
                    connection.dispatch(Intent::ReviewWorkspace(op::ReviewWorkspace { cwd }))
                }
                Some("diff") => workspace.open(PanelKind::Diff, Some(cwd), None),
                Some("files") if !cwd.is_empty() => {
                    workspace.open(PanelKind::Files, Some(cwd), None)
                }
                Some("files") => {
                    let choice = pick_paths(nsh_core::file_dialog::OpenDialog::folders());
                    spawn(async move {
                        match choice.await {
                            Ok(Some(paths)) => {
                                if let Some(path) = paths.into_iter().next() {
                                    connection.dispatch(Intent::NewChat {
                                        cwd: path.to_string_lossy().into_owned(),
                                    });
                                }
                            }
                            Ok(None) => {}
                            Err(reason) => native::message("フォルダー", reason),
                        }
                    });
                }
                Some("attach") => crate::composer::pick_files(
                    connection.application.get(),
                    connection.host.get_untracked(),
                    connection.pane,
                    connection
                        .composer
                        .with_untracked(|p| p.controls.navigation.draft_key.clone()),
                ),
                Some("source") => {
                    if let Some(path) = event.data["path"].as_str() {
                        files.open(
                            path.into(),
                            cwd,
                            home,
                            connection.host.get_untracked(),
                            connection.client.get(),
                        );
                    }
                }
                Some("editor" | "preferredEditor")
                    if connection.host.with_untracked(|h| h == "local") =>
                {
                    if let Ok(editor) =
                        serde_json::from_value::<EditorId>(event.data["editor"].clone())
                    {
                        if event.data["action"] == "preferredEditor" {
                            connection.dispatch(Intent::SetPreferredEditor { editor });
                        } else if !cwd.is_empty() {
                            connection.dispatch(Intent::OpenInEditor(op::OpenInEditor {
                                path: cwd,
                                line: None,
                                column: None,
                                editor,
                                reveal: false,
                            }));
                        }
                    }
                }
                _ => {}
            }
        })
        .flex_1()
        .min_h_0();
    view().w(px(300.0)).h_full().p_4().pt(px(36.0)).child(card)
}
