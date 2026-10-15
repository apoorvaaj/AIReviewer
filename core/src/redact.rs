//! Local redaction of identifying details. Runs entirely on the laptop.
//!
//! Sources of redaction, all applied together:
//!   1. Identifiers the student types (name, candidate code, school, supervisor).
//!      These are held in memory only and are never written anywhere.
//!      Names are matched as the full name (any case) AND as each capitalised
//!      part of the name ("Jordan", "Avery") as a whole word.
//!   2. Labelled lines such as "Name: ...", "School: ...", "Supervisor: ...".
//!   3. Patterns: e-mail addresses, phone numbers, IB-style candidate codes.
//!   4. Extra terms the student adds on the review screen.
//! The student can also mark a detected term as "keep visible" (excluded_terms),
//! e.g. when a surname is also a physics word ("Young's modulus").
//!
//! Each redacted span is replaced by a placeholder such as [STUDENT_NAME].

use std::collections::{BTreeMap, HashSet};
use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::parse::Paragraph;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Identifiers {
    pub student_names: Vec<String>,
    pub candidate_codes: Vec<String>,
    pub school_names: Vec<String>,
    pub supervisor_names: Vec<String>,
    pub extra_terms: Vec<String>,
    pub excluded_terms: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RedactionKind {
    StudentName,
    CandidateCode,
    School,
    Supervisor,
    Email,
    Phone,
    StudentAdded,
}

impl RedactionKind {
    pub fn placeholder(self) -> &'static str {
        match self {
            RedactionKind::StudentName => "[STUDENT_NAME]",
            RedactionKind::CandidateCode => "[CANDIDATE_CODE]",
            RedactionKind::School => "[SCHOOL]",
            RedactionKind::Supervisor => "[SUPERVISOR]",
            RedactionKind::Email => "[EMAIL]",
            RedactionKind::Phone => "[PHONE]",
            RedactionKind::StudentAdded => "[REDACTED]",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            RedactionKind::StudentName => "student_name",
            RedactionKind::CandidateCode => "candidate_code",
            RedactionKind::School => "school",
            RedactionKind::Supervisor => "supervisor",
            RedactionKind::Email => "email",
            RedactionKind::Phone => "phone",
            RedactionKind::StudentAdded => "student_added",
        }
    }
}

/// A piece of the redacted text, for highlighting on the review screen.
#[derive(Debug, Clone, Serialize)]
pub struct Segment {
    pub text: String,
    pub redacted: Option<RedactionKind>,
}

/// One distinct redacted term and how often it was found (review screen only;
/// never written to the receipt).
#[derive(Debug, Clone, Serialize)]
pub struct TermSummary {
    pub term: String,
    pub kind: RedactionKind,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct RedactionResult {
    pub paragraphs: Vec<Paragraph>,
    pub segments: Vec<Segment>,
    pub terms: Vec<TermSummary>,
    /// Counts only. This is the only redaction information that goes into the receipt.
    pub counts_by_kind: BTreeMap<String, usize>,
}

impl RedactionResult {
    pub fn full_text(&self) -> String {
        crate::parse::join_paragraphs(&self.paragraphs)
    }
}

struct Detectors {
    email: Regex,
    phone_international: Regex,
    phone_national: Regex,
    phone_labelled: Regex,
    ib_personal_code: Regex,
    ib_session_number: Regex,
    labelled_line: Regex,
}

fn detectors() -> &'static Detectors {
    static D: OnceLock<Detectors> = OnceLock::new();
    D.get_or_init(|| Detectors {
        email: Regex::new(r"(?i)\b[A-Z0-9._%+\-]+@[A-Z0-9.\-]+\.[A-Z]{2,}\b").unwrap(),
        // +44 7700 900123, +1 (555) 010-9999 ... digit count checked in code (8..=15).
        phone_international: Regex::new(r"\+\d{1,3}[\s\-]?\(?\d{1,4}\)?(?:[\s\-]?\d{2,6}){1,4}").unwrap(),
        // 07700 900123, (020) 7946 0958 — must start with 0, no decimal points,
        // so measurement data such as "0.45 0.46" never matches.
        phone_national: Regex::new(r"\(?\b0\d{2,4}\)?[\s\-]?\d{3,4}[\s\-]?\d{3,4}\b").unwrap(),
        phone_labelled: Regex::new(
            r"(?i)\b(?:phone|tel|telephone|mobile|cell)\b\.?\s*(?:no\.?|number)?\s*[:\-]?\s*([+\d][\d\s\-()]{6,}\d)",
        )
        .unwrap(),
        // IB personal code style: three letters + three digits (e.g. abc123).
        ib_personal_code: Regex::new(r"\b[A-Za-z]{3}\d{3}\b").unwrap(),
        // Older session-number style: 001234-0012.
        ib_session_number: Regex::new(r"\b\d{6}-\d{4}\b").unwrap(),
        labelled_line: Regex::new(
            r"(?im)^[ \t]*(student name|candidate name|name|student|candidate number|candidate no\.?|candidate code|candidate|personal code|session number|school name|school|supervisor|teacher|email|e-mail)[ \t]*[:\-\x{2013}][ \t]*(\S.*?)[ \t]*$",
        )
        .unwrap(),
    })
}

