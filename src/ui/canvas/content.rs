//! "Objekte bearbeiten" and "Text bearbeiten" on the page:
//! hover frames, selection with handles,
//! rubber band, moving with a live picture, scaling, rotating, the context
//! menu and the text editor on the page.

use egui::{Color32, CursorIcon, Pos2, Rect, Shape, Stroke, StrokeKind, Ui, pos2, vec2};

use super::{CanvasCtx, CanvasEvent, PageOnScreen, find_page};
use crate::content::{Affine, ContentObject, ObjectFilter, TextStyle, ZOrder, fit_rect, text_blocks};
use crate::geometry::PointF;
use crate::ui::content_state::{ContentCmd, ContentOp, ObjDrag, TextEditing, union};
use crate::ui::tab::DocTab;
use crate::ui::tools::Tool;
use crate::ui::widgets::to_color32;

const ACCENT: Color32 = Color32::from_rgb(30, 110, 230);
const HOVER: Color32 = Color32::from_rgb(120, 170, 240);
/// Half size of a handle (screen points).
const HANDLE: f32 = 4.0;

fn screen_point(pos: &PageOnScreen, p: PointF) -> Pos2 {
    pos.view_to_screen(pos.geo.pdf_to_view(p))
}

fn screen_quad(pos: &PageOnScreen, o: &ContentObject) -> Vec<Pos2> {
    o.quad.iter().map(|p| screen_point(pos, *p)).collect()
}

fn screen_rect(pos: &PageOnScreen, o: &ContentObject) -> Rect {
    let q = screen_quad(pos, o);
    Rect::from_points(&q)
}

