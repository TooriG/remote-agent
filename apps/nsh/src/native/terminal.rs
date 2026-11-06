//! SwiftTerm owns rendering and native input; the native component only moves bytes.
use libloading::Library;
use nsh_core::JsonValue;
use objc2::{MainThreadMarker, rc::Retained};
use objc2_foundation::{NSBundle, NSString};
use serde_json::json;
use std::{ffi::c_void, mem::ManuallyDrop, path::PathBuf, ptr::NonNull, rc::Rc, sync::OnceLock};

#[cfg(target_os = "ios")]
use nsh_platform_ios::{ComponentContext, Components, NativeComponent};
#[cfg(target_os = "macos")]
use nsh_platform_macos::{ComponentContext, Components, NativeComponent};
#[cfg(target_os = "macos")]
use objc2_app_kit::NSView as PlatformView;
#[cfg(target_os = "ios")]
use objc2_ui_kit::UIView as PlatformView;

type Callback = unsafe extern "C" fn(*mut c_void, i32, *const u8, isize, i32, i32);

struct Bridge {
    _library: ManuallyDrop<Library>,
    create: unsafe extern "C" fn(*mut c_void, Callback) -> *mut c_void,
    view: unsafe extern "C" fn(*mut c_void) -> *mut PlatformView,
    input: unsafe extern "C" fn(*mut c_void, bool),
    feed: unsafe extern "C" fn(*mut c_void, *const u8, isize, i32, i32),
    release: unsafe extern "C" fn(*mut c_void),
}

// Swift/Objective-C metadata stays registered for the lifetime of the process.
// Keep its library loaded after the last terminal view has gone away.
static BRIDGE: OnceLock<Result<Bridge, String>> = OnceLock::new();

impl Bridge {
    fn load() -> Result<Self, String> {
        let directory = NSBundle::mainBundle()
            .privateFrameworksPath()
            .ok_or_else(|| "アプリ内のターミナル部品が見つかりません。".to_owned())?;
        let path = PathBuf::from(directory.to_string()).join("BexNshNative.framework/BexNshNative");
        // This is the application's own bundled, signed framework. Each symbol's
        // C ABI is declared beside its Swift implementation in apple/Sources.
        unsafe {
            // Even an incompatible framework has registered Swift/Objective-C metadata.
            // Do not unload it on an early symbol-lookup error either.
            let library =
                ManuallyDrop::new(Library::new(path).map_err(|reason| reason.to_string())?);
            Ok(Self {
                create: *library
                    .get(b"bex_nsh_terminal_create\0")
                    .map_err(|e| e.to_string())?,
                view: *library
                    .get(b"bex_nsh_terminal_view\0")
                    .map_err(|e| e.to_string())?,
                input: *library
                    .get(b"bex_nsh_terminal_input\0")
                    .map_err(|e| e.to_string())?,
                feed: *library
                    .get(b"bex_nsh_terminal_feed\0")
                    .map_err(|e| e.to_string())?,
                release: *library
                    .get(b"bex_nsh_terminal_release\0")
                    .map_err(|e| e.to_string())?,
                _library: library,
            })
        }
    }
}

struct ForeignTerminal {
    bridge: &'static Bridge,
    handle: NonNull<c_void>,
    context: Box<ComponentContext>,
    sequence: u64,
}

impl ForeignTerminal {
    fn new(context: ComponentContext) -> Result<Self, String> {
        let bridge = BRIDGE
            .get_or_init(Bridge::load)
            .as_ref()
            .map_err(Clone::clone)?;
        let mut context = Box::new(context);
        // The box's address remains stable until release has disconnected the delegate.
        let handle =
            unsafe { (bridge.create)((&mut *context as *mut ComponentContext).cast(), event) };
        let handle =
            NonNull::new(handle).ok_or_else(|| "ターミナルを作成できませんでした。".to_owned())?;
        Ok(Self {
            bridge,
            handle,
            context,
            sequence: 0,
        })
    }

