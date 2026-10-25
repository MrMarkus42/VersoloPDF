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
