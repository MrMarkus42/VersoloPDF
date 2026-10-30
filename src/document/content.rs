//! Editing page content through PDFium: reading the page objects, moving,
//! scaling, rotating, deleting, restyling and reordering them, copying and
//! pasting, images, and replacing text blocks.

use pdfium_render::prelude::*;

use super::{Document, page_index};
use crate::content::{
    Affine, Align, BlendMode, ContentObject, FontChoice, ObjectKind, ObjectStyle, TextStyle, ZOrder, bounds,
    win_ansi_only, wrap_text, z_order,
};
use crate::error::{Error, Result};
use crate::geometry::PointF;
use crate::model::Rgba;

/// Objects copied with "Kopieren" / "Ausschneiden".
#[derive(Debug, Clone, Default)]
pub struct ObjectClip {
    pub items: Vec<ClipItem>,
}

impl ObjectClip {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// One copied object.
#[derive(Debug, Clone)]
pub enum ClipItem {
    Text {
        text: String,
        font: String,
        size: f32,
        color: Rgba,
        bold: bool,
        italic: bool,
        serif: bool,
        mono: bool,
        matrix: Affine,
    },
    Image {
        image: image::DynamicImage,
        matrix: Affine,
    },
    Path {
        segments: Vec<(PathSeg, f32, f32, bool)>,
        fill: Option<Rgba>,
        stroke: Option<Rgba>,
        width: f32,
        matrix: Affine,
    },
}

/// Segment kinds of a copied path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathSeg {
    Move,
    Line,
    Bezier,
}

fn to_rgba(c: PdfColor) -> Rgba {
    Rgba { r: c.red(), g: c.green(), b: c.blue(), a: c.alpha() }
}

fn to_pdf_color(c: Rgba) -> PdfColor {
    PdfColor::new(c.r, c.g, c.b, c.a)
}

fn to_matrix(m: Affine) -> PdfMatrix {
    let [a, b, c, d, e, f] = m.0;
    PdfMatrix::new(a, b, c, d, e, f)
}

fn from_matrix(m: &PdfMatrix) -> Affine {
    Affine([m.a(), m.b(), m.c(), m.d(), m.e(), m.f()])
}

fn kind_of(t: PdfPageObjectType) -> ObjectKind {
    match t {
        PdfPageObjectType::Text => ObjectKind::Text,
        PdfPageObjectType::Image => ObjectKind::Image,
        PdfPageObjectType::Path => ObjectKind::Path,
        PdfPageObjectType::Shading => ObjectKind::Shading,
        PdfPageObjectType::XObjectForm => ObjectKind::Form,
        _ => ObjectKind::Other,
    }
}

fn blend(b: BlendMode) -> PdfPageObjectBlendMode {
    match b {
        BlendMode::Normal => PdfPageObjectBlendMode::Normal,
        BlendMode::Multiply => PdfPageObjectBlendMode::Multiply,
        BlendMode::Screen => PdfPageObjectBlendMode::Screen,
        BlendMode::Overlay => PdfPageObjectBlendMode::Overlay,
        BlendMode::Darken => PdfPageObjectBlendMode::Darken,
        BlendMode::Lighten => PdfPageObjectBlendMode::Lighten,
        BlendMode::Difference => PdfPageObjectBlendMode::Difference,
        BlendMode::Exclusion => PdfPageObjectBlendMode::Exclusion,
    }
}

