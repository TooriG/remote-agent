use super::*;

#[derive(PartialEq)]
struct HeaderModel {
    title: String,
    sidebar: bool,
    panel_open: bool,
    details: bool,
    details_popover: bool,
}

pub(in crate::app) struct HeaderView {
    desktop: WeakEntity<Desktop>,
    model: Option<HeaderModel>,
    _observation: Subscription,
    #[cfg(test)]
    pub(super) renders: usize,
}
impl HeaderView {
    pub(in crate::app) fn new(desktop: &Entity<Desktop>, cx: &mut Context<Self>) -> Self {
        Self {
            desktop: desktop.downgrade(),
            model: None,
            _observation: cx.observe(desktop, |view, desktop, cx| {
                view.set_model(
                    desktop
                        .read(cx)
                        .header_model(crate::localization::language(cx)),
                    cx,
                );
            }),
            #[cfg(test)]
            renders: 0,
        }
    }
    fn set_model(&mut self, model: HeaderModel, cx: &mut Context<Self>) {
        if self.model.as_ref() != Some(&model) {
            self.model = Some(model);
            cx.notify();
        }
    }
}
impl Render for HeaderView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        {
            self.renders += 1;
        }
        let width = window.viewport_size().width;
        let render_owner = cx.entity_id();
        self.desktop
            .update(cx, |_, cx| {
                self.model
                    .as_ref()
                    .map(|model| {
                        Desktop::render_header(model, width, render_owner, cx).into_any_element()
                    })
                    .unwrap_or_else(|| div().into_any_element())
            })
            .unwrap_or_else(|_| div().into_any_element())
    }
}
impl Desktop {
    fn header_model(&self, language: agent_core::localization::UiLanguage) -> HeaderModel {
        let panel_open = self.panels.get(self.panel_scope()).open;
        let title = self
            .thread()
            .and_then(|thread| thread.name.as_deref())
            .or_else(|| {
                self.snapshot
                    .threads
                    .as_ref()
                    .and_then(|page| {
                        page.data
                            .iter()
                            .find(|thread| thread.id.as_ref() == self.selected())
                    })
                    .and_then(|thread| thread.name.as_deref())
            })
            .unwrap_or(language.text("新しいチャット"))
            .to_owned();
        HeaderModel {
            title,
            sidebar: self.sidebar,
            panel_open,
            details: self.details,
            details_popover: self.details_popover,
        }
    }
    pub(super) fn conversation_header(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.header_view.read(cx).model.is_none() {
            let model = self.header_model(crate::localization::language(cx));
            self.header_view
                .update(cx, |view, cx| view.set_model(model, cx));
        }
        self.header_view
            .clone()
            .cached(
                StyleRefinement::default()
                    .w_full()
                    .h(px(crate::WINDOW_HEADER_HEIGHT))
                    .flex_shrink_0(),
            )
            .into_any_element()
    }
    fn render_header(
        model: &HeaderModel,
        width: Pixels,
        render_owner: EntityId,
        cx: &Context<Self>,
    ) -> Div {
        let mut header = h_flex()
            .debug_selector(|| "conversation-header".into())
            .w_full()
            .h(px(crate::WINDOW_HEADER_HEIGHT))
            .flex_shrink_0()
            .px_4()
            .gap_2();
        if !model.sidebar {
            header = header.pl_0().child(sidebar_header(false, cx));
        }
        header
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .text_ellipsis()
                    .child(model.title.clone()),
            )
            .child({
                let inline = details_inline(model.panel_open, width);
                let open = if inline {
                    model.details
                } else {
                    model.details_popover
                };
                Self::details_toggle(inline, open, render_owner, cx)
            })
            .when(!model.panel_open, |header| {
                header.child(Self::panel_toggle(false, cx))
            })
    }
}
