#[cfg(target_os = "macos")]
fn main() -> std::process::ExitCode {
    agent_nsh::run_macos()
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
