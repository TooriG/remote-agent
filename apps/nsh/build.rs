fn main() {
    println!("cargo:rerun-if-changed=ios-simulator.entitlements");
    if std::env::var("TARGET").is_ok_and(|target| target.ends_with("-apple-ios-sim")) {
        // Xcode embeds Simulator entitlements in Mach-O. Keeping them out of
        // the ad-hoc macOS signature also keeps the Simulator binary launchable.
        let path = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
            .join("ios-simulator.entitlements");
        println!(
            "cargo:rustc-link-arg-bin=agent-nsh=-Wl,-sectcreate,__TEXT,__entitlements,{}",
            path.display()
        );
    }
}
