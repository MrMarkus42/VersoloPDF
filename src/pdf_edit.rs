//! Structural edits performed directly on the PDF object graph (via `lopdf`).
//!
//! All edits are written as *incremental updates*: the original bytes are
//! kept unchanged and the modified objects are appended. This keeps the file
//! intact even for constructs MrPDF does not understand.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use lopdf::{Dictionary, Document, IncrementalDocument, LoadOptions, Object, ObjectId, Stream, StringFormat};

use crate::appearance::{self, ApInput, Appearance};
use crate::error::{Error, Result};
use crate::geometry::{PointF, RectF};
use crate::model::{AnnotKind, AnnotStyle, Metadata, Rgba};

/// Upper limit for a single decompressed stream.
const MAX_DECOMPRESSED_STREAM: usize = 256 * 1024 * 1024;

/// Parameters of a new annotation.
#[derive(Debug, Clone, PartialEq)]
pub struct NewAnnotation {
    pub kind: AnnotKind,
    /// Rectangle for notes, shapes, text boxes and stamps (PDF space).
    pub rect: RectF,
    /// Text markup: one rectangle per line.
    pub quads: Vec<RectF>,
    /// Line end points, ink strokes, polygon vertices.
    pub paths: Vec<Vec<PointF>>,
    pub arrow_end: bool,
    pub contents: String,
    /// Stamp label or free text box text colour source.
    pub label: String,
    pub style: AnnotStyle,
    /// Standard stamp/icon name (`/Name`).
    pub icon: Option<String>,
    /// Polygon with a cloudy border.
    pub cloudy: bool,
    /// Callout: leader line from the arrow tip (first point) to the text box.
    pub leader: Vec<PointF>,
    /// Measurement text shown next to lines and polygons.
    pub measure_label: String,
    /// Embedded file of a file attachment annotation (name, content).
    pub attachment: Option<(String, Vec<u8>)>,
}

impl NewAnnotation {
    pub fn new(kind: AnnotKind, style: AnnotStyle) -> Self {
        Self {
            kind,
            rect: RectF::default(),
            quads: Vec::new(),
            paths: Vec::new(),
            arrow_end: false,
            contents: String::new(),
            label: String::new(),
            style,
            icon: None,
            cloudy: false,
            leader: Vec::new(),
            measure_label: String::new(),
            attachment: None,
        }
    }
}

/// Changes applied to an existing annotation. `None` keeps a value.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AnnotUpdate {
    pub contents: Option<String>,
    pub author: Option<String>,
    pub color: Option<Rgba>,
    pub fill: Option<Option<Rgba>>,
    pub width: Option<f32>,
    pub opacity: Option<f32>,
    pub font_size: Option<f32>,
    /// Displacement in PDF space.
    pub translate: Option<(f32, f32)>,
}

impl AnnotUpdate {
    fn changes_style(&self) -> bool {
        self.color.is_some()
            || self.fill.is_some()
            || self.width.is_some()
            || self.opacity.is_some()
            || self.font_size.is_some()
    }
}

/// Style properties read back from an annotation dictionary.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AnnotDetails {
    pub kind: Option<AnnotKind>,
    pub color: Option<Rgba>,
    pub fill: Option<Rgba>,
    pub width: f32,
    pub opacity: f32,
    pub font_size: f32,
    pub subject: String,
    /// True if MrPDF can regenerate the appearance of this annotation.
    pub restylable: bool,
}

/// Runs a closure that works on untrusted PDF structures and converts a
/// panic inside the parser into an ordinary error.
pub(crate) fn guarded<T>(f: impl FnOnce() -> Result<T>) -> Result<T> {
    catch_unwind(AssertUnwindSafe(f))
        .unwrap_or_else(|_| Err(Error::Structure("internal parser error".into())))
}

pub(crate) struct Edit {
    pub(crate) inc: IncrementalDocument,
}

impl Edit {
    pub(crate) fn open(bytes: &[u8], password: Option<&str>) -> Result<Self> {
        let mut options = match password {
            Some(pw) => LoadOptions::with_password(pw),
            None => LoadOptions::default(),
        };
        // Protection against decompression bombs.
        options.max_decompressed_size = Some(MAX_DECOMPRESSED_STREAM);
        let doc = Document::load_mem_with_options(bytes, options)?;
        if doc.is_encrypted() {
            return Err(Error::EncryptedNotSupported);
        }
        Ok(Self { inc: IncrementalDocument::create_from(bytes.to_vec(), doc) })
    }

    pub(crate) fn prev(&self) -> &Document {
        self.inc.get_prev_documents()
    }

    pub(crate) fn page_ids(&self) -> Vec<ObjectId> {
        self.prev().get_pages().into_values().collect()
    }

    pub(crate) fn page_id(&self, page: usize) -> Result<ObjectId> {
        let no = u32::try_from(page + 1).map_err(|_| Error::PageOutOfRange(page))?;
        self.prev().get_pages().get(&no).copied().ok_or(Error::PageOutOfRange(page))
    }

    pub(crate) fn object(&self, id: ObjectId) -> Result<&Object> {
        if self.inc.new_document.has_object(id) {
            Ok(self.inc.new_document.get_object(id)?)
        } else {
            Ok(self.prev().get_object(id)?)
        }
    }

    pub(crate) fn object_mut(&mut self, id: ObjectId) -> Result<&mut Object> {
        self.inc.opt_clone_object_to_new_document(id)?;
        Ok(self.inc.new_document.get_object_mut(id)?)
    }

    pub(crate) fn dict_mut(&mut self, id: ObjectId) -> Result<&mut Dictionary> {
        Ok(self.object_mut(id)?.as_dict_mut()?)
    }

    pub(crate) fn add(&mut self, object: impl Into<Object>) -> ObjectId {
        self.inc.new_document.add_object(object)
    }

    /// Resolves `obj` if it is a reference.
    pub(crate) fn resolve<'a>(&'a self, obj: &'a Object) -> Result<&'a Object> {
        match obj {
            Object::Reference(id) => self.object(*id),
            other => Ok(other),
        }
    }

