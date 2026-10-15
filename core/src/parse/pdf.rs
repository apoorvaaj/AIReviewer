//! .pdf text extraction (pure Rust, via the `pdf-extract` crate).
//!
//! PDF text extraction is inherently approximate; the student sees the
//! extracted text on the redaction-review screen before anything is sent.
//! Document metadata (author, title, ...) is never read.

use super::{Paragraph, ParseError};
use crate::sections;

pub fn parse(bytes: &[u8]) -> Result<Vec<Paragraph>, ParseError> {
    // pdf-extract can panic on malformed files; contain that.
    let result = std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem(bytes));
    match result {
        Ok(Ok(text)) => Ok(text_to_paragraphs(&text)),
        _ => Err(ParseError::Corrupt),
    }
}

/// Turn line-oriented text into paragraphs. Blank lines end a paragraph;
/// lines that look like section headings, table rows ("| a | b |") or list
/// items become their own paragraphs. A leading Markdown "#" marks a heading
/// (used only by the synthetic test documents).
pub fn text_to_paragraphs(text: &str) -> Vec<Paragraph> {
    let mut out = Vec::new();
    let mut current: Vec<&str> = Vec::new();

    fn flush(current: &mut Vec<&str>, out: &mut Vec<Paragraph>) {
        if !current.is_empty() {
            let text = current.join(" ").split_whitespace().collect::<Vec<_>>().join(" ");
            if !text.is_empty() {
                out.push(Paragraph { text, heading_level: None });
            }
            current.clear();
        }
    }

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            flush(&mut current, &mut out);
            continue;
        }
        if let Some(stripped) = line.strip_prefix('#') {
            flush(&mut current, &mut out);
            let level = 1 + stripped.chars().take_while(|c| *c == '#').count() as u8;
            let text = stripped.trim_start_matches('#').trim().to_string();
            out.push(Paragraph { text, heading_level: Some(level) });
            continue;
        }
        let candidate = Paragraph { text: line.to_string(), heading_level: None };
        if sections::looks_like_heading(&candidate) {
            flush(&mut current, &mut out);
            out.push(candidate);
            continue;
        }
        if line.starts_with('|') || line.starts_with("- ") || line.starts_with("* ") {
            flush(&mut current, &mut out);
            let cleaned = line.trim_matches('|').trim().to_string();
            out.push(Paragraph { text: cleaned, heading_level: None });
            continue;
        }
        current.push(line);
    }
    flush(&mut current, &mut out);
    out
}