    fn output(&mut self, chunks: &[agent_core::state::TerminalOutput]) {
        let before = self.sequence;
        for chunk in chunks.iter().filter(|chunk| chunk.sequence > before) {
            let size = chunk.reset_size;
            // Vec/slice lengths fit isize. Swift copies the bytes before this call returns.
            unsafe {
                (self.bridge.feed)(
                    self.handle.as_ptr(),
                    chunk.data.as_ptr(),
                    chunk.data.len() as isize,
                    size.map_or(0, |size| i32::from(size.cols)),
                    size.map_or(0, |size| i32::from(size.rows)),
                );
            }
            self.sequence = chunk.sequence;
        }
        if self.sequence > before {
            self.context.emit("ack", json!(self.sequence));
        }
    }
}

impl Drop for ForeignTerminal {
    fn drop(&mut self) {
        // Swift clears its delegate/callback before the context box can be dropped.
        unsafe {
            (self.bridge.release)(self.handle.as_ptr());
        }
    }
}

unsafe extern "C" fn event(
    context: *mut c_void,
    kind: i32,
    bytes: *const u8,
    count: isize,
    cols: i32,
    rows: i32,
) {
    // Swift calls synchronously on the main actor while ForeignTerminal retains this box.
    let context = unsafe { &*context.cast::<ComponentContext>() };
    match kind {
        1 if count > 0 && !bytes.is_null() => {
            let bytes = unsafe { std::slice::from_raw_parts(bytes, count as usize) };
            context.emit("input", json!(bytes));
        }
        2 => context.emit("resize", json!({"cols": cols, "rows": rows})),
        _ => {}
    }
}

struct Terminal {
    view: Retained<PlatformView>,
    native: Option<ForeignTerminal>,
}

impl Terminal {
    fn new(context: ComponentContext) -> Self {
        context.set_size(0.0, 0.0);
        match ForeignTerminal::new(context) {
            Ok(native) => {
                // Swift retains its native view until native.release; NSH also needs its own retain.
                let view =
                    unsafe { Retained::retain((native.bridge.view)(native.handle.as_ptr())) }
                        .expect("the Swift terminal owner always has a native view");
                Self {
                    view,
                    native: Some(native),
                }
            }
            Err(reason) => Self {
                view: failure(&reason),
                native: None,
            },
        }
    }
}

impl NativeComponent for Terminal {
    #[cfg(target_os = "macos")]
    fn view(&self) -> Retained<PlatformView> {
        self.view.clone()
    }

    #[cfg(target_os = "ios")]
    fn view(&self) -> &PlatformView {
        &self.view
    }

    fn set_prop(&mut self, name: &str, value: Option<&JsonValue>) {
        let Some(native) = &mut self.native else {
            return;
        };
        match name {
            "input" => unsafe {
                (native.bridge.input)(
                    native.handle.as_ptr(),
                    value.and_then(JsonValue::as_bool).unwrap_or(false),
                );
            },
            "output" => {
                if let Some(chunks) = value.and_then(|v| {
                    serde_json::from_value::<Vec<agent_core::state::TerminalOutput>>(v.clone()).ok()
                }) {
                    native.output(&chunks);
                }
            }
            _ => {}
        }
    }
}

fn failure(reason: &str) -> Retained<PlatformView> {
    let message = NSString::from_str(&format!("ターミナルを読み込めませんでした。\n{reason}"));
    #[cfg(target_os = "macos")]
    {
        let label =
            objc2_app_kit::NSTextField::labelWithString(&message, MainThreadMarker::new().unwrap());
        label.setSelectable(true);
        label.into_super().into_super()
    }
    #[cfg(target_os = "ios")]
    {
        let label = objc2_ui_kit::UILabel::new(MainThreadMarker::new().unwrap());
        label.setText(Some(&message));
        label.setNumberOfLines(0);
        label.into_super()
    }
}

pub(super) fn register(components: &Components) {
    components.register(
        "bex.terminal",
        Rc::new(|context| Box::new(Terminal::new(context))),
    );
}
