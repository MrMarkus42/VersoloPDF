//! Conversions: grayscale, image pages, plain text and Word (DOCX) export.

use std::collections::{HashMap, HashSet};

use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document, LoadOptions, Object, ObjectId, Stream};

use crate::error::{Error, Result};
use crate::pdf_edit::{MAX_DECOMPRESSED_STREAM, RgbaImage, guarded, image_xobject, rect_array};

// ---------------------------------------------------------------------------
// Grayscale

fn luminance(r: f32, g: f32, b: f32) -> f32 {
    (0.299 * r + 0.587 * g + 0.114 * b).clamp(0.0, 1.0)
}

fn cmyk_gray(c: f32, m: f32, y: f32, k: f32) -> f32 {
    1.0 - (0.3 * c + 0.59 * m + 0.11 * y + k).clamp(0.0, 1.0)
}

fn num(o: &Object) -> Option<f32> {
    match o {
        Object::Integer(i) => Some(*i as f32),
        Object::Real(r) => Some(*r),
        _ => None,
    }
}

/// Number of colour components a colour space converts from (3 = RGB,
/// 4 = CMYK), or `None` if it is left alone.
fn convertible_components(doc: &Document, cs: &Object) -> Option<usize> {
    let cs = match cs {
        Object::Reference(id) => doc.get_object(*id).ok()?,
        o => o,
    };
    match cs {
        Object::Name(n) => match n.as_slice() {
            b"DeviceRGB" | b"RGB" | b"CalRGB" => Some(3),
            b"DeviceCMYK" | b"CMYK" => Some(4),
            _ => None,
        },
        Object::Array(a) => {
            let kind = a.first()?.as_name().ok()?;
            match kind {
                b"CalRGB" => Some(3),
                b"ICCBased" => {
                    let stream = match a.get(1)? {
                        Object::Reference(id) => doc.get_object(*id).ok()?,
                        o => o,
                    };
                    let n = stream.as_stream().ok()?.dict.get(b"N").ok()?.as_i64().ok()?;
                    matches!(n, 3 | 4).then_some(n as usize)
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn to_gray(values: &[f32], comps: usize) -> f32 {
    match comps {
        3 => luminance(values[0], values[1], values[2]),
        4 => cmyk_gray(values[0], values[1], values[2], values[3]),
        _ => values.first().copied().unwrap_or(0.0),
    }
}

/// Rewrites colour operators of a content stream to gray.
fn gray_operations(ops: Vec<Operation>, names: &HashMap<Vec<u8>, usize>) -> Vec<Operation> {
    // Components of the current fill / stroke colour space (1 = already gray
    // or unknown -> left alone).
    let mut fill = 0usize;
    let mut stroke = 0usize;
    let gray_op = |op: &str, v: f32| Operation::new(op, vec![Object::Real(v)]);
    let mut out = Vec::with_capacity(ops.len());
    for op in ops {
        let vals: Vec<f32> = op.operands.iter().filter_map(num).collect();
        match op.operator.as_str() {
            "rg" if vals.len() == 3 => out.push(gray_op("g", to_gray(&vals, 3))),
            "RG" if vals.len() == 3 => out.push(gray_op("G", to_gray(&vals, 3))),
            "k" if vals.len() == 4 => out.push(gray_op("g", to_gray(&vals, 4))),
            "K" if vals.len() == 4 => out.push(gray_op("G", to_gray(&vals, 4))),
            "cs" | "CS" => {
                let comps = match op.operands.first() {
                    Some(Object::Name(n)) => match n.as_slice() {
                        b"DeviceRGB" => 3,
                        b"DeviceCMYK" => 4,
                        other => names.get(other).copied().unwrap_or(0),
                    },
                    _ => 0,
                };
                if op.operator == "cs" {
                    fill = comps;
                } else {
                    stroke = comps;
                }
                if comps == 0 {
                    out.push(op);
                } else {
                    let name = match op.operands.first() {
                        Some(Object::Name(n)) if n == b"DeviceRGB" || n == b"DeviceCMYK" => {
                            Object::Name(b"DeviceGray".to_vec())
                        }
                        Some(o) => o.clone(),
                        None => Object::Name(b"DeviceGray".to_vec()),
                    };
                    out.push(Operation::new(&op.operator, vec![name]));
                }
            }
            "sc" | "scn" if fill > 1 && vals.len() == fill && vals.len() == op.operands.len() => {
                out.push(Operation::new(&op.operator, vec![Object::Real(to_gray(&vals, fill))]))
            }
            "SC" | "SCN" if stroke > 1 && vals.len() == stroke && vals.len() == op.operands.len() => {
                out.push(Operation::new(&op.operator, vec![Object::Real(to_gray(&vals, stroke))]))
            }
            _ => out.push(op),
        }
    }
    out
}

/// Converts an image XObject to 8-bit gray where possible.
fn gray_image(doc: &Document, stream: &mut Stream) -> Result<()> {
    let dict = &stream.dict;
    if dict.get(b"ImageMask").ok().and_then(|m| m.as_bool().ok()) == Some(true) {
        return Ok(());
    }
    let Ok(cs) = dict.get(b"ColorSpace").cloned() else { return Ok(()) };
    let bpc = dict.get(b"BitsPerComponent").ok().and_then(|b| b.as_i64().ok()).unwrap_or(8);
    let width = dict.get(b"Width").ok().and_then(|w| w.as_i64().ok()).unwrap_or(0) as usize;
    let height = dict.get(b"Height").ok().and_then(|h| h.as_i64().ok()).unwrap_or(0) as usize;
    if width == 0 || height == 0 || width * height > 100_000_000 {
        return Ok(());
    }
    // Indexed images: only the palette changes.
    if let Some(gray_cs) = gray_indexed(doc, &cs) {
        stream.dict.set("ColorSpace", gray_cs);
        return Ok(());
    }
    let Some(comps) = convertible_components(doc, &cs) else { return Ok(()) };
    if bpc != 8 {
        return Ok(());
    }
    let filter = dict.get(b"Filter").ok().map(|f| match f {
        Object::Array(a) => a.iter().filter_map(|n| n.as_name().ok()).map(<[u8]>::to_vec).collect::<Vec<_>>(),
        Object::Name(n) => vec![n.clone()],
        _ => Vec::new(),
    });
    let filters = filter.unwrap_or_default();
    let (pixels, jpeg) = match filters.as_slice() {
        [] | [_] if filters.iter().all(|f| f == b"FlateDecode") => {
            let data =
                if filters.is_empty() { stream.content.clone() } else { stream.decompressed_content()? };
            (data, false)
        }
        [f] if f == b"DCTDecode" => {
            let img = image::load_from_memory_with_format(&stream.content, image::ImageFormat::Jpeg)
                .map_err(|e| Error::Invalid(e.to_string()))?;
            // CMYK JPEGs are delivered as RGB by the decoder.
            let rgb = img.to_rgb8();
            if rgb.width() as usize != width || rgb.height() as usize != height {
                return Ok(());
            }
            (rgb.into_raw(), true)
        }
        _ => return Ok(()),
    };
    let comps = if jpeg { 3 } else { comps };
    if pixels.len() < width * height * comps {
        return Ok(());
    }
    let gray: Vec<u8> = pixels
        .as_slice()
        .chunks_exact(comps)
        .take(width * height)
        .map(|p| {
            let v: Vec<f32> = p.iter().map(|b| f32::from(*b) / 255.0).collect();
            let v = if comps == 4 && !jpeg { to_gray(&v, 4) } else { to_gray(&v, comps) };
            (v * 255.0).round() as u8
        })
        .collect();
    stream.dict.remove(b"DecodeParms");
    stream.dict.remove(b"Decode");
    stream.dict.set("ColorSpace", Object::Name(b"DeviceGray".to_vec()));
    if jpeg {
        let mut out = Vec::new();
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85);
        image::ImageEncoder::write_image(
            encoder,
            &gray,
            width as u32,
            height as u32,
            image::ExtendedColorType::L8,
        )
        .map_err(|e| Error::Invalid(e.to_string()))?;
        stream.dict.set("Filter", Object::Name(b"DCTDecode".to_vec()));
        stream.set_content(out);
    } else {
        stream.dict.remove(b"Filter");
        stream.set_plain_content(gray);
        stream.compress()?;
    }
    Ok(())
}

/// `[/Indexed base hival lookup]` with an RGB/CMYK base -> gray palette.
fn gray_indexed(doc: &Document, cs: &Object) -> Option<Object> {
    let a = match cs {
        Object::Reference(id) => doc.get_object(*id).ok()?.as_array().ok()?,
        Object::Array(a) => a,
        _ => return None,
    };
    if a.first()?.as_name().ok()? != b"Indexed" || a.len() != 4 {
        return None;
    }
    let comps = convertible_components(doc, a.get(1)?)?;
    let hival = a.get(2)?.as_i64().ok()?;
    let lookup = match a.get(3)? {
        Object::String(s, _) => s.clone(),
        Object::Reference(id) => match doc.get_object(*id).ok()? {
            Object::Stream(s) => s.decompressed_content().unwrap_or_else(|_| s.content.clone()),
            Object::String(s, _) => s.clone(),
            _ => return None,
        },
        _ => return None,
    };
    let gray: Vec<u8> = lookup
        .chunks_exact(comps)
        .take(hival as usize + 1)
        .map(|p| {
            let v: Vec<f32> = p.iter().map(|b| f32::from(*b) / 255.0).collect();
            (to_gray(&v, comps) * 255.0).round() as u8
        })
        .collect();
    Some(Object::Array(vec![
        Object::Name(b"Indexed".to_vec()),
        Object::Name(b"DeviceGray".to_vec()),
        Object::Integer(hival),
        Object::String(gray, lopdf::StringFormat::Hexadecimal),
    ]))
}

fn gray_color_array(o: &mut Object) {
    if let Object::Array(a) = o {
        let vals: Vec<f32> = a.iter().filter_map(num).collect();
        if vals.len() == a.len() && matches!(vals.len(), 3 | 4) {
            *a = vec![Object::Real(to_gray(&vals, vals.len()))];
        }
    }
}

/// Converts the document to grayscale: colour operators, colour spaces,
/// images and annotation colours. Shadings keep their colours. The file is
/// rewritten completely.
pub fn to_grayscale(bytes: &[u8], password: Option<&str>) -> Result<Vec<u8>> {
    guarded(|| {
        let mut options = match password {
            Some(pw) => LoadOptions::with_password(pw),
            None => LoadOptions::default(),
        };
        options.max_decompressed_size = Some(MAX_DECOMPRESSED_STREAM);
        let mut doc = Document::load_mem_with_options(bytes, options)?;
        if doc.is_encrypted() {
            return Err(Error::EncryptedNotSupported);
        }

        // Named colour spaces in resources: name -> components. Names used
        // with different meanings are left alone.
        let mut names: HashMap<Vec<u8>, usize> = HashMap::new();
        let mut ambiguous: HashSet<Vec<u8>> = HashSet::new();
        let mut cs_dicts: Vec<ObjectId> = Vec::new();
        for (id, obj) in &doc.objects {
            let dict = match obj {
                Object::Dictionary(d) => d,
                Object::Stream(s) => &s.dict,
                _ => continue,
            };
            let Ok(res) = dict.get(b"ColorSpace") else { continue };
            let res = match res {
                Object::Reference(r) => match doc.get_object(*r) {
                    Ok(Object::Dictionary(d)) => {
                        cs_dicts.push(*r);
                        d
                    }
                    _ => continue,
                },
                Object::Dictionary(d) => {
                    // Image dictionaries have a colour space, not a resource dictionary.
                    if dict.get(b"Subtype").ok().and_then(|s| s.as_name().ok()) == Some(b"Image") {
                        continue;
                    }
                    cs_dicts.push(*id);
                    d
                }
                _ => continue,
            };
            for (name, cs) in res.iter() {
                let comps = convertible_components(&doc, cs).unwrap_or(0);
                match names.get(name) {
                    Some(c) if *c != comps => {
                        ambiguous.insert(name.clone());
                    }
                    _ => {
                        names.insert(name.clone(), comps);
                    }
                }
            }
        }
        names.retain(|n, c| *c > 0 && !ambiguous.contains(n));
        // Point those resource entries to DeviceGray.
        for id in cs_dicts {
            let Ok(obj) = doc.get_object_mut(id) else { continue };
            let res = match obj {
                Object::Dictionary(d) => match d.get_mut(b"ColorSpace") {
                    Ok(Object::Dictionary(r)) => r,
                    _ => d,
                },
                Object::Stream(s) => match s.dict.get_mut(b"ColorSpace") {
                    Ok(Object::Dictionary(r)) => r,
                    _ => continue,
                },
                _ => continue,
            };
            let keys: Vec<Vec<u8>> =
                res.iter().map(|(k, _)| k.clone()).filter(|k| names.contains_key(k)).collect();
            for k in keys {
                res.set(k, Object::Name(b"DeviceGray".to_vec()));
            }
        }

        // Content streams: page contents and form XObjects.
        let mut content_ids: HashSet<ObjectId> = HashSet::new();
        for page_id in doc.get_pages().values() {
            if let Ok(page) = doc.get_dictionary(*page_id) {
                match page.get(b"Contents") {
                    Ok(Object::Reference(r)) => match doc.get_object(*r) {
                        Ok(Object::Array(a)) => {
                            content_ids.extend(a.iter().filter_map(|o| o.as_reference().ok()))
                        }
                        _ => {
                            content_ids.insert(*r);
                        }
                    },
                    Ok(Object::Array(a)) => {
                        content_ids.extend(a.iter().filter_map(|o| o.as_reference().ok()))
                    }
                    _ => {}
                }
            }
        }
        let ids: Vec<ObjectId> = doc.objects.keys().copied().collect();
        for id in ids {
            let snapshot = doc.get_object(id)?.clone();
            match snapshot {
                Object::Stream(mut s) => {
                    let subtype =
                        s.dict.get(b"Subtype").ok().and_then(|n| n.as_name().ok()).map(<[u8]>::to_vec);
                    if subtype.as_deref() == Some(b"Image") {
                        gray_image(&doc, &mut s)?;
                    } else if subtype.as_deref() == Some(b"Form") || content_ids.contains(&id) {
                        let data = s.decompressed_content().unwrap_or_else(|_| s.content.clone());
                        let Ok(content) = Content::decode(&data) else { continue };
                        let ops = gray_operations(content.operations, &names);
                        let encoded = Content { operations: ops }.encode()?;
                        s.dict.remove(b"Filter");
                        s.dict.remove(b"DecodeParms");
                        s.set_plain_content(encoded);
                        s.compress()?;
                    } else {
                        continue;
                    }
                    doc.objects.insert(id, Object::Stream(s));
                }
                Object::Dictionary(mut d)
                    if d.get(b"Type").ok().and_then(|t| t.as_name().ok()) == Some(b"Annot") =>
                {
                    for key in [&b"C"[..], b"IC"] {
                        if let Ok(o) = d.get_mut(key) {
                            gray_color_array(o);
                        }
                    }
                    doc.objects.insert(id, Object::Dictionary(d));
                }
                _ => {}
            }
        }
        doc.trailer.remove(b"Prev");
        doc.trailer.remove(b"XRefStm");
        let mut out = Vec::new();
        doc.save_to(&mut out).map_err(|e| Error::Structure(e.to_string()))?;
        Ok(out)
    })
}

// ---------------------------------------------------------------------------
// Recolour ("Seiten neu einfärben")

/// Colour for a gray value: dark parts take `text`, light parts `background`.
fn tint(v: f32, background: [u8; 3], text: [u8; 3]) -> [f32; 3] {
    let v = v.clamp(0.0, 1.0);
    let c = |i: usize| (f32::from(text[i]) + (f32::from(background[i]) - f32::from(text[i])) * v) / 255.0;
    [c(0), c(1), c(2)]
}

fn rgb_op(op: &str, c: [f32; 3]) -> Operation {
    Operation::new(op, c.iter().map(|v| Object::Real((v * 1000.0).round() / 1000.0)).collect())
}

/// Rewrites the gray colour operators of a (grayscale) content stream.
fn tint_operations(
    ops: Vec<Operation>,
    gray_names: &HashSet<Vec<u8>>,
    bg: [u8; 3],
    fg: [u8; 3],
) -> Vec<Operation> {
    let (mut fill_gray, mut stroke_gray) = (false, false);
    let mut out = Vec::with_capacity(ops.len());
    for op in ops {
        let vals: Vec<f32> = op.operands.iter().filter_map(num).collect();
        match op.operator.as_str() {
            "g" if vals.len() == 1 => out.push(rgb_op("rg", tint(vals[0], bg, fg))),
            "G" if vals.len() == 1 => out.push(rgb_op("RG", tint(vals[0], bg, fg))),
            "cs" | "CS" => {
                let gray = match op.operands.first() {
                    Some(Object::Name(n)) => n == b"DeviceGray" || gray_names.contains(n),
                    _ => false,
                };
                if op.operator == "cs" {
                    fill_gray = gray;
                } else {
                    stroke_gray = gray;
                }
                if gray {
                    out.push(Operation::new(&op.operator, vec![Object::Name(b"DeviceRGB".to_vec())]));
                } else {
                    out.push(op);
                }
            }
            "sc" | "scn" if fill_gray && vals.len() == 1 && op.operands.len() == 1 => {
                out.push(rgb_op(&op.operator, tint(vals[0], bg, fg)))
            }
            "SC" | "SCN" if stroke_gray && vals.len() == 1 && op.operands.len() == 1 => {
                out.push(rgb_op(&op.operator, tint(vals[0], bg, fg)))
            }
            _ => out.push(op),
        }
    }
    out
}

/// "Seiten neu einfärben": the document is made gray first, then every gray
/// value becomes a colour between `text` (black) and `background`
/// (white), the paper included. Images are recoloured through a palette.
pub fn recolor(bytes: &[u8], password: Option<&str>, background: [u8; 3], text: [u8; 3]) -> Result<Vec<u8>> {
    let gray = to_grayscale(bytes, password)?;
    guarded(|| {
        let options =
            LoadOptions { max_decompressed_size: Some(MAX_DECOMPRESSED_STREAM), ..LoadOptions::default() };
        let mut doc = Document::load_mem_with_options(&gray, options)?;
        let palette: Vec<u8> = (0..=255u8)
            .flat_map(|v| tint(f32::from(v) / 255.0, background, text).map(|c| (c * 255.0).round() as u8))
            .collect();
        // Named colour spaces that are gray now become RGB.
        let mut gray_names: HashSet<Vec<u8>> = HashSet::new();
        let ids: Vec<ObjectId> = doc.objects.keys().copied().collect();
        for id in &ids {
            let Ok(obj) = doc.get_object_mut(*id) else { continue };
            let dict = match obj {
                Object::Dictionary(d) => d,
                Object::Stream(s) => &mut s.dict,
                _ => continue,
            };
            if dict.get(b"Subtype").ok().and_then(|s| s.as_name().ok()) == Some(b"Image") {
                continue;
            }
            if let Ok(Object::Dictionary(res)) = dict.get_mut(b"ColorSpace") {
                let keys: Vec<Vec<u8>> = res
                    .iter()
                    .filter(|(_, v)| v.as_name().ok() == Some(b"DeviceGray"))
                    .map(|(k, _)| k.clone())
                    .collect();
                for k in keys {
                    res.set(k.clone(), Object::Name(b"DeviceRGB".to_vec()));
                    gray_names.insert(k);
                }
            }
        }
        let mut content_ids: HashSet<ObjectId> = HashSet::new();
        let pages: Vec<ObjectId> = doc.get_pages().values().copied().collect();
        for page_id in &pages {
            if let Ok(page) = doc.get_dictionary(*page_id) {
                match page.get(b"Contents") {
                    Ok(Object::Reference(r)) => match doc.get_object(*r) {
                        Ok(Object::Array(a)) => {
                            content_ids.extend(a.iter().filter_map(|o| o.as_reference().ok()))
                        }
                        _ => {
                            content_ids.insert(*r);
                        }
                    },
                    Ok(Object::Array(a)) => {
                        content_ids.extend(a.iter().filter_map(|o| o.as_reference().ok()))
                    }
                    _ => {}
                }
            }
        }
        let gray_cs = Object::Name(b"DeviceGray".to_vec());
        // Soft masks stay gray (they are transparency, not colour).
        let smasks: HashSet<ObjectId> = doc
            .objects
            .values()
            .filter_map(|o| match o {
                Object::Stream(s) => s.dict.get(b"SMask").ok().and_then(|r| r.as_reference().ok()),
                _ => None,
            })
            .collect();
        for id in ids {
            let snapshot = doc.get_object(id)?.clone();
            match snapshot {
                Object::Stream(mut s) => {
                    let subtype =
                        s.dict.get(b"Subtype").ok().and_then(|n| n.as_name().ok()).map(<[u8]>::to_vec);
                    if subtype.as_deref() == Some(b"Image") {
                        let mask = s.dict.get(b"ImageMask").ok().and_then(|m| m.as_bool().ok()) == Some(true);
                        let bpc = s.dict.get(b"BitsPerComponent").ok().and_then(|b| b.as_i64().ok());
                        let is_smask = smasks.contains(&id);
                        if !mask
                            && !is_smask
                            && bpc == Some(8)
                            && s.dict.get(b"ColorSpace").ok() == Some(&gray_cs)
                        {
                            s.dict.set(
                                "ColorSpace",
                                Object::Array(vec![
                                    Object::Name(b"Indexed".to_vec()),
                                    Object::Name(b"DeviceRGB".to_vec()),
                                    Object::Integer(255),
                                    Object::String(palette.clone(), lopdf::StringFormat::Hexadecimal),
                                ]),
                            );
                            s.dict.remove(b"Decode");
                        }
                    } else if subtype.as_deref() == Some(b"Form") || content_ids.contains(&id) {
                        let data = s.decompressed_content().unwrap_or_else(|_| s.content.clone());
                        let Ok(content) = Content::decode(&data) else { continue };
                        let ops = tint_operations(content.operations, &gray_names, background, text);
                        let encoded = Content { operations: ops }.encode()?;
                        s.dict.remove(b"Filter");
                        s.dict.remove(b"DecodeParms");
                        s.set_plain_content(encoded);
                        s.compress()?;
                    } else {
                        continue;
                    }
                    doc.objects.insert(id, Object::Stream(s));
                }
                Object::Dictionary(mut d)
                    if d.get(b"Type").ok().and_then(|t| t.as_name().ok()) == Some(b"Annot") =>
                {
                    for key in [&b"C"[..], b"IC"] {
                        if let Ok(Object::Array(a)) = d.get_mut(key)
                            && let [v] = a.as_slice()
                            && let Some(v) = num(v)
                        {
                            *a = tint(v, background, text).iter().map(|c| Object::Real(*c)).collect();
                        }
                    }
                    doc.objects.insert(id, Object::Dictionary(d));
                }
                _ => {}
            }
        }
        // The paper gets the background colour; text without a colour of
        // its own (default black) gets the text colour.
        let [br, bgc, bb] = background.map(|c| f32::from(c) / 255.0);
        let [tr, tg, tb] = text.map(|c| f32::from(c) / 255.0);
        for page_id in pages {
            let media = doc
                .get_dictionary(page_id)
                .ok()
                .and_then(|p| p.get(b"MediaBox").ok().cloned())
                .and_then(|m| m.as_array().ok().cloned())
                .map(|a| a.iter().filter_map(num).collect::<Vec<f32>>())
                .filter(|v| v.len() == 4)
                .unwrap_or_else(|| vec![-10_000.0, -10_000.0, 10_000.0, 10_000.0]);
            let prefix = format!(
                "q {br:.3} {bgc:.3} {bb:.3} rg {} {} {} {} re f Q {tr:.3} {tg:.3} {tb:.3} rg {tr:.3} {tg:.3} {tb:.3} RG\n",
                media[0],
                media[1],
                media[2] - media[0],
                media[3] - media[1]
            );
            let stream_id = doc.add_object(Stream::new(Dictionary::new(), prefix.into_bytes()));
            let current = doc.get_dictionary(page_id).ok().and_then(|p| p.get(b"Contents").ok().cloned());
            let mut list = match current {
                Some(Object::Array(a)) => a,
                Some(Object::Reference(r)) => match doc.get_object(r) {
                    Ok(Object::Array(a)) => a.clone(),
                    _ => vec![Object::Reference(r)],
                },
                _ => Vec::new(),
            };
            list.insert(0, Object::Reference(stream_id));
            let contents = list;
            let Ok(page) = doc.get_dictionary_mut(page_id) else { continue };
            page.set("Contents", Object::Array(contents));
        }
        let mut out = Vec::new();
        doc.save_to(&mut out).map_err(|e| Error::Structure(e.to_string()))?;
        Ok(out)
    })
}

// ---------------------------------------------------------------------------
// Image pages

/// A one-page PDF showing `image` at `dpi` (page size follows the image).
pub fn image_page_pdf(image: &RgbaImage, dpi: f32) -> Result<Vec<u8>> {
    let (w, h) = (image.width, image.height);
    if w == 0 || h == 0 || image.rgba.len() != (w as usize) * (h as usize) * 4 {
        return Err(Error::Invalid("invalid image".into()));
    }
    let scale = 72.0 / dpi.clamp(36.0, 1200.0);
    let (pw, ph) = (w as f32 * scale, h as f32 * scale);
    let mut doc = Document::with_version("1.7");
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    let mut alpha = Vec::with_capacity((w * h) as usize);
    for px in image.rgba.as_chunks::<4>().0 {
        rgb.extend_from_slice(&px[..3]);
        alpha.push(px[3]);
    }
    let smask = if alpha.iter().any(|&a| a != 255) {
        Some(doc.add_object(image_xobject(alpha, w, h, true, None)?))
    } else {
        None
    };
    let image_id = doc.add_object(image_xobject(rgb, w, h, false, smask)?);
    let content =
        format!("q {} 0 0 {} 0 0 cm /Im0 Do Q\n", crate::appearance::num(pw), crate::appearance::num(ph));
    let content_id = doc.add_object(Stream::new(Dictionary::new(), content.into_bytes()));
    let pages_id = doc.new_object_id();
    let mut xobjects = Dictionary::new();
    xobjects.set("Im0", Object::Reference(image_id));
    let mut resources = Dictionary::new();
    resources.set("XObject", Object::Dictionary(xobjects));
    let mut page = Dictionary::new();
    page.set("Type", Object::Name(b"Page".to_vec()));
    page.set("Parent", Object::Reference(pages_id));
    page.set("MediaBox", rect_array(&crate::geometry::RectF::new(0.0, 0.0, pw, ph)));
    page.set("Resources", Object::Dictionary(resources));
    page.set("Contents", Object::Reference(content_id));
    let page_id = doc.add_object(page);
    let mut pages = Dictionary::new();
    pages.set("Type", Object::Name(b"Pages".to_vec()));
    pages.set("Count", Object::Integer(1));
    pages.set("Kids", Object::Array(vec![Object::Reference(page_id)]));
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

// ---------------------------------------------------------------------------
// Text and Word export

/// Plain text of all pages, separated by form feeds.
pub fn text_export(pages: &[String]) -> String {
    let mut out = String::new();
    for (i, page) in pages.iter().enumerate() {
        if i > 0 {
            out.push('\u{c}');
            out.push('\n');
        }
        out.push_str(page.trim_end());
        out.push('\n');
    }
    out
}

pub(crate) fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            // Characters not allowed in XML 1.0.
            c if (c as u32) < 0x20 && c != '\t' => {}
            c => out.push(c),
        }
    }
    out
}

/// Content of a Word document page.
#[derive(Debug, Clone, PartialEq)]
pub enum DocBlock {
    /// A paragraph; true for a heading.
    Text(String, bool),
    /// A PNG picture and its size in points.
    Image(Vec<u8>, f32, f32),
    /// A table: rows of cells.
    Table(Vec<Vec<String>>),
}

/// Widest picture: the text width of an A4 page with 2 cm margins.
const DOCX_TEXT_WIDTH: f32 = 481.0;

/// A table with thin borders, columns as wide as their longest text.
fn docx_table(rows: &[Vec<String>]) -> String {
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    if columns == 0 {
        return String::new();
    }
    let longest: Vec<usize> = (0..columns)
        .map(|c| rows.iter().filter_map(|r| r.get(c)).map(|t| t.chars().count()).max().unwrap_or(1).max(3))
        .collect();
    let total: usize = longest.iter().sum();
    let widths: Vec<u32> = longest.iter().map(|l| (9638 * *l / total.max(1)) as u32).collect();
    let border = "w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"999999\"";
    let mut out = format!(
        "<w:tbl><w:tblPr><w:tblW w:w=\"0\" w:type=\"auto\"/><w:tblBorders><w:top {border}/><w:left {border}/><w:bottom {border}/><w:right {border}/><w:insideH {border}/><w:insideV {border}/></w:tblBorders><w:tblLayout w:type=\"fixed\"/></w:tblPr><w:tblGrid>"
    );
    for w in &widths {
        out.push_str(&format!("<w:gridCol w:w=\"{w}\"/>"));
    }
    out.push_str("</w:tblGrid>");
    for row in rows {
        out.push_str("<w:tr>");
        for (c, w) in widths.iter().enumerate() {
            let text = row.get(c).map_or("", String::as_str);
            out.push_str(&format!(
                "<w:tc><w:tcPr><w:tcW w:w=\"{w}\" w:type=\"dxa\"/></w:tcPr><w:p><w:r><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p></w:tc>",
                xml_escape(text)
            ));
        }
        out.push_str("</w:tr>");
    }
    out.push_str("</w:tbl><w:p/>");
    out
}

/// Drawing of picture `n` (relationship `rIdImg{n}`), `cx` × `cy` EMU.
pub(crate) fn docx_picture(n: usize, cx: i64, cy: i64) -> String {
    format!(
        "<a:graphic xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:pic xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:nvPicPr><pic:cNvPr id=\"{n}\" name=\"Bild {n}\"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed=\"rIdImg{n}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic>"
    )
}

/// A minimal Word document: one paragraph per text line, pictures and
/// tables in between, a page break between PDF pages.
pub fn docx(title: &str, pages: &[Vec<DocBlock>]) -> Vec<u8> {
    docx_protected(title, pages, None)
}

/// As [`docx`]; with `password` the document can only be changed with it.
pub fn docx_protected(title: &str, pages: &[Vec<DocBlock>], password: Option<&str>) -> Vec<u8> {
    let mut body = String::new();
    let mut images: Vec<&[u8]> = Vec::new();
    for (i, page) in pages.iter().enumerate() {
        if i > 0 {
            body.push_str("<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>");
        }
        for block in page {
            match block {
                DocBlock::Text(line, heading) => {
                    body.push_str("<w:p>");
                    if *heading {
                        body.push_str("<w:pPr><w:pStyle w:val=\"Heading1\"/></w:pPr>");
                    }
                    body.push_str("<w:r><w:t xml:space=\"preserve\">");
                    body.push_str(&xml_escape(line));
                    body.push_str("</w:t></w:r></w:p>");
                }
                DocBlock::Image(png, w, h) => {
                    images.push(png);
                    let n = images.len();
                    let scale = (DOCX_TEXT_WIDTH / w.max(1.0)).min(1.0);
                    let cx = (w * scale * 12_700.0).round().max(12_700.0) as i64;
                    let cy = (h * scale * 12_700.0).round().max(12_700.0) as i64;
                    body.push_str(&format!(
                        "<w:p><w:r><w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\"><wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:docPr id=\"{n}\" name=\"Bild {n}\"/>{}</wp:inline></w:drawing></w:r></w:p>",
                        docx_picture(n, cx, cy)
                    ));
                }
                DocBlock::Table(rows) => body.push_str(&docx_table(rows)),
            }
        }
    }
    body.push_str("<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:pgMar w:top=\"1134\" w:right=\"1134\" w:bottom=\"1134\" w:left=\"1134\" w:header=\"709\" w:footer=\"709\" w:gutter=\"0\"/></w:sectPr>");
    docx_package(title, &body, &images, password)
}

/// The Word file around `body` (incl. its final `w:sectPr`) with the
/// pictures `rIdImg1…` and optional write protection.
pub(crate) fn docx_package(title: &str, body: &str, images: &[&[u8]], password: Option<&str>) -> Vec<u8> {
    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\"><w:body>{body}</w:body></w:document>"
    );
    let protection = password.filter(|p| !p.is_empty()).map(crate::office::word_write_protection);
    let settings = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:settings xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">{}</w:settings>",
        protection.unwrap_or_default()
    );
    let styles = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii=\"Calibri\" w:hAnsi=\"Calibri\" w:cs=\"Calibri\"/><w:sz w:val=\"22\"/></w:rPr></w:rPrDefault></w:docDefaults><w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/></w:style><w:style w:type=\"paragraph\" w:styleId=\"Heading1\"><w:name w:val=\"heading 1\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:pPr><w:spacing w:before=\"240\" w:after=\"120\"/><w:outlineLvl w:val=\"0\"/></w:pPr><w:rPr><w:b/><w:sz w:val=\"32\"/></w:rPr></w:style></w:styles>";
    let content_types = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Default Extension=\"png\" ContentType=\"image/png\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/><Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/><Override PartName=\"/word/settings.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml\"/><Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/></Types>";
    let rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/></Relationships>";
    let mut doc_rels = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/><Relationship Id=\"rIdSettings\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings\" Target=\"settings.xml\"/>",
    );
    for n in 1..=images.len() {
        doc_rels.push_str(&format!(
            "<Relationship Id=\"rIdImg{n}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"media/image{n}.png\"/>"
        ));
    }
    doc_rels.push_str("</Relationships>");
    let core = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:title>{}</dc:title><dc:creator>VersoloPDF</dc:creator></cp:coreProperties>",
        xml_escape(title)
    );
    let mut zip = ZipWriter::default();
    zip.add("[Content_Types].xml", content_types.as_bytes());
    zip.add("_rels/.rels", rels.as_bytes());
    zip.add("docProps/core.xml", core.as_bytes());
    zip.add("word/_rels/document.xml.rels", doc_rels.as_bytes());
    zip.add("word/styles.xml", styles.as_bytes());
    zip.add("word/settings.xml", settings.as_bytes());
    zip.add("word/document.xml", document.as_bytes());
    for (i, png) in images.iter().enumerate() {
        zip.add(&format!("word/media/image{}.png", i + 1), png);
    }
    zip.finish()
}

