//! Managed page marks: watermarks, backgrounds, headers/footers and Bates
//! numbers ("Seitenmarkierungen").
//!
//! Every mark is written as its own content stream, wrapped in a marked
//! content sequence `/Artifact <</Type /Pagination /Subtype /Watermark …>>`
//! and tagged with `/VersoloPDFMark (id)` in the stream dictionary. The definition
//! of the mark is stored in the catalog (`/VersoloPDFMarks`), so marks can be
//! listed, edited, reordered and removed later. Removing all marks of a kind
//! also strips artifacts of the same subtype written by other programs.

use std::collections::HashSet;

use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Object, ObjectId, Stream};
use serde::{Deserialize, Serialize};

use crate::appearance::{StdFont, num, win_ansi};
use crate::compose::{Align, TextFont};
use crate::error::{Error, Result};
use crate::geometry::RectF;
use crate::import::{Matrix, upright};
use crate::model::Rgba;
use crate::pages::{add_content, add_resource, page_rotation, transform_page_matrix, visible_box};
use crate::pdf_edit::{Edit, RgbaImage, guarded, image_xobject, real, text_string};

/// Upper bound for stored marks (all kinds).
const MAX_MARKS: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Kind {
    Watermark,
    Background,
    HeaderFooter,
    Bates,
}

impl Kind {
    /// Artifact subtypes of this kind (the first is written).
    fn subtypes(self) -> &'static [&'static [u8]] {
        match self {
            Kind::Watermark => &[b"Watermark"],
            Kind::Background => &[b"Background"],
            Kind::HeaderFooter => &[b"Header", b"Footer"],
            Kind::Bates => &[b"BatesN", b"Bates"],
        }
    }

    fn name(self) -> &'static str {
        match self {
            Kind::Watermark => "Watermark",
            Kind::Background => "Background",
            Kind::HeaderFooter => "HeaderFooter",
            Kind::Bates => "Bates",
        }
    }

    fn from_name(n: &[u8]) -> Option<Kind> {
        Some(match n {
            b"Watermark" => Kind::Watermark,
            b"Background" => Kind::Background,
            b"HeaderFooter" => Kind::HeaderFooter,
            b"Bates" => Kind::Bates,
            _ => return None,
        })
    }
}

/// Where the mark is visible ("Sichtbar auf").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Visibility {
    #[default]
    Always,
    ViewOnly,
    PrintOnly,
}

/// Text font settings ("Schriftart…").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FontSpec {
    pub font: TextFont,
    pub bold: bool,
    pub size: f32,
    pub color: Rgba,
}

impl Default for FontSpec {
    fn default() -> Self {
        Self { font: TextFont::Helvetica, bold: false, size: 12.0, color: Rgba::BLACK }
    }
}

impl FontSpec {
    fn std(&self) -> StdFont {
        match (self.font, self.bold) {
            (TextFont::Courier, _) => StdFont::Courier,
            (_, true) => StdFont::HelveticaBold,
            _ => StdFont::Helvetica,
        }
    }
}

/// Bates number format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BatesFormat {
    pub prefix: String,
    pub start: u64,
    pub digits: u32,
    pub suffix: String,
}

impl Default for BatesFormat {
    fn default() -> Self {
        Self { prefix: String::new(), start: 1, digits: 6, suffix: String::new() }
    }
}

impl BatesFormat {
    pub fn number(&self, index: u64) -> String {
        let digits = self.digits.min(20) as usize;
        format!("{}{:0digits$}{}", self.prefix, self.start + index, self.suffix)
    }
}

/// What is drawn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Source {
    Text {
        text: String,
        font: FontSpec,
        align: Align,
    },
    /// An image (stored as XObject) or a page of a PDF file.
    Image {
        name: String,
    },
    Color {
        color: Rgba,
    },
    /// Six texts: header left/centre/right, footer left/centre/right.
    HeaderFooter {
        texts: [String; 6],
        font: FontSpec,
        margins: [f32; 4],
        shrink: bool,
    },
}

