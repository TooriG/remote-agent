//! Runs a scenario on the app's own windows through AppKit's accessibility tree,
//! as VoiceOver sees them: NSH views and AppKit controls answer the same
//! `NSAccessibility` labels, values and presses. Menus are tracked from AppKit's
//! notifications, since a menu's items live outside the windows' trees.
//!
//! A repeating timer in the run loop's common modes takes the steps, so they go
//! on while a menu or a modal alert runs its own loop. A press that opens one
//! returns only when it closes, so the step is recorded before pressing and the
//! press runs in a callout of its own, inside which the next ticks run.
//!
//! The driver posts mid-scenario `fixture` steps to the fixture Host's control
//! server, whose port the runner passes as `BEX_ACCEPTANCE_FIXTURE`. It keeps
//! the clipboard's items from the start and puts them back at the end, since
//! the app's copy actions write the user's clipboard.
//!
//! The runner reads `result.json` from the output directory: `passed` (null while
//! running) and one entry per step with `line`, `source`, `status` (`passed`,
//! `failed` or `skipped`), `message`, `elapsedMs` and the step's `artifact` file;
//! `evidence` names the screenshot and tree written when a step fails.
use super::script::{Composition, Find, Key, Line, Step};
use objc2::{
    MainThreadMarker, Message, msg_send,
    rc::Retained,
    runtime::{AnyObject, ProtocolObject, Sel},
    sel,
};
use objc2_app_kit::{
    NSApplication, NSBitmapImageFileType, NSEvent, NSEventModifierFlags, NSEventType, NSMenu,
    NSMenuDidBeginTrackingNotification, NSMenuDidEndTrackingNotification, NSModalResponseOK,
    NSOpenPanel, NSPasteboard, NSPasteboardItem, NSPasteboardTypeString, NSPasteboardWriting,
    NSResponder, NSScreen, NSView, NSWindow,
};
use objc2_core_graphics::{CGEvent, CGScrollEventUnit};
use objc2_foundation::{
    NSArray, NSAttributedString, NSData, NSDictionary, NSNotFound, NSNotification,
    NSNotificationCenter, NSNumber, NSPoint, NSRange, NSRect, NSRunLoop, NSRunLoopCommonModes,
    NSString, NSTimer, NSURL,
};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    path::{Path, PathBuf},
    ptr::NonNull,
    rc::Rc,
    time::{Duration, Instant},
};

const TICK: f64 = 0.2;

/// Schedules `lines` on the main run loop. The returned flag says whether every
/// step passed once the app returns from its event loop.
pub(crate) fn start(
    scenario: PathBuf,
    lines: Vec<Line>,
    output: PathBuf,
    fixture: Option<u16>,
    mtm: MainThreadMarker,
) -> Rc<Cell<bool>> {
    let passed = Rc::new(Cell::new(false));
    let driver = Rc::new(RefCell::new(Driver {
        scenario,
        output,
        fixture,
        clipboard: Clipboard::save(),
        lines,
        index: 0,
        since: Instant::now(),
        ready: false,
        finished: false,
        steps: Vec::new(),
        evidence: Vec::new(),
        menus: Vec::new(),
        passed: passed.clone(),
    }));
    let center = NSNotificationCenter::defaultCenter();
    for (name, open) in [
        (unsafe { NSMenuDidBeginTrackingNotification }, true),
        (unsafe { NSMenuDidEndTrackingNotification }, false),
    ] {
        let driver = driver.clone();
        let block = block2::RcBlock::new(move |notification: NonNull<NSNotification>| {
            let notification = unsafe { notification.as_ref() };
            let Some(menu) = notification
                .object()
                .and_then(|object| object.downcast::<NSMenu>().ok())
            else {
                return;
            };
            let mut driver = driver.borrow_mut();
            driver
                .menus
                .retain(|tracked| !std::ptr::eq(&**tracked, &*menu));
            if open {
                driver.menus.push(menu);
            }
        });
        // The observers last as long as the process: the driver ends it.
        std::mem::forget(unsafe {
            center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block)
        });
    }
    let block = block2::RcBlock::new(move |_: NonNull<NSTimer>| tick(&driver, mtm));
    let timer = unsafe { NSTimer::timerWithTimeInterval_repeats_block(TICK, true, &block) };
    unsafe { NSRunLoop::mainRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes) };
    passed
}

struct Driver {
    scenario: PathBuf,
    output: PathBuf,
    /// The fixture Host's control port.
    fixture: Option<u16>,
    clipboard: Clipboard,
    lines: Vec<Line>,
    index: usize,
    since: Instant,
    ready: bool,
    finished: bool,
    steps: Vec<Value>,
    evidence: Vec<String>,
    menus: Vec<Retained<NSMenu>>,
    passed: Rc<Cell<bool>>,
}

