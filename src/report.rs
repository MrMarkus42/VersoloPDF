//! Simple text reports as PDF pages (comment summary, comparison and
//! accessibility reports, table of contents).
//!
//! Text is laid out with the standard Helvetica fonts (WinAnsi), so no font
//! files are embedded. Long lines are wrapped at word boundaries.

use lopdf::{Dictionary, Document as LoDocument, Object, ObjectId, Stream};

use crate::appearance::{StdFont, num, win_ansi};
use crate::error::{Error, Result};
use crate::geometry::RectF;
use crate::model::Rgba;
use crate::pdf_edit::{Edit, guarded, rect_array, text_string};

/// A4 portrait in points.
pub const A4: (f32, f32) = (595.28, 841.89);
const MARGIN: f32 = 56.0;
/// Upper bound for the number of report pages.
const MAX_PAGES: usize = 5000;

/// One paragraph of a report.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub text: String,
    pub size: f32,
    pub bold: bool,
    pub indent: f32,
    pub color: Rgba,
    /// Small coloured square in front of the text (e.g. the comment colour).
    pub swatch: Option<Rgba>,
    /// Page index the paragraph links to (only used for tables of contents).
    pub link: Option<usize>,
    /// Text drawn right-aligned on the first line (e.g. a page number).
    pub right: String,
    /// Extra space above the paragraph.
    pub space_before: f32,
    /// Fill between the text and `right` ("Kapitel……1").
    pub leader: Leader,
}

/// Fill between a contents entry and its page number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Leader {
    /// "Kapitel   1".
    #[default]
    Space,
    /// "Kapitel……1".
    Dots,
    /// "Kapitel____1".
    Line,
}

/// Page margins in points: left, top, right, bottom.
pub type Margins = [f32; 4];
pub const DEFAULT_MARGINS: Margins = [MARGIN, MARGIN, MARGIN, MARGIN];

impl Line {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            size: 10.0,
            bold: false,
            indent: 0.0,
            color: Rgba::BLACK,
            swatch: None,
            link: None,
            right: String::new(),
            space_before: 0.0,
            leader: Leader::Space,
        }
    }

    pub fn title(text: impl Into<String>) -> Self {
        Self { size: 18.0, bold: true, space_before: 0.0, ..Self::new(text) }
    }

    pub fn heading(text: impl Into<String>) -> Self {
        Self { size: 12.0, bold: true, space_before: 10.0, ..Self::new(text) }
    }

    pub fn small(text: impl Into<String>) -> Self {
        Self { size: 8.5, color: Rgba::rgb(90, 90, 90), ..Self::new(text) }
    }

    pub fn indent(mut self, indent: f32) -> Self {
        self.indent = indent;
        self
    }

    pub fn color(mut self, color: Rgba) -> Self {
        self.color = color;
        self
    }
}

/// A laid-out page: content stream and link areas (rect, target page).
#[derive(Debug, Clone, Default)]
pub struct PageLayout {
    pub content: String,
    pub links: Vec<(RectF, usize)>,
}

fn escape(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        match b {
            b'(' | b')' | b'\\' => {
                out.push('\\');
                out.push(b as char);
            }
            b'\r' | b'\n' => out.push(' '),
            0x20..=0x7e => out.push(b as char),
            _ => out.push_str(&format!("\\{b:03o}")),
        }
    }
    out
}

/// Splits `text` into lines no wider than `width`.
fn wrap(text: &str, font: StdFont, size: f32, width: f32) -> Vec<Vec<u8>> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut current: Vec<u8> = Vec::new();
        for word in paragraph.split(' ') {
            let word = win_ansi(word);
            let candidate = if current.is_empty() {
                word.clone()
            } else {
                let mut c = current.clone();
                c.push(b' ');
                c.extend_from_slice(&word);
                c
            };
            if font.text_width(&candidate, size) <= width || current.is_empty() {
                current = candidate;
                // A single word longer than the line is cut hard.
                while font.text_width(&current, size) > width && current.len() > 1 {
                    let mut cut = current.len() - 1;
                    while cut > 1 && font.text_width(&current[..cut], size) > width {
                        cut -= 1;
                    }
                    lines.push(current[..cut].to_vec());
                    current = current[cut..].to_vec();
                }
            } else {
                lines.push(std::mem::take(&mut current));
                current = word;
            }
        }
        lines.push(current);
    }
    lines
}

