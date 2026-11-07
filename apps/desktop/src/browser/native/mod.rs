//! GPUI only supplies its native parent, layout and application lifetime.
use gpui_kit::*;
use native_browser::{Browser, Prepared};
use objc2::{MainThreadMarker, rc::Retained};
use objc2_app_kit::NSView;
use objc2_foundation::{NSPoint, NSRect, NSSize};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

struct NativeGlobal(Option<Browser>);
impl Global for NativeGlobal {}

fn parent(window: &Window) -> Option<Retained<NSView>> {
    MainThreadMarker::new()?;
    let RawWindowHandle::AppKit(handle) = window.window_handle().ok()?.as_raw() else {
        return None;
    };
    // SAFETY: GPUI's main-thread window retains its AppKit content view.
    unsafe { Retained::retain(handle.ns_view.as_ptr().cast::<NSView>()) }
}

pub fn init(window: &mut Window, cx: &mut App) {
    cx.set_global(NativeGlobal(None));
    let Some(parent) = parent(window) else {
        return;
    };
    let runtime = cx.global::<crate::Runtime>().clone();
    window.spawn(cx, async move |cx| {
        let prepared = runtime.handle.spawn(async {
            let host = crate::platform::discover_local_host().await?;
            Prepared::load(host.directory).await
        }).await;
        match prepared {
            Ok(Ok(prepared)) => { let _ = cx.update(|_, cx| {
                match Browser::new(prepared, &parent, runtime.handle, &runtime.closing) {
                    Ok(browser) => cx.set_global(NativeGlobal(Some(browser))),
                    Err(error) => tracing::error!(target:"agent", operation="browser_start", %error),
                }
            }); }
            _ => tracing::error!(target:"agent", operation="browser_start", "Cannot initialize the native browser owner"),
        }
    }).detach();
    window.on_window_should_close(cx, |_, cx| {
        shutdown(cx);
        true
    });
}

pub fn shutdown(cx: &mut App) {
    if cx.has_global::<NativeGlobal>()
        && let Some(owner) = cx.global_mut::<NativeGlobal>().0.take()
    {
        owner.shutdown();
    }
}

fn owner(cx: &App) -> Option<&Browser> {
    cx.try_global::<NativeGlobal>()
        .and_then(|global| global.0.as_ref())
}

pub fn renderer_id(cx: &App) -> String {
    owner(cx).map(Browser::renderer_id).unwrap_or_default()
}
pub fn hide(scope: &str, cx: &mut App) {
    if let Some(owner) = owner(cx) {
        owner.hide(scope);
    }
}
pub fn has_surface(scope: &str, renderer: &str, tab: &str, cx: &App) -> bool {
    owner(cx).is_some_and(|owner| owner.has_surface(scope, renderer, tab))
}
pub fn has_focus(scope: &str, tab: &str, cx: &App) -> bool {
    owner(cx).is_some_and(|owner| owner.has_focus(scope, tab))
}
pub fn focus_parent(cx: &App) {
    if let Some(owner) = owner(cx) {
        owner.focus_parent();
    }
}
pub fn layout(scope: &str, tab: &str, bounds: Bounds<Pixels>, window: &Window, cx: &mut App) {
    if let Some(owner) = owner(cx)
        && let Some(parent) = parent(window)
    {
        owner.layout(
            scope,
            tab,
            &parent,
            NSRect::new(
                NSPoint::new(
                    f64::from(bounds.left() / px(1.)),
                    f64::from(bounds.top() / px(1.)),
                ),
                NSSize::new(
                    f64::from(bounds.size.width / px(1.)),
                    f64::from(bounds.size.height / px(1.)),
                ),
            ),
        );
    }
}
