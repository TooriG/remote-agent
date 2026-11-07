//! AppKit owns hierarchy, disclosure controls, keyboard navigation and selection.
use super::{ListRow, macos::symbol};
use nsh_core::JsonValue;
use nsh_platform_macos::{ComponentContext, NativeComponent};
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, ProtocolObject},
    sel,
};
use objc2_app_kit::*;
use objc2_core_foundation::{CGRect, CGSize};
use objc2_foundation::{
    NSArray, NSBundle, NSData, NSIndexSet, NSInteger, NSNotification, NSObject, NSObjectProtocol,
    NSString,
};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
};

struct Item {
    row: ListRow,
    // NSOutlineView requires stable, unique object identities across reloads.
    object: Retained<NSString>,
    children: Vec<usize>,
}

#[derive(Default)]
struct Data {
    items: Vec<Item>,
    roots: Vec<usize>,
    indices: HashMap<String, usize>,
}
impl Data {
    fn item(&self, object: &AnyObject) -> Option<&Item> {
        let id = object.downcast_ref::<NSString>()?.to_string();
        self.indices.get(&id).map(|index| &self.items[*index])
    }

    fn children(&self, object: Option<&AnyObject>) -> &[usize] {
        object.map_or(&self.roots, |object| {
            self.item(object).map_or(&[], |item| &item.children)
        })
    }
}
struct State {
    context: ComponentContext,
    data: RefCell<Data>,
    updating: Cell<bool>,
    selected: RefCell<Option<String>>,
    expansion_enabled: Cell<bool>,
    sidebar: Cell<bool>,
}

impl State {
    fn row(&self, object: &AnyObject) -> Option<std::cell::Ref<'_, ListRow>> {
        std::cell::Ref::filter_map(self.data.borrow(), |data| {
            data.item(object).map(|item| &item.row)
        })
        .ok()
    }

    fn expansion(&self, notification: &NSNotification, expanded: bool) {
        if self.updating.get() {
            return;
        }
        let id = notification
            .userInfo()
            .and_then(|info| info.objectForKey(&NSString::from_str("NSObject")))
            .and_then(|item| self.row(&item).map(|row| row.id.clone()));
        if let Some(id) = id {
            self.context.emit(
                "expand",
                serde_json::json!({"id": id, "expanded": expanded}),
            );
        }
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = State]
    struct Delegate;
    unsafe impl NSObjectProtocol for Delegate {}
    unsafe impl NSControlTextEditingDelegate for Delegate {}
    unsafe impl NSOutlineViewDataSource for Delegate {
        #[unsafe(method(outlineView:numberOfChildrenOfItem:))]
        fn count(&self, _: &NSOutlineView, item: Option<&AnyObject>) -> NSInteger {
            self.ivars().data.borrow().children(item).len() as NSInteger
        }
        #[unsafe(method_id(outlineView:child:ofItem:))]
        fn child(
            &self,
            _: &NSOutlineView,
            index: NSInteger,
            item: Option<&AnyObject>,
        ) -> Retained<AnyObject> {
            let data = self.ivars().data.borrow();
            data.items[data.children(item)[index as usize]]
                .object
                .clone()
                .into_super()
                .into_super()
        }
        #[unsafe(method(outlineView:isItemExpandable:))]
        fn expandable(&self, _: &NSOutlineView, item: &AnyObject) -> bool {
            self.ivars().row(item).is_some_and(|row| row.disclosure)
        }
    }
    unsafe impl NSOutlineViewDelegate for Delegate {
        #[unsafe(method(outlineView:isGroupItem:))]
        fn group(&self, _: &NSOutlineView, item: &AnyObject) -> bool {
            !self.ivars().sidebar.get() && self.ivars().row(item).is_some_and(|row| row.group)
        }
        // Only completed changes emit events. AppKit also queries shouldExpand /
        // shouldCollapse while inspecting accessibility; those queries stay pure.
        #[unsafe(method(outlineView:shouldExpandItem:))]
        fn can_expand(&self, _: &NSOutlineView, _: &AnyObject) -> bool {
            self.ivars().updating.get() || self.ivars().expansion_enabled.get()
        }
        #[unsafe(method(outlineView:shouldCollapseItem:))]
        fn can_collapse(&self, _: &NSOutlineView, _: &AnyObject) -> bool {
            self.ivars().updating.get() || self.ivars().expansion_enabled.get()
        }
        #[unsafe(method(outlineViewItemDidExpand:))]
        fn expanded(&self, notification: &NSNotification) { self.ivars().expansion(notification, true); }
        #[unsafe(method(outlineViewItemDidCollapse:))]
        fn collapsed(&self, notification: &NSNotification) { self.ivars().expansion(notification, false); }
        #[unsafe(method_id(outlineView:viewForTableColumn:item:))]
        fn view(
            &self,
            outline: &NSOutlineView,
            _: Option<&NSTableColumn>,
            item: &AnyObject,
        ) -> Option<Retained<NSView>> {
            self.ivars().row(item).map(|row| {
                let identifier = NSString::from_str("item");
                let reused = unsafe { outline.makeViewWithIdentifier_owner(&identifier, None) };
                let cell = reused
                    .and_then(|view| view.downcast::<NSTableCellView>().ok())
                    .unwrap_or_else(|| cell_view(self.mtm()));
                cell.setAccessibilityIdentifier(Some(&NSString::from_str(&row.id)));
                // The cell's subviews retain both outlets for the cell's entire lifetime.
                unsafe {
                    if let Some(field) = cell.textField() {
                        field.setStringValue(&NSString::from_str(&row.title));
                        if self.ivars().sidebar.get() {
                            let weight = if row.group { NSFontWeightSemibold } else { NSFontWeightRegular };
                            field.setFont(Some(&NSFont::systemFontOfSize_weight(13.0, weight)));
                            field.setTextColor(Some(&NSColor::labelColor()));
                        }
                    }
                    if let Some(view) = cell.imageView() {
                        let image = if let Some(png) = &row.png {
                            NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(png))
                                .inspect(|image| image.setSize(CGSize::new(17.0, 17.0)))
                        } else if let Some(asset) = &row.asset {
                            NSBundle::mainBundle()
                                .URLForResource_withExtension(
                                    Some(&NSString::from_str(asset)),
                                    None,
                                )
                                .and_then(|url| {
                                    NSImage::initWithContentsOfURL(NSImage::alloc(), &url)
                                })
                                .inspect(|image| image.setSize(CGSize::new(17.0, 17.0)))
                        } else {
                            None
                        }.or_else(|| symbol(&row.symbol));
                        view.setImage(image.as_deref());
                    }
                }
                cell.into_super()
            })
        }
        #[unsafe(method(outlineViewSelectionDidChange:))]
        fn selected(&self, notification: &NSNotification) {
            if self.ivars().updating.get() {
                return;
            }
            if let Some(object) = notification.object()
                && let Some(view) = object.downcast_ref::<NSOutlineView>()
                && let Some(item) = view.itemAtRow(view.selectedRow())
            {
                let id = self.ivars().row(&item).map(|row| row.id.clone());
                let changed = id.as_ref() != self.ivars().selected.borrow().as_ref();
                if let Some(id) = id && changed {
                    *self.ivars().selected.borrow_mut() = Some(id.clone());
                    self.ivars().context.emit("select", id.into());
                }
            }
        }
    }
    impl Delegate {
        #[unsafe(method(activate:))]
        fn activate(&self, view: &NSOutlineView) {
            let id = view.itemAtRow(view.selectedRow()).and_then(|item| self.ivars().row(&item).map(|row| row.id.clone()));
            if let Some(id) = id { self.ivars().context.emit("activate", id.into()); }
        }
    }
);

