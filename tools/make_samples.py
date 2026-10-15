"""Generate synthetic sample IAs as .docx and .pdf from samples/source/*.md.

Standard library only (no third-party packages), so it is easy to audit.
All sample content is SYNTHETIC: written for this repository, not real student work.

Usage:  python tools/make_samples.py
Output: samples/generated/<name>.docx and samples/generated/<name>.pdf

The .docx gets a page HEADER containing the student's name, so tests can
prove that headers are never read. Markdown '#' lines become Heading 1
paragraphs, '| a | b |' lines become table rows.
"""

import pathlib
import zipfile
from xml.sax.saxutils import escape

ROOT = pathlib.Path(__file__).resolve().parent.parent
SRC = ROOT / "samples" / "source"
OUT = ROOT / "samples" / "generated"

W_NS = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" ' \
       'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"'


def para(text, style=None):
    ppr = f'<w:pPr><w:pStyle w:val="{style}"/></w:pPr>' if style else ""
    return f'<w:p>{ppr}<w:r><w:t xml:space="preserve">{escape(text)}</w:t></w:r></w:p>'


def table(rows):
    out = ["<w:tbl>"]
    for row in rows:
        out.append("<w:tr>")
        for cell in row:
            out.append(f"<w:tc>{para(cell)}</w:tc>")
        out.append("</w:tr>")
    out.append("</w:tbl>")
    return "".join(out)


def md_blocks(md):
    """Yield ('h', text) | ('p', text) | ('table', rows)."""
    rows = []
    for line in md.splitlines():
        line = line.strip()
        if line.startswith("|"):
            rows.append([c.strip() for c in line.strip("|").split("|")])
            continue
        if rows:
            yield ("table", rows)
            rows = []
        if not line:
            continue
        if line.startswith("#"):
            yield ("h", line.lstrip("#").strip())
        else:
            yield ("p", line)
    if rows:
        yield ("table", rows)


def make_docx(md, header_text, path):
    body = []
    for kind, value in md_blocks(md):
        if kind == "h":
            body.append(para(value, "Heading1"))
        elif kind == "p":
            body.append(para(value))
        else:
            body.append(table(value))
    sect = '<w:sectPr><w:headerReference w:type="default" r:id="rIdHeader1"/></w:sectPr>'
    document = f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?>' \
               f'<w:document {W_NS}><w:body>{"".join(body)}{sect}</w:body></w:document>'
    header = f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?>' \
             f'<w:hdr {W_NS}>{para(header_text)}</w:hdr>'
    content_types = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
        '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
        '<Default Extension="xml" ContentType="application/xml"/>'
        '<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>'
        '<Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/>'
        '<Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/>'
        '</Types>')
    rels = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
        '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>'
        '<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/>'
        '</Relationships>')
    doc_rels = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
        '<Relationship Id="rIdHeader1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/>'
        '</Relationships>')
    # Metadata deliberately contains the name too: the parser must never read it.
    core = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        '<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" '
        'xmlns:dc="http://purl.org/dc/elements/1.1/">'
        f'<dc:creator>{escape(header_text)}</dc:creator></cp:coreProperties>')
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("[Content_Types].xml", content_types)
        z.writestr("_rels/.rels", rels)
        z.writestr("word/document.xml", document)
        z.writestr("word/_rels/document.xml.rels", doc_rels)
        z.writestr("word/header1.xml", header)
        z.writestr("docProps/core.xml", core)


# --- minimal PDF writer (Helvetica, WinAnsi) --------------------------------

def pdf_escape(s):
    # Map a few characters to WinAnsi; anything else unsupported becomes '?'.
    table_ = {"π": "pi", "√": "sqrt", "²": "\xb2", "⁻": "-", "¹": "\xb9", "⁶": "^6", "⁷": "^7",
              "Ω": "Ohm", "ρ": "rho", "±": "\xb1", "×": "\xd7", "é": "\xe9", "á": "\xe1"}
    out = []
    for ch in s:
        ch = table_.get(ch, ch)
        for c in ch:
            if c in "()\\":
                out.append("\\" + c)
            elif ord(c) < 256:
                out.append(c)
            else:
                out.append("?")
    return "".join(out)


def wrap(text, width=90):
    words, lines, cur = text.split(), [], ""
    for w in words:
        if len(cur) + len(w) + 1 > width:
            lines.append(cur)
            cur = w
        else:
            cur = f"{cur} {w}".strip()
    if cur:
        lines.append(cur)
    return lines


def make_pdf(md, path):
    lines = []
    for kind, value in md_blocks(md):
        if kind == "table":
            lines.extend("| " + " | ".join(r) + " |" for r in value)
            lines.append("")
        elif kind == "h":
            lines.extend(["", value, ""])
        else:
            lines.extend(wrap(value))
            lines.append("")
    per_page = 50
    pages = [lines[i:i + per_page] for i in range(0, len(lines), per_page)]

    objects = []  # index i -> object number i+1

    def add(obj):
        objects.append(obj)
        return len(objects)

    font_id = add(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>")
    pages_id_placeholder = add(b"")  # filled later
    page_ids = []
    for page in pages:
        ops = ["BT", "/F1 10 Tf", "13 TL", "50 790 Td"]
        for ln in page:
            ops.append(f"({pdf_escape(ln)}) Tj T*")
        ops.append("ET")
        stream = "\n".join(ops).encode("latin-1")
        content_id = add(b"<< /Length %d >>\nstream\n" % len(stream) + stream + b"\nendstream")
        page_ids.append(add(
            f"<< /Type /Page /Parent {pages_id_placeholder} 0 R /MediaBox [0 0 595 842] "
            f"/Resources << /Font << /F1 {font_id} 0 R >> >> /Contents {content_id} 0 R >>".encode()))
    kids = " ".join(f"{p} 0 R" for p in page_ids)
    objects[pages_id_placeholder - 1] = f"<< /Type /Pages /Kids [{kids}] /Count {len(page_ids)} >>".encode()
    catalog_id = add(f"<< /Type /Catalog /Pages {pages_id_placeholder} 0 R >>".encode())

    out = bytearray(b"%PDF-1.4\n")
    offsets = []
    for i, obj in enumerate(objects, start=1):
        offsets.append(len(out))
        out += f"{i} 0 obj\n".encode() + obj + b"\nendobj\n"
    xref = len(out)
    out += f"xref\n0 {len(objects) + 1}\n0000000000 65535 f \n".encode()
    for off in offsets:
        out += f"{off:010d} 00000 n \n".encode()
    out += f"trailer\n<< /Size {len(objects) + 1} /Root {catalog_id} 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode()
    path.write_bytes(bytes(out))


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    for src in sorted(SRC.glob("*.md")):
        md = src.read_text(encoding="utf-8")
        first = md.splitlines()[0]
        header_text = first.split(":", 1)[1].strip() + " - Physics IA draft"
        make_docx(md, header_text, OUT / f"{src.stem}.docx")
        make_pdf(md, OUT / f"{src.stem}.pdf")
        print("wrote", src.stem)


if __name__ == "__main__":
    main()
