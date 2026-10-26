//! Generation of annotation appearance streams.
//!
//! Every annotation MrPDF creates carries its own appearance stream so it
//! looks identical in all PDF viewers. Coordinates in the generated content
//! are absolute PDF user space coordinates; the form's `/BBox` equals the
//! annotation `/Rect` and its matrix is the identity.

use std::fmt::Write as _;

use crate::geometry::{PointF, RectF};
use crate::model::{AnnotKind, Rgba};

/// The standard-14 fonts used in generated appearances.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StdFont {
    Helvetica,
    HelveticaBold,
    /// Fixed width (every glyph 600/1000 em).
    Courier,
}

impl StdFont {
    pub fn base_font(self) -> &'static str {
        match self {
            StdFont::Helvetica => "Helvetica",
            StdFont::HelveticaBold => "Helvetica-Bold",
            StdFont::Courier => "Courier",
        }
    }

    pub fn resource_name(self) -> &'static str {
        match self {
            StdFont::Helvetica => "Helv",
            StdFont::HelveticaBold => "HeBo",
            StdFont::Courier => "Cour",
        }
    }

    /// Advance width of a WinAnsi byte in 1/1000 em.
    pub fn width(self, byte: u8) -> u16 {
        const HELV: [u16; 95] = [
            278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556,
            556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667,
            611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667,
            667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500,
            222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
        ];
        const BOLD: [u16; 95] = [
            278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556,
            556, 556, 556, 556, 556, 556, 556, 333, 333, 584, 584, 584, 611, 975, 722, 722, 722, 722, 667,
            611, 778, 722, 278, 556, 722, 611, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667,
            667, 611, 333, 278, 333, 584, 556, 333, 556, 611, 556, 611, 556, 333, 611, 611, 278, 278, 556,
            278, 889, 611, 611, 611, 611, 389, 556, 333, 611, 556, 778, 556, 556, 500, 389, 280, 389, 584,
        ];
        let table = match self {
            StdFont::Helvetica => &HELV,
            StdFont::HelveticaBold => &BOLD,
            StdFont::Courier => return 600,
        };
        match byte {
            32..=126 => table[usize::from(byte - 32)],
            // Accented letters: approximate with the width of a lower case letter.
            0xDF => 611,
            0xC0..=0xFF => 600,
            _ => 556,
        }
    }

    pub fn text_width(self, text: &[u8], size: f32) -> f32 {
        text.iter().map(|&b| f32::from(self.width(b))).sum::<f32>() * size / 1000.0
    }
}

/// Encodes text into WinAnsiEncoding; unsupported characters become `?`.
pub fn win_ansi(text: &str) -> Vec<u8> {
    text.chars()
        .map(|ch| match ch {
            ' '..='~' => ch as u8,
            '\u{A0}'..='\u{FF}' => ch as u32 as u8,
            '€' => 0x80,
            '‚' => 0x82,
            '„' => 0x84,
            '…' => 0x85,
            '†' => 0x86,
            '‡' => 0x87,
            '‰' => 0x89,
            '‹' => 0x8B,
            '‘' => 0x91,
            '’' => 0x92,
            '“' => 0x93,
            '”' => 0x94,
            '•' => 0x95,
            '–' => 0x96,
            '—' => 0x97,
            '™' => 0x99,
            '›' => 0x9B,
            '\t' => b' ',
            _ => b'?',
        })
        .collect()
}

/// Graphics resources the content stream refers to.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ApResources {
    /// Stroke and fill opacity for `/GS0`, if not fully opaque.
    pub opacity: Option<f32>,
    /// Use the multiply blend mode (highlights).
    pub multiply: bool,
    pub fonts: Vec<StdFont>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Appearance {
    pub bbox: RectF,
    pub content: Vec<u8>,
    pub resources: ApResources,
}

/// Everything required to draw an annotation.
#[derive(Debug, Clone, PartialEq)]
pub struct ApInput {
    pub kind: AnnotKind,
    pub rect: RectF,
    pub color: Rgba,
    pub fill: Option<Rgba>,
    pub width: f32,
    pub opacity: f32,
    /// Text markup quads (one rectangle per line).
    pub quads: Vec<RectF>,
    /// Line end points, ink strokes or polygon vertices.
    pub paths: Vec<Vec<PointF>>,
    pub arrow_end: bool,
    pub text: String,
    pub font_size: f32,
    pub text_color: Rgba,
    /// Polygon drawn with a cloudy border.
    pub cloudy: bool,
    /// Text box of a callout (the rest of `rect` holds the leader line).
    pub inner: Option<RectF>,
    /// Callout leader line from the arrow tip towards the box.
    pub leader: Vec<PointF>,
    /// Measurement label drawn next to lines and polygons.
    pub label: String,
}

