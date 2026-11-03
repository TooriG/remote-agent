use gpui_kit::{
    App,
    component::{Theme, ThemeMode},
    px, rgb,
};

pub(crate) const BACKGROUND: u32 = 0x080808;
pub(crate) const SIDEBAR: u32 = 0x000000;
pub(crate) const SURFACE: u32 = 0x0f0f0f;
pub(crate) const RAISED: u32 = 0x141414;
pub(crate) const HOVER: u32 = 0x202020;
pub(crate) const SELECTED: u32 = 0x282828;
pub(crate) const BORDER: u32 = 0x303030;
pub(crate) const FOREGROUND: u32 = 0xe6e6e6;
pub(crate) const MUTED: u32 = 0xa6a6a6;
pub(crate) const ACCENT: u32 = 0x346bf1;

#[cfg(target_os = "windows")]
pub(crate) fn style_native_titlebar(window: &gpui_kit::Window) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::{
        Foundation::HWND,
        Graphics::Dwm::{
            DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR,
            DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute,
        },
    };

    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };
    let hwnd = HWND(handle.hwnd.get() as *mut std::ffi::c_void);
    // Native Windows chrome does not inherit the GPUI theme. Explicit colors
    // keep the system accent color out of the app's monochrome title bar.
    let colorref = |rgb: u32| ((rgb & 0xff) << 16) | (rgb & 0xff00) | ((rgb >> 16) & 0xff);
    for (attribute, value) in [
        (DWMWA_USE_IMMERSIVE_DARK_MODE, 1u32),
        (DWMWA_CAPTION_COLOR, colorref(BACKGROUND)),
        (DWMWA_BORDER_COLOR, colorref(BORDER)),
        (DWMWA_TEXT_COLOR, colorref(FOREGROUND)),
    ] {
        // SAFETY: the live window owns HWND; each attribute takes a four-byte
        // BOOL or COLORREF, and DWM copies the value during this call.
        if let Err(error) = unsafe {
            DwmSetWindowAttribute(
                hwnd,
                attribute,
                (&value as *const u32).cast(),
                std::mem::size_of_val(&value) as u32,
            )
        } {
            tracing::warn!(?attribute, %error, "could not style native title bar");
        }
    }
}

pub(crate) fn init(cx: &mut App) {
    gpui_kit::init(cx);
    Theme::change(ThemeMode::Dark, None, cx);
    let theme = Theme::global_mut(cx);
    theme.colors.background = rgb(BACKGROUND).into();
    theme.colors.foreground = rgb(FOREGROUND).into();
    theme.colors.border = rgb(BORDER).into();
    theme.colors.input = rgb(0x666666).into();
    theme.colors.caret = rgb(FOREGROUND).into();
    theme.colors.accent = rgb(SELECTED).into();
    theme.colors.accent_foreground = rgb(FOREGROUND).into();
    theme.colors.muted = rgb(RAISED).into();
    theme.colors.muted_foreground = rgb(MUTED).into();
    theme.colors.secondary = rgb(RAISED).into();
    theme.colors.secondary_hover = rgb(HOVER).into();
    theme.colors.secondary_active = rgb(SELECTED).into();
    theme.colors.secondary_foreground = rgb(FOREGROUND).into();
    theme.colors.primary = rgb(ACCENT).into();
    theme.colors.primary_hover = rgb(0x477af5).into();
    theme.colors.primary_active = rgb(0x285bd5).into();
    theme.colors.primary_foreground = rgb(FOREGROUND).into();
    theme.colors.button_primary = rgb(ACCENT).into();
    theme.colors.button_primary_hover = rgb(0x477af5).into();
    theme.colors.button_primary_active = rgb(0x285bd5).into();
    theme.colors.button_primary_foreground = rgb(FOREGROUND).into();
    theme.colors.button_secondary = rgb(RAISED).into();
    theme.colors.button_secondary_hover = rgb(HOVER).into();
    theme.colors.button_secondary_active = rgb(SELECTED).into();
    theme.colors.button_secondary_foreground = rgb(FOREGROUND).into();
    theme.colors.ring = rgb(ACCENT).into();
    theme.colors.selection = rgb(ACCENT).opacity(0.25).into();
    theme.colors.button = rgb(RAISED).into();
    theme.colors.button_hover = rgb(HOVER).into();
    theme.colors.button_active = rgb(SELECTED).into();
    theme.colors.button_foreground = rgb(FOREGROUND).into();
    theme.colors.sidebar = rgb(SIDEBAR).into();
    theme.colors.sidebar_foreground = rgb(MUTED).into();
    theme.colors.sidebar_accent = rgb(SELECTED).into();
    theme.colors.sidebar_accent_foreground = rgb(FOREGROUND).into();
    theme.colors.sidebar_border = rgb(BORDER).into();
    theme.colors.sidebar_primary = rgb(SELECTED).into();
    theme.colors.sidebar_primary_foreground = rgb(FOREGROUND).into();
    theme.colors.popover = rgb(SURFACE).into();
    theme.colors.popover_foreground = rgb(FOREGROUND).into();
    theme.colors.list = rgb(SURFACE).into();
    theme.colors.list_hover = rgb(HOVER).into();
    theme.colors.list_active = rgb(SELECTED).into();
    theme.colors.list_active_border = rgb(BORDER).into();
    theme.colors.list_even = rgb(RAISED).into();
    theme.colors.list_head = rgb(SURFACE).into();
    theme.colors.tab = rgb(BACKGROUND).into();
    theme.colors.tab_bar = rgb(BACKGROUND).into();
    theme.colors.tab_bar_segmented = rgb(SURFACE).into();
    theme.colors.tab_active = rgb(SELECTED).into();
    theme.colors.tab_active_foreground = rgb(FOREGROUND).into();
    theme.colors.tab_foreground = rgb(MUTED).into();
    theme.colors.table = rgb(BACKGROUND).into();
    theme.colors.table_even = rgb(SURFACE).into();
    theme.colors.table_head = rgb(RAISED).into();
    theme.colors.table_hover = rgb(HOVER).into();
    theme.colors.table_active = rgb(SELECTED).into();
    theme.colors.table_active_border = rgb(BORDER).into();
    theme.colors.table_head_foreground = rgb(MUTED).into();
    theme.colors.table_foot = rgb(SURFACE).into();
    theme.colors.table_foot_foreground = rgb(MUTED).into();
    theme.colors.table_row_border = rgb(BORDER).into();
    theme.colors.title_bar = rgb(BACKGROUND).into();
    theme.colors.title_bar_border = rgb(BORDER).into();
    theme.colors.window_border = rgb(BORDER).into();
    theme.colors.accordion = rgb(SURFACE).into();
    theme.colors.group_box = rgb(SURFACE).into();
    theme.colors.group_box_foreground = rgb(MUTED).into();
    theme.colors.skeleton = rgb(RAISED).into();
    theme.colors.scrollbar_thumb = rgb(0x333333).into();
    theme.colors.scrollbar_thumb_hover = rgb(0x454545).into();
    // Components and Base both keep resolved copies of the palette.
    theme.tokens = theme.colors.into();
    theme.radius = px(8.);
    Theme::sync_base(cx);
}
