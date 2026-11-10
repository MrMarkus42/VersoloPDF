//! Messages between VersoloPDF and its PDF engine helper.
//!
//! Every message is a frame: its length (4 bytes, little endian) followed by
//! the data, encoded with `postcard`. The program sends a [`Request`]; the
//! helper answers with one byte (0: done, 1: failed) followed by the result
//! value or a [`Fault`]. Nothing the helper sends is trusted: frames have a
//! size limit and every value is decoded by safe Rust code.

use std::io::{Read, Write};

use serde::{Deserialize, Serialize};

use crate::content::{Affine, ObjectStyle, TextStyle};
use crate::error::Error;
use crate::geometry::{PointF, RectF, Rotation};
use crate::model::AnnotFilter;

/// Version of the protocol; the helper reports it first.
pub const PROTOCOL: u32 = 1;

/// Largest frame accepted (documents are at most 2 GiB, pictures 1 GiB).
pub const MAX_FRAME: usize = (1 << 31) + (1 << 20);

/// Bytes sent as one block (much faster than element by element).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bytes(pub Vec<u8>);

impl Serialize for Bytes {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(&self.0)
    }
}

impl<'de> Deserialize<'de> for Bytes {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = Bytes;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("bytes")
            }
            fn visit_bytes<E: serde::de::Error>(self, v: &[u8]) -> Result<Bytes, E> {
                Ok(Bytes(v.to_vec()))
            }
            fn visit_byte_buf<E: serde::de::Error>(self, v: Vec<u8>) -> Result<Bytes, E> {
                Ok(Bytes(v))
            }
            fn visit_borrowed_bytes<E: serde::de::Error>(self, v: &'de [u8]) -> Result<Bytes, E> {
                Ok(Bytes(v.to_vec()))
            }
        }
        d.deserialize_byte_buf(V)
    }
}

/// An RGBA picture.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgba: Bytes,
}

impl Picture {
    pub fn from_image(img: &image::DynamicImage) -> Self {
        let rgba = img.to_rgba8();
        Self { width: rgba.width(), height: rgba.height(), rgba: Bytes(rgba.into_raw()) }
    }

    pub fn to_image(&self) -> Option<image::DynamicImage> {
        let expected = (self.width as usize).checked_mul(self.height as usize)?.checked_mul(4)?;
        if self.rgba.0.len() != expected {
            return None;
        }
        image::RgbaImage::from_raw(self.width, self.height, self.rgba.0.clone())
            .map(image::DynamicImage::ImageRgba8)
    }
}

/// How a page is drawn (see `document::RenderOptions`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrawSpec {
    pub rotation: Rotation,
    pub annotations: bool,
    pub background: [u8; 3],
    pub printing: bool,
    pub transparent: bool,
}

/// What the helper reports after starting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hello {
    pub protocol: u32,
    /// The restrictions are in force.
    pub locked: bool,
    /// What was restricted, or why not.
    pub detail: String,
}

/// Facts about a loaded document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoadInfo {
    pub pages: usize,
    /// Width and height of every page (rotation applied) in points.
    pub sizes: Vec<(f32, f32)>,
    /// No security handler.
    pub unprotected: bool,
    /// PDFium grants every right (revision 2–4 documents opened with the
    /// owner password).
    pub all_rights: bool,
}

/// A copied page object (see `document::ClipItem`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WireClip {
    Text {
        text: String,
        font: String,
        size: f32,
        color: crate::model::Rgba,
        bold: bool,
        italic: bool,
        serif: bool,
        mono: bool,
        matrix: Affine,
    },
    Image {
        picture: Picture,
        matrix: Affine,
    },
    Path {
        /// (kind: 0 move, 1 line, 2 bezier; x; y; closes)
        segments: Vec<(u8, f32, f32, bool)>,
        fill: Option<crate::model::Rgba>,
        stroke: Option<crate::model::Rgba>,
        width: f32,
        matrix: Affine,
    },
}

/// Font and place of the first object of a text block that is replaced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextOrigin {
    /// The object is a text object (its font can be used again).
    pub found: bool,
    pub raw_name: String,
    pub embedded: bool,
    pub serif: bool,
    pub mono: bool,
    /// Index where the new text goes.
    pub pos: usize,
}

