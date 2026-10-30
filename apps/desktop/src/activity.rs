//! Activity indicators animate in a layer above the cached window content.
//!
//! GPUI re-renders every view between an animating element and the window
//! root on each frame. A spinner inside the content would rebuild the whole
//! window at the display refresh rate while any task runs, so the content only
//! reserves each indicator's place and stays cached while this layer spins the
//! icons. Content that floats over an indicator must be `deferred` so that it
//! paints above this layer.
use gpui_kit::{
    component::{IconName, IconNamed},
    *,
};
use std::{
    collections::{HashMap, HashSet},
    rc::{Rc, Weak},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

/// The rotation of the `Spinner` that indicators replace.
const TURN: Duration = Duration::from_millis(800);
const PULSE: Duration = Duration::from_secs(2);

#[derive(Clone, Copy)]
enum Effect {
    Spinner,
    Pulse,
}

/// Element state keeps each place alive, including when GPUI reuses a cached
/// subtree. A window-wide reset would lose indicators in unchanged subviews.
#[derive(Default)]
struct Places(HashMap<WindowId, Vec<Weak<Place>>>);
impl Global for Places {}

struct Place {
    element_id: GlobalElementId,
    effect: Effect,
    bounds: Bounds<Pixels>,
    mask: ContentMask<Pixels>,
    color: Hsla,
}

/// Reserves the place of a spinning activity indicator.
pub(crate) fn indicator() -> Stateful<Div> {
    placeholder(Effect::Spinner, None).flex_none().size_3p5()
}

/// The same two-second, half-opacity pulse as an image Skeleton, drawn without
/// invalidating the image, conversation, or composer that reserves its space.
pub(crate) fn pulse(background: Hsla) -> Stateful<Div> {
    placeholder(Effect::Pulse, Some(background)).size_full()
}

fn placeholder(effect: Effect, color: Option<Hsla>) -> Stateful<Div> {
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    div().id(("activity-indicator", id)).child(
        canvas(
            move |bounds, window, cx| {
                window.with_global_id("place".into(), |id, window| {
                    let place = Rc::new(Place {
                        element_id: id.clone(),
                        effect,
                        bounds,
                        mask: window.content_mask(),
                        color: color.unwrap_or_else(|| window.text_style().color),
                    });
                    if let Some(places) = cx
                        .default_global::<Places>()
                        .0
                        .get_mut(&window.window_handle().window_id())
                    {
                        places.push(Rc::downgrade(&place));
                    }
                    window.with_element_state(id, |_: Option<Rc<Place>>, _| ((), place));
                });
            },
            |_, _, _, _| {},
        )
        .size_full(),
    )
}

/// Window root that keeps its content cached while the indicators spin.
pub(crate) struct ActivityWindow {
    content: AnyView,
    started: Instant,
}

impl ActivityWindow {
    pub(crate) fn new(
        content: impl Into<AnyView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let id = window.window_handle().window_id();
        cx.default_global::<Places>().0.insert(id, Vec::new());
        cx.on_release(move |_, cx| {
            cx.default_global::<Places>().0.remove(&id);
        })
        .detach();
        Self {
            content: content.into(),
            started: Instant::now(),
        }
    }
}

impl Render for ActivityWindow {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let started = self.started;
        div()
            .relative()
            .size_full()
            .child(
                self.content
                    .clone()
                    .cached(StyleRefinement::default().size_full()),
            )
            .child(
                canvas(
                    |_, _, _| {},
                    move |_, _, window, cx| spin(started, window, cx),
                )
                .absolute()
                .size_full(),
            )
    }
}

fn active_places(window: &Window, cx: &App) -> Vec<Rc<Place>> {
    let active: HashSet<_> = window.prepainted_element_state_ids::<Rc<Place>>().collect();
    cx.try_global::<Places>()
        .and_then(|places| places.0.get(&window.window_handle().window_id()))
        .into_iter()
        .flatten()
        .filter_map(Weak::upgrade)
        .filter(|place| active.contains(&place.element_id))
        .collect()
}

fn spin(started: Instant, window: &mut Window, cx: &mut App) {
    if let Some(places) = cx
        .default_global::<Places>()
        .0
        .get_mut(&window.window_handle().window_id())
    {
        places.retain(|place| place.strong_count() > 0);
    }
    let places = active_places(window, cx);
    if places.is_empty() {
        return;
    }
    let elapsed = if cx.reduce_motion() {
        Duration::ZERO
    } else {
        started.elapsed()
    };
    let turn = (elapsed.as_secs_f32() / TURN.as_secs_f32()).fract();
    let angle = radians(ease_in_out(turn) * std::f32::consts::TAU);
    let scale = window.scale_factor();
    for place in places {
        window.with_content_mask(Some(place.mask), |window| match place.effect {
            Effect::Spinner => {
                let center = place.bounds.center();
                let transformation = TransformationMatrix::unit()
                    .translate(center.scale(scale))
                    .rotate(angle)
                    .translate(center.scale(-scale));
                if let Err(error) = window.paint_svg(
                    place.bounds, IconName::Loader.path(), None, transformation, place.color, cx,
                ) {
                    tracing::error!(target: "agent", operation = "activity.paint", message = %error);
                }
            }
            Effect::Pulse => {
                window.paint_quad(fill(place.bounds, place.color.opacity(pulse_opacity(elapsed)))
                    .corner_radii(window.rem_size() * 0.5));
            }
        });
    }
    // Only this view is notified, so the cached content is reused next frame.
    if !cx.reduce_motion() {
        window.request_animation_frame();
    }
}