/// A complete mark definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MarkDef {
    pub kind: Kind,
    pub source: Source,
    /// Counter-clockwise rotation in degrees.
    pub rotation: f32,
    pub opacity: f32,
    /// 1.0 = 100 %; relative to the page if `relative`.
    pub scale: f32,
    pub relative: bool,
    pub visibility: Visibility,
    pub dx: f32,
    pub dy: f32,
    pub h_from: Align,
    pub v_from: Align,
    pub behind: bool,
    /// Colour backgrounds: laid over the page content and multiplied with
    /// it (white turns into the colour, text stays dark), for pages that
    /// cover their background (scans, own white area).
    pub multiply: bool,
    /// Pages (0-based) the mark is placed on.
    pub pages: Vec<usize>,
    pub bates: BatesFormat,
}

impl Default for MarkDef {
    fn default() -> Self {
        Self {
            kind: Kind::Watermark,
            source: Source::Text { text: String::new(), font: FontSpec::default(), align: Align::Start },
            rotation: 0.0,
            opacity: 1.0,
            scale: 1.0,
            relative: true,
            visibility: Visibility::Always,
            dx: 0.0,
            dy: 0.0,
            h_from: Align::Center,
            v_from: Align::Center,
            behind: false,
            multiply: false,
            pages: Vec::new(),
            bates: BatesFormat::default(),
        }
    }
}

impl MarkDef {
    /// A colour background laid over the content in multiply mode.
    pub fn multiplies(&self) -> bool {
        self.multiply && self.kind == Kind::Background && matches!(self.source, Source::Color { .. })
    }

    /// Short description for lists.
    pub fn describe(&self) -> String {
        match &self.source {
            Source::Text { text, .. } => text.lines().next().unwrap_or("").to_string(),
            Source::Image { name } => name.clone(),
            Source::Color { color } => color.to_hex(),
            Source::HeaderFooter { texts, .. } => {
                texts.iter().filter(|t| !t.trim().is_empty()).cloned().collect::<Vec<_>>().join(" | ")
            }
        }
    }
}

/// Values for the text placeholders.
#[derive(Debug, Clone, Default)]
pub struct MarkContext {
    pub file_name: String,
    pub date: String,
    pub time: String,
    /// Added to the Bates index (several files).
    pub bates_offset: u64,
    /// Added to page numbers (previews of a single extracted page).
    pub page_base: usize,
    /// Page count shown instead of the real one (previews).
    pub total: Option<usize>,
}

/// Image data for a new image mark: pixels or a PDF page.
pub enum ImageData {
    Pixels(RgbaImage),
    PdfPage(Box<crate::import::Source>, usize),
    /// The XObject of an existing mark (editing).
    Existing(ObjectId),
}

/// Replaces %[Page], %[Pages], %[Date], %[Time], %[FileName], %[Bates]
/// (and the older <<page>> forms).
pub fn expand(text: &str, page: usize, pages: usize, ctx: &MarkContext, bates: &str) -> String {
    let stem = ctx.file_name.trim_end_matches(".pdf").trim_end_matches(".PDF");
    text.replace("%[Page]", &(page + 1).to_string())
        .replace("%[Pages]", &pages.to_string())
        .replace("%[Date]", &ctx.date)
        .replace("%[Time]", &ctx.time)
        .replace("%[FileName]", stem)
        .replace("%[Bates]", bates)
        .replace("<<page>>", &(page + 1).to_string())
        .replace("<<pages>>", &pages.to_string())
        .replace("<<date>>", &ctx.date)
        .replace("<<file>>", stem)
}

fn escape(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        match b {
            b'(' | b')' | b'\\' => {
                out.push('\\');
                out.push(b as char);
            }
            0x20..=0x7e => out.push(b as char),
            _ => out.push_str(&format!("\\{b:03o}")),
        }
    }
    out
}

fn font_dict(font: StdFont) -> Object {
    let mut f = Dictionary::new();
    f.set("Type", Object::Name(b"Font".to_vec()));
    f.set("Subtype", Object::Name(b"Type1".to_vec()));
    f.set("BaseFont", Object::Name(font.base_font().as_bytes().to_vec()));
    f.set("Encoding", Object::Name(b"WinAnsiEncoding".to_vec()));
    Object::Dictionary(f)
}

// ---------------------------------------------------------------------------
// Catalog storage

const STORE_KEY: &[u8] = b"VersoloPDFMarks";

/// A stored mark: id, definition and (for images) its XObject.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredMark {
    pub id: String,
    pub def: MarkDef,
    pub image: Option<ObjectId>,
}

