mod agents;
pub(in crate::app) mod composer;
mod conversation;
pub(in crate::app) mod errors;
mod file_icon;
mod file_tree;
mod header;
mod markdown_diagram;
pub(super) mod markdown_file;
pub(in crate::app) mod media;
mod model_settings;
mod onboarding;
mod permissions;
mod project_management;
mod question;
pub(in crate::app) mod settings;
mod sidebar;
pub(in crate::app) mod workbench;
pub(in crate::app) mod workspace;
use super::*;
use agent_core::{presentation::error::error_text, state::operations as op};
pub(super) use composer::ComposerView;
pub(super) use conversation::ConversationView;
pub(super) use errors::error_view;
use gpui_kit::component::{
    resizable::{h_resizable, resizable_panel, v_resizable},
    sidebar::{Sidebar, SidebarItem, SidebarMenuItem},
};
pub(super) use header::HeaderView;
pub(super) use sidebar::{SidebarAction, SidebarView};
use std::path::PathBuf;
pub(super) use workbench::WorkbenchView;

// Popup contents can change while their trigger remains identical. The
// observation belongs to the open popup and is dropped with its element state.
fn observe_popup<V: 'static>(
    desktop: &WeakEntity<V>,
    render_owner: EntityId,
    window: &mut Window,
    cx: &mut Context<popover::PopoverState>,
) {
    let Some(desktop) = desktop.upgrade() else {
        return;
    };
    window.with_global_id(ElementId::View(cx.entity().entity_id()), |id, window| {
        window.with_element_state(id, |observation: Option<Subscription>, _| {
            (
                (),
                observation.unwrap_or_else(|| {
                    let app: &mut App = cx;
                    app.observe(&desktop, move |_, cx| cx.notify(render_owner))
                }),
            )
        });
    });
}

fn file_name(path: &str) -> String {
    agent_core::presentation::markdown::file_basename(path).into()
}

const CHAT_WIDTH: f32 = 780.;

// Like T3, thread details dock beside the chat when the window is wide enough and
// the right panel is closed; otherwise the details toggle opens them as a popover.
fn details_inline(panel_open: bool, width: Pixels) -> bool {
    !panel_open && width >= px(1280.)
}

fn new_chat_icon() -> Icon {
    Icon::default().path("icons/square-pen.svg")
}

pub(super) fn section_heading(title: &'static str, description: &'static str) -> Div {
    v_flex()
        .gap_1()
        .child(div().text_lg().font_semibold().child(title))
        .child(
            div()
                .text_sm()
                .text_color(rgb(appearance::MUTED))
                .child(description),
        )
}

pub(super) fn settings_group() -> Div {
    v_flex()
        .w_full()
        .rounded(px(8.))
        .border_1()
        .border_color(rgb(appearance::BORDER))
        .bg(rgb(appearance::SURFACE))
}

fn sidebar_header(expanded: bool, owner: &WeakEntity<Desktop>, cx: &App) -> Div {
    let language = crate::localization::language(cx);
    let (id, icon, label) = if expanded {
        (
            "collapse-sidebar",
            IconName::PanelLeftClose,
            language.text("サイドバーを閉じる"),
        )
    } else {
        (
            "expand-sidebar",
            IconName::PanelLeftOpen,
            language.text("サイドバーを開く"),
        )
    };
    h_flex()
        .h(px(crate::WINDOW_HEADER_HEIGHT))
        .flex_shrink_0()
        .pl(px(if cfg!(target_os = "windows") {
            12.
        } else {
            88.
        }))
        .gap_2()
        .when(
            cfg!(target_os = "windows"),
            crate::window_chrome::drag_region,
        )
        .child(
            img("icons/icon.png")
                .size(px(22.))
                .rounded(px(5.))
                .flex_shrink_0(),
        )
        .when(expanded, |header| {
            header.w_full().pr_3().child(div().flex_1())
        })
        .child(
            div()
                .when(cfg!(target_os = "windows"), |view| view.occlude())
                .child(
                    Button::new(id)
                        .debug_selector(move || id.into())
                        .icon(icon)
                        .small()
                        .ghost()
                        .tooltip(label)
                        .accessibility_label(label)
                        .on_click({
                            let owner = owner.clone();
                            move |_, _, cx| {
                                let _ = owner.update(cx, |desktop, cx| {
                                    desktop.sidebar = !expanded;
                                    cx.notify();
                                });
                            }
                        }),
                ),
        )
}

fn image_skeleton(height: f32, background: Hsla) -> Div {
    div()
        .w_full()
        .min_w_0()
        .max_w(px(height))
        .h(px(height))
        .flex_shrink_0()
        .child(crate::activity::pulse(background))
}

fn user_message_bubble() -> Div {
    v_flex()
        .debug_selector(|| "user-message-bubble".into())
        .gap_3()
        .min_w_0()
        .max_w_full()
        .p_4()
        .rounded(px(18.))
        .bg(rgb(appearance::RAISED))
}

fn review_counts(
    additions: Option<u64>,
    deletions: Option<u64>,
    language: agent_core::localization::UiLanguage,
) -> AnyElement {
    let counts = h_flex().gap_1().text_xs();
    match (additions, deletions) {
        (Some(added), Some(deleted)) => counts
            .child(div().text_color(rgb(0x37cf77)).child(format!("+{added}")))
            .child(div().text_color(rgb(0xff6259)).child(format!("−{deleted}")))
            .into_any_element(),
        _ => counts
            .text_color(rgb(appearance::MUTED))
            .child(language.text("バイナリ"))
            .into_any_element(),
    }
}

fn conversation_file_path(source: &str, cwd: &str) -> Result<PathBuf, String> {
    agent_core::presentation::markdown::markdown_file_target(
        source.into(),
        cwd.into(),
        agent_core::presentation::markdown::MarkdownFileSurface::Desktop,
    )
    .map(|file| PathBuf::from(file.path))
    .ok_or_else(|| "ファイルパスが不正です".into())
}

