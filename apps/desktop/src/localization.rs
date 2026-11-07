use agent_core::localization::UiLanguage;
use gpui_kit::{App, Global};

pub(crate) struct Language(pub(crate) UiLanguage);
impl Global for Language {}

pub(crate) fn language(cx: &App) -> UiLanguage {
    cx.try_global::<Language>()
        .map_or(UiLanguage::default(), |value| value.0)
}

pub(crate) fn init(cx: &mut App) {
    let language = cx
        .global::<crate::Runtime>()
        .connections
        .preferences
        .language();
    apply(language, cx);
}

pub(crate) fn select(value: UiLanguage, cx: &mut App) -> Result<(), String> {
    cx.global::<crate::Runtime>()
        .connections
        .preferences
        .select_language(value)
        .map_err(|error| error.to_string())?;
    apply(value, cx);
    Ok(())
}

fn apply(value: UiLanguage, cx: &mut App) {
    if cx
        .try_global::<Language>()
        .is_some_and(|current| current.0 == value)
    {
        return;
    }
    gpui_kit::component::set_locale(value.code());
    cx.set_global(Language(value));
    cx.refresh_windows();
}
