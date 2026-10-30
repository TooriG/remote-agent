mod agents;
mod composer;
mod conversation;
mod errors;
mod header;
mod markdown_diagram;
mod media;
mod model_settings;
mod onboarding;
mod permissions;
mod project_management;
mod settings;
mod sidebar;
mod workbench;
use super::*;
use agent_core::{presentation::error::error_message, state::operations as op};
use base64::Engine;
pub(super) use composer::ComposerView;
pub(super) use conversation::ConversationView;
use gpui_kit::component::{
    resizable::{h_resizable, resizable_panel, v_resizable},
    sidebar::{Sidebar, SidebarItem, SidebarMenuItem},
};
pub(super) use header::HeaderView;
pub(super) use sidebar::SidebarView;
use std::path::PathBuf;
pub(super) use workbench::WorkbenchView;

// Popup contents can change while their trigger remains identical. The
// observation belongs to the open popup and is dropped with its element state.
fn observe_popup_desktop(
    desktop: &WeakEntity<Desktop>,
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
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_owned()
}

const CHAT_WIDTH: f32 = 780.;

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

fn sidebar_header(expanded: bool, cx: &Context<Desktop>) -> Div {
    let (id, icon, label) = if expanded {
        (
            "collapse-sidebar",
            IconName::PanelLeftClose,
            "サイドバーを閉じる",
        )
    } else {
        (
            "expand-sidebar",
            IconName::PanelLeftOpen,
            "サイドバーを開く",
        )
    };
    h_flex()
        .h(px(crate::WINDOW_HEADER_HEIGHT))
        .flex_shrink_0()
        .pl(px(88.))
        .gap_2()
        .child(div().flex_shrink_0().text_sm().font_semibold().child("bex"))
        .when(expanded, |header| {
            header.w_full().pr_3().child(div().flex_1())
        })
        .child(
            Button::new(id)
                .debug_selector(move || id.into())
                .icon(icon)
                .small()
                .ghost()
                .tooltip(label)
                .accessibility_label(label)
                .on_click(cx.listener(move |desktop, _, _, cx| {
                    desktop.sidebar = !expanded;
                    cx.notify();
                })),
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

fn review_counts(additions: Option<u64>, deletions: Option<u64>) -> AnyElement {
    let counts = h_flex().gap_1().text_xs();
    match (additions, deletions) {
        (Some(added), Some(deleted)) => counts
            .child(div().text_color(rgb(0x37cf77)).child(format!("+{added}")))
            .child(div().text_color(rgb(0xff6259)).child(format!("−{deleted}")))
            .into_any_element(),
        _ => counts
            .text_color(rgb(appearance::MUTED))
            .child("バイナリ")
            .into_any_element(),
    }
}

fn conversation_file_path(source: &str, cwd: &str) -> Result<PathBuf, String> {
    agent_core::presentation::markdown::markdown_file_target(source.into(), cwd.into())
        .map(|file| PathBuf::from(file.path))
        .ok_or_else(|| "ファイルパスが不正です".into())
}

impl Render for Desktop {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.media.gallery.is_some() {
            let gallery = self.image_gallery_view(window, cx);
            return h_flex()
                .size_full()
                .bg(rgb(appearance::BACKGROUND))
                .text_color(rgb(appearance::FOREGROUND))
                .text_size(px(14.))
                .child(gallery);
        }
        if self.import_projects || (self.onboarding && self.tab == Tab::Chat) {
            return self.onboarding_view(window, cx);
        }
        let main = if self.project_editor.is_some() {
            self.project_editor_view(cx)
        } else {
            match self.tab {
                Tab::Chat => self.conversation_content(window.viewport_size().width, cx),
                Tab::Settings => self.settings(cx),
            }
        };
        h_flex()
            .relative()
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
    }
}

impl Desktop {
    fn panel_toggle(open: bool, cx: &Context<Self>) -> Button {
        icon_button(
            "panel-toggle",
            IconName::PanelRight,
            "右パネルを切り替え",
            cx,
            move |view, window, cx| {
                let key = view.panel_scope();
                view.panels.set_open(key.clone(), !open);
                if !open && view.panels.get(key.clone()).active.is_none() {
                    view.panel_focus.focus(window, cx);
                }
            },
        )
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
                    div()
                        .w(px(300.))
                        .flex_shrink_0()
                        .child(self.workspace_card(cx)),
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
            self.conversation_main(width >= px(1280.), cx)
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
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_kit as gpui;

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
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let app_runtime = Runtime {
            handle: runtime.handle().clone(),
            connections: Arc::default(),
            closing: tokio_util::task::TaskTracker::new(),
            logging_error: None,
        };
        cx.update(|cx| {
            crate::appearance::init(cx);
            cx.set_global(app_runtime.clone());
        });
        let mut snapshot = Snapshot {
            connected: true,
            ..Default::default()
        };
        Arc::make_mut(&mut snapshot.navigation).draft_key = DraftKey::Session {
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
    fn open_model_menu_updates_when_its_trigger_is_unchanged(cx: &mut TestAppContext) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        cx.update(|cx| {
            crate::appearance::init(cx);
            cx.set_global(Runtime {
                handle: runtime.handle().clone(),
                connections: Arc::default(),
                closing: tokio_util::task::TaskTracker::new(),
                logging_error: None,
            });
        });
        let mut owner = None;
        let (_, window) = cx.add_window_view(|window, cx| {
            let desktop = cx.new(|cx| {
                let mut desktop = Desktop::new(Mode::Main, window, cx);
                desktop.onboarding = false;
                let key = desktop.draft_key().clone();
                Arc::make_mut(&mut Arc::make_mut(&mut desktop.snapshot).drafts).insert(
                    key,
                    Arc::new(Draft {
                        model: Some(agent_protocol::models::ModelRef {
                            provider: agent_protocol::session::ProviderKind::Codex,
                            id: "gpt".into(),
                        }),
                        ..Default::default()
                    }),
                );
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
            Arc::make_mut(&mut desktop.snapshot).models = Arc::new(
                serde_json::from_value(serde_json::json!([
                    {"id":"extra","model":{"provider":"codex","id":"extra"},"displayName":"Extra"}
                ]))
                .unwrap(),
            );
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
            window.debug_bounds("model-accounts-manage").is_some(),
            "account details must replace the model catalog inside the open popup"
        );
    }

    #[gpui::test]
    fn typing_streaming_and_animation_rebuild_only_the_affected_views(cx: &mut TestAppContext) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        cx.update(|cx| {
            crate::appearance::init(cx);
            cx.set_global(Runtime {
                handle: runtime.handle().clone(),
                connections: Arc::default(),
                closing: tokio_util::task::TaskTracker::new(),
                logging_error: None,
            });
        });
        let session = SessionRef {
            provider: agent_protocol::session::ProviderKind::Codex,
            id: "selected".into(),
        };
        let mut desktop = None;
        let window = cx.open_window(size(px(1600.), px(700.)), |window, cx| {
            let view = cx.new(|cx| {
                let mut view = Desktop::new(Mode::Main, window, cx);
                view.onboarding = false;
                view.panels.open(view.panel_scope(), Panel::Files, None);
                let snapshot = Arc::make_mut(&mut view.snapshot);
                snapshot.connected = true;
                snapshot.composer_catalog = Some(Arc::new(agent_protocol::composer::ComposerCatalog::default()));
                Arc::make_mut(&mut snapshot.navigation).thread_id = Some(session.clone());
                Arc::make_mut(&mut snapshot.navigation).draft_key = session.clone().into();
                snapshot.threads = Some(Arc::new(serde_json::from_value(serde_json::json!({
                    "data":[{"id":{"provider":"codex","id":"selected"},"name":"Task","status":"running"}],
                    "projects":[],"hasMore":false,"hasMoreProjects":false,"projectPages":{},"limit":5,
                })).unwrap()));
                Arc::make_mut(&mut snapshot.conversations).insert(session.clone(), Arc::new(serde_json::from_value(serde_json::json!({
                    "id":{"provider":"codex","id":"selected"},
                    "turns":[{"id":"turn","status":"completed","items":[{"id":"answer","body":{"inline":{"body":{"assistantText":{"text":"Original answer","phase":"final"}}}}}]}]
                })).unwrap()));
                let store = runtime.block_on(async {
                    Arc::new(agent_core::store::Store::offline((*view.snapshot).clone()))
                });
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
                    receive.recv().await.unwrap().unwrap()
                }));
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
            Arc::make_mut(snapshot.threads.as_mut().unwrap()).data[0].status =
                agent_protocol::models::SessionStatus::Running;
            cx.notify();
        });
        cx.run_until_parked();
        let before = desktop.read_with(cx, renders);
        desktop.update(cx, |view, cx| {
            let snapshot = Arc::make_mut(&mut view.snapshot);
            let thread = Arc::make_mut(
                Arc::make_mut(&mut snapshot.conversations)
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
            cx.notify();
        });
        cx.run_until_parked();
        let after = desktop.read_with(cx, renders);
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
        let store = desktop.read_with(cx, |view, _| view.session.as_ref().unwrap().store.clone());
        runtime
            .block_on(store.dispatch(Intent::NewChat {
                cwd: "/next-project".into(),
            }))
            .unwrap();
        for _ in 0..20 {
            runtime.block_on(tokio::task::yield_now());
            cx.run_until_parked();
            if desktop.read_with(cx, |view, _| {
                view.snapshot.navigation.cwd == "/next-project"
            }) {
                break;
            }
        }
        desktop.read_with(cx, |view, cx| {
            assert_eq!(view.snapshot.navigation.cwd, "/next-project");
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
