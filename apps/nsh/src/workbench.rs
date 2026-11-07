use crate::{Connection, native, shared::Shared};
use agent_core::{
    panels::{PanelKind, PanelScope, PanelState, Panels},
    state::Intent,
};
use nsh_core::prelude::*;
use serde_json::json;

#[derive(Clone, Copy)]
pub(crate) struct Workbench {
    panels: Signal<Shared<Panels>>,
    pub scope: Memo<PanelScope>,
    pub state: Memo<PanelState>,
    instance: Stored<uuid::Uuid>,
}

impl Workbench {
    pub fn new() -> Self {
        let connection = use_context::<Connection>().unwrap();
        let panels = signal(Shared(Panels::new()));
        let scope = computed(move || {
            connection.publication.with(|p| {
                p.snapshot
                    .panel_scope(connection.host.get(), p.context.navigation().draft_key)
            })
        });
        let state = computed(move || panels.with(|panels| panels.get(scope.get())));
        Self {
            panels,
            scope,
            state,
            instance: stored(uuid::Uuid::new_v4()),
        }
    }

    pub fn rekey(self, before: PanelScope, after: PanelScope) {
        let next = self
            .panels
            .with_untracked(|panels| panels.0.clone().rekey(before, after));
        self.panels.set(Shared(next));
    }

    pub fn open(self, kind: PanelKind, path: Option<String>) -> u64 {
        let next = self.panels.with_untracked(|panels| {
            panels
                .0
                .clone()
                .open(self.scope.get_untracked(), kind, path)
        });
        self.panels.set(Shared(next.panels));
        next.id
    }

    pub fn hide(self) {
        let next = self
            .panels
            .with_untracked(|panels| panels.0.clone().set_open(self.scope.get_untracked(), false));
        self.panels.set(Shared(next));
    }

    fn activate(self, id: u64) {
        let next = self
            .panels
            .with_untracked(|panels| panels.0.clone().activate(self.scope.get_untracked(), id));
        self.panels.set(Shared(next.panels));
    }

    pub fn close_selected(self) {
        let Some(surface) = self.state.with_untracked(|state| state.selected().cloned()) else {
            return;
        };
        let scope = self.scope.get_untracked();
        let next = self
            .panels
            .with_untracked(|panels| panels.0.clone().close(scope.clone(), surface.id));
        self.panels.set(Shared(next));
        if surface.kind == PanelKind::Terminal {
            let connection = use_context::<Connection>().unwrap();
            connection.client.with(|client| {
                client.dispatch(
                    &scope.host,
                    Intent::CloseTerminal {
                        handle: self.terminal_handle(surface.id),
                    },
                )
            });
        }
    }

    pub fn terminal_handle(self, id: u64) -> String {
        format!("agent-terminal-{}-{id}", self.instance.get())
    }
}

pub(crate) fn title() -> String {
    let workspace = use_context::<Workbench>().unwrap();
    workspace.state.with(|state| {
        state
            .selected()
            .map(|surface| surface_title(surface.kind, surface.path.as_deref()))
            .unwrap_or_else(|| "ツール".into())
    })
}

fn surface_title(kind: PanelKind, path: Option<&str>) -> String {
    if kind == PanelKind::File {
        path.map(agent_core::presentation::markdown::file_basename)
            .unwrap_or("ファイル")
            .to_owned()
    } else {
        agent_core::panels::panel_title(kind)
    }
}

pub(crate) fn symbol(kind: PanelKind) -> &'static str {
    match kind {
        PanelKind::Browser => "globe",
        PanelKind::Terminal => "terminal",
        PanelKind::Files | PanelKind::File => "folder",
        PanelKind::Diff => "arrow.left.arrow.right",
        PanelKind::SideChat => "bubble.left.and.bubble.right",
        PanelKind::Agents => "network",
        PanelKind::Home => "plus",
    }
}

