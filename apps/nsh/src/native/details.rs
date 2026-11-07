//! SwiftUI owns disclosure, text selection, scrolling and accessibility.
use nsh_core::JsonValue;
use objc2::rc::Retained;
use std::{ffi::c_void, ptr::NonNull, rc::Rc, sync::OnceLock};

#[cfg(target_os = "ios")]
use nsh_platform_ios::{ComponentContext, Components, NativeComponent};
#[cfg(target_os = "macos")]
use nsh_platform_macos::{ComponentContext, Components, NativeComponent};
#[cfg(target_os = "macos")]
use objc2_app_kit::NSView as PlatformView;
#[cfg(target_os = "ios")]
use objc2_ui_kit::UIView as PlatformView;

struct Bridge {
    create: unsafe extern "C" fn() -> *mut c_void,
    view: unsafe extern "C" fn(*mut c_void) -> *mut PlatformView,
    update: unsafe extern "C" fn(*mut c_void, *const u8, isize),
    release: unsafe extern "C" fn(*mut c_void),
}

static BRIDGE: OnceLock<Result<Bridge, String>> = OnceLock::new();

impl Bridge {
    fn load() -> Result<Self, String> {
        // The declarations match the framework's C exports.
        unsafe {
            Ok(Self {
                create: super::apple::symbol(b"bex_nsh_details_create\0")?,
                view: super::apple::symbol(b"bex_nsh_details_view\0")?,
                update: super::apple::symbol(b"bex_nsh_details_update\0")?,
                release: super::apple::symbol(b"bex_nsh_details_release\0")?,
            })
        }
    }
}

struct Surface {
    bridge: &'static Bridge,
    handle: NonNull<c_void>,
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe { (self.bridge.release)(self.handle.as_ptr()) };
    }
}

struct Details {
    view: Retained<PlatformView>,
    surface: Option<Surface>,
}

impl Details {
    fn new(context: ComponentContext) -> Self {
        context.set_size(0.0, 0.0);
        let create = || {
            let bridge = BRIDGE
                .get_or_init(Bridge::load)
                .as_ref()
                .map_err(Clone::clone)?;
            let handle = NonNull::new(unsafe { (bridge.create)() })
                .ok_or_else(|| "表示を作成できませんでした。".to_owned())?;
            let surface = Surface { bridge, handle };
            // The Swift owner and NSH each retain the native host view.
            let view = unsafe { Retained::retain((bridge.view)(handle.as_ptr())) }
                .expect("the Swift details owner always has a native view");
            Ok::<_, String>((surface, view))
        };
        match create() {
            Ok((surface, view)) => Self {
                view,
                surface: Some(surface),
            },
            Err(reason) => Self {
                view: super::apple::failure(&reason),
                surface: None,
            },
        }
    }
}

impl NativeComponent for Details {
    #[cfg(target_os = "macos")]
    fn view(&self) -> Retained<PlatformView> {
        self.view.clone()
    }
    #[cfg(target_os = "ios")]
    fn view(&self) -> &PlatformView {
        &self.view
    }

    fn set_prop(&mut self, name: &str, value: Option<&JsonValue>) {
        if name == "data"
            && let Some(surface) = &self.surface
            && let Some(value) = value
            && let Ok(data) = serde_json::to_vec(value)
        {
            // Swift decodes synchronously and retains no borrowed bytes.
            unsafe {
                (surface.bridge.update)(
                    surface.handle.as_ptr(),
                    data.as_ptr(),
                    data.len() as isize,
                );
            }
        }
    }
}

pub(super) fn register(components: &Components) {
    components.register(
        "bex.details",
        Rc::new(|context| Box::new(Details::new(context))),
    );
}
