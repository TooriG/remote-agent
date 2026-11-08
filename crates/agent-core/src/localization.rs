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
        serde_json::from_str(include_str!("localization/Localizable.xcstrings"))
            .expect("validated UI translation catalog")
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
    language.text(&source).to_owned()
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
        for source in ["echo 5時間枠", "Fable · 週間枠"] {
            assert_eq!(
                localized_ui_text(UiLanguage::English, source.into()),
                source
            );
        }
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
            localized_ui_format(
                UiLanguage::English,
                "▤ {0}\nタップしてプレビュー".into(),
                vec!["設定/{0}".into()]
            ),
            "▤ 設定/{0}\nTap to preview"
        );
        assert_eq!(
            localized_ui_format(
                UiLanguage::English,
                "対応するブラウザーやアプリを確認してください。\n{0}".into(),
                vec!["scheme://設定/{0}".into()]
            ),
            "Check for a compatible browser or app.\nscheme://設定/{0}"
        );
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
    fn lookup_does_not_interpret_formatted_copy_or_multiline_data() {
        for source in [
            "残り 42%",
            "フォルダ: 設定/{0}",
            "設定\nconnection closed\n未登録",
            "Cookieを2件取り込みました",
            "This is unchanged",
        ] {
            for language in [UiLanguage::English, UiLanguage::Japanese] {
                assert_eq!(localized_ui_text(language, source.into()), source);
            }
        }
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