    /// The page's `/Annots` array, made editable in the new revision.
    pub(crate) fn annots_mut(&mut self, page_id: ObjectId) -> Result<&mut Vec<Object>> {
        let current = self.object(page_id)?.as_dict()?.get(b"Annots").ok().cloned();
        match current {
            Some(Object::Reference(id)) => Ok(self.object_mut(id)?.as_array_mut()?),
            Some(Object::Array(_)) => Ok(self.dict_mut(page_id)?.get_mut(b"Annots")?.as_array_mut()?),
            _ => {
                let page = self.dict_mut(page_id)?;
                page.set("Annots", Object::Array(Vec::new()));
                Ok(page.get_mut(b"Annots")?.as_array_mut()?)
            }
        }
    }

    pub(crate) fn annots(&self, page_id: ObjectId) -> Result<Vec<Object>> {
        let page = self.object(page_id)?.as_dict()?;
        match page.get(b"Annots") {
            Ok(obj) => Ok(self.resolve(obj)?.as_array()?.clone()),
            Err(_) => Ok(Vec::new()),
        }
    }

    /// Returns the annotation entry at `index` as object id (direct
    /// annotation dictionaries are converted into indirect objects).
    pub(crate) fn annotation_id(&mut self, page_id: ObjectId, index: usize) -> Result<ObjectId> {
        let entry = self.annots(page_id)?.get(index).cloned().ok_or(Error::AnnotationOutOfRange(index))?;
        match entry {
            Object::Reference(id) => Ok(id),
            Object::Dictionary(dict) => {
                let id = self.add(Object::Dictionary(dict));
                self.annots_mut(page_id)?[index] = Object::Reference(id);
                Ok(id)
            }
            _ => Err(Error::AnnotationOutOfRange(index)),
        }
    }

    pub(crate) fn finish(mut self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        self.inc.save_to(&mut out).map_err(|e| Error::Structure(e.to_string()))?;
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// Value helpers

/// Encodes a PDF text string (PDFDocEncoding for ASCII, UTF-16BE otherwise).
pub fn text_string(s: &str) -> Object {
    if s.chars().all(|c| (' '..='~').contains(&c) || c == '\n' || c == '\r' || c == '\t') {
        Object::String(s.as_bytes().to_vec(), StringFormat::Literal)
    } else {
        let mut bytes = vec![0xFE, 0xFF];
        for unit in s.encode_utf16() {
            bytes.extend_from_slice(&unit.to_be_bytes());
        }
        Object::String(bytes, StringFormat::Hexadecimal)
    }
}

/// Decodes a PDF text string.
pub fn decode_text_string(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        let units: Vec<u16> = bytes[2..].as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c)).collect();
        String::from_utf16_lossy(&units)
    } else if bytes.len() >= 3 && bytes[..3] == [0xEF, 0xBB, 0xBF] {
        String::from_utf8_lossy(&bytes[3..]).into_owned()
    } else {
        // PDFDocEncoding equals Latin-1 for all printable characters we care about.
        bytes.iter().map(|&b| b as char).collect()
    }
}

/// Current time as PDF date string (UTC).
pub fn pdf_date_now() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    pdf_date(secs)
}

/// Formats seconds since the Unix epoch as `D:YYYYMMDDHHmmSSZ`.
pub fn pdf_date(unix_secs: i64) -> String {
    let days = unix_secs.div_euclid(86_400);
    let secs = unix_secs.rem_euclid(86_400);
    // Civil-from-days algorithm (Howard Hinnant, public domain).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    if month <= 2 {
        year += 1;
    }
    format!("D:{year:04}{month:02}{day:02}{:02}{:02}{:02}Z", secs / 3600, (secs / 60) % 60, secs % 60)
}

/// Turns `D:20240131124500+01'00'` into `2024-01-31 12:45`.
pub fn format_pdf_date(raw: &str) -> String {
    let s = raw.trim().trim_start_matches("D:");
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.len() < 8 {
        return raw.to_string();
    }
    let mut out = format!("{}-{}-{}", &digits[0..4], &digits[4..6], &digits[6..8]);
    if digits.len() >= 12 {
        out.push_str(&format!(" {}:{}", &digits[8..10], &digits[10..12]));
    }
    out
}

fn unique_name() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("mrpdf-{nanos:x}-{}", COUNTER.fetch_add(1, Ordering::Relaxed))
}

pub(crate) fn real(v: f32) -> Object {
    Object::Real(if v.is_finite() { v } else { 0.0 })
}

pub(crate) fn rect_array(r: &RectF) -> Object {
    let r = r.normalized();
    Object::Array(vec![real(r.x0), real(r.y0), real(r.x1), real(r.y1)])
}

fn color_array(c: Rgba) -> Object {
    Object::Array(c.to_pdf().into_iter().map(real).collect())
}

fn points_array(points: &[PointF]) -> Object {
    Object::Array(points.iter().flat_map(|p| [real(p.x), real(p.y)]).collect())
}

fn quad_points(quads: &[RectF]) -> Object {
    // Order used by Acrobat and PDFium: upper-left, upper-right, lower-left, lower-right.
    Object::Array(
        quads
            .iter()
            .map(|q| q.normalized())
            .flat_map(|q| [q.x0, q.y1, q.x1, q.y1, q.x0, q.y0, q.x1, q.y0])
            .map(real)
            .collect(),
    )
}

pub(crate) fn number(obj: &Object) -> Option<f32> {
    match obj {
        Object::Integer(i) => Some(*i as f32),
        Object::Real(r) => Some(*r),
        _ => None,
    }
}

pub(crate) fn numbers(obj: Option<&Object>) -> Vec<f32> {
    match obj {
        Some(Object::Array(items)) => items.iter().filter_map(number).collect(),
        _ => Vec::new(),
    }
}

pub(crate) fn read_rect(dict: &Dictionary) -> Option<RectF> {
    match numbers(dict.get(b"Rect").ok()).as_slice() {
        [a, b, c, d] => Some(RectF::new(*a, *b, *c, *d).normalized()),
        _ => None,
    }
}

fn read_color(dict: &Dictionary, key: &[u8]) -> Option<Rgba> {
    Rgba::from_pdf(&numbers(dict.get(key).ok()))
}

fn read_points(values: &[f32]) -> Vec<PointF> {
    values.as_chunks::<2>().0.iter().map(|c| PointF::new(c[0], c[1])).collect()
}

fn read_quads(dict: &Dictionary) -> Vec<RectF> {
    numbers(dict.get(b"QuadPoints").ok())
        .as_chunks::<8>()
        .0
        .iter()
        .filter_map(|q| RectF::bounding(read_points(q)))
        .collect()
}