impl ApInput {
    pub fn new(kind: AnnotKind, rect: RectF, color: Rgba) -> Self {
        Self {
            kind,
            rect,
            color,
            fill: None,
            width: 1.0,
            opacity: 1.0,
            quads: Vec::new(),
            paths: Vec::new(),
            arrow_end: false,
            text: String::new(),
            font_size: 12.0,
            text_color: Rgba::BLACK,
            cloudy: false,
            inner: None,
            leader: Vec::new(),
            label: String::new(),
        }
    }
}

/// Formats a number compactly for content streams.
pub fn num(v: f32) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    let s = format!("{:.3}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" || s.is_empty() { "0".into() } else { s.to_string() }
}

struct Builder {
    out: String,
    bytes: Vec<u8>,
}

impl Builder {
    fn new() -> Self {
        Self { out: String::new(), bytes: Vec::new() }
    }

    fn op(&mut self, s: &str) {
        self.out.push_str(s);
        self.out.push('\n');
    }

    fn stroke_color(&mut self, c: Rgba) {
        let [r, g, b] = c.to_pdf();
        let _ = writeln!(self.out, "{} {} {} RG", num(r), num(g), num(b));
    }

    fn fill_color(&mut self, c: Rgba) {
        let [r, g, b] = c.to_pdf();
        let _ = writeln!(self.out, "{} {} {} rg", num(r), num(g), num(b));
    }

    fn move_to(&mut self, p: PointF) {
        let _ = writeln!(self.out, "{} {} m", num(p.x), num(p.y));
    }

    fn line_to(&mut self, p: PointF) {
        let _ = writeln!(self.out, "{} {} l", num(p.x), num(p.y));
    }

    fn curve_to(&mut self, c1: PointF, c2: PointF, p: PointF) {
        let _ = writeln!(
            self.out,
            "{} {} {} {} {} {} c",
            num(c1.x),
            num(c1.y),
            num(c2.x),
            num(c2.y),
            num(p.x),
            num(p.y)
        );
    }

    fn rect(&mut self, r: &RectF) {
        let r = r.normalized();
        let _ = writeln!(self.out, "{} {} {} {} re", num(r.x0), num(r.y0), num(r.width()), num(r.height()));
    }

    fn ellipse(&mut self, r: &RectF) {
        const K: f32 = 0.552_284_8;
        let r = r.normalized();
        let (cx, cy) = ((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0);
        let (rx, ry) = (r.width() / 2.0, r.height() / 2.0);
        let (kx, ky) = (rx * K, ry * K);
        self.move_to(PointF::new(cx + rx, cy));
        self.curve_to(PointF::new(cx + rx, cy + ky), PointF::new(cx + kx, cy + ry), PointF::new(cx, cy + ry));
        self.curve_to(PointF::new(cx - kx, cy + ry), PointF::new(cx - rx, cy + ky), PointF::new(cx - rx, cy));
        self.curve_to(PointF::new(cx - rx, cy - ky), PointF::new(cx - kx, cy - ry), PointF::new(cx, cy - ry));
        self.curve_to(PointF::new(cx + kx, cy - ry), PointF::new(cx + rx, cy - ky), PointF::new(cx + rx, cy));
        self.op("h");
    }

    fn polyline(&mut self, points: &[PointF]) {
        if let Some((first, rest)) = points.split_first() {
            self.move_to(*first);
            if rest.is_empty() {
                // A single click: draw a dot.
                self.line_to(PointF::new(first.x + 0.01, first.y));
            }
            for p in rest {
                self.line_to(*p);
            }
        }
    }

    /// Writes a text show operation for already encoded bytes.
    fn show_text(&mut self, font: StdFont, size: f32, x: f32, y: f32, text: &[u8]) {
        let _ = write!(self.out, "BT /{} {} Tf {} {} Td (", font.resource_name(), num(size), num(x), num(y));
        self.flush_out();
        for &b in text {
            match b {
                b'(' | b')' | b'\\' => {
                    self.bytes.push(b'\\');
                    self.bytes.push(b);
                }
                _ => self.bytes.push(b),
            }
        }
        self.out.push_str(") Tj ET\n");
    }

    fn flush_out(&mut self) {
        self.bytes.extend_from_slice(self.out.as_bytes());
        self.out.clear();
    }

    fn finish(mut self) -> Vec<u8> {
        self.flush_out();
        self.bytes
    }
}

/// Splits text into lines that fit `max_width`.
pub fn wrap_text(text: &str, font: StdFont, size: f32, max_width: f32) -> Vec<Vec<u8>> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let words = win_ansi(paragraph.trim_end_matches('\r'));
        let mut line: Vec<u8> = Vec::new();
        for word in words.split(|&b| b == b' ') {
            let mut candidate = line.clone();
            if !candidate.is_empty() {
                candidate.push(b' ');
            }
            candidate.extend_from_slice(word);
            if line.is_empty() || font.text_width(&candidate, size) <= max_width {
                line = candidate;
            } else {
                lines.push(std::mem::take(&mut line));
                line.extend_from_slice(word);
            }
            // Hard-break words longer than the available width.
            while font.text_width(&line, size) > max_width && line.len() > 1 {
                let mut cut = line.len() - 1;
                while cut > 1 && font.text_width(&line[..cut], size) > max_width {
                    cut -= 1;
                }
                let rest = line.split_off(cut);
                lines.push(std::mem::replace(&mut line, rest));
            }
        }
        lines.push(line);
    }
    lines
}

