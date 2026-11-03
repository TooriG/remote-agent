//! UI copy only. Callers keep conversation text, file contents and provider
//! output out of this lookup. The same catalog is bundled by the iOS client.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::OnceLock};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(uniffi::Enum))]
pub enum UiLanguage {
    #[serde(rename = "en")]
    English,
    #[default]
    #[serde(rename = "ja")]
    Japanese,
}

#[derive(Deserialize)]
struct Catalog {
    strings: BTreeMap<String, Entry>,
    #[serde(skip)]
    templates: Vec<UiTemplate>,
}

struct UiTemplate {
    source: String,
    parts: Vec<String>,
}

impl UiTemplate {
    fn parse(source: &str) -> Option<Self> {
        let mut parts = Vec::new();
        let mut tail = source;
        while let Some((prefix, rest)) = tail.split_once('{') {
            let (slot, rest) = rest.split_once('}')?;
            if slot.parse::<usize>().ok()? != parts.len() {
                return None;
            }
            parts.push(prefix.to_owned());
            tail = rest;
        }
        if parts.is_empty() {
            return None;
        }
        parts.push(tail.to_owned());
        Some(Self {
            source: source.into(),
            parts,
        })
    }

    fn arguments(&self, value: &str) -> Option<Vec<String>> {
        let mut tail = value.strip_prefix(&self.parts[0])?;
        let mut arguments = Vec::new();
        for (index, suffix) in self.parts.iter().enumerate().skip(1) {
            if index == self.parts.len() - 1 {
                arguments.push(tail.strip_suffix(suffix)?.to_owned());
                return Some(arguments);
            }
            let (argument, rest) = tail.split_once(suffix.as_str())?;
            arguments.push(argument.to_owned());
            tail = rest;
        }
        None
    }
}
#[derive(Deserialize)]
struct Entry {
    localizations: BTreeMap<String, Translation>,
}
#[derive(Deserialize)]
struct Translation {
    #[serde(rename = "stringUnit")]
    unit: StringUnit,
}
#[derive(Deserialize)]
struct StringUnit {
    value: String,
}

fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let mut catalog: Catalog =
            serde_json::from_str(include_str!("localization/Localizable.xcstrings"))
                .expect("validated UI translation catalog");
        catalog.templates = catalog
            .strings
            .keys()
            .filter_map(|source| UiTemplate::parse(source))
            .collect();
        // Prefer the most specific copy when templates share a suffix.
        catalog.templates.sort_by_cached_key(|template| {
            std::cmp::Reverse(template.parts.iter().map(String::len).sum::<usize>())
        });
        catalog
    })
}

impl UiLanguage {
    pub fn code(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Japanese => "ja",
        }
    }

    pub fn text(self, source: &str) -> &str {
        catalog()
            .strings
            .get(source)
            .and_then(|entry| entry.localizations.get(self.code()))
            .map_or(source, |translation| translation.unit.value.as_str())
    }
}

#[cfg_attr(feature = "bindings", uniffi::export)]
pub fn localized_ui_text(language: UiLanguage, source: String) -> String {
    if language == UiLanguage::Japanese || catalog().strings.contains_key(&source) {
        return language.text(&source).to_owned();
    }
    // Core presentation values can arrive already formatted. Resolve only
    // cataloged UI templates; inserted paths, commands and names stay verbatim.
    for template in &catalog().templates {
        if let Some(arguments) = template.arguments(&source) {
            return localized_ui_format(language, template.source.clone(), arguments);
        }
    }
    source
}

/// Numbered slots let translations change word order. Inserted values are
/// copied verbatim and never interpreted as templates or translation keys.
#[cfg_attr(feature = "bindings", uniffi::export)]
pub fn localized_ui_format(language: UiLanguage, source: String, arguments: Vec<String>) -> String {
    let mut remaining = language.text(&source);
    let mut output = String::new();
    while let Some((prefix, tail)) = remaining.split_once('{') {
        output.push_str(prefix);
        let Some((slot, rest)) = tail.split_once('}') else {
            output.push('{');
            output.push_str(tail);
            return output;
        };
        match slot
            .parse::<usize>()
            .ok()
            .and_then(|index| arguments.get(index))
        {
            Some(value) => output.push_str(value),
            None => {
                output.push('{');
                output.push_str(slot);
                output.push('}');
            }
        }
        remaining = rest;
    }
    output.push_str(remaining);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switching_languages_changes_only_known_ui_copy() {
        assert_eq!(UiLanguage::English.text("設定"), "Settings");
        assert_eq!(UiLanguage::Japanese.text("設定"), "設定");
        assert_eq!(
            localized_ui_text(UiLanguage::English, "設定".into()),
            "Settings"
        );
        assert_eq!(
            localized_ui_text(UiLanguage::Japanese, "設定".into()),
            "設定"
        );
        assert_eq!(
            UiLanguage::English.text("ユーザーが書いた文章"),
            "ユーザーが書いた文章"
        );
        assert_eq!(UiLanguage::Japanese.text("README.md"), "README.md");
        assert_eq!(UiLanguage::English.text(""), "");
    }

    #[test]
    fn every_catalog_entry_has_both_languages() {
        for (source, entry) in &catalog().strings {
            for locale in ["en", "ja"] {
                assert!(
                    !entry.localizations[locale].unit.value.is_empty(),
                    "{locale}: {source}"
                );
            }
        }
    }

    #[test]
    fn formatting_reorders_copy_and_keeps_values_verbatim() {
        assert_eq!(
            localized_ui_format(UiLanguage::English, "残り {0}%".into(), vec!["42".into()]),
            "42% remaining"
        );
        assert_eq!(
            localized_ui_format(UiLanguage::Japanese, "残り {0}%".into(), vec!["42".into()]),
            "残り 42%"
        );
        assert_eq!(
            localized_ui_format(
                UiLanguage::English,
                "{1}: {0}".into(),
                vec!["{1}設定".into(), "file".into()]
            ),
            "file: {1}設定"
        );
        assert_eq!(
            localized_ui_format(UiLanguage::English, "{2} {x} {".into(), vec![]),
            "{2} {x} {"
        );
    }

    #[test]
    fn formatted_presentation_labels_preserve_inserted_values() {
        assert_eq!(
            localized_ui_text(UiLanguage::English, "残り 42%".into()),
            "42% remaining"
        );
        assert_eq!(
            localized_ui_text(UiLanguage::English, "フォルダ: 設定/{0}".into()),
            "Folder: 設定/{0}"
        );
        assert_eq!(
            localized_ui_text(UiLanguage::Japanese, "残り 42%".into()),
            "残り 42%"
        );
        assert_eq!(
            localized_ui_text(UiLanguage::English, "This is unchanged".into()),
            "This is unchanged"
        );
        assert_eq!(
            localized_ui_text(UiLanguage::English, "残り 42% extra".into()),
            "残り 42% extra"
        );
        assert_eq!(
            localized_ui_text(UiLanguage::English, "1時間 2分 3秒 作業しました".into()),
            "Worked for 1h 2m 3s · completed"
        );
    }

    #[test]
    fn language_preferences_round_trip_without_host_state() {
        for (language, value) in [
            (UiLanguage::English, "\"en\""),
            (UiLanguage::Japanese, "\"ja\""),
        ] {
            assert_eq!(serde_json::to_string(&language).unwrap(), value);
            assert_eq!(serde_json::from_str::<UiLanguage>(value).unwrap(), language);
        }
        assert!(serde_json::from_str::<UiLanguage>("\"unknown\"").is_err());
    }
}
