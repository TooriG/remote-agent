//! Activity indicators spin in a layer above the cached window content.
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
    collections::HashMap,
    time::{Duration, Instant},
};

/// The rotation of the `Spinner` that indicators replace.
const TURN: Duration = Duration::from_millis(800);

/// Places reserved by each window's latest content render, in paint order.
#[derive(Default)]
struct Places(HashMap<WindowId, Vec<Place>>);
impl Global for Places {}

struct Place {
    bounds: Bounds<Pixels>,
    mask: ContentMask<Pixels>,
    color: Hsla,
}

/// Reserves the place of a spinning activity indicator.
pub(crate) fn indicator() -> impl IntoElement {
    canvas(
        |bounds, window, cx| {
            let place = Place {
                bounds,
                mask: window.content_mask(),
                color: window.text_style().color,
            };
            if let Some(places) = cx
                .default_global::<Places>()
                .0
                .get_mut(&window.window_handle().window_id())
            {
                places.push(place);
            }
        },
        |_, _, _, _| {},
    )
    // The size of a small `Spinner`.
    .flex_none()
    .size_3p5()
}

/// Window root that keeps its content cached while the indicators spin.
pub(crate) struct ActivityWindow {
    content: Entity<Content>,
    started: Instant,
}

/// Caching boundary whose render replaces the places reserved by the last one.
struct Content(AnyView);

impl Render for Content {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        cx.default_global::<Places>()
            .0
            .insert(window.window_handle().window_id(), Vec::new());
        self.0.clone()
    }
}

impl ActivityWindow {
    pub(crate) fn new(
        content: impl Into<AnyView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let id = window.window_handle().window_id();
        cx.on_release(move |_, cx| {
            cx.default_global::<Places>().0.remove(&id);
        })
        .detach();
        let content = content.into();
        Self {
            content: cx.new(|_| Content(content)),
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

fn spin(started: Instant, window: &mut Window, cx: &App) {
    let Some(places) = cx
        .try_global::<Places>()
        .and_then(|places| places.0.get(&window.window_handle().window_id()))
        .filter(|places| !places.is_empty())
    else {
        return;
    };
    let turn = (started.elapsed().as_secs_f32() / TURN.as_secs_f32()).fract();
    let angle = radians(ease_in_out(turn) * std::f32::consts::TAU);
    let scale = window.scale_factor();
    for place in places {
        let center = place.bounds.center();
        let transformation = TransformationMatrix::unit()
            .translate(center.scale(scale))
            .rotate(angle)
            .translate(center.scale(-scale));
        window.with_content_mask(Some(place.mask.clone()), |window| {
            if let Err(error) = window.paint_svg(
                place.bounds,
                IconName::Loader.path(),
                None,
                transformation,
                place.color,
                cx,
            ) {
                tracing::error!(target: "agent", operation = "activity.paint", message = %error);
            }
        });
    }
    // Only this view is notified, so the cached content is reused next frame.
    window.request_animation_frame();
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_kit::{self as gpui, prelude::FluentBuilder};
    use std::{cell::Cell, rc::Rc};

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
}