fn label_kind(label: &str) -> RedactionKind {
    let l = label.to_lowercase();
    if l.starts_with("school") {
        RedactionKind::School
    } else if l == "supervisor" || l == "teacher" {
        RedactionKind::Supervisor
    } else if l.contains("mail") {
        RedactionKind::Email
    } else if l.starts_with("candidate") && l != "candidate name" || l.contains("code") || l.contains("session") {
        RedactionKind::CandidateCode
    } else {
        RedactionKind::StudentName
    }
}

const TITLES: &[&str] = &["mr", "mrs", "ms", "miss", "mx", "dr", "prof", "sir", "madam"];

struct TermMatcher {
    re: Regex,
    kind: RedactionKind,
}

fn term_matcher(term: &str, kind: RedactionKind, case_insensitive: bool) -> TermMatcher {
    let escaped = regex::escape(term);
    let pattern = if case_insensitive { format!("(?i){escaped}") } else { escaped };
    TermMatcher { re: Regex::new(&pattern).expect("escaped literal is a valid regex"), kind }
}

/// Full name (any case) plus each capitalised name part (exact case, whole word).
fn name_matchers(full: &str, kind: RedactionKind, excluded: &HashSet<String>, out: &mut Vec<TermMatcher>) {
    let full = full.trim();
    if full.is_empty() || excluded.contains(&full.to_lowercase()) {
        return;
    }
    out.push(term_matcher(full, kind, true));
    for part in full.split(|c: char| c.is_whitespace() || c == ',') {
        let part = part.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '\'');
        let starts_upper = part.chars().next().is_some_and(|c| c.is_uppercase());
        if part.chars().count() >= 2
            && starts_upper
            && part != full
            && !TITLES.contains(&part.to_lowercase().trim_end_matches('.'))
            && !excluded.contains(&part.to_lowercase())
        {
            out.push(term_matcher(part, kind, false));
        }
    }
}

fn exact_matchers(terms: &[String], kind: RedactionKind, excluded: &HashSet<String>, out: &mut Vec<TermMatcher>) {
    for t in terms {
        let t = t.trim();
        if !t.is_empty() && !excluded.contains(&t.to_lowercase()) {
            out.push(term_matcher(t, kind, true));
        }
    }
}

fn whole_word(text: &str, start: usize, end: usize) -> bool {
    let before = text[..start].chars().next_back();
    let after = text[end..].chars().next();
    !before.is_some_and(|c| c.is_alphanumeric()) && !after.is_some_and(|c| c.is_alphanumeric())
}

#[derive(Debug, Clone, Copy)]
struct Hit {
    start: usize,
    end: usize,
    kind: RedactionKind,
}