impl Driver {
    fn record(&mut self, status: &str, message: &str, artifact: Option<String>) {
        let line = &self.lines[self.index];
        self.steps.push(json!({
            "line": line.number,
            "source": line.source,
            "status": status,
            "message": message,
            "elapsedMs": self.since.elapsed().as_millis() as u64,
            "artifact": artifact,
        }));
        self.index += 1;
        self.since = Instant::now();
        self.write(None);
    }

    fn write(&self, passed: Option<bool>) {
        let report = json!({
            "scenario": self.scenario,
            "passed": passed,
            "steps": self.steps,
            "evidence": self.evidence,
        });
        if let Err(reason) = std::fs::write(
            self.output.join("result.json"),
            serde_json::to_vec_pretty(&report).unwrap_or_default(),
        ) {
            eprintln!("acceptance: result.json: {reason}");
        }
    }
}

/// What a step found this tick.
enum Attempt {
    Pending(String),
    Passed(String, Option<String>),
    Press(String, Target),
    Failed(String),
}

fn tick(driver: &Rc<RefCell<Driver>>, mtm: MainThreadMarker) {
    let (timeout, step, elapsed, menus, output, fixture) = {
        let mut driver = driver.borrow_mut();
        if driver.finished {
            return;
        }
        if !driver.ready {
            let Some(window) = main_window(mtm) else {
                return;
            };
            // Typed keys reach the key window's monitors only while the app is active.
            NSApplication::sharedApplication(mtm).activate();
            window.makeKeyAndOrderFront(None);
            driver.ready = true;
            driver.since = Instant::now();
        }
        let step = driver.lines[driver.index].step.clone();
        (
            limit(&step),
            step,
            driver.since.elapsed(),
            driver.menus.clone(),
            driver.output.clone(),
            driver.fixture,
        )
    };
    match attempt(&step, elapsed, &menus, &output, fixture, mtm) {
        Attempt::Passed(message, artifact) => {
            driver.borrow_mut().record("passed", &message, artifact)
        }
        Attempt::Press(message, target) => {
            driver.borrow_mut().record("passed", &message, None);
            // A press that opens a menu returns when the menu closes, and a timer
            // does not fire again during its own callout: the press gets a callout
            // of its own, and the next steps run in the menu's loop meanwhile.
            let press = block2::RcBlock::new(move |_: NonNull<NSTimer>| target.press());
            let timer = unsafe { NSTimer::timerWithTimeInterval_repeats_block(0.0, false, &press) };
            unsafe { NSRunLoop::mainRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes) };
        }
        Attempt::Pending(reason) if elapsed >= timeout => return fail(driver, &reason, mtm),
        Attempt::Pending(_) => {}
        Attempt::Failed(reason) => return fail(driver, &reason, mtm),
    }
    let done = {
        let driver = driver.borrow();
        !driver.finished && driver.index == driver.lines.len()
    };
    if done {
        finish(driver, true);
    }
}

/// How long `step` may take.
fn limit(step: &Step) -> Duration {
    match step {
        Step::Press(find)
        | Step::Type(find, _)
        | Step::Compose(find, _)
        | Step::Scroll(find, _)
        | Step::Select(find, _)
        | Step::Menu(find)
        | Step::Expect(find)
        | Step::ExpectAbsent(find) => find.timeout,
        Step::Wait(duration) | Step::ExpectClipboard(_, duration) => *duration,
        _ => super::script::DEFAULT_TIMEOUT,
    }
}

/// `fixture` is the fixture Host's control port.
fn attempt(
    step: &Step,
    elapsed: Duration,
    menus: &[Retained<NSMenu>],
    output: &Path,
    fixture: Option<u16>,
    mtm: MainThreadMarker,
) -> Attempt {
    match step {
        Step::Fixture(_) => Attempt::Passed("prepared by the runner".into(), None),
        Step::Control(path) => done(
            fixture
                .ok_or_else(|| "the runner gave no fixture control port".to_owned())
                .and_then(|port| post(port, path))
                .map(|()| format!("posted {path}")),
        ),
        Step::Wait(duration) if elapsed >= *duration => {
            Attempt::Passed(format!("waited {} ms", duration.as_millis()), None)
        }
        Step::Wait(_) => Attempt::Pending(String::new()),
        Step::Screenshot(name) => artifact(screenshot(mtm, output, name)),
        Step::Dump(name) => artifact(dump(&Tree::read(mtm, menus), output, name)),
        Step::Expect(find) => shown(&Tree::read(mtm, menus), find, true),
        Step::ExpectAbsent(find) => shown(&Tree::read(mtm, menus), find, false),
        Step::Press(find) => press(&Tree::read(mtm, menus), find),
        Step::Type(find, text) => on_element(&Tree::read(mtm, menus), find, |tree, index| {
            insert(tree, index, text)
                .map(|()| format!("typed into {}", tree.nodes[index].describe()))
        }),
        Step::Compose(find, composition) => {
            on_element(&Tree::read(mtm, menus), find, |tree, index| {
                compose(tree, index, composition)
                    .map(|()| format!("{composition:?} in {}", tree.nodes[index].describe()))
            })
        }
        Step::Scroll(find, lines) => on_element(&Tree::read(mtm, menus), find, |tree, index| {
            scroll(tree, index, *lines, mtm).map(|()| {
                format!(
                    "scrolled {lines} lines over {}",
                    tree.nodes[index].describe()
                )
            })
        }),
        Step::Select(find, text) => on_element(&Tree::read(mtm, menus), find, |tree, index| {
            select(tree, index, text)
                .map(|()| format!("selected {text:?} in {}", tree.nodes[index].describe()))
        }),
        Step::Menu(find) => context_menu(&Tree::read(mtm, menus), find),
        Step::Choose(path) => choose(path, mtm),
        Step::ExpectClipboard(text, _) => {
            let copied = Clipboard::text();
            if copied.contains(text.as_str()) {
                Attempt::Passed(format!("the clipboard has {copied:?}"), None)
            } else {
                Attempt::Pending(format!("the clipboard has {copied:?}"))
            }
        }
        Step::Key(key) => {
            done(post_key(key, mtm).map(|()| format!("posted key code {}", key.code)))
        }
    }
}

