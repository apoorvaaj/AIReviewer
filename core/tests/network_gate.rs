//! Proves that NO network call happens before the student confirms the payload,
//! that a changed payload cannot be sent, and that what is sent is exactly
//! what was shown.

mod common;

use coach_core::payload::{build_bundle, confirm, ConfirmError};
use coach_core::pipeline::{build_tasks, load_document, run};
use coach_core::redact::{redact_paragraphs, Identifiers};
use coach_core::{parse::FileKind, sections};
use common::*;

fn responder() -> std::sync::Arc<MockTransport> {
    MockTransport::new(|body| {
        if is_second_pass(body) {
            api_response("claude-haiku-5-5", &verdicts_all(body, false))
        } else {
            api_response("claude-sonnet-5-5", r#"{"weakness_present":false,"items":[]}"#)
        }
    })
}

#[test]
fn no_network_calls_before_confirmation() {
    let transport = responder();
    let engine = engine_with(transport.clone());
    let kb = load_kb();
    let rules = load_rules();

    // Everything the app does before the payload screen:
    let doc = load_document(&sample("pendulum-g.docx"), FileKind::Docx).unwrap();
    let ids = Identifiers { student_names: vec!["Jordan Avery".into()], ..Default::default() };
    let red = redact_paragraphs(&doc.parsed.paragraphs, &ids);
    let secs = sections::split(&red.paragraphs);
    let tasks = build_tasks(&kb, &secs, &all_criteria());
    let bundle = build_bundle(&engine, &tasks);
    assert_eq!(transport.count(), 0, "network used before the payload screen");

    // Student is looking at the payload screen; a wrong / stale hash is refused.
    let err = confirm(bundle.clone(), "0000").err().unwrap();
    assert_eq!(err, ConfirmError::HashMismatch);
    assert_eq!(transport.count(), 0);

    // A payload modified after it was shown is refused, even with the shown hash.
    let mut tampered = bundle.clone();
    tampered.requests[0].body.push_str(" extra");
    assert!(confirm(tampered, &bundle.sha256).is_err());
    let mut tampered = bundle.clone();
    tampered.engine.primary_model = "some-other-model".into();
    assert!(confirm(tampered, &bundle.sha256).is_err());
    assert_eq!(transport.count(), 0);

    // Student presses Send.
    let shown_hash = bundle.sha256.clone();
    let confirmed = confirm(bundle.clone(), &shown_hash).unwrap();
    assert_eq!(transport.count(), 0, "confirming alone must not send");
    let outcome = run(&confirmed, &engine, &kb, &rules, 4, &|_, _| {});
    assert_eq!(transport.count(), bundle.requests.len(), "one call per check (no items => no second pass)");
    assert_eq!(outcome.log.check_requests_sent, bundle.requests.len());

    // What went over the wire is byte-for-byte what was displayed.
    let seen = transport.seen.lock().unwrap();
    for r in &bundle.requests {
        assert!(seen.iter().any(|s| s.raw_body == r.body && s.url == r.url), "sent body differs for {}", r.check_id);
    }
}

#[test]
fn sent_text_contains_no_identifiers() {
    let transport = responder();
    let engine = engine_with(transport.clone());
    let kb = load_kb();
    let doc = load_document(&sample("pendulum-g.docx"), FileKind::Docx).unwrap();
    let ids = Identifiers {
        student_names: vec!["Jordan Avery".into()],
        school_names: vec!["Northfield International School".into()],
        supervisor_names: vec!["Ms R. Okafor".into()],
        ..Default::default()
    };
    let red = redact_paragraphs(&doc.parsed.paragraphs, &ids);
    let bundle = build_bundle(&engine, &build_tasks(&kb, &sections::split(&red.paragraphs), &all_criteria()));
    let confirmed = confirm(bundle.clone(), &bundle.sha256).unwrap();
    run(&confirmed, &engine, &kb, &load_rules(), 2, &|_, _| {});
    for s in transport.seen.lock().unwrap().iter() {
        for leaked in ["Jordan", "Avery", "Northfield", "Okafor", "jxa123", "example.com", "900123"] {
            assert!(!s.raw_body.contains(leaked), "{leaked} was sent");
        }
    }
}

#[test]
fn missing_api_key_sends_nothing() {
    let transport = responder();
    let engine = coach_core::reasoning::cloud::CloudEngine::new(Default::default(), transport.clone(), None);
    let kb = load_kb();
    let doc = coach_core::parse::parse_plain_text("# Method\nWe timed twenty swings with a stopwatch and repeated each length three times for reliability.").unwrap();
    let bundle = build_bundle(&engine, &build_tasks(&kb, &sections::split(&doc.paragraphs), &["research_design".into()]));
    let confirmed = confirm(bundle.clone(), &bundle.sha256).unwrap();
    let outcome = run(&confirmed, &engine, &kb, &load_rules(), 2, &|_, _| {});
    assert_eq!(transport.count(), 0);
    assert_eq!(outcome.log.failed_calls, bundle.requests.len());
    assert!(outcome.items.is_empty());
}
