//! The image viewer's zoom by hand on the Mac: a trackpad's pinch and a wheel
//! with ⌘ (or Control) held over the image. NSH's root hands the view under the
//! pointer its wheels and pinches; the rest of its wheels go up the responder
//! chain to the root, which scrolls the image as it would without the view.
use agent_core::presentation::media::ZoomGesture;
use nsh_core::JsonValue;
use nsh_platform_macos::{ComponentContext, NativeComponent};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained};
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSResponder, NSView};
use objc2_core_foundation::CGRect;
use objc2_foundation::NSObject;

/// A touchpad's precise scroll, in points, that counts as one wheel notch.
const POINTS_PER_NOTCH: f64 = 48.0;

define_class!(
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = ComponentContext]
    struct ZoomView;
    impl ZoomView {
        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, event: &NSEvent) {
            let zooms = event.modifierFlags().intersects(NSEventModifierFlags::Command | NSEventModifierFlags::Control);
            // AppKit's deltas move the content (positive: down, toward its start).
            let notches = if event.hasPreciseScrollingDeltas() {
                -event.scrollingDeltaY() / POINTS_PER_NOTCH
            } else {
                -event.scrollingDeltaY()
            };
            if zooms && notches != 0.0 {
                self.zoomed(ZoomGesture::Wheel { notches: notches as f32 });
            } else {
                // SAFETY: NSResponder's own handling passes it up to NSH's root.
                unsafe { msg_send![super(self), scrollWheel: event] }
            }
        }
        #[unsafe(method(magnifyWithEvent:))]
        fn magnify(&self, event: &NSEvent) {
            self.zoomed(ZoomGesture::Pinch { magnification: event.magnification() as f32 });
        }
    }
);

impl ZoomView {
    fn zoomed(&self, gesture: ZoomGesture) {
        self.ivars().emit(
            "zoom",
            serde_json::to_value(gesture).expect("a gesture is plain numbers"),
        );
    }
}

pub(super) struct ZoomArea(Retained<ZoomView>);

impl ZoomArea {
    pub(super) fn new(context: ComponentContext) -> Self {
        let view = ZoomView::alloc(MainThreadMarker::new().unwrap()).set_ivars(context);
        Self(unsafe { msg_send![super(view), initWithFrame: CGRect::ZERO] })
    }
}

impl NativeComponent for ZoomArea {
    fn view(&self) -> Retained<NSView> {
        self.0.clone().into_super()
    }

    fn set_prop(&mut self, _: &str, _: Option<&JsonValue>) {}
}
