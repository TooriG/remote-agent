//! One NSH surface draws the whole window; WebKit's pages are owned by a hosted container.
use crate::{Area, application::Application, shared::Shared};
use nsh_core::element::*;
use nsh_platform_macos::{AppOptions, AppSource, Root, run_app};
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

pub fn run() -> std::process::ExitCode {
    let (runtime, application, local_directory) =
        match crate::storage::directory().and_then(|root| {
            let local = crate::storage::local_host()?;
            let directory = local.as_ref().and_then(|host| host.local_directory.clone());
            Application::new(root, local).map(|(runtime, app)| (runtime, app, directory))
        }) {
            Ok(service) => service,
            Err(reason) => {
                eprintln!("{reason}");
                return std::process::ExitCode::FAILURE;
            }
        };
    nsh_platform_macos::set_components(crate::register_macos);
    let options = AppOptions {
        app_id: "com.bex.nsh-preview".into(),
        title: "Bex · NSH Preview".into(),
        size: (1360, 900),
        ..Default::default()
    };
    let browser = crate::native::browser_macos::Service::new(
        application.clone(),
        runtime.handle().clone(),
        local_directory,
    );
    crate::native::browser_macos::register(&options.components, browser.clone());
    let acceptance = match crate::acceptance::from_environment(
        objc2::MainThreadMarker::new().expect("the app starts on the main thread"),
    ) {
        Ok(acceptance) => acceptance,
        Err(reason) => {
            eprintln!("{reason}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let result = run_app(
        AppSource::Rust(root(application.clone(), Area::Shell)),
        options,
    );
    runtime.block_on(browser.shutdown().wait());
    if let Err(reason) = runtime.block_on(application.shutdown()) {
        eprintln!("{reason}");
        return std::process::ExitCode::FAILURE;
    }
    if acceptance.is_some_and(|passed| !passed.get()) {
        return std::process::ExitCode::FAILURE;
    }
    result
}
