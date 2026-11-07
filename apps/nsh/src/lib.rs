//! Native UI over the same ClientSession and presentation as the existing apps.
mod accounts;
mod activity;
mod application;
mod browser;
mod client;
mod composer;
mod connections;
mod conversation;
#[cfg(target_os = "macos")]
mod desktop;
#[cfg(target_os = "macos")]
mod desktop_workspace;
mod details;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod dictation;
mod editor;
mod environment;
mod files;
mod invocations;
mod media;
mod model_controls;
mod native;
mod notices;
mod projects;
mod requests;
mod rich_text;
mod shared;
mod side_chat;
#[cfg(target_os = "macos")]
mod sidebar;
mod storage;
mod terminal;
mod threads;
mod ui;
mod workbench;
mod worktrees;

#[cfg(target_os = "macos")]
pub use desktop::run as run_macos;
#[cfg(target_os = "ios")]
pub use native::ios::register as register_ios;
#[cfg(target_os = "macos")]
pub use native::macos::register as register_macos;

use agent_core::{presentation::view::Presentation, state::Intent};
use application::Navigation;
use nsh_core::{HeaderSystemItem, platform::Lifecycle, prelude::*};
use serde_json::json;
use shared::Shared;
use std::sync::Arc;

#[derive(Clone, Copy)]
pub(crate) struct Connection {
    application: Stored<Arc<application::Application>>,
    state: ReadSignal<Shared<application::State>>,
    client: Stored<Arc<client::Client>>,
    host: Memo<String>,
    publication: Memo<Shared<Presentation>>,
    pairing: Memo<bool>,
    pane: Option<u64>,
    composer_ref: ElementRef,
}

impl Connection {
    fn uploading(self) -> bool {
        // Read the owner as well as tracking its publication, so a second input
        // event cannot submit while a newly staged batch is waiting to render.
        self.state.with(|_| ());
        let host = self.host.get();
        let draft = self
            .publication
            .with(|p| p.composer.controls.navigation.draft_key.clone());
        self.application
            .with(|application| application.snapshot().uploading(&host, &draft))
    }

    fn dispatch(self, intent: Intent) {
        self.client
            .with(|client| client.dispatch(&self.host.get_untracked(), self.pane, intent));
    }
}

