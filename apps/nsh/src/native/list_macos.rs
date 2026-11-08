//! AppKit owns hierarchy, disclosure controls, keyboard navigation and selection.
use super::{ListRow, macos::symbol};
use crate::l10n::tr;
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
    NSArray, NSBundle, NSData, NSEdgeInsets, NSIndexSet, NSInteger, NSNotification, NSObject,
    NSObjectProtocol, NSString,
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
        #[unsafe(method(outlineView:heightOfRowByItem:))]
        fn height(&self, outline: &NSOutlineView, item: &AnyObject) -> f64 {
            if self.ivars().sidebar.get() && self.ivars().row(item).is_some_and(|row| row.disclosure) {
                44.0
            } else {
                outline.rowHeight()
            }
        }
        #[unsafe(method(outlineView:shouldSelectItem:))]
        fn can_select(&self, _: &NSOutlineView, item: &AnyObject) -> bool {
            self.ivars().row(item).is_some_and(|row| !row.group && row.enabled != Some(false))
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
                let identifier = NSString::from_str(match (row.group, row.menu.is_some(), row.compose.is_some()) {
                    (true, true, _) => "heading-menu",
                    (true, false, _) => "heading",
                    (false, true, true) => "project-compose",
                    (false, true, false) => "menu-item",
                    (false, false, _) => "item",
                });
                let reused = unsafe { outline.makeViewWithIdentifier_owner(&identifier, None) };
                let cell = reused
                    .and_then(|view| view.downcast::<NSTableCellView>().ok())
                    .unwrap_or_else(|| cell_view(self, &identifier, row.group, row.menu.is_some(), row.compose.is_some()));
                cell.setAccessibilityIdentifier(Some(&NSString::from_str(&row.id)));
                for stack in cell.subviews().iter().filter_map(|view| view.downcast::<NSStackView>().ok()) {
                    if self.ivars().sidebar.get() {
                        stack.setEdgeInsets(NSEdgeInsets { top: 0.0, left: 12.0 + 24.0 * f64::from(row.level), bottom: 0.0, right: 16.0 });
                    }
                    // Hidden stacks detach from subviews, but remain owned views.
                    for view in stack.views().iter() {
                        if let Some(button) = view.downcast_ref::<NSPopUpButton>() && let Some(entries) = &row.menu {
                            super::macos::set_menu_items(button, Some(entries), self);
                            button.setAccessibilityIdentifier(Some(&NSString::from_str(&format!("{}.menu", row.id))));
                        } else if let Some(button) = view.downcast_ref::<NSButton>() && let Some(compose) = &row.compose {
                            button.setIdentifier(Some(&NSString::from_str(&compose.id)));
                            button.setAccessibilityIdentifier(Some(&NSString::from_str(&compose.id)));
                            button.setEnabled(compose.enabled);
                        }
                        if let Some(indicators) = view.downcast_ref::<NSStackView>() {
                            set_indicators(indicators, row.indicators.as_ref(), row.loading == Some(true));
                        }
                    }
                }
                unsafe {
                    cell.setAccessibilityValue(row.indicators.as_ref().map(|indicators| NSString::from_str(&indicators.label())).as_deref().map(|value| &***value));
                }
                // The stack owns both outlets, including detached hidden images.
                unsafe {
                    if let Some(field) = cell.textField() {
                        field.setEnabled(row.enabled != Some(false));
                        field.setStringValue(&NSString::from_str(&row.title));
                        if self.ivars().sidebar.get() {
                            let weight = if row.group || row.disclosure { NSFontWeightSemibold } else { NSFontWeightRegular };
                            field.setFont(Some(&NSFont::systemFontOfSize_weight(if row.group { 12.0 } else { 14.0 }, weight)));
                            let color = if row.disclosure { NSColor::labelColor() } else { NSColor::secondaryLabelColor() };
                            field.setTextColor(Some(&color));
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
                        view.setHidden(image.is_none());
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
        #[unsafe(method(compose:))]
        fn compose(&self, sender: &NSButton) {
            if let Some(id) = sender.identifier() {
                self.ivars().context.emit("compose", id.to_string().into());
            }
        }
        #[unsafe(method(chose:))]
        fn chose(&self, sender: &NSMenuItem) {
            if let Some(id) = sender.representedObject().and_then(|value| value.downcast::<NSString>().ok()) {
                self.ivars().context.emit("menu", id.to_string().into());
            }
        }
        #[unsafe(method(activate:))]
        fn activate(&self, view: &NSOutlineView) {
            let id = view.itemAtRow(view.selectedRow()).and_then(|item| self.ivars().row(&item).map(|row| row.id.clone()));
            if let Some(id) = id { self.ivars().context.emit("activate", id.into()); }
        }
    }
);

fn cell_view(
    target: &Delegate,
    identifier: &NSString,
    heading: bool,
    has_menu: bool,
    has_compose: bool,
) -> Retained<NSTableCellView> {
    let mtm = target.mtm();
    let cell = NSTableCellView::initWithFrame(NSTableCellView::alloc(mtm), CGRect::ZERO);
    cell.setIdentifier(Some(identifier));
    let stack = NSStackView::initWithFrame(NSStackView::alloc(mtm), CGRect::ZERO);
    stack.setDistribution(NSStackViewDistribution::Fill);
    stack.setAlignment(NSLayoutAttribute::CenterY);
    stack.setSpacing(6.0);
    stack.setTranslatesAutoresizingMaskIntoConstraints(false);
    cell.addSubview(&stack);
    if !heading {
        let image = NSImageView::initWithFrame(NSImageView::alloc(mtm), CGRect::ZERO);
        image.setAccessibilityElement(false);
        stack.addArrangedSubview(&image);
        NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
            image.widthAnchor().constraintEqualToConstant(18.0),
            image.heightAnchor().constraintEqualToConstant(18.0),
        ]));
        unsafe {
            cell.setImageView(Some(&image));
        }
    }
    let field = NSTextField::labelWithString(&NSString::new(), mtm);
    field.setMaximumNumberOfLines(1);
    if let Some(control_cell) = field.cell() {
        control_cell.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
    }
    field.setContentHuggingPriority_forOrientation(1.0, NSLayoutConstraintOrientation::Horizontal);
    field.setContentCompressionResistancePriority_forOrientation(
        1.0,
        NSLayoutConstraintOrientation::Horizontal,
    );
    stack.addArrangedSubview(&field);
    unsafe {
        cell.setTextField(Some(&field));
    }
    if has_menu {
        let menu = super::macos::menu_button(mtm);
        menu.setBordered(false);
        menu.setAccessibilityLabel(Some(&NSString::from_str(if heading {
            tr("プロジェクトを管理")
        } else {
            tr("プロジェクトの操作")
        })));
        if let Some(item) = menu.itemAtIndex(0) {
            item.setImage(
                symbol(if heading {
                    "folder.badge.plus"
                } else {
                    "ellipsis"
                })
                .as_deref(),
            );
        }
        if let Some(control_cell) = menu
            .cell()
            .and_then(|cell| cell.downcast::<NSPopUpButtonCell>().ok())
        {
            control_cell.setArrowPosition(NSPopUpArrowPosition::NoArrow);
        }
        stack.addArrangedSubview(&menu);
        NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
            menu.widthAnchor().constraintEqualToConstant(24.0),
            menu.heightAnchor().constraintEqualToConstant(24.0),
        ]));
        if has_compose {
            let button = unsafe {
                NSButton::buttonWithTitle_target_action(
                    &NSString::new(),
                    Some(target),
                    Some(sel!(compose:)),
                    mtm,
                )
            };
            button.setImage(symbol("square.and.pencil").as_deref());
            button.setBordered(false);
            button.setAccessibilityLabel(Some(&NSString::from_str(tr(
                "このプロジェクトで新しいチャット",
            ))));
            stack.addArrangedSubview(&button);
            NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
                button.widthAnchor().constraintEqualToConstant(24.0),
                button.heightAnchor().constraintEqualToConstant(24.0),
            ]));
        }
    } else if !heading {
        let indicators = NSStackView::initWithFrame(NSStackView::alloc(mtm), CGRect::ZERO);
        indicators.setDistribution(NSStackViewDistribution::Fill);
        indicators.setAlignment(NSLayoutAttribute::CenterY);
        indicators.setSpacing(6.0);
        indicators
            .setHuggingPriority_forOrientation(1000.0, NSLayoutConstraintOrientation::Horizontal);
        stack.addArrangedSubview(&indicators);
    }
    NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
        stack
            .leadingAnchor()
            .constraintEqualToAnchor(&cell.leadingAnchor()),
        stack
            .trailingAnchor()
            .constraintEqualToAnchor_constant(&cell.trailingAnchor(), -4.0),
        stack
            .centerYAnchor()
            .constraintEqualToAnchor(&cell.centerYAnchor()),
    ]));
    cell
}