fn done(result: Result<String, String>) -> Attempt {
    match result {
        Ok(message) => Attempt::Passed(message, None),
        Err(reason) => Attempt::Failed(reason),
    }
}

/// Whether some element after `find`'s anchor shows its text, as `present` wants.
fn shown(tree: &Tree, find: &Find, present: bool) -> Attempt {
    match (
        tree.start(find)
            .map(|start| tree.containing(&find.text, start)),
        present,
    ) {
        (Ok(Some(index)), true) => {
            Attempt::Passed(format!("found in {}", tree.nodes[index].describe()), None)
        }
        (Ok(Some(index)), false) => {
            Attempt::Pending(format!("still shown in {}", tree.nodes[index].describe()))
        }
        (Ok(None), true) => Attempt::Pending(format!("no element shows {:?}", find.text)),
        (Ok(None), false) => Attempt::Passed(format!("no element shows {:?}", find.text), None),
        // Without its anchor, nothing after it is shown.
        (Err(reason), true) => Attempt::Pending(reason),
        (Err(reason), false) => Attempt::Passed(reason, None),
    }
}

fn press(tree: &Tree, find: &Find) -> Attempt {
    let matches = match tree.labelled(find) {
        Ok(matches) => matches,
        Err(reason) => return Attempt::Pending(reason),
    };
    if let Some(target) = matches.iter().find_map(|&index| tree.link(index)) {
        return Attempt::Press(
            format!("clicked {}", tree.nodes[matches[0]].describe()),
            target,
        );
    }
    // An element that takes no press (a label in a row) passes it to the
    // nearest ancestor that does.
    match matches
        .iter()
        .flat_map(|&index| tree.lineage(index))
        .find(|&index| tree.nodes[index].target.pressable())
    {
        Some(index) => Attempt::Press(
            format!("pressed {}", tree.nodes[index].describe()),
            tree.nodes[index].target.clone(),
        ),
        None => Attempt::Pending(format!(
            "{} takes no press now (disabled?)",
            tree.nodes[matches[0]].describe()
        )),
    }
}

/// `act` on the first element `find` names, once one is shown.
fn on_element(
    tree: &Tree,
    find: &Find,
    act: impl FnOnce(&Tree, usize) -> Result<String, String>,
) -> Attempt {
    match tree.labelled(find) {
        Ok(matches) => done(act(tree, matches[0])),
        Err(reason) => Attempt::Pending(reason),
    }
}

fn artifact(written: Result<String, String>) -> Attempt {
    match written {
        Ok(file) => Attempt::Passed(format!("wrote {file}"), Some(file)),
        Err(reason) => Attempt::Failed(reason),
    }
}

fn fail(driver: &Rc<RefCell<Driver>>, reason: &str, mtm: MainThreadMarker) {
    let (output, menus) = {
        let driver = driver.borrow();
        (driver.output.clone(), driver.menus.clone())
    };
    let evidence: Vec<String> = [
        screenshot(mtm, &output, "failure"),
        dump(&Tree::read(mtm, &menus), &output, "failure-tree"),
    ]
    .into_iter()
    .filter_map(|written| {
        written
            .map_err(|reason| eprintln!("acceptance: {reason}"))
            .ok()
    })
    .collect();
    {
        let mut driver = driver.borrow_mut();
        driver.evidence = evidence;
        driver.record("failed", reason, None);
        while driver.index < driver.lines.len() {
            driver.record("skipped", "", None);
        }
    }
    finish(driver, false);
}

