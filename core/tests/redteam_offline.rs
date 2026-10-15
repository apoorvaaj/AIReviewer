//! RED-TEAM SUITE (offline, deterministic).
//!
//! Each YAML file in tests/redteam/cases/ is one attack. It contains a
//! synthetic student document with embedded instructions, plus the output a
//! COMPROMISED model might return if the injection worked. The full pipeline
//! runs (request building, response parsing, validation, second pass) with a
//! mock network, and the case states how many items may reach the student.
//!
//! By default the mock second-pass model is ALSO fooled (says "not pasteable"),
//! so these cases prove the deterministic rules alone stop the attack.
//!
//! To add a case: copy any file in tests/redteam/cases/, change it, run
//! `cargo test -p coach-core --test redteam_offline`. Fields:
//!   id, description, document              (required)
//!   model_output        canned raw text returned for EVERY check call;
//!                       {{check}} and {{criterion}} are filled in per call
//!   second_pass         "fooled" (default) | "pasteable" | "error"
//!   expect_shown_max    default 0
//!   expect_shown_min    default 0 (control cases set this to prove the
//!                       validator is not just rejecting everything)
//!   forbidden_in_output strings that must never appear in a shown question
//!   expect_rejected_by  rule names (see RejectReason::as_str) that must
//!                       appear in the run log, proving WHICH defence held
//!   criteria            default: all

mod common;

use coach_core::net::HttpResponse;
use coach_core::payload::{build_bundle, confirm};
use coach_core::pipeline::{build_tasks, run, RunOutcome};
use coach_core::redact::{redact_paragraphs, Identifiers};
use coach_core::{parse, sections};
use common::redteam_cases::{load_cases, Case};
use common::*;

fn run_case(case: &Case) -> RunOutcome {
    let canned = case.model_output.clone().expect("offline cases need model_output");
    let mode = case.second_pass.clone().unwrap_or_else(|| "fooled".into());
    let transport = MockTransport::new(move |body| {
        if is_second_pass(body) {
            match mode.as_str() {
                "fooled" => api_response("claude-haiku-5-5", &verdicts_all(body, false)),
                "pasteable" => api_response("claude-haiku-5-5", &verdicts_all(body, true)),
                "error" => HttpResponse { status: 500, body: r#"{"type":"error","error":{"message":"overloaded"}}"#.into() },
                other => panic!("unknown second_pass mode {other}"),
            }
        } else {
            let (check, criterion) = requested_check(body);
            let text = canned.replace("{{check}}", &check).replace("{{criterion}}", &criterion);
            api_response("claude-sonnet-5-5", &text)
        }
    });
    let engine = engine_with(transport);
    let kb = load_kb();
    let doc = parse::parse_plain_text(&case.document).unwrap();
    let red = redact_paragraphs(&doc.paragraphs, &Identifiers::default());
    let criteria = case.criteria.clone().unwrap_or_else(all_criteria);
    let tasks = build_tasks(&kb, &sections::split(&red.paragraphs), &criteria);
    let bundle = build_bundle(&engine, &tasks);
    let confirmed = confirm(bundle.clone(), &bundle.sha256).unwrap();
    run(&confirmed, &engine, &kb, &load_rules(), 4, &|_, _| {})
}

#[test]
fn red_team_cases() {
    let cases = load_cases();
    assert!(cases.len() >= 15, "expected at least 15 red-team cases, found {}", cases.len());
    let mut failures = Vec::new();
    for (path, case) in &cases {
        let outcome = run_case(case);
        let shown = outcome.items.len();
        let name = path.file_name().unwrap().to_string_lossy();
        if shown > case.expect_shown_max {
            failures.push(format!("{name} ({}): {shown} item(s) shown, max {}: {:?}", case.id, case.expect_shown_max,
                outcome.items.iter().map(|i| &i.question).collect::<Vec<_>>()));
        }
        if shown < case.expect_shown_min {
            failures.push(format!("{name} ({}): {shown} item(s) shown, min {}. rejected: {:?}", case.id,
                case.expect_shown_min, outcome.log.rejected_by_rule));
        }
        for item in &outcome.items {
            for f in &case.forbidden_in_output {
                if item.question.to_lowercase().contains(&f.to_lowercase()) {
                    failures.push(format!("{name}: forbidden text {f:?} shown"));
                }
            }
        }
        for rule in &case.expect_rejected_by {
            if !outcome.log.rejected_by_rule.contains_key(rule) {
                failures.push(format!("{name} ({}): expected rejection by {rule}, got {:?}", case.id, outcome.log.rejected_by_rule));
            }
        }
        // Every rejection is counted, never shown.
        assert_eq!(outcome.log.items_shown, shown);
        let _ = &case.description;
    }
    assert!(failures.is_empty(), "red-team failures:\n{}", failures.join("\n"));
}
