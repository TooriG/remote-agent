use crate::{Connection, native, shared::Shared};
use agent_core::{
    presentation::{
        conversation::{
            ActivityPresentation, ConversationRowContent, RenderedConversation, WorkProgress,
            activity_is_expanded,
        },
        view::Presentation,
    },
    state::{Intent, operations as op},
};
use nsh_base::{
    text_view::{MarkdownOptions, text_view},
    theme,
};
use nsh_core::prelude::*;
use serde_json::json;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    User,
    Response,
    Activity,
}

#[derive(Clone, PartialEq)]
struct Row {
    id: String,
    body: SharedString,
    kind: Kind,
    actions: bool,
    activity: Option<ActivityPresentation>,
    fork: Option<agent_protocol::ids::TurnId>,
    retry: Option<agent_protocol::operations::RetryPreparation>,
    images: Vec<String>,
    detail: Option<crate::activity::Item>,
    progress: Option<WorkProgress>,
}

fn project(
    conversation: &RenderedConversation,
    expanded_work: &HashMap<String, String>,
) -> Vec<Row> {
    conversation
        .turns
        .iter()
        .flat_map(|turn| {
            let mut expanded = false;
            let progress_shown = turn.rows.iter().any(|row| matches!(&row.content, ConversationRowContent::ActivityHeader { activity } if activity.is_in_progress));
            turn.conversation_rows().into_iter().filter_map(move |row| {
                match &row.content {
                    ConversationRowContent::ActivityHeader { activity } => {
                        expanded = activity_is_expanded(&activity.status, expanded_work.get(&activity.id).cloned());
                    }
                    ConversationRowContent::Activity { .. } if !expanded => return None,
                    ConversationRowContent::InProgress { .. } if progress_shown => return None,
                    _ => {}
                }
                let progress = match &row.content {
                    ConversationRowContent::ActivityHeader { activity } if activity.is_in_progress => Some(turn.work_progress(!expanded)),
                    ConversationRowContent::InProgress { .. } => Some(turn.work_progress(false)),
                    _ => None,
                };
                Some((row, progress))
            })
        })
        .chain(conversation.queued.iter().map(|item| {
            (agent_core::presentation::conversation::ConversationRow {
                id: item.data.id.clone(),
                content: ConversationRowContent::User { item: item.clone() },
                timestamp_ms: item.data.timestamp_ms,
            }, None)
        }))
        .chain(conversation.session_rows.iter().cloned().map(|row| (row, None)))
        .filter_map(|(row, progress)| {
            let images = match &row.content {
                ConversationRowContent::User { item }
                | ConversationRowContent::Activity { item, .. }
                | ConversationRowContent::Response { item, .. } => item.data.image_sources.clone(),
                _ => Vec::new(),
            };
            let activity = match &row.content { ConversationRowContent::ActivityHeader { activity } => Some(activity.clone()), _ => None };
            let actions = matches!(&row.content, ConversationRowContent::Response { show_actions: true, .. });
            let fork = match &row.content { ConversationRowContent::Response { fork_turn_id, .. } => fork_turn_id.clone(), _ => None };
            let retry = match &row.content { ConversationRowContent::Preparation { preparation } => preparation.retry.clone(), _ => None };
            let detail = match &row.content {
                ConversationRowContent::Activity { item, turn_id } if item.data.collapsible => {
                    Some(crate::activity::Item { source: Shared(item.clone()), turn: turn_id.clone() })
                }
                _ => None,
            };
            let (kind, body) = match row.content {
                ConversationRowContent::User { item } => {
                    (Kind::User, item.data.body.clone().unwrap_or_default())
                }
                ConversationRowContent::Response { item, .. } => {
                    (Kind::Response, item.data.body.clone().unwrap_or_default())
                }
                ConversationRowContent::Activity { item, .. } => {
                    (Kind::Activity, item.data.body.clone().or_else(|| item.data.title.clone()).unwrap_or_default())
                }
                ConversationRowContent::ActivityHeader { activity } => {
                    (Kind::Activity, activity.activity_summary)
                }
                ConversationRowContent::Preparation { preparation } => {
                    (Kind::Activity, preparation.text)
                }
                ConversationRowContent::ProviderHandoff { text } => (Kind::Activity, text),
                ConversationRowContent::Error { error } => (Kind::Activity, error.message),
                ConversationRowContent::InProgress { .. } => (Kind::Activity, "作業中…".into()),
                ConversationRowContent::PendingRequest { .. } => return None,
            };
            Some(Row {
                id: row.id,
                body: body.into(),
                kind,
                actions,
                activity,
                fork,
                retry,
                images,
                detail,
                progress,
            })
        })
        .collect()
}

