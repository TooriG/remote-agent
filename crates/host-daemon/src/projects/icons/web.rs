use super::{IconSource, appearance};
use std::{path::PathBuf, sync::LazyLock};

/// HTML links and route metadata keep their declared appearance and order.
pub(super) fn source(source: &str) -> Option<IconSource> {
    static LINKS: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"(?is)<link\b[^>]*>").unwrap());
    static REL: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r#"(?i)\brel\s*([=:])\s*["'](?:icon|shortcut icon|apple-touch-icon)["']"#)
            .unwrap()
    });
    static HREF: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r#"(?i)\bhref\s*[=:]\s*["']([^"'?#]+)"#).unwrap());
    static MEDIA: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r#"(?i)\bmedia\s*[=:]\s*["']([^"']+)["']"#).unwrap());
    let entries: Vec<_> = LINKS
        .find_iter(source)
        .map(|tag| tag.as_str())
        .chain(source.split(['{', '}']).filter(|entry| {
            REL.captures(entry)
                .is_some_and(|capture| &capture[1] == ":")
        }))
        .filter(|entry| REL.is_match(entry))
        .filter_map(|entry| {
            let href = HREF.captures(entry)?[1].to_owned();
            if href.starts_with("//") || href.contains(':') {
                return None;
            }
            let media = MEDIA.captures(entry).map(|capture| capture[1].to_owned());
            Some((href, media))
        })
        .collect();
    let paths = |dark| {
        let mut entries: Vec<_> = entries
            .iter()
            .filter(|(_, media)| {
                media
                    .as_ref()
                    .is_none_or(|media| appearance::media_matches(media, dark))
            })
            .collect();
        entries.sort_by_key(|(_, media)| media.is_none());
        entries
            .into_iter()
            .flat_map(|(href, _)| {
                let href = href.trim_start_matches('/');
                [PathBuf::from(format!("public/{href}")), PathBuf::from(href)]
            })
            .collect()
    };
    let source = IconSource {
        light: paths(false),
        dark: paths(true),
    };
    (!source.light.is_empty() || !source.dark.is_empty()).then_some(source)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unrelated_tags_and_metadata_objects_never_supply_an_icon() {
        assert!(source(r#"<link rel="icon"><link rel="stylesheet" href="/style.css">"#).is_none());
        assert!(source(r#"[{rel: 'icon'}, {href: '/unrelated.png'}]"#).is_none());
        assert!(source(r#"<link rel="icon" href="https://example.com/icon.svg">"#).is_none());
    }

    #[test]
    fn media_specific_links_precede_a_neutral_link_in_each_appearance() {
        let icons = source(r#"<link rel="icon" href="/neutral.png"><link rel="icon" href="/dark.svg" media="(prefers-color-scheme: dark)"><link rel="icon" href="/light.svg" media="(prefers-color-scheme: light)">"#).unwrap();
        assert_eq!(
            icons.light,
            [
                "public/light.svg",
                "light.svg",
                "public/neutral.png",
                "neutral.png"
            ]
            .map(PathBuf::from)
        );
        assert_eq!(
            icons.dark,
            [
                "public/dark.svg",
                "dark.svg",
                "public/neutral.png",
                "neutral.png"
            ]
            .map(PathBuf::from)
        );
    }

    proptest::proptest! {
        #[test]
        fn icon_declarations_accept_attribute_order_and_whitespace(name in "[a-z]{1,20}", whitespace in "[ \\t]{0,5}", href_first in proptest::bool::ANY) {
            let href = format!("href{whitespace}={whitespace}\"/{name}.png?revision=1\"");
            let rel = format!("rel{whitespace}={whitespace}\"icon\"");
            let html = if href_first { format!("<link {href} {rel}>") } else { format!("<link {rel} {href}>") };
            let icons = source(&html).unwrap();
            let expected = [PathBuf::from(format!("public/{name}.png")), PathBuf::from(format!("{name}.png"))];
            proptest::prop_assert_eq!(icons.light, expected.clone());
            proptest::prop_assert_eq!(icons.dark, expected);
        }
    }
}
