//! Page content as editable objects ("Text bearbeiten", "Objekte
//! bearbeiten"): object descriptions, affine
//! matrices, grouping text objects into blocks and line wrapping. The
//! PDFium side is in `document/content.rs`.

use crate::geometry::{PointF, RectF};
use crate::model::Rgba;

/// Kind of a page object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObjectKind {
    Text,
    Image,
    Path,
    Shading,
    /// Form XObject (a group of objects).
    Form,
    Other,
}

/// Which objects "Objekte bearbeiten" offers ("Alle", "Text", "Bilder",
/// "Formen").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ObjectFilter {
    #[default]
    All,
    Text,
    Images,
    Shapes,
}

impl ObjectFilter {
    pub fn accepts(self, kind: ObjectKind) -> bool {
        match self {
            ObjectFilter::All => true,
            ObjectFilter::Text => kind == ObjectKind::Text,
            ObjectFilter::Images => kind == ObjectKind::Image,
            ObjectFilter::Shapes => matches!(kind, ObjectKind::Path | ObjectKind::Shading | ObjectKind::Form),
        }
    }
}

/// An object on a page as VersoloPDF shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct ContentObject {
    /// Index among the page objects (back to front).
    pub index: usize,
    pub kind: ObjectKind,
    /// Corners in PDF space (rotated objects are not axis aligned).
    pub quad: [PointF; 4],
    /// Bounding box in PDF space.
    pub rect: RectF,
    /// Object matrix (a b c d e f).
    pub matrix: Affine,
    pub fill: Option<Rgba>,
    pub stroke: Option<Rgba>,
    pub stroke_width: f32,
    // Text objects:
    pub text: String,
    pub font: String,
    /// Font size on the page (unscaled size × matrix scale).
    pub font_size: f32,
    pub bold: bool,
    pub italic: bool,
    pub serif: bool,
    pub monospace: bool,
    /// Invisible text (e.g. the OCR layer of a scan).
    pub invisible: bool,
}

impl ContentObject {
    /// Baseline origin of a text object.
    pub fn origin(&self) -> PointF {
        PointF::new(self.matrix.0[4], self.matrix.0[5])
    }

    /// Whether the object is drawn without rotation or shear.
    pub fn upright(&self) -> bool {
        let [a, b, c, d, ..] = self.matrix.0;
        b.abs() < 1e-3 * a.abs().max(1e-6) && c.abs() < 1e-3 * d.abs().max(1e-6) && a > 0.0 && d > 0.0
    }
}

// ---------------------------------------------------------------------------
// Matrices

/// An affine matrix as in PDF: x' = a·x + c·y + e, y' = b·x + d·y + f.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affine(pub [f32; 6]);

