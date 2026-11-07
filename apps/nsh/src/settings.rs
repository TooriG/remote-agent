//! The desktop settings, as T3 Code's: a blurred overlay whose searchable
//! sections sit beside a breadcrumb, each section a set of grouped rows.
use crate::ui::{self, Enabled, Icon, colors};
use crate::{Connection, Navigation, application::Host, environment};
use agent_core::presentation::connections::ConnectionAgent;
use agent_protocol::session::ProviderKind;
use nsh_core::{element::ImageSrc, prelude::*};
use serde_json::json;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Page {
    General,
    Providers,
    Environments,
    Worktrees,
}

impl Page {
    const ALL: [Self; 4] = [
        Self::General,
        Self::Providers,
        Self::Environments,
        Self::Worktrees,
    ];

    fn title(self) -> &'static str {
        match self {
            Self::General => "一般",
            Self::Providers => "プロバイダー",
            Self::Environments => "環境",
            Self::Worktrees => "ワークツリー",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::General => "この端末で使用するアプリの設定です。",
            Self::Providers => "選択した環境のプロバイダー・アカウント・更新を管理します。",
            Self::Environments => "作業する環境と、このPCへ接続する端末を管理します。",
            Self::Worktrees => "新しい会話の作業場所と、作成済みのワークツリーを管理します。",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Self::General => Icon::SLIDERS,
            Self::Providers => Icon::BOT,
            Self::Environments => Icon::SERVER,
            Self::Worktrees => Icon::FOLDER_TREE,
        }
    }
}

pub(crate) fn overlay(hosts: Memo<Vec<Host>>) -> View {
    let connection = use_context::<Connection>().unwrap();
    let page = signal(Page::General);
    let locked = computed(move || connection.state.with(|state| state.account_locked()));
    let close = move || {
        if !locked.get_untracked() {
            connection
                .application
                .with(|app| app.navigate(Navigation::Settings(false)));
        }
    };
    environment::read_settings(connection);
    view()
        .absolute()
        .top(px(0.0))
        .left(px(0.0))
        .right(px(0.0))
        .bottom(px(0.0))
        .items_center()
        .justify_center()
        .p(px(40.0))
        .bg(rgba(0x080808a3))
        .backdrop_blur(px(4.0))
        .on_press(close)
        .child(
            view()
                .flex_row()
                .w_full()
                .h_full()
                .max_w(px(1280.0))
                .max_h(px(960.0))
                .rounded(px(16.0))
                .border_1()
                .border_color(colors::BORDER)
                .overflow_hidden()
                .bg(colors::BACKGROUND)
                .on_press(|| {})
                .child(navigation(page))
                .child(
                    view()
                        .flex_1()
                        .min_w_0()
                        .child(header(page, close, locked))
                        .child(
                            scroll().flex_1().child(
                                view()
                                    .w_full()
                                    .max_w(px(928.0))
                                    .p_8()
                                    .gap_8()
                                    .child(heading(page))
                                    .child(when(
                                        move || page.get() == Page::General,
                                        move || general(connection),
                                    ))
                                    .child(when(
                                        move || page.get() == Page::Providers,
                                        move || providers(connection),
                                    ))
                                    .child(when(
                                        move || page.get() == Page::Environments,
                                        move || environments(connection, hosts),
                                    ))
                                    .child(when(
                                        move || page.get() == Page::Worktrees,
                                        move || worktrees(connection),
                                    )),
                            ),
                        ),
                ),
        )
}

/// The sections, filtered by the search above them.
fn navigation(page: Signal<Page>) -> View {
    let query = signal(String::new());
    let visible = move || {
        let query = query.get().trim().to_lowercase();
        Page::ALL
            .into_iter()
            .filter(|page| query.is_empty() || page.title().to_lowercase().contains(&query))
            .collect::<Vec<_>>()
    };
    view()
        .w(px(240.0))
        .flex_shrink_0()
        .h_full()
        .p_3()
        .gap_1()
        .bg(colors::SIDEBAR)
        .border_r_1()
        .border_r_color(colors::BORDER)
        .child(
            crate::native::search(query)
                .prop("appearance", json!("plain"))
                .prop("placeholder", json!("設定を検索"))
                .h(px(32.0))
                .mb_2(),
        )
        .child(
            text("設定")
                .text_size(px(12.0))
                .text_color(colors::MUTED)
                .px_2()
                .py_1(),
        )
        .child(each(
            visible,
            |page: &Page| *page,
            move |item, _| {
                let item = item.get_untracked();
                nsh_base::button()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .h(px(32.0))
                    .px_2()
                    .rounded(px(ui::RADIUS))
                    .text_color(colors::FOREGROUND)
                    .hover(|s| s.bg(colors::ROW_HOVER))
                    .style_when(move || page.get() == item, |s| s.bg(colors::SELECTED))
                    .on_press(move || page.set(item))
                    .child(ui::icon(item.icon(), 16.0))
                    .child(text(item.title()).text_size(px(14.0)))
            },
        ))
}

