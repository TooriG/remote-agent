//! Native UI over the same ClientSession and presentation as the existing apps.
mod accounts;
mod activity;
mod client;
mod composer;
mod connections;
mod conversation;
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

#[cfg(target_os = "ios")]
pub use native::ios::register as register_ios;
#[cfg(target_os = "macos")]
pub use native::macos::register as register_macos;

use agent_core::{presentation::view::Presentation, state::Intent};
use nsh_core::{HeaderSystemItem, platform::Lifecycle, prelude::*};
use serde_json::json;
use shared::Shared;
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Copy)]
pub(crate) struct Connection {
    client: Stored<Arc<client::Client>>,
    host: Signal<String>,
    publication: Memo<Shared<Presentation>>,
    pairing: Signal<bool>,
    uploads: Signal<BTreeMap<uuid::Uuid, (String, agent_core::state::DraftKey)>>,
    uploading: Memo<bool>,
}

impl Connection {
    fn attach(
        self,
        host: String,
        draft: agent_core::state::DraftKey,
        sources: Vec<client::AttachmentSource>,
    ) {
        if sources.is_empty() {
            return;
        }
        let id = uuid::Uuid::new_v4();
        self.uploads.update(|uploads| {
            uploads.insert(id, (host.clone(), draft.clone()));
        });
        self.client
            .with(|client| client.attach_batch(id, host, draft, sources));
    }

    fn dispatch(self, intent: Intent) {
        self.client
            .with(|client| client.dispatch(&self.host.get_untracked(), intent));
    }
}

