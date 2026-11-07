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
                        .prop("identifier", move || json!(notice.with(|notice| {
                            matches!(&notice.id, agent_core::state::operations::ErrorId::Operation {
                                key: agent_core::state::operations::OperationKey::WorkspaceReview, ..
                            }).then_some("files.diff.retry")
                        })))
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