fn read_store(edit: &Edit) -> Result<Vec<StoredMark>> {
    let catalog_id = edit.prev().trailer.get(b"Root")?.as_reference()?;
    let catalog = edit.object(catalog_id)?.as_dict()?;
    let Ok(list) = catalog.get(STORE_KEY) else { return Ok(Vec::new()) };
    let list = edit.resolve(list)?.as_array()?.clone();
    let mut out = Vec::new();
    for item in list.iter().take(MAX_MARKS) {
        let Ok(d) = edit.resolve(item).and_then(|o| Ok(o.as_dict()?)) else { continue };
        let id = crate::pdf_edit::string(d, b"ID").unwrap_or_default();
        let Some(def_text) = crate::pdf_edit::string(d, b"Def") else { continue };
        let Ok(def) = ron::from_str::<MarkDef>(&def_text) else { continue };
        let image = d.get(b"Image").ok().and_then(|o| o.as_reference().ok());
        out.push(StoredMark { id, def, image });
    }
    Ok(out)
}

fn write_store(edit: &mut Edit, marks: &[StoredMark]) -> Result<()> {
    let catalog_id = edit.prev().trailer.get(b"Root")?.as_reference()?;
    let mut list = Vec::new();
    for m in marks {
        let mut d = Dictionary::new();
        d.set("ID", text_string(&m.id));
        d.set("Kind", Object::Name(m.def.kind.name().as_bytes().to_vec()));
        let text = ron::to_string(&m.def).map_err(|e| Error::Invalid(e.to_string()))?;
        d.set("Def", text_string(&text));
        if let Some(img) = m.image {
            d.set("Image", Object::Reference(img));
        }
        list.push(Object::Dictionary(d));
    }
    let catalog = edit.dict_mut(catalog_id)?;
    if list.is_empty() {
        catalog.remove(STORE_KEY);
    } else {
        catalog.set(STORE_KEY.to_vec(), Object::Array(list));
    }
    Ok(())
}

/// The stored marks of a document.
pub fn list(bytes: &[u8], password: Option<&str>) -> Result<Vec<StoredMark>> {
    guarded(|| read_store(&Edit::open(bytes, password)?))
}

// ---------------------------------------------------------------------------
// Drawing

/// Optional content group for "Sichtbar auf".
fn visibility_group(edit: &mut Edit, id: &str, vis: Visibility) -> Result<Option<ObjectId>> {
    if vis == Visibility::Always {
        return Ok(None);
    }
    let (view, print) = match vis {
        Visibility::ViewOnly => (b"ON".as_slice(), b"OFF".as_slice()),
        _ => (b"OFF".as_slice(), b"ON".as_slice()),
    };
    let mut usage = Dictionary::new();
    let mut v = Dictionary::new();
    v.set("ViewState", Object::Name(view.to_vec()));
    let mut p = Dictionary::new();
    p.set("PrintState", Object::Name(print.to_vec()));
    usage.set("View", Object::Dictionary(v));
    usage.set("Print", Object::Dictionary(p));
    let mut ocg = Dictionary::new();
    ocg.set("Type", Object::Name(b"OCG".to_vec()));
    ocg.set("Name", text_string(&format!("VersoloPDF {id}")));
    ocg.set("Usage", Object::Dictionary(usage));
    let ocg_id = edit.add(Object::Dictionary(ocg));
    let catalog_id = edit.prev().trailer.get(b"Root")?.as_reference()?;
    let existing = edit.object(catalog_id)?.as_dict()?.get(b"OCProperties").ok().cloned();
    let mut props = match existing {
        Some(o) => edit.resolve(&o)?.as_dict().cloned().unwrap_or_default(),
        None => Dictionary::new(),
    };
    let get_array = |d: &Dictionary, key: &[u8], edit: &Edit| -> Vec<Object> {
        d.get(key)
            .ok()
            .and_then(|o| edit.resolve(o).ok())
            .and_then(|o| o.as_array().ok())
            .cloned()
            .unwrap_or_default()
    };
    let mut ocgs = get_array(&props, b"OCGs", edit);
    ocgs.push(Object::Reference(ocg_id));
    let mut d = props
        .get(b"D")
        .ok()
        .and_then(|o| edit.resolve(o).ok())
        .and_then(|o| o.as_dict().ok())
        .cloned()
        .unwrap_or_default();
    // Default state as shown on screen; printing switches by usage (/AS).
    let state_key: &[u8] = if view == b"ON" { b"ON" } else { b"OFF" };
    let mut states = get_array(&d, state_key, edit);
    states.push(Object::Reference(ocg_id));
    d.set(state_key.to_vec(), Object::Array(states));
    let mut auto = get_array(&d, b"AS", edit);
    for (event, category) in
        [(b"Print".as_slice(), b"Print".as_slice()), (b"View".as_slice(), b"View".as_slice())]
    {
        let mut a = Dictionary::new();
        a.set("Event", Object::Name(event.to_vec()));
        a.set("Category", Object::Array(vec![Object::Name(category.to_vec())]));
        a.set("OCGs", Object::Array(vec![Object::Reference(ocg_id)]));
        auto.push(Object::Dictionary(a));
    }
    d.set("AS", Object::Array(auto));
    props.set("OCGs", Object::Array(ocgs));
    props.set("D", Object::Dictionary(d));
    edit.dict_mut(catalog_id)?.set("OCProperties", Object::Dictionary(props));
    Ok(Some(ocg_id))
}

