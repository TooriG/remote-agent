use super::{ConversationRow, ConversationRowContent};
use crate::presentation::markdown::{
    MarkdownFileSurface, markdown_file_target, markdown_without_images,
};
use std::collections::HashSet;

fn image_key(source: &str) -> String {
    markdown_file_target(source.into(), String::new(), MarkdownFileSurface::Mobile)
        .map_or_else(|| source.to_owned(), |file| file.path)
}

/// A visible inline image takes over its earlier generation preview. Only
/// responses in the same user exchange count; collapsed work cannot replace it.
pub(super) fn without_repeated_generated_images(
    mut rows: Vec<ConversationRow>,
) -> Vec<ConversationRow> {
    if !rows.iter().any(|row| {
        matches!(&row.content,
            ConversationRowContent::Response { item, .. } if item.data.kind == "imageGeneration"
        )
    }) {
        return rows;
    }
    let mut inline_images = HashSet::new();
    rows.reverse();
    rows.retain(|row| {
        match &row.content {
            ConversationRowContent::User { .. } => inline_images.clear(),
            ConversationRowContent::Activity { item, .. } if item.data.kind == "user" => {
                inline_images.clear()
            }
            ConversationRowContent::Response { item, .. } if item.data.kind == "agent" => {
                if let Some(text) = &item.data.body {
                    inline_images.extend(
                        markdown_without_images(text)
                            .1
                            .iter()
                            .map(|source| image_key(source)),
                    );
                }
            }
            ConversationRowContent::Response { item, .. }
                if item.data.kind == "imageGeneration" =>
            {
                return item.data.body.is_some()
                    || item.data.image_sources.is_empty()
                    || !item
                        .data
                        .image_sources
                        .iter()
                        .all(|source| inline_images.contains(&image_key(source)));
            }
            _ => {}
        }
        true
    });
    rows.reverse();
    rows
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use serde_json::json;

    fn image(id: &str, path: &str) -> Arc<models::Item> {
        Arc::new(models::Item::new(
            id.into(),
            models::ItemStatus::Completed,
            models::ItemBody::ImageGeneration {
                saved_path: Some(path.into()),
                data: None,
                revised_prompt: None,
            },
        ))
    }

    fn answer(text: &str, phase: &str) -> Arc<models::Item> {
        Arc::new(
            serde_json::from_value(json!({
                "id": "answer", "status": "completed",
                "body": {"inline": {"body": {"assistantText": {"text": text, "phase": phase}}}}
            }))
            .unwrap(),
        )
    }

    fn thread(items: Vec<Arc<models::Item>>, status: models::TurnStatus) -> Arc<models::Thread> {
        Arc::new(models::Thread {
            turns: Some(vec![Arc::new(models::Turn {
                id: "turn".into(),
                status,
                items: Some(items),
                ..Default::default()
            })]),
            ..Default::default()
        })
    }

    fn response_ids(conversation: &RenderedConversation) -> Vec<&str> {
        conversation.turns[0]
            .rows
            .iter()
            .filter_map(|row| match &row.content {
                ConversationRowContent::Response { item, .. } => Some(item.data.id.as_str()),
                _ => None,
            })
            .collect()
    }

    #[rstest::rstest]
    #[case::inline("![phone](/phone.png)", false)]
    #[case::reference("![phone][preview]\n\n[preview]: /phone.png", false)]
    #[case::file_url("![phone](file:///phone.png)", false)]
    #[case::encoded_path("![phone](/%70hone.png)", false)]
    #[case::disclosure(
        "<details><summary>Preview</summary>\n\n![phone](/phone.png)\n\n</details>",
        false
    )]
    #[case::link("[phone](/phone.png)", true)]
    #[case::code("`![phone](/phone.png)`", true)]
    #[case::fenced("```md\n![phone](/phone.png)\n```", true)]
    #[case::escaped(r"\![phone](/phone.png)", true)]
    #[case::incomplete("![phone](/phone.png", true)]
    #[case::different_file("![phone](/other/phone.png)", true)]
    fn generated_image_is_shown_once_at_its_place_in_the_response(
        #[case] text: &str,
        #[case] standalone: bool,
        #[values(models::TurnStatus::Running, models::TurnStatus::Completed)]
        status: models::TurnStatus,
    ) {
        let projected = project_conversation(
            &Snapshot::default(),
            thread(
                vec![
                    image("phone", "/phone.png"),
                    image("desktop", "/desktop.png"),
                    answer(text, "final"),
                ],
                status,
            ),
            &None,
        );
        assert_eq!(
            response_ids(&projected),
            if standalone {
                vec!["phone", "desktop", "answer"]
            } else {
                vec!["desktop", "answer"]
            }
        );
        assert_eq!(
            projected.turns[0]
                .items()
                .last()
                .unwrap()
                .data
                .body
                .as_deref(),
            Some(text)
        );
    }

    #[test]
    fn collapsed_commentary_and_later_user_input_keep_the_generated_image() {
        let user = Arc::new(models::Item::new(
            "user".into(),
            models::ItemStatus::Completed,
            models::ItemBody::UserMessage {
                text: Some("Next question".into()),
                content: vec![],
            },
        ));
        for items in [
            vec![
                image("phone", "/phone.png"),
                answer("![phone](/phone.png)", "commentary"),
                answer("Done", "final"),
            ],
            vec![
                image("phone", "/phone.png"),
                user,
                answer("![phone](/phone.png)", "final"),
            ],
        ] {
            let projected = project_conversation(
                &Snapshot::default(),
                thread(items, models::TurnStatus::Completed),
                &None,
            );
            assert_eq!(response_ids(&projected), ["phone", "answer"]);
        }
    }

    #[test]
    fn all_embedded_results_replace_previews_across_live_commentary_segments() {
        for status in [models::TurnStatus::Running, models::TurnStatus::Completed] {
            let projected = project_conversation(
                &Snapshot::default(),
                thread(
                    vec![
                        image("phone", "/phone.png"),
                        answer("Now generating a desktop version", "commentary"),
                        image("desktop", "/desktop.png"),
                        answer(
                            "Phone:\n\n![phone](/phone.png)\n\nDesktop:\n\n![desktop](/desktop.png)",
                            "final",
                        ),
                    ],
                    status,
                ),
                &None,
            );
            assert!(
                projected.turns[0]
                    .items()
                    .all(|item| item.data.kind != "imageGeneration")
            );
            assert_eq!(
                response_ids(&projected).len(),
                if status == models::TurnStatus::Running {
                    2
                } else {
                    1
                }
            );
        }
    }

    #[test]
    fn inline_data_replaces_its_preview_but_preserves_generation_errors() {
        for status in [models::ItemStatus::Completed, models::ItemStatus::Failed] {
            let generated = Arc::new(models::Item::new(
                "image".into(),
                status,
                models::ItemBody::ImageGeneration {
                    saved_path: None,
                    data: Some("png-data".into()),
                    revised_prompt: None,
                },
            ));
            let projected = project_conversation(
                &Snapshot::default(),
                thread(
                    vec![
                        generated,
                        answer("![image](data:image/png;base64,png-data)", "final"),
                    ],
                    models::TurnStatus::Completed,
                ),
                &None,
            );
            assert_eq!(
                response_ids(&projected),
                if status == models::ItemStatus::Failed {
                    vec!["image", "answer"]
                } else {
                    vec!["answer"]
                }
            );
        }
    }

    #[test]
    fn streaming_response_takes_over_the_preview_and_reopening_keeps_it_unique() {
        let generated = image("phone", "/phone.png");
        let mut previous = None;
        for (text, standalone, status) in [
            ("", true, models::TurnStatus::Running),
            ("![phone](/phone.png", true, models::TurnStatus::Running),
            ("![phone](/phone.png)", false, models::TurnStatus::Running),
            (
                "Done\n\n![phone](/phone.png)",
                false,
                models::TurnStatus::Completed,
            ),
            (
                "The image is available above",
                true,
                models::TurnStatus::Completed,
            ),
        ] {
            let source = thread(vec![generated.clone(), answer(text, "final")], status);
            let projected = project_conversation(&Snapshot::default(), source.clone(), &previous);
            let reopened = project_conversation(&Snapshot::default(), source, &None);
            let expected = if standalone {
                vec!["phone", "answer"]
            } else {
                vec!["answer"]
            };
            assert_eq!(response_ids(&projected), expected);
            assert_eq!(response_ids(&reopened), expected);
            assert_eq!(
                projected.turns[0]
                    .items()
                    .last()
                    .unwrap()
                    .data
                    .body
                    .as_deref(),
                Some(text)
            );
            previous = Some(projected);
        }
    }
}
