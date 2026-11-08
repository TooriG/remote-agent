#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(desktop)]
fn main() -> std::process::ExitCode {
    // The updater's commands run before any window or Host connection.
    if agent_local_host::install::run_command(agent_core::updates::build_number()) {
        return std::process::ExitCode::SUCCESS;
    }
    agent_nsh::run_desktop()
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

#[cfg(not(any(desktop, target_os = "ios")))]
fn main() {
    eprintln!("Build the NSH client for macOS, Windows or the iPhone simulator.");
}
