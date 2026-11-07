//! AppKit controls and a real NSToolbar; NSH only supplies data and receives events.
use nsh_core::JsonValue;
use nsh_platform_macos::{ComponentContext, Components, NativeComponent};
use objc2::{
    DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained,
    runtime::ProtocolObject, sel,
};
use objc2_app_kit::*;
use objc2_core_foundation::CGRect;
use objc2_foundation::{NSArray, NSObject, NSObjectProtocol, NSString};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = ComponentContext]
    struct ActionTarget;
    unsafe impl NSObjectProtocol for ActionTarget {}
    impl ActionTarget {
        #[unsafe(method(pressed:))]
        fn pressed(&self, _: &NSButton) { self.ivars().emit("press", JsonValue::Null); }
        #[unsafe(method(chose:))]
        fn chose(&self, sender: &NSMenuItem) {
            if let Some(value) = sender.representedObject()
                && let Some(id) = value.downcast_ref::<NSString>() { self.ivars().emit("select", id.to_string().into()); }
        }
        #[unsafe(method(searched:))]
        fn searched(&self, sender: &NSSearchField) { self.ivars().emit("change", sender.stringValue().to_string().into()); }
    }
);
impl ActionTarget {
    fn new(context: ComponentContext) -> Retained<Self> {
        let this = Self::alloc(MainThreadMarker::new().unwrap()).set_ivars(context);
        unsafe { msg_send![super(this), init] }
    }
}
pub(super) fn symbol(name: &str) -> Option<Retained<NSImage>> {
    NSImage::imageWithSystemSymbolName_accessibilityDescription(&NSString::from_str(name), None)
}

struct Switch {
    view: Retained<NSSwitch>,
    target: Retained<SwitchTarget>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = (ComponentContext, Cell<NSControlStateValue>)]
    struct SwitchTarget;
    unsafe impl NSObjectProtocol for SwitchTarget {}
    impl SwitchTarget {
        #[unsafe(method(changed:))]
        fn changed(&self, sender: &NSSwitch) {
            let requested = sender.state() == NSControlStateValueOn;
            // A failed operation may publish the same value, so restore it here too.
            sender.setState(self.ivars().1.get());
            self.ivars().0.emit("change", requested.into());
        }
    }
);

impl Switch {
    fn new(context: ComponentContext) -> Self {
        let target = SwitchTarget::alloc(MainThreadMarker::new().unwrap())
            .set_ivars((context.clone(), Cell::new(NSControlStateValueOff)));
        let target: Retained<SwitchTarget> = unsafe { msg_send![super(target), init] };
        let view = NSSwitch::new(target.mtm());
        // This component retains the target for as long as AppKit uses the action.
        unsafe {
            view.setTarget(Some(&target));
            view.setAction(Some(sel!(changed:)));
        }
        let size = view.intrinsicContentSize();
        context.set_size(size.width as f32, size.height as f32);
        Self { view, target }
    }
}

impl NativeComponent for Switch {
    fn view(&self) -> Retained<NSView> {
        self.view.clone().into_super().into_super()
    }

    fn set_prop(&mut self, name: &str, value: Option<&JsonValue>) {
        match name {
            "value" => {
                let state = if value.and_then(JsonValue::as_bool).unwrap_or(false) {
                    NSControlStateValueOn
                } else {
                    NSControlStateValueOff
                };
                self.target.ivars().1.set(state);
                self.view.setState(state);
            }
            "enabled" => self
                .view
                .setEnabled(value.and_then(JsonValue::as_bool).unwrap_or(true)),
            "label" => self.view.setAccessibilityLabel(
                value
                    .and_then(JsonValue::as_str)
                    .map(NSString::from_str)
                    .as_deref(),
            ),
            _ => {}
        }
    }
}

struct Progress(Retained<NSProgressIndicator>);

impl Progress {
    fn new(context: ComponentContext) -> Self {
        let view = NSProgressIndicator::new(MainThreadMarker::new().unwrap());
        view.setIndeterminate(false);
        view.setMinValue(0.0);
        view.setMaxValue(100.0);
        context.set_size(0.0, view.intrinsicContentSize().height as f32);
        Self(view)
    }
}

impl NativeComponent for Progress {
    fn view(&self) -> Retained<NSView> {
        self.0.clone().into_super()
    }

    fn set_prop(&mut self, name: &str, value: Option<&JsonValue>) {
        if name == "value" {
            self.0
                .setDoubleValue(value.and_then(JsonValue::as_f64).unwrap_or_default());
        }
    }
}