pub(crate) fn list(publication: Memo<Shared<Presentation>>) -> impl IntoElement {
    let connection = use_context::<Connection>().unwrap();
    let preview = use_context::<crate::media::Preview>().unwrap();
    let files = use_context::<crate::files::Browser>().unwrap();
    let expanded = signal(HashMap::<String, String>::new());
    let expanded_items = signal(HashSet::<String>::new());
    let list = element_ref();
    let clock = signal(std::time::SystemTime::now());
    spawn(async move {
        loop {
            timer(std::time::Duration::from_secs(1)).await;
            if connection
                .publication
                .with_untracked(|p| p.composer.controls.running_turn.is_some())
            {
                clock.set(std::time::SystemTime::now());
            }
        }
    });
    let target = computed(move || {
        (
            connection.host.get(),
            publication.with(|p| p.composer.controls.navigation.draft_key.clone()),
        )
    });
    effect(move || {
        target.get();
        expanded.set(HashMap::new());
        expanded_items.set(HashSet::new());
    });
    let rows = computed(move || {
        publication.with(|p| {
            p.conversation
                .as_ref()
                .map(|conversation| expanded.with(|expanded| project(conversation, expanded)))
                .unwrap_or_default()
        })
    });
    let history = computed(move || publication.with(|p| p.context.history()));
    view()
        .flex_1()
        .min_h_0()
        .child(when(
            move || history.get().is_some(),
            move || {
                view()
                    .gap_2()
                    .items_center()
                    .child(
                        native::text_button(
                            move || {
                                json!(history.with(|history| {
                                    history
                                        .as_ref()
                                        .map(|history| history.load_label.clone())
                                        .unwrap_or_default()
                                }))
                            },
                            move || {
                                if let Some(thread_id) =
                                    publication.with_untracked(|p| p.context.navigation().thread_id)
                                {
                                    connection.dispatch(Intent::ReadOlder { thread_id });
                                }
                            },
                        )
                        .prop("enabled", move || {
                            json!(history.with(|history| {
                                history.as_ref().is_some_and(|history| !history.loading)
                            }))
                        }),
                    )
                    .child(text(move || {
                        history.with(|history| {
                            history
                                .as_ref()
                                .and_then(|history| history.error.as_ref())
                                .map(ToString::to_string)
                                .unwrap_or_default()
                        })
                    }))
            },
        ))
        .child(text(move || {
            publication.with(|p| p.context.history_notice().unwrap_or_default())
        }))
        .child(
            virtual_list(
                move || rows.with(Vec::len),
                move |at| {
                    let row = computed(move || rows.with(|rows| rows.get(at.index()).cloned()));
                    let user =
                        move || row.with(|r| r.as_ref().is_some_and(|r| r.kind == Kind::User));
                    let activity =
                        move || row.with(|r| r.as_ref().is_some_and(|r| r.kind == Kind::Activity));
                    let detail = computed(move || row.with(|r| r.as_ref().and_then(|r| r.detail.clone())));
                    let label = move || row.with(|row| row.as_ref().map(|row| {
                        row.progress.as_ref().map(|progress| progress.label(clock.get().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs_f64()).into()).unwrap_or_else(|| row.body.clone())
                    }));
                    view()
                        .w_full()
                        .items_center()
                        .px(move || if is_desktop() { px(24.0) } else { px(16.0) })
                        .py_3()
                        .child(
                            view()
                                .w_full()
                                .max_w(px(780.0))
                                .items_start()
                                .style_when(user, |s| s.items_end())
                                .child(when(
                                    move || {
                                        row.with(|row| {
                                            row.as_ref().is_some_and(|row| row.activity.as_ref().is_some_and(|activity| activity.activity_can_collapse))
                                        })
                                    },
                                    move || {
                                        native::text_button(
                                            move || {
                                                json!(label().unwrap_or_default().to_string())
                                            },
                                            move || {
                                                if let Some(activity) = row.with_untracked(|row| {
                                                    row.as_ref()
                                                        .and_then(|row| row.activity.clone())
                                                }) {
                                                    let opening = !expanded.with_untracked(|all| {
                                                        activity_is_expanded(
                                                            &activity.status,
                                                            all.get(&activity.id).cloned(),
                                                        )
                                                    });
                                                    expanded.update(|all| {
                                                        if opening {
                                                            all.insert(
                                                                activity.id.clone(),
                                                                activity.status,
                                                            );
                                                        } else {
                                                            all.remove(&activity.id);
                                                        }
                                                    });
                                                    if opening
                                                        && let Some(load) = activity.load_items
                                                    {
                                                        connection
                                                            .dispatch(Intent::LoadTurnItems(load));
                                                    }
                                                    if opening {
                                                        list.scroll_to_index(at.index() as u32, nsh_core::geometry::Percentage(0.0), px(0.0), false);
                                                    }
                                                }
                                            },
                                        )
                                    },
                                ))
                                .child(when(move || detail.get().is_some(), move || crate::activity::details(detail, expanded_items, list, at)))
                                .child(
                                    view()
                                        .max_w(relative(1.0))
                                        .style_when(user, |s| {
                                            s.max_w(relative(0.9))
                                                .px(px(16.0))
                                                .py(px(14.0))
                                                .rounded(px(22.0))
                                                .bg(move || theme().muted)
                                        })
                                        .style_when(move || !user(), |s| s.w_full())
                                        .child(
                                            text_view()
                                                .on_image_press(move |image: &nsh_base::text_view::Image| {
                                                    preview.load(crate::media::load_image(connection.client.get(), connection.host.get_untracked(), connection.publication.with_untracked(|p| p.composer.controls.navigation.cwd.clone()), image.src.clone()));
                                                })
                                                .on_link_press(move |event: &nsh_base::text_view::TextViewLinkPressEvent| files.open(event.url.to_string(), connection.publication.with_untracked(|p| p.composer.controls.navigation.cwd.clone()), connection.publication.with_untracked(|p| p.context.home()), connection.host.get_untracked(), connection.client.get()))
                                                .render_image(
                                                    |image: &nsh_base::text_view::Image| {
                                                        let source = image.src.to_string();
                                                        crate::media::image_view(move || {
                                                            source.clone()
                                                        }, 320.0, 240.0)
                                                    },
                                                )
                                                .markdown(label)
                                                .selectable(true)
                                                .font_weight(FontWeight::NORMAL)
                                                .text_size(move || {
                                                    if is_desktop() { px(14.0) } else { px(16.0) }
                                                })
                                                .options(MarkdownOptions {
                                                    streaming: true,
                                                    ..Default::default()
                                                })
                                                .style_when(activity, |s| {
                                                    s.text_color(move || theme().muted_foreground)
                                                })
                                                .style_when(
                                                    move || {
                                                        row.with(|row| {
                                                            row.as_ref().is_some_and(|row| {
                                                                row.activity.as_ref().is_some_and(|activity| activity.activity_can_collapse) || row.detail.is_some()
                                                            })
                                                        })
                                                    },
                                                    |s| s.hidden(),
                                                ),
                                        ),
                                )
                                .child(each(
                                    move || {
                                        row.with(|row| {
                                            row.as_ref()
                                                .map(|row| row.images.clone())
                                                .unwrap_or_default()
                                        })
                                    },
                                    |source: &String| source.clone(),
                                    |source, _| crate::media::image_view(move || source.get(), 320.0, 240.0),
                                ))
                                .child(when(
                                    move || {
                                        row.with(|row| row.as_ref().is_some_and(|row| row.actions))
                                    },
                                    move || {
                                        view()
                                            .flex_row()
                                            .gap_4()
                                            .mt_2()
                                            .child(native::button(
                                                "doc.on.doc",
                                                "コピー",
                                                move || {
                                                    if let Some(row) = row.get_untracked() {
                                                        set_clipboard(row.body.to_string());
                                                    }
                                                },
                                            ))
                                            .child(when(
                                                move || {
                                                    row.with(|row| {
                                                        row.as_ref()
                                                            .is_some_and(|row| row.fork.is_some())
                                                    })
                                                },
                                                move || {
                                                    native::button(
                                                        "arrow.triangle.branch",
                                                        "会話を分岐",
                                                        move || {
                                                            if let Some(last_turn_id) = row
                                                                .with_untracked(|row| {
                                                                    row.as_ref().and_then(|row| {
                                                                        row.fork.clone()
                                                                    })
                                                                })
                                                                && let Some(thread_id) = publication
                                                                    .with_untracked(|p| {
                                                                        p.context
                                                                            .navigation()
                                                                            .thread_id
                                                                    })
                                                            {
                                                                connection.dispatch(
                                                                    Intent::ForkSession(
                                                                        op::ForkSession {
                                                                            thread_id,
                                                                            last_turn_id,
                                                                        },
                                                                    ),
                                                                );
                                                            }
                                                        },
                                                    )
                                                },
                                            ))
                                    },
                                )),
                        )
                        .child(when(
                            move || {
                                row.with(|row| row.as_ref().is_some_and(|row| row.retry.is_some()))
                            },
                            move || {
                                native::text_button(json!("再試行"), move || {
                                    if let Some(retry) = row.with_untracked(|row| {
                                        row.as_ref().and_then(|row| row.retry.clone())
                                    }) {
                                        connection.dispatch(Intent::RetryPreparation(retry));
                                    }
                                })
                            },
                        ))
                },
            )
            .keys(move || rows.with(|rows| rows.iter().map(|r| r.id.clone()).collect::<Vec<_>>()))
            .element_ref(&list)
            .estimated_item_size(px(170.0))
            .follow_end(px(60.0))
            .flex_1()
            .min_h_0(),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expanding_running_work_reveals_output_without_a_second_progress_row() {
        let source: agent_protocol::models::Thread = serde_json::from_value(json!({
            "id": {"provider":"codex", "id":"thread"},
            "turns": [{"id":"turn", "status":"running", "items":[
                {"id":"command", "status":"completed", "clientInputId":null,
                 "body":{"inline":{"body":{"commandExecution":{
                     "command":"pwd", "cwd":null, "output":"/project", "exitCode":0
                 }}}}},
                {"id":"answer", "status":"unknown", "clientInputId":null,
                 "body":{"inline":{"body":{"assistantText":{"text":"answer", "phase":"unknown"}}}}}
            ]}]
        }))
        .unwrap();
        let key = source.id.as_ref().unwrap().into();
        let conversation = agent_core::presentation::conversation::project_conversation(
            &key,
            &Default::default(),
            std::sync::Arc::new(source),
            &None,
        );
        let collapsed = project(&conversation, &HashMap::new());
        assert!(!collapsed.iter().any(|row| row.detail.is_some()));
        let activity = collapsed
            .iter()
            .find_map(|row| row.activity.as_ref())
            .unwrap();
        let expanded = project(
            &conversation,
            &[(activity.id.clone(), activity.status.clone())].into(),
        );
        assert!(expanded.iter().any(|row| row.detail.is_some()));
        assert!(expanded.iter().any(|row| row.body.as_ref() == "answer"));
        assert_eq!(
            expanded
                .iter()
                .filter(|row| row.kind == Kind::Activity)
                .count(),
            2
        );
    }
}