fn read_paths(dict: &Dictionary, kind: AnnotKind) -> Vec<Vec<PointF>> {
    match kind {
        AnnotKind::Line => vec![read_points(&numbers(dict.get(b"L").ok()))],
        AnnotKind::Polygon | AnnotKind::Polyline => {
            vec![read_points(&numbers(dict.get(b"Vertices").ok()))]
        }
        AnnotKind::Ink => match dict.get(b"InkList") {
            Ok(Object::Array(strokes)) => strokes.iter().map(|s| read_points(&numbers(Some(s)))).collect(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

fn read_width(dict: &Dictionary) -> f32 {
    if let Ok(Object::Dictionary(bs)) = dict.get(b"BS")
        && let Some(w) = bs.get(b"W").ok().and_then(number)
    {
        return w;
    }
    numbers(dict.get(b"Border").ok()).get(2).copied().unwrap_or(1.0)
}

pub(crate) fn name(dict: &Dictionary, key: &[u8]) -> Option<String> {
    dict.get(key).ok().and_then(|o| o.as_name().ok()).map(|n| String::from_utf8_lossy(n).into_owned())
}

pub(crate) fn string(dict: &Dictionary, key: &[u8]) -> Option<String> {
    match dict.get(key) {
        Ok(Object::String(bytes, _)) => Some(decode_text_string(bytes)),
        _ => None,
    }
}

pub fn kind_from_subtype(subtype: &str) -> AnnotKind {
    match subtype {
        "Text" => AnnotKind::Note,
        "FreeText" => AnnotKind::FreeText,
        "Line" => AnnotKind::Line,
        "Square" => AnnotKind::Square,
        "Circle" => AnnotKind::Circle,
        "Polygon" => AnnotKind::Polygon,
        "PolyLine" => AnnotKind::Polyline,
        "Highlight" => AnnotKind::Highlight,
        "Underline" => AnnotKind::Underline,
        "Squiggly" => AnnotKind::Squiggly,
        "StrikeOut" => AnnotKind::StrikeOut,
        "Stamp" => AnnotKind::Stamp,
        "Caret" => AnnotKind::Caret,
        "Ink" => AnnotKind::Ink,
        "Popup" => AnnotKind::Popup,
        "FileAttachment" => AnnotKind::FileAttachment,
        "Link" => AnnotKind::Link,
        "Widget" => AnnotKind::Widget,
        "Redact" => AnnotKind::Redact,
        _ => AnnotKind::Other,
    }
}

pub(crate) fn font_size_from_da(da: &str) -> Option<f32> {
    let tokens: Vec<&str> = da.split_whitespace().collect();
    tokens
        .iter()
        .position(|t| *t == "Tf")
        .and_then(|i| i.checked_sub(1))
        .and_then(|i| tokens[i].parse::<f32>().ok())
}

fn text_color_from_da(da: &str) -> Option<Rgba> {
    let tokens: Vec<&str> = da.split_whitespace().collect();
    let pos = tokens.iter().position(|t| matches!(*t, "rg" | "g" | "k"))?;
    let count = match tokens[pos] {
        "rg" => 3,
        "g" => 1,
        _ => 4,
    };
    let start = pos.checked_sub(count)?;
    let values: Vec<f32> = tokens[start..pos].iter().filter_map(|t| t.parse().ok()).collect();
    Rgba::from_pdf(&values)
}

fn can_regenerate(kind: AnnotKind) -> bool {
    matches!(
        kind,
        AnnotKind::Highlight
            | AnnotKind::Underline
            | AnnotKind::StrikeOut
            | AnnotKind::Squiggly
            | AnnotKind::Square
            | AnnotKind::Circle
            | AnnotKind::Line
            | AnnotKind::Ink
            | AnnotKind::Polygon
            | AnnotKind::Polyline
            | AnnotKind::FreeText
            | AnnotKind::Stamp
            | AnnotKind::Note
            | AnnotKind::Caret
            | AnnotKind::FileAttachment
            | AnnotKind::Redact
    )
}

/// Reconstructs the appearance input from an annotation dictionary.
fn ap_input_from_dict(dict: &Dictionary) -> Option<ApInput> {
    let kind = kind_from_subtype(&name(dict, b"Subtype")?);
    if !can_regenerate(kind) {
        return None;
    }
    let rect = read_rect(dict)?;
    let color = read_color(dict, b"C").unwrap_or(Rgba::BLACK);
    let mut input = ApInput::new(kind, rect, color);
    input.fill = read_color(dict, b"IC");
    input.width = read_width(dict);
    input.opacity = dict.get(b"CA").ok().and_then(number).unwrap_or(1.0);
    input.quads = read_quads(dict);
    input.paths = read_paths(dict, kind);
    input.arrow_end = matches!(
        dict.get(b"LE"),
        Ok(Object::Array(ends)) if ends.get(1).and_then(|e| e.as_name().ok()).is_some_and(|n| n != b"None")
    );
    let da = string(dict, b"DA").unwrap_or_default();
    input.font_size = font_size_from_da(&da).unwrap_or(12.0);
    input.text_color = text_color_from_da(&da).unwrap_or(Rgba::BLACK);
    input.text = match kind {
        AnnotKind::Stamp => string(dict, b"Subj").or_else(|| name(dict, b"Name")).unwrap_or_default(),
        _ => string(dict, b"Contents").unwrap_or_default(),
    };
    if kind == AnnotKind::FreeText {
        // MrPDF stores the text box background in /IC like other shapes.
        input.fill = read_color(dict, b"IC");
        let leader = read_points(&numbers(dict.get(b"CL").ok()));
        if leader.len() >= 2 {
            let rd = numbers(dict.get(b"RD").ok());
            if let [l, b, r, t] = rd.as_slice() {
                input.inner = Some(RectF::new(rect.x0 + l, rect.y0 + b, rect.x1 - r, rect.y1 - t));
            }
            input.leader = leader;
        }
    }
    input.cloudy = matches!(dict.get(b"BE"), Ok(Object::Dictionary(be)) if be.get(b"S").ok().and_then(|s| s.as_name().ok()) == Some(b"C".as_slice()));
    if name(dict, b"IT").is_some_and(|it| it.ends_with("Dimension")) {
        input.label = string(dict, b"Contents").unwrap_or_default();
    }
    Some(input)
}

fn appearance_stream(ap: &Appearance) -> Stream {
    let mut resources = Dictionary::new();
    if let Some(opacity) = ap.resources.opacity {
        let mut gs = Dictionary::new();
        gs.set("Type", Object::Name(b"ExtGState".to_vec()));
        gs.set("CA", real(opacity));
        gs.set("ca", real(opacity));
        if ap.resources.multiply {
            gs.set("BM", Object::Name(b"Multiply".to_vec()));
        }
        let mut states = Dictionary::new();
        states.set("GS0", Object::Dictionary(gs));
        resources.set("ExtGState", Object::Dictionary(states));
    }
    if !ap.resources.fonts.is_empty() {
        let mut fonts = Dictionary::new();
        for font in &ap.resources.fonts {
            let mut f = Dictionary::new();
            f.set("Type", Object::Name(b"Font".to_vec()));
            f.set("Subtype", Object::Name(b"Type1".to_vec()));
            f.set("BaseFont", Object::Name(font.base_font().as_bytes().to_vec()));
            f.set("Encoding", Object::Name(b"WinAnsiEncoding".to_vec()));
            fonts.set(font.resource_name(), Object::Dictionary(f));
        }
        resources.set("Font", Object::Dictionary(fonts));
    }
    let mut dict = Dictionary::new();
    dict.set("Type", Object::Name(b"XObject".to_vec()));
    dict.set("Subtype", Object::Name(b"Form".to_vec()));
    dict.set("FormType", Object::Integer(1));
    dict.set("BBox", rect_array(&ap.bbox));
    dict.set("Matrix", Object::Array(vec![1.into(), 0.into(), 0.into(), 1.into(), 0.into(), 0.into()]));
    dict.set("Resources", Object::Dictionary(resources));
    Stream::new(dict, ap.content.clone())
}

fn set_appearance(edit: &mut Edit, annot_id: ObjectId, input: &ApInput) -> Result<()> {
    let ap = appearance::build(input);
    let stream_id = edit.add(appearance_stream(&ap));
    let mut ap_dict = Dictionary::new();
    ap_dict.set("N", Object::Reference(stream_id));
    edit.dict_mut(annot_id)?.set("AP", Object::Dictionary(ap_dict));
    Ok(())
}

// ---------------------------------------------------------------------------
// Public operations

/// Adds a new annotation to `page` and returns the updated document bytes.
pub fn add_annotation(
    bytes: &[u8],
    password: Option<&str>,
    page: usize,
    annot: &NewAnnotation,
) -> Result<Vec<u8>> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let page_id = edit.page_id(page)?;
        let style = &annot.style;
        let kind = annot.kind;

        let mut input = ApInput::new(kind, annot.rect, style.color);
        input.fill = style.fill;
        input.width = style.width;
        input.opacity = style.opacity;
        input.quads = annot.quads.clone();
        input.paths = annot.paths.clone();
        input.arrow_end = annot.arrow_end;
        input.font_size = style.font_size;
        input.text = match kind {
            AnnotKind::Stamp => annot.label.clone(),
            _ => annot.contents.clone(),
        };
        if kind == AnnotKind::FreeText {
            input.text_color = style.color;
        }
        input.cloudy = annot.cloudy;
        input.label = annot.measure_label.clone();
        if annot.leader.len() >= 2 {
            input.leader = annot.leader.clone();
            input.inner = Some(annot.rect.normalized());
        }
        let rect = appearance::required_rect(&input);
        if rect.is_empty() && !matches!(kind, AnnotKind::Line | AnnotKind::Ink) {
            return Err(Error::Invalid("annotation has no area".into()));
        }
        input.rect = rect;

        let now = pdf_date_now();
        let mut d = Dictionary::new();
        d.set("Type", Object::Name(b"Annot".to_vec()));
        d.set("Subtype", Object::Name(kind.pdf_subtype().as_bytes().to_vec()));
        d.set("Rect", rect_array(&rect));
        d.set("P", Object::Reference(page_id));
        d.set("NM", text_string(&unique_name()));
        d.set("M", Object::string_literal(now.clone()));
        d.set("CreationDate", Object::string_literal(now));
        d.set("F", Object::Integer(4)); // Print
        if !style.author.is_empty() {
            d.set("T", text_string(&style.author));
        }
        if !annot.contents.is_empty() {
            d.set("Contents", text_string(&annot.contents));
        }
        if style.opacity < 0.999 {
            d.set("CA", real(style.opacity));
        }
        match kind {
            AnnotKind::FreeText => {
                let [r, g, b] = style.color.to_pdf();
                let da = format!(
                    "/Helv {} Tf {} {} {} rg",
                    appearance::num(style.font_size),
                    appearance::num(r),
                    appearance::num(g),
                    appearance::num(b)
                );
                d.set("DA", Object::string_literal(da));
                d.set("C", color_array(style.color));
                if let Some(fill) = style.fill {
                    d.set("IC", color_array(fill));
                }
            }
            _ => d.set("C", color_array(style.color)),
        }
        if matches!(
            kind,
            AnnotKind::Square
                | AnnotKind::Circle
                | AnnotKind::Line
                | AnnotKind::Ink
                | AnnotKind::Polygon
                | AnnotKind::Polyline
                | AnnotKind::FreeText
        ) {
            let mut bs = Dictionary::new();
            bs.set("Type", Object::Name(b"Border".to_vec()));
            bs.set("W", real(style.width));
            bs.set("S", Object::Name(b"S".to_vec()));
            d.set("BS", Object::Dictionary(bs));
        }
        if let (AnnotKind::Square | AnnotKind::Circle | AnnotKind::Polygon, Some(fill)) = (kind, style.fill) {
            d.set("IC", color_array(fill));
        }
        match kind {
            AnnotKind::Highlight | AnnotKind::Underline | AnnotKind::StrikeOut | AnnotKind::Squiggly => {
                if annot.quads.is_empty() {
                    return Err(Error::Invalid("text markup needs at least one area".into()));
                }
                d.set("QuadPoints", quad_points(&annot.quads));
            }
            AnnotKind::Line => {
                let path = annot
                    .paths
                    .first()
                    .filter(|p| p.len() >= 2)
                    .ok_or_else(|| Error::Invalid("a line needs two points".into()))?;
                d.set("L", points_array(&[path[0], path[path.len() - 1]]));
                let end: &[u8] = if annot.arrow_end { b"OpenArrow" } else { b"None" };
                d.set("LE", Object::Array(vec![Object::Name(b"None".to_vec()), Object::Name(end.to_vec())]));
            }
            AnnotKind::Ink => {
                if annot.paths.iter().all(Vec::is_empty) {
                    return Err(Error::Invalid("ink needs at least one point".into()));
                }
                d.set("InkList", Object::Array(annot.paths.iter().map(|p| points_array(p)).collect()));
            }
            AnnotKind::Polygon | AnnotKind::Polyline => {
                d.set("Vertices", points_array(annot.paths.first().map_or(&[][..], Vec::as_slice)));
            }
            AnnotKind::Stamp => {
                d.set("Subj", text_string(&annot.label));
                let icon = annot.icon.clone().unwrap_or_else(|| "Draft".into());
                d.set("Name", Object::Name(icon.into_bytes()));
            }
            AnnotKind::Note => {
                d.set("Name", Object::Name(b"Comment".to_vec()));
                d.set("Open", Object::Boolean(false));
            }
            AnnotKind::FileAttachment => {
                let (file_name, data) = annot
                    .attachment
                    .clone()
                    .ok_or_else(|| Error::Invalid("file attachment without file".into()))?;
                let mut params = Dictionary::new();
                params.set("Size", Object::Integer(data.len() as i64));
                let mut ef_dict = Dictionary::new();
                ef_dict.set("Type", Object::Name(b"EmbeddedFile".to_vec()));
                ef_dict.set("Params", Object::Dictionary(params));
                let mut stream = Stream::new(ef_dict, data);
                stream.compress()?;
                let stream_id = edit.add(stream);
                let mut ef = Dictionary::new();
                ef.set("F", Object::Reference(stream_id));
                let mut fs = Dictionary::new();
                fs.set("Type", Object::Name(b"Filespec".to_vec()));
                fs.set("F", text_string(&file_name));
                fs.set("UF", text_string(&file_name));
                fs.set("EF", Object::Dictionary(ef));
                d.set("FS", Object::Dictionary(fs));
                d.set("Name", Object::Name(b"Paperclip".to_vec()));
            }
            AnnotKind::Redact => {
                d.set("IC", color_array(style.fill.unwrap_or(Rgba::BLACK)));
            }
            _ => {}
        }
        if annot.cloudy {
            let mut be = Dictionary::new();
            be.set("S", Object::Name(b"C".to_vec()));
            be.set("I", Object::Integer(1));
            d.set("BE", Object::Dictionary(be));
        }
        if kind == AnnotKind::FreeText && annot.leader.len() >= 2 {
            let inner = annot.rect.normalized();
            d.set("IT", Object::Name(b"FreeTextCallout".to_vec()));
            d.set("CL", points_array(&annot.leader));
            d.set("LE", Object::Name(b"OpenArrow".to_vec()));
            d.set(
                "RD",
                Object::Array(vec![
                    real(inner.x0 - rect.x0),
                    real(inner.y0 - rect.y0),
                    real(rect.x1 - inner.x1),
                    real(rect.y1 - inner.y1),
                ]),
            );
        }
        if !annot.measure_label.is_empty() {
            let intent: &[u8] = match kind {
                AnnotKind::Line => b"LineDimension",
                AnnotKind::Polygon => b"PolygonDimension",
                _ => b"PolyLineDimension",
            };
            d.set("IT", Object::Name(intent.to_vec()));
            if annot.contents.is_empty() {
                d.set("Contents", text_string(&annot.measure_label));
            }
        }

        let annot_id = edit.add(Object::Dictionary(d));
        set_appearance(&mut edit, annot_id, &input)?;
        edit.annots_mut(page_id)?.push(Object::Reference(annot_id));
        edit.finish()
    })
}

/// Reads style details of an annotation.
pub fn annotation_details(
    bytes: &[u8],
    password: Option<&str>,
    page: usize,
    index: usize,
) -> Result<AnnotDetails> {
    guarded(|| {
        let edit = Edit::open(bytes, password)?;
        let page_id = edit.page_id(page)?;
        let entry = edit.annots(page_id)?.get(index).cloned().ok_or(Error::AnnotationOutOfRange(index))?;
        let dict = edit.resolve(&entry)?.as_dict()?;
        let kind = name(dict, b"Subtype").map(|s| kind_from_subtype(&s));
        let da = string(dict, b"DA").unwrap_or_default();
        Ok(AnnotDetails {
            kind,
            color: read_color(dict, b"C"),
            fill: read_color(dict, b"IC"),
            width: read_width(dict),
            opacity: dict.get(b"CA").ok().and_then(number).unwrap_or(1.0),
            font_size: font_size_from_da(&da).unwrap_or(12.0),
            subject: string(dict, b"Subj").unwrap_or_default(),
            restylable: kind.is_some_and(can_regenerate),
        })
    })
}

/// Changes an existing annotation.
pub fn update_annotation(
    bytes: &[u8],
    password: Option<&str>,
    page: usize,
    index: usize,
    update: &AnnotUpdate,
) -> Result<Vec<u8>> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let page_id = edit.page_id(page)?;
        let annot_id = edit.annotation_id(page_id, index)?;
        let now = pdf_date_now();
        {
            let d = edit.dict_mut(annot_id)?;
            let kind = name(d, b"Subtype").map(|s| kind_from_subtype(&s));
            if let Some(contents) = &update.contents {
                if contents.is_empty() {
                    d.remove(b"Contents");
                } else {
                    d.set("Contents", text_string(contents));
                }
            }
            if let Some(author) = &update.author {
                d.set("T", text_string(author));
            }
            if let Some(color) = update.color {
                d.set("C", color_array(color));
            }
            if let Some(fill) = update.fill {
                match fill {
                    Some(fill) => d.set("IC", color_array(fill)),
                    None => {
                        d.remove(b"IC");
                    }
                }
            }
            if let Some(width) = update.width {
                let mut bs = Dictionary::new();
                bs.set("Type", Object::Name(b"Border".to_vec()));
                bs.set("W", real(width.max(0.0)));
                bs.set("S", Object::Name(b"S".to_vec()));
                d.set("BS", Object::Dictionary(bs));
                d.remove(b"Border");
            }
            if let Some(opacity) = update.opacity {
                d.set("CA", real(opacity.clamp(0.05, 1.0)));
            }
            if kind == Some(AnnotKind::FreeText) && (update.font_size.is_some() || update.color.is_some()) {
                let da = string(d, b"DA").unwrap_or_default();
                let size = update.font_size.or_else(|| font_size_from_da(&da)).unwrap_or(12.0);
                let color = update.color.or_else(|| text_color_from_da(&da)).unwrap_or(Rgba::BLACK);
                let [r, g, b] = color.to_pdf();
                d.set(
                    "DA",
                    Object::string_literal(format!(
                        "/Helv {} Tf {} {} {} rg",
                        appearance::num(size),
                        appearance::num(r),
                        appearance::num(g),
                        appearance::num(b)
                    )),
                );
            }
            if let Some((dx, dy)) = update.translate {
                translate_annotation(d, dx, dy);
            }
            d.set("M", Object::string_literal(now));
        }

        let dict = edit.object(annot_id)?.as_dict()?.clone();
        let kind = name(&dict, b"Subtype").map(|s| kind_from_subtype(&s));
        let text_changed = update.contents.is_some() && kind == Some(AnnotKind::FreeText);
        if (update.changes_style() || text_changed)
            && let Some(input) = ap_input_from_dict(&dict)
        {
            set_appearance(&mut edit, annot_id, &input)?;
        }
        edit.finish()
    })
}

