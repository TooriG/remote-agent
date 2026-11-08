fn main() {
    let manifest = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    println!("cargo:rerun-if-changed=ios-simulator.entitlements");
    // `desktop`: the window shell the Mac and Windows apps share.
    println!("cargo::rustc-check-cfg=cfg(desktop)");
    if matches!(
        std::env::var("CARGO_CFG_TARGET_OS").as_deref(),
        Ok("macos" | "windows")
    ) {
        println!("cargo::rustc-cfg=desktop");
    }
    if std::env::var("TARGET").is_ok_and(|target| target.ends_with("-apple-ios-sim")) {
        // Xcode embeds Simulator entitlements in Mach-O. Keeping them out of
        // the ad-hoc macOS signature also keeps the Simulator binary launchable.
        println!(
            "cargo:rustc-link-arg-bin=agent-nsh=-Wl,-sectcreate,__TEXT,__entitlements,{}",
            manifest.join("ios-simulator.entitlements").display()
        );
    }
    file_icons(&manifest);
}

/// The desktop client's file glyphs (T3 Code's Pierre tree icons) in the app's
/// dark look, by the names Core's `desktop_file_icon` gives.
fn file_icons(manifest: &std::path::Path) {
    let directory = manifest.join("../desktop/assets/file-icons");
    println!("cargo:rerun-if-changed={}", directory.display());
    let mut icons = std::fs::read_dir(&directory)
        .unwrap()
        .filter_map(|entry| {
            let path = entry.unwrap().path();
            let name = path.file_name()?.to_str()?.strip_suffix("-dark.svg")?;
            Some((name.to_owned(), path.canonicalize().unwrap()))
        })
        .collect::<Vec<_>>();
    icons.sort();
    let arms = icons
        .iter()
        .map(|(name, path)| format!("        {name:?} => include_str!({:?}),\n", path.display()))
        .collect::<String>();
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    std::fs::write(
        out.join("file_icons.rs"),
        format!(
            "fn file_icon_markup(name: &str) -> Option<&'static str> {{\n    Some(match name {{\n{arms}        _ => return None,\n    }})\n}}\n"
        ),
    )
    .unwrap();
}