/// Font look read from a PDF font.
fn font_look(font: &PdfFont) -> (String, bool, bool, bool, bool) {
    let name = font.name();
    let base = name.split_once('+').map_or(name.as_str(), |(_, r)| r).to_string();
    let lower = base.to_lowercase();
    let bold = font.weight().is_ok_and(|w| {
        matches!(
            w,
            PdfFontWeight::Weight600
                | PdfFontWeight::Weight700Bold
                | PdfFontWeight::Weight800
                | PdfFontWeight::Weight900
        ) || matches!(w, PdfFontWeight::Custom(v) if v >= 600)
    }) || font.is_bold_reenforced()
        || lower.contains("bold")
        || lower.contains("black")
        || lower.contains("heavy");
    let italic = font.is_italic() || lower.contains("italic") || lower.contains("oblique");
    let mono = font.is_fixed_pitch() || lower.contains("courier") || lower.contains("mono");
    const SERIF: [&str; 14] = [
        "times",
        "roman",
        "nimbusrom",
        "georgia",
        "garamond",
        "cambria",
        "minion",
        "palatino",
        "palladio",
        "baskerville",
        "century",
        "bookman",
        "antiqua",
        "caslon",
    ];
    let serif = !mono
        && (font.is_serif()
            || SERIF.iter().any(|s| lower.contains(s))
            || lower.contains("serif") && !lower.contains("sans"));
    (base, bold, italic, serif, mono)
}

/// A standard font for a family and style.
fn standard_font(
    fonts: &mut PdfFonts,
    choice: &FontChoice,
    serif: bool,
    mono: bool,
    bold: bool,
    italic: bool,
) -> PdfFontToken {
    let family = match choice {
        FontChoice::Times => 1,
        FontChoice::Courier => 2,
        FontChoice::Helvetica => 0,
        FontChoice::Original if mono => 2,
        FontChoice::Original if serif => 1,
        FontChoice::Original => 0,
    };
    match (family, bold, italic) {
        (1, false, false) => fonts.times_roman(),
        (1, true, false) => fonts.times_bold(),
        (1, false, true) => fonts.times_italic(),
        (1, true, true) => fonts.times_bold_italic(),
        (2, false, false) => fonts.courier(),
        (2, true, false) => fonts.courier_bold(),
        (2, false, true) => fonts.courier_oblique(),
        (2, true, true) => fonts.courier_bold_oblique(),
        (_, false, false) => fonts.helvetica(),
        (_, true, false) => fonts.helvetica_bold(),
        (_, false, true) => fonts.helvetica_oblique(),
        (_, true, true) => fonts.helvetica_bold_oblique(),
    }
}

/// A system font with many characters (for text outside WinAnsi).
fn system_font_bytes(bold: bool) -> Option<Vec<u8>> {
    let candidates: &[&str] = if bold {
        &[
            r"C:\Windows\Fonts\arialbd.ttf",
            r"C:\Windows\Fonts\segoeuib.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
            "/Library/Fonts/Arial Bold.ttf",
            "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
        ]
    } else {
        &[
            r"C:\Windows\Fonts\arial.ttf",
            r"C:\Windows\Fonts\segoeui.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/Library/Fonts/Arial.ttf",
            "/System/Library/Fonts/Supplemental/Arial.ttf",
        ]
    };
    candidates.iter().find_map(|p| std::fs::read(p).ok().filter(|b| b.len() < 64 << 20))
}

