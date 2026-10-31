use agent_core::localization::UiLanguage;
use gpui_kit::{App, Global};

pub(crate) struct Language(pub(crate) UiLanguage);
impl Global for Language {}

pub(crate) fn language(cx: &App) -> UiLanguage {
    cx.try_global::<Language>()
        .map_or(UiLanguage::default(), |value| value.0)
}

pub(crate) fn init(cx: &mut App) {
    let saved = crate::platform::state_dir()
        .ok()
        .and_then(|directory| std::fs::read(directory.join("language.json")).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    apply(saved, cx);
}

pub(crate) fn select(value: UiLanguage, cx: &mut App) -> Result<(), String> {
    let directory = crate::platform::state_dir()?;
    agent_local_host::save_private_json(&directory.join("language.json"), &value)
        .map_err(|error| error.to_string())?;
    apply(value, cx);
    Ok(())
}

fn apply(value: UiLanguage, cx: &mut App) {
    gpui_kit::component::set_locale(value.code());
    cx.set_global(Language(value));
    cx.refresh_windows();
}
