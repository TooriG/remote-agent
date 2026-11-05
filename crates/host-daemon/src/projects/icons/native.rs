use super::{IconSource, read_within};
use serde_json::Value;
use std::{
    cmp::Reverse,
    collections::VecDeque,
    path::{Path, PathBuf},
};

const MAX_DEPTH: usize = 8;
const MAX_ENTRIES: usize = 4096;

/// Read source assets only, with bounded traversal and no directory symlinks.
pub(super) fn sources(root: &Path) -> Vec<IconSource> {
    let mut pending = VecDeque::from([(PathBuf::new(), 0)]);
    let mut remaining = MAX_ENTRIES;
    let mut sources = Vec::new();
    while let Some((directory, depth)) = pending.pop_front() {
        let Ok(entries) = std::fs::read_dir(root.join(&directory)) else {
            continue;
        };
        let mut entries: Vec<_> = entries.take(remaining).filter_map(Result::ok).collect();
        remaining = remaining.saturating_sub(entries.len());
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let path = directory.join(name.as_ref());
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() && name.ends_with(".appiconset") {
                if let Some(source) = catalog(root, &path) {
                    sources.push((
                        if name == "AppIcon.appiconset" { 0 } else { 2 },
                        path,
                        source,
                    ));
                }
            } else if kind.is_dir() && depth < MAX_DEPTH && !ignored_directory(&name) {
                pending.push_back((path, depth + 1));
            } else if kind.is_file() && app_image(&directory, &name) {
                sources.push((1, path.clone(), IconSource::image(path)));
            }
        }
        if remaining == 0 {
            break;
        }
    }
    sources.sort_by(|left, right| {
        (left.0, left.1.components().count(), &left.1).cmp(&(
            right.0,
            right.1.components().count(),
            &right.1,
        ))
    });
    sources.into_iter().map(|(_, _, source)| source).collect()
}

fn ignored_directory(name: &str) -> bool {
    name.starts_with('.')
        || matches!(
            name,
            "target"
                | "build"
                | "dist"
                | "out"
                | "node_modules"
                | "vendor"
                | "Pods"
                | "Carthage"
                | "DerivedData"
        )
}

fn app_image(directory: &Path, name: &str) -> bool {
    let parent = directory
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if parent.starts_with("mipmap-") {
        return matches!(name, "ic_launcher.png" | "ic_launcher.webp");
    }
    matches!(parent, "assets" | "icons" | "resources" | "Resources")
        && matches!(
            name,
            "icon.png" | "icon.svg" | "logo.png" | "logo.svg" | "128x128.png"
        )
}

fn catalog(root: &Path, directory: &Path) -> Option<IconSource> {
    let bytes = read_within(root, &directory.join("Contents.json"))?;
    let contents: Value = serde_json::from_slice(&bytes).ok()?;
    let images: Vec<_> = contents
        .get("images")?
        .as_array()?
        .iter()
        .filter_map(|image| {
            let filename = image.get("filename")?.as_str()?;
            let dark = appearance(image)?;
            let size = image
                .get("size")
                .and_then(Value::as_str)
                .and_then(|size| size.split('x').next()?.parse::<f64>().ok())
                .unwrap_or(0.);
            let scale = image
                .get("scale")
                .and_then(Value::as_str)
                .and_then(|scale| scale.trim_end_matches('x').parse::<f64>().ok())
                .unwrap_or(1.);
            Some((dark, (size * scale) as u64, directory.join(filename)))
        })
        .collect();
    let paths = |dark| {
        let mut choices: Vec<_> = images
            .iter()
            .filter(|(appearance, _, _)| appearance.is_none_or(|value| value == dark))
            .collect();
        choices.sort_by_key(|(appearance, pixels, path)| {
            (appearance.is_none(), Reverse(*pixels), path)
        });
        choices
            .into_iter()
            .map(|(_, _, path)| path.clone())
            .collect::<Vec<_>>()
    };
    let source = IconSource {
        light: paths(false),
        dark: paths(true),
    };
    (!source.light.is_empty() || !source.dark.is_empty()).then_some(source)
}

fn appearance(image: &Value) -> Option<Option<bool>> {
    for appearance in image
        .get("appearances")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if appearance.get("appearance")?.as_str()? == "luminosity" {
            return match appearance.get("value")?.as_str()? {
                "dark" => Some(Some(true)),
                "light" => Some(Some(false)),
                _ => None,
            };
        }
    }
    Some(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_prefers_the_largest_matching_appearance_and_ignores_tinted_images() {
        let directory = tempfile::tempdir().unwrap();
        // Match resolve(), including the verbatim path prefix used on Windows.
        let root = directory.path().canonicalize().unwrap();
        let path = Path::new("App/Assets.xcassets/AppIcon.appiconset");
        std::fs::create_dir_all(root.join(path)).unwrap();
        std::fs::write(root.join(path).join("Contents.json"), serde_json::json!({"images":[
            {"filename":"small.png","size":"16x16","scale":"2x"},
            {"filename":"large.png","size":"1024x1024"},
            {"filename":"dark.png","size":"1024x1024","appearances":[{"appearance":"luminosity","value":"dark"}]},
            {"filename":"tinted.png","size":"1024x1024","appearances":[{"appearance":"luminosity","value":"tinted"}]}
        ]}).to_string()).unwrap();
        let source = catalog(&root, path).unwrap();
        assert_eq!(
            source.light,
            [path.join("large.png"), path.join("small.png")]
        );
        assert_eq!(
            source.dark,
            [
                path.join("dark.png"),
                path.join("large.png"),
                path.join("small.png")
            ]
        );
    }

    #[test]
    fn discovery_ignores_dependencies_builds_and_symlinked_directories() {
        let root = tempfile::tempdir().unwrap();
        for path in [
            "node_modules/App/assets",
            "vendor/App/assets",
            "target/assets",
            ".worktree/other/assets",
            "apps/desktop/assets",
        ] {
            std::fs::create_dir_all(root.path().join(path)).unwrap();
            std::fs::write(root.path().join(path).join("icon.png"), []).unwrap();
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.path().join("node_modules"), root.path().join("linked"))
            .unwrap();
        let icons = sources(root.path());
        assert_eq!(icons.len(), 1);
        assert_eq!(
            icons[0].light,
            [PathBuf::from("apps/desktop/assets/icon.png")]
        );
    }
    #[test]
    fn discovery_stops_before_deeply_nested_assets() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("a/b/c/d/e/f/g/h/i/j/assets");
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("icon.png"), []).unwrap();
        assert!(sources(root.path()).is_empty());
    }
}