pub(crate) fn tools() -> Vec<agent_core::panels::PanelTool> {
    let connection = use_context::<Connection>().unwrap();
    connection
        .publication
        .with(|p| {
            agent_core::panels::panel_tools(
                p.settings.connected(),
                !p.context.selected_directory().is_empty(),
            )
        })
        .into_iter()
        .filter(|tool| {
            matches!(
                tool.kind,
                PanelKind::Files | PanelKind::Terminal | PanelKind::Diff | PanelKind::Agents
            )
        })
        .collect()
}

pub(crate) fn open_tool(key: &str) {
    let workspace = use_context::<Workbench>().unwrap();
    let connection = use_context::<Connection>().unwrap();
    if let Some(tool) = tools()
        .into_iter()
        .find(|tool| tool.key == key && tool.enabled)
    {
        let directory = connection.publication.with_untracked(|p| {
            agent_core::presentation::markdown::file_host_path(
                p.context.selected_directory(),
                p.context.home(),
            )
        });
        workspace.open(tool.kind, Some(directory));
    }
}

pub(crate) fn screen() -> impl IntoElement {
    let workspace = use_context::<Workbench>().unwrap();
    let browser = use_context::<crate::files::Browser>().unwrap();
    view().flex_1().min_h_0().pb_keyboard()
        .child(view().flex_row().gap_2().items_center().p_2()
            .child(when(is_desktop, move || native::button("xmark", "ツールを閉じる", move || browser.close()).w(px(44.0)).h(px(44.0))))
            .child(native::menu("rectangle.stack", "タブ", move || workspace.state.with(|state| json!(state.surfaces.iter().map(|surface| json!({
                "id": surface.id.to_string(), "title": surface_title(surface.kind, surface.path.as_deref()), "symbol": symbol(surface.kind), "selected": state.active == Some(surface.id)
            })).collect::<Vec<_>>())), move |id| { if let Ok(id) = id.parse() { workspace.activate(id); } })
                .prop("symbol", move || json!(workspace.state.with(|state| symbol(state.kind()))))
                .prop("title", move || json!(title())).min_h(px(44.0)).flex_1())
            .child(native::menu("plus", "ツールを追加", move || json!(tools().into_iter().map(|tool| json!({"id": tool.key, "title": tool.label, "symbol": symbol(tool.kind), "enabled": tool.enabled})).collect::<Vec<_>>()), open_tool).w(px(44.0)).h(px(44.0)))
            .child(native::button("xmark.square", "タブを閉じる", move || workspace.close_selected()).prop("enabled", move || json!(workspace.state.with(|s| s.active.is_some()))).w(px(44.0)).h(px(44.0))))
        .child(each(move || workspace.state.with(|s| s.selected().cloned().into_iter().collect::<Vec<_>>()),
            |surface: &agent_core::panels::PanelSurface| surface.id,
            move |surface, _| {
                match surface.with_untracked(|surface| surface.kind) {
                    PanelKind::Files => crate::files::screen(surface.with_untracked(|s| s.id)).into_element(),
                    PanelKind::File => crate::editor::editors().into_element(),
                    PanelKind::Terminal => crate::terminal::screen(surface.get_untracked(), workspace.scope.get_untracked().host).into_element(),
                    PanelKind::Diff => crate::details::diff(surface.with_untracked(|s| s.path.clone().unwrap_or_default()), workspace.scope.get_untracked().host).into_element(),
                    PanelKind::Agents => crate::details::agents(workspace.scope.get_untracked().host).into_element(),
                    _ => view().into_element(),
                }
            }))
        .child(when(move || workspace.state.with(|s| s.selected().is_none()), move || {
            view().p_4().gap_3().child(each(tools, |tool: &agent_core::panels::PanelTool| tool.key.clone(), move |tool, _| {
                native::text_button(move || json!(tool.with(|t| t.label.clone())), move || open_tool(&tool.with_untracked(|t| t.key.clone())))
                    .prop("symbol", move || json!(tool.with(|t| symbol(t.kind))))
                    .prop("enabled", move || json!(tool.with(|t| t.enabled)))
            }))
        }))
}
