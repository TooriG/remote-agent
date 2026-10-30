mod agents;
mod composer;
mod conversation;
mod errors;
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
use gpui_kit::component::{
    resizable::{h_resizable, resizable_panel, v_resizable},
    sidebar::{Sidebar, SidebarItem, SidebarMenuItem},
};
pub(super) use sidebar::SidebarView;
use std::path::PathBuf;

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
        .child(
            skeleton::Skeleton::new()
                .size_full()
                .rounded_lg()
                .bg(background),
        )
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
        let panel = self.panels.get(self.panel_scope());
        let panel_open = panel.open;
        let panel_kind = panel.kind();
        let panel_layout = panel.layout;
        let active_surface = panel.active;
        let active = self.project_editor.is_none()
            && self.timeline.gallery.is_none()
            && !self.onboarding
            && !self.import_projects
            && panel_open
            && self.tab == Tab::Chat;
        let thread_id = (active && panel_kind == Panel::Agents)
            .then(|| self.snapshot.navigation.thread_id.clone())
            .flatten();
        if self.snapshot.observed_agents != thread_id {
            self.dispatch(Intent::WatchAgents { thread_id });
        }
        let composer_visible = self.project_editor.is_none()
            && self.timeline.gallery.is_none()
            && self.tab == Tab::Chat
            && (!panel_open
                || (!self.side_chat_mode
                    && panel_layout != PanelLayout::Expanded
                    && (panel_layout == PanelLayout::Bottom
                        || window.viewport_size().width >= px(1080.))));
        if !composer_visible {
            self.cancel_recording();
        }
        if active
            && let Some(id) = active_surface
            && let Some(connection) = self.session.as_ref()
            && let Some(PanelScope {
                draft: DraftKey::Session { session: thread },
                ..
            }) = self.panels.scope(id)
            && let Some(SurfaceResource::Pending { url }) = self.surface_resources.get(&id)
        {
            let browser = crate::browser::Browser::new(
                connection.store.clone(),
                thread,
                url.clone(),
                window,
                cx,
            );
            let observation = cx.observe(&browser, |_, _, cx| cx.notify());
            self.surface_resources.insert(
                id,
                SurfaceResource::Browser {
                    view: browser,
                    _observation: observation,
                },
            );
        }
        for (id, resource) in &self.surface_resources {
            match resource {
                SurfaceResource::Browser { view, .. } => view.update(cx, |view, _| {
                    view.set_visible(active && Some(*id) == active_surface);
                }),
                SurfaceResource::SideChat(chat) if !active || Some(*id) != active_surface => {
                    chat.update(cx, |chat, cx| chat.hide_surfaces(cx));
                }
                _ => (),
            }
        }
        if self.timeline.gallery.is_some() {
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
    fn conversation_header(&self, cx: &Context<Self>) -> Div {
        let panel_open = self.panels.get(self.panel_scope()).open;
        let title = self
            .thread()
            .and_then(|thread| thread.name.as_deref())
            .or_else(|| {
                self.snapshot
                    .threads
                    .as_ref()
                    .and_then(|page| {
                        page.data
                            .iter()
                            .find(|thread| thread.id.as_ref() == self.selected())
                    })
                    .and_then(|thread| thread.name.as_deref())
            })
            .unwrap_or("新しいチャット")
            .to_owned();
        let mut header = h_flex()
            .debug_selector(|| "conversation-header".into())
            .h(px(crate::WINDOW_HEADER_HEIGHT))
            .flex_shrink_0()
            .px_4()
            .gap_2();
        if !self.sidebar {
            header = header.pl_0().child(sidebar_header(false, cx));
        }
        header
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .text_ellipsis()
                    .child(title),
            )
            .when(!panel_open, |header| {
                header.child(Self::panel_toggle(false, cx))
            })
    }

    fn panel_toggle(open: bool, cx: &Context<Self>) -> Button {
        Self::icon_button(
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
            .child(self.chat(cx))
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
                self.workbench(false, cx)
            } else {
                self.chat(cx)
            }
        } else if panel_open {
            let right = self.workbench(
                panel_layout == PanelLayout::Expanded
                    || (panel_layout == PanelLayout::Right && !wide),
                cx,
            );
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

impl Desktop {
    fn button(
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        cx: &Context<Self>,
        action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
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
    pub(super) fn icon_button(
        id: impl Into<SharedString>,
        icon: impl Into<Icon>,
        label: impl Into<SharedString>,
        cx: &Context<Self>,
        action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
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