/// CRC-32 (IEEE) as used by ZIP.
pub fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, entry) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *entry = c;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = table[((crc ^ u32::from(b)) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

/// A ZIP archive with uncompressed ("stored") entries.
#[derive(Default)]
pub(crate) struct ZipWriter {
    data: Vec<u8>,
    central: Vec<u8>,
    count: u16,
}

impl ZipWriter {
    pub(crate) fn add(&mut self, name: &str, content: &[u8]) {
        let crc = crc32(content);
        let offset = self.data.len() as u32;
        let size = content.len() as u32;
        let name = name.as_bytes();
        // Local file header (version 2.0, no flags, stored, DOS date 1980-01-01).
        let mut header = Vec::new();
        header.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        header.extend_from_slice(&20u16.to_le_bytes());
        header.extend_from_slice(&0u16.to_le_bytes());
        header.extend_from_slice(&0u16.to_le_bytes());
        header.extend_from_slice(&0u16.to_le_bytes());
        header.extend_from_slice(&0x21u16.to_le_bytes());
        header.extend_from_slice(&crc.to_le_bytes());
        header.extend_from_slice(&size.to_le_bytes());
        header.extend_from_slice(&size.to_le_bytes());
        header.extend_from_slice(&(name.len() as u16).to_le_bytes());
        header.extend_from_slice(&0u16.to_le_bytes());
        self.data.extend_from_slice(&header);
        self.data.extend_from_slice(name);
        self.data.extend_from_slice(content);
        // Central directory entry.
        self.central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        self.central.extend_from_slice(&20u16.to_le_bytes());
        self.central.extend_from_slice(&header[4..]);
        self.central.extend_from_slice(&0u16.to_le_bytes()); // comment
        self.central.extend_from_slice(&0u16.to_le_bytes()); // disk
        self.central.extend_from_slice(&0u16.to_le_bytes()); // internal attributes
        self.central.extend_from_slice(&0u32.to_le_bytes()); // external attributes
        self.central.extend_from_slice(&offset.to_le_bytes());
        self.central.extend_from_slice(name);
        self.count += 1;
    }

    pub(crate) fn finish(mut self) -> Vec<u8> {
        let offset = self.data.len() as u32;
        let size = self.central.len() as u32;
        self.data.extend_from_slice(&self.central);
        self.data.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        self.data.extend_from_slice(&0u16.to_le_bytes());
        self.data.extend_from_slice(&0u16.to_le_bytes());
        self.data.extend_from_slice(&self.count.to_le_bytes());
        self.data.extend_from_slice(&self.count.to_le_bytes());
        self.data.extend_from_slice(&size.to_le_bytes());
        self.data.extend_from_slice(&offset.to_le_bytes());
        self.data.extend_from_slice(&0u16.to_le_bytes());
        self.data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_matches_reference() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn docx_is_a_zip_with_the_text() {
        let bytes = docx(
            "T",
            &[
                vec![DocBlock::Text("Überschrift".into(), true), DocBlock::Text("a < b & c".into(), false)],
                vec![DocBlock::Text("Seite 2".into(), false)],
            ],
        );
        assert_eq!(&bytes[..4], b"PK\x03\x04");
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("a &lt; b &amp; c"));
        assert!(text.contains("w:type=\"page\""));
        assert!(text.contains("word/document.xml"));
        // End of central directory with six entries.
        let eocd = bytes.len() - 22;
        assert_eq!(&bytes[eocd..eocd + 4], &0x0605_4b50u32.to_le_bytes());
        assert_eq!(u16::from_le_bytes([bytes[eocd + 10], bytes[eocd + 11]]), 7);
    }

    #[test]
    fn colour_operators_become_gray() {
        let ops = vec![
            Operation::new("rg", vec![Object::Real(1.0), Object::Real(0.0), Object::Real(0.0)]),
            Operation::new("cs", vec![Object::Name(b"CS0".to_vec())]),
            Operation::new("scn", vec![Object::Real(0.0), Object::Real(0.0), Object::Real(1.0)]),
            Operation::new("K", vec![0.into(), 0.into(), 0.into(), 1.into()]),
        ];
        let names = HashMap::from([(b"CS0".to_vec(), 3usize)]);
        let out = gray_operations(ops, &names);
        assert_eq!(out[0].operator, "g");
        assert!((num(&out[0].operands[0]).unwrap() - 0.299).abs() < 1e-3);
        assert_eq!(out[2].operands.len(), 1);
        assert_eq!(out[3].operator, "G");
        assert_eq!(num(&out[3].operands[0]), Some(0.0));
    }
}