/// Centre of a block of `w`×`h` on a page of `tw`×`th` (display space).
fn place(def: &MarkDef, w: f32, h: f32, tw: f32, th: f32) -> (f32, f32) {
    let cx = match def.h_from {
        Align::Start => w / 2.0 + def.dx,
        Align::Center => tw / 2.0 + def.dx,
        Align::End => tw - w / 2.0 - def.dx,
    };
    let cy = match def.v_from {
        Align::Start => th - h / 2.0 - def.dy,
        Align::Center => th / 2.0 - def.dy,
        Align::End => h / 2.0 + def.dy,
    };
    (cx, cy)
}

/// Scale so a block of `w`×`h` rotated by `deg` fits `tw`×`th`.
fn fit_scale(w: f32, h: f32, deg: f32, tw: f32, th: f32) -> f32 {
    let (s, c) = deg.to_radians().sin_cos();
    let bw = (w * c).abs() + (h * s).abs();
    let bh = (w * s).abs() + (h * c).abs();
    (tw / bw.max(0.01)).min(th / bh.max(0.01))
}

/// Text lines of a block as (bytes, width).
fn lines_of(text: &str, font: StdFont, size: f32) -> Vec<(Vec<u8>, f32)> {
    text.lines()
        .map(|l| {
            let b = win_ansi(l);
            let w = font.text_width(&b, size);
            (b, w)
        })
        .collect()
}

/// Content of one mark on one page (display space → user space via `back`).
#[allow(clippy::too_many_arguments)]
fn mark_content(
    def: &MarkDef,
    text_for: &dyn Fn(&str) -> String,
    tw: f32,
    th: f32,
    back: Matrix,
    names: &Names,
    image_size: Option<(f32, f32)>,
) -> String {
    let mut out = String::new();
    match &def.source {
        Source::Color { color } => {
            let [r, g, b] = color.to_pdf();
            out.push_str(&format!(
                "{} {} {} {} rg 0 0 {} {} re f\n",
                back.operator(),
                num(r),
                num(g),
                num(b),
                num(tw),
                num(th)
            ));
        }
        Source::Image { .. } => {
            let (iw, ih) = image_size.unwrap_or((100.0, 100.0));
            let s =
                if def.relative { fit_scale(iw, ih, def.rotation, tw, th) * def.scale } else { def.scale };
            let (w, h) = (iw * s, ih * s);
            let (cx, cy) = place(def, w, h, tw, th);
            let m = Matrix::scale(w, h)
                .then(Matrix::translate(-w / 2.0, -h / 2.0))
                .then(Matrix::rotate(def.rotation))
                .then(Matrix::translate(cx, cy))
                .then(back);
            out.push_str(&format!("{} /{} Do\n", m.operator(), names.image));
        }
        Source::Text { text, font, align } => {
            let text = text_for(text);
            let std = font.std();
            let base = font.size.max(1.0);
            let lines = lines_of(&text, std, base);
            if lines.is_empty() {
                return out;
            }
            let w = lines.iter().map(|l| l.1).fold(0.0, f32::max);
            let lead = base * 1.2;
            let h = lead * lines.len() as f32;
            let s = if def.relative { fit_scale(w, h, def.rotation, tw, th) * def.scale } else { def.scale };
            let (bw, bh) = (w * s, h * s);
            let (cx, cy) = place(def, bw, bh, tw, th);
            let m = Matrix::scale(s, s)
                .then(Matrix::translate(-bw / 2.0, -bh / 2.0))
                .then(Matrix::rotate(def.rotation))
                .then(Matrix::translate(cx, cy))
                .then(back);
            let [r, g, b] = font.color.to_pdf();
            out.push_str(&format!(
                "{} {} {} {} rg BT /{} {} Tf\n",
                m.operator(),
                num(r),
                num(g),
                num(b),
                names.font,
                num(base)
            ));
            for (i, (bytes, lw)) in lines.iter().enumerate() {
                let x = match align {
                    Align::Start => 0.0,
                    Align::Center => (w - lw) / 2.0,
                    Align::End => w - lw,
                };
                let y = h - lead * (i as f32 + 1.0) + base * 0.25;
                out.push_str(&format!("1 0 0 1 {} {} Tm ({}) Tj\n", num(x), num(y), escape(bytes)));
            }
            out.push_str("ET\n");
        }
        Source::HeaderFooter { texts, font, margins, .. } => {
            let std = font.std();
            let size = font.size.max(1.0);
            let lead = size * 1.2;
            let [ml, mr, mt, mb] = *margins;
            let [r, g, b] = font.color.to_pdf();
            out.push_str(&format!(
                "{} {} {} {} rg BT /{} {} Tf\n",
                back.operator(),
                num(r),
                num(g),
                num(b),
                names.font,
                num(size)
            ));
            for (i, raw) in texts.iter().enumerate() {
                let text = text_for(raw);
                let lines = lines_of(&text, std, size);
                let header = i < 3;
                let n = lines.len() as f32;
                for (k, (bytes, lw)) in lines.iter().enumerate() {
                    let x = match i % 3 {
                        0 => ml,
                        1 => (tw - lw) / 2.0,
                        _ => tw - mr - lw,
                    };
                    let y = if header {
                        th - mt - size * 0.8 - lead * k as f32
                    } else {
                        mb + lead * (n - 1.0 - k as f32)
                    };
                    out.push_str(&format!("1 0 0 1 {} {} Tm ({}) Tj\n", num(x), num(y), escape(bytes)));
                }
            }
            out.push_str("ET\n");
        }
    }
    out
}

