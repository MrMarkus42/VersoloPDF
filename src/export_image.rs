//! "Exportieren → In Bilder": pages as PNG, JPEG,
//! BMP or (multi-page) TIFF with resolution, zoom and background.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::document::{Document, MAX_BITMAP_EDGE, RenderOptions};
use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ImageFormat {
    Png,
    Jpeg,
    Bmp,
    #[default]
    Tiff,
}

impl ImageFormat {
    pub const ALL: [ImageFormat; 4] =
        [ImageFormat::Bmp, ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::Tiff];

    pub fn extension(self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpg",
            ImageFormat::Bmp => "bmp",
            ImageFormat::Tiff => "tif",
        }
    }

    /// Name in the list.
    pub fn label(self) -> &'static str {
        match self {
            ImageFormat::Png => "PNG - Portable Network Graphics",
            ImageFormat::Jpeg => "JPEG - Joint Photographic Experts Group",
            ImageFormat::Bmp => "BMP - Windows Bitmap",
            ImageFormat::Tiff => "TIFF - Tag Image File Format",
        }
    }

    /// Only TIFF holds several pages in one file.
    pub fn multi_page(self) -> bool {
        self == ImageFormat::Tiff
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ExportMode {
    /// "Alle Seiten in eine Mehr-Seiten-Bilddatei speichern"
    #[default]
    OneFile,
    /// "Jeden Seitenbereich in eine eigene Mehr-Seiten-Bilddatei speichern"
    PerRange,
    /// "Jede Seite in eine eigene Bilddatei speichern"
    PerPage,
}

/// Settings of the "Als Bilddateien exportieren" dialog.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ImageExportOptions {
    pub format: ImageFormat,
    pub jpeg_quality: u8,
    /// TIFF: ZIP (Deflate) instead of uncompressed.
    pub tiff_zip: bool,
    pub mode: ExportMode,
    /// "Vergrößerung" in percent.
    pub zoom: f32,
    /// Horizontal and vertical resolution.
    pub dpi: (f32, f32),
    pub background: [u8; 3],
    /// File name pattern (`%[FileName]`, `%[AutoNumber]`, `%[Page]` …).
    pub file_name: String,
    pub open_folder: bool,
}

impl Default for ImageExportOptions {
    fn default() -> Self {
        Self {
            format: ImageFormat::Tiff,
            jpeg_quality: 85,
            tiff_zip: true,
            mode: ExportMode::OneFile,
            zoom: 100.0,
            dpi: (150.0, 150.0),
            background: [255, 255, 255],
            file_name: "%[FileName]_%[AutoNumber]".into(),
            open_folder: true,
        }
    }
}

impl ImageExportOptions {
    /// The effective mode (single-page formats always write one file per page).
    pub fn effective_mode(&self) -> ExportMode {
        if self.format.multi_page() { self.mode } else { ExportMode::PerPage }
    }

    /// Pixel size of a page of `(w, h)` points.
    pub fn pixel_size(&self, (w, h): (f32, f32)) -> (u32, u32) {
        let z = self.zoom.clamp(1.0, 6400.0) / 100.0;
        let px = |pt: f32, dpi: f32| {
            ((pt / 72.0 * dpi.clamp(9.0, 2400.0) * z).round() as u32).clamp(1, MAX_BITMAP_EDGE)
        };
        (px(w, self.dpi.0), px(h, self.dpi.1))
    }
}

/// Renders a page as RGB image with the options' size and background.
pub fn render(doc: &Document, page: usize, o: &ImageExportOptions) -> Result<image::RgbImage> {
    let geo = doc.geometry(page).ok_or(Error::PageOutOfRange(page))?;
    let (w, h) = o.pixel_size(geo.view_size());
    let opts = RenderOptions { background: o.background, ..RenderOptions::default() };
    let img = doc.render_tile(page, w, h, 0, 0, w, h, opts)?;
    let rgba = image::RgbaImage::from_raw(img.width, img.height, img.rgba)
        .ok_or_else(|| Error::Invalid("bitmap size mismatch".into()))?;
    Ok(image::DynamicImage::ImageRgba8(rgba).to_rgb8())
}

