//! Second-pass check by a separate, cheap model: "Could any part of this item
//! be pasted into the essay as content?"
//!
//! FAIL-CLOSED. An item is shown only if the second pass returned a verdict
//! for exactly that item and the verdict is `pasteable: false`. A missing
//! verdict, a malformed response, a refusal or a network error drops the item.

use serde::Deserialize;

use super::{FeedbackItem, RejectReason};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecondPassResponse {
    pub verdicts: Vec<Verdict>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Verdict {
    pub index: usize,
    pub pasteable: bool,
}

pub fn parse_verdicts(raw: &str) -> Result<Vec<Verdict>, String> {
    serde_json::from_str::<SecondPassResponse>(raw.trim())
        .map(|r| r.verdicts)
        .map_err(|e| format!("malformed second-pass output: {e}"))
}

/// `verdicts = None` means the second pass failed entirely.
pub fn apply(items: Vec<FeedbackItem>, verdicts: Option<&[Verdict]>) -> (Vec<FeedbackItem>, Vec<RejectReason>) {
    let mut kept = Vec::new();
    let mut rejected = Vec::new();
    for (i, item) in items.into_iter().enumerate() {
        let decision: Vec<bool> = verdicts
            .unwrap_or(&[])
            .iter()
            .filter(|v| v.index == i)
            .map(|v| v.pasteable)
            .collect();
        if decision.is_empty() {
            rejected.push(RejectReason::SecondPassUnavailable);
        } else if decision.iter().any(|p| *p) {
            // conflicting duplicate verdicts count as "pasteable"
            rejected.push(RejectReason::SecondPassPasteable);
        } else {
            kept.push(item);
        }
    }
    (kept, rejected)
}
