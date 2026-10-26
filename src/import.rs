//! Importing pages of another PDF as Form XObjects ("page graphics").
//!
//! Used by "Seiten ersetzen" (keeping the target page object with its
//! comments) and "Seiten überlagern". Objects are deep-copied with fresh
//! numbers; page-tree links (/Parent, /P) are not followed.

use std::collections::HashMap;

use lopdf::{Dictionary, Document, LoadOptions, Object, ObjectId, Stream};

use crate::appearance::num;
use crate::error::{Error, Result};
use crate::geometry::RectF;
use crate::pdf_edit::{Edit, MAX_DECOMPRESSED_STREAM, guarded, rect_array};

/// Upper bound for objects copied from one source page.
const MAX_COPIED: usize = 200_000;
const MAX_DEPTH: usize = 64;

/// A source document loaded for importing.
pub struct Source {
    doc: Document,
}

impl Source {
    pub fn load(bytes: &[u8], password: Option<&str>) -> Result<Self> {
        guarded(|| {
            let mut options = match password {
                Some(pw) => LoadOptions::with_password(pw),
                None => LoadOptions::default(),
            };
            options.max_decompressed_size = Some(MAX_DECOMPRESSED_STREAM);
            let doc = Document::load_mem_with_options(bytes, options)?;
            if doc.is_encrypted() {
                return Err(Error::EncryptedNotSupported);
            }
            Ok(Self { doc })
        })
    }

    pub fn page_count(&self) -> usize {
        self.doc.get_pages().len()
    }

    fn page_id(&self, page: usize) -> Result<ObjectId> {
        let no = u32::try_from(page + 1).map_err(|_| Error::PageOutOfRange(page))?;
        self.doc.get_pages().get(&no).copied().ok_or(Error::PageOutOfRange(page))
    }

    /// Looks up an (inheritable) page attribute.
    fn inherited(&self, page_id: ObjectId, key: &[u8]) -> Option<Object> {
        let mut id = page_id;
        for _ in 0..MAX_DEPTH {
            let dict = self.doc.get_dictionary(id).ok()?;
            if let Ok(v) = dict.get(key) {
                return Some(v.clone());
            }
            id = dict.get(b"Parent").ok()?.as_reference().ok()?;
        }
        None
    }

    fn rect(&self, obj: Option<Object>) -> Option<RectF> {
        let obj = match obj? {
            Object::Reference(id) => self.doc.get_object(id).ok()?.clone(),
            o => o,
        };
        let v: Vec<f32> = obj.as_array().ok()?.iter().filter_map(crate::pdf_edit::number).collect();
        (v.len() == 4).then(|| RectF::new(v[0], v[1], v[2], v[3]).normalized())
    }

    /// Visible box (crop box within the media box) and rotation of a page.
    pub fn page_box(&self, page: usize) -> Result<(RectF, i32)> {
        let id = self.page_id(page)?;
        let media = self.rect(self.inherited(id, b"MediaBox")).unwrap_or(RectF::new(0.0, 0.0, 595.0, 842.0));
        let crop = self.rect(self.inherited(id, b"CropBox")).unwrap_or(media);
        let rotate = self.inherited(id, b"Rotate").and_then(|r| r.as_i64().ok()).unwrap_or(0);
        Ok((crop, (rotate.rem_euclid(360) as i32 / 90) * 90))
    }
}

