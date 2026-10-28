//! "Medien" of the ribbon tab "Anmerkungen": video, audio and 3D files are
//! embedded as screen and 3D annotations, recordings as sound notes. MrPDF
//! does not play media itself; "Abspielen" hands the embedded file to the
//! program the system uses for it.

use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Object, Stream, StringFormat};

use crate::appearance::win_ansi;
use crate::error::{Error, Result};
use crate::geometry::RectF;
use crate::pdf_edit::{Edit, guarded, text_string};

/// Largest media file that is embedded.
pub const MAX_MEDIA: usize = 256 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Video,
    Audio,
    /// U3D or PRC model.
    Model3d,
}

impl MediaKind {
    /// File name extensions offered in the file dialog.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            MediaKind::Video => &["mp4", "m4v", "mov", "webm", "avi", "wmv", "mpg", "mpeg"],
            MediaKind::Audio => &["mp3", "wav", "m4a", "aac", "ogg", "flac", "wma"],
            MediaKind::Model3d => &["u3d", "prc"],
        }
    }
}

/// MIME type for the media clip.
fn mime(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or_default().to_ascii_lowercase();
    match ext.as_str() {
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "avi" => "video/x-msvideo",
        "wmv" => "video/x-ms-wmv",
        "mpg" | "mpeg" => "video/mpeg",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "m4a" | "aac" => "audio/mp4",
        "ogg" => "audio/ogg",
        "flac" => "audio/flac",
        "wma" => "audio/x-ms-wma",
        _ => "application/octet-stream",
    }
}

fn rect_array(r: &RectF) -> Object {
    Object::Array(vec![Object::Real(r.x0), Object::Real(r.y0), Object::Real(r.x1), Object::Real(r.y1)])
}

/// Appearance: dark box with a symbol and the file name.
fn appearance(edit: &mut Edit, rect: &RectF, title: &str, play: bool) -> Result<lopdf::ObjectId> {
    let (w, h) = (rect.width(), rect.height());
    let (cx, cy) = (w / 2.0, h / 2.0);
    let s = (w.min(h) * 0.18).clamp(4.0, 24.0);
    let op = |o: &str, v: Vec<Object>| Operation::new(o, v);
    let r = |v: f32| Object::Real(v);
    let mut ops = vec![
        op("q", vec![]),
        op("rg", vec![r(0.18), r(0.2), r(0.24)]),
        op("re", vec![r(0.0), r(0.0), r(w), r(h)]),
        op("f", vec![]),
        op("rg", vec![r(1.0), r(1.0), r(1.0)]),
    ];
    if play {
        ops.extend([
            op("m", vec![r(cx - s * 0.7), r(cy - s)]),
            op("l", vec![r(cx + s), r(cy)]),
            op("l", vec![r(cx - s * 0.7), r(cy + s)]),
            op("f", vec![]),
        ]);
    } else {
        // A cube outline for 3D models.
        ops.extend([
            op("RG", vec![r(1.0), r(1.0), r(1.0)]),
            op("w", vec![r(1.2)]),
            op("re", vec![r(cx - s), r(cy - s), r(s * 1.6), r(s * 1.6)]),
            op("m", vec![r(cx - s), r(cy + s * 0.6)]),
            op("l", vec![r(cx - s * 0.6), r(cy + s)]),
            op("l", vec![r(cx + s), r(cy + s)]),
            op("l", vec![r(cx + s * 0.6), r(cy + s * 0.6)]),
            op("m", vec![r(cx + s), r(cy + s)]),
            op("l", vec![r(cx + s), r(cy - s * 0.6)]),
            op("l", vec![r(cx + s * 0.6), r(cy - s)]),
            op("S", vec![]),
        ]);
    }
    if h >= 24.0 {
        let mut label = win_ansi(title);
        label.truncate(60);
        ops.extend([
            op("BT", vec![]),
            op("Tf", vec![Object::Name(b"Helv".to_vec()), r(8.0)]),
            op("Td", vec![r(4.0), r(4.0)]),
            op("Tj", vec![Object::String(label, StringFormat::Literal)]),
            op("ET", vec![]),
        ]);
    }
    ops.push(op("Q", vec![]));
    let content = Content { operations: ops }.encode().map_err(|e| Error::Structure(e.to_string()))?;
    let mut font = Dictionary::new();
    font.set("Type", Object::Name(b"Font".to_vec()));
    font.set("Subtype", Object::Name(b"Type1".to_vec()));
    font.set("BaseFont", Object::Name(b"Helvetica".to_vec()));
    font.set("Encoding", Object::Name(b"WinAnsiEncoding".to_vec()));
    let mut fonts = Dictionary::new();
    fonts.set("Helv", Object::Dictionary(font));
    let mut resources = Dictionary::new();
    resources.set("Font", Object::Dictionary(fonts));
    let mut d = Dictionary::new();
    d.set("Type", Object::Name(b"XObject".to_vec()));
    d.set("Subtype", Object::Name(b"Form".to_vec()));
    d.set("BBox", rect_array(&RectF::new(0.0, 0.0, w, h)));
    d.set("Resources", Object::Dictionary(resources));
    Ok(edit.add(Stream::new(d, content)))
}

