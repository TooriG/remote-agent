//! The UI copy in the language the application selected. The application sets it
//! before publishing a change, and the root is made again for each language, so
//! views read it once when they are made rather than tracking it.
use agent_core::localization::{UiLanguage, localized_ui_format, localized_ui_text};
use std::sync::atomic::{AtomicBool, Ordering};

static ENGLISH: AtomicBool = AtomicBool::new(false);

pub(crate) fn set(language: UiLanguage) {
    ENGLISH.store(language == UiLanguage::English, Ordering::Relaxed);
}

pub(crate) fn language() -> UiLanguage {
    if ENGLISH.load(Ordering::Relaxed) {
        UiLanguage::English
    } else {
        UiLanguage::Japanese
    }
}

/// The cataloged copy for `source`.
pub(crate) fn tr(source: &'static str) -> &'static str {
    language().text(source)
}

/// A cataloged template with its `{0}` slots filled verbatim.
pub(crate) fn tr_format<const N: usize>(
    template: &'static str,
    arguments: [&dyn std::fmt::Display; N],
) -> String {
    localized_ui_format(
        language(),
        template.into(),
        arguments.iter().map(ToString::to_string).collect(),
    )
}

/// Copy Core composed (labels, statuses, summaries and errors).
pub(crate) fn localized(source: String) -> String {
    localized_ui_text(language(), source)
}

#[cfg(test)]
mod tests {
    use agent_core::localization::UiLanguage;
    use std::{collections::BTreeMap, path::Path};

    /// Every literal a view passes to `tr` or `tr_format`.
    fn sources(directory: &Path, found: &mut BTreeMap<String, String>) {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                sources(&path, found);
                continue;
            }
            let code = std::fs::read_to_string(&path).unwrap();
            for call in ["tr(\"", "tr_format(\""] {
                for (offset, _) in code.match_indices(call) {
                    let preceded = code[..offset]
                        .chars()
                        .next_back()
                        .is_some_and(|c| c.is_alphanumeric() || c == '_');
                    let rest = &code[offset + call.len()..];
                    if preceded || !rest.contains('"') {
                        continue;
                    }
                    let literal = &rest[..rest.find('"').unwrap()];
                    found.insert(literal.replace("\\n", "\n"), path.display().to_string());
                }
            }
        }
    }

    #[test]
    fn every_translated_literal_has_english_copy() {
        let mut found = BTreeMap::new();
        sources(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut found,
        );
        assert!(found.len() > 100, "the scan finds the views' copy");
        let missing = found
            .iter()
            .filter(|(source, _)| UiLanguage::English.text(source) == source.as_str())
            .map(|(source, path)| format!("{path}: {source}"))
            .collect::<Vec<_>>();
        assert!(missing.is_empty(), "{missing:#?}");
    }
}
