//! AppKit owns pane containment and resizing; each NSH engine observes the same application.
use crate::{Area, application::Application, shared::Shared};
use nsh_core::element::*;
use nsh_platform_macos::{AppOptions, AppSource, Root, run_app};
use std::{rc::Rc, sync::Arc};

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
    let (runtime, application) = match crate::storage::directory().and_then(Application::new) {
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
    let owner = application.clone();
    options.components.register(
        "bex.split",
        Rc::new(move |_| Box::new(crate::native::split::Split::new(owner.clone()))),
    );
    let result = run_app(
        AppSource::Rust(root(application.clone(), Area::Shell)),
        options,
    );
    if let Err(reason) = runtime.block_on(application.shutdown()) {
        eprintln!("{reason}");
        return std::process::ExitCode::FAILURE;
    }
    result
}