fn base_annot(
    subtype: &[u8],
    rect: &RectF,
    page_id: lopdf::ObjectId,
    title: &str,
    author: &str,
) -> Dictionary {
    let mut d = Dictionary::new();
    d.set("Type", Object::Name(b"Annot".to_vec()));
    d.set("Subtype", Object::Name(subtype.to_vec()));
    d.set("Rect", rect_array(rect));
    d.set("P", Object::Reference(page_id));
    d.set("F", Object::Integer(4));
    d.set("Contents", text_string(title));
    d.set("M", Object::string_literal(crate::pdf_edit::pdf_date_now()));
    if !author.is_empty() {
        d.set("T", text_string(author));
    }
    d
}

fn file_spec(edit: &mut Edit, name: &str, data: &[u8]) -> Result<Dictionary> {
    let mut params = Dictionary::new();
    params.set("Size", Object::Integer(data.len() as i64));
    let mut ef_dict = Dictionary::new();
    ef_dict.set("Type", Object::Name(b"EmbeddedFile".to_vec()));
    ef_dict.set("Params", Object::Dictionary(params));
    let mut stream = Stream::new(ef_dict, data.to_vec());
    stream.compress()?;
    let id = edit.add(stream);
    let mut ef = Dictionary::new();
    ef.set("F", Object::Reference(id));
    let mut fs = Dictionary::new();
    fs.set("Type", Object::Name(b"Filespec".to_vec()));
    fs.set("F", text_string(name));
    fs.set("UF", text_string(name));
    fs.set("EF", Object::Dictionary(ef));
    Ok(fs)
}

/// Embeds a video, audio or 3D file in `rect` of `page`.
#[allow(clippy::too_many_arguments)]
pub fn add_media(
    bytes: &[u8],
    password: Option<&str>,
    page: usize,
    rect: RectF,
    kind: MediaKind,
    name: &str,
    data: &[u8],
    author: &str,
) -> Result<Vec<u8>> {
    if data.len() > MAX_MEDIA {
        return Err(Error::FileTooLarge { size: data.len() as u64, limit: MAX_MEDIA as u64 });
    }
    let rect = rect.normalized();
    if rect.width() < 4.0 || rect.height() < 4.0 {
        return Err(Error::Invalid("media area is too small".into()));
    }
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let page_id = edit.page_id(page)?;
        let ap = appearance(&mut edit, &rect, name, kind != MediaKind::Model3d)?;
        let mut d = match kind {
            MediaKind::Model3d => {
                let ext = name.rsplit('.').next().unwrap_or_default().to_ascii_uppercase();
                let mut sd = Dictionary::new();
                sd.set("Type", Object::Name(b"3D".to_vec()));
                sd.set("Subtype", Object::Name(if ext == "PRC" { b"PRC".to_vec() } else { b"U3D".to_vec() }));
                let mut stream = Stream::new(sd, data.to_vec());
                stream.compress()?;
                let id = edit.add(stream);
                let mut d = base_annot(b"3D", &rect, page_id, name, author);
                d.set("3DD", Object::Reference(id));
                d
            }
            _ => {
                let fs = file_spec(&mut edit, name, data)?;
                let mut clip = Dictionary::new();
                clip.set("Type", Object::Name(b"MediaClip".to_vec()));
                clip.set("S", Object::Name(b"MCD".to_vec()));
                clip.set("CT", Object::string_literal(mime(name)));
                clip.set("D", Object::Dictionary(fs));
                clip.set("N", text_string(name));
                let mut rendition = Dictionary::new();
                rendition.set("Type", Object::Name(b"Rendition".to_vec()));
                rendition.set("S", Object::Name(b"MR".to_vec()));
                rendition.set("C", Object::Dictionary(clip));
                let mut d = base_annot(b"Screen", &rect, page_id, name, author);
                let annot_id = edit.add(Object::Null);
                let mut action = Dictionary::new();
                action.set("S", Object::Name(b"Rendition".to_vec()));
                action.set("OP", Object::Integer(0));
                action.set("R", Object::Dictionary(rendition));
                action.set("AN", Object::Reference(annot_id));
                d.set("A", Object::Dictionary(action));
                let mut ap_dict = Dictionary::new();
                ap_dict.set("N", Object::Reference(ap));
                d.set("AP", Object::Dictionary(ap_dict));
                *edit.object_mut(annot_id)? = Object::Dictionary(d);
                edit.annots_mut(page_id)?.push(Object::Reference(annot_id));
                return edit.finish();
            }
        };
        let mut ap_dict = Dictionary::new();
        ap_dict.set("N", Object::Reference(ap));
        d.set("AP", Object::Dictionary(ap_dict));
        let id = edit.add(Object::Dictionary(d));
        edit.annots_mut(page_id)?.push(Object::Reference(id));
        edit.finish()
    })
}

