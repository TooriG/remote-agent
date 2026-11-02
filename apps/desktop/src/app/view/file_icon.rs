use super::*;

pub(super) fn file_icon(path: &str, tree: bool, cx: &App) -> AnyElement {
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
    img(SharedString::from(format!(
        "icons/file-icons/{name}{context}-{theme}.svg"
    )))
    .size(px(16.))
    .flex_shrink_0()
    .into_any_element()
}