struct Button {
    view: Retained<NSButton>,
    context: ComponentContext,
    _target: Retained<ActionTarget>,
}
impl Button {
    fn new(context: ComponentContext) -> Self {
        let target = ActionTarget::new(context.clone());
        // The target implements pressed: and is retained by this component.
        let view = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::new(),
                Some(&target),
                Some(sel!(pressed:)),
                target.mtm(),
            )
        };
        let size = view.intrinsicContentSize();
        context.set_size(size.width.max(0.0) as f32, size.height.max(0.0) as f32);
        Self {
            view,
            context,
            _target: target,
        }
    }
}
impl NativeComponent for Button {
    fn view(&self) -> Retained<NSView> {
        self.view.clone().into_super().into_super()
    }
    fn set_prop(&mut self, name: &str, value: Option<&JsonValue>) {
        match name {
            "title" => self.view.setTitle(&NSString::from_str(
                value.and_then(JsonValue::as_str).unwrap_or(""),
            )),
            "symbol" => {
                self.view.setImage(
                    value
                        .and_then(JsonValue::as_str)
                        .and_then(symbol)
                        .as_deref(),
                );
            }
            "asset" => self.view.setImage(
                value
                    .and_then(JsonValue::as_str)
                    .and_then(|name| NSImage::imageNamed(&NSString::from_str(name)))
                    .as_deref(),
            ),
            "label" => self.view.setAccessibilityLabel(
                value
                    .and_then(JsonValue::as_str)
                    .map(NSString::from_str)
                    .as_deref(),
            ),
            "identifier" => self.view.setAccessibilityIdentifier(
                value
                    .and_then(JsonValue::as_str)
                    .map(NSString::from_str)
                    .as_deref(),
            ),
            "enabled" => self
                .view
                .setEnabled(value.and_then(JsonValue::as_bool).unwrap_or(true)),
            "appearance" => {
                let appearance = value.and_then(JsonValue::as_str);
                self.view.setBordered(appearance != Some("plain"));
                self.view.setBezelStyle(match appearance {
                    Some("circular") => NSBezelStyle::Circular,
                    Some("glass" | "prominent") => NSBezelStyle::Glass,
                    _ => NSBezelStyle::Automatic,
                });
            }
            "selected" => {
                self.view.setButtonType(NSButtonType::PushOnPushOff);
                self.view
                    .setState(if value.and_then(JsonValue::as_bool).unwrap_or(false) {
                        NSControlStateValueOn
                    } else {
                        NSControlStateValueOff
                    });
            }
            _ => {}
        }
        self.view.setImagePosition(if self.view.title().is_empty() {
            NSCellImagePosition::ImageOnly
        } else {
            NSCellImagePosition::ImageLeft
        });
        let size = self.view.intrinsicContentSize();
        self.context
            .set_size(size.width.max(0.0) as f32, size.height.max(0.0) as f32);
    }
}

struct Menu {
    view: Retained<NSPopUpButton>,
    context: ComponentContext,
    target: Retained<ActionTarget>,
}
impl Menu {
    fn new(context: ComponentContext) -> Self {
        let target = ActionTarget::new(context.clone());
        let view = NSPopUpButton::initWithFrame_pullsDown(
            NSPopUpButton::alloc(target.mtm()),
            CGRect::ZERO,
            true,
        );
        // AppKit reserves the first item as a pull-down button's label.
        view.addItemWithTitle(&NSString::new());
        context.set_size(100.0, view.intrinsicContentSize().height as f32);
        Self {
            view,
            context,
            target,
        }
    }
}

