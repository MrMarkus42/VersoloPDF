//! Signing a PDF (appended as an incremental update, so earlier signatures
//! stay valid) and checking the signatures of a PDF.
//!
//! The signature dictionary is written with placeholders for /ByteRange
//! and /Contents; after saving, the byte ranges are filled in, their
//! SHA-256 digest is signed (CMS, PAdES baseline: ETSI.CAdES.detached)
//! and the CMS goes into /Contents.

use lopdf::{Dictionary, IncrementalDocument, Object, ObjectId, Stream, StringFormat};

use super::cms;
use super::id::DigitalId;
use crate::geometry::RectF;
use crate::{Error, Result};

/// Room for the CMS in bytes (certificate chains of RSA 4096 fit).
const CONTENTS_BYTES: usize = 24_000;
/// Placeholder numbers of /ByteRange (replaced after saving).
const RANGE_MARK: &[u8] = b"1111111111 2222222222 3333333333";

fn bad(what: impl Into<String>) -> Error {
    Error::Invalid(what.into())
}

/// Where the signature goes.
#[derive(Debug, Clone, PartialEq)]
pub enum Place {
    /// No visible appearance.
    Invisible,
    /// An existing empty signature field (by its full name).
    Field(String),
    /// A new field on `page` (0-based) at `rect` (PDF space).
    Rect { page: usize, rect: RectF },
}

#[derive(Debug, Clone, PartialEq)]
pub struct SignOptions {
    pub place: Place,
    pub reason: String,
    pub location: String,
    pub contact: String,
}

/// PDF date "D:YYYYMMDDHHmmSS+hh'mm'" for `unix` in the local offset
/// `offset_minutes`.
pub fn pdf_date(unix: i64, offset_minutes: i32) -> String {
    let (y, mo, d, h, mi, s) = super::cert::civil(unix + i64::from(offset_minutes) * 60);
    let sign = if offset_minutes < 0 { '-' } else { '+' };
    let o = offset_minutes.unsigned_abs();
    format!("D:{y:04}{mo:02}{d:02}{h:02}{mi:02}{s:02}{sign}{:02}'{:02}'", o / 60, o % 60)
}

/// Minutes the local time is ahead of UTC (now).
#[cfg_attr(windows, allow(unsafe_code))]
pub fn local_offset_minutes() -> i32 {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
        // SAFETY: plain struct filled by the call.
        let mut info: TIME_ZONE_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: valid pointer to a TIME_ZONE_INFORMATION.
        let id = unsafe { GetTimeZoneInformation(&mut info) };
        let extra = if id == 2 { info.DaylightBias } else { info.StandardBias };
        if id == u32::MAX { 0 } else { -(info.Bias + extra) }
    }
    #[cfg(not(windows))]
    {
        // "+0100" from date(1); UTC when not available.
        std::process::Command::new("date")
            .arg("+%z")
            .output()
            .ok()
            .and_then(|o| {
                let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                let sign = if s.starts_with('-') { -1 } else { 1 };
                let digits = s.trim_start_matches(['+', '-']);
                let h: i32 = digits.get(0..2)?.parse().ok()?;
                let m: i32 = digits.get(2..4)?.parse().ok()?;
                Some(sign * (h * 60 + m))
            })
            .unwrap_or(0)
    }
}

