use crate::{Connection, client::Client, media::Preview, native, workbench::Workbench};
use agent_core::{
    panels::{PanelKind, PanelScope},
    presentation::files::{
        FileTreeAction, FileTreeKey, FileTreeSurface, file_preview_mime_type, file_tree_path,
        mobile_file_icon, mobile_file_open_request,
    },
    state::Intent,
    store::Outcome,
};
use nsh_core::prelude::*;
use serde_json::json;
use std::sync::Arc;

#[derive(Clone, Copy)]
pub(crate) struct Browser {
    pub root: Memo<Option<String>>,
    pub file: Memo<Option<TextFile>>,
    workspace: Workbench,
    target: Memo<(PanelScope, (bool, Option<u64>))>,
    line: Signal<Option<(u64, Option<u32>)>>,
    preview: Preview,
    owner: Owner,
}

#[derive(Clone, PartialEq)]
pub(crate) struct TextFile {
    pub id: u64,
    pub path: String,
    pub line: Option<u32>,
}

impl Browser {
    pub fn close(self) {
        self.workspace.hide();
        self.preview.close();
    }

    pub fn show(self, path: String) {
        self.preview.close();
        self.workspace.open(PanelKind::Files, Some(path));
    }

    pub fn new(preview: Preview) -> Self {
        let workspace = use_context::<Workbench>().unwrap();
        let line = signal(None::<(u64, Option<u32>)>);
        let root = computed(move || {
            workspace.state.with(|state| {
                state
                    .surfaces
                    .iter()
                    .rev()
                    .find(|surface| surface.kind == PanelKind::Files)
                    .and_then(|surface| surface.path.clone())
            })
        });
        let file = computed(move || {
            workspace.state.with(|state| {
                let surface = state
                    .selected()
                    .filter(|surface| state.open && surface.kind == PanelKind::File)?;
                Some(TextFile {
                    id: surface.id,
                    path: surface.path.clone()?,
                    line: line
                        .get()
                        .filter(|(id, _)| *id == surface.id)
                        .and_then(|(_, line)| line),
                })
            })
        });
        let target = computed(move || {
            (
                workspace.scope.get(),
                workspace.state.with(|state| (state.open, state.active)),
            )
        });
        effect(move || {
            target.get();
            preview.close();
        });
        Self {
            root,
            file,
            workspace,
            target,
            line,
            preview,
            owner: Owner::current(),
        }
    }

    pub fn open(
        self,
        source: String,
        cwd: String,
        home: Option<String>,
        host: String,
        client: Arc<Client>,
    ) {
        let line = agent_core::presentation::markdown::markdown_file_target(
            source.clone(),
            cwd.clone(),
            agent_core::presentation::markdown::MarkdownFileSurface::Mobile,
        )
        .and_then(|target| target.line);
        let Some(request) = mobile_file_open_request(source.clone(), cwd, home) else {
            if !source.starts_with('#') {
                spawn(async move {
                    if !open_url(source).await {
                        native::message("リンク", "このリンクを開けませんでした。");
                    }
                });
            }
            return;
        };
        let token = self.preview.begin();
        let target = self.target.get_untracked();
        let resolve = client.request(&host, Intent::ResolveFileReference(request));
        self.owner.run(move || {
            spawn(async move {
                let result = resolve
                    .recv()
                    .await
                    .map_err(|reason| reason.to_string())
                    .and_then(|result| result.map_err(|reason| reason.to_string()));
                if !self.preview.is_pending(token) || self.target.get_untracked() != target {
                    return;
                }
                match result {
                    Ok(Outcome::FileReferenceResolved {
                        path,
                        directory: true,
                    }) => self.show(path),
                    Ok(Outcome::FileReferenceResolved {
                        path,
                        directory: false,
                    }) => self.open_file(path, line, host, client),
                    Err(reason) => {
                        self.preview.close();
                        native::message("ファイル", reason);
                    }
                    _ => {
                        self.preview.close();
                        native::message("ファイル", "ファイルの場所を確認できませんでした。");
                    }
                }
            })
        });
    }

    pub fn open_file(self, path: String, line: Option<u32>, host: String, client: Arc<Client>) {
        if file_preview_mime_type(path.clone()).is_some() {
            self.preview
                .load(crate::media::receive(client.download(host, path)));
        } else {
            self.preview.close();
            let id = self.workspace.open(PanelKind::File, Some(path.clone()));
            self.line.set(Some((id, line)));
            client.dispatch(
                &host,
                Intent::ReadFile(agent_core::state::operations::ReadFile {
                    path,
                    discard_draft: false,
                }),
            );
        }
    }
}