impl Desktop {
    fn publish_projects(&self, cx: &mut Context<Self>) {
        let mut projects = Vec::new();
        let mut folders = Vec::new();
        let mut more = false;
        if let Some(list) = &self.publication.list {
            more = list.has_more_projects;
            projects.extend(
                list.projects
                    .iter()
                    .chain(&list.hidden_projects)
                    .map(|project| (project.id.clone(), project.name.clone())),
            );
            for project in &list.projects {
                for root in &project.roots {
                    let label = if project.roots.len() == 1 {
                        project.name.clone()
                    } else {
                        root.path.clone()
                    };
                    folders.push((root.path.clone(), label));
                }
            }
        }
        self.preferences.update(cx, |view, cx| {
            if view.projects != projects {
                view.projects = projects;
                cx.notify();
            }
        });
        self.composer_view
            .read(cx)
            .content
            .clone()
            .update(cx, |view, cx| {
                view.set_folders(folders, more, cx);
            });
    }
    pub(in crate::app) fn publish_preferences(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.preferences.update(cx, |view, cx| {
            view.bind(
                self.session.as_ref().map(|session| session.pane.clone()),
                self.remote.clone(),
                window,
                cx,
            );
            let notices = self.error_rows(&ErrorScope::Settings);
            let changed = view.notices != notices
                || view.blocked != self.busy()
                || view.opened != (self.tab == Tab::Settings)
                || view.hosts != self.hosts
                || view.app_updates != self.app_updates;
            view.notices = notices;
            view.blocked = self.busy();
            view.opened = self.tab == Tab::Settings;
            view.hosts = self.hosts.clone();
            view.app_updates = self.app_updates.clone();
            if changed {
                cx.notify();
            }
            view.sync(self.publication.settings.clone(), window, cx);
        });
    }
    // Fixtures inject immutable state at the same core projection boundary as Store.
    #[cfg(test)]
    pub(in crate::app) fn project_fixture(&mut self) {
        self.publication = agent_core::presentation::view::Presentation::project(
            self.snapshot.clone(),
            Some(&self.publication),
        );
    }

    fn publish_media(&self, cx: &mut Context<Self>) {
        let source = media::MediaSource {
            host: self.remote_key().into(),
            cwd: self.snapshot.pane.navigation.cwd.clone(),
            remote: self.remote.is_some(),
            session: self.selected().cloned(),
        };
        self.media.update(cx, |media, cx| {
            media.sync(
                source,
                self.session
                    .as_ref()
                    .map(|session| session.host.store.clone()),
                self.session.as_ref().map(|session| session.pane.clone()),
                cx,
            )
        });
    }
    fn publish_sidebar(&self, cx: &mut Context<Self>) {
        if let Some(sidebar) = &self.sidebar_view {
            let model = self.sidebar_model(cx);
            let pane = self.session.as_ref().map(|session| session.pane.clone());
            sidebar.update(cx, |view, cx| view.sync(model, pane, cx));
        }
    }
    fn publish_header(&self, cx: &mut Context<Self>) {
        let header = self.header_model(crate::localization::language(cx));
        self.header_view
            .update(cx, |view, cx| view.set_model(header, cx));
    }
    pub(in crate::app) fn publish_views(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.publish_projects(cx);
        self.publish_media(cx);
        self.publish_sidebar(cx);
        self.publish_preferences(window, cx);
        self.publish_workspace(cx);
        self.publish_conversation(window, cx);
        self.sync_composer(cx);
        self.publish_header(cx);
        self.sync_surfaces(window, cx);
    }
}

impl Render for Desktop {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.media.read(cx).gallery.is_some() {
            let gallery = self
                .media
                .clone()
                .cached(StyleRefinement::default().size_full());
            return h_flex()
                .size_full()
                .bg(rgb(appearance::BACKGROUND))
                .text_color(rgb(appearance::FOREGROUND))
                .text_size(px(14.))
                .child(gallery)
                .when(
                    cfg!(target_os = "windows") && !self.side_chat_mode,
                    |content| crate::window_chrome::screen(content, "bex", window),
                );
        }
        if self.import_projects || (self.onboarding && self.tab == Tab::Chat) {
            return self.onboarding_view(window, cx).when(
                cfg!(target_os = "windows") && !self.side_chat_mode,
                |content| crate::window_chrome::screen(content, "bex", window),
            );
        }
        let main = if self.project_editor.is_some() {
            let editor = self.project_editor_view(cx);
            if cfg!(target_os = "windows") && !self.side_chat_mode {
                crate::window_chrome::screen(
                    editor,
                    crate::localization::language(cx).text("プロジェクトを編集"),
                    window,
                )
                .into_any_element()
            } else {
                editor
            }
        } else {
            self.conversation_content(window.viewport_size().width, cx)
        };
        h_flex()
            .relative()
            .on_action(files::run_file_menu_command)
            .size_full()
            .items_stretch()
            .bg(rgb(appearance::BACKGROUND))
            .text_color(rgb(appearance::FOREGROUND))
            .text_size(px(14.))
            .font_weight(FontWeight::NORMAL)
            .when(self.sidebar, |body| {
                body.children(self.sidebar_view.clone().map(|sidebar| {
                    sidebar.cached(
                        StyleRefinement::default()
                            .w(px(272.))
                            .h_full()
                            .flex_shrink_0(),
                    )
                }))
            })
            .child(main)
            .child(deferred(
                div()
                    .absolute()
                    .top_12()
                    .right_4()
                    .w(px(420.))
                    .max_w_full()
                    .child(self.error_notices(ErrorScope::Application, cx)),
            ))
            .when(self.tab == Tab::Settings, |body| {
                body.child(deferred(
                    self.preferences
                        .clone()
                        .cached(StyleRefinement::default().size_full()),
                ))
            })
    }
}

