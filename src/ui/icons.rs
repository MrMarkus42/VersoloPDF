//! Vector icons drawn with the egui painter.
//!
//! All icons are original drawings made of simple shapes, so the application
//! does not depend on third-party icon sets or their licences.
//!
//! Design rules: one stroke width, slightly rounded corners, the outline in
//! the theme's icon colour ("ink") and at most one topic colour per icon:
//! blue for pages and the view, green for adding, red for deleting, amber and
//! orange for comments, purple for forms and signatures, teal for converting
//! and measuring. A small round badge in the lower right corner (+, ×, arrow)
//! tells related page icons apart. Paper is white in light mode and dark grey
//! in dark mode, so icons never glare.

use egui::epaint::{CubicBezierShape, PathShape, PathStroke};
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, Vec2, pos2, vec2};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Icon {
    New,
    Open,
    Save,
    SaveAs,
    Print,
    Undo,
    Redo,
    Hand,
    SelectText,
    ZoomIn,
    ZoomOut,
    FitWidth,
    FitPage,
    ActualSize,
    Highlight,
    Underline,
    StrikeOut,
    Squiggly,
    Note,
    TextBox,
    Rectangle,
    Ellipse,
    Line,
    Arrow,
    Pencil,
    Polygon,
    Stamp,
    RotateLeft,
    RotateRight,
    DeletePage,
    InsertPage,
    InsertFile,
    ExtractPages,
    MoveUp,
    MoveDown,
    /// "Menüband reduzieren" (chevron up).
    CollapseUp,
    /// "Menüband anheften".
    Pin,
    Search,
    First,
    Prev,
    Next,
    Last,
    Thumbnails,
    Bookmarks,
    Comments,
    Properties,
    Info,
    Close,
    Delete,
    SinglePage,
    Continuous,
    TwoPages,
    Flatten,
    Image,
    Copy,
    Sidebar,
    Language,
    Theme,
    Form,
    Lock,
    Unlock,
    Attachment,
    Fullscreen,
    Gear,
    Mail,
    Back,
    Forward,
    Link,
    LinkOff,
    CoverPages,
    Eye,
    Summary,
    Export,
    Import,
    Redact,
    Merge,
    Split,
    Swap,
    Resize,
    Crop,
    Margins,
    Watermark,
    Background,
    HeaderFooter,
    Numbering,
    TextFile,
    WordFile,
    Count,
    Compare,
    Accessibility,
    Sort,
    Help,
    Bug,
    Console,
    Web,
    Pointer,
    Camera,
    AreaHighlight,
    Replace,
    Caret,
    Typewriter,
    Callout,
    Polyline,
    Cloud,
    Eraser,
    MeasureDistance,
    MeasurePerimeter,
    MeasureArea,
    Signature,
    FieldText,
    FieldButton,
    CheckBox,
    Radio,
    ListBox,
    ComboBox,
    Logo,
    AlignLeft,
    AlignCenterH,
    AlignRight,
    AlignTop,
    AlignCenterV,
    AlignBottom,
    SpaceH,
    SpaceV,
    DistLeft,
    DistCenterH,
    DistRight,
    DistTop,
    DistCenterV,
    DistBottom,
    SameWidth,
    SameHeight,
    SameSize,
    FlipH,
    FlipV,
    BringForward,
    BringToFront,
    SendBackward,
    SendToBack,
    Cut,
    Paste,
    Duplicate,
    EditText,
    EditObjects,
    SpellCheck,
    Calculator,
    Barcode,
    Rotate180,
}

impl Icon {
    /// Every icon (for the icon gallery test).
    pub const ALL: &'static [Icon] = &[
        Icon::New,
        Icon::Open,
        Icon::Save,
        Icon::SaveAs,
        Icon::Print,
        Icon::Undo,
        Icon::Redo,
        Icon::Hand,
        Icon::SelectText,
        Icon::ZoomIn,
        Icon::ZoomOut,
        Icon::FitWidth,
        Icon::FitPage,
        Icon::ActualSize,
        Icon::Highlight,
        Icon::Underline,
        Icon::StrikeOut,
        Icon::Squiggly,
        Icon::Note,
        Icon::TextBox,
        Icon::Rectangle,
        Icon::Ellipse,
        Icon::Line,
        Icon::Arrow,
        Icon::Pencil,
        Icon::Polygon,
        Icon::Stamp,
        Icon::RotateLeft,
        Icon::RotateRight,
        Icon::DeletePage,
        Icon::InsertPage,
        Icon::InsertFile,
        Icon::ExtractPages,
        Icon::MoveUp,
        Icon::MoveDown,
        Icon::CollapseUp,
        Icon::Pin,
        Icon::Search,
        Icon::First,
        Icon::Prev,
        Icon::Next,
        Icon::Last,
        Icon::Thumbnails,
        Icon::Bookmarks,
        Icon::Comments,
        Icon::Properties,
        Icon::Info,
        Icon::Close,
        Icon::Delete,
        Icon::SinglePage,
        Icon::Continuous,
        Icon::TwoPages,
        Icon::Flatten,
        Icon::Image,
        Icon::Copy,
        Icon::Sidebar,
        Icon::Language,
        Icon::Theme,
        Icon::Form,
        Icon::Lock,
        Icon::Unlock,
        Icon::Attachment,
        Icon::Fullscreen,
        Icon::Gear,
        Icon::Mail,
        Icon::Back,
        Icon::Forward,
        Icon::Link,
        Icon::LinkOff,
        Icon::CoverPages,
        Icon::Eye,
        Icon::Summary,
        Icon::Export,
        Icon::Import,
        Icon::Redact,
        Icon::Merge,
        Icon::Split,
        Icon::Swap,
        Icon::Resize,
        Icon::Crop,
        Icon::Margins,
        Icon::Watermark,
        Icon::Background,
        Icon::HeaderFooter,
        Icon::Numbering,
        Icon::TextFile,
        Icon::WordFile,
        Icon::Count,
        Icon::Compare,
        Icon::Accessibility,
        Icon::Sort,
        Icon::Help,
        Icon::Bug,
        Icon::Console,
        Icon::Web,
        Icon::Pointer,
        Icon::Camera,
        Icon::AreaHighlight,
        Icon::Replace,
        Icon::Caret,
        Icon::Typewriter,
        Icon::Callout,
        Icon::Polyline,
        Icon::Cloud,
        Icon::Eraser,
        Icon::MeasureDistance,
        Icon::MeasurePerimeter,
        Icon::MeasureArea,
        Icon::Signature,
        Icon::FieldText,
        Icon::FieldButton,
        Icon::CheckBox,
        Icon::Radio,
        Icon::ListBox,
        Icon::ComboBox,
        Icon::Logo,
        Icon::AlignLeft,
        Icon::AlignCenterH,
        Icon::AlignRight,
        Icon::AlignTop,
        Icon::AlignCenterV,
        Icon::AlignBottom,
        Icon::SpaceH,
        Icon::SpaceV,
        Icon::DistLeft,
        Icon::DistCenterH,
        Icon::DistRight,
        Icon::DistTop,
        Icon::DistCenterV,
        Icon::DistBottom,
        Icon::SameWidth,
        Icon::SameHeight,
        Icon::SameSize,
        Icon::FlipH,
        Icon::FlipV,
        Icon::BringForward,
        Icon::BringToFront,
        Icon::SendBackward,
        Icon::SendToBack,
        Icon::Cut,
        Icon::Paste,
        Icon::Duplicate,
        Icon::EditText,
        Icon::EditObjects,
        Icon::SpellCheck,
        Icon::Calculator,
        Icon::Barcode,
        Icon::Rotate180,
    ];
}

