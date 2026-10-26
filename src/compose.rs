//! Creating new pages from images, text files and RTF documents
//! ("Bilder einfügen", "Text einfügen", "RTF einfügen").

use lopdf::{Dictionary, Document, Object, ObjectId, Stream};
use serde::{Deserialize, Serialize};

use crate::appearance::{StdFont, num, win_ansi};
use crate::error::{Error, Result};
use crate::geometry::RectF;
use crate::model::Rgba;
use crate::pdf_edit::{RgbaImage, image_xobject, rect_array, text_string};

const MM: f32 = 72.0 / 25.4;

/// A generated PDF plus (page index, title) pairs for bookmarks.
pub type Composed = (Vec<u8>, Vec<(usize, String)>);
/// Upper bound for generated pages.
const MAX_PAGES: usize = 20_000;

// ---------------------------------------------------------------------------
// A tiny PDF writer

struct Builder {
    doc: Document,
    pages_id: ObjectId,
    kids: Vec<Object>,
}

impl Builder {
    fn new() -> Self {
        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        Self { doc, pages_id, kids: Vec::new() }
    }

    fn add_page(&mut self, w: f32, h: f32, content: String, resources: Dictionary) -> Result<()> {
        if self.kids.len() >= MAX_PAGES {
            return Err(Error::Invalid("too many pages".into()));
        }
        let mut stream = Stream::new(Dictionary::new(), content.into_bytes());
        stream.compress()?;
        let content_id = self.doc.add_object(stream);
        let mut page = Dictionary::new();
        page.set("Type", Object::Name(b"Page".to_vec()));
        page.set("Parent", Object::Reference(self.pages_id));
        page.set("MediaBox", rect_array(&RectF::new(0.0, 0.0, w, h)));
        page.set("Resources", Object::Dictionary(resources));
        page.set("Contents", Object::Reference(content_id));
        let id = self.doc.add_object(page);
        self.kids.push(Object::Reference(id));
        Ok(())
    }

    fn finish(mut self, title: &str) -> Result<Vec<u8>> {
        let mut pages = Dictionary::new();
        pages.set("Type", Object::Name(b"Pages".to_vec()));
        pages.set("Count", Object::Integer(self.kids.len() as i64));
        pages.set("Kids", Object::Array(self.kids));
        self.doc.objects.insert(self.pages_id, Object::Dictionary(pages));
        let mut info = Dictionary::new();
        info.set("Title", text_string(title));
        info.set("Producer", Object::string_literal(format!("MrPDF {}", env!("CARGO_PKG_VERSION"))));
        let info_id = self.doc.add_object(info);
        let mut catalog = Dictionary::new();
        catalog.set("Type", Object::Name(b"Catalog".to_vec()));
        catalog.set("Pages", Object::Reference(self.pages_id));
        let catalog_id = self.doc.add_object(catalog);
        self.doc.trailer.set("Root", Object::Reference(catalog_id));
        self.doc.trailer.set("Info", Object::Reference(info_id));
        let mut out = Vec::new();
        self.doc.save_to(&mut out).map_err(|e| Error::Structure(e.to_string()))?;
        Ok(out)
    }
}