fn find_hits(text: &str, matchers: &[TermMatcher], excluded: &HashSet<String>) -> Vec<Hit> {
    let d = detectors();
    let mut hits = Vec::new();
    let mut add = |start: usize, end: usize, kind: RedactionKind| {
        if start < end && !excluded.contains(&text[start..end].trim().to_lowercase()) {
            hits.push(Hit { start, end, kind });
        }
    };

    for m in matchers {
        for found in m.re.find_iter(text) {
            if whole_word(text, found.start(), found.end()) {
                add(found.start(), found.end(), m.kind);
            }
        }
    }
    for caps in d.labelled_line.captures_iter(text) {
        let (label, value) = (caps.get(1).unwrap(), caps.get(2).unwrap());
        add(value.start(), value.end(), label_kind(label.as_str()));
    }
    for m in d.email.find_iter(text) {
        add(m.start(), m.end(), RedactionKind::Email);
    }
    for re in [&d.phone_international, &d.phone_national] {
        for m in re.find_iter(text) {
            let digits = m.as_str().chars().filter(|c| c.is_ascii_digit()).count();
            if (8..=15).contains(&digits) {
                add(m.start(), m.end(), RedactionKind::Phone);
            }
        }
    }
    for caps in d.phone_labelled.captures_iter(text) {
        let m = caps.get(1).unwrap();
        add(m.start(), m.end(), RedactionKind::Phone);
    }
    for re in [&d.ib_personal_code, &d.ib_session_number] {
        for m in re.find_iter(text) {
            add(m.start(), m.end(), RedactionKind::CandidateCode);
        }
    }

    // Merge overlapping hits; the earliest (then longest) hit decides the kind.
    hits.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
    let mut merged: Vec<Hit> = Vec::new();
    for h in hits {
        match merged.last_mut() {
            Some(last) if h.start < last.end => last.end = last.end.max(h.end),
            _ => merged.push(h),
        }
    }
    merged
}

pub fn redact_paragraphs(paragraphs: &[Paragraph], ids: &Identifiers) -> RedactionResult {
    let excluded: HashSet<String> = ids
        .excluded_terms
        .iter()
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty())
        .collect();

    let mut matchers = Vec::new();
    for n in &ids.student_names {
        name_matchers(n, RedactionKind::StudentName, &excluded, &mut matchers);
    }
    for n in &ids.supervisor_names {
        name_matchers(n, RedactionKind::Supervisor, &excluded, &mut matchers);
    }
    exact_matchers(&ids.school_names, RedactionKind::School, &excluded, &mut matchers);
    exact_matchers(&ids.candidate_codes, RedactionKind::CandidateCode, &excluded, &mut matchers);
    exact_matchers(&ids.extra_terms, RedactionKind::StudentAdded, &excluded, &mut matchers);

    let mut out_paragraphs = Vec::with_capacity(paragraphs.len());
    let mut segments = Vec::new();
    let mut term_counts: BTreeMap<(String, RedactionKind), (String, usize)> = BTreeMap::new();
    let mut counts_by_kind: BTreeMap<String, usize> = BTreeMap::new();

    for (i, p) in paragraphs.iter().enumerate() {
        if i > 0 {
            segments.push(Segment { text: "\n".into(), redacted: None });
        }
        let hits = find_hits(&p.text, &matchers, &excluded);
        let mut redacted = String::with_capacity(p.text.len());
        let mut cursor = 0;
        for h in &hits {
            if h.start > cursor {
                let plain = &p.text[cursor..h.start];
                redacted.push_str(plain);
                segments.push(Segment { text: plain.into(), redacted: None });
            }
            let original = p.text[h.start..h.end].to_string();
            let entry = term_counts
                .entry((original.to_lowercase(), h.kind))
                .or_insert((original, 0));
            entry.1 += 1;
            *counts_by_kind.entry(h.kind.as_str().to_string()).or_insert(0) += 1;

            redacted.push_str(h.kind.placeholder());
            segments.push(Segment { text: h.kind.placeholder().into(), redacted: Some(h.kind) });
            cursor = h.end;
        }
        if cursor < p.text.len() {
            let plain = &p.text[cursor..];
            redacted.push_str(plain);
            segments.push(Segment { text: plain.into(), redacted: None });
        }
        out_paragraphs.push(Paragraph { text: redacted, heading_level: p.heading_level });
    }

    let terms = term_counts
        .into_iter()
        .map(|((_, kind), (term, count))| TermSummary { term, kind, count })
        .collect();

    RedactionResult { paragraphs: out_paragraphs, segments, terms, counts_by_kind }
}
