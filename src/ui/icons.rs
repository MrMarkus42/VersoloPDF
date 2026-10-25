//! Vector icons drawn with the egui painter.
//!
//! All icons are original drawings made of simple shapes, so the application
//! does not depend on third-party icon sets or their licences.

use egui::epaint::{CubicBezierShape, PathShape, PathStroke};
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Stroke, Vec2, pos2, vec2};

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
}

/// Paints `icon` into `rect` using `color` as the main colour.
pub fn paint(p: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let s = rect.width().min(rect.height());
    let o = rect.center() - vec2(s, s) / 2.0;
    // Maps unit coordinates (0..1) into the icon square.
    let u = |x: f32, y: f32| pos2(o.x + x * s, o.y + y * s);
    let w = (s / 14.0).max(1.0);
    let st = Stroke::new(w, color);
    let thin = Stroke::new((w * 0.7).max(1.0), color);
    let accent = Color32::from_rgb(30, 100, 220);
    let red = Color32::from_rgb(210, 45, 45);
    let yellow = Color32::from_rgb(250, 210, 40);
    let page = |x0: f32, y0: f32, x1: f32, y1: f32| {
        let fold = (x1 - x0) * 0.3;
        let pts = vec![u(x0, y0), u(x1 - fold, y0), u(x1, y0 + fold * 0.9), u(x1, y1), u(x0, y1)];
        p.add(PathShape::convex_polygon(pts, Color32::WHITE, st));
        p.line_segment([u(x1 - fold, y0), u(x1 - fold, y0 + fold * 0.9)], thin);
        p.line_segment([u(x1 - fold, y0 + fold * 0.9), u(x1, y0 + fold * 0.9)], thin);
    };
    let text = |x: f32, y: f32, t: &str, size: f32, c: Color32| {
        p.text(u(x, y), Align2::CENTER_CENTER, t, FontId::proportional(s * size), c);
    };
    let arrow = |from: Pos2, to: Pos2, stroke: Stroke| {
        p.line_segment([from, to], stroke);
        let dir = (to - from).normalized();
        let n = vec2(-dir.y, dir.x);
        let head = s * 0.16;
        p.line_segment([to, to - dir * head + n * head * 0.7], stroke);
        p.line_segment([to, to - dir * head - n * head * 0.7], stroke);
    };
    let magnifier = |p: &Painter| {
        p.circle_stroke(u(0.42, 0.42), s * 0.27, st);
        p.line_segment([u(0.62, 0.62), u(0.88, 0.88)], Stroke::new(w * 2.0, color));
    };
    let curved = |p: &Painter, left: bool| {
        let (a, b, c, d) = if left {
            (u(0.8, 0.75), u(0.85, 0.3), u(0.45, 0.15), u(0.2, 0.45))
        } else {
            (u(0.2, 0.75), u(0.15, 0.3), u(0.55, 0.15), u(0.8, 0.45))
        };
        p.add(CubicBezierShape::from_points_stroke(
            [a, b, c, d],
            false,
            Color32::TRANSPARENT,
            PathStroke::new(w * 1.4, color),
        ));
        let tip = d;
        let (e1, e2) = if left { (u(0.2, 0.2), u(0.45, 0.47)) } else { (u(0.8, 0.2), u(0.55, 0.47)) };
        p.line_segment([tip, e1], Stroke::new(w * 1.4, color));
        p.line_segment([tip, e2], Stroke::new(w * 1.4, color));
    };

    match icon {
        Icon::New => {
            page(0.2, 0.08, 0.78, 0.92);
            p.line_segment([u(0.49, 0.42), u(0.49, 0.74)], Stroke::new(w * 1.5, accent));
            p.line_segment([u(0.33, 0.58), u(0.65, 0.58)], Stroke::new(w * 1.5, accent));
        }
        Icon::Open => {
            let back =
                vec![u(0.08, 0.22), u(0.38, 0.22), u(0.46, 0.32), u(0.9, 0.32), u(0.9, 0.82), u(0.08, 0.82)];
            p.add(PathShape::convex_polygon(back, Color32::from_rgb(240, 200, 90), st));
            let front = vec![u(0.18, 0.45), u(0.98, 0.45), u(0.88, 0.82), u(0.08, 0.82)];
            p.add(PathShape::convex_polygon(front, Color32::from_rgb(252, 222, 120), st));
        }
        Icon::Save | Icon::SaveAs => {
            p.rect(
                Rect::from_min_max(u(0.12, 0.12), u(0.88, 0.88)),
                2.0,
                accent,
                st,
                egui::StrokeKind::Inside,
            );
            p.rect_filled(Rect::from_min_max(u(0.28, 0.12), u(0.72, 0.38)), 0.0, Color32::WHITE);
            p.rect_filled(Rect::from_min_max(u(0.56, 0.17), u(0.66, 0.33)), 0.0, accent);
            p.rect_filled(Rect::from_min_max(u(0.24, 0.55), u(0.76, 0.84)), 1.0, Color32::WHITE);
            if icon == Icon::SaveAs {
                p.line_segment([u(0.55, 0.95), u(0.98, 0.52)], Stroke::new(w * 2.2, red));
            }
        }
        Icon::Print => {
            p.rect(
                Rect::from_min_max(u(0.28, 0.1), u(0.72, 0.4)),
                0.0,
                Color32::WHITE,
                st,
                egui::StrokeKind::Inside,
            );
            p.rect(
                Rect::from_min_max(u(0.08, 0.36), u(0.92, 0.72)),
                2.0,
                Color32::from_gray(200),
                st,
                egui::StrokeKind::Inside,
            );
            p.rect(
                Rect::from_min_max(u(0.26, 0.6), u(0.74, 0.92)),
                0.0,
                Color32::WHITE,
                st,
                egui::StrokeKind::Inside,
            );
            p.circle_filled(u(0.8, 0.47), s * 0.035, Color32::from_rgb(60, 180, 60));
        }
        Icon::Undo => curved(p, true),
        Icon::Redo => curved(p, false),
        Icon::Hand => {
            let fill = Color32::from_rgb(255, 225, 190);
            for (i, x) in [0.3f32, 0.43, 0.56, 0.69].iter().enumerate() {
                let top = [0.25f32, 0.12, 0.15, 0.25][i];
                p.rect(
                    Rect::from_min_max(u(*x, top), u(x + 0.12, 0.6)),
                    s * 0.06,
                    fill,
                    thin,
                    egui::StrokeKind::Inside,
                );
            }
            let palm =
                vec![u(0.3, 0.5), u(0.81, 0.5), u(0.78, 0.82), u(0.62, 0.92), u(0.4, 0.92), u(0.28, 0.75)];
            p.add(PathShape::convex_polygon(palm, fill, thin));
            let thumb = vec![u(0.3, 0.6), u(0.14, 0.48), u(0.1, 0.55), u(0.28, 0.78)];
            p.add(PathShape::convex_polygon(thumb, fill, thin));
        }
        Icon::SelectText => {
            p.line_segment([u(0.5, 0.15), u(0.5, 0.85)], st);
            p.line_segment([u(0.36, 0.12), u(0.5, 0.18)], st);
            p.line_segment([u(0.64, 0.12), u(0.5, 0.18)], st);
            p.line_segment([u(0.36, 0.88), u(0.5, 0.82)], st);
            p.line_segment([u(0.64, 0.88), u(0.5, 0.82)], st);
            p.rect_filled(Rect::from_min_max(u(0.12, 0.4), u(0.42, 0.6)), 1.0, accent.gamma_multiply(0.5));
            p.rect_filled(Rect::from_min_max(u(0.58, 0.4), u(0.88, 0.6)), 1.0, accent.gamma_multiply(0.5));
        }
        Icon::ZoomIn | Icon::ZoomOut => {
            magnifier(p);
            p.line_segment([u(0.3, 0.42), u(0.54, 0.42)], st);
            if icon == Icon::ZoomIn {
                p.line_segment([u(0.42, 0.3), u(0.42, 0.54)], st);
            }
        }
        Icon::Search => magnifier(p),
        Icon::FitWidth => {
            page(0.25, 0.1, 0.75, 0.9);
            arrow(u(0.5, 0.5), u(0.08, 0.5), Stroke::new(w * 1.2, accent));
            arrow(u(0.5, 0.5), u(0.92, 0.5), Stroke::new(w * 1.2, accent));
        }
        Icon::FitPage => {
            page(0.25, 0.1, 0.75, 0.9);
            arrow(u(0.5, 0.5), u(0.5, 0.06), Stroke::new(w * 1.2, accent));
            arrow(u(0.5, 0.5), u(0.5, 0.94), Stroke::new(w * 1.2, accent));
        }
        Icon::ActualSize => {
            page(0.2, 0.1, 0.8, 0.9);
            text(0.5, 0.58, "1:1", 0.32, accent);
        }
        Icon::Highlight => {
            p.rect_filled(Rect::from_min_max(u(0.08, 0.72), u(0.92, 0.92)), 1.0, yellow);
            let pen = vec![u(0.32, 0.62), u(0.72, 0.12), u(0.88, 0.26), u(0.48, 0.74)];
            p.add(PathShape::convex_polygon(pen, Color32::from_gray(235), st));
            let tip = vec![u(0.32, 0.62), u(0.48, 0.74), u(0.3, 0.8)];
            p.add(PathShape::convex_polygon(tip, yellow, thin));
        }
        Icon::Underline => {
            text(0.5, 0.42, "U", 0.62, color);
            p.line_segment([u(0.2, 0.86), u(0.8, 0.86)], Stroke::new(w * 1.6, accent));
        }
        Icon::StrikeOut => {
            text(0.5, 0.48, "S", 0.66, color);
            p.line_segment([u(0.15, 0.52), u(0.85, 0.52)], Stroke::new(w * 1.6, red));
        }
        Icon::Squiggly => {
            text(0.5, 0.4, "W", 0.56, color);
            let pts: Vec<Pos2> =
                (0..=8).map(|i| u(0.12 + i as f32 * 0.095, if i % 2 == 0 { 0.82 } else { 0.92 })).collect();
            p.add(PathShape::line(pts, Stroke::new(w * 1.2, accent)));
        }
        Icon::Note | Icon::Comments => {
            let fill = if icon == Icon::Note { yellow } else { Color32::WHITE };
            let pts = vec![
                u(0.1, 0.12),
                u(0.9, 0.12),
                u(0.9, 0.68),
                u(0.5, 0.68),
                u(0.28, 0.9),
                u(0.32, 0.68),
                u(0.1, 0.68),
            ];
            p.add(PathShape::closed_line(pts.clone(), st));
            p.add(PathShape::convex_polygon(vec![pts[0], pts[1], pts[2], pts[6]], fill, Stroke::NONE));
            p.add(PathShape::closed_line(pts, st));
            for y in [0.3, 0.42, 0.54] {
                p.line_segment([u(0.24, y), u(0.76, y)], thin);
            }
        }
        Icon::TextBox => {
            p.rect_stroke(
                Rect::from_min_max(u(0.08, 0.15), u(0.92, 0.85)),
                1.0,
                Stroke::new(w, accent),
                egui::StrokeKind::Inside,
            );
            text(0.5, 0.5, "T", 0.55, color);
        }
        Icon::Rectangle => {
            p.rect_stroke(
                Rect::from_min_max(u(0.12, 0.22), u(0.88, 0.78)),
                0.0,
                Stroke::new(w * 1.4, red),
                egui::StrokeKind::Inside,
            );
        }
        Icon::Ellipse => {
            p.add(egui::Shape::ellipse_stroke(
                u(0.5, 0.5),
                vec2(s * 0.4, s * 0.28),
                Stroke::new(w * 1.4, red),
            ));
        }
        Icon::Line => {
            p.line_segment([u(0.12, 0.85), u(0.88, 0.15)], Stroke::new(w * 1.4, red));
        }
        Icon::Arrow => arrow(u(0.12, 0.85), u(0.88, 0.15), Stroke::new(w * 1.4, red)),
        Icon::Pencil => {
            let body = vec![u(0.18, 0.7), u(0.68, 0.2), u(0.82, 0.34), u(0.32, 0.84)];
            p.add(PathShape::convex_polygon(body, Color32::from_rgb(250, 190, 60), st));
            let tip = vec![u(0.18, 0.7), u(0.32, 0.84), u(0.1, 0.92)];
            p.add(PathShape::convex_polygon(tip, Color32::from_rgb(240, 220, 190), thin));
            p.add(PathShape::convex_polygon(
                vec![u(0.68, 0.2), u(0.74, 0.14), u(0.88, 0.28), u(0.82, 0.34)],
                red,
                thin,
            ));
        }
        Icon::Polygon => {
            let pts = vec![u(0.5, 0.1), u(0.9, 0.42), u(0.74, 0.88), u(0.26, 0.88), u(0.1, 0.42)];
            p.add(PathShape::closed_line(pts, Stroke::new(w * 1.3, red)));
        }
        Icon::Stamp => {
            p.rect_filled(Rect::from_min_max(u(0.4, 0.1), u(0.6, 0.45)), 2.0, color);
            p.rect_filled(Rect::from_min_max(u(0.18, 0.45), u(0.82, 0.62)), 2.0, color);
            p.rect_filled(Rect::from_min_max(u(0.1, 0.72), u(0.9, 0.86)), 1.0, red);
        }
        Icon::RotateLeft => {
            page(0.32, 0.3, 0.78, 0.9);
            curved(p, true);
        }
        Icon::RotateRight => {
            page(0.22, 0.3, 0.68, 0.9);
            curved(p, false);
        }
        Icon::DeletePage => {
            page(0.15, 0.08, 0.7, 0.88);
            p.line_segment([u(0.55, 0.6), u(0.92, 0.95)], Stroke::new(w * 1.6, red));
            p.line_segment([u(0.92, 0.6), u(0.55, 0.95)], Stroke::new(w * 1.6, red));
        }
        Icon::InsertPage => {
            page(0.15, 0.08, 0.7, 0.88);
            p.line_segment(
                [u(0.75, 0.55), u(0.75, 0.97)],
                Stroke::new(w * 1.6, Color32::from_rgb(40, 160, 60)),
            );
            p.line_segment(
                [u(0.54, 0.76), u(0.96, 0.76)],
                Stroke::new(w * 1.6, Color32::from_rgb(40, 160, 60)),
            );
        }
        Icon::InsertFile => {
            page(0.1, 0.05, 0.55, 0.7);
            page(0.4, 0.3, 0.9, 0.95);
        }
        Icon::ExtractPages => {
            page(0.1, 0.08, 0.62, 0.88);
            arrow(u(0.45, 0.55), u(0.95, 0.55), Stroke::new(w * 1.6, accent));
        }
        Icon::MoveUp => arrow(u(0.5, 0.88), u(0.5, 0.12), Stroke::new(w * 1.8, accent)),
        Icon::MoveDown => arrow(u(0.5, 0.12), u(0.5, 0.88), Stroke::new(w * 1.8, accent)),
        Icon::First | Icon::Prev | Icon::Next | Icon::Last => {
            let left = matches!(icon, Icon::First | Icon::Prev);
            let tri = if left {
                vec![u(0.72, 0.2), u(0.72, 0.8), u(0.3, 0.5)]
            } else {
                vec![u(0.28, 0.2), u(0.28, 0.8), u(0.7, 0.5)]
            };
            p.add(PathShape::convex_polygon(tri, color, Stroke::NONE));
            if icon == Icon::First {
                p.line_segment([u(0.24, 0.2), u(0.24, 0.8)], Stroke::new(w * 1.6, color));
            }
            if icon == Icon::Last {
                p.line_segment([u(0.76, 0.2), u(0.76, 0.8)], Stroke::new(w * 1.6, color));
            }
        }
        Icon::Thumbnails => {
            for (x, y) in [(0.12, 0.1), (0.55, 0.1), (0.12, 0.55), (0.55, 0.55)] {
                p.rect(
                    Rect::from_min_max(u(x, y), u(x + 0.33, y + 0.35)),
                    1.0,
                    Color32::WHITE,
                    st,
                    egui::StrokeKind::Inside,
                );
            }
        }
        Icon::Bookmarks => {
            let pts = vec![u(0.28, 0.08), u(0.72, 0.08), u(0.72, 0.92), u(0.5, 0.72), u(0.28, 0.92)];
            p.add(PathShape::closed_line(pts.clone(), st));
            p.add(PathShape::convex_polygon(vec![pts[0], pts[1], pts[2], pts[3]], red, Stroke::NONE));
            p.add(PathShape::convex_polygon(vec![pts[0], pts[3], pts[4]], red, Stroke::NONE));
        }
        Icon::Properties => {
            for (i, y) in [0.25f32, 0.5, 0.75].iter().enumerate() {
                p.line_segment([u(0.1, *y), u(0.9, *y)], st);
                let x = [0.3f32, 0.65, 0.45][i];
                p.circle(u(x, *y), s * 0.08, Color32::WHITE, st);
            }
        }
        Icon::Info => {
            p.circle_stroke(u(0.5, 0.5), s * 0.4, st);
            text(0.5, 0.54, "i", 0.55, accent);
        }
        Icon::Close | Icon::Delete => {
            let c = if icon == Icon::Delete { red } else { color };
            p.line_segment([u(0.22, 0.22), u(0.78, 0.78)], Stroke::new(w * 1.5, c));
            p.line_segment([u(0.78, 0.22), u(0.22, 0.78)], Stroke::new(w * 1.5, c));
        }
        Icon::SinglePage => page(0.25, 0.1, 0.75, 0.9),
        Icon::Continuous => {
            p.rect(
                Rect::from_min_max(u(0.28, -0.05), u(0.72, 0.42)),
                1.0,
                Color32::WHITE,
                st,
                egui::StrokeKind::Inside,
            );
            p.rect(
                Rect::from_min_max(u(0.28, 0.52), u(0.72, 1.0)),
                1.0,
                Color32::WHITE,
                st,
                egui::StrokeKind::Inside,
            );
        }
        Icon::TwoPages => {
            p.rect(
                Rect::from_min_max(u(0.06, 0.15), u(0.47, 0.85)),
                1.0,
                Color32::WHITE,
                st,
                egui::StrokeKind::Inside,
            );
            p.rect(
                Rect::from_min_max(u(0.53, 0.15), u(0.94, 0.85)),
                1.0,
                Color32::WHITE,
                st,
                egui::StrokeKind::Inside,
            );
        }
        Icon::Flatten => {
            for (i, y) in [0.25f32, 0.45, 0.65].iter().enumerate() {
                let c = [
                    Color32::from_gray(230),
                    Color32::from_rgb(200, 220, 250),
                    Color32::from_rgb(250, 230, 160),
                ][i];
                let pts = vec![u(0.5, y - 0.15), u(0.92, *y), u(0.5, y + 0.15), u(0.08, *y)];
                p.add(PathShape::convex_polygon(pts, c, thin));
            }
        }
        Icon::Image => {
            p.rect(
                Rect::from_min_max(u(0.08, 0.15), u(0.92, 0.85)),
                1.0,
                Color32::from_rgb(220, 235, 250),
                st,
                egui::StrokeKind::Inside,
            );
            p.add(PathShape::convex_polygon(
                vec![u(0.15, 0.78), u(0.42, 0.42), u(0.62, 0.78)],
                Color32::from_rgb(60, 150, 70),
                Stroke::NONE,
            ));
            p.add(PathShape::convex_polygon(
                vec![u(0.45, 0.78), u(0.65, 0.52), u(0.85, 0.78)],
                Color32::from_rgb(90, 180, 90),
                Stroke::NONE,
            ));
            p.circle_filled(u(0.72, 0.32), s * 0.07, Color32::from_rgb(250, 200, 50));
        }
        Icon::Copy => {
            p.rect(
                Rect::from_min_max(u(0.12, 0.08), u(0.62, 0.68)),
                1.0,
                Color32::WHITE,
                st,
                egui::StrokeKind::Inside,
            );
            p.rect(
                Rect::from_min_max(u(0.38, 0.32), u(0.88, 0.92)),
                1.0,
                Color32::WHITE,
                st,
                egui::StrokeKind::Inside,
            );
        }
        Icon::Sidebar => {
            p.rect_stroke(
                Rect::from_min_max(u(0.08, 0.15), u(0.92, 0.85)),
                1.0,
                st,
                egui::StrokeKind::Inside,
            );
            p.rect_filled(Rect::from_min_max(u(0.08, 0.15), u(0.36, 0.85)), 1.0, color.gamma_multiply(0.5));
        }
        Icon::Language => {
            p.circle_stroke(u(0.5, 0.5), s * 0.4, st);
            p.add(egui::Shape::ellipse_stroke(u(0.5, 0.5), vec2(s * 0.18, s * 0.4), thin));
            p.line_segment([u(0.1, 0.5), u(0.9, 0.5)], thin);
        }
        Icon::Theme => {
            p.circle_stroke(u(0.5, 0.5), s * 0.36, st);
            let half = Rect::from_min_max(u(0.5, 0.14), u(0.86, 0.86));
            p.with_clip_rect(half).circle_filled(u(0.5, 0.5), s * 0.36, color);
        }
        Icon::Form => {
            p.rect_stroke(Rect::from_min_max(u(0.08, 0.2), u(0.92, 0.45)), 1.0, st, egui::StrokeKind::Inside);
            p.rect_stroke(Rect::from_min_max(u(0.08, 0.58), u(0.3, 0.8)), 1.0, st, egui::StrokeKind::Inside);
            p.line_segment([u(0.12, 0.69), u(0.18, 0.76)], Stroke::new(w * 1.3, accent));
            p.line_segment([u(0.18, 0.76), u(0.27, 0.6)], Stroke::new(w * 1.3, accent));
            p.line_segment([u(0.4, 0.69), u(0.9, 0.69)], thin);
        }
        Icon::Lock | Icon::Unlock => {
            let body = Rect::from_min_max(u(0.2, 0.45), u(0.8, 0.9));
            let shackle_right = if icon == Icon::Lock { 0.68 } else { 0.9 };
            let pts = vec![
                u(0.32, 0.45),
                u(0.32, 0.25),
                u(0.5, 0.1),
                u(shackle_right, 0.25),
                u(shackle_right, if icon == Icon::Lock { 0.45 } else { 0.35 }),
            ];
            p.add(PathShape::line(pts, Stroke::new(w * 1.6, color)));
            p.rect(body, 2.0, Color32::from_rgb(250, 200, 60), st, egui::StrokeKind::Inside);
            p.circle_filled(u(0.5, 0.65), s * 0.06, color);
        }
        Icon::Attachment => {
            let pts: Vec<Pos2> = vec![
                u(0.62, 0.22),
                u(0.62, 0.72),
                u(0.5, 0.85),
                u(0.38, 0.72),
                u(0.38, 0.18),
                u(0.5, 0.08),
                u(0.74, 0.18),
                u(0.74, 0.7),
            ];
            p.add(PathShape::line(pts, Stroke::new(w * 1.4, color)));
        }
        Icon::Fullscreen => {
            for (a, b, c) in [
                ((0.1, 0.35), (0.1, 0.1), (0.35, 0.1)),
                ((0.65, 0.1), (0.9, 0.1), (0.9, 0.35)),
                ((0.9, 0.65), (0.9, 0.9), (0.65, 0.9)),
                ((0.35, 0.9), (0.1, 0.9), (0.1, 0.65)),
            ] {
                p.add(PathShape::line(
                    vec![u(a.0, a.1), u(b.0, b.1), u(c.0, c.1)],
                    Stroke::new(w * 1.5, color),
                ));
            }
        }
        Icon::Gear => {
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::FRAC_PI_4;
                let (s1, c1) = a.sin_cos();
                p.line_segment(
                    [u(0.5 + 0.22 * c1, 0.5 + 0.22 * s1), u(0.5 + 0.42 * c1, 0.5 + 0.42 * s1)],
                    Stroke::new(w * 2.2, color),
                );
            }
            p.circle(u(0.5, 0.5), s * 0.26, Color32::from_gray(200), st);
            p.circle_filled(u(0.5, 0.5), s * 0.09, color);
        }
        Icon::Mail => {
            p.rect(
                Rect::from_min_max(u(0.08, 0.22), u(0.92, 0.78)),
                1.0,
                Color32::WHITE,
                st,
                egui::StrokeKind::Inside,
            );
            p.add(PathShape::line(vec![u(0.08, 0.22), u(0.5, 0.55), u(0.92, 0.22)], st));
        }
        Icon::Back | Icon::Forward => {
            p.circle_stroke(u(0.5, 0.5), s * 0.4, Stroke::new(w, Color32::from_rgb(60, 160, 60)));
            let (a, b) =
                if icon == Icon::Back { (u(0.72, 0.5), u(0.28, 0.5)) } else { (u(0.28, 0.5), u(0.72, 0.5)) };
            arrow(a, b, Stroke::new(w * 1.4, Color32::from_rgb(60, 160, 60)));
        }
        Icon::Link | Icon::LinkOff => {
            p.add(egui::Shape::ellipse_stroke(
                u(0.36, 0.6),
                vec2(s * 0.22, s * 0.12),
                Stroke::new(w * 1.5, accent),
            ));
            p.add(egui::Shape::ellipse_stroke(
                u(0.64, 0.4),
                vec2(s * 0.22, s * 0.12),
                Stroke::new(w * 1.5, accent),
            ));
            if icon == Icon::LinkOff {
                p.line_segment([u(0.15, 0.15), u(0.85, 0.85)], Stroke::new(w * 1.6, red));
            }
        }
        Icon::CoverPages => {
            p.rect(
                Rect::from_min_max(u(0.3, 0.02), u(0.7, 0.4)),
                1.0,
                Color32::WHITE,
                st,
                egui::StrokeKind::Inside,
            );
            p.rect(
                Rect::from_min_max(u(0.06, 0.55), u(0.47, 0.98)),
                1.0,
                Color32::WHITE,
                st,
                egui::StrokeKind::Inside,
            );
            p.rect(
                Rect::from_min_max(u(0.53, 0.55), u(0.94, 0.98)),
                1.0,
                Color32::WHITE,
                st,
                egui::StrokeKind::Inside,
            );
        }
        Icon::Eye => {
            p.add(egui::Shape::ellipse_stroke(u(0.5, 0.5), vec2(s * 0.42, s * 0.24), st));
            p.circle_filled(u(0.5, 0.5), s * 0.13, accent);
        }
        Icon::Summary | Icon::TextFile => {
            page(0.18, 0.06, 0.82, 0.94);
            for y in [0.38, 0.52, 0.66, 0.8] {
                p.line_segment([u(0.3, y), u(0.7, y)], thin);
            }
            if icon == Icon::Summary {
                p.circle_filled(u(0.3, 0.38), s * 0.03, accent);
            }
        }
        Icon::WordFile => {
            page(0.22, 0.06, 0.86, 0.94);
            p.rect_filled(
                Rect::from_min_max(u(0.06, 0.35), u(0.5, 0.75)),
                2.0,
                Color32::from_rgb(40, 90, 180),
            );
            text(0.28, 0.55, "W", 0.32, Color32::WHITE);
        }
        Icon::Export | Icon::Import => {
            page(0.1, 0.06, 0.6, 0.9);
            let (a, b) = if icon == Icon::Export {
                (u(0.45, 0.6), u(0.95, 0.6))
            } else {
                (u(0.95, 0.6), u(0.45, 0.6))
            };
            arrow(a, b, Stroke::new(w * 1.6, accent));
        }
        Icon::Redact => {
            page(0.15, 0.06, 0.85, 0.94);
            p.rect_filled(Rect::from_min_max(u(0.26, 0.4), u(0.74, 0.52)), 0.0, Color32::BLACK);
            p.line_segment([u(0.26, 0.65), u(0.74, 0.65)], thin);
            p.line_segment([u(0.26, 0.78), u(0.6, 0.78)], thin);
        }
        Icon::Merge => {
            page(0.05, 0.05, 0.45, 0.55);
            page(0.55, 0.05, 0.95, 0.55);
            arrow(u(0.25, 0.6), u(0.5, 0.9), Stroke::new(w * 1.3, accent));
            arrow(u(0.75, 0.6), u(0.5, 0.9), Stroke::new(w * 1.3, accent));
        }
        Icon::Split => {
            page(0.3, 0.05, 0.7, 0.45);
            arrow(u(0.5, 0.5), u(0.2, 0.9), Stroke::new(w * 1.3, accent));
            arrow(u(0.5, 0.5), u(0.8, 0.9), Stroke::new(w * 1.3, accent));
        }
        Icon::Swap => {
            arrow(u(0.15, 0.35), u(0.85, 0.35), Stroke::new(w * 1.6, accent));
            arrow(u(0.85, 0.65), u(0.15, 0.65), Stroke::new(w * 1.6, red));
        }
        Icon::Resize => {
            p.rect_stroke(Rect::from_min_max(u(0.1, 0.1), u(0.9, 0.9)), 0.0, thin, egui::StrokeKind::Inside);
            p.rect(
                Rect::from_min_max(u(0.1, 0.45), u(0.55, 0.9)),
                0.0,
                Color32::WHITE,
                st,
                egui::StrokeKind::Inside,
            );
            arrow(u(0.45, 0.55), u(0.85, 0.15), Stroke::new(w * 1.3, accent));
        }
        Icon::Crop => {
            p.add(PathShape::line(
                vec![u(0.25, 0.05), u(0.25, 0.75), u(0.95, 0.75)],
                Stroke::new(w * 1.6, color),
            ));
            p.add(PathShape::line(
                vec![u(0.05, 0.25), u(0.75, 0.25), u(0.75, 0.95)],
                Stroke::new(w * 1.6, accent),
            ));
        }
        Icon::Margins => {
            page(0.15, 0.05, 0.85, 0.95);
            p.rect_stroke(
                Rect::from_min_max(u(0.27, 0.2), u(0.73, 0.82)),
                0.0,
                Stroke::new(w * 0.8, accent),
                egui::StrokeKind::Inside,
            );
        }
        Icon::Watermark => {
            page(0.15, 0.05, 0.85, 0.95);
            p.text(
                u(0.5, 0.55),
                Align2::CENTER_CENTER,
                "W",
                FontId::proportional(s * 0.45),
                Color32::from_rgba_unmultiplied(200, 60, 60, 120),
            );
        }
        Icon::Background => {
            p.rect_filled(
                Rect::from_min_max(u(0.15, 0.05), u(0.85, 0.95)),
                1.0,
                Color32::from_rgb(200, 225, 250),
            );
            p.rect_stroke(
                Rect::from_min_max(u(0.15, 0.05), u(0.85, 0.95)),
                1.0,
                st,
                egui::StrokeKind::Inside,
            );
        }
        Icon::HeaderFooter => {
            page(0.15, 0.05, 0.85, 0.95);
            p.rect_filled(Rect::from_min_max(u(0.25, 0.15), u(0.62, 0.24)), 0.0, accent);
            p.rect_filled(Rect::from_min_max(u(0.25, 0.8), u(0.75, 0.88)), 0.0, accent);
        }
        Icon::Numbering => {
            page(0.15, 0.05, 0.85, 0.95);
            text(0.5, 0.58, "#", 0.45, accent);
        }
        Icon::Count => {
            text(0.5, 0.32, "ABC", 0.3, accent);
            text(0.5, 0.72, "123", 0.3, color);
        }
        Icon::Compare => {
            page(0.05, 0.1, 0.48, 0.9);
            page(0.52, 0.1, 0.95, 0.9);
            p.line_segment([u(0.15, 0.5), u(0.38, 0.5)], Stroke::new(w * 1.4, red));
            p.line_segment(
                [u(0.62, 0.5), u(0.85, 0.5)],
                Stroke::new(w * 1.4, Color32::from_rgb(40, 160, 60)),
            );
        }
        Icon::Accessibility => {
            p.circle_stroke(u(0.5, 0.5), s * 0.42, st);
            p.circle_filled(u(0.5, 0.25), s * 0.07, accent);
            p.line_segment([u(0.25, 0.4), u(0.75, 0.4)], Stroke::new(w * 1.4, accent));
            p.add(PathShape::line(
                vec![u(0.36, 0.82), u(0.5, 0.5), u(0.64, 0.82)],
                Stroke::new(w * 1.4, accent),
            ));
        }
        Icon::Sort => {
            text(0.3, 0.3, "A", 0.3, color);
            text(0.3, 0.72, "Z", 0.3, color);
            arrow(u(0.72, 0.15), u(0.72, 0.88), Stroke::new(w * 1.4, accent));
        }
        Icon::Help => {
            p.circle_filled(u(0.5, 0.5), s * 0.42, accent);
            text(0.5, 0.52, "?", 0.55, Color32::WHITE);
        }
        Icon::Bug => {
            p.add(egui::Shape::ellipse_filled(
                u(0.5, 0.58),
                vec2(s * 0.2, s * 0.28),
                Color32::from_rgb(90, 140, 60),
            ));
            p.circle_filled(u(0.5, 0.25), s * 0.11, Color32::from_rgb(70, 110, 50));
            for y in [0.45f32, 0.6, 0.75] {
                p.line_segment([u(0.15, y), u(0.85, y)], thin);
            }
            p.add(PathShape::convex_polygon(vec![u(0.75, 0.62), u(0.95, 0.95), u(0.55, 0.95)], yellow, thin));
        }
        Icon::Console => {
            p.rect(
                Rect::from_min_max(u(0.08, 0.15), u(0.92, 0.85)),
                2.0,
                Color32::from_gray(40),
                st,
                egui::StrokeKind::Inside,
            );
            p.add(PathShape::line(
                vec![u(0.22, 0.38), u(0.36, 0.5), u(0.22, 0.62)],
                Stroke::new(w * 1.3, Color32::WHITE),
            ));
            p.line_segment([u(0.42, 0.65), u(0.62, 0.65)], Stroke::new(w * 1.3, Color32::WHITE));
        }
        Icon::Web => {
            p.circle_stroke(u(0.5, 0.5), s * 0.4, Stroke::new(w, accent));
            p.add(egui::Shape::ellipse_stroke(
                u(0.5, 0.5),
                vec2(s * 0.17, s * 0.4),
                Stroke::new(w * 0.8, accent),
            ));
            p.line_segment([u(0.1, 0.5), u(0.9, 0.5)], Stroke::new(w * 0.8, accent));
            p.line_segment([u(0.18, 0.3), u(0.82, 0.3)], Stroke::new(w * 0.6, accent));
            p.line_segment([u(0.18, 0.7), u(0.82, 0.7)], Stroke::new(w * 0.6, accent));
        }
        Icon::Pointer => {
            let pts = vec![
                u(0.3, 0.1),
                u(0.3, 0.8),
                u(0.47, 0.64),
                u(0.6, 0.92),
                u(0.7, 0.87),
                u(0.57, 0.6),
                u(0.8, 0.6),
            ];
            p.add(PathShape::closed_line(pts, st));
        }
        Icon::Camera => {
            p.rect(
                Rect::from_min_max(u(0.08, 0.3), u(0.92, 0.82)),
                3.0,
                Color32::from_gray(200),
                st,
                egui::StrokeKind::Inside,
            );
            p.rect_filled(Rect::from_min_max(u(0.35, 0.2), u(0.6, 0.3)), 1.0, color);
            p.circle(u(0.5, 0.56), s * 0.16, Color32::WHITE, st);
        }
        Icon::AreaHighlight => {
            p.rect_filled(Rect::from_min_max(u(0.1, 0.25), u(0.9, 0.75)), 1.0, yellow);
            p.rect_stroke(
                Rect::from_min_max(u(0.1, 0.25), u(0.9, 0.75)),
                1.0,
                Stroke::new(w * 0.8, color),
                egui::StrokeKind::Inside,
            );
        }
        Icon::Replace => {
            text(0.5, 0.4, "T", 0.55, color);
            p.line_segment([u(0.2, 0.42), u(0.8, 0.42)], Stroke::new(w * 1.4, red));
            p.add(PathShape::line(
                vec![u(0.6, 0.95), u(0.72, 0.75), u(0.84, 0.95)],
                Stroke::new(w * 1.4, accent),
            ));
        }
        Icon::Caret => {
            p.line_segment([u(0.1, 0.4), u(0.9, 0.4)], thin);
            p.add(PathShape::line(vec![u(0.3, 0.9), u(0.5, 0.5), u(0.7, 0.9)], Stroke::new(w * 1.8, accent)));
        }
        Icon::Typewriter => {
            text(0.5, 0.45, "Tt", 0.55, color);
            p.line_segment([u(0.2, 0.85), u(0.8, 0.85)], Stroke::new(w * 1.2, accent));
        }
        Icon::Callout => {
            p.rect(
                Rect::from_min_max(u(0.35, 0.1), u(0.95, 0.55)),
                1.0,
                Color32::from_rgb(255, 255, 220),
                st,
                egui::StrokeKind::Inside,
            );
            text(0.65, 0.33, "T", 0.35, color);
            p.add(PathShape::line(vec![u(0.35, 0.4), u(0.2, 0.6), u(0.05, 0.92)], Stroke::new(w * 1.2, red)));
        }
        Icon::Polyline => {
            p.add(PathShape::line(
                vec![u(0.08, 0.85), u(0.3, 0.25), u(0.6, 0.7), u(0.92, 0.15)],
                Stroke::new(w * 1.3, red),
            ));
        }
        Icon::Cloud => {
            for (x, y, r) in [(0.3, 0.55, 0.18), (0.5, 0.42, 0.22), (0.7, 0.55, 0.18), (0.5, 0.65, 0.18)] {
                p.circle_stroke(u(x, y), s * r, Stroke::new(w * 1.2, red));
            }
        }
        Icon::Eraser => {
            let pts = vec![u(0.15, 0.62), u(0.55, 0.22), u(0.88, 0.55), u(0.48, 0.95)];
            p.add(PathShape::convex_polygon(pts, Color32::from_rgb(240, 150, 170), st));
            p.line_segment([u(0.32, 0.45), u(0.65, 0.78)], thin);
        }
        Icon::MeasureDistance | Icon::MeasurePerimeter | Icon::MeasureArea => {
            match icon {
                Icon::MeasureDistance => {
                    arrow(u(0.5, 0.5), u(0.08, 0.5), Stroke::new(w * 1.2, accent));
                    arrow(u(0.5, 0.5), u(0.92, 0.5), Stroke::new(w * 1.2, accent));
                }
                Icon::MeasurePerimeter => {
                    let pts = vec![u(0.15, 0.25), u(0.85, 0.2), u(0.75, 0.8), u(0.25, 0.85)];
                    p.add(PathShape::closed_line(pts, Stroke::new(w * 1.3, accent)));
                }
                _ => {
                    let pts = vec![u(0.15, 0.25), u(0.85, 0.2), u(0.75, 0.8), u(0.25, 0.85)];
                    p.add(PathShape::convex_polygon(
                        pts,
                        accent.gamma_multiply(0.35),
                        Stroke::new(w * 1.3, accent),
                    ));
                }
            }
            for x in [0.2f32, 0.4, 0.6, 0.8] {
                p.line_segment([u(x, 0.92), u(x, 0.99)], thin);
            }
        }
        Icon::Signature => {
            let pts: Vec<Pos2> = (0..=20)
                .map(|i| {
                    let t = i as f32 / 20.0;
                    u(0.08 + t * 0.84, 0.55 - 0.25 * (t * 9.0).sin() * (1.0 - t))
                })
                .collect();
            p.add(PathShape::line(pts, Stroke::new(w * 1.3, Color32::from_rgb(20, 40, 120))));
            p.line_segment([u(0.08, 0.85), u(0.92, 0.85)], thin);
        }
        Icon::FieldText => {
            p.rect(
                Rect::from_min_max(u(0.05, 0.3), u(0.95, 0.7)),
                1.0,
                Color32::WHITE,
                st,
                egui::StrokeKind::Inside,
            );
            text(0.3, 0.5, "T", 0.35, accent);
            p.line_segment([u(0.5, 0.38), u(0.5, 0.62)], thin);
        }
        Icon::FieldButton => {
            p.rect(
                Rect::from_min_max(u(0.08, 0.28), u(0.92, 0.72)),
                4.0,
                Color32::from_gray(220),
                st,
                egui::StrokeKind::Inside,
            );
            text(0.5, 0.5, "OK", 0.3, color);
        }
        Icon::CheckBox => {
            p.rect(
                Rect::from_min_max(u(0.15, 0.15), u(0.85, 0.85)),
                2.0,
                Color32::WHITE,
                st,
                egui::StrokeKind::Inside,
            );
            p.add(PathShape::line(
                vec![u(0.28, 0.5), u(0.45, 0.68), u(0.74, 0.3)],
                Stroke::new(w * 1.8, accent),
            ));
        }
        Icon::Radio => {
            p.circle(u(0.5, 0.5), s * 0.36, Color32::WHITE, st);
            p.circle_filled(u(0.5, 0.5), s * 0.17, accent);
        }
        Icon::ListBox => {
            p.rect(
                Rect::from_min_max(u(0.08, 0.12), u(0.92, 0.88)),
                1.0,
                Color32::WHITE,
                st,
                egui::StrokeKind::Inside,
            );
            p.rect_filled(Rect::from_min_max(u(0.12, 0.4), u(0.88, 0.58)), 0.0, accent.gamma_multiply(0.4));
            for y in [0.27f32, 0.49, 0.72] {
                p.line_segment([u(0.2, y), u(0.72, y)], thin);
            }
        }
        Icon::ComboBox => {
            p.rect(
                Rect::from_min_max(u(0.05, 0.3), u(0.95, 0.7)),
                1.0,
                Color32::WHITE,
                st,
                egui::StrokeKind::Inside,
            );
            p.add(PathShape::convex_polygon(
                vec![u(0.66, 0.42), u(0.86, 0.42), u(0.76, 0.58)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Logo => {
            p.rect_filled(Rect::from_min_max(u(0.05, 0.05), u(0.95, 0.95)), s * 0.18, red);
            text(0.5, 0.52, "M", 0.62, Color32::WHITE);
        }
    }
}

/// Size of the icon square for a given button height.
pub fn icon_size(height: f32) -> Vec2 {
    Vec2::splat(height)
}