struct Names {
    font: String,
    gs: String,
    image: String,
    oc: String,
}

/// Adds the mark `def` (with `id`, or a new one) to the document. Returns the
/// bytes and the id. `image` is required for image marks that are new.
pub fn add(
    bytes: &[u8],
    password: Option<&str>,
    def: &MarkDef,
    id: Option<&str>,
    image: Option<ImageData>,
    ctx: &MarkContext,
) -> Result<(Vec<u8>, String)> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let mut store = read_store(&edit)?;
        if store.len() >= MAX_MARKS {
            return Err(Error::Invalid("too many page marks".into()));
        }
        let id = match id {
            Some(i) => i.to_string(),
            None => (1..)
                .map(|n| format!("m{n}"))
                .find(|c| !store.iter().any(|m| &m.id == c))
                .expect("infinite range"),
        };
        let pages = edit.page_ids();
        let total = pages.len();
        let names = Names {
            font: format!("MrF_{id}"),
            gs: format!("MrG_{id}"),
            image: format!("MrI_{id}"),
            oc: format!("MrOC_{id}"),
        };
        // Image: new pixels, a PDF page, or the XObject of the stored mark.
        let previous_image = store.iter().find(|m| m.id == id).and_then(|m| m.image);
        let (image_id, image_size) = match (&def.source, image) {
            (Source::Image { .. }, Some(ImageData::Pixels(img))) => {
                let mut rgb = Vec::with_capacity((img.width * img.height * 3) as usize);
                let mut alpha = Vec::with_capacity((img.width * img.height) as usize);
                for px in img.rgba.as_chunks::<4>().0 {
                    rgb.extend_from_slice(&px[..3]);
                    alpha.push(px[3]);
                }
                let smask = if alpha.iter().any(|a| *a != 255) {
                    Some(edit.add(image_xobject(alpha, img.width, img.height, true, None)?))
                } else {
                    None
                };
                let xid = edit.add(image_xobject(rgb, img.width, img.height, false, smask)?);
                // 150 dpi as natural size.
                (Some(xid), Some((img.width as f32 * 72.0 / 150.0, img.height as f32 * 72.0 / 150.0)))
            }
            (Source::Image { .. }, Some(ImageData::PdfPage(src, page))) => {
                let (xid, bbox, rot) = crate::import::import_page(&mut edit, src.as_ref(), page)?;
                // A wrapper form maps the page to the unit square like an image.
                let (up, w, h) = upright(bbox, rot);
                let m = up.then(Matrix::scale(1.0 / w, 1.0 / h));
                let mut res = Dictionary::new();
                let mut xo = Dictionary::new();
                xo.set("P", Object::Reference(xid));
                res.set("XObject", Object::Dictionary(xo));
                let mut d = Dictionary::new();
                d.set("Type", Object::Name(b"XObject".to_vec()));
                d.set("Subtype", Object::Name(b"Form".to_vec()));
                d.set("BBox", crate::pdf_edit::rect_array(&RectF::new(0.0, 0.0, 1.0, 1.0)));
                d.set("Resources", Object::Dictionary(res));
                d.set("VersoloPDFSize", Object::Array(vec![real(w), real(h)]));
                let wrapper = edit.add(Stream::new(d, format!("q {} /P Do Q\n", m.operator()).into_bytes()));
                (Some(wrapper), Some((w, h)))
            }
            (Source::Image { .. }, image @ (None | Some(ImageData::Existing(_)))) => {
                let given = match image {
                    Some(ImageData::Existing(i)) => Some(i),
                    _ => None,
                };
                let Some(prev) = given.or(previous_image) else {
                    return Err(Error::Invalid("image mark without image".into()));
                };
                let size = match edit.object(prev)? {
                    Object::Stream(s) => {
                        if let Ok(Object::Array(a)) = s.dict.get(b"VersoloPDFSize") {
                            let v: Vec<f32> = a.iter().filter_map(crate::pdf_edit::number).collect();
                            (v.first().copied().unwrap_or(100.0), v.get(1).copied().unwrap_or(100.0))
                        } else {
                            let w =
                                s.dict.get(b"Width").ok().and_then(|o| o.as_i64().ok()).unwrap_or(100) as f32;
                            let h = s.dict.get(b"Height").ok().and_then(|o| o.as_i64().ok()).unwrap_or(100)
                                as f32;
                            (w * 72.0 / 150.0, h * 72.0 / 150.0)
                        }
                    }
                    _ => (100.0, 100.0),
                };
                (Some(prev), Some(size))
            }
            _ => (None, None),
        };
        let ocg = visibility_group(&mut edit, &id, def.visibility)?;
        let gs = {
            let mut d = Dictionary::new();
            d.set("Type", Object::Name(b"ExtGState".to_vec()));
            d.set("CA", real(def.opacity.clamp(0.0, 1.0)));
            d.set("ca", real(def.opacity.clamp(0.0, 1.0)));
            if def.multiplies() {
                d.set("BM", Object::Name(b"Multiply".to_vec()));
            }
            edit.add(Object::Dictionary(d))
        };
        let font = match &def.source {
            Source::Text { font, .. } | Source::HeaderFooter { font, .. } => Some(font.std()),
            _ => None,
        };
        let subtype = String::from_utf8_lossy(def.kind.subtypes()[0]).into_owned();
        let selected: Vec<usize> = def.pages.iter().copied().filter(|p| *p < total).collect();
        for (index, &page) in selected.iter().enumerate() {
            let page_id = pages[page];
            let vb = visible_box(&edit, page_id);
            let rot = page_rotation(&edit, page_id);
            let (up, tw, th) = upright(vb, rot);
            let back = up.invert();
            // Header/footer may shrink the page content to make room.
            if let Source::HeaderFooter { texts, font: f, margins, shrink: true } = &def.source {
                let size = f.size.max(1.0) * 1.2;
                let rows = |range: std::ops::Range<usize>| {
                    texts[range].iter().map(|t| t.lines().count()).max().unwrap_or(0) as f32
                };
                let top = if rows(0..3) > 0.0 { margins[2] + rows(0..3) * size } else { 0.0 };
                let bottom = if rows(3..6) > 0.0 { margins[3] + rows(3..6) * size } else { 0.0 };
                if top + bottom > 0.0 && top + bottom < th * 0.5 {
                    let s = (th - top - bottom) / th;
                    let m = up
                        .then(Matrix::scale(s, s))
                        .then(Matrix::translate(tw * (1.0 - s) / 2.0, bottom))
                        .then(back);
                    transform_page_matrix(&mut edit, page_id, m, 0.0)?;
                }
            }
            let bates = def.bates.number(ctx.bates_offset + index as u64);
            let shown_total = ctx.total.unwrap_or(total);
            let text_for = |t: &str| expand(t, page + ctx.page_base, shown_total, ctx, &bates);
            let body = mark_content(def, &text_for, tw, th, back, &names, image_size);
            let mut content = String::new();
            if ocg.is_some() {
                content.push_str(&format!("/OC /{} BDC\n", names.oc));
            }
            content.push_str(&format!(
                "/Artifact <</Type /Pagination /Subtype /{subtype} /VersoloPDF ({id})>> BDC\nq /{} gs\n",
                names.gs
            ));
            content.push_str(&body);
            content.push_str("Q\nEMC\n");
            if ocg.is_some() {
                content.push_str("EMC\n");
            }
            if let Some(f) = font {
                add_resource(&mut edit, page_id, b"Font", &names.font, font_dict(f))?;
            }
            add_resource(&mut edit, page_id, b"ExtGState", &names.gs, Object::Reference(gs))?;
            if let Some(img) = image_id {
                add_resource(&mut edit, page_id, b"XObject", &names.image, Object::Reference(img))?;
            }
            if let Some(oc) = ocg {
                add_resource(&mut edit, page_id, b"Properties", &names.oc, Object::Reference(oc))?;
            }
            let behind = (def.behind || def.kind == Kind::Background) && !def.multiplies();
            add_content(&mut edit, page_id, content.into_bytes(), behind)?;
            tag_last_stream(&mut edit, page_id, &id, def.kind, behind)?;
        }
        store.retain(|m| m.id != id);
        store.push(StoredMark { id: id.clone(), def: def.clone(), image: image_id });
        write_store(&mut edit, &store)?;
        Ok((edit.finish()?, id))
    })
}

