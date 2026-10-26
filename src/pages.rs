//! Page level edits on the PDF structure: scaling/resizing pages, page marks
//! (watermark, background, header/footer, Bates numbers) and page labels.

use lopdf::{Dictionary, Object, ObjectId, Stream};

use crate::appearance::{StdFont, num, win_ansi};
use crate::error::{Error, Result};
use crate::geometry::RectF;
use crate::import::Matrix;
use crate::model::Rgba;
use crate::pdf_edit::{Edit, guarded, number, numbers, real, rect_array, text_string};

const INHERITABLE: [&[u8]; 4] = [b"Resources", b"MediaBox", b"CropBox", b"Rotate"];
const MAX_DEPTH: usize = 64;

/// Returns a page attribute, following the page tree for inherited ones.
fn page_attr(edit: &Edit, page_id: ObjectId, key: &[u8]) -> Option<Object> {
    let mut id = page_id;
    for _ in 0..MAX_DEPTH {
        let dict = edit.object(id).ok()?.as_dict().ok()?;
        if let Ok(v) = dict.get(key) {
            return Some(v.clone());
        }
        if !INHERITABLE.contains(&key) {
            return None;
        }
        id = dict.get(b"Parent").ok()?.as_reference().ok()?;
    }
    None
}

fn page_box(edit: &Edit, page_id: ObjectId, key: &[u8]) -> Option<RectF> {
    let obj = page_attr(edit, page_id, key)?;
    let obj = edit.resolve(&obj).ok()?.clone();
    match numbers(Some(&obj)).as_slice() {
        [a, b, c, d] => Some(RectF::new(*a, *b, *c, *d).normalized()),
        _ => None,
    }
}

/// The visible box (crop box, else media box) of a page.
pub(crate) fn visible_box(edit: &Edit, page_id: ObjectId) -> RectF {
    page_box(edit, page_id, b"CropBox")
        .or_else(|| page_box(edit, page_id, b"MediaBox"))
        .unwrap_or(RectF::new(0.0, 0.0, 595.0, 842.0))
}

/// Makes the page's resources a direct, editable dictionary and returns it.
fn page_resources(edit: &mut Edit, page_id: ObjectId) -> Result<&mut Dictionary> {
    let inherited = page_attr(edit, page_id, b"Resources");
    let resources = match inherited {
        Some(obj) => edit.resolve(&obj)?.as_dict().cloned().unwrap_or_default(),
        None => Dictionary::new(),
    };
    let page = edit.dict_mut(page_id)?;
    page.set("Resources", Object::Dictionary(resources));
    Ok(page.get_mut(b"Resources")?.as_dict_mut()?)
}

/// Adds an entry to a resource category (`Font`, `ExtGState`, …) of a page.
pub(crate) fn add_resource(
    edit: &mut Edit,
    page_id: ObjectId,
    category: &[u8],
    name: &str,
    value: Object,
) -> Result<()> {
    let existing = {
        let res = page_resources(edit, page_id)?;
        res.get(category).ok().cloned()
    };
    let mut cat = match existing {
        Some(obj) => edit.resolve(&obj)?.as_dict().cloned().unwrap_or_default(),
        None => Dictionary::new(),
    };
    cat.set(name.as_bytes().to_vec(), value);
    page_resources(edit, page_id)?.set(category.to_vec(), Object::Dictionary(cat));
    Ok(())
}

/// Adds content before (`behind`) or after the existing page content.
pub(crate) fn add_content(edit: &mut Edit, page_id: ObjectId, content: Vec<u8>, behind: bool) -> Result<()> {
    let existing = edit.object(page_id)?.as_dict()?.get(b"Contents").ok().cloned();
    let mut list: Vec<Object> = match existing {
        Some(Object::Array(items)) => items,
        Some(Object::Reference(id)) => match edit.object(id)? {
            Object::Array(items) => items.clone(),
            _ => vec![Object::Reference(id)],
        },
        _ => Vec::new(),
    };
    // Existing content is wrapped in q/Q so its graphics state cannot leak.
    let open = edit.add(Stream::new(Dictionary::new(), b"q\n".to_vec()));
    let close = edit.add(Stream::new(Dictionary::new(), b"\nQ\n".to_vec()));
    let mut stream = Stream::new(Dictionary::new(), content);
    stream.compress()?;
    let new_id = edit.add(stream);
    if behind {
        let mut out = vec![Object::Reference(new_id), Object::Reference(open)];
        out.append(&mut list);
        out.push(Object::Reference(close));
        list = out;
    } else {
        list.insert(0, Object::Reference(open));
        list.push(Object::Reference(close));
        list.push(Object::Reference(new_id));
    }
    edit.dict_mut(page_id)?.set("Contents", Object::Array(list));
    Ok(())
}

/// Position of a page mark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    TopLeft,
    TopCenter,
    TopRight,
    Center,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

