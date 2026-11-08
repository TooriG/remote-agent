use super::*;
use agent_core::presentation::files::{EXPLORER_DEFAULT_WIDTH, explorer_width};
use gpui_kit::component::resizable::ResizableState;

pub(in crate::app) struct FileExplorer {
    pub state: Entity<ResizableState>,
    pub focus: FocusHandle,
    pub preferred: f32,
    pub available: f32,
    storage: Option<PathBuf>,
}

impl FileExplorer {
    pub fn new(cx: &mut App) -> Self {
        let storage = platform::state_dir()
            .ok()
            .map(|directory| directory.join("file-explorer-width.json"));
        Self {
            state: cx.new(|_| ResizableState::default()),
            focus: cx.focus_handle(),
            preferred: read_width(storage.as_deref()),
            available: f32::INFINITY,
            storage,
        }
    }
    fn save(&mut self, width: f32) {
        self.preferred = width;
        if let Some(path) = &self.storage
            && let Err(error) = agent_local_host::save_private_json(path, &width)
        {
            tracing::warn!(%error, "Could not persist file explorer width");
        }
    }
}
fn read_width(path: Option<&Path>) -> f32 {
    path.and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice::<f32>(&bytes).ok())
        .filter(|width| width.is_finite())
        .map(|width| explorer_width(width, f32::INFINITY))
        .unwrap_or(EXPLORER_DEFAULT_WIDTH)
}

impl Desktop {
    pub(in crate::app) fn fit_file_explorer(
        &mut self,
        available: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.file_explorer.available == available {
            return;
        }
        self.file_explorer.available = available;
        let width = explorer_width(self.file_explorer.preferred, available);
        if self.file_explorer.state.read(cx).sizes().get(1) != Some(&px(width)) {
            self.file_explorer
                .state
                .update(cx, |state, cx| state.resize_panel(1, px(width), window, cx));
        }
        cx.notify();
    }
    pub(in crate::app) fn commit_file_explorer_width(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let width = self
            .file_explorer
            .state
            .read(cx)
            .sizes()
            .get(1)
            .copied()
            .map(f32::from);
        if let Some(width) = width {
            self.resize_file_explorer(width, window, cx);
        }
    }
    pub(in crate::app) fn resize_file_explorer(
        &mut self,
        width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let width = explorer_width(width, self.file_explorer.available);
        if self.file_explorer.state.read(cx).sizes().get(1) != Some(&px(width)) {
            self.file_explorer
                .state
                .update(cx, |state, cx| state.resize_panel(1, px(width), window, cx));
        }
        self.file_explorer.save(width);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    #[test]
    fn malformed_width_preferences_recover_and_valid_widths_survive_reopening() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("file-explorer-width.json");
        assert_eq!(read_width(Some(&path)), 256.);
        std::fs::write(&path, b"broken").unwrap();
        assert_eq!(read_width(Some(&path)), 256.);
        agent_local_host::save_private_json(&path, &320.5f32).unwrap();
        assert_eq!(read_width(Some(&path)), 320.5);
        agent_local_host::save_private_json(&path, &900f32).unwrap();
        assert_eq!(read_width(Some(&path)), 560.);
    }

    #[gpui::test]
    fn explorer_keyboard_resizes_the_native_split_and_narrowing_keeps_the_preference(
        cx: &mut TestAppContext,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let storage = directory.path().join("width.json");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        cx.update(|cx| {
            crate::appearance::init(cx);
            cx.set_global(crate::Runtime {
                handle: runtime.handle().clone(),
                sessions: Default::default(),
                connections: Arc::new(crate::platform::Connections::default()),
                closing: tokio_util::task::TaskTracker::new(),
                logging_error: None,
            });
        });
        let mut desktop = None;
        let (_, window) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| {
                Desktop::new(
                    Mode::Fixture {
                        remote: None,
                        cwd: "/workspace".into(),
                    },
                    window,
                    cx,
                )
            });
            desktop = Some(view.clone());
            Root::new(view, window, cx)
        });
        let desktop = desktop.unwrap();
        window.simulate_resize(size(px(1500.), px(900.)));
        window.update(|_, cx| {
            desktop.update(cx, |view, cx| {
                Arc::make_mut(
                    &mut Arc::make_mut(Arc::make_mut(&mut view.snapshot).panes.primary_mut())
                        .navigation,
                )
                .cwd = "/workspace".into();
                view.file_explorer.storage = Some(storage.clone());
                view.file_explorer.preferred = 320.;
                view.open_surface(
                    view.panel_scope(),
                    Panel::File,
                    Some("/workspace/a.txt".into()),
                );
                view.panels = view
                    .panels
                    .clone()
                    .set_layout(view.panel_scope(), PanelLayout::Expanded);
                cx.notify();
            })
        });
        window.run_until_parked();
        window.update(|window, cx| {
            desktop.update(cx, |view, cx| {
                assert_eq!(
                    f32::from(view.file_explorer.state.read(cx).sizes()[1]),
                    320.
                );
                view.file_explorer.focus.focus(window, cx);
            })
        });
        window.simulate_keystrokes("left");
        window.update(|_, cx| {
            desktop.update(cx, |view, cx| {
                assert_eq!(
                    f32::from(view.file_explorer.state.read(cx).sizes()[1]),
                    336.
                );
                assert_eq!(view.file_explorer.preferred, 336.);
            })
        });
        assert_eq!(read_width(Some(&storage)), 336.);
        window.simulate_resize(size(px(550.), px(900.)));
        window.run_until_parked();
        window.update(|_, cx| {
            desktop.update(cx, |view, cx| {
                assert_eq!(view.file_explorer.preferred, 336.);
                assert!(f32::from(view.file_explorer.state.read(cx).sizes()[1]) < 336.);
            })
        });
        window.simulate_resize(size(px(1500.), px(900.)));
        window.run_until_parked();
        window.update(|_, cx| {
            desktop.update(cx, |view, cx| {
                assert_eq!(
                    f32::from(view.file_explorer.state.read(cx).sizes()[1]),
                    336.
                );
            })
        });
        assert_eq!(read_width(Some(&storage)), 336.);
    }
}
