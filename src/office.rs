//! "In MS Excel" and "In MS PowerPoint" (ribbon tab "Umwandeln"): the page
//! text is split into positioned pieces, arranged as table cells or as text
//! boxes on slides, and written as Office Open XML files. MrPDF writes the
//! files itself; no Office program is needed.

use std::path::Path;

use crate::convert::{ZipWriter, xml_escape};
use crate::document::{Document, write_atomic};
use crate::error::{Error, Result};
use crate::geometry::RectF;
use crate::model::LinkTarget;
use crate::text::PageText;

/// A run of text on one line, separated from its neighbours by a gap.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub text: String,
    /// PDF space.
    pub rect: RectF,
    /// Average character height (about the font size).
    pub size: f32,
    /// Font and colour of its first character (styled page text only).
    pub look: Option<Look>,
    /// Indices of its first and last character in the page text.
    pub chars: (usize, usize),
}

/// How a text looks: font family as installed on computers, style, colour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Look {
    pub family: String,
    pub bold: bool,
    pub italic: bool,
    pub color: [u8; 3],
}

/// Turns a PDF font name ("ABCDEF+TimesNewRomanPS-BoldItalicMT") into the
/// family name programs know ("Times New Roman") and its style.
pub fn font_family(pdf_name: &str) -> (String, bool, bool) {
    let name = match pdf_name.split_once('+') {
        Some((prefix, rest)) if prefix.len() == 6 && prefix.chars().all(|c| c.is_ascii_uppercase()) => rest,
        _ => pdf_name,
    };
    let (base, style) = match name.split_once([',', '-']) {
        Some((b, s)) => (b, s.to_ascii_lowercase()),
        None => (name, String::new()),
    };
    let lower = name.to_ascii_lowercase();
    let bold = style.contains("bold")
        || lower.contains("bold")
        || style.contains("black")
        || style.contains("heavy");
    let italic = style.contains("italic") || style.contains("oblique") || lower.contains("italic");
    let mut base = base.to_string();
    for suffix in ["PSMT", "MT", "PS"] {
        if base.len() > suffix.len() + 2 && base.ends_with(suffix) {
            base.truncate(base.len() - suffix.len());
            break;
        }
    }
    let known = match base.to_ascii_lowercase().as_str() {
        "helvetica" | "arial" | "arialnarrow" => Some("Arial"),
        "times" | "timesnewroman" | "timesroman" => Some("Times New Roman"),
        "courier" | "couriernew" => Some("Courier New"),
        "symbol" => Some("Symbol"),
        "zapfdingbats" => Some("Wingdings"),
        _ => None,
    };
    let family = match known {
        Some(k) => k.to_string(),
        None => {
            // "SegoeUI" -> "Segoe UI", "OpenSans" -> "Open Sans".
            let mut out = String::new();
            let mut prev_lower = false;
            for c in base.chars() {
                if c.is_ascii_uppercase() && prev_lower {
                    out.push(' ');
                }
                prev_lower = c.is_ascii_lowercase();
                out.push(c);
            }
            if out.trim().is_empty() { "Arial".into() } else { out }
        }
    };
    (family, bold, italic)
}

fn look_of(page: &PageText, index: usize) -> Option<Look> {
    let style = page.styles.get(index)?;
    let font = page.fonts.get(usize::from(style.font))?;
    let (family, bold, italic) = font_family(&font.name);
    Some(Look { family, bold: bold || font.bold, italic: italic || font.italic, color: style.color })
}

/// The text of a page as lines (top to bottom) of segments (left to right).
pub fn segments(page: &PageText) -> Vec<Vec<Segment>> {
    struct Open {
        text: String,
        rect: RectF,
        heights: f32,
        count: f32,
        first: usize,
        last: usize,
    }
    let mut lines: Vec<Vec<Segment>> = Vec::new();
    let mut line: Vec<Segment> = Vec::new();
    let mut cur: Option<Open> = None;
    let close = |cur: &mut Option<Open>, line: &mut Vec<Segment>| {
        if let Some(o) = cur.take() {
            let text = o.text.split_whitespace().collect::<Vec<_>>().join(" ");
            if !text.is_empty() {
                line.push(Segment {
                    text,
                    rect: o.rect,
                    size: o.heights / o.count.max(1.0),
                    look: look_of(page, o.first),
                    chars: (o.first, o.last),
                });
            }
        }
    };
    let end_line = |cur: &mut Option<Open>, line: &mut Vec<Segment>, lines: &mut Vec<Vec<Segment>>| {
        close(cur, line);
        if !line.is_empty() {
            lines.push(std::mem::take(line));
        }
    };
    for (index, c) in page.chars.iter().enumerate() {
        if c.ch == '\n' || c.ch == '\r' {
            end_line(&mut cur, &mut line, &mut lines);
            continue;
        }
        if c.ch.is_control() {
            continue;
        }
        if c.generated || c.rect.is_empty() {
            if let Some(o) = cur.as_mut() {
                o.text.push(' ');
            }
            continue;
        }
        let r = c.rect.normalized();
        let h = r.height().max(1.0);
        if let Some(o) = cur.as_mut() {
            let same_line = (r.center().y - o.rect.center().y).abs() < h * 0.6;
            let gap = r.x0 - o.rect.x1;
            if !same_line {
                end_line(&mut cur, &mut line, &mut lines);
            } else if gap > h * 1.2
                || gap < -h * 2.0
                || (!c.ch.is_whitespace()
                    && match (page.styles.get(index), page.styles.get(o.first)) {
                        (Some(a), Some(b)) => !a.same_look(b),
                        _ => false,
                    })
            {
                // A gap, or another font or colour (styled text only).
                close(&mut cur, &mut line);
            } else {
                if c.ch.is_whitespace() {
                    o.text.push(' ');
                } else {
                    if gap > h * 0.25 && !o.text.ends_with(' ') {
                        o.text.push(' ');
                    }
                    o.text.push(c.ch);
                    o.heights += h;
                    o.count += 1.0;
                    o.last = index;
                }
                o.rect = RectF::new(
                    o.rect.x0.min(r.x0),
                    o.rect.y0.min(r.y0),
                    o.rect.x1.max(r.x1),
                    o.rect.y1.max(r.y1),
                );
                continue;
            }
        }
        if c.ch.is_whitespace() {
            continue;
        }
        cur =
            Some(Open { text: c.ch.to_string(), rect: r, heights: h, count: 1.0, first: index, last: index });
    }
    end_line(&mut cur, &mut line, &mut lines);
    // Lines in reading order may jump; sort top to bottom and join lines on
    // the same height (e.g. table cells extracted column by column).
    lines.sort_by(|a, b| b[0].rect.center().y.total_cmp(&a[0].rect.center().y));
    let mut merged: Vec<Vec<Segment>> = Vec::new();
    for l in lines {
        match merged.last_mut() {
            Some(m)
                if (m[0].rect.center().y - l[0].rect.center().y).abs() < m[0].size.min(l[0].size) * 0.5 =>
            {
                m.extend(l);
            }
            _ => merged.push(l),
        }
    }
    for l in &mut merged {
        l.sort_by(|a, b| a.rect.x0.total_cmp(&b.rect.x0));
    }
    merged
}