/// A text placed on pages (watermark, header/footer, Bates number).
#[derive(Debug, Clone, PartialEq)]
pub struct TextMark {
    pub text: String,
    pub anchor: Anchor,
    pub font_size: f32,
    pub color: Rgba,
    pub opacity: f32,
    /// Counter-clockwise rotation in degrees (watermarks).
    pub rotation: f32,
    /// Distance from the page edge in points.
    pub margin: f32,
    pub bold: bool,
}

/// What to put on the pages.
#[derive(Debug, Clone, PartialEq)]
pub enum PageMark {
    Texts { marks: Vec<TextMark>, behind: bool },
    Fill { color: Rgba },
}

/// Replaces the placeholders `<<page>>`, `<<pages>>`, `<<date>>` and `<<file>>`.
pub fn expand_placeholders(text: &str, page: usize, pages: usize, date: &str, file: &str) -> String {
    text.replace("<<page>>", &(page + 1).to_string())
        .replace("<<pages>>", &pages.to_string())
        .replace("<<date>>", date)
        .replace("<<file>>", file)
}

/// Adds page marks to `pages`. `text_for` turns a mark's text into the
/// final text of a page (placeholders, Bates numbers).
pub fn add_page_marks(
    bytes: &[u8],
    password: Option<&str>,
    pages: &[usize],
    mark: &PageMark,
    text_for: &dyn Fn(&str, usize) -> String,
) -> Result<Vec<u8>> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        for &page in pages {
            let page_id = edit.page_id(page)?;
            let vb = visible_box(&edit, page_id);
            let rotate = page_attr(&edit, page_id, b"Rotate").and_then(|r| number(&r)).unwrap_or(0.0) as i32;
            match mark {
                PageMark::Fill { color } => {
                    let [r, g, b] = color.to_pdf();
                    let content = format!(
                        "{} {} {} rg {} {} {} {} re f\n",
                        num(r),
                        num(g),
                        num(b),
                        num(vb.x0),
                        num(vb.y0),
                        num(vb.width()),
                        num(vb.height())
                    );
                    add_content(&mut edit, page_id, content.into_bytes(), true)?;
                }
                PageMark::Texts { marks, behind } => {
                    let mut out: Vec<u8> = b"/Artifact <</Type /Pagination>> BDC\n".to_vec();
                    for (i, m) in marks.iter().enumerate() {
                        let text = text_for(&m.text, page);
                        if text.trim().is_empty() {
                            continue;
                        }
                        let font = if m.bold { StdFont::HelveticaBold } else { StdFont::Helvetica };
                        add_resource(&mut edit, page_id, b"Font", &format!("MrF{i}"), std_font_dict(font))?;
                        let gs = format!("MrG{i}");
                        let mut state = Dictionary::new();
                        state.set("Type", Object::Name(b"ExtGState".to_vec()));
                        state.set("ca", real(m.opacity.clamp(0.05, 1.0)));
                        state.set("CA", real(m.opacity.clamp(0.05, 1.0)));
                        add_resource(&mut edit, page_id, b"ExtGState", &gs, Object::Dictionary(state))?;
                        out.extend_from_slice(&text_operators(
                            m,
                            &text,
                            font,
                            &format!("MrF{i}"),
                            &gs,
                            vb,
                            rotate,
                        ));
                    }
                    out.extend_from_slice(b"EMC\n");
                    add_content(&mut edit, page_id, out, *behind)?;
                }
            }
        }
        edit.finish()
    })
}

fn std_font_dict(font: StdFont) -> Object {
    let mut f = Dictionary::new();
    f.set("Type", Object::Name(b"Font".to_vec()));
    f.set("Subtype", Object::Name(b"Type1".to_vec()));
    f.set("BaseFont", Object::Name(font.base_font().as_bytes().to_vec()));
    f.set("Encoding", Object::Name(b"WinAnsiEncoding".to_vec()));
    Object::Dictionary(f)
}

