#[cfg(target_os = "macos")]
fn main() -> std::process::ExitCode {
    use nsh_platform_macos::{AppOptions, AppSource, Root, run_app};
    nsh_platform_macos::set_components(agent_nsh::register_macos);
    run_app(
        AppSource::Rust(Root::of::<agent_nsh::App>()),
        AppOptions {
            app_id: "com.bex.nsh-preview".into(),
            title: "Bex · NSH Preview".into(),
            size: (1360, 900),
            ..Default::default()
        },
    )
}

#[cfg(target_os = "ios")]
fn main() {
    use nsh_platform_ios::{AppOptions, AppSource, Root, run_app};
    nsh_platform_ios::set_components(agent_nsh::register_ios);
    run_app(
        AppSource::Rust(Root::of::<agent_nsh::App>()),
        AppOptions::default(),
    );
}

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
fn main() {
    eprintln!("Build the NSH preview for macOS or the iPhone simulator.");
}
