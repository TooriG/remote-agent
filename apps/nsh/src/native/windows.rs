//! Win32 input NSH's views do not report: the wheel and touchpad scrolls over
//! a view whose content is not an NSH scroll view (a Host's page image, the
//! terminal's grid), which NSH would otherwise only use to scroll.
use super::Scroll;
use nsh_platform_windows::{ComponentContext, Components, NativeComponent, Wheel};
use std::rc::Rc;

pub(crate) fn register(components: &Components) {
    components.register("bex.wheel", Rc::new(|context| Box::new(WheelArea(context))));
}

/// Draws nothing; its view keeps presses and moves, it takes the wheel.
struct WheelArea(ComponentContext);

impl NativeComponent for WheelArea {
    fn set_prop(&mut self, _: &str, _: Option<&serde_json::Value>) {}

    fn wheel(&mut self, event: &Wheel) -> bool {
        let scroll = Scroll {
            x: event.x,
            y: event.y,
            dx: event.dx,
            dy: event.dy,
            shift: event.shift,
            ctrl: event.ctrl,
            alt: event.alt,
        };
        self.0.emit(
            "wheel",
            serde_json::to_value(scroll).expect("a scroll is plain numbers"),
        );
        true
    }
}