/// Content operators that draw `text` for a mark. Positions are computed in
/// the displayed orientation (honouring `/Rotate`) and mapped back to PDF space.
fn text_operators(
    m: &TextMark,
    text: &str,
    font: StdFont,
    font_name: &str,
    gs: &str,
    vb: RectF,
    rotate: i32,
) -> Vec<u8> {
    let encoded = win_ansi(text);
    let size = m.font_size.max(1.0);
    let tw = font.text_width(&encoded, size);
    let quarter = rotate.rem_euclid(360) / 90;
    // Displayed width/height.
    let (dw, dh) = if quarter % 2 == 1 { (vb.height(), vb.width()) } else { (vb.width(), vb.height()) };
    let mg = m.margin;
    // Anchor of the text's baseline start in displayed coordinates (origin bottom-left).
    let (x, y) = match m.anchor {
        Anchor::TopLeft => (mg, dh - mg - size * 0.8),
        Anchor::TopCenter => ((dw - tw) / 2.0, dh - mg - size * 0.8),
        Anchor::TopRight => (dw - mg - tw, dh - mg - size * 0.8),
        Anchor::Center => (dw / 2.0, dh / 2.0),
        Anchor::BottomLeft => (mg, mg),
        Anchor::BottomCenter => ((dw - tw) / 2.0, mg),
        Anchor::BottomRight => (dw - mg - tw, mg),
    };
    // Matrix from displayed space to PDF space (page rotation is clockwise).
    let (a, b, c, d, e, f) = match quarter {
        1 => (0.0, 1.0, -1.0, 0.0, vb.x1, vb.y0),
        2 => (-1.0, 0.0, 0.0, -1.0, vb.x1, vb.y1),
        3 => (0.0, -1.0, 1.0, 0.0, vb.x0, vb.y1),
        _ => (1.0, 0.0, 0.0, 1.0, vb.x0, vb.y0),
    };
    let [r, g, bl] = m.color.to_pdf();
    let mut s = format!(
        "q /{gs} gs {} {} {} {} {} {} cm {} {} {} rg BT /{font_name} {} Tf ",
        num(a),
        num(b),
        num(c),
        num(d),
        num(e),
        num(f),
        num(r),
        num(g),
        num(bl),
        num(size)
    );
    if m.anchor == Anchor::Center {
        // Centred, optionally rotated around the page centre.
        let (sin, cos) = m.rotation.to_radians().sin_cos();
        let ox = -tw / 2.0;
        let oy = -size * 0.35;
        s.push_str(&format!(
            "{} {} {} {} {} {} Tm (",
            num(cos),
            num(sin),
            num(-sin),
            num(cos),
            num(x + ox * cos - oy * sin),
            num(y + ox * sin + oy * cos)
        ));
    } else {
        s.push_str(&format!("{} {} Td (", num(x), num(y)));
    }
    let mut out = s.into_bytes();
    for &byte in &encoded {
        if matches!(byte, b'(' | b')' | b'\\') {
            out.push(b'\\');
        }
        out.push(byte);
    }
    out.extend_from_slice(b") Tj ET Q\n");
    out
}

/// Scales the content of `pages` into new page boxes of `size` (points).
/// With `fit` the content is scaled proportionally and centred; otherwise
/// the content keeps its size and is centred on the new page.
pub fn resize_pages(
    bytes: &[u8],
    password: Option<&str>,
    pages: &[usize],
    size: (f32, f32),
    fit: bool,
) -> Result<Vec<u8>> {
    guarded(|| {
        let (tw, th) = size;
        if !(1.0..=14_400.0).contains(&tw) || !(1.0..=14_400.0).contains(&th) {
            return Err(Error::Invalid("invalid page size".into()));
        }
        let mut edit = Edit::open(bytes, password)?;
        for &page in pages {
            let page_id = edit.page_id(page)?;
            let vb = visible_box(&edit, page_id);
            let rotate = page_attr(&edit, page_id, b"Rotate").and_then(|r| number(&r)).unwrap_or(0.0) as i32;
            // Target size is meant for the displayed orientation.
            let (tw, th) = if rotate.rem_euclid(180) == 90 { (th, tw) } else { (tw, th) };
            let s = if fit { (tw / vb.width()).min(th / vb.height()) } else { 1.0 };
            let dx = (tw - vb.width() * s) / 2.0 - vb.x0 * s;
            let dy = (th - vb.height() * s) / 2.0 - vb.y0 * s;
            transform_page(&mut edit, page_id, s, dx, dy)?;
            let media = RectF::new(0.0, 0.0, tw, th);
            let dict = edit.dict_mut(page_id)?;
            dict.set("MediaBox", rect_array(&media));
            dict.remove(b"CropBox");
            dict.remove(b"TrimBox");
            dict.remove(b"BleedBox");
            dict.remove(b"ArtBox");
        }
        edit.finish()
    })
}

/// Applies `scale` and translation to the page content and annotations.
fn transform_page(edit: &mut Edit, page_id: ObjectId, s: f32, dx: f32, dy: f32) -> Result<()> {
    let m = Matrix::scale(s, s).then(Matrix::translate(dx, dy));
    transform_page_matrix(edit, page_id, m, 0.0)
}

/// Clockwise rotation of a page in degrees (0, 90, 180, 270).
pub(crate) fn page_rotation(edit: &Edit, page_id: ObjectId) -> i32 {
    let r = page_attr(edit, page_id, b"Rotate").and_then(|r| number(&r)).unwrap_or(0.0) as i32;
    (r.rem_euclid(360) / 90) * 90
}

fn transform_points(items: &mut [Object], m: &Matrix) {
    for pair in items.chunks_mut(2) {
        if let [x, y] = pair
            && let (Some(px), Some(py)) = (number(x), number(y))
        {
            let (nx, ny) = m.apply(px, py);
            *x = real(nx);
            *y = real(ny);
        }
    }
}

