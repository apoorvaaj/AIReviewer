//! The "paste-able prose" heuristic.
//!
//! Idea: a genuine Socratic question is ADDRESSED TO THE STUDENT and ASKS
//! something. Text that could be pasted into an essay is a declarative
//! statement ABOUT THE PHYSICS. So we look for a long run of words that is
//! neither addressed to the student nor interrogative.
//!
//! Steps (all on the lower-cased, normalised question):
//!   1. Mask any run of >= 6 words copied from the student's own excerpt
//!      (quoting the student back is fine; it is their text). Shorter
//!      overlaps are NOT masked: common phrases like "the period of a pendulum"
//!      must not let a claim hide behind the student's vocabulary.
//!   2. Leading-question complements: after an opener such as
//!      "have you considered THAT ...", the rest of the question up to '?' is a
//!      proposition. If it has more than `prose_run_max_words` words and does
//!      not mention the student (you/your), it is a claim the student could
//!      paste ("...that the parallax error accounts for the scatter?").
//!   3. Split into clauses at , ; : ( ) " ! ? sentence-ending '.', " - " and
//!      masked spans. A clause with more than `prose_run_max_words` words is
//!      prose unless it contains a second-person word (you/your/...) or a
//!      wh-word (how/why/what/...), or — for the FIRST clause only — starts
//!      with an auxiliary verb (does/is/could/...), i.e. a real inversion.
//!
//! Known limitation (documented in README): a question can still carry a
//! short hint ("Have you considered friction?") or wrap a claim inside a
//! clause that also says "your". The second-pass model check exists for that.

use super::ValidatorRules;

const MIN_MASKED_RUN_WORDS: usize = 6;

pub fn find_pasteable_run(question_norm: &str, excerpt_lower: &str, rules: &ValidatorRules) -> Option<String> {
    let lower = question_norm.to_lowercase();
    let tokens: Vec<&str> = lower.split(' ').filter(|t| !t.is_empty()).collect();

    // 1. mask student-text runs of >= MIN_MASKED_RUN_WORDS words
    let mut masked = vec![false; tokens.len()];
    let mut i = 0;
    while i < tokens.len() {
        let mut best = 0;
        let mut j = i + MIN_MASKED_RUN_WORDS;
        while j <= tokens.len() {
            let window = tokens[i..j].join(" ");
            let w = window.trim_matches(|c: char| !c.is_alphanumeric());
            if !w.is_empty() && excerpt_lower.contains(w) {
                best = j;
                j += 1;
            } else {
                break;
            }
        }
        if best > 0 {
            masked[i..best].iter_mut().for_each(|m| *m = true);
            i = best;
        } else {
            i += 1;
        }
    }
    let rebuilt = tokens
        .iter()
        .zip(&masked)
        .map(|(t, m)| if *m { "|" } else { *t })
        .collect::<Vec<_>>()
        .join(" ");

    // 2. leading-question complements
    for opener in &rules.leading_openers {
        let mut from = 0;
        while let Some(pos) = rebuilt[from..].find(opener.as_str()) {
            let start = from + pos + opener.len();
            let rest = &rebuilt[start..];
            let end = rest.find(['?', '|']).unwrap_or(rest.len());
            let complement = &rest[..end];
            let words = content_words(complement);
            if words.len() > rules.prose_run_max_words
                && !words.iter().any(|w| rules.second_person_words.contains(w))
            {
                return Some(complement.trim().to_string());
            }
            from = start;
        }
    }

    // 3. clauses
    for (index, clause) in split_clauses(&rebuilt).iter().enumerate() {
        let words = content_words(clause);
        if words.len() <= rules.prose_run_max_words {
            continue;
        }
        let addressed_or_asking = words
            .iter()
            .any(|w| rules.second_person_words.contains(w) || rules.wh_words.contains(w));
        let inverted_question = index == 0 && words.first().is_some_and(|w| rules.auxiliary_openers.contains(w));
        if !addressed_or_asking && !inverted_question {
            return Some(clause.trim().to_string());
        }
    }
    None
}

fn content_words(s: &str) -> Vec<String> {
    s.split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'').to_string())
        .filter(|w| w.chars().any(|c| c.is_alphabetic()))
        .collect()
}

/// Split at clause punctuation. '.' splits only when it ends a sentence
/// (so 9.81 stays one token); '-' splits only as a spaced dash.
fn split_clauses(s: &str) -> Vec<String> {
    let chars: Vec<char> = s.chars().collect();
    let mut clauses = Vec::new();
    let mut current = String::new();
    for (i, &c) in chars.iter().enumerate() {
        let next = chars.get(i + 1).copied();
        let prev = if i > 0 { chars.get(i - 1).copied() } else { None };
        let breaks = match c {
            ',' | ';' | ':' | '(' | ')' | '"' | '!' | '?' | '|' => true,
            '.' => next.is_none_or(|n| n.is_whitespace()),
            '-' => prev == Some(' ') && next == Some(' '),
            _ => false,
        };
        if breaks {
            if !current.trim().is_empty() {
                clauses.push(std::mem::take(&mut current));
            }
            current.clear();
        } else {
            current.push(c);
        }
    }
    if !current.trim().is_empty() {
        clauses.push(current);
    }
    clauses
}