fn menu_items(menu: &NSMenu, entries: &JsonValue, target: &ActionTarget) {
    menu.setAutoenablesItems(false);
    for entry in entries.as_array().into_iter().flatten() {
        let Some(title) = entry["title"].as_str() else {
            continue;
        };
        // Every actionable item targets the retained delegate's chose: selector.
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(target.mtm()),
                &NSString::from_str(title),
                Some(sel!(chose:)),
                &NSString::new(),
            )
        };
        if let Some(children) = entry.get("children") {
            unsafe { item.setAction(None) };
            let submenu =
                NSMenu::initWithTitle(NSMenu::alloc(target.mtm()), &NSString::from_str(title));
            menu_items(&submenu, children, target);
            item.setSubmenu(Some(&submenu));
        } else if let Some(id) = entry["id"].as_str() {
            unsafe {
                item.setTarget(Some(target));
                item.setRepresentedObject(Some(&NSString::from_str(id)));
            }
            item.setState(if entry["selected"].as_bool().unwrap_or(false) {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            });
        }
        item.setImage(entry["symbol"].as_str().and_then(symbol).as_deref());
        item.setEnabled(entry["enabled"].as_bool().unwrap_or(true));
        menu.addItem(&item);
    }
}
impl NativeComponent for Menu {
    fn view(&self) -> Retained<NSView> {
        self.view.clone().into_super().into_super().into_super()
    }
    fn set_prop(&mut self, name: &str, value: Option<&JsonValue>) {
        match name {
            "appearance" => self
                .view
                .setBordered(value.and_then(JsonValue::as_str) != Some("plain")),
            "label" => self.view.setAccessibilityLabel(
                value
                    .and_then(JsonValue::as_str)
                    .map(NSString::from_str)
                    .as_deref(),
            ),
            "title" => {
                let title = NSString::from_str(value.and_then(JsonValue::as_str).unwrap_or(""));
                // A pull-down button displays its first item's title.
                if let Some(item) = self.view.itemAtIndex(0) {
                    item.setTitle(&title);
                }
            }
            "symbol" => {
                if let Some(item) = self.view.itemAtIndex(0) {
                    item.setImage(
                        value
                            .and_then(JsonValue::as_str)
                            .and_then(symbol)
                            .as_deref(),
                    );
                }
            }
            "enabled" => self
                .view
                .setEnabled(value.and_then(JsonValue::as_bool).unwrap_or(true)),
            "items" => {
                while self.view.numberOfItems() > 1 {
                    self.view.removeItemAtIndex(1);
                }
                if let Some(entries) = value
                    && let Some(menu) = self.view.menu()
                {
                    menu_items(&menu, entries, &self.target);
                }
            }
            _ => {}
        }
        let size = self.view.intrinsicContentSize();
        self.context
            .set_size(size.width.max(0.0) as f32, size.height.max(0.0) as f32);
    }
}

struct Search {
    view: Retained<NSSearchField>,
    _target: Retained<ActionTarget>,
}
impl Search {
    fn new(context: ComponentContext) -> Self {
        let target = ActionTarget::new(context.clone());
        let view = NSSearchField::initWithFrame(NSSearchField::alloc(target.mtm()), CGRect::ZERO);
        view.setPlaceholderString(Some(&NSString::from_str("会話を検索")));
        view.setSendsWholeSearchString(false);
        // The target implements searched: and lives as long as the field.
        unsafe {
            view.setTarget(Some(&target));
            view.setAction(Some(sel!(searched:)));
        }
        context.set_size(0.0, view.intrinsicContentSize().height as f32);
        Self {
            view,
            _target: target,
        }
    }
}
impl NativeComponent for Search {
    fn view(&self) -> Retained<NSView> {
        self.view.clone().into_super().into_super().into_super()
    }
    fn set_prop(&mut self, name: &str, value: Option<&JsonValue>) {
        if name == "appearance" {
            let plain = value.and_then(JsonValue::as_str) == Some("plain");
            self.view.setBezeled(!plain);
            self.view.setDrawsBackground(!plain);
        }
        if name == "placeholder" {
            self.view.setPlaceholderString(
                value
                    .and_then(JsonValue::as_str)
                    .map(NSString::from_str)
                    .as_deref(),
            );
        }
        if name == "value" {
            let text = value.and_then(JsonValue::as_str).unwrap_or("");
            // An echo from the input delegate must preserve selection and marked text.
            if self.view.stringValue().to_string() != text {
                self.view.setStringValue(&NSString::from_str(text));
            }
        }
    }
}

