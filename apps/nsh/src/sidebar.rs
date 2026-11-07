//! The desktop sidebar: the window's leading column in the existing desktop client's layout.
use crate::{
    Connection,
    application::{Host, ListRequest, Navigation},
    ui::{self, Enabled, Icon, colors},
};
use nsh_core::prelude::*;
use serde_json::json;

pub(crate) const WIDTH: f32 = 272.0;
pub(crate) const HEADER_HEIGHT: f32 = 44.0;

pub(crate) fn sidebar(hosts: Memo<Vec<Host>>, compose: impl Fn() + 'static) -> impl IntoElement {
    let connection = use_context::<Connection>().unwrap();
    let query = signal(String::new());
    let navigate = move |action| {
        connection
            .application
            .with(|application| application.navigate(action))
    };
    let connected = move || connection.settings.with(|p| p.connected());
    view()
        .w(px(WIDTH))
        .flex_shrink_0()
        .bg(colors::SIDEBAR)
        .border_r_1()
        .border_r_color(colors::BORDER)
        .child(header(true, move || navigate(Navigation::ToggleSidebar)))
        .child(
            view()
                .pt_3()
                .px_3()
                .gap_3()
                // The desktop client keeps the menu's place while one Host is listed.
                .child(view().child(when(
                    move || {
                        hosts.with(|hosts| hosts.len() > 1)
                            || connection.state.with(|state| state.all_hosts)
                    },
                    move || connection_menu(hosts),
                )))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .child(
                            crate::native::search(query)
                                .prop("appearance", json!("plain"))
                                .flex_1()
                                .min_w_0(),
                        )
                        .child(
                            ui::icon_button(Icon::NewChat, "新しいチャット", compose)
                                .enabled(connected),
                        ),
                ),
        )
        .child(crate::threads::list(query, || {}))
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .px_3()
                .pt_2()
                .pb(px(20.0))
                .child(ui::icon_button(
                    Icon::Settings,
                    "設定を開く",
                    move || navigate(Navigation::Settings(true)),
                ))
                .child(ui::icon_button(
                    Icon::Refresh,
                    "会話を更新",
                    move || {
                        connection.application.with(|application| {
                            application.request_list(ListRequest::Refresh);
                        })
                    },
                )),
        )
}

/// The title bar's leading part: the app icon after the window controls, and the
/// sidebar's toggle.
pub(crate) fn header(expanded: bool, toggle: impl Fn() + 'static) -> View {
    use base64::Engine;
    let logo = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD
            .encode(include_bytes!("../../desktop/assets/icon.png"))
    );
    let (glyph, label) = if expanded {
        (Icon::PanelLeftClose, "サイドバーを閉じる")
    } else {
        (Icon::PanelLeftOpen, "サイドバーを開く")
    };
    view()
        .h(px(HEADER_HEIGHT))
        .flex_shrink_0()
        .flex_row()
        .items_center()
        .gap_2()
        .pl(px(88.0))
        .pr_3()
        .window_drag(true)
        .child(
            image(logo)
                .w(px(22.0))
                .h(px(22.0))
                .rounded(px(5.0))
                .flex_shrink_0(),
        )
        .child(expanded.then(|| view().flex_1()))
        .child(
            view()
                .window_drag(false)
                .child(ui::icon_button(glyph, label, toggle)),
        )
}

/// The conversation's title bar: its title and the right panel's toggle; the
/// sidebar's header when the sidebar is closed.
pub(crate) fn conversation_header(
    title: Memo<String>,
    sidebar: Memo<bool>,
    panel_open: impl Fn() -> bool + Copy + 'static,
    toggle_sidebar: impl Fn() + Copy + 'static,
    toggle_panel: impl Fn() + Copy + 'static,
) -> impl IntoElement {
    view()
        .h(px(HEADER_HEIGHT))
        .flex_shrink_0()
        .flex_row()
        .items_center()
        .gap_2()
        .px_4()
        .window_drag(true)
        .style_when(move || !sidebar.get(), |s| s.pl_0())
        .child(when(
            move || !sidebar.get(),
            move || header(false, toggle_sidebar),
        ))
        .child(
            text(move || title.get())
                .flex_1()
                .min_w_0()
                .text_size(px(14.0))
                .number_of_lines(1),
        )
        .child(when(
            move || !panel_open(),
            move || {
                view().window_drag(false).child(ui::icon_button(
                    Icon::PanelRight,
                    "右パネルを切り替え",
                    toggle_panel,
                ))
            },
        ))
}

/// The list's connection: all connections or one, chosen from a popup menu.
fn connection_menu(hosts: Memo<Vec<Host>>) -> impl IntoElement {
    let connection = use_context::<Connection>().unwrap();
    let all = move || connection.state.with(|state| state.all_hosts);
    let title = move || {
        if all() {
            return "すべての接続先".to_owned();
        }
        let selected = connection.host.get();
        hosts.with(|hosts| {
            hosts
                .iter()
                .find(|host| host.id == selected)
                .map(|host| host.name.clone())
                .unwrap_or_else(|| "このPC".into())
        })
    };
    let items = move || {
        let all = all();
        let selected = connection.host.get_untracked();
        let mut entries = vec![
            json!({"id": "", "title": "すべての接続先", "selected": all}),
            json!({"separator": true}),
        ];
        entries.extend(hosts.with_untracked(|hosts| {
            hosts
                .iter()
                .map(|host| json!({"id": host.id, "title": host.name, "selected": !all && host.id == selected}))
                .collect::<Vec<_>>()
        }));
        json!(entries)
    };
    view()
        .flex_row()
        .items_center()
        .justify_center()
        .gap_1()
        .h(px(32.0))
        .px_3()
        .rounded(px(ui::RADIUS))
        .text_color(colors::FOREGROUND)
        .role(AccessibilityRole::Button)
        .label("接続先")
        .hover(|s| s.bg(colors::GHOST_HOVER))
        .active(|s| s.bg(colors::SELECTED))
        .on_press(move || {
            ui::popup(&items(), move |id: &str| {
                connection.application.with(|application| {
                    if id.is_empty() {
                        application.select_all_hosts();
                    } else {
                        application.select_host(id.to_owned(), false);
                    }
                })
            })
        })
        .child(text(title).text_size(px(14.0)).number_of_lines(1))
        .child(ui::icon(Icon::ChevronDown, 14.0))
}