impl Default for Affine {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Affine {
    pub const IDENTITY: Affine = Affine([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    pub fn translate(dx: f32, dy: f32) -> Self {
        Affine([1.0, 0.0, 0.0, 1.0, dx, dy])
    }

    pub fn scale(sx: f32, sy: f32) -> Self {
        Affine([sx, 0.0, 0.0, sy, 0.0, 0.0])
    }

    /// Counter-clockwise rotation (PDF space, y up) by `degrees`.
    pub fn rotate(degrees: f32) -> Self {
        let (s, c) = degrees.to_radians().sin_cos();
        // Snap exact quarter turns to avoid tiny shear.
        let snap = |v: f32| if v.abs() < 1e-6 { 0.0 } else { v };
        let (s, c) = (snap(s), snap(c));
        Affine([c, s, -s, c, 0.0, 0.0])
    }

    /// `self` followed by `next`.
    pub fn then(self, next: Affine) -> Affine {
        let [a, b, c, d, e, f] = self.0;
        let [a2, b2, c2, d2, e2, f2] = next.0;
        Affine([
            a * a2 + b * c2,
            a * b2 + b * d2,
            c * a2 + d * c2,
            c * b2 + d * d2,
            e * a2 + f * c2 + e2,
            e * b2 + f * d2 + f2,
        ])
    }

    /// `m` applied around the point `p` (scale, rotate, flip).
    pub fn about(m: Affine, p: PointF) -> Affine {
        Affine::translate(-p.x, -p.y).then(m).then(Affine::translate(p.x, p.y))
    }

    pub fn apply(&self, p: PointF) -> PointF {
        let [a, b, c, d, e, f] = self.0;
        PointF::new(a * p.x + c * p.y + e, b * p.x + d * p.y + f)
    }

    pub fn invert(&self) -> Option<Affine> {
        let [a, b, c, d, e, f] = self.0;
        let det = a * d - b * c;
        if det.abs() < 1e-9 {
            return None;
        }
        let (ia, ib, ic, id) = (d / det, -b / det, -c / det, a / det);
        Some(Affine([ia, ib, ic, id, -(e * ia + f * ic), -(e * ib + f * id)]))
    }

    /// Scale factor along y (the font size factor of upright text).
    pub fn scale_y(&self) -> f32 {
        let [_, _, c, d, ..] = self.0;
        (c * c + d * d).sqrt()
    }
}

/// Bounding box of points.
pub fn bounds(points: &[PointF]) -> RectF {
    let mut r = RectF::new(f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in points {
        r.x0 = r.x0.min(p.x);
        r.y0 = r.y0.min(p.y);
        r.x1 = r.x1.max(p.x);
        r.y1 = r.y1.max(p.y);
    }
    if r.x0 > r.x1 { RectF::default() } else { r }
}

/// Scaling of `from` onto `to` (resize handles), as matrix.
pub fn fit_rect(from: &RectF, to: &RectF) -> Affine {
    let sx = if from.width().abs() < 1e-3 { 1.0 } else { to.width() / from.width() };
    let sy = if from.height().abs() < 1e-3 { 1.0 } else { to.height() / from.height() };
    Affine::translate(-from.x0, -from.y0).then(Affine::scale(sx, sy)).then(Affine::translate(to.x0, to.y0))
}

// ---------------------------------------------------------------------------
// Text blocks

/// A paragraph of text objects that "Text bearbeiten" edits as a whole.
#[derive(Debug, Clone, PartialEq)]
pub struct TextBlock {
    /// Indices of the text objects (page object indices).
    pub objects: Vec<usize>,
    pub rect: RectF,
    pub lines: Vec<String>,
    /// Baseline of every line (PDF y).
    pub baselines: Vec<f32>,
    /// Left edge of the first line.
    pub left: f32,
    pub size: f32,
    pub font: String,
    pub bold: bool,
    pub italic: bool,
    pub serif: bool,
    pub monospace: bool,
    pub color: Rgba,
    /// Alignment guessed from the lines.
    pub align: Align,
}

impl TextBlock {
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    /// Distance of the baselines (or 1.2 × size for one line).
    pub fn leading(&self) -> f32 {
        if self.baselines.len() >= 2 {
            let n = (self.baselines.len() - 1) as f32;
            ((self.baselines[0] - self.baselines[self.baselines.len() - 1]) / n).abs().max(self.size * 0.8)
        } else {
            self.size * 1.2
        }
    }
}

/// Horizontal alignment of a text block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

/// A horizontal run of text objects on one baseline.
#[derive(Debug, Clone)]
struct Segment {
    objects: Vec<usize>,
    text: String,
    rect: RectF,
    baseline: f32,
    size: f32,
    first: usize,
}

/// Groups the visible, upright text objects into blocks: objects on one
/// baseline close together form a line segment (far apart: columns), and
/// segments below each other with the same size and left edge (or centre)
/// form a block.
pub fn text_blocks(objects: &[ContentObject]) -> Vec<TextBlock> {
    let mut items: Vec<&ContentObject> = objects
        .iter()
        .filter(|o| o.kind == ObjectKind::Text && !o.invisible && o.upright() && !o.text.trim().is_empty())
        .collect();
    items.sort_by(|a, b| {
        b.origin()
            .y
            .partial_cmp(&a.origin().y)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.rect.x0.partial_cmp(&b.rect.x0).unwrap_or(std::cmp::Ordering::Equal))
    });
    // Lines and segments.
    let mut segments: Vec<Segment> = Vec::new();
    for o in items {
        let size = o.font_size.max(1.0);
        let y = o.origin().y;
        let joined = segments.iter_mut().rev().take(8).find(|s| {
            (s.baseline - y).abs() < 0.3 * size
                && (s.size - size).abs() < 0.3 * size.max(s.size)
                && o.rect.x0 - s.rect.x1 < 1.2 * size
                && o.rect.x1 > s.rect.x0
        });
        match joined {
            Some(s) => {
                let gap = o.rect.x0 - s.rect.x1;
                if gap > 0.15 * size && !s.text.ends_with(' ') && !o.text.starts_with(' ') {
                    s.text.push(' ');
                }
                s.text.push_str(&o.text);
                s.rect = s.rect.union(&o.rect);
                s.objects.push(o.index);
            }
            None => segments.push(Segment {
                objects: vec![o.index],
                text: o.text.clone(),
                rect: o.rect,
                baseline: y,
                size,
                first: o.index,
            }),
        }
    }
    // Blocks.
    let style = |i: usize| objects.iter().find(|o| o.index == i);
    let mut blocks: Vec<(Vec<Segment>, RectF)> = Vec::new();
    segments.sort_by(|a, b| b.baseline.partial_cmp(&a.baseline).unwrap_or(std::cmp::Ordering::Equal));
    for seg in segments {
        let fits = blocks.iter_mut().rev().find(|(segs, rect)| {
            let last = segs.last().expect("blocks are not empty");
            let gap = last.baseline - seg.baseline;
            let same_size = (last.size - seg.size).abs() < 0.15 * last.size.max(seg.size);
            let left = (last.rect.x0 - seg.rect.x0).abs() < 1.5 * seg.size;
            let centre = ((last.rect.x0 + last.rect.x1) - (seg.rect.x0 + seg.rect.x1)).abs() < 2.0 * seg.size;
            let overlap = seg.rect.x0 < rect.x1 && seg.rect.x1 > rect.x0;
            same_size && (left || centre) && overlap && gap > 0.7 * seg.size && gap < 2.1 * seg.size
        });
        match fits {
            Some((segs, rect)) => {
                *rect = rect.union(&seg.rect);
                segs.push(seg);
            }
            None => {
                let r = seg.rect;
                blocks.push((vec![seg], r));
            }
        }
    }
    blocks
        .into_iter()
        .map(|(segs, rect)| {
            // The look of the block is that of its longest text (a bullet or
            // a symbol in front of a line has another font and size).
            let main = segs
                .iter()
                .flat_map(|s| s.objects.iter().copied())
                .filter_map(style)
                .max_by_key(|o| o.text.chars().count())
                .map(|o| o.index)
                .unwrap_or(segs[0].first);
            let first = style(main);
            let lefts: Vec<f32> = segs.iter().map(|s| s.rect.x0).collect();
            let rights: Vec<f32> = segs.iter().map(|s| s.rect.x1).collect();
            let spread = |v: &[f32]| {
                v.iter().cloned().fold(f32::MIN, f32::max) - v.iter().cloned().fold(f32::MAX, f32::min)
            };
            let size = first.map_or(segs[0].size, |o| o.font_size.max(1.0));
            let align = if segs.len() < 2 || spread(&lefts) < 0.5 * size {
                Align::Left
            } else if spread(&rights) < 0.5 * size {
                Align::Right
            } else {
                Align::Center
            };
            TextBlock {
                objects: segs.iter().flat_map(|s| s.objects.iter().copied()).collect(),
                rect,
                lines: segs.iter().map(|s| s.text.trim_end().to_string()).collect(),
                baselines: segs.iter().map(|s| s.baseline).collect(),
                left: segs[0].rect.x0,
                size,
                font: first.map(|o| o.font.clone()).unwrap_or_default(),
                bold: first.is_some_and(|o| o.bold),
                italic: first.is_some_and(|o| o.italic),
                serif: first.is_some_and(|o| o.serif),
                monospace: first.is_some_and(|o| o.monospace),
                color: first.and_then(|o| o.fill).unwrap_or(Rgba::BLACK),
                align,
            }
        })
        .collect()
}

/// Wraps `text` (paragraphs separated by '\n') to lines no wider than
/// `width`; `measure` gives the width of a string. Words longer than a
/// line are cut.
pub fn wrap_text(text: &str, width: f32, measure: &dyn Fn(&str) -> f32) -> Vec<String> {
    let mut out = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in paragraph.split(' ') {
            let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if line.is_empty() || measure(&candidate) <= width {
                line = candidate;
            } else {
                out.push(std::mem::take(&mut line));
                line = word.to_string();
            }
            // A single word wider than the line is cut hard.
            while measure(&line) > width && line.chars().count() > 1 {
                let chars: Vec<char> = line.chars().collect();
                let mut cut = chars.len() - 1;
                while cut > 1 && measure(&chars[..cut].iter().collect::<String>()) > width {
                    cut -= 1;
                }
                out.push(chars[..cut].iter().collect());
                line = chars[cut..].iter().collect();
            }
        }
        out.push(line);
    }
    out
}

/// Whether all characters exist in WinAnsi (the encoding of the standard
/// fonts).
pub fn win_ansi_only(s: &str) -> bool {
    const EXTRA: &str = "€‚ƒ„…†‡ˆ‰Š‹ŒŽ‘’“”•–—˜™š›œžŸ";
    s.chars().all(|c| {
        (' '..='~').contains(&c) || ('\u{a0}'..='\u{ff}').contains(&c) || EXTRA.contains(c) || c == '\n'
    })
}

/// Font for new or changed text.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum FontChoice {
    /// The font of the edited text (falls back if characters are missing).
    #[default]
    Original,
    Helvetica,
    Times,
    Courier,
}

impl FontChoice {
    pub const ALL: [FontChoice; 4] =
        [FontChoice::Original, FontChoice::Helvetica, FontChoice::Times, FontChoice::Courier];