fn font_dict(font: StdFont) -> Object {
    let mut f = Dictionary::new();
    f.set("Type", Object::Name(b"Font".to_vec()));
    f.set("Subtype", Object::Name(b"Type1".to_vec()));
    f.set("BaseFont", Object::Name(font.base_font().as_bytes().to_vec()));
    f.set("Encoding", Object::Name(b"WinAnsiEncoding".to_vec()));
    Object::Dictionary(f)
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

fn text_op(font: &str, size: f32, color: Rgba, x: f32, y: f32, text: &[u8]) -> String {
    let [r, g, b] = color.to_pdf();
    format!(
        "BT /{font} {} Tf {} {} {} rg {} {} Td ({}) Tj ET\n",
        num(size),
        num(r),
        num(g),
        num(b),
        num(x),
        num(y),
        escape(text)
    )
}

/// Splits text into lines no wider than `width` (hard cut for long words).
fn wrap(text: &str, font: StdFont, size: f32, width: f32) -> Vec<Vec<u8>> {
    let mut lines = Vec::new();
    let mut current: Vec<u8> = Vec::new();
    for word in text.split(' ') {
        let word = win_ansi(word);
        let mut candidate = current.clone();
        if !candidate.is_empty() {
            candidate.push(b' ');
        }
        candidate.extend_from_slice(&word);
        if font.text_width(&candidate, size) <= width || current.is_empty() {
            current = candidate;
            while font.text_width(&current, size) > width && current.len() > 1 {
                let mut cut = current.len() - 1;
                while cut > 1 && font.text_width(&current[..cut], size) > width {
                    cut -= 1;
                }
                lines.push(current[..cut].to_vec());
                current = current[cut..].to_vec();
            }
        } else {
            lines.push(std::mem::take(&mut current));
            current = word;
        }
    }
    lines.push(current);
    lines
}

// ---------------------------------------------------------------------------
// Images

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PageSizeMode {
    /// Page size follows each image ("Nach Bild").
    ByImage,
    #[default]
    Standard,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Fit {
    /// Shrink large images to the cell ("Auf Zelle reduzieren").
    #[default]
    Shrink,
    /// Scale every image to the cell ("An Zelle anpassen").
    Fill,
    /// Keep the original size.
    Original,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Align {
    Start,
    #[default]
    Center,
    End,
}

/// Settings of "Einstellungen für Bild-zu-PDF-Umwandlung".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ImageLayout {
    pub page_mode: PageSizeMode,
    /// Page size in points (portrait) for Standard/Custom.
    pub page_size: (f32, f32),
    pub landscape: bool,
    /// Turn each page to the orientation of its image.
    pub auto_rotate: bool,
    /// Margins left, right, top, bottom (points).
    pub margins: [f32; 4],
    pub columns: u32,
    pub rows: u32,
    pub h_gap: f32,
    pub v_gap: f32,
    pub fit: Fit,
    pub keep_aspect: bool,
    pub h_align: Align,
    pub v_align: Align,
    /// Caption pattern (`%[FileName]`), if captions are wanted.
    pub caption: Option<String>,
    pub caption_above: bool,
    pub caption_size: f32,
    /// Add a bookmark per image (named like the caption).
    pub bookmarks: bool,
    /// Resolution assumed for images (pixels per inch) for "Nach Bild".
    pub image_dpi: f32,
    /// Downsample images above this resolution (to `downsample_to`).
    pub downsample_above: Option<f32>,
    pub downsample_to: f32,
    /// JPEG compression for colour and grey images (quality 1..100);
    /// `None` = lossless.
    pub jpeg_quality: Option<u8>,
}

impl Default for ImageLayout {
    fn default() -> Self {
        Self {
            page_mode: PageSizeMode::Standard,
            page_size: (595.28, 841.89),
            landscape: false,
            auto_rotate: false,
            margins: [0.0; 4],
            columns: 1,
            rows: 1,
            h_gap: 4.23 * MM,
            v_gap: 4.23 * MM,
            fit: Fit::Shrink,
            keep_aspect: true,
            h_align: Align::Center,
            v_align: Align::Center,
            caption: None,
            caption_above: false,
            caption_size: 9.0,
            bookmarks: false,
            image_dpi: 150.0,
            downsample_above: None,
            downsample_to: 150.0,
            jpeg_quality: Some(75),
        }
    }
}

/// Fills `%[FileName]`, `%[Page]` and `%[Index]` in a caption pattern.
pub fn expand_caption(pattern: &str, file_name: &str, index: usize) -> String {
    let stem = file_name.rsplit_once('.').map_or(file_name, |(s, _)| s);
    pattern
        .replace("%[FileName]", stem)
        .replace("%[FullName]", file_name)
        .replace("%[Index]", &(index + 1).to_string())
}

fn encode_image(doc: &mut Document, img: &RgbaImage, jpeg_quality: Option<u8>) -> Result<ObjectId> {
    let (w, h) = (img.width, img.height);
    let has_alpha = img.rgba.as_chunks::<4>().0.iter().any(|p| p[3] != 255);
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    let mut alpha = Vec::with_capacity((w * h) as usize);
    for px in img.rgba.as_chunks::<4>().0 {
        rgb.extend_from_slice(&px[..3]);
        alpha.push(px[3]);
    }
    let smask = if has_alpha { Some(doc.add_object(image_xobject(alpha, w, h, true, None)?)) } else { None };
    match jpeg_quality {
        Some(q) => {
            let mut data = Vec::new();
            let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut data, q.clamp(1, 100));
            image::ImageEncoder::write_image(encoder, &rgb, w, h, image::ExtendedColorType::Rgb8)
                .map_err(|e| Error::Invalid(e.to_string()))?;
            let mut dict = Dictionary::new();
            dict.set("Type", Object::Name(b"XObject".to_vec()));
            dict.set("Subtype", Object::Name(b"Image".to_vec()));
            dict.set("Width", Object::Integer(i64::from(w)));
            dict.set("Height", Object::Integer(i64::from(h)));
            dict.set("ColorSpace", Object::Name(b"DeviceRGB".to_vec()));
            dict.set("BitsPerComponent", Object::Integer(8));
            dict.set("Filter", Object::Name(b"DCTDecode".to_vec()));
            if let Some(m) = smask {
                dict.set("SMask", Object::Reference(m));
            }
            Ok(doc.add_object(Stream::new(dict, data)))
        }
        None => Ok(doc.add_object(image_xobject(rgb, w, h, false, smask)?)),
    }
}

fn resample(img: &RgbaImage, w: u32, h: u32) -> RgbaImage {
    let buffer = image::RgbaImage::from_raw(img.width, img.height, img.rgba.clone()).unwrap_or_default();
    let out = image::imageops::resize(&buffer, w.max(1), h.max(1), image::imageops::FilterType::Triangle);
    RgbaImage { width: out.width(), height: out.height(), rgba: out.into_raw() }
}

/// Lays out images on pages. `images` are (file name, pixels). Returns the
/// PDF and (page index, caption) for bookmarks.
pub fn images_to_pdf(images: &[(String, RgbaImage)], layout: &ImageLayout) -> Result<Composed> {
    if images.is_empty() {
        return Err(Error::Invalid("no images".into()));
    }
    let mut b = Builder::new();
    let mut marks = Vec::new();
    let per_page = if layout.page_mode == PageSizeMode::ByImage {
        1
    } else {
        (layout.columns.clamp(1, 20) * layout.rows.clamp(1, 20)) as usize
    };
    let caption_h = if layout.caption.is_some() { layout.caption_size * 1.6 } else { 0.0 };
    let [ml, mr, mt, mb] = layout.margins;
    for (chunk_index, chunk) in images.chunks(per_page).enumerate() {
        let page_index = chunk_index;
        // Page size.
        let first = &chunk[0].1;
        let px_to_pt = 72.0 / layout.image_dpi.clamp(10.0, 2400.0);
        let (mut pw, mut ph) = match layout.page_mode {
            PageSizeMode::ByImage => (
                first.width as f32 * px_to_pt + ml + mr,
                first.height as f32 * px_to_pt + mt + mb + caption_h,
            ),
            _ => layout.page_size,
        };
        if layout.page_mode != PageSizeMode::ByImage {
            let landscape = if layout.auto_rotate { first.width > first.height } else { layout.landscape };
            if landscape != (pw > ph) {
                std::mem::swap(&mut pw, &mut ph);
            }
        }
        let (cols, rows) =
            if per_page == 1 { (1, 1) } else { (layout.columns.clamp(1, 20), layout.rows.clamp(1, 20)) };
        let cell_w = ((pw - ml - mr - (cols - 1) as f32 * layout.h_gap) / cols as f32).max(1.0);
        let cell_h = ((ph - mt - mb - (rows - 1) as f32 * layout.v_gap) / rows as f32).max(1.0);
        let mut content = String::new();
        let mut xobjects = Dictionary::new();
        for (i, (name, img)) in chunk.iter().enumerate() {
            let (col, row) = (i as u32 % cols, i as u32 / cols);
            let cx = ml + col as f32 * (cell_w + layout.h_gap);
            let top = ph - mt - row as f32 * (cell_h + layout.v_gap);
            let avail_h = (cell_h - caption_h).max(1.0);
            let nat_w = img.width as f32 * px_to_pt;
            let nat_h = img.height as f32 * px_to_pt;
            let (mut w, mut h) = match layout.fit {
                Fit::Original => (nat_w, nat_h),
                Fit::Shrink if nat_w <= cell_w && nat_h <= avail_h => (nat_w, nat_h),
                _ => {
                    if layout.keep_aspect {
                        let s = (cell_w / nat_w).min(avail_h / nat_h);
                        (nat_w * s, nat_h * s)
                    } else {
                        (cell_w, avail_h)
                    }
                }
            };
            w = w.max(0.5);
            h = h.max(0.5);
            let x = match layout.h_align {
                Align::Start => cx,
                Align::Center => cx + (cell_w - w) / 2.0,
                Align::End => cx + cell_w - w,
            };
            let image_top = if layout.caption_above { top - caption_h } else { top };
            let y = match layout.v_align {
                Align::Start => image_top - h,
                Align::Center => image_top - (avail_h + h) / 2.0,
                Align::End => image_top - avail_h,
            };
            // Downsample above the limit (effective resolution on the page).
            let mut pixels = img.clone();
            if let Some(limit) = layout.downsample_above {
                let dpi = img.width as f32 / (w / 72.0);
                if dpi > limit {
                    let f = layout.downsample_to.min(limit) / dpi;
                    pixels = resample(img, (img.width as f32 * f) as u32, (img.height as f32 * f) as u32);
                }
            }
            let id = encode_image(&mut b.doc, &pixels, layout.jpeg_quality)?;
            let im_name = format!("Im{i}");
            xobjects.set(im_name.clone(), Object::Reference(id));
            content.push_str(&format!(
                "q {} 0 0 {} {} {} cm /{im_name} Do Q\n",
                num(w),
                num(h),
                num(x),
                num(y)
            ));
            if let Some(pattern) = &layout.caption {
                let text = expand_caption(pattern, name, page_index * per_page + i);
                let bytes = win_ansi(&text);
                let tw = StdFont::Helvetica.text_width(&bytes, layout.caption_size);
                let tx = cx + (cell_w - tw).max(0.0) / 2.0;
                let ty = if layout.caption_above {
                    top - layout.caption_size * 1.1
                } else {
                    y - layout.caption_size * 1.3
                };
                content.push_str(&text_op("H", layout.caption_size, Rgba::BLACK, tx, ty, &bytes));
                if layout.bookmarks {
                    marks.push((page_index, text));
                }
            } else if layout.bookmarks {
                marks.push((page_index, expand_caption("%[FileName]", name, page_index * per_page + i)));
            }
        }
        let mut resources = Dictionary::new();
        resources.set("XObject", Object::Dictionary(xobjects));
        let mut fonts = Dictionary::new();
        fonts.set("H", font_dict(StdFont::Helvetica));
        resources.set("Font", Object::Dictionary(fonts));
        b.add_page(pw, ph, content, resources)?;
    }
    Ok((b.finish("Images")?, marks))
}

// ---------------------------------------------------------------------------
// Text

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TextEncoding {
    #[default]
    Auto,
    Utf8,
    Windows1252,
    Utf16Le,
    Utf16Be,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ParagraphMode {
    /// Every line break starts a new paragraph.
    #[default]
    EachLine,
    /// Only empty lines separate paragraphs; other breaks are joined.
    EmptyLine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum JoinMode {
    /// Each file starts on a new page.
    #[default]
    NewPage,
    /// Files follow each other directly.
    Continuous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TextFont {
    #[default]
    Helvetica,
    Courier,
}

impl TextFont {
    fn std(self) -> StdFont {
        match self {
            TextFont::Helvetica => StdFont::Helvetica,
            TextFont::Courier => StdFont::Courier,
        }
    }
}

/// Settings of "Textdateien in PDF umwandeln".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextLayout {
    pub page_size: (f32, f32),
    pub landscape: bool,
    /// Margins left, right, top, bottom (points).
    pub margins: [f32; 4],
    pub font: TextFont,
    pub font_size: f32,
    pub line_spacing: f32,
    pub color: Rgba,
    pub columns: u32,
    pub column_gap: f32,
    pub column_line: bool,
    pub line_width: f32,
    pub line_color: Rgba,
    /// Title header pattern (`%[FileName]`), if wanted.
    pub header: Option<String>,
    pub header_size: f32,
    pub bookmark_per_file: bool,
    /// Line below the header.
    pub header_line: bool,
    /// Line above the header.
    pub header_line_above: bool,
    pub encoding: TextEncoding,
    pub paragraphs: ParagraphMode,
    pub join: JoinMode,
}

impl Default for TextLayout {
    fn default() -> Self {
        Self {
            page_size: (595.28, 841.89),
            landscape: false,
            margins: [9.98 * MM; 4],
            font: TextFont::Helvetica,
            font_size: 10.0,
            line_spacing: 1.2,
            color: Rgba::BLACK,
            columns: 1,
            column_gap: 7.06 * MM,
            column_line: false,
            line_width: 0.35 * MM,
            line_color: Rgba::BLACK,
            header: None,
            header_size: 12.0,
            bookmark_per_file: false,
            header_line: false,
            header_line_above: false,
            encoding: TextEncoding::Auto,
            paragraphs: ParagraphMode::EachLine,
            join: JoinMode::NewPage,
        }
    }
}

fn cp1252(b: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}',
        '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
    ];
    match b {
        0x80..=0x9f => HIGH[usize::from(b - 0x80)],
        _ => char::from(b),
    }
}

/// Decodes a text file.
pub fn decode_text(bytes: &[u8], encoding: TextEncoding) -> String {
    let utf16 = |data: &[u8], le: bool| {
        let units: Vec<u16> = data
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) })
            .collect();
        String::from_utf16_lossy(&units)
    };
    match encoding {
        TextEncoding::Utf8 => {
            String::from_utf8_lossy(bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes)).into_owned()
        }
        TextEncoding::Windows1252 => bytes.iter().map(|b| cp1252(*b)).collect(),
        TextEncoding::Utf16Le => utf16(bytes.strip_prefix(b"\xFF\xFE").unwrap_or(bytes), true),
        TextEncoding::Utf16Be => utf16(bytes.strip_prefix(b"\xFE\xFF").unwrap_or(bytes), false),
        TextEncoding::Auto => {
            if let Some(rest) = bytes.strip_prefix(b"\xEF\xBB\xBF") {
                String::from_utf8_lossy(rest).into_owned()
            } else if bytes.starts_with(b"\xFF\xFE") {
                utf16(&bytes[2..], true)
            } else if bytes.starts_with(b"\xFE\xFF") {
                utf16(&bytes[2..], false)
            } else {
                match std::str::from_utf8(bytes) {
                    Ok(s) => s.to_string(),
                    Err(_) => bytes.iter().map(|b| cp1252(*b)).collect(),
                }
            }
        }
    }
}