fn cell_view(mtm: MainThreadMarker) -> Retained<NSTableCellView> {
    let cell = NSTableCellView::initWithFrame(NSTableCellView::alloc(mtm), CGRect::ZERO);
    cell.setIdentifier(Some(&NSString::from_str("item")));
    let image = NSImageView::initWithFrame(NSImageView::alloc(mtm), CGRect::ZERO);
    image.setAccessibilityElement(false);
    let field = NSTextField::labelWithString(&NSString::new(), mtm);
    field.setMaximumNumberOfLines(1);
    image.setTranslatesAutoresizingMaskIntoConstraints(false);
    field.setTranslatesAutoresizingMaskIntoConstraints(false);
    cell.addSubview(&image);
    cell.addSubview(&field);
    // These unretained outlets point at subviews owned by the standard cell.
    unsafe {
        cell.setImageView(Some(&image));
        cell.setTextField(Some(&field));
    }
    NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
        image
            .leadingAnchor()
            .constraintEqualToAnchor(&cell.leadingAnchor()),
        image
            .centerYAnchor()
            .constraintEqualToAnchor(&cell.centerYAnchor()),
        image.widthAnchor().constraintEqualToConstant(18.0),
        image.heightAnchor().constraintEqualToConstant(18.0),
        field
            .leadingAnchor()
            .constraintEqualToAnchor_constant(&image.trailingAnchor(), 6.0),
        field
            .trailingAnchor()
            .constraintEqualToAnchor_constant(&cell.trailingAnchor(), -4.0),
        field
            .centerYAnchor()
            .constraintEqualToAnchor(&cell.centerYAnchor()),
    ]));
    cell
}

pub(super) struct List {
    view: Retained<NSScrollView>,
    outline: Retained<NSOutlineView>,
    delegate: Retained<Delegate>,
}