    pub fn label(&self, de: bool) -> &'static str {
        match self {
            FontChoice::Original => {
                if de {
                    "Originalschrift"
                } else {
                    "Original font"
                }
            }
            FontChoice::Helvetica => "Helvetica (Arial)",
            FontChoice::Times => "Times",
            FontChoice::Courier => "Courier",
        }
    }
}

/// Look of edited text.
#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    pub font: FontChoice,
    pub size: f32,
    pub color: Rgba,
    pub bold: bool,
    pub italic: bool,
    pub align: Align,
}

impl TextStyle {
    pub fn of_block(b: &TextBlock) -> Self {
        Self {
            font: FontChoice::Original,
            size: b.size,
            color: b.color,
            bold: b.bold,
            italic: b.italic,
            align: b.align,
        }
    }
}

/// Changes of the look of objects ("Format").
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ObjectStyle {
    pub fill: Option<Rgba>,
    pub stroke: Option<Rgba>,
    pub stroke_width: Option<f32>,
    /// Dash pattern (empty: solid).
    pub dash: Option<Vec<f32>>,
    /// Opacity of the fill (0–1).
    pub fill_opacity: Option<f32>,
    /// Opacity of the line (0–1).
    pub stroke_opacity: Option<f32>,
    pub blend: Option<BlendMode>,
}

