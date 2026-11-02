use super::*;

#[derive(PartialEq)]
struct HeaderModel {
    title: String,
    sidebar: bool,
    panel_open: bool,
    editors: Vec<agent_protocol::editors::EditorId>,
    preferred_editor: Option<agent_protocol::editors::EditorId>,
    can_open_editor: bool,
    cwd: String,
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        {
            self.renders += 1;
        }
        self.desktop
            .update(cx, |_, cx| {
                self.model
                    .as_ref()
                    .map(|model| Desktop::render_header(model, cx).into_any_element())
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
            editors: self
                .snapshot
                .file_open_capabilities
                .as_ref()
                .map(|capabilities| capabilities.editors.clone())
                .unwrap_or_default(),
            preferred_editor: self.snapshot.preferred_editor,
            can_open_editor: self.local_shell && self.snapshot.connected,
            cwd: self.snapshot.navigation.cwd.clone(),
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
    fn render_header(model: &HeaderModel, cx: &Context<Self>) -> Div {
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
            .when(model.can_open_editor, |header| {
                header.child(Self::editor_picker(model, cx))
            })
            .when(!model.panel_open, |header| {
                header.child(Self::panel_toggle(false, cx))
            })
    }

    fn editor_picker(model: &HeaderModel, cx: &Context<Self>) -> AnyElement {
        use agent_core::presentation::editors::{editor_label, preferred_editor};
        use agent_protocol::editors::FileManagerRevealKind;
        let selected = preferred_editor(model.preferred_editor, &model.editors);
        let owner = cx.entity().downgrade();
        let options = model.editors.clone();
        let path = model.cwd.clone();
        let target = path.clone();
        h_flex()
            .gap_0()
            .child(
                Button::new("open-preferred-editor")
                    .label("Open")
                    .small()
                    .ghost()
                    .disabled(selected.is_none() || path.is_empty())
                    .on_click(cx.listener(move |view, _, _, cx| {
                        if let Some(editor) = selected {
                            view.dispatch(Intent::SetPreferredEditor { editor });
                            view.open_in_editor(target.clone(), None, None, editor, false, cx);
                        }
                    })),
            )
            .child(
                Button::new("preferred-editor-picker")
                    .label("⌄")
                    .small()
                    .ghost()
                    .disabled(options.is_empty() || path.is_empty())
                    .dropdown_menu(move |mut menu, _, _| {
                        for editor in &options {
                            let editor = *editor;
                            let owner = owner.clone();
                            let file_manager = if cfg!(target_os = "macos") {
                                FileManagerRevealKind::Finder
                            } else if cfg!(target_os = "windows") {
                                FileManagerRevealKind::FileExplorer
                            } else {
                                FileManagerRevealKind::Files
                            };
                            let label = editor_label(editor, file_manager);
                            menu = menu.item(
                                PopupMenuItem::new(label)
                                    .checked(selected == Some(editor))
                                    .on_click(move |_, _, cx| {
                                        let _ = owner.update(cx, |view, cx| {
                                            view.dispatch(Intent::SetPreferredEditor { editor });
                                            cx.notify();
                                        });
                                    }),
                            );
                        }
                        menu
                    }),
            )
            .into_any_element()
    }
}