// ---------------------------------------------------------------------------
// Excel

/// A cell of a worksheet.
#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    Text(String),
    Number(f64),
}

/// How numbers are written in the document.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NumberFormat {
    pub decimal: char,
    pub thousands: Option<char>,
}

impl NumberFormat {
    pub const GERMAN: NumberFormat = NumberFormat { decimal: ',', thousands: Some('.') };
    pub const ENGLISH: NumberFormat = NumberFormat { decimal: '.', thousands: Some(',') };
}

/// Reads "1.234,56", "-12", "3,5 %" (as 0.035) … in `fmt`; None for text.
pub fn parse_number(s: &str, fmt: NumberFormat) -> Option<f64> {
    let t = s.trim();
    let (t, percent) = match t.strip_suffix('%') {
        Some(rest) => (rest.trim_end(), true),
        None => (t, false),
    };
    if t.is_empty() || t.len() > 40 {
        return None;
    }
    let mut out = String::new();
    let mut seen_decimal = false;
    let mut digits_since_sep: Option<usize> = None;
    for (i, c) in t.chars().enumerate() {
        match c {
            '-' | '+' if i == 0 => out.push(c),
            '0'..='9' => {
                out.push(c);
                if let Some(n) = digits_since_sep.as_mut() {
                    *n += 1;
                }
            }
            c if c == fmt.decimal && !seen_decimal => {
                if digits_since_sep.is_some_and(|n| n != 3) {
                    return None;
                }
                seen_decimal = true;
                digits_since_sep = None;
                out.push('.');
            }
            c if Some(c) == fmt.thousands && !seen_decimal => {
                if digits_since_sep.is_some_and(|n| n != 3) || out.trim_start_matches(['-', '+']).is_empty() {
                    return None;
                }
                digits_since_sep = Some(0);
            }
            _ => return None,
        }
    }
    if digits_since_sep.is_some_and(|n| n != 3) || !out.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    let v: f64 = out.parse().ok()?;
    v.is_finite().then_some(if percent { v / 100.0 } else { v })
}

