//! Windows shares its top row with the app's existing headers. Only the
//! rightmost header owns caption buttons; embedded side chats never own them.
use gpui_kit::{
    component::{Icon, IconName, Sizable, h_flex, v_flex},
    *,
};

pub(crate) fn drag_region(region: Div) -> Div {
    region.window_control_area(WindowControlArea::Drag)
}

pub(crate) fn header(content: Div, window: &Window) -> Div {
    drag_region(h_flex())
        .debug_selector(|| "windows-titlebar".into())
        .w_full()
        .h(px(crate::WINDOW_HEADER_HEIGHT))
        .flex_shrink_0()
        // Block the parent drag region so toolbar buttons receive client input.
        // Text and other draggable children opt back in with `drag_region`.
        .child(content.flex_1().min_w_0().occlude())
        .child(div().w(px(32.)).h_full().flex_shrink_0())
        .child(controls(window))
}

pub(crate) fn screen(content: impl IntoElement, title: &'static str, window: &Window) -> Div {
    v_flex()
        .size_full()
        .min_w_0()
        .child(header(
            drag_region(h_flex().h_full().px_3().child(title)),
            window,
        ))
        .child(div().flex_1().min_h_0().flex().child(content))
}

fn controls(window: &Window) -> Div {
    let maximize = if window.is_maximized() {
        IconName::WindowRestore
    } else {
        IconName::WindowMaximize
    };
    h_flex()
        .debug_selector(|| "windows-caption-controls".into())
        .h_full()
        .flex_shrink_0()
        .child(control(
            "window-minimize",
            "Minimize",
            IconName::WindowMinimize,
            WindowControlArea::Min,
        ))
        .child(control(
            "window-maximize",
            "Maximize or restore",
            maximize,
            WindowControlArea::Max,
        ))
        .child(control(
            "window-close",
            "Close",
            IconName::WindowClose,
            WindowControlArea::Close,
        ))
}

fn control(
    id: &'static str,
    label: &'static str,
    icon: IconName,
    area: WindowControlArea,
) -> Stateful<Div> {
    let close = area == WindowControlArea::Close;
    div()
        .id(id)
        .debug_selector(move || id.into())
        .role(accesskit::Role::Button)
        .aria_label(label)
        .on_a11y_action(accesskit::Action::Click, move |_, window, _| match area {
            WindowControlArea::Min => window.minimize_window(),
            WindowControlArea::Max => window.zoom_window(),
            WindowControlArea::Close => window.remove_window(),
            WindowControlArea::Drag => {}
        })
        .w(px(46.))
        .h_full()
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .occlude()
        // GPUI maps these to native HTMINBUTTON / HTMAXBUTTON / HTCLOSE.
        // Let Windows handle the actions, including the maximize hover menu.
        .window_control_area(area)
        .hover(move |style| {
            style.bg(rgb(if close {
                0xc42b1c
            } else {
                crate::appearance::HOVER
            }))
        })
        .active(move |style| {
            style.bg(rgb(if close {
                0xa02016
            } else {
                crate::appearance::SELECTED
            }))
        })
        .child(Icon::new(icon).small())
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_kit as gpui;
    use gpui_kit::{component::button::Button, prelude::FluentBuilder};
    use std::{cell::Cell, rc::Rc};

    struct HeaderFixture {
        header: Entity<HeaderContents>,
        overlay: bool,
    }

    struct HeaderContents(Rc<Cell<usize>>);

    impl Render for HeaderContents {
        fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let clicks = self.0.clone();
            header(
                h_flex()
                    .h_full()
                    .child(
                        drag_region(div().h_full().flex_1().min_w_0())
                            .debug_selector(|| "header-title".into())
                            .child("Chat"),
                    )
                    .child(
                        Button::new("sidebar-toggle")
                            .debug_selector(|| "sidebar-toggle".into())
                            .label("Toggle")
                            .on_click(move |_, _, _| clicks.set(clicks.get() + 1)),
                    ),
                window,
            )
        }
    }

    impl Render for HeaderFixture {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            v_flex()
                .relative()
                .size_full()
                .child(
                    self.header.clone().cached(
                        StyleRefinement::default()
                            .w_full()
                            .h(px(crate::WINDOW_HEADER_HEIGHT))
                            .flex_shrink_0(),
                    ),
                )
                .child(
                    div()
                        .debug_selector(|| "header-body".into())
                        .flex_1()
                        .w_full(),
                )
                .when(self.overlay, |root| {
                    root.child(deferred(
                        div()
                            .absolute()
                            .top_0()
                            .right_0()
                            .w(px(46.))
                            .h(px(crate::WINDOW_HEADER_HEIGHT))
                            .occlude(),
                    ))
                })
        }
    }

    fn area_at(
        window: &mut VisualTestContext,
        position: Point<Pixels>,
    ) -> Option<WindowControlArea> {
        window.simulate_mouse_move(position, None, Modifiers::default());
        window.run_until_parked();
        window.window_control_area()
    }

    #[gpui::test]
    fn caption_regions_preserve_toolbar_clicks_and_follow_resize_and_overlays(
        cx: &mut TestAppContext,
    ) {
        cx.update(crate::appearance::init);
        let clicks = Rc::new(Cell::new(0));
        let (view, window) = cx.add_window_view(|_, cx| HeaderFixture {
            header: cx.new(|_| HeaderContents(clicks.clone())),
            overlay: false,
        });
        for (index, width) in [1200., 480., 260.].into_iter().enumerate() {
            window.simulate_resize(size(px(width), px(300.)));
            window.run_until_parked();
            let close = window.debug_bounds("window-close").unwrap();
            assert_eq!(close.right(), px(width));
            assert_eq!(close.top(), px(0.));
            assert_eq!(close.size.height, px(crate::WINDOW_HEADER_HEIGHT));
            for (id, area) in [
                ("window-minimize", WindowControlArea::Min),
                ("window-maximize", WindowControlArea::Max),
                ("window-close", WindowControlArea::Close),
            ] {
                let bounds = window.debug_bounds(id).unwrap();
                assert_eq!(area_at(window, bounds.center()), Some(area));
            }
            let title = window.debug_bounds("header-title").unwrap();
            assert_eq!(
                area_at(window, title.center()),
                Some(WindowControlArea::Drag)
            );
            let toggle = window.debug_bounds("sidebar-toggle").unwrap();
            assert!(toggle.right() < window.debug_bounds("window-minimize").unwrap().left());
            assert_eq!(area_at(window, toggle.center()), None);
            window.simulate_click(toggle.center(), Modifiers::default());
            assert_eq!(clicks.get(), index + 1);
            let body = window.debug_bounds("header-body").unwrap();
            assert_eq!(body.top(), close.bottom());
            assert_eq!(area_at(window, body.center()), None);
        }
        for _ in 0..3 {
            view.update(window, |_, cx| cx.notify());
            window.run_until_parked();
            let title = window.debug_bounds("header-title").unwrap();
            assert_eq!(
                area_at(window, title.center()),
                Some(WindowControlArea::Drag)
            );
        }
        view.update(window, |view, cx| {
            view.overlay = true;
            cx.notify();
        });
        window.run_until_parked();
        let close = window.debug_bounds("window-close").unwrap();
        assert_eq!(area_at(window, close.center()), None);
    }
}