/// "Mischmodus".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    Difference,
    Exclusion,
}

impl BlendMode {
    pub const ALL: [BlendMode; 8] = [
        BlendMode::Normal,
        BlendMode::Multiply,
        BlendMode::Screen,
        BlendMode::Overlay,
        BlendMode::Darken,
        BlendMode::Lighten,
        BlendMode::Difference,
        BlendMode::Exclusion,
    ];

    pub fn label(self, de: bool) -> &'static str {
        let (g, e) = match self {
            BlendMode::Normal => ("Normal", "Normal"),
            BlendMode::Multiply => ("Multiplizieren", "Multiply"),
            BlendMode::Screen => ("Negativ multiplizieren", "Screen"),
            BlendMode::Overlay => ("Überlagern", "Overlay"),
            BlendMode::Darken => ("Abdunkeln", "Darken"),
            BlendMode::Lighten => ("Aufhellen", "Lighten"),
            BlendMode::Difference => ("Differenz", "Difference"),
            BlendMode::Exclusion => ("Ausschluss", "Exclusion"),
        };
        if de { g } else { e }
    }
}

/// Stacking order changes ("Z-Order").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ZOrder {
    /// "Nach vorn": one step up.
    Forward,
    /// "Nach hinten": one step down.
    Backward,
    /// "In den Vordergrund".
    Front,
    /// "In den Hintergrund".
    Back,
}