/// Lays out the lines on pages of `size`.
pub fn layout(lines: &[Line], size: (f32, f32)) -> Vec<PageLayout> {
    layout_with(lines, size, DEFAULT_MARGINS, 0.0)
}

/// Lays out the lines with the given margins and extra space between lines.
pub fn layout_with(lines: &[Line], size: (f32, f32), margins: Margins, spacing: f32) -> Vec<PageLayout> {
    let (w, h) = size;
    let [ml, mt, mr, mb] = margins.map(|m| m.clamp(0.0, w.min(h) / 3.0));
    let mut pages = vec![PageLayout::default()];
    let mut y = h - mt;
    for line in lines {
        let font = if line.bold { StdFont::HelveticaBold } else { StdFont::Helvetica };
        let font_name = if line.bold { "HB" } else { "H" };
        let lead = line.size * 1.3 + spacing.max(0.0);
        let x = ml + line.indent + if line.swatch.is_some() { line.size + 4.0 } else { 0.0 };
        let right = win_ansi(&line.right);
        let right_w = if right.is_empty() { 0.0 } else { font.text_width(&right, line.size) + 12.0 };
        let avail = (w - mr - x - right_w).max(40.0);
        let wrapped = wrap(&line.text, font, line.size, avail);
        y -= line.space_before;
        for (i, text) in wrapped.iter().enumerate() {
            if y - lead < mb {
                if pages.len() >= MAX_PAGES {
                    return pages;
                }
                pages.push(PageLayout::default());
                y = h - mt;
            }
            y -= lead;
            let page = pages.last_mut().expect("at least one page");
            let [r, g, b] = line.color.to_pdf();
            if i == 0
                && let Some(sw) = line.swatch
            {
                let [sr, sg, sb] = sw.to_pdf();
                page.content.push_str(&format!(
                    "{} {} {} rg {} {} {} {} re f\n",
                    num(sr),
                    num(sg),
                    num(sb),
                    num(ml + line.indent),
                    num(y),
                    num(line.size * 0.8),
                    num(line.size * 0.8)
                ));
            }
            page.content.push_str(&format!(
                "BT /{font_name} {} Tf {} {} {} rg {} {} Td ({}) Tj ET\n",
                num(line.size),
                num(r),
                num(g),
                num(b),
                num(x),
                num(y),
                escape(text)
            ));
            if i == 0 && !right.is_empty() {
                let rx = w - mr - font.text_width(&right, line.size);
                let fill = match line.leader {
                    Leader::Space => None,
                    Leader::Dots => Some(b'.'),
                    Leader::Line => Some(b'_'),
                };
                if let Some(ch) = fill {
                    let from = x + font.text_width(text, line.size) + 3.0;
                    let one = font.text_width(&[ch], line.size).max(0.5);
                    let count = ((rx - 3.0 - from) / one).floor();
                    if count >= 1.0 {
                        let dots = vec![ch; count as usize];
                        page.content.push_str(&format!(
                            "BT /{font_name} {} Tf {} {} {} rg {} {} Td ({}) Tj ET\n",
                            num(line.size),
                            num(r),
                            num(g),
                            num(b),
                            num(rx - 3.0 - count * one),
                            num(y),
                            escape(&dots)
                        ));
                    }
                }
                page.content.push_str(&format!(
                    "BT /{font_name} {} Tf {} {} {} rg {} {} Td ({}) Tj ET\n",
                    num(line.size),
                    num(r),
                    num(g),
                    num(b),
                    num(rx),
                    num(y),
                    escape(&right)
                ));
            }
            if let Some(target) = line.link {
                let rect = RectF::new(ml + line.indent, y - line.size * 0.3, w - mr, y + line.size);
                page.links.push((rect, target));
            }
        }
    }
    pages
}