/// Arranges the lines of a page as rows and columns. With `detect` the
/// segments are put into columns by their left edges, otherwise each line
/// is one cell. With `paragraphs` neighbouring one-cell lines are joined.
pub fn table(lines: &[Vec<Segment>], detect: bool, paragraphs: bool) -> Vec<Vec<String>> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    if detect {
        let mut starts: Vec<f32> = lines.iter().flatten().map(|s| s.rect.x0).collect();
        starts.sort_by(f32::total_cmp);
        let mut columns: Vec<f32> = Vec::new();
        for x in starts {
            if columns.last().is_none_or(|c| x - c > 8.0) {
                columns.push(x);
            }
        }
        for line in lines {
            let mut row = vec![String::new(); columns.len()];
            for s in line {
                let col = columns.iter().rposition(|c| *c <= s.rect.x0 + 0.5).unwrap_or(0);
                if !row[col].is_empty() {
                    row[col].push(' ');
                }
                row[col].push_str(&s.text);
            }
            rows.push(row);
        }
        // Columns that stay empty are dropped.
        let used: Vec<bool> = (0..columns.len()).map(|c| rows.iter().any(|r| !r[c].is_empty())).collect();
        for r in &mut rows {
            let mut i = 0;
            r.retain(|_| {
                let keep = used[i];
                i += 1;
                keep
            });
        }
    } else {
        rows = lines
            .iter()
            .map(|l| vec![l.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ")])
            .collect();
    }
    if !paragraphs {
        return rows;
    }
    let mut out: Vec<Vec<String>> = Vec::new();
    let mut prev: Option<&Vec<Segment>> = None;
    for (row, line) in rows.into_iter().zip(lines) {
        let single = row.iter().filter(|c| !c.is_empty()).count() == 1;
        let close = prev.is_some_and(|p| {
            let gap = p[0].rect.y0 - line[0].rect.y1;
            gap < p[0].size * 0.8 && p.len() == 1 && line.len() == 1
        });
        match out.last_mut() {
            Some(last) if single && close && last.len() == row.len() => {
                for (a, b) in last.iter_mut().zip(row) {
                    if !b.is_empty() {
                        if !a.is_empty() {
                            a.push(' ');
                        }
                        a.push_str(&b);
                    }
                }
            }
            _ => out.push(row),
        }
        prev = Some(line);
    }
    out
}

// ---------------------------------------------------------------------------
// "Vor Änderung mit Kennwort schützen"

/// Rounds of the password hash (as Office uses).
pub const SPIN_COUNT: u32 = 100_000;

pub(crate) fn base64(data: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ABC[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Office "password to modify" hash: SHA-512 over salt and the UTF-16LE
/// password, then `spin` rounds over hash and round number. Returns
/// (hash, salt) as base64.
pub fn modify_hash(password: &str, salt: &[u8], spin: u32) -> (String, String) {
    use sha2::{Digest, Sha512};
    let mut h = Sha512::new();
    h.update(salt);
    for unit in password.encode_utf16() {
        h.update(unit.to_le_bytes());
    }
    let mut hash = h.finalize().to_vec();
    for i in 0..spin {
        let mut h = Sha512::new();
        h.update(&hash);
        h.update(i.to_le_bytes());
        hash = h.finalize().to_vec();
    }
    (base64(&hash), base64(salt))
}

/// (hash, salt) with a random salt.
fn protect(password: &str) -> (String, String) {
    let mut salt = [0u8; 16];
    if getrandom::fill(&mut salt).is_err() {
        // Without random numbers the time still makes the salt unique.
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        salt = t.to_le_bytes();
    }
    modify_hash(password, &salt, SPIN_COUNT)
}

/// `<w:writeProtection …/>` for word/settings.xml.
pub(crate) fn word_write_protection(password: &str) -> String {
    let (hash, salt) = protect(password);
    format!(
        "<w:writeProtection w:recommended=\"0\" w:algorithmName=\"SHA-512\" w:hashValue=\"{hash}\" w:saltValue=\"{salt}\" w:spinCount=\"{SPIN_COUNT}\"/>"
    )
}

fn column_name(mut n: usize) -> String {
    let mut s = Vec::new();
    loop {
        s.push(b'A' + (n % 26) as u8);
        if n < 26 {
            break;
        }
        n = n / 26 - 1;
    }
    s.reverse();
    String::from_utf8(s).unwrap_or_default()
}

/// Worksheet names: at most 31 characters, no []:*?/\, unique.
fn sheet_names(names: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for name in names {
        let base: String = name
            .chars()
            .filter(|c| !matches!(c, '[' | ']' | ':' | '*' | '?' | '/' | '\\'))
            .take(28)
            .collect();
        let base = if base.trim().is_empty() { "Tabelle".to_string() } else { base };
        let mut candidate = base.clone();
        let mut n = 2;
        while out.iter().any(|o| o.eq_ignore_ascii_case(&candidate)) {
            candidate = format!("{base} {n}");
            n += 1;
        }
        out.push(candidate);
    }
    out
}

/// An Excel workbook (.xlsx) with the given sheets; with `password` it can
/// only be changed with the password ("Schreibschutz-Kennwort").
pub fn xlsx(title: &str, sheets: &[(String, Vec<Vec<Option<Cell>>>)], password: Option<&str>) -> Vec<u8> {
    let names = sheet_names(&sheets.iter().map(|s| s.0.clone()).collect::<Vec<_>>());
    let mut zip = ZipWriter::default();
    let mut types = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/><Override PartName=\"/xl/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml\"/><Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/>",
    );
    let mut wb_sheets = String::new();
    let mut wb_rels = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rIdStyles\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/>",
    );
    for (i, ((_, rows), name)) in sheets.iter().zip(&names).enumerate() {
        let n = i + 1;
        types.push_str(&format!(
            "<Override PartName=\"/xl/worksheets/sheet{n}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>"
        ));
        wb_sheets
            .push_str(&format!("<sheet name=\"{}\" sheetId=\"{n}\" r:id=\"rId{n}\"/>", xml_escape(name)));
        wb_rels.push_str(&format!(
            "<Relationship Id=\"rId{n}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet{n}.xml\"/>"
        ));
        let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
        let mut cols = String::new();
        for c in 0..columns {
            let chars = rows
                .iter()
                .filter_map(|r| r.get(c).and_then(|v| v.as_ref()))
                .map(|v| match v {
                    Cell::Text(t) => t.chars().count(),
                    Cell::Number(n) => n.to_string().len(),
                })
                .max()
                .unwrap_or(8);
            let width = (chars as f32 * 1.1 + 2.0).clamp(6.0, 80.0);
            cols.push_str(&format!(
                "<col min=\"{0}\" max=\"{0}\" width=\"{width:.1}\" customWidth=\"1\"/>",
                c + 1
            ));
        }
        let mut data = String::new();
        for (r, row) in rows.iter().enumerate() {
            data.push_str(&format!("<row r=\"{}\">", r + 1));
            for (c, cell) in row.iter().enumerate() {
                let reference = format!("{}{}", column_name(c), r + 1);
                match cell {
                    Some(Cell::Number(v)) => data.push_str(&format!("<c r=\"{reference}\"><v>{v}</v></c>")),
                    Some(Cell::Text(t)) if !t.is_empty() => data.push_str(&format!(
                        "<c r=\"{reference}\" t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>",
                        xml_escape(t)
                    )),
                    _ => {}
                }
            }
            data.push_str("</row>");
        }
        let cols = if cols.is_empty() { String::new() } else { format!("<cols>{cols}</cols>") };
        let sheet = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">{cols}<sheetData>{data}</sheetData></worksheet>"
        );
        zip.add(&format!("xl/worksheets/sheet{n}.xml"), sheet.as_bytes());
    }
    types.push_str("</Types>");
    wb_rels.push_str("</Relationships>");
    let sharing = password.filter(|p| !p.is_empty()).map_or(String::new(), |p| {
        let (hash, salt) = protect(p);
        format!(
            "<fileSharing userName=\"MrPDF\" algorithmName=\"SHA-512\" hashValue=\"{hash}\" saltValue=\"{salt}\" spinCount=\"{SPIN_COUNT}\"/>"
        )
    });
    let workbook = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\">{sharing}<sheets>{wb_sheets}</sheets></workbook>"
    );
    let styles = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<styleSheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><fonts count=\"1\"><font><sz val=\"11\"/><name val=\"Calibri\"/></font></fonts><fills count=\"2\"><fill><patternFill patternType=\"none\"/></fill><fill><patternFill patternType=\"gray125\"/></fill></fills><borders count=\"1\"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs><cellXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\"/></cellXfs><cellStyles count=\"1\"><cellStyle name=\"Normal\" xfId=\"0\" builtinId=\"0\"/></cellStyles></styleSheet>";
    zip.add("[Content_Types].xml", types.as_bytes());
    zip.add("_rels/.rels", ROOT_RELS.replace("{main}", "xl/workbook.xml").as_bytes());
    zip.add("docProps/core.xml", core(title).as_bytes());
    zip.add("xl/workbook.xml", workbook.as_bytes());
    zip.add("xl/_rels/workbook.xml.rels", wb_rels.as_bytes());
    zip.add("xl/styles.xml", styles.as_bytes());
    zip.finish()
}

const ROOT_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"{main}\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/></Relationships>";

fn core(title: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:title>{}</dc:title><dc:creator>MrPDF</dc:creator></cp:coreProperties>",
        xml_escape(title)
    )
}

/// Options of "Einstellungen für Excel-Umwandlung".
#[derive(Debug, Clone, PartialEq)]
pub struct ExcelOptions {
    pub numbers: NumberFormat,
    /// "Tabellen erkennen".
    pub detect_tables: bool,
    /// "Arbeitsblatt erstellen: Für jede Seite" (false: one for the document).
    pub sheet_per_page: bool,
    /// "Neue Zeile in der Tabelle nutzen pro: Absatz" (false: Zeile).
    pub row_per_paragraph: bool,
    /// "Beschädigten Text mit Texterkennung verbessern".
    pub ocr: Option<crate::ocr::Engine>,
}

/// The page text; pages without usable text are recognised with OCR if
/// an engine is given.
fn text_for_export(
    doc: &Document,
    page: usize,
    styled: bool,
    ocr: Option<&crate::ocr::Engine>,
) -> Result<PageText> {
    let text = if styled { doc.page_text_styled(page)? } else { doc.page_text(page)? };
    let usable = text.chars.iter().filter(|c| c.ch.is_alphanumeric() && !c.generated).count();
    match ocr {
        Some(engine) if usable < 10 => doc.ocr_page_text(page, engine),
        _ => Ok(text),
    }
}