/// Whether `p` lies in the polygon `q`.
fn in_polygon(q: &[Pos2], p: Pos2) -> bool {
    let mut inside = false;
    let mut j = q.len().wrapping_sub(1);
    for i in 0..q.len() {
        let (a, b) = (q[i], q[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Topmost object under the screen point `p` (thin objects get a margin).
fn object_at(objects: &[ContentObject], pos: &PageOnScreen, p: Pos2, filter: ObjectFilter) -> Option<usize> {
    objects.iter().rev().filter(|o| filter.accepts(o.kind) && !o.invisible).find_map(|o| {
        let q = screen_quad(pos, o);
        let r = Rect::from_points(&q);
        let hit =
            if r.width() < 6.0 || r.height() < 6.0 { r.expand(4.0).contains(p) } else { in_polygon(&q, p) };
        hit.then_some(o.index)
    })
}

/// The 8 handles of a rectangle (clockwise from top left).
fn handles(r: Rect) -> [Pos2; 8] {
    let (c, m) = (r.center(), r);
    [
        m.left_top(),
        pos2(c.x, m.top()),
        m.right_top(),
        pos2(m.right(), c.y),
        m.right_bottom(),
        pos2(c.x, m.bottom()),
        m.left_bottom(),
        pos2(m.left(), c.y),
    ]
}

fn rotate_handle(r: Rect) -> Pos2 {
    pos2(r.center().x, r.top() - 22.0)
}

fn handle_cursor(h: u8) -> CursorIcon {
    match h {
        0 | 4 => CursorIcon::ResizeNwSe,
        2 | 6 => CursorIcon::ResizeNeSw,
        1 | 5 => CursorIcon::ResizeVertical,
        _ => CursorIcon::ResizeHorizontal,
    }
}

/// The rectangle while dragging handle `h` to `cur` (Shift keeps the ratio).
fn resized(start: Rect, h: u8, cur: Pos2, keep_ratio: bool) -> Rect {
    let (mut l, mut t, mut r, mut b) = (start.left(), start.top(), start.right(), start.bottom());
    match h {
        0 => (l, t) = (cur.x, cur.y),
        1 => t = cur.y,
        2 => (r, t) = (cur.x, cur.y),
        3 => r = cur.x,
        4 => (r, b) = (cur.x, cur.y),
        5 => b = cur.y,
        6 => (l, b) = (cur.x, cur.y),
        _ => l = cur.x,
    }
    let mut out = Rect::from_min_max(pos2(l.min(r), t.min(b)), pos2(l.max(r), t.max(b)));
    if keep_ratio && matches!(h, 0 | 2 | 4 | 6) && start.height() > 0.0 {
        let ratio = start.width() / start.height();
        let w = out.width().max(out.height() * ratio);
        let size = vec2(w, w / ratio);
        out = match h {
            0 => Rect::from_min_size(start.right_bottom() - size, size),
            2 => Rect::from_min_size(pos2(start.left(), start.bottom() - size.y), size),
            4 => Rect::from_min_size(start.left_top(), size),
            _ => Rect::from_min_size(pos2(start.right() - size.x, start.top()), size),
        };
    }
    if out.width() < 2.0 || out.height() < 2.0 { start } else { out }
}

fn angle(center: Pos2, p: Pos2) -> f32 {
    (p.y - center.y).atan2(p.x - center.x).to_degrees()
}

fn dashed(painter: &egui::Painter, pts: &[Pos2], color: Color32) {
    let mut closed = pts.to_vec();
    if let Some(f) = pts.first() {
        closed.push(*f);
    }
    painter.extend(Shape::dashed_line(&closed, Stroke::new(1.0, color), 4.0, 3.0));
}

/// Texture of the page as drawn this frame (for live pictures while dragging).
fn page_texture(
    ui: &Ui,
    tab: &mut DocTab,
    cx: &mut CanvasCtx,
    pos: &PageOnScreen,
) -> Option<egui::TextureId> {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let max_side = (ctx.input(|i| i.max_texture_side) as u32).min(4096);
    let px_w = (pos.rect.width() * ppp).round().max(1.0) as u32;
    let px_h = (pos.rect.height() * ppp).round().max(1.0) as u32;
    let fit = (max_side as f32 / px_w.max(px_h) as f32).min(1.0);
    let base = [((px_w as f32 * fit) as u32).max(1), ((px_h as f32 * fit) as u32).max(1)];
    let opts = tab.render_options();
    let id = tab.id;
    tab.textures.page(&ctx, &tab.doc, pos.page, base, cx.budget, id, opts).map(|img| img.tex.id())
}

/// Draws the part `from` of the page picture into `to` (white below `from`).
fn draw_moved(painter: &egui::Painter, tex: egui::TextureId, page: Rect, from: Rect, to: Rect) {
    let uv = Rect::from_min_max(
        pos2((from.left() - page.left()) / page.width(), (from.top() - page.top()) / page.height()),
        pos2((from.right() - page.left()) / page.width(), (from.bottom() - page.top()) / page.height()),
    );
    painter.rect_filled(from, 0.0, Color32::from_white_alpha(230));
    painter.image(tex, to, uv, Color32::from_white_alpha(235));
}

/// Entry point from the canvas for the two content tools.
pub(super) fn interact(
    ui: &mut Ui,
    tab: &mut DocTab,
    cx: &mut CanvasCtx,
    response: &egui::Response,
    on_screen: &[PageOnScreen],
) {
    let rev = tab.doc.revision();
    // Changes made elsewhere (undo, other tools) invalidate the selection.
    if tab.content.revision != rev {
        tab.content.clear();
        tab.content.revision = rev;
        let stale =
            tab.content.text.as_ref().map(|t| (t.page, t.block.objects.iter().copied().max().unwrap_or(0)));
        if let Some((page, max)) = stale
            && max >= tab.objects(page).len()
        {
            tab.content.text = None;
        }
    }
    if response.dragged_by(egui::PointerButton::Middle) {
        let offset = tab.scroll_offset - response.drag_delta();
        tab.pending_scroll = Some(crate::ui::tab::ScrollRequest::Offset(offset.max(egui::Vec2::ZERO)));
        return;
    }
    match cx.tool {
        Tool::EditText => text_tool(ui, tab, cx, response, on_screen),
        _ => objects_tool(ui, tab, cx, response, on_screen),
    }
}

fn selection_rect(tab: &mut DocTab, pos: &PageOnScreen) -> Option<Rect> {
    let objects = tab.objects(pos.page);
    let sel = &tab.content.selected;
    union(objects.iter().filter(|o| sel.contains(&o.index)).map(|o| o.rect))
        .map(|r| pos.pdf_rect_to_screen(&r))
}

fn objects_tool(
    ui: &mut Ui,
    tab: &mut DocTab,
    cx: &mut CanvasCtx,
    response: &egui::Response,
    on_screen: &[PageOnScreen],
) {
    let painter = ui.painter_at(ui.clip_rect());
    let filter = tab.content.filter;
    let shift = ui.input(|i| i.modifiers.shift);
    let command = ui.input(|i| i.modifiers.command);
    let sel_page = on_screen.iter().find(|p| p.page == tab.content.page).copied();
    let sel_rect =
        sel_page.and_then(|p| if tab.content.selected.is_empty() { None } else { selection_rect(tab, &p) });

    // Hover: frame and cursor.
    if let Some(h) = response.hover_pos()
        && tab.content.drag.is_none()
        && let Some(pos) = find_page(on_screen, h)
    {
        let on_handle = sel_rect.and_then(|r| {
            if rotate_handle(r).distance(h) <= HANDLE + 3.0 {
                return Some(CursorIcon::Alias);
            }
            handles(r).iter().position(|p| p.distance(h) <= HANDLE + 2.0).map(|i| handle_cursor(i as u8))
        });
        let objects = tab.objects(pos.page);
        let hovered = object_at(&objects, &pos, h, filter);
        if let Some(i) = hovered
            && !(pos.page == tab.content.page && tab.content.selected.contains(&i))
            && let Some(o) = objects.iter().find(|o| o.index == i)
        {
            dashed(&painter, &screen_quad(&pos, o), HOVER);
        }
        let cursor =
            on_handle.unwrap_or(if hovered.is_some() { CursorIcon::Move } else { CursorIcon::Default });
        ui.ctx().set_cursor_icon(cursor);
    }

    // Start of a drag.
    let press = ui.input(|i| i.pointer.press_origin());
    if response.drag_started_by(egui::PointerButton::Primary)
        && let Some(start) = press
    {
        let handle = sel_rect.and_then(|r| {
            if rotate_handle(r).distance(start) <= HANDLE + 3.0 {
                return Some(ObjDrag::Rotate {
                    center: r.center(),
                    start: angle(r.center(), start),
                    cur: angle(r.center(), start),
                });
            }
            handles(r).iter().position(|p| p.distance(start) <= HANDLE + 2.0).map(|i| ObjDrag::Resize {
                handle: i as u8,
                start: r,
                cur: r,
            })
        });
        tab.content.drag = match handle {
            Some(d) => Some(d),
            None => match find_page(on_screen, start) {
                Some(pos) => {
                    let objects = tab.objects(pos.page);
                    match object_at(&objects, &pos, start, filter) {
                        Some(i) => {
                            let already = pos.page == tab.content.page && tab.content.selected.contains(&i);
                            if !already {
                                let rev = tab.doc.revision();
                                let mut sel = if shift && pos.page == tab.content.page {
                                    tab.content.selected.clone()
                                } else {
                                    Vec::new()
                                };
                                sel.push(i);
                                tab.content.select(pos.page, sel, rev);
                            }
                            Some(ObjDrag::Move { start, cur: start })
                        }
                        None => Some(ObjDrag::Rubber { start, cur: start }),
                    }
                }
                None => None,
            },
        };
    }

    // Dragging.
    if response.dragged_by(egui::PointerButton::Primary)
        && let Some(cur) = response.interact_pointer_pos()
    {
        match tab.content.drag.as_mut() {
            Some(ObjDrag::Move { cur: c, .. }) | Some(ObjDrag::Rubber { cur: c, .. }) => *c = cur,
            Some(ObjDrag::Resize { handle, start, cur: c }) => *c = resized(*start, *handle, cur, shift),
            Some(ObjDrag::Rotate { center, start, cur: c }) => {
                let mut a = angle(*center, cur);
                if shift {
                    a = *start + ((a - *start) / 15.0).round() * 15.0;
                }
                *c = a;
            }
            None => {}
        }
    }

    // Preview of the drag.
    if let (Some(drag), Some(pos)) = (tab.content.drag.clone(), sel_page) {
        let objects = tab.objects(pos.page);
        let selected: Vec<&ContentObject> =
            objects.iter().filter(|o| tab.content.selected.contains(&o.index)).collect();
        match &drag {
            ObjDrag::Move { start, cur } => {
                let d = *cur - *start;
                if let Some(tex) = page_texture(ui, tab, cx, &pos) {
                    for o in &selected {
                        let from = screen_rect(&pos, o).expand(1.0);
                        draw_moved(&painter, tex, pos.rect, from, from.translate(d));
                    }
                }
                if let Some(r) = sel_rect {
                    painter.rect_stroke(r.translate(d), 0.0, Stroke::new(1.0, ACCENT), StrokeKind::Outside);
                }
            }
            ObjDrag::Resize { start, cur, .. } => {
                if let Some(tex) = page_texture(ui, tab, cx, &pos) {
                    draw_moved(&painter, tex, pos.rect, *start, *cur);
                }
                painter.rect_stroke(*cur, 0.0, Stroke::new(1.0, ACCENT), StrokeKind::Outside);
            }
            ObjDrag::Rotate { center, start, cur } => {
                let a = (cur - start).to_radians();
                let rot = |p: Pos2| {
                    let v = p - *center;
                    *center + vec2(v.x * a.cos() - v.y * a.sin(), v.x * a.sin() + v.y * a.cos())
                };
                for o in &selected {
                    let q: Vec<Pos2> = screen_quad(&pos, o).into_iter().map(rot).collect();
                    dashed(&painter, &q, ACCENT);
                }
                painter.text(
                    *center,
                    egui::Align2::CENTER_CENTER,
                    format!("{:.0}°", -(cur - start)),
                    egui::FontId::proportional(13.0),
                    ACCENT,
                );
            }
            ObjDrag::Rubber { .. } => {}
        }
    }
    if let Some(ObjDrag::Rubber { start, cur }) = &tab.content.drag {
        let r = Rect::from_two_pos(*start, *cur);
        painter.rect(r, 0.0, ACCENT.gamma_multiply(0.12), Stroke::new(1.0, ACCENT), StrokeKind::Inside);
    }

    // End of a drag: one change.
    if response.drag_stopped()
        && let Some(drag) = tab.content.drag.take()
    {
        finish_drag(tab, cx, drag, on_screen, sel_page, command || shift);
    }

    // Click: select, Shift adds/removes; empty space clears.
    if response.clicked()
        && let Some(p) = response.interact_pointer_pos()
    {
        let rev = tab.doc.revision();
        match find_page(on_screen, p) {
            Some(pos) => {
                let objects = tab.objects(pos.page);
                match object_at(&objects, &pos, p, filter) {
                    Some(i) if shift && pos.page == tab.content.page => {
                        let mut sel = tab.content.selected.clone();
                        if let Some(k) = sel.iter().position(|x| *x == i) {
                            sel.remove(k);
                        } else {
                            sel.push(i);
                        }
                        tab.content.select(pos.page, sel, rev);
                    }
                    Some(i) => tab.content.select(pos.page, vec![i], rev),
                    None => tab.content.clear(),
                }
                tab.current_page = pos.page;
            }
            None => tab.content.clear(),
        }
    }

    // Double click on text: edit it.
    if response.double_clicked()
        && let Some(p) = response.interact_pointer_pos()
        && let Some(pos) = find_page(on_screen, p)
    {
        open_text_at(tab, &pos, p);
        if tab.content.text.is_some() {
            cx.events.push(CanvasEvent::Content(ContentOp::Command(ContentCmd::EditText)));
        }
    }

    // Selection frame with handles.
    if tab.content.drag.is_none()
        && let (Some(pos), Some(r)) =
            (sel_page, sel_rect.or_else(|| sel_page.and_then(|p| selection_rect(tab, &p))))
    {
        let objects = tab.objects(pos.page);
        for o in objects.iter().filter(|o| tab.content.selected.contains(&o.index)) {
            dashed(&painter, &screen_quad(&pos, o), ACCENT);
        }
        painter.rect_stroke(r, 0.0, Stroke::new(1.0, ACCENT), StrokeKind::Outside);
        painter.line_segment([pos2(r.center().x, r.top()), rotate_handle(r)], Stroke::new(1.0, ACCENT));
        painter.circle(rotate_handle(r), HANDLE + 1.0, Color32::WHITE, Stroke::new(1.0, ACCENT));
        for h in handles(r) {
            painter.rect(
                Rect::from_center_size(h, vec2(2.0 * HANDLE, 2.0 * HANDLE)),
                0.0,
                Color32::WHITE,
                Stroke::new(1.0, ACCENT),
                StrokeKind::Middle,
            );
        }
    }

    context_menu(tab, cx, response, on_screen);
}

fn finish_drag(
    tab: &mut DocTab,
    cx: &mut CanvasCtx,
    drag: ObjDrag,
    on_screen: &[PageOnScreen],
    sel_page: Option<PageOnScreen>,
    add: bool,
) {
    let page = tab.content.page;
    let selected = tab.content.selected.clone();
    match drag {
        ObjDrag::Rubber { start, cur } => {
            let Some(pos) = find_page(on_screen, start) else { return };
            let r = Rect::from_two_pos(start, cur);
            let filter = tab.content.filter;
            let objects = tab.objects(pos.page);
            let mut hit: Vec<usize> = objects
                .iter()
                .filter(|o| filter.accepts(o.kind) && !o.invisible && r.contains_rect(screen_rect(&pos, o)))
                .map(|o| o.index)
                .collect();
            if add && pos.page == page {
                let extra: Vec<usize> =
                    tab.content.selected.iter().copied().filter(|i| !hit.contains(i)).collect();
                hit.extend(extra);
            }
            // Comments are not page content: say which tool selects them.
            if hit.is_empty()
                && tab
                    .annotations(pos.page)
                    .iter()
                    .any(|a| r.contains(pos.pdf_rect_to_screen(&a.rect).center()))
            {
                cx.events.push(CanvasEvent::Status(cx.t.hint_comments_not_objects.into()));
            }
            let rev = tab.doc.revision();
            tab.content.select(pos.page, hit, rev);
        }
        _ if selected.is_empty() => {}
        ObjDrag::Move { start, cur } => {
            let Some(pos) = sel_page else { return };
            if start.distance(cur) < 1.0 {
                return;
            }
            let (a, b) = (pos.pdf_at(start), pos.pdf_at(cur));
            let m = Affine::translate(b.x - a.x, b.y - a.y);
            cx.events.push(CanvasEvent::Content(ContentOp::Transform {
                page,
                moves: selected.iter().map(|i| (*i, m)).collect(),
            }));
        }
        ObjDrag::Resize { start, cur, .. } => {
            let Some(pos) = sel_page else { return };
            if start == cur {
                return;
            }
            let from = pos.view_rect_to_pdf(pos.view_at(start.min), pos.view_at(start.max));
            let to = pos.view_rect_to_pdf(pos.view_at(cur.min), pos.view_at(cur.max));
            let m = fit_rect(&from, &to);
            cx.events.push(CanvasEvent::Content(ContentOp::Transform {
                page,
                moves: selected.iter().map(|i| (*i, m)).collect(),
            }));
        }
        ObjDrag::Rotate { center, start, cur } => {
            let Some(pos) = sel_page else { return };
            let d = cur - start;
            if d.abs() < 0.5 {
                return;
            }
            // Screen angles turn clockwise (y down); PDF angles counter-clockwise.
            let m = Affine::about(Affine::rotate(-d), pos.pdf_at(center));
            cx.events.push(CanvasEvent::Content(ContentOp::Transform {
                page,
                moves: selected.iter().map(|i| (*i, m)).collect(),
            }));
        }
    }
}

fn context_menu(tab: &mut DocTab, cx: &mut CanvasCtx, response: &egui::Response, on_screen: &[PageOnScreen]) {
    if response.secondary_clicked()
        && let Some(p) = response.interact_pointer_pos()
        && let Some(pos) = find_page(on_screen, p)
    {
        let filter = tab.content.filter;
        let objects = tab.objects(pos.page);
        if let Some(i) = object_at(&objects, &pos, p, filter)
            && !(pos.page == tab.content.page && tab.content.selected.contains(&i))
        {
            let rev = tab.doc.revision();
            tab.content.select(pos.page, vec![i], rev);
        }
        tab.content.menu_at = Some((pos.page, pos.pdf_at(p)));
    }
    let de = cx.t.tab_home == "Start";
    let has_sel = !tab.content.selected.is_empty();
    let objects = tab.objects(tab.content.page);
    let image = tab.content.selected.len() == 1
        && objects
            .iter()
            .any(|o| o.index == tab.content.selected[0] && o.kind == crate::content::ObjectKind::Image);
    let text = objects
        .iter()
        .any(|o| tab.content.selected.contains(&o.index) && o.kind == crate::content::ObjectKind::Text);
    let at = tab.content.menu_at;
    let mut chosen: Option<ContentOp> = None;
    let mut paste = None;
    response.context_menu(|ui| {
        ui.set_min_width(240.0);
        let mut item = |ui: &mut Ui, cmd: ContentCmd, enabled: bool| {
            let (g, e) = cmd.names();
            let label = match cmd.shortcut() {
                Some((sg, se)) => format!("{}\t{}", if de { g } else { e }, if de { sg } else { se }),
                None => (if de { g } else { e }).to_string(),
            };
            if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                chosen = Some(ContentOp::Command(cmd));
                ui.close();
            }
        };
        if text {
            item(ui, ContentCmd::EditText, true);
            ui.separator();
        }
        item(ui, ContentCmd::Cut, has_sel);
        item(ui, ContentCmd::Copy, has_sel);
        if ui.button(if de { "Einfügen\tStrg+V" } else { "Paste\tCtrl+V" }).clicked() {
            paste = at;
            ui.close();
        }
        item(ui, ContentCmd::Delete, has_sel);
        item(ui, ContentCmd::Duplicate, has_sel);
        ui.separator();
        ui.add_enabled_ui(has_sel, |ui| {
            ui.menu_button(if de { "Anordnen" } else { "Arrange" }, |ui| {
                for z in [ZOrder::Front, ZOrder::Forward, ZOrder::Backward, ZOrder::Back] {
                    item(ui, ContentCmd::Z(z), true);
                }
            });
            ui.menu_button(if de { "Drehen und Spiegeln" } else { "Rotate and flip" }, |ui| {
                for c in [
                    ContentCmd::RotateLeft,
                    ContentCmd::RotateRight,
                    ContentCmd::Rotate180,
                    ContentCmd::FlipH,
                    ContentCmd::FlipV,
                ] {
                    item(ui, c, true);
                }
            });
        });
        ui.menu_button(if de { "Auswählen" } else { "Select" }, |ui| {
            item(ui, ContentCmd::SelectAll, true);
            for f in [ObjectFilter::Text, ObjectFilter::Images, ObjectFilter::Shapes] {
                item(ui, ContentCmd::SelectKind(f), true);
            }
        });
        item(ui, ContentCmd::Deselect, has_sel);
        if image {
            ui.separator();
            item(ui, ContentCmd::ExportImage, true);
            item(ui, ContentCmd::ReplaceImage, true);
        }
    });
    if let Some(op) = chosen {
        cx.events.push(CanvasEvent::Content(op));
    }
    if let Some((page, at)) = paste {
        cx.events.push(CanvasEvent::Content(ContentOp::PasteAt { page, at }));
    }
}

/// Opens the text block under the screen point `p` in the editor.
fn open_text_at(tab: &mut DocTab, pos: &PageOnScreen, p: Pos2) -> bool {
    let objects = tab.objects(pos.page);
    let pdf = pos.pdf_at(p);
    let blocks = text_blocks(&objects);
    let Some(block) = blocks.into_iter().find(|b| b.rect.inflate(2.0).contains(pdf)) else { return false };
    let width = if block.lines.len() > 1 { block.rect.width() } else { 0.0 };
    let rev = tab.doc.revision();
    tab.content.select(pos.page, block.objects.clone(), rev);
    tab.content.text = Some(TextEditing {
        page: pos.page,
        text: block.text(),
        style: TextStyle::of_block(&block),
        width,
        block,
        focus_requested: false,
    });
    true
}

fn text_tool(
    ui: &mut Ui,
    tab: &mut DocTab,
    cx: &mut CanvasCtx,
    response: &egui::Response,
    on_screen: &[PageOnScreen],
) {
    let painter = ui.painter_at(ui.clip_rect());
    // Frames around the text blocks of the page under the pointer.
    if tab.content.text.is_none()
        && let Some(h) = response.hover_pos()
        && let Some(pos) = find_page(on_screen, h)
    {
        let objects = tab.objects(pos.page);
        let pdf = pos.pdf_at(h);
        let mut over = false;
        for b in text_blocks(&objects) {
            let r = pos.pdf_rect_to_screen(&b.rect.inflate(1.0));
            let hot = b.rect.inflate(2.0).contains(pdf);
            over |= hot;
            painter.rect_stroke(
                r,
                0.0,
                Stroke::new(if hot { 1.5 } else { 0.7 }, if hot { ACCENT } else { HOVER }),
                StrokeKind::Outside,
            );
        }
        ui.ctx().set_cursor_icon(if over { CursorIcon::Text } else { CursorIcon::Default });
    }
    if response.clicked()
        && let Some(p) = response.interact_pointer_pos()
    {
        if tab.content.text.is_some() {
            commit_text(tab, cx);
        }
        if let Some(pos) = find_page(on_screen, p)
            && open_text_at(tab, &pos, p)
        {
            tab.current_page = pos.page;
        }
    }
    draw_text_editor(ui, tab, cx, on_screen);
}

/// Applies the open text editor (nothing if unchanged).
pub(super) fn commit_text(tab: &mut DocTab, cx: &mut CanvasCtx) {
    let Some(edit) = tab.content.text.take() else { return };
    if edit.changed() {
        cx.events.push(CanvasEvent::Content(edit.to_op()));
    }
}

fn draw_text_editor(ui: &mut Ui, tab: &mut DocTab, cx: &mut CanvasCtx, on_screen: &[PageOnScreen]) {
    let Some(edit) = tab.content.text.as_mut() else { return };
    let Some(pos) = on_screen.iter().find(|p| p.page == edit.page).copied() else { return };
    let b = &edit.block;
    let size = edit.style.size * pos.scale;
    let lines = edit.text.split('\n').count().max(1) as f32;
    let leading = b.leading() * edit.style.size / b.size.max(1.0) * pos.scale;
    let top = pos.pdf_rect_to_screen(&b.rect).top();
    let width = (pos.pdf_rect_to_screen(&b.rect).width()).max(edit.width * pos.scale).max(80.0) + size;
    let area = Rect::from_min_size(
        pos2(pos.pdf_rect_to_screen(&b.rect).left() - 2.0, top - 2.0),
        vec2(width + 4.0, leading * lines + size * 0.6 + 4.0),
    );
    let painter = ui.painter_at(ui.clip_rect());
    // Hide the old text and show the editor in its place.
    painter.rect_filled(pos.pdf_rect_to_screen(&b.rect.inflate(1.5)), 0.0, Color32::WHITE);
    painter.rect_filled(area, 2.0, Color32::WHITE);
    painter.rect_stroke(area.expand(1.0), 2.0, Stroke::new(1.0, ACCENT), StrokeKind::Outside);
    let align = match edit.style.align {
        crate::content::Align::Left => egui::Align::LEFT,
        crate::content::Align::Center => egui::Align::Center,
        crate::content::Align::Right => egui::Align::RIGHT,
    };
    let mut font = egui::FontId::proportional(size.max(4.0));
    if edit.style.font == crate::content::FontChoice::Courier
        || b.monospace && edit.style.font == crate::content::FontChoice::Original
    {
        font = egui::FontId::monospace(size.max(4.0));
    }
    let text_edit = egui::TextEdit::multiline(&mut edit.text)
        .font(font)
        .text_color(to_color32(edit.style.color))
        .frame(egui::Frame::NONE)
        .margin(egui::Margin::ZERO)
        .horizontal_align(align)
        .desired_width(width)
        .desired_rows(lines as usize)
        .id_salt(("content-text", edit.page, b.objects.first().copied().unwrap_or(0)));
    let resp = ui.put(area, text_edit);
    let first = !edit.focus_requested;
    if first {
        resp.request_focus();
        edit.focus_requested = true;
    }
    let (escape, apply) = ui.input(|i| {
        (i.key_pressed(egui::Key::Escape), i.key_pressed(egui::Key::Enter) && i.modifiers.command)
    });
    if escape {
        tab.content.text = None;
        return;
    }
    // A click on the ribbon (Format) must not end the editing; only a
    // click on the page outside the editor or Strg+Enter does.
    let clicked_outside = !first
        && ui.input(|i| i.pointer.primary_clicked())
        && ui
            .input(|i| i.pointer.interact_pos())
            .is_some_and(|p| !area.expand(4.0).contains(p) && ui.clip_rect().contains(p));
    if apply || clicked_outside {
        commit_text(tab, cx);
    }
}
