mod common;

use coach_core::parse::{self, docx, FileKind, ParseError};
use coach_core::sections;
use common::sample;

#[test]
fn docx_body_text_tables_and_headings() {
    let doc = parse::parse_bytes(&sample("pendulum-g.docx"), FileKind::Docx).unwrap();
    let text = doc.full_text();
    assert!(text.contains("How does the length of a pendulum affect it?"));
    assert!(text.contains("0.60 | 1.58 | 1.52 | 1.60"), "table rows flattened: {text}");
    assert!(doc.paragraphs.iter().any(|p| p.text == "Research question" && p.heading_level == Some(1)));
}

#[test]
fn docx_header_and_metadata_are_never_read() {
    // The sample's page header and docProps/core.xml both contain
    // "Jordan Avery - Physics IA draft". That string must not be extracted.
    let doc = parse::parse_bytes(&sample("pendulum-g.docx"), FileKind::Docx).unwrap();
    assert!(!doc.full_text().contains("Physics IA draft"));
    let doc2 = parse::parse_bytes(&sample("resistance-wire.docx"), FileKind::Docx).unwrap();
    assert!(!doc2.full_text().contains("Physics IA draft"));
}

#[test]
fn docx_skips_fallback_duplicates_deleted_text_and_reads_equations() {
    let xml = r#"<w:document xmlns:w="w" xmlns:m="m" xmlns:mc="mc"><w:body>
      <w:p><w:r><w:t>Visible</w:t></w:r>
        <mc:AlternateContent><mc:Choice><w:txbxContent><w:p><w:r><w:t>Box text</w:t></w:r></w:p></w:txbxContent></mc:Choice>
        <mc:Fallback><w:p><w:r><w:t>Box text</w:t></w:r></w:p></mc:Fallback></mc:AlternateContent>
        <w:r><w:t xml:space="preserve"> sentence</w:t></w:r></w:p>
      <w:p><w:del><w:r><w:delText>deleted words</w:delText></w:r></w:del><w:r><w:t>kept</w:t></w:r></w:p>
      <w:p><m:oMath><m:r><m:t>T=2π</m:t></m:r></m:oMath></w:p>
    </w:body></w:document>"#;
    let paras = docx::parse_document_xml(xml).unwrap();
    let all: Vec<&str> = paras.iter().map(|p| p.text.as_str()).collect();
    assert_eq!(all.iter().filter(|t| t.contains("Box text")).count(), 1, "{all:?}");
    assert!(all.contains(&"Visible sentence"), "{all:?}");
    assert!(all.contains(&"kept"));
    assert!(!all.iter().any(|t| t.contains("deleted")));
    assert!(all.contains(&"T=2π"));
}

#[test]
fn pdf_text_extracted() {
    let doc = parse::parse_bytes(&sample("resistance-wire.pdf"), FileKind::Pdf).unwrap();
    let text = doc.full_text();
    assert!(text.contains("constantan"), "{text}");
    assert!(text.contains("Research question"), "{text}");
}

#[test]
fn rejects_wrong_or_damaged_files() {
    assert_eq!(parse::parse_bytes(b"hello", FileKind::Docx).unwrap_err(), ParseError::NotADocx);
    assert_eq!(parse::parse_bytes(b"PK\x03\x04garbage", FileKind::Docx).unwrap_err(), ParseError::NotADocx);
    assert_eq!(parse::parse_bytes(b"%PDF-1.4 garbage", FileKind::Pdf).unwrap_err(), ParseError::Corrupt);
    assert_eq!(parse::parse_bytes(b"not a pdf", FileKind::Pdf).unwrap_err(), ParseError::Corrupt);
    assert!(parse::kind_from_path(std::path::Path::new("ia.DOCX")) == Some(FileKind::Docx));
    assert!(parse::kind_from_path(std::path::Path::new("ia.txt")).is_none());
    let big = vec![b'a'; parse::MAX_FILE_BYTES + 1];
    assert_eq!(parse::parse_bytes(&big, FileKind::Text).unwrap_err(), ParseError::TooLarge);
}

#[test]
fn sections_are_detected() {
    let doc = parse::parse_bytes(&sample("resistance-wire.docx"), FileKind::Docx).unwrap();
    let s = sections::split(&doc.paragraphs);
    let detected = s.detected();
    for id in ["background", "research_question", "hypothesis", "variables", "method", "data", "analysis", "conclusion", "evaluation", "references"] {
        assert!(detected.contains(id), "missing {id}: {detected:?}");
    }
    // The RQ line sits inside the Introduction but is also labelled research_question.
    assert!(s.text_for(&["research_question".into()]).contains("0.38 mm diameter"));
    // References are never part of the payload text.
    assert!(!s.full_text_for_payload().contains("Supplier data sheet"));
    assert!(s.text_for(&["evaluation".into()]).contains("crocodile clips"));
    assert!(!s.text_for(&["evaluation".into()]).contains("Resistance increases linearly"));
}

#[test]
fn heading_classification() {
    assert_eq!(sections::classify_heading("Data analysis"), Some("analysis"));
    assert_eq!(sections::classify_heading("Evaluation of uncertainties"), Some("evaluation"));
    assert_eq!(sections::classify_heading("Aims"), Some("background"));
    assert_eq!(sections::classify_heading("A claim"), None);
}