fn pulse_opacity(elapsed: Duration) -> f32 {
    let phase = (elapsed.as_secs_f64() / PULSE.as_secs_f64()).fract() as f32;
    1.0 - bounce(ease_in_out)(phase) * 0.5
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_kit::{self as gpui, prelude::FluentBuilder};
    use std::{cell::Cell, rc::Rc};

    #[test]
    fn image_pulse_preserves_its_two_second_half_opacity_cycle() {
        for (milliseconds, opacity) in [(0, 1.), (500, 0.75), (1000, 0.5), (1500, 0.75), (2000, 1.)]
        {
            assert_eq!(pulse_opacity(Duration::from_millis(milliseconds)), opacity);
        }
    }

    #[gpui::test]
    fn reduced_motion_keeps_activity_static_without_requesting_frames(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let window = cx.open_window(size(px(100.), px(100.)), |window, cx| {
            let content = cx.new(|_| Content {
                active: true,
                renders: Rc::new(Cell::new(0)),
            });
            ActivityWindow::new(content, window, cx)
        });
        cx.run_until_parked();
        assert_eq!(next_frame(&window, cx), 0);
    }

    struct Content {
        active: bool,
        renders: Rc<Cell<usize>>,
    }

    impl Render for Content {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.renders.set(self.renders.get() + 1);
            div()
                .size_full()
                .when(self.active, |content| content.child(indicator()))
        }
    }

    fn next_frame(window: &WindowHandle<ActivityWindow>, cx: &mut TestAppContext) -> usize {
        let callbacks = window
            .update(cx, |_, window, cx| window.simulate_next_frame(cx))
            .unwrap();
        cx.run_until_parked();
        callbacks
    }

    #[gpui::test]
    fn cached_children_remain_drawable_and_interactive_after_parent_replay(
        cx: &mut TestAppContext,
    ) {
        struct Child {
            renders: Rc<Cell<usize>>,
            clicks: Rc<Cell<usize>>,
        }
        impl Render for Child {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                self.renders.set(self.renders.get() + 1);
                let clicks = self.clicks.clone();
                div().size_full().child(
                    div()
                        .id("cached-action")
                        .debug_selector(|| "cached-action".into())
                        .w(px(200.))
                        .h(px(60.))
                        .child("共有ブラウザのキャッシュされた操作ボタン")
                        .on_mouse_down(MouseButton::Left, move |_, _, _| {
                            clicks.set(clicks.get() + 1);
                        }),
                )
            }
        }
        struct Parent(Entity<Child>);
        impl Render for Parent {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                self.0
                    .clone()
                    .cached(StyleRefinement::default().size_full())
            }
        }
        struct Scene {
            prefix: usize,
            parent: Entity<Parent>,
        }
        impl Render for Scene {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                div()
                    .relative()
                    .size_full()
                    .child(
                        div()
                            .absolute()
                            .w(px(200.))
                            .children((0..self.prefix).map(|i| {
                                div()
                                    .id(format!("prefix-{i}"))
                                    .child(format!("prefix {i}"))
                                    .on_mouse_down(MouseButton::Left, |_, _, _| {})
                            })),
                    )
                    .child(
                        self.parent
                            .clone()
                            .cached(StyleRefinement::default().size_full()),
                    )
            }
        }
        let renders = Rc::new(Cell::new(0));
        let clicks = Rc::new(Cell::new(0));
        let (scene, window) = cx.add_window_view(|_, cx| {
            let child = cx.new(|_| Child {
                renders: renders.clone(),
                clicks: clicks.clone(),
            });
            Scene {
                prefix: 12,
                parent: cx.new(|_| Parent(child)),
            }
        });
        window.run_until_parked();
        let parent = scene.read_with(window, |scene, _| scene.parent.clone());
        for prefix in [0, 16, 2] {
            let before = renders.get();
            scene.update(window, |scene, cx| {
                scene.prefix = prefix;
                cx.notify();
            });
            window.run_until_parked();
            // The parent was replayed at a different position in the frame's
            // resources. Its unchanged child must still be reusable next frame.
            parent.update(window, |_, cx| cx.notify());
            window.run_until_parked();
            assert_eq!(
                renders.get(),
                before,
                "replaying a parent must preserve child caching"
            );
            let bounds = window.debug_bounds("cached-action").unwrap();
            window.simulate_click(bounds.center(), Modifiers::default());
            window.run_until_parked();
        }
        assert_eq!(
            clicks.get(),
            3,
            "replayed input must reach the cached child"
        );
    }

    #[gpui::test]
    fn indicators_spin_without_rendering_the_content_again(cx: &mut TestAppContext) {
        let renders = Rc::new(Cell::new(0));
        let mut content = None;
        let window = cx.open_window(size(px(100.), px(100.)), |window, cx| {
            let view = cx.new(|_| Content {
                active: true,
                renders: renders.clone(),
            });
            content = Some(view.clone());
            ActivityWindow::new(view, window, cx)
        });
        let content = content.unwrap();
        cx.run_until_parked();
        assert_eq!(renders.get(), 1);
        for _ in 0..3 {
            assert!(
                next_frame(&window, cx) > 0,
                "a visible indicator keeps spinning"
            );
        }
        assert_eq!(renders.get(), 1, "spinning must not rebuild the content");

        content.update(cx, |content, cx| {
            content.active = false;
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(renders.get(), 2);
        next_frame(&window, cx);
        assert_eq!(
            next_frame(&window, cx),
            0,
            "a removed indicator stops requesting frames"
        );
    }

    #[gpui::test]
    fn cached_indicators_survive_parent_updates_and_disappear_with_their_view(
        cx: &mut TestAppContext,
    ) {
        struct Parent {
            child: Entity<Content>,
            visible: bool,
        }
        impl Render for Parent {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                div().size_full().when(self.visible, |body| {
                    body.child(
                        self.child
                            .clone()
                            .cached(StyleRefinement::default().size_full()),
                    )
                })
            }
        }
        let renders = Rc::new(Cell::new(0));
        let mut parent = None;
        let window = cx.open_window(size(px(100.), px(100.)), |window, cx| {
            let child = cx.new(|_| Content {
                active: true,
                renders: renders.clone(),
            });
            let view = cx.new(|_| Parent {
                child,
                visible: true,
            });
            parent = Some(view.clone());
            ActivityWindow::new(view, window, cx)
        });
        let parent = parent.unwrap();
        cx.run_until_parked();
        for _ in 0..3 {
            parent.update(cx, |_, cx| cx.notify());
            cx.run_until_parked();
            let places = cx.read(|cx| {
                cx.global::<Places>().0[&window.window_id()]
                    .iter()
                    .filter_map(Weak::upgrade)
                    .collect::<Vec<_>>()
            });
            assert_eq!(
                places.len(),
                1,
                "the cached indicator must remain registered exactly once"
            );
            assert!(places[0].bounds.size.width > px(0.));
            assert!(next_frame(&window, cx) > 0);
        }
        assert_eq!(renders.get(), 1, "parent updates reuse the child");
        parent.update(cx, |parent, cx| {
            parent.visible = false;
            cx.notify();
        });
        cx.run_until_parked();
        next_frame(&window, cx);
        assert_eq!(
            next_frame(&window, cx),
            0,
            "hidden cached views must stop spinning"
        );
        parent.update(cx, |parent, cx| {
            parent.visible = true;
            cx.notify();
        });
        cx.run_until_parked();
        assert!(
            next_frame(&window, cx) > 0,
            "showing the view restores its indicator"
        );
    }

    #[gpui::test]
    fn moving_and_hiding_an_indicator_never_paints_its_previous_place(cx: &mut TestAppContext) {
        struct Moving {
            right: bool,
            visible: bool,
        }
        impl Render for Moving {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .when(self.right, |body| body.justify_end())
                    .when(self.visible, |body| body.child(indicator()))
            }
        }
        struct Probe {
            activity: Entity<ActivityWindow>,
            painted: Rc<std::cell::RefCell<Vec<Bounds<Pixels>>>>,
        }
        impl Render for Probe {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                let painted = self.painted.clone();
                div()
                    .relative()
                    .size_full()
                    .child(self.activity.clone())
                    .child(
                        canvas(
                            |_, _, _| {},
                            move |_, _, window, cx| {
                                *painted.borrow_mut() = active_places(window, cx)
                                    .iter()
                                    .map(|place| place.bounds)
                                    .collect();
                            },
                        )
                        .absolute()
                        .size_full(),
                    )
            }
        }
        let painted = Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut moving = None;
        let _window = cx.open_window(size(px(100.), px(100.)), |window, cx| {
            let content = cx.new(|_| Moving {
                right: false,
                visible: true,
            });
            moving = Some(content.clone());
            Probe {
                activity: cx.new(|cx| ActivityWindow::new(content, window, cx)),
                painted: painted.clone(),
            }
        });
        let moving = moving.unwrap();
        cx.run_until_parked();
        assert_eq!(painted.borrow().len(), 1);
        let original = painted.borrow()[0];
        moving.update(cx, |moving, cx| {
            moving.right = true;
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(
            painted.borrow().len(),
            1,
            "moving must paint only the new place in the same frame"
        );
        assert!(painted.borrow()[0].left() > original.left());
        moving.update(cx, |moving, cx| {
            moving.visible = false;
            cx.notify();
        });
        cx.run_until_parked();
        assert!(
            painted.borrow().is_empty(),
            "hiding must not paint the previous frame's place"
        );
    }
}