/// Applies an affine matrix to the page content and its annotations.
/// `ap_rotation` (counter-clockwise degrees) is added to the appearance
/// streams so that comments keep their look when the page is rotated.
pub(crate) fn transform_page_matrix(
    edit: &mut Edit,
    page_id: ObjectId,
    m: Matrix,
    ap_rotation: f32,
) -> Result<()> {
    let cm = format!("q {}\n", m.operator());
    let existing = edit.object(page_id)?.as_dict()?.get(b"Contents").ok().cloned();
    let mut list: Vec<Object> = match existing {
        Some(Object::Array(items)) => items,
        Some(Object::Reference(id)) => match edit.object(id)? {
            Object::Array(items) => items.clone(),
            _ => vec![Object::Reference(id)],
        },
        _ => Vec::new(),
    };
    let open = edit.add(Stream::new(Dictionary::new(), cm.into_bytes()));
    let close = edit.add(Stream::new(Dictionary::new(), b"\nQ\n".to_vec()));
    list.insert(0, Object::Reference(open));
    list.push(Object::Reference(close));
    edit.dict_mut(page_id)?.set("Contents", Object::Array(list));

    // Move annotations with the content.
    for entry in edit.annots(page_id)? {
        let Ok(id) = entry.as_reference() else { continue };
        let mut ap_streams = Vec::new();
        {
            let dict = edit.dict_mut(id)?;
            if let Ok(Object::Array(items)) = dict.get_mut(b"Rect")
                && items.len() == 4
                && let Some(v) = items.iter().map(number).collect::<Option<Vec<f32>>>()
            {
                // The rectangle becomes the bounding box of its moved corners.
                let corners =
                    [(v[0], v[1]), (v[2], v[1]), (v[0], v[3]), (v[2], v[3])].map(|(x, y)| m.apply(x, y));
                let r = RectF::bounding(corners.iter().map(|(x, y)| crate::geometry::PointF::new(*x, *y)))
                    .unwrap_or_default();
                *items = vec![real(r.x0), real(r.y0), real(r.x1), real(r.y1)];
            }
            for key in [&b"QuadPoints"[..], b"L", b"Vertices", b"CL"] {
                if let Ok(Object::Array(items)) = dict.get_mut(key) {
                    transform_points(items, &m);
                }
            }
            if let Ok(Object::Array(strokes)) = dict.get_mut(b"InkList") {
                for stroke in strokes {
                    if let Object::Array(items) = stroke {
                        transform_points(items, &m);
                    }
                }
            }
            if ap_rotation != 0.0
                && let Ok(Object::Dictionary(ap)) = dict.get(b"AP")
            {
                for (_, v) in ap.iter() {
                    match v {
                        Object::Reference(r) => ap_streams.push(*r),
                        Object::Dictionary(states) => {
                            ap_streams.extend(states.iter().filter_map(|(_, s)| s.as_reference().ok()))
                        }
                        _ => {}
                    }
                }
            }
        }
        for stream_id in ap_streams {
            if let Ok(Object::Stream(s)) = edit.object_mut(stream_id) {
                let old = match s.dict.get(b"Matrix") {
                    Ok(Object::Array(a)) if a.len() == 6 => {
                        let v: Vec<f32> = a.iter().filter_map(number).collect();
                        if v.len() == 6 {
                            Matrix([v[0], v[1], v[2], v[3], v[4], v[5]])
                        } else {
                            Matrix::IDENTITY
                        }
                    }
                    _ => Matrix::IDENTITY,
                };
                let new = old.then(Matrix::rotate(ap_rotation));
                s.dict.set("Matrix", Object::Array(new.0.iter().map(|x| real(*x)).collect()));
            }
        }
    }
    Ok(())
}

/// "Seiten normalisieren": removes page offsets and /Rotate without changing
/// the look; the visible box becomes the media box at (0,0). Returns the
/// bytes and the number of changed pages.
pub fn normalize_pages(bytes: &[u8], password: Option<&str>, pages: &[usize]) -> Result<(Vec<u8>, usize)> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let mut changed = 0;
        for &page in pages {
            let page_id = edit.page_id(page)?;
            let vb = visible_box(&edit, page_id);
            let rot = page_rotation(&edit, page_id);
            let media = page_box(&edit, page_id, b"MediaBox").unwrap_or(vb);
            let at_origin = vb.x0.abs() < 0.01 && vb.y0.abs() < 0.01;
            if rot == 0 && at_origin && media == vb {
                continue;
            }
            let (m, w, h) = crate::import::upright(vb, rot);
            transform_page_matrix(&mut edit, page_id, m, -(rot as f32))?;
            let dict = edit.dict_mut(page_id)?;
            dict.set("MediaBox", rect_array(&RectF::new(0.0, 0.0, w, h)));
            for key in [&b"CropBox"[..], b"TrimBox", b"BleedBox", b"ArtBox"] {
                dict.remove(key);
            }
            dict.set("Rotate", Object::Integer(0));
            changed += 1;
        }
        if changed == 0 {
            return Ok((bytes.to_vec(), 0));
        }
        Ok((edit.finish()?, changed))
    })
}

