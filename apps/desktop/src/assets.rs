use gpui_kit::{AssetSource, SharedString};
use std::borrow::Cow;

#[derive(rust_embed::RustEmbed)]
#[folder = "assets/"]
struct Icons;

pub(crate) struct DesktopAssets;
impl DesktopAssets {
    const SHARED: &[(&str, &[u8])] = &[
        (
            "icons/openai.svg",
            include_bytes!("../../mobile/iosApp/Bex/Assets.xcassets/openai.imageset/openai.svg"),
        ),
        (
            "icons/anthropic.svg",
            include_bytes!(
                "../../mobile/iosApp/Bex/Assets.xcassets/anthropic.imageset/anthropic.svg"
            ),
        ),
        (
            "icons/claude.svg",
            include_bytes!("../../mobile/iosApp/Bex/Assets.xcassets/claude.imageset/claude.svg"),
        ),
    ];
}
impl AssetSource for DesktopAssets {
    fn load(&self, path: &str) -> gpui_kit::Result<Option<Cow<'static, [u8]>>> {
        if let Some((_, bytes)) = Self::SHARED.iter().find(|(name, _)| *name == path) {
            return Ok(Some(Cow::Borrowed(*bytes)));
        }
        if let Some(icon) = path.strip_prefix("icons/").and_then(Icons::get) {
            return Ok(Some(icon.data));
        }
        gpui_kit::assets::Assets.load(path)
    }
    fn list(&self, path: &str) -> gpui_kit::Result<Vec<SharedString>> {
        let mut paths = gpui_kit::assets::Assets.list(path)?;
        paths.extend(
            Self::SHARED
                .iter()
                .map(|(name, _)| (*name).to_owned())
                .chain(Icons::iter().map(|name| format!("icons/{name}")))
                .filter(|name| name.starts_with(path))
                .map(SharedString::from),
        );
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_glyphs_and_tree_controls_are_embedded_without_filesystem_access() {
        for path in [
            "AGENTS.md",
            "pnpm-lock.yaml",
            "app.tsx",
            "README.md",
            "build.rs",
            "video.mp4",
            "a.pdf",
            "unknown",
        ] {
            let icon = agent_core::presentation::files::desktop_file_icon(path);
            for theme in ["light", "dark"] {
                let path = format!("icons/file-icons/{icon}-{theme}.svg");
                assert!(
                    matches!(DesktopAssets.load(&path).unwrap(), Some(Cow::Borrowed(_))),
                    "{path}"
                );
            }
        }
        for path in [
            "pencil.svg",
            "trash.svg",
            "chevrons-down-up.svg",
            "file-icons/chevron-down.svg",
            "file-icons/chevron-right.svg",
            "file-icons/loading.svg",
        ] {
            assert!(
                DesktopAssets
                    .load(&format!("icons/{path}"))
                    .unwrap()
                    .is_some(),
                "{path}"
            );
        }
        for path in DesktopAssets.list("icons/file-icons/").unwrap() {
            assert!(DesktopAssets.load(&path).unwrap().is_some(), "{path}");
        }
    }
}
