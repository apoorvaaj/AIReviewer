//! Output validation: the guarantee that the student only ever sees grounded
//! Socratic questions, never text they could paste into their work.
//!
//! Pipeline for every model response:
//!   1. Strict JSON parse into `CheckResponse` (unknown fields => reject all).
//!   2. Per item: `rules::check_item` (deterministic rules, see rules.rs).
//!   3. Across all items: duplicates removed.
//!   4. Second pass (a separate cheap model) asked "could this be pasted in as
//!      content?" — FAIL-CLOSED: any "yes", missing verdict or error => drop.
//! Rejected items are never shown; they are only counted by reason.

pub mod prose;
pub mod rules;
pub mod second_pass;

use serde::{Deserialize, Serialize};

pub use rules::ValidatorRules;

/// The ONLY shape of feedback the system can produce.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedbackItem {
    pub criterion: String,
    pub check: String,
    pub student_quote: String,
    pub question: String,
}

/// What the model must return for one check (enforced by JSON schema on the
/// API side AND re-checked here).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckResponse {
    pub weakness_present: bool,
    pub items: Vec<FeedbackItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectReason {
    MalformedOutput,
    InconsistentOutput,
    TooManyItems,
    CheckMismatch,
    EmptyField,
    SuspiciousCharacters,
    QuoteTooShort,
    QuoteTooLong,
    QuoteNotInDocument,
    NotAQuestion,
    MultipleQuestions,
    QuestionTooShort,
    QuestionTooLong,
    BannedPhrase,
    GradeLanguage,
    QuotedTextNotFromStudent,
    PasteableProse,
    Duplicate,
    SecondPassPasteable,
    SecondPassUnavailable,
}

impl RejectReason {
    pub fn as_str(self) -> &'static str {
        match self {
            RejectReason::MalformedOutput => "malformed_output",
            RejectReason::InconsistentOutput => "inconsistent_output",
            RejectReason::TooManyItems => "too_many_items",
            RejectReason::CheckMismatch => "check_mismatch",
            RejectReason::EmptyField => "empty_field",
            RejectReason::SuspiciousCharacters => "suspicious_characters",
            RejectReason::QuoteTooShort => "quote_too_short",
            RejectReason::QuoteTooLong => "quote_too_long",
            RejectReason::QuoteNotInDocument => "quote_not_in_document",
            RejectReason::NotAQuestion => "not_a_question",
            RejectReason::MultipleQuestions => "multiple_questions",
            RejectReason::QuestionTooShort => "question_too_short",
            RejectReason::QuestionTooLong => "question_too_long",
            RejectReason::BannedPhrase => "banned_phrase",
            RejectReason::GradeLanguage => "grade_language",
            RejectReason::QuotedTextNotFromStudent => "quoted_text_not_from_student",
            RejectReason::PasteableProse => "pasteable_prose",
            RejectReason::Duplicate => "duplicate",
            RejectReason::SecondPassPasteable => "second_pass_pasteable",
            RejectReason::SecondPassUnavailable => "second_pass_unavailable",
        }
    }
}

/// What a single check call was about: the item must match it exactly.
pub struct CheckContext<'a> {
    pub check_id: &'a str,
    pub criterion: &'a str,
    /// The exact excerpt that was sent for this check. Quotes must come from it.
    pub excerpt: &'a str,
}

#[derive(Debug, Default)]
pub struct CheckValidation {
    pub accepted: Vec<FeedbackItem>,
    pub rejected: Vec<RejectReason>,
    /// Number of items the model produced (0 if the output was malformed).
    pub generated: usize,
}

/// Validate the raw text the model returned for one check.
pub fn validate_check_output(raw: &str, ctx: &CheckContext, rules: &ValidatorRules) -> CheckValidation {
    let mut result = CheckValidation::default();
    let parsed: CheckResponse = match serde_json::from_str(raw.trim()) {
        Ok(p) => p,
        Err(_) => {
            result.rejected.push(RejectReason::MalformedOutput);
            return result;
        }
    };
    result.generated = parsed.items.len();
    if !parsed.weakness_present {
        // "No weakness" must mean "no items". Anything else is inconsistent.
        result.rejected.extend(parsed.items.iter().map(|_| RejectReason::InconsistentOutput));
        return result;
    }

    let excerpt_norm = crate::text::normalize_for_match(ctx.excerpt);
    for (i, item) in parsed.items.into_iter().enumerate() {
        if i >= rules.max_items_per_check {
            result.rejected.push(RejectReason::TooManyItems);
            continue;
        }
        match rules::check_item(&item, ctx, &excerpt_norm, rules) {
            Ok(()) => result.accepted.push(item),
            Err(reason) => result.rejected.push(reason),
        }
    }
    result
}

/// Remove items whose question duplicates an earlier one (case/space-insensitive).
pub fn dedupe(items: Vec<FeedbackItem>) -> (Vec<FeedbackItem>, Vec<RejectReason>) {
    let mut seen = std::collections::HashSet::new();
    let mut kept = Vec::new();
    let mut rejected = Vec::new();
    for item in items {
        let key = crate::text::normalize_for_match(&item.question).to_lowercase();
        if seen.insert(key) {
            kept.push(item);
        } else {
            rejected.push(RejectReason::Duplicate);
        }
    }
    (kept, rejected)
}
