mod common;

use coach_core::parse::{self, FileKind, Paragraph};
use coach_core::redact::{redact_paragraphs, Identifiers};
use common::sample;

fn paras(lines: &[&str]) -> Vec<Paragraph> {
    lines.iter().map(|l| Paragraph { text: l.to_string(), heading_level: None }).collect()
}

fn redact_text(lines: &[&str], ids: &Identifiers) -> String {
    redact_paragraphs(&paras(lines), ids).full_text()
}

#[test]
fn sample_one_all_identifiers_removed() {
    let doc = parse::parse_bytes(&sample("pendulum-g.docx"), FileKind::Docx).unwrap();
    let ids = Identifiers {
        student_names: vec!["Jordan Avery".into()],
        candidate_codes: vec!["jxa123".into()],
        school_names: vec!["Northfield International School".into()],
        supervisor_names: vec!["Ms R. Okafor".into()],
        ..Default::default()
    };
    let r = redact_paragraphs(&doc.paragraphs, &ids);
    let t = r.full_text();
    for leaked in ["Jordan", "Avery", "jxa123", "Northfield", "Okafor", "jordan.avery@example.com", "7700", "900123"] {
        assert!(!t.contains(leaked), "{leaked} leaked:\n{t}");
    }
    assert!(t.contains("[STUDENT_NAME]") && t.contains("[SCHOOL]") && t.contains("[SUPERVISOR]"));
    assert!(t.contains("[EMAIL]") && t.contains("[PHONE]") && t.contains("[CANDIDATE_CODE]"));
    // Name inside body text ("my grandfather, Jordan Avery senior")
    assert!(t.contains("my grandfather, [STUDENT_NAME] senior"), "{t}");
    // Physics content untouched
    assert!(t.contains("T = 2π√(L/g)"));
    assert!(t.contains("0.60 | 1.58 | 1.52 | 1.60"));
}

#[test]
fn sample_two_detected_without_typed_identifiers() {
    // Labelled lines, e-mail, "tel" phone and session-number pattern are found
    // even if the student types nothing.
    let doc = parse::parse_bytes(&sample("resistance-wire.docx"), FileKind::Docx).unwrap();
    let r = redact_paragraphs(&doc.paragraphs, &Identifiers::default());
    let t = r.full_text();
    for leaked in ["Priya", "Raman-Clarke", "004321-0007", "Lakeside", "Ferreira", "priya.rc@example.org", "900456"] {
        assert!(!t.contains(leaked), "{leaked} leaked:\n{t}");
    }
    assert!(r.counts_by_kind["email"] >= 1 && r.counts_by_kind["phone"] >= 1);
}

#[test]
fn measurement_data_is_not_mistaken_for_phone_numbers() {
    let lines = [
        "| 0.100 | 0.42 | 0.268 | 1.57 |",
        "Times: 0.92 0.88 0.95 1.29 1.25 1.31 s",
        "g = 9.81 ± 0.02 m s-2 and 100 200 300 400 500 readings",
        "Gradient 13.5 Ω m⁻¹ from 2024-03-15 data",
    ];
    let r = redact_paragraphs(&paras(&lines), &Identifiers::default());
    assert!(r.counts_by_kind.is_empty(), "false positives: {:?}", r.terms);
    assert_eq!(r.full_text(), lines.join("\n"));
}

#[test]
fn name_parts_whole_words_and_case() {
    let ids = Identifiers { student_names: vec!["Ana Li".into()], ..Default::default() };
    let t = redact_text(&["Ana's results and Li's notes. Banana. analysis. ANA LI wrote this."], &ids);
    assert_eq!(t, "[STUDENT_NAME]'s results and [STUDENT_NAME]'s notes. Banana. analysis. [STUDENT_NAME] wrote this.");
}

#[test]
fn excluded_terms_stay_visible() {
    // Surname "Young" is also a physics word; the student can keep it visible.
    let ids = Identifiers {
        student_names: vec!["Sam Young".into()],
        excluded_terms: vec!["Young".into()],
        ..Default::default()
    };
    let t = redact_text(&["By Sam Young. Young's modulus was measured. Sam repeated it."], &ids);
    assert_eq!(t, "By [STUDENT_NAME]. Young's modulus was measured. [STUDENT_NAME] repeated it.");
}

#[test]
fn student_added_terms() {
    let ids = Identifiers { extra_terms: vec!["Room 4B".into()], ..Default::default() };
    assert_eq!(redact_text(&["Done in room 4b on Monday."], &ids), "Done in [REDACTED] on Monday.");
}

#[test]
fn titles_are_not_redacted_alone() {
    let ids = Identifiers { supervisor_names: vec!["Dr Tomás Ferreira".into()], ..Default::default() };
    let t = redact_text(&["Dr Tomás Ferreira helped. Tomás checked it. Dr Smith did not."], &ids);
    assert_eq!(t, "[SUPERVISOR] helped. [SUPERVISOR] checked it. Dr Smith did not.");
}

#[test]
fn segments_mark_redactions_for_the_review_screen() {
    let ids = Identifiers { student_names: vec!["Jo Bloggs".into()], ..Default::default() };
    let r = redact_paragraphs(&paras(&["Written by Jo Bloggs today."]), &ids);
    let marked: Vec<_> = r.segments.iter().filter(|s| s.redacted.is_some()).collect();
    assert_eq!(marked.len(), 1);
    assert_eq!(marked[0].text, "[STUDENT_NAME]");
    assert_eq!(r.terms[0].term, "Jo Bloggs");
}
