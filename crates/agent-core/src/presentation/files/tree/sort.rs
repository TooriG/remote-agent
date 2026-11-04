//! Pierre's default segment ordering (ASCII numeric runs and UTF-16 strings).
//! Mobile uses Intl.Collator instead; its locale ordering is handled in the view.
use std::{cmp::Ordering, ops::Range};

enum Token {
    Text(Range<usize>),
    Number(f64),
}
pub(super) struct Key<'a> {
    original: &'a str,
    lower: String,
    tokens: Vec<Token>,
}
impl<'a> Key<'a> {
    pub(super) fn new(value: &'a str) -> Self {
        let lower = value.to_lowercase();
        let mut tokens = Vec::new();
        let mut start = 0;
        while start < lower.len() {
            let remaining = &lower[start..];
            let numeric = remaining.as_bytes()[0].is_ascii_digit();
            let end = remaining
                .char_indices()
                .find(|(_, ch)| ch.is_ascii_digit() != numeric)
                .map_or(remaining.len(), |(index, _)| index);
            let part = &remaining[..end];
            tokens.push(if numeric {
                Token::Number(
                    part.bytes()
                        .fold(0.0, |value, byte| value * 10.0 + f64::from(byte - b'0')),
                )
            } else {
                Token::Text(start..start + end)
            });
            start += end;
        }
        Self {
            original: value,
            lower,
            tokens,
        }
    }
    pub(super) fn compare(&self, other: &Key<'_>) -> Ordering {
        self.tokens
            .iter()
            .zip(&other.tokens)
            .map(|(left, right)| match (left, right) {
                (Token::Number(left), Token::Number(right)) => left.partial_cmp(right).unwrap(),
                (Token::Text(left), Token::Text(right)) => {
                    utf16_compare(&self.lower[left.clone()], &other.lower[right.clone()])
                }
                (Token::Number(number), Token::Text(text)) => {
                    utf16_compare(number_text(*number), &other.lower[text.clone()])
                }
                (Token::Text(text), Token::Number(number)) => {
                    utf16_compare(&self.lower[text.clone()], number_text(*number))
                }
            })
            .find(|order| !order.is_eq())
            .unwrap_or_else(|| self.tokens.len().cmp(&other.tokens.len()))
            .then_with(|| utf16_compare(&self.lower, &other.lower))
            .then_with(|| utf16_compare(self.original, other.original))
    }
}
fn utf16_compare(left: &str, right: &str) -> Ordering {
    left.encode_utf16().cmp(right.encode_utf16())
}
fn number_text(number: f64) -> &'static str {
    // Text runs start with a non-ASCII-digit, so every finite number's leading
    // ASCII digit has the same UTF-16 order relative to them. Keep Infinity's
    // full spelling when JS-style accumulation overflows.
    if number.is_infinite() {
        "Infinity"
    } else {
        "0"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    #[test]
    fn pierre_order_preserves_numeric_case_padding_and_utf16_ties() {
        let mut names = [
            "a10", "a2", "a02", "a", "A", "a-1", "a1", "a01", "🦀", "\u{e000}",
        ];
        names.sort_by(|left, right| Key::new(left).compare(&Key::new(right)));
        assert_eq!(
            names,
            [
                "A", "a", "a01", "a1", "a02", "a2", "a10", "a-1", "🦀", "\u{e000}"
            ]
        );
        assert!(Key::new(&"9".repeat(400)).compare(&Key::new("z")).is_lt());
    }

    #[test]
    fn unicode_runs_keep_numeric_order_and_utf16_case_and_padding_ties() {
        let mut names = ["é10字", "é2字", "É2字", "é02字", "é-1字", "é2あ"];
        names.sort_by(|a, b| Key::new(a).compare(&Key::new(b)));
        assert_eq!(names, ["é2あ", "é02字", "É2字", "é2字", "é10字", "é-1字"]);
    }

    #[test]
    fn numeric_and_text_prefixes_preserve_punctuation_and_overflow_order() {
        let overflow = "9".repeat(400);
        let mut names = ["a", "2", "@", "_", &overflow, "/", "0", "10"];
        names.sort_by(|a, b| Key::new(a).compare(&Key::new(b)));
        assert_eq!(names, ["/", "0", "2", "10", "@", &overflow, "_", "a"]);
    }

    proptest! {
        #[test]
        fn unicode_segments_preserve_numeric_value_order(
            prefix in "[^0-9]{0,16}", suffix in "[^0-9]{0,16}",
            left in any::<u32>(), right in any::<u32>(),
        ) {
            let a = format!("{prefix}{left}{suffix}");
            let b = format!("{prefix}{right}{suffix}");
            prop_assert_eq!(Key::new(&a).compare(&Key::new(&b)), left.cmp(&right));
        }

        #[test]
        fn numeric_names_compare_to_non_digit_names_by_utf16(
            number in any::<u64>(), text in "[^0-9]{1,16}",
        ) {
            let number = number.to_string();
            let expected = number.encode_utf16().cmp(text.to_lowercase().encode_utf16());
            prop_assert_eq!(Key::new(&number).compare(&Key::new(&text)), expected);
            prop_assert_eq!(Key::new(&text).compare(&Key::new(&number)), expected.reverse());
        }
    }
}
