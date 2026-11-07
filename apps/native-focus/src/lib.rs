//! A native child must release AppKit's keyboard destination when hidden or
//! when its surrounding client takes focus.
//! A separate crate keeps its AppKit test from building the desktop app.
#![cfg(target_os = "macos")]
use objc2_app_kit::NSView;
use objc2_foundation::NSObjectProtocol;

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
        // Native containers such as NSSplitView can refuse keyboard focus.
        // AppKit may report success while leaving the old responder in place.
        if has_focus(view) {
            window.makeFirstResponder(None);
        }
    }
}

/// AppKit converts a key into editing commands only for the first responder.
/// A synthesized key borrows it and returns it, so a hidden page that receives
/// remote input never keeps the PC's keyboard.
pub fn with_keyboard(input: &NSView, dispatch: impl FnOnce()) -> bool {
    let Some(window) = input.window() else {
        return false;
    };
    let previous = window.firstResponder();
    // A refusing view still reports success and leaves the window focused.
    window.makeFirstResponder(Some(input));
    let borrowed = window
        .firstResponder()
        .is_some_and(|responder| responder.isEqual(Some(input)));
    if borrowed {
        dispatch();
    }
    window.makeFirstResponder(previous.as_deref());
    borrowed
}

pub fn hide(view: &NSView) {
    // setHidden alone leaves the invisible view receiving keyboard input.
    release(view);
    view.setHidden(true);
}
