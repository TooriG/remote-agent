//! Quick Look owns the panel, preview controls and window lifecycle.
use nsh_core::JsonValue;
use nsh_platform_macos::{ComponentContext, NativeComponent};
use objc2::{
    DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained,
    runtime::ProtocolObject, sel,
};
use objc2_app_kit::{NSResponder, NSView, NSWindowDelegate};
use objc2_core_foundation::CGRect;
use objc2_foundation::{
    NSInteger, NSObject, NSObjectNSThreadPerformAdditions, NSObjectProtocol, NSString, NSURL,
};
use objc2_quick_look_ui::{
    QLPreviewItem, QLPreviewPanel, QLPreviewPanelDataSource, QLPreviewPanelDelegate,
};
use std::cell::{Cell, RefCell};

struct State {
    context: ComponentContext,
    path: RefCell<String>,
    live: Cell<bool>,
}
define_class!(
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = State]
    struct PreviewView;
    unsafe impl NSObjectProtocol for PreviewView {}
    impl PreviewView {
        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool { true }
        #[unsafe(method(acceptsPreviewPanelControl:))]
        fn accepts(&self, _: &QLPreviewPanel) -> bool { self.ivars().live.get() && !self.ivars().path.borrow().is_empty() }
        #[unsafe(method(beginPreviewPanelControl:))]
        fn begin(&self, panel: &QLPreviewPanel) {
            unsafe {
                panel.setDataSource(Some(ProtocolObject::from_ref(self)));
                panel.setDelegate(Some(self));
            }
        }
        #[unsafe(method(endPreviewPanelControl:))]
        fn end(&self, panel: &QLPreviewPanel) {
            unsafe { panel.setDataSource(None); panel.setDelegate(None); }
            // The shared panel hides instead of closing its NSWindow.
            self.ivars().context.emit("closed", self.ivars().path.borrow().clone().into());
        }
        #[unsafe(method(applyPreviewPath:))]
        fn apply(&self, path: &NSString) {
            if path.to_string() != *self.ivars().path.borrow() { return; }
            if path.is_empty() {
                self.close_panel();
                return;
            }
            if !self.ivars().live.get() { return; }
            let Some(window) = self.window() else { return; };
            if !window.makeFirstResponder(Some(self)) {
                self.ivars().context.emit("error", JsonValue::Null);
                return;
            }
            let Some(panel) = (unsafe { QLPreviewPanel::sharedPreviewPanel(self.mtm()) }) else {
                self.ivars().context.emit("error", JsonValue::Null);
                return;
            };
            panel.makeKeyAndOrderFront(None);
            unsafe { panel.updateController(); }
            if !self.controls(&panel) {
                self.ivars().context.emit("error", JsonValue::Null);
                return;
            }
            unsafe { panel.reloadData(); }
        }
    }
    unsafe impl NSWindowDelegate for PreviewView {}
    unsafe impl QLPreviewPanelDelegate for PreviewView {}
    unsafe impl QLPreviewPanelDataSource for PreviewView {
        #[unsafe(method(numberOfPreviewItemsInPreviewPanel:))]
        fn count(&self, _: Option<&QLPreviewPanel>) -> NSInteger {
            if self.ivars().path.borrow().is_empty() { 0 } else { 1 }
        }
        #[unsafe(method_id(previewPanel:previewItemAtIndex:))]
        fn item(&self, _: Option<&QLPreviewPanel>, _: NSInteger) -> Option<Retained<ProtocolObject<dyn QLPreviewItem>>> {
            Some(ProtocolObject::from_retained(NSURL::fileURLWithPath(&NSString::from_str(&self.ivars().path.borrow()))))
        }
    }
);
impl PreviewView {
    fn controls(&self, panel: &QLPreviewPanel) -> bool {
        unsafe {
            panel.currentController().is_some_and(|controller| {
                std::ptr::eq(Retained::as_ptr(&controller).cast::<Self>(), self)
            })
        }
    }
    fn close_panel(&self) {
        if unsafe { QLPreviewPanel::sharedPreviewPanelExists(self.mtm()) }
            && let Some(panel) = unsafe { QLPreviewPanel::sharedPreviewPanel(self.mtm()) }
            && self.controls(&panel)
        {
            unsafe {
                panel.setDelegate(None);
            }
            panel.close();
        }
    }
    fn request(&self, path: &str) {
        *self.ivars().path.borrow_mut() = path.into();
        // Quick Look can run a nested AppKit loop (notably its text viewer).
        // Present after NSH's mount batch releases the native view tree.
        unsafe {
            self.performSelectorOnMainThread_withObject_waitUntilDone(
                sel!(applyPreviewPath:),
                Some(&NSString::from_str(path)),
                false,
            );
        }
    }
}

pub(super) struct Preview(Retained<PreviewView>);
impl Preview {
    pub fn new(context: ComponentContext) -> Self {
        let this = PreviewView::alloc(MainThreadMarker::new().unwrap()).set_ivars(State {
            context,
            path: RefCell::default(),
            live: Cell::new(true),
        });
        Self(unsafe { msg_send![super(this), initWithFrame: CGRect::ZERO] })
    }
}
impl NativeComponent for Preview {
    fn view(&self) -> Retained<NSView> {
        self.0.clone().into_super()
    }
    fn set_prop(&mut self, name: &str, value: Option<&JsonValue>) {
        if name == "path" {
            self.0
                .request(value.and_then(JsonValue::as_str).unwrap_or(""));
        }
    }
}
impl Drop for Preview {
    fn drop(&mut self) {
        self.0.ivars().live.set(false);
        self.0.request("");
    }
}
