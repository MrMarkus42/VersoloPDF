//! "PDF optimieren" (Optimiert speichern) and "Speicherverbrauch
//! untersuchen".
//!
//! Works on the PDF structure with lopdf: images are downsampled and
//! recompressed (JPEG/ZIP), embedded fonts can be removed, unneeded objects
//! and user data are deleted, duplicates are merged and the file is written
//! again with compressed streams. Steps that need the PDF engine
//! (embedding annotations, removing content outside the crop box) are done
//! by the caller before.

use std::collections::{HashMap, HashSet};

use lopdf::content::Content;
use lopdf::{Dictionary, Document, LoadOptions, Object, ObjectId, Stream};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};
use crate::import::Matrix;

/// Largest image that is decoded (pixels).
const MAX_PIXELS: u64 = 100_000_000;
/// Largest decompressed stream.
const MAX_STREAM: usize = 512 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Options

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Downsample {
    /// "Nicht herunterrechnen"
    None,
    /// "Durchschnittliches Herunterrechnen auf"
    Average,
    /// "Herunterrechnen auf" (pick pixels)
    Subsample,
    /// "Bikubisches Herunterrechnen auf"
    #[default]
    Bicubic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Compression {
    /// "Existierende behalten"
    Keep,
    #[default]
    Jpeg,
    /// ZIP (Flate, lossless)
    Zip,
    /// "JPEG/ZIP": the smaller result
    JpegOrZip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Quality {
    Minimal,
    Low,
    #[default]
    Medium,
    High,
    Maximum,
}

impl Quality {
    pub fn jpeg(self) -> u8 {
        match self {
            Quality::Minimal => 20,
            Quality::Low => 40,
            Quality::Medium => 60,
            Quality::High => 80,
            Quality::Maximum => 95,
        }
    }
}

/// Settings of one image class (colour, grey, indexed, black and white).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ImageClass {
    pub downsample: Downsample,
    /// Target resolution.
    pub dpi: f32,
    /// Only images above this resolution are downsampled.
    pub above: f32,
    pub compression: Compression,
    pub quality: Quality,
}

impl Default for ImageClass {
    fn default() -> Self {
        Self {
            downsample: Downsample::Bicubic,
            dpi: 150.0,
            above: 225.0,
            compression: Compression::Jpeg,
            quality: Quality::Medium,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FontRemoval {
    /// "Keine eingebettete Schriftart entfernen"
    None,
    /// Standard fonts every viewer has (Helvetica/Arial, Times, Courier,
    /// Symbol, ZapfDingbats).
    #[default]
    Recommended,
    /// All embedded simple fonts (composite fonts are always kept).
    All,
    /// The fonts listed in `remove_fonts`.
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum EmbedAnnots {
    /// "Keine Anmerkung einbetten"
    #[default]
    None,
    /// "Formularfelder einbetten"
    Fields,
    /// "Markierungs-Anmerkungen einbetten" (comments, not links or fields)
    Markup,
    /// "Anmerkungen und Formularfelder einbetten"
    All,
}

impl EmbedAnnots {
    /// Whether an annotation of `subtype` becomes page content.
    pub fn embeds(self, subtype: &[u8]) -> bool {
        match self {
            EmbedAnnots::None => false,
            EmbedAnnots::Fields => subtype == b"Widget",
            EmbedAnnots::Markup => MARKUP_SUBTYPES.contains(&subtype),
            EmbedAnnots::All => true,
        }
    }
}

/// Markup annotations of PDF 32000-1, 12.5.6.2 (table 170).
const MARKUP_SUBTYPES: [&[u8]; 17] = [
    b"Text",
    b"FreeText",
    b"Line",
    b"Square",
    b"Circle",
    b"Polygon",
    b"PolyLine",
    b"Highlight",
    b"Underline",
    b"Squiggly",
    b"StrikeOut",
    b"Stamp",
    b"Caret",
    b"Ink",
    b"FileAttachment",
    b"Sound",
    b"Redact",
];

/// Private page key that holds the annotations a partial embed keeps.
const PARKED_ANNOTS: &[u8] = b"VersoloPDFKeptAnnots";

/// All settings of the "PDF optimieren" dialog.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OptimizeOptions {
    /// "Kompatibel mit": None keeps the version, otherwise "1.3" … "1.7".
    pub version: Option<String>,

    pub images: bool,
    pub color: ImageClass,
    pub gray: ImageClass,
    pub indexed: ImageClass,
    pub mono: ImageClass,
    /// "Bilder nur optimieren, falls Platz eingespart wird"
    pub only_if_smaller: bool,
    /// "Anmerkungen ebenfalls optimieren"
    pub annotation_images: bool,

    pub fonts: bool,
    pub font_removal: FontRemoval,
    /// Base names for [`FontRemoval::Manual`].
    pub remove_fonts: Vec<String>,

    pub objects: bool,
    pub form_actions: bool,
    pub embed_annots: EmbedAnnots,
    pub javascript: bool,
    pub alternate_images: bool,
    pub thumbnails: bool,
    pub print_settings: bool,
    pub bookmarks: bool,

    pub user_data: bool,
    pub all_annotations: bool,
    pub links: bool,
    pub metadata: bool,
    pub attachments: bool,
    pub external_refs: bool,
    pub private_data: bool,
    /// "Versteckte Ebenen entfernen und sichtbare Ebenen zusammenfassen"
    pub hidden_layers: bool,

    pub cleanup: bool,
    pub dup_fonts: bool,
    pub dup_images: bool,
    pub dup_forms: bool,
    pub compress_streams: bool,
    pub lzw_to_flate: bool,
    pub invalid_bookmarks: bool,
    pub invalid_links: bool,
    pub unused_dests: bool,
    pub outside_crop: bool,
    pub unused_layers: bool,
    pub optimize_content: bool,
}

impl Default for OptimizeOptions {
    /// The preset "Standard".
    fn default() -> Self {
        let photo = ImageClass::default();
        Self {
            version: None,
            images: true,
            color: photo,
            gray: photo,
            indexed: ImageClass { downsample: Downsample::Subsample, compression: Compression::Zip, ..photo },
            mono: ImageClass { dpi: 300.0, above: 450.0, compression: Compression::Zip, ..photo },
            only_if_smaller: true,
            annotation_images: false,
            fonts: true,
            font_removal: FontRemoval::Recommended,
            remove_fonts: Vec::new(),
            objects: true,
            form_actions: false,
            embed_annots: EmbedAnnots::None,
            javascript: false,
            alternate_images: true,
            thumbnails: false,
            print_settings: false,
            bookmarks: false,
            user_data: true,
            all_annotations: false,
            links: false,
            metadata: false,
            attachments: false,
            external_refs: false,
            private_data: false,
            hidden_layers: false,
            cleanup: true,
            dup_fonts: true,
            dup_images: true,
            dup_forms: true,
            compress_streams: true,
            lzw_to_flate: true,
            invalid_bookmarks: true,
            invalid_links: true,
            unused_dests: false,
            outside_crop: false,
            unused_layers: false,
            optimize_content: true,
        }
    }
}

impl OptimizeOptions {
    /// The preset "Mobil": lower resolution and quality.
    pub fn mobile() -> Self {
        let photo = ImageClass { dpi: 96.0, above: 144.0, quality: Quality::Low, ..ImageClass::default() };
        Self {
            color: photo,
            gray: photo,
            indexed: ImageClass { downsample: Downsample::Subsample, compression: Compression::Zip, ..photo },
            mono: ImageClass { dpi: 200.0, above: 300.0, compression: Compression::Zip, ..photo },
            thumbnails: true,
            ..Self::default()
        }
    }
}

/// What the optimizer did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OptimizeReport {
    pub before: usize,
    pub after: usize,
    pub images_changed: usize,
    pub fonts_removed: usize,
    pub duplicates_merged: usize,
    pub objects_removed: usize,
}

// ---------------------------------------------------------------------------
// Helpers

fn load(bytes: &[u8], password: Option<&str>) -> Result<Document> {
    let mut options = match password {
        Some(pw) => LoadOptions::with_password(pw),
        None => LoadOptions::default(),
    };
    options.max_decompressed_size = Some(MAX_STREAM);
    let doc = Document::load_mem_with_options(bytes, options)?;
    if doc.is_encrypted() {
        return Err(Error::EncryptedNotSupported);
    }
    Ok(doc)
}

fn resolve<'a>(doc: &'a Document, obj: &'a Object) -> &'a Object {
    match obj {
        Object::Reference(id) => doc.get_object(*id).unwrap_or(obj),
        other => other,
    }
}

fn dict_of<'a>(doc: &'a Document, obj: &'a Object) -> Option<&'a Dictionary> {
    match resolve(doc, obj) {
        Object::Dictionary(d) => Some(d),
        Object::Stream(s) => Some(&s.dict),
        _ => None,
    }
}

fn name_of(dict: &Dictionary, key: &[u8]) -> Option<Vec<u8>> {
    dict.get(key).ok().and_then(|o| o.as_name().ok()).map(<[u8]>::to_vec)
}

fn num(obj: &Object) -> Option<f32> {
    match obj {
        Object::Integer(i) => Some(*i as f32),
        Object::Real(r) => Some(*r),
        _ => None,
    }
}

fn catalog_id(doc: &Document) -> Option<ObjectId> {
    doc.trailer.get(b"Root").ok().and_then(|o| o.as_reference().ok())
}

fn catalog_mut(doc: &mut Document) -> Option<&mut Dictionary> {
    let id = catalog_id(doc)?;
    doc.get_object_mut(id).ok()?.as_dict_mut().ok()
}

/// The page's resources (inherited from parents if needed).
fn page_resources(doc: &Document, page: ObjectId) -> Option<Dictionary> {
    let mut current = doc.get_dictionary(page).ok()?;
    for _ in 0..32 {
        if let Ok(res) = current.get(b"Resources") {
            return dict_of(doc, res).cloned();
        }
        let parent = current.get(b"Parent").ok()?.as_reference().ok()?;
        current = doc.get_dictionary(parent).ok()?;
    }
    None
}

// ---------------------------------------------------------------------------
// Where images are shown (effective resolution)

/// Smallest effective resolution (dpi) of every image drawn on a page.
fn image_resolutions(doc: &Document) -> HashMap<ObjectId, f32> {
    let mut out: HashMap<ObjectId, f32> = HashMap::new();
    for page in doc.get_pages().into_values() {
        let Some(resources) = page_resources(doc, page) else { continue };
        let content = doc.get_page_content(page);
        scan_content(doc, &content, &resources, Matrix::IDENTITY, &mut out, 0);
    }
    out
}

fn scan_content(
    doc: &Document,
    content: &[u8],
    resources: &Dictionary,
    base: Matrix,
    out: &mut HashMap<ObjectId, f32>,
    depth: u32,
) {
    if depth > 8 {
        return;
    }
    let Ok(ops) = Content::decode(content) else { return };
    let xobjects = resources.get(b"XObject").ok().and_then(|o| dict_of(doc, o));
    let mut stack = Vec::new();
    let mut ctm = base;
    for op in ops.operations {
        match op.operator.as_str() {
            "q" => stack.push(ctm),
            "Q" => ctm = stack.pop().unwrap_or(base),
            "cm" if op.operands.len() == 6 => {
                let v: Vec<f32> = op.operands.iter().filter_map(num).collect();
                if v.len() == 6 {
                    ctm = Matrix([v[0], v[1], v[2], v[3], v[4], v[5]]).then(ctm);
                }
            }
            "Do" => {
                let Some(name) = op.operands.first().and_then(|o| o.as_name().ok()) else { continue };
                let Some(Ok(Object::Reference(id))) = xobjects.map(|x| x.get(name)) else { continue };
                let Ok(Object::Stream(stream)) = doc.get_object(*id) else { continue };
                match name_of(&stream.dict, b"Subtype").as_deref() {
                    Some(b"Image") => {
                        let w = stream.dict.get(b"Width").ok().and_then(num).unwrap_or(0.0);
                        let h = stream.dict.get(b"Height").ok().and_then(num).unwrap_or(0.0);
                        let [a, b, c, d, ..] = ctm.0;
                        let (sw, sh) = ((a * a + b * b).sqrt(), (c * c + d * d).sqrt());
                        if sw > 0.01 && sh > 0.01 {
                            let dpi = (w / (sw / 72.0)).min(h / (sh / 72.0));
                            let entry = out.entry(*id).or_insert(dpi);
                            *entry = entry.min(dpi);
                        }
                    }
                    Some(b"Form") => {
                        let matrix = stream
                            .dict
                            .get(b"Matrix")
                            .ok()
                            .and_then(|m| m.as_array().ok())
                            .map(|a| a.iter().filter_map(num).collect::<Vec<_>>())
                            .filter(|v| v.len() == 6)
                            .map(|v| Matrix([v[0], v[1], v[2], v[3], v[4], v[5]]))
                            .unwrap_or(Matrix::IDENTITY);
                        let res = stream
                            .dict
                            .get(b"Resources")
                            .ok()
                            .and_then(|r| dict_of(doc, r))
                            .cloned()
                            .unwrap_or_else(|| resources.clone());
                        if let Ok(inner) = stream.decompressed_content() {
                            scan_content(doc, &inner, &res, matrix.then(ctm), out, depth + 1);
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

/// Ids of all objects reachable from annotation appearances.
fn annotation_objects(doc: &Document) -> HashSet<ObjectId> {
    let mut starts = Vec::new();
    for page in doc.get_pages().into_values() {
        let Ok(dict) = doc.get_dictionary(page) else { continue };
        let Some(annots) = dict.get(b"Annots").ok().map(|a| resolve(doc, a)) else { continue };
        for annot in annots.as_array().map(|a| a.as_slice()).unwrap_or_default() {
            if let Some(ap) = dict_of(doc, annot).and_then(|d| d.get(b"AP").ok()) {
                starts.push(ap.clone());
            }
        }
    }
    let mut seen = HashSet::new();
    for start in starts {
        reach(doc, &start, &mut seen);
    }
    seen
}

/// Collects object ids reachable from `obj`; does not walk into pages,
/// page trees, the catalog or `/P` / `/Parent` back links.
fn reach(doc: &Document, obj: &Object, seen: &mut HashSet<ObjectId>) {
    let mut todo = vec![obj.clone()];
    while let Some(obj) = todo.pop() {
        match obj {
            Object::Reference(id) => {
                if seen.contains(&id) {
                    continue;
                }
                let Ok(target) = doc.get_object(id) else { continue };
                let dict = match target {
                    Object::Dictionary(d) => Some(d),
                    Object::Stream(s) => Some(&s.dict),
                    _ => None,
                };
                if dict
                    .and_then(|d| name_of(d, b"Type"))
                    .is_some_and(|t| t == b"Page" || t == b"Pages" || t == b"Catalog")
                {
                    continue;
                }
                seen.insert(id);
                todo.push(target.clone());
            }
            Object::Array(items) => todo.extend(items),
            Object::Dictionary(d) => todo.extend(
                d.iter()
                    .filter(|(k, _)| !matches!(k.as_slice(), b"P" | b"Parent" | b"Pg"))
                    .map(|(_, v)| v.clone()),
            ),
            Object::Stream(s) => todo.extend(
                s.dict
                    .iter()
                    .filter(|(k, _)| !matches!(k.as_slice(), b"P" | b"Parent"))
                    .map(|(_, v)| v.clone()),
            ),
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Images

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Color,
    Gray,
    Indexed,
    Mono,
}

/// Image class and number of components (None: not handled, e.g. CMYK,
/// 16 bit, JPEG 2000).
fn classify(doc: &Document, dict: &Dictionary) -> Option<(Class, usize)> {
    let bpc = dict.get(b"BitsPerComponent").ok().and_then(num).unwrap_or(1.0) as u32;
    if dict.get(b"ImageMask").ok().and_then(|o| o.as_bool().ok()) == Some(true) {
        return Some((Class::Mono, 1));
    }
    let cs = resolve(doc, dict.get(b"ColorSpace").ok()?);
    let (family, n) = match cs {
        Object::Name(n) => (n.clone(), 0),
        Object::Array(a) => {
            let family = a.first().and_then(|o| o.as_name().ok()).map(<[u8]>::to_vec).unwrap_or_default();
            let n = if family == b"ICCBased" {
                a.get(1)
                    .and_then(|s| dict_of(doc, s))
                    .and_then(|d| d.get(b"N").ok())
                    .and_then(num)
                    .unwrap_or(0.0) as usize
            } else {
                0
            };
            (family, n)
        }
        _ => return None,
    };
    match (family.as_slice(), n, bpc) {
        (b"DeviceRGB" | b"CalRGB", _, 8) | (b"ICCBased", 3, 8) => Some((Class::Color, 3)),
        (b"DeviceGray" | b"CalGray", _, 8) | (b"ICCBased", 1, 8) => Some((Class::Gray, 1)),
        (b"DeviceGray" | b"CalGray", _, 1) | (b"ICCBased", 1, 1) => Some((Class::Mono, 1)),
        (b"Indexed", _, 8) => Some((Class::Indexed, 1)),
        _ => None,
    }
}

fn filters(dict: &Dictionary) -> Vec<Vec<u8>> {
    match dict.get(b"Filter") {
        Ok(Object::Name(n)) => vec![n.clone()],
        Ok(Object::Array(a)) => a.iter().filter_map(|o| o.as_name().ok()).map(<[u8]>::to_vec).collect(),
        _ => Vec::new(),
    }
}

/// Decoded samples: one byte per component (1-bit images unpacked to 0/1).
fn decode_samples(stream: &Stream, class: Class, comps: usize, w: usize, h: usize) -> Option<Vec<u8>> {
    let f = filters(&stream.dict);
    if f.iter().any(|f| f.as_slice() == b"DCTDecode") {
        if f.len() != 1 || class == Class::Indexed || class == Class::Mono {
            return None;
        }
        let img = image::load_from_memory_with_format(&stream.content, image::ImageFormat::Jpeg).ok()?;
        if img.width() as usize != w || img.height() as usize != h {
            return None;
        }
        return Some(if comps == 3 { img.to_rgb8().into_raw() } else { img.to_luma8().into_raw() });
    }
    if f.iter().any(|f| {
        !matches!(
            f.as_slice(),
            b"FlateDecode" | b"LZWDecode" | b"RunLengthDecode" | b"ASCII85Decode" | b"ASCIIHexDecode"
        )
    }) {
        return None;
    }
    if stream.dict.get(b"DecodeParms").is_ok()
        && f.iter().all(|f| f.as_slice() != b"FlateDecode" && f.as_slice() != b"LZWDecode")
    {
        return None;
    }
    let raw = if f.is_empty() {
        stream.content.clone()
    } else {
        stream.decompressed_content_with_limit(MAX_STREAM).ok()?
    };
    if class == Class::Mono {
        let row = w.div_ceil(8);
        if raw.len() < row * h {
            return None;
        }
        let mut out = Vec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                out.push((raw[y * row + x / 8] >> (7 - x % 8)) & 1);
            }
        }
        return Some(out);
    }
    (raw.len() >= w * h * comps).then(|| raw[..w * h * comps].to_vec())
}

/// Scales samples from `(w, h)` to `(nw, nh)`.
fn resample(
    samples: &[u8],
    comps: usize,
    (w, h): (u32, u32),
    (nw, nh): (u32, u32),
    method: Downsample,
    class: Class,
) -> Vec<u8> {
    use image::imageops::FilterType;
    // Indexed and black-and-white images keep their exact values.
    if method == Downsample::Subsample || class == Class::Indexed || class == Class::Mono {
        let mut out = Vec::with_capacity((nw * nh) as usize * comps);
        for y in 0..nh {
            let sy = ((u64::from(y) * u64::from(h)) / u64::from(nh)) as usize;
            for x in 0..nw {
                let sx = ((u64::from(x) * u64::from(w)) / u64::from(nw)) as usize;
                let i = (sy * w as usize + sx) * comps;
                out.extend_from_slice(&samples[i..i + comps]);
            }
        }
        return out;
    }
    let filter = if method == Downsample::Average { FilterType::Triangle } else { FilterType::CatmullRom };
    if comps == 3 {
        let img = image::RgbImage::from_raw(w, h, samples.to_vec()).expect("size checked");
        image::imageops::resize(&img, nw, nh, filter).into_raw()
    } else {
        let img = image::GrayImage::from_raw(w, h, samples.to_vec()).expect("size checked");
        image::imageops::resize(&img, nw, nh, filter).into_raw()
    }
}

fn flate(data: &[u8]) -> Option<Vec<u8>> {
    let mut s = Stream::new(Dictionary::new(), data.to_vec());
    s.compress().ok()?;
    Some(s.content)
}

fn jpeg(samples: &[u8], comps: usize, w: u32, h: u32, quality: u8) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let color = if comps == 3 { image::ExtendedColorType::Rgb8 } else { image::ExtendedColorType::L8 };
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
        .encode(samples, w, h, color)
        .ok()?;
    Some(out)
}

fn pack_bits(samples: &[u8], w: usize, h: usize) -> Vec<u8> {
    let row = w.div_ceil(8);
    let mut out = vec![0u8; row * h];
    for y in 0..h {
        for x in 0..w {
            if samples[y * w + x] != 0 {
                out[y * row + x / 8] |= 0x80 >> (x % 8);
            }
        }
    }
    out
}

/// Downsamples and recompresses one image; returns true if it changed.
fn optimize_image(doc: &mut Document, id: ObjectId, dpi: Option<f32>, options: &OptimizeOptions) -> bool {
    let Ok(Object::Stream(stream)) = doc.get_object(id) else { return false };
    let Some((class, comps)) = classify(doc, &stream.dict) else { return false };
    let settings = match class {
        Class::Color => options.color,
        Class::Gray => options.gray,
        Class::Indexed => options.indexed,
        Class::Mono => options.mono,
    };
    let w = stream.dict.get(b"Width").ok().and_then(num).unwrap_or(0.0) as u32;
    let h = stream.dict.get(b"Height").ok().and_then(num).unwrap_or(0.0) as u32;
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
        return false;
    }
    // New size.
    let (mut nw, mut nh) = (w, h);
    if let Some(dpi) = dpi
        && settings.downsample != Downsample::None
        && settings.dpi >= 9.0
        && dpi > settings.above.max(settings.dpi)
    {
        let factor = settings.dpi / dpi;
        nw = ((w as f32 * factor).round() as u32).max(1);
        nh = ((h as f32 * factor).round() as u32).max(1);
    }
    let resized = (nw, nh) != (w, h);
    let was_jpeg = filters(&stream.dict).iter().any(|f| f.as_slice() == b"DCTDecode");
    if settings.compression == Compression::Keep && !resized {
        return false;
    }
    let Some(samples) = decode_samples(stream, class, comps, w as usize, h as usize) else { return false };
    let samples = if resized {
        resample(&samples, comps, (w, h), (nw, nh), settings.downsample, class)
    } else {
        samples
    };
    let lossy_ok = matches!(class, Class::Color | Class::Gray);
    let want = match settings.compression {
        Compression::Keep if was_jpeg => Compression::Jpeg,
        Compression::Keep => Compression::Zip,
        c if !lossy_ok => {
            let _ = c;
            Compression::Zip
        }
        c => c,
    };
    let zip = || {
        if class == Class::Mono {
            flate(&pack_bits(&samples, nw as usize, nh as usize))
        } else {
            flate(&samples)
        }
    };
    let encoded: Option<(Vec<u8>, &[u8])> = match want {
        Compression::Jpeg => {
            jpeg(&samples, comps, nw, nh, settings.quality.jpeg()).map(|d| (d, b"DCTDecode".as_slice()))
        }
        Compression::Zip | Compression::Keep => zip().map(|d| (d, b"FlateDecode".as_slice())),
        Compression::JpegOrZip => {
            let j = jpeg(&samples, comps, nw, nh, settings.quality.jpeg());
            let z = zip();
            match (j, z) {
                (Some(j), Some(z)) if j.len() < z.len() => Some((j, b"DCTDecode".as_slice())),
                (_, Some(z)) => Some((z, b"FlateDecode".as_slice())),
                (Some(j), None) => Some((j, b"DCTDecode".as_slice())),
                _ => None,
            }
        }
    };
    let Some((data, filter)) = encoded else { return false };
    if options.only_if_smaller && data.len() >= stream.content.len() && !resized {
        return false;
    }
    let Ok(Object::Stream(stream)) = doc.get_object_mut(id) else { return false };
    stream.dict.set("Width", Object::Integer(i64::from(nw)));
    stream.dict.set("Height", Object::Integer(i64::from(nh)));
    stream.dict.set("Filter", Object::Name(filter.to_vec()));
    stream.dict.remove(b"DecodeParms");
    stream.dict.remove(b"Length");
    stream.set_content(data);
    true
}

// ---------------------------------------------------------------------------
// Fonts

/// An embedded font of the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontInfo {
    /// Base font name without the subset prefix.
    pub name: String,
    pub subtype: String,
    pub subset: bool,
    /// Simple font (only these can be un-embedded).
    pub simple: bool,
}

fn base_name(raw: &[u8]) -> (String, bool) {
    let s = String::from_utf8_lossy(raw).into_owned();
    match s.split_once('+') {
        Some((tag, rest)) if tag.len() == 6 && tag.chars().all(|c| c.is_ascii_uppercase()) => {
            (rest.to_string(), true)
        }
        _ => (s, false),
    }
}

/// Font dictionaries with an embedded font file: (font id, descriptor id).
fn embedded_fonts(doc: &Document) -> Vec<(ObjectId, Option<ObjectId>, FontInfo)> {
    let mut out = Vec::new();
    for (id, obj) in &doc.objects {
        let Object::Dictionary(dict) = obj else { continue };
        if name_of(dict, b"Type").as_deref() != Some(b"Font") {
            continue;
        }
        let subtype = name_of(dict, b"Subtype").unwrap_or_default();
        if subtype == b"Type0" || subtype == b"Type3" {
            continue;
        }
        let descriptor = dict.get(b"FontDescriptor").ok();
        let desc_dict = descriptor.and_then(|d| dict_of(doc, d));
        let embedded =
            desc_dict.is_some_and(|d| d.has(b"FontFile") || d.has(b"FontFile2") || d.has(b"FontFile3"));
        if !embedded {
            continue;
        }
        let simple = matches!(subtype.as_slice(), b"Type1" | b"TrueType" | b"MMType1");
        let (name, subset) = base_name(&name_of(dict, b"BaseFont").unwrap_or_default());
        let desc_id = descriptor.and_then(|d| d.as_reference().ok());
        out.push((
            *id,
            desc_id,
            FontInfo { name, subtype: String::from_utf8_lossy(&subtype).into_owned(), subset, simple },
        ));
    }
    // CID fonts below Type0 fonts are listed but never removed.
    for obj in doc.objects.values() {
        let Object::Dictionary(dict) = obj else { continue };
        if name_of(dict, b"Type").as_deref() == Some(b"Font")
            && matches!(name_of(dict, b"Subtype").as_deref(), Some(b"CIDFontType0" | b"CIDFontType2"))
        {
            let (name, subset) = base_name(&name_of(dict, b"BaseFont").unwrap_or_default());
            let subtype =
                String::from_utf8_lossy(&name_of(dict, b"Subtype").unwrap_or_default()).into_owned();
            if !out.iter().any(|(_, _, f)| f.name == name) {
                out.push(((0, 0), None, FontInfo { name, subtype, subset, simple: false }));
            }
        }
    }
    out
}

/// The embedded fonts (for the "Schriftarten" page and the properties).
pub fn list_fonts(bytes: &[u8], password: Option<&str>) -> Result<Vec<FontInfo>> {
    let doc = load(bytes, password)?;
    let mut fonts: Vec<FontInfo> = embedded_fonts(&doc).into_iter().map(|(_, _, f)| f).collect();
    fonts.sort_by(|a, b| a.name.cmp(&b.name));
    fonts.dedup_by(|a, b| a.name == b.name);
    Ok(fonts)
}

/// Fonts every PDF viewer provides itself.
pub fn is_standard_font(name: &str) -> bool {
    let n = name.to_ascii_lowercase().replace([' ', '-', ','], "");
    [
        "helvetica",
        "arial",
        "times",
        "timesnewroman",
        "timesroman",
        "courier",
        "couriernew",
        "symbol",
        "zapfdingbats",
    ]
    .iter()
    .any(|base| {
        // e.g. "Arial-BoldMT", "TimesNewRomanPS-ItalicMT", "Helvetica-Oblique"
        n.strip_prefix(base).is_some_and(|rest| {
            let rest = rest.strip_prefix("ps").unwrap_or(rest);
            let rest = rest.strip_suffix("mt").unwrap_or(rest);
            matches!(rest, "" | "bold" | "italic" | "bolditalic" | "oblique" | "boldoblique" | "roman")
        })
    })
}

fn remove_fonts(doc: &mut Document, options: &OptimizeOptions) -> usize {
    let mut removed = 0;
    for (_, desc, info) in embedded_fonts(doc) {
        if !info.simple {
            continue;
        }
        let remove = match options.font_removal {
            FontRemoval::None => false,
            FontRemoval::Recommended => is_standard_font(&info.name),
            FontRemoval::All => true,
            FontRemoval::Manual => options.remove_fonts.contains(&info.name),
        };
        if remove
            && let Some(desc) = desc
            && let Ok(d) = doc.get_object_mut(desc).and_then(Object::as_dict_mut)
        {
            for key in [b"FontFile".as_slice(), b"FontFile2", b"FontFile3"] {
                d.remove(key);
            }
            removed += 1;
        }
    }
    removed
}

// ---------------------------------------------------------------------------
// Objects and user data

fn action_kind(doc: &Document, action: &Object) -> Option<Vec<u8>> {
    dict_of(doc, action).and_then(|a| name_of(a, b"S"))
}

fn is_external(kind: &[u8]) -> bool {
    matches!(kind, b"GoToR" | b"GoToE" | b"Launch" | b"ImportData" | b"SubmitForm")
}

/// Removes objects and user data chosen in the dialog; returns how many
/// things were removed.
fn remove_objects(doc: &mut Document, o: &OptimizeOptions) -> usize {
    let mut removed = 0;
    let page_ids: Vec<ObjectId> = doc.get_pages().into_values().collect();
    let objects_on = o.objects;
    let data_on = o.user_data;

    // Annotations of the pages.
    for &page in &page_ids {
        let annots: Vec<Object> = match doc.get_dictionary(page).ok().and_then(|d| d.get(b"Annots").ok()) {
            Some(a) => resolve(doc, a).as_array().cloned().unwrap_or_default(),
            None => continue,
        };
        let keep: Vec<Object> = annots
            .iter()
            .filter(|a| {
                let Some(dict) = dict_of(doc, a) else { return true };
                let subtype = name_of(dict, b"Subtype").unwrap_or_default();
                let action = dict.get(b"A").ok().and_then(|x| action_kind(doc, x)).unwrap_or_default();
                !(data_on && o.all_annotations
                    || data_on && o.links && subtype == b"Link"
                    || data_on && o.attachments && subtype == b"FileAttachment"
                    || data_on && o.external_refs && subtype == b"Link" && is_external(&action))
            })
            .cloned()
            .collect();
        if keep.len() != annots.len() {
            removed += annots.len() - keep.len();
            if let Ok(d) = doc.get_object_mut(page).and_then(Object::as_dict_mut) {
                if keep.is_empty() {
                    d.remove(b"Annots");
                } else {
                    d.set("Annots", Object::Array(keep));
                }
            }
        }
        if objects_on
            && o.thumbnails
            && let Ok(d) = doc.get_object_mut(page).and_then(Object::as_dict_mut)
            && d.remove(b"Thumb").is_some()
        {
            removed += 1;
        }
        if data_on
            && o.private_data
            && let Ok(d) = doc.get_object_mut(page).and_then(Object::as_dict_mut)
            && d.remove(b"PieceInfo").is_some()
        {
            removed += 1;
        }
    }

    // Actions on all dictionaries (widgets, fields, annotations, outlines).
    let ids: Vec<ObjectId> = doc.objects.keys().copied().collect();
    for id in ids {
        let (remove_a, remove_aa, alternates, piece) = {
            let Ok(obj) = doc.get_object(id) else { continue };
            let dict = match obj {
                Object::Dictionary(d) => d,
                Object::Stream(s) => &s.dict,
                _ => continue,
            };
            let action = dict.get(b"A").ok().and_then(|a| action_kind(doc, a)).unwrap_or_default();
            let remove_a = objects_on
                && o.form_actions
                && matches!(action.as_slice(), b"SubmitForm" | b"ImportData" | b"ResetForm")
                || objects_on && o.javascript && action == b"JavaScript"
                || data_on && o.external_refs && is_external(&action);
            let remove_aa = objects_on && o.javascript && dict.has(b"AA");
            let alternates = objects_on && o.alternate_images && dict.has(b"Alternates");
            let piece = data_on && o.private_data && dict.has(b"PieceInfo");
            (remove_a, remove_aa, alternates, piece)
        };
        if !(remove_a || remove_aa || alternates || piece) {
            continue;
        }
        let dict = match doc.get_object_mut(id) {
            Ok(Object::Dictionary(d)) => d,
            Ok(Object::Stream(s)) => &mut s.dict,
            _ => continue,
        };
        for (flag, key) in [
            (remove_a, b"A".as_slice()),
            (remove_aa, b"AA"),
            (alternates, b"Alternates"),
            (piece, b"PieceInfo"),
        ] {
            if flag && dict.remove(key).is_some() {
                removed += 1;
            }
        }
    }

    // Catalog level.
    let open_action_js = catalog_id(doc)
        .and_then(|c| doc.get_dictionary(c).ok())
        .and_then(|c| c.get(b"OpenAction").ok())
        .and_then(|a| action_kind(doc, a))
        .unwrap_or_default();
    let names_id = catalog_id(doc)
        .and_then(|c| doc.get_dictionary(c).ok())
        .and_then(|c| c.get(b"Names").ok())
        .and_then(|n| n.as_reference().ok());
    if let Some(cat) = catalog_mut(doc) {
        let mut drop = |cat: &mut Dictionary, key: &[u8]| {
            if cat.remove(key).is_some() {
                removed += 1;
            }
        };
        if objects_on && o.javascript {
            drop(cat, b"AA");
            if open_action_js == b"JavaScript" {
                drop(cat, b"OpenAction");
            }
        }
        if data_on && o.external_refs && is_external(&open_action_js) {
            drop(cat, b"OpenAction");
        }
        if objects_on && o.bookmarks {
            drop(cat, b"Outlines");
            if cat.get(b"PageMode").ok().and_then(|m| m.as_name().ok()) == Some(b"UseOutlines") {
                cat.set("PageMode", Object::Name(b"UseNone".to_vec()));
            }
        }
        if data_on && o.all_annotations {
            drop(cat, b"AcroForm");
        }
        if data_on && o.metadata {
            drop(cat, b"Metadata");
        }
        if data_on && o.private_data {
            drop(cat, b"PieceInfo");
        }
        if objects_on
            && o.print_settings
            && let Ok(Object::Dictionary(vp)) = cat.get_mut(b"ViewerPreferences")
        {
            for key in
                [b"PrintScaling".as_slice(), b"Duplex", b"PickTrayByPDFSize", b"PrintPageRange", b"NumCopies"]
            {
                if vp.remove(key).is_some() {
                    removed += 1;
                }
            }
        }
        // The names dictionary may be direct.
        if let Ok(Object::Dictionary(names)) = cat.get_mut(b"Names") {
            if objects_on && o.javascript && names.remove(b"JavaScript").is_some() {
                removed += 1;
            }
            if data_on && o.attachments && names.remove(b"EmbeddedFiles").is_some() {
                removed += 1;
            }
        }
    }
    if let Some(id) = names_id
        && let Ok(names) = doc.get_object_mut(id).and_then(Object::as_dict_mut)
    {
        if objects_on && o.javascript && names.remove(b"JavaScript").is_some() {
            removed += 1;
        }
        if data_on && o.attachments && names.remove(b"EmbeddedFiles").is_some() {
            removed += 1;
        }
    }
    // Viewer preferences stored indirectly.
    if objects_on
        && o.print_settings
        && let Some(vp_id) = catalog_id(doc)
            .and_then(|c| doc.get_dictionary(c).ok())
            .and_then(|c| c.get(b"ViewerPreferences").ok())
            .and_then(|v| v.as_reference().ok())
        && let Ok(vp) = doc.get_object_mut(vp_id).and_then(Object::as_dict_mut)
    {
        for key in
            [b"PrintScaling".as_slice(), b"Duplex", b"PickTrayByPDFSize", b"PrintPageRange", b"NumCopies"]
        {
            if vp.remove(key).is_some() {
                removed += 1;
            }
        }
    }
    if data_on && o.metadata && doc.trailer.remove(b"Info").is_some() {
        removed += 1;
    }
    removed
}

// ---------------------------------------------------------------------------
// Clean-up

/// Fingerprint of a stream (dictionary without /Length plus content).
fn fingerprint(stream: &Stream) -> [u8; 32] {
    let mut dict = stream.dict.clone();
    dict.remove(b"Length");
    let mut h = Sha256::new();
    h.update(format!("{dict:?}").as_bytes());
    h.update(&stream.content);
    h.finalize().into()
}

/// Replaces every reference to a key of `map` by its value.
fn redirect(doc: &mut Document, map: &HashMap<ObjectId, ObjectId>) {
    if map.is_empty() {
        return;
    }
    fn fix(obj: &mut Object, map: &HashMap<ObjectId, ObjectId>) {
        match obj {
            Object::Reference(id) => {
                if let Some(new) = map.get(id) {
                    *id = *new;
                }
            }
            Object::Array(items) => items.iter_mut().for_each(|o| fix(o, map)),
            Object::Dictionary(d) => d.iter_mut().for_each(|(_, v)| fix(v, map)),
            Object::Stream(s) => s.dict.iter_mut().for_each(|(_, v)| fix(v, map)),
            _ => {}
        }
    }
    for obj in doc.objects.values_mut() {
        fix(obj, map);
    }
    for (_, v) in doc.trailer.iter_mut() {
        fix(v, map);
    }
    for id in map.keys() {
        doc.objects.remove(id);
    }
}

/// Font file streams (referenced from font descriptors).
fn font_file_ids(doc: &Document) -> HashSet<ObjectId> {
    doc.objects
        .values()
        .filter_map(|o| o.as_dict().ok())
        .filter(|d| name_of(d, b"Type").as_deref() == Some(b"FontDescriptor"))
        .flat_map(|d| {
            [b"FontFile".as_slice(), b"FontFile2", b"FontFile3"]
                .into_iter()
                .filter_map(|k| d.get(k).ok().and_then(|o| o.as_reference().ok()))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn merge_duplicates(doc: &mut Document, o: &OptimizeOptions) -> usize {
    let fonts = font_file_ids(doc);
    let mut first: HashMap<[u8; 32], ObjectId> = HashMap::new();
    let mut map = HashMap::new();
    let mut ids: Vec<ObjectId> = doc.objects.keys().copied().collect();
    ids.sort();
    for id in ids {
        let Ok(Object::Stream(s)) = doc.get_object(id) else { continue };
        let subtype = name_of(&s.dict, b"Subtype").unwrap_or_default();
        let wanted = (o.dup_images && subtype == b"Image")
            || (o.dup_forms && subtype == b"Form")
            || (o.dup_fonts && fonts.contains(&id));
        if !wanted {
            continue;
        }
        let key = fingerprint(s);
        match first.get(&key) {
            Some(&keep) => {
                map.insert(id, keep);
            }
            None => {
                first.insert(key, id);
            }
        }
    }
    let mut merged = map.len();
    redirect(doc, &map);
    // Font dictionaries that became identical.
    if o.dup_fonts {
        let mut seen: HashMap<String, ObjectId> = HashMap::new();
        let mut map = HashMap::new();
        let mut ids: Vec<ObjectId> = doc.objects.keys().copied().collect();
        ids.sort();
        for id in ids {
            let Ok(Object::Dictionary(d)) = doc.get_object(id) else { continue };
            if name_of(d, b"Type").as_deref() != Some(b"Font")
                && name_of(d, b"Type").as_deref() != Some(b"FontDescriptor")
            {
                continue;
            }
            let key = format!("{d:?}");
            match seen.get(&key) {
                Some(&keep) => {
                    map.insert(id, keep);
                }
                None => {
                    seen.insert(key, id);
                }
            }
        }
        merged += map.len();
        redirect(doc, &map);
    }
    merged
}

/// Destination of an outline item or link: false if it points to a page
/// object that does not exist.
fn dest_valid(doc: &Document, pages: &HashSet<ObjectId>, dict: &Dictionary) -> bool {
    let dest = dict.get(b"Dest").ok().or_else(|| {
        dict.get(b"A")
            .ok()
            .and_then(|a| dict_of(doc, a))
            .filter(|a| name_of(a, b"S").as_deref() == Some(b"GoTo"))
            .and_then(|a| a.get(b"D").ok())
    });
    match dest.map(|d| resolve(doc, d)) {
        Some(Object::Array(a)) => match a.first() {
            Some(Object::Reference(id)) => pages.contains(id),
            Some(Object::Integer(_)) => true,
            _ => false,
        },
        _ => true,
    }
}

fn remove_invalid(doc: &mut Document, o: &OptimizeOptions) -> usize {
    let pages: HashSet<ObjectId> = doc.get_pages().into_values().collect();
    let mut removed = 0;
    if o.invalid_links {
        for &page in &pages {
            let annots: Vec<Object> = match doc.get_dictionary(page).ok().and_then(|d| d.get(b"Annots").ok())
            {
                Some(a) => resolve(doc, a).as_array().cloned().unwrap_or_default(),
                None => continue,
            };
            let keep: Vec<Object> = annots
                .iter()
                .filter(|a| {
                    dict_of(doc, a).is_none_or(|d| {
                        name_of(d, b"Subtype").as_deref() != Some(b"Link") || dest_valid(doc, &pages, d)
                    })
                })
                .cloned()
                .collect();
            if keep.len() != annots.len() {
                removed += annots.len() - keep.len();
                if let Ok(d) = doc.get_object_mut(page).and_then(Object::as_dict_mut) {
                    d.set("Annots", Object::Array(keep));
                }
            }
        }
    }
    if o.invalid_bookmarks
        && let Some(root) = catalog_id(doc)
            .and_then(|c| doc.get_dictionary(c).ok())
            .and_then(|c| c.get(b"Outlines").ok())
            .and_then(|r| r.as_reference().ok())
    {
        removed += prune_outline(doc, &pages, root, 0);
    }
    removed
}

/// Removes outline items with invalid destinations below `parent` (with
/// their children) and relinks the rest.
fn prune_outline(doc: &mut Document, pages: &HashSet<ObjectId>, parent: ObjectId, depth: u32) -> usize {
    if depth > 64 {
        return 0;
    }
    let mut children = Vec::new();
    let mut next = doc
        .get_dictionary(parent)
        .ok()
        .and_then(|d| d.get(b"First").ok())
        .and_then(|f| f.as_reference().ok());
    while let Some(id) = next {
        if children.contains(&id) || children.len() > 100_000 {
            break;
        }
        children.push(id);
        next =
            doc.get_dictionary(id).ok().and_then(|d| d.get(b"Next").ok()).and_then(|n| n.as_reference().ok());
    }
    let mut removed = 0;
    let mut keep = Vec::new();
    for id in children {
        let valid = doc.get_dictionary(id).map(|d| dest_valid(doc, pages, d)).unwrap_or(false);
        if valid {
            removed += prune_outline(doc, pages, id, depth + 1);
            keep.push(id);
        } else {
            removed += 1;
        }
    }
    if removed == 0 {
        return 0;
    }
    for (i, id) in keep.iter().enumerate() {
        if let Ok(d) = doc.get_object_mut(*id).and_then(Object::as_dict_mut) {
            match i.checked_sub(1).map(|p| keep[p]) {
                Some(prev) => d.set("Prev", Object::Reference(prev)),
                None => {
                    d.remove(b"Prev");
                }
            }
            match keep.get(i + 1) {
                Some(next) => d.set("Next", Object::Reference(*next)),
                None => {
                    d.remove(b"Next");
                }
            }
        }
    }
    if let Ok(d) = doc.get_object_mut(parent).and_then(Object::as_dict_mut) {
        match (keep.first(), keep.last()) {
            (Some(f), Some(l)) => {
                d.set("First", Object::Reference(*f));
                d.set("Last", Object::Reference(*l));
                let open = d.get(b"Count").ok().and_then(|c| c.as_i64().ok()).unwrap_or(0) >= 0;
                d.set("Count", Object::Integer(if open { keep.len() as i64 } else { -(keep.len() as i64) }));
            }
            _ => {
                d.remove(b"First");
                d.remove(b"Last");
                d.remove(b"Count");
            }
        }
    }
    removed
}

/// Names used as destinations anywhere in the document.
fn used_dest_names(doc: &Document) -> HashSet<Vec<u8>> {
    let mut used = HashSet::new();
    for obj in doc.objects.values() {
        let dict = match obj {
            Object::Dictionary(d) => d,
            Object::Stream(s) => &s.dict,
            _ => continue,
        };
        for key in [b"Dest".as_slice(), b"D"] {
            match dict.get(key) {
                Ok(Object::String(s, _)) => {
                    used.insert(s.clone());
                }
                Ok(Object::Name(n)) => {
                    used.insert(n.clone());
                }
                _ => {}
            }
        }
    }
    used
}

fn remove_unused_dests(doc: &mut Document) -> usize {
    let used = used_dest_names(doc);
    let mut removed = 0;
    // Old style /Dests dictionary in the catalog.
    let dests = catalog_id(doc)
        .and_then(|c| doc.get_dictionary(c).ok())
        .and_then(|c| c.get(b"Dests").ok())
        .and_then(|d| d.as_reference().ok());
    if let Some(id) = dests
        && let Ok(d) = doc.get_object_mut(id).and_then(Object::as_dict_mut)
    {
        let unused: Vec<Vec<u8>> = d.iter().map(|(k, _)| k.clone()).filter(|k| !used.contains(k)).collect();
        for k in unused {
            d.remove(&k);
            removed += 1;
        }
    }
    // Name tree leaves under /Names /Dests.
    let names = catalog_id(doc)
        .and_then(|c| doc.get_dictionary(c).ok())
        .and_then(|c| c.get(b"Names").ok())
        .and_then(|n| dict_of(doc, n))
        .and_then(|n| n.get(b"Dests").ok())
        .and_then(|d| d.as_reference().ok());
    let mut todo: Vec<ObjectId> = names.into_iter().collect();
    let mut seen = HashSet::new();
    while let Some(id) = todo.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Ok(node) = doc.get_dictionary(id) else { continue };
        if let Ok(kids) = node.get(b"Kids").and_then(Object::as_array) {
            todo.extend(kids.iter().filter_map(|k| k.as_reference().ok()));
        }
        let Ok(list) = node.get(b"Names").and_then(Object::as_array) else { continue };
        let mut keep = Vec::new();
        for pair in list.chunks(2) {
            let name = match &pair[0] {
                Object::String(s, _) => s.clone(),
                _ => Vec::new(),
            };
            if pair.len() == 2 && used.contains(&name) {
                keep.extend_from_slice(pair);
            } else {
                removed += 1;
            }
        }
        if let Ok(node) = doc.get_object_mut(id).and_then(Object::as_dict_mut) {
            node.set("Names", Object::Array(keep));
        }
    }
    removed
}

fn remove_unused_layers(doc: &mut Document) -> usize {
    let Some(props_obj) = catalog_id(doc)
        .and_then(|c| doc.get_dictionary(c).ok())
        .and_then(|c| c.get(b"OCProperties").ok())
        .cloned()
    else {
        return 0;
    };
    let Some(props) = dict_of(doc, &props_obj).cloned() else { return 0 };
    let all: Vec<ObjectId> = props
        .get(b"OCGs")
        .ok()
        .and_then(|a| resolve(doc, a).as_array().ok())
        .map(|a| a.iter().filter_map(|o| o.as_reference().ok()).collect())
        .unwrap_or_default();
    // Every reference to a layer outside /OCProperties counts as use.
    let mut used = HashSet::new();
    let props_id = props_obj.as_reference().ok();
    for (id, obj) in &doc.objects {
        if Some(*id) == props_id || all.contains(id) {
            continue;
        }
        let mut refs = Vec::new();
        collect_refs(obj, &mut refs);
        used.extend(refs.into_iter().filter(|r| all.contains(r)));
    }
    let unused: Vec<ObjectId> = all.iter().copied().filter(|id| !used.contains(id)).collect();
    if unused.is_empty() {
        return 0;
    }
    fn strip(obj: &mut Object, unused: &[ObjectId]) {
        match obj {
            Object::Array(items) => {
                items.retain(|o| !o.as_reference().is_ok_and(|r| unused.contains(&r)));
                items.iter_mut().for_each(|o| strip(o, unused));
            }
            Object::Dictionary(d) => d.iter_mut().for_each(|(_, v)| strip(v, unused)),
            _ => {}
        }
    }
    let mut props = props;
    for (_, v) in props.iter_mut() {
        strip(v, &unused);
    }
    match props_id {
        Some(id) => {
            if let Ok(obj) = doc.get_object_mut(id) {
                *obj = Object::Dictionary(props);
            }
        }
        None => {
            if let Some(cat) = catalog_mut(doc) {
                cat.set("OCProperties", Object::Dictionary(props));
            }
        }
    }
    for id in &unused {
        doc.objects.remove(id);
    }
    unused.len()
}

/// The layers (OCGs) that the default configuration hides.
fn hidden_layers(doc: &Document, props: &Dictionary) -> HashSet<ObjectId> {
    let refs = |obj: Option<&Object>| -> Vec<ObjectId> {
        obj.and_then(|a| resolve(doc, a).as_array().ok())
            .map(|a| a.iter().filter_map(|o| o.as_reference().ok()).collect())
            .unwrap_or_default()
    };
    let all = refs(props.get(b"OCGs").ok());
    let config = props.get(b"D").ok().and_then(|d| dict_of(doc, d));
    let on = refs(config.and_then(|d| d.get(b"ON").ok()));
    let off = refs(config.and_then(|d| d.get(b"OFF").ok()));
    if config.and_then(|d| name_of(d, b"BaseState")).as_deref() == Some(b"OFF") {
        all.into_iter().filter(|id| !on.contains(id)).collect()
    } else {
        off.into_iter().collect()
    }
}

/// Whether an /OC entry (layer or membership dictionary) is visible.
fn oc_visible(doc: &Document, oc: &Object, hidden: &HashSet<ObjectId>) -> bool {
    let Some(dict) = dict_of(doc, oc) else { return true };
    if name_of(dict, b"Type").as_deref() != Some(b"OCMD") {
        return oc.as_reference().map_or(true, |id| !hidden.contains(&id));
    }
    let states: Vec<bool> = match dict.get(b"OCGs") {
        Ok(Object::Reference(id)) => vec![!hidden.contains(id)],
        Ok(other) => resolve(doc, other)
            .as_array()
            .map(|a| a.iter().filter_map(|o| o.as_reference().ok()).map(|id| !hidden.contains(&id)).collect())
            .unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    if states.is_empty() {
        return true;
    }
    match name_of(dict, b"P").as_deref() {
        Some(b"AllOn") => states.iter().all(|&s| s),
        Some(b"AnyOff") => states.iter().any(|&s| !s),
        Some(b"AllOff") => states.iter().all(|&s| !s),
        _ => states.iter().any(|&s| s),
    }
}

/// Drops the content of hidden layers and the layer markers of visible
/// ones. Returns None if nothing changed.
fn filter_layers(
    doc: &Document,
    ops: Vec<lopdf::content::Operation>,
    resources: &Dictionary,
    hidden: &HashSet<ObjectId>,
) -> Option<Vec<lopdf::content::Operation>> {
    let entry = |kind: &[u8], name: &Object| -> Option<Object> {
        let d = resources.get(kind).ok().and_then(|d| dict_of(doc, d))?;
        d.get(name.as_name().ok()?).ok().cloned()
    };
    // Per open marked-content block: true = layer marker (dropped).
    let mut stack: Vec<bool> = Vec::new();
    // Depth of the stack where a hidden block started.
    let mut hidden_from: Option<usize> = None;
    let mut out = Vec::with_capacity(ops.len());
    let mut changed = false;
    for op in ops {
        let keep = match op.operator.as_str() {
            "BDC" | "BMC" => {
                let layer = op.operator == "BDC"
                    && op.operands.first().and_then(|o| o.as_name().ok()) == Some(b"OC".as_slice());
                if layer && hidden_from.is_none() {
                    let oc = match op.operands.get(1) {
                        Some(n @ Object::Name(_)) => entry(b"Properties", n),
                        other => other.cloned(),
                    };
                    if !oc.is_some_and(|oc| oc_visible(doc, &oc, hidden)) {
                        hidden_from = Some(stack.len());
                    }
                }
                stack.push(layer);
                hidden_from.is_none() && !layer
            }
            "EMC" => {
                let layer = stack.pop().unwrap_or(false);
                let inside = hidden_from.is_some();
                if hidden_from == Some(stack.len()) {
                    hidden_from = None;
                }
                !inside && !layer
            }
            "Do" if hidden_from.is_none() => op
                .operands
                .first()
                .and_then(|n| entry(b"XObject", n))
                .and_then(|x| dict_of(doc, &x).and_then(|d| d.get(b"OC").ok()).cloned())
                .is_none_or(|oc| oc_visible(doc, &oc, hidden)),
            _ => hidden_from.is_none(),
        };
        if keep {
            out.push(op);
        } else {
            changed = true;
        }
    }
    changed.then_some(out)
}

/// "Versteckte Ebenen entfernen und sichtbare Ebenen zusammenfassen": the
/// content of layers hidden by default is deleted, visible layers become
/// normal content and the layer list is removed. Streams with inline images
/// are not rewritten; then the layer list stays so nothing hidden shows up.
fn merge_layers(doc: &mut Document) {
    let Some(props) = catalog_id(doc)
        .and_then(|c| doc.get_dictionary(c).ok())
        .and_then(|c| c.get(b"OCProperties").ok())
        .and_then(|p| dict_of(doc, p))
        .cloned()
    else {
        return;
    };
    let hidden = hidden_layers(doc, &props);
    let mut complete = true;
    let rewrite =
        |doc: &Document, plain: &[u8], resources: &Dictionary, complete: &mut bool| -> Option<Vec<u8>> {
            if plain.windows(3).any(|w| w == b"BI ") {
                *complete = false;
                return None;
            }
            let Ok(content) = Content::decode(plain) else {
                *complete = false;
                return None;
            };
            let ops = filter_layers(doc, content.operations, resources, &hidden)?;
            Content { operations: ops }.encode().ok()
        };
    // Pages: the contents (possibly split into several streams) as one stream.
    for page in doc.get_pages().into_values() {
        let resources = page_resources(doc, page).unwrap_or_default();
        let plain = doc.get_page_content(page);
        if let Some(new) = rewrite(doc, &plain, &resources, &mut complete) {
            let id = doc.add_object(Stream::new(Dictionary::new(), new));
            if let Ok(d) = doc.get_object_mut(page).and_then(Object::as_dict_mut) {
                d.set("Contents", id);
            }
        }
    }
    // Form XObjects with their own resources.
    let forms: Vec<ObjectId> = doc
        .objects
        .iter()
        .filter(|(_, o)| matches!(o, Object::Stream(s) if name_of(&s.dict, b"Subtype").as_deref() == Some(b"Form")))
        .map(|(id, _)| *id)
        .collect();
    for id in forms {
        let Ok(Object::Stream(s)) = doc.get_object(id) else { continue };
        let resources =
            s.dict.get(b"Resources").ok().and_then(|r| dict_of(doc, r)).cloned().unwrap_or_default();
        let Ok(plain) = s.decompressed_content_with_limit(MAX_STREAM) else { continue };
        if let Some(new) = rewrite(doc, &plain, &resources, &mut complete)
            && let Ok(Object::Stream(s)) = doc.get_object_mut(id)
        {
            s.set_plain_content(new);
            s.dict.remove(b"Filter");
            s.dict.remove(b"DecodeParms");
        }
    }
    // Annotations of hidden layers.
    for page in doc.get_pages().into_values() {
        let Some(annots) = doc
            .get_dictionary(page)
            .ok()
            .and_then(|d| d.get(b"Annots").ok())
            .and_then(|a| resolve(doc, a).as_array().ok())
            .cloned()
        else {
            continue;
        };
        let keep: Vec<Object> = annots
            .into_iter()
            .filter(|a| {
                dict_of(doc, a).and_then(|d| d.get(b"OC").ok()).is_none_or(|oc| oc_visible(doc, oc, &hidden))
            })
            .collect();
        if let Ok(d) = doc.get_object_mut(page).and_then(Object::as_dict_mut) {
            d.set("Annots", Object::Array(keep));
        }
    }
    if !complete {
        return;
    }
    // No layers any more: drop /OC entries and the layers in /Properties.
    let layer_type = |d: &Dictionary| matches!(name_of(d, b"Type").as_deref(), Some(b"OCG" | b"OCMD"));
    let layers: HashSet<ObjectId> =
        doc.objects.iter().filter(|(_, o)| o.as_dict().is_ok_and(layer_type)).map(|(id, _)| *id).collect();
    let strip = |resources: &mut Dictionary| {
        if let Ok(Object::Dictionary(props)) = resources.get_mut(b"Properties") {
            let names: Vec<Vec<u8>> = props
                .iter()
                .filter(|(_, v)| match v {
                    Object::Reference(id) => layers.contains(id),
                    Object::Dictionary(d) => layer_type(d),
                    _ => false,
                })
                .map(|(k, _)| k.clone())
                .collect();
            for k in names {
                props.remove(&k);
            }
        }
    };
    for obj in doc.objects.values_mut() {
        let dict = match obj {
            Object::Dictionary(d) => d,
            Object::Stream(s) => &mut s.dict,
            _ => continue,
        };
        dict.remove(b"OC");
        // A resource dictionary of its own, or one inside a page or form.
        strip(dict);
        if let Ok(Object::Dictionary(resources)) = dict.get_mut(b"Resources") {
            strip(resources);
        }
    }
    if let Some(cat) = catalog_mut(doc) {
        cat.remove(b"OCProperties");
    }
}

fn collect_refs(obj: &Object, out: &mut Vec<ObjectId>) {
    match obj {
        Object::Reference(id) => out.push(*id),
        Object::Array(items) => items.iter().for_each(|o| collect_refs(o, out)),
        Object::Dictionary(d) => d.iter().for_each(|(_, v)| collect_refs(v, out)),
        Object::Stream(s) => s.dict.iter().for_each(|(_, v)| collect_refs(v, out)),
        _ => {}
    }
}

/// Rewrites page content streams without superfluous whitespace.
fn optimize_contents(doc: &mut Document) {
    let mut ids = HashSet::new();
    for page in doc.get_pages().into_values() {
        if let Ok(d) = doc.get_dictionary(page) {
            match d.get(b"Contents") {
                Ok(Object::Reference(id)) => {
                    ids.insert(*id);
                }
                Ok(Object::Array(a)) => ids.extend(a.iter().filter_map(|o| o.as_reference().ok())),
                _ => {}
            }
        }
    }
    for id in ids {
        let Ok(Object::Stream(s)) = doc.get_object_mut(id) else { continue };
        let Ok(plain) = s.decompressed_content_with_limit(MAX_STREAM) else { continue };
        // Inline images contain binary data the parser does not round-trip.
        if plain.windows(3).any(|w| w == b"BI ") {
            continue;
        }
        let Ok(content) = Content::decode(&plain) else { continue };
        let Ok(encoded) = content.encode() else { continue };
        if encoded.len() < plain.len() {
            s.set_plain_content(encoded);
            s.dict.remove(b"Filter");
            s.dict.remove(b"DecodeParms");
        }
    }
}

fn compress_streams(doc: &mut Document, o: &OptimizeOptions) {
    for obj in doc.objects.values_mut() {
        let Object::Stream(s) = obj else { continue };
        let f = filters(&s.dict);
        if o.lzw_to_flate
            && f.len() == 1
            && f[0] == b"LZWDecode"
            && let Ok(plain) = s.decompressed_content_with_limit(MAX_STREAM)
        {
            s.dict.remove(b"Filter");
            s.dict.remove(b"DecodeParms");
            s.set_plain_content(plain);
            let _ = s.compress();
            continue;
        }
        if o.compress_streams && f.is_empty() && s.allows_compression {
            let _ = s.compress();
        }
    }
}

// ---------------------------------------------------------------------------
// Partial embedding of annotations

fn save(doc: &mut Document) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(|e| Error::Structure(e.to_string()))?;
    Ok(out)
}

/// Prepares a partial "Anmerkungen einbetten": the annotations `embed` does
/// not embed (and their pop-ups) move from /Annots to a private page key, so
/// the engine's flatten only sees the others. [`restore_annotations`] puts
/// them back after flattening.
pub fn park_annotations(bytes: &[u8], embed: EmbedAnnots) -> Result<Vec<u8>> {
    crate::pdf_edit::guarded(|| {
        let mut doc = load(bytes, None)?;
        let pages: Vec<ObjectId> = doc.get_pages().into_values().collect();
        for page in pages {
            let Some(annots) = doc
                .get_dictionary(page)
                .ok()
                .and_then(|d| d.get(b"Annots").ok())
                .map(|a| resolve(&doc, a).clone())
                .and_then(|a| a.as_array().ok().cloned())
            else {
                continue;
            };
            let subtype_of =
                |obj: &Object| dict_of(&doc, obj).and_then(|d| name_of(d, b"Subtype")).unwrap_or_default();
            let kept_parent = |obj: &Object| {
                dict_of(&doc, obj)
                    .and_then(|d| d.get(b"Parent").ok())
                    .is_some_and(|p| !embed.embeds(&subtype_of(p)))
            };
            let (mut flatten, mut keep) = (Vec::new(), Vec::new());
            for a in annots {
                let subtype = subtype_of(&a);
                let parked = if subtype == b"Popup" { kept_parent(&a) } else { !embed.embeds(&subtype) };
                if parked { keep.push(a) } else { flatten.push(a) }
            }
            if let Ok(d) = doc.get_object_mut(page).and_then(Object::as_dict_mut) {
                d.set("Annots", Object::Array(flatten));
                if !keep.is_empty() {
                    d.set(PARKED_ANNOTS, Object::Array(keep));
                }
            }
        }
        save(&mut doc)
    })
}

/// Undoes [`park_annotations`] after the engine flattened the pages.
pub fn restore_annotations(bytes: &[u8]) -> Result<Vec<u8>> {
    crate::pdf_edit::guarded(|| {
        let mut doc = load(bytes, None)?;
        let pages: Vec<ObjectId> = doc.get_pages().into_values().collect();
        let mut changed = false;
        for page in pages {
            let mut annots = doc
                .get_dictionary(page)
                .ok()
                .and_then(|d| d.get(b"Annots").ok())
                .and_then(|a| resolve(&doc, a).as_array().ok().cloned())
                .unwrap_or_default();
            let Ok(d) = doc.get_object_mut(page).and_then(Object::as_dict_mut) else { continue };
            let Some(Object::Array(kept)) = d.remove(PARKED_ANNOTS) else { continue };
            annots.extend(kept);
            d.set("Annots", Object::Array(annots));
            changed = true;
        }
        if changed { save(&mut doc) } else { Ok(bytes.to_vec()) }
    })
}

// ---------------------------------------------------------------------------
// Main entry

/// Optimizes `bytes` (unencrypted or opened with `password`) and returns
/// the new file. The engine based steps (embedding annotations, content
/// outside the crop box) must be done by the caller first.
pub fn optimize(
    bytes: &[u8],
    password: Option<&str>,
    o: &OptimizeOptions,
) -> Result<(Vec<u8>, OptimizeReport)> {
    crate::pdf_edit::guarded(|| {
        let mut doc = load(bytes, password)?;
        let mut report = OptimizeReport { before: bytes.len(), ..OptimizeReport::default() };
        let objects_before = doc.objects.len();

        report.objects_removed += remove_objects(&mut doc, o);
        if o.user_data && o.hidden_layers {
            merge_layers(&mut doc);
        }
        if o.cleanup {
            report.objects_removed += remove_invalid(&mut doc, o);
            if o.unused_dests {
                report.objects_removed += remove_unused_dests(&mut doc);
            }
            if o.unused_layers {
                report.objects_removed += remove_unused_layers(&mut doc);
            }
        }
        if o.fonts {
            report.fonts_removed = remove_fonts(&mut doc, o);
        }
        if o.images {
            let resolutions = image_resolutions(&doc);
            let skip = if o.annotation_images { HashSet::new() } else { annotation_objects(&doc) };
            let images: Vec<ObjectId> = doc
                .objects
                .iter()
                .filter(|(id, obj)| {
                    !skip.contains(id)
                        && matches!(obj, Object::Stream(s) if name_of(&s.dict, b"Subtype").as_deref() == Some(b"Image"))
                })
                .map(|(id, _)| *id)
                .collect();
            for id in images {
                if optimize_image(&mut doc, id, resolutions.get(&id).copied(), o) {
                    report.images_changed += 1;
                }
            }
        }
        if o.cleanup {
            if o.dup_fonts || o.dup_images || o.dup_forms {
                report.duplicates_merged = merge_duplicates(&mut doc, o);
            }
            if o.optimize_content {
                optimize_contents(&mut doc);
            }
            compress_streams(&mut doc, o);
        }
        doc.prune_objects();
        doc.delete_zero_length_streams();
        doc.renumber_objects();
        report.objects_removed += objects_before.saturating_sub(doc.objects.len());

        let version = o.version.clone().unwrap_or_else(|| doc.version.clone());
        doc.version = version.clone();
        let modern = version.as_str() >= "1.5";
        let mut out = Vec::new();
        let saved = if modern { doc.save_modern(&mut out) } else { doc.save_to(&mut out).map(|_| ()) };
        saved.map_err(|e| Error::Structure(e.to_string()))?;
        report.after = out.len();
        Ok((out, report))
    })
}

// ---------------------------------------------------------------------------
// Space usage

/// Categories of "Speicherverbrauch untersuchen".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum UsageKind {
    Images,
    Fonts,
    Forms,
    Content,
    Annotations,
    Bookmarks,
    Structure,
    Metadata,
    Attachments,
    Other,
    /// Cross reference table, object headers, file structure.
    Overhead,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Usage {
    pub total: u64,
    /// (kind, bytes, number of objects), biggest first.
    pub parts: Vec<(UsageKind, u64, usize)>,
}

/// Approximate size of an object as written in the file.
fn object_size(obj: &Object) -> u64 {
    match obj {
        Object::Null => 4,
        Object::Boolean(_) => 5,
        Object::Integer(i) => i.to_string().len() as u64 + 1,
        Object::Real(_) => 6,
        Object::Name(n) => n.len() as u64 + 2,
        Object::String(s, _) => s.len() as u64 + 2,
        Object::Array(a) => 2 + a.iter().map(object_size).sum::<u64>(),
        Object::Dictionary(d) => 4 + d.iter().map(|(k, v)| k.len() as u64 + 2 + object_size(v)).sum::<u64>(),
        Object::Stream(s) => object_size(&Object::Dictionary(s.dict.clone())) + s.content.len() as u64 + 18,
        Object::Reference(_) => 6,
    }
}

/// How the bytes of the file are used.
pub fn space_usage(bytes: &[u8], password: Option<&str>) -> Result<Usage> {
    crate::pdf_edit::guarded(|| {
        let doc = load(bytes, password)?;
        let mut kind_of: HashMap<ObjectId, UsageKind> = HashMap::new();
        let assign = |ids: HashSet<ObjectId>, kind: UsageKind, kind_of: &mut HashMap<ObjectId, UsageKind>| {
            for id in ids {
                kind_of.entry(id).or_insert(kind);
            }
        };
        // Images and forms from all streams.
        let mut images = HashSet::new();
        let mut forms = HashSet::new();
        for (id, obj) in &doc.objects {
            if let Object::Stream(s) = obj {
                match name_of(&s.dict, b"Subtype").as_deref() {
                    Some(b"Image") => {
                        images.insert(*id);
                        for key in [b"SMask".as_slice(), b"Mask"] {
                            if let Ok(Object::Reference(m)) = s.dict.get(key) {
                                images.insert(*m);
                            }
                        }
                    }
                    Some(b"Form") => {
                        forms.insert(*id);
                    }
                    _ => {}
                }
            }
        }
        assign(images, UsageKind::Images, &mut kind_of);
        // Fonts with everything they reference.
        let mut fonts = HashSet::new();
        for (id, obj) in &doc.objects {
            if let Object::Dictionary(d) = obj
                && matches!(name_of(d, b"Type").as_deref(), Some(b"Font" | b"FontDescriptor"))
            {
                fonts.insert(*id);
                reach(&doc, &Object::Dictionary(d.clone()), &mut fonts);
            }
        }
        assign(fonts, UsageKind::Fonts, &mut kind_of);
        // Annotation appearances are forms too, so annotations come first.
        let mut annots = HashSet::new();
        let mut contents = HashSet::new();
        for page in doc.get_pages().into_values() {
            let Ok(d) = doc.get_dictionary(page) else { continue };
            if let Ok(a) = d.get(b"Annots") {
                if let Ok(id) = a.as_reference() {
                    annots.insert(id);
                }
                reach(&doc, a, &mut annots);
            }
            match d.get(b"Contents") {
                Ok(Object::Reference(id)) => {
                    contents.insert(*id);
                }
                Ok(Object::Array(a)) => contents.extend(a.iter().filter_map(|o| o.as_reference().ok())),
                _ => {}
            }
        }
        assign(annots, UsageKind::Annotations, &mut kind_of);
        assign(forms, UsageKind::Forms, &mut kind_of);
        assign(contents, UsageKind::Content, &mut kind_of);
        let catalog = catalog_id(&doc).and_then(|c| doc.get_dictionary(c).ok()).cloned().unwrap_or_default();
        for (key, kind) in [
            (b"Outlines".as_slice(), UsageKind::Bookmarks),
            (b"StructTreeRoot", UsageKind::Structure),
            (b"Metadata", UsageKind::Metadata),
        ] {
            if let Ok(obj) = catalog.get(key) {
                let mut set = HashSet::new();
                if let Ok(id) = obj.as_reference() {
                    set.insert(id);
                }
                reach(&doc, obj, &mut set);
                assign(set, kind, &mut kind_of);
            }
        }
        if let Some(files) = catalog
            .get(b"Names")
            .ok()
            .and_then(|n| dict_of(&doc, n))
            .and_then(|n| n.get(b"EmbeddedFiles").ok())
        {
            let mut set = HashSet::new();
            reach(&doc, files, &mut set);
            assign(set, UsageKind::Attachments, &mut kind_of);
        }
        if let Ok(info) = doc.trailer.get(b"Info") {
            let mut set = HashSet::new();
            if let Ok(id) = info.as_reference() {
                set.insert(id);
            }
            reach(&doc, info, &mut set);
            assign(set, UsageKind::Metadata, &mut kind_of);
        }
        let mut sums: HashMap<UsageKind, (u64, usize)> = HashMap::new();
        for (id, obj) in &doc.objects {
            let kind = kind_of.get(id).copied().unwrap_or(UsageKind::Other);
            let e = sums.entry(kind).or_default();
            e.0 += object_size(obj) + 16;
            e.1 += 1;
        }
        let total = bytes.len() as u64;
        let counted: u64 = sums.values().map(|v| v.0).sum();
        let mut parts: Vec<(UsageKind, u64, usize)> = sums.into_iter().map(|(k, (b, n))| (k, b, n)).collect();
        // Compressed object streams make the estimate larger than the file.
        if counted > total && counted > 0 {
            for p in &mut parts {
                p.1 = p.1 * total / counted;
            }
        } else {
            parts.push((UsageKind::Overhead, total - counted, 0));
        }
        parts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        Ok(Usage { total, parts })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;

    /// A one page PDF showing a 600×600 RGB image (raw) on 1 inch: 600 dpi.
    fn sample(extra_link_to_missing_page: bool) -> Vec<u8> {
        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let mut pixels = Vec::with_capacity(600 * 600 * 3);
        for y in 0..600u32 {
            for x in 0..600u32 {
                pixels.extend_from_slice(&[(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8]);
            }
        }
        let image = Stream::new(
            dictionary! {"Type" => "XObject", "Subtype" => "Image", "Width" => 600, "Height" => 600,
            "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8},
            pixels,
        );
        let img = doc.add_object(image.clone());
        let img2 = doc.add_object(image); // exact duplicate
        let font_file = doc.add_object(Stream::new(dictionary! {}, b"fake font program".to_vec()));
        let desc = doc.add_object(dictionary! {"Type" => "FontDescriptor", "FontName" => "ABCDEF+Arial-BoldMT", "FontFile2" => font_file});
        let font = doc.add_object(dictionary! {"Type" => "Font", "Subtype" => "TrueType", "BaseFont" => "ABCDEF+Arial-BoldMT", "FontDescriptor" => desc});
        let content = doc.add_object(Stream::new(
            dictionary! {},
            b"q   72 0 0 72 100 100 cm   /Im1 Do   Q\nq 72 0 0 72 300 300 cm /Im2 Do Q\nBT /F1 12 Tf 10 10 Td (Hi) Tj ET".to_vec(),
        ));
        let js =
            doc.add_object(dictionary! {"S" => "JavaScript", "JS" => Object::string_literal("app.alert(1)")});
        let mut annots = vec![];
        if extra_link_to_missing_page {
            let missing = doc.add_object(dictionary! {"Type" => "Annot"});
            annots.push(Object::Reference(doc.add_object(dictionary! {"Type" => "Annot", "Subtype" => "Link",
                "Rect" => vec![0.into(), 0.into(), 10.into(), 10.into()], "Dest" => vec![missing.into(), "Fit".into()]})));
        }
        let page = doc.add_object(dictionary! {"Type" => "Page", "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()], "Contents" => content, "Annots" => annots,
            "Thumb" => img2,
            "Resources" => dictionary! {"XObject" => dictionary! {"Im1" => img, "Im2" => img2}, "Font" => dictionary! {"F1" => font}}});
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {"Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1}),
        );
        let info = doc.add_object(dictionary! {"Title" => Object::string_literal("Geheim"), "Author" => Object::string_literal("X")});
        let catalog =
            doc.add_object(dictionary! {"Type" => "Catalog", "Pages" => pages_id, "OpenAction" => js});
        doc.trailer.set("Root", catalog);
        doc.trailer.set("Info", info);
        let mut out = Vec::new();
        doc.save_to(&mut out).unwrap();
        out
    }

    #[test]
    fn hidden_layers_are_removed_and_visible_ones_merged() {
        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let a = doc.add_object(dictionary! {"Type" => "OCG", "Name" => Object::string_literal("Sichtbar")});
        let b = doc.add_object(dictionary! {"Type" => "OCG", "Name" => Object::string_literal("Versteckt")});
        // Visible only if both layers are on: hidden.
        let both = doc
            .add_object(dictionary! {"Type" => "OCMD", "OCGs" => vec![a.into(), b.into()], "P" => "AllOn"});
        let form = doc.add_object(Stream::new(
            dictionary! {"Type" => "XObject", "Subtype" => "Form", "BBox" => vec![0.into(), 0.into(), 10.into(), 10.into()], "OC" => b},
            b"(InForm) Tj".to_vec(),
        ));
        let content = doc.add_object(Stream::new(
            dictionary! {},
            b"BT /OC /A BDC (Visible) Tj EMC /OC /B BDC (Hidden) Tj /Span <<>> BDC (Inner) Tj EMC EMC \
              /Artifact BMC (Art) Tj EMC /OC /M BDC (Both) Tj EMC ET /X1 Do"
                .to_vec(),
        ));
        let rect = vec![0.into(), 0.into(), 10.into(), 10.into()];
        let hidden_note = doc.add_object(
            dictionary! {"Type" => "Annot", "Subtype" => "Square", "Rect" => rect.clone(), "OC" => b},
        );
        let note =
            doc.add_object(dictionary! {"Type" => "Annot", "Subtype" => "Square", "Rect" => rect, "OC" => a});
        let page = doc.add_object(dictionary! {"Type" => "Page", "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()], "Contents" => content,
            "Annots" => vec![hidden_note.into(), note.into()],
            "Resources" => dictionary! {"Properties" => dictionary! {"A" => a, "B" => b, "M" => both}, "XObject" => dictionary! {"X1" => form}}});
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {"Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1}),
        );
        let catalog = doc.add_object(dictionary! {"Type" => "Catalog", "Pages" => pages_id,
            "OCProperties" => dictionary! {"OCGs" => vec![a.into(), b.into()], "D" => dictionary! {"OFF" => vec![b.into()]}}});
        doc.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();

        let layer_options = |on: bool| OptimizeOptions { hidden_layers: on, ..OptimizeOptions::default() };
        let (out, _) = optimize(&bytes, None, &layer_options(true)).unwrap();
        let doc = load(&out, None).unwrap();
        let page = *doc.get_pages().values().next().unwrap();
        let text = String::from_utf8_lossy(&doc.get_page_content(page)).into_owned();
        for kept in ["Visible", "Art", "BMC", "EMC"] {
            assert!(text.contains(kept), "{kept} missing: {text}");
        }
        for gone in ["Hidden", "Inner", "Both", "Do", "/OC", "BDC"] {
            assert!(!text.contains(gone), "{gone} left: {text}");
        }
        let catalog = doc.get_dictionary(catalog_id(&doc).unwrap()).unwrap();
        assert!(catalog.get(b"OCProperties").is_err());
        let annots = doc.get_dictionary(page).unwrap().get(b"Annots").unwrap().as_array().unwrap();
        assert_eq!(annots.len(), 1);
        assert!(
            !doc.objects
                .values()
                .any(|o| o.as_dict().is_ok_and(|d| name_of(d, b"Type").as_deref() == Some(b"OCG")))
        );

        // Without the option the layers stay.
        let (out, _) = optimize(&bytes, None, &layer_options(false)).unwrap();
        let doc = load(&out, None).unwrap();
        let catalog = doc.get_dictionary(catalog_id(&doc).unwrap()).unwrap();
        assert!(catalog.get(b"OCProperties").is_ok());
    }

    #[test]
    fn resolution_is_measured() {
        let doc = load(&sample(false), None).unwrap();
        let res = image_resolutions(&doc);
        assert_eq!(res.len(), 2);
        assert!(res.values().all(|dpi| (dpi - 600.0).abs() < 0.5), "{res:?}");
    }

    #[test]
    fn images_fonts_and_data_are_optimized() {
        let bytes = sample(true);
        let options = OptimizeOptions {
            javascript: true,
            thumbnails: true,
            metadata: true,
            ..OptimizeOptions::default()
        };
        let (out, report) = optimize(&bytes, None, &options).unwrap();
        assert!(report.after < report.before / 10, "{report:?}");
        assert_eq!(report.images_changed, 2);
        assert_eq!(report.fonts_removed, 1, "Arial is a standard font");
        assert!(report.duplicates_merged >= 1);
        let doc = load(&out, None).unwrap();
        let images: Vec<&Stream> = doc
            .objects
            .values()
            .filter_map(|o| o.as_stream().ok())
            .filter(|s| name_of(&s.dict, b"Subtype").as_deref() == Some(b"Image"))
            .collect();
        assert_eq!(images.len(), 1, "duplicate image merged");
        let img = images[0];
        assert_eq!(img.dict.get(b"Width").unwrap().as_i64().unwrap(), 150, "600 dpi → 150 dpi");
        assert_eq!(name_of(&img.dict, b"Filter").as_deref(), Some(b"DCTDecode".as_slice()));
        assert!(doc.trailer.get(b"Info").is_err(), "document info removed");
        let catalog = doc.get_dictionary(catalog_id(&doc).unwrap()).unwrap();
        assert!(catalog.get(b"OpenAction").is_err(), "JavaScript removed");
        let page = doc.get_dictionary(*doc.get_pages().get(&1).unwrap()).unwrap();
        assert!(page.get(b"Thumb").is_err());
        let annots = page.get(b"Annots").ok().and_then(|a| a.as_array().ok()).map_or(0, Vec::len);
        assert_eq!(annots, 0, "link to a missing page removed");
    }

    #[test]
    fn options_keep_things() {
        let bytes = sample(false);
        let options = OptimizeOptions {
            images: false,
            fonts: false,
            cleanup: false,
            user_data: false,
            objects: false,
            version: Some("1.4".into()),
            ..OptimizeOptions::default()
        };
        let (out, report) = optimize(&bytes, None, &options).unwrap();
        assert_eq!(report.images_changed, 0);
        assert!(out.starts_with(b"%PDF-1.4"));
        let doc = load(&out, None).unwrap();
        assert!(doc.trailer.get(b"Info").is_ok());
    }

    #[test]
    fn fonts_and_usage() {
        assert!(
            is_standard_font("Arial-BoldMT")
                && is_standard_font("TimesNewRomanPSMT")
                && is_standard_font("Helvetica")
        );
        assert!(!is_standard_font("Calibri") && !is_standard_font("ArialNarrow"));
        let bytes = sample(false);
        let fonts = list_fonts(&bytes, None).unwrap();
        assert_eq!(fonts.len(), 1);
        assert_eq!(fonts[0].name, "Arial-BoldMT");
        assert!(fonts[0].subset && fonts[0].simple);
        let usage = space_usage(&bytes, None).unwrap();
        assert_eq!(usage.parts[0].0, UsageKind::Images, "{usage:?}");
        assert!(usage.parts.iter().map(|p| p.1).sum::<u64>() <= usage.total);
    }
}