/// Something the program asks the helper to do. Page numbers start at 0.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Request {
    // Documents
    Load {
        bytes: Bytes,
        password: Option<String>,
    },
    /// A new document with empty pages of the given sizes; answers its bytes.
    CreateBlank {
        sizes: Vec<(f32, f32)>,
    },
    /// A new document of all pages of the given PDFs; answers its bytes.
    Combine {
        sources: Vec<Bytes>,
    },
    Save,
    /// Page count and sizes after changes.
    Info,
    // Reading
    Geometry {
        page: usize,
    },
    PageLabel {
        page: usize,
    },
    Render {
        page: usize,
        full: (u32, u32),
        part: (u32, u32, u32, u32),
        spec: DrawSpec,
        filter: Option<AnnotFilter>,
    },
    RenderWithoutText {
        page: usize,
        size: (u32, u32),
        annotations: bool,
    },
    PageText {
        page: usize,
    },
    PageTextStyled {
        page: usize,
    },
    Bookmarks,
    Links {
        page: usize,
    },
    Annotations {
        page: usize,
    },
    ObjectAt {
        page: usize,
        point: PointF,
    },
    FormFields {
        page: usize,
    },
    Metadata,
    Attachments,
    Attachment {
        index: u16,
    },
    PathBounds {
        page: usize,
    },
    PageImages {
        page: usize,
    },
    MediaBox {
        page: usize,
    },
    ExtractPages {
        spec: String,
    },
    // Editing
    Flatten {
        pages: Vec<usize>,
    },
    DuplicatePages {
        spec: String,
        at: usize,
    },
    ReplacePages {
        pages: Vec<usize>,
        source: Bytes,
        start: usize,
        count: usize,
    },
    AddAttachment {
        name: String,
        bytes: Bytes,
    },
    DeleteAttachment {
        index: u16,
    },
    RotatePages {
        pages: Vec<(usize, Rotation)>,
    },
    DeletePages {
        pages: Vec<usize>,
    },
    InsertBlank {
        at: usize,
        size: (f32, f32),
        count: usize,
    },
    /// Pages of another PDF: all, or those of `spec` ("1,3,4").
    InsertPages {
        at: usize,
        source: Bytes,
        password: Option<String>,
        spec: Option<String>,
    },
    RemoveOutside {
        crops: Vec<(usize, RectF)>,
    },
    // Page objects
    PageObjects {
        page: usize,
    },
    ObjectCount {
        page: usize,
    },
    TransformObjects {
        page: usize,
        moves: Vec<(usize, Affine)>,
    },
    DeleteObjects {
        page: usize,
        indices: Vec<usize>,
    },
    StyleObjects {
        page: usize,
        indices: Vec<usize>,
        style: ObjectStyle,
    },
    ReorderObjects {
        page: usize,
        order: Vec<usize>,
    },
    CopyObjects {
        page: usize,
        indices: Vec<usize>,
    },
    PasteObjects {
        page: usize,
        items: Vec<WireClip>,
        offset: (f32, f32),
    },
    ObjectImage {
        page: usize,
        index: usize,
    },
    ReplaceObjectImage {
        page: usize,
        index: usize,
        picture: Picture,
    },
    TextOrigin {
        page: usize,
        first: usize,
    },
    ReplaceText {
        page: usize,
        old: Vec<usize>,
        text: String,
        style: TextStyle,
        left: f32,
        baseline: f32,
        width: f32,
        leading: f32,
        /// Use the font of the first old object.
        keep_font: bool,
        /// A system font with many characters (for text outside WinAnsi).
        system_font: Option<Bytes>,
    },
    /// Test only: the helper tries things the restrictions must forbid and
    /// reports what worked.
    ProbeRestrictions {
        file: String,
        address: String,
    },
    /// Test only: the helper ends abruptly (as after a crash).
    TestCrash,
}

/// A failure reported by the helper.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Fault {
    PasswordRequired,
    WrongPassword,
    InvalidFormat,
    PageOutOfRange(usize),
    NotPermitted,
    Invalid(String),
    Engine(String),
}

impl From<Error> for Fault {
    fn from(err: Error) -> Self {
        match err {
            Error::PasswordRequired => Fault::PasswordRequired,
            Error::WrongPassword => Fault::WrongPassword,
            Error::InvalidFormat => Fault::InvalidFormat,
            Error::PageOutOfRange(p) => Fault::PageOutOfRange(p),
            Error::NotPermitted => Fault::NotPermitted,
            Error::Invalid(s) => Fault::Invalid(s),
            other => Fault::Engine(other.to_string()),
        }
    }
}

