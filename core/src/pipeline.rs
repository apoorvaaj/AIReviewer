//! Wires the stages together. The desktop shell calls these in order:
//!
//!   load_document -> redact -> build_tasks -> payload::build_bundle
//!   -> (student reviews and presses Send) -> payload::confirm -> run
//!
//! Everything up to and including `build_bundle` is local and pure.
//! Only `run` (which needs a `ConfirmedPayload`) can reach the network.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use serde::Serialize;

use crate::kb::{KnowledgeBase, Scope};
use crate::parse::{self, FileKind, ParseError, ParsedDocument};
use crate::payload::ConfirmedPayload;
use crate::reasoning::prompts::sanitize_excerpt;
use crate::reasoning::{CheckTask, EngineError, EngineResponse, ReasoningEngine};
use crate::sections::Sections;
use crate::text::{normalize_for_match, sha256_hex, word_count};
use crate::validate::{self, second_pass, CheckContext, FeedbackItem, RejectReason, ValidatorRules};

/// Below this many words, a check's sections are treated as "not found" and
/// the whole document is sent for that check instead.
const MIN_SECTION_WORDS: usize = 15;

pub struct LoadedDocument {
    pub parsed: ParsedDocument,
    /// SHA-256 of the original file bytes (for the receipt).
    pub original_sha256: String,
}

pub fn load_document(bytes: &[u8], kind: FileKind) -> Result<LoadedDocument, ParseError> {
    let parsed = parse::parse_bytes(bytes, kind)?;
    Ok(LoadedDocument { parsed, original_sha256: sha256_hex(bytes) })
}