#[component]
pub fn App() -> impl IntoElement {
    let service = storage::directory().and_then(client::Client::new);
    let (runtime, service, updates) = match service {
        Ok(service) => service,
        Err(reason) => return view().p_4().child(text(reason.to_string())).into_element(),
    };
    let client = stored(service.clone());
    let hosts = signal_with(Vec::<storage::Host>::new(), SignalEquals::Never);
    let host = signal(String::new());
    let publications = signal_with(
        BTreeMap::<String, Shared<Presentation>>::new(),
        SignalEquals::Never,
    );
    let empty = stored(Shared(Presentation::empty()));
    let publication = computed(move || {
        publications
            .with(|all| all.get(&host.get()).cloned())
            .unwrap_or_else(|| empty.get())
    });
    let uploads = signal(BTreeMap::<uuid::Uuid, (String, agent_core::state::DraftKey)>::new());
    let uploading = computed(move || {
        let host = host.get();
        let draft = publication.with(|p| p.composer.controls.navigation.draft_key.clone());
        uploads.with(|uploads| {
            uploads
                .values()
                .any(|(target_host, target_draft)| target_host == &host && target_draft == &draft)
        })
    });
    let connection = Connection {
        client,
        host,
        publication,
        pairing: signal(false),
        uploads,
        uploading,
    };
    provide_context(connection);
    let preview = media::Preview::new();
    provide_context(preview);
    let workspace = workbench::Workbench::new();
    provide_context(workspace);
    let files = files::Browser::new(preview);
    provide_context(files);
    let failure = signal(None::<String>);
    let routes = signal(vec![0_u8]);
    let settings = signal(false);
    let provider = signal(None::<agent_protocol::session::ProviderKind>);
    let account_pending = signal(false);
    let account_locked = computed(move || {
        account_pending.get()
            || publication.with(|p| {
                p.settings.account_login().is_some()
                    || provider.get().is_some_and(|provider| {
                        p.snapshot.operation_running(
                            agent_core::state::operations::OperationKey::AccountLogin { provider },
                        )
                    })
            })
    });
    let close_settings = move || {
        provider.set(None);
        settings.set(false);
    };
    let sidebar = signal(true);
    let searching = signal(false);
    let query = signal(String::new());
    effect(move || {
        client.with(|client| client.scene(lifecycle() == Lifecycle::Active));
    });
    cleanup(move || {
        let _ = runtime.block_on(service.shutdown());
    });
    spawn(async move {
        while let Ok(update) = updates.recv().await {
            match update {
                client::Update::AttachmentsFinished(id) => uploads.update(|uploads| {
                    uploads.remove(&id);
                }),
                client::Update::Hosts(next) => {
                    if host.get_untracked().is_empty()
                        || !next.iter().any(|item| item.id == host.get_untracked())
                    {
                        host.set(next.first().map(|item| item.id.clone()).unwrap_or_default());
                    }
                    hosts.set(next);
                }
                client::Update::Presentation { host, value } => {
                    let before = publications.with_untracked(|all| {
                        all.get(&host).map(|p| {
                            p.snapshot
                                .panel_scope(host.clone(), p.context.navigation().draft_key)
                        })
                    });
                    if let Some(before) = before
                        && let Some(session) =
                            value.snapshot.submitted_session(before.draft.clone())
                    {
                        workspace.rekey(
                            before,
                            value.snapshot.panel_scope(
                                host.clone(),
                                agent_core::state::DraftKey::Session { session },
                            ),
                        );
                    }
                    publications.update(|all| {
                        all.insert(host, Shared(value));
                    });
                }
                client::Update::Error(reason) => failure.set(Some(reason)),
                client::Update::Paired(result) => {
                    connection.pairing.set(false);
                    match result {
                        Ok(id) => {
                            host.set(id);
                            settings.set(false);
                        }
                        Err(reason) => failure.set(Some(reason)),
                    }
                }
            }
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
        if account_locked.get_untracked() {
            return;
        }
        match action {
            "sidebar" => sidebar.update(|open| *open = !*open),
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
            _ => settings.set(true),
        }
    };
    let environment = move || {
        view()
            .flex_1()
            .child(when(
                move || provider.get().is_none(),
                move || environment::overview(hosts, host, provider, close_settings),
            ))
            .child(each(
                move || provider.get().into_iter().collect::<Vec<_>>(),
                |provider: &agent_protocol::session::ProviderKind| *provider,
                move |selected, _| {
                    accounts::screen(selected.get_untracked(), account_pending, move || {
                        provider.set(None)
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
                            failure.set(None)
                        }))
                },
            ))
    };
    if is_desktop() {
        return view()
            .flex_1()
            .text_color(move || nsh_base::theme().foreground)
            .child(media::preview())
            .child(native::toolbar(title, toolbar_action).h(px(0.0)))
            .child(notices())
            .child(
                view()
                    .flex_1()
                    .flex_row()
                    .child(
                        view()
                            .w(px(280.0))
                            .p_2()
                            .gap_2()
                            .style_when(move || !sidebar.get(), |s| s.hidden())
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
                                        move || {
                                            toolbar_action(&tool.with_untracked(|t| t.key.clone()))
                                        },
                                    )
                                    .prop("symbol", move || {
                                        json!(tool.with(|t| workbench::symbol(t.kind)))
                                    })
                                    .prop(
                                        "enabled",
                                        move || {
                                            json!(!account_locked.get() && tool.with(|t| t.enabled))
                                        },
                                    )
                                },
                            ))
                            .child(native::text_button(json!("環境"), move || {
                                settings.set(true)
                            })),
                    )
                    .child(when(
                        move || settings.get() || hosts.with(Vec::is_empty),
                        environment,
                    ))
                    .child(when(
                        move || {
                            !settings.get()
                                && !hosts.with(Vec::is_empty)
                                && !workspace.state.with(|state| state.open)
                        },
                        content,
                    ))
                    .child(when(
                        move || !settings.get() && workspace.state.with(|state| state.open),
                        workbench::screen,
                    )),
            )
            .into_element();
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
                    .child(environment::overview(hosts, host, provider, close_settings)),
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
                        account_pending,
                        move || provider.set(None),
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
                        move || connections::screen(hosts, host, || {}),
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
                    provider.set(None);
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
