//! Present Quick Look modally so NSH's navigation stack keeps its own controllers.
use nsh_core::JsonValue;
use nsh_platform_ios::{ComponentContext, NativeComponent};
use objc2::{
    DefinedClass, MainThreadMarker, MainThreadOnly, define_class, extern_class, extern_methods,
    extern_protocol, msg_send,
    rc::{Retained, Weak},
    runtime::ProtocolObject,
    sel,
};
use objc2_core_foundation::CGRect;
use objc2_foundation::{NSInteger, NSObject, NSObjectProtocol, NSString, NSURL};
use objc2_ui_kit::{
    UIAdaptivePresentationControllerDelegate, UIBarButtonItem, UIBarButtonSystemItem,
    UINavigationController, UIPresentationController, UIResponder, UIView, UIViewController,
};

// objc2 0.3.2's QuickLook crate omits UIKit's QLPreviewController. These are
// only declarations from QuickLook/QLPreview{Controller,Item}.h; the OS supplies them.
#[link(name = "QuickLook", kind = "framework")]
unsafe extern "C" {}
extern_class!(
    #[unsafe(super(UIViewController, UIResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    struct QLPreviewController;
);
impl QLPreviewController {
    extern_methods!(
        #[unsafe(method(new))]
        unsafe fn new(mtm: MainThreadMarker) -> Retained<Self>;
        #[unsafe(method(setDataSource:))]
        unsafe fn set_data_source(
            &self,
            source: Option<&ProtocolObject<dyn QLPreviewControllerDataSource>>,
        );
        #[unsafe(method(setDelegate:))]
        unsafe fn set_delegate(
            &self,
            delegate: Option<&ProtocolObject<dyn QLPreviewControllerDelegate>>,
        );
    );
}
extern_protocol!(
    #[expect(
        clippy::missing_safety_doc,
        reason = "Clippy misses the safety docs forwarded by extern_protocol!"
    )]
    #[doc = "UIKit file preview item.\n\n# Safety\n\nImplementations must expose QuickLook’s nullable file-URL property with the SDK ABI."]
    unsafe trait QLPreviewItem: NSObjectProtocol {
        #[unsafe(method(previewItemURL))]
        unsafe fn url(&self) -> Option<Retained<NSURL>>;
    }
);
// QuickLook's NSURL(QLPreviewConvenienceAdditions) implements this protocol.
unsafe impl QLPreviewItem for NSURL {}
extern_protocol!(
    #[expect(
        clippy::missing_safety_doc,
        reason = "Clippy misses the safety docs forwarded by extern_protocol!"
    )]
    #[doc = "UIKit preview collection.\n\n# Safety\n\nItem indices must match the reported count and return live SDK preview items on the main thread."]
    unsafe trait QLPreviewControllerDataSource {
        #[unsafe(method(numberOfPreviewItemsInPreviewController:))]
        unsafe fn count(&self, controller: &QLPreviewController) -> NSInteger;
        #[unsafe(method(previewController:previewItemAtIndex:))]
        unsafe fn item(
            &self,
            controller: &QLPreviewController,
            index: NSInteger,
        ) -> Retained<ProtocolObject<dyn QLPreviewItem>>;
    }
);
extern_protocol!(
    #[expect(
        clippy::missing_safety_doc,
        reason = "Clippy misses the safety docs forwarded by extern_protocol!"
    )]
    #[doc = "UIKit preview lifecycle.\n\n# Safety\n\nImplemented selectors must use QuickLook’s delegate ABI and run on the main thread."]
    unsafe trait QLPreviewControllerDelegate: NSObjectProtocol {
        #[optional]
        #[unsafe(method(previewControllerDidDismiss:))]
        unsafe fn closed(&self, controller: &QLPreviewController);
    }
);