/// "D:20261104193000+01'00'" as "04.11.2026 19:30:00".
pub fn readable_date(d: &str) -> String {
    let s = d.trim_start_matches("D:");
    let n = |a: usize, b: usize| s.get(a..b).unwrap_or("00");
    if s.len() < 8 {
        return d.to_string();
    }
    format!("{}.{}.{} {}:{}:{}", n(6, 8), n(4, 6), n(0, 4), n(8, 10), n(10, 12), n(12, 14))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

/// The visible signature: name, date, reason and place in lines.
fn appearance(width: f32, height: f32, lines: &[String]) -> Stream {
    let n = lines.len().max(1) as f32;
    let longest = lines.iter().map(|l| l.chars().count()).max().unwrap_or(1).max(1) as f32;
    // Helvetica is about half as wide as high on average.
    let size = ((height - 4.0) / (n * 1.25)).min((width - 6.0) / (longest * 0.52)).clamp(3.0, 14.0);
    let mut content = String::from("q 0.94 0.96 1 rg 0 0 ");
    content.push_str(&format!(
        "{width:.2} {height:.2} re f 0.2 0.4 0.85 RG 1 w 0.5 0.5 {:.2} {:.2} re S Q\n",
        width - 1.0,
        height - 1.0
    ));
    content.push_str("BT 0.1 0.1 0.15 rg\n");
    let top = height - 3.0 - size;
    for (i, l) in lines.iter().enumerate() {
        let font = if i == 0 { "/HeBo" } else { "/Helv" };
        let y = top - i as f32 * size * 1.25;
        content.push_str(&format!(
            "{font} {size:.2} Tf 1 0 0 1 4 {y:.2} Tm <{}> Tj\n",
            hex(&crate::appearance::win_ansi(l))
        ));
    }
    content.push_str("ET\n");
    let font = |base: &str| {
        let mut f = Dictionary::new();
        f.set("Type", Object::Name(b"Font".to_vec()));
        f.set("Subtype", Object::Name(b"Type1".to_vec()));
        f.set("BaseFont", Object::Name(base.as_bytes().to_vec()));
        f.set("Encoding", Object::Name(b"WinAnsiEncoding".to_vec()));
        Object::Dictionary(f)
    };
    let mut fonts = Dictionary::new();
    fonts.set("Helv", font("Helvetica"));
    fonts.set("HeBo", font("Helvetica-Bold"));
    let mut res = Dictionary::new();
    res.set("Font", Object::Dictionary(fonts));
    let mut d = Dictionary::new();
    d.set("Type", Object::Name(b"XObject".to_vec()));
    d.set("Subtype", Object::Name(b"Form".to_vec()));
    d.set("BBox", Object::Array(vec![0.into(), 0.into(), Object::Real(width), Object::Real(height)]));
    d.set("Resources", Object::Dictionary(res));
    Stream::new(d, content.into_bytes())
}

/// Full name of a field (parent names joined with ".").
fn field_name(doc: &lopdf::Document, dict: &Dictionary) -> String {
    let own = dict
        .get(b"T")
        .ok()
        .and_then(|t| t.as_str().ok())
        .map(crate::pdf_edit::decode_text_string)
        .unwrap_or_default();
    match dict.get(b"Parent").and_then(Object::as_reference).ok().and_then(|p| doc.get_dictionary(p).ok()) {
        Some(parent) => {
            let up = field_name(doc, parent);
            if up.is_empty() { own } else { format!("{up}.{own}") }
        }
        None => own,
    }
}

/// All field objects (terminal fields and widgets) below the AcroForm.
fn all_fields(doc: &lopdf::Document) -> Vec<ObjectId> {
    let mut out = Vec::new();
    let Ok(root) = doc.catalog() else { return out };
    let form = match root.get(b"AcroForm") {
        Ok(Object::Reference(r)) => doc.get_dictionary(*r).ok(),
        Ok(Object::Dictionary(d)) => Some(d),
        _ => None,
    };
    let Some(fields) = form
        .and_then(|f| f.get(b"Fields").ok())
        .and_then(|f| doc.dereference(f).ok())
        .and_then(|(_, o)| o.as_array().ok())
    else {
        return out;
    };
    let mut stack: Vec<ObjectId> = fields.iter().filter_map(|o| o.as_reference().ok()).collect();
    while let Some(id) = stack.pop() {
        if out.contains(&id) {
            continue;
        }
        out.push(id);
        if let Ok(d) = doc.get_dictionary(id)
            && let Ok(kids) = d.get(b"Kids").and_then(Object::as_array)
        {
            stack.extend(kids.iter().filter_map(|o| o.as_reference().ok()));
        }
    }
    out
}

fn is_sig_field(doc: &lopdf::Document, d: &Dictionary) -> bool {
    let mut cur = Some(d);
    while let Some(c) = cur {
        if let Ok(ft) = c.get(b"FT").and_then(Object::as_name) {
            return ft == b"Sig";
        }
        cur = c.get(b"Parent").and_then(Object::as_reference).ok().and_then(|p| doc.get_dictionary(p).ok());
    }
    false
}

/// Names of the empty (unsigned) signature fields.
pub fn empty_signature_fields(bytes: &[u8]) -> Vec<String> {
    let Ok(doc) = lopdf::Document::load_mem(bytes) else { return Vec::new() };
    all_fields(&doc)
        .into_iter()
        .filter_map(|id| doc.get_dictionary(id).ok().map(|d| (id, d)))
        .filter(|(_, d)| is_sig_field(&doc, d) && !d.has(b"V") && d.has(b"T"))
        .map(|(_, d)| field_name(&doc, d))
        .collect()
}

/// Signs the PDF `bytes` with `id`; returns the signed file.
pub fn sign(
    bytes: &[u8],
    id: &DigitalId,
    opts: &SignOptions,
    now_unix: i64,
    offset_minutes: i32,
) -> Result<Vec<u8>> {
    let prev = lopdf::Document::load_mem(bytes).map_err(|e| bad(format!("PDF lesen: {e}")))?;
    // lopdf decrypts on loading (empty user password) and may drop /Encrypt.
    if prev.is_encrypted() || prev.trailer.has(b"Encrypt") || find(bytes, b"/Encrypt").is_some() {
        return Err(bad(
            "Verschlüsselte Dokumente können nicht signiert werden. Bitte zuerst den Kennwortschutz entfernen.",
        ));
    }
    let root_id =
        prev.trailer.get(b"Root").and_then(Object::as_reference).map_err(|_| bad("PDF ohne Katalog"))?;
    let pages: Vec<ObjectId> = prev.get_pages().into_values().collect();
    let existing = all_fields(&prev);
    let mut inc = IncrementalDocument::create_from(bytes.to_vec(), prev);
    let date = pdf_date(now_unix, offset_minutes);

    // The signature dictionary with placeholders.
    let mut sig = Dictionary::new();
    sig.set("Type", Object::Name(b"Sig".to_vec()));
    sig.set("Filter", Object::Name(b"Adobe.PPKLite".to_vec()));
    sig.set("SubFilter", Object::Name(b"ETSI.CAdES.detached".to_vec()));
    sig.set(
        "ByteRange",
        Object::Array(vec![
            0.into(),
            1_111_111_111i64.into(),
            2_222_222_222i64.into(),
            3_333_333_333i64.into(),
        ]),
    );
    sig.set("Contents", Object::String(vec![0u8; CONTENTS_BYTES], StringFormat::Hexadecimal));
    sig.set("M", crate::pdf_edit::text_string(&date));
    sig.set("Name", crate::pdf_edit::text_string(&id.cert.name));
    for (key, value) in
        [("Reason", &opts.reason), ("Location", &opts.location), ("ContactInfo", &opts.contact)]
    {
        if !value.trim().is_empty() {
            sig.set(key, crate::pdf_edit::text_string(value.trim()));
        }
    }
    let sig_id = inc.new_document.add_object(Object::Dictionary(sig));

    // Lines of the visible signature.
    let mut lines =
        vec![format!("Digital signiert von {}", id.cert.name), format!("Datum: {}", readable_date(&date))];
    if !opts.reason.trim().is_empty() {
        lines.push(format!("Grund: {}", opts.reason.trim()));
    }
    if !opts.location.trim().is_empty() {
        lines.push(format!("Ort: {}", opts.location.trim()));
    }

    // The field (existing or new) gets /V.
    let mut new_field: Option<ObjectId> = None;
    match &opts.place {
        Place::Field(name) => {
            let target = existing
                .iter()
                .copied()
                .find(|&f| {
                    let d = inc.get_prev_documents().get_dictionary(f);
                    d.is_ok_and(|d| {
                        is_sig_field(inc.get_prev_documents(), d)
                            && field_name(inc.get_prev_documents(), d) == *name
                    })
                })
                .ok_or_else(|| bad(format!("Signaturfeld „{name}“ nicht gefunden")))?;
            inc.opt_clone_object_to_new_document(target).map_err(|e| bad(e.to_string()))?;
            // The widget: the field itself or its first kid.
            let widget = {
                let d = inc.new_document.get_dictionary(target).map_err(|e| bad(e.to_string()))?;
                d.get(b"Kids").and_then(Object::as_array).ok().and_then(|k| k.first()?.as_reference().ok())
            };
            let rect = {
                let w = widget.unwrap_or(target);
                inc.opt_clone_object_to_new_document(w).map_err(|e| bad(e.to_string()))?;
                let d = inc.new_document.get_dictionary(w).map_err(|e| bad(e.to_string()))?;
                d.get(b"Rect").and_then(Object::as_array).ok().map(|r| {
                    let v: Vec<f32> = r.iter().filter_map(|o| o.as_float().ok()).collect();
                    if v.len() == 4 {
                        RectF::new(v[0], v[1], v[2], v[3]).normalized()
                    } else {
                        RectF::default()
                    }
                })
            };
            if let Some(r) = rect.filter(|r| r.width() > 1.0 && r.height() > 1.0) {
                let ap =
                    inc.new_document.add_object(Object::Stream(appearance(r.width(), r.height(), &lines)));
                let w = widget.unwrap_or(target);
                if let Ok(d) = inc.new_document.get_dictionary_mut(w) {
                    let mut apd = Dictionary::new();
                    apd.set("N", Object::Reference(ap));
                    d.set("AP", Object::Dictionary(apd));
                }
            }
            let d = inc.new_document.get_dictionary_mut(target).map_err(|e| bad(e.to_string()))?;
            d.set("V", Object::Reference(sig_id));
        }
        Place::Invisible | Place::Rect { .. } => {
            let (page, rect) = match &opts.place {
                Place::Rect { page, rect } => (*page, rect.normalized()),
                _ => (0, RectF::default()),
            };
            let page_id = *pages.get(page).ok_or_else(|| bad("Seite nicht vorhanden"))?;
            // A free field name.
            let names: Vec<String> = existing
                .iter()
                .filter_map(|&f| {
                    inc.get_prev_documents()
                        .get_dictionary(f)
                        .ok()
                        .map(|d| field_name(inc.get_prev_documents(), d))
                })
                .collect();
            let name = (1..).map(|n| format!("Signatur{n}")).find(|n| !names.contains(n)).unwrap_or_default();
            let mut w = Dictionary::new();
            w.set("Type", Object::Name(b"Annot".to_vec()));
            w.set("Subtype", Object::Name(b"Widget".to_vec()));
            w.set("FT", Object::Name(b"Sig".to_vec()));
            w.set("T", crate::pdf_edit::text_string(&name));
            w.set("F", 4i64);
            w.set("P", Object::Reference(page_id));
            w.set("V", Object::Reference(sig_id));
            w.set(
                "Rect",
                Object::Array(vec![
                    Object::Real(rect.x0),
                    Object::Real(rect.y0),
                    Object::Real(rect.x1),
                    Object::Real(rect.y1),
                ]),
            );
            if rect.width() > 1.0 && rect.height() > 1.0 {
                let ap = inc.new_document.add_object(Object::Stream(appearance(
                    rect.width(),
                    rect.height(),
                    &lines,
                )));
                let mut apd = Dictionary::new();
                apd.set("N", Object::Reference(ap));
                w.set("AP", Object::Dictionary(apd));
            }
            let wid = inc.new_document.add_object(Object::Dictionary(w));
            new_field = Some(wid);
            // Into the page's annotations.
            inc.opt_clone_object_to_new_document(page_id).map_err(|e| bad(e.to_string()))?;
            let annots =
                inc.new_document.get_dictionary(page_id).ok().and_then(|p| p.get(b"Annots").ok().cloned());
            match annots {
                Some(Object::Reference(a)) => {
                    inc.opt_clone_object_to_new_document(a).map_err(|e| bad(e.to_string()))?;
                    if let Ok(Object::Array(arr)) = inc.new_document.get_object_mut(a) {
                        arr.push(Object::Reference(wid));
                    }
                }
                Some(Object::Array(mut arr)) => {
                    arr.push(Object::Reference(wid));
                    if let Ok(p) = inc.new_document.get_dictionary_mut(page_id) {
                        p.set("Annots", Object::Array(arr));
                    }
                }
                _ => {
                    if let Ok(p) = inc.new_document.get_dictionary_mut(page_id) {
                        p.set("Annots", Object::Array(vec![Object::Reference(wid)]));
                    }
                }
            }
        }
    }

    // AcroForm: the new field into /Fields, SigFlags 3 (signatures, append only).
    inc.opt_clone_object_to_new_document(root_id).map_err(|e| bad(e.to_string()))?;
    let form = inc.new_document.get_dictionary(root_id).ok().and_then(|c| c.get(b"AcroForm").ok().cloned());
    let form_id = match form {
        Some(Object::Reference(f)) => {
            inc.opt_clone_object_to_new_document(f).map_err(|e| bad(e.to_string()))?;
            f
        }
        other => {
            let d = match other {
                Some(Object::Dictionary(d)) => d,
                _ => Dictionary::new(),
            };
            let f = inc.new_document.add_object(Object::Dictionary(d));
            if let Ok(c) = inc.new_document.get_dictionary_mut(root_id) {
                c.set("AcroForm", Object::Reference(f));
            }
            f
        }
    };
    let fields = inc.new_document.get_dictionary(form_id).ok().and_then(|f| f.get(b"Fields").ok().cloned());
    if let Some(wid) = new_field {
        match fields {
            Some(Object::Reference(a)) => {
                inc.opt_clone_object_to_new_document(a).map_err(|e| bad(e.to_string()))?;
                if let Ok(Object::Array(arr)) = inc.new_document.get_object_mut(a) {
                    arr.push(Object::Reference(wid));
                }
            }
            Some(Object::Array(mut arr)) => {
                arr.push(Object::Reference(wid));
                if let Ok(f) = inc.new_document.get_dictionary_mut(form_id) {
                    f.set("Fields", Object::Array(arr));
                }
            }
            _ => {
                if let Ok(f) = inc.new_document.get_dictionary_mut(form_id) {
                    f.set("Fields", Object::Array(vec![Object::Reference(wid)]));
                }
            }
        }
    }
    if let Ok(f) = inc.new_document.get_dictionary_mut(form_id) {
        f.set("SigFlags", 3i64);
    }

    // Save, then fill in the byte ranges and the signature.
    let mut out = Vec::new();
    inc.save_to(&mut out).map_err(|e| bad(format!("Speichern: {e}")))?;
    let range_at = find(&out, RANGE_MARK).ok_or_else(|| bad("Platzhalter /ByteRange nicht gefunden"))?;
    let zeros = vec![b'0'; CONTENTS_BYTES * 2];
    let hex_start = find_from(&out, &zeros, range_at)
        .or_else(|| find(&out, &zeros))
        .ok_or_else(|| bad("Platzhalter /Contents nicht gefunden"))?;
    let (a, b) = (hex_start - 1, hex_start + zeros.len() + 1);
    if out.get(a) != Some(&b'<') || out.get(b - 1) != Some(&b'>') {
        return Err(bad("Platzhalter /Contents unerwartet"));
    }
    let ranges = format!("{} {} {}", a, b, out.len() - b);
    if ranges.len() > RANGE_MARK.len() {
        return Err(bad("Datei zu groß zum Signieren"));
    }
    let padded = format!("{ranges:<width$}", width = RANGE_MARK.len());
    out[range_at..range_at + RANGE_MARK.len()].copy_from_slice(padded.as_bytes());
    let digest = super::cert::digest(super::cert::SHA256, &[&out[..a], &out[b..]])?;
    let cms_bytes = cms::signed_data(id, &digest)?;
    if cms_bytes.len() > CONTENTS_BYTES {
        return Err(bad("Die Signatur ist zu groß (Zertifikatskette)."));
    }
    out[hex_start..hex_start + cms_bytes.len() * 2].copy_from_slice(hex(&cms_bytes).as_bytes());
    Ok(out)
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    find_from(hay, needle, 0)
}

fn find_from(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || from >= hay.len() {
        return None;
    }
    hay[from..].windows(needle.len()).position(|w| w == needle).map(|p| p + from)
}

/// One signature of a document, checked.
#[derive(Debug, Clone, PartialEq)]
pub struct SigInfo {
    pub field: String,
    /// Name in the certificate (else /Name).
    pub signer: String,
    pub subject: String,
    pub issuer: String,
    pub self_signed: bool,
    pub cert_valid_until: String,
    /// "04.11.2026 19:30:00" from /M.
    pub time: String,
    pub reason: String,
    pub location: String,
    pub sub_filter: String,
    pub check: cms::Check,
    /// The signature covers the whole file (nothing appended later).
    pub whole_file: bool,
    /// Byte length of the signed revision.
    pub revision_end: usize,
}

impl SigInfo {
    pub fn valid(&self) -> bool {
        self.check == cms::Check::Valid
    }
}

/// Quick test whether the file may contain signatures (no parsing).
pub fn has_signatures(bytes: &[u8]) -> bool {
    find(bytes, b"/ByteRange").is_some()
}

/// The signatures of a PDF, each checked against the file bytes.
pub fn signatures(bytes: &[u8]) -> Vec<SigInfo> {
    let Ok(doc) = lopdf::Document::load_mem(bytes) else { return Vec::new() };
    let mut out = Vec::new();
    let mut seen: Vec<ObjectId> = Vec::new();
    for fid in all_fields(&doc) {
        let Ok(field) = doc.get_dictionary(fid) else { continue };
        if !is_sig_field(&doc, field) {
            continue;
        }
        let Ok(v) = field.get(b"V").and_then(Object::as_reference) else { continue };
        if seen.contains(&v) {
            continue;
        }
        seen.push(v);
        let Ok(sig) = doc.get_dictionary(v) else { continue };
        out.push(check_one(&doc, bytes, field_name(&doc, field), sig));
    }
    // Earliest signature first.
    out.sort_by_key(|s| s.revision_end);
    out
}

fn text_of(d: &Dictionary, key: &[u8]) -> String {
    d.get(key).ok().and_then(|o| o.as_str().ok()).map(crate::pdf_edit::decode_text_string).unwrap_or_default()
}

fn check_one(_doc: &lopdf::Document, bytes: &[u8], field: String, sig: &Dictionary) -> SigInfo {
    let sub_filter = sig
        .get(b"SubFilter")
        .and_then(Object::as_name)
        .map(|n| String::from_utf8_lossy(n).into_owned())
        .unwrap_or_default();
    let range: Vec<i64> = sig
        .get(b"ByteRange")
        .and_then(Object::as_array)
        .map(|a| a.iter().filter_map(|o| o.as_i64().ok()).collect())
        .unwrap_or_default();
    let contents = sig.get(b"Contents").ok().and_then(|o| o.as_str().ok()).unwrap_or_default();
    let mut info = SigInfo {
        field,
        signer: text_of(sig, b"Name"),
        subject: String::new(),
        issuer: String::new(),
        self_signed: false,
        cert_valid_until: String::new(),
        time: readable_date(&text_of(sig, b"M")),
        reason: text_of(sig, b"Reason"),
        location: text_of(sig, b"Location"),
        sub_filter: sub_filter.clone(),
        check: cms::Check::Unsupported(String::new()),
        whole_file: false,
        revision_end: 0,
    };
    let ok_range = range.len() == 4 && range.iter().all(|v| *v >= 0) && range[0] == 0 && {
        let (l0, s1, l1) = (range[1] as usize, range[2] as usize, range[3] as usize);
        l0 <= s1 && s1.checked_add(l1).is_some_and(|e| e <= bytes.len())
    };
    if !ok_range {
        info.check = cms::Check::Unsupported("Bytebereich ungültig".into());
        return info;
    }
    let (l0, s1, l1) = (range[1] as usize, range[2] as usize, range[3] as usize);
    info.revision_end = s1 + l1;
    info.whole_file = s1 + l1 == bytes.len();
    if !matches!(sub_filter.as_str(), "ETSI.CAdES.detached" | "adbe.pkcs7.detached") {
        info.check = cms::Check::Unsupported(format!("Format {sub_filter}"));
        return info;
    }
    match cms::parse(contents) {
        Ok(p) => {
            if let Some(c) = p.signer.and_then(|i| p.certs.get(i)) {
                info.signer = c.name.clone();
                info.subject = c.subject.clone();
                info.issuer = c.issuer.clone();
                info.self_signed = c.self_signed();
                info.cert_valid_until = c.not_after.clone();
            }
            info.check = cms::check(&p, &[&bytes[..l0], &bytes[s1..s1 + l1]]);
        }
        Err(e) => info.check = cms::Check::Unsupported(e.to_string()),
    }
    info
}
