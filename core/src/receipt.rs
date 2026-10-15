//! Session receipt (JSON + readable HTML).
//!
//! The `Receipt` struct has NO field that can hold document text, file names,
//! identifiers or feedback questions: only versions, hashes, model names,
//! criterion ids and counts. Adding such a field would be a visible change in
//! code review. tests/receipt.rs also checks that no run of document words
//! appears in either output.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::kb::KnowledgeBase;
use crate::parse::FileKind;
use crate::payload::RequestBundle;
use crate::pipeline::RunOutcome;
use crate::text::html_escape as esc;
use crate::validate::ValidatorRules;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub receipt_format: u32,
    pub app_name: String,
    pub app_version: String,
    pub created_utc: String,
    pub engine: String,
    pub destination: String,
    pub models_configured: Vec<String>,
    pub models_responded: Vec<String>,
    pub knowledge_base_id: String,
    pub knowledge_base_version: String,
    pub knowledge_base_syllabus_year: u32,
    pub knowledge_base_reviewed_by_teachers: bool,
    pub knowledge_base_sha256: String,
    pub validator_rules_version: u32,
    pub validator_rules_sha256: String,
    pub original_document_type: String,
    pub original_document_sha256: String,
    pub redacted_payload_sha256: String,
    pub redactions_by_kind: BTreeMap<String, usize>,
    pub criteria_reviewed: Vec<String>,
    pub check_requests_sent: usize,
    pub second_pass_requests_sent: usize,
    pub failed_calls: usize,
    pub feedback_items_generated: usize,
    pub feedback_items_shown: usize,
    pub feedback_items_rejected: usize,
    pub rejected_by_rule: BTreeMap<String, usize>,
    pub acknowledgement_statement: String,
}

pub struct ReceiptInput<'a> {
    pub created_utc: chrono::DateTime<chrono::Utc>,
    pub kb: &'a KnowledgeBase,
    pub rules: &'a ValidatorRules,
    pub bundle: &'a RequestBundle,
    pub outcome: &'a RunOutcome,
    pub original_kind: FileKind,
    pub original_sha256: &'a str,
    pub redactions_by_kind: &'a BTreeMap<String, usize>,
    pub criteria: &'a [String],
}

pub fn build(input: ReceiptInput) -> Receipt {
    let ReceiptInput { created_utc, kb, rules, bundle, outcome, original_kind, original_sha256, redactions_by_kind, criteria } = input;
    let log = &outcome.log;
    let responded: Vec<String> = outcome.models_responded.iter().cloned().collect();
    let models_text = if responded.is_empty() { bundle.engine.primary_model.clone() } else { responded.join(" and ") };
    let criteria_names: Vec<String> = criteria.iter().map(|c| kb.criterion_name(c)).collect();

    let statement = format!(
        "On {date} I used {app} (version {ver}) to get feedback on a draft of my IB Physics Internal Assessment. \
The tool removed identifying details on my computer and then sent the redacted draft to Anthropic's {models} model(s). \
It returned {n} question(s) about the criteria: {crit}. The tool gives questions only and does not write or rewrite text. \
All wording in my submitted work is my own. Session receipt: {hash}.",
        date = created_utc.format("%d %B %Y"),
        app = crate::APP_NAME,
        ver = crate::APP_VERSION,
        models = models_text,
        n = log.items_shown,
        crit = criteria_names.join(", "),
        hash = &bundle.sha256[..16],
    );

    Receipt {
        receipt_format: 1,
        app_name: crate::APP_NAME.into(),
        app_version: crate::APP_VERSION.into(),
        created_utc: created_utc.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        engine: bundle.engine.engine.clone(),
        destination: bundle.engine.destination.clone(),
        models_configured: vec![bundle.engine.primary_model.clone(), bundle.engine.second_pass_model.clone()],
        models_responded: responded,
        knowledge_base_id: kb.manifest.kb_id.clone(),
        knowledge_base_version: kb.manifest.kb_version.clone(),
        knowledge_base_syllabus_year: kb.manifest.syllabus_year,
        knowledge_base_reviewed_by_teachers: !kb.has_unreviewed_checks(),
        knowledge_base_sha256: kb.sha256.clone(),
        validator_rules_version: rules.rules_version,
        validator_rules_sha256: rules.sha256.clone(),
        original_document_type: original_kind.as_str().into(),
        original_document_sha256: original_sha256.into(),
        redacted_payload_sha256: bundle.sha256.clone(),
        redactions_by_kind: redactions_by_kind.clone(),
        criteria_reviewed: criteria.to_vec(),
        check_requests_sent: log.check_requests_sent,
        second_pass_requests_sent: log.second_pass_requests_sent,
        failed_calls: log.failed_calls,
        feedback_items_generated: log.items_generated,
        feedback_items_shown: log.items_shown,
        feedback_items_rejected: log.items_rejected,
        rejected_by_rule: log.rejected_by_rule.clone(),
        acknowledgement_statement: statement,
    }
}