fn font_resources() -> Dictionary {
    let font = |base: &str| {
        let mut f = Dictionary::new();
        f.set("Type", Object::Name(b"Font".to_vec()));
        f.set("Subtype", Object::Name(b"Type1".to_vec()));
        f.set("BaseFont", Object::Name(base.as_bytes().to_vec()));
        f.set("Encoding", Object::Name(b"WinAnsiEncoding".to_vec()));
        Object::Dictionary(f)
    };
    let mut fonts = Dictionary::new();
    fonts.set("H", font("Helvetica"));
    fonts.set("HB", font("Helvetica-Bold"));
    let mut res = Dictionary::new();
    res.set("Font", Object::Dictionary(fonts));
    res
}

fn page_dict(parent: ObjectId, content: ObjectId, size: (f32, f32)) -> Dictionary {
    let mut page = Dictionary::new();
    page.set("Type", Object::Name(b"Page".to_vec()));
    page.set("Parent", Object::Reference(parent));
    page.set("MediaBox", rect_array(&RectF::new(0.0, 0.0, size.0, size.1)));
    page.set("Resources", Object::Dictionary(font_resources()));
    page.set("Contents", Object::Reference(content));
    page
}

fn link_annot(rect: &RectF, dest_page: ObjectId) -> Dictionary {
    let mut a = Dictionary::new();
    a.set("Type", Object::Name(b"Annot".to_vec()));
    a.set("Subtype", Object::Name(b"Link".to_vec()));
    a.set("Rect", rect_array(rect));
    a.set("Border", Object::Array(vec![0.into(), 0.into(), 0.into()]));
    a.set("Dest", Object::Array(vec![Object::Reference(dest_page), Object::Name(b"Fit".to_vec())]));
    a
}

/// Builds a standalone PDF from the lines.
pub fn build_pdf(title: &str, lines: &[Line]) -> Result<Vec<u8>> {
    let pages = layout(lines, A4);
    let mut doc = LoDocument::with_version("1.7");
    let pages_id = doc.new_object_id();
    let mut kids = Vec::new();
    let mut page_ids = Vec::new();
    for page in &pages {
        let content = doc.add_object(Stream::new(Dictionary::new(), page.content.clone().into_bytes()));
        let id = doc.add_object(page_dict(pages_id, content, A4));
        kids.push(Object::Reference(id));
        page_ids.push(id);
    }
    // Links inside a standalone report point to its own pages.
    for (page, id) in pages.iter().zip(&page_ids) {
        let annots: Vec<Object> = page
            .links
            .iter()
            .filter_map(|(r, target)| page_ids.get(*target).map(|d| Object::Dictionary(link_annot(r, *d))))
            .collect();
        if !annots.is_empty()
            && let Ok(Object::Dictionary(d)) = doc.get_object_mut(*id)
        {
            d.set("Annots", Object::Array(annots));
        }
    }
    let mut pages_dict = Dictionary::new();
    pages_dict.set("Type", Object::Name(b"Pages".to_vec()));
    pages_dict.set("Count", Object::Integer(kids.len() as i64));
    pages_dict.set("Kids", Object::Array(kids));
    doc.objects.insert(pages_id, Object::Dictionary(pages_dict));
    let mut info = Dictionary::new();
    info.set("Title", text_string(title));
    info.set("Producer", Object::string_literal(format!("MrPDF {}", env!("CARGO_PKG_VERSION"))));
    let info_id = doc.add_object(info);
    let mut catalog = Dictionary::new();
    catalog.set("Type", Object::Name(b"Catalog".to_vec()));
    catalog.set("Pages", Object::Reference(pages_id));
    let catalog_id = doc.add_object(catalog);
    doc.trailer.set("Root", Object::Reference(catalog_id));
    doc.trailer.set("Info", Object::Reference(info_id));
    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(|e| Error::Invalid(format!("could not write report: {e}")))?;
    Ok(out)
}

/// Inserts report pages at the front of an existing document. The `link`
/// targets of the lines are page indices of the document *before*
/// insertion. Returns the new bytes and the number of inserted pages.
pub fn insert_front(bytes: &[u8], password: Option<&str>, lines: &[Line]) -> Result<(Vec<u8>, usize)> {
    insert_at(bytes, password, lines, &Placement::default())
}