/// PCM data of a WAV file.
struct Wav<'a> {
    rate: u32,
    channels: u16,
    bits: u16,
    samples: &'a [u8],
}

fn parse_wav(data: &[u8]) -> Result<Wav<'_>> {
    let bad = || Error::Invalid("only uncompressed WAV files (PCM) can be used for audio notes".into());
    if data.len() < 12 || &data[..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        return Err(bad());
    }
    let (mut pos, mut fmt, mut samples) = (12usize, None, None);
    while pos + 8 <= data.len() {
        let id = &data[pos..pos + 4];
        let len = u32::from_le_bytes(data[pos + 4..pos + 8].try_into().map_err(|_| bad())?) as usize;
        let body = data.get(pos + 8..pos + 8 + len).ok_or_else(bad)?;
        match id {
            b"fmt " if body.len() >= 16 => {
                let format = u16::from_le_bytes([body[0], body[1]]);
                if format != 1 {
                    return Err(bad());
                }
                let channels = u16::from_le_bytes([body[2], body[3]]);
                let rate = u32::from_le_bytes([body[4], body[5], body[6], body[7]]);
                let bits = u16::from_le_bytes([body[14], body[15]]);
                fmt = Some((rate, channels, bits));
            }
            b"data" => samples = Some(body),
            _ => {}
        }
        pos += 8 + len + (len & 1);
    }
    match (fmt, samples) {
        (Some((rate, channels, bits)), Some(samples))
            if (1..=8).contains(&channels) && matches!(bits, 8 | 16 | 24 | 32) && rate > 0 =>
        {
            Ok(Wav { rate, channels, bits, samples })
        }
        _ => Err(bad()),
    }
}

/// Adds a sound note ("Audio" in "Notizen") from a WAV recording.
pub fn add_sound(
    bytes: &[u8],
    password: Option<&str>,
    page: usize,
    rect: RectF,
    name: &str,
    wav: &[u8],
    author: &str,
) -> Result<Vec<u8>> {
    if wav.len() > MAX_MEDIA {
        return Err(Error::FileTooLarge { size: wav.len() as u64, limit: MAX_MEDIA as u64 });
    }
    let w = parse_wav(wav)?;
    let rect = rect.normalized();
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let page_id = edit.page_id(page)?;
        // PDF sound samples are big-endian; WAV stores little-endian, and
        // 8 bit WAV samples are unsigned.
        let mut samples = w.samples.to_vec();
        let width = usize::from(w.bits / 8);
        if width > 1 {
            for chunk in samples.chunks_exact_mut(width) {
                chunk.reverse();
            }
        }
        let mut sd = Dictionary::new();
        sd.set("Type", Object::Name(b"Sound".to_vec()));
        sd.set("R", Object::Integer(i64::from(w.rate)));
        sd.set("C", Object::Integer(i64::from(w.channels)));
        sd.set("B", Object::Integer(i64::from(w.bits)));
        sd.set("E", Object::Name(if w.bits == 8 { b"Raw".to_vec() } else { b"Signed".to_vec() }));
        let mut stream = Stream::new(sd, samples);
        stream.compress()?;
        let sound = edit.add(stream);
        let ap = appearance(&mut edit, &rect, "", true)?;
        let mut d = base_annot(b"Sound", &rect, page_id, name, author);
        d.set("Sound", Object::Reference(sound));
        d.set("Name", Object::Name(b"Speaker".to_vec()));
        let mut ap_dict = Dictionary::new();
        ap_dict.set("N", Object::Reference(ap));
        d.set("AP", Object::Dictionary(ap_dict));
        let id = edit.add(Object::Dictionary(d));
        edit.annots_mut(page_id)?.push(Object::Reference(id));
        edit.finish()
    })
}

