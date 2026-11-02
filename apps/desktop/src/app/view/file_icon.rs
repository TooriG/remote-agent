use super::*;

pub(super) fn ignored_ink(dark: bool) -> Hsla {
    rgb(if dark { 0x4a4a4e } else { 0xadadb1 }).into()
}

pub(super) fn file_icon(path: &str, tree: bool, ignored: bool, cx: &App) -> AnyElement {
    let name = agent_core::presentation::files::desktop_file_icon(path);
    // In a tree, T3's custom agents/pnpm glyphs inherit the row foreground.
    // Markdown's PierreEntryIcon gives them the default icon ink instead.
    let context = if tree && matches!(name, "t3-file-icon-agents" | "t3-file-icon-pnpm") {
        "-tree"
    } else {
        ""
    };
    let theme = if cx.theme().is_dark() {
        "dark"
    } else {
        "light"
    };
    let asset = SharedString::from(format!("icons/file-icons/{name}{context}-{theme}.svg"));
    if tree && ignored && !context.is_empty() {
        return Icon::default()
            .path(asset)
            .size(px(16.))
            .text_color(ignored_ink(cx.theme().is_dark()))
            .into_any_element();
    }
    img(asset).size(px(16.)).flex_shrink_0().into_any_element()
}