/// Builds the normal appearance for `input`.
pub fn build(input: &ApInput) -> Appearance {
    let mut b = Builder::new();
    let mut res = ApResources::default();
    let opacity = input.opacity.clamp(0.05, 1.0);
    if opacity < 0.999 || input.kind == AnnotKind::Highlight {
        res.opacity = Some(opacity);
        res.multiply = input.kind == AnnotKind::Highlight;
        b.op("/GS0 gs");
    }
    let w = input.width.max(0.0);
    let rect = input.rect.normalized();

    match input.kind {
        AnnotKind::Highlight => {
            b.fill_color(input.color);
            for q in &input.quads {
                b.rect(q);
            }
            b.op("f");
        }
        AnnotKind::Underline | AnnotKind::StrikeOut => {
            b.stroke_color(input.color);
            for q in &input.quads {
                let q = q.normalized();
                let lw = (q.height() / 14.0).max(0.6);
                let y =
                    if input.kind == AnnotKind::Underline { q.y0 + lw * 1.5 } else { (q.y0 + q.y1) / 2.0 };
                b.op(&format!("{} w", num(lw)));
                b.move_to(PointF::new(q.x0, y));
                b.line_to(PointF::new(q.x1, y));
                b.op("S");
            }
        }
        AnnotKind::Squiggly => {
            b.stroke_color(input.color);
            for q in &input.quads {
                let q = q.normalized();
                let amp = (q.height() / 12.0).max(0.8);
                let step = amp * 2.0;
                b.op(&format!("{} w", num(amp * 0.6)));
                let mut x = q.x0;
                let base = q.y0 + amp;
                b.move_to(PointF::new(x, base));
                let mut up = true;
                while x < q.x1 {
                    x = (x + step).min(q.x1);
                    b.line_to(PointF::new(x, if up { base + amp } else { base - amp }));
                    up = !up;
                }
                b.op("S");
            }
        }
        AnnotKind::Square | AnnotKind::Circle => {
            b.stroke_color(input.color);
            b.op(&format!("{} w", num(w)));
            let inner = rect.inflate(-w / 2.0);
            if input.kind == AnnotKind::Square {
                b.rect(&inner);
            } else {
                b.ellipse(&inner);
            }
            match (input.fill, w > 0.0) {
                (Some(fill), true) => {
                    b.fill_color(fill);
                    b.op("B");
                }
                (Some(fill), false) => {
                    b.fill_color(fill);
                    b.op("f");
                }
                (None, _) => b.op("S"),
            }
        }
        AnnotKind::Line => {
            b.stroke_color(input.color);
            b.op(&format!("{} w 1 J 1 j", num(w)));
            if let Some(points) = input.paths.first()
                && points.len() >= 2
            {
                let (from, to) = (points[0], points[points.len() - 1]);
                b.move_to(from);
                b.line_to(to);
                b.op("S");
                if input.arrow_end {
                    let (a, c) = arrow_head(from, to, w);
                    b.move_to(a);
                    b.line_to(to);
                    b.line_to(c);
                    b.op("S");
                }
            }
        }
        AnnotKind::Polygon if input.cloudy => {
            b.stroke_color(input.color);
            b.op(&format!("{} w 1 J 1 j", num(w.max(0.1))));
            if let Some(path) = input.paths.first() {
                cloud_path(&mut b, path, cloud_radius(w));
                if let Some(fill) = input.fill {
                    b.fill_color(fill);
                    b.op("B");
                } else {
                    b.op("S");
                }
            }
        }
        AnnotKind::Ink | AnnotKind::Polyline | AnnotKind::Polygon => {
            b.stroke_color(input.color);
            b.op(&format!("{} w 1 J 1 j", num(w.max(0.1))));
            for path in &input.paths {
                b.polyline(path);
                if input.kind == AnnotKind::Polygon {
                    b.op("h");
                }
                if let (AnnotKind::Polygon, Some(fill)) = (input.kind, input.fill) {
                    b.fill_color(fill);
                    b.op("B");
                } else {
                    b.op("S");
                }
            }
        }
        AnnotKind::FreeText => {
            if input.leader.len() >= 2 {
                // Callout leader with an open arrow at the tip.
                b.stroke_color(input.color);
                b.op(&format!("{} w 1 J 1 j", num(w.max(0.5))));
                b.polyline(&input.leader);
                b.op("S");
                let (a, c) = arrow_head(input.leader[1], input.leader[0], w.max(0.5));
                b.move_to(a);
                b.line_to(input.leader[0]);
                b.line_to(c);
                b.op("S");
            }
            let rect = input.inner.map(|r| r.normalized()).unwrap_or(rect);
            if let Some(fill) = input.fill {
                b.fill_color(fill);
                b.rect(&rect);
                b.op("f");
            }
            if w > 0.0 {
                b.stroke_color(input.color);
                b.op(&format!("{} w", num(w)));
                b.rect(&rect.inflate(-w / 2.0));
                b.op("S");
            }
            let font = StdFont::Helvetica;
            res.fonts.push(font);
            let size = input.font_size.max(1.0);
            let pad = 2.0 + w;
            let lines = wrap_text(&input.text, font, size, (rect.width() - 2.0 * pad).max(size));
            b.op(&format!(
                "{} {} {} {} re W n",
                num(rect.x0),
                num(rect.y0),
                num(rect.width()),
                num(rect.height())
            ));
            b.fill_color(input.text_color);
            let mut y = rect.y1 - pad - size * 0.85;
            for line in &lines {
                if y < rect.y0 - size {
                    break;
                }
                b.show_text(font, size, rect.x0 + pad, y, line);
                y -= size * 1.2;
            }
        }
        AnnotKind::Stamp => {
            let font = StdFont::HelveticaBold;
            res.fonts.push(font);
            let label = win_ansi(&input.text.to_uppercase());
            let inner = rect.inflate(-1.5);
            let radius = (inner.height() * 0.18).min(inner.width() / 4.0);
            b.stroke_color(input.color);
            b.op("2.5 w");
            rounded_rect(&mut b, &inner, radius);
            b.op("S");
            let size = (inner.height() * 0.55).max(4.0);
            let fitted = {
                let tw = font.text_width(&label, size);
                if tw > inner.width() - 8.0 { size * (inner.width() - 8.0) / tw } else { size }
            };
            let tw = font.text_width(&label, fitted);
            b.fill_color(input.color);
            b.show_text(font, fitted, inner.center().x - tw / 2.0, inner.center().y - fitted * 0.35, &label);
        }
        AnnotKind::Caret => {
            let r = rect.inflate(-0.5);
            b.fill_color(input.color);
            b.move_to(PointF::new(r.x0, r.y0));
            b.curve_to(
                PointF::new(r.x0 + r.width() * 0.35, r.y0 + r.height() * 0.2),
                PointF::new(r.center().x, r.y1 - r.height() * 0.3),
                PointF::new(r.center().x, r.y1),
            );
            b.curve_to(
                PointF::new(r.center().x, r.y1 - r.height() * 0.3),
                PointF::new(r.x1 - r.width() * 0.35, r.y0 + r.height() * 0.2),
                PointF::new(r.x1, r.y0),
            );
            b.op("h f");
        }
        AnnotKind::FileAttachment => {
            // A paper clip filling the rectangle.
            let r = rect.inflate(-1.0);
            let (x0, x1, xm) = (r.x0 + r.width() * 0.3, r.x1 - r.width() * 0.3, r.center().x);
            b.stroke_color(input.color);
            b.op("1.6 w 1 J 1 j");
            b.move_to(PointF::new(x1, r.y1 - r.height() * 0.25));
            b.line_to(PointF::new(x1, r.y0 + r.height() * 0.2));
            b.curve_to(
                PointF::new(x1, r.y0),
                PointF::new(x0, r.y0),
                PointF::new(x0, r.y0 + r.height() * 0.2),
            );
            b.line_to(PointF::new(x0, r.y1 - r.height() * 0.1));
            b.curve_to(
                PointF::new(x0, r.y1 + 1.0),
                PointF::new(xm, r.y1 + 1.0),
                PointF::new(xm, r.y1 - r.height() * 0.1),
            );
            b.line_to(PointF::new(xm, r.y0 + r.height() * 0.3));
            b.op("S");
        }
        AnnotKind::Redact => {
            b.stroke_color(input.color);
            b.fill_color(input.color);
            b.op(&format!("{} w", num(w.max(0.5))));
            for q in if input.quads.is_empty() { vec![rect] } else { input.quads.clone() } {
                b.rect(&q.inflate(-w / 2.0));
            }
            b.op("S");
        }
        AnnotKind::Note => {
            // A speech bubble icon filling the rectangle.
            let r = rect.inflate(-0.5);
            let h = r.height();
            let body = RectF::new(r.x0, r.y0 + h * 0.25, r.x1, r.y1);
            b.fill_color(input.color);
            b.stroke_color(Rgba::rgb(60, 60, 60));
            b.op("0.8 w 1 j");
            b.move_to(PointF::new(body.x0, body.y1));
            b.line_to(PointF::new(body.x1, body.y1));
            b.line_to(PointF::new(body.x1, body.y0));
            b.line_to(PointF::new(body.x0 + body.width() * 0.45, body.y0));
            b.line_to(PointF::new(body.x0 + body.width() * 0.2, r.y0));
            b.line_to(PointF::new(body.x0 + body.width() * 0.25, body.y0));
            b.line_to(PointF::new(body.x0, body.y0));
            b.op("h B");
            b.op("0.6 w");
            for i in 1..=3 {
                let y = body.y0 + body.height() * (i as f32) / 4.0;
                b.move_to(PointF::new(body.x0 + body.width() * 0.18, y));
                b.line_to(PointF::new(body.x1 - body.width() * 0.18, y));
            }
            b.op("S");
        }
        _ => {}
    }

    if let Some((x, y, bg, text)) = label_layout(input) {
        let font = StdFont::Helvetica;
        if !res.fonts.contains(&font) {
            res.fonts.push(font);
        }
        b.fill_color(Rgba::WHITE);
        b.rect(&bg);
        b.op("f");
        b.fill_color(input.color);
        b.show_text(font, input.font_size.max(4.0), x, y, &text);
    }

    Appearance { bbox: rect, content: b.finish(), resources: res }
}