/// Builds a WAV file from PDF sound samples.
fn wav_file(rate: u32, channels: u16, bits: u16, signed: bool, samples: &[u8]) -> Vec<u8> {
    let width = usize::from(bits / 8).max(1);
    let mut data = samples.to_vec();
    if width > 1 {
        for chunk in data.chunks_exact_mut(width) {
            chunk.reverse();
        }
    } else if signed {
        for b in &mut data {
            *b ^= 0x80;
        }
    }
    let block = u32::from(channels) * width as u32;
    let mut out = Vec::with_capacity(data.len() + 44);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * block).to_le_bytes());
    out.extend_from_slice(&(block as u16).to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&data);
    out
}

/// The media file of a screen, 3D or sound annotation: (file name, data).
pub fn media_file(
    bytes: &[u8],
    password: Option<&str>,
    page: usize,
    index: usize,
) -> Result<Option<(String, Vec<u8>)>> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let page_id = edit.page_id(page)?;
        let id = edit.annotation_id(page_id, index)?;
        let d = edit.object(id)?.as_dict()?.clone();
        let stream_of = |edit: &Edit, obj: &Object| -> Result<Vec<u8>> {
            let s = edit.resolve(obj)?.as_stream()?;
            Ok(s.decompressed_content().unwrap_or_else(|_| s.content.clone()))
        };
        let name_of = |edit: &Edit, obj: Option<&Object>, fallback: &str| -> String {
            obj.and_then(|o| edit.resolve(o).ok())
                .and_then(|o| o.as_str().ok())
                .map(crate::pdf_edit::decode_text_string)
                .filter(|n| !n.trim().is_empty())
                .unwrap_or_else(|| fallback.to_string())
        };
        match d.get(b"Subtype").and_then(|s| s.as_name()).unwrap_or_default() {
            b"Screen" => {
                let action = edit.resolve(d.get(b"A")?)?.as_dict()?;
                let rendition = edit.resolve(action.get(b"R")?)?.as_dict()?;
                let clip = edit.resolve(rendition.get(b"C")?)?.as_dict()?;
                let fs = edit.resolve(clip.get(b"D")?)?.as_dict()?;
                let ef = edit.resolve(fs.get(b"EF")?)?.as_dict()?;
                let data = stream_of(&edit, ef.get(b"F")?)?;
                let name = name_of(&edit, fs.get(b"UF").or_else(|_| fs.get(b"F")).ok(), "media.bin");
                Ok(Some((name, data)))
            }
            b"3D" => {
                let s = edit.resolve(d.get(b"3DD")?)?.as_stream()?;
                let ext = if s.dict.get(b"Subtype").and_then(|n| n.as_name()).ok() == Some(b"PRC") {
                    "prc"
                } else {
                    "u3d"
                };
                let data = stream_of(&edit, d.get(b"3DD")?)?;
                Ok(Some((format!("model.{ext}"), data)))
            }
            b"Sound" => {
                let s = edit.resolve(d.get(b"Sound")?)?.as_stream()?;
                let int = |k: &[u8], def: i64| s.dict.get(k).and_then(|o| o.as_i64()).unwrap_or(def);
                let rate = int(b"R", 8000).clamp(1, 384_000) as u32;
                let channels = int(b"C", 1).clamp(1, 8) as u16;
                let bits = match int(b"B", 8) {
                    16 => 16,
                    24 => 24,
                    32 => 32,
                    _ => 8,
                };
                let signed = s.dict.get(b"E").and_then(|o| o.as_name()).ok() == Some(b"Signed");
                let samples = stream_of(&edit, d.get(b"Sound")?)?;
                Ok(Some(("sound.wav".to_string(), wav_file(rate, channels, bits, signed, &samples))))
            }
            _ => Ok(None),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_round_trip() {
        let samples: Vec<u8> = (0..200u16).flat_map(|v| (v * 300).to_le_bytes()).collect();
        let wav =
            wav_file(22_050, 1, 16, true, &samples.chunks(2).flat_map(|c| [c[1], c[0]]).collect::<Vec<_>>());
        let parsed = parse_wav(&wav).unwrap();
        assert_eq!((parsed.rate, parsed.channels, parsed.bits), (22_050, 1, 16));
        assert_eq!(parsed.samples, &samples[..]);
        assert!(parse_wav(b"RIFF\0\0\0\0WAVEjunk").is_err());
        assert!(parse_wav(b"not a wav").is_err());
        assert_eq!(mime("Film.MP4"), "video/mp4");
        assert_eq!(mime("x.unknown"), "application/octet-stream");
    }
}