fn set_indicators(stack: &NSStackView, data: Option<&super::RowIndicators>, loading: bool) {
    for view in stack.arrangedSubviews().iter() {
        if let Some(progress) = view.downcast_ref::<NSProgressIndicator>() {
            unsafe {
                progress.stopAnimation(None);
            }
        }
        stack.removeArrangedSubview(&view);
        view.removeFromSuperview();
    }
    stack.setHidden(data.is_none() && !loading);
    let mtm = stack.mtm();
    let activity = if loading {
        Some(agent_core::presentation::thread_status::ThreadActivity::Running)
    } else {
        data.and_then(|data| data.activity)
    };
    if let Some(activity) = activity {
        use agent_core::presentation::thread_status::ThreadActivity;
        let view: Retained<NSView> = match activity {
            ThreadActivity::Running => {
                let progress = NSProgressIndicator::new(mtm);
                progress.setStyle(NSProgressIndicatorStyle::Spinning);
                progress.setControlSize(NSControlSize::Small);
                progress.setIndeterminate(true);
                unsafe {
                    progress.startAnimation(None);
                }
                progress.into_super()
            }
            ThreadActivity::Waiting | ThreadActivity::Unread => {
                let image = NSImageView::initWithFrame(NSImageView::alloc(mtm), CGRect::ZERO);
                let glyph = symbol(if activity == ThreadActivity::Waiting {
                    "person.crop.circle.badge.questionmark"
                } else {
                    "circle.fill"
                });

                image.setImage(glyph.as_deref());
                let color = if activity == ThreadActivity::Waiting {
                    NSColor::systemOrangeColor()
                } else {
                    NSColor::labelColor()
                };
                image.setContentTintColor(Some(&color));
                image.into_super().into_super()
            }
        };
        add_indicator(
            stack,
            &view,
            if activity == ThreadActivity::Unread {
                8.0
            } else {
                16.0
            },
        );
    }
    if let Some(status) = data.and_then(|data| data.worktree) {
        let unmerged = status == agent_protocol::models::WorktreeStatus::Unmerged;
        let image = NSImageView::initWithFrame(NSImageView::alloc(mtm), CGRect::ZERO);
        image.setImage(
            NSImage::imageNamed(&NSString::from_str(if unmerged {
                "GitDiff"
            } else {
                "GitMerge"
            }))
            .as_deref(),
        );
        let color = if unmerged {
            NSColor::systemOrangeColor()
        } else {
            NSColor::systemPurpleColor()
        };
        image.setContentTintColor(Some(&color));
        add_indicator(stack, &image, 16.0);
    }
}