/// Marks the content stream just added by `add_content` with the mark id.
fn tag_last_stream(edit: &mut Edit, page_id: ObjectId, id: &str, kind: Kind, behind: bool) -> Result<()> {
    let contents = edit.object(page_id)?.as_dict()?.get(b"Contents")?.clone();
    let list = edit.resolve(&contents)?.as_array()?.clone();
    let target = if behind { list.first() } else { list.last() };
    if let Some(Object::Reference(sid)) = target
        && let Object::Stream(s) = edit.object_mut(*sid)?
    {
        s.dict.set("VersoloPDFMark", text_string(id));
        s.dict.set("VersoloPDFKind", Object::Name(kind.name().as_bytes().to_vec()));
    }
    Ok(())
}

/// Which marks to remove.
pub struct RemoveFilter<'a> {
    pub kind: Option<Kind>,
    /// Only these ids (None = all of `kind`).
    pub ids: Option<&'a [String]>,
    /// Also strip matching artifacts written by other programs.
    pub foreign: bool,
}

/// Removes marks. Returns the bytes and the number of removed page marks.
pub fn remove(bytes: &[u8], password: Option<&str>, filter: &RemoveFilter) -> Result<(Vec<u8>, usize)> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let store = read_store(&edit)?;
        let matches = |id: &str, kind: Option<Kind>| {
            filter.ids.is_none_or(|ids| ids.iter().any(|i| i == id))
                && (filter.kind.is_none() || kind == filter.kind)
        };
        let mut removed = 0;
        let subtypes: HashSet<&[u8]> =
            filter.kind.map(|k| k.subtypes().iter().copied().collect()).unwrap_or_default();
        for page_id in edit.page_ids() {
            let Ok(contents) = edit.object(page_id)?.as_dict()?.get(b"Contents").cloned() else { continue };
            let list: Vec<Object> = match edit.resolve(&contents)? {
                Object::Array(a) => a.clone(),
                Object::Stream(_) => vec![contents.clone()],
                _ => continue,
            };
            let mut kept = Vec::new();
            let mut changed = false;
            for entry in list {
                let Ok(sid) = entry.as_reference() else {
                    kept.push(entry);
                    continue;
                };
                let tag = match edit.object(sid) {
                    Ok(Object::Stream(s)) => s
                        .dict
                        .get(b"VersoloPDFMark")
                        .ok()
                        .and_then(|o| match o {
                            Object::String(b, _) => Some(String::from_utf8_lossy(b).into_owned()),
                            _ => None,
                        })
                        .map(|id| {
                            (
                                id,
                                s.dict
                                    .get(b"VersoloPDFKind")
                                    .ok()
                                    .and_then(|k| k.as_name().ok())
                                    .and_then(Kind::from_name),
                            )
                        }),
                    _ => None,
                };
                match tag {
                    Some((id, kind)) if matches(&id, kind) => {
                        removed += 1;
                        changed = true;
                    }
                    Some(_) => kept.push(entry),
                    None if filter.foreign && !subtypes.is_empty() => {
                        if let Some(n) = strip_artifacts(&mut edit, sid, &subtypes)? {
                            removed += n;
                        }
                        kept.push(entry);
                    }
                    None => kept.push(entry),
                }
            }
            if changed {
                edit.dict_mut(page_id)?.set("Contents", Object::Array(kept));
            }
        }
        let remaining: Vec<StoredMark> =
            store.into_iter().filter(|m| !matches(&m.id, Some(m.def.kind))).collect();
        write_store(&mut edit, &remaining)?;
        Ok((edit.finish()?, removed))
    })
}