/// Numbering style of page labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelStyle {
    Decimal,
    RomanUpper,
    RomanLower,
    LettersUpper,
    LettersLower,
    None,
}

impl LabelStyle {
    fn pdf_name(self) -> Option<&'static [u8]> {
        match self {
            LabelStyle::Decimal => Some(b"D"),
            LabelStyle::RomanUpper => Some(b"R"),
            LabelStyle::RomanLower => Some(b"r"),
            LabelStyle::LettersUpper => Some(b"A"),
            LabelStyle::LettersLower => Some(b"a"),
            LabelStyle::None => None,
        }
    }
}

/// Starts a page label range at `start_page` (replacing a range starting there).
pub fn set_page_labels(
    bytes: &[u8],
    password: Option<&str>,
    start_page: usize,
    style: LabelStyle,
    prefix: &str,
    first: u32,
) -> Result<Vec<u8>> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        edit.page_id(start_page)?;
        let catalog_id = edit.prev().trailer.get(b"Root")?.as_reference()?;
        let existing = edit.object(catalog_id)?.as_dict()?.get(b"PageLabels").ok().cloned();
        let mut entries: Vec<(i64, Object)> = Vec::new();
        if let Some(obj) = existing {
            let tree = edit.resolve(&obj)?.as_dict()?.clone();
            if let Ok(nums) = tree.get(b"Nums") {
                let nums = edit.resolve(nums)?.as_array()?.clone();
                for pair in nums.as_chunks::<2>().0 {
                    if let Ok(i) = pair[0].as_i64() {
                        entries.push((i, pair[1].clone()));
                    }
                }
            }
        }
        let mut label = Dictionary::new();
        label.set("Type", Object::Name(b"PageLabel".to_vec()));
        if let Some(s) = style.pdf_name() {
            label.set("S", Object::Name(s.to_vec()));
        }
        if !prefix.is_empty() {
            label.set("P", text_string(prefix));
        }
        if first > 1 {
            label.set("St", Object::Integer(i64::from(first)));
        }
        entries.retain(|(i, _)| *i != start_page as i64);
        entries.push((start_page as i64, Object::Dictionary(label)));
        if !entries.iter().any(|(i, _)| *i == 0) {
            let mut d = Dictionary::new();
            d.set("S", Object::Name(b"D".to_vec()));
            entries.push((0, Object::Dictionary(d)));
        }
        entries.sort_by_key(|(i, _)| *i);
        let nums: Vec<Object> = entries.into_iter().flat_map(|(i, d)| [Object::Integer(i), d]).collect();
        let mut tree = Dictionary::new();
        tree.set("Nums", Object::Array(nums));
        edit.dict_mut(catalog_id)?.set("PageLabels", Object::Dictionary(tree));
        edit.finish()
    })
}

/// Inserts the new page `new_id` directly after `after_id` in the page tree.
fn insert_page_after(edit: &mut Edit, after_id: ObjectId, new_id: ObjectId) -> Result<()> {
    let parent = edit.object(after_id)?.as_dict()?.get(b"Parent")?.as_reference()?;
    let kids_obj = edit.object(parent)?.as_dict()?.get(b"Kids")?.clone();
    let mut kids = edit.resolve(&kids_obj)?.as_array()?.clone();
    let pos = kids.iter().position(|k| k.as_reference().ok() == Some(after_id)).map_or(kids.len(), |p| p + 1);
    kids.insert(pos, Object::Reference(new_id));
    edit.dict_mut(parent)?.set("Kids", Object::Array(kids));
    edit.dict_mut(new_id)?.set("Parent", Object::Reference(parent));
    // Every ancestor counts one page more.
    let mut node = Some(parent);
    for _ in 0..MAX_DEPTH {
        let Some(id) = node else { break };
        let dict = edit.dict_mut(id)?;
        let count = dict.get(b"Count").ok().and_then(|c| c.as_i64().ok()).unwrap_or(0);
        dict.set("Count", Object::Integer(count + 1));
        node = dict.get(b"Parent").ok().and_then(|p| p.as_reference().ok());
    }
    Ok(())
}

/// How "Seiten aufteilen" cuts a page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum SplitMethod {
    /// Only the visible area (crop box) of each piece is set; the whole
    /// original content stays in the file ("Originalinhalt erhalten").
    #[default]
    CropBox,
    /// The page size (media box) is set to the piece as well.
    MediaBox,
}