struct State {
    context: ComponentContext,
    url: Retained<NSURL>,
    navigation: Weak<UINavigationController>,
}
fn closed(context: &ComponentContext, url: &NSURL) {
    if let Some(path) = url.path() {
        context.emit("closed", path.to_string().into());
    }
}
define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = State]
    struct Delegate;
    unsafe impl NSObjectProtocol for Delegate {}
    impl Delegate {
        #[unsafe(method(done:))]
        fn done(&self, _: &UIBarButtonItem) {
            if let Some(navigation) = self.ivars().navigation.load() {
                let context = self.ivars().context.clone();
                let url = self.ivars().url.clone();
                let completion = block2::RcBlock::new(move || closed(&context, &url));
                navigation.dismissViewControllerAnimated_completion(true, Some(&completion));
            }
        }
    }
    unsafe impl QLPreviewControllerDataSource for Delegate {
        #[unsafe(method(numberOfPreviewItemsInPreviewController:))]
        fn count(&self, _: &QLPreviewController) -> NSInteger {
            1
        }
        #[unsafe(method_id(previewController:previewItemAtIndex:))]
        fn item(
            &self,
            _: &QLPreviewController,
            _: NSInteger,
        ) -> Retained<ProtocolObject<dyn QLPreviewItem>> {
            ProtocolObject::from_retained(self.ivars().url.clone())
        }
    }
    unsafe impl QLPreviewControllerDelegate for Delegate {
        #[unsafe(method(previewControllerDidDismiss:))]
        fn closed(&self, _: &QLPreviewController) {
            closed(&self.ivars().context, &self.ivars().url);
        }
    }
    unsafe impl UIAdaptivePresentationControllerDelegate for Delegate {
        #[unsafe(method(presentationControllerDidDismiss:))]
        fn dismissed(&self, _: &UIPresentationController) {
            closed(&self.ivars().context, &self.ivars().url);
        }
    }
);

struct Presented {
    preview: Retained<QLPreviewController>,
    navigation: Retained<UINavigationController>,
    _delegate: Retained<Delegate>,
}
impl Drop for Presented {
    fn drop(&mut self) {
        unsafe {
            self.preview.set_delegate(None);
            self.preview.set_data_source(None);
        }
        if self.navigation.presentingViewController().is_some() {
            self.navigation
                .dismissViewControllerAnimated_completion(false, None);
        }
    }
}

pub(super) struct Preview {
    view: Retained<UIView>,
    context: ComponentContext,
    presented: Option<Presented>,
}
impl Preview {
    pub fn new(context: ComponentContext) -> Self {
        Self {
            view: UIView::initWithFrame(
                UIView::alloc(MainThreadMarker::new().unwrap()),
                CGRect::ZERO,
            ),
            context,
            presented: None,
        }
    }
}
impl NativeComponent for Preview {
    fn view(&self) -> &UIView {
        &self.view
    }
    fn set_prop(&mut self, name: &str, value: Option<&JsonValue>) {
        if name != "path" {
            return;
        }
        self.presented = None;
        let path = value.and_then(JsonValue::as_str).unwrap_or("");
        if path.is_empty() {
            return;
        }
        let Some(mut presenter) = self
            .view
            .window()
            .and_then(|window| window.rootViewController())
        else {
            return;
        };
        while let Some(presented) = presenter.presentedViewController() {
            presenter = presented;
        }
        let controller = unsafe { QLPreviewController::new(self.view.mtm()) };
        let navigation = UINavigationController::initWithRootViewController(
            UINavigationController::alloc(self.view.mtm()),
            &controller,
        );
        let this = Delegate::alloc(self.view.mtm()).set_ivars(State {
            context: self.context.clone(),
            url: NSURL::fileURLWithPath(&NSString::from_str(path)),
            navigation: Weak::from_retained(&navigation),
        });
        let delegate: Retained<Delegate> = unsafe { msg_send![super(this), init] };
        unsafe {
            controller.set_data_source(Some(ProtocolObject::from_ref(&*delegate)));
            controller.set_delegate(Some(ProtocolObject::from_ref(&*delegate)));
            let done = UIBarButtonItem::initWithBarButtonSystemItem_target_action(
                UIBarButtonItem::alloc(self.view.mtm()),
                UIBarButtonSystemItem::Done,
                Some(&delegate),
                Some(sel!(done:)),
            );
            controller
                .navigationItem()
                .setLeftBarButtonItem(Some(&done));
        }
        presenter.presentViewController_animated_completion(&navigation, true, None);
        if let Some(presentation) = navigation.presentationController() {
            // Presented retains the SDK's weak delegate through dismissal.
            unsafe {
                presentation.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            }
        }
        self.presented = Some(Presented {
            preview: controller,
            navigation,
            _delegate: delegate,
        });
    }
}