/// Colours of one icon, derived from the ink colour (light or dark theme,
/// enabled or faded).
struct Palette {
    ink: Color32,
    soft: Color32,
    paper: Color32,
    grey: Color32,
    /// Outline around badges, matches the ribbon background.
    ring: Color32,
    /// Glyphs on coloured badges and circles.
    on: Color32,
    blue: Color32,
    green: Color32,
    red: Color32,
    amber: Color32,
    yellow: Color32,
    orange: Color32,
    purple: Color32,
    teal: Color32,
}

impl Palette {
    fn new(ink: Color32) -> Self {
        let alpha = ink.a();
        let fade = alpha as f32 / 255.0;
        // Colours are premultiplied: undo that to see the real brightness.
        let dark = alpha > 0 && u32::from(ink.r()) * 255 / u32::from(alpha) > 128;
        // A faded ink means "disabled": topic colours turn mostly grey too.
        let muted = fade < 0.99;
        let c = |light: [u8; 3], dk: [u8; 3]| {
            let [r, g, b] = if dark { dk } else { light };
            let mix = |v: u8| {
                if !muted {
                    return v;
                }
                let grey = 0.3 * f32::from(r) + 0.59 * f32::from(g) + 0.11 * f32::from(b);
                (f32::from(v) * 0.3 + grey * 0.7) as u8
            };
            Color32::from_rgb(mix(r), mix(g), mix(b)).gamma_multiply(fade)
        };
        Palette {
            ink,
            soft: ink.gamma_multiply(0.55),
            paper: c([255, 255, 255], [64, 68, 76]),
            grey: c([214, 218, 224], [98, 104, 114]),
            ring: c([250, 250, 252], [40, 42, 46]),
            on: c([255, 255, 255], [24, 26, 30]),
            blue: c([37, 99, 235], [96, 165, 250]),
            green: c([22, 163, 74], [74, 222, 128]),
            red: c([220, 38, 38], [248, 113, 113]),
            amber: c([217, 119, 6], [251, 191, 36]),
            yellow: c([250, 204, 21], [234, 179, 8]),
            orange: c([234, 88, 12], [251, 146, 60]),
            purple: c([124, 58, 237], [167, 139, 250]),
            teal: c([13, 148, 136], [45, 212, 191]),
        }
    }
}

/// Small sign in the lower right corner of an icon.
#[derive(Clone, Copy)]
enum Badge {
    Plus,
    Cross,
    Pen,
}

/// Orders polygon points clockwise (on screen), as egui's fill expects.
fn clockwise(mut pts: Vec<Pos2>) -> Vec<Pos2> {
    let mut area = 0.0;
    for i in 0..pts.len() {
        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
        area += a.x * b.y - b.x * a.y;
    }
    if area < 0.0 {
        pts.reverse();
    }
    pts
}