/// Cuts each of `pages` along vertical lines at `xs` (fractions of the
/// displayed width from the left) and horizontal lines at `ys` (fractions
/// from the top). Pieces follow each other row by row (right to left with
/// `rtl`). Comments stay on the first piece.
pub fn split_pages(
    bytes: &[u8],
    password: Option<&str>,
    pages: &[usize],
    xs: &[f32],
    ys: &[f32],
    rtl: bool,
    method: SplitMethod,
) -> Result<Vec<u8>> {
    guarded(|| {
        let clean = |v: &[f32]| {
            let mut v: Vec<f32> = v.iter().copied().filter(|x| *x > 0.001 && *x < 0.999).collect();
            v.sort_by(f32::total_cmp);
            v.dedup_by(|a, b| (*a - *b).abs() < 0.001);
            v
        };
        let (xs, ys) = (clean(xs), clean(ys));
        if xs.is_empty() && ys.is_empty() {
            return Err(Error::Invalid("no split lines".into()));
        }
        let mut edit = Edit::open(bytes, password)?;
        let ids: Vec<ObjectId> = pages.iter().map(|&p| edit.page_id(p)).collect::<Result<_>>()?;
        for page_id in ids {
            let vb = visible_box(&edit, page_id);
            let rot = page_rotation(&edit, page_id);
            let (up, w, h) = crate::import::upright(vb, rot);
            let back = up.invert();
            let mut xb = vec![0.0];
            xb.extend(xs.iter().map(|x| x * w));
            xb.push(w);
            let mut yb = vec![0.0];
            yb.extend(ys.iter().map(|y| y * h));
            yb.push(h);
            // Cells in display space (y from the top), reading order.
            let mut cells = Vec::new();
            for r in 0..yb.len() - 1 {
                let mut row: Vec<RectF> = (0..xb.len() - 1)
                    .map(|c| RectF::new(xb[c], h - yb[r + 1], xb[c + 1], h - yb[r]))
                    .collect();
                if rtl {
                    row.reverse();
                }
                cells.extend(row);
            }
            let to_user = |r: &RectF| {
                let corners =
                    [(r.x0, r.y0), (r.x1, r.y0), (r.x0, r.y1), (r.x1, r.y1)].map(|(x, y)| back.apply(x, y));
                RectF::bounding(corners.iter().map(|(x, y)| crate::geometry::PointF::new(*x, *y)))
                    .unwrap_or(*r)
            };
            // A complete, standalone copy of the page dictionary.
            let mut base = edit.object(page_id)?.as_dict()?.clone();
            for key in INHERITABLE {
                if !base.has(key)
                    && let Some(v) = page_attr(&edit, page_id, key)
                {
                    base.set(key.to_vec(), v);
                }
            }
            base.remove(b"Annots");
            base.remove(b"StructParents");
            let mut previous = page_id;
            for (i, cell) in cells.iter().enumerate() {
                let user = to_user(cell);
                let target = if i == 0 {
                    page_id
                } else {
                    let id = edit.add(Object::Dictionary(base.clone()));
                    insert_page_after(&mut edit, previous, id)?;
                    id
                };
                let dict = edit.dict_mut(target)?;
                dict.set("CropBox", rect_array(&user));
                if method == SplitMethod::MediaBox {
                    dict.set("MediaBox", rect_array(&user));
                }
                previous = target;
            }
        }
        edit.finish()
    })
}

// ---------------------------------------------------------------------------
// Boxes in display terms (left/right/top/bottom as the page is shown)

/// Margins as shown on screen: left, right, top, bottom (points).
pub type Margins = [f32; 4];

/// Which page box "Seiten zuschneiden" changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum BoxKind {
    #[default]
    Crop,
    Media,
    Trim,
    Bleed,
    Art,
}

impl BoxKind {
    pub const ALL: [BoxKind; 5] =
        [BoxKind::Crop, BoxKind::Media, BoxKind::Trim, BoxKind::Bleed, BoxKind::Art];

    pub fn key(self) -> &'static [u8] {
        match self {
            BoxKind::Crop => b"CropBox",
            BoxKind::Media => b"MediaBox",
            BoxKind::Trim => b"TrimBox",
            BoxKind::Bleed => b"BleedBox",
            BoxKind::Art => b"ArtBox",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            BoxKind::Crop => "Crop-Box",
            BoxKind::Media => "Media-Box",
            BoxKind::Trim => "Trim-Box",
            BoxKind::Bleed => "Bleed-Box",
            BoxKind::Art => "Art-Box",
        }
    }
}

/// The user-space rectangle of a rectangle given in display coordinates
/// (upright, origin bottom left) of a page.
fn display_to_user(edit: &Edit, page_id: ObjectId, disp: RectF) -> RectF {
    let vb = visible_box(edit, page_id);
    let rot = page_rotation(edit, page_id);
    let (up, _, _) = crate::import::upright(vb, rot);
    let back = up.invert();
    let corners = [(disp.x0, disp.y0), (disp.x1, disp.y0), (disp.x0, disp.y1), (disp.x1, disp.y1)]
        .map(|(x, y)| back.apply(x, y));
    RectF::bounding(corners.iter().map(|(x, y)| crate::geometry::PointF::new(*x, *y))).unwrap_or(vb)
}

/// Displayed size of a page (after /Rotate).
fn display_size(edit: &Edit, page_id: ObjectId) -> (f32, f32) {
    let vb = visible_box(edit, page_id);
    if page_rotation(edit, page_id) % 180 == 0 {
        (vb.width(), vb.height())
    } else {
        (vb.height(), vb.width())
    }
}