/// Writes `pages` of `doc` as Excel workbook; `password` protects it from
/// changes.
pub fn export_excel_protected(
    doc: &Document,
    dest: &Path,
    pages: &[usize],
    o: &ExcelOptions,
    password: Option<&str>,
) -> Result<()> {
    write_atomic(dest, &xlsx(doc.display_name(), &excel_sheets(doc, pages, o)?, password))
}

/// Writes `pages` of `doc` as Excel workbook.
pub fn export_excel(doc: &Document, dest: &Path, pages: &[usize], o: &ExcelOptions) -> Result<()> {
    export_excel_protected(doc, dest, pages, o, None)
}

type Sheets = Vec<(String, Vec<Vec<Option<Cell>>>)>;

fn excel_sheets(doc: &Document, pages: &[usize], o: &ExcelOptions) -> Result<Sheets> {
    let mut sheets: Vec<(String, Vec<Vec<Option<Cell>>>)> = Vec::new();
    let cell = |s: String| -> Option<Cell> {
        if s.is_empty() {
            None
        } else {
            Some(parse_number(&s, o.numbers).map_or(Cell::Text(s), Cell::Number))
        }
    };
    for &page in pages {
        let lines = segments(&text_for_export(doc, page, false, o.ocr.as_ref())?);
        let rows: Vec<Vec<Option<Cell>>> = table(&lines, o.detect_tables, o.row_per_paragraph)
            .into_iter()
            .map(|r| r.into_iter().map(cell).collect())
            .collect();
        if o.sheet_per_page || sheets.is_empty() {
            let name = if o.sheet_per_page { format!("Seite {}", page + 1) } else { "Tabelle1".into() };
            sheets.push((name, rows));
        } else if let Some(last) = sheets.last_mut() {
            last.1.push(Vec::new());
            last.1.extend(rows);
        }
    }
    if sheets.is_empty() {
        return Err(Error::Invalid("no pages".into()));
    }
    Ok(sheets)
}

// ---------------------------------------------------------------------------
// Word

/// Options of "Einstellungen für Word-Umwandlung".
#[derive(Debug, Clone, PartialEq)]
pub struct WordOptions {
    /// "Seitenlayout erhalten": every PDF page becomes a Word page of the
    /// same size with its graphics as picture and the text in positioned
    /// frames; otherwise the text flows ("Textfluss erhalten").
    pub keep_layout: bool,
    /// "Tabellen erkennen" (text flow).
    pub tables: bool,
    /// "Bilder übernehmen" (text flow).
    pub images: bool,
    /// Resolution of the page pictures in layout mode.
    pub dpi: f32,
    pub ocr: Option<crate::ocr::Engine>,
}

/// Lines with two or more segments in a row become tables.
fn flow_blocks(
    lines: Vec<Vec<Segment>>,
    tables: bool,
    heading: impl Fn(&str) -> bool,
) -> Vec<(f32, crate::convert::DocBlock)> {
    use crate::convert::DocBlock;
    let top = |l: &[Segment]| l.iter().map(|s| s.rect.y1).fold(f32::MIN, f32::max);
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let run_end = if tables {
            let mut j = i;
            while j < lines.len() && lines[j].len() >= 2 {
                j += 1;
            }
            j
        } else {
            i
        };
        if run_end - i >= 2 {
            let rows = table(&lines[i..run_end], true, false);
            out.push((top(&lines[i]), DocBlock::Table(rows)));
            i = run_end;
            continue;
        }
        let t = lines[i].iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ");
        let h = heading(&t);
        out.push((top(&lines[i]), DocBlock::Text(t, h)));
        i += 1;
    }
    out
}

/// Writes `pages` of `doc` as Word document.
pub fn export_word(
    doc: &mut Document,
    dest: &Path,
    pages: &[usize],
    o: &WordOptions,
    password: Option<&str>,
) -> Result<()> {
    if o.keep_layout {
        return export_word_layout(doc, dest, pages, o, password);
    }
    export_word_flow(doc, dest, pages, o, password)
}

/// "Textfluss erhalten".
pub fn export_word_flow(
    doc: &Document,
    dest: &Path,
    pages: &[usize],
    o: &WordOptions,
    password: Option<&str>,
) -> Result<()> {
    let headings: Vec<(usize, String)> =
        doc.heading_bookmarks()?.into_iter().map(|b| (b.page, b.title)).collect();
    let mut out = Vec::new();
    for &page in pages {
        let lines = segments(&text_for_export(doc, page, false, o.ocr.as_ref())?);
        let mut blocks = flow_blocks(lines, o.tables, |t| headings.iter().any(|(p, h)| *p == page && h == t));
        if o.images {
            for (rect, png) in doc.page_images(page)? {
                blocks.push((rect.y1, crate::convert::DocBlock::Image(png, rect.width(), rect.height())));
            }
        }
        blocks.sort_by(|a, b| b.0.total_cmp(&a.0));
        out.push(blocks.into_iter().map(|(_, b)| b).collect());
    }
    write_atomic(dest, &crate::convert::docx_protected(doc.display_name(), &out, password))
}

fn twips(pt: f32) -> i64 {
    (pt * 20.0).round() as i64
}

