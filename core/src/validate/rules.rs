//! Deterministic validation rules. Thresholds and phrase lists live in
//! `validator-rules.yaml` so they can be tuned without code changes.
//!
//! An item is REJECTED if any rule fails (checked in this order):
//!   empty field · wrong check/criterion · control/zero-width/bidi/Cyrillic chars
//!   quote too short/long · quote not in the excerpt sent for this check
//!   question not ending in '?' · more than one '?' · too short · too long
//!   banned (imperative / rewrite) phrase · grade/mark language
//!   quoted text (in "...") that is not the student's own words
//!   a run of paste-able prose (see prose.rs)

use regex::Regex;
use serde::Deserialize;

use super::{prose, CheckContext, FeedbackItem, RejectReason};
use crate::text::{contains_phrase, normalize_for_match, sha256_hex, word_count};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidatorRules {
    pub rules_version: u32,
    pub max_items_per_check: usize,
    pub min_quote_words: usize,
    pub max_quote_words: usize,
    pub min_question_words: usize,
    pub max_question_words: usize,
    pub max_question_chars: usize,
    pub prose_run_max_words: usize,
    pub max_unattributed_quoted_words: usize,
    pub banned_phrases: Vec<String>,
    pub grade_patterns: Vec<String>,
    pub leading_openers: Vec<String>,
    pub second_person_words: Vec<String>,
    pub wh_words: Vec<String>,
    pub auxiliary_openers: Vec<String>,
    #[serde(skip)]
    pub grade_regexes: Vec<Regex>,
    #[serde(skip)]
    pub sha256: String,
}

impl ValidatorRules {
    pub fn from_yaml(text: &str) -> Result<Self, String> {
        let mut rules: ValidatorRules =
            serde_yaml::from_str(text).map_err(|e| format!("validator-rules.yaml: {e}"))?;
        let lower = |v: &mut Vec<String>| {
            for s in v.iter_mut() {
                *s = s.trim().to_lowercase();
            }
            v.retain(|s| !s.is_empty());
        };
        lower(&mut rules.banned_phrases);
        lower(&mut rules.leading_openers);
        lower(&mut rules.second_person_words);
        lower(&mut rules.wh_words);
        lower(&mut rules.auxiliary_openers);
        rules.grade_regexes = rules
            .grade_patterns
            .iter()
            .map(|p| Regex::new(&format!("(?i){p}")).map_err(|e| format!("grade pattern {p:?}: {e}")))
            .collect::<Result<_, _>>()?;
        if rules.max_items_per_check == 0 || rules.prose_run_max_words == 0 {
            return Err("validator-rules.yaml: limits must be positive".into());
        }
        rules.sha256 = sha256_hex(text.as_bytes());
        Ok(rules)
    }

    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_yaml(&text)
    }
}

/// Zero-width, bidirectional-control and control characters can hide text or
/// defeat phrase matching; Cyrillic letters are the usual homoglyph trick
/// ("rеphrase" with a Cyrillic 'е'). Greek letters are allowed (physics symbols).
/// A quote may span a paragraph break, so line breaks are allowed there only.
fn has_suspicious_chars(s: &str, allow_line_breaks: bool) -> bool {
    s.chars().any(|c| {
        (c.is_control() && !(allow_line_breaks && c.is_whitespace()))
            || matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2069}' | '\u{FEFF}')
            || ('\u{0400}'..='\u{04FF}').contains(&c)
    })
}

/// Spans inside double quotes, or inside single quotes used as quotation marks
/// (not apostrophes).
fn quoted_spans(s: &str) -> Vec<String> {
    let mut spans: Vec<String> = s
        .split('"')
        .enumerate()
        .filter(|(i, _)| i % 2 == 1)
        .map(|(_, t)| t.to_string())
        .collect();
    static SINGLE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = SINGLE.get_or_init(|| Regex::new(r"(?:^|\s)'([^']{3,}?)'(?:[\s?.,;:!]|$)").unwrap());
    spans.extend(re.captures_iter(s).map(|c| c[1].to_string()));
    spans
}

pub fn check_item(
    item: &FeedbackItem,
    ctx: &CheckContext,
    excerpt_norm: &str,
    rules: &ValidatorRules,
) -> Result<(), RejectReason> {
    use RejectReason::*;

    if [&item.criterion, &item.check, &item.student_quote, &item.question]
        .iter()
        .any(|f| f.trim().is_empty())
    {
        return Err(EmptyField);
    }
    if item.check != ctx.check_id || item.criterion != ctx.criterion {
        return Err(CheckMismatch);
    }
    if has_suspicious_chars(&item.question, false) || has_suspicious_chars(&item.student_quote, true) {
        return Err(SuspiciousCharacters);
    }

    // --- the quote must be the student's own words, from the text we sent ---
    let quote = normalize_for_match(&item.student_quote);
    let quote_words = word_count(&quote);
    if quote_words < rules.min_quote_words {
        return Err(QuoteTooShort);
    }
    if quote_words > rules.max_quote_words {
        return Err(QuoteTooLong);
    }
    if !excerpt_norm.contains(&quote) {
        return Err(QuoteNotInDocument);
    }

    // --- the question must be one short question ---
    let q = normalize_for_match(&item.question);
    if !q.ends_with('?') {
        return Err(NotAQuestion);
    }
    if q.matches('?').count() > 1 {
        return Err(MultipleQuestions);
    }
    let q_words = word_count(&q);
    if q_words < rules.min_question_words {
        return Err(QuestionTooShort);
    }
    if q_words > rules.max_question_words || q.chars().count() > rules.max_question_chars {
        return Err(QuestionTooLong);
    }

    let q_lower = q.to_lowercase();
    if rules.banned_phrases.iter().any(|p| contains_phrase(&q_lower, p)) {
        return Err(BannedPhrase);
    }
    if rules.grade_regexes.iter().any(|re| re.is_match(&q)) {
        return Err(GradeLanguage);
    }

    // --- no "suggested wording" smuggled in quotation marks ---
    let excerpt_lower = excerpt_norm.to_lowercase();
    for span in quoted_spans(&q) {
        let span = span.trim().to_lowercase();
        if word_count(&span) > rules.max_unattributed_quoted_words && !excerpt_lower.contains(&span) {
            return Err(QuotedTextNotFromStudent);
        }
    }

    if prose::find_pasteable_run(&q, &excerpt_lower, rules).is_some() {
        return Err(PasteableProse);
    }
    Ok(())
}