/// New order of `count` objects when `selected` move as told. Returns the
/// old index of every new position.
pub fn z_order(count: usize, selected: &[usize], how: ZOrder) -> Vec<usize> {
    let sel: Vec<usize> = {
        let mut s: Vec<usize> = selected.iter().copied().filter(|i| *i < count).collect();
        s.sort_unstable();
        s.dedup();
        s
    };
    let mut order: Vec<usize> = (0..count).collect();
    match how {
        ZOrder::Front | ZOrder::Back => {
            let rest: Vec<usize> = order.iter().copied().filter(|i| !sel.contains(i)).collect();
            order = if how == ZOrder::Front {
                rest.into_iter().chain(sel).collect()
            } else {
                sel.into_iter().chain(rest).collect()
            };
        }
        ZOrder::Forward => {
            for pos in (0..count.saturating_sub(1)).rev() {
                if sel.contains(&order[pos]) && !sel.contains(&order[pos + 1]) {
                    order.swap(pos, pos + 1);
                }
            }
        }
        ZOrder::Backward => {
            for pos in 1..count {
                if sel.contains(&order[pos]) && !sel.contains(&order[pos - 1]) {
                    order.swap(pos, pos - 1);
                }
            }
        }
    }
    order
}

/// "Ausrichtung" and "Verteilen" of several objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Arrange {
    Left,
    CenterH,
    Right,
    Top,
    CenterV,
    Bottom,
    /// Equal gaps horizontally / vertically.
    SpaceH,
    SpaceV,
    /// Equal distance of the left edges / centres / right edges …
    DistLeft,
    DistCenterH,
    DistRight,
    DistTop,
    DistCenterV,
    DistBottom,
    SameSize,
    SameWidth,
    SameHeight,
}