/// "Seitenlayout erhalten".
fn export_word_layout(
    doc: &mut Document,
    dest: &Path,
    pages: &[usize],
    o: &WordOptions,
    password: Option<&str>,
) -> Result<()> {
    let mut body = String::new();
    let mut images: Vec<Vec<u8>> = Vec::new();
    for (k, &page) in pages.iter().enumerate() {
        let geo = *doc.geometry(page).ok_or(Error::PageOutOfRange(page))?;
        let (w, h) = geo.view_size();
        let scale = (o.dpi / 72.0).clamp(0.5, 600.0 / 72.0);
        let px = |v: f32| ((v * scale).round() as u32).clamp(1, crate::document::MAX_BITMAP_EDGE);
        let img = doc.render_background(page, px(w), px(h), true, true)?;
        let mut png = Vec::new();
        image::RgbaImage::from_raw(img.width, img.height, img.rgba)
            .ok_or_else(|| Error::Invalid("image".into()))?
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .map_err(|e| Error::Invalid(e.to_string()))?;
        images.push(png);
        let n = images.len();
        let (cx, cy) = (emu(w), emu(h));
        body.push_str(&format!(
            "<w:p><w:pPr><w:spacing w:before=\"0\" w:after=\"0\" w:line=\"20\" w:lineRule=\"exact\"/></w:pPr><w:r><w:drawing><wp:anchor distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\" simplePos=\"0\" relativeHeight=\"0\" behindDoc=\"1\" locked=\"1\" layoutInCell=\"1\" allowOverlap=\"1\"><wp:simplePos x=\"0\" y=\"0\"/><wp:positionH relativeFrom=\"page\"><wp:posOffset>0</wp:posOffset></wp:positionH><wp:positionV relativeFrom=\"page\"><wp:posOffset>0</wp:posOffset></wp:positionV><wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:effectExtent l=\"0\" t=\"0\" r=\"0\" b=\"0\"/><wp:wrapNone/><wp:docPr id=\"{n}\" name=\"Seite {n}\"/><wp:cNvGraphicFramePr/>{}</wp:anchor></w:drawing></w:r></w:p>",
            crate::convert::docx_picture(n, cx, cy)
        ));
        let text = text_for_export(doc, page, true, o.ocr.as_ref())?;
        let recognised = text.styles.is_empty() && !text.chars.is_empty() && o.ocr.is_some();
        for line in segments(&text) {
            for s in line {
                let v = geo.pdf_rect_to_view(&s.rect);
                let size = text.styles.get(s.chars.0).map_or(s.size.max(1.0) * 0.85, |st| st.size.max(1.0));
                let mut rpr = String::new();
                if let Some(l) = &s.look {
                    let f = xml_escape(&l.family);
                    rpr.push_str(&format!("<w:rFonts w:ascii=\"{f}\" w:hAnsi=\"{f}\" w:cs=\"{f}\"/>"));
                    if l.bold {
                        rpr.push_str("<w:b/>");
                    }
                    if l.italic {
                        rpr.push_str("<w:i/>");
                    }
                    rpr.push_str(&format!(
                        "<w:color w:val=\"{:02X}{:02X}{:02X}\"/>",
                        l.color[0], l.color[1], l.color[2]
                    ));
                }
                if recognised {
                    rpr.push_str("<w:vanish/>");
                }
                let half_points = ((size * 2.0).round() as i64).clamp(2, 3276);
                rpr.push_str(&format!("<w:sz w:val=\"{half_points}\"/>"));
                let fh = (v.height().max(size * 1.2)) * 1.1;
                body.push_str(&format!(
                    "<w:p><w:pPr><w:framePr w:w=\"{}\" w:h=\"{}\" w:hRule=\"exact\" w:x=\"{}\" w:y=\"{}\" w:hAnchor=\"page\" w:vAnchor=\"page\" w:wrap=\"none\"/><w:spacing w:before=\"0\" w:after=\"0\" w:line=\"{}\" w:lineRule=\"exact\"/></w:pPr><w:r><w:rPr>{rpr}</w:rPr><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>",
                    twips(v.width() * 1.15 + size),
                    twips(fh),
                    twips(v.x0),
                    twips(v.y0 - size * 0.2),
                    twips(fh),
                    xml_escape(&s.text)
                ));
            }
        }
        let sect = format!(
            "<w:sectPr><w:pgSz w:w=\"{}\" w:h=\"{}\"/><w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/></w:sectPr>",
            twips(w),
            twips(h)
        );
        if k + 1 < pages.len() {
            body.push_str(&format!("<w:p><w:pPr>{sect}</w:pPr></w:p>"));
        } else {
            body.push_str(&sect);
        }
    }
    if pages.is_empty() {
        return Err(Error::Invalid("no pages".into()));
    }
    let refs: Vec<&[u8]> = images.iter().map(Vec::as_slice).collect();
    write_atomic(dest, &crate::convert::docx_package(doc.display_name(), &body, &refs, password))
}

// ---------------------------------------------------------------------------
// PowerPoint

/// A text box on a slide (points from the top left of the page).
#[derive(Debug, Clone, PartialEq)]
pub struct SlideText {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub text: String,
    pub size: f32,
    /// Original font and colour (Arial black without).
    pub look: Option<Look>,
    /// "In Kurven umwandeln": the glyph outlines (slide points) shown as a
    /// shape instead of the editable text.
    pub outline: Option<Vec<PathOp>>,
    /// Text recognised on a scan: transparent over the picture.
    pub invisible: bool,
}

/// A step of an outline path.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathOp {
    Move(f32, f32),
    Line(f32, f32),
    Cubic(f32, f32, f32, f32, f32, f32),
    Close,
}

/// One slide: page size in points, background picture (PNG) and text.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Slide {
    pub width: f32,
    pub height: f32,
    pub background: Option<Vec<u8>>,
    pub texts: Vec<SlideText>,
    /// Web links: (x, y, w, h, address).
    pub links: Vec<(f32, f32, f32, f32, String)>,
}

const EMU: f32 = 12_700.0;

fn emu(pt: f32) -> i64 {
    (pt * EMU).round() as i64
}

const THEME: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<a:theme xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" name=\"MrPDF\"><a:themeElements><a:clrScheme name=\"MrPDF\"><a:dk1><a:srgbClr val=\"000000\"/></a:dk1><a:lt1><a:srgbClr val=\"FFFFFF\"/></a:lt1><a:dk2><a:srgbClr val=\"1F1F1F\"/></a:dk2><a:lt2><a:srgbClr val=\"EEEEEE\"/></a:lt2><a:accent1><a:srgbClr val=\"1E64DC\"/></a:accent1><a:accent2><a:srgbClr val=\"E53935\"/></a:accent2><a:accent3><a:srgbClr val=\"2EA043\"/></a:accent3><a:accent4><a:srgbClr val=\"FB8C00\"/></a:accent4><a:accent5><a:srgbClr val=\"8E24AA\"/></a:accent5><a:accent6><a:srgbClr val=\"00ACC1\"/></a:accent6><a:hlink><a:srgbClr val=\"0563C1\"/></a:hlink><a:folHlink><a:srgbClr val=\"954F72\"/></a:folHlink></a:clrScheme><a:fontScheme name=\"MrPDF\"><a:majorFont><a:latin typeface=\"Arial\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:majorFont><a:minorFont><a:latin typeface=\"Arial\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:minorFont></a:fontScheme><a:fmtScheme name=\"MrPDF\"><a:fillStyleLst><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:fillStyleLst><a:lnStyleLst><a:ln w=\"6350\"><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:ln><a:ln w=\"12700\"><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:ln><a:ln w=\"19050\"><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:ln></a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst><a:bgFillStyleLst><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>";

const NS: &str = "xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\"";

const EMPTY_TREE: &str = "<p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/></p:spTree></p:cSld>";