/// Where and how report pages are inserted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// Index of the page the report goes in front of (past the end: last).
    pub before: usize,
    /// Page size; None: like the page at the insert position.
    pub size: Option<(f32, f32)>,
    pub margins: Margins,
    /// Extra space between lines.
    pub spacing: f32,
}

impl Default for Placement {
    fn default() -> Self {
        Self { before: 0, size: None, margins: DEFAULT_MARGINS, spacing: 0.0 }
    }
}

/// Inserts report pages before page `at.before`. The `link` targets of the
/// lines are page indices of the document *before* insertion. Returns the
/// new bytes and the number of inserted pages.
pub fn insert_at(
    bytes: &[u8],
    password: Option<&str>,
    lines: &[Line],
    at: &Placement,
) -> Result<(Vec<u8>, usize)> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let old_pages = edit.page_ids();
        let before = at.before.min(old_pages.len());
        let near = *old_pages
            .get(before)
            .or(old_pages.last())
            .ok_or_else(|| Error::Invalid("document has no pages".into()))?;
        let size = at.size.unwrap_or_else(|| {
            let r = crate::pages::visible_box(&edit, near);
            (r.width(), r.height())
        });
        let size = (size.0.max(200.0), size.1.max(200.0));
        let layout = layout_with(lines, size, at.margins, at.spacing);
        let catalog_id = edit.prev().trailer.get(b"Root")?.as_reference()?;
        let root_pages = edit.object(catalog_id)?.as_dict()?.get(b"Pages")?.as_reference()?;
        let mut new_ids = Vec::new();
        for page in &layout {
            let content = edit.add(Stream::new(Dictionary::new(), page.content.clone().into_bytes()));
            let mut dict = page_dict(root_pages, content, size);
            let annots: Vec<Object> = page
                .links
                .iter()
                .filter_map(|(r, target)| {
                    old_pages.get(*target).map(|d| Object::Dictionary(link_annot(r, *d)))
                })
                .collect();
            if !annots.is_empty() {
                dict.set("Annots", Object::Array(annots));
            }
            new_ids.push(edit.add(Object::Dictionary(dict)));
        }
        let root = edit.dict_mut(root_pages)?;
        let mut kids = match root.get(b"Kids") {
            Ok(Object::Array(k)) => k.clone(),
            _ => return Err(Error::Invalid("page tree without kids".into())),
        };
        // Position among the root's kids: in front of the kid holding the
        // page (pages in nested trees go in front of their subtree).
        let mut pos = kids.len();
        if let Some(target) = old_pages.get(before) {
            let mut node = *target;
            for _ in 0..64 {
                if let Some(i) = kids.iter().position(|k| k.as_reference().ok() == Some(node)) {
                    pos = i;
                    break;
                }
                match edit
                    .object(node)
                    .ok()
                    .and_then(|o| o.as_dict().ok())
                    .and_then(|d| d.get(b"Parent").ok())
                {
                    Some(Object::Reference(p)) => node = *p,
                    _ => break,
                }
            }
        }
        let root = edit.dict_mut(root_pages)?;
        for (i, id) in new_ids.iter().enumerate() {
            kids.insert(pos + i, Object::Reference(*id));
        }
        let count = root.get(b"Count").ok().and_then(|c| c.as_i64().ok()).unwrap_or(old_pages.len() as i64);
        root.set("Kids", Object::Array(kids));
        root.set("Count", Object::Integer(count + new_ids.len() as i64));
        Ok((edit.finish()?, new_ids.len()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_text_is_wrapped() {
        let text = "wort ".repeat(200);
        let lines = wrap(&text, StdFont::Helvetica, 10.0, 200.0);
        assert!(lines.len() > 5);
        assert!(lines.iter().all(|l| StdFont::Helvetica.text_width(l, 10.0) <= 200.0));
        let hard = wrap(&"x".repeat(500), StdFont::Helvetica, 10.0, 100.0);
        assert!(hard.len() > 3);
    }

    #[test]
    fn many_lines_make_several_pages() {
        let lines: Vec<Line> = (0..200).map(|i| Line::new(format!("Zeile {i} (ä)"))).collect();
        let pages = layout(&lines, A4);
        assert!(pages.len() >= 3);
        assert!(pages[0].content.contains("(Zeile 0 \\(\\344\\)) Tj"));
    }
}
