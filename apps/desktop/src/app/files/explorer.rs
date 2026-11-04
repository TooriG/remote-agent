use super::*;
use agent_core::presentation::files::{
    EXPLORER_DEFAULT_WIDTH, EXPLORER_MIN_WIDTH, explorer_key_width, explorer_max_width,
    explorer_width,
};
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
    fn resize_file_explorer(&mut self, width: f32, window: &mut Window, cx: &mut Context<Self>) {
        let width = explorer_width(width, self.file_explorer.available);
        if self.file_explorer.state.read(cx).sizes().get(1) != Some(&px(width)) {
            self.file_explorer
                .state
                .update(cx, |state, cx| state.resize_panel(1, px(width), window, cx));
        }
        self.file_explorer.save(width);
        cx.notify();
    }
    pub(in crate::app) fn file_explorer_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let modifiers = event.keystroke.modifiers;
        if modifiers.alt || modifiers.control || modifiers.platform {
            return;
        }
        let Some(current) = self.file_explorer.state.read(cx).sizes().get(1).copied() else {
            return;
        };
        if let Some(width) = explorer_key_width(
            &event.keystroke.key,
            current.into(),
            self.file_explorer.available,
        ) {
            self.resize_file_explorer(width, window, cx);
            cx.stop_propagation();
        }
    }
    pub(in crate::app) fn file_explorer_handle(
        &self,
        active: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        let focus = self.file_explorer.focus.clone();
        div()
            .id("resize-file-explorer")
            .size_full()
            .track_focus(&focus)
            .tab_index(0)
            .role(Role::Splitter)
            .aria_orientation(Orientation::Vertical)
            .aria_label(crate::localization::language(cx).text("ファイル一覧の幅を変更"))
            .aria_min_numeric_value(EXPLORER_MIN_WIDTH as f64)
            .aria_max_numeric_value(explorer_max_width(self.file_explorer.available) as f64)
            .when_some(
                self.file_explorer.state.read(cx).sizes().get(1),
                |handle, width| handle.aria_numeric_value(f32::from(*width) as f64),
            )
            .border_l_1()
            .border_color(rgb(appearance::BORDER))
            .when(active, |handle| {
                handle.border_color(rgb(appearance::ACCENT))
            })
            .focus_visible(|handle| handle.border_color(rgb(appearance::ACCENT)))
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                focus.focus(window, cx)
            })
            .on_key_down(cx.listener(Self::file_explorer_key))
            .into_any_element()
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
                connections: Arc::new(crate::platform::Connections::default()),
                closing: tokio_util::task::TaskTracker::new(),
                logging_error: None,
            });
        });
        let mut desktop = None;
        let (_, window) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| {
                Desktop::new(
                    Mode::SideChat {
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
                Arc::make_mut(&mut Arc::make_mut(&mut view.snapshot).navigation).cwd =
                    "/workspace".into();
                view.file_explorer.storage = Some(storage.clone());
                view.file_explorer.preferred = 320.;
                view.panels.open(
                    view.panel_scope(),
                    Panel::File,
                    Some("/workspace/a.txt".into()),
                );
                view.panels
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