/// The breadcrumb and the close button.
fn header(page: Signal<Page>, close: impl Fn() + 'static, locked: Memo<bool>) -> View {
    view()
        .flex_row()
        .items_center()
        .gap_3()
        .h(px(48.0))
        .flex_shrink_0()
        .px_5()
        .border_b_1()
        .border_b_color(colors::BORDER)
        .child(text("設定").text_size(px(14.0)).text_color(colors::MUTED))
        .child(text("/").text_size(px(14.0)).text_color(colors::MUTED))
        .child(
            text(move || page.get().title())
                .flex_1()
                .text_size(px(14.0)),
        )
        .child(ui::icon_button(Icon::CLOSE, "閉じる", close).enabled(move || !locked.get()))
}

fn heading(page: Signal<Page>) -> View {
    view()
        .gap_2()
        .child(
            text(move || page.get().title())
                .text_size(px(26.0))
                .font_weight(FontWeight::SEMIBOLD),
        )
        .child(
            text(move || page.get().description())
                .text_size(px(14.0))
                .text_color(colors::MUTED),
        )
}

fn general(connection: Connection) -> View {
    let connected = move || connection.settings.with(|p| p.connected());
    ui::card(vec![
        ui::setting(
            "Bex Host",
            move || environment::host_software(connection),
            ui::text_button("バージョン情報を再読み込み", move || {
                environment::reload_host_software(connection)
            })
            .enabled(connected),
        ),
        ui::setting(
            "アップデート",
            || environment::UPDATE_NOTE.to_owned(),
            ui::text_button("アップデートを確認", environment::check_updates),
        ),
    ])
}

/// The Host's providers beside the selected one's accounts.
fn providers(connection: Connection) -> View {
    let selected = computed(move || {
        connection
            .state
            .with(|state| state.provider)
            .unwrap_or(ProviderKind::Codex)
    });
    let select = move |provider| {
        connection
            .application
            .with(|app| app.navigate(Navigation::Provider(Some(provider))))
    };
    view()
        .gap_4()
        .child(
            view().flex_row().justify_end().child(
                ui::text_button("状態を確認", move || {
                    environment::reload_providers(connection)
                })
                .enabled(move || environment::providers_reloadable(connection)),
            ),
        )
        .child(
            view()
                .flex_row()
                .items_start()
                .gap_6()
                .child(
                    view()
                        .w(px(250.0))
                        .flex_shrink_0()
                        .rounded(px(12.0))
                        .border_1()
                        .border_color(colors::BORDER)
                        .bg(colors::SURFACE)
                        .overflow_hidden()
                        .child(each(
                            move || environment::agents(connection),
                            |agent: &ConnectionAgent| agent.provider,
                            move |agent, _| provider_row(connection, agent, selected, select),
                        )),
                )
                .child(view().flex_1().min_w_0().child(each(
                    move || vec![selected.get()],
                    |provider: &ProviderKind| *provider,
                    |provider, _| crate::accounts::screen(provider.get_untracked()),
                ))),
        )
        .child(
            text(environment::PROVIDERS_NOTE)
                .text_size(px(12.0))
                .text_color(colors::MUTED),
        )
}

fn provider_row(
    connection: Connection,
    agent: ReadSignal<ConnectionAgent>,
    selected: Memo<ProviderKind>,
    select: impl Fn(ProviderKind) + 'static,
) -> View {
    let provider = agent.with_untracked(|a| a.provider);
    view()
        .flex_row()
        .items_center()
        .gap_2()
        .p_3()
        .border_b_1()
        .border_b_color(colors::BORDER)
        .style_when(
            move || selected.get() == provider,
            |s| s.bg(colors::SELECTED),
        )
        .child(
            nsh_base::button()
                .flex_1()
                .min_w_0()
                .flex_row()
                .items_center()
                .gap_2()
                .on_press(move || select(provider))
                .child(match crate::model_controls::provider_mark(provider.key()) {
                    Some((markup, color)) => svg(ImageSrc::svg_markup(markup))
                        .w(px(16.0))
                        .h(px(16.0))
                        .text_color(color)
                        .into_element(),
                    None => view().into_element(),
                })
                .child(
                    view()
                        .flex_1()
                        .min_w_0()
                        .gap_1()
                        .child(
                            text(move || agent.with(|a| a.name.clone()))
                                .text_size(px(14.0))
                                .font_weight(FontWeight::SEMIBOLD),
                        )
                        .child(
                            text(move || {
                                agent.with(|a| {
                                    a.error
                                        .as_ref()
                                        .map(ToString::to_string)
                                        .unwrap_or_else(|| a.label.clone())
                                })
                            })
                            .text_size(px(12.0))
                            .text_color(colors::MUTED),
                        ),
                ),
        )
        .child(environment::provider_switch(connection, agent))
}

fn environments(connection: Connection, hosts: Memo<Vec<Host>>) -> View {
    view()
        .gap_8()
        .child(ui::card(vec![ui::setting(
            move || SharedString::from(environment::host_name(connection)),
            move || environment::connection_status(connection),
            environment::connection_switch(connection),
        )]))
        .child(crate::connections::screen(hosts))
}

fn worktrees(connection: Connection) -> View {
    ui::card(vec![ui::setting(
        "ワークツリー設定",
        || {
            "新しい会話をワークツリーで始めるか、引き継ぐファイル、自動削除を設定します。"
                .to_owned()
        },
        ui::text_button("開く", crate::worktrees::open)
            .enabled(move || connection.settings.with(|p| p.connected())),
    )])
}