fn normalize(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl Document {
    /// The objects of a page (back to front).
    pub fn page_objects(&self, page: usize) -> Result<Vec<ContentObject>> {
        let p = self.page(page)?;
        let mut out = Vec::new();
        for (index, obj) in p.objects().iter().enumerate() {
            let Ok(q) = obj.bounds() else { continue };
            let quad = [
                PointF::new(q.x1().value, q.y1().value),
                PointF::new(q.x2().value, q.y2().value),
                PointF::new(q.x3().value, q.y3().value),
                PointF::new(q.x4().value, q.y4().value),
            ];
            let matrix = obj.matrix().map(|m| from_matrix(&m)).unwrap_or_default();
            let mut o = ContentObject {
                index,
                kind: kind_of(obj.object_type()),
                quad,
                rect: bounds(&quad),
                matrix,
                fill: obj.fill_color().ok().map(to_rgba),
                stroke: obj.stroke_color().ok().map(to_rgba),
                stroke_width: obj.stroke_width().map_or(0.0, |w| w.value),
                text: String::new(),
                font: String::new(),
                font_size: 0.0,
                bold: false,
                italic: false,
                serif: false,
                monospace: false,
                invisible: false,
            };
            if let Some(t) = obj.as_text_object() {
                o.text = t.text().trim_end().to_string();
                let (name, bold, italic, serif, mono) = font_look(&t.font());
                o.font = name;
                o.bold = bold;
                o.italic = italic;
                o.serif = serif;
                o.monospace = mono;
                o.font_size = t.unscaled_font_size().value * matrix.scale_y();
                o.invisible = !t.is_visible();
            }
            out.push(o);
        }
        Ok(out)
    }

    /// Runs `f` on the objects of `page` as one undo step.
    fn edit_objects(
        &mut self,
        label: &str,
        page: usize,
        f: impl FnOnce(&mut PdfPageObjects) -> Result<()>,
    ) -> Result<()> {
        self.require(self.permissions().modify)?;
        self.check_page(page)?;
        let idx = page_index(page)?;
        self.edit_engine(label, |pdf| {
            let mut p = pdf.pages().get(idx)?;
            f(p.objects_mut())?;
            // Some changes (colours) are only written when the content is regenerated.
            p.regenerate_content()?;
            Ok(())
        })
    }

    /// Applies `m` to the objects (move, scale, rotate, flip).
    pub fn transform_objects(&mut self, page: usize, indices: &[usize], m: Affine) -> Result<()> {
        self.transform_objects_each(page, &indices.iter().map(|i| (*i, m)).collect::<Vec<_>>())
    }

    /// Applies a matrix per object (arranging).
    pub fn transform_objects_each(&mut self, page: usize, moves: &[(usize, Affine)]) -> Result<()> {
        if moves.is_empty() {
            return Ok(());
        }
        self.edit_objects("move objects", page, |objects| {
            for (i, m) in moves {
                if *m == Affine::IDENTITY {
                    continue;
                }
                let mut o = objects.get(*i)?;
                o.apply_matrix(to_matrix(*m))?;
            }
            Ok(())
        })
    }

    pub fn delete_objects(&mut self, page: usize, indices: &[usize]) -> Result<()> {
        let mut sorted = indices.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        self.edit_objects("delete objects", page, |objects| {
            for i in sorted.into_iter().rev() {
                objects.remove_object_at_index(i)?;
            }
            Ok(())
        })
    }

    /// "Format" of objects: colours, line, opacity, blend mode.
    pub fn style_objects(&mut self, page: usize, indices: &[usize], style: &ObjectStyle) -> Result<()> {
        self.edit_objects("format objects", page, |objects| {
            for &i in indices {
                let mut o = objects.get(i)?;
                let text = o.object_type() == PdfPageObjectType::Text;
                if let Some(c) = style.fill {
                    let alpha = o.fill_color().map_or(255, |f| f.alpha());
                    o.set_fill_color(to_pdf_color(Rgba { a: alpha, ..c }))?;
                }
                if let Some(op) = style.fill_opacity {
                    let c = o.fill_color().unwrap_or(PdfColor::new(0, 0, 0, 255));
                    o.set_fill_color(PdfColor::new(
                        c.red(),
                        c.green(),
                        c.blue(),
                        (op.clamp(0.0, 1.0) * 255.0).round() as u8,
                    ))?;
                }
                if !text || style.stroke.is_some() {
                    if let Some(c) = style.stroke {
                        let alpha = o.stroke_color().map_or(255, |f| f.alpha());
                        o.set_stroke_color(to_pdf_color(Rgba { a: alpha, ..c }))?;
                    }
                    if let Some(op) = style.stroke_opacity {
                        let c = o.stroke_color().unwrap_or(PdfColor::new(0, 0, 0, 255));
                        o.set_stroke_color(PdfColor::new(
                            c.red(),
                            c.green(),
                            c.blue(),
                            (op.clamp(0.0, 1.0) * 255.0).round() as u8,
                        ))?;
                    }
                }
                if let Some(w) = style.stroke_width {
                    o.set_stroke_width(PdfPoints::new(w.max(0.0)))?;
                }
                if let Some(d) = &style.dash {
                    let pts: Vec<PdfPoints> = d.iter().map(|v| PdfPoints::new(*v)).collect();
                    o.set_dash_array(&pts, PdfPoints::ZERO)?;
                }
                if let Some(b) = style.blend {
                    o.set_blend_mode(blend(b))?;
                }
            }
            Ok(())
        })
    }

    /// "Z-Order". Returns the new indices of the selected objects.
    pub fn reorder_objects(&mut self, page: usize, indices: &[usize], how: ZOrder) -> Result<Vec<usize>> {
        let count = self.page(page)?.objects().len();
        let order = z_order(count, indices, how);
        if order.iter().enumerate().all(|(i, o)| i == *o) {
            return Ok(indices.to_vec());
        }
        self.edit_objects("change stacking order", page, |objects| {
            // Take every object off the page, then add them back in order.
            let mut taken: Vec<Option<PdfPageObject>> = Vec::with_capacity(count);
            for _ in 0..count {
                taken.push(Some(objects.remove_object_at_index(0)?));
            }
            for old in &order {
                let obj = taken[*old].take().ok_or_else(|| Error::Invalid("object order".into()))?;
                objects.add_object(obj)?;
            }
            Ok(())
        })?;
        Ok(order.iter().enumerate().filter(|(_, o)| indices.contains(o)).map(|(i, _)| i).collect())
    }

    /// Copies objects for "Einfügen" (text, images and paths; other kinds are
    /// skipped).
    pub fn copy_objects(&self, page: usize, indices: &[usize]) -> Result<ObjectClip> {
        let p = self.page(page)?;
        let objects = p.objects();
        let mut items = Vec::new();
        for &i in indices {
            let obj = objects.get(i)?;
            let matrix = obj.matrix().map(|m| from_matrix(&m)).unwrap_or_default();
            if let Some(t) = obj.as_text_object() {
                let (font, bold, italic, serif, mono) = font_look(&t.font());
                items.push(ClipItem::Text {
                    text: t.text().trim_end().to_string(),
                    font,
                    size: t.unscaled_font_size().value,
                    color: obj.fill_color().map(to_rgba).unwrap_or(Rgba::BLACK),
                    bold,
                    italic,
                    serif,
                    mono,
                    matrix,
                });
            } else if let Some(img) = obj.as_image_object() {
                if let Ok(image) = img.get_raw_image() {
                    items.push(ClipItem::Image { image, matrix });
                }
            } else if let Some(path) = obj.as_path_object() {
                let segments = path
                    .segments()
                    .raw()
                    .iter()
                    .filter_map(|s| {
                        let kind = match s.segment_type() {
                            PdfPathSegmentType::MoveTo => PathSeg::Move,
                            PdfPathSegmentType::LineTo => PathSeg::Line,
                            PdfPathSegmentType::BezierTo => PathSeg::Bezier,
                            PdfPathSegmentType::Unknown => return None,
                        };
                        Some((kind, s.x().value, s.y().value, s.is_close()))
                    })
                    .collect();
                let fill = path.fill_mode().is_ok_and(|m| m != PdfPathFillMode::None);
                let stroke = path.is_stroked().unwrap_or(false);
                items.push(ClipItem::Path {
                    segments,
                    fill: fill.then(|| obj.fill_color().map(to_rgba).unwrap_or(Rgba::BLACK)),
                    stroke: stroke.then(|| obj.stroke_color().map(to_rgba).unwrap_or(Rgba::BLACK)),
                    width: obj.stroke_width().map_or(1.0, |w| w.value),
                    matrix,
                });
            }
        }
        Ok(ObjectClip { items })
    }

    /// "Einfügen": adds copied objects on top, moved by `offset`. Text keeps
    /// its font when the page already uses a font of the same name.
    /// Returns the indices of the new objects.
    pub fn paste_objects(
        &mut self,
        page: usize,
        clip: &ObjectClip,
        offset: (f32, f32),
    ) -> Result<Vec<usize>> {
        if clip.is_empty() {
            return Ok(Vec::new());
        }
        // Fonts of the page by name (for text that was copied here).
        let mut fonts: Vec<(String, PdfFontToken)> = Vec::new();
        {
            let p = self.page(page)?;
            for obj in p.objects().iter() {
                if let Some(t) = obj.as_text_object() {
                    let font = t.font();
                    let (name, ..) = font_look(&font);
                    if !fonts.iter().any(|(n, _)| *n == name) {
                        fonts.push((name, font.token()));
                    }
                }
            }
        }
        let before = self.page(page)?.objects().len();
        let move_by = Affine::translate(offset.0, offset.1);
        self.require(self.permissions().modify)?;
        let idx = page_index(page)?;
        self.edit_engine("paste objects", |pdf| {
            let mut std_fonts = Vec::new();
            for item in &clip.items {
                if let ClipItem::Text { font, bold, italic, serif, mono, .. } = item {
                    let token = match fonts.iter().find(|(n, _)| n == font) {
                        Some((_, t)) => *t,
                        None => standard_font(
                            pdf.fonts_mut(),
                            &FontChoice::Original,
                            *serif,
                            *mono,
                            *bold,
                            *italic,
                        ),
                    };
                    std_fonts.push(token);
                }
            }
            let doc: &PdfDocument = pdf;
            let mut p = doc.pages().get(idx)?;
            let mut text_fonts = std_fonts.into_iter();
            for item in &clip.items {
                match item {
                    ClipItem::Text { text, size, color, matrix, .. } => {
                        let token = text_fonts.next().ok_or_else(|| Error::Invalid("font".into()))?;
                        let mut t = PdfPageTextObject::new(doc, text, token, PdfPoints::new(*size))?;
                        t.set_fill_color(to_pdf_color(*color))?;
                        t.apply_matrix(to_matrix(matrix.then(move_by)))?;
                        p.objects_mut().add_text_object(t)?;
                    }
                    ClipItem::Image { image, matrix } => {
                        let mut img = PdfPageImageObject::new(doc, image)?;
                        img.apply_matrix(to_matrix(matrix.then(move_by)))?;
                        p.objects_mut().add_image_object(img)?;
                    }
                    ClipItem::Path { segments, fill, stroke, width, matrix } => {
                        let (x, y) = segments.first().map_or((0.0, 0.0), |s| (s.1, s.2));
                        let mut path = PdfPagePathObject::new(
                            doc,
                            PdfPoints::new(x),
                            PdfPoints::new(y),
                            stroke.map(to_pdf_color),
                            stroke.map(|_| PdfPoints::new(*width)),
                            fill.map(to_pdf_color),
                        )?;
                        let mut bez: Vec<(f32, f32)> = Vec::new();
                        for (kind, sx, sy, close) in segments.iter().skip(1) {
                            match kind {
                                PathSeg::Move => path.move_to(PdfPoints::new(*sx), PdfPoints::new(*sy))?,
                                PathSeg::Line => path.line_to(PdfPoints::new(*sx), PdfPoints::new(*sy))?,
                                PathSeg::Bezier => {
                                    bez.push((*sx, *sy));
                                    if bez.len() == 3 {
                                        let p =
                                            |i: usize| (PdfPoints::new(bez[i].0), PdfPoints::new(bez[i].1));
                                        let ((x1, y1), (x2, y2), (x3, y3)) = (p(0), p(1), p(2));
                                        path.bezier_to(x3, y3, x1, y1, x2, y2)?;
                                        bez.clear();
                                    }
                                }
                            }
                            if *close {
                                path.close_path()?;
                            }
                        }
                        path.apply_matrix(to_matrix(matrix.then(move_by)))?;
                        p.objects_mut().add_path_object(path)?;
                    }
                }
            }
            Ok(())
        })?;
        let after = self.page(page)?.objects().len();
        Ok((before..after).collect())
    }

    /// "Duplizieren": copies of the objects, moved by `offset`.
    pub fn duplicate_objects(
        &mut self,
        page: usize,
        indices: &[usize],
        offset: (f32, f32),
    ) -> Result<Vec<usize>> {
        let clip = self.copy_objects(page, indices)?;
        self.paste_objects(page, &clip, offset)
    }

    /// The picture of an image object (masks applied).
    pub fn object_image(&self, page: usize, index: usize) -> Result<image::DynamicImage> {
        let p = self.page(page)?;
        let obj = p.objects().get(index)?;
        let img = obj.as_image_object().ok_or_else(|| Error::Invalid("not an image".into()))?;
        Ok(img.get_processed_image(&self.pdf).or_else(|_| img.get_raw_image())?)
    }

    /// "Bild ersetzen": the image object shows `image` in the same place.
    pub fn replace_object_image(
        &mut self,
        page: usize,
        index: usize,
        image: &image::DynamicImage,
    ) -> Result<()> {
        self.edit_objects("replace image", page, |objects| {
            let mut obj = objects.get(index)?;
            let img = obj.as_image_object_mut().ok_or_else(|| Error::Invalid("not an image".into()))?;
            img.set_image(image)?;
            Ok(())
        })
    }

    /// Whether the font `raw_name` of `page` can show every character of
    /// `text`. Fonts that are not embedded are complete (the reader
    /// substitutes them); full embedded TrueType/OpenType fonts are asked
    /// for their character map; subset fonts ("ABCDEF+Name") and fonts that
    /// cannot be read only count the characters the document already shows
    /// in them.
    fn font_covers(&mut self, page: usize, raw_name: &str, embedded: bool, text: &str) -> bool {
        if !embedded {
            return true;
        }
        let (prefix, base) = match raw_name.split_once('+') {
            Some((p, b)) if p.len() == 6 && p.chars().all(|c| c.is_ascii_uppercase()) => (true, b),
            _ => (false, raw_name),
        };
        let mut missing: std::collections::BTreeSet<char> =
            text.chars().filter(|c| !c.is_whitespace()).collect();
        if missing.is_empty() {
            return true;
        }
        if !prefix
            && let Ok(programs) = self.font_programs(page)
            && let Some(data) = programs.get(crate::outlines::base_name(base))
            && let Ok(face) = skrifa::FontRef::new(data)
        {
            use skrifa::MetadataProvider;
            let map = face.charmap();
            return missing.iter().all(|c| map.map(*c).is_some());
        }
        let same = |name: &str| name.split_once('+').map_or(name, |(_, b)| b) == base;
        for p in 0..self.page_count() {
            let Ok(t) = self.page_text_styled(p) else { continue };
            for (i, c) in t.chars.iter().enumerate() {
                let font = t.styles.get(i).and_then(|s| t.fonts.get(s.font as usize));
                if font.is_some_and(|f| same(&f.name)) {
                    missing.remove(&c.ch);
                }
            }
            if missing.is_empty() {
                return true;
            }
        }
        false
    }

    /// "Text bearbeiten": replaces the text objects `old` by `text` set in
    /// `style`. The first line's baseline is at `baseline`, lines are
    /// `leading` apart and wrapped to `width` (0: no wrapping), starting at
    /// `left` (alignment within `width`). Returns the indices of the new
    /// objects.
    #[allow(clippy::too_many_arguments)]
    pub fn replace_text_block(
        &mut self,
        page: usize,
        old: &[usize],
        text: &str,
        style: &TextStyle,
        left: f32,
        baseline: f32,
        width: f32,
        leading: f32,
    ) -> Result<Vec<usize>> {
        self.require(self.permissions().modify)?;
        let idx = page_index(page)?;
        let mut old = old.to_vec();
        old.sort_unstable();
        old.dedup();
        // Font, look and position of the first old object.
        let (orig_font, raw_name, embedded, serif, mono, pos) = {
            let p = self.page(page)?;
            let objects = p.objects();
            let first = old.first().copied().unwrap_or(objects.len());
            let found = objects.get(first).ok().and_then(|o| {
                o.as_text_object().map(|t| {
                    let font = t.font();
                    (font.token(), font.name(), font.is_embedded().unwrap_or(true), font_look(&font))
                })
            });
            match found {
                Some((token, raw, embedded, (_, _, _, serif, mono))) => {
                    (Some(token), raw, embedded, serif, mono, first)
                }
                None => (None, String::new(), false, false, false, objects.len()),
            }
        };
        // The original font only if it has a glyph for every character.
        let orig_font = orig_font.filter(|_| self.font_covers(page, &raw_name, embedded, text));
        let size = style.size.clamp(1.0, 1000.0);
        let mut new_count = 0;
        self.edit_engine("edit text", |pdf| {
            let std = standard_font(pdf.fonts_mut(), &style.font, serif, mono, style.bold, style.italic);
            let system = if win_ansi_only(text) {
                None
            } else {
                system_font_bytes(style.bold)
                    .and_then(|b| pdf.fonts_mut().load_true_type_from_bytes(&b, true).ok())
            };
            let doc: &PdfDocument = pdf;
            let mut p = doc.pages().get(idx)?;
            // Candidate fonts in order; the first one that shows the text wins.
            let mut candidates = Vec::new();
            if style.font == FontChoice::Original
                && let Some(t) = orig_font
            {
                candidates.push(t);
            }
            if let Some(s) = system {
                candidates.push(s);
            }
            candidates.push(std);
            let measure_with = |token: PdfFontToken| {
                move |s: &str| -> f32 {
                    if s.is_empty() {
                        return 0.0;
                    }
                    PdfPageTextObject::new(doc, s, token, PdfPoints::new(size))
                        .ok()
                        .and_then(|o| o.width().ok())
                        .map_or(s.chars().count() as f32 * size * 0.5, |w| w.value)
                }
            };
            // The old objects go first: new text on top of the same old text
            // would be taken for "fake bold" by the text extraction.
            for i in old.iter().rev() {
                p.objects_mut().remove_object_at_index(*i)?;
            }
            for (n, token) in candidates.iter().enumerate() {
                let measure = measure_with(*token);
                let lines = if width > 1.0 {
                    wrap_text(text, width, &measure)
                } else {
                    text.split('\n').map(str::to_string).collect()
                };
                let mut placed = 0;
                let mut ok = true;
                for (k, line) in lines.iter().enumerate() {
                    if line.is_empty() {
                        continue;
                    }
                    let w = measure(line);
                    let x = match style.align {
                        Align::Left => left,
                        Align::Center => left + (width.max(w) - w) / 2.0,
                        Align::Right => left + width.max(w) - w,
                    };
                    let y = baseline - leading * k as f32;
                    let mut t = PdfPageTextObject::new(doc, line, *token, PdfPoints::new(size))?;
                    t.set_fill_color(to_pdf_color(style.color))?;
                    t.translate(PdfPoints::new(x), PdfPoints::new(y))?;
                    let added = p.objects_mut().insert_object_at_index(pos + placed, t.into())?;
                    placed += 1;
                    // The font must show every character (subset fonts often lack some).
                    let shown = added.as_text_object().map(|t| t.text()).unwrap_or_default();
                    if normalize(&shown) != normalize(line) {
                        ok = false;
                    }
                }
                let last = n + 1 == candidates.len();
                if ok || last {
                    new_count = placed;
                    break;
                }
                for _ in 0..placed {
                    p.objects_mut().remove_object_at_index(pos)?;
                }
            }
            Ok(())
        })?;
        Ok((pos..pos + new_count).collect())
    }
}
