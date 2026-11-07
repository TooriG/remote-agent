use crate::{Area, application::Application};
use nsh_core::JsonValue;
use nsh_platform_macos::{AppOptions, AppView, NativeComponent};
use objc2::{MainThreadMarker, rc::Retained};
use objc2_app_kit::NSView;
use std::{ffi::c_void, ptr::NonNull, rc::Rc, sync::Arc};

struct Bridge {
    create: unsafe extern "C" fn(*mut NSView, *mut NSView, *mut NSView) -> *mut c_void,
    view: unsafe extern "C" fn(*mut c_void) -> *mut NSView,
    update: unsafe extern "C" fn(*mut c_void, bool, bool, *const u8, isize),
    release: unsafe extern "C" fn(*mut c_void),
}

struct Surface {
    bridge: Bridge,
    handle: NonNull<c_void>,
}
impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            (self.bridge.release)(self.handle.as_ptr());
        }
    }
}

pub(crate) struct Split {
    view: Retained<NSView>,
    surface: Option<Surface>,
    // Retain every engine until its native controller has been released.
    views: Vec<Rc<AppView>>,
    sidebar: bool,
    tools: bool,
    layout: String,
}

impl Drop for Split {
    fn drop(&mut self) {
        for view in &self.views {
            view.stop();
        }
    }
}

impl Split {
    pub fn new(application: Arc<Application>, browser: Rc<super::browser_macos::Service>) -> Self {
        let mut views = Vec::new();
        let result = (|| {
            let bridge = unsafe {
                Bridge {
                    create: super::apple::symbol(b"bex_nsh_split_create\0")?,
                    view: super::apple::symbol(b"bex_nsh_split_view\0")?,
                    update: super::apple::symbol(b"bex_nsh_split_update\0")?,
                    release: super::apple::symbol(b"bex_nsh_split_release\0")?,
                }
            };

            for area in [Area::Sidebar, Area::Conversation, Area::Workbench] {
                let options = AppOptions::default();
                super::browser_macos::register(&options.components, browser.clone());
                let view = AppView::new(MainThreadMarker::new().unwrap(), options);
                view.load(crate::desktop::root(application.clone(), area).into());
                views.push(view);
            }
            let handle = NonNull::new(unsafe {
                (bridge.create)(
                    Retained::as_ptr(&views[0].view()).cast_mut(),
                    Retained::as_ptr(&views[1].view()).cast_mut(),
                    Retained::as_ptr(&views[2].view()).cast_mut(),
                )
            })
            .ok_or_else(|| "分割ビューを作成できませんでした。".to_owned())?;
            let view = unsafe { Retained::retain((bridge.view)(handle.as_ptr())) }
                .expect("AppKit split controller owns a view");
            browser.attach(&view);
            Ok::<_, String>((Surface { bridge, handle }, view))
        })();
        let (surface, view) = match result {
            Ok((surface, view)) => (Some(surface), view),
            Err(reason) => (None, super::apple::failure(&reason)),
        };
        Self {
            view,
            surface,
            views,
            sidebar: true,
            tools: false,
            layout: "Right".into(),
        }
    }
}

impl NativeComponent for Split {
    fn view(&self) -> Retained<NSView> {
        self.view.clone()
    }
    fn set_prop(&mut self, name: &str, value: Option<&JsonValue>) {
        match name {
            "sidebar" => self.sidebar = value.and_then(JsonValue::as_bool).unwrap_or(true),
            "tools" => self.tools = value.and_then(JsonValue::as_bool).unwrap_or(false),
            "layout" => self.layout = value.and_then(JsonValue::as_str).unwrap_or("Right").into(),
            _ => return,
        }
        if let Some(surface) = &self.surface {
            unsafe {
                (surface.bridge.update)(
                    surface.handle.as_ptr(),
                    self.sidebar,
                    self.tools,
                    self.layout.as_ptr(),
                    self.layout.len() as isize,
                );
            }
        }
    }
}