pub(crate) fn screen(id: u64) -> impl Element {
    let connection = use_context::<Connection>().unwrap();
    let browser = use_context::<Browser>().unwrap();
    let key = computed(move || FileTreeKey {
        owner: format!("nsh-files-{id}"),
        root: browser.root.get().unwrap_or_default(),
    });
    let mounted = stored(None::<(String, FileTreeKey)>);
    effect(move || {
        let host = connection.host.get();
        let key = key.get();
        let next = (!key.root.is_empty()).then(|| (host.clone(), key.clone()));
        if let Some((host, key)) = mounted.replace(next) {
            connection.client.with(|client| {
                client.dispatch(
                    &host,
                    Intent::FileTree {
                        key,
                        action: FileTreeAction::Close,
                    },
                )
            });
        }
        if key.root.is_empty() {
            return;
        }
        let workspace_root = connection
            .publication
            .with_untracked(|p| p.composer.controls.navigation.cwd.clone());
        connection.client.with(|client| {
            client.dispatch(
                &host,
                Intent::FileTree {
                    key,
                    action: FileTreeAction::Open {
                        surface: if is_desktop() {
                            FileTreeSurface::Desktop
                        } else {
                            FileTreeSurface::Mobile
                        },
                        locale: locale().as_str().into(),
                        workspace_root,
                    },
                },
            )
        });
    });
    cleanup(move || {
        if let Some((host, key)) = mounted.replace(None) {
            connection.client.with(|client| {
                client.dispatch(
                    &host,
                    Intent::FileTree {
                        key,
                        action: FileTreeAction::Close,
                    },
                )
            });
        }
    });
    let tree = computed(move || {
        connection
            .publication
            .with(|p| p.snapshot.files.file_tree(key.get()))
    });
    let send = move |action| {
        connection.dispatch(Intent::FileTree {
            key: key.get_untracked(),
            action,
        })
    };
    let opened = computed(move || tree.with(|tree| tree.open_file.clone()));
    let previous = stored(opened.get_untracked());
    effect(move || {
        let next = opened.get();
        if previous.replace(next.clone()) != next
            && let Some(request) = next
        {
            browser.open_file(
                file_tree_path(&key.get_untracked().root, &request.path),
                None,
                connection.host.get_untracked(),
                connection.client.get(),
            );
        }
    });
    view().flex_1().gap_2().p_2()
        .child(text(move || key.with(|key| key.root.clone())).selectable(true).truncate().text_size(px(12.0)).w_full())
        .child(view().flex_row().items_center().gap_2()
            .child(view().flex_1())
            .child(native::button("arrow.clockwise", "更新", move || send(FileTreeAction::Refresh)).w(px(44.0)).h(px(44.0)))
            .child(native::button("arrow.up.left.and.arrow.down.right", "すべてのフォルダーを展開", move || send(FileTreeAction::ToggleAll))
                .prop("symbol", move || json!(if tree.with(|tree| tree.all_expanded) { "arrow.down.right.and.arrow.up.left" } else { "arrow.up.left.and.arrow.down.right" }))
                .prop("label", move || json!(tree.with(|tree| tree.toggle_all_label.clone())))
                .prop("enabled", move || json!(tree.with(|tree| tree.can_expand_all))).w(px(44.0)).h(px(44.0))))
        .child(native("bex.search").prop("placeholder", json!("ファイルを検索")).prop("value", move || json!(tree.with(|tree| tree.query.clone()))).on_native_event(move |event: &NativeEvent| {
            if &*event.name == "change" && let Some(text) = event.data.as_str() { send(FileTreeAction::Query { text: text.into() }); }
        }).w_full())
        .child(text(move || tree.with(|tree| tree.error.as_ref().map(ToString::to_string).unwrap_or_default())))
        .child(when(move || tree.with(|tree| tree.rows.is_empty()), move || text(move || tree.with(|tree| {
            if tree.pending { "ファイルを読み込み中…" } else if tree.error.is_some() { "更新して再試行してください。" }
            else if tree.query.trim().is_empty() { "ファイルはありません" } else { "一致するファイルはありません" }
        }))))
        .child(when(move || tree.with(|tree| tree.truncated), || text("ほかにも一致する項目があります。検索語を絞り込んでください。")))
        .child(native("bex.list").prop("items", move || json!(tree.with(|tree| tree.rows.iter().map(|row| json!({
            "id": row.path, "title": row.label,
            "symbol": if row.kind == agent_protocol::file_tree::EntryKind::Directory { "folder" } else { "" },
            "asset": (row.kind != agent_protocol::file_tree::EntryKind::Directory).then(|| format!("file-icons/pierre_{}.png", mobile_file_icon(row.path.clone()))),
            "level": row.depth, "disclosure": row.kind == agent_protocol::file_tree::EntryKind::Directory, "group": false, "expanded": row.expanded,
        })).collect::<Vec<_>>()))).prop("selected", move || json!(tree.with(|tree| tree.selected_paths.first().cloned())))
        .prop("expansion_enabled", move || json!(tree.with(|tree| tree.can_change_expansion)))
        .on_native_event(move |event: &NativeEvent| {
            if &*event.name == "activate" && let Some(path) = event.data.as_str()
                && let Some(kind) = tree.with_untracked(|tree| tree.rows.iter().find(|row| row.path == path).map(|row| row.kind)) {
                if kind == agent_protocol::file_tree::EntryKind::Directory {
                    send(FileTreeAction::Toggle { path: path.into() });
                } else {
                    browser.open_file(file_tree_path(&key.get_untracked().root, path), None, connection.host.get_untracked(), connection.client.get());
                }
            }
            if &*event.name == "expand" && let Some(path) = event.data["id"].as_str()
                && let Some(expanded) = event.data["expanded"].as_bool() {
                send(FileTreeAction::SetExpanded { path: path.into(), expanded });
            }
            if &*event.name == "select" && let Some(path) = event.data.as_str()
                && tree.with_untracked(|tree| tree.rows.iter().any(|row| row.path == path)) {
                send(if is_desktop() { FileTreeAction::Select { path: path.into(), additive: false, range: false } }
                else { FileTreeAction::Click { path: path.into(), additive: false, range: false } });
            }
        }).flex_1())
}