/// Options of [`crop_pages`].
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CropOptions {
    pub kind: BoxKind,
    /// Set the media box to the new crop box ("Seitengröße an die Crop-Box anpassen").
    pub fit_media: bool,
    /// Make all changed pages as large as the largest one (centred).
    pub same_size: bool,
}

/// Cuts `margins` (display terms, positive = smaller) off the visible area
/// of each page and stores the result in the chosen box.
pub fn crop_pages(
    bytes: &[u8],
    password: Option<&str>,
    items: &[(usize, Margins)],
    opts: CropOptions,
) -> Result<Vec<u8>> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        // Display rectangles first, so all pages can get the same size.
        let mut rects = Vec::new();
        for (page, m) in items {
            let page_id = edit.page_id(*page)?;
            let (w, h) = display_size(&edit, page_id);
            let r = RectF::new(m[0], m[3], w - m[1], h - m[2]);
            if r.x1 - r.x0 < 3.0 || r.y1 - r.y0 < 3.0 {
                return Err(Error::Invalid("the page would become too small".into()));
            }
            rects.push((page_id, r));
        }
        if opts.same_size {
            let mw = rects.iter().map(|(_, r)| r.width()).fold(0.0, f32::max);
            let mh = rects.iter().map(|(_, r)| r.height()).fold(0.0, f32::max);
            for (_, r) in &mut rects {
                let (cx, cy) = ((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0);
                *r = RectF::new(cx - mw / 2.0, cy - mh / 2.0, cx + mw / 2.0, cy + mh / 2.0);
            }
        }
        let converted: Vec<(ObjectId, RectF)> =
            rects.iter().map(|(id, r)| (*id, display_to_user(&edit, *id, *r))).collect();
        for (page_id, user) in converted {
            let media = page_box(&edit, page_id, b"MediaBox").unwrap_or(user);
            let dict = edit.dict_mut(page_id)?;
            dict.set(opts.kind.key().to_vec(), rect_array(&user));
            if opts.kind == BoxKind::Crop && opts.fit_media {
                dict.set("MediaBox", rect_array(&user));
            } else if opts.kind == BoxKind::Media {
                // A smaller media box also limits the crop box.
                dict.remove(b"CropBox");
            } else if !media.contains(crate::geometry::PointF::new(user.x0, user.y0))
                || !media.contains(crate::geometry::PointF::new(user.x1, user.y1))
            {
                dict.set("MediaBox", rect_array(&media.union(&user)));
            }
        }
        edit.finish()
    })
}

/// Adds margins (display terms; negative values make the page smaller)
/// around the visible area of each page by enlarging media and crop box.
pub fn add_margins(bytes: &[u8], password: Option<&str>, items: &[(usize, Margins)]) -> Result<Vec<u8>> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        for (page, m) in items {
            let page_id = edit.page_id(*page)?;
            let (w, h) = display_size(&edit, page_id);
            let disp = RectF::new(-m[0], -m[3], w + m[1], h + m[2]);
            if disp.x1 - disp.x0 < 10.0 || disp.y1 - disp.y0 < 10.0 {
                return Err(Error::Invalid("the page would become too small".into()));
            }
            let user = display_to_user(&edit, page_id, disp);
            let media = page_box(&edit, page_id, b"MediaBox").unwrap_or(user);
            let dict = edit.dict_mut(page_id)?;
            dict.set("CropBox", rect_array(&user));
            dict.set("MediaBox", rect_array(&media.union(&user)));
        }
        edit.finish()
    })
}

/// How "Größe ändern" computes the new page size.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum SizeMode {
    /// A fixed size (display orientation, points).
    Fixed(f32, f32),
    /// A factor (1.0 = 100 %).
    Scale(f32),
    /// Width and height grow by these amounts (points).
    Grow(f32, f32),
}

/// Options of [`resize_pages_with`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResizeSpec {
    pub mode: SizeMode,
    /// 0 = keep, 1 = portrait, 2 = landscape.
    pub orientation: u8,
    /// Skip pages: 0 = none, 1 = portrait pages, 2 = landscape pages.
    pub skip: u8,
    /// Scale the content to the new size.
    pub fit: bool,
    pub keep_aspect: bool,
    /// Anchor of the content: 0 = left/bottom, 0.5 = centre, 1 = right/top.
    pub anchor: (f32, f32),
    /// Extra offset (points, display terms).
    pub offset: (f32, f32),
    /// Move the media box to the origin.
    pub normalize: bool,
}

