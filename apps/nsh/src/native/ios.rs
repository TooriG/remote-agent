//! Standard UIKit objects, hosted through NSH's native-component API.
//! No drawing, hit testing, menu layout, font metrics or navigation is replaced.
use block2::RcBlock;
use nsh_core::JsonValue;
use nsh_platform_ios::{ComponentContext, Components, NativeComponent};
use objc2::{
    DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained,
    runtime::ProtocolObject,
};
use objc2_core_foundation::{CGRect, CGSize};
use objc2_foundation::{
    NSArray, NSData, NSIndexPath, NSInteger, NSObject, NSObjectProtocol, NSString,
};
use objc2_ui_kit::*;
use std::{
    cell::{Cell, RefCell},
    ptr::NonNull,
    rc::Rc,
};

fn action(context: &ComponentContext, event: &'static str, value: &str) -> Retained<UIAction> {
    let context = context.clone();
    let value = value.to_owned();
    let handler = RcBlock::new(move |_: NonNull<UIAction>| {
        context.emit(event, JsonValue::String(value.clone()))
    });
    // UIKit copies the block; its captures own their data and context.
    unsafe {
        UIAction::actionWithHandler(RcBlock::as_ptr(&handler), MainThreadMarker::new().unwrap())
    }
}

struct Switch {
    view: Retained<UISwitch>,
    value: Rc<Cell<bool>>,
}

impl Switch {
    fn new(context: ComponentContext) -> Self {
        let mtm = MainThreadMarker::new().unwrap();
        let view = UISwitch::new(mtm);
        let size = view.intrinsicContentSize();
        context.set_size(size.width as f32, size.height as f32);
        let value = Rc::new(Cell::new(false));
        let current = value.clone();
        let handler = RcBlock::new(move |action: NonNull<UIAction>| {
            // UIKit supplies the live action, whose sender is the control being changed.
            if let Some(sender) = unsafe { action.as_ref() }.sender()
                && let Some(view) = sender.downcast_ref::<UISwitch>()
            {
                let requested = view.isOn();
                // Core owns the setting, including the unchanged value after a failed operation.
                view.setOn_animated(current.get(), false);
                context.emit("change", requested.into());
            }
        });
        let action = unsafe { UIAction::actionWithHandler(RcBlock::as_ptr(&handler), mtm) };
        view.addAction_forControlEvents(&action, UIControlEvents::ValueChanged);
        Self { view, value }
    }
}

impl NativeComponent for Switch {
    fn view(&self) -> &UIView {
        &self.view
    }

    fn set_prop(&mut self, name: &str, value: Option<&JsonValue>) {
        match name {
            "value" => {
                let value = value.and_then(JsonValue::as_bool).unwrap_or(false);
                self.value.set(value);
                self.view.setOn_animated(value, false);
            }
            "enabled" => self
                .view
                .setEnabled(value.and_then(JsonValue::as_bool).unwrap_or(true)),
            "label" => self.view.setAccessibilityLabel(
                value
                    .and_then(JsonValue::as_str)
                    .map(NSString::from_str)
                    .as_deref(),
                self.view.mtm(),
            ),
            _ => {}
        }
    }
}

struct Progress(Retained<UIProgressView>);

impl Progress {
    fn new(context: ComponentContext) -> Self {
        let view = UIProgressView::new(MainThreadMarker::new().unwrap());
        context.set_size(0.0, view.intrinsicContentSize().height as f32);
        Self(view)
    }
}

impl NativeComponent for Progress {
    fn view(&self) -> &UIView {
        &self.0
    }

    fn set_prop(&mut self, name: &str, value: Option<&JsonValue>) {
        if name == "value" {
            self.0.setProgress(
                (value.and_then(JsonValue::as_f64).unwrap_or_default() / 100.0) as f32,
            );
        }
    }
}

