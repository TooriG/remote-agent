//! The app's controls, drawn with NSH's views in the existing desktop client's look.
//! Text input, scrolling, menus, alerts and web views stay the platform's.
use nsh_core::{
    element::{ImageSrc, Rgba, desktop::MenuItem, rgb, rgba},
    prelude::*,
};
use serde_json::Value;

pub(crate) mod colors {
    use super::{Rgba, rgb, rgba};

    #[cfg(target_os = "macos")]
    pub(crate) const BACKGROUND: Rgba = rgb(0x080808);
    pub(crate) const SIDEBAR: Rgba = rgb(0x000000);
    pub(crate) const SURFACE: Rgba = rgb(0x0f0f0f);
    pub(crate) const RAISED: Rgba = rgb(0x141414);
    pub(crate) const SELECTED: Rgba = rgb(0x282828);
    pub(crate) const BORDER: Rgba = rgb(0x303030);
    pub(crate) const FOREGROUND: Rgba = rgb(0xe6e6e6);
    pub(crate) const MUTED: Rgba = rgb(0xa6a6a6);
    #[cfg(target_os = "macos")]
    pub(crate) const ACCENT: Rgba = rgb(0x346bf1);
    /// A ghost control under the pointer: the selection fill at half opacity.
    pub(crate) const GHOST_HOVER: Rgba = rgba(0x28282880);
    /// A sidebar row under the pointer.
    pub(crate) const ROW_HOVER: Rgba = rgba(0x282828cc);
}

pub(crate) const RADIUS: f32 = 6.0;

/// An icon: its markup in the current text color and, on the Mac, the SF Symbol
/// AppKit draws for it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Icon {
    markup: &'static str,
    #[cfg(target_os = "macos")]
    symbol: &'static str,
}

macro_rules! icon {
    ($file:literal) => {
        Icon {
            markup: include_str!($file),
        }
    };
    ($file:literal, $symbol:literal) => {
        Icon {
            markup: include_str!($file),
            #[cfg(target_os = "macos")]
            symbol: $symbol,
        }
    };
}

