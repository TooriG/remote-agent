//! Resolve the client's appearance before usvg (which ignores CSS media rules).
use cssparser::{Delimiter, ParseError, Parser, ParserInput, Token};

pub(super) fn media_matches(query: &str, dark: bool) -> bool {
    query.split(',').any(|query| {
        let compact: String = query
            .chars()
            .filter(|c| !c.is_whitespace())
            .flat_map(char::to_lowercase)
            .collect();
        let (query, inverted) = compact
            .strip_prefix("not")
            .map_or((compact.as_str(), false), |query| (query, true));
        let query = query
            .strip_prefix("screenand")
            .or_else(|| query.strip_prefix("alland"))
            .unwrap_or(query);
        let matches = match query {
            "" | "screen" | "all" => Some(true),
            "print" => Some(false),
            "(prefers-color-scheme:dark)" => Some(dark),
            "(prefers-color-scheme:light)" => Some(!dark),
            _ => None,
        };
        matches.is_some_and(|matches| matches != inverted)
    })
}

pub(super) fn svg(source: &str, dark: bool) -> Option<String> {
    let document = roxmltree::Document::parse(source).ok()?;
    let styles: Vec<_> = document
        .descendants()
        .filter(|node| {
            node.has_tag_name("style") && node.children().all(|child| !child.is_element())
        })
        .map(|node| {
            let text: String = node
                .children()
                .filter(|child| child.is_text())
                .filter_map(|child| child.text())
                .collect();
            let css = if node
                .attribute("media")
                .is_none_or(|media| media_matches(media, dark))
            {
                css(&text, dark, 16)
            } else {
                String::new()
            };
            let css = css
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            (node.range(), format!("<style>{css}</style>"))
        })
        .collect();
    let mut result = source.to_owned();
    for (range, css) in styles.into_iter().rev() {
        result.replace_range(range, &css);
    }
    let root = document.root_element();
    if root.attribute("color").is_none() {
        let start = root.range().start + 1;
        let end = source[start..].find(|c: char| c.is_whitespace() || matches!(c, '/' | '>'))?;
        result.insert_str(
            start + end,
            if dark {
                " color=\"white\""
            } else {
                " color=\"black\""
            },
        );
    }
    Some(result)
}

fn text(parser: &mut Parser<'_, '_>) -> String {
    let start = parser.position();
    while parser.next_including_whitespace_and_comments().is_ok() {}
    parser.slice_from(start).to_owned()
}

fn css(source: &str, dark: bool, depth: usize) -> String {
    if depth == 0 {
        return String::new();
    }
    let mut input = ParserInput::new(source);
    let mut parser = Parser::new(&mut input);
    let mut result = String::new();
    loop {
        let start = parser.position();
        let Ok(token) = parser.next_including_whitespace_and_comments().cloned() else {
            break;
        };
        if matches!(&token, Token::AtKeyword(name) if name.eq_ignore_ascii_case("media")) {
            let Ok(query) = parser.parse_until_before(Delimiter::CurlyBracketBlock, |parser| {
                Ok::<_, ParseError<'_, ()>>(text(parser))
            }) else {
                return source.to_owned();
            };
            if parser.expect_curly_bracket_block().is_err() {
                return source.to_owned();
            }
            let Ok(body) =
                parser.parse_nested_block(|parser| Ok::<_, ParseError<'_, ()>>(text(parser)))
            else {
                return source.to_owned();
            };
            if media_matches(&query, dark) {
                result.push_str(&css(&body, dark, depth - 1));
            }
        } else {
            if matches!(
                token,
                Token::CurlyBracketBlock
                    | Token::SquareBracketBlock
                    | Token::ParenthesisBlock
                    | Token::Function(_)
            ) {
                let _ =
                    parser.parse_nested_block(|parser| Ok::<_, ParseError<'_, ()>>(text(parser)));
            }
            result.push_str(parser.slice_from(start));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_scheme_media_respects_not_screen_lists_and_whitespace() {
        for (query, light, dark) in [
            ("screen and (prefers-color-scheme: dark)", false, true),
            (" NOT ( prefers-color-scheme : light ) ", false, true),
            ("print, (prefers-color-scheme: light)", true, false),
            ("all", true, true),
            ("print", false, false),
            ("not (unknown: feature)", false, false),
        ] {
            assert_eq!(media_matches(query, false), light, "{query}");
            assert_eq!(media_matches(query, true), dark, "{query}");
        }
    }

    #[test]
    fn media_rules_preserve_quoted_braces_and_resolve_nested_blocks() {
        let source = r#".a { content: "}"; fill: red; } @media (prefers-color-scheme: dark) { @media screen { .a { fill: white; } } }"#;
        assert_eq!(
            css(source, false, 16).trim(),
            r#".a { content: "}"; fill: red; }"#
        );
        let dark = css(source, true, 16);
        assert!(dark.contains("fill: white") && !dark.contains("@media"));
        assert!(dark.contains(r#"content: "}""#));
    }
}