fn deep_copy(
    edit: &mut Edit,
    src: &Document,
    obj: &Object,
    map: &mut HashMap<ObjectId, ObjectId>,
    depth: usize,
) -> Result<Object> {
    if depth > MAX_DEPTH {
        return Ok(Object::Null);
    }
    Ok(match obj {
        Object::Reference(id) => {
            if let Some(new) = map.get(id) {
                return Ok(Object::Reference(*new));
            }
            if map.len() >= MAX_COPIED {
                return Err(Error::Invalid("source page is too complex".into()));
            }
            let Ok(target) = src.get_object(*id) else { return Ok(Object::Null) };
            let target = target.clone();
            // Reserve the number first so cycles resolve to it.
            let new_id = edit.add(Object::Null);
            map.insert(*id, new_id);
            let copied = deep_copy(edit, src, &target, map, depth + 1)?;
            edit.inc.new_document.objects.insert(new_id, copied);
            Object::Reference(new_id)
        }
        Object::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(deep_copy(edit, src, item, map, depth + 1)?);
            }
            Object::Array(out)
        }
        Object::Dictionary(dict) => Object::Dictionary(copy_dict(edit, src, dict, map, depth)?),
        Object::Stream(stream) => {
            let dict = copy_dict(edit, src, &stream.dict, map, depth)?;
            let mut s = Stream::new(dict, stream.content.clone());
            s.allows_compression = stream.allows_compression;
            Object::Stream(s)
        }
        other => other.clone(),
    })
}

fn copy_dict(
    edit: &mut Edit,
    src: &Document,
    dict: &Dictionary,
    map: &mut HashMap<ObjectId, ObjectId>,
    depth: usize,
) -> Result<Dictionary> {
    let mut out = Dictionary::new();
    for (k, v) in dict.iter() {
        // Never pull in the page tree of the source document.
        if k.as_slice() == b"Parent" || k.as_slice() == b"P" {
            continue;
        }
        out.set(k.clone(), deep_copy(edit, src, v, map, depth + 1)?);
    }
    Ok(out)
}

/// Copies page `page` of `src` into `edit` as a Form XObject. Returns its id,
/// its bounding box (source user space) and the source rotation.
pub(crate) fn import_page(edit: &mut Edit, src: &Source, page: usize) -> Result<(ObjectId, RectF, i32)> {
    let page_id = src.page_id(page)?;
    let (bbox, rotation) = src.page_box(page)?;
    let mut content = Vec::new();
    let contents = src.doc.get_dictionary(page_id)?.get(b"Contents").ok().cloned();
    let ids: Vec<ObjectId> = match contents {
        Some(Object::Reference(id)) => match src.doc.get_object(id)? {
            Object::Array(a) => a.iter().filter_map(|o| o.as_reference().ok()).collect(),
            _ => vec![id],
        },
        Some(Object::Array(a)) => a.iter().filter_map(|o| o.as_reference().ok()).collect(),
        _ => Vec::new(),
    };
    for id in ids {
        if let Ok(Object::Stream(s)) = src.doc.get_object(id) {
            let data = s.decompressed_content().unwrap_or_else(|_| s.content.clone());
            content.extend_from_slice(&data);
            content.push(b'\n');
        }
    }
    let mut map = HashMap::new();
    let resources = match src.inherited(page_id, b"Resources") {
        Some(r) => deep_copy(edit, &src.doc, &r, &mut map, 0)?,
        None => Object::Dictionary(Dictionary::new()),
    };
    let mut dict = Dictionary::new();
    dict.set("Type", Object::Name(b"XObject".to_vec()));
    dict.set("Subtype", Object::Name(b"Form".to_vec()));
    dict.set("BBox", rect_array(&bbox));
    dict.set("Resources", resources);
    if let Ok(group) = src.doc.get_dictionary(page_id).and_then(|d| d.get(b"Group")).cloned() {
        dict.set("Group", deep_copy(edit, &src.doc, &group, &mut map, 0)?);
    }
    let mut stream = Stream::new(dict, content);
    stream.compress()?;
    Ok((edit.add(stream), bbox, rotation))
}

/// An affine matrix `[a b c d e f]` as used by the `cm` operator.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Matrix(pub [f32; 6]);

