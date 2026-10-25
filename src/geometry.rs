//! Coordinate systems.
//!
//! * **PDF user space**: points (1/72 inch), origin bottom-left of the media
//!   box, y grows upwards. All annotation data is stored in this space.
//! * **View space**: points, origin at the top-left corner of the page *as it
//!   is displayed* (crop box, after applying the page's `/Rotate`), y grows
//!   downwards. Multiply by the zoom factor to obtain screen coordinates.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct PointF {
    pub x: f32,
    pub y: f32,
}

impl PointF {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub fn distance(self, other: PointF) -> f32 {
        ((self.x - other.x).powi(2) + (self.y - other.y).powi(2)).sqrt()
    }
}

/// Axis aligned rectangle. In PDF space `bottom <= top`; in view space the
/// same struct is used with `top <= bottom` semantics avoided by always using
/// [`RectF::normalized`] which guarantees `x0 <= x1` and `y0 <= y1`.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct RectF {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl RectF {
    pub const fn new(x0: f32, y0: f32, x1: f32, y1: f32) -> Self {
        Self { x0, y0, x1, y1 }
    }

    pub fn from_points(a: PointF, b: PointF) -> Self {
        Self::new(a.x, a.y, b.x, b.y).normalized()
    }

    pub fn normalized(self) -> Self {
        Self {
            x0: self.x0.min(self.x1),
            y0: self.y0.min(self.y1),
            x1: self.x0.max(self.x1),
            y1: self.y0.max(self.y1),
        }
    }

    pub fn width(&self) -> f32 {
        (self.x1 - self.x0).abs()
    }

    pub fn height(&self) -> f32 {
        (self.y1 - self.y0).abs()
    }

    pub fn is_empty(&self) -> bool {
        self.width() <= f32::EPSILON || self.height() <= f32::EPSILON
    }

    pub fn contains(&self, p: PointF) -> bool {
        let r = self.normalized();
        p.x >= r.x0 && p.x <= r.x1 && p.y >= r.y0 && p.y <= r.y1
    }

    pub fn intersects(&self, other: &RectF) -> bool {
        let a = self.normalized();
        let b = other.normalized();
        a.x0 <= b.x1 && b.x0 <= a.x1 && a.y0 <= b.y1 && b.y0 <= a.y1
    }

    pub fn union(&self, other: &RectF) -> RectF {
        let a = self.normalized();
        let b = other.normalized();
        RectF::new(a.x0.min(b.x0), a.y0.min(b.y0), a.x1.max(b.x1), a.y1.max(b.y1))
    }

    pub fn inflate(&self, d: f32) -> RectF {
        let r = self.normalized();
        RectF::new(r.x0 - d, r.y0 - d, r.x1 + d, r.y1 + d)
    }

    pub fn translate(&self, dx: f32, dy: f32) -> RectF {
        RectF::new(self.x0 + dx, self.y0 + dy, self.x1 + dx, self.y1 + dy)
    }

    pub fn center(&self) -> PointF {
        PointF::new((self.x0 + self.x1) / 2.0, (self.y0 + self.y1) / 2.0)
    }

    pub fn bounding(points: impl IntoIterator<Item = PointF>) -> Option<RectF> {
        let mut iter = points.into_iter();
        let first = iter.next()?;
        let mut r = RectF::new(first.x, first.y, first.x, first.y);
        for p in iter {
            r.x0 = r.x0.min(p.x);
            r.y0 = r.y0.min(p.y);
            r.x1 = r.x1.max(p.x);
            r.y1 = r.y1.max(p.y);
        }
        Some(r)
    }
}

/// Page rotation in clockwise quarter turns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Rotation {
    #[default]
    R0,
    R90,
    R180,
    R270,
}

impl Rotation {
    pub fn from_degrees(degrees: i32) -> Self {
        match degrees.rem_euclid(360) {
            90 => Rotation::R90,
            180 => Rotation::R180,
            270 => Rotation::R270,
            _ => Rotation::R0,
        }
    }

    pub fn degrees(self) -> i32 {
        match self {
            Rotation::R0 => 0,
            Rotation::R90 => 90,
            Rotation::R180 => 180,
            Rotation::R270 => 270,
        }
    }

    pub fn rotated_by(self, delta_degrees: i32) -> Self {
        Self::from_degrees(self.degrees() + delta_degrees)
    }

    pub fn is_quarter(self) -> bool {
        matches!(self, Rotation::R90 | Rotation::R270)
    }
}

/// Everything needed to map between PDF user space and view space for a page.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageGeometry {
    /// The visible area (crop box) in PDF user space.
    pub crop: RectF,
    pub rotation: Rotation,
}

impl PageGeometry {
    pub fn new(crop: RectF, rotation: Rotation) -> Self {
        Self {
            crop: crop.normalized(),
            rotation,
        }
    }