fn menu_items(context: &ComponentContext, entries: &JsonValue) -> Vec<Retained<UIMenuElement>> {
    entries
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let title = NSString::from_str(entry["title"].as_str()?);
            if let Some(children) = entry.get("children") {
                let children = menu_items(context, children);
                Some(
                    UIMenu::menuWithTitle_children(
                        &title,
                        &NSArray::from_retained_slice(&children),
                        MainThreadMarker::new().unwrap(),
                    )
                    .into_super(),
                )
            } else {
                let action = action(context, "select", entry["id"].as_str()?);
                action.setTitle(&title);
                action.setAttributes(if entry["enabled"].as_bool().unwrap_or(true) {
                    UIMenuElementAttributes::empty()
                } else {
                    UIMenuElementAttributes::Disabled
                });
                action.setState(if entry["selected"].as_bool().unwrap_or(false) {
                    UIMenuElementState::On
                } else {
                    UIMenuElementState::Off
                });
                Some(action.into_super())
            }
        })
        .collect()
}

struct Button {
    view: Retained<UIButton>,
    context: ComponentContext,
}

impl Button {
    fn new(context: ComponentContext) -> Self {
        let mtm = MainThreadMarker::new().unwrap();
        let config = UIButtonConfiguration::plainButtonConfiguration(mtm);
        let view = UIButton::buttonWithConfiguration_primaryAction(
            &config,
            Some(&action(&context, "press", "")),
        );
        context.set_size(
            view.intrinsicContentSize().width as f32,
            view.intrinsicContentSize().height as f32,
        );
        Self { view, context }
    }

    fn menu(&self, items: &JsonValue) {
        let actions = menu_items(&self.context, items);
        let menu = UIMenu::menuWithTitle_children(
            &NSString::new(),
            &NSArray::from_retained_slice(&actions),
            self.view.mtm(),
        );
        self.view.setMenu(Some(&menu));
        self.view.setShowsMenuAsPrimaryAction(true);
    }
}

impl NativeComponent for Button {
    fn view(&self) -> &UIView {
        &self.view
    }
    fn set_prop(&mut self, name: &str, value: Option<&JsonValue>) {
        let mtm = self.view.mtm();
        match name {
            "title" => {
                let config = self.view.configuration().unwrap();
                config.setTitle(
                    value
                        .and_then(JsonValue::as_str)
                        .map(NSString::from_str)
                        .as_deref(),
                );
                self.view.setConfiguration(Some(&config));
            }
            "symbol" => {
                let image = value
                    .and_then(JsonValue::as_str)
                    .and_then(|s| UIImage::systemImageNamed(&NSString::from_str(s)));
                let config = self.view.configuration().unwrap();
                config.setImage(image.as_deref());
                self.view.setConfiguration(Some(&config));
            }
            "asset" => {
                let config = self.view.configuration().unwrap();
                let image = value
                    .and_then(JsonValue::as_str)
                    .and_then(|name| UIImage::imageNamed(&NSString::from_str(name)));
                config.setImage(image.as_deref());
                self.view.setConfiguration(Some(&config));
            }
            "label" => self.view.setAccessibilityLabel(
                value
                    .and_then(JsonValue::as_str)
                    .map(NSString::from_str)
                    .as_deref(),
                mtm,
            ),
            "enabled" => self
                .view
                .setEnabled(value.and_then(JsonValue::as_bool).unwrap_or(true)),
            "selected" => self
                .view
                .setSelected(value.and_then(JsonValue::as_bool).unwrap_or(false)),
            "appearance" => {
                let previous = self.view.configuration();
                let image = previous.as_ref().and_then(|config| config.image());
                let title = previous.as_ref().and_then(|config| config.title());
                let config = match value.and_then(JsonValue::as_str) {
                    Some("prominent") => {
                        UIButtonConfiguration::prominentGlassButtonConfiguration(mtm)
                    }
                    Some("glass") => UIButtonConfiguration::glassButtonConfiguration(mtm),
                    _ => UIButtonConfiguration::plainButtonConfiguration(mtm),
                };
                config.setImage(image.as_deref());
                config.setTitle(title.as_deref());
                self.view.setConfiguration(Some(&config));
            }
            "items" => {
                if let Some(items) = value {
                    self.menu(items);
                }
            }
            _ => {}
        }
        let size = self.view.intrinsicContentSize();
        self.context
            .set_size(size.width.max(0.0) as f32, size.height.max(0.0) as f32);
    }
}