/// Paints `icon` into `rect` using `color` as the ink colour.
pub fn paint(p: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let s = rect.width().min(rect.height());
    let o = rect.center() - vec2(s, s) / 2.0;
    // Maps unit coordinates (0..1) into the icon square.
    let u = |x: f32, y: f32| pos2(o.x + x * s, o.y + y * s);
    let k = Palette::new(color);
    // One fine stroke for everything (modern outline style).
    let w = (s / 15.0).max(1.1);
    // Areas in a topic colour: a light tint with an outline in the colour.
    let tint = |c: Color32| c.gamma_multiply(0.16);
    let edge = |c: Color32| Stroke::new(w, c);
    // Small icons (16 px) leave out fine detail.
    let small = s < 20.0;
    let st = Stroke::new(w, k.ink);
    let thin = Stroke::new((w * 0.75).max(1.0), k.ink);
    let bold = |c: Color32| Stroke::new(w * 1.5, c);

    let line = |a: (f32, f32), b: (f32, f32), stroke: Stroke| {
        p.line_segment([u(a.0, a.1), u(b.0, b.1)], stroke);
    };
    let poly = |pts: &[(f32, f32)], fill: Color32, stroke: Stroke| {
        let v = pts.iter().map(|&(x, y)| u(x, y)).collect();
        p.add(PathShape::convex_polygon(clockwise(v), fill, stroke));
    };
    let path = |pts: &[(f32, f32)], stroke: Stroke| {
        p.add(PathShape::line(pts.iter().map(|&(x, y)| u(x, y)).collect(), stroke));
    };
    let closed = |pts: &[(f32, f32)], stroke: Stroke| {
        p.add(PathShape::closed_line(pts.iter().map(|&(x, y)| u(x, y)).collect(), stroke));
    };
    let rr = |x0: f32, y0: f32, x1: f32, y1: f32, fill: Color32, stroke: Stroke| {
        p.rect(Rect::from_min_max(u(x0, y0), u(x1, y1)), s * 0.1, fill, stroke, StrokeKind::Inside);
    };
    let dashed = |x0: f32, y0: f32, x1: f32, y1: f32, stroke: Stroke| {
        let pts = [u(x0, y0), u(x1, y0), u(x1, y1), u(x0, y1), u(x0, y0)];
        p.extend(Shape::dashed_line(&pts, stroke, s * 0.09, s * 0.06));
    };
    let text = |x: f32, y: f32, t: &str, size: f32, c: Color32| {
        p.text(u(x, y), Align2::CENTER_CENTER, t, FontId::proportional(s * size), c);
    };
    let page = |x0: f32, y0: f32, x1: f32, y1: f32| {
        let f = ((x1 - x0) * 0.32).min(0.2);
        poly(&[(x0, y0), (x1 - f, y0), (x1, y0 + f), (x1, y1), (x0, y1)], k.paper, st);
        path(&[(x1 - f, y0), (x1 - f, y0 + f), (x1, y0 + f)], thin);
    };
    let sheet = |x0: f32, y0: f32, x1: f32, y1: f32| rr(x0, y0, x1, y1, k.paper, st);
    let lines = |x0: f32, x1: f32, ys: &[f32]| {
        if small {
            return;
        }
        for &y in ys {
            line((x0, y), (x1, y), Stroke::new(thin.width, k.soft));
        }
    };
    let arrow = |a: (f32, f32), b: (f32, f32), c: Color32, width: f32| {
        let (from, to) = (u(a.0, a.1), u(b.0, b.1));
        let dir = (to - from).normalized();
        let n = vec2(-dir.y, dir.x);
        let head = (s * 0.2).max(width * 2.6);
        let base = to - dir * head * 0.9;
        p.line_segment([from, base + dir * 0.5], Stroke::new(width, c));
        let tri = vec![to, base + n * head * 0.55, base - n * head * 0.55];
        p.add(PathShape::convex_polygon(clockwise(tri), c, Stroke::NONE));
    };
    let both = |a: (f32, f32), b: (f32, f32), c: Color32| {
        let m = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
        arrow(m, a, c, w);
        arrow(m, b, c, w);
    };
    let curve = |left: bool, c: Color32| {
        let m = |x: f32| if left { x } else { 1.0 - x };
        let pts = [u(m(0.84), 0.8), u(m(0.88), 0.3), u(m(0.5), 0.12), u(m(0.26), 0.4)];
        p.add(CubicBezierShape::from_points_stroke(
            pts,
            false,
            Color32::TRANSPARENT,
            PathStroke::new(w * 1.35, c),
        ));
        let dir = (pts[3] - pts[2]).normalized();
        let n = vec2(-dir.y, dir.x);
        let head = s * 0.2;
        let tip = pts[3] + dir * head * 0.55;
        let base = pts[3] - dir * head * 0.35;
        let tri = vec![tip, base + n * head * 0.6, base - n * head * 0.6];
        p.add(PathShape::convex_polygon(clockwise(tri), c, Stroke::NONE));
    };
    let badge = |b: Badge, c: Color32| {
        let center = u(0.75, 0.75);
        let r = s * 0.235;
        p.circle(center, r, c, Stroke::new(w * 0.9, k.ring));
        let g = Stroke::new((w * 1.05).max(1.0), k.on);
        let d = r * 0.5;
        match b {
            Badge::Plus => {
                p.line_segment([center - vec2(d, 0.0), center + vec2(d, 0.0)], g);
                p.line_segment([center - vec2(0.0, d), center + vec2(0.0, d)], g);
            }
            Badge::Cross => {
                let e = d * 0.8;
                p.line_segment([center - vec2(e, e), center + vec2(e, e)], g);
                p.line_segment([center - vec2(e, -e), center + vec2(e, -e)], g);
            }
            Badge::Pen => {
                p.line_segment(
                    [center + vec2(-d, d), center + vec2(d, -d)],
                    Stroke::new(g.width * 1.3, k.on),
                );
            }
        }
    };
    let magnifier = || {
        p.circle(u(0.42, 0.42), s * 0.28, k.blue.gamma_multiply(0.14), st);
        line((0.63, 0.63), (0.88, 0.88), Stroke::new(w * 2.2, k.ink));
    };
    // Bars for the align and distribute icons; `tp` swaps the axes.
    let bar = |vertical: bool, x0: f32, y0: f32, x1: f32, y1: f32| {
        if vertical {
            rr(y0, x0, y1, x1, k.soft, Stroke::NONE)
        } else {
            rr(x0, y0, x1, y1, k.soft, Stroke::NONE)
        }
    };
    let guide = |vertical: bool, x: f32| {
        if vertical {
            line((0.06, x), (0.94, x), Stroke::new(w, k.blue));
        } else {
            line((x, 0.06), (x, 0.94), Stroke::new(w, k.blue));
        }
    };
    let align = |vertical: bool, anchor: f32| {
        for (y0, y1, len) in [(0.18, 0.4, 0.62), (0.58, 0.8, 0.38)] {
            let x0 = 0.14 + anchor * (0.72 - len);
            bar(vertical, x0, y0, x0 + len, y1);
        }
        guide(vertical, 0.14 + anchor * 0.72);
    };
    let distribute = |vertical: bool, anchor: Option<f32>| {
        let boxes = [(0.06, 0.26, 0.3, 0.7), (0.38, 0.6, 0.18, 0.82), (0.7, 0.94, 0.36, 0.64)];
        if anchor.is_none() {
            for (a, b) in [(0.26, 0.38), (0.6, 0.7)] {
                if vertical {
                    rr(0.12, a, 0.88, b, k.blue.gamma_multiply(0.35), Stroke::NONE);
                } else {
                    rr(a, 0.12, b, 0.88, k.blue.gamma_multiply(0.35), Stroke::NONE);
                }
            }
        }
        for (x0, x1, y0, y1) in boxes {
            bar(vertical, x0, y0, x1, y1);
            if let Some(a) = anchor {
                guide(vertical, x0 + a * (x1 - x0));
            }
        }
    };
    let pointer = |x: f32, y: f32, sc: f32| {
        let q = |a: f32, b: f32| (x + a * sc, y + b * sc);
        poly(&[q(0.0, 0.0), q(0.0, 0.68), q(0.48, 0.5)], k.paper, Stroke::NONE);
        poly(&[q(0.16, 0.55), q(0.28, 0.84), q(0.4, 0.78), q(0.28, 0.5)], k.paper, Stroke::NONE);
        closed(
            &[
                q(0.0, 0.0),
                q(0.0, 0.68),
                q(0.16, 0.55),
                q(0.28, 0.84),
                q(0.4, 0.78),
                q(0.28, 0.5),
                q(0.48, 0.5),
            ],
            st,
        );
    };

    match icon {
        Icon::New => {
            page(0.16, 0.06, 0.7, 0.9);
            lines(0.26, 0.56, &[0.4, 0.52]);
            badge(Badge::Plus, k.green);
        }
        Icon::Open => {
            rr(0.06, 0.14, 0.42, 0.34, tint(k.amber), edge(k.amber));
            rr(0.06, 0.22, 0.86, 0.84, tint(k.amber), edge(k.amber));
            poly(
                &[(0.2, 0.42), (0.98, 0.42), (0.86, 0.86), (0.06, 0.86)],
                k.yellow.gamma_multiply(0.55),
                edge(k.amber),
            );
        }
        Icon::Save | Icon::SaveAs => {
            rr(0.1, 0.1, 0.88, 0.9, tint(k.blue), edge(k.blue));
            rr(0.28, 0.1, 0.68, 0.36, k.paper, edge(k.blue));
            line((0.56, 0.16), (0.56, 0.3), edge(k.blue));
            rr(0.24, 0.54, 0.74, 0.9, k.paper, edge(k.blue));
            lines(0.32, 0.66, &[0.66, 0.78]);
            if icon == Icon::SaveAs {
                badge(Badge::Pen, k.amber);
            }
        }
        Icon::Print => {
            sheet(0.26, 0.08, 0.74, 0.4);
            rr(0.06, 0.34, 0.94, 0.72, k.grey, st);
            sheet(0.24, 0.58, 0.76, 0.94);
            lines(0.34, 0.66, &[0.72, 0.83]);
            p.circle_filled(u(0.8, 0.46), s * 0.04, k.green);
        }
        Icon::Undo => curve(true, k.blue),
        Icon::Redo => curve(false, k.blue),
        Icon::Hand => {
            for (x, top) in [(0.3f32, 0.24f32), (0.43, 0.1), (0.56, 0.13), (0.69, 0.24)] {
                rr(x, top, x + 0.13, 0.62, k.paper, thin);
            }
            poly(
                &[(0.3, 0.5), (0.82, 0.5), (0.78, 0.82), (0.62, 0.93), (0.4, 0.93), (0.28, 0.76)],
                k.paper,
                thin,
            );
            poly(&[(0.3, 0.6), (0.14, 0.46), (0.08, 0.53), (0.28, 0.8)], k.paper, thin);
            // Hide the inner finger ends under the palm.
            poly(&[(0.31, 0.52), (0.81, 0.52), (0.78, 0.8), (0.32, 0.8)], k.paper, Stroke::NONE);
        }
        Icon::SelectText => {
            rr(0.08, 0.38, 0.4, 0.62, k.blue.gamma_multiply(0.4), Stroke::NONE);
            rr(0.6, 0.38, 0.92, 0.62, k.blue.gamma_multiply(0.4), Stroke::NONE);
            line((0.5, 0.16), (0.5, 0.84), st);
            path(&[(0.36, 0.1), (0.5, 0.17), (0.64, 0.1)], st);
            path(&[(0.36, 0.9), (0.5, 0.83), (0.64, 0.9)], st);
        }
        Icon::ZoomIn | Icon::ZoomOut => {
            magnifier();
            line((0.3, 0.42), (0.54, 0.42), st);
            if icon == Icon::ZoomIn {
                line((0.42, 0.3), (0.42, 0.54), st);
            }
        }
        Icon::Search => magnifier(),
        Icon::FitWidth => {
            page(0.26, 0.08, 0.74, 0.92);
            both((0.02, 0.56), (0.98, 0.56), k.blue);
        }
        Icon::FitPage => {
            page(0.22, 0.08, 0.78, 0.92);
            both((0.5, 0.2), (0.5, 0.86), k.blue);
        }
        Icon::ActualSize => {
            page(0.16, 0.06, 0.84, 0.94);
            text(0.5, 0.6, "1:1", 0.32, k.blue);
        }
        Icon::Highlight => {
            rr(0.06, 0.76, 0.94, 0.94, k.yellow, Stroke::NONE);
            poly(&[(0.32, 0.6), (0.7, 0.1), (0.88, 0.24), (0.5, 0.74)], k.paper, st);
            poly(&[(0.32, 0.6), (0.5, 0.74), (0.3, 0.8)], k.yellow, thin);
        }
        Icon::Underline => {
            text(0.5, 0.42, "U", 0.62, k.ink);
            line((0.2, 0.86), (0.8, 0.86), bold(k.amber));
        }
        Icon::StrikeOut => {
            text(0.5, 0.48, "S", 0.66, k.ink);
            line((0.14, 0.52), (0.86, 0.52), bold(k.red));
        }
        Icon::Squiggly => {
            text(0.5, 0.4, "W", 0.56, k.ink);
            let pts: Vec<(f32, f32)> =
                (0..=8).map(|i| (0.12 + i as f32 * 0.095, if i % 2 == 0 { 0.8 } else { 0.92 })).collect();
            path(&pts, Stroke::new(w * 1.2, k.amber));
        }
        Icon::Note | Icon::Comments => {
            let fill = if icon == Icon::Note { k.yellow } else { k.paper };
            poly(&[(0.08, 0.12), (0.92, 0.12), (0.92, 0.68), (0.08, 0.68)], fill, Stroke::NONE);
            poly(&[(0.3, 0.66), (0.52, 0.66), (0.26, 0.9)], fill, Stroke::NONE);
            closed(
                &[
                    (0.08, 0.12),
                    (0.92, 0.12),
                    (0.92, 0.68),
                    (0.5, 0.68),
                    (0.26, 0.9),
                    (0.3, 0.68),
                    (0.08, 0.68),
                ],
                st,
            );
            let c = if icon == Icon::Note { k.ink } else { k.amber };
            for y in [0.3, 0.42, 0.54] {
                line((0.22, y), (0.78, y), Stroke::new(thin.width, c));
            }
        }
        Icon::TextBox => {
            rr(0.06, 0.14, 0.94, 0.86, k.paper, Stroke::new(w, k.amber));
            text(0.5, 0.5, "T", 0.55, k.ink);
        }
        Icon::Rectangle => rr(0.1, 0.2, 0.9, 0.8, k.orange.gamma_multiply(0.14), bold(k.orange)),
        Icon::Ellipse => {
            p.add(Shape::ellipse_filled(u(0.5, 0.5), vec2(s * 0.4, s * 0.29), k.orange.gamma_multiply(0.14)));
            p.add(Shape::ellipse_stroke(u(0.5, 0.5), vec2(s * 0.4, s * 0.29), bold(k.orange)));
        }
        Icon::Line => line((0.12, 0.86), (0.88, 0.14), bold(k.orange)),
        Icon::Arrow => arrow((0.12, 0.88), (0.9, 0.1), k.orange, w * 1.5),
        Icon::Pencil => {
            poly(&[(0.2, 0.68), (0.66, 0.22), (0.8, 0.36), (0.34, 0.82)], k.yellow, st);
            poly(&[(0.2, 0.68), (0.34, 0.82), (0.1, 0.92)], k.paper, thin);
            poly(&[(0.66, 0.22), (0.74, 0.14), (0.88, 0.28), (0.8, 0.36)], k.orange, thin);
        }
        Icon::Polygon => {
            let pts = [(0.5, 0.1), (0.9, 0.4), (0.75, 0.88), (0.25, 0.88), (0.1, 0.4)];
            poly(&pts, k.orange.gamma_multiply(0.14), Stroke::NONE);
            closed(&pts, bold(k.orange));
        }
        Icon::Stamp => {
            rr(0.38, 0.08, 0.62, 0.46, k.paper, st);
            rr(0.16, 0.44, 0.84, 0.64, k.grey, st);
            rr(0.08, 0.74, 0.92, 0.9, tint(k.orange), edge(k.orange));
        }
        Icon::RotateLeft | Icon::RotateRight => {
            if icon == Icon::RotateLeft {
                page(0.34, 0.36, 0.82, 0.94);
            } else {
                page(0.18, 0.36, 0.66, 0.94);
            }
            curve(icon == Icon::RotateLeft, k.blue);
        }
        Icon::Rotate180 => {
            page(0.28, 0.36, 0.72, 0.94);
            text(0.5, 0.68, "180", 0.2, k.blue);
            curve(false, k.blue);
        }
        Icon::DeletePage => {
            page(0.14, 0.06, 0.68, 0.9);
            lines(0.24, 0.54, &[0.36, 0.48]);
            badge(Badge::Cross, k.red);
        }
        Icon::InsertPage => {
            page(0.14, 0.06, 0.68, 0.9);
            lines(0.24, 0.54, &[0.36, 0.48]);
            badge(Badge::Plus, k.green);
        }
        // A file going into the document: arrow from the left into a page.
        Icon::InsertFile => {
            page(0.34, 0.06, 0.9, 0.94);
            lines(0.5, 0.78, &[0.38, 0.72]);
            arrow((0.02, 0.56), (0.6, 0.56), k.green, w * 1.6);
        }
        Icon::Duplicate => {
            sheet(0.06, 0.04, 0.5, 0.62);
            sheet(0.22, 0.18, 0.66, 0.84);
            badge(Badge::Plus, k.green);
        }
        // Pages taken out: arrow from inside the page to the right.
        Icon::ExtractPages => {
            page(0.08, 0.06, 0.62, 0.94);
            lines(0.18, 0.5, &[0.36, 0.76]);
            arrow((0.36, 0.56), (0.98, 0.56), k.blue, w * 1.6);
        }
        Icon::MoveUp => arrow((0.5, 0.9), (0.5, 0.08), k.blue, w * 1.7),
        Icon::MoveDown => arrow((0.5, 0.1), (0.5, 0.92), k.blue, w * 1.7),
        Icon::First | Icon::Prev | Icon::Next | Icon::Last => {
            let round = Stroke::new(w * 0.8, k.ink);
            if matches!(icon, Icon::First | Icon::Prev) {
                poly(&[(0.7, 0.22), (0.7, 0.78), (0.32, 0.5)], k.ink, round);
            } else {
                poly(&[(0.3, 0.22), (0.3, 0.78), (0.68, 0.5)], k.ink, round);
            }
            if icon == Icon::First {
                line((0.22, 0.22), (0.22, 0.78), bold(k.ink));
            }
            if icon == Icon::Last {
                line((0.78, 0.22), (0.78, 0.78), bold(k.ink));
            }
        }
        Icon::Thumbnails => {
            for (i, (x, y)) in [(0.1, 0.08), (0.54, 0.08), (0.1, 0.52), (0.54, 0.52)].into_iter().enumerate()
            {
                let fill = if i == 0 { k.blue.gamma_multiply(0.35) } else { k.paper };
                rr(x, y, x + 0.36, y + 0.4, fill, st);
            }
        }
        Icon::Bookmarks => {
            poly(&[(0.26, 0.06), (0.74, 0.06), (0.74, 0.92), (0.5, 0.72)], tint(k.blue), Stroke::NONE);
            poly(&[(0.26, 0.06), (0.5, 0.72), (0.26, 0.92)], tint(k.blue), Stroke::NONE);
            closed(&[(0.26, 0.06), (0.74, 0.06), (0.74, 0.92), (0.5, 0.72), (0.26, 0.92)], edge(k.blue));
        }
        Icon::Properties => {
            for (y, x) in [(0.24f32, 0.32f32), (0.5, 0.66), (0.76, 0.44)] {
                line((0.08, y), (0.92, y), st);
                p.circle(u(x, y), s * 0.09, k.paper, Stroke::new(w, k.blue));
            }
        }
        Icon::Info => {
            p.circle(u(0.5, 0.5), s * 0.42, k.blue.gamma_multiply(0.12), Stroke::new(w, k.blue));
            p.circle_filled(u(0.5, 0.3), s * 0.055, k.blue);
            line((0.5, 0.44), (0.5, 0.74), Stroke::new(w * 1.6, k.blue));
        }
        Icon::Help => {
            p.circle(u(0.5, 0.5), s * 0.42, tint(k.blue), edge(k.blue));
            text(0.5, 0.53, "?", 0.56, k.blue);
        }
        Icon::Close => {
            line((0.24, 0.24), (0.76, 0.76), bold(k.ink));
            line((0.76, 0.24), (0.24, 0.76), bold(k.ink));
        }
        Icon::CollapseUp => {
            path(&[(0.22, 0.64), (0.5, 0.36), (0.78, 0.64)], bold(k.ink));
        }
        Icon::Pin => {
            // A push pin, slightly tilted like in office programs.
            line((0.2, 0.84), (0.42, 0.6), bold(k.ink));
            poly(&[(0.36, 0.48), (0.54, 0.3), (0.72, 0.48), (0.52, 0.68)], tint(k.blue), edge(k.blue));
            line((0.3, 0.46), (0.56, 0.72), bold(k.blue));
            line((0.56, 0.22), (0.8, 0.46), bold(k.blue));
        }
        Icon::Delete => {
            line((0.2, 0.2), (0.8, 0.8), Stroke::new(w * 1.9, k.red));
            line((0.8, 0.2), (0.2, 0.8), Stroke::new(w * 1.9, k.red));
        }
        Icon::SinglePage => {
            page(0.22, 0.08, 0.78, 0.92);
            lines(0.32, 0.62, &[0.42, 0.54, 0.66]);
        }
        Icon::Continuous => {
            sheet(0.24, -0.04, 0.76, 0.42);
            sheet(0.24, 0.54, 0.76, 1.04);
            both((0.9, 0.12), (0.9, 0.88), k.blue);
        }
        Icon::TwoPages => {
            sheet(0.04, 0.14, 0.48, 0.86);
            sheet(0.52, 0.14, 0.96, 0.86);
            lines(0.12, 0.4, &[0.34, 0.46, 0.58]);
            lines(0.6, 0.88, &[0.34, 0.46, 0.58]);
        }
        Icon::CoverPages => {
            sheet(0.3, 0.02, 0.7, 0.42);
            sheet(0.06, 0.54, 0.48, 0.98);
            sheet(0.52, 0.54, 0.94, 0.98);
            rr(0.3, 0.02, 0.7, 0.42, k.blue.gamma_multiply(0.3), Stroke::NONE);
        }
        Icon::Flatten => {
            let fills = [k.paper, k.blue.gamma_multiply(0.4), k.yellow];
            for (i, y) in [0.26f32, 0.46, 0.66].into_iter().enumerate() {
                poly(&[(0.5, y - 0.16), (0.94, y), (0.5, y + 0.16), (0.06, y)], fills[i], thin);
            }
        }
        Icon::Image => {
            rr(0.06, 0.14, 0.94, 0.86, k.blue.gamma_multiply(0.14), st);
            poly(&[(0.14, 0.78), (0.4, 0.42), (0.62, 0.78)], k.green, Stroke::NONE);
            poly(&[(0.46, 0.78), (0.66, 0.52), (0.86, 0.78)], k.green.gamma_multiply(0.7), Stroke::NONE);
            p.circle_filled(u(0.72, 0.32), s * 0.075, k.yellow);
        }
        Icon::Copy => {
            sheet(0.1, 0.06, 0.6, 0.66);
            sheet(0.36, 0.3, 0.88, 0.94);
            lines(0.46, 0.78, &[0.5, 0.62, 0.74]);
        }
        Icon::Paste => {
            rr(0.1, 0.14, 0.7, 0.92, tint(k.amber), edge(k.amber));
            rr(0.26, 0.06, 0.54, 0.22, k.grey, st);
            sheet(0.4, 0.36, 0.92, 0.96);
            lines(0.5, 0.82, &[0.56, 0.68, 0.8]);
        }
        Icon::Cut => {
            line((0.38, 0.62), (0.68, 0.08), bold(k.ink));
            line((0.62, 0.62), (0.32, 0.08), bold(k.ink));
            p.circle(u(0.3, 0.76), s * 0.14, k.paper, Stroke::new(w * 1.2, k.blue));
            p.circle(u(0.7, 0.76), s * 0.14, k.paper, Stroke::new(w * 1.2, k.blue));
        }
        Icon::Sidebar => {
            rr(0.06, 0.14, 0.94, 0.86, k.paper, st);
            rr(0.06, 0.14, 0.36, 0.86, k.blue.gamma_multiply(0.45), st);
        }
        Icon::Language => {
            p.circle_stroke(u(0.5, 0.5), s * 0.4, st);
            p.add(Shape::ellipse_stroke(u(0.5, 0.5), vec2(s * 0.17, s * 0.4), thin));
            line((0.1, 0.5), (0.9, 0.5), thin);
            line((0.16, 0.3), (0.84, 0.3), thin);
            line((0.16, 0.7), (0.84, 0.7), thin);
        }
        Icon::Web => {
            let b = Stroke::new(w, k.blue);
            p.circle(u(0.5, 0.5), s * 0.4, k.blue.gamma_multiply(0.12), b);
            p.add(Shape::ellipse_stroke(u(0.5, 0.5), vec2(s * 0.17, s * 0.4), b));
            line((0.1, 0.5), (0.9, 0.5), b);
        }
        Icon::Theme => {
            p.circle(u(0.5, 0.5), s * 0.38, k.paper, st);
            let half = Rect::from_min_max(u(0.5, 0.1), u(0.9, 0.9));
            p.with_clip_rect(half).circle_filled(u(0.5, 0.5), s * 0.38, k.ink);
        }
        Icon::Form => {
            rr(0.06, 0.16, 0.94, 0.42, k.paper, Stroke::new(w, k.purple));
            line((0.14, 0.29), (0.5, 0.29), Stroke::new(thin.width, k.soft));
            rr(0.06, 0.56, 0.32, 0.82, k.paper, st);
            path(&[(0.11, 0.68), (0.18, 0.76), (0.28, 0.58)], Stroke::new(w * 1.3, k.purple));
            line((0.42, 0.69), (0.92, 0.69), thin);
        }
        Icon::Lock | Icon::Unlock => {
            let right = if icon == Icon::Lock { 0.68 } else { 0.9 };
            let end = if icon == Icon::Lock { 0.46 } else { 0.32 };
            path(
                &[
                    (0.32, 0.46),
                    (0.32, 0.28),
                    (0.4, 0.14),
                    (0.5, 0.1),
                    (right - 0.08, 0.14),
                    (right, 0.28),
                    (right, end),
                ],
                bold(k.ink),
            );
            rr(0.18, 0.44, 0.82, 0.92, tint(k.amber), edge(k.amber));
            p.circle_filled(u(0.5, 0.64), s * 0.06, k.amber);
            line((0.5, 0.66), (0.5, 0.78), Stroke::new(w * 1.2, k.amber));
        }
        Icon::Attachment => {
            path(
                &[
                    (0.64, 0.24),
                    (0.64, 0.72),
                    (0.58, 0.84),
                    (0.5, 0.88),
                    (0.42, 0.84),
                    (0.36, 0.72),
                    (0.36, 0.2),
                    (0.42, 0.1),
                    (0.5, 0.07),
                    (0.58, 0.1),
                    (0.76, 0.2),
                    (0.76, 0.7),
                ],
                bold(k.ink),
            );
        }
        Icon::Fullscreen => {
            for pts in [
                [(0.1, 0.36), (0.1, 0.1), (0.36, 0.1)],
                [(0.64, 0.1), (0.9, 0.1), (0.9, 0.36)],
                [(0.9, 0.64), (0.9, 0.9), (0.64, 0.9)],
                [(0.36, 0.9), (0.1, 0.9), (0.1, 0.64)],
            ] {
                path(&pts, bold(k.blue));
            }
        }
        Icon::Gear => {
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::FRAC_PI_4;
                let (s1, c1) = a.sin_cos();
                line(
                    (0.5 + 0.24 * c1, 0.5 + 0.24 * s1),
                    (0.5 + 0.42 * c1, 0.5 + 0.42 * s1),
                    Stroke::new(w * 2.0, k.ink),
                );
            }
            p.circle(u(0.5, 0.5), s * 0.28, k.paper, st);
            p.circle(u(0.5, 0.5), s * 0.11, tint(k.blue), edge(k.blue));
        }
        Icon::Mail => {
            rr(0.06, 0.2, 0.94, 0.8, k.paper, st);
            path(&[(0.08, 0.24), (0.5, 0.56), (0.92, 0.24)], Stroke::new(w, k.blue));
        }
        Icon::Back | Icon::Forward => {
            p.circle(u(0.5, 0.5), s * 0.42, tint(k.blue), edge(k.blue));
            let (a, b) =
                if icon == Icon::Back { ((0.74, 0.5), (0.24, 0.5)) } else { ((0.26, 0.5), (0.76, 0.5)) };
            arrow(a, b, k.blue, w * 1.4);
        }
        Icon::Link | Icon::LinkOff => {
            let b = Stroke::new(w * 1.5, k.blue);
            p.add(Shape::ellipse_stroke(u(0.35, 0.6), vec2(s * 0.22, s * 0.13), b));
            p.add(Shape::ellipse_stroke(u(0.65, 0.4), vec2(s * 0.22, s * 0.13), b));
            if icon == Icon::LinkOff {
                line((0.14, 0.14), (0.86, 0.86), Stroke::new(w * 1.8, k.ring));
                line((0.14, 0.14), (0.86, 0.86), bold(k.red));
            }
        }
        Icon::Eye => {
            let top = [u(0.06, 0.5), u(0.3, 0.18), u(0.7, 0.18), u(0.94, 0.5)];
            let bottom = [u(0.06, 0.5), u(0.3, 0.82), u(0.7, 0.82), u(0.94, 0.5)];
            for pts in [top, bottom] {
                p.add(CubicBezierShape::from_points_stroke(
                    pts,
                    false,
                    Color32::TRANSPARENT,
                    PathStroke::new(w, k.ink),
                ));
            }
            p.circle_filled(u(0.5, 0.5), s * 0.16, k.blue);
            p.circle_filled(u(0.5, 0.5), s * 0.06, k.ink);
        }
        Icon::Summary => {
            page(0.16, 0.06, 0.84, 0.94);
            for y in [0.38, 0.54, 0.7] {
                p.circle_filled(u(0.3, y), s * 0.04, k.blue);
                line((0.38, y), (0.72, y), Stroke::new(thin.width, k.soft));
            }
        }
        Icon::TextFile => {
            page(0.16, 0.06, 0.84, 0.94);
            lines(0.28, 0.72, &[0.4, 0.52, 0.64, 0.76]);
            line((0.28, 0.26), (0.5, 0.26), Stroke::new(w * 1.4, k.teal));
        }
        Icon::WordFile => {
            page(0.24, 0.06, 0.88, 0.94);
            lines(0.56, 0.78, &[0.4, 0.52, 0.64]);
            rr(0.06, 0.32, 0.52, 0.76, k.blue, Stroke::NONE);
            text(0.29, 0.55, "W", 0.32, k.on);
        }
        // Out of a tray (export) or into it (import).
        Icon::Export | Icon::Import => {
            path(&[(0.1, 0.5), (0.1, 0.9), (0.9, 0.9), (0.9, 0.5)], st);
            if icon == Icon::Export {
                arrow((0.5, 0.74), (0.5, 0.06), k.teal, w * 1.6);
            } else {
                arrow((0.5, 0.06), (0.5, 0.76), k.teal, w * 1.6);
            }
        }
        Icon::Redact => {
            page(0.14, 0.06, 0.86, 0.94);
            rr(0.24, 0.38, 0.76, 0.52, Color32::BLACK.gamma_multiply(color.a() as f32 / 255.0), Stroke::NONE);
            lines(0.24, 0.76, &[0.66]);
            lines(0.24, 0.6, &[0.78]);
        }
        Icon::Merge => {
            sheet(0.04, 0.04, 0.4, 0.46);
            sheet(0.6, 0.04, 0.96, 0.46);
            arrow((0.22, 0.52), (0.46, 0.92), k.blue, w * 1.2);
            arrow((0.78, 0.52), (0.54, 0.92), k.blue, w * 1.2);
        }
        Icon::Split => {
            sheet(0.32, 0.04, 0.68, 0.44);
            arrow((0.46, 0.5), (0.16, 0.92), k.blue, w * 1.2);
            arrow((0.54, 0.5), (0.84, 0.92), k.blue, w * 1.2);
        }
        Icon::Swap => {
            arrow((0.12, 0.32), (0.88, 0.32), k.blue, w * 1.4);
            arrow((0.88, 0.68), (0.12, 0.68), k.blue, w * 1.4);
        }
        Icon::Resize => {
            dashed(0.08, 0.08, 0.92, 0.92, thin);
            sheet(0.08, 0.46, 0.54, 0.92);
            arrow((0.44, 0.56), (0.86, 0.14), k.blue, w * 1.3);
        }
        Icon::Crop => {
            path(&[(0.26, 0.04), (0.26, 0.74), (0.96, 0.74)], bold(k.ink));
            path(&[(0.04, 0.26), (0.74, 0.26), (0.74, 0.96)], bold(k.blue));
        }
        Icon::Margins => {
            page(0.14, 0.04, 0.86, 0.96);
            dashed(0.26, 0.2, 0.74, 0.82, Stroke::new(w * 0.8, k.blue));
        }
        Icon::Watermark => {
            page(0.14, 0.04, 0.86, 0.96);
            line((0.26, 0.8), (0.74, 0.32), Stroke::new(w * 2.6, k.blue.gamma_multiply(0.35)));
        }
        Icon::Background => {
            page(0.14, 0.04, 0.86, 0.96);
            rr(0.2, 0.22, 0.8, 0.9, k.blue.gamma_multiply(0.35), Stroke::NONE);
        }
        Icon::HeaderFooter => {
            page(0.14, 0.04, 0.86, 0.96);
            rr(0.24, 0.14, 0.6, 0.24, k.blue, Stroke::NONE);
            lines(0.24, 0.76, &[0.42, 0.54, 0.66]);
            rr(0.24, 0.8, 0.76, 0.88, k.blue, Stroke::NONE);
        }
        Icon::Numbering => {
            page(0.14, 0.04, 0.86, 0.96);
            lines(0.24, 0.76, &[0.3, 0.42, 0.54]);
            text(0.62, 0.76, "1", 0.3, k.blue);
        }
        Icon::Count => {
            text(0.5, 0.3, "abc", 0.32, k.blue);
            text(0.5, 0.7, "123", 0.36, k.ink);
        }
        Icon::Calculator => {
            rr(0.16, 0.06, 0.84, 0.94, k.paper, st);
            rr(0.26, 0.16, 0.74, 0.36, k.teal.gamma_multiply(0.45), Stroke::NONE);
            for y in [0.5f32, 0.64, 0.78] {
                for x in [0.32f32, 0.5, 0.68] {
                    p.circle_filled(u(x, y), s * 0.045, k.ink);
                }
            }
        }
        Icon::Barcode => {
            let mut x = 0.1;
            for (i, wd) in
                [0.06f32, 0.03, 0.08, 0.03, 0.05, 0.03, 0.08, 0.04, 0.03, 0.06].into_iter().enumerate()
            {
                if i % 2 == 0 {
                    rr(x, 0.16, x + wd, 0.84, k.ink, Stroke::NONE);
                }
                x += wd + 0.03;
            }
        }
        Icon::Compare => {
            page(0.04, 0.1, 0.48, 0.9);
            page(0.52, 0.1, 0.96, 0.9);
            line((0.12, 0.5), (0.38, 0.5), bold(k.red));
            line((0.6, 0.5), (0.86, 0.5), bold(k.green));
        }
        Icon::Accessibility => {
            p.circle(u(0.5, 0.5), s * 0.42, tint(k.blue), edge(k.blue));
            p.circle_filled(u(0.5, 0.25), s * 0.07, k.blue);
            let b = Stroke::new(w * 1.3, k.blue);
            line((0.26, 0.4), (0.74, 0.4), b);
            line((0.5, 0.4), (0.5, 0.58), b);
            path(&[(0.36, 0.8), (0.5, 0.56), (0.64, 0.8)], b);
        }
        Icon::Sort => {
            text(0.3, 0.28, "A", 0.32, k.ink);
            text(0.3, 0.72, "Z", 0.32, k.ink);
            arrow((0.72, 0.12), (0.72, 0.9), k.blue, w * 1.3);
        }
        Icon::Bug => {
            for y in [0.46f32, 0.6, 0.74] {
                line((0.14, y), (0.86, y), thin);
            }
            p.add(Shape::ellipse_filled(u(0.5, 0.6), vec2(s * 0.22, s * 0.3), k.paper));
            p.add(Shape::ellipse_filled(u(0.5, 0.6), vec2(s * 0.22, s * 0.3), tint(k.green)));
            p.add(Shape::ellipse_stroke(u(0.5, 0.6), vec2(s * 0.22, s * 0.3), edge(k.green)));
            p.circle(u(0.5, 0.24), s * 0.11, tint(k.green), edge(k.green));
            line((0.5, 0.36), (0.5, 0.88), Stroke::new(thin.width, k.green));
        }
        Icon::Console => {
            rr(0.06, 0.14, 0.94, 0.86, Color32::from_gray(36).gamma_multiply(color.a() as f32 / 255.0), st);
            path(&[(0.2, 0.36), (0.36, 0.5), (0.2, 0.64)], Stroke::new(w * 1.3, k.green));
            line((0.42, 0.66), (0.64, 0.66), Stroke::new(w * 1.3, Color32::from_gray(230)));
        }
        Icon::Pointer => pointer(0.26, 0.06, 1.0),
        Icon::Camera => {
            rr(0.34, 0.16, 0.62, 0.32, k.paper, st);
            rr(0.06, 0.28, 0.94, 0.84, k.paper, st);
            p.circle(u(0.5, 0.56), s * 0.18, tint(k.blue), edge(k.blue));
            p.circle_filled(u(0.5, 0.56), s * 0.06, k.blue);
        }
        Icon::AreaHighlight => {
            rr(0.08, 0.24, 0.92, 0.76, k.yellow, Stroke::NONE);
            dashed(0.08, 0.24, 0.92, 0.76, thin);
        }
        Icon::Replace => {
            text(0.5, 0.38, "T", 0.55, k.ink);
            line((0.2, 0.4), (0.8, 0.4), bold(k.red));
            path(&[(0.58, 0.94), (0.72, 0.72), (0.86, 0.94)], bold(k.amber));
        }
        Icon::Caret => {
            line((0.08, 0.38), (0.92, 0.38), thin);
            path(&[(0.28, 0.9), (0.5, 0.5), (0.72, 0.9)], Stroke::new(w * 1.8, k.amber));
        }
        Icon::Typewriter => {
            text(0.44, 0.48, "T", 0.66, k.ink);
            line((0.8, 0.2), (0.8, 0.8), Stroke::new(w * 1.2, k.amber));
        }
        Icon::Callout => {
            rr(0.34, 0.08, 0.96, 0.56, k.paper, Stroke::new(w, k.amber));
            text(0.65, 0.32, "T", 0.36, k.ink);
            path(&[(0.34, 0.44), (0.2, 0.62), (0.06, 0.92)], bold(k.amber));
        }
        Icon::Polyline => path(&[(0.08, 0.86), (0.3, 0.26), (0.6, 0.7), (0.92, 0.14)], bold(k.orange)),
        Icon::Cloud => {
            for (x, y, r) in [(0.3, 0.56, 0.18), (0.5, 0.42, 0.22), (0.7, 0.56, 0.18), (0.5, 0.66, 0.18)] {
                p.circle_filled(u(x, y), s * r, k.orange.gamma_multiply(0.14));
                p.circle_stroke(u(x, y), s * r, Stroke::new(w * 1.2, k.orange));
            }
        }
        Icon::Eraser => {
            poly(&[(0.14, 0.62), (0.54, 0.22), (0.88, 0.56), (0.48, 0.96)], k.paper, st);
            poly(&[(0.34, 0.42), (0.54, 0.22), (0.88, 0.56), (0.68, 0.76)], k.orange, st);
        }
        Icon::MeasureDistance | Icon::MeasurePerimeter | Icon::MeasureArea => {
            let pts = [(0.14, 0.24), (0.86, 0.18), (0.76, 0.78), (0.24, 0.84)];
            match icon {
                Icon::MeasureDistance => both((0.06, 0.48), (0.94, 0.48), k.teal),
                Icon::MeasurePerimeter => closed(&pts, bold(k.teal)),
                _ => {
                    poly(&pts, k.teal.gamma_multiply(0.35), Stroke::NONE);
                    closed(&pts, bold(k.teal));
                }
            }
            for x in [0.2f32, 0.4, 0.6, 0.8] {
                line((x, 0.92), (x, 0.99), thin);
            }
        }
        Icon::Signature => {
            let pts: Vec<(f32, f32)> = (0..=20)
                .map(|i| {
                    let t = i as f32 / 20.0;
                    (0.08 + t * 0.84, 0.52 - 0.26 * (t * 9.0).sin() * (1.0 - t))
                })
                .collect();
            path(&pts, Stroke::new(w * 1.3, k.purple));
            line((0.08, 0.86), (0.92, 0.86), thin);
        }
        Icon::FieldText => {
            rr(0.04, 0.28, 0.96, 0.72, k.paper, st);
            text(0.28, 0.5, "T", 0.36, k.purple);
            line((0.5, 0.38), (0.5, 0.62), Stroke::new(w, k.purple));
        }
        Icon::FieldButton => {
            p.rect(
                Rect::from_min_max(u(0.06, 0.26), u(0.94, 0.74)),
                s * 0.2,
                tint(k.purple),
                edge(k.purple),
                StrokeKind::Inside,
            );
            text(0.5, 0.5, "OK", 0.3, k.purple);
        }
        Icon::CheckBox => {
            rr(0.14, 0.14, 0.86, 0.86, k.paper, st);
            path(&[(0.28, 0.5), (0.45, 0.68), (0.74, 0.3)], Stroke::new(w * 1.8, k.purple));
        }
        Icon::Radio => {
            p.circle(u(0.5, 0.5), s * 0.37, k.paper, st);
            p.circle_filled(u(0.5, 0.5), s * 0.18, k.purple);
        }
        Icon::ListBox => {
            rr(0.06, 0.1, 0.94, 0.9, k.paper, st);
            rr(0.1, 0.39, 0.9, 0.6, k.purple.gamma_multiply(0.35), Stroke::NONE);
            lines(0.18, 0.72, &[0.27, 0.49, 0.72]);
        }
        Icon::ComboBox => {
            rr(0.04, 0.28, 0.96, 0.72, k.paper, st);
            line((0.14, 0.5), (0.5, 0.5), Stroke::new(thin.width, k.soft));
            poly(&[(0.64, 0.42), (0.86, 0.42), (0.75, 0.58)], k.purple, Stroke::NONE);
        }
        Icon::Logo => {
            // The application icon itself (a picture, not a drawing).
            let tint = Color32::WHITE.gamma_multiply(color.a() as f32 / 255.0);
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            p.image(logo_texture(p.ctx()).id(), Rect::from_min_size(o, vec2(s, s)), uv, tint);
        }
        Icon::AlignLeft => align(false, 0.0),
        Icon::AlignCenterH => align(false, 0.5),
        Icon::AlignRight => align(false, 1.0),
        Icon::AlignTop => align(true, 0.0),
        Icon::AlignCenterV => align(true, 0.5),
        Icon::AlignBottom => align(true, 1.0),
        Icon::SpaceH => distribute(false, None),
        Icon::SpaceV => distribute(true, None),
        Icon::DistLeft => distribute(false, Some(0.0)),
        Icon::DistCenterH => distribute(false, Some(0.5)),
        Icon::DistRight => distribute(false, Some(1.0)),
        Icon::DistTop => distribute(true, Some(0.0)),
        Icon::DistCenterV => distribute(true, Some(0.5)),
        Icon::DistBottom => distribute(true, Some(1.0)),
        Icon::SameWidth => {
            rr(0.18, 0.1, 0.82, 0.34, k.soft, Stroke::NONE);
            rr(0.18, 0.66, 0.82, 0.9, k.soft, Stroke::NONE);
            both((0.12, 0.5), (0.88, 0.5), k.blue);
        }
        Icon::SameHeight => {
            rr(0.1, 0.18, 0.34, 0.82, k.soft, Stroke::NONE);
            rr(0.66, 0.18, 0.9, 0.82, k.soft, Stroke::NONE);
            both((0.5, 0.12), (0.5, 0.88), k.blue);
        }
        Icon::SameSize => {
            rr(0.06, 0.06, 0.44, 0.44, k.soft, Stroke::NONE);
            rr(0.56, 0.56, 0.94, 0.94, k.soft, Stroke::NONE);
            both((0.2, 0.8), (0.8, 0.2), k.blue);
        }
        Icon::FlipH => {
            poly(&[(0.08, 0.82), (0.42, 0.82), (0.42, 0.18)], k.soft, Stroke::NONE);
            closed(&[(0.92, 0.82), (0.58, 0.82), (0.58, 0.18)], st);
            p.extend(Shape::dashed_line(
                &[u(0.5, 0.04), u(0.5, 0.96)],
                Stroke::new(w, k.blue),
                s * 0.1,
                s * 0.06,
            ));
        }
        Icon::FlipV => {
            poly(&[(0.18, 0.08), (0.18, 0.42), (0.82, 0.42)], k.soft, Stroke::NONE);
            closed(&[(0.18, 0.92), (0.18, 0.58), (0.82, 0.58)], st);
            p.extend(Shape::dashed_line(
                &[u(0.04, 0.5), u(0.96, 0.5)],
                Stroke::new(w, k.blue),
                s * 0.1,
                s * 0.06,
            ));
        }
        Icon::BringForward | Icon::BringToFront | Icon::SendBackward | Icon::SendToBack => {
            let forward = matches!(icon, Icon::BringForward | Icon::BringToFront);
            // Squares on the left, arrow on the right.
            let q =
                |x0: f32, y0: f32, x1: f32, y1: f32, fill: Color32| rr(x0 * 0.8, y0, x1 * 0.8, y1, fill, st);
            let blue = k.blue.gamma_multiply(0.85);
            if forward {
                q(0.06, 0.1, 0.62, 0.62, k.paper);
                q(0.3, 0.36, 0.86, 0.88, blue);
            } else {
                q(0.3, 0.36, 0.86, 0.88, blue);
                q(0.06, 0.1, 0.62, 0.62, k.paper);
            }
            let x = 0.86;
            if forward {
                arrow((x, 0.9), (x, 0.14), k.blue, w * 1.3);
            } else {
                arrow((x, 0.1), (x, 0.86), k.blue, w * 1.3);
            }
            match icon {
                Icon::BringToFront => line((0.76, 0.06), (0.96, 0.06), bold(k.blue)),
                Icon::SendToBack => line((0.76, 0.94), (0.96, 0.94), bold(k.blue)),
                _ => {}
            }
        }
        Icon::EditText => {
            dashed(0.04, 0.14, 0.7, 0.86, Stroke::new(thin.width, k.blue));
            text(0.37, 0.5, "T", 0.6, k.ink);
            let b = Stroke::new(w * 1.2, k.blue);
            line((0.84, 0.16), (0.84, 0.84), b);
            line((0.76, 0.16), (0.92, 0.16), b);
            line((0.76, 0.84), (0.92, 0.84), b);
        }
        Icon::EditObjects => {
            dashed(0.08, 0.08, 0.68, 0.68, Stroke::new(thin.width, k.blue));
            for (x, y) in [(0.08, 0.08), (0.68, 0.08), (0.08, 0.68), (0.68, 0.68)] {
                let h = 0.06;
                rr(x - h, y - h, x + h, y + h, k.blue, Stroke::NONE);
            }
            rr(0.2, 0.2, 0.56, 0.56, k.soft, Stroke::NONE);
            pointer(0.5, 0.42, 0.62);
        }
        Icon::SpellCheck => {
            text(0.5, 0.32, "abc", 0.4, k.ink);
            path(&[(0.22, 0.7), (0.42, 0.88), (0.82, 0.5)], Stroke::new(w * 1.7, k.green));
        }
    }
}