fn toolbar_identifiers() -> Retained<NSArray<NSString>> {
    NSArray::from_retained_slice(&[
        NSString::from_str("sidebar"),
        unsafe { NSToolbarFlexibleSpaceItemIdentifier }.to_owned(),
        NSString::from_str("compose"),
        NSString::from_str("more"),
    ])
}
define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = ComponentContext]
    struct ToolbarDelegate;
    unsafe impl NSObjectProtocol for ToolbarDelegate {}
    impl ToolbarDelegate {
        #[unsafe(method(pressed:))]
        fn pressed(&self, item: &NSToolbarItem) { self.ivars().emit("select", item.itemIdentifier().to_string().into()); }
    }
    unsafe impl NSToolbarDelegate for ToolbarDelegate {
        #[unsafe(method_id(toolbarDefaultItemIdentifiers:))]
        fn identifiers(&self, _: &NSToolbar) -> Retained<NSArray<NSString>> {
            toolbar_identifiers()
        }
        #[unsafe(method_id(toolbarAllowedItemIdentifiers:))]
        fn allowed(&self, _: &NSToolbar) -> Retained<NSArray<NSString>> { toolbar_identifiers() }
        #[unsafe(method_id(toolbar:itemForItemIdentifier:willBeInsertedIntoToolbar:))]
        fn item(&self, _: &NSToolbar, identifier: &NSString, _: bool) -> Option<Retained<NSToolbarItem>> {
            let info = match identifier.to_string().as_str() {
                "sidebar" => Some(("sidebar.left", "サイドバー")), "compose" => Some(("square.and.pencil", "新しい会話")), "more" => Some(("ellipsis", "その他")), _ => None,
            };
            info.map(|(image, label)| {
                let item = NSToolbarItem::initWithItemIdentifier(NSToolbarItem::alloc(self.mtm()), identifier);
                item.setLabel(&NSString::from_str(label));
                item.setImage(symbol(image).as_deref());
                // The toolbar delegate owns this selector and is retained by its host.
                unsafe { item.setTarget(Some(self)); item.setAction(Some(sel!(pressed:))); }
                item
            })
        }
    }
);
impl ToolbarDelegate {
    fn new(mtm: MainThreadMarker, context: ComponentContext) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(context);
        unsafe { msg_send![super(this), init] }
    }
}
struct ToolbarState {
    // NSToolbar's delegate reference is weak.
    _delegate: Retained<ToolbarDelegate>,
    toolbar: Retained<NSToolbar>,
    title: RefCell<String>,
}
define_class!(
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = ToolbarState]
    struct ToolbarHost;
    unsafe impl NSObjectProtocol for ToolbarHost {}
    impl ToolbarHost {
        #[unsafe(method(viewDidMoveToWindow))]
        fn moved(&self) {
            // Keep NSView's lifecycle, then attach the standard AppKit toolbar.
            let _: () = unsafe { msg_send![super(self), viewDidMoveToWindow] };
            if let Some(window) = self.window() {
                window.setToolbarStyle(NSWindowToolbarStyle::UnifiedCompact);
                window.setToolbar(Some(&self.ivars().toolbar));
                window.setTitle(&NSString::from_str(&self.ivars().title.borrow()));
            }
        }
    }
);
impl ToolbarHost {
    fn new(mtm: MainThreadMarker, state: ToolbarState) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(state);
        unsafe { msg_send![super(this), initWithFrame: CGRect::ZERO] }
    }
}
struct Toolbar {
    view: Retained<ToolbarHost>,
}
impl Toolbar {
    fn new(context: ComponentContext) -> Self {
        let mtm = MainThreadMarker::new().unwrap();
        let delegate = ToolbarDelegate::new(mtm, context);
        let toolbar = NSToolbar::initWithIdentifier(
            NSToolbar::alloc(mtm),
            &NSString::from_str("BexNativeToolbar"),
        );
        toolbar.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        toolbar.setDisplayMode(NSToolbarDisplayMode::IconOnly);
        let view = ToolbarHost::new(
            mtm,
            ToolbarState {
                _delegate: delegate,
                toolbar,
                title: RefCell::new("Bex NSH".into()),
            },
        );
        Self { view }
    }
}
impl NativeComponent for Toolbar {
    fn view(&self) -> Retained<NSView> {
        self.view.clone().into_super()
    }
    fn set_prop(&mut self, name: &str, value: Option<&JsonValue>) {
        if name != "title" {
            return;
        }
        let title = value.and_then(JsonValue::as_str).unwrap_or("Bex NSH");
        *self.view.ivars().title.borrow_mut() = title.to_owned();
        if let Some(window) = self.view.window() {
            window.setTitle(&NSString::from_str(title));
        }
    }
}

pub fn register(components: &Components) {
    super::hosted::register(components, "bex.details", "bex_nsh_details");
    super::hosted::register(components, "bex.browser.remote", "bex_nsh_browser");
    super::terminal::register(components);
    components.register(
        "bex.preview",
        Rc::new(|cx| Box::new(super::preview_macos::Preview::new(cx))),
    );
    components.register("bex.button", Rc::new(|cx| Box::new(Button::new(cx))));
    components.register("bex.menu", Rc::new(|cx| Box::new(Menu::new(cx))));
    components.register("bex.search", Rc::new(|cx| Box::new(Search::new(cx))));
    components.register("bex.switch", Rc::new(|cx| Box::new(Switch::new(cx))));
    components.register("bex.progress", Rc::new(|cx| Box::new(Progress::new(cx))));
    components.register(
        "bex.list",
        Rc::new(|cx| Box::new(super::list_macos::List::new(cx))),
    );
    components.register("bex.toolbar", Rc::new(|cx| Box::new(Toolbar::new(cx))));
}