/// A filled shape from outline paths (slide points), drawn at the place of
/// the text.
fn outline_shape(id: usize, ops: &[PathOp], scale: f32, ox: f32, oy: f32, fill: &str) -> String {
    let mut xs = Vec::new();
    let mut ys = Vec::new();
    for op in ops {
        match *op {
            PathOp::Move(x, y) | PathOp::Line(x, y) => {
                xs.push(x);
                ys.push(y);
            }
            PathOp::Cubic(a, b, c, d, x, y) => {
                xs.extend([a, c, x]);
                ys.extend([b, d, y]);
            }
            PathOp::Close => {}
        }
    }
    let (x0, x1) = xs.iter().fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
    let (y0, y1) = ys.iter().fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
    let (w, h) = ((x1 - x0).max(0.1), (y1 - y0).max(0.1));
    let pt =
        |x: f32, y: f32| format!("<a:pt x=\"{}\" y=\"{}\"/>", emu((x - x0) * scale), emu((y - y0) * scale));
    let mut path = String::new();
    for op in ops {
        match *op {
            PathOp::Move(x, y) => path.push_str(&format!("<a:moveTo>{}</a:moveTo>", pt(x, y))),
            PathOp::Line(x, y) => path.push_str(&format!("<a:lnTo>{}</a:lnTo>", pt(x, y))),
            PathOp::Cubic(a, b, c, d, x, y) => {
                path.push_str(&format!("<a:cubicBezTo>{}{}{}</a:cubicBezTo>", pt(a, b), pt(c, d), pt(x, y)))
            }
            PathOp::Close => path.push_str("<a:close/>"),
        }
    }
    let (cx, cy) = (emu(w * scale).max(1), emu(h * scale).max(1));
    format!(
        "<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"Kurven {id}\"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:custGeom><a:avLst/><a:gdLst/><a:ahLst/><a:cxnLst/><a:rect l=\"0\" t=\"0\" r=\"r\" b=\"b\"/><a:pathLst><a:path w=\"{cx}\" h=\"{cy}\">{path}</a:path></a:pathLst></a:custGeom>{fill}<a:ln><a:noFill/></a:ln></p:spPr></p:sp>",
        emu(ox + x0 * scale),
        emu(oy + y0 * scale)
    )
}

/// A PowerPoint presentation (.pptx) with one slide per entry; with
/// `password` it can only be changed with the password.
pub fn pptx(title: &str, slides: &[Slide], password: Option<&str>) -> Vec<u8> {
    let (sw, sh) = slides.first().map_or((960.0, 540.0), |s| (s.width.max(72.0), s.height.max(72.0)));
    let mut zip = ZipWriter::default();
    let mut types = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Default Extension=\"png\" ContentType=\"image/png\"/><Override PartName=\"/ppt/presentation.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml\"/><Override PartName=\"/ppt/slideMasters/slideMaster1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml\"/><Override PartName=\"/ppt/slideLayouts/slideLayout1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml\"/><Override PartName=\"/ppt/theme/theme1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.theme+xml\"/><Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/>",
    );
    let mut sld_ids = String::new();
    let mut pres_rels = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rIdMaster\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster\" Target=\"slideMasters/slideMaster1.xml\"/><Relationship Id=\"rIdTheme\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme\" Target=\"theme/theme1.xml\"/>",
    );
    for (i, slide) in slides.iter().enumerate() {
        let n = i + 1;
        types.push_str(&format!(
            "<Override PartName=\"/ppt/slides/slide{n}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slide+xml\"/>"
        ));
        sld_ids.push_str(&format!("<p:sldId id=\"{}\" r:id=\"rId{n}\"/>", 255 + n));
        pres_rels.push_str(&format!(
            "<Relationship Id=\"rId{n}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide\" Target=\"slides/slide{n}.xml\"/>"
        ));
        // Pages of another size are scaled into the slide and centred.
        let scale = (sw / slide.width.max(1.0)).min(sh / slide.height.max(1.0));
        let (ox, oy) = ((sw - slide.width * scale) / 2.0, (sh - slide.height * scale) / 2.0);
        let mut rels = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rIdLayout\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout\" Target=\"../slideLayouts/slideLayout1.xml\"/>",
        );
        let mut shapes = String::new();
        let mut id = 2;
        if let Some(png) = &slide.background {
            zip.add(&format!("ppt/media/image{n}.png"), png);
            rels.push_str(&format!(
                "<Relationship Id=\"rIdImg\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"../media/image{n}.png\"/>"
            ));
            shapes.push_str(&format!(
                "<p:pic><p:nvPicPr><p:cNvPr id=\"{id}\" name=\"Seite\"/><p:cNvPicPr><a:picLocks noChangeAspect=\"1\"/></p:cNvPicPr><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed=\"rIdImg\"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr><a:xfrm><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr></p:pic>",
                emu(ox),
                emu(oy),
                emu(slide.width * scale),
                emu(slide.height * scale)
            ));
            id += 1;
        }
        for t in &slide.texts {
            let color = t.look.as_ref().map_or([0, 0, 0], |l| l.color);
            let alpha = if t.invisible { "<a:alpha val=\"0\"/>" } else { "" };
            let fill = format!(
                "<a:solidFill><a:srgbClr val=\"{:02X}{:02X}{:02X}\">{alpha}</a:srgbClr></a:solidFill>",
                color[0], color[1], color[2]
            );
            if let Some(ops) = t.outline.as_ref().filter(|o| !o.is_empty()) {
                shapes.push_str(&outline_shape(id, ops, scale, ox, oy, &fill));
                id += 1;
                continue;
            }
            let size = ((t.size * scale * 100.0).round() as i64).clamp(100, 400_000);
            let (family, style) = match &t.look {
                Some(l) => (
                    xml_escape(&l.family),
                    format!(
                        "{}{}",
                        if l.bold { " b=\"1\"" } else { "" },
                        if l.italic { " i=\"1\"" } else { "" }
                    ),
                ),
                None => ("Arial".to_string(), String::new()),
            };
            shapes.push_str(&format!(
                "<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"Text {id}\"/><p:cNvSpPr txBox=\"1\"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom><a:noFill/></p:spPr><p:txBody><a:bodyPr wrap=\"none\" lIns=\"0\" tIns=\"0\" rIns=\"0\" bIns=\"0\" rtlCol=\"0\"><a:noAutofit/></a:bodyPr><a:lstStyle/><a:p><a:r><a:rPr lang=\"de-DE\" sz=\"{size}\"{style} dirty=\"0\">{fill}<a:latin typeface=\"{family}\"/><a:cs typeface=\"{family}\"/></a:rPr><a:t>{}</a:t></a:r></a:p></p:txBody></p:sp>",
                emu(ox + t.x * scale),
                emu(oy + t.y * scale),
                emu((t.w * scale).max(1.0)),
                emu((t.h * scale).max(1.0)),
                xml_escape(&t.text)
            ));
            id += 1;
        }
        for (k, (x, y, w, h, url)) in slide.links.iter().enumerate() {
            rels.push_str(&format!(
                "<Relationship Id=\"rIdLink{k}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink\" Target=\"{}\" TargetMode=\"External\"/>",
                xml_escape(url)
            ));
            shapes.push_str(&format!(
                "<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"Verweis {id}\"><a:hlinkClick r:id=\"rIdLink{k}\"/></p:cNvPr><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom><a:noFill/><a:ln><a:noFill/></a:ln></p:spPr></p:sp>",
                emu(ox + x * scale),
                emu(oy + y * scale),
                emu((w * scale).max(1.0)),
                emu((h * scale).max(1.0))
            ));
            id += 1;
        }
        rels.push_str("</Relationships>");
        let xml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<p:sld {NS}><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>{shapes}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"
        );
        zip.add(&format!("ppt/slides/slide{n}.xml"), xml.as_bytes());
        zip.add(&format!("ppt/slides/_rels/slide{n}.xml.rels"), rels.as_bytes());
    }
    types.push_str("</Types>");
    pres_rels.push_str("</Relationships>");
    let verifier = password.filter(|p| !p.is_empty()).map_or(String::new(), |p| {
        let (hash, salt) = protect(p);
        format!(
            "<p:modifyVerifier algorithmName=\"SHA-512\" hashValue=\"{hash}\" saltValue=\"{salt}\" spinValue=\"{SPIN_COUNT}\"/>"
        )
    });
    let presentation = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<p:presentation {NS}><p:sldMasterIdLst><p:sldMasterId id=\"2147483648\" r:id=\"rIdMaster\"/></p:sldMasterIdLst><p:sldIdLst>{sld_ids}</p:sldIdLst><p:sldSz cx=\"{}\" cy=\"{}\"/><p:notesSz cx=\"6858000\" cy=\"9144000\"/>{verifier}</p:presentation>",
        emu(sw).max(914_400),
        emu(sh).max(914_400)
    );
    let master = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<p:sldMaster {NS}>{EMPTY_TREE}<p:clrMap bg1=\"lt1\" tx1=\"dk1\" bg2=\"lt2\" tx2=\"dk2\" accent1=\"accent1\" accent2=\"accent2\" accent3=\"accent3\" accent4=\"accent4\" accent5=\"accent5\" accent6=\"accent6\" hlink=\"hlink\" folHlink=\"folHlink\"/><p:sldLayoutIdLst><p:sldLayoutId id=\"2147483649\" r:id=\"rId1\"/></p:sldLayoutIdLst></p:sldMaster>"
    );
    let master_rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout\" Target=\"../slideLayouts/slideLayout1.xml\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme\" Target=\"../theme/theme1.xml\"/></Relationships>";
    let layout = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<p:sldLayout {NS} type=\"blank\" preserve=\"1\">{EMPTY_TREE}<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"
    );
    let layout_rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster\" Target=\"../slideMasters/slideMaster1.xml\"/></Relationships>";
    zip.add("[Content_Types].xml", types.as_bytes());
    zip.add("_rels/.rels", ROOT_RELS.replace("{main}", "ppt/presentation.xml").as_bytes());
    zip.add("docProps/core.xml", core(title).as_bytes());
    zip.add("ppt/presentation.xml", presentation.as_bytes());
    zip.add("ppt/_rels/presentation.xml.rels", pres_rels.as_bytes());
    zip.add("ppt/slideMasters/slideMaster1.xml", master.as_bytes());
    zip.add("ppt/slideMasters/_rels/slideMaster1.xml.rels", master_rels.as_bytes());
    zip.add("ppt/slideLayouts/slideLayout1.xml", layout.as_bytes());
    zip.add("ppt/slideLayouts/_rels/slideLayout1.xml.rels", layout_rels.as_bytes());
    zip.add("ppt/theme/theme1.xml", THEME.as_bytes());
    zip.finish()
}