impl Matrix {
    pub const IDENTITY: Matrix = Matrix([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    /// `self` applied after `first`.
    pub fn then(self, next: Matrix) -> Matrix {
        let [a, b, c, d, e, f] = self.0;
        let [a2, b2, c2, d2, e2, f2] = next.0;
        Matrix([
            a * a2 + b * c2,
            a * b2 + b * d2,
            c * a2 + d * c2,
            c * b2 + d * d2,
            e * a2 + f * c2 + e2,
            e * b2 + f * d2 + f2,
        ])
    }

    pub fn translate(x: f32, y: f32) -> Matrix {
        Matrix([1.0, 0.0, 0.0, 1.0, x, y])
    }

    pub fn scale(sx: f32, sy: f32) -> Matrix {
        Matrix([sx, 0.0, 0.0, sy, 0.0, 0.0])
    }

    /// Counter-clockwise rotation by `degrees`.
    pub fn rotate(degrees: f32) -> Matrix {
        let (s, c) = degrees.to_radians().sin_cos();
        Matrix([c, s, -s, c, 0.0, 0.0])
    }

    pub fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        let [a, b, c, d, e, f] = self.0;
        (a * x + c * y + e, b * x + d * y + f)
    }

    pub fn operator(&self) -> String {
        let v: Vec<String> = self.0.iter().map(|x| num(*x)).collect();
        format!("{} cm", v.join(" "))
    }

    /// The inverse matrix (identity if not invertible).
    pub fn invert(&self) -> Matrix {
        let [a, b, c, d, e, f] = self.0;
        let det = a * d - b * c;
        if det.abs() < 1e-9 {
            return Matrix::IDENTITY;
        }
        Matrix([d / det, -b / det, -c / det, a / det, (c * f - d * e) / det, (b * e - a * f) / det])
    }
}

// ---------------------------------------------------------------------------
// Several pages per sheet ("Seiten zusammenführen")

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum SheetSize {
    /// Computed from the page size and the grid ("Berechnen").
    Compute,
    #[default]
    Standard,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum PageOrder {
    /// Left to right, then the next row.
    #[default]
    Horizontal,
    /// Top to bottom, then the next column.
    Vertical,
}

/// Layout of [`nup`].
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct NUpLayout {
    pub sheet_mode: SheetSize,
    /// Sheet size in points (portrait, for Standard/Custom).
    pub sheet: (f32, f32),
    pub landscape: bool,
    /// Margins left, right, top, bottom.
    pub margins: [f32; 4],
    pub columns: u32,
    pub rows: u32,
    pub order: PageOrder,
    /// Right to left (horizontal) or bottom to top (vertical).
    pub reverse_order: bool,
    pub h_align: crate::compose::Align,
    pub v_align: crate::compose::Align,
    pub h_gap: f32,
    pub v_gap: f32,
    /// Turn pages by 90° when that makes them larger in their cell.
    pub auto_rotate: bool,
    /// Draw the border of every placed page.
    pub border: bool,
    /// Never scale pages above 100 %.
    pub no_enlarge: bool,
}

impl Default for NUpLayout {
    fn default() -> Self {
        Self {
            sheet_mode: SheetSize::Standard,
            sheet: (595.28, 841.89),
            landscape: false,
            margins: [0.0; 4],
            columns: 2,
            rows: 3,
            order: PageOrder::Horizontal,
            reverse_order: false,
            h_align: crate::compose::Align::Center,
            v_align: crate::compose::Align::Center,
            h_gap: 0.0,
            v_gap: 0.0,
            auto_rotate: false,
            border: false,
            no_enlarge: false,
        }
    }
}

impl NUpLayout {
    pub fn per_sheet(&self) -> usize {
        (self.columns.clamp(1, 16) * self.rows.clamp(1, 16)) as usize
    }

    /// Sheet size in points for pages of `page` size.
    pub fn sheet_size(&self, page: (f32, f32)) -> (f32, f32) {
        let (cols, rows) = (self.columns.clamp(1, 16) as f32, self.rows.clamp(1, 16) as f32);
        let [ml, mr, mt, mb] = self.margins;
        let (w, h) = match self.sheet_mode {
            SheetSize::Compute => (
                page.0 * cols + self.h_gap * (cols - 1.0) + ml + mr,
                page.1 * rows + self.v_gap * (rows - 1.0) + mt + mb,
            ),
            _ => self.sheet,
        };
        if self.sheet_mode != SheetSize::Compute && self.landscape != (w > h) { (h, w) } else { (w, h) }
    }
}

/// A PDF with empty pages of the given sizes.
fn blank_pdf(sizes: &[(f32, f32)]) -> Result<Vec<u8>> {
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let mut kids = Vec::new();
    for (w, h) in sizes {
        let mut page = Dictionary::new();
        page.set("Type", Object::Name(b"Page".to_vec()));
        page.set("Parent", Object::Reference(pages_id));
        page.set("MediaBox", rect_array(&RectF::new(0.0, 0.0, *w, *h)));
        page.set("Resources", Object::Dictionary(Dictionary::new()));
        kids.push(Object::Reference(doc.add_object(page)));
    }
    let mut pages = Dictionary::new();
    pages.set("Type", Object::Name(b"Pages".to_vec()));
    pages.set("Count", Object::Integer(kids.len() as i64));
    pages.set("Kids", Object::Array(kids));
    doc.objects.insert(pages_id, Object::Dictionary(pages));
    let mut catalog = Dictionary::new();
    catalog.set("Type", Object::Name(b"Catalog".to_vec()));
    catalog.set("Pages", Object::Reference(pages_id));
    let catalog_id = doc.add_object(catalog);
    doc.trailer.set("Root", Object::Reference(catalog_id));
    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(|e| Error::Structure(e.to_string()))?;
    Ok(out)
}

/// Places `pages` of `src` on new sheets, `layout.per_sheet()` per sheet.
pub fn nup(src: &Source, pages: &[usize], layout: &NUpLayout) -> Result<Vec<u8>> {
    guarded(|| {
        let Some(&first) = pages.first() else { return Err(Error::Invalid("no pages".into())) };
        let (fbox, frot) = src.page_box(first)?;
        let (_, fw, fh) = upright(fbox, frot);
        let (sw, sh) = layout.sheet_size((fw, fh));
        if !(10.0..=14_400.0).contains(&sw) || !(10.0..=14_400.0).contains(&sh) {
            return Err(Error::Invalid("invalid sheet size".into()));
        }
        let per = layout.per_sheet();
        let sheets = pages.len().div_ceil(per);
        let mut edit = Edit::open(&blank_pdf(&vec![(sw, sh); sheets])?, None)?;
        let (cols, rows) = (layout.columns.clamp(1, 16), layout.rows.clamp(1, 16));
        let [ml, mr, mt, mb] = layout.margins;
        let cell_w = ((sw - ml - mr - layout.h_gap * (cols - 1) as f32) / cols as f32).max(1.0);
        let cell_h = ((sh - mt - mb - layout.v_gap * (rows - 1) as f32) / rows as f32).max(1.0);
        let mut imported: HashMap<usize, (ObjectId, RectF, i32)> = HashMap::new();
        for (sheet, chunk) in pages.chunks(per).enumerate() {
            let sheet_id = edit.page_id(sheet)?;
            let mut content = String::new();
            let mut xobjects = Dictionary::new();
            for (slot, &page) in chunk.iter().enumerate() {
                let slot = slot as u32;
                let (mut col, mut row) = match layout.order {
                    PageOrder::Horizontal => (slot % cols, slot / cols),
                    PageOrder::Vertical => (slot / rows, slot % rows),
                };
                if layout.reverse_order {
                    match layout.order {
                        PageOrder::Horizontal => col = cols - 1 - col,
                        PageOrder::Vertical => row = rows - 1 - row,
                    }
                }
                let entry = match imported.get(&page) {
                    Some(e) => *e,
                    None => {
                        let e = import_page(&mut edit, src, page)?;
                        imported.insert(page, e);
                        e
                    }
                };
                let (xobj, bbox, rot) = entry;
                let (mut m, mut pw, mut ph) = upright(bbox, rot);
                if layout.auto_rotate {
                    let s0 = (cell_w / pw).min(cell_h / ph);
                    let s90 = (cell_w / ph).min(cell_h / pw);
                    if s90 > s0 * 1.01 {
                        m = m.then(Matrix::rotate(90.0)).then(Matrix::translate(ph, 0.0));
                        std::mem::swap(&mut pw, &mut ph);
                    }
                }
                let mut s = (cell_w / pw).min(cell_h / ph);
                if layout.no_enlarge {
                    s = s.min(1.0);
                }
                let (w, h) = (pw * s, ph * s);
                let cx = ml + col as f32 * (cell_w + layout.h_gap);
                let top = sh - mt - row as f32 * (cell_h + layout.v_gap);
                use crate::compose::Align;
                let x = match layout.h_align {
                    Align::Start => cx,
                    Align::Center => cx + (cell_w - w) / 2.0,
                    Align::End => cx + cell_w - w,
                };
                let y = match layout.v_align {
                    Align::Start => top - h,
                    Align::Center => top - (cell_h + h) / 2.0,
                    Align::End => top - cell_h,
                };
                let place = m.then(Matrix::scale(s, s)).then(Matrix::translate(x, y));
                let name = format!("MrPg{slot}");
                xobjects.set(name.clone().into_bytes(), Object::Reference(xobj));
                content.push_str(&format!(
                    "q {} re W n {} /{name} Do Q\n",
                    rect_ops(x, y, w, h),
                    place.operator()
                ));
                if layout.border {
                    content.push_str(&format!("q 0.5 w 0 G {} re S Q\n", rect_ops(x, y, w, h)));
                }
            }
            let mut resources = Dictionary::new();
            resources.set("XObject", Object::Dictionary(xobjects));
            let content_id = edit.add(Stream::new(Dictionary::new(), content.into_bytes()));
            let dict = edit.dict_mut(sheet_id)?;
            dict.set("Resources", Object::Dictionary(resources));
            dict.set("Contents", Object::Reference(content_id));
        }
        // A fresh file: write it completely instead of as an update.
        let bytes = edit.finish()?;
        let mut doc = Document::load_mem(&bytes)?;
        doc.trailer.remove(b"Prev");
        doc.prune_objects();
        let mut out = Vec::new();
        doc.save_to(&mut out).map_err(|e| Error::Structure(e.to_string()))?;
        Ok(out)
    })
}

fn rect_ops(x: f32, y: f32, w: f32, h: f32) -> String {
    format!("{} {} {} {}", num(x), num(y), num(w), num(h))
}

// ---------------------------------------------------------------------------
// Overlay ("Seiten überlagern")

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Blend {
    #[default]
    Normal,
    Multiply,
    Screen,
    Darken,
    Lighten,
    Overlay,
    Difference,
}

impl Blend {
    pub const ALL: [Blend; 7] = [
        Blend::Normal,
        Blend::Multiply,
        Blend::Screen,
        Blend::Darken,
        Blend::Lighten,
        Blend::Overlay,
        Blend::Difference,
    ];

    pub fn pdf_name(self) -> &'static str {
        match self {
            Blend::Normal => "Normal",
            Blend::Multiply => "Multiply",
            Blend::Screen => "Screen",
            Blend::Darken => "Darken",
            Blend::Lighten => "Lighten",
            Blend::Overlay => "Overlay",
            Blend::Difference => "Difference",
        }
    }
}

/// Options of [`overlay`].
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct OverlayOptions {
    /// Start again with the first source page when the source is shorter.
    pub repeat: bool,
    /// Counter-clockwise rotation in degrees.
    pub rotation: f32,
    pub opacity: f32,
    /// Scale (1.0 = 100 %), relative to the target page if `relative`.
    pub scale: f32,
    pub relative: bool,
    pub blend: Blend,
    /// Horizontal / vertical offset in points.
    pub dx: f32,
    pub dy: f32,
    /// Reference edge: Start = left/top, Center, End = right/bottom.
    pub h_from: crate::compose::Align,
    pub v_from: crate::compose::Align,
    /// Below the page content ("Im Hintergrund").
    pub behind: bool,
    /// Put the overlay on a new layer with this name.
    pub layer: Option<String>,
}

impl Default for OverlayOptions {
    fn default() -> Self {
        Self {
            repeat: false,
            rotation: 0.0,
            opacity: 1.0,
            scale: 1.0,
            relative: true,
            blend: Blend::Normal,
            dx: 0.0,
            dy: 0.0,
            h_from: crate::compose::Align::Center,
            v_from: crate::compose::Align::Center,
            behind: false,
            layer: None,
        }
    }
}

/// Matrix placing a source page (upright size `sw`×`sh` after `up`) on a
/// target page with displayed size `tw`×`th`.
pub fn overlay_matrix(up: Matrix, sw: f32, sh: f32, tw: f32, th: f32, o: &OverlayOptions) -> Matrix {
    use crate::compose::Align;
    let s = if o.relative { (tw / sw).min(th / sh) * o.scale } else { o.scale }.max(0.001);
    let (w, h) = (sw * s, sh * s);
    let cx = match o.h_from {
        Align::Start => w / 2.0 + o.dx,
        Align::Center => tw / 2.0 + o.dx,
        Align::End => tw - w / 2.0 - o.dx,
    };
    let cy = match o.v_from {
        Align::Start => th - h / 2.0 - o.dy,
        Align::Center => th / 2.0 - o.dy,
        Align::End => h / 2.0 + o.dy,
    };
    up.then(Matrix::scale(s, s))
        .then(Matrix::translate(-w / 2.0, -h / 2.0))
        .then(Matrix::rotate(o.rotation))
        .then(Matrix::translate(cx, cy))
}

/// Draws source pages over (or under) target pages. Target `i` gets source
/// `sources[i]` (cycling with `repeat`). Returns the bytes and the number of
/// changed pages.
pub fn overlay(
    bytes: &[u8],
    password: Option<&str>,
    targets: &[usize],
    src: &Source,
    sources: &[usize],
    o: &OverlayOptions,
) -> Result<(Vec<u8>, usize)> {
    guarded(|| {
        if sources.is_empty() {
            return Err(Error::Invalid("no source pages".into()));
        }
        let mut edit = Edit::open(bytes, password)?;
        let ocg = match &o.layer {
            Some(name) if !name.trim().is_empty() => Some(add_layer(&mut edit, name.trim())?),
            _ => None,
        };
        let gs = {
            let mut d = Dictionary::new();
            d.set("Type", Object::Name(b"ExtGState".to_vec()));
            d.set("CA", Object::Real(o.opacity.clamp(0.0, 1.0)));
            d.set("ca", Object::Real(o.opacity.clamp(0.0, 1.0)));
            d.set("BM", Object::Name(o.blend.pdf_name().as_bytes().to_vec()));
            edit.add(Object::Dictionary(d))
        };
        let mut imported: HashMap<usize, (ObjectId, RectF, i32)> = HashMap::new();
        let mut changed = 0;
        for (i, &target) in targets.iter().enumerate() {
            let source = if o.repeat {
                sources[i % sources.len()]
            } else if let Some(s) = sources.get(i) {
                *s
            } else {
                break;
            };
            let entry = match imported.get(&source) {
                Some(e) => *e,
                None => {
                    let e = import_page(&mut edit, src, source)?;
                    imported.insert(source, e);
                    e
                }
            };
            let (xobj, bbox, rot) = entry;
            let page_id = edit.page_id(target)?;
            let tbox = crate::pages::visible_box(&edit, page_id);
            let trot = crate::pages::page_rotation(&edit, page_id);
            let (tup, tw, th) = upright(tbox, trot);
            let (up, sw, sh) = upright(bbox, rot);
            let m = overlay_matrix(up, sw, sh, tw, th, o).then(tup.invert());
            let mut content = String::new();
            if ocg.is_some() {
                content.push_str("/OC /MrOC BDC\n");
            }
            content.push_str(&format!("q /MrOvGS gs {} /MrOv Do Q\n", m.operator()));
            if ocg.is_some() {
                content.push_str("EMC\n");
            }
            crate::pages::add_resource(&mut edit, page_id, b"XObject", "MrOv", Object::Reference(xobj))?;
            crate::pages::add_resource(&mut edit, page_id, b"ExtGState", "MrOvGS", Object::Reference(gs))?;
            if let Some(layer) = ocg {
                crate::pages::add_resource(
                    &mut edit,
                    page_id,
                    b"Properties",
                    "MrOC",
                    Object::Reference(layer),
                )?;
            }
            crate::pages::add_content(&mut edit, page_id, content.into_bytes(), o.behind)?;
            changed += 1;
        }
        Ok((edit.finish()?, changed))
    })
}

/// Creates an optional content group (layer), visible by default.
fn add_layer(edit: &mut Edit, name: &str) -> Result<ObjectId> {
    let mut d = Dictionary::new();
    d.set("Type", Object::Name(b"OCG".to_vec()));
    d.set("Name", crate::pdf_edit::text_string(name));
    let id = edit.add(Object::Dictionary(d));
    let catalog_id = edit.prev().trailer.get(b"Root")?.as_reference()?;
    let existing = edit.object(catalog_id)?.as_dict()?.get(b"OCProperties").ok().cloned();
    let mut props = match existing {
        Some(obj) => edit.resolve(&obj)?.as_dict().cloned().unwrap_or_default(),
        None => Dictionary::new(),
    };
    let mut ocgs = match props.get(b"OCGs") {
        Ok(o) => edit.resolve(o)?.as_array().cloned().unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    ocgs.push(Object::Reference(id));
    let mut d = match props.get(b"D") {
        Ok(o) => edit.resolve(o)?.as_dict().cloned().unwrap_or_default(),
        Err(_) => Dictionary::new(),
    };
    let mut order = match d.get(b"Order") {
        Ok(o) => edit.resolve(o)?.as_array().cloned().unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    order.push(Object::Reference(id));
    d.set("Order", Object::Array(order));
    props.set("OCGs", Object::Array(ocgs));
    props.set("D", Object::Dictionary(d));
    edit.dict_mut(catalog_id)?.set("OCProperties", Object::Dictionary(props));
    Ok(id)
}

/// Maps a box shown with clockwise `rotation` to an upright box at the
/// origin. Returns the matrix and the displayed size.
pub fn upright(bbox: RectF, rotation: i32) -> (Matrix, f32, f32) {
    let (x0, y0, x1, y1) = (bbox.x0, bbox.y0, bbox.x1, bbox.y1);
    let (w, h) = (bbox.width(), bbox.height());
    match rotation.rem_euclid(360) {
        90 => (Matrix([0.0, -1.0, 1.0, 0.0, -y0, x1]), h, w),
        180 => (Matrix([-1.0, 0.0, 0.0, -1.0, x1, y1]), w, h),
        270 => (Matrix([0.0, 1.0, -1.0, 0.0, y1, -x0]), h, w),
        _ => (Matrix::translate(-x0, -y0), w, h),
    }
}

/// Options of [`replace_pages`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReplaceOptions {
    /// Keep comments, links and form fields of the target pages.
    pub keep_annotations: bool,
    /// Keep the target page size (and scale the new content into it).
    pub fit_target: bool,
    /// When fitting: scale the content (otherwise it is centred unscaled).
    pub scale_content: bool,
}

/// Replaces the content of `targets[i]` by page `sources[i]` of `src`.
pub fn replace_pages(
    bytes: &[u8],
    password: Option<&str>,
    targets: &[usize],
    src: &Source,
    sources: &[usize],
    opts: ReplaceOptions,
) -> Result<Vec<u8>> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        for (i, (&target, &source)) in targets.iter().zip(sources).enumerate() {
            let page_id = edit.page_id(target)?;
            let (xobj, bbox, rotation) = import_page(&mut edit, src, source)?;
            let (up, sw, sh) = upright(bbox, rotation);
            let target_box = crate::pages::visible_box(&edit, page_id);
            let target_rot = crate::pages::page_rotation(&edit, page_id);
            let (tw, th) = if target_rot % 180 == 0 {
                (target_box.width(), target_box.height())
            } else {
                (target_box.height(), target_box.width())
            };
            let (matrix, pw, ph) = if opts.fit_target {
                let s = if opts.scale_content { (tw / sw).min(th / sh) } else { 1.0 };
                let place = up
                    .then(Matrix::scale(s, s))
                    .then(Matrix::translate((tw - sw * s) / 2.0, (th - sh * s) / 2.0));
                (place, tw, th)
            } else {
                (up, sw, sh)
            };
            let name = format!("MrImp{i}");
            let content = format!("q {} /{name} Do Q\n", matrix.operator());
            let content_id = edit.add(Stream::new(Dictionary::new(), content.into_bytes()));
            let mut xobjects = Dictionary::new();
            xobjects.set(name.into_bytes(), Object::Reference(xobj));
            let mut resources = Dictionary::new();
            resources.set("XObject", Object::Dictionary(xobjects));
            let page = edit.dict_mut(page_id)?;
            page.set("Contents", Object::Reference(content_id));
            page.set("Resources", Object::Dictionary(resources));
            page.set("MediaBox", rect_array(&RectF::new(0.0, 0.0, pw, ph)));
            page.remove(b"CropBox");
            page.remove(b"TrimBox");
            page.remove(b"BleedBox");
            page.remove(b"ArtBox");
            page.remove(b"Thumb");
            page.set("Rotate", Object::Integer(0));
            if !opts.keep_annotations {
                page.remove(b"Annots");
            }
        }
        edit.finish()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upright_maps_corners() {
        let b = RectF::new(10.0, 20.0, 110.0, 220.0); // 100 x 200
        for (rot, size) in
            [(0, (100.0, 200.0)), (90, (200.0, 100.0)), (180, (100.0, 200.0)), (270, (200.0, 100.0))]
        {
            let (m, w, h) = upright(b, rot);
            assert_eq!((w, h), size);
            for (x, y) in [(10.0, 20.0), (110.0, 20.0), (10.0, 220.0), (110.0, 220.0)] {
                let (u, v) = m.apply(x, y);
                assert!((-0.01..=w + 0.01).contains(&u) && (-0.01..=h + 0.01).contains(&v), "{rot}: {u},{v}");
            }
        }
        // Rotation 90 (clockwise): the top-left corner ends up top-right.
        let (m, w, h) = upright(b, 90);
        assert_eq!(m.apply(10.0, 220.0), (200.0, 100.0));
        assert_eq!((w, h), (200.0, 100.0));
    }

    #[test]
    fn matrices_compose() {
        let m = Matrix::scale(2.0, 2.0).then(Matrix::translate(5.0, 1.0));
        assert_eq!(m.apply(1.0, 1.0), (7.0, 3.0));
        let r = Matrix::rotate(90.0).apply(1.0, 0.0);
        assert!((r.0).abs() < 1e-6 && (r.1 - 1.0).abs() < 1e-6);
    }
}
