//! NSH's native-component boundary. Standard OS controls own their drawing,
//! intrinsic sizes, accessibility, menus, search and input behavior.
use nsh_core::{element::NativeView, prelude::*};
use serde_json::{Value, json};

#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct RowIndicators {
    pub activity: Option<agent_core::presentation::thread_status::ThreadActivity>,
    pub worktree: Option<agent_protocol::models::WorktreeStatus>,
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
impl RowIndicators {
    fn label(&self) -> String {
        use agent_core::presentation::thread_status::{
            thread_activity_label, worktree_status_label,
        };
        self.activity
            .map(thread_activity_label)
            .into_iter()
            .chain(self.worktree.map(worktree_status_label))
            .collect::<Vec<_>>()
            .join("、")
    }
}

#[derive(PartialEq, serde::Deserialize)]
#[cfg(target_os = "macos")]
pub(super) struct ListCompose {
    pub id: String,
    pub enabled: bool,
}

#[derive(PartialEq, serde::Deserialize)]
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(super) struct ListRow {
    pub id: String,
    pub title: String,
    pub symbol: String,
    pub asset: Option<String>,
    pub png: Option<Vec<u8>>,
    pub level: u32,
    pub disclosure: bool,
    pub menu: Option<Value>,
    pub indicators: Option<RowIndicators>,
    #[cfg(target_os = "macos")]
    pub group: bool,
    #[cfg(target_os = "macos")]
    pub expanded: bool,
    #[cfg(target_os = "macos")]
    pub compose: Option<ListCompose>,
}

pub fn items(choices: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>) -> Value {
    json!(
        choices
            .into_iter()
            .map(|(id, title)| json!({"id": id.into(), "title": title.into()}))
            .collect::<Vec<_>>()
    )
}

pub fn choices(titles: &[&str]) -> Value {
    items(titles.iter().map(|title| (*title, *title)))
}

#[cfg(target_os = "macos")]
pub(crate) mod browser_macos;
#[cfg(target_os = "ios")]
mod document_picker;
#[cfg(target_os = "ios")]
pub mod ios;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "macos")]
pub(crate) mod split;

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod apple;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod hosted;
#[cfg(target_os = "macos")]
mod list_macos;
#[cfg(target_os = "ios")]
mod preview_ios;
#[cfg(target_os = "macos")]
mod preview_macos;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod terminal;

pub fn image_extension(mime: &str) -> Option<String> {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        use objc2_foundation::NSString;
        use objc2_uniform_type_identifiers::UTType;
        UTType::typeWithMIMEType(&NSString::from_str(mime))?
            .preferredFilenameExtension()
            .map(|value| value.to_string())
    }
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    {
        let _ = mime;
        None
    }
}

pub fn image_file(path: &std::path::Path) -> bool {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        use objc2_foundation::NSString;
        use objc2_uniform_type_identifiers::UTType;
        let Some(extension) = path.extension().and_then(std::ffi::OsStr::to_str) else {
            return false;
        };
        UTType::typeWithFilenameExtension(&NSString::from_str(extension))
            .zip(UTType::typeWithIdentifier(&NSString::from_str(
                "public.image",
            )))
            .is_some_and(|(file, image)| file.conformsToType(&image))
    }
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    {
        let _ = path;
        false
    }
}

pub fn button(symbol: &'static str, label: &'static str, press: impl Fn() + 'static) -> NativeView {
    native("bex.button")
        .prop("symbol", json!(symbol))
        .prop("label", json!(label))
        .on_native_event(move |event: &NativeEvent| {
            if &*event.name == "press" {
                press();
            }
        })
}

pub fn menu(
    symbol: &'static str,
    label: &'static str,
    items: impl IntoReactive<Value>,
    select: impl Fn(&str) + 'static,
) -> NativeView {
    native("bex.menu")
        .prop("symbol", json!(symbol))
        .prop("label", json!(label))
        .prop("items", items)
        .on_native_event(move |event: &NativeEvent| {
            if &*event.name == "select"
                && let Some(title) = event.data.as_str()
            {
                select(title);
            }
        })
}

pub fn text_button(title: impl IntoReactive<Value>, press: impl Fn() + 'static) -> NativeView {
    native("bex.button")
        .prop("title", title)
        .on_native_event(move |event: &NativeEvent| {
            if &*event.name == "press" {
                press();
            }
        })
}

pub fn search(query: Signal<String>) -> NativeView {
    native("bex.search")
        .prop("value", move || json!(query.get()))
        .on_native_event(move |event: &NativeEvent| {
            if &*event.name == "change"
                && let Some(text) = event.data.as_str()
            {
                query.set(text.to_owned());
            }
        })
}

pub fn switch(
    label: impl IntoReactive<Value>,
    value: impl IntoReactive<Value>,
    change: impl Fn(bool) + 'static,
) -> NativeView {
    native("bex.switch")
        .prop("label", label)
        .prop("value", value)
        .on_native_event(move |event: &NativeEvent| {
            if &*event.name == "change"
                && let Some(value) = event.data.as_bool()
            {
                change(value);
            }
        })
}

pub fn toolbar(title: Memo<String>, action: impl Fn(&str) + 'static) -> NativeView {
    native("bex.toolbar")
        .prop("title", move || json!(title.get()))
        .on_native_event(move |event: &NativeEvent| {
            if &*event.name == "select"
                && let Some(id) = event.data.as_str()
            {
                action(id);
            }
        })
}

pub fn message(title: impl Into<String>, description: impl Into<String>) {
    let title = title.into();
    let description = description.into();
    spawn(async move {
        let _ = nsh_base::alert_dialog()
            .title(title)
            .description(description)
            .action("close", "閉じる")
            .show()
            .await;
    });
}

pub fn open_link(url: impl Into<String>) {
    let url = url.into();
    spawn(async move {
        if !open_url(url).await {
            message("リンクを開けませんでした", "開くアプリを確認してください。");
        }
    });
}

pub fn date_time(seconds: i64) -> String {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        use objc2_foundation::{NSDate, NSDateFormatter, NSDateFormatterStyle};
        NSDateFormatter::localizedStringFromDate_dateStyle_timeStyle(
            &NSDate::dateWithTimeIntervalSince1970(seconds as f64),
            NSDateFormatterStyle::MediumStyle,
            NSDateFormatterStyle::ShortStyle,
        )
        .to_string()
    }
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    seconds.to_string()
}
