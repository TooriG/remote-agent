//! One NSH surface draws the whole window: AppKit's on the Mac, Win32's on Windows.
//! On the Mac, WebKit's pages are owned by a hosted container.
use crate::{Area, application::Application, shared::Shared};
use nsh_core::element::*;
#[cfg(target_os = "macos")]
use nsh_platform_macos as platform;
#[cfg(windows)]
use nsh_platform_windows as platform;
use platform::{AppOptions, AppSource, Root, run_app};
use std::sync::Arc;

#[derive(Clone, PartialEq)]
struct ViewProps {
    application: Option<Shared<Application>>,
    area: Area,
}

// This is a Rust-only embedding root. The shared owner never crosses a JSON boundary.
impl Props for ViewProps {
    fn spec(_: &mut TsTypes) -> Vec<PropSpec> {
        Vec::new()
    }
    fn initial() -> Self {
        Self {
            application: None,
            area: Area::Shell,
        }
    }
    fn set(&mut self, name: &str, value: Option<AnyValue>) -> Result<bool, PropError> {
        ().set(name, value)
    }
    fn set_slot(&mut self, _: &str, _: Option<Slot>) -> bool {
        false
    }
    fn set_children(&mut self, _: Children) -> bool {
        false
    }
}

struct ViewRoot;
impl Component for ViewRoot {
    type Props = ViewProps;
    type Event = NoEvent;
    fn view(props: ViewProps, _: Scope<NoEvent>) -> impl IntoElement {
        let content = crate::app_view(
            props.application.expect("native root has an application").0,
            props.area,
        );
        view()
            .flex_1()
            .text_color(move || nsh_base::theme().foreground)
            .child(content)
    }
}

pub(super) fn root(application: Arc<Application>, area: Area) -> Root {
    Root::new::<ViewRoot>(move || ViewProps {
        application: Some(Shared(application.clone())),
        area,
    })
}

/// Ends the app's event loop once an update's worker waits to replace it.
pub(crate) fn quit() {
    #[cfg(target_os = "macos")]
    nsh_platform_macos::quit_app();
    // Windows' app ends with its only window.
    #[cfg(windows)]
    nsh_core::element::desktop::close_window();
}

pub fn run() -> std::process::ExitCode {
    // The app's data follows its Host's scope, so a source build keeps apart
    // from the installed app as their Hosts do.
    let (runtime, application, local_directory) =
        match agent_local_host::launch::HostScope::from_environment(cfg!(debug_assertions))
            .and_then(|scope| Ok((scope.resolve()?.directory, scope)))
            .map_err(crate::storage::error)
            .and_then(|(directory, scope)| {
                Application::new(scope.directory.join("nsh-client"), Some(scope))
                    .map(|(runtime, app)| (runtime, app, Some(directory)))
            }) {
            Ok(service) => service,
            Err(reason) => return fail(&reason.to_string()),
        };
    #[cfg(target_os = "macos")]
    nsh_platform_macos::set_components(crate::native::macos::register);
    let options = AppOptions {
        app_id: "app.bex.desktop".into(),
        title: "Bex".into(),
        size: (1360, 900),
        // Closing hides the window; the Dock reopens it, and Quit stops the app.
        #[cfg(target_os = "macos")]
        keep_alive_on_close: true,
        ..Default::default()
    };
    #[cfg(target_os = "macos")]
    let browser = {
        let browser = crate::native::browser_macos::Service::new(
            application.clone(),
            runtime.handle().clone(),
            local_directory,
        );
        crate::native::browser_macos::register(&options.components, browser.clone());
        browser
    };
    // Windows' pages are the Host's rendered images; no local renderer starts.
    #[cfg(windows)]
    let _ = local_directory;
    #[cfg(all(target_os = "macos", debug_assertions))]
    let acceptance = match crate::acceptance::from_environment(
        objc2::MainThreadMarker::new().expect("the app starts on the main thread"),
    ) {
        Ok(acceptance) => acceptance,
        Err(reason) => return fail(&reason),
    };
    let result = run_app(
        AppSource::Rust(root(application.clone(), Area::Shell)),
        options,
    );
    #[cfg(target_os = "macos")]
    runtime.block_on(browser.shutdown().wait());
    if let Err(reason) = runtime.block_on(application.shutdown()) {
        return fail(&reason.to_string());
    }
    #[cfg(all(target_os = "macos", debug_assertions))]
    if acceptance.is_some_and(|passed| !passed.get()) {
        return std::process::ExitCode::FAILURE;
    }
    result
}

/// A failure before or after the window: on the console on the Mac, in a
/// message box on Windows, whose app has no console.
fn fail(reason: &str) -> std::process::ExitCode {
    #[cfg(target_os = "macos")]
    eprintln!("{reason}");
    #[cfg(windows)]
    nsh_platform_windows::fatal_error(reason);
    std::process::ExitCode::FAILURE
}