impl Icon {
    pub(crate) const ARROW_DOWN: Self = icon!("../assets/icons/arrow-down.svg", "arrow.down");
    pub(crate) const ARROW_UP: Self = icon!("../assets/icons/arrow-up.svg", "arrow.up");
    pub(crate) const FORK: Self = icon!("../../desktop/assets/branch.svg", "arrow.triangle.branch");
    #[cfg(target_os = "macos")]
    pub(crate) const CHECK: Self = icon!("../assets/icons/check.svg", "checkmark");
    pub(crate) const CHEVRON_DOWN: Self = icon!("../assets/icons/chevron-down.svg", "chevron.down");
    pub(crate) const CHEVRON_RIGHT: Self =
        icon!("../assets/icons/chevron-right.svg", "chevron.right");
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub(crate) const CLOSE: Self = icon!("../assets/icons/x.svg", "xmark");
    pub(crate) const COPY: Self = icon!("../assets/icons/copy.svg", "doc.on.doc");
    #[cfg(target_os = "macos")]
    pub(crate) const DETAILS: Self = icon!(
        "../../desktop/assets/square-menu.svg",
        "list.bullet.rectangle"
    );
    pub(crate) const DIFF: Self = icon!("../../desktop/assets/diff.svg", "plusminus");
    pub(crate) const ELLIPSIS: Self = icon!("../assets/icons/ellipsis.svg", "ellipsis");
    pub(crate) const WRENCH: Self = icon!("../assets/icons/wrench.svg", "wrench.adjustable");
    pub(crate) const TERMINAL: Self =
        icon!("../assets/icons/square-terminal.svg", "apple.terminal");
    pub(crate) const FILE_PEN: Self = icon!("../assets/icons/file-pen.svg", "square.and.pencil");
    pub(crate) const BRAIN: Self = icon!("../assets/icons/brain.svg", "brain");
    #[cfg(target_os = "macos")]
    pub(crate) const REPLACE: Self = icon!("../assets/icons/replace.svg", "arrow.left.arrow.right");
    #[cfg(target_os = "macos")]
    pub(crate) const FOLDER_OPEN: Self = icon!("../assets/icons/folder-open.svg", "folder");
    #[cfg(target_os = "macos")]
    pub(crate) const FILE_TEXT: Self = icon!("../assets/icons/file-text.svg", "doc");
    #[cfg(target_os = "macos")]
    pub(crate) const EXTERNAL_LINK: Self =
        icon!("../assets/icons/external-link.svg", "arrow.up.right.square");
    pub(crate) const FOLDER: Self = icon!("../assets/icons/folder.svg", "folder");
    pub(crate) const FOLDER_PLUS: Self =
        icon!("../../desktop/assets/folder-plus.svg", "folder.badge.plus");
    pub(crate) const LOADER: Self = icon!("../assets/icons/loader.svg", "progress.indicator");
    pub(crate) const MERGE: Self = icon!("../../desktop/assets/merge.svg", "arrow.triangle.merge");
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub(crate) const MICROPHONE: Self = icon!("../../desktop/assets/microphone.svg", "mic");
    #[cfg(not(target_os = "macos"))]
    pub(crate) const MONITOR: Self = icon!("../../desktop/assets/monitor.svg");
    pub(crate) const NEW_CHAT: Self =
        icon!("../../desktop/assets/square-pen.svg", "square.and.pencil");
    pub(crate) const PENCIL: Self = icon!("../../desktop/assets/pencil.svg", "pencil");
    #[cfg(target_os = "macos")]
    pub(crate) const PANEL_LEFT_CLOSE: Self =
        icon!("../assets/icons/panel-left-close.svg", "sidebar.left");
    #[cfg(target_os = "macos")]
    pub(crate) const PANEL_LEFT_OPEN: Self =
        icon!("../assets/icons/panel-left-open.svg", "sidebar.left");
    #[cfg(target_os = "macos")]
    pub(crate) const PANEL_RIGHT: Self = icon!("../assets/icons/panel-right.svg", "sidebar.right");
    pub(crate) const PLUS: Self = icon!("../assets/icons/plus.svg", "plus");
    #[cfg(target_os = "macos")]
    pub(crate) const REFRESH: Self = icon!("../assets/icons/rotate-cw.svg", "arrow.clockwise");
    #[cfg(target_os = "macos")]
    pub(crate) const SETTINGS: Self = icon!("../assets/icons/settings.svg", "gearshape");
    #[cfg(target_os = "macos")]
    pub(crate) const SLIDERS: Self = icon!("../assets/icons/settings-2.svg", "slider.horizontal.3");
    #[cfg(target_os = "macos")]
    pub(crate) const FOLDER_TREE: Self = icon!("../assets/icons/folder-tree.svg", "folder");
    #[cfg(target_os = "macos")]
    pub(crate) const SERVER: Self = icon!("../assets/icons/server.svg", "server.rack");
    #[cfg(target_os = "macos")]
    pub(crate) const BOT: Self = icon!("../assets/icons/bot.svg", "cpu");
    pub(crate) const STEER: Self = icon!("../assets/icons/redo-2.svg", "arrow.uturn.forward");
    pub(crate) const STOP: Self = icon!("../../desktop/assets/stop.svg", "stop.fill");
}

impl Icon {
    /// A work item's tool, as T3 Code's work log shows it.
    pub(crate) fn work(kind: &str) -> Self {
        match kind {
            "command" => Self::TERMINAL,
            "fileChange" => Self::FILE_PEN,
            "reasoning" => Self::BRAIN,
            _ => Self::WRENCH,
        }
    }
}

/// An icon in the current text color.
pub(crate) fn icon(icon: Icon, size: f32) -> impl IntoElement {
    svg(ImageSrc::svg_markup(icon.markup))
        .w(px(size))
        .h(px(size))
        .flex_shrink_0()
}

/// A card of rows divided by rules, as T3 Code's settings groups.
pub(crate) fn card(rows: Vec<View>) -> View {
    rows.into_iter().enumerate().fold(
        view()
            .rounded(px(12.0))
            .border_1()
            .border_color(colors::BORDER)
            .bg(colors::SURFACE),
        |group, (index, row)| {
            group.child(if index == 0 {
                row
            } else {
                row.border_t_1().border_t_color(colors::BORDER)
            })
        },
    )
}