/// Moves all geometry of an annotation dictionary. The appearance stream is
/// kept: viewers map its bounding box onto the (moved) rectangle.
fn translate_annotation(d: &mut Dictionary, dx: f32, dy: f32) {
    let shift_pairs = |obj: &mut Object| {
        if let Object::Array(items) = obj {
            for (i, item) in items.iter_mut().enumerate() {
                if let Some(v) = number(item) {
                    *item = real(v + if i % 2 == 0 { dx } else { dy });
                }
            }
        }
    };
    for key in [&b"Rect"[..], b"QuadPoints", b"L", b"Vertices", b"CL"] {
        if let Ok(obj) = d.get_mut(key) {
            shift_pairs(obj);
        }
    }
    if let Ok(Object::Array(strokes)) = d.get_mut(b"InkList") {
        for stroke in strokes {
            shift_pairs(stroke);
        }
    }
}

/// Removes an annotation (and its pop-up) from a page.
pub fn delete_annotation(bytes: &[u8], password: Option<&str>, page: usize, index: usize) -> Result<Vec<u8>> {
    delete_annotations(bytes, password, page, &[index])
}

/// Removes several annotations together with their pop-ups and replies.
/// Deleted form field widgets are also removed from the form's field list.
pub fn delete_annotations(
    bytes: &[u8],
    password: Option<&str>,
    page: usize,
    indices: &[usize],
) -> Result<Vec<u8>> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let page_id = edit.page_id(page)?;
        let annots = edit.annots(page_id)?;
        let mut targets: Vec<ObjectId> = Vec::new();
        let mut popups: Vec<ObjectId> = Vec::new();
        for &index in indices {
            let entry = annots.get(index).ok_or(Error::AnnotationOutOfRange(index))?;
            if let Ok(id) = entry.as_reference() {
                targets.push(id);
            }
            if let Some(p) = edit
                .resolve(entry)?
                .as_dict()
                .ok()
                .and_then(|d| d.get(b"Popup").ok())
                .and_then(|p| p.as_reference().ok())
            {
                popups.push(p);
            }
        }
        let mut kept = Vec::with_capacity(annots.len());
        for (i, obj) in annots.into_iter().enumerate() {
            if indices.contains(&i) {
                continue;
            }
            let id = obj.as_reference().ok();
            if id.is_some_and(|id| popups.contains(&id)) {
                continue;
            }
            // Pop-ups and replies that belong to a removed annotation.
            if let Ok(dict) = edit.resolve(&obj).and_then(|o| Ok(o.as_dict()?)) {
                let refers = |key: &[u8]| {
                    dict.get(key)
                        .ok()
                        .and_then(|p| p.as_reference().ok())
                        .is_some_and(|p| targets.contains(&p))
                };
                let is_popup = dict.get(b"Subtype").ok().and_then(|s| s.as_name().ok()) == Some(b"Popup");
                if (refers(b"Parent") && is_popup) || refers(b"IRT") {
                    continue;
                }
            }
            kept.push(obj);
        }
        *edit.annots_mut(page_id)? = kept;
        remove_form_fields(&mut edit, &targets)?;
        edit.finish()
    })
}