impl List {
    pub(super) fn new(context: ComponentContext) -> Self {
        let mtm = MainThreadMarker::new().unwrap();
        let delegate = Delegate::alloc(mtm).set_ivars(State {
            context,
            data: RefCell::default(),
            updating: Cell::new(false),
            selected: RefCell::new(None),
            expansion_enabled: Cell::new(true),
            sidebar: Cell::new(false),
        });
        let delegate: Retained<Delegate> = unsafe { msg_send![super(delegate), init] };
        let outline = NSOutlineView::initWithFrame(NSOutlineView::alloc(mtm), CGRect::ZERO);
        let column = NSTableColumn::initWithIdentifier(
            NSTableColumn::alloc(mtm),
            &NSString::from_str("items"),
        );
        column.setWidth(260.0);
        outline.addTableColumn(&column);
        outline.setHeaderView(None);
        // AppKit retains neither delegate nor data source; List owns both.
        unsafe {
            outline.setOutlineTableColumn(Some(&column));
            outline.setDataSource(Some(ProtocolObject::from_ref(&*delegate)));
            outline.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            outline.setTarget(Some(&delegate));
            outline.setDoubleAction(Some(sel!(activate:)));
        }
        outline.setStyle(NSTableViewStyle::SourceList);
        let view = NSScrollView::initWithFrame(NSScrollView::alloc(mtm), CGRect::ZERO);
        view.setDocumentView(Some(&outline));
        view.setBorderType(NSBorderType::NoBorder);
        view.setHasVerticalScroller(true);
        Self {
            view,
            outline,
            delegate,
        }
    }

    fn reload(&self, rows: Vec<ListRow>) {
        if self
            .delegate
            .ivars()
            .data
            .borrow()
            .items
            .iter()
            .map(|item| &item.row)
            .eq(rows.iter())
        {
            return;
        }
        let previous = std::mem::take(&mut *self.delegate.ivars().data.borrow_mut());
        let mut objects: HashMap<_, _> = previous
            .items
            .into_iter()
            .map(|item| (item.row.id, item.object))
            .collect();
        let mut data = Data::default();
        let mut ancestors: Vec<usize> = Vec::new();
        for row in rows {
            while ancestors
                .last()
                .is_some_and(|index| data.items[*index].row.level >= row.level)
            {
                ancestors.pop();
            }
            let object = objects
                .remove(&row.id)
                .unwrap_or_else(|| NSString::from_str(&row.id));
            let index = data.items.len();
            if let Some(parent) = ancestors.last() {
                data.items[*parent].children.push(index);
            } else {
                data.roots.push(index);
            }
            data.indices.insert(row.id.clone(), index);
            ancestors.push(index);
            data.items.push(Item {
                row,
                object,
                children: Vec::new(),
            });
        }
        *self.delegate.ivars().data.borrow_mut() = data;
        self.outline.reloadData();
        // Release the RefCell borrow before calling AppKit's data source callbacks.
        let expansion: Vec<_> = self
            .delegate
            .ivars()
            .data
            .borrow()
            .items
            .iter()
            .filter(|item| item.row.disclosure)
            .map(|item| (item.object.clone(), item.row.expanded))
            .collect();
        for (object, expanded) in expansion {
            unsafe {
                if expanded {
                    self.outline.expandItem(Some(&object));
                } else {
                    self.outline.collapseItem(Some(&object));
                }
            }
        }
    }
}

impl NativeComponent for List {
    fn view(&self) -> Retained<NSView> {
        self.view.clone().into_super()
    }

    fn set_prop(&mut self, name: &str, value: Option<&JsonValue>) {
        if name == "sidebar" {
            let sidebar = value.and_then(JsonValue::as_bool).unwrap_or(false);
            self.delegate.ivars().sidebar.set(sidebar);
            self.outline.setRowHeight(if sidebar { 32.0 } else { 24.0 });
            self.outline.reloadData();
            return;
        }
        if name == "expansion_enabled" {
            self.delegate
                .ivars()
                .expansion_enabled
                .set(value.and_then(JsonValue::as_bool).unwrap_or(true));
            return;
        }
        if !matches!(name, "items" | "selected") {
            return;
        }
        self.delegate.ivars().updating.set(true);
        if name == "items" {
            self.reload(
                value
                    .and_then(|value| serde_json::from_value(value.clone()).ok())
                    .unwrap_or_default(),
            );
        } else {
            *self.delegate.ivars().selected.borrow_mut() =
                value.and_then(JsonValue::as_str).map(str::to_owned);
        }
        let object = self
            .delegate
            .ivars()
            .data
            .borrow()
            .items
            .iter()
            .find(|item| Some(&item.row.id) == self.delegate.ivars().selected.borrow().as_ref())
            .map(|item| item.object.clone());
        let row = unsafe {
            self.outline
                .rowForItem(object.as_deref().map(|object| &***object))
        };
        if self.outline.selectedRow() != row {
            if row >= 0 {
                self.outline.selectRowIndexes_byExtendingSelection(
                    &NSIndexSet::indexSetWithIndex(row as usize),
                    false,
                );
            } else {
                unsafe {
                    self.outline.deselectAll(Some(&self.outline));
                }
            }
        }
        self.delegate.ivars().updating.set(false);
    }
}

impl Drop for List {
    fn drop(&mut self) {
        unsafe {
            self.outline.setDelegate(None);
            self.outline.setDataSource(None);
            self.outline.setTarget(None);
        }
    }
}