    /// Size of the page in view space (points).
    pub fn view_size(&self) -> (f32, f32) {
        let (w, h) = (self.crop.width(), self.crop.height());
        if self.rotation.is_quarter() { (h, w) } else { (w, h) }
    }

    pub fn pdf_to_view(&self, p: PointF) -> PointF {
        let c = &self.crop;
        match self.rotation {
            Rotation::R0 => PointF::new(p.x - c.x0, c.y1 - p.y),
            Rotation::R90 => PointF::new(p.y - c.y0, p.x - c.x0),
            Rotation::R180 => PointF::new(c.x1 - p.x, p.y - c.y0),
            Rotation::R270 => PointF::new(c.y1 - p.y, c.x1 - p.x),
        }
    }

    pub fn view_to_pdf(&self, v: PointF) -> PointF {
        let c = &self.crop;
        match self.rotation {
            Rotation::R0 => PointF::new(v.x + c.x0, c.y1 - v.y),
            Rotation::R90 => PointF::new(v.y + c.x0, v.x + c.y0),
            Rotation::R180 => PointF::new(c.x1 - v.x, v.y + c.y0),
            Rotation::R270 => PointF::new(c.x1 - v.y, c.y1 - v.x),
        }
    }

    pub fn pdf_rect_to_view(&self, r: &RectF) -> RectF {
        let a = self.pdf_to_view(PointF::new(r.x0, r.y0));
        let b = self.pdf_to_view(PointF::new(r.x1, r.y1));
        RectF::from_points(a, b)
    }

    pub fn view_rect_to_pdf(&self, r: &RectF) -> RectF {
        let a = self.view_to_pdf(PointF::new(r.x0, r.y0));
        let b = self.view_to_pdf(PointF::new(r.x1, r.y1));
        RectF::from_points(a, b)
    }

    /// Converts a displacement in view space into PDF space.
    pub fn view_delta_to_pdf(&self, dx: f32, dy: f32) -> (f32, f32) {
        let o = self.view_to_pdf(PointF::new(0.0, 0.0));
        let p = self.view_to_pdf(PointF::new(dx, dy));
        (p.x - o.x, p.y - o.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geometry(rotation: Rotation) -> PageGeometry {
        PageGeometry::new(RectF::new(10.0, 20.0, 210.0, 320.0), rotation)
    }

    #[test]
    fn view_size_swaps_for_quarter_turns() {
        assert_eq!(geometry(Rotation::R0).view_size(), (200.0, 300.0));
        assert_eq!(geometry(Rotation::R90).view_size(), (300.0, 200.0));
        assert_eq!(geometry(Rotation::R180).view_size(), (200.0, 300.0));
        assert_eq!(geometry(Rotation::R270).view_size(), (300.0, 200.0));
    }

    #[test]
    fn round_trip_all_rotations() {
        for rot in [Rotation::R0, Rotation::R90, Rotation::R180, Rotation::R270] {
            let g = geometry(rot);
            for p in [
                PointF::new(10.0, 20.0),
                PointF::new(210.0, 320.0),
                PointF::new(57.5, 111.25),
            ] {
                let back = g.view_to_pdf(g.pdf_to_view(p));
                assert!((back.x - p.x).abs() < 1e-4 && (back.y - p.y).abs() < 1e-4, "{rot:?}");
            }
        }
    }

    #[test]
    fn corners_map_into_view_box() {
        for rot in [Rotation::R0, Rotation::R90, Rotation::R180, Rotation::R270] {
            let g = geometry(rot);
            let (w, h) = g.view_size();
            let v = g.pdf_rect_to_view(&g.crop);
            assert_eq!(v, RectF::new(0.0, 0.0, w, h), "{rot:?}");
        }
    }

    #[test]
    fn unrotated_top_left_is_origin() {
        let g = geometry(Rotation::R0);
        assert_eq!(g.pdf_to_view(PointF::new(10.0, 320.0)), PointF::new(0.0, 0.0));
    }

    #[test]
    fn rotation_arithmetic() {
        assert_eq!(Rotation::R270.rotated_by(90), Rotation::R0);
        assert_eq!(Rotation::R0.rotated_by(-90), Rotation::R270);
        assert_eq!(Rotation::from_degrees(-180), Rotation::R180);
    }

    #[test]
    fn rect_helpers() {
        let a = RectF::new(0.0, 0.0, 10.0, 10.0);
        let b = RectF::new(5.0, 5.0, 20.0, 20.0);
        assert!(a.intersects(&b));
        assert_eq!(a.union(&b), RectF::new(0.0, 0.0, 20.0, 20.0));
        assert!(!a.intersects(&RectF::new(11.0, 11.0, 12.0, 12.0)));
        assert_eq!(RectF::new(5.0, 6.0, 1.0, 2.0).normalized(), RectF::new(1.0, 2.0, 5.0, 6.0));
    }
}