/// Removes `/Artifact … /Subtype /X … BDC … EMC` blocks with a subtype in
/// `subtypes` from a content stream. Returns the number of removed blocks.
fn strip_artifacts(edit: &mut Edit, sid: ObjectId, subtypes: &HashSet<&[u8]>) -> Result<Option<usize>> {
    let Object::Stream(s) = edit.object(sid)? else { return Ok(None) };
    let data = s.decompressed_content().unwrap_or_else(|_| s.content.clone());
    // Quick test before parsing.
    if !data.windows(8).any(|w| w == b"Artifact") {
        return Ok(None);
    }
    let Ok(content) = Content::decode(&data) else { return Ok(None) };
    let mut out: Vec<Operation> = Vec::with_capacity(content.operations.len());
    let mut depth_skip: Option<usize> = None;
    let mut depth = 0usize;
    let mut count = 0;
    for op in content.operations {
        match op.operator.as_str() {
            "BDC" | "BMC" => {
                depth += 1;
                if depth_skip.is_none() && op.operator == "BDC" && is_mark_artifact(&op, subtypes) {
                    depth_skip = Some(depth);
                    count += 1;
                    continue;
                }
            }
            "EMC" => {
                let closing = depth;
                depth = depth.saturating_sub(1);
                if depth_skip == Some(closing) {
                    depth_skip = None;
                    continue;
                }
            }
            _ => {}
        }
        if depth_skip.is_none() {
            out.push(op);
        }
    }
    if count == 0 {
        return Ok(Some(0));
    }
    let encoded = Content { operations: out }.encode()?;
    if let Object::Stream(s) = edit.object_mut(sid)? {
        s.dict.remove(b"Filter");
        s.dict.remove(b"DecodeParms");
        s.set_plain_content(encoded);
        s.compress()?;
    }
    Ok(Some(count))
}

