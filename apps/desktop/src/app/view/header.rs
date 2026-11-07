use super::*;

#[derive(PartialEq)]
pub(in crate::app) struct HeaderModel {
    title: String,
    sidebar: bool,
    panel_open: bool,
    panel_bottom: bool,
    details: bool,
    details_popover: bool,
    workspace: Entity<workspace::WorkspaceView>,
}

pub(in crate::app) struct HeaderView {
    desktop: WeakEntity<Desktop>,
    model: Option<HeaderModel>,
    #[cfg(test)]
    pub(super) renders: usize,
}
impl HeaderView {
    pub(in crate::app) fn new(desktop: &Entity<Desktop>, cx: &mut Context<Self>) -> Self {
        cx.observe_global::<crate::localization::Language>(|_, cx| cx.notify())
            .detach();

        Self {
            desktop: desktop.downgrade(),
            model: None,
            #[cfg(test)]
            renders: 0,
        }
    }
    pub(in crate::app) fn set_model(&mut self, model: HeaderModel, cx: &mut Context<Self>) {
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
        self.model
            .as_ref()
            .map(|model| {
                Desktop::render_header(model, width, render_owner, window, &self.desktop, cx)
                    .into_any_element()
            })
            .unwrap_or_else(|| div().into_any_element())
    }
}
impl Desktop {
    pub(in crate::app) fn header_model(
        &self,
        language: agent_core::localization::UiLanguage,
    ) -> HeaderModel {
        let panel = self.panels.get(self.panel_scope());
        let panel_open = panel.open;
        let title = self
            .publication
            .context
            .title()
            .unwrap_or_else(|| language.text("新しいチャット").into());
        HeaderModel {
            title,
            sidebar: self.sidebar,
            panel_open,
            panel_bottom: panel.layout == PanelLayout::Bottom,
            details: self.details,
            details_popover: self.details_popover,
            workspace: self.workspace_view.clone(),
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
        window: &Window,
        owner: &WeakEntity<Desktop>,
        cx: &App,
    ) -> Div {
        let mut header = h_flex()
            .debug_selector(|| "conversation-header".into())
            .w_full()
            .h(px(crate::WINDOW_HEADER_HEIGHT))
            .flex_shrink_0()
            .px_4()
            .gap_2();
        if !model.sidebar {
            header = header.pl_0().child(sidebar_header(false, owner, cx));
        }
        header
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .text_ellipsis()
                    .when(cfg!(target_os = "windows"), |title| {
                        crate::window_chrome::drag_region(title.h_full().flex().items_center())
                    })
                    .child(model.title.clone()),
            )
            .child({
                let inline = details_inline(model.panel_open, width);
                let open = if inline {
                    model.details
                } else {
                    model.details_popover
                };
                workspace::details_toggle(
                    inline,
                    open,
                    render_owner,
                    owner,
                    model.workspace.clone(),
                    cx,
                )
            })
            .when(!model.panel_open, |header| {
                header.child(Self::panel_toggle(false, owner, cx))
            })
            .when(
                cfg!(target_os = "windows") && (!model.panel_open || model.panel_bottom),
                |header| crate::window_chrome::header(header, window),
            )
    }
}