/// Splits text into paragraphs according to `mode`.
pub fn paragraphs(text: &str, mode: ParagraphMode) -> Vec<String> {
    let text = text.replace("\r\n", "\n").replace('\r', "\n").replace('\t', "    ");
    match mode {
        ParagraphMode::EachLine => text.split('\n').map(str::to_string).collect(),
        ParagraphMode::EmptyLine => {
            let mut out = Vec::new();
            let mut current = String::new();
            for line in text.split('\n') {
                if line.trim().is_empty() {
                    if !current.is_empty() {
                        out.push(std::mem::take(&mut current));
                    }
                    out.push(String::new());
                } else {
                    if !current.is_empty() {
                        current.push(' ');
                    }
                    current.push_str(line.trim());
                }
            }
            if !current.is_empty() {
                out.push(current);
            }
            out
        }
    }
}

/// Lays out text documents (name, text) on pages. Returns the PDF and
/// (page index, title) of the first page of every document.
pub fn text_to_pdf(docs: &[(String, String)], layout: &TextLayout) -> Result<Composed> {
    let (mut pw, mut ph) = layout.page_size;
    if layout.landscape != (pw > ph) {
        std::mem::swap(&mut pw, &mut ph);
    }
    let [ml, mr, mt, mb] = layout.margins;
    let font = layout.font.std();
    let size = layout.font_size.clamp(4.0, 72.0);
    let lead = size * layout.line_spacing.clamp(0.8, 4.0);
    let cols = layout.columns.clamp(1, 6);
    let col_w = ((pw - ml - mr - (cols - 1) as f32 * layout.column_gap) / cols as f32).max(20.0);
    let header_h = if layout.header.is_some() { layout.header_size * 1.8 } else { 0.0 };
    let body_top = ph - mt - header_h;
    let mut b = Builder::new();
    let mut marks = Vec::new();

    let mut pages: Vec<(String, String)> = Vec::new(); // (content, header title)
    let mut content = String::new();
    let mut col = 0u32;
    let mut y = body_top;
    let mut started = false;
    for (doc_index, (name, text)) in docs.iter().enumerate() {
        let title = expand_caption(layout.header.as_deref().unwrap_or("%[FileName]"), name, doc_index);
        if started && layout.join == JoinMode::NewPage {
            pages.push((std::mem::take(&mut content), docs[doc_index - 1].0.clone()));
            col = 0;
            y = body_top;
        }
        started = true;
        marks.push((pages.len(), title));
        for para in paragraphs(text, layout.paragraphs) {
            let lines = if para.is_empty() { vec![Vec::new()] } else { wrap(&para, font, size, col_w) };
            for line in lines {
                if y - lead < mb {
                    col += 1;
                    y = body_top;
                    if col >= cols {
                        pages.push((std::mem::take(&mut content), name.clone()));
                        col = 0;
                        if pages.len() >= MAX_PAGES {
                            return Err(Error::Invalid("too many pages".into()));
                        }
                    }
                }
                y -= lead;
                if !line.is_empty() {
                    let x = ml + col as f32 * (col_w + layout.column_gap);
                    content.push_str(&text_op("F", size, layout.color, x, y + (lead - size) / 2.0, &line));
                }
            }
        }
    }
    let last_name = docs.last().map(|d| d.0.clone()).unwrap_or_default();
    pages.push((content, last_name));

    for (index, (body, name)) in pages.into_iter().enumerate() {
        let mut page = String::new();
        if let Some(pattern) = &layout.header {
            let title = win_ansi(&expand_caption(pattern, &name, index));
            page.push_str(&text_op(
                "FB",
                layout.header_size,
                layout.color,
                ml,
                ph - mt - layout.header_size,
                &title,
            ));
            let [r, g, bl] = layout.line_color.to_pdf();
            let stroke = format!("{} {} {} RG {} w ", num(r), num(g), num(bl), num(layout.line_width));
            if layout.header_line {
                let ly = ph - mt - header_h + layout.header_size * 0.3;
                page.push_str(&format!(
                    "{stroke}{} {} m {} {} l S\n",
                    num(ml),
                    num(ly),
                    num(pw - mr),
                    num(ly)
                ));
            }
            if layout.header_line_above {
                let ly = ph - mt + 2.0;
                page.push_str(&format!(
                    "{stroke}{} {} m {} {} l S\n",
                    num(ml),
                    num(ly),
                    num(pw - mr),
                    num(ly)
                ));
            }
        }
        if layout.column_line && cols > 1 {
            let [r, g, bl] = layout.line_color.to_pdf();
            for c in 1..cols {
                let x = ml + c as f32 * (col_w + layout.column_gap) - layout.column_gap / 2.0;
                page.push_str(&format!(
                    "{} {} {} RG {} w {} {} m {} {} l S\n",
                    num(r),
                    num(g),
                    num(bl),
                    num(layout.line_width),
                    num(x),
                    num(body_top),
                    num(x),
                    num(mb)
                ));
            }
        }
        page.push_str(&body);
        let mut fonts = Dictionary::new();
        fonts.set("F", font_dict(font));
        fonts.set(
            "FB",
            font_dict(if font == StdFont::Courier { StdFont::Courier } else { StdFont::HelveticaBold }),
        );
        let mut resources = Dictionary::new();
        resources.set("Font", Object::Dictionary(fonts));
        b.add_page(pw, ph, page, resources)?;
    }
    if !layout.bookmark_per_file {
        marks.clear();
    }
    Ok((b.finish("Text")?, marks))
}