#[component]
pub fn App() -> impl IntoElement {
    let service = storage::directory()
        .and_then(|root| application::Application::new(root, storage::local_host()?));
    let (runtime, service) = match service {
        Ok(service) => service,
        Err(reason) => return view().p_4().child(text(reason.to_string())).into_element(),
    };
    let stored_service = service.clone();
    cleanup(move || {
        let _ = runtime.block_on(service.shutdown());
    });
    app_view(stored_service, Area::Mobile)
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Area {
    Mobile,
    #[cfg(target_os = "macos")]
    Shell,
}

fn app_view(service: Arc<application::Application>, area: Area) -> AnyElement {
    let application = stored(service.clone());
    let client = stored(service.client.clone());
    let mut updates = service.subscribe();
    let state = signal(Shared(updates.borrow_and_update().clone()));
    let hosts = computed(move || state.with(|state| state.hosts.clone()));
    let host = computed(move || state.with(|state| state.host.clone()));
    let empty = stored(Shared(Presentation::empty()));
    let publication = computed(move || {
        state
            .with(|state| state.publications.get(&host.get()).cloned())
            .unwrap_or_else(|| empty.get())
    });
    let connection = Connection {
        application,
        state: state.read_only(),
        client,
        host,
        publication,
        pane: None,
        composer_ref: element_ref(),
        pairing: computed(move || state.with(|state| state.pairing)),
    };
    provide_context(connection);
    let preview = media::Preview::new();
    provide_context(preview);
    let workspace = workbench::Workbench::new();
    provide_context(workspace);
    let files = files::Browser::new(preview);
    provide_context(files);
    let failure = computed(move || state.with(|state| state.failure.clone()));
    let routes = signal(vec![0_u8]);
    let settings = computed(move || state.with(|state| state.settings));
    let provider = computed(move || state.with(|state| state.provider));
    let navigate = move |action| application.with(|application| application.navigate(action));
    let account_locked = computed(move || state.with(|state| state.account_locked()));
    let close_settings = move || navigate(Navigation::Settings(false));
    #[cfg(target_os = "macos")]
    let sidebar = computed(move || state.with(|state| state.sidebar));
    let searching = signal(false);
    let query = signal(String::new());
    let primary = area == Area::Mobile;
    #[cfg(target_os = "macos")]
    let primary = primary || area == Area::Shell;
    if primary {
        effect(move || client.with(|client| client.scene(lifecycle() == Lifecycle::Active)));
    }
    spawn(async move {
        while updates.changed().await.is_ok() {
            let next = updates.borrow_and_update().clone();
            state.set(Shared(next));
        }
    });
    let title = computed(move || {
        publication
            .with(|p| p.context.title())
            .unwrap_or_else(|| "新しいチャット".into())
    });
    let list = move || {
        threads::list(publication, query, move || {
            dismiss_keyboard();
            if !is_desktop() {
                routes.set(vec![0, 1]);
            }
        })
    };
    let toolbar_action = move |action: &str| {
        if application.with(|app| app.snapshot().account_locked()) {
            return;
        }
        match action {
            "sidebar" => navigate(Navigation::ToggleSidebar),
            "search" => searching.update(|open| *open = !*open),
            "worktrees" => worktrees::open(),
            "browser" | "files" | "terminal" | "diff" | "agents" | "sideChat" => {
                workbench::open_tool(action)
            }
            "compose" => {
                connection.dispatch(Intent::NewChat {
                    cwd: if is_desktop() {
                        publication.with(|p| p.snapshot.pane.navigation.cwd.clone())
                    } else {
                        String::new()
                    },
                });
                if !is_desktop() {
                    routes.set(vec![0, 1]);
                }
            }
            _ => navigate(Navigation::Settings(true)),
        }
    };
    #[cfg(target_os = "macos")]
    let environment = move || {
        view()
            .flex_1()
            .child(when(
                move || provider.get().is_none(),
                move || environment::overview(hosts, close_settings),
            ))
            .child(each(
                move || provider.get().into_iter().collect::<Vec<_>>(),
                |provider: &agent_protocol::session::ProviderKind| *provider,
                move |selected, _| {
                    accounts::screen(selected.get_untracked(), move || {
                        navigate(Navigation::Provider(None))
                    })
                },
            ))
    };
    let content = move || {
        let following = signal(true);
        let welcome =
            computed(move || is_desktop() && publication.with(|p| p.conversation.is_none()));
        view()
            .flex_1()
            .min_w_0()
            .pb_keyboard()
            .child(when(
                move || !welcome.get(),
                move || conversation::list(publication, following),
            ))
            .child(when(
                move || welcome.get(),
                move || {
                    view()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .gap_4()
                        .child(text("何から始めましょうか？").text_size(px(24.0)))
                        .child(
                            text(move || {
                                let directory =
                                    publication.with(|p| p.context.selected_directory());
                                if directory.is_empty() {
                                    "メッセージを入力して、作業を始めましょう。".to_owned()
                                } else {
                                    agent_core::presentation::markdown::file_basename(&directory)
                                        .to_owned()
                                }
                            })
                            .text_size(px(14.0)),
                        )
                },
            ))
            .child(composer::composer(publication, following))
    };
    let notices = move || {
        view()
            .child(notices::notices(
                move || workspace.state.with(|state| state.open),
                move || settings.get(),
            ))
            .child(when(
                move || failure.get().is_some(),
                move || {
                    view()
                        .flex_row()
                        .gap_2()
                        .p_2()
                        .child(text(move || failure.get().unwrap_or_default()))
                        .child(native::text_button(json!("閉じる"), move || {
                            application.with(|application| application.dismiss_failure())
                        }))
                },
            ))
    };
    match area {
        #[cfg(target_os = "macos")]
        Area::Shell => {
            use nsh_core::{desktop::TitleBarStyle, element::desktop::set_title_bar};
            set_title_bar(TitleBarStyle::Hidden, Some(px(sidebar::HEADER_HEIGHT)));
            let tools = move || {
                !settings.get() && !hosts.with(Vec::is_empty) && workspace.state.with(|s| s.open)
            };
            let layout = move || workspace.state.with(|s| s.layout);
            let main = move || {
                view()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .flex_row()
                    .style_when(
                        move || layout() == agent_core::panels::PanelLayout::Bottom,
                        |s| s.flex_col(),
                    )
                    .child(when(
                        move || !tools() || layout() != agent_core::panels::PanelLayout::Expanded,
                        move || {
                            view()
                                .flex_1()
                                .min_w_0()
                                .child(sidebar::conversation_header(
                                    title,
                                    sidebar,
                                    tools,
                                    move || navigate(Navigation::ToggleSidebar),
                                    move || {
                                        application.with(|application| {
                                            application.set_panel_open(
                                                workspace.scope.get_untracked(),
                                                true,
                                            )
                                        })
                                    },
                                ))
                                .child(view().flex_1().min_h_0().flex_row().child(content()).child(
                                    when(
                                        move || window().width.0 >= 1080.0 && !tools(),
                                        desktop_workspace::card,
                                    ),
                                ))
                        },
                    ))
                    .child(when(tools, move || {
                        view()
                            .style_when(
                                move || layout() == agent_core::panels::PanelLayout::Right,
                                |s| {
                                    s.w(px(540.0))
                                        .border_l_1()
                                        .border_l_color(ui::colors::BORDER)
                                },
                            )
                            .style_when(
                                move || layout() == agent_core::panels::PanelLayout::Bottom,
                                |s| {
                                    s.h(px(360.0))
                                        .border_t_1()
                                        .border_t_color(ui::colors::BORDER)
                                },
                            )
                            .style_when(
                                move || layout() == agent_core::panels::PanelLayout::Expanded,
                                |s| s.flex_1(),
                            )
                            .min_w_0()
                            .child(workbench::screen())
                    }))
            };
            return view()
                .flex_1()
                .flex_row()
                .bg(ui::colors::BACKGROUND)
                .text_color(ui::colors::FOREGROUND)
                .child(media::preview())
                .child(worktrees::presenter(|| {}))
                .child(native("bex.browser.host").w(px(0.0)).h(px(0.0)))
                .child(when(
                    move || sidebar.get(),
                    move || sidebar::sidebar(publication, hosts, move || toolbar_action("compose")),
                ))
                .child(
                    view()
                        .flex_1()
                        .min_w_0()
                        .child(notices())
                        .child(when(
                            move || settings.get() || hosts.with(Vec::is_empty),
                            environment,
                        ))
                        .child(when(
                            move || !settings.get() && !hosts.with(Vec::is_empty),
                            main,
                        )),
                )
                .into_element();
        }
        Area::Mobile => {}
    }
    let navigation = nsh_base::nav_stack().flex_1().child(each(
        move || {
            let mut stack = routes.get();
            if workspace.state.with(|state| state.open) {
                stack.push(3);
            }
            if settings.get() {
                stack.push(2);
                if provider.get().is_some() {
                    stack.push(5);
                }
            }
            stack
        },
        |route: &u8| *route,
        move |route, position| {
            let page = match route.get_untracked() {
                1 => nsh_base::nav_page()
                    .title(move || title.get())
                    .child(notices())
                    .child(content())
                    .header_right(
                        native::menu(
                            "ellipsis",
                            "その他",
                            move || {
                                let mut entries = vec![json!({"id": "refresh", "title": "更新", "symbol": "arrow.clockwise"})];
                                entries.extend(workbench::tools().into_iter().map(|tool| json!({"id": tool.key, "title": tool.label, "symbol": workbench::symbol(tool.kind), "enabled": tool.enabled})));
                                entries.push(json!({"id": "settings", "title": "環境", "symbol": "gearshape"}));
                                entries.push(json!({"id": "worktrees", "title": "ワークツリー設定", "symbol": "arrow.triangle.branch", "enabled": publication.with(|p| p.settings.connected())}));
                                json!(entries)
                            },
                            move |item| {
                                if item == "refresh" {
                                    connection.application.with(|application| { application.request_list(application::ListRequest::Refresh); });
                                } else {
                                    toolbar_action(item);
                                }
                            },
                        )
                        .w(px(44.0))
                        .h(px(44.0)),
                    ),
                2 => nsh_base::nav_page()
                    .title("環境")
                    .child(notices())
                    .child(environment::overview(hosts, close_settings)),
                5 => nsh_base::nav_page()
                    .title(move || {
                        publication.with(|p| {
                            p.settings
                                .connection_setup()
                                .agents
                                .into_iter()
                                .find(|a| Some(a.provider) == provider.get())
                                .map(|a| a.name)
                                .unwrap_or_else(|| "アカウント".into())
                        })
                    })
                    .header_back_visible(move || !account_locked.get())
                    .pop_on_back(move || !account_locked.get())
                    .pop_on_swipe(move || !account_locked.get())
                    .child(notices())
                    .child(accounts::screen(
                        provider.get_untracked().unwrap(),
                        move || navigate(Navigation::Provider(None)),
                    )),
                3 => nsh_base::nav_page()
                    .title(workbench::title)
                    .child(notices())
                    .child(workbench::screen()),
                _ => nsh_base::nav_page()
                    .title("Bex")
                    .header_back_title("戻る")
                    .header_right_system_item(HeaderSystemItem::Compose)
                    .header_right(view().on_press(move || toolbar_action("compose")))
                    .child(notices())
                    .child(when(
                        move || hosts.with(Vec::is_empty),
                        move || connections::screen(hosts, || {}),
                    ))
                    .child(when(
                        move || !hosts.with(Vec::is_empty),
                        move || {
                            view()
                                .flex_1()
                                .child(connections::selector(hosts, host, true))
                                .child(when(
                                    move || searching.get(),
                                    move || native::search(query).w_full(),
                                ))
                                .child(list())
                                .child(native::toolbar(title, toolbar_action).w_full())
                        },
                    )),
            };
            page.on_dismiss(move || {
                if route.get_untracked() == 2 {
                    close_settings();
                }
                if route.get_untracked() == 5 {
                    navigate(Navigation::Provider(None));
                }
                if route.get_untracked() == 3 {
                    files.close();
                }
                routes.update(|routes| routes.truncate(position.get_untracked()));
            })
        },
    ));
    view()
        .flex_1()
        .child(media::preview())
        .child(worktrees::presenter(move || routes.set(vec![0, 1])))
        .child(navigation)
        .into_element()
}