/// Radius of the scallops of a cloudy border.
pub fn cloud_radius(width: f32) -> f32 {
    4.0 + width * 1.5
}

/// Appends a closed path with outward bulging arcs along points.
fn cloud_path(b: &mut Builder, points: &[PointF], radius: f32) {
    const K: f32 = 0.552_284_8;
    if points.len() < 2 {
        return;
    }
    // Signed area tells the winding direction; arcs must bulge outwards.
    let area: f32 =
        points.iter().zip(points.iter().cycle().skip(1)).map(|(p, q)| p.x * q.y - q.x * p.y).sum();
    let ccw = area > 0.0;
    b.move_to(points[0]);
    for (i, p) in points.iter().enumerate() {
        let q = points[(i + 1) % points.len()];
        let (dx, dy) = (q.x - p.x, q.y - p.y);
        let len = (dx * dx + dy * dy).sqrt();
        if len < 0.01 {
            continue;
        }
        let n = (len / (2.0 * radius)).round().max(1.0) as usize;
        let (ux, uy) = (dx / len, dy / len);
        let (nx, ny) = if ccw { (uy, -ux) } else { (-uy, ux) };
        for k in 0..n {
            let a = PointF::new(p.x + dx * k as f32 / n as f32, p.y + dy * k as f32 / n as f32);
            let c = PointF::new(p.x + dx * (k + 1) as f32 / n as f32, p.y + dy * (k + 1) as f32 / n as f32);
            let r = len / n as f32 / 2.0;
            let m = PointF::new((a.x + c.x) / 2.0, (a.y + c.y) / 2.0);
            let top = PointF::new(m.x + nx * r, m.y + ny * r);
            b.curve_to(
                PointF::new(a.x + nx * K * r, a.y + ny * K * r),
                PointF::new(top.x - ux * K * r, top.y - uy * K * r),
                top,
            );
            b.curve_to(
                PointF::new(top.x + ux * K * r, top.y + uy * K * r),
                PointF::new(c.x + nx * K * r, c.y + ny * K * r),
                c,
            );
        }
    }
    b.op("h");
}

