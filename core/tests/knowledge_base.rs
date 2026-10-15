mod common;

use coach_core::kb::{Scope, STATUS_DRAFT};
use coach_core::validate::{rules, CheckContext, FeedbackItem};
use common::{load_kb, load_rules};

#[test]
fn kb_loads_with_6_to_10_checks_per_criterion() {
    let kb = load_kb();
    assert_eq!(kb.manifest.syllabus_year, 2025);
    assert_eq!(kb.criteria.len(), 4);
    for c in &kb.criteria {
        let n = kb.checks.iter().filter(|k| k.criterion == c.id && k.scope == Scope::Sections).count();
        assert!((6..=10).contains(&n), "{} has {n} section checks", c.id);
    }
    assert!(kb.checks.iter().filter(|k| k.scope == Scope::WholeDocument).count() >= 3);
}

#[test]
fn every_entry_is_marked_for_teacher_review() {
    let kb = load_kb();
    assert_eq!(kb.manifest.status, STATUS_DRAFT);
    assert!(kb.checks.iter().all(|c| c.status == STATUS_DRAFT));
    assert!(kb.has_unreviewed_checks());
}

#[test]
fn every_question_template_passes_the_validator() {
    // If a teacher edits a template into something the validator would block,
    // this test tells them immediately.
    let kb = load_kb();
    let rules = load_rules();
    let excerpt = "this is the student's own sentence about the pendulum";
    let norm = coach_core::text::normalize_for_match(excerpt);
    for c in &kb.checks {
        for t in &c.question_templates {
            let item = FeedbackItem {
                criterion: c.criterion.clone(),
                check: c.id.clone(),
                student_quote: "the student's own sentence".into(),
                question: t.clone(),
            };
            let ctx = CheckContext { check_id: &c.id, criterion: &c.criterion, excerpt };
            assert_eq!(rules::check_item(&item, &ctx, &norm, &rules), Ok(()), "template of {}: {t}", c.id);
        }
    }
}

#[test]
fn kb_rejects_inconsistent_files() {
    let dir = std::env::temp_dir().join(format!("kb-test-{}", std::process::id()));
    let src = common::repo_root().join("knowledge-base/physics-ia");
    std::fs::create_dir_all(&dir).unwrap();
    for f in std::fs::read_dir(&src).unwrap() {
        let f = f.unwrap();
        std::fs::copy(f.path(), dir.join(f.file_name())).unwrap();
    }
    let path = dir.join("evaluation.yaml");
    let text = std::fs::read_to_string(&path).unwrap().replacen("kb_version: \"0.1.0\"", "kb_version: \"0.2.0\"", 1);
    std::fs::write(&path, text).unwrap();
    let err = coach_core::kb::load(&dir).unwrap_err().to_string();
    std::fs::remove_dir_all(&dir).ok();
    assert!(err.contains("kb_version"), "{err}");
}