fn finish(driver: &Rc<RefCell<Driver>>, passed: bool) {
    let menus = {
        let mut driver = driver.borrow_mut();
        driver.finished = true;
        driver.passed.set(passed);
        driver.write(Some(passed));
        driver.clipboard.restore();
        std::mem::take(&mut driver.menus)
    };
    for menu in menus {
        menu.cancelTracking();
    }
    nsh_platform_macos::quit_app();
}

/// The window the app draws in: the largest visible one.
fn main_window(mtm: MainThreadMarker) -> Option<Retained<NSWindow>> {
    NSApplication::sharedApplication(mtm)
        .orderedWindows()
        .iter()
        .filter(|window| window.isVisible() && window.contentView().is_some())
        .max_by(|a, b| {
            let area = |window: &NSWindow| window.frame().size.width * window.frame().size.height;
            area(a).total_cmp(&area(b))
        })
}

/// What a press acts on.
#[derive(Clone)]
enum Target {
    Element(Retained<AnyObject>),
    MenuItem {
        menu: Retained<NSMenu>,
        index: isize,
        root: Retained<NSMenu>,
    },
    /// A right click on a view at a point of its window.
    ContextMenu {
        view: Retained<NSView>,
        point: NSPoint,
    },
    /// A link of a text view at a character, which offers no press of its own.
    Link {
        view: Retained<AnyObject>,
        link: Retained<AnyObject>,
        index: usize,
    },
}

impl Target {
    /// Whether it offers `AXPress` and is enabled. AppKit's controls do not
    /// report through the press's result whether it did anything, so this is
    /// asked first.
    fn pressable(&self) -> bool {
        match self {
            Self::Element(object) => {
                let press = sel!(accessibilityPerformPress);
                responds(object, press)
                    && (!responds(object, sel!(isAccessibilitySelectorAllowed:))
                        || unsafe { msg_send![&**object, isAccessibilitySelectorAllowed: press] })
                    && (!responds(object, sel!(isAccessibilityEnabled))
                        || unsafe { msg_send![&**object, isAccessibilityEnabled] })
            }
            Self::MenuItem { menu, index, .. } => menu
                .itemAtIndex(*index)
                .is_some_and(|item| item.isEnabled()),
            Self::ContextMenu { .. } | Self::Link { .. } => true,
        }
    }

    fn press(&self) {
        match self {
            Self::Element(object) => {
                let _: bool = unsafe { msg_send![&**object, accessibilityPerformPress] };
            }
            Self::MenuItem { menu, index, root } => {
                // As a click on the item: its action, then the menu closes.
                menu.performActionForItemAtIndex(*index);
                root.cancelTracking();
            }
            Self::Link { view, link, index } => {
                let _: () = unsafe { msg_send![&**view, clickedOnLink: &**link, atIndex: *index] };
            }
            Self::ContextMenu { view, point } => {
                let Some(window) = view.window() else {
                    return;
                };
                let event = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
                    NSEventType::RightMouseDown, *point, NSEventModifierFlags::empty(), 0.0, window.windowNumber(), None, 0, 1, 1.0,
                );
                if let Some(event) = event
                    && let Some(menu) = view.menuForEvent(&event)
                {
                    NSMenu::popUpContextMenu_withEvent_forView(&menu, &event, view);
                }
            }
        }
    }
}

struct Node {
    target: Target,
    parent: Option<usize>,
    depth: usize,
    role: String,
    /// The attributes that have text, by name.
    texts: Vec<(&'static str, String)>,
    window: Option<Retained<NSWindow>>,
}

impl Node {
    fn describe(&self) -> String {
        let mut text = self
            .texts
            .first()
            .map(|(_, text)| text.clone())
            .unwrap_or_default();
        if text.chars().count() > 60 {
            text = text.chars().take(60).collect::<String>() + "…";
        }
        format!("{} {text:?}", self.role)
    }
}

/// Every element of the visible windows and open menus, parents before children.
struct Tree {
    nodes: Vec<Node>,
}

const MAX_NODES: usize = 50_000;

impl Tree {
    fn read(mtm: MainThreadMarker, menus: &[Retained<NSMenu>]) -> Self {
        let mut tree = Self { nodes: Vec::new() };
        for window in NSApplication::sharedApplication(mtm).orderedWindows() {
            if window.isVisible() {
                let object = unsafe { Retained::cast_unchecked::<AnyObject>(window.clone()) };
                tree.walk(object, None, 0, &window);
            }
        }
        for root in menus {
            tree.menu(root, root, None, 0);
        }
        tree
    }