pub fn to_json(r: &Receipt) -> String {
    serde_json::to_string_pretty(r).expect("receipt serialises")
}

pub fn render_html(r: &Receipt) -> String {
    let row = |k: &str, v: &str| format!("<tr><th>{}</th><td>{}</td></tr>\n", esc(k), esc(v));
    let map = |m: &BTreeMap<String, usize>| {
        if m.is_empty() {
            "none".to_string()
        } else {
            m.iter().map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>().join(", ")
        }
    };
    let mut rows = String::new();
    rows += &row("Created (UTC)", &r.created_utc);
    rows += &row("App", &format!("{} {}", r.app_name, r.app_version));
    rows += &row("Engine / destination", &format!("{} → {}", r.engine, r.destination));
    rows += &row("Models configured", &r.models_configured.join(", "));
    rows += &row("Models that responded", &r.models_responded.join(", "));
    rows += &row(
        "Knowledge base",
        &format!(
            "{} v{} ({} syllabus) — {}",
            r.knowledge_base_id,
            r.knowledge_base_version,
            r.knowledge_base_syllabus_year,
            if r.knowledge_base_reviewed_by_teachers { "teacher-reviewed" } else { "DRAFT, not yet teacher-reviewed" }
        ),
    );
    rows += &row("Knowledge base SHA-256", &r.knowledge_base_sha256);
    rows += &row("Validator rules", &format!("v{} · SHA-256 {}", r.validator_rules_version, r.validator_rules_sha256));
    rows += &row("Original document", &format!("{} · SHA-256 {}", r.original_document_type, r.original_document_sha256));
    rows += &row("Redacted payload SHA-256", &r.redacted_payload_sha256);
    rows += &row("Redactions applied", &map(&r.redactions_by_kind));
    rows += &row("Criteria reviewed", &r.criteria_reviewed.join(", "));
    rows += &row(
        "Requests sent",
        &format!("{} check request(s), {} second-pass request(s), {} failed", r.check_requests_sent, r.second_pass_requests_sent, r.failed_calls),
    );
    rows += &row(
        "Feedback items",
        &format!("{} generated · {} shown · {} rejected", r.feedback_items_generated, r.feedback_items_shown, r.feedback_items_rejected),
    );
    rows += &row("Rejected by rule", &map(&r.rejected_by_rule));

    format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Session receipt</title>
<style>
body{{font-family:system-ui,Segoe UI,sans-serif;max-width:820px;margin:2rem auto;padding:0 1rem;color:#1d2330}}
table{{border-collapse:collapse;width:100%;font-size:.9rem}} th,td{{text-align:left;vertical-align:top;padding:.35rem .5rem;border-bottom:1px solid #dde}}
th{{width:32%;color:#445}} td{{word-break:break-all}} .ack{{background:#f4f5f8;padding:.8rem 1rem;border-radius:4px}}
@media print{{button{{display:none}}}}
</style></head><body>
<h1>Session receipt</h1>
<p>This receipt contains no text from your document, only hashes, versions and counts. You can verify the original-document hash yourself
(for example with <code>certutil -hashfile yourfile.docx SHA256</code> on Windows or <code>shasum -a 256 yourfile.docx</code> on macOS).</p>
<table>
{rows}</table>
<h2>Statement for your AI acknowledgement</h2>
<p class="ack">{ack}</p>
<p><small>Only use this statement if every part of it is true. Check your school's and the IB's current guidance on acknowledging AI tools.</small></p>
<button onclick="window.print()">Print / save as PDF</button>
</body></html>
"#,
        ack = esc(&r.acknowledgement_statement),
    )
}