struct ListState {
    context: ComponentContext,
    items: RefCell<Vec<super::ListRow>>,
}
define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = ListState]
    struct ListDelegate;
    unsafe impl NSObjectProtocol for ListDelegate {}
    unsafe impl UITableViewDataSource for ListDelegate {
        #[unsafe(method(tableView:numberOfRowsInSection:))]
        fn rows(&self, _: &UITableView, _: NSInteger) -> NSInteger {
            self.ivars().items.borrow().len() as NSInteger
        }

        #[unsafe(method_id(tableView:cellForRowAtIndexPath:))]
        fn cell(&self, table: &UITableView, index: &NSIndexPath) -> Retained<UITableViewCell> {
            let identifier = NSString::from_str("item");
            let cell = table
                .dequeueReusableCellWithIdentifier(&identifier)
                .unwrap_or_else(|| {
                    UITableViewCell::initWithStyle_reuseIdentifier(
                        UITableViewCell::alloc(self.mtm()),
                        UITableViewCellStyle::Default,
                        Some(&identifier),
                    )
                });
            let config = UIListContentConfiguration::cellConfiguration(self.mtm());
            if let Some(row) = self.ivars().items.borrow().get(index.row() as usize) {
                cell.setAccessibilityIdentifier(Some(&NSString::from_str(&row.id)));
                config.setText(Some(&NSString::from_str(&row.title)));
                let image = if let Some(png) = &row.png {
                    UIImage::imageWithData(&NSData::with_bytes(png))
                } else if let Some(asset) = &row.asset {
                    UIImage::imageNamed(&NSString::from_str(asset))
                } else {
                    None
                }
                .or_else(|| UIImage::systemImageNamed(&NSString::from_str(&row.symbol)));
                config.setImage(image.as_deref());
                config
                    .imageProperties()
                    .setMaximumSize(CGSize::new(20.0, 20.0));
                cell.setIndentationLevel(row.level as NSInteger);
                cell.setAccessoryType(if row.disclosure {
                    UITableViewCellAccessoryType::DisclosureIndicator
                } else {
                    UITableViewCellAccessoryType::None
                });
            }
            cell.setContentConfiguration(Some(ProtocolObject::from_ref(&*config)));
            cell
        }
    }
    unsafe impl UIScrollViewDelegate for ListDelegate {}
    unsafe impl UITableViewDelegate for ListDelegate {
        #[unsafe(method(tableView:didSelectRowAtIndexPath:))]
        fn selected(&self, table: &UITableView, index: &NSIndexPath) {
            table.deselectRowAtIndexPath_animated(index, true);
            let id = self
                .ivars()
                .items
                .borrow()
                .get(index.row() as usize)
                .map(|row| row.id.clone());
            if let Some(id) = id {
                self.ivars().context.emit("select", id.into());
            }
        }
    }
);

impl ListDelegate {
    fn new(mtm: MainThreadMarker, context: ComponentContext) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ListState {
            context,
            items: RefCell::default(),
        });
        unsafe { msg_send![super(this), init] }
    }
}