/// Encodes one or more pages in the chosen format (several only for TIFF).
pub fn encode(pages: &[image::RgbImage], o: &ImageExportOptions) -> Result<Vec<u8>> {
    let first = pages.first().ok_or_else(|| Error::Invalid("no pages".into()))?;
    let mut out = std::io::Cursor::new(Vec::new());
    let err = |e: image::ImageError| Error::Invalid(e.to_string());
    match o.format {
        ImageFormat::Tiff => return Ok(write_tiff(pages, o.dpi, o.tiff_zip)),
        ImageFormat::Png => first.write_to(&mut out, image::ImageFormat::Png).map_err(err)?,
        ImageFormat::Bmp => first.write_to(&mut out, image::ImageFormat::Bmp).map_err(err)?,
        ImageFormat::Jpeg => {
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, o.jpeg_quality.clamp(1, 100))
                .encode_image(first)
                .map_err(err)?;
        }
    }
    Ok(out.into_inner())
}

/// Writes RGB pages as a (multi-page) baseline TIFF, optionally Deflate
/// compressed (compression 8).
pub fn write_tiff(pages: &[image::RgbImage], dpi: (f32, f32), zip: bool) -> Vec<u8> {
    fn u16le(v: &mut Vec<u8>, x: u16) {
        v.extend_from_slice(&x.to_le_bytes());
    }
    fn u32le(v: &mut Vec<u8>, x: u32) {
        v.extend_from_slice(&x.to_le_bytes());
    }
    let mut out = b"II*\0".to_vec();
    u32le(&mut out, 0); // offset of the first IFD, patched below
    let mut link_at = 4usize;
    for page in pages {
        let raw = page.as_raw();
        let data = if zip {
            let mut s = lopdf::Stream::new(lopdf::Dictionary::new(), raw.clone());
            if s.compress().is_ok() && s.content.len() < raw.len() { s.content } else { raw.clone() }
        } else {
            raw.clone()
        };
        let compressed = zip && data.len() != raw.len();
        let data_at = out.len() as u32;
        out.extend_from_slice(&data);
        if out.len() % 2 == 1 {
            out.push(0);
        }
        let bps_at = out.len() as u32;
        for _ in 0..3 {
            u16le(&mut out, 8);
        }
        let xres_at = out.len() as u32;
        u32le(&mut out, (dpi.0.clamp(1.0, 10_000.0) * 100.0) as u32);
        u32le(&mut out, 100);
        let yres_at = out.len() as u32;
        u32le(&mut out, (dpi.1.clamp(1.0, 10_000.0) * 100.0) as u32);
        u32le(&mut out, 100);
        let ifd_at = out.len() as u32;
        out[link_at..link_at + 4].copy_from_slice(&ifd_at.to_le_bytes());
        // (tag, type, count, value): type 3 SHORT, 4 LONG, 5 RATIONAL
        let entries: [(u16, u16, u32, u32); 13] = [
            (256, 4, 1, page.width()),
            (257, 4, 1, page.height()),
            (258, 3, 3, bps_at),
            (259, 3, 1, if compressed { 8 } else { 1 }),
            (262, 3, 1, 2),
            (273, 4, 1, data_at),
            (277, 3, 1, 3),
            (278, 4, 1, page.height()),
            (279, 4, 1, data.len() as u32),
            (282, 5, 1, xres_at),
            (283, 5, 1, yres_at),
            (284, 3, 1, 1),
            (296, 3, 1, 2),
        ];
        u16le(&mut out, entries.len() as u16);
        for (tag, kind, count, value) in entries {
            u16le(&mut out, tag);
            u16le(&mut out, kind);
            u32le(&mut out, count);
            if kind == 3 && count == 1 {
                u16le(&mut out, value as u16);
                u16le(&mut out, 0);
            } else {
                u32le(&mut out, value);
            }
        }
        link_at = out.len();
        u32le(&mut out, 0);
    }
    out
}

/// Replaces the macros of the file name pattern.
pub fn file_name(pattern: &str, stem: &str, page: usize, auto: usize, pages: &str) -> String {
    let name = pattern
        .replace("%[FileName]", stem)
        .replace("%[AutoNumber]", &auto.to_string())
        .replace("%[Page]", &(page + 1).to_string())
        .replace("%[Pages]", pages);
    let clean: String = name
        .chars()
        .map(|c| if c.is_control() || "/\\:*?\"<>|".contains(c) { '_' } else { c })
        .take(200)
        .collect();
    if clean.trim().is_empty() { stem.to_string() } else { clean }
}