/// Position (baseline), background box and encoded text of a measurement label.
fn label_layout(input: &ApInput) -> Option<(f32, f32, RectF, Vec<u8>)> {
    if input.label.is_empty()
        || !matches!(input.kind, AnnotKind::Line | AnnotKind::Polyline | AnnotKind::Polygon)
    {
        return None;
    }
    let points = input.paths.first().filter(|p| !p.is_empty())?;
    let anchor = match input.kind {
        AnnotKind::Polygon => {
            let n = points.len() as f32;
            PointF::new(
                points.iter().map(|p| p.x).sum::<f32>() / n,
                points.iter().map(|p| p.y).sum::<f32>() / n,
            )
        }
        _ => {
            let (a, c) = if points.len() >= 2 {
                (points[points.len() - 2], points[points.len() - 1])
            } else {
                (points[0], points[0])
            };
            PointF::new((a.x + c.x) / 2.0, (a.y + c.y) / 2.0)
        }
    };
    let size = input.font_size.max(4.0);
    let text = win_ansi(&input.label);
    let tw = StdFont::Helvetica.text_width(&text, size);
    let x = anchor.x - tw / 2.0;
    let y = anchor.y + 2.0;
    let bg = RectF::new(x - 2.0, y - size * 0.25, x + tw + 2.0, y + size * 0.85);
    Some((x, y, bg, text))
}