/// "Größe ändern": new page sizes, content scaled and/or positioned.
pub fn resize_pages_with(
    bytes: &[u8],
    password: Option<&str>,
    pages: &[usize],
    spec: &ResizeSpec,
) -> Result<(Vec<u8>, usize)> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let mut changed = 0;
        for &page in pages {
            let page_id = edit.page_id(page)?;
            let vb = visible_box(&edit, page_id);
            let rot = page_rotation(&edit, page_id);
            let (up, w, h) = crate::import::upright(vb, rot);
            match spec.skip {
                1 if h >= w => continue,
                2 if w > h => continue,
                _ => {}
            }
            let (mut tw, mut th) = match spec.mode {
                SizeMode::Fixed(a, b) => (a, b),
                SizeMode::Scale(f) => (w * f, h * f),
                SizeMode::Grow(a, b) => (w + a, h + b),
            };
            match spec.orientation {
                1 if tw > th => std::mem::swap(&mut tw, &mut th),
                2 if th > tw => std::mem::swap(&mut tw, &mut th),
                _ => {}
            }
            if !(3.0..=14_400.0).contains(&tw) || !(3.0..=14_400.0).contains(&th) {
                return Err(Error::Invalid("invalid page size".into()));
            }
            let (sx, sy) = if spec.fit {
                if spec.keep_aspect {
                    let s = (tw / w).min(th / h);
                    (s, s)
                } else {
                    (tw / w, th / h)
                }
            } else {
                (1.0, 1.0)
            };
            let x = (tw - w * sx) * spec.anchor.0 + spec.offset.0;
            let y = (th - h * sy) * spec.anchor.1 + spec.offset.1;
            // New box in user orientation (width/height swap for 90°/270°).
            let (uw, uh) = if rot % 180 == 0 { (tw, th) } else { (th, tw) };
            let origin = if spec.normalize { (0.0, 0.0) } else { (vb.x0, vb.y0) };
            let new_box = RectF::new(origin.0, origin.1, origin.0 + uw, origin.1 + uh);
            let (new_up, _, _) = crate::import::upright(new_box, rot);
            let m = up.then(Matrix::scale(sx, sy)).then(Matrix::translate(x, y)).then(new_up.invert());
            transform_page_matrix(&mut edit, page_id, m, 0.0)?;
            let dict = edit.dict_mut(page_id)?;
            dict.set("MediaBox", rect_array(&new_box));
            for key in [&b"CropBox"[..], b"TrimBox", b"BleedBox", b"ArtBox"] {
                dict.remove(key);
            }
            changed += 1;
        }
        if changed == 0 {
            return Ok((bytes.to_vec(), 0));
        }
        Ok((edit.finish()?, changed))
    })
}

/// Enlarges (positive) or reduces the media and crop boxes of `pages` by the
/// given margins (left, bottom, right, top) in points.
pub fn change_margins(
    bytes: &[u8],
    password: Option<&str>,
    pages: &[usize],
    m: [f32; 4],
    crop_only: bool,
) -> Result<Vec<u8>> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        for &page in pages {
            let page_id = edit.page_id(page)?;
            let vb = visible_box(&edit, page_id);
            let r = RectF::new(vb.x0 - m[0], vb.y0 - m[1], vb.x1 + m[2], vb.y1 + m[3]);
            if r.x1 - r.x0 < 10.0 || r.y1 - r.y0 < 10.0 {
                return Err(Error::Invalid("the page would become too small".into()));
            }
            let dict = edit.dict_mut(page_id)?;
            dict.set("CropBox", rect_array(&r));
            if !crop_only {
                let media = page_box(&edit, page_id, b"MediaBox").unwrap_or(vb);
                let dict = edit.dict_mut(page_id)?;
                dict.set("MediaBox", rect_array(&media.union(&r)));
            }
        }
        edit.finish()
    })
}

/// Formats a Bates number.
pub fn bates_number(prefix: &str, number: u64, digits: usize, suffix: &str) -> String {
    format!("{prefix}{number:0digits$}{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders() {
        assert_eq!(expand_placeholders("Seite <<page>> von <<pages>>", 1, 9, "", ""), "Seite 2 von 9");
        assert_eq!(expand_placeholders("<<file>> <<date>>", 0, 1, "1.1.2026", "a.pdf"), "a.pdf 1.1.2026");
    }

    #[test]
    fn bates() {
        assert_eq!(bates_number("ABC", 42, 6, "-X"), "ABC000042-X");
        assert_eq!(bates_number("", 1234567, 3, ""), "1234567");
    }

    #[test]
    fn text_operators_escape_and_rotate() {
        let m = TextMark {
            text: String::new(),
            anchor: Anchor::Center,
            font_size: 40.0,
            color: Rgba::RED,
            opacity: 0.3,
            rotation: 45.0,
            margin: 20.0,
            bold: true,
        };
        let ops = text_operators(
            &m,
            "a(b)",
            StdFont::HelveticaBold,
            "F",
            "G",
            RectF::new(0.0, 0.0, 595.0, 842.0),
            0,
        );
        let s = String::from_utf8_lossy(&ops);
        assert!(s.contains("(a\\(b\\)) Tj"), "{s}");
        assert!(s.contains("Tm"), "{s}");
        assert!(s.contains("/G gs"), "{s}");
    }
}
