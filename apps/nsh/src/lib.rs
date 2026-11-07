//! Native UI over the same ClientSession and presentation as the existing apps.
mod accounts;
mod activity;
mod application;
mod client;
mod composer;
mod connections;
mod conversation;
#[cfg(target_os = "macos")]
mod desktop;
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
mod requests;
mod rich_text;
mod shared;
mod storage;
mod terminal;
mod threads;
mod workbench;

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
            .with(|client| client.dispatch(&self.host.get_untracked(), intent));
    }
}

#[component]
pub fn App() -> impl IntoElement {
    let service = storage::directory().and_then(application::Application::new);
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
    #[cfg(target_os = "macos")]
    Sidebar,
    #[cfg(target_os = "macos")]
    Conversation,
    #[cfg(target_os = "macos")]
    Workbench,
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
            "files" | "terminal" | "diff" | "agents" => workbench::open_tool(action),
            "compose" => {
                connection.dispatch(Intent::NewChat {
                    cwd: publication.with(|p| p.snapshot.pane.navigation.cwd.clone()),
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
        view()
            .flex_1()
            .min_w_0()
            .pb_keyboard()
            .child(conversation::list(publication, following))
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
            return view()
                .flex_1()
                .child(native::toolbar(title, toolbar_action).h(px(0.0)))
                .child(notices())
                .child(
                    native("bex.split")
                        .flex_1()
                        .prop("sidebar", move || json!(sidebar.get()))
                        .prop("tools", move || {
                            json!(
                                !settings.get()
                                    && !hosts.with(Vec::is_empty)
                                    && workspace.state.with(|s| s.open)
                            )
                        })
                        .prop("layout", move || {
                            json!(format!("{:?}", workspace.state.with(|s| s.layout)))
                        }),
                )
                .into_element();
        }
        #[cfg(target_os = "macos")]
        Area::Sidebar => {
            return view()
                .flex_1()
                .p_2()
                .gap_2()
                .child(
                    connections::selector(hosts, host)
                        .prop("enabled", move || json!(!account_locked.get())),
                )
                .child(native::search(query).w_full())
                .child(list())
                .child(each(
                    workbench::tools,
                    |tool: &agent_core::panels::PanelTool| tool.key.clone(),
                    move |tool, _| {
                        native::text_button(
                            move || json!(tool.with(|t| t.label.clone())),
                            move || toolbar_action(&tool.with_untracked(|t| t.key.clone())),
                        )
                        .prop("symbol", move || {
                            json!(tool.with(|t| workbench::symbol(t.kind)))
                        })
                        .prop("enabled", move || {
                            json!(!account_locked.get() && tool.with(|t| t.enabled))
                        })
                    },
                ))
                .child(native::text_button(json!("環境"), move || {
                    navigate(Navigation::Settings(true))
                }))
                .into_element();
        }
        #[cfg(target_os = "macos")]
        Area::Conversation => {
            return view()
                .flex_1()
                .min_w_0()
                .child(media::preview())
                .child(when(
                    move || settings.get() || hosts.with(Vec::is_empty),
                    environment,
                ))
                .child(when(
                    move || !settings.get() && !hosts.with(Vec::is_empty),
                    content,
                ))
                .into_element();
        }
        #[cfg(target_os = "macos")]
        Area::Workbench => {
            return view()
                .flex_1()
                .min_w_0()
                .child(media::preview())
                .child(when(
                    move || workspace.state.with(|s| s.open),
                    workbench::screen,
                ))
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
                                json!(entries)
                            },
                            move |item| {
                                if item == "refresh" {
                                    connection.dispatch(Intent::ShowThreadList);
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
                                .child(connections::selector(hosts, host))
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
        .child(navigation)
        .into_element()
}