/// The application icon (256 px PNG, see `scripts/make_icon.rs`).
pub const LOGO_PNG: &[u8] = include_bytes!("../../assets/versolopdf.png");

/// The application icon as RGBA pixels (width, height, pixels).
pub fn logo_rgba() -> (u32, u32, Vec<u8>) {
    match image::load_from_memory(LOGO_PNG) {
        Ok(img) => {
            let img = img.to_rgba8();
            (img.width(), img.height(), img.into_raw())
        }
        // Cannot happen with the built-in file; a red square keeps it visible.
        Err(_) => (1, 1, vec![200, 35, 45, 255]),
    }
}

/// The application icon as a texture, loaded once per context.
fn logo_texture(ctx: &egui::Context) -> egui::TextureHandle {
    let id = egui::Id::new("versolopdf-logo-texture");
    if let Some(tex) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return tex;
    }
    let (w, h, rgba) = logo_rgba();
    let image = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
    // Mipmaps keep the picture smooth when it is drawn small (20 px).
    let options = egui::TextureOptions::LINEAR.with_mipmap_mode(Some(egui::TextureFilter::Linear));
    let tex = ctx.load_texture("versolopdf-logo", image, options);
    ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
    tex
}

/// Size of the icon square for a given button height.
pub fn icon_size(height: f32) -> Vec2 {
    Vec2::splat(height)
}
