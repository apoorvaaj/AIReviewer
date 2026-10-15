//! The receipt must never contain document text.

mod common;

use coach_core::payload::{build_bundle, confirm};
use coach_core::pipeline::{build_tasks, load_document, run};
use coach_core::receipt::{self, ReceiptInput};
use coach_core::redact::{redact_paragraphs, Identifiers};
use coach_core::{parse::FileKind, report, sections};
use common::*;

#[test]
fn receipt_has_hashes_and_counts_but_no_document_text() {
    let quote = "The results were as expected.";
    let transport = MockTransport::new(move |body| {
        if is_second_pass(body) {
            api_response("claude-haiku-5-5", &verdicts_all(body, false))
        } else {
            let (check, criterion) = requested_check(body);
            let sent = sent_student_text(body);
            let text = if sent.contains(quote) {
                format!(
                    r#"{{"weakness_present":true,"items":[{{"criterion":"{criterion}","check":"{check}","student_quote":"{quote}","question":"Which values in your data were you expecting, and why {check}?"}}]}}"#
                )
            } else {
                r#"{"weakness_present":false,"items":[]}"#.to_string()
            };
            api_response("claude-sonnet-5-5", &text)
        }
    });
    let engine = engine_with(transport);
    let kb = load_kb();
    let rules = load_rules();
    let bytes = sample("pendulum-g.docx");
    let doc = load_document(&bytes, FileKind::Docx).unwrap();
    let ids = Identifiers { student_names: vec!["Jordan Avery".into()], ..Default::default() };
    let red = redact_paragraphs(&doc.parsed.paragraphs, &ids);
    let criteria = all_criteria();
    let bundle = build_bundle(&engine, &build_tasks(&kb, &sections::split(&red.paragraphs), &criteria));
    let confirmed = confirm(bundle.clone(), &bundle.sha256).unwrap();
    let outcome = run(&confirmed, &engine, &kb, &rules, 4, &|_, _| {});
    assert!(outcome.items.len() >= 1, "fixture should produce shown items: {:?}", outcome.log);

    let r = receipt::build(ReceiptInput {
        created_utc: chrono::Utc::now(),
        kb: &kb,
        rules: &rules,
        bundle: &bundle,
        outcome: &outcome,
        original_kind: FileKind::Docx,
        original_sha256: &doc.original_sha256,
        redactions_by_kind: &red.counts_by_kind,
        criteria: &criteria,
    });
    let json = receipt::to_json(&r);
    let html = receipt::render_html(&r);

    // hashes and counts are present and correct
    assert_eq!(r.original_document_sha256, coach_core::text::sha256_hex(&bytes));
    assert_eq!(r.redacted_payload_sha256, bundle.sha256);
    assert_eq!(r.feedback_items_shown, outcome.items.len());
    assert_eq!(r.check_requests_sent, bundle.requests.len());
    assert!(!r.knowledge_base_reviewed_by_teachers);
    assert!(json.contains(&bundle.sha256) && html.contains(&bundle.sha256));
    assert!(r.acknowledgement_statement.contains("claude-sonnet-5-5"));

    // no run of 5 consecutive document words, no identifiers, no questions
    let full = doc.parsed.full_text();
    let words: Vec<&str> = full.split_whitespace().collect();
    for w in words.windows(5) {
        let phrase = w.join(" ");
        assert!(!json.contains(&phrase), "receipt JSON contains document text: {phrase}");
        assert!(!html.contains(&phrase), "receipt HTML contains document text: {phrase}");
    }
    for leaked in ["Jordan", "Avery", "Northfield", "Okafor", "pendulum-g", quote, "Which values in your data"] {
        assert!(!json.contains(leaked) && !html.contains(leaked), "{leaked} in receipt");
    }

    // round-trips with deny_unknown_fields (format is stable and closed)
    let back: receipt::Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(back, r);

    // the report, by contrast, does contain the questions and quotes
    let rep = report::render_html(&outcome, &kb, &criteria, "today");
    assert!(rep.contains("Which values in your data"));
    assert!(!rep.contains("<script"));
}