fn rounded_rect(b: &mut Builder, r: &RectF, radius: f32) {
    const K: f32 = 0.552_284_8;
    let k = radius * K;
    b.move_to(PointF::new(r.x0 + radius, r.y0));
    b.line_to(PointF::new(r.x1 - radius, r.y0));
    b.curve_to(
        PointF::new(r.x1 - radius + k, r.y0),
        PointF::new(r.x1, r.y0 + radius - k),
        PointF::new(r.x1, r.y0 + radius),
    );
    b.line_to(PointF::new(r.x1, r.y1 - radius));
    b.curve_to(
        PointF::new(r.x1, r.y1 - radius + k),
        PointF::new(r.x1 - radius + k, r.y1),
        PointF::new(r.x1 - radius, r.y1),
    );
    b.line_to(PointF::new(r.x0 + radius, r.y1));
    b.curve_to(
        PointF::new(r.x0 + radius - k, r.y1),
        PointF::new(r.x0, r.y1 - radius + k),
        PointF::new(r.x0, r.y1 - radius),
    );
    b.line_to(PointF::new(r.x0, r.y0 + radius));
    b.curve_to(
        PointF::new(r.x0, r.y0 + radius - k),
        PointF::new(r.x0 + radius - k, r.y0),
        PointF::new(r.x0 + radius, r.y0),
    );
    b.op("h");
}

/// The two outer points of an open arrow head at `to`.
pub fn arrow_head(from: PointF, to: PointF, width: f32) -> (PointF, PointF) {
    let len = (width * 4.0).max(8.0);
    let angle = (to.y - from.y).atan2(to.x - from.x);
    let spread = std::f32::consts::PI / 7.0;
    let a = PointF::new(to.x - len * (angle - spread).cos(), to.y - len * (angle - spread).sin());
    let c = PointF::new(to.x - len * (angle + spread).cos(), to.y - len * (angle + spread).sin());
    (a, c)
}