    fn walk(
        &mut self,
        object: Retained<AnyObject>,
        parent: Option<usize>,
        depth: usize,
        window: &Retained<NSWindow>,
    ) {
        if self.nodes.len() >= MAX_NODES || depth > 200 {
            return;
        }
        let mut texts = Vec::new();
        for (name, selector) in [
            ("label", sel!(accessibilityLabel)),
            ("title", sel!(accessibilityTitle)),
            ("value", sel!(accessibilityValue)),
            ("identifier", sel!(accessibilityIdentifier)),
            ("placeholder", sel!(accessibilityPlaceholderValue)),
        ] {
            if let Some(text) = text(&object, selector)
                && !text.trim().is_empty()
                && !texts.iter().any(|(_, seen)| *seen == text)
            {
                texts.push((name, text));
            }
        }
        let index = self.nodes.len();
        let children: Option<Retained<NSArray<AnyObject>>> =
            if responds(&object, sel!(accessibilityChildren)) {
                unsafe { msg_send![&*object, accessibilityChildren] }
            } else {
                None
            };
        self.nodes.push(Node {
            role: text(&object, sel!(accessibilityRole)).unwrap_or_default(),
            target: Target::Element(object),
            parent,
            depth,
            texts,
            window: Some(window.clone()),
        });
        for child in children.iter().flat_map(|children| children.iter()) {
            self.walk(child, Some(index), depth + 1, window);
        }
    }

    fn menu(
        &mut self,
        menu: &Retained<NSMenu>,
        root: &Retained<NSMenu>,
        parent: Option<usize>,
        depth: usize,
    ) {
        for index in 0..menu.numberOfItems() {
            let Some(item) = menu.itemAtIndex(index) else {
                continue;
            };
            if item.isSeparatorItem() || item.isHidden() {
                continue;
            }
            let node = self.nodes.len();
            self.nodes.push(Node {
                target: Target::MenuItem {
                    menu: menu.clone(),
                    index,
                    root: root.clone(),
                },
                parent,
                depth,
                role: "AXMenuItem".into(),
                texts: vec![("title", item.title().to_string())],
                window: None,
            });
            if let Some(submenu) = item.submenu() {
                self.menu(&submenu, root, Some(node), depth + 1);
            }
        }
    }

    /// Where `find`'s search begins: after its anchor, if it names one.
    fn start(&self, find: &Find) -> Result<usize, String> {
        let Some(anchor) = &find.after else {
            return Ok(0);
        };
        self.nodes
            .iter()
            .position(|node| {
                node.texts
                    .iter()
                    .any(|(_, text)| super::script::labels(text, anchor))
            })
            .map(|index| index + 1)
            .ok_or_else(|| format!("no element is labelled {anchor:?} to search after"))
    }

    fn containing(&self, needle: &str, start: usize) -> Option<usize> {
        (start..self.nodes.len()).find(|&index| {
            self.nodes[index]
                .texts
                .iter()
                .any(|(_, text)| text.contains(needle))
        })
    }

    /// Elements whose text [`super::script::labels`] `find`'s; open menus' items first.
    fn labelled(&self, find: &Find) -> Result<Vec<usize>, String> {
        let start = self.start(find)?;
        let mut found: Vec<usize> = (start..self.nodes.len())
            .filter(|&index| {
                self.nodes[index]
                    .texts
                    .iter()
                    .any(|(_, text)| super::script::labels(text, &find.text))
            })
            .collect();
        found.sort_by_key(|&index| !matches!(self.nodes[index].target, Target::MenuItem { .. }));
        if found.is_empty() {
            return Err(self.missing(&find.text));
        }
        Ok(found)
    }

    /// A click on the link `index` is, as its text view takes one: AppKit's
    /// links in text answer no press.
    fn link(&self, index: usize) -> Option<Target> {
        let node = &self.nodes[index];
        let label = node.texts.first().map(|(_, text)| text)?;
        let Target::Element(view) = &self.nodes[node.parent?].target else {
            return None;
        };
        if node.role != "AXLink" || !responds(view, sel!(clickedOnLink:atIndex:)) {
            return None;
        }
        let string = text(view, sel!(string))?;
        let index = string[..string.find(label.as_str())?]
            .encode_utf16()
            .count();
        let storage: Retained<AnyObject> = unsafe { msg_send![&**view, textStorage] };
        let name = NSString::from_str("NSLink");
        let link: Option<Retained<AnyObject>> = unsafe {
            msg_send![&*storage, attribute: &*name, atIndex: index, effectiveRange: std::ptr::null_mut::<NSRange>()]
        };
        Some(Target::Link {
            view: view.clone(),
            link: link?,
            index,
        })
    }

    /// `index` and its ancestors, nearest first.
    fn lineage(&self, index: usize) -> Vec<usize> {
        std::iter::successors(Some(index), |&index| self.nodes[index].parent).collect()
    }

    fn missing(&self, label: &str) -> String {
        let similar: Vec<String> = self
            .nodes
            .iter()
            .filter(|node| {
                node.texts
                    .iter()
                    .any(|(_, text)| text.to_lowercase().contains(&label.to_lowercase()))
            })
            .take(8)
            .map(Node::describe)
            .collect();
        if similar.is_empty() {
            format!("no element is labelled {label:?}")
        } else {
            format!(
                "no element is labelled {label:?}; similar: {}",
                similar.join(", ")
            )
        }
    }

