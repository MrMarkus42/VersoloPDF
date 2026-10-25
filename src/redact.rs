//! Redaction: marked areas are removed for good.
//!
//! Every page carrying redaction marks is replaced by an image of the page in
//! which the marked areas are painted black. Text, vector graphics and images
//! of those pages are therefore gone. Afterwards the whole file is rewritten
//! without its revision history and unused objects are dropped, so the
//! removed content cannot be recovered from older revisions.

use std::collections::BTreeMap;

use lopdf::{Dictionary, Document, LoadOptions, Object, ObjectId, Stream};

use crate::appearance::num;
use crate::error::{Error, Result};
use crate::geometry::RectF;
use crate::pdf_edit::{MAX_DECOMPRESSED_STREAM, guarded, image_xobject, name, read_rect};

/// A rendered page that replaces the original page content.
#[derive(Debug, Clone)]
pub struct PageImage {
    pub page: usize,
    pub width: u32,
    pub height: u32,
    /// RGB pixels, top row first.
    pub rgb: Vec<u8>,
    /// Maps the unit square of the image into PDF user space (`cm` operands).
    pub matrix: [f32; 6],
}

fn load(bytes: &[u8], password: Option<&str>) -> Result<Document> {
    let mut options = match password {
        Some(pw) => LoadOptions::with_password(pw),
        None => LoadOptions::default(),
    };
    options.max_decompressed_size = Some(MAX_DECOMPRESSED_STREAM);
    let doc = Document::load_mem_with_options(bytes, options)?;
    if doc.is_encrypted() {
        return Err(Error::EncryptedNotSupported);
    }
    Ok(doc)
}

fn resolve<'a>(doc: &'a Document, obj: &'a Object) -> Option<&'a Object> {
    match obj {
        Object::Reference(id) => doc.get_object(*id).ok(),
        other => Some(other),
    }
}

fn annots_of(doc: &Document, page_id: ObjectId) -> Vec<Object> {
    doc.get_object(page_id)
        .ok()
        .and_then(|p| p.as_dict().ok())
        .and_then(|p| p.get(b"Annots").ok())
        .and_then(|a| resolve(doc, a))
        .and_then(|a| a.as_array().ok())
        .cloned()
        .unwrap_or_default()
}

fn is_redact(doc: &Document, entry: &Object) -> bool {
    resolve(doc, entry)
        .and_then(|o| o.as_dict().ok())
        .is_some_and(|d| name(d, b"Subtype").as_deref() == Some("Redact"))
}

/// Areas marked for redaction, by page index.
pub fn redaction_marks(bytes: &[u8], password: Option<&str>) -> Result<BTreeMap<usize, Vec<RectF>>> {
    guarded(|| {
        let doc = load(bytes, password)?;
        let mut out: BTreeMap<usize, Vec<RectF>> = BTreeMap::new();
        for (no, page_id) in doc.get_pages() {
            for entry in annots_of(&doc, page_id) {
                let Some(dict) = resolve(&doc, &entry).and_then(|o| o.as_dict().ok()) else { continue };
                if name(dict, b"Subtype").as_deref() != Some("Redact") {
                    continue;
                }
                // QuadPoints (text marks) take precedence over the rectangle.
                let quads = crate::pdf_edit::numbers(dict.get(b"QuadPoints").ok());
                let mut rects: Vec<RectF> = quads
                    .as_chunks::<8>()
                    .0
                    .iter()
                    .filter_map(|q| {
                        RectF::bounding(
                            q.as_chunks::<2>().0.iter().map(|c| crate::geometry::PointF::new(c[0], c[1])),
                        )
                    })
                    .collect();
                if rects.is_empty()
                    && let Some(r) = read_rect(dict)
                {
                    rects.push(r);
                }
                out.entry(no as usize - 1).or_default().extend(rects);
            }
        }
        Ok(out)
    })
}

/// Replaces the given pages by their images, removes all redaction marks and
/// rewrites the file from scratch.
pub fn apply(bytes: &[u8], password: Option<&str>, images: &[PageImage]) -> Result<Vec<u8>> {
    guarded(|| {
        let mut doc = load(bytes, password)?;
        let pages = doc.get_pages();
        for img in images {
            if img.width == 0 || img.height == 0 || img.rgb.len() != (img.width * img.height * 3) as usize {
                return Err(Error::Invalid("invalid page image".into()));
            }
            let no = u32::try_from(img.page + 1).map_err(|_| Error::PageOutOfRange(img.page))?;
            let page_id = *pages.get(&no).ok_or(Error::PageOutOfRange(img.page))?;
            let image_id =
                doc.add_object(image_xobject(img.rgb.clone(), img.width, img.height, false, None)?);
            let [a, b, c, d, e, f] = img.matrix;
            let content = format!(
                "q {} {} {} {} {} {} cm /MrRedacted Do Q\n",
                num(a),
                num(b),
                num(c),
                num(d),
                num(e),
                num(f)
            );
            let mut content_stream = Stream::new(Dictionary::new(), content.into_bytes());
            content_stream.compress()?;
            let content_id = doc.add_object(content_stream);
            let mut xobjects = Dictionary::new();
            xobjects.set("MrRedacted", Object::Reference(image_id));
            let mut resources = Dictionary::new();
            resources.set("XObject", Object::Dictionary(xobjects));
            let page = doc.get_object_mut(page_id)?.as_dict_mut()?;
            page.set("Contents", Object::Reference(content_id));
            page.set("Resources", Object::Dictionary(resources));
            page.remove(b"Thumb");
            page.remove(b"PieceInfo");
        }
        // Remove the redaction marks from every page.
        for page_id in pages.values() {
            let annots = annots_of(&doc, *page_id);
            if !annots.iter().any(|a| is_redact(&doc, a)) {
                continue;
            }
            let kept: Vec<Object> = annots.into_iter().filter(|a| !is_redact(&doc, a)).collect();
            doc.get_object_mut(*page_id)?.as_dict_mut()?.set("Annots", Object::Array(kept));
        }
        // A fresh file without history; unreferenced (removed) objects go.
        doc.trailer.remove(b"Prev");
        doc.trailer.remove(b"XRefStm");
        doc.prune_objects();
        let mut out = Vec::new();
        doc.save_to(&mut out).map_err(|e| Error::Structure(e.to_string()))?;
        Ok(out)
    })
}
