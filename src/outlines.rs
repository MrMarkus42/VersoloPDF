//! "In Kurven umwandeln": glyph outlines of the page text as paths. The
//! outlines come from the font embedded in the PDF (TrueType/OpenType); if
//! it has no usable character table, a built-in sans serif font is used.

use std::collections::HashMap;

use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, MetadataProvider};

use crate::office::PathOp;

/// Collects the path of one glyph, moved and flipped into page view space.
struct Pen<'a> {
    ops: &'a mut Vec<PathOp>,
    /// Glyph origin in view space (y down).
    x: f32,
    y: f32,
}

impl Pen<'_> {
    fn p(&self, x: f32, y: f32) -> (f32, f32) {
        (self.x + x, self.y - y)
    }
}

impl OutlinePen for Pen<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        let (a, b) = self.p(x, y);
        self.ops.push(PathOp::Move(a, b));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let (a, b) = self.p(x, y);
        self.ops.push(PathOp::Line(a, b));
    }

    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        // Quadratic to cubic: control points at 2/3 towards the quad control.
        let start = match self.ops.last() {
            Some(PathOp::Move(a, b) | PathOp::Line(a, b) | PathOp::Cubic(_, _, _, _, a, b)) => (*a, *b),
            _ => self.p(cx, cy),
        };
        let (qx, qy) = self.p(cx, cy);
        let (ex, ey) = self.p(x, y);
        self.ops.push(PathOp::Cubic(
            start.0 + 2.0 / 3.0 * (qx - start.0),
            start.1 + 2.0 / 3.0 * (qy - start.1),
            ex + 2.0 / 3.0 * (qx - ex),
            ey + 2.0 / 3.0 * (qy - ey),
            ex,
            ey,
        ));
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let (a, b) = self.p(cx0, cy0);
        let (c, d) = self.p(cx1, cy1);
        let (e, f) = self.p(x, y);
        self.ops.push(PathOp::Cubic(a, b, c, d, e, f));
    }

    fn close(&mut self) {
        self.ops.push(PathOp::Close);
    }
}

/// Font programs by PDF font name (without subset prefix).
pub struct Fonts {
    programs: HashMap<String, Vec<u8>>,
    fallback: Vec<u8>,
}

/// "ABCDEF+Calibri-Bold" -> "Calibri-Bold".
pub fn base_name(name: &str) -> &str {
    match name.split_once('+') {
        Some((p, rest)) if p.len() == 6 && p.chars().all(|c| c.is_ascii_uppercase()) => rest,
        _ => name,
    }
}

impl Fonts {
    pub fn new(programs: HashMap<String, Vec<u8>>) -> Self {
        let fallback = egui::FontDefinitions::default()
            .font_data
            .get("Ubuntu-Light")
            .map(|f| (*f.font).as_ref().to_vec())
            .unwrap_or_default();
        Self { programs, fallback }
    }

    /// Appends the outline of `ch` drawn at `size` points with its origin
    /// at (x, y) in view space. Returns false if no font has the glyph.
    pub fn glyph(&self, font: &str, ch: char, size: f32, x: f32, y: f32, ops: &mut Vec<PathOp>) -> bool {
        let embedded = self.programs.get(base_name(font)).map(Vec::as_slice);
        for data in embedded.into_iter().chain([self.fallback.as_slice()]) {
            let Ok(face) = FontRef::new(data) else { continue };
            let Some(gid) = face.charmap().map(ch) else { continue };
            let Some(glyph) = face.outline_glyphs().get(gid) else { continue };
            let mut pen = Pen { ops, x, y };
            if glyph.draw(DrawSettings::unhinted(Size::new(size), LocationRef::default()), &mut pen).is_ok() {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_font_draws_letters() {
        let fonts = Fonts::new(HashMap::new());
        let mut ops = Vec::new();
        assert!(fonts.glyph("Unknown", 'A', 10.0, 100.0, 200.0, &mut ops));
        assert!(matches!(ops.first(), Some(PathOp::Move(..))));
        // The letter sits on the baseline (y 200) and rises above it.
        let ys: Vec<f32> = ops
            .iter()
            .filter_map(|o| match o {
                PathOp::Move(_, y) | PathOp::Line(_, y) => Some(*y),
                _ => None,
            })
            .collect();
        assert!(ys.iter().all(|y| *y <= 200.5 && *y > 190.0), "{ys:?}");
        assert_eq!(base_name("ABCDEF+Calibri"), "Calibri");
        assert_eq!(base_name("Arial+X"), "Arial+X");
    }
}