    fn text(&self) -> String {
        let mut text = String::new();
        for node in &self.nodes {
            text.push_str(&"  ".repeat(node.depth));
            text.push_str(&node.role);
            for (name, value) in &node.texts {
                text.push_str(&format!(" {name}={value:?}"));
            }
            text.push('\n');
        }
        text
    }
}

fn responds(object: &AnyObject, selector: Sel) -> bool {
    unsafe { msg_send![object, respondsToSelector: selector] }
}

/// An object-valued accessibility attribute as text.
fn text(object: &AnyObject, selector: Sel) -> Option<String> {
    if !responds(object, selector) {
        return None;
    }
    let value: Option<Retained<AnyObject>> =
        unsafe { msg_send![object, performSelector: selector] };
    let value = value?;
    if let Some(string) = value.downcast_ref::<NSString>() {
        Some(string.to_string())
    } else if let Some(string) = value.downcast_ref::<NSAttributedString>() {
        Some(string.string().to_string())
    } else {
        value
            .downcast_ref::<NSNumber>()
            .map(|number| number.stringValue().to_string())
    }
}

fn insert(tree: &Tree, index: usize, text: &str) -> Result<(), String> {
    let responder = editor(tree, index)?;
    let text = NSString::from_str(text);
    let _: () = unsafe { msg_send![&*responder, insertText: &*text] };
    Ok(())
}

/// Takes `composition` as AppKit hands an input method's work to the focused
/// text view: no input source changes, so the user's is never touched.
fn compose(tree: &Tree, index: usize, composition: &Composition) -> Result<(), String> {
    let responder = editor(tree, index)?;
    // No replacement range: the marked text, or else the selection.
    let marked = NSRange::new(NSNotFound as usize, 0);
    match composition {
        Composition::Mark(text) => {
            let caret = NSRange::new(text.encode_utf16().count(), 0);
            let text = NSString::from_str(text);
            let _: () = unsafe {
                msg_send![&*responder, setMarkedText: &*text, selectedRange: caret, replacementRange: marked]
            };
        }
        Composition::Commit(text) => {
            let text = NSString::from_str(text);
            let _: () =
                unsafe { msg_send![&*responder, insertText: &*text, replacementRange: marked] };
        }
        Composition::Unmark => {
            let _: () = unsafe { msg_send![&*responder, unmarkText] };
        }
    }
    Ok(())
}

/// The focused text view editing the element's text.
fn editor(tree: &Tree, index: usize) -> Result<Retained<NSResponder>, String> {
    let node = &tree.nodes[index];
    let (Target::Element(object), Some(window)) = (&node.target, &node.window) else {
        return Err(format!("{} takes no text", node.describe()));
    };
    // A control's cell (a search field's) stands for it in accessibility.
    let view: Option<Retained<NSView>> = match object.downcast_ref::<NSView>() {
        Some(view) => Some(view.retain()),
        None if responds(object, sel!(controlView)) => unsafe { msg_send![&**object, controlView] },
        None => None,
    };
    let focused = view.is_some_and(|view| focus(window, &view));
    if !focused {
        return Err(format!(
            "{} could not take the keyboard focus",
            node.describe()
        ));
    }
    let responder = window
        .firstResponder()
        .ok_or("the window has no first responder")?;
    if !responds(
        &responder,
        sel!(setMarkedText:selectedRange:replacementRange:),
    ) {
        return Err(format!("{} does not edit text", node.describe()));
    }
    Ok(responder)
}

/// Focuses `view`, or the first view inside it that takes the focus (a field in
/// its padding).
fn focus(window: &NSWindow, view: &NSView) -> bool {
    if view.acceptsFirstResponder() && window.makeFirstResponder(Some(view)) {
        return true;
    }
    view.subviews().iter().any(|child| focus(window, &child))
}

fn post_key(key: &Key, mtm: MainThreadMarker) -> Result<(), String> {
    let app = NSApplication::sharedApplication(mtm);
    let window = app
        .keyWindow()
        .or_else(|| main_window(mtm))
        .ok_or("no window takes keys")?;
    let mut flags = NSEventModifierFlags::empty();
    for (on, flag) in [
        (key.command, NSEventModifierFlags::Command),
        (key.shift, NSEventModifierFlags::Shift),
        (key.option, NSEventModifierFlags::Option),
        (key.control, NSEventModifierFlags::Control),
        // Arrow keys, as AppKit reports them.
        (
            (123..=126).contains(&key.code),
            NSEventModifierFlags::NumericPad | NSEventModifierFlags::Function,
        ),
    ] {
        if on {
            flags |= flag;
        }
    }
    let characters = NSString::from_str(&key.characters);
    for kind in [NSEventType::KeyDown, NSEventType::KeyUp] {
        let event = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
            kind, NSPoint::new(0.0, 0.0), flags, 0.0, window.windowNumber(), None, &characters, &characters, false, key.code,
        ).ok_or("AppKit made no key event")?;
        // Queued as typing is, so the keymap's local monitor sees it.
        app.postEvent_atStart(&event, false);
    }
    Ok(())
}