/// Exports `groups` of pages (one file per group) into `folder`. Returns
/// the written files.
pub fn export(
    doc: &Document,
    groups: &[Vec<usize>],
    folder: &Path,
    o: &ImageExportOptions,
) -> Result<Vec<std::path::PathBuf>> {
    let stem = doc.display_name().trim_end_matches(".pdf").trim_end_matches(".PDF").to_string();
    let mut written = Vec::new();
    for (i, group) in groups.iter().enumerate() {
        let images = group.iter().map(|&p| render(doc, p, o)).collect::<Result<Vec<_>>>()?;
        let bytes = encode(&images, o)?;
        let first = group.first().copied().unwrap_or(0);
        let pages = crate::print::format_ranges(group);
        let name =
            format!("{}.{}", file_name(&o.file_name, &stem, first, i + 1, &pages), o.format.extension());
        let path = folder.join(name);
        std::fs::write(&path, bytes).map_err(|e| Error::io(&path, e))?;
        written.push(path);
    }
    Ok(written)
}

/// Splits the pages into files according to the export mode; `ranges`
/// are the comma separated parts of the page range.
pub fn groups(pages: &[usize], ranges: &[Vec<usize>], mode: ExportMode) -> Vec<Vec<usize>> {
    match mode {
        ExportMode::OneFile => vec![pages.to_vec()],
        ExportMode::PerRange if ranges.len() > 1 => ranges.to_vec(),
        ExportMode::PerRange => vec![pages.to_vec()],
        ExportMode::PerPage => pages.iter().map(|&p| vec![p]).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads (width, height, compression) of every page of a TIFF.
    fn tiff_pages(data: &[u8]) -> Vec<(u32, u32, u16)> {
        let u16at = |o: usize| u16::from_le_bytes([data[o], data[o + 1]]);
        let u32at = |o: usize| u32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]);
        assert_eq!(&data[..4], b"II*\0");
        let mut out = Vec::new();
        let mut ifd = u32at(4) as usize;
        while ifd != 0 {
            let n = u16at(ifd) as usize;
            let (mut w, mut h, mut c) = (0, 0, 0);
            for e in 0..n {
                let at = ifd + 2 + e * 12;
                match u16at(at) {
                    256 => w = u32at(at + 8),
                    257 => h = u32at(at + 8),
                    259 => c = u16at(at + 8),
                    _ => {}
                }
            }
            out.push((w, h, c));
            ifd = u32at(ifd + 2 + n * 12) as usize;
        }
        out
    }

    #[test]
    fn tiff_with_several_pages() {
        let a = image::RgbImage::from_pixel(30, 20, image::Rgb([255, 255, 255]));
        let b = image::RgbImage::from_pixel(10, 40, image::Rgb([1, 2, 3]));
        let data = write_tiff(&[a.clone(), b], (150.0, 150.0), true);
        assert_eq!(tiff_pages(&data), vec![(30, 20, 8), (10, 40, 8)]);
        let plain = write_tiff(&[a], (72.0, 72.0), false);
        assert_eq!(tiff_pages(&plain), vec![(30, 20, 1)]);
        assert!(plain.len() > 30 * 20 * 3);
    }

    #[test]
    fn names_groups_and_formats() {
        assert_eq!(file_name("%[FileName]_%[AutoNumber]", "Bericht", 4, 2, "5"), "Bericht_2");
        assert_eq!(file_name("%[FileName]-S%[Page]", "a", 0, 1, "1"), "a-S1");
        assert_eq!(file_name("x/y:%[Page]", "a", 1, 1, "2"), "x_y_2");
        let ranges = vec![vec![0, 1], vec![4]];
        assert_eq!(groups(&[0, 1, 4], &ranges, ExportMode::PerRange), ranges);
        assert_eq!(groups(&[0, 1, 4], &ranges, ExportMode::PerPage).len(), 3);
        assert_eq!(groups(&[0, 1, 4], &ranges, ExportMode::OneFile), vec![vec![0, 1, 4]]);
        let o = ImageExportOptions { format: ImageFormat::Png, ..ImageExportOptions::default() };
        assert_eq!(o.effective_mode(), ExportMode::PerPage);
        assert_eq!(o.pixel_size((595.0, 842.0)), (1240, 1754));
        let img = image::RgbImage::from_pixel(8, 8, image::Rgb([10, 20, 30]));
        for format in ImageFormat::ALL {
            let data = encode(
                std::slice::from_ref(&img),
                &ImageExportOptions { format, ..ImageExportOptions::default() },
            )
            .unwrap();
            assert!(data.len() > 20, "{format:?}");
        }
    }
}