/// Options of "Einstellungen für PowerPoint-Umwandlung".
#[derive(Debug, Clone, PartialEq)]
pub struct PowerPointOptions {
    /// "Beschädigten Text mit Texterkennung verbessern": recognised text is
    /// put invisibly over the picture of the scanned page.
    pub ocr: Option<crate::ocr::Engine>,
    /// "Gesamten Seiteninhalt in ein Bild umwandeln" (no editable text).
    pub whole_page_image: bool,
    /// Resolution of the pictures in dpi.
    pub dpi: f32,
    /// "Kommentare einschließen".
    pub comments: bool,
    /// "Verweise einschließen".
    pub links: bool,
    /// "In Kurven umwandeln": the text as outlines instead of text boxes.
    pub curves: bool,
}

/// Writes `pages` of `doc` as PowerPoint presentation: each page becomes a
/// slide with its graphics as picture and (unless `whole_page_image`) its
/// text as editable text boxes.
pub fn export_powerpoint(
    doc: &mut Document,
    dest: &Path,
    pages: &[usize],
    o: &PowerPointOptions,
) -> Result<()> {
    export_powerpoint_protected(doc, dest, pages, o, None)
}

/// As [`export_powerpoint`]; `password` protects the file from changes.
pub fn export_powerpoint_protected(
    doc: &mut Document,
    dest: &Path,
    pages: &[usize],
    o: &PowerPointOptions,
    password: Option<&str>,
) -> Result<()> {
    let mut slides = Vec::new();
    for &page in pages {
        let geo = *doc.geometry(page).ok_or(Error::PageOutOfRange(page))?;
        let (w, h) = geo.view_size();
        let scale = (o.dpi / 72.0).clamp(0.5, 600.0 / 72.0);
        let px = |v: f32| ((v * scale).round() as u32).clamp(1, crate::document::MAX_BITMAP_EDGE);
        let img = doc.render_background(page, px(w), px(h), !o.whole_page_image, o.comments)?;
        let mut png = Vec::new();
        image::RgbaImage::from_raw(img.width, img.height, img.rgba)
            .ok_or_else(|| Error::Invalid("image".into()))?
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .map_err(|e| Error::Invalid(e.to_string()))?;
        let mut slide = Slide { width: w, height: h, background: Some(png), ..Slide::default() };
        if !o.whole_page_image {
            let text = text_for_export(doc, page, true, o.ocr.as_ref())?;
            // Recognised text (no styles) lies invisibly over the scan.
            let recognised = text.styles.is_empty() && !text.chars.is_empty() && o.ocr.is_some();
            let fonts = if o.curves && !recognised {
                Some(crate::outlines::Fonts::new(doc.font_programs(page)?))
            } else {
                None
            };
            for line in segments(&text) {
                for s in line {
                    let v = geo.pdf_rect_to_view(&s.rect);
                    // The real font size where known, else from the glyph height.
                    let size =
                        text.styles.get(s.chars.0).map_or(s.size.max(1.0) * 0.85, |st| st.size.max(1.0));
                    let outline = fonts.as_ref().map(|fonts| {
                        let mut ops = Vec::new();
                        for i in s.chars.0..=s.chars.1 {
                            let (Some(c), Some(st)) = (text.chars.get(i), text.styles.get(i)) else {
                                continue;
                            };
                            if c.ch.is_whitespace() || c.generated {
                                continue;
                            }
                            let name = text.fonts.get(usize::from(st.font)).map_or("", |f| f.name.as_str());
                            let p = geo.pdf_to_view(crate::geometry::PointF::new(st.origin[0], st.origin[1]));
                            fonts.glyph(name, c.ch, st.size.max(1.0), p.x, p.y, &mut ops);
                        }
                        ops
                    });
                    slide.texts.push(SlideText {
                        x: v.x0,
                        y: v.y0 - size * 0.15,
                        w: v.width() * 1.08 + size * 0.5,
                        h: v.height().max(size * 1.2),
                        text: s.text,
                        size,
                        look: s.look,
                        outline,
                        invisible: recognised,
                    });
                }
            }
        }
        if o.links {
            for l in doc.links(page)? {
                if let LinkTarget::Uri(url) = l.target
                    && (url.starts_with("http://")
                        || url.starts_with("https://")
                        || url.starts_with("mailto:"))
                {
                    let v = geo.pdf_rect_to_view(&l.rect);
                    slide.links.push((v.x0, v.y0, v.width(), v.height(), url));
                }
            }
        }
        slides.push(slide);
    }
    if slides.is_empty() {
        return Err(Error::Invalid("no pages".into()));
    }
    write_atomic(dest, &pptx(doc.display_name(), &slides, password))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_columns() {
        let de = NumberFormat::GERMAN;
        assert_eq!(parse_number("1.234,56", de), Some(1234.56));
        assert_eq!(parse_number("-12", de), Some(-12.0));
        assert_eq!(parse_number("3,5 %", de), Some(0.035));
        assert_eq!(parse_number("12.34", de), None, "not German");
        assert_eq!(parse_number("1,234.5", NumberFormat::ENGLISH), Some(1234.5));
        assert_eq!(parse_number("Seite 2", de), None);
        assert_eq!(parse_number("12.3456", de), None);
        assert_eq!(column_name(0), "A");
        assert_eq!(column_name(25), "Z");
        assert_eq!(column_name(26), "AA");
        assert_eq!(column_name(701), "ZZ");
        assert_eq!(sheet_names(&["a/b".into(), "A/B".into(), "".into()]), ["ab", "AB 2", "Tabelle"]);
    }

    fn seg(text: &str, x: f32, y: f32) -> Segment {
        Segment {
            text: text.into(),
            rect: RectF::new(x, y, x + 30.0, y + 10.0),
            size: 10.0,
            look: None,
            chars: (0, 0),
        }
    }

    #[test]
    fn tables_and_paragraphs() {
        let lines = vec![
            vec![seg("Name", 50.0, 700.0), seg("Betrag", 200.0, 700.0)],
            vec![seg("Miete", 50.0, 685.0), seg("700,00", 203.0, 685.0)],
            vec![seg("Ein langer", 50.0, 600.0)],
            vec![seg("Absatz", 50.0, 589.0)],
        ];
        let t = table(&lines, true, false);
        assert_eq!(t[0], ["Name", "Betrag"]);
        assert_eq!(t[1], ["Miete", "700,00"]);
        assert_eq!(t.len(), 4);
        let p = table(&lines, true, true);
        assert_eq!(p.len(), 3);
        assert_eq!(p[2][0], "Ein langer Absatz");
        let plain = table(&lines, false, false);
        assert_eq!(plain[0], ["Name Betrag"]);
    }

    #[test]
    fn office_files_are_zips_with_their_parts() {
        let rows = vec![vec![Some(Cell::Text("a<b".into())), Some(Cell::Number(1.5))]];
        let x = xlsx("T", &[("Seite 1".into(), rows)], Some("geheim"));
        let s = String::from_utf8_lossy(&x);
        assert_eq!(&x[..4], b"PK\x03\x04");
        assert!(s.contains("xl/worksheets/sheet1.xml") && s.contains("a&lt;b") && s.contains("<v>1.5</v>"));
        assert!(s.contains("<fileSharing userName=\"MrPDF\" algorithmName=\"SHA-512\""));
        let look = Look { family: "Times New Roman".into(), bold: true, italic: false, color: [200, 0, 0] };
        let text = |outline| SlideText {
            x: 10.0,
            y: 10.0,
            w: 50.0,
            h: 12.0,
            text: "Hallo".into(),
            size: 10.0,
            look: Some(look.clone()),
            outline,
            invisible: false,
        };
        let slide = Slide {
            width: 595.0,
            height: 842.0,
            background: Some(vec![1, 2, 3]),
            texts: vec![
                text(None),
                text(Some(vec![
                    PathOp::Move(10.0, 10.0),
                    PathOp::Line(20.0, 10.0),
                    PathOp::Line(15.0, 20.0),
                    PathOp::Close,
                ])),
            ],
            links: vec![(0.0, 0.0, 10.0, 10.0, "https://example.org/?a=1&b=2".into())],
        };
        let p = pptx("T", &[slide.clone(), slide], Some("geheim"));
        let s = String::from_utf8_lossy(&p);
        assert!(s.contains("ppt/slides/slide2.xml") && s.contains("<a:t>Hallo</a:t>"));
        assert!(s.contains("a=1&amp;b=2") && s.contains("ppt/media/image1.png"));
        assert!(s.contains("<p:sldSz cx=\"7556500\" cy=\"10693400\"/>"));
        assert!(s.contains("b=\"1\"") && s.contains("typeface=\"Times New Roman\"") && s.contains("C80000"));
        assert!(s.contains("<a:custGeom>") && s.contains("<a:close/>"));
        assert!(s.contains("<p:modifyVerifier algorithmName=\"SHA-512\""));
    }

    #[test]
    fn password_hash_and_font_names() {
        // Known answer: SHA-512 of salt + UTF-16LE password, no extra rounds.
        let (hash, salt) = modify_hash("a", &[0u8; 16], 0);
        assert_eq!(salt, "AAAAAAAAAAAAAAAAAAAAAA==");
        assert_eq!(hash.len(), 88);
        let (again, _) = modify_hash("a", &[0u8; 16], 0);
        assert_eq!(hash, again);
        assert_ne!(modify_hash("b", &[0u8; 16], 0).0, hash);
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(
            font_family("ABCDEF+TimesNewRomanPS-BoldItalicMT"),
            ("Times New Roman".into(), true, true)
        );
        assert_eq!(font_family("Helvetica-Bold"), ("Arial".into(), true, false));
        assert_eq!(font_family("SegoeUI"), ("Segoe UI".into(), false, false));
        assert_eq!(font_family("Calibri"), ("Calibri".into(), false, false));
    }
}
