//! Text extraction. Only the BODY text of a document is read.
//!
//! Deliberately NOT read: page headers/footers, footnotes, comments, document
//! properties/metadata (author, company, ...), images and embedded objects.
//! These often carry the student's name or candidate number and are not needed
//! for feedback.

pub mod docx;
pub mod pdf;

use serde::Serialize;
use std::path::Path;

pub const MAX_FILE_BYTES: usize = 20 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FileKind {
    Docx,
    Pdf,
    /// Plain text; used by tests and the red-team suite only.
    Text,
}

impl FileKind {
    pub fn as_str(self) -> &'static str {
        match self {
            FileKind::Docx => "docx",
            FileKind::Pdf => "pdf",
            FileKind::Text => "text",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Paragraph {
    pub text: String,
    /// Some(level) when the source marks this paragraph as a heading.
    pub heading_level: Option<u8>,
}

#[derive(Debug, Clone)]
pub struct ParsedDocument {
    pub kind: FileKind,
    pub paragraphs: Vec<Paragraph>,
}

impl ParsedDocument {
    pub fn full_text(&self) -> String {
        join_paragraphs(&self.paragraphs)
    }

    pub fn word_count(&self) -> usize {
        self.paragraphs.iter().map(|p| crate::text::word_count(&p.text)).sum()
    }
}

pub fn join_paragraphs(paragraphs: &[Paragraph]) -> String {
    paragraphs
        .iter()
        .map(|p| p.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ParseError {
    #[error("Unsupported file type. Please choose a .docx or .pdf file.")]
    Unsupported,
    #[error("The file is larger than the 20 MB limit.")]
    TooLarge,
    #[error("This does not look like a valid Word (.docx) file.")]
    NotADocx,
    #[error("The file could not be read. It may be damaged or password-protected.")]
    Corrupt,
    #[error("No text could be extracted. Scanned (image-only) PDFs are not supported.")]
    NoText,
}

pub fn kind_from_path(path: &Path) -> Option<FileKind> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "docx" => Some(FileKind::Docx),
        "pdf" => Some(FileKind::Pdf),
        _ => None,
    }
}

pub fn parse_bytes(bytes: &[u8], kind: FileKind) -> Result<ParsedDocument, ParseError> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err(ParseError::TooLarge);
    }
    let paragraphs = match kind {
        FileKind::Docx => {
            if !bytes.starts_with(b"PK\x03\x04") {
                return Err(ParseError::NotADocx);
            }
            docx::parse(bytes)?
        }
        FileKind::Pdf => {
            if !bytes.starts_with(b"%PDF") {
                return Err(ParseError::Corrupt);
            }
            pdf::parse(bytes)?
        }
        FileKind::Text => {
            let text = std::str::from_utf8(bytes).map_err(|_| ParseError::Corrupt)?;
            pdf::text_to_paragraphs(text)
        }
    };
    if paragraphs.iter().all(|p| crate::text::word_count(&p.text) == 0) {
        return Err(ParseError::NoText);
    }
    Ok(ParsedDocument { kind, paragraphs })
}

pub fn parse_plain_text(text: &str) -> Result<ParsedDocument, ParseError> {
    parse_bytes(text.as_bytes(), FileKind::Text)
}