/// A mouse wheel over the element's center, handed to its window without moving
/// the pointer: NSH routes a wheel by where the event says it is.
fn scroll(tree: &Tree, index: usize, lines: i32, mtm: MainThreadMarker) -> Result<(), String> {
    let node = &tree.nodes[index];
    let (Target::Element(object), Some(window)) = (&node.target, &node.window) else {
        return Err(format!("{} cannot be scrolled", node.describe()));
    };
    let frame = window.convertRectFromScreen(unsafe { msg_send![&**object, accessibilityFrame] });
    let height = NSScreen::screens(mtm)
        .firstObject()
        .map(|screen| screen.frame().size.height)
        .ok_or("no screen")?;
    // Content follows the wheel, so content moving up is a negative delta.
    let event = CGEvent::new_scroll_wheel_event2(None, CGScrollEventUnit::Line, 1, -lines, 0, 0)
        .ok_or("CoreGraphics made no wheel event")?;
    // A window-less event's location in its window is its screen point, which
    // the window takes as its own: place it so that they match.
    CGEvent::set_location(
        Some(&event),
        objc2_core_foundation::CGPoint::new(
            frame.origin.x + frame.size.width / 2.0,
            height - (frame.origin.y + frame.size.height / 2.0),
        ),
    );
    let event = NSEvent::eventWithCGEvent(&event).ok_or("AppKit took no wheel event")?;
    window.sendEvent(&event);
    Ok(())
}

fn screenshot(mtm: MainThreadMarker, output: &Path, name: &str) -> Result<String, String> {
    let view = main_window(mtm)
        .and_then(|window| window.contentView())
        .ok_or("no window to capture")?;
    let bounds = view.bounds();
    let rep = view
        .bitmapImageRepForCachingDisplayInRect(bounds)
        .ok_or("the window cannot be captured")?;
    view.cacheDisplayInRect_toBitmapImageRep(bounds, &rep);
    let data = unsafe {
        rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
    }
    .ok_or("the capture has no PNG")?;
    let file = format!("{name}.png");
    std::fs::write(output.join(&file), data.to_vec())
        .map_err(|reason| format!("{file}: {reason}"))?;
    Ok(file)
}

fn dump(tree: &Tree, output: &Path, name: &str) -> Result<String, String> {
    let file = format!("{name}.txt");
    std::fs::write(output.join(&file), tree.text())
        .map_err(|reason| format!("{file}: {reason}"))?;
    Ok(file)
}

/// Posts `path` to the fixture Host's loopback control server.
fn post(port: u16, path: &str) -> Result<(), String> {
    use std::io::{Read, Write};
    let failed = |reason: std::io::Error| format!("fixture control: {reason}");
    let mut stream =
        std::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).map_err(failed)?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(failed)?;
    write!(
        stream,
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )
    .map_err(failed)?;
    let mut response = String::new();
    stream.read_to_string(&mut response).map_err(failed)?;
    let status = response.split_whitespace().nth(1).unwrap_or_default();
    if status.starts_with('2') {
        Ok(())
    } else {
        Err(format!("fixture {path} answered {status:?}"))
    }
}

/// Selects the first `text` in the element's text through its accessibility.
fn select(tree: &Tree, index: usize, text: &str) -> Result<(), String> {
    let node = &tree.nodes[index];
    let Target::Element(object) = &node.target else {
        return Err(format!("{} has no text to select", node.describe()));
    };
    if !responds(object, sel!(setAccessibilitySelectedTextRange:)) {
        return Err(format!("{} takes no selection", node.describe()));
    }
    let value = self::text(object, sel!(accessibilityValue)).unwrap_or_default();
    let start = value
        .find(text)
        .ok_or_else(|| format!("{} does not contain {text:?}", node.describe()))?;
    let range = NSRange::new(
        value[..start].encode_utf16().count(),
        text.encode_utf16().count(),
    );
    let _: () = unsafe { msg_send![&**object, setAccessibilitySelectedTextRange: range] };
    Ok(())
}