/// Removes references to `ids` from the AcroForm field list.
fn remove_form_fields(edit: &mut Edit, ids: &[ObjectId]) -> Result<()> {
    let catalog_id = edit.prev().trailer.get(b"Root")?.as_reference()?;
    let Some(form) = edit.object(catalog_id)?.as_dict()?.get(b"AcroForm").ok().cloned() else {
        return Ok(());
    };
    let form_id = match form {
        Object::Reference(id) => id,
        Object::Dictionary(dict) => {
            let id = edit.add(Object::Dictionary(dict));
            edit.dict_mut(catalog_id)?.set("AcroForm", Object::Reference(id));
            id
        }
        _ => return Ok(()),
    };
    let fields = edit.object(form_id)?.as_dict()?.get(b"Fields").ok().cloned();
    let list = match &fields {
        Some(obj) => edit.resolve(obj)?.as_array()?.clone(),
        None => return Ok(()),
    };
    let filtered: Vec<Object> =
        list.iter().filter(|f| !f.as_reference().is_ok_and(|id| ids.contains(&id))).cloned().collect();
    if filtered.len() != list.len() {
        edit.dict_mut(form_id)?.set("Fields", Object::Array(filtered));
    }
    Ok(())
}

/// Target of a new link annotation.
#[derive(Debug, Clone, PartialEq)]
pub enum LinkSpec {
    Uri(String),
    Page(usize),
}

