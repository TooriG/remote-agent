use super::MarkdownRun;
use std::sync::LazyLock;
use syntect::{
    highlighting::{Color, HighlightIterator, HighlightState, Highlighter, ThemeSet},
    parsing::{ParseState, ScopeStack, SyntaxSet},
    util::LinesWithEndings,
};

static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
static THEMES: LazyLock<ThemeSet> = LazyLock::new(ThemeSet::load_defaults);

pub(super) fn fence_filename(metadata: Option<&str>) -> Option<String> {
    let value = metadata?.trim();
    let value = value
        .strip_prefix("title=")
        .or_else(|| value.strip_prefix("filename="))
        .unwrap_or(value);
    let value = if let Some(quote) = value.chars().next().filter(|c| matches!(c, '\'' | '"')) {
        value[1..].split(quote).next()?
    } else {
        value.split_whitespace().next()?
    };
    (value.contains('/') || value.contains('.') || value.contains('\\')).then(|| value.to_owned())
}

fn rgb(color: Color) -> u32 {
    (u32::from(color.r) << 16) | (u32::from(color.g) << 8) | u32::from(color.b)
}

pub(super) fn highlight(text: &str, language: Option<&str>) -> Vec<MarkdownRun> {
    let fallback = || {
        vec![MarkdownRun {
            text: text.into(),
            ..Default::default()
        }]
    };
    // Bound synchronous work before loading grammar data for large tool output.
    let Some(language) = language.filter(|_| text.len() <= 128 * 1024) else {
        return fallback();
    };
    let language = match language {
        "typescript" | "ts" | "tsx" | "jsx" => "js",
        "shell" | "bash" | "zsh" => "sh",
        "kotlin" => "java",
        other => other,
    };
    let Some(syntax) = SYNTAXES.find_syntax_by_token(language) else {
        return fallback();
    };
    let mut parser = ParseState::new(syntax);
    let dark = Highlighter::new(&THEMES.themes["base16-ocean.dark"]);
    let light = Highlighter::new(&THEMES.themes["InspiredGitHub"]);
    let mut dark_state = HighlightState::new(&dark, ScopeStack::new());
    let mut light_state = HighlightState::new(&light, ScopeStack::new());
    let mut runs = Vec::new();
    for line in LinesWithEndings::from(text) {
        let Ok(ops) = parser.parse_line(line, &SYNTAXES) else {
            return fallback();
        };
        let mut dark_runs = HighlightIterator::new(&mut dark_state, &ops, line, &dark);
        let mut light_runs = HighlightIterator::new(&mut light_state, &ops, line, &light);
        // Shared operations give both themes the same text boundaries. Advance
        // both through the final None to apply trailing scope changes as well.
        while let (Some((dark, text)), Some((light, _))) = (dark_runs.next(), light_runs.next()) {
            runs.push(MarkdownRun {
                text: text.into(),
                dark_color: Some(rgb(dark.foreground)),
                light_color: Some(rgb(light.foreground)),
                ..Default::default()
            });
        }
    }
    if runs.is_empty() { fallback() } else { runs }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_theme_runs_match_syntect(text: &str, language: &str) {
        let syntax = SYNTAXES.find_syntax_by_token(language).unwrap();
        let runs = highlight(text, Some(language));
        // The upstream per-theme API defines the same scopes and colors, with
        // independent parser state to catch mistakes at multiline boundaries.
        for (theme, dark) in [("base16-ocean.dark", true), ("InspiredGitHub", false)] {
            let mut reference = syntect::easy::HighlightLines::new(syntax, &THEMES.themes[theme]);
            let expected = LinesWithEndings::from(text)
                .flat_map(|line| reference.highlight_line(line, &SYNTAXES).unwrap())
                .map(|(style, text)| (text, Some(rgb(style.foreground))))
                .collect::<Vec<_>>();
            let actual = runs
                .iter()
                .map(|run| {
                    (
                        run.text.as_str(),
                        if dark {
                            run.dark_color
                        } else {
                            run.light_color
                        },
                    )
                })
                .collect::<Vec<_>>();
            assert_eq!(actual, expected, "{language} {theme}");
        }
    }

    #[test]
    fn both_themes_keep_multiline_and_embedded_scope_transitions() {
        for (language, text) in [
            (
                "rs",
                "/* comment\n   nested /* comment */\n*/\nfn main() {\n  let s = r#\"日本語\nraw string\"#;\n}\n",
            ),
            (
                "js",
                "/* comment */\nconst x = `日本語 ${1 + 2}\nnext line`;\n// comment\nlet y = /a+/g;\n",
            ),
            (
                "html",
                "<style>\n.a { color: red; }\n</style>\n<script>\nconst x = `日本語`;\n</script>\n<p>text</p>\n",
            ),
            (
                "py",
                "s = \"\"\"日本語\ncontinued\"\"\"\n# comment\nprint(s)\r\n",
            ),
            ("sh", "cat <<EOF\n日本語\nEOF\necho \"next\"\n"),
        ] {
            assert_theme_runs_match_syntect(text, language);
        }
    }

    #[test]
    fn highlight_budget_preserves_the_inclusive_limit_and_plain_fallback() {
        for length in [128 * 1024, 128 * 1024 + 1] {
            let text = "x".repeat(length);
            let runs = highlight(&text, Some("rs"));
            assert_eq!(
                runs.iter().map(|run| run.text.as_str()).collect::<String>(),
                text
            );
            assert_eq!(
                runs.iter()
                    .all(|run| run.dark_color.is_some() && run.light_color.is_some()),
                length == 128 * 1024
            );
        }
        for (text, language) in [
            ("", Some("rs")),
            ("plain", None),
            ("plain", Some("unknown-grammar")),
        ] {
            let runs = highlight(text, language);
            assert_eq!(runs.len(), 1);
            assert_eq!(runs[0].text, text);
            assert_eq!((runs[0].dark_color, runs[0].light_color), (None, None));
        }
    }
    #[test]
    fn fenced_metadata_and_highlighted_text_survive() {
        assert_eq!(
            fence_filename(Some("title=\"src/hello world.rs\"")),
            Some("src/hello world.rs".into())
        );
        assert_eq!(
            fence_filename(Some("filename=main.swift")),
            Some("main.swift".into())
        );
        assert_eq!(fence_filename(Some("linenos")), None);
        let text = "fn main() {\n    println!(\"こんにちは\");\n}";
        let runs = highlight(text, Some("rust"));
        assert_eq!(
            runs.iter().map(|run| run.text.as_str()).collect::<String>(),
            text
        );
        assert!(
            runs.iter()
                .filter_map(|run| run.dark_color)
                .collect::<std::collections::HashSet<_>>()
                .len()
                > 1
        );
        let unknown = highlight(text, Some("unknown-grammar"));
        assert_eq!(unknown.len(), 1);
        assert_eq!(unknown[0].text, text);
        assert_eq!(unknown[0].dark_color, None);
    }
    proptest::proptest! {
        #[test]
        fn both_theme_colors_match_the_upstream_pipeline(
            language in proptest::sample::select(vec!["rs", "js", "html", "py", "sh", "java"]),
            lines in proptest::collection::vec("[^\\x00]{1,48}", 1..8),
        ) {
            assert_theme_runs_match_syntect(&lines.join("\n"), language);
        }

        #[test]
        fn highlighting_never_rewrites_unicode_code(text in "[^\\x00]{0,128}") {
            let runs = highlight(&text, Some("rs"));
            proptest::prop_assert_eq!(runs.iter().map(|run| run.text.as_str()).collect::<String>(), text);
        }
    }
}
