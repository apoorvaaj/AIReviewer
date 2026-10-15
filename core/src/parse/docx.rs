//! .docx body-text extraction.
//!
//! A .docx is a zip archive. We open exactly ONE entry, `word/document.xml`
//! (the body). Headers (`word/header*.xml`), footers, footnotes, comments and
//! `docProps/*` (metadata such as author) are never opened.
//!
//! Inside the body we keep: paragraph text (`w:t`), equation text (`m:t`),
//! heading styles, and table rows flattened to "cell | cell | cell".
//! We skip `mc:Fallback` blocks (duplicate copies of text-box content) and
//! never read deleted tracked-change text (`w:delText`) or field codes.

use std::io::Read;

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use super::{Paragraph, ParseError};

const MAX_XML_BYTES: u64 = 50 * 1024 * 1024; // zip-bomb guard

pub fn parse(bytes: &[u8]) -> Result<Vec<Paragraph>, ParseError> {
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|_| ParseError::NotADocx)?;
    let file = archive
        .by_name("word/document.xml")
        .map_err(|_| ParseError::NotADocx)?;
    if file.size() > MAX_XML_BYTES {
        return Err(ParseError::TooLarge);
    }
    let mut xml = String::new();
    file.take(MAX_XML_BYTES)
        .read_to_string(&mut xml)
        .map_err(|_| ParseError::Corrupt)?;
    parse_document_xml(&xml)
}

pub fn parse_document_xml(xml: &str) -> Result<Vec<Paragraph>, ParseError> {
    let mut reader = Reader::from_str(xml);
    let mut out: Vec<Paragraph> = Vec::new();

    // Paragraphs can nest (text boxes inside a paragraph), so keep a stack.
    let mut open: Vec<(String, Option<u8>)> = Vec::new();
    let mut in_text = false;
    let mut fallback_depth = 0usize;
    let mut table_depth = 0usize;
    let mut cell = String::new();
    let mut row: Vec<String> = Vec::new();

    loop {
        let event = reader.read_event().map_err(|_| ParseError::Corrupt)?;
        match event {
            Event::Eof => break,
            Event::Start(e) => {
                let name = e.name();
                let n = name.as_ref();
                if fallback_depth > 0 {
                    if n == b"mc:Fallback" {
                        fallback_depth += 1;
                    }
                    continue;
                }
                match n {
                    b"mc:Fallback" => fallback_depth = 1,
                    b"w:t" | b"m:t" => in_text = true,
                    b"w:tbl" => table_depth += 1,
                    b"w:p" => open.push((String::new(), None)),
                    b"w:pStyle" => set_heading(&mut open, &e),
                    _ => {}
                }
            }
            Event::Empty(e) => {
                if fallback_depth > 0 {
                    continue;
                }
                match e.name().as_ref() {
                    b"w:tab" | b"w:br" | b"w:cr" => push_text(&mut open, " "),
                    b"w:noBreakHyphen" => push_text(&mut open, "-"),
                    b"w:pStyle" => set_heading(&mut open, &e),
                    _ => {}
                }
            }
            Event::Text(t) => {
                if in_text && fallback_depth == 0 {
                    let s = t.unescape().map_err(|_| ParseError::Corrupt)?;
                    push_text(&mut open, &s);
                }
            }
            Event::End(e) => {
                let name = e.name();
                let n = name.as_ref();
                if fallback_depth > 0 {
                    if n == b"mc:Fallback" {
                        fallback_depth -= 1;
                    }
                    continue;
                }
                match n {
                    b"w:t" | b"m:t" => in_text = false,
                    b"w:p" => {
                        if let Some((raw, heading)) = open.pop() {
                            let text = collapse_whitespace(&raw);
                            if text.is_empty() {
                                continue;
                            }
                            if table_depth > 0 {
                                if !cell.is_empty() {
                                    cell.push(' ');
                                }
                                cell.push_str(&text);
                            } else {
                                out.push(Paragraph { text, heading_level: heading });
                            }
                        }
                    }
                    b"w:tc" if table_depth > 0 => row.push(std::mem::take(&mut cell)),
                    b"w:tr" if table_depth > 0 => {
                        if row.iter().any(|c| !c.trim().is_empty()) {
                            out.push(Paragraph { text: row.join(" | "), heading_level: None });
                        }
                        row.clear();
                    }
                    b"w:tbl" => table_depth = table_depth.saturating_sub(1),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

fn push_text(open: &mut [(String, Option<u8>)], s: &str) {
    if let Some((buf, _)) = open.last_mut() {
        buf.push_str(s);
    }
}

fn set_heading(open: &mut [(String, Option<u8>)], e: &BytesStart) {
    let Some((_, heading)) = open.last_mut() else { return };
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() != b"w:val" {
            continue;
        }
        let val = String::from_utf8_lossy(&attr.value).to_ascii_lowercase();
        if val == "title" {
            *heading = Some(1);
        } else if let Some(rest) = val.strip_prefix("heading") {
            *heading = Some(rest.trim().parse::<u8>().unwrap_or(1));
        }
    }
}

fn collapse_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}