/// Adds link annotations to a page.
pub fn add_links(
    bytes: &[u8],
    password: Option<&str>,
    page: usize,
    links: &[(RectF, LinkSpec)],
) -> Result<Vec<u8>> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let page_id = edit.page_id(page)?;
        for (rect, spec) in links {
            let mut d = Dictionary::new();
            d.set("Type", Object::Name(b"Annot".to_vec()));
            d.set("Subtype", Object::Name(b"Link".to_vec()));
            d.set("Rect", rect_array(rect));
            d.set("P", Object::Reference(page_id));
            d.set("Border", Object::Array(vec![0.into(), 0.into(), 0.into()]));
            d.set("NM", text_string(&unique_name()));
            match spec {
                LinkSpec::Uri(uri) => {
                    let mut action = Dictionary::new();
                    action.set("S", Object::Name(b"URI".to_vec()));
                    action.set("URI", Object::string_literal(uri.as_bytes().to_vec()));
                    d.set("A", Object::Dictionary(action));
                }
                LinkSpec::Page(target) => {
                    let target_id = edit.page_id(*target)?;
                    d.set(
                        "Dest",
                        Object::Array(vec![Object::Reference(target_id), Object::Name(b"Fit".to_vec())]),
                    );
                }
            }
            let id = edit.add(Object::Dictionary(d));
            edit.annots_mut(page_id)?.push(Object::Reference(id));
        }
        edit.finish()
    })
}

