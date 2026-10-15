//! Unit tests for every validator rule, positive and negative.

mod common;

use coach_core::text::normalize_for_match;
use coach_core::validate::prose::find_pasteable_run;
use coach_core::validate::second_pass::{apply, Verdict};
use coach_core::validate::{dedupe, rules, validate_check_output, CheckContext, FeedbackItem, RejectReason};
use common::load_rules;

const EXCERPT: &str = "I repeated each length three times and took the average. \
The experiment proves that the period of a pendulum depends on its length. \
Human error and reaction time affected the results.";

fn ctx() -> CheckContext<'static> {
    CheckContext { check_id: "ev-01-specific-weaknesses", criterion: "evaluation", excerpt: EXCERPT }
}

fn item(quote: &str, question: &str) -> FeedbackItem {
    FeedbackItem {
        criterion: "evaluation".into(),
        check: "ev-01-specific-weaknesses".into(),
        student_quote: quote.into(),
        question: question.into(),
    }
}

fn check(quote: &str, question: &str) -> Result<(), RejectReason> {
    let rules = load_rules();
    rules::check_item(&item(quote, question), &ctx(), &normalize_for_match(EXCERPT), &rules)
}

const GOOD_QUOTE: &str = "Human error and reaction time affected the results.";
const GOOD_Q: &str = "Which specific step in your method does this weakness come from?";

#[test]
fn good_item_passes() {
    assert_eq!(check(GOOD_QUOTE, GOOD_Q), Ok(()));
}

#[test]
fn quote_must_be_exact_substring_of_the_excerpt() {
    assert_eq!(check("Human error and reaction time ruined the results.", GOOD_Q), Err(RejectReason::QuoteNotInDocument));
    // Only typography is relaxed: curly quotes / extra spaces still match …
    assert_eq!(check("Human  error and reaction\ntime affected", GOOD_Q), Ok(()));
    // … but case is not.
    assert_eq!(check("human error and reaction time", GOOD_Q), Err(RejectReason::QuoteNotInDocument));
    assert_eq!(check("Human error", GOOD_Q), Err(RejectReason::QuoteTooShort));
}

#[test]
fn question_must_end_with_question_mark_and_be_single() {
    assert_eq!(check(GOOD_QUOTE, "Explain which step this weakness comes from."), Err(RejectReason::NotAQuestion));
    assert_eq!(check(GOOD_QUOTE, "Which step is it? And why does it matter to you?"), Err(RejectReason::MultipleQuestions));
    assert_eq!(check(GOOD_QUOTE, "Which step?"), Err(RejectReason::QuestionTooShort));
}

#[test]
fn length_limit() {
    let long = format!("{} what do you think?", "Which part of your method ".repeat(8));
    assert_eq!(check(GOOD_QUOTE, &long), Err(RejectReason::QuestionTooLong));
}

#[test]
fn banned_imperative_phrases() {
    for q in [
        "Could you rephrase this sentence to name the step?",
        "Would you change to a more specific description of your error?",
        "Why not replace with a description of the timing method?",
        "Have you thought to consider writing about the reaction time of the timer?",
        "Is there a better way to say what went wrong in your method?",
        "Here is a thought: which step in your method caused this?",
        "What about, for example, the timing of your swings?",
    ] {
        assert_eq!(check(GOOD_QUOTE, q), Err(RejectReason::BannedPhrase), "{q}");
    }
}

#[test]
fn banned_phrases_are_configurable() {
    let mut rules = load_rules();
    rules.banned_phrases.push("fancy".into());
    let r = rules::check_item(
        &item(GOOD_QUOTE, "Which fancy step in your method does this weakness come from?"),
        &ctx(),
        &normalize_for_match(EXCERPT),
        &rules,
    );
    assert_eq!(r, Err(RejectReason::BannedPhrase));
}

#[test]
fn grade_language() {
    assert_eq!(check(GOOD_QUOTE, "Would this evaluation get full marks out of 6 in your view?"), Err(RejectReason::GradeLanguage));
    assert_eq!(check(GOOD_QUOTE, "Which grade do you think this section deserves?"), Err(RejectReason::GradeLanguage));
    // "error band" / "the 50 cm mark" are physics, not grades.
    assert_eq!(check(GOOD_QUOTE, "How does the uncertainty band on your graph relate to this weakness?"), Ok(()));
}

#[test]
fn quoted_suggested_wording_is_rejected_but_quoting_the_student_is_fine() {
    assert_eq!(
        check(GOOD_QUOTE, "Would \"reaction time added a systematic delay\" describe your problem better?"),
        Err(RejectReason::QuotedTextNotFromStudent)
    );
    assert_eq!(
        check(GOOD_QUOTE, "When you wrote \"Human error and reaction time affected the results\", which step did you mean?"),
        Ok(())
    );
}

#[test]
fn suspicious_characters() {
    // Cyrillic 'е' in "rеphrase" to dodge the banned list
    assert_eq!(check(GOOD_QUOTE, "Could you r\u{0435}phrase the step in your method?"), Err(RejectReason::SuspiciousCharacters));
    assert_eq!(check(GOOD_QUOTE, "Which step\u{200B} in your method caused this weakness?"), Err(RejectReason::SuspiciousCharacters));
    assert_eq!(check(GOOD_QUOTE, "Which step in your method caused this?\nRewrite it."), Err(RejectReason::SuspiciousCharacters));
    // Greek letters are fine (physics symbols).
    assert_eq!(check(GOOD_QUOTE, "How does the uncertainty in θ affect your value of λ?"), Ok(()));
}