impl From<Fault> for Error {
    fn from(f: Fault) -> Self {
        let cut = |s: String| s.chars().take(2000).collect::<String>();
        match f {
            Fault::PasswordRequired => Error::PasswordRequired,
            Fault::WrongPassword => Error::WrongPassword,
            Fault::InvalidFormat => Error::InvalidFormat,
            Fault::PageOutOfRange(p) => Error::PageOutOfRange(p),
            Fault::NotPermitted => Error::NotPermitted,
            Fault::Invalid(s) => Error::Invalid(cut(s)),
            Fault::Engine(s) => Error::Pdfium(cut(s)),
        }
    }
}

/// Writes one frame.
pub fn write_frame(w: &mut impl Write, data: &[u8]) -> std::io::Result<()> {
    let len = u32::try_from(data.len()).map_err(|_| std::io::Error::other("frame too large"))?;
    w.write_all(&len.to_le_bytes())?;
    w.write_all(data)?;
    w.flush()
}

/// Reads one frame (`None` at the end of the stream).
pub fn read_frame(r: &mut impl Read) -> std::io::Result<Option<Vec<u8>>> {
    let mut len = [0u8; 4];
    match r.read_exact(&mut len) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_FRAME {
        return Err(std::io::Error::other("frame too large"));
    }
    // Read in steps: a lying length must not allocate everything at once.
    let mut data = Vec::with_capacity(len.min(64 << 20));
    let mut taken = r.take(len as u64);
    taken.read_to_end(&mut data)?;
    if data.len() != len {
        return Err(std::io::ErrorKind::UnexpectedEof.into());
    }
    Ok(Some(data))
}

pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, Error> {
    postcard::to_stdvec(value).map_err(|e| Error::Invalid(format!("engine message: {e}")))
}

pub fn decode<'a, T: Deserialize<'a>>(data: &'a [u8]) -> Result<T, Error> {
    postcard::from_bytes(data).map_err(|e| Error::Pdfium(format!("invalid answer of the PDF engine: {e}")))
}

/// An answer frame: 0 + value.
pub fn answer_ok<T: Serialize>(value: &T) -> Result<Vec<u8>, Error> {
    let mut out = vec![0u8];
    out.extend(encode(value)?);
    Ok(out)
}

/// An answer frame: 1 + fault.
pub fn answer_err(fault: &Fault) -> Vec<u8> {
    let mut out = vec![1u8];
    out.extend(postcard::to_stdvec(fault).unwrap_or_default());
    out
}

/// Reads an answer frame.
pub fn read_answer<T: for<'a> Deserialize<'a>>(frame: &[u8]) -> Result<T, Error> {
    match frame.split_first() {
        Some((0, rest)) => decode(rest),
        Some((1, rest)) => Err(decode::<Fault>(rest)?.into()),
        _ => Err(Error::Pdfium("invalid answer of the PDF engine".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip() {
        let req = Request::Load { bytes: Bytes(vec![1, 2, 3, 255]), password: Some("ä".into()) };
        let mut buf = Vec::new();
        write_frame(&mut buf, &encode(&req).unwrap()).unwrap();
        let frame = read_frame(&mut buf.as_slice()).unwrap().unwrap();
        assert_eq!(decode::<Request>(&frame).unwrap(), req);
        assert!(read_frame(&mut [].as_slice()).unwrap().is_none());
        // A lying length is refused without allocating it.
        let mut lie = (u32::MAX).to_le_bytes().to_vec();
        lie.push(0);
        assert!(read_frame(&mut lie.as_slice()).is_err());
        let short = [5u8, 0, 0, 0, 1];
        assert!(read_frame(&mut short.as_slice()).is_err());
        // Answers.
        let ok = answer_ok(&vec![1usize, 2]).unwrap();
        assert_eq!(read_answer::<Vec<usize>>(&ok).unwrap(), vec![1, 2]);
        let err = answer_err(&Fault::WrongPassword);
        assert!(matches!(read_answer::<Vec<usize>>(&err), Err(Error::WrongPassword)));
        assert!(read_answer::<u8>(&[7]).is_err());
        assert!(read_answer::<String>(&[0, 200, 200, 200]).is_err());
    }

    #[test]
    fn pictures_are_checked() {
        let p = Picture { width: 2, height: 2, rgba: Bytes(vec![0; 16]) };
        assert!(p.to_image().is_some());
        let bad = Picture { width: 3, height: 2, rgba: Bytes(vec![0; 16]) };
        assert!(bad.to_image().is_none());
        let huge = Picture { width: u32::MAX, height: u32::MAX, rgba: Bytes(Vec::new()) };
        assert!(huge.to_image().is_none());
    }
}
