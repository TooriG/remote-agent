//! Native UI over the same ClientSession and presentation as the existing apps.
mod activity;
mod client;
mod composer;
mod connections;
mod conversation;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod dictation;
mod editor;
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
mod threads;

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
    let files = files::Browser::new(preview);
    provide_context(files);
    let failure = signal(None::<String>);
    let routes = signal(vec![0_u8]);
    let settings = signal(false);
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
                client::Update::Presentation { host, value } => publications.update(|all| {
                    all.insert(host, Shared(value));
                }),
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
    let toolbar_action = move |action: &str| match action {
        "sidebar" => sidebar.update(|open| *open = !*open),
        "search" => searching.update(|open| *open = !*open),
        "files" => {
            let (cwd, home) = publication
                .with_untracked(|p| (p.composer.controls.navigation.cwd.clone(), p.context.home()));
            if cwd.is_empty() {
                native::message("ファイル", "作業フォルダーのある会話を開いてください。");
            } else {
                files.show(agent_core::presentation::markdown::file_host_path(
                    cwd, home,
                ));
            }
        }
        "compose" => {
            connection.dispatch(Intent::NewChat {
                cwd: publication.with(|p| p.snapshot.pane.navigation.cwd.clone()),
            });
            if !is_desktop() {
                routes.set(vec![0, 1]);
            }
        }
        _ => settings.set(true),
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
            .child(notices::notices(move || {
                files.root.get().is_some() || files.file.get().is_some()
            }))
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
                            .child(connections::selector(hosts, host))
                            .child(native::search(query).w_full())
                            .child(list())
                            .child(native::text_button(json!("ファイル"), move || {
                                toolbar_action("files")
                            }))
                            .child(native::text_button(json!("環境"), move || {
                                settings.set(true)
                            })),
                    )
                    .child(when(
                        move || settings.get() || hosts.with(Vec::is_empty),
                        move || connections::screen(hosts, host, move || settings.set(false)),
                    ))
                    .child(when(
                        move || {
                            !settings.get()
                                && !hosts.with(Vec::is_empty)
                                && files.root.get().is_none()
                                && files.file.get().is_none()
                        },
                        content,
                    ))
                    .child(when(
                        move || {
                            !settings.get()
                                && files.root.get().is_some()
                                && files.file.get().is_none()
                        },
                        files::screen,
                    ))
                    .child(when(
                        move || !settings.get() && files.file.get().is_some(),
                        editor::editors,
                    )),
            )
            .into_element();
    }
    let navigation = nsh_base::nav_stack().flex_1().child(each(
        move || {
            let mut stack = routes.get();
            if files.root.get().is_some() {
                stack.push(3);
            }
            if files.file.get().is_some() {
                stack.push(4);
            }
            if settings.get() {
                stack.push(2);
            }
            stack
        },
        |route: &u8| *route,
        move |route, position| {
            let page =
                match route.get_untracked() {
                    1 => nsh_base::nav_page()
                        .title(move || title.get())
                        .child(notices())
                        .child(content())
                        .header_right(
                            native::menu(
                                "ellipsis",
                                "その他",
                                native::choices(&["更新", "ファイル", "環境"]),
                                move |item| {
                                    if item == "更新" {
                                        connection.dispatch(Intent::ShowThreadList);
                                    } else if item == "ファイル" {
                                        toolbar_action("files");
                                    } else {
                                        settings.set(true);
                                    }
                                },
                            )
                            .w(px(44.0))
                            .h(px(44.0)),
                        ),
                    2 => nsh_base::nav_page().title("環境").child(notices()).child(
                        connections::screen(hosts, host, move || settings.set(false)),
                    ),
                    3 => nsh_base::nav_page()
                        .title("ファイル")
                        .child(notices())
                        .child(files::screen()),
                    4 => nsh_base::nav_page()
                        .title(move || {
                            files.file.with(|target| {
                                target
                                    .as_ref()
                                    .map(|target| {
                                        agent_core::presentation::markdown::file_basename(
                                            &target.path,
                                        )
                                        .to_owned()
                                    })
                                    .unwrap_or_default()
                            })
                        })
                        .child(notices())
                        .child(editor::editors()),
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
                settings.set(false);
                if route.get_untracked() == 3 {
                    files.close();
                }
                if route.get_untracked() == 4 {
                    files.close_file();
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