/// A setting: its title and description, and its control at the end.
pub(crate) fn setting(
    title: impl IntoReactive<SharedString>,
    description: impl Fn() -> String + 'static,
    control: impl IntoElement,
) -> View {
    view()
        .flex_row()
        .items_center()
        .gap_4()
        .px_5()
        .py_4()
        .child(
            view()
                .flex_1()
                .min_w_0()
                .gap_1()
                .child(
                    text(title)
                        .text_size(px(14.0))
                        .font_weight(FontWeight::MEDIUM),
                )
                .child(
                    text(description)
                        .text_size(px(13.0))
                        .text_color(colors::MUTED)
                        .selectable(true),
                ),
        )
        .child(control)
}

/// The activity indicator: a turning loader, as the desktop client's.
pub(crate) fn spinner() -> impl IntoElement {
    view()
        .w(px(14.0))
        .h(px(14.0))
        .animate_spin()
        .child(icon(Icon::LOADER, 14.0))
}

/// A control: nsh-base's button (focus, keys, accessibility) in the desktop client's
/// ghost look, its content filling it.
#[cfg(not(target_os = "macos"))]
pub(crate) type Control = nsh_base::ButtonElement;

#[cfg(not(target_os = "macos"))]
fn ghost(label: impl IntoReactive<SharedString>, content: View) -> Control {
    nsh_base::button()
        .accessibility_label(nsh_core::element::Prop::from(
            label.into_reactive().map(Some),
        ))
        .flex_shrink_0()
        .flex_row()
        .items_center()
        .justify_center()
        .gap_2()
        .rounded(px(RADIUS))
        .text_color(colors::FOREGROUND)
        .hover(|s| s.bg(colors::GHOST_HOVER))
        .active(|s| s.bg(colors::SELECTED))
        .disabled_style(|s| s.opacity(0.5))
        .child(content.flex_row().items_center().min_w_0())
}

/// A small ghost icon button (24 pt, a 14 pt icon) with its label as tooltip.
#[cfg(not(target_os = "macos"))]
pub(crate) fn icon_button(icon: Icon, label: &'static str, press: impl Fn() + 'static) -> Control {
    icon_button_sized(icon, label, 24.0, 14.0, press)
}