/// Removes all links that open web addresses (URI actions). Returns the
/// new bytes and the number of removed links.
pub fn remove_web_links(bytes: &[u8], password: Option<&str>) -> Result<(Vec<u8>, usize)> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let mut removed = 0;
        for page_id in edit.page_ids() {
            let annots = edit.annots(page_id)?;
            let mut kept = Vec::with_capacity(annots.len());
            for obj in &annots {
                let is_web_link = edit.resolve(obj).ok().and_then(|o| o.as_dict().ok()).is_some_and(|d| {
                    name(d, b"Subtype").as_deref() == Some("Link")
                        && matches!(d.get(b"A").ok().and_then(|a| edit.resolve(a).ok()).and_then(|a| a.as_dict().ok()),
                            Some(a) if name(a, b"S").as_deref() == Some("URI"))
                });
                if is_web_link {
                    removed += 1;
                } else {
                    kept.push(obj.clone());
                }
            }
            if kept.len() != annots.len() {
                *edit.annots_mut(page_id)? = kept;
            }
        }
        Ok((edit.finish()?, removed))
    })
}
const INHERITABLE: [&[u8]; 4] = [b"Resources", b"MediaBox", b"CropBox", b"Rotate"];

/// Puts the pages into a new order. `order[i]` is the old index of the page
/// that becomes page `i`. Must be a permutation of all pages.
pub fn reorder_pages(bytes: &[u8], password: Option<&str>, order: &[usize]) -> Result<Vec<u8>> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let ids = edit.page_ids();
        let mut seen = vec![false; ids.len()];
        if order.len() != ids.len()
            || !order.iter().all(|&i| i < ids.len() && !std::mem::replace(&mut seen[i], true))
        {
            return Err(Error::Invalid("invalid page order".into()));
        }
        let root_pages = edit.prev().catalog()?.get(b"Pages")?.as_reference()?;

        for &page_id in &ids {
            // Copy inherited attributes down so the page no longer depends on
            // intermediate page tree nodes.
            let mut inherited: Vec<(Vec<u8>, Object)> = Vec::new();
            let page = edit.object(page_id)?.as_dict()?.clone();
            let mut parent = page.get(b"Parent").ok().and_then(|p| p.as_reference().ok());
            let mut guard = 0;
            while let Some(pid) = parent {
                guard += 1;
                if guard > 64 {
                    break;
                }
                let node = edit.object(pid)?.as_dict()?;
                for key in INHERITABLE {
                    if !page.has(key)
                        && !inherited.iter().any(|(k, _)| k == key)
                        && let Ok(value) = node.get(key)
                    {
                        inherited.push((key.to_vec(), value.clone()));
                    }
                }
                parent = node.get(b"Parent").ok().and_then(|p| p.as_reference().ok());
            }
            let page = edit.dict_mut(page_id)?;
            for (key, value) in inherited {
                page.set(key, value);
            }
            page.set("Parent", Object::Reference(root_pages));
        }

        let kids: Vec<Object> = order.iter().map(|&i| Object::Reference(ids[i])).collect();
        let root = edit.dict_mut(root_pages)?;
        root.set("Kids", Object::Array(kids));
        root.set("Count", Object::Integer(ids.len() as i64));
        edit.finish()
    })
}

/// Reads the document information dictionary.
pub fn read_metadata(bytes: &[u8], password: Option<&str>) -> Result<Metadata> {
    guarded(|| {
        let edit = Edit::open(bytes, password)?;
        let info = match edit.prev().trailer.get(b"Info") {
            Ok(obj) => edit.resolve(obj)?.as_dict()?.clone(),
            Err(_) => return Ok(Metadata::default()),
        };
        let get = |key: &[u8]| string(&info, key).unwrap_or_default();
        Ok(Metadata {
            title: get(b"Title"),
            author: get(b"Author"),
            subject: get(b"Subject"),
            keywords: get(b"Keywords"),
            creator: get(b"Creator"),
            producer: get(b"Producer"),
            created: format_pdf_date(&get(b"CreationDate")),
            modified: format_pdf_date(&get(b"ModDate")),
        })
    })
}

