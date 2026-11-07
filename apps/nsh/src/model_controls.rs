use crate::{Connection, native};
use agent_core::{
    presentation::permissions::PERMISSION_CHOICES,
    state::{Intent, operations as op},
};
use nsh_core::prelude::*;
use serde_json::json;

pub(crate) fn controls() -> impl Element {
    let connection = use_context::<Connection>().unwrap();
    let publication = connection.publication;
    effect(move || {
        if let Some(request) = publication.with(|p| {
            p.snapshot
                .permission_control(&p.composer.controls.navigation.draft_key)
                .load_request
        }) {
            connection.dispatch(Intent::ReadPermissionSettings(request));
        }
    });
    let label = computed(move || {
        publication.with(|p| {
            p.snapshot
                .catalog
                .models
                .iter()
                .find(|model| p.composer.controls.selected_model.as_ref() == Some(&model.model))
                .map(|model| model.display_name.clone())
                .unwrap_or_else(|| "モデル".into())
        })
    });
    native::menu("slider.horizontal.3", "モデル", move || {
        publication.with(|p| {
            let controls = &p.composer.controls;
            let permissions = p.snapshot.permission_control(&controls.navigation.draft_key);
            let mut sections = vec![json!({"title": "モデル", "children": p.snapshot.catalog.models.iter().map(|model| json!({
                "id": format!("model:{}", model.id), "title": model.display_name, "selected": controls.selected_model.as_ref() == Some(&model.model)
            })).collect::<Vec<_>>()})];
            if !controls.quick.efforts.is_empty() {
                sections.push(json!({"title": "思考の深さ", "children": controls.quick.efforts.iter().map(|effort| json!({"id": format!("effort:{effort}"), "title": effort, "selected": *effort == controls.quick.effort})).collect::<Vec<_>>()}));
            }
            if !controls.quick.speed_choices.is_empty() {
                sections.push(json!({"title": "速度", "children": controls.quick.speed_choices.iter().map(|choice| json!({"id": format!("speed:{}", choice.id), "title": choice.label, "selected": choice.selected})).collect::<Vec<_>>()}));
            }
            if permissions.version.is_some() {
                sections.push(json!({"title": permissions.label, "children": PERMISSION_CHOICES.iter().enumerate().map(|(index, (mode, label, _))| json!({"id": format!("permission:{index}"), "title": label, "selected": permissions.mode == Some(*mode)})).collect::<Vec<_>>()}));
            }
            json!(sections)
        })
    }, move |id| {
        let Some((kind, value)) = id.split_once(':') else { return; };
        let thread_id = publication.with_untracked(|p| p.composer.controls.navigation.draft_key.clone());
        let intent = match kind {
            "model" => publication.with_untracked(|p| p.snapshot.catalog.models.iter().find(|model| model.id == value).map(|model| Intent::SelectModel { thread_id, model: model.model.clone() })),
            "effort" => Some(Intent::SelectEffort { thread_id, effort: value.into() }),
            "speed" => Some(Intent::SelectServiceTier { thread_id, service_tier: value.into() }),
            "permission" => publication.with_untracked(|p| {
                let control = p.snapshot.permission_control(&thread_id);
                let (mode, _, _) = value.parse::<usize>().ok().and_then(|index| PERMISSION_CHOICES.get(index))?;
                Some(Intent::UpdatePermissionSettings(op::UpdatePermissionSettings { provider: control.provider?, mode: *mode, version: control.version?.into() }))
            }),
            _ => None,
        };
        if let Some(intent) = intent { connection.dispatch(intent); }
    }).prop("title", move || json!(label.get())).prop("label", move || json!(label.get()))
        .prop("appearance", json!(if is_desktop() { "plain" } else { "automatic" }))
        .prop("enabled", move || json!(publication.with(|p| p.composer.controls.connected && !p.composer.controls.sending)))
        .min_h(px(44.0))
}