/// An extra-small ghost icon button (20 pt, a 12 pt icon).
#[cfg(not(target_os = "macos"))]
pub(crate) fn small_icon_button(
    icon: Icon,
    label: &'static str,
    press: impl Fn() + 'static,
) -> Control {
    icon_button_sized(icon, label, 20.0, 12.0, press)
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn icon_button_sized(
    glyph: Icon,
    label: &'static str,
    size: f32,
    icon_size: f32,
    press: impl Fn() + 'static,
) -> Control {
    ghost(
        label,
        view().tooltip(tooltip(label)).child(icon(glyph, icon_size)),
    )
    .w(px(size))
    .h(px(size))
    .on_press(press)
}

/// A hint over a control, as the desktop client's tooltips.
#[cfg(not(target_os = "macos"))]
fn tooltip(label: &'static str) -> AnyElement {
    view()
        .px_2()
        .py_1()
        .rounded(px(RADIUS))
        .bg(colors::SURFACE)
        .border_1()
        .border_color(colors::BORDER)
        .child(
            text(label)
                .text_size(px(12.0))
                .text_color(colors::FOREGROUND),
        )
        .into_element()
}

/// A small ghost text button.
#[cfg(not(target_os = "macos"))]
pub(crate) fn text_button(
    title: impl IntoReactive<SharedString>,
    press: impl Fn() + 'static,
) -> Control {
    let title = title.into_reactive();
    ghost(
        title.clone(),
        view().child(text(title).text_size(px(14.0)).number_of_lines(1)),
    )
    .h(px(24.0))
    .px_2()
    .on_press(press)
}

/// A ghost button with an icon and a label (32 pt), as the composer's folder.
#[cfg(not(target_os = "macos"))]
pub(crate) fn labeled_button(
    glyph: Icon,
    title: impl Fn() -> String + Copy + 'static,
    press: impl Fn() + 'static,
) -> Control {
    ghost(
        move || SharedString::from(title()),
        view()
            .gap_2()
            .min_w_0()
            .child(icon(glyph, 16.0))
            .child(text(title).text_size(px(14.0)).number_of_lines(1).min_w_0()),
    )
    .h(px(32.0))
    .px_2()
    .min_w_0()
    .max_w(px(260.0))
    .on_press(press)
}

pub(crate) trait Enabled {
    fn enabled(self, enabled: impl Fn() -> bool + Copy + 'static) -> Self;
}

impl Enabled for nsh_base::ButtonElement {
    fn enabled(self, enabled: impl Fn() -> bool + Copy + 'static) -> Self {
        self.disabled(move || !enabled())
    }
}

/// On the Mac a control is AppKit's own: a borderless `NSButton` with an SF Symbol,
/// placed and sized by NSH.
#[cfg(target_os = "macos")]
pub(crate) type Control = nsh_core::element::NativeView;

#[cfg(target_os = "macos")]
fn appkit_button(label: impl IntoReactive<SharedString>, press: impl Fn() + 'static) -> Control {
    let label = label.into_reactive();
    native("bex.button")
        .prop(
            "label",
            label
                .clone()
                .map(|label| serde_json::json!(label.to_string())),
        )
        .prop(
            "tooltip",
            label.map(|label| serde_json::json!(label.to_string())),
        )
        .prop("appearance", serde_json::json!("plain"))
        .on_native_event(move |event: &NativeEvent| {
            if &*event.name == "press" {
                press();
            }
        })
}

#[cfg(target_os = "macos")]
pub(crate) fn icon_button(icon: Icon, label: &'static str, press: impl Fn() + 'static) -> Control {
    icon_button_sized(icon, label, 24.0, 14.0, press)
}

#[cfg(target_os = "macos")]
pub(crate) fn small_icon_button(
    icon: Icon,
    label: &'static str,
    press: impl Fn() + 'static,
) -> Control {
    icon_button_sized(icon, label, 20.0, 12.0, press)
}

#[cfg(target_os = "macos")]
pub(crate) fn icon_button_sized(
    glyph: Icon,
    label: &'static str,
    size: f32,
    icon_size: f32,
    press: impl Fn() + 'static,
) -> Control {
    appkit_button(label, press)
        .prop("symbol", serde_json::json!(glyph.symbol))
        .prop("symbolSize", serde_json::json!(icon_size))
        .w(px(size))
        .h(px(size))
}

#[cfg(target_os = "macos")]
pub(crate) fn text_button(
    title: impl IntoReactive<SharedString>,
    press: impl Fn() + 'static,
) -> Control {
    let title = title.into_reactive();
    appkit_button(title.clone(), press).prop(
        "title",
        title.map(|title| serde_json::json!(title.to_string())),
    )
}

#[cfg(target_os = "macos")]
pub(crate) fn labeled_button(
    glyph: Icon,
    title: impl Fn() -> String + Copy + 'static,
    press: impl Fn() + 'static,
) -> Control {
    appkit_button(move || SharedString::from(title()), press)
        .prop("symbol", serde_json::json!(glyph.symbol))
        .prop("symbolSize", serde_json::json!(16.0))
        .prop("title", move || serde_json::json!(title()))
        .min_w_0()
        .max_w(px(260.0))
}

#[cfg(target_os = "macos")]
impl Enabled for Control {
    fn enabled(self, enabled: impl Fn() -> bool + Copy + 'static) -> Self {
        self.prop("enabled", move || serde_json::json!(enabled()))
    }
}

/// Shows `items` (`{id, title, enabled, selected}`, `{separator}`, `{title, children}`)
/// as the platform's popup menu and reports the chosen id.
pub(crate) fn popup(items: &Value, select: impl Fn(&str) + Clone + 'static) {
    let items = menu_items(items, &select);
    drop(nsh_core::element::desktop::popup_menu(items));
}

fn menu_items(items: &Value, select: &(impl Fn(&str) + Clone + 'static)) -> Vec<MenuItem> {
    items
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|entry| {
                    let title = entry["title"].as_str().unwrap_or_default();
                    if entry["separator"].as_bool().unwrap_or(false) {
                        MenuItem::separator()
                    } else if let Some(children) = entry.get("children") {
                        MenuItem::submenu(title, menu_items(children, select))
                    } else {
                        let id = entry["id"].as_str().unwrap_or_default().to_owned();
                        let select = select.clone();
                        MenuItem::new(title)
                            .disabled(!entry["enabled"].as_bool().unwrap_or(true))
                            .checked(entry["selected"].as_bool().unwrap_or(false))
                            .on_select(move || select(&id))
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}