/// Writes title, author, subject and keywords into the information dictionary.
pub fn write_metadata(bytes: &[u8], password: Option<&str>, meta: &Metadata) -> Result<Vec<u8>> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let existing = edit.prev().trailer.get(b"Info").ok().cloned();
        let info_id = match existing {
            Some(Object::Reference(id)) => id,
            Some(Object::Dictionary(dict)) => edit.add(Object::Dictionary(dict)),
            _ => edit.add(Object::Dictionary(Dictionary::new())),
        };
        {
            let info = edit.dict_mut(info_id)?;
            for (key, value) in [
                ("Title", &meta.title),
                ("Author", &meta.author),
                ("Subject", &meta.subject),
                ("Keywords", &meta.keywords),
            ] {
                if value.is_empty() {
                    info.remove(key.as_bytes());
                } else {
                    info.set(key, text_string(value));
                }
            }
            info.set("ModDate", Object::string_literal(pdf_date_now()));
        }
        edit.inc.new_document.trailer.set("Info", Object::Reference(info_id));
        edit.finish()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_strings_round_trip() {
        for s in ["Hello", "Grüße aus Köln", "日本語", ""] {
            let obj = text_string(s);
            let Object::String(bytes, _) = obj else { panic!("not a string") };
            assert_eq!(decode_text_string(&bytes), s);
        }
    }

    #[test]
    fn dates() {
        assert_eq!(pdf_date(0), "D:19700101000000Z");
        assert_eq!(pdf_date(951_782_400), "D:20000229000000Z");
        assert_eq!(pdf_date(1_767_225_599), "D:20251231235959Z");
        assert_eq!(format_pdf_date("D:20240131124500+01'00'"), "2024-01-31 12:45");
        assert_eq!(format_pdf_date("garbage"), "garbage");
    }

    #[test]
    fn default_appearance_parsing() {
        assert_eq!(font_size_from_da("/Helv 14 Tf 0 0 1 rg"), Some(14.0));
        assert_eq!(text_color_from_da("/Helv 14 Tf 0 0 1 rg"), Some(Rgba::rgb(0, 0, 255)));
        assert_eq!(text_color_from_da("0.5 g /F1 9 Tf"), Some(Rgba::rgb(128, 128, 128)));
        assert_eq!(font_size_from_da("Tf"), None);
    }

    #[test]
    fn translation_moves_all_geometry() {
        let mut d = Dictionary::new();
        d.set("Rect", rect_array(&RectF::new(0.0, 0.0, 10.0, 10.0)));
        d.set("L", points_array(&[PointF::new(1.0, 2.0), PointF::new(3.0, 4.0)]));
        translate_annotation(&mut d, 5.0, -1.0);
        assert_eq!(read_rect(&d), Some(RectF::new(5.0, -1.0, 15.0, 9.0)));
        assert_eq!(numbers(d.get(b"L").ok()), vec![6.0, 1.0, 8.0, 3.0]);
    }

    #[test]
    fn invalid_input_is_rejected() {
        assert!(
            add_annotation(
                b"not a pdf",
                None,
                0,
                &NewAnnotation::new(AnnotKind::Square, AnnotStyle::default())
            )
            .is_err()
        );
        assert!(reorder_pages(b"%PDF-1.4 garbage", None, &[0]).is_err());
    }
}

/// An image in straight RGBA, used for image stamps.
#[derive(Debug, Clone, PartialEq)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

fn image_xobject(
    rgb: Vec<u8>,
    width: u32,
    height: u32,
    gray: bool,
    smask: Option<ObjectId>,
) -> Result<Stream> {
    let mut dict = Dictionary::new();
    dict.set("Type", Object::Name(b"XObject".to_vec()));
    dict.set("Subtype", Object::Name(b"Image".to_vec()));
    dict.set("Width", Object::Integer(i64::from(width)));
    dict.set("Height", Object::Integer(i64::from(height)));
    dict.set("ColorSpace", Object::Name(if gray { b"DeviceGray".to_vec() } else { b"DeviceRGB".to_vec() }));
    dict.set("BitsPerComponent", Object::Integer(8));
    if let Some(mask) = smask {
        dict.set("SMask", Object::Reference(mask));
    }
    let mut stream = Stream::new(dict, rgb);
    stream.compress()?;
    Ok(stream)
}

/// Adds an image stamp (e.g. a scanned signature) filling `rect`.
pub fn add_image_stamp(
    bytes: &[u8],
    password: Option<&str>,
    page: usize,
    rect: RectF,
    image: &RgbaImage,
    style: &AnnotStyle,
) -> Result<Vec<u8>> {
    guarded(|| {
        let (w, h) = (image.width, image.height);
        if w == 0 || h == 0 || image.rgba.len() != (w as usize) * (h as usize) * 4 {
            return Err(Error::Invalid("invalid image".into()));
        }
        let rect = rect.normalized();
        let mut edit = Edit::open(bytes, password)?;
        let page_id = edit.page_id(page)?;

        let mut rgb = Vec::with_capacity((w * h * 3) as usize);
        let mut alpha = Vec::with_capacity((w * h) as usize);
        for px in image.rgba.as_chunks::<4>().0 {
            rgb.extend_from_slice(&px[..3]);
            alpha.push(px[3]);
        }
        let smask = if alpha.iter().any(|&a| a != 255) {
            Some(edit.add(image_xobject(alpha, w, h, true, None)?))
        } else {
            None
        };
        let image_id = edit.add(image_xobject(rgb, w, h, false, smask)?);

        let content = format!(
            "q {} 0 0 {} {} {} cm /Im0 Do Q\n",
            appearance::num(rect.width()),
            appearance::num(rect.height()),
            appearance::num(rect.x0),
            appearance::num(rect.y0)
        );
        let mut xobjects = Dictionary::new();
        xobjects.set("Im0", Object::Reference(image_id));
        let mut resources = Dictionary::new();
        resources.set("XObject", Object::Dictionary(xobjects));
        let mut ap = Dictionary::new();
        ap.set("Type", Object::Name(b"XObject".to_vec()));
        ap.set("Subtype", Object::Name(b"Form".to_vec()));
        ap.set("BBox", rect_array(&rect));
        ap.set("Resources", Object::Dictionary(resources));
        let ap_id = edit.add(Stream::new(ap, content.into_bytes()));

        let now = pdf_date_now();
        let mut d = Dictionary::new();
        d.set("Type", Object::Name(b"Annot".to_vec()));
        d.set("Subtype", Object::Name(b"Stamp".to_vec()));
        d.set("Name", Object::Name(b"Image".to_vec()));
        d.set("Rect", rect_array(&rect));
        d.set("P", Object::Reference(page_id));
        d.set("NM", text_string(&unique_name()));
        d.set("M", Object::string_literal(now.clone()));
        d.set("CreationDate", Object::string_literal(now));
        d.set("F", Object::Integer(4));
        if !style.author.is_empty() {
            d.set("T", text_string(&style.author));
        }
        let mut ap_dict = Dictionary::new();
        ap_dict.set("N", Object::Reference(ap_id));
        d.set("AP", Object::Dictionary(ap_dict));
        let annot_id = edit.add(Object::Dictionary(d));
        edit.annots_mut(page_id)?.push(Object::Reference(annot_id));
        edit.finish()
    })
}
