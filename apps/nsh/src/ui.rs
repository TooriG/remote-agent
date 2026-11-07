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

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Icon {
    ArrowDown,
    ArrowUp,
    Branch,
    #[cfg(target_os = "macos")]
    Check,
    ChevronDown,
    #[cfg(target_os = "macos")]
    ChevronRight,
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    Close,
    Copy,
    #[cfg(target_os = "macos")]
    Details,
    Diff,
    Ellipsis,
    #[cfg(target_os = "macos")]
    Replace,
    #[cfg(target_os = "macos")]
    FolderOpen,
    #[cfg(target_os = "macos")]
    FileText,
    #[cfg(target_os = "macos")]
    ExternalLink,
    Folder,
    FolderPlus,
    Loader,
    Merge,
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    Microphone,
    #[cfg(not(target_os = "macos"))]
    Monitor,
    NewChat,
    Pencil,
    #[cfg(target_os = "macos")]
    PanelLeftClose,
    #[cfg(target_os = "macos")]
    PanelLeftOpen,
    #[cfg(target_os = "macos")]
    PanelRight,
    Plus,
    #[cfg(target_os = "macos")]
    Refresh,
    #[cfg(target_os = "macos")]
    Settings,
    Stop,
}

impl Icon {
    /// The SF Symbol AppKit draws for this icon.
    #[cfg(target_os = "macos")]
    fn symbol(self) -> &'static str {
        match self {
            Self::ArrowDown => "arrow.down",
            Self::ArrowUp => "arrow.up",
            Self::Branch => "arrow.triangle.branch",
            Self::Check => "checkmark",
            Self::ChevronDown => "chevron.down",
            Self::ChevronRight => "chevron.right",
            Self::Close => "xmark",
            Self::Copy => "doc.on.doc",
            Self::Details => "list.bullet.rectangle",
            Self::Diff => "plusminus",
            Self::Ellipsis => "ellipsis",
            Self::Replace => "arrow.left.arrow.right",
            Self::FolderOpen => "folder",
            Self::FileText => "doc",
            Self::ExternalLink => "arrow.up.right.square",
            Self::Folder => "folder",
            Self::FolderPlus => "folder.badge.plus",
            Self::Loader => "progress.indicator",
            Self::Merge => "arrow.triangle.merge",
            Self::Microphone => "mic",
            Self::NewChat => "square.and.pencil",
            Self::Pencil => "pencil",
            Self::PanelLeftClose | Self::PanelLeftOpen => "sidebar.left",
            Self::PanelRight => "sidebar.right",
            Self::Plus => "plus",
            Self::Refresh => "arrow.clockwise",
            Self::Settings => "gearshape",
            Self::Stop => "stop.fill",
        }
    }

    fn markup(self) -> &'static str {
        match self {
            Self::ArrowDown => include_str!("../assets/icons/arrow-down.svg"),
            Self::ArrowUp => include_str!("../assets/icons/arrow-up.svg"),
            Self::Branch => include_str!("../../desktop/assets/branch.svg"),
            #[cfg(target_os = "macos")]
            Self::Check => include_str!("../assets/icons/check.svg"),
            Self::ChevronDown => include_str!("../assets/icons/chevron-down.svg"),
            #[cfg(target_os = "macos")]
            Self::ChevronRight => include_str!("../assets/icons/chevron-right.svg"),
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            Self::Close => include_str!("../assets/icons/x.svg"),
            Self::Copy => include_str!("../assets/icons/copy.svg"),
            #[cfg(target_os = "macos")]
            Self::Details => include_str!("../../desktop/assets/square-menu.svg"),
            Self::Diff => include_str!("../../desktop/assets/diff.svg"),
            Self::Ellipsis => include_str!("../assets/icons/ellipsis.svg"),
            #[cfg(target_os = "macos")]
            Self::Replace => include_str!("../assets/icons/replace.svg"),
            #[cfg(target_os = "macos")]
            Self::FolderOpen => include_str!("../assets/icons/folder-open.svg"),
            #[cfg(target_os = "macos")]
            Self::FileText => include_str!("../assets/icons/file-text.svg"),
            #[cfg(target_os = "macos")]
            Self::ExternalLink => include_str!("../assets/icons/external-link.svg"),
            Self::Folder => include_str!("../assets/icons/folder.svg"),
            Self::FolderPlus => include_str!("../../desktop/assets/folder-plus.svg"),
            Self::Loader => include_str!("../assets/icons/loader.svg"),
            Self::Merge => include_str!("../../desktop/assets/merge.svg"),
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            Self::Microphone => include_str!("../../desktop/assets/microphone.svg"),
            #[cfg(not(target_os = "macos"))]
            Self::Monitor => include_str!("../../desktop/assets/monitor.svg"),
            Self::NewChat => include_str!("../../desktop/assets/square-pen.svg"),
            Self::Pencil => include_str!("../../desktop/assets/pencil.svg"),
            #[cfg(target_os = "macos")]
            Self::PanelLeftClose => include_str!("../assets/icons/panel-left-close.svg"),
            #[cfg(target_os = "macos")]
            Self::PanelLeftOpen => include_str!("../assets/icons/panel-left-open.svg"),
            #[cfg(target_os = "macos")]
            Self::PanelRight => include_str!("../assets/icons/panel-right.svg"),
            Self::Plus => include_str!("../assets/icons/plus.svg"),
            #[cfg(target_os = "macos")]
            Self::Refresh => include_str!("../assets/icons/rotate-cw.svg"),
            #[cfg(target_os = "macos")]
            Self::Settings => include_str!("../assets/icons/settings.svg"),
            Self::Stop => include_str!("../../desktop/assets/stop.svg"),
        }
    }
}

/// An icon in the current text color.
pub(crate) fn icon(icon: Icon, size: f32) -> impl IntoElement {
    svg(ImageSrc::svg_markup(icon.markup()))
        .w(px(size))
        .h(px(size))
        .flex_shrink_0()
}

/// The activity indicator: a turning loader, as the desktop client's.
pub(crate) fn spinner() -> impl IntoElement {
    view()
        .w(px(14.0))
        .h(px(14.0))
        .animate_spin()
        .child(icon(Icon::Loader, 14.0))
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
        .prop("symbol", serde_json::json!(glyph.symbol()))
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
        .prop("symbol", serde_json::json!(glyph.symbol()))
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
