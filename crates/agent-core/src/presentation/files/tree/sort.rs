//! Pierre's default segment ordering (ASCII numeric runs and UTF-16 strings).
//! Mobile uses Intl.Collator instead; its locale ordering is handled in the view.
use std::cmp::Ordering;

enum Token {
    Text(String),
    Number(f64),
}
pub(super) struct Key {
    original: String,
    lower: String,
    tokens: Vec<Token>,
}
impl Key {
    pub(super) fn new(value: &str) -> Self {
        let lower = value.to_lowercase();
        let mut tokens = Vec::new();
        let mut remaining = lower.as_str();
        while !remaining.is_empty() {
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
                Token::Text(part.into())
            });
            remaining = &remaining[end..];
        }
        Self {
            original: value.into(),
            lower,
            tokens,
        }
    }
    pub(super) fn compare(&self, other: &Self) -> Ordering {
        self.tokens
            .iter()
            .zip(&other.tokens)
            .map(|(left, right)| match (left, right) {
                (Token::Number(left), Token::Number(right)) => left.partial_cmp(right).unwrap(),
                (Token::Text(left), Token::Text(right)) => utf16_compare(left, right),
                (Token::Number(number), Token::Text(text)) => {
                    utf16_compare(&number_text(*number), text)
                }
                (Token::Text(text), Token::Number(number)) => {
                    utf16_compare(text, &number_text(*number))
                }
            })
            .find(|order| !order.is_eq())
            .unwrap_or_else(|| self.tokens.len().cmp(&other.tokens.len()))
            .then_with(|| utf16_compare(&self.lower, &other.lower))
            .then_with(|| utf16_compare(&self.original, &other.original))
    }
}
fn utf16_compare(left: &str, right: &str) -> Ordering {
    left.encode_utf16().cmp(right.encode_utf16())
}
fn number_text(number: f64) -> String {
    // Only compared to a non-digit run, so finite-number formatting after the
    // leading digit cannot affect order. JS accumulation can overflow to Infinity.
    if number.is_infinite() {
        "Infinity".into()
    } else {
        number.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