fn is_mark_artifact(op: &Operation, subtypes: &HashSet<&[u8]>) -> bool {
    if op.operands.first().and_then(|o| o.as_name().ok()) != Some(b"Artifact") {
        return false;
    }
    match op.operands.get(1) {
        Some(Object::Dictionary(d)) => {
            d.get(b"Subtype").ok().and_then(|s| s.as_name().ok()).is_some_and(|s| subtypes.contains(s))
        }
        _ => false,
    }
}

/// Changes the order of the stored marks (redraws are done by the caller).
pub fn set_order(bytes: &[u8], password: Option<&str>, ids: &[String]) -> Result<Vec<u8>> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let mut store = read_store(&edit)?;
        store.sort_by_key(|m| ids.iter().position(|i| *i == m.id).unwrap_or(usize::MAX));
        write_store(&mut edit, &store)?;
        edit.finish()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_and_bates() {
        let ctx = MarkContext {
            file_name: "Bericht.pdf".into(),
            date: "26.10.2026".into(),
            ..MarkContext::default()
        };
        assert_eq!(
            expand("Seite %[Page] von %[Pages] – %[FileName]", 1, 5, &ctx, ""),
            "Seite 2 von 5 – Bericht"
        );
        assert_eq!(expand("<<page>>/<<pages>>", 0, 3, &ctx, ""), "1/3");
        let b = BatesFormat { prefix: "AB".into(), start: 100, digits: 6, suffix: "-X".into() };
        assert_eq!(b.number(5), "AB000105-X");
        assert!(fit_scale(100.0, 20.0, 0.0, 200.0, 200.0) - 2.0 < 1e-4);
    }
}
