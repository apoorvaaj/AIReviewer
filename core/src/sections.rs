//! Labels each paragraph with an IA section, so each knowledge-base check is
//! sent only the part of the document it needs (less data leaves the laptop,
//! and the model gets a focused task).

use std::collections::BTreeSet;

use serde::Serialize;

use crate::parse::Paragraph;
use crate::text::word_count;

/// Section ids used by `applies_to_sections` in the knowledge base.
pub const SECTION_IDS: &[&str] = &[
    "front_matter",
    "research_question",
    "background",
    "hypothesis",
    "variables",
    "method",
    "data",
    "analysis",
    "conclusion",
    "evaluation",
    "references",
];

/// Heading keyword -> section. Order matters: first match wins
/// (so "Data analysis" is analysis, "Evaluation of uncertainties" is evaluation).
const HEADING_KEYWORDS: &[(&str, &[&str])] = &[
    ("research_question", &["research question"]),
    ("hypothesis", &["hypothes", "prediction"]),
    ("variables", &["variable"]),
    (
        "evaluation",
        &["evaluat", "limitation", "improvement", "weakness", "extension", "sources of error", "error analysis"],
    ),
    ("conclusion", &["conclusion", "discussion"]),
    ("analysis", &["analys", "processing", "calculation", "graph", "uncertaint"]),
    ("data", &["raw data", "result", "data collect", "observation", "data"]),
    (
        "method",
        &["method", "procedure", "apparatus", "material", "equipment", "safety", "set-up", "setup", "design", "exploration"],
    ),
    ("background", &["introduction", "background", "theory", "context", "rationale", "aim", "personal engagement"]),
    ("references", &["reference", "bibliograph", "works cited", "citation"]),
];

/// Matches a keyword only at the start of a word ("aim" matches "Aims" but not "claim").
fn has_keyword(lower: &str, keyword: &str) -> bool {
    lower.match_indices(keyword).any(|(pos, _)| {
        !lower[..pos].chars().next_back().is_some_and(|c| c.is_alphabetic())
    })
}

pub fn classify_heading(text: &str) -> Option<&'static str> {
    let lower = text.to_lowercase();
    HEADING_KEYWORDS
        .iter()
        .find(|(_, kws)| kws.iter().any(|k| has_keyword(&lower, k)))
        .map(|(id, _)| *id)
}

pub fn looks_like_heading(p: &Paragraph) -> bool {
    if p.heading_level.is_some() {
        return true;
    }
    let t = p.text.trim();
    let wc = word_count(t);
    wc > 0 && wc <= 8 && !t.ends_with(['.', '?', ',', ';']) && classify_heading(t).is_some()
}

#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    pub sections: Vec<String>,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Sections {
    pub entries: Vec<Entry>,
}

pub fn split(paragraphs: &[Paragraph]) -> Sections {
    let mut current = "front_matter";
    let mut entries = Vec::new();
    for p in paragraphs {
        if looks_like_heading(p) {
            if let Some(id) = classify_heading(&p.text) {
                current = id;
            }
        }
        let mut secs = vec![current.to_string()];
        // A "Research question: ..." line belongs to the RQ wherever it appears.
        if current != "research_question" && p.text.trim_start().to_lowercase().starts_with("research question") {
            secs.push("research_question".to_string());
        }
        entries.push(Entry { sections: secs, text: p.text.clone() });
    }
    Sections { entries }
}

impl Sections {
    /// Paragraphs from any of `ids`, in document order, each at most once.
    pub fn text_for(&self, ids: &[String]) -> String {
        self.entries
            .iter()
            .filter(|e| e.sections.iter().any(|s| ids.contains(s)))
            .map(|e| e.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The whole document except the reference list (not needed for feedback).
    pub fn full_text_for_payload(&self) -> String {
        self.entries
            .iter()
            .filter(|e| !e.sections.iter().any(|s| s == "references"))
            .map(|e| e.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn detected(&self) -> BTreeSet<String> {
        self.entries.iter().flat_map(|e| e.sections.iter().cloned()).collect()
    }
}
