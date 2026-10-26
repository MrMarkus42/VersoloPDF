//! Print preparation: which content is printed and
//! how pages are placed on sheets – no scaling, fit, shrink, custom zoom,
//! tiling of large pages, several pages per sheet and booklets.

use lopdf::{Dictionary, Document, Object, Stream};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::geometry::RectF;
use crate::import::{Matrix, PageOrder, Placement, SheetSpec};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Scaling {
    /// "Keine Größenanpassung"
    #[default]
    None,
    /// "In Druckbereich einpassen"
    Fit,
    /// "Auf Druckbereich reduzieren"
    Shrink,
    /// "Eigene Vergrößerung"
    Custom,
    /// "Große Seiten teilen"
    TileLarge,
    /// "Alle Seiten teilen"
    TileAll,
    /// "Mehrere Seiten pro Blatt"
    NUp,
    /// "Booklet"
    Booklet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Duplex {
    /// "Auto (Ohne)": the printer's setting
    #[default]
    Auto,
    Simplex,
    LongEdge,
    ShortEdge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PrintContent {
    /// Page content, form fields, media
    Document,
    /// … plus comments
    #[default]
    DocumentAndMarkup,
    /// … plus stamps only
    DocumentAndStamps,
    /// Only the values of form fields
    FormData,
    /// Only comments
    MarkupOnly,
    /// Everything, also comments marked as "do not print"
    Everything,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Binding {
    #[default]
    Left,
    Right,
}

/// Settings of the print dialog (also stored as presets).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PrintSettings {
    pub copies: u32,
    pub collate: bool,
    pub duplex: Duplex,
    /// "Reihenfolge umkehren"
    pub reverse: bool,
    pub scaling: Scaling,
    /// Percent for Custom and the tiles.
    pub zoom: f32,
    pub auto_rotate: bool,
    pub auto_center: bool,
    /// "Papiergröße anhand der Dokumentseitengröße wählen"
    pub paper_from_page: bool,
    /// "Druckerrandeinstellungen ignorieren"
    pub ignore_margins: bool,
    /// Overlap of tiles in points.
    pub tile_overlap: f32,
    pub columns: u32,
    pub rows: u32,
    pub order: PageOrder,
    pub border: bool,
    pub binding: Binding,
    pub content: PrintContent,
    /// "Als Bild drucken"
    pub as_image: bool,
    /// Paper name (systems without printer driver dialog).
    pub paper: String,
}

impl Default for PrintSettings {
    fn default() -> Self {
        Self {
            copies: 1,
            collate: true,
            duplex: Duplex::Auto,
            reverse: false,
            scaling: Scaling::None,
            zoom: 100.0,
            auto_rotate: false,
            auto_center: false,
            paper_from_page: false,
            ignore_margins: true,
            tile_overlap: 0.0,
            columns: 2,
            rows: 2,
            order: PageOrder::Horizontal,
            border: false,
            binding: Binding::Left,
            content: PrintContent::DocumentAndMarkup,
            as_image: false,
            paper: "A4".into(),
        }
    }
}

/// Paper of the printer: size (points, portrait) and printable area.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Paper {
    pub size: (f32, f32),
    pub printable: RectF,
}

impl Paper {
    /// Paper without printer margins.
    pub fn full(w: f32, h: f32) -> Self {
        Self { size: (w, h), printable: RectF::new(0.0, 0.0, w, h) }
    }
}

fn intersect(a: RectF, b: RectF) -> RectF {
    RectF::new(
        a.x0.max(b.x0),
        a.y0.max(b.y0),
        a.x1.min(b.x1).max(a.x0.max(b.x0)),
        a.y1.min(b.y1).max(a.y0.max(b.y0)),
    )
}

/// Places a page of upright size `(w, h)` into `area`.
fn place(
    page: usize,
    (w, h): (f32, f32),
    area: RectF,
    scale: f32,
    rotate: bool,
    center: bool,
    border: bool,
) -> Placement {
    let (pw, ph) = if rotate { (h, w) } else { (w, h) };
    let (dw, dh) = (pw * scale, ph * scale);
    let x = if center { area.x0 + (area.width() - dw) / 2.0 } else { area.x0 };
    let y = if center { area.y0 + (area.height() - dh) / 2.0 } else { area.y1 - dh };
    let mut m = Matrix::IDENTITY;
    if rotate {
        m = m.then(Matrix::rotate(90.0)).then(Matrix::translate(h, 0.0));
    }
    let matrix = m.then(Matrix::scale(scale, scale)).then(Matrix::translate(x, y));
    Placement { page, matrix, clip: intersect(RectF::new(x, y, x + dw, y + dh), area), border }
}

fn fit_scale((w, h): (f32, f32), area: RectF) -> f32 {
    (area.width() / w.max(1.0)).min(area.height() / h.max(1.0)).max(0.001)
}

/// Whether turning the page makes it match the area's orientation.
fn should_rotate(s: &PrintSettings, (w, h): (f32, f32), area: RectF) -> bool {
    s.auto_rotate && (w > h) != (area.width() > area.height()) && (w - h).abs() > 1.0
}

/// Sheets for `pages` (page index, upright size) on `paper`.
pub fn layout(pages: &[(usize, (f32, f32))], paper: &Paper, s: &PrintSettings) -> Vec<SheetSpec> {
    let mut pages = pages.to_vec();
    if s.reverse {
        pages.reverse();
    }
    let full = RectF::new(0.0, 0.0, paper.size.0, paper.size.1);
    let printable = intersect(paper.printable, full);
    let zoom = s.zoom.clamp(1.0, 6400.0) / 100.0;
    let mut sheets = Vec::new();
    match s.scaling {
        Scaling::None | Scaling::Custom | Scaling::Fit | Scaling::Shrink => {
            for &(page, size) in &pages {
                if s.paper_from_page {
                    let z = if s.scaling == Scaling::Custom { zoom } else { 1.0 };
                    let sheet = (size.0 * z, size.1 * z);
                    let area = RectF::new(0.0, 0.0, sheet.0, sheet.1);
                    sheets.push(SheetSpec {
                        size: sheet,
                        items: vec![place(page, size, area, z, false, true, false)],
                    });
                    continue;
                }
                let area = if s.ignore_margins && matches!(s.scaling, Scaling::Fit | Scaling::Shrink) {
                    full
                } else {
                    printable
                };
                let rotate = should_rotate(s, size, area);
                let upright = if rotate { (size.1, size.0) } else { size };
                let scale = match s.scaling {
                    Scaling::None => 1.0,
                    Scaling::Custom => zoom,
                    Scaling::Fit => fit_scale(upright, area),
                    _ => fit_scale(upright, area).min(1.0),
                };
                let center = s.auto_center || matches!(s.scaling, Scaling::Fit | Scaling::Shrink);
                sheets.push(SheetSpec {
                    size: paper.size,
                    items: vec![place(page, size, area, scale, rotate, center, false)],
                });
            }
        }
        Scaling::TileLarge | Scaling::TileAll => {
            let area = printable;
            let ov = s.tile_overlap.clamp(0.0, area.width().min(area.height()) / 2.0);
            for &(page, size) in &pages {
                let (dw, dh) = (size.0 * zoom, size.1 * zoom);
                let fits = dw <= area.width() + 0.5 && dh <= area.height() + 0.5;
                if s.scaling == Scaling::TileLarge && fits {
                    sheets.push(SheetSpec {
                        size: paper.size,
                        items: vec![place(page, size, area, zoom, false, s.auto_center, false)],
                    });
                    continue;
                }
                let (sx, sy) = ((area.width() - ov).max(1.0), (area.height() - ov).max(1.0));
                let cols = (((dw - ov) / sx).ceil() as usize).clamp(1, 100);
                let rows = (((dh - ov) / sy).ceil() as usize).clamp(1, 100);
                for r in 0..rows {
                    for c in 0..cols {
                        let x = area.x0 - c as f32 * sx;
                        let y = area.y1 - dh + r as f32 * sy;
                        let matrix = Matrix::scale(zoom, zoom).then(Matrix::translate(x, y));
                        sheets.push(SheetSpec {
                            size: paper.size,
                            items: vec![Placement { page, matrix, clip: area, border: false }],
                        });
                    }
                }
            }
        }
        Scaling::NUp => {
            let (cols, rows) = (s.columns.clamp(1, 16) as usize, s.rows.clamp(1, 16) as usize);
            let area = printable;
            let (cw, ch) = (area.width() / cols as f32, area.height() / rows as f32);
            for chunk in pages.chunks(cols * rows) {
                let items = chunk
                    .iter()
                    .enumerate()
                    .map(|(slot, &(page, size))| {
                        let (col, row) = match s.order {
                            PageOrder::Horizontal => (slot % cols, slot / cols),
                            PageOrder::Vertical => (slot / rows, slot % rows),
                        };
                        let cell = RectF::new(
                            area.x0 + col as f32 * cw,
                            area.y1 - (row + 1) as f32 * ch,
                            area.x0 + (col + 1) as f32 * cw,
                            area.y1 - row as f32 * ch,
                        );
                        let rotate = should_rotate(s, size, cell);
                        let upright = if rotate { (size.1, size.0) } else { size };
                        place(page, size, cell, fit_scale(upright, cell), rotate, true, s.border)
                    })
                    .collect();
                sheets.push(SheetSpec { size: paper.size, items });
            }
        }
        Scaling::Booklet => {
            // Landscape sheets, two pages per side, folded in the middle.
            let size = (paper.size.1, paper.size.0);
            let p = &paper.printable;
            let area = RectF::new(p.y0, p.x0, p.y1, p.x1);
            let mid = area.x0 + area.width() / 2.0;
            let halves =
                [RectF::new(area.x0, area.y0, mid, area.y1), RectF::new(mid, area.y0, area.x1, area.y1)];
            let n = pages.len().div_ceil(4) * 4;
            for side in 0..n / 2 {
                let (mut left, mut right) =
                    if side % 2 == 0 { (n - 1 - side, side) } else { (side, n - 1 - side) };
                if s.binding == Binding::Right {
                    std::mem::swap(&mut left, &mut right);
                }
                let items = [left, right]
                    .iter()
                    .zip(halves)
                    .filter_map(|(&i, half)| {
                        let &(page, size) = pages.get(i)?;
                        let rotate =
                            should_rotate(&PrintSettings { auto_rotate: true, ..s.clone() }, size, half);
                        let upright = if rotate { (size.1, size.0) } else { size };
                        Some(place(page, size, half, fit_scale(upright, half), rotate, true, false))
                    })
                    .collect();
                sheets.push(SheetSpec { size, items });
            }
        }
    }
    sheets
}

/// Keeps only the content chosen under "Drucken:" (comments, stamps, form
/// fields, page content). Returns the PDF and whether annotations remain
/// (then they must be flattened before the pages are placed on sheets).
pub fn filter_content(
    bytes: &[u8],
    password: Option<&str>,
    content: PrintContent,
) -> Result<(Vec<u8>, bool)> {
    crate::pdf_edit::guarded(|| {
        let mut options = match password {
            Some(pw) => lopdf::LoadOptions::with_password(pw),
            None => lopdf::LoadOptions::default(),
        };
        options.max_decompressed_size = Some(crate::pdf_edit::MAX_DECOMPRESSED_STREAM);
        let mut doc = Document::load_mem_with_options(bytes, options)?;
        if doc.is_encrypted() {
            return Err(Error::EncryptedNotSupported);
        }
        let empty = doc.add_object(Stream::new(Dictionary::new(), Vec::new()));
        let mut any = false;
        for page in doc.get_pages().into_values().collect::<Vec<_>>() {
            let annots: Vec<Object> = doc
                .get_dictionary(page)?
                .get(b"Annots")
                .ok()
                .map(|a| match a {
                    Object::Reference(id) => {
                        doc.get_object(*id).ok().and_then(|o| o.as_array().ok()).cloned().unwrap_or_default()
                    }
                    Object::Array(a) => a.clone(),
                    _ => Vec::new(),
                })
                .unwrap_or_default();
            let mut keep = Vec::new();
            for a in annots {
                let Some(id) = a.as_reference().ok() else { continue };
                let Ok(dict) = doc.get_dictionary(id) else { continue };
                let subtype =
                    dict.get(b"Subtype").ok().and_then(|s| s.as_name().ok()).unwrap_or_default().to_vec();
                if matches!(subtype.as_slice(), b"Link" | b"Popup") {
                    continue;
                }
                let widget = subtype == b"Widget";
                let wanted = match content {
                    PrintContent::Document | PrintContent::FormData => widget,
                    PrintContent::DocumentAndStamps => widget || subtype == b"Stamp",
                    PrintContent::MarkupOnly => !widget,
                    PrintContent::DocumentAndMarkup | PrintContent::Everything => true,
                };
                if !wanted {
                    continue;
                }
                if content == PrintContent::Everything
                    && let Ok(d) = doc.get_object_mut(id).and_then(Object::as_dict_mut)
                {
                    let flags = d.get(b"F").ok().and_then(|f| f.as_i64().ok()).unwrap_or(0);
                    // Print on, hidden and no-view off.
                    d.set("F", Object::Integer((flags | 4) & !(2 | 32)));
                }
                keep.push(Object::Reference(id));
            }
            any |= !keep.is_empty();
            let page_dict = doc.get_object_mut(page).and_then(Object::as_dict_mut)?;
            if keep.is_empty() {
                page_dict.remove(b"Annots");
            } else {
                page_dict.set("Annots", Object::Array(keep));
            }
            if matches!(content, PrintContent::FormData | PrintContent::MarkupOnly) {
                page_dict.set("Contents", Object::Reference(empty));
            }
        }
        let mut out = Vec::new();
        doc.save_to(&mut out).map_err(|e| Error::Structure(e.to_string()))?;
        Ok((out, any))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const A4: (f32, f32) = (595.0, 842.0);

    fn pages(n: usize, size: (f32, f32)) -> Vec<(usize, (f32, f32))> {
        (0..n).map(|i| (i, size)).collect()
    }

    fn scale(p: &Placement) -> f32 {
        let [a, b, ..] = p.matrix.0;
        (a * a + b * b).sqrt()
    }

    #[test]
    fn fit_shrink_and_custom() {
        let paper = Paper { size: A4, printable: RectF::new(20.0, 20.0, 575.0, 822.0) };
        let big = pages(1, (1190.0, 1684.0)); // A2-ish
        let fit = layout(
            &big,
            &paper,
            &PrintSettings { scaling: Scaling::Fit, ignore_margins: false, ..Default::default() },
        );
        assert!((scale(&fit[0].items[0]) - 0.4663).abs() < 0.01);
        let small = pages(1, (300.0, 400.0));
        let shrink =
            layout(&small, &paper, &PrintSettings { scaling: Scaling::Shrink, ..Default::default() });
        assert!((scale(&shrink[0].items[0]) - 1.0).abs() < 1e-3, "shrink never enlarges");
        let custom = layout(
            &small,
            &paper,
            &PrintSettings { scaling: Scaling::Custom, zoom: 50.0, ..Default::default() },
        );
        assert!((scale(&custom[0].items[0]) - 0.5).abs() < 1e-3);
        let own = layout(&small, &paper, &PrintSettings { paper_from_page: true, ..Default::default() });
        assert_eq!(own[0].size, (300.0, 400.0));
        // Landscape page, auto rotate: turned and larger.
        let wide = pages(1, (842.0, 595.0));
        let r = layout(
            &wide,
            &Paper::full(595.0, 842.0),
            &PrintSettings { scaling: Scaling::Fit, auto_rotate: true, ..Default::default() },
        );
        assert!((scale(&r[0].items[0]) - 1.0).abs() < 0.01);
    }

    #[test]
    fn tiles_nup_and_booklet() {
        let paper = Paper::full(A4.0, A4.1);
        // An A2 page needs 4 A4 tiles at 100 %.
        let tiles = layout(
            &pages(1, (1190.0, 1684.0)),
            &paper,
            &PrintSettings { scaling: Scaling::TileAll, ..Default::default() },
        );
        assert_eq!(tiles.len(), 4);
        let large = layout(
            &pages(2, (300.0, 300.0)),
            &paper,
            &PrintSettings { scaling: Scaling::TileLarge, ..Default::default() },
        );
        assert_eq!(large.len(), 2, "small pages are not tiled");
        let nup = layout(
            &pages(9, A4),
            &paper,
            &PrintSettings { scaling: Scaling::NUp, columns: 2, rows: 2, ..Default::default() },
        );
        assert_eq!(nup.len(), 3);
        assert_eq!(nup[0].items.len(), 4);
        assert_eq!(nup[2].items.len(), 1);
        // 6 pages → 8 page slots → 2 sheets, 4 sides.
        let booklet =
            layout(&pages(6, A4), &paper, &PrintSettings { scaling: Scaling::Booklet, ..Default::default() });
        assert_eq!(booklet.len(), 4);
        assert_eq!(booklet[0].size, (A4.1, A4.0));
        // First side: last slot (blank) left, page 1 right.
        assert_eq!(booklet[0].items.iter().map(|p| p.page).collect::<Vec<_>>(), vec![0]);
        assert_eq!(booklet[1].items.iter().map(|p| p.page).collect::<Vec<_>>(), vec![1]);
        assert_eq!(booklet[2].items.iter().map(|p| p.page).collect::<Vec<_>>(), vec![5, 2]);
        let reversed = layout(&pages(3, A4), &paper, &PrintSettings { reverse: true, ..Default::default() });
        assert_eq!(reversed[0].items[0].page, 2);
    }
}