/// Matrix per object (same order as `rects`) for `how`. `reference` is the
/// box to align to (selection or page); the first rect is the model for
/// "Selbe Größe".
pub fn arrange(rects: &[RectF], how: Arrange, reference: RectF) -> Vec<Affine> {
    let n = rects.len();
    let mut out = vec![Affine::IDENTITY; n];
    if n == 0 {
        return out;
    }
    let cx = |r: &RectF| (r.x0 + r.x1) / 2.0;
    let cy = |r: &RectF| (r.y0 + r.y1) / 2.0;
    let mv = |dx: f32, dy: f32| Affine::translate(dx, dy);
    match how {
        Arrange::Left => rects.iter().zip(&mut out).for_each(|(r, m)| *m = mv(reference.x0 - r.x0, 0.0)),
        Arrange::Right => rects.iter().zip(&mut out).for_each(|(r, m)| *m = mv(reference.x1 - r.x1, 0.0)),
        Arrange::CenterH => {
            rects.iter().zip(&mut out).for_each(|(r, m)| *m = mv(cx(&reference) - cx(r), 0.0))
        }
        Arrange::Top => rects.iter().zip(&mut out).for_each(|(r, m)| *m = mv(0.0, reference.y1 - r.y1)),
        Arrange::Bottom => rects.iter().zip(&mut out).for_each(|(r, m)| *m = mv(0.0, reference.y0 - r.y0)),
        Arrange::CenterV => {
            rects.iter().zip(&mut out).for_each(|(r, m)| *m = mv(0.0, cy(&reference) - cy(r)))
        }
        Arrange::SameSize | Arrange::SameWidth | Arrange::SameHeight => {
            let model = rects[0];
            for (r, m) in rects.iter().zip(&mut out).skip(1) {
                let w = if how == Arrange::SameHeight { r.width() } else { model.width() };
                let h = if how == Arrange::SameWidth { r.height() } else { model.height() };
                *m = fit_rect(r, &RectF::new(r.x0, r.y1 - h, r.x0 + w, r.y1));
            }
        }
        _ => {
            if n < 3 && !matches!(how, Arrange::SpaceH | Arrange::SpaceV) || n < 2 {
                return out;
            }
            let horizontal = matches!(
                how,
                Arrange::SpaceH | Arrange::DistLeft | Arrange::DistCenterH | Arrange::DistRight
            );
            let key = |r: &RectF| match how {
                Arrange::DistLeft | Arrange::SpaceH => r.x0,
                Arrange::DistCenterH => cx(r),
                Arrange::DistRight => r.x1,
                Arrange::DistTop => -r.y1,
                Arrange::DistCenterV => -cy(r),
                Arrange::DistBottom | Arrange::SpaceV => -r.y0,
                _ => 0.0,
            };
            let mut idx: Vec<usize> = (0..n).collect();
            idx.sort_by(|a, b| {
                key(&rects[*a]).partial_cmp(&key(&rects[*b])).unwrap_or(std::cmp::Ordering::Equal)
            });
            let (first, last) = (rects[idx[0]], rects[idx[n - 1]]);
            if matches!(how, Arrange::SpaceH | Arrange::SpaceV) {
                // Equal gaps between neighbours.
                let total: f32 =
                    idx.iter().map(|i| if horizontal { rects[*i].width() } else { rects[*i].height() }).sum();
                let span = if horizontal { last.x1 - first.x0 } else { first.y1 - last.y0 };
                let gap = (span - total) / (n - 1) as f32;
                let mut pos = if horizontal { first.x0 } else { first.y1 };
                for i in idx {
                    let r = rects[i];
                    out[i] = if horizontal { mv(pos - r.x0, 0.0) } else { mv(0.0, pos - r.y1) };
                    pos += if horizontal { r.width() + gap } else { -(r.height() + gap) };
                }
            } else {
                let (a, b) = (key(&first), key(&last));
                for (k, i) in idx.iter().enumerate() {
                    let want = a + (b - a) * k as f32 / (n - 1) as f32;
                    let d = want - key(&rects[*i]);
                    out[*i] = if horizontal { mv(d, 0.0) } else { mv(0.0, -d) };
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(index: usize, s: &str, x: f32, y: f32, size: f32) -> ContentObject {
        let w = s.chars().count() as f32 * size * 0.5;
        ContentObject {
            index,
            kind: ObjectKind::Text,
            quad: [PointF::default(); 4],
            rect: RectF::new(x, y - 0.2 * size, x + w, y + 0.8 * size),
            matrix: Affine::translate(x, y),
            fill: Some(Rgba::BLACK),
            stroke: None,
            stroke_width: 0.0,
            text: s.into(),
            font: "Arial".into(),
            font_size: size,
            bold: false,
            italic: false,
            serif: false,
            monospace: false,
            invisible: false,
        }
    }

    #[test]
    fn affine_maths() {
        let m = Affine::about(Affine::rotate(90.0), PointF::new(10.0, 10.0));
        let p = m.apply(PointF::new(20.0, 10.0));
        assert!((p.x - 10.0).abs() < 1e-4 && (p.y - 20.0).abs() < 1e-4, "{p:?}");
        let inv = m.invert().unwrap();
        let back = inv.apply(p);
        assert!((back.x - 20.0).abs() < 1e-4 && (back.y - 10.0).abs() < 1e-4);
        let f = fit_rect(&RectF::new(0.0, 0.0, 10.0, 10.0), &RectF::new(5.0, 5.0, 25.0, 15.0));
        assert_eq!(f.apply(PointF::new(10.0, 10.0)), PointF::new(25.0, 15.0));
    }

    #[test]
    fn blocks_from_lines_and_columns() {
        let objs = vec![
            text(0, "Haushaltsbuch", 50.0, 700.0, 24.0),
            text(1, "Dies ist eine", 50.0, 660.0, 12.0),
            text(2, "Beispielseite.", 140.0, 660.0, 12.0),
            text(3, "Zweite Zeile", 50.0, 646.0, 12.0),
            text(4, "Miete", 50.0, 600.0, 12.0),
            text(5, "850,00", 300.0, 600.0, 12.0),
        ];
        let blocks = text_blocks(&objs);
        let texts: Vec<String> = blocks.iter().map(TextBlock::text).collect();
        assert_eq!(texts, ["Haushaltsbuch", "Dies ist eine Beispielseite.\nZweite Zeile", "Miete", "850,00"]);
        assert_eq!(blocks[1].objects, vec![1, 2, 3]);
        assert!((blocks[1].leading() - 14.0).abs() < 0.01);
    }

    #[test]
    fn wrapping() {
        let measure = |s: &str| s.chars().count() as f32;
        assert_eq!(wrap_text("aa bb cc", 5.0, &measure), ["aa bb", "cc"]);
        assert_eq!(wrap_text("abcdefgh", 3.0, &measure), ["abc", "def", "gh"]);
        assert_eq!(wrap_text("a\n\nb", 9.0, &measure), ["a", "", "b"]);
        assert!(win_ansi_only("Grüße – 5 €") && !win_ansi_only("Ω"));
    }

    #[test]
    fn stacking_and_arranging() {
        assert_eq!(z_order(4, &[1], ZOrder::Front), vec![0, 2, 3, 1]);
        assert_eq!(z_order(4, &[2], ZOrder::Back), vec![2, 0, 1, 3]);
        assert_eq!(z_order(4, &[1], ZOrder::Forward), vec![0, 2, 1, 3]);
        assert_eq!(z_order(4, &[0], ZOrder::Backward), vec![0, 1, 2, 3]);
        let rects = [
            RectF::new(0.0, 0.0, 10.0, 10.0),
            RectF::new(50.0, 20.0, 60.0, 40.0),
            RectF::new(15.0, 5.0, 25.0, 10.0),
        ];
        let sel = rects.iter().skip(1).fold(rects[0], |a, r| a.union(r));
        let m = arrange(&rects, Arrange::Left, sel);
        assert_eq!(m[1].apply(PointF::new(50.0, 20.0)).x, 0.0);
        let d = arrange(&rects, Arrange::SpaceH, sel);
        // Widths 10+10+10 in 0..60: gaps of 15 → x0 = 0, 25, 50.
        assert_eq!(d[2].apply(PointF::new(15.0, 0.0)).x, 25.0);
        let s = arrange(&rects, Arrange::SameSize, sel);
        let r = s[1].apply(PointF::new(60.0, 20.0));
        assert_eq!((r.x, r.y), (60.0, 30.0));
    }
}
