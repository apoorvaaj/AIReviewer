//! Small text helpers shared by several modules.

use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

/// Normalise text for quote matching ONLY.
///
/// Models routinely change typography when copying text, so an exact byte
/// comparison drops genuine quotes. We relax exactly these character classes,
/// nothing else (no case folding, no punctuation removal):
///   * Unicode NFC composition
///   * any run of whitespace -> one space; leading/trailing whitespace removed
///   * curly single quotes / primes -> '      curly double quotes -> "
///   * hyphen/dash variants and the minus sign -> -
///   * ellipsis character -> ...      ligatures fi/fl -> fi/fl
///   * soft hyphen and zero-width characters -> removed
pub fn normalize_for_match(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pending_space = false;
    for ch in s.nfc() {
        if ch.is_whitespace() {
            pending_space = true;
            continue;
        }
        let replacement: Option<&str> = match ch {
            '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' | '\u{2032}' => Some("'"),
            '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' | '\u{2033}' => Some("\""),
            '\u{2010}'..='\u{2015}' | '\u{2212}' => Some("-"),
            '\u{2026}' => Some("..."),
            '\u{FB01}' => Some("fi"),
            '\u{FB02}' => Some("fl"),
            '\u{00AD}' | '\u{200B}'..='\u{200D}' | '\u{2060}' | '\u{FEFF}' => Some(""),
            _ => None,
        };
        if replacement == Some("") {
            continue;
        }
        if pending_space && !out.is_empty() {
            out.push(' ');
        }
        pending_space = false;
        match replacement {
            Some(r) => out.push_str(r),
            None => out.push(ch),
        }
    }
    out
}

/// Number of words that contain at least one letter or digit.
pub fn word_count(s: &str) -> usize {
    s.split_whitespace()
        .filter(|w| w.chars().any(|c| c.is_alphanumeric()))
        .count()
}

/// True if `phrase` occurs in `haystack` and is not glued to letters/digits on
/// either side. Both arguments should already be lower-case.
pub fn contains_phrase(haystack: &str, phrase: &str) -> bool {
    if phrase.is_empty() {
        return false;
    }
    let starts_alnum = phrase.chars().next().is_some_and(|c| c.is_alphanumeric());
    let ends_alnum = phrase.chars().next_back().is_some_and(|c| c.is_alphanumeric());
    for (pos, _) in haystack.match_indices(phrase) {
        let before = haystack[..pos].chars().next_back();
        let after = haystack[pos + phrase.len()..].chars().next();
        let left_ok = !starts_alnum || !before.is_some_and(|c| c.is_alphanumeric());
        let right_ok = !ends_alnum || !after.is_some_and(|c| c.is_alphanumeric());
        if left_ok && right_ok {
            return true;
        }
    }
    false
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

pub fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalisation_relaxes_only_typography() {
        assert_eq!(
            normalize_for_match("The  student\u{2019}s \u{201C}value\u{201D}\n\u{2013} 9.8\u{2026}"),
            "The student's \"value\" - 9.8..."
        );
        assert_eq!(normalize_for_match("Case Matters"), "Case Matters");
        assert_ne!(normalize_for_match("Case"), normalize_for_match("case"));
    }

    #[test]
    fn phrase_boundaries() {
        assert!(contains_phrase("please rephrase this?", "rephrase"));
        assert!(!contains_phrase("unrephrased", "rephrase"));
        assert!(contains_phrase("for example: x", "for example:"));
    }
}