// ---------------------------------------------------------------------------
// RTF

/// Extracts the plain text of an RTF document (paragraphs become line
/// breaks; formatting, pictures and fields are dropped).
pub fn rtf_to_text(rtf: &[u8]) -> Result<String> {
    if !rtf.starts_with(b"{\\rtf") {
        return Err(Error::Invalid("not an RTF document".into()));
    }
    // Destinations whose content is not document text.
    const SKIP: [&str; 14] = [
        "fonttbl",
        "colortbl",
        "stylesheet",
        "info",
        "pict",
        "object",
        "header",
        "footer",
        "footnote",
        "listtable",
        "listoverridetable",
        "rsidtbl",
        "generator",
        "themedata",
    ];
    let mut out = String::new();
    // Per group: (skipping, unicode skip count).
    let mut stack: Vec<(bool, usize)> = vec![(false, 1)];
    let mut skip_chars = 0usize;
    let mut i = 0;
    while i < rtf.len() {
        let skipping = stack.last().is_some_and(|g| g.0);
        match rtf[i] {
            b'{' => {
                if stack.len() > 256 {
                    return Err(Error::Invalid("RTF nesting too deep".into()));
                }
                let parent = *stack.last().unwrap_or(&(false, 1));
                stack.push(parent);
                // "{\*\destination" is an ignorable destination.
                if rtf[i + 1..].starts_with(b"\\*")
                    && let Some(g) = stack.last_mut()
                {
                    g.0 = true;
                }
                i += 1;
            }
            b'}' => {
                stack.pop();
                if stack.is_empty() {
                    break;
                }
                i += 1;
            }
            b'\\' => {
                i += 1;
                let Some(&c) = rtf.get(i) else { break };
                if c.is_ascii_alphabetic() {
                    let start = i;
                    while i < rtf.len() && rtf[i].is_ascii_alphabetic() {
                        i += 1;
                    }
                    let word = std::str::from_utf8(&rtf[start..i]).unwrap_or("");
                    let num_start = i;
                    if i < rtf.len() && (rtf[i] == b'-' || rtf[i].is_ascii_digit()) {
                        i += 1;
                        while i < rtf.len() && rtf[i].is_ascii_digit() {
                            i += 1;
                        }
                    }
                    let param: Option<i32> =
                        std::str::from_utf8(&rtf[num_start..i]).ok().and_then(|s| s.parse().ok());
                    if i < rtf.len() && rtf[i] == b' ' {
                        i += 1;
                    }
                    if SKIP.contains(&word) {
                        if let Some(g) = stack.last_mut() {
                            g.0 = true;
                        }
                        continue;
                    }
                    if skipping {
                        continue;
                    }
                    match word {
                        "par" | "line" | "sect" | "page" => out.push('\n'),
                        "tab" => out.push('\t'),
                        "emdash" => out.push('—'),
                        "endash" => out.push('–'),
                        "bullet" => out.push('•'),
                        "lquote" => out.push('‘'),
                        "rquote" => out.push('’'),
                        "ldblquote" => out.push('“'),
                        "rdblquote" => out.push('”'),
                        "uc" => {
                            if let Some(g) = stack.last_mut() {
                                g.1 = param.unwrap_or(1).clamp(0, 10) as usize;
                            }
                        }
                        "u" => {
                            if let Some(p) = param {
                                let code = if p < 0 { (p + 65536) as u32 } else { p as u32 };
                                out.push(char::from_u32(code).unwrap_or('?'));
                                skip_chars = stack.last().map_or(1, |g| g.1);
                            }
                        }
                        _ => {}
                    }
                } else if c == b'\'' {
                    let hex = rtf.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok());
                    i += 3;
                    if let Some(v) = hex.and_then(|h| u8::from_str_radix(h, 16).ok())
                        && !skipping
                    {
                        if skip_chars > 0 {
                            skip_chars -= 1;
                        } else {
                            out.push(cp1252(v));
                        }
                    }
                } else {
                    // Escaped characters: \\ \{ \} and control symbols.
                    if !skipping {
                        match c {
                            b'\\' | b'{' | b'}' => out.push(char::from(c)),
                            b'~' => out.push('\u{a0}'),
                            b'_' => out.push('-'),
                            _ => {}
                        }
                    }
                    i += 1;
                }
            }
            b'\r' | b'\n' => i += 1,
            c => {
                if !skipping {
                    if skip_chars > 0 {
                        skip_chars -= 1;
                    } else {
                        out.push(cp1252(c));
                    }
                }
                i += 1;
            }
        }
    }
    Ok(out.trim_end().to_string())
}

