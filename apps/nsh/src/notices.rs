use crate::{Connection, native};
use agent_core::{
    presentation::error::{ErrorNotice, ErrorScope},
    state::Intent,
};
use nsh_core::prelude::*;
use serde_json::json;

pub(crate) fn notices(
    workspace: impl Fn() -> bool + 'static,
    settings: impl Fn() -> bool + 'static,
) -> impl IntoElement {
    let connection = use_context::<Connection>().unwrap();
    each(
        move || {
            connection.publication.with(|p| {
                let mut notices = p.context.notices();
                for scope in [ErrorScope::Application, ErrorScope::Projects] {
                    notices.extend(p.snapshot.error_notices(scope));
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
                .p_2()
                .gap_2()
                .child(text(move || notice.with(|notice| notice.title.clone())))
                .child(text(move || notice.with(|notice| notice.text.to_string())).selectable(true))
                .child(when(
                    move || notice.with(|notice| notice.recovery_label.is_some()),
                    move || {
                        native::text_button(
                            move || {
                                json!(notice.with(|notice| {
                                    notice.recovery_label.clone().unwrap_or_default()
                                }))
                            },
                            move || {
                                connection.dispatch(Intent::RecoverError {
                                    id: notice.with_untracked(|notice| notice.id.clone()),
                                });
                            },
                        )
                    },
                ))
                .child(native::text_button(json!("閉じる"), move || {
                    connection.dispatch(Intent::DismissError {
                        id: notice.with_untracked(|notice| notice.id.clone()),
                    });
                }))
        },
    )
}
