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