pub fn build_tasks(kb: &KnowledgeBase, sections: &Sections, criteria: &[String]) -> Vec<CheckTask> {
    let full = sections.full_text_for_payload();
    kb.checks_for(criteria)
        .into_iter()
        .map(|check| {
            let (excerpt, scope) = match check.scope {
                Scope::WholeDocument => (full.clone(), "whole document".to_string()),
                Scope::Sections => {
                    let t = sections.text_for(&check.applies_to_sections);
                    if word_count(&t) < MIN_SECTION_WORDS {
                        (full.clone(), "whole document (matching sections not found)".to_string())
                    } else {
                        (t, format!("sections: {}", check.applies_to_sections.join(", ")))
                    }
                }
            };
            CheckTask {
                check: check.clone(),
                criterion_name: kb.criterion_name(&check.criterion),
                excerpt: sanitize_excerpt(&excerpt),
                scope,
            }
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct ShownItem {
    pub criterion: String,
    pub criterion_name: String,
    pub check: String,
    pub check_description: String,
    pub student_quote: String,
    pub question: String,
}

/// The run log. Lives in memory only; its counts go into the receipt.
#[derive(Debug, Clone, Serialize, Default)]
pub struct RunLog {
    pub check_requests_sent: usize,
    pub second_pass_requests_sent: usize,
    pub failed_calls: usize,
    pub items_generated: usize,
    pub items_shown: usize,
    pub items_rejected: usize,
    pub rejected_by_rule: BTreeMap<String, usize>,
    pub checks_using_whole_document_fallback: usize,
}

impl RunLog {
    fn reject(&mut self, reasons: &[RejectReason]) {
        for r in reasons {
            // A malformed response is counted, but it is not an "item".
            if *r != RejectReason::MalformedOutput {
                self.items_rejected += 1;
            }
            *self.rejected_by_rule.entry(r.as_str().to_string()).or_insert(0) += 1;
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RunOutcome {
    pub items: Vec<ShownItem>,
    pub log: RunLog,
    pub models_responded: BTreeSet<String>,
    /// Short, content-free error summaries (e.g. "3 checks failed: network error").
    pub errors: Vec<String>,
}

pub fn run(
    confirmed: &ConfirmedPayload,
    engine: &dyn ReasoningEngine,
    kb: &KnowledgeBase,
    rules: &ValidatorRules,
    concurrency: usize,
    progress: &(dyn Fn(usize, usize) + Sync),
) -> RunOutcome {
    let requests = &confirmed.bundle().requests;
    let total = requests.len();
    let mut log = RunLog {
        checks_using_whole_document_fallback: requests.iter().filter(|r| r.scope.contains("not found")).count(),
        ..Default::default()
    };

    // ---- 1. per-check calls (bounded concurrency) ----
    let results: Mutex<Vec<Option<Result<EngineResponse, EngineError>>>> = Mutex::new(vec![None; total]);
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    std::thread::scope(|s| {
        for _ in 0..concurrency.clamp(1, 16).min(total.max(1)) {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                if i >= total {
                    break;
                }
                let r = engine.run_check(confirmed, i);
                results.lock().unwrap()[i] = Some(r);
                progress(done.fetch_add(1, Ordering::SeqCst) + 1, total);
            });
        }
    });
    let results = results.into_inner().unwrap();

    // ---- 2. validate each response ----
    let mut accepted: Vec<FeedbackItem> = Vec::new();
    let mut models = BTreeSet::new();
    let mut error_kinds: BTreeMap<String, usize> = BTreeMap::new();
    for (req, result) in requests.iter().zip(results) {
        log.check_requests_sent += 1;
        match result {
            Some(Ok(resp)) => {
                models.insert(resp.model.clone());
                let ctx = CheckContext { check_id: &req.check_id, criterion: &req.criterion, excerpt: &req.excerpt };
                let v = validate::validate_check_output(&resp.text, &ctx, rules);
                log.items_generated += v.generated;
                log.reject(&v.rejected);
                accepted.extend(v.accepted);
            }
            Some(Err(e)) => {
                log.failed_calls += 1;
                *error_kinds.entry(error_kind(&e)).or_insert(0) += 1;
            }
            None => log.failed_calls += 1,
        }
    }
    let (accepted, dupes) = validate::dedupe(accepted);
    log.reject(&dupes);

    // ---- 3. second pass (fail-closed) ----
    let mut shown: Vec<FeedbackItem> = Vec::new();
    if !accepted.is_empty() {
        // Defence in depth: only items whose quote is inside confirmed text.
        let confirmed_text: Vec<String> = requests.iter().map(|r| normalize_for_match(&r.excerpt)).collect();
        let (eligible, ineligible): (Vec<_>, Vec<_>) = accepted.into_iter().partition(|it| {
            let q = normalize_for_match(&it.student_quote);
            confirmed_text.iter().any(|t| t.contains(&q))
        });
        log.reject(&vec![RejectReason::QuoteNotInDocument; ineligible.len()]);

        if !eligible.is_empty() {
            log.second_pass_requests_sent += 1;
            let (kept, rejected) = match engine.run_second_pass(confirmed, &eligible) {
                Ok((verdicts, model)) => {
                    models.insert(model);
                    second_pass::apply(eligible, Some(&verdicts))
                }
                Err(e) => {
                    log.failed_calls += 1;
                    *error_kinds.entry(format!("second pass: {}", error_kind(&e))).or_insert(0) += 1;
                    second_pass::apply(eligible, None)
                }
            };
            log.reject(&rejected);
            shown = kept;
        }
    }

    shown.sort_by_key(|it| kb.sort_key(&it.criterion, &it.check));
    log.items_shown = shown.len();
    let items = shown
        .into_iter()
        .map(|it| ShownItem {
            criterion_name: kb.criterion_name(&it.criterion),
            check_description: kb.check(&it.check).map(|c| c.description.trim().to_string()).unwrap_or_default(),
            criterion: it.criterion,
            check: it.check,
            student_quote: it.student_quote,
            question: it.question,
        })
        .collect();

    let errors = error_kinds.into_iter().map(|(k, n)| format!("{n} call(s) failed: {k}")).collect();
    RunOutcome { items, log, models_responded: models, errors }
}

/// Error summaries contain no document content.
fn error_kind(e: &EngineError) -> String {
    match e {
        EngineError::NotConfigured => "no API key configured".into(),
        EngineError::Network(_) => "network error".into(),
        EngineError::Api { status, .. } => format!("API error (HTTP {status})"),
        EngineError::Refused => "model declined".into(),
        EngineError::Truncated => "response cut off".into(),
        EngineError::Malformed(_) => "unexpected response".into(),
    }
}
