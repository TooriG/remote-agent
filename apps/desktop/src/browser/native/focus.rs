//! GPUI focus and AppKit's first responder are separate. A native child must
//! return keyboard input to its parent when hidden or when GPUI is clicked.
use objc2_app_kit::NSView;

pub fn has_focus(view: &NSView) -> bool {
    view.window()
        .and_then(|window| window.firstResponder())
        .and_then(|responder| responder.downcast::<NSView>().ok())
        .is_some_and(|responder| responder.isDescendantOf(view))
}

pub fn release(view: &NSView) {
    if has_focus(view)
        && let Some(window) = view.window()
        // SAFETY: the main-thread view hierarchy stays alive while retaining
        // the parent; no hierarchy mutation occurs before makeFirstResponder.
        && let Some(parent) = (unsafe { view.superview() })
    {
        window.makeFirstResponder(Some(&parent));
    }
}

pub fn hide(view: &NSView) {
    // setHidden alone leaves the invisible view receiving keyboard input.
    release(view);
    view.setHidden(true);
}
