use super::*;
use agent_core::presentation::error::localized_error_message;
use agent_protocol::browser::BrowserImportProfile;

impl Browser {
    pub(super) fn credentials_panel(
        &self,
        metadata: BrowserCredentials,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let language = crate::localization::language(cx);
        let mut content = v_flex()
            .w_full()
            .max_w(px(560.))
            .mx_auto()
            .p_4()
            .gap_3()
            .child(div().text_sm().text_color(rgb(appearance::MUTED)).child(
                language.text("ホストのブラウザから、ログイン状態や保存パスワードを取り込めます。"),
            ));
        if let Some(origin) = metadata.origin {
            content = content.child(Self::saved_passwords(
                origin,
                metadata.accounts,
                metadata.password_error,
                cx,
            ));
        }
        content = content.child(
            div()
                .mt_2()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .child(language.text("取り込み元のプロファイル")),
        );
        if metadata.profiles.is_empty() {
            content = content.child(
                div()
                    .text_sm()
                    .text_color(rgb(appearance::MUTED))
                    .child(language.text("取り込めるブラウザプロファイルがありません。")),
            );
        }
        for (index, profile) in metadata.profiles.into_iter().enumerate() {
            content = content.child(Self::import_profile(index, profile, cx));
        }
        if !metadata.import_errors.is_empty() {
            content = content.child(
                div()
                    .mt_3()
                    .text_xs()
                    .text_color(rgb(appearance::MUTED))
                    .child(language.text("読み取れなかったブラウザ")),
            );
            for error in metadata.import_errors {
                content = content.child(
                    v_flex()
                        .gap_1()
                        .text_xs()
                        .text_color(rgb(appearance::MUTED))
                        .child(div().font_weight(FontWeight::MEDIUM).child(error.browser))
                        .child(localized_error_message(language, &error.message)),
                );
            }
        }
        v_flex()
            .absolute()
            .inset_0()
            .size_full()
            .min_h_0()
            .bg(rgb(appearance::BACKGROUND))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|view, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    view.credentials = None;
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .child(
                h_flex()
                    .px_4()
                    .py_3()
                    .gap_2()
                    .border_b_1()
                    .border_color(rgb(appearance::BORDER))
                    .child(
                        Icon::new(IconName::Globe)
                            .small()
                            .text_color(rgb(appearance::MUTED)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(language.text("ブラウザデータ")),
                    )
                    .child(
                        Button::new("close-browser-data")
                            .icon(IconName::Close)
                            .ghost()
                            .small()
                            .accessibility_label(language.text("ブラウザデータを閉じる"))
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.credentials = None;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .id("browser-credentials")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(content),
            )
    }

    fn import_profile(
        index: usize,
        profile: BrowserImportProfile,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let language = crate::localization::language(cx);
        let cookies = profile.id.clone();
        let passwords = profile.id;
        v_flex()
            .p_3()
            .gap_3()
            .rounded_lg()
            .border_1()
            .border_color(rgb(appearance::BORDER))
            .bg(rgb(appearance::SURFACE))
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(profile.label),
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        Button::new(SharedString::from(format!("import-cookies-{index}")))
                            .label(language.text("ログイン状態を取り込む"))
                            .small()
                            .outline()
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.send(
                                    BrowserAction::ImportCookies {
                                        profile: cookies.clone(),
                                    },
                                    cx,
                                )
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("import-passwords-{index}")))
                            .label(language.text("パスワードを取り込む"))
                            .small()
                            .outline()
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.send(
                                    BrowserAction::ImportPasswords {
                                        profile: passwords.clone(),
                                    },
                                    cx,
                                )
                            })),
                    ),
            )
    }

    fn saved_passwords(
        origin: String,
        accounts: Vec<String>,
        error: Option<String>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let language = crate::localization::language(cx);
        let mut section = v_flex()
            .p_3()
            .gap_2()
            .rounded_lg()
            .border_1()
            .border_color(rgb(appearance::BORDER))
            .bg(rgb(appearance::SURFACE))
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(language.text("保存パスワード")),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(appearance::MUTED))
                    .child(origin.clone()),
            );
        if accounts.is_empty() {
            section = section.child(div().text_sm().text_color(rgb(appearance::MUTED)).child(
                localized_error_message(
                    language,
                    &error.unwrap_or_else(|| "このサイトの保存パスワードはありません。".into()),
                ),
            ));
        }
        for (index, username) in accounts.iter().enumerate() {
            let origin = origin.clone();
            let username = username.clone();
            section = section.child(
                Button::new(SharedString::from(format!("fill-password-{index}")))
                    .label(if username.is_empty() {
                        language.text("ユーザー名なし").into()
                    } else {
                        username.clone()
                    })
                    .small()
                    .outline()
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.send(
                            BrowserAction::FillPassword {
                                origin: origin.clone(),
                                username: username.clone(),
                            },
                            cx,
                        )
                    })),
            );
        }
        section.when(!accounts.is_empty(), |section| {
            section.child(
                Button::new("delete-site-passwords")
                    .label(language.text("このサイトの保存パスワードを削除"))
                    .small()
                    .ghost()
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.send(
                            BrowserAction::DeletePasswords {
                                origin: origin.clone(),
                            },
                            cx,
                        )
                    })),
            )
        })
    }
}