// ---------------------------------------------------------------------------
// Markdown

/// A styled paragraph of a generated document.
#[derive(Debug, Clone, PartialEq)]
struct Para {
    /// Font resource: "F" (body), "FB" (bold) or "FC" (fixed width).
    font: &'static str,
    size: f32,
    indent: f32,
    space_before: f32,
    /// Bullet or number drawn left of the first line.
    prefix: Option<String>,
    /// Horizontal line instead of text.
    rule: bool,
    text: String,
}

impl Para {
    fn body(text: String, size: f32) -> Self {
        Self { font: "F", size, indent: 0.0, space_before: size * 0.5, prefix: None, rule: false, text }
    }
}

/// Removes inline Markdown markup (emphasis, code spans, links, images and
/// escapes) and keeps the visible text.
fn strip_inline(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\\' if i + 1 < chars.len() && chars[i + 1].is_ascii_punctuation() => {
                out.push(chars[i + 1]);
                i += 2;
            }
            '*' | '`' => i += 1,
            '_' if (i + 1 < chars.len() && chars[i + 1] == '_')
                || i == 0
                || i + 1 == chars.len()
                || !chars[i - 1].is_alphanumeric()
                || !chars[i + 1].is_alphanumeric() =>
            {
                i += 1
            }
            '!' if chars.get(i + 1) == Some(&'[') => i += 1,
            '[' => {
                // [text](url) → text
                let close = chars[i..].iter().position(|&c| c == ']').map(|p| p + i);
                match close {
                    Some(end) if chars.get(end + 1) == Some(&'(') => {
                        let label: String = chars[i + 1..end].iter().collect();
                        out.push_str(&strip_inline(&label));
                        let after = chars[end + 1..].iter().position(|&c| c == ')').map(|p| p + end + 1);
                        i = after.map_or(chars.len(), |p| p + 1);
                    }
                    _ => {
                        out.push(c);
                        i += 1;
                    }
                }
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// Turns Markdown into styled paragraphs (headings, lists, quotes, code
/// blocks, rules). `size` is the body font size.
fn markdown_paras(text: &str, size: f32) -> Vec<Para> {
    let text = text.replace("\r\n", "\n").replace('\r', "\n").replace('\t', "    ");
    let mut paras: Vec<Para> = Vec::new();
    let mut current: Option<Para> = None;
    let mut code = false;
    let flush = |current: &mut Option<Para>, paras: &mut Vec<Para>| {
        if let Some(mut p) = current.take() {
            p.text = strip_inline(p.text.trim());
            paras.push(p);
        }
    };
    for line in text.split('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            flush(&mut current, &mut paras);
            code = !code;
            continue;
        }
        if code {
            paras.push(Para {
                font: "FC",
                indent: 12.0,
                space_before: 0.0,
                text: line.to_string(),
                ..Para::body(String::new(), size * 0.9)
            });
            continue;
        }
        if trimmed.is_empty() {
            flush(&mut current, &mut paras);
            continue;
        }
        let level = trimmed.chars().take_while(|&c| c == '#').count();
        if (1..=6).contains(&level) && trimmed[level..].starts_with(' ') {
            flush(&mut current, &mut paras);
            const SCALE: [f32; 6] = [2.0, 1.6, 1.35, 1.2, 1.1, 1.0];
            let s = size * SCALE[level - 1];
            let title = trimmed[level..].trim().trim_end_matches('#').trim();
            paras.push(Para {
                font: "FB",
                space_before: s * 0.7,
                text: strip_inline(title),
                ..Para::body(String::new(), s)
            });
            continue;
        }
        let compact: String = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
        if compact.len() >= 3
            && (compact.chars().all(|c| c == '-')
                || compact.chars().all(|c| c == '*')
                || compact.chars().all(|c| c == '_'))
        {
            flush(&mut current, &mut paras);
            paras.push(Para { rule: true, ..Para::body(String::new(), size) });
            continue;
        }
        let depth = (line.len() - line.trim_start().len()) / 2;
        let indent = 14.0 * (depth as f32 + 1.0);
        let bullet = ["- ", "* ", "+ "].iter().find_map(|m| trimmed.strip_prefix(m));
        let number = trimmed
            .split_once(". ")
            .filter(|(n, _)| !n.is_empty() && n.len() <= 4 && n.chars().all(|c| c.is_ascii_digit()));
        if let Some(rest) = bullet {
            flush(&mut current, &mut paras);
            current = Some(Para {
                indent,
                space_before: size * 0.2,
                prefix: Some("•".into()),
                text: rest.to_string(),
                ..Para::body(String::new(), size)
            });
            continue;
        }
        if let Some((n, rest)) = number {
            flush(&mut current, &mut paras);
            current = Some(Para {
                indent,
                space_before: size * 0.2,
                prefix: Some(format!("{n}.")),
                text: rest.to_string(),
                ..Para::body(String::new(), size)
            });
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix('>') {
            let rest = rest.trim_start_matches('>').trim();
            match &mut current {
                Some(p) if p.indent == 18.0 && p.prefix.is_none() => {
                    p.text.push(' ');
                    p.text.push_str(rest);
                }
                _ => {
                    flush(&mut current, &mut paras);
                    current = Some(Para {
                        indent: 18.0,
                        text: rest.to_string(),
                        ..Para::body(String::new(), size)
                    });
                }
            }
            continue;
        }
        if trimmed.starts_with('|') {
            // Table rows are kept as fixed-width lines.
            flush(&mut current, &mut paras);
            let cells: Vec<&str> = trimmed.trim_matches('|').split('|').map(str::trim).collect();
            if cells.iter().all(|c| !c.is_empty() && c.chars().all(|ch| matches!(ch, '-' | ':'))) {
                continue;
            }
            paras.push(Para {
                font: "FC",
                space_before: 0.0,
                text: strip_inline(&cells.join(" | ")),
                ..Para::body(String::new(), size * 0.9)
            });
            continue;
        }
        // Continuation of a paragraph or list item.
        match &mut current {
            Some(p) => {
                p.text.push(' ');
                p.text.push_str(trimmed);
            }
            None => current = Some(Para::body(trimmed.to_string(), size)),
        }
    }
    flush(&mut current, &mut paras);
    paras
}

/// Lays out styled paragraphs on pages; returns the page index at which
/// each document starts.
fn paras_to_pdf(docs: &[(String, Vec<Para>)], layout: &TextLayout, title: &str) -> Result<Composed> {
    let (mut pw, mut ph) = layout.page_size;
    if layout.landscape != (pw > ph) {
        std::mem::swap(&mut pw, &mut ph);
    }
    let [ml, mr, mt, mb] = layout.margins;
    let width = (pw - ml - mr).max(40.0);
    let body = layout.font.std();
    let font_of = |name: &str| match name {
        "FB" if body == StdFont::Courier => StdFont::Courier,
        "FB" => StdFont::HelveticaBold,
        "FC" => StdFont::Courier,
        _ => body,
    };
    let spacing = layout.line_spacing.clamp(0.8, 4.0);
    let mut pages: Vec<String> = Vec::new();
    let mut content = String::new();
    let mut y = ph - mt;
    let mut marks = Vec::new();
    for (doc_index, (name, paras)) in docs.iter().enumerate() {
        if doc_index > 0 && layout.join == JoinMode::NewPage {
            pages.push(std::mem::take(&mut content));
            y = ph - mt;
        }
        marks.push((pages.len(), expand_caption("%[FileName]", name, doc_index)));
        for para in paras {
            let size = para.size.clamp(4.0, 96.0);
            let lead = size * spacing;
            let at_top = (y - (ph - mt)).abs() < 0.01;
            if !at_top {
                y -= para.space_before;
            }
            let lines = if para.rule {
                Vec::new()
            } else {
                wrap(&para.text, font_of(para.font), size, (width - para.indent).max(20.0))
            };
            let needed = if para.rule { size } else { lead * lines.len().max(1) as f32 };
            for (i, line) in lines.iter().enumerate() {
                if y - lead < mb {
                    pages.push(std::mem::take(&mut content));
                    if pages.len() >= MAX_PAGES {
                        return Err(Error::Invalid("too many pages".into()));
                    }
                    y = ph - mt;
                }
                y -= lead;
                let x = ml + para.indent;
                let base = y + (lead - size) / 2.0;
                if i == 0
                    && let Some(prefix) = &para.prefix
                {
                    content.push_str(&text_op("F", size, layout.color, x - 12.0, base, &win_ansi(prefix)));
                }
                content.push_str(&text_op(para.font, size, layout.color, x, base, line));
            }
            if para.rule {
                if y - needed < mb {
                    pages.push(std::mem::take(&mut content));
                    y = ph - mt;
                }
                y -= needed / 2.0;
                let [r, g, b] = layout.line_color.to_pdf();
                content.push_str(&format!(
                    "{} {} {} RG {} w {} {} m {} {} l S\n",
                    num(r),
                    num(g),
                    num(b),
                    num(layout.line_width),
                    num(ml),
                    num(y),
                    num(pw - mr),
                    num(y)
                ));
                y -= needed / 2.0;
            }
        }
    }
    pages.push(content);
    let mut b = Builder::new();
    for page in pages {
        let mut fonts = Dictionary::new();
        fonts.set("F", font_dict(body));
        fonts.set("FB", font_dict(font_of("FB")));
        fonts.set("FC", font_dict(StdFont::Courier));
        let mut resources = Dictionary::new();
        resources.set("Font", Object::Dictionary(fonts));
        b.add_page(pw, ph, page, resources)?;
    }
    if !layout.bookmark_per_file {
        marks.clear();
    }
    Ok((b.finish(title)?, marks))
}

/// "Neues Dokument → Aus Markdown": headings, paragraphs, lists, quotes,
/// code blocks, tables (as fixed-width lines) and rules. Inline markup is
/// removed; the body font and page settings come from `layout`.
pub fn markdown_to_pdf(docs: &[(String, String)], layout: &TextLayout) -> Result<Composed> {
    let size = layout.font_size.clamp(4.0, 72.0);
    let docs: Vec<(String, Vec<Para>)> =
        docs.iter().map(|(name, text)| (name.clone(), markdown_paras(text, size))).collect();
    paras_to_pdf(&docs, layout, "Markdown")
}

// ---------------------------------------------------------------------------
// CSV

/// Splits CSV text into rows of fields. The delimiter (`;`, `,` or tab) is
/// detected from the first line; quoted fields may contain delimiters,
/// doubled quotes and line breaks.
pub fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let first = text.lines().next().unwrap_or("");
    let count = |d: char| {
        let mut quoted = false;
        first
            .chars()
            .filter(|&c| {
                if c == '"' {
                    quoted = !quoted;
                }
                !quoted && c == d
            })
            .count()
    };
    let delimiter = [';', ',', '\t'].into_iter().max_by_key(|&d| (count(d), d == ';')).unwrap_or(';');
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    quoted = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => quoted = true,
            '\r' => {}
            '\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            c if c == delimiter => row.push(std::mem::take(&mut field)),
            c => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows.retain(|r| r.iter().any(|f| !f.trim().is_empty()));
    rows
}