/// A right click on the first element `find` names: on its selected text, if
/// it has some, as the selection's own menu shows there.
fn context_menu(tree: &Tree, find: &Find) -> Attempt {
    let index = match tree.labelled(find) {
        Ok(matches) => matches[0],
        Err(reason) => return Attempt::Pending(reason),
    };
    let node = &tree.nodes[index];
    let (Target::Element(object), Some(window)) = (&node.target, &node.window) else {
        return Attempt::Failed(format!("{} shows no context menu", node.describe()));
    };
    let Some(view) = object.downcast_ref::<NSView>() else {
        return Attempt::Failed(format!("{} is not a view", node.describe()));
    };
    let selected = responds(object, sel!(accessibilitySelectedTextRange))
        .then(|| -> NSRange { unsafe { msg_send![&**object, accessibilitySelectedTextRange] } })
        .filter(|range| range.length > 0);
    let frame: NSRect = match selected {
        Some(range) => unsafe { msg_send![&**object, accessibilityFrameForRange: range] },
        None => unsafe { msg_send![&**object, accessibilityFrame] },
    };
    let frame = window.convertRectFromScreen(frame);
    Attempt::Press(
        format!("opened the context menu of {}", node.describe()),
        Target::ContextMenu {
            view: view.retain(),
            point: NSPoint::new(
                frame.origin.x + frame.size.width / 2.0,
                frame.origin.y + frame.size.height / 2.0,
            ),
        },
    )
}

thread_local! {
    /// What the open panel answers once `choose` ends it.
    static CHOSEN: RefCell<Option<Retained<NSArray<NSURL>>>> =
        const { RefCell::new(None) };
}

/// The open panel's `URLs` while the driver chooses: AppKit offers no way to
/// select a file in it, and its view runs in another process.
unsafe extern "C-unwind" fn chosen_urls(_: *mut AnyObject, _: Sel) -> *mut NSArray<NSURL> {
    Retained::autorelease_return(CHOSEN.with_borrow(|urls| urls.clone().unwrap_or_default()))
}

/// Picks `path` in the app's open panel once it shows: the panel answers it as
/// the user's choice and closes as its Open button would.
fn choose(path: &str, mtm: MainThreadMarker) -> Attempt {
    let path = match path.strip_prefix(super::script::STATE_DIRECTORY) {
        Some(rest) => match std::env::var("AGENT_STATE_DIR") {
            Ok(directory) => format!("{directory}{rest}"),
            Err(_) => return Attempt::Failed("AGENT_STATE_DIR is not set".into()),
        },
        None => path.to_owned(),
    };
    if !std::path::Path::new(&path).exists() {
        return Attempt::Failed(format!("{path} does not exist"));
    }
    let Some((panel, parent)) = NSApplication::sharedApplication(mtm)
        .windows()
        .iter()
        .filter(|window| window.isVisible())
        .filter_map(|window| window.downcast::<NSOpenPanel>().ok())
        .find_map(|panel| panel.sheetParent().map(|parent| (panel, parent)))
    else {
        return Attempt::Pending("no open panel shows".into());
    };
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path));
    CHOSEN.set(Some(NSArray::from_retained_slice(&[url])));
    let Some(method) = panel.class().instance_method(sel!(URLs)) else {
        return Attempt::Failed("the open panel has no URLs".into());
    };
    // SAFETY: the replacement takes and returns what `URLs` does.
    unsafe {
        method.set_implementation(std::mem::transmute::<
            unsafe extern "C-unwind" fn(*mut AnyObject, Sel) -> *mut NSArray<NSURL>,
            objc2::runtime::Imp,
        >(chosen_urls));
    }
    parent.endSheet_returnCode(&panel, NSModalResponseOK);
    Attempt::Passed(format!("chose {path}"), None)
}

/// The clipboard's items, each as its data by type.
struct Clipboard {
    items: Vec<Vec<(Retained<NSString>, Retained<NSData>)>>,
    /// The clipboard's change count then: unchanged, it is left alone.
    count: isize,
}

impl Clipboard {
    fn save() -> Self {
        let pasteboard = NSPasteboard::generalPasteboard();
        let items = pasteboard
            .pasteboardItems()
            .map(|items| {
                items
                    .iter()
                    .map(|item| {
                        item.types()
                            .iter()
                            .filter_map(|kind| item.dataForType(&kind).map(|data| (kind, data)))
                            .collect()
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self {
            items,
            count: pasteboard.changeCount(),
        }
    }

    fn restore(&self) {
        let pasteboard = NSPasteboard::generalPasteboard();
        if pasteboard.changeCount() == self.count {
            return;
        }
        pasteboard.clearContents();
        let items: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> = self
            .items
            .iter()
            .map(|kinds| {
                let item = NSPasteboardItem::new();
                for (kind, data) in kinds {
                    item.setData_forType(data, kind);
                }
                ProtocolObject::from_retained(item)
            })
            .collect();
        if !items.is_empty() {
            pasteboard.writeObjects(&NSArray::from_retained_slice(&items));
        }
    }

    fn text() -> String {
        NSPasteboard::generalPasteboard()
            .stringForType(unsafe { NSPasteboardTypeString })
            .map(|text| text.to_string())
            .unwrap_or_default()
    }
}