struct List {
    view: Retained<UITableView>,
    delegate: Retained<ListDelegate>,
}
impl List {
    fn new(context: ComponentContext) -> Self {
        let mtm = MainThreadMarker::new().unwrap();
        let delegate = ListDelegate::new(mtm, context);
        let view = UITableView::initWithFrame_style(
            UITableView::alloc(mtm),
            CGRect::ZERO,
            UITableViewStyle::Plain,
        );
        view.setDataSource(Some(ProtocolObject::from_ref(&*delegate)));
        // UIKit keeps a weak reference; this component owns the protocol implementation.
        unsafe { view.setDelegate(Some(ProtocolObject::from_ref(&*delegate))) };
        Self { view, delegate }
    }
}
impl NativeComponent for List {
    fn view(&self) -> &UIView {
        &self.view
    }
    fn set_prop(&mut self, name: &str, value: Option<&JsonValue>) {
        if name != "items" {
            return;
        }
        let rows = value
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .unwrap_or_default();
        if *self.delegate.ivars().items.borrow() == rows {
            return;
        }
        *self.delegate.ivars().items.borrow_mut() = rows;
        self.view.reloadData();
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = ComponentContext]
    struct SearchDelegate;
    unsafe impl NSObjectProtocol for SearchDelegate {}
    unsafe impl UIBarPositioningDelegate for SearchDelegate {}
    unsafe impl UISearchBarDelegate for SearchDelegate {
        #[unsafe(method(searchBar:textDidChange:))]
        fn changed(&self, _: &UISearchBar, text: &NSString) {
            self.ivars().emit("change", text.to_string().into());
        }
    }
);
impl SearchDelegate {
    fn new(mtm: MainThreadMarker, context: ComponentContext) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(context);
        unsafe { msg_send![super(this), init] }
    }
}
struct Search {
    view: Retained<UISearchBar>,
    _delegate: Retained<SearchDelegate>,
}
impl Search {
    fn new(context: ComponentContext) -> Self {
        let mtm = MainThreadMarker::new().unwrap();
        let delegate = SearchDelegate::new(mtm, context.clone());
        let view = UISearchBar::initWithFrame(UISearchBar::alloc(mtm), CGRect::ZERO);
        view.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        view.setPlaceholder(Some(&NSString::from_str("チャットを検索")));
        context.set_size(
            0.0,
            view.sizeThatFits(CGSize::new(320.0, f64::MAX)).height as f32,
        );
        Self {
            view,
            _delegate: delegate,
        }
    }
}
impl NativeComponent for Search {
    fn view(&self) -> &UIView {
        &self.view
    }
    fn set_prop(&mut self, name: &str, value: Option<&JsonValue>) {
        if name == "placeholder" {
            self.view.setPlaceholder(
                value
                    .and_then(JsonValue::as_str)
                    .map(NSString::from_str)
                    .as_deref(),
            );
        }
        if name == "value" {
            let text = value.and_then(JsonValue::as_str).unwrap_or("");
            // An echo from the input delegate must preserve selection and marked text.
            if self
                .view
                .text()
                .is_none_or(|current| current.to_string() != text)
            {
                self.view.setText(Some(&NSString::from_str(text)));
            }
        }
    }
}

struct Toolbar {
    view: Retained<UIToolbar>,
}
impl Toolbar {
    fn new(context: ComponentContext) -> Self {
        let mtm = MainThreadMarker::new().unwrap();
        let view = UIToolbar::initWithFrame(UIToolbar::alloc(mtm), CGRect::ZERO);
        let items = [
            (UIBarButtonSystemItem::Search, "search", "検索"),
            (UIBarButtonSystemItem::FlexibleSpace, "", ""),
            (UIBarButtonSystemItem::Compose, "compose", "新しい会話"),
        ]
        .into_iter()
        .map(|(kind, id, label)| {
            let item = UIBarButtonItem::initWithBarButtonSystemItem_primaryAction(
                UIBarButtonItem::alloc(mtm),
                kind,
                (!id.is_empty())
                    .then(|| action(&context, "select", id))
                    .as_deref(),
            );
            item.setAccessibilityLabel(Some(&NSString::from_str(label)), mtm);
            item
        })
        .collect::<Vec<_>>();
        view.setItems_animated(Some(&NSArray::from_retained_slice(&items)), false);
        context.set_size(
            0.0,
            view.sizeThatFits(CGSize::new(320.0, f64::MAX)).height as f32,
        );
        Self { view }
    }
}
impl NativeComponent for Toolbar {
    fn view(&self) -> &UIView {
        &self.view
    }
    fn set_prop(&mut self, _: &str, _: Option<&JsonValue>) {}
}

pub fn register(components: &Components) {
    super::terminal::register(components);
    components.register(
        "bex.preview",
        Rc::new(|cx| Box::new(super::preview_ios::Preview::new(cx))),
    );
    components.register(
        "bex.filePicker",
        Rc::new(|cx| Box::new(super::document_picker::DocumentPicker::new(cx))),
    );
    components.register("bex.button", Rc::new(|cx| Box::new(Button::new(cx))));
    components.register("bex.menu", Rc::new(|cx| Box::new(Button::new(cx))));
    components.register("bex.list", Rc::new(|cx| Box::new(List::new(cx))));
    components.register("bex.search", Rc::new(|cx| Box::new(Search::new(cx))));
    components.register("bex.switch", Rc::new(|cx| Box::new(Switch::new(cx))));
    components.register("bex.progress", Rc::new(|cx| Box::new(Progress::new(cx))));
    components.register("bex.toolbar", Rc::new(|cx| Box::new(Toolbar::new(cx))));
}