impl Desktop {
    fn panel_toggle(open: bool, owner: &WeakEntity<Desktop>, cx: &App) -> Button {
        let language = crate::localization::language(cx);
        let owner = owner.clone();
        Button::new("panel-toggle")
            .icon(IconName::PanelRight)
            .small()
            .ghost()
            .tooltip(language.text("右パネルを切り替え"))
            .accessibility_label(language.text("右パネルを切り替え"))
            .on_click(move |_, window, cx| {
                let _ = owner.update(cx, |view, cx| {
                    let key = view.panel_scope();
                    view.panels = view.panels.clone().set_open(key.clone(), !open);
                    if !open && view.panels.get(key).active.is_none() {
                        view.panel_focus.focus(window, cx);
                    }
                    cx.notify();
                });
            })
            .debug_selector(|| "panel-toggle".into())
            .selected(open)
    }

    fn conversation_main(&mut self, show_workspace: bool, cx: &mut Context<Self>) -> AnyElement {
        let content = h_flex()
            .flex_1()
            .min_h_0()
            .items_stretch()
            .child(self.chat())
            .when(show_workspace, |content| {
                content.child(
                    div().w(px(300.)).flex_shrink_0().child(
                        self.workspace_view
                            .clone()
                            .cached(StyleRefinement::default().size_full()),
                    ),
                )
            });
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(self.conversation_header(cx))
            .child(content)
            .into_any_element()
    }

    fn conversation_content(&mut self, width: Pixels, cx: &mut Context<Self>) -> AnyElement {
        let panel = self.panels.get(self.panel_scope());
        let panel_open = panel.open;
        let panel_layout = panel.layout;
        let wide = width >= px(1080.);
        let content = if self.side_chat_mode {
            if panel_open {
                self.workbench()
            } else {
                self.chat()
            }
        } else if panel_open {
            let right = self.workbench();
            if panel_layout == PanelLayout::Bottom {
                v_resizable(SharedString::from(format!(
                    "chat-workbench-bottom-split:{:?}",
                    self.panel_scope()
                )))
                .child(resizable_panel().child(self.conversation_main(false, cx)))
                .child(resizable_panel().size(px(360.)).child(right))
                .into_any_element()
            } else if wide && panel_layout == PanelLayout::Right {
                h_resizable(SharedString::from(format!(
                    "chat-workbench-split:{:?}",
                    self.panel_scope()
                )))
                .child(
                    resizable_panel()
                        .size_range(px(360.)..px(2400.))
                        .child(self.conversation_main(false, cx)),
                )
                .child(
                    resizable_panel()
                        .size(px(540.))
                        .size_range(px(320.)..px(1100.))
                        .child(right),
                )
                .into_any_element()
            } else {
                right
            }
        } else {
            self.conversation_main(self.details && details_inline(panel_open, width), cx)
        };
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .items_stretch()
                    .child(content),
            )
            .into_any_element()
    }
}

pub(super) fn button<V: 'static>(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    cx: &Context<V>,
    action: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
) -> Button {
    let label = label.into();
    Button::new(id.into())
        .accessibility_label(label.clone())
        .child(div().flex_1().min_w_0().text_ellipsis().child(label))
        .small()
        .ghost()
        .on_click(cx.listener(move |s, _, w, cx| {
            action(s, w, cx);
            cx.notify();
        }))
}
pub(super) fn icon_button<V: 'static>(
    id: impl Into<SharedString>,
    icon: impl Into<Icon>,
    label: impl Into<SharedString>,
    cx: &Context<V>,
    action: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
) -> Button {
    let label = label.into();
    Button::new(id.into())
        .icon(icon.into())
        .small()
        .ghost()
        .tooltip(label.clone())
        .accessibility_label(label)
        .on_click(cx.listener(move |s, _, w, cx| {
            action(s, w, cx);
            cx.notify();
        }))
}
#[cfg(test)]
mod link_tests {
    use super::conversation_file_path;

    #[test]
    fn file_links_resolve_on_the_selected_host_with_spaces_and_line_numbers() {
        for source in [
            "/tmp/project/a%20b.png",
            "file:///tmp/project/a%20b.png",
            "a%20b.png",
            "a%20b.png:12",
            "a%20b.png#L12",
        ] {
            assert_eq!(
                conversation_file_path(source, "/tmp/project").unwrap(),
                std::path::Path::new("/tmp/project/a b.png")
            );
        }
        assert!(conversation_file_path("https://example.com", "/tmp/project").is_err());
        assert!(conversation_file_path("javascript:alert(1)", "/tmp/project").is_err());
        assert!(conversation_file_path("file://other-host/private.png", "/tmp/project").is_err());
        assert_eq!(
            conversation_file_path("~/Movies/video.mp4", "/tmp/project").unwrap(),
            std::path::Path::new("~/Movies/video.mp4")
        );
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_kit as gpui;

    const TOOLTIP_DELAY: std::time::Duration = std::time::Duration::from_millis(1);

    fn init(cx: &mut TestAppContext) -> (tokio::runtime::Runtime, Runtime) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let app_runtime = Runtime {
            handle: runtime.handle().clone(),
            sessions: Default::default(),
            connections: Arc::default(),
            closing: tokio_util::task::TaskTracker::new(),
            logging_error: None,
        };
        cx.update(|cx| {
            crate::appearance::init(cx);
            cx.set_global(app_runtime.clone());
        });
        (runtime, app_runtime)
    }

    struct CachedRegion {
        renders: std::rc::Rc<std::cell::Cell<usize>>,
        interactive: bool,
        hover: bool,
    }

    struct RegionTooltip;