fn add_indicator(stack: &NSStackView, view: &NSView, size: f64) {
    view.setAccessibilityElement(false);
    stack.addArrangedSubview(view);
    NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
        view.widthAnchor().constraintEqualToConstant(size),
        view.heightAnchor().constraintEqualToConstant(size),
    ]));
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
        let view = NSScrollView::initWithFrame(NSScrollView::alloc(mtm), CGRect::ZERO);
        view.setDocumentView(Some(&outline));
        view.setBorderType(NSBorderType::NoBorder);
        view.setHasVerticalScroller(true);
        view.setAutohidesScrollers(true);
        view.setDrawsBackground(false);
        let list = Self {
            view,
            outline,
            delegate,
        };
        list.style(false);
        list
    }

    fn style(&self, sidebar: bool) {
        self.outline.setStyle(if sidebar {
            NSTableViewStyle::Plain
        } else {
            NSTableViewStyle::SourceList
        });
        self.outline
            .setRowSizeStyle(NSTableViewRowSizeStyle::Custom);
        self.outline.setRowHeight(if sidebar { 32.0 } else { 24.0 });
        if sidebar {
            // Native stacks provide product spacing; AppKit still owns the
            // hierarchy, disclosure controls and keyboard navigation.
            self.outline.setIndentationPerLevel(0.0);
            self.outline.setIntercellSpacing(CGSize::ZERO);
        }
        // Changing the table style resets its background color.
        self.outline.setBackgroundColor(&NSColor::clearColor());
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
            self.style(sidebar);
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
            let candidates = value
                .and_then(JsonValue::as_array)
                .into_iter()
                .flatten()
                .filter_map(JsonValue::as_str)
                .collect::<Vec<_>>();
            let mut selected = self.delegate.ivars().selected.borrow_mut();
            // A logical selection can appear in a project and in recent chats.
            // Keep the native position the user chose while it remains valid.
            if !candidates.iter().any(|id| selected.as_deref() == Some(*id)) {
                *selected = candidates.first().map(|id| (*id).to_owned());
            }
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
