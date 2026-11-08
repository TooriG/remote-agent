use crate::Connection;
use crate::l10n::{localized, tr};
use crate::ui::{self, Icon, colors};
use agent_core::{
    presentation::error::{ErrorNotice, ErrorScope},
    state::Intent,
};
use nsh_core::{element::rgba, prelude::*};

/// Errors as T3 Code's error banners: the title and its dismissal, the reason,
/// and the recovery Core offers.
pub(crate) fn notices(
    workspace: impl Fn() -> bool + 'static,
    settings: impl Fn() -> bool + 'static,
) -> impl IntoElement {
    let connection = use_context::<Connection>().unwrap();
    each(
        move || {
            connection.publication.with(|p| {
                let mut notices = p.context.notices();
                if connection.pane.is_none() {
                    for scope in [ErrorScope::Application, ErrorScope::Projects] {
                        notices.extend(p.snapshot.error_notices(scope));
                    }
                } else {
                    notices.extend(p.snapshot.pane_error_notice());
                }
                if workspace() {
                    notices.extend(p.snapshot.error_notices(ErrorScope::Workspace));
                }
                if settings() {
                    notices.extend(p.settings.notices());
                }
                notices
            })
        },
        |notice: &ErrorNotice| format!("{:?}", notice.id),
        move |notice, _| {
            view()
                .m_2()
                .px_4()
                .py_3()
                .gap_2()
                .rounded(px(12.0))
                .border_1()
                .border_color(rgba(0xff8e8652))
                .bg(rgba(0xff8e8614))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(
                            text(move || notice.with(|notice| localized(notice.title.clone())))
                                .text_size(px(13.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .flex_1()
                                .min_w_0(),
                        )
                        .child(ui::small_icon_button(
                            Icon::CLOSE,
                            tr("閉じる"),
                            move || {
                                connection.dispatch(Intent::DismissError {
                                    id: notice.with_untracked(|notice| notice.id.clone()),
                                });
                            },
                        )),
                )
                .child(
                    text(move || notice.with(|notice| localized(notice.text.to_string())))
                        .selectable(true)
                        .text_size(px(13.0))
                        .text_color(colors::MUTED),
                )
                .child(when(
                    move || notice.with(|notice| notice.recovery_label.is_some()),
                    move || {
                        view().flex_row().child(ui::text_button(
                            move || {
                                SharedString::from(notice.with(|notice| {
                                    localized(notice.recovery_label.clone().unwrap_or_default())
                                }))
                            },
                            move || {
                                connection.dispatch(Intent::RecoverError {
                                    id: notice.with_untracked(|notice| notice.id.clone()),
                                });
                            },
                        ))
                    },
                ))
        },
    )
}