    impl Render for RegionTooltip {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .debug_selector(|| "cache-test-tooltip".into())
                .child("Tooltip")
        }
    }

    impl Render for CachedRegion {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.renders.set(self.renders.get() + 1);
            let selector = if self.interactive {
                "cache-test-target"
            } else {
                "cache-test-static"
            };
            div()
                .id("cache-test-region")
                .debug_selector(move || selector.into())
                .size_full()
                .when(self.interactive, |region| {
                    region
                        .when(self.hover, |region| {
                            region.hover(|style| style.bg(rgb(0x123456)))
                        })
                        .active(|style| style.bg(rgb(0x654321)))
                        .tooltip_show_delay(TOOLTIP_DELAY)
                        .tooltip(|_, cx| cx.new(|_| RegionTooltip).into())
                        .on_click(|_, _, _| {})
                })
        }
    }

    struct CachedRegions([gpui::AnyView; 2]);

    impl Render for CachedRegions {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            h_flex()
                .size_full()
                .children(self.0.iter().cloned().map(|region| {
                    region.cached(gpui::StyleRefinement::default().w(px(100.)).h(px(100.)))
                }))
        }
    }

    #[gpui::test]
    fn pointer_and_tooltip_changes_reuse_unrelated_cached_views(cx: &mut TestAppContext) {
        for hover in [false, true] {
            let counts = [
                std::rc::Rc::new(std::cell::Cell::new(0)),
                std::rc::Rc::new(std::cell::Cell::new(0)),
            ];
            let (_, window) = cx.add_window_view(|_, cx| {
                CachedRegions([
                    cx.new(|_| CachedRegion {
                        renders: counts[0].clone(),
                        interactive: true,
                        hover,
                    })
                    .into(),
                    cx.new(|_| CachedRegion {
                        renders: counts[1].clone(),
                        interactive: false,
                        hover: false,
                    })
                    .into(),
                ])
            });
            window.update(|window, cx| window.draw(cx).clear(cx));
            window.run_until_parked();
            let baseline = counts[1].get();
            let pointer = window.debug_bounds("cache-test-target").unwrap().center();
            let moved_pointer = pointer + point(px(1.), px(0.));
            let outside = window.debug_bounds("cache-test-static").unwrap().center();
            window.simulate_mouse_move(pointer, None, gpui::Modifiers::default());
            window.executor().advance_clock(TOOLTIP_DELAY);
            window.run_until_parked();
            assert!(window.debug_bounds("cache-test-tooltip").is_some());
            assert_eq!(
                counts[1].get(),
                baseline,
                "showing a tooltip must reuse the other view"
            );

            let hovered = counts[0].get();
            window.update(|window, cx| {
                window.dispatch_keystroke(gpui::Keystroke::parse("a").unwrap(), cx);
            });
            window.run_until_parked();
            assert!(
                counts[0].get() > hovered,
                "keyboard input must suppress hover and its tooltip"
            );
            assert!(window.debug_bounds("cache-test-tooltip").is_none());
            assert_eq!(counts[1].get(), baseline);

            window.simulate_mouse_move(moved_pointer, None, gpui::Modifiers::default());
            window.executor().advance_clock(TOOLTIP_DELAY);
            window.run_until_parked();
            assert!(window.debug_bounds("cache-test-tooltip").is_some());
            window.simulate_click(moved_pointer, gpui::Modifiers::default());
            window.run_until_parked();
            assert_eq!(
                counts[1].get(),
                baseline,
                "click feedback must reuse the other view"
            );
            window.simulate_mouse_move(outside, None, gpui::Modifiers::default());
            window.run_until_parked();
            assert!(window.debug_bounds("cache-test-tooltip").is_none());
            assert_eq!(counts[1].get(), baseline);
        }
    }

    #[gpui::test]
    fn focus_visible_changes_preserve_other_view_caches(cx: &mut TestAppContext) {
        struct FocusRegion(gpui::FocusHandle);
        impl Render for FocusRegion {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                div()
                    .track_focus(&self.0)
                    .size_full()
                    .focus_visible(|style| style.pl(px(10.)))
                    .child(
                        div()
                            .debug_selector(|| "focus-visible-marker".into())
                            .size(px(10.)),
                    )
            }
        }
        let renders = std::rc::Rc::new(std::cell::Cell::new(0));
        let mut focus = None;
        let (_, window) = cx.add_window_view(|_, cx| {
            let handle = cx.focus_handle();
            focus = Some(handle.clone());
            CachedRegions([
                cx.new(|_| FocusRegion(handle)).into(),
                cx.new(|_| CachedRegion {
                    renders: renders.clone(),
                    interactive: false,
                    hover: false,
                })
                .into(),
            ])
        });
        window.simulate_mouse_move(point(px(150.), px(50.)), None, gpui::Modifiers::default());
        window.update(|window, cx| window.focus(focus.as_ref().unwrap(), cx));
        window.run_until_parked();
        let baseline = renders.get();
        let resting = window.debug_bounds("focus-visible-marker").unwrap().left();
        window.update(|window, cx| {
            window.dispatch_keystroke(gpui::Keystroke::parse("a").unwrap(), cx);
        });
        window.run_until_parked();
        assert_eq!(
            window.debug_bounds("focus-visible-marker").unwrap().left(),
            resting + px(10.)
        );
        assert_eq!(renders.get(), baseline);
        window.simulate_mouse_move(point(px(151.), px(50.)), None, gpui::Modifiers::default());
        window.run_until_parked();
        assert_eq!(
            window.debug_bounds("focus-visible-marker").unwrap().left(),
            resting
        );
        assert_eq!(renders.get(), baseline);
    }

    #[gpui::test]
    fn snapshot_bursts_adopt_the_latest_state_once_per_frame(cx: &mut TestAppContext) {
        let (runtime, app_runtime) = init(cx);
        let store = runtime
            .block_on(async { Arc::new(agent_core::store::Store::offline(Snapshot::default())) });
        let mut session = Some(runtime.block_on(async {
            let (updates, receive) = async_channel::unbounded();
            tokio::spawn(StoreSession::publish(
                Ok(store.clone()),
                app_runtime,
                updates,
                Some,
                |_| None,
            ));
            receive.recv().await.unwrap().unwrap().unwrap()
        }));
        let (desktop, window) = cx.add_window_view(|window, cx| {
            let mut desktop = Desktop::new(
                Mode::Fixture {
                    remote: None,
                    cwd: String::new(),
                },
                window,
                cx,
            );
            desktop.snapshot = store.snapshot();
            desktop.session = session.take();
            desktop.epoch += 1;
            desktop.connecting = false;
            desktop
        });
        window.run_until_parked();
        let key = desktop.read_with(window, |view, _| view.draft_key().clone());
        for expected in ["first batch", "次のフレーム🌸"] {
            for text in ["intermediate", expected] {
                runtime
                    .block_on(store.dispatch(Intent::SetDraftText {
                        thread_id: key.clone(),
                        text: text.into(),
                    }))
                    .unwrap();
                window.update(|window, cx| {
                    desktop.update(cx, |view, cx| {
                        view.receive((view.epoch, Update::Snapshot), window, cx);
                    });
                });
            }
            assert_ne!(
                desktop.read_with(window, |view, _| view.draft().text.clone()),
                expected
            );
            let callbacks = window.update(|window, cx| window.simulate_next_frame(cx));
            assert_eq!(
                callbacks, 1,
                "a burst must schedule only one snapshot adoption"
            );
            window.run_until_parked();
            assert_eq!(
                desktop.read_with(window, |view, _| view.draft().text.clone()),
                expected
            );
            assert!(desktop.read_with(window, |view, _| view.snapshot_pending.is_none()));
        }
        for publish_current in [false, true] {
            let previous = desktop.read_with(window, |view, _| view.draft().text.clone());
            let text = format!("new generation {publish_current}");
            runtime
                .block_on(store.dispatch(Intent::SetDraftText {
                    thread_id: key.clone(),
                    text: text.clone(),
                }))
                .unwrap();
            window.update(|window, cx| {
                desktop.update(cx, |view, cx| {
                    view.receive((view.epoch, Update::Snapshot), window, cx);
                    view.epoch += 1;
                    if publish_current {
                        view.receive((view.epoch, Update::Snapshot), window, cx);
                    }
                });
                window.simulate_next_frame(cx);
            });
            window.run_until_parked();
            assert_eq!(
                desktop.read_with(window, |view, _| view.draft().text.clone()),
                if publish_current { text } else { previous },
                "a replaced connection's callback must neither adopt its state nor clear the current callback"
            );
            assert!(desktop.read_with(window, |view, _| view.snapshot_pending.is_none()));
        }
    }

    #[gpui::test]
    fn loading_images_pulse_without_rebuilding_their_content(cx: &mut TestAppContext) {
        struct Loading(std::rc::Rc<std::cell::Cell<usize>>);
        impl Render for Loading {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                self.0.set(self.0.get() + 1);
                div()
                    .size_full()
                    .child(image_skeleton(320., rgb(appearance::RAISED).into()))
            }
        }
        let renders = std::rc::Rc::new(std::cell::Cell::new(0));
        let window = cx.open_window(size(px(400.), px(400.)), |window, cx| {
            let content = cx.new(|_| Loading(renders.clone()));
            crate::activity::ActivityWindow::new(content, window, cx)
        });
        cx.run_until_parked();
        let initial = renders.get();
        for _ in 0..3 {
            assert!(
                window
                    .update(cx, |_, window, cx| window.simulate_next_frame(cx))
                    .unwrap()
                    > 0
            );
            cx.run_until_parked();
        }
        assert_eq!(
            renders.get(),
            initial,
            "image loading must not rebuild its surrounding view on animation frames"
        );
    }

    fn renders(desktop: &Desktop, cx: &App) -> [usize; 5] {
        [
            desktop.sidebar_view.as_ref().unwrap().read(cx).renders,
            desktop.conversation.read(cx).renders,
            desktop.composer_view.read(cx).content.read(cx).renders,
            desktop.workbench_view.read(cx).renders,
            desktop.header_view.read(cx).renders,
        ]
    }

    #[gpui::test]
    fn browser_frames_do_not_broadcast_desktop_notifications(cx: &mut TestAppContext) {
        let (runtime, app_runtime) = init(cx);
        let mut snapshot = Snapshot {
            connection: agent_core::state::Connection {
                connected: true,
                ..Default::default()
            },
            ..Default::default()
        };
        Arc::make_mut(&mut Arc::make_mut(&mut snapshot.pane).navigation).draft_key =
            DraftKey::Session {
                session: SessionRef {
                    provider: agent_protocol::session::ProviderKind::Codex,
                    id: "browser-fixture".into(),
                },
            };
        let store =
            runtime.block_on(async { Arc::new(agent_core::store::Store::offline(snapshot)) });
        let mut session = Some(runtime.block_on(async {
            let (updates, receive) = async_channel::unbounded();
            tokio::spawn(StoreSession::publish(
                Ok(store.clone()),
                app_runtime,
                updates,
                Some,
                |_| None,
            ));
            receive.recv().await.unwrap().unwrap().unwrap()
        }));
        let (desktop, window) = cx.add_window_view(|window, cx| {
            let mut desktop = Desktop::new(Mode::Main, window, cx);
            desktop.onboarding = false;
            desktop.snapshot = store.snapshot();
            desktop.session = session.take();
            desktop.open_panel(Panel::Browser, window, cx);
            desktop
        });
        window.simulate_resize(size(px(1600.), px(900.)));
        window.run_until_parked();
        let (id, browser) = desktop.read_with(window, |desktop, _| {
            let id = desktop.panels.get(desktop.panel_scope()).active.unwrap();
            let SurfaceResource::Browser(browser) = &desktop.surface_resources[&id] else {
                panic!("the production renderer must create the browser")
            };
            (id, browser.clone())
        });
        let before = desktop.read_with(window, renders);
        let notifications = std::rc::Rc::new(std::cell::Cell::new(0));
        let _observation = window.update(|_, cx| {
            let count = notifications.clone();
            cx.observe(&desktop, move |_, _| count.set(count.get() + 1))
        });
        for _ in 0..3 {
            browser.update(window, |_, cx| cx.notify());
            window.run_until_parked();
            assert_eq!(
                notifications.get(),
                0,
                "browser frames must stay in the workbench"
            );
            let after = desktop.read_with(window, renders);
            for index in [0, 1, 2, 4] {
                assert_eq!(
                    after[index], before[index],
                    "browser frames must reuse other regions"
                );
            }
        }
        let before_title = desktop.read_with(window, renders);
        browser.update(window, |_, cx| {
            cx.emit(crate::browser::BrowserTitle("Updated page".into()))
        });
        window.run_until_parked();
        let after_title = desktop.read_with(window, renders);
        assert!(
            after_title[3] > before_title[3],
            "a changed page title must redraw the workbench tab"
        );
        assert_eq!(
            notifications.get(),
            0,
            "title changes must stay in the workbench too"
        );
        browser.update(window, |_, cx| {
            cx.emit(crate::browser::BrowserTitle("Updated page".into()))
        });
        window.run_until_parked();
        assert_eq!(
            desktop.read_with(window, renders),
            after_title,
            "identical titles must reuse the workbench"
        );
        window.update(|window, cx| {
            desktop.update(cx, |desktop, cx| {
                desktop.close_surface(id, window, cx);
            })
        });
        window.run_until_parked();
        let before = desktop.read_with(window, renders);
        browser.update(window, |_, cx| {
            cx.emit(crate::browser::BrowserTitle("Closed page".into()));
            cx.notify();
        });
        window.run_until_parked();
        assert_eq!(
            desktop.read_with(window, renders),
            before,
            "closed browser tabs must release their workbench observation"
        );
    }

    #[gpui::test]
    fn thread_details_open_as_a_popover_wherever_the_card_cannot_dock(cx: &mut TestAppContext) {
        let (_runtime, _) = init(cx);
        let mut owner = None;
        let (_, window) = cx.add_window_view(|window, cx| {
            let desktop = cx.new(|cx| {
                let mut desktop = Desktop::new(Mode::Main, window, cx);
                desktop.onboarding = false;
                desktop
            });
            owner = Some(desktop.clone());
            crate::activity::ActivityWindow::new(desktop, window, cx)
        });
        let desktop = owner.unwrap();
        let open_details = |window: &mut VisualTestContext| {
            let toggle = window.debug_bounds("thread-details-toggle").unwrap();
            window.simulate_click(toggle.center(), Modifiers::default());
            window.run_until_parked();
            window.debug_bounds("thread-details-popover").is_some()
        };

        window.simulate_resize(size(px(1000.), px(700.)));
        window.run_until_parked();
        assert!(
            open_details(window),
            "a narrow window opens details from the header"
        );
        assert!(desktop.read_with(window, |desktop, _| desktop.details_popover));

        window.simulate_resize(size(px(1400.), px(700.)));
        window.run_until_parked();
        assert!(
            window.debug_bounds("thread-details-popover").is_none()
                && !desktop.read_with(window, |desktop, _| desktop.details_popover),
            "the popover closes once the card docks again"
        );

        window.simulate_resize(size(px(1000.), px(700.)));
        desktop.update(window, |desktop, cx| {
            let key = desktop.panel_scope();
            desktop.panels = desktop.panels.clone().set_open(key.clone(), true);
            desktop.panels = desktop
                .panels
                .clone()
                .set_layout(key, PanelLayout::Expanded);
            cx.notify();
        });
        window.run_until_parked();
        assert!(window.debug_bounds("conversation-header").is_none());
        assert!(
            open_details(window),
            "a panel covering the conversation keeps details reachable"
        );
    }

    #[gpui::test]
    fn open_model_menu_updates_when_its_trigger_is_unchanged(cx: &mut TestAppContext) {
        let (_runtime, _) = init(cx);
        let mut owner = None;
        let (_, window) = cx.add_window_view(|window, cx| {
            let desktop = cx.new(|cx| {
                let mut desktop = Desktop::new(Mode::Main, window, cx);
                desktop.onboarding = false;
                let key = desktop.draft_key().clone();
                Arc::make_mut(&mut Arc::make_mut(&mut desktop.snapshot).input.drafts).insert(
                    key,
                    Arc::new(Draft {
                        model: Some(agent_protocol::models::ModelRef {
                            provider: agent_protocol::session::ProviderKind::Codex,
                            id: "gpt".into(),
                        }),
                        ..Default::default()
                    }),
                );
                desktop.project_fixture();
                desktop
            });
            owner = Some(desktop.clone());
            crate::activity::ActivityWindow::new(desktop, window, cx)
        });
        window.simulate_resize(size(px(1000.), px(700.)));
        window.run_until_parked();
        let trigger = window.debug_bounds("model-select").unwrap();
        window.simulate_click(trigger.center(), Modifiers::default());
        window.run_until_parked();
        assert!(window.debug_bounds("model-choice-extra").is_none());
        owner.unwrap().update(window, |desktop, cx| {
            Arc::make_mut(&mut desktop.snapshot).catalog.models = Arc::new(
                serde_json::from_value(serde_json::json!([
                    {"id":"extra","model":{"provider":"codex","id":"extra"},"displayName":"Extra"}
                ]))
                .unwrap(),
            );
            desktop.project_fixture();
            cx.notify();
        });
        window.run_until_parked();
        assert!(
            window.debug_bounds("model-choice-extra").is_some(),
            "catalog updates must reach an open popup even when the selected model and trigger stay unchanged"
        );
        let summary = window
            .debug_bounds("model-account-summary")
            .expect("model menu opens");
        window.simulate_click(summary.center(), Modifiers::default());
        window.run_until_parked();
        assert!(
            window.debug_bounds("account-start-login").is_some(),
            "account management must open directly inside the model popup"
        );
        let back = window.debug_bounds("model-panel-back").unwrap();
        window.simulate_click(back.center(), Modifiers::default());
        window.run_until_parked();
        assert!(window.debug_bounds("model-choice-extra").is_some());
    }

    #[gpui::test]
    fn native_commands_use_the_bound_pane_without_notifying_the_window(cx: &mut TestAppContext) {
        let (runtime, app_runtime) = init(cx);
        let store = runtime
            .block_on(async { Arc::new(agent_core::store::Store::offline(Snapshot::default())) });
        let session = runtime.block_on(async { StoreSession::new(store.clone(), app_runtime) });
        let key = store.snapshot().pane.navigation.draft_key.clone();
        let (desktop, window) = cx.add_window_view(|window, cx| {
            let mut view = Desktop::new(
                Mode::Fixture {
                    remote: None,
                    cwd: "/fixture".into(),
                },
                window,
                cx,
            );
            view.session = Some(session);
            view.accept_snapshot(window, cx);
            // Bind every native resource, including the currently closed workbench.
            view.publish_views(window, cx);
            view
        });
        window.run_until_parked();
        let notifications = std::rc::Rc::new(std::cell::Cell::new(0));
        let _observation = window.update(|_, cx| {
            let notifications = notifications.clone();
            cx.observe(&desktop, move |_, _| {
                notifications.set(notifications.get() + 1)
            })
        });
        let mut updates = store.subscribe();
        for owner in 0..3 {
            let text = format!("Native command from owner {owner}");
            let intent = Intent::SetDraftText {
                thread_id: key.clone(),
                text: text.clone(),
            };
            window.update(|_, cx| {
                desktop.update(cx, |view, cx| match owner {
                    0 => view
                        .composer_view
                        .read(cx)
                        .content
                        .read(cx)
                        .dispatch(intent),
                    1 => view.conversation.read(cx).dispatch(intent),
                    _ => view.workbench_view.read(cx).dispatch(intent),
                })
            });
            runtime.block_on(async {
                tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    updates.wait_for(|snapshot| {
                        snapshot
                            .input
                            .drafts
                            .get(&key)
                            .is_some_and(|draft| draft.text == text)
                    }),
                )
                .await
                .expect("the bound pane must receive the command")
                .unwrap();
            });
            window.run_until_parked();
            assert_eq!(
                notifications.get(),
                0,
                "native commands must not notify Desktop observers"
            );
        }
    }

    #[gpui::test]
    fn typing_streaming_and_animation_rebuild_only_the_affected_views(cx: &mut TestAppContext) {
        let (runtime, _) = init(cx);
        let session = SessionRef {
            provider: agent_protocol::session::ProviderKind::Codex,
            id: "selected".into(),
        };
        let mut desktop = None;
        let window = cx.open_window(size(px(1600.), px(700.)), |window, cx| {
            let view = cx.new(|cx| {
                let mut view = Desktop::new(Mode::Main, window, cx);
                view.onboarding = false;
                // Host discovery and updater tasks are independent of this
                // input/Store boundary and can notify while Tokio is pumped.
                view.hosts = None;
                view.app_updates = None;
                view.open_surface(view.panel_scope(), Panel::Files, None);
                let snapshot = Arc::make_mut(&mut view.snapshot);
                Arc::make_mut(&mut Arc::make_mut(&mut snapshot.pane).navigation).thread_id = Some(session.clone());
                Arc::make_mut(&mut Arc::make_mut(&mut snapshot.pane).navigation).draft_key = session.clone().into();
                snapshot.lists.threads = Some(Arc::new(serde_json::from_value(serde_json::json!({
                    "data":[{"id":{"provider":"codex","id":"selected"},"name":"Task"}],
                    "projects":[],"hasMore":false,"hasMoreProjects":false,"projectPages":{},"limit":5,
                })).unwrap()));
                Arc::make_mut(&mut snapshot.sessions.conversations).insert(session.clone(), Arc::new(serde_json::from_value(serde_json::json!({
                    "id":{"provider":"codex","id":"selected"},
                    "turns":[{"id":"turn","status":"completed","items":[{"id":"answer","body":{"inline":{"body":{"assistantText":{"text":"Original answer","phase":"final"}}}}}]}]
                })).unwrap()));
                let store = runtime.block_on(async {
                    Arc::new(agent_core::store::Store::offline((*view.snapshot).clone()))
                });
                view.snapshot = store.snapshot();
                view.epoch += 1;
                let epoch = view.epoch;
                let updates = view.updates.clone();
                let app_runtime = view.runtime.clone();
                view.session = Some(runtime.block_on(async {
                    let (connected, receive) = async_channel::bounded(1);
                    tokio::spawn(StoreSession::publish(
                        Ok(store), app_runtime, updates,
                        move |session| {
                            connected.send_blocking(session).unwrap();
                            (epoch, Update::Snapshot)
                        },
                        move |_| (epoch, Update::Snapshot),
                    ));
                    let session = receive.recv().await.unwrap().unwrap();
                    session.pane.subscribe().wait_for(|value| value.list.is_some()).await.unwrap();
                    session
                }));
                view.accept_snapshot(window, cx);
                cx.notify();
                view
            });
            desktop = Some(view.clone());
            crate::activity::ActivityWindow::new(view, window, cx)
        });
        let desktop = desktop.unwrap();
        cx.run_until_parked();
        let notifications = std::rc::Rc::new(std::cell::Cell::new(0));
        let _observation = cx.update(|cx| {
            let notifications = notifications.clone();
            cx.observe(&desktop, move |_, _| {
                notifications.set(notifications.get() + 1)
            })
        });
        let before = desktop.read_with(cx, renders);
        let previous = desktop.read_with(cx, |view, _| view.publication.clone());
        for text in ["typing", "typing more", "typing more日本語🙂"] {
            window
                .update(cx, |_, window, cx| {
                    desktop.update(cx, |view, cx| {
                        view.composer.input.update(cx, |input, cx| {
                            input.set_value(text, window, cx);
                            // Deliver the same change event as typing. The offline
                            // fixture makes the rendered control read-only.
                            cx.emit(InputEvent::Change);
                        });
                    })
                })
                .unwrap();
            for _ in 0..20 {
                runtime.block_on(tokio::task::yield_now());
                cx.run_until_parked();
                // Store publications are adopted on a display frame, as on the platform.
                window
                    .update(cx, |_, window, cx| window.simulate_next_frame(cx))
                    .unwrap();
                cx.run_until_parked();
                if desktop.read_with(cx, |view, _| view.composer.text.pending.is_none()) {
                    break;
                }
            }
            desktop.read_with(cx, |view, _| {
                assert!(
                    view.composer.text.pending.is_none(),
                    "the real save completion must arrive"
                );
                assert_eq!(
                    view.draft().text,
                    text,
                    "the Store snapshot must contain the native input"
                );
            });
            let after = desktop.read_with(cx, renders);
            assert_eq!(
                after[0], before[0],
                "typing must reuse the actual sidebar render"
            );
            assert_eq!(
                after[1], before[1],
                "typing must reuse the actual conversation render"
            );
            assert!(
                after[2] > before[2],
                "the composer must reflect native input changes"
            );
            assert_eq!(
                &after[3..],
                &before[3..],
                "typing must reuse the open panel and header"
            );
            desktop.read_with(cx, |view, _| {
                use agent_core::state::Same;
                assert!(view.publication.list.same(&previous.list));
                assert!(view.publication.conversation.same(&previous.conversation));
                assert!(Arc::ptr_eq(&view.publication.context, &previous.context));
                assert!(Arc::ptr_eq(
                    &view.snapshot.files.workspace,
                    &previous.snapshot.files.workspace,
                ));
            });
            assert_eq!(
                notifications.get(),
                0,
                "native input, Store publication, and save completion must not notify Desktop observers"
            );
        }
        // Offline restoration clears running status. Supply the live status
        // fixture separately before checking streaming and animation reuse.
        desktop.update(cx, |view, cx| {
            let snapshot = Arc::make_mut(&mut view.snapshot);
            Arc::make_mut(snapshot.lists.threads.as_mut().unwrap()).data[0].status =
                agent_protocol::models::SessionStatus::Running;
            view.project_fixture();
            cx.notify();
        });
        cx.run_until_parked();
        let before = desktop.read_with(cx, renders);
        let previous = desktop.read_with(cx, |view, _| view.publication.clone());
        window
            .update(cx, |_, window, cx| {
                desktop.update(cx, |view, cx| {
                    let snapshot = Arc::make_mut(&mut view.snapshot);
                    let thread = Arc::make_mut(
                        Arc::make_mut(&mut snapshot.sessions.conversations)
                            .get_mut(&session)
                            .unwrap(),
                    );
                    let turn = Arc::make_mut(&mut thread.turns.as_mut().unwrap()[0]);
                    turn.items.as_mut().unwrap()[0] = Arc::new(Item::new(
                        "answer".into(),
                        Default::default(),
                        agent_protocol::items::ItemBody::AssistantText {
                            text: "Updated answer".into(),
                            phase: agent_protocol::items::AssistantPhase::Final,
                        },
                    ));
                    view.project_fixture();
                    view.publish_views(window, cx);
                });
            })
            .unwrap();
        cx.run_until_parked();
        let after = desktop.read_with(cx, renders);
        desktop.read_with(cx, |view, _| {
            use agent_core::state::Same;
            assert!(view.publication.list.same(&previous.list));
            assert!(!view.publication.conversation.same(&previous.conversation));
            assert!(Arc::ptr_eq(&view.publication.composer, &previous.composer));
            assert!(Arc::ptr_eq(&view.publication.context, &previous.context));
            assert!(Arc::ptr_eq(
                &view.snapshot.files.workspace,
                &previous.snapshot.files.workspace,
            ));
        });
        assert_eq!(after[0], before[0], "streaming must reuse the sidebar");
        assert!(
            after[1] > before[1],
            "streaming must redraw the conversation"
        );
        assert_eq!(after[2], before[2], "streaming must reuse the composer");
        assert_eq!(
            &after[3..],
            &before[3..],
            "streaming must reuse the open panel and header"
        );
        // Markdown finishes its own asynchronous measurement on the next frame.
        window
            .update(cx, |_, window, cx| window.simulate_next_frame(cx))
            .unwrap();
        cx.run_until_parked();
        let settled = desktop.read_with(cx, renders);
        for _ in 0..3 {
            assert!(
                window
                    .update(cx, |_, window, cx| window.simulate_next_frame(cx))
                    .unwrap()
                    > 0
            );
            cx.run_until_parked();
        }
        assert_eq!(
            desktop.read_with(cx, renders),
            settled,
            "animation must reuse every region"
        );
        let old_height = desktop.read_with(cx, |view, cx| view.composer_view.read(cx).height);
        window
            .update(cx, |_, window, cx| {
                desktop.update(cx, |view, cx| {
                    let text = "many lines\n".repeat(8);
                    view.composer.text.replace(&text);
                    view.composer
                        .input
                        .update(cx, |input, cx| input.set_value(text, window, cx));
                })
            })
            .unwrap();
        cx.run_until_parked();
        let new_height = desktop.read_with(cx, |view, cx| view.composer_view.read(cx).height);
        assert!(
            new_height > old_height,
            "the cached composer must grow with multiline input"
        );
        let before = notifications.get();
        let store = desktop.read_with(cx, |view, _| {
            view.session.as_ref().unwrap().host.store.clone()
        });
        runtime
            .block_on(store.dispatch(Intent::NewChat {
                cwd: "/next-project".into(),
            }))
            .unwrap();
        for _ in 0..20 {
            runtime.block_on(tokio::task::yield_now());
            cx.run_until_parked();
            window
                .update(cx, |_, window, cx| window.simulate_next_frame(cx))
                .unwrap();
            cx.run_until_parked();
            if desktop.read_with(cx, |view, _| {
                view.snapshot.pane.navigation.cwd == "/next-project"
            }) {
                break;
            }
        }
        desktop.read_with(cx, |view, cx| {
            assert_eq!(view.snapshot.pane.navigation.cwd, "/next-project");
            assert!(view.composer.input.read(cx).value().is_empty());
        });
        assert!(
            notifications.get() > before,
            "Store navigation must still notify Desktop observers"
        );
        assert!(
            VisualTestContext::from_window(window.into(), cx)
                .debug_bounds("workbench-header")
                .is_none()
        );
    }
}