/// "Neues Dokument → Aus CSV-Datei": a table with grid lines; the first row
/// is the (bold, shaded) header and is repeated on every page.
pub fn csv_to_pdf(docs: &[(String, String)], layout: &TextLayout) -> Result<Composed> {
    let (mut pw, mut ph) = layout.page_size;
    if layout.landscape != (pw > ph) {
        std::mem::swap(&mut pw, &mut ph);
    }
    let [ml, mr, mt, mb] = layout.margins;
    let width = (pw - ml - mr).max(40.0);
    let size = layout.font_size.clamp(4.0, 36.0);
    let lead = size * layout.line_spacing.clamp(0.8, 4.0);
    let pad = 3.0;
    let font = layout.font.std();
    let bold = if font == StdFont::Courier { StdFont::Courier } else { StdFont::HelveticaBold };
    let [lr, lg, lb] = layout.line_color.to_pdf();
    let grid = format!("{} {} {} RG 0.5 w\n", num(lr), num(lg), num(lb));
    let mut pages: Vec<String> = Vec::new();
    let mut marks = Vec::new();
    let mut content = String::new();
    for (doc_index, (name, text)) in docs.iter().enumerate() {
        if doc_index > 0 && !content.is_empty() {
            pages.push(std::mem::take(&mut content));
        }
        marks.push((pages.len(), expand_caption("%[FileName]", name, doc_index)));
        let rows = parse_csv(text);
        let cols = rows.iter().map(Vec::len).max().unwrap_or(0).clamp(1, 200);
        // Natural column widths, shrunk proportionally to the page width.
        let mut widths = vec![0.0f32; cols];
        for (r, row) in rows.iter().enumerate() {
            for (c, field) in row.iter().enumerate().take(cols) {
                let f = if r == 0 { bold } else { font };
                let w = f.text_width(&win_ansi(field.trim()), size) + 2.0 * pad;
                widths[c] = widths[c].max(w.min(width * 0.6));
            }
        }
        let min = (2.0 * pad + size).min(width / cols as f32);
        for w in &mut widths {
            *w = w.max(min);
        }
        let total: f32 = widths.iter().sum();
        if total > width {
            let flexible: f32 = widths.iter().map(|w| w - min).sum();
            let excess = total - width;
            for w in &mut widths {
                *w -= if flexible > 0.0 { (*w - min) * excess / flexible } else { 0.0 };
            }
        }
        let wrap_row = |row: &[String], f: StdFont| -> Vec<Vec<Vec<u8>>> {
            (0..cols)
                .map(|c| {
                    let field = row.get(c).map(String::as_str).unwrap_or("");
                    field
                        .split('\n')
                        .flat_map(|part| wrap(part.trim(), f, size, (widths[c] - 2.0 * pad).max(4.0)))
                        .collect()
                })
                .collect()
        };
        let max_lines = (((ph - mt - mb) / lead) as usize / 2).max(1);
        let draw_row = |content: &mut String, cells: &[Vec<Vec<u8>>], top: f32, header: bool| -> f32 {
            let lines = cells.iter().map(Vec::len).max().unwrap_or(1).clamp(1, max_lines);
            let h = lines as f32 * lead + 2.0 * pad;
            if header {
                content.push_str(&format!(
                    "0.9 0.9 0.9 rg {} {} {} {} re f\n",
                    num(ml),
                    num(top - h),
                    num(widths.iter().sum()),
                    num(h)
                ));
            }
            let mut x = ml;
            for (c, cell) in cells.iter().enumerate() {
                for (i, line) in cell.iter().take(lines).enumerate() {
                    let y = top - pad - (i + 1) as f32 * lead + (lead - size) / 2.0;
                    let f = if header { "FB" } else { "F" };
                    content.push_str(&text_op(f, size, layout.color, x + pad, y, line));
                }
                content.push_str(&format!(
                    "{grid}{} {} {} {} re S\n",
                    num(x),
                    num(top - h),
                    num(widths[c]),
                    num(h)
                ));
                x += widths[c];
            }
            h
        };
        let header = rows.first().map(|r| wrap_row(r, bold));
        let mut y = ph - mt;
        let mut fresh = true;
        for (index, row) in rows.iter().enumerate() {
            let is_header = index == 0;
            let cells = wrap_row(row, if is_header { bold } else { font });
            let lines = cells.iter().map(Vec::len).max().unwrap_or(1).clamp(1, max_lines);
            let h = lines as f32 * lead + 2.0 * pad;
            if y - h < mb && !fresh {
                pages.push(std::mem::take(&mut content));
                if pages.len() >= MAX_PAGES {
                    return Err(Error::Invalid("too many pages".into()));
                }
                y = ph - mt;
                if let Some(head) = &header {
                    y -= draw_row(&mut content, head, y, true);
                }
            }
            y -= draw_row(&mut content, &cells, y, is_header);
            fresh = false;
        }
    }
    pages.push(content);
    let mut b = Builder::new();
    for page in pages {
        let mut fonts = Dictionary::new();
        fonts.set("F", font_dict(font));
        fonts.set("FB", font_dict(bold));
        let mut resources = Dictionary::new();
        resources.set("Font", Object::Dictionary(fonts));
        b.add_page(pw, ph, page, resources)?;
    }
    if !layout.bookmark_per_file {
        marks.clear();
    }
    Ok((b.finish("CSV")?, marks))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rtf_text_is_extracted() {
        // The Unicode escape is assembled so no tool turns it into a character.
        let rtf = [
            &br"{\rtf1\ansi{\fonttbl{\f0 Arial;}}{\colortbl;\red0\green0\blue0;}\f0 Hallo {\b Welt}\par Gr\'fc\'dfe "[..],
            b"\\",
            b"u8364? Euro",
            br"\par{\*\generator X;}Ende}",
        ]
        .concat();
        assert_eq!(rtf_to_text(&rtf).unwrap(), "Hallo Welt\nGrüße € Euro\nEnde");
        assert!(rtf_to_text(b"plain").is_err());
    }

    #[test]
    fn encodings_are_detected() {
        assert_eq!(decode_text("Grüße".as_bytes(), TextEncoding::Auto), "Grüße");
        assert_eq!(decode_text(&[0x47, 0x72, 0xfc, 0xdf, 0x65, 0x80], TextEncoding::Auto), "Grüße€");
        assert_eq!(decode_text(&[0xff, 0xfe, 0x41, 0x00], TextEncoding::Auto), "A");
    }

    #[test]
    fn paragraph_modes() {
        assert_eq!(paragraphs("a\nb\n\nc", ParagraphMode::EachLine), vec!["a", "b", "", "c"]);
        assert_eq!(paragraphs("a\nb\n\nc", ParagraphMode::EmptyLine), vec!["a b", "", "c"]);
    }

    #[test]
    fn text_and_images_make_pages() {
        let long = "Zeile mit etwas Text. ".repeat(40);
        let docs =
            vec![("a.txt".to_string(), format!("{long}\n").repeat(30)), ("b.txt".to_string(), "kurz".into())];
        let layout = TextLayout {
            bookmark_per_file: true,
            header: Some("%[FileName]".into()),
            ..TextLayout::default()
        };
        let (pdf, marks) = text_to_pdf(&docs, &layout).unwrap();
        let doc = Document::load_mem(&pdf).unwrap();
        assert!(doc.get_pages().len() >= 3);
        assert_eq!(marks.first(), Some(&(0, "a".to_string())));
        assert_eq!(marks[1].1, "b");
        assert_eq!(marks[1].0 + 1, doc.get_pages().len(), "b starts on its own page");

        let img = RgbaImage { width: 300, height: 150, rgba: vec![200; 300 * 150 * 4] };
        let images: Vec<_> = (0..5).map(|i| (format!("bild{i}.png"), img.clone())).collect();
        let layout = ImageLayout {
            columns: 2,
            rows: 2,
            caption: Some("%[FileName]".into()),
            bookmarks: true,
            ..ImageLayout::default()
        };
        let (pdf, marks) = images_to_pdf(&images, &layout).unwrap();
        assert_eq!(Document::load_mem(&pdf).unwrap().get_pages().len(), 2);
        assert_eq!(marks.len(), 5);
        assert_eq!(marks[4], (1, "bild4".to_string()));
    }

    #[test]
    fn markdown_blocks() {
        let md = "# Titel\n\nEin **fetter** und *kursiver* Text\nmit [Link](https://x.y) und `code`.\n\n\
                  - eins\n- zwei\n  - tief\n1. erster\n\n> Zitat\n\n---\n```\nlet a = 1;\n```\n| A | B |\n|---|---|\n| 1 | 2 |\n";
        let paras = markdown_paras(md, 10.0);
        let texts: Vec<&str> = paras.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(
            texts,
            vec![
                "Titel",
                "Ein fetter und kursiver Text mit Link und code.",
                "eins",
                "zwei",
                "tief",
                "erster",
                "Zitat",
                "",
                "let a = 1;",
                "A | B",
                "1 | 2",
            ]
        );
        assert_eq!((paras[0].font, paras[0].size), ("FB", 20.0));
        assert_eq!(paras[2].prefix.as_deref(), Some("•"));
        assert!(paras[4].indent > paras[3].indent, "nested list item");
        assert_eq!(paras[5].prefix.as_deref(), Some("1."));
        assert!(paras[7].rule);
        assert_eq!(paras[8].font, "FC");
        assert_eq!(strip_inline(r"snake_case \*x\* __b__ ![Bild](a.png)"), "snake_case *x* b Bild");

        let long = format!("{md}\n{}", "Absatz mit Text. ".repeat(400));
        let (pdf, marks) = markdown_to_pdf(
            &[("a.md".into(), long)],
            &TextLayout { bookmark_per_file: true, ..TextLayout::default() },
        )
        .unwrap();
        assert!(Document::load_mem(&pdf).unwrap().get_pages().len() >= 2);
        assert_eq!(marks, vec![(0, "a".to_string())]);
    }

    #[test]
    fn csv_tables() {
        let rows = parse_csv(
            "Name;Ort;Notiz\r\nAnna;Köln;\"mit ; und \"\"Zitat\"\"\"\nBen;Bonn;\"zwei\nZeilen\"\n\n",
        );
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1][2], "mit ; und \"Zitat\"");
        assert_eq!(rows[2][2], "zwei\nZeilen");
        assert_eq!(parse_csv("a,b\n1,2")[1], vec!["1", "2"]);
        assert_eq!(parse_csv("a\tb\n1\t2")[0], vec!["a", "b"]);

        let mut csv = String::from("Nr,Beschreibung,Betrag\n");
        for i in 0..300 {
            csv.push_str(&format!("{i},\"Eine längere Beschreibung der Zeile {i}\",{}.50\n", i * 3));
        }
        let (pdf, _) = csv_to_pdf(&[("t.csv".into(), csv)], &TextLayout::default()).unwrap();
        let doc = Document::load_mem(&pdf).unwrap();
        let pages = doc.get_pages();
        assert!(pages.len() >= 5, "{} pages", pages.len());
        // The header is repeated on the second page.
        let second = doc.get_page_content(*pages.get(&2).unwrap());
        let text = String::from_utf8_lossy(&second);
        assert!(text.contains("(Beschreibung) Tj"), "header repeated");
    }
}