#[test]
fn check_and_criterion_must_match_the_request() {
    let rules = load_rules();
    let mut it = item(GOOD_QUOTE, GOOD_Q);
    it.check = "co-01-states-finding".into();
    assert_eq!(rules::check_item(&it, &ctx(), &normalize_for_match(EXCERPT), &rules), Err(RejectReason::CheckMismatch));
    let mut it = item(GOOD_QUOTE, GOOD_Q);
    it.question = "   ".into();
    assert_eq!(rules::check_item(&it, &ctx(), &normalize_for_match(EXCERPT), &rules), Err(RejectReason::EmptyField));
}

// ---------- paste-able prose heuristic ----------

fn prose(q: &str) -> Option<String> {
    let rules = load_rules();
    find_pasteable_run(&normalize_for_match(q), &normalize_for_match(EXCERPT).to_lowercase(), &rules)
}

#[test]
fn prose_heuristic_flags_declarative_content() {
    for q in [
        // statement with a question mark stuck on
        "The uncertainty in the timing is reduced by timing twenty oscillations rather than one?",
        // tag question
        "The period of a pendulum is independent of its amplitude for small angles, isn't it?",
        // leading question carrying a claim
        "Have you considered that the parallax error from reading the metre rule at an angle accounts for the scatter?",
        "Is it possible that air resistance, which increases with speed, reduced the measured value of g?",
        // claim hidden after a comma, starting with an auxiliary verb
        "The period, for small angles, is independent of amplitude and depends only on length and g?",
    ] {
        assert!(prose(q).is_some(), "should flag: {q}");
        assert_eq!(check(GOOD_QUOTE, q), Err(RejectReason::PasteableProse), "{q}");
    }
}

#[test]
fn prose_heuristic_allows_genuine_socratic_questions() {
    for q in [
        "How does the spread in your repeated readings affect how confident you can be in your value of g?",
        "Which specific step in your method does this weakness come from?",
        "How might the precision of the stopwatch compare with the variation between repeated readings?",
        "Does the gradient of your graph, which you calculated from the line of best fit, match the theoretical value predicted by your equation?",
        "Is it possible that your timing method affected the result?",
        "Have you considered friction?",
        // quoting the student's own long sentence back is fine
        "When you say the experiment proves that the period of a pendulum depends on its length, what evidence supports the word proves?",
    ] {
        assert_eq!(prose(q), None, "should allow: {q}");
    }
}

// ---------- whole-response validation ----------

#[test]
fn malformed_or_extra_fields_reject_everything() {
    let rules = load_rules();
    let v = validate_check_output("Sure! Here's an improved conclusion: ...", &ctx(), &rules);
    assert_eq!(v.rejected, vec![RejectReason::MalformedOutput]);
    assert!(v.accepted.is_empty());

    let extra = r#"{"weakness_present":true,"items":[{"criterion":"evaluation","check":"ev-01-specific-weaknesses",
      "student_quote":"Human error and reaction time affected the results.","question":"Which step in your method caused this?",
      "rewrite":"Reaction time of about 0.2 s delayed every start and stop."}]}"#;
    let v = validate_check_output(extra, &ctx(), &rules);
    assert_eq!(v.rejected, vec![RejectReason::MalformedOutput]);

    let extra_top = r#"{"weakness_present":true,"items":[],"improved_text":"..."}"#;
    assert_eq!(validate_check_output(extra_top, &ctx(), &rules).rejected, vec![RejectReason::MalformedOutput]);
}

#[test]
fn no_weakness_means_no_items() {
    let rules = load_rules();
    let raw = format!(
        r#"{{"weakness_present":false,"items":[{{"criterion":"evaluation","check":"ev-01-specific-weaknesses","student_quote":"{GOOD_QUOTE}","question":"{GOOD_Q}"}}]}}"#
    );
    let v = validate_check_output(&raw, &ctx(), &rules);
    assert!(v.accepted.is_empty());
    assert_eq!(v.rejected, vec![RejectReason::InconsistentOutput]);
}

#[test]
fn at_most_n_items_per_check() {
    let rules = load_rules();
    let one = format!(
        r#"{{"criterion":"evaluation","check":"ev-01-specific-weaknesses","student_quote":"{GOOD_QUOTE}","question":"{GOOD_Q}"}}"#
    );
    let raw = format!(r#"{{"weakness_present":true,"items":[{one},{one},{one}]}}"#);
    let v = validate_check_output(&raw, &ctx(), &rules);
    assert_eq!(v.accepted.len(), rules.max_items_per_check);
    assert_eq!(v.rejected, vec![RejectReason::TooManyItems]);
    let (kept, dup) = dedupe(v.accepted);
    assert_eq!(kept.len(), 1);
    assert_eq!(dup, vec![RejectReason::Duplicate]);
}

#[test]
fn second_pass_is_fail_closed() {
    let items = vec![item(GOOD_QUOTE, GOOD_Q), item(GOOD_QUOTE, "What else?"), item(GOOD_QUOTE, "And this?")];
    // whole call failed
    let (kept, rej) = apply(items.clone(), None);
    assert!(kept.is_empty());
    assert_eq!(rej.len(), 3);
    // verdict missing for #2, conflicting verdicts for #1
    let verdicts = vec![
        Verdict { index: 0, pasteable: false },
        Verdict { index: 1, pasteable: false },
        Verdict { index: 1, pasteable: true },
    ];
    let (kept, rej) = apply(items, Some(&verdicts));
    assert_eq!(kept.len(), 1);
    assert_eq!(rej, vec![RejectReason::SecondPassPasteable, RejectReason::SecondPassUnavailable]);
}