/// The rectangle an annotation needs so that its whole appearance is visible.
pub fn required_rect(input: &ApInput) -> RectF {
    let w = input.width.max(0.0);
    let base = match input.kind {
        AnnotKind::Line | AnnotKind::Ink | AnnotKind::Polyline | AnnotKind::Polygon => {
            let mut points: Vec<PointF> = input.paths.iter().flatten().copied().collect();
            if input.kind == AnnotKind::Line
                && input.arrow_end
                && let Some(path) = input.paths.first()
                && path.len() >= 2
            {
                let (a, c) = arrow_head(path[0], path[path.len() - 1], w);
                points.push(a);
                points.push(c);
            }
            let grow = if input.cloudy { cloud_radius(w) + w } else { w / 2.0 + 1.0 };
            RectF::bounding(points).unwrap_or(input.rect).inflate(grow)
        }
        AnnotKind::Highlight | AnnotKind::Underline | AnnotKind::StrikeOut | AnnotKind::Squiggly => {
            input.quads.iter().copied().reduce(|a, b| a.union(&b)).unwrap_or(input.rect)
        }
        AnnotKind::FreeText if input.leader.len() >= 2 => {
            let boxed = input.inner.unwrap_or(input.rect);
            let tip = input.leader[0];
            let (a, c) = arrow_head(input.leader[1], tip, w.max(0.5));
            input
                .leader
                .iter()
                .chain([a, c].iter())
                .fold(boxed.normalized(), |r, p| r.union(&RectF::new(p.x, p.y, p.x, p.y)))
                .inflate(w + 1.0)
        }
        _ => input.rect.normalized(),
    };
    match label_layout(input) {
        Some((_, _, bg, _)) => base.union(&bg),
        None => base,
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_compact() {
        assert_eq!(num(1.0), "1");
        assert_eq!(num(0.5), "0.5");
        assert_eq!(num(-0.0001), "0");
        assert_eq!(num(12.3456), "12.346");
        assert_eq!(num(f32::NAN), "0");
    }

    #[test]
    fn win_ansi_maps_german_text() {
        assert_eq!(win_ansi("Grüße €"), vec![b'G', b'r', 0xFC, 0xDF, b'e', b' ', 0x80]);
        assert_eq!(win_ansi("日本"), b"??".to_vec());
    }

    #[test]
    fn text_is_escaped() {
        let mut input = ApInput::new(AnnotKind::FreeText, RectF::new(0.0, 0.0, 200.0, 50.0), Rgba::BLACK);
        input.text = "a(b)c\\".into();
        let ap = build(&input);
        let s = String::from_utf8_lossy(&ap.content);
        assert!(s.contains(r"(a\(b\)c\\) Tj"), "{s}");
        assert_eq!(ap.resources.fonts, vec![StdFont::Helvetica]);
    }

    #[test]
    fn wrapping_respects_width() {
        let lines = wrap_text("one two three four five six", StdFont::Helvetica, 10.0, 60.0);
        assert!(lines.len() > 1);
        for l in &lines {
            assert!(StdFont::Helvetica.text_width(l, 10.0) <= 60.0 || !l.contains(&b' '));
        }
        let long = wrap_text("Donaudampfschifffahrtsgesellschaft", StdFont::Helvetica, 10.0, 40.0);
        assert!(long.len() > 2);
    }

    #[test]
    fn highlight_uses_multiply_blend() {
        let mut input = ApInput::new(AnnotKind::Highlight, RectF::new(0.0, 0.0, 10.0, 10.0), Rgba::YELLOW);
        input.quads = vec![RectF::new(0.0, 0.0, 10.0, 10.0)];
        let ap = build(&input);
        assert!(ap.resources.multiply);
        assert!(String::from_utf8_lossy(&ap.content).contains("re\nf"));
    }

    #[test]
    fn line_rect_contains_arrow() {
        let mut input = ApInput::new(AnnotKind::Line, RectF::default(), Rgba::RED);
        input.paths = vec![vec![PointF::new(0.0, 0.0), PointF::new(100.0, 0.0)]];
        input.arrow_end = true;
        input.width = 2.0;
        let r = required_rect(&input);
        assert!(r.y0 < -3.0 && r.y1 > 3.0 && r.x1 >= 101.0);
    }
}
