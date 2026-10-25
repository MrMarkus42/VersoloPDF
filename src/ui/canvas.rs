//! The document view: draws pages and handles the interactive tools.

use egui::{
    Color32, CursorIcon, Key, PointerButton, Pos2, Rect, ScrollArea, Sense, Stroke, StrokeKind,
    Ui, Vec2, pos2, vec2,
};

use crate::appearance::{StdFont, win_ansi};
use crate::geometry::{PageGeometry, PointF, RectF};
use crate::model::{AnnotKind, AnnotStyle, FieldKind, LinkTarget};
use crate::pdf_edit::NewAnnotation;
use crate::text::TextRange;
use crate::ui::cache::Budget;
use crate::ui::i18n::Strings;
use crate::ui::tab::{AnnotRef, DocTab, FieldEdit, Interaction, ScrollRequest, TextSelection};
use crate::ui::theme;
use crate::ui::tools::Tool;
use crate::ui::view::{
    Layout, MARGIN, PageLayout, ZoomMode, fit_page_zoom, fit_width_zoom, MAX_ZOOM, MIN_ZOOM,
};

/// Requests from the canvas that the application has to carry out.
#[derive(Debug, Clone, PartialEq)]
pub enum CanvasEvent {
    Create { page: usize, annot: Box<NewAnnotation> },
    RequestText { page: usize, rect: RectF, kind: AnnotKind },
    EditText(AnnotRef),
    Move { annot: AnnotRef, dx: f32, dy: f32 },
    OpenUri(String),
    CommitField { page: usize, annot_index: usize, value: String },
    SetChecked { page: usize, annot_index: usize, checked: bool },
    ChooseOption { page: usize, annot_index: usize, options: Vec<String>, current: String },
    MarkupSelection(Tool),
    CopySelection,
    DeleteSelected,
    ShowProperties,
    Status(String),
}

pub struct CanvasCtx<'a> {
    pub tool: Tool,
    pub style: AnnotStyle,
    pub stamp_label: String,
    pub stamp_icon: &'static str,
    pub highlight_fields: bool,
    pub budget: &'a mut Budget,
    pub events: &'a mut Vec<CanvasEvent>,
    pub t: &'static Strings,
}

/// Placement of a page on screen for one frame.
#[derive(Clone, Copy)]
struct PageOnScreen {
    page: usize,
    rect: Rect,
    geo: PageGeometry,
    scale: f32,
}

impl PageOnScreen {
    fn to_view(&self, p: Pos2) -> PointF {
        PointF::new((p.x - self.rect.min.x) / self.scale, (p.y - self.rect.min.y) / self.scale)
    }

    fn to_pdf(&self, p: Pos2) -> PointF {
        self.geo.view_to_pdf(self.to_view(p))
    }

    fn view_to_screen(&self, v: PointF) -> Pos2 {
        pos2(self.rect.min.x + v.x * self.scale, self.rect.min.y + v.y * self.scale)
    }

    fn pdf_rect_to_screen(&self, r: &RectF) -> Rect {
        let v = self.geo.pdf_rect_to_view(r);
        Rect::from_min_max(
            self.view_to_screen(PointF::new(v.x0, v.y0)),
            self.view_to_screen(PointF::new(v.x1, v.y1)),
        )
    }

}

fn scroll_offset_for(tab: &DocTab, req: &ScrollRequest, viewport: Vec2) -> Option<Vec2> {
    let scale = tab.scale();
    match req {
        ScrollRequest::Offset(o) => Some(*o),
        ScrollRequest::Page(p) => tab
            .layout
            .slot(*p)
            .map(|s| vec2(tab.scroll_offset.x, (s.rect.min.y + tab.content_shift.y - MARGIN / 2.0).max(0.0))),
        ScrollRequest::Point(p, v) => tab.layout.slot(*p).map(|s| {
            let target = s.rect.min.to_vec2() + tab.content_shift + vec2(v.x, v.y) * scale;
            (target - viewport / 2.0).max(Vec2::ZERO)
        }),
    }
}

pub fn show(ui: &mut Ui, tab: &mut DocTab, cx: &mut CanvasCtx) {
    let ctx = ui.ctx().clone();
    tab.textures.begin_frame();
    let avail = ui.available_size();
    let sizes = tab.page_sizes();
    if sizes.is_empty() {
        return;
    }
    let facing = tab.layout_mode == PageLayout::Facing;
    let max_w = sizes.iter().map(|s| s.0).fold(0.0, f32::max);
    let scrollbar = ui.spacing().scroll.bar_width + 4.0;
    match tab.zoom_mode {
        ZoomMode::FitWidth => tab.zoom = fit_width_zoom(max_w, avail.x - scrollbar, facing),
        ZoomMode::FitPage => {
            let page = sizes[tab.current_page.min(sizes.len() - 1)];
            tab.zoom = fit_page_zoom(page, avail - vec2(scrollbar, 0.0), facing);
        }
        ZoomMode::Custom => {}
    }

    // Ctrl + mouse wheel / pinch zoom anchored at the pointer.
    let zoom_delta = ui.input(|i| i.zoom_delta());
    let canvas_rect = Rect::from_min_size(ui.cursor().min, avail);
    if zoom_delta != 1.0
        && let Some(pointer) = ui.input(|i| i.pointer.hover_pos())
        && canvas_rect.contains(pointer)
    {
        let old_scale = tab.scale();
        let new_zoom = (tab.zoom * zoom_delta).clamp(MIN_ZOOM, MAX_ZOOM);
        let content_pt = (pointer - canvas_rect.min) + tab.scroll_offset - tab.content_shift;
        tab.zoom = new_zoom;
        tab.zoom_mode = ZoomMode::Custom;
        let factor = tab.scale() / old_scale;
        let new_offset = content_pt * factor - (pointer - canvas_rect.min);
        tab.pending_scroll = Some(ScrollRequest::Offset(new_offset.max(Vec2::ZERO)));
    }

    tab.layout = Layout::compute(&sizes, tab.zoom, tab.layout_mode, tab.current_page);

    let mut area = ScrollArea::both()
        .id_salt(("doc", tab.id))
        .auto_shrink([false, false]);
    if let Some(req) = tab.pending_scroll.take() {
        let viewport_size = avail - vec2(scrollbar, scrollbar);
        if let Some(off) = scroll_offset_for(tab, &req, viewport_size) {
            area = area.scroll_offset(off);
            tab.scroll_offset = off;
        }
    }
    let output = area.show_viewport(ui, |ui, viewport| {
        draw_and_interact(ui, tab, cx, viewport, &ctx);
    });
    tab.scroll_offset = output.state.offset;
}

fn draw_and_interact(ui: &mut Ui, tab: &mut DocTab, cx: &mut CanvasCtx, viewport: Rect, ctx: &egui::Context) {
    let layout_size = tab.layout.size;
    let size = vec2(layout_size.x.max(viewport.width()), layout_size.y.max(viewport.height()));
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let shift = vec2(
        ((size.x - layout_size.x) / 2.0).max(0.0),
        if tab.layout_mode == PageLayout::Single {
            ((size.y - layout_size.y) / 2.0).max(0.0)
        } else {
            0.0
        },
    );
    tab.content_shift = shift;
    tab.viewport = viewport;
    let origin = rect.min + shift;
    let painter = ui.painter_at(ui.clip_rect());
    painter.rect_filled(ui.clip_rect(), 0.0, theme::canvas_background(ui.visuals().dark_mode));

    // Track the current page in scrolling layouts.
    if tab.layout_mode != PageLayout::Single
        && let Some(p) = tab.layout.page_at(viewport.center().y - shift.y)
        && tab.pending_scroll.is_none()
    {
        tab.current_page = p;
    }

    let scale = tab.scale();
    let ppp = ctx.pixels_per_point();
    let max_side = (ctx.input(|i| i.max_texture_side) as u32).min(4096);
    let visible_screen = ui.clip_rect();
    let mut on_screen: Vec<PageOnScreen> = Vec::new();

    let slots = tab.layout.slots.clone();
    for slot in &slots {
        let screen = slot.rect.translate(origin.to_vec2());
        let Some(geo) = tab.doc.geometry(slot.page).copied() else {
            continue;
        };
        let pos = PageOnScreen {
            page: slot.page,
            rect: screen,
            geo,
            scale,
        };
        on_screen.push(pos);
        if !screen.intersects(visible_screen.expand(200.0)) {
            continue;
        }
        // Page shadow and paper.
        painter.rect_filled(screen.translate(vec2(2.0, 3.0)), 1.0, Color32::from_black_alpha(60));
        painter.rect_filled(screen, 0.0, Color32::WHITE);

        let px_w = (screen.width() * ppp).round().max(1.0) as u32;
        let px_h = (screen.height() * ppp).round().max(1.0) as u32;
        let fit = (max_side as f32 / px_w.max(px_h) as f32).min(1.0);
        let base_px = [((px_w as f32 * fit) as u32).max(1), ((px_h as f32 * fit) as u32).max(1)];
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        if let Some(img) = tab.textures.page(ctx, &tab.doc, slot.page, base_px, cx.budget, tab.id) {
            painter.image(img.tex.id(), screen, uv, Color32::WHITE);
        }
        if fit < 1.0 {
            // Zoomed in beyond the texture limit: add a sharp tile for the visible part.
            let vis = screen.intersect(visible_screen);
            if vis.width() > 0.0 && vis.height() > 0.0 {
                let ax = ((vis.min.x - screen.min.x) * ppp).max(0.0) as u32;
                let ay = ((vis.min.y - screen.min.y) * ppp).max(0.0) as u32;
                let aw = ((vis.width() * ppp).ceil() as u32).min(max_side.saturating_sub(512)).max(1);
                let ah = ((vis.height() * ppp).ceil() as u32).min(max_side.saturating_sub(512)).max(1);
                let aw = aw.min(px_w.saturating_sub(ax)).max(1);
                let ah = ah.min(px_h.saturating_sub(ay)).max(1);
                if let Some(tile) = tab.textures.tile(ctx, &tab.doc, slot.page, [px_w, px_h], [ax, ay, aw, ah], cx.budget, tab.id) {
                    let [tx, ty, tw, th] = tile.area;
                    let r = Rect::from_min_size(
                        screen.min + vec2(tx as f32 / ppp, ty as f32 / ppp),
                        vec2(tw as f32 / ppp, th as f32 / ppp),
                    );
                    painter.image(tile.tex.id(), r, uv, Color32::WHITE);
                }
            }
        }
        painter.rect_stroke(screen, 0.0, Stroke::new(1.0, Color32::from_gray(150)), StrokeKind::Outside);
        draw_overlays(&painter, tab, &pos, cx);
    }

    handle_input(ui, tab, cx, &response, &on_screen);
    draw_field_editor(ui, tab, &on_screen, cx);
    tab.textures.evict();
}

fn draw_overlays(painter: &egui::Painter, tab: &mut DocTab, pos: &PageOnScreen, cx: &CanvasCtx) {
    let page = pos.page;
    if cx.highlight_fields {
        for f in tab.fields(page).iter() {
            if !f.read_only && f.kind != FieldKind::PushButton {
                painter.rect_filled(pos.pdf_rect_to_screen(&f.rect), 0.0, Color32::from_rgba_unmultiplied(120, 160, 255, 50));
            }
        }
    }
    // Search hits.
    let current = tab.search.current;
    for (i, hit) in tab.search.hits.iter().enumerate().filter(|(_, h)| h.page == page) {
        let color = if Some(i) == current {
            Color32::from_rgba_unmultiplied(255, 120, 0, 110)
        } else {
            Color32::from_rgba_unmultiplied(255, 210, 0, 90)
        };
        for r in &hit.rects {
            painter.rect_filled(pos.pdf_rect_to_screen(r).expand(1.0), 1.0, color);
        }
    }
    // Text selection (also previewed while using a markup tool).
    let interaction = tab.interaction.clone();
    let selection = match &interaction {
        Interaction::Markup { page: p, anchor: Some(a), current, .. } if *p == page => {
            tab.page_text(page).and_then(|t| {
                t.hit_test(pos.geo.view_to_pdf(*current), 8.0).map(|c| TextRange::new(*a, c))
            })
        }
        _ => tab.selection.filter(|s| s.page == page).map(|s| s.range),
    };
    if let (Some(range), Some(text)) = (selection, tab.page_text(page)) {
        for r in text.line_rects(range) {
            painter.rect_filled(pos.pdf_rect_to_screen(&r), 0.0, Color32::from_rgba_unmultiplied(40, 110, 230, 70));
        }
    }
    // Selected annotation.
    if let Some(sel) = tab.selected_annot.filter(|s| s.page == page)
        && let Some(a) = tab.annotations(page).get(sel.index)
    {
        let mut r = pos.pdf_rect_to_screen(&a.rect).expand(2.0);
        if let Interaction::MoveAnnot { annot, delta } = &tab.interaction
            && *annot == sel
        {
            r = r.translate(*delta);
        }
        let accent = theme::ACCENT;
        painter.rect_stroke(r, 0.0, Stroke::new(1.5, accent), StrokeKind::Outside);
        for c in [r.left_top(), r.right_top(), r.left_bottom(), r.right_bottom()] {
            painter.rect_filled(Rect::from_center_size(c, vec2(6.0, 6.0)), 0.0, accent);
        }
    }
    // Shape preview.
    if let Interaction::Shape { page: p, start, current, points } = &tab.interaction
        && *p == page
    {
        let c = cx.style.color;
        let color = Color32::from_rgb(c.r, c.g, c.b);
        let stroke = Stroke::new((cx.style.width * pos.scale).max(1.0), color);
        let a = pos.view_to_screen(*start);
        let b = pos.view_to_screen(*current);
        match cx.tool {
            Tool::Rectangle | Tool::TextBox => {
                painter.rect_stroke(Rect::from_two_pos(a, b), 0.0, stroke, StrokeKind::Middle);
            }
            Tool::Ellipse => {
                let r = Rect::from_two_pos(a, b);
                painter.add(egui::Shape::ellipse_stroke(r.center(), r.size() / 2.0, stroke));
            }
            Tool::Line | Tool::Arrow => {
                painter.line_segment([a, b], stroke);
            }
            Tool::Pencil => {
                let pts: Vec<Pos2> = points.iter().map(|p| pos.view_to_screen(*p)).collect();
                painter.add(egui::Shape::line(pts, stroke));
            }
            _ => {}
        }
    }
}

fn find_page(on_screen: &[PageOnScreen], p: Pos2) -> Option<PageOnScreen> {
    on_screen.iter().find(|s| s.rect.contains(p)).copied()
}

/// Topmost comment annotation at `pdf` on `page`.
fn annotation_at(tab: &mut DocTab, page: usize, pdf: PointF, tolerance: f32) -> Option<usize> {
    tab.annotations(page)
        .iter()
        .rev()
        .find(|a| a.kind.is_comment() && a.rect.inflate(tolerance).contains(pdf))
        .map(|a| a.index)
}

fn snap_45(start: PointF, end: PointF) -> PointF {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let len = (dx * dx + dy * dy).sqrt();
    let angle = dy.atan2(dx);
    let step = std::f32::consts::FRAC_PI_4;
    let snapped = (angle / step).round() * step;
    PointF::new(start.x + len * snapped.cos(), start.y + len * snapped.sin())
}

fn handle_input(
    ui: &mut Ui,
    tab: &mut DocTab,
    cx: &mut CanvasCtx,
    response: &egui::Response,
    on_screen: &[PageOnScreen],
) {
    let hover = response.hover_pos();
    let shift_down = ui.input(|i| i.modifiers.shift);
    let tol = 4.0 / tab.scale();

    // Cursor feedback.
    if let Some(h) = hover
        && let Some(pos) = find_page(on_screen, h)
    {
        let pdf = pos.to_pdf(h);
        let cursor = match cx.tool {
            Tool::Hand => {
                let over_link = tab.links(pos.page).iter().any(|l| l.rect.contains(pdf));
                let over_field = tab.fields(pos.page).iter().any(|f| !f.read_only && f.rect.contains(pdf));
                let over_annot = annotation_at(tab, pos.page, pdf, tol).is_some();
                if matches!(tab.interaction, Interaction::Pan) {
                    CursorIcon::Grabbing
                } else if over_link || over_field {
                    CursorIcon::PointingHand
                } else if over_annot {
                    CursorIcon::Move
                } else {
                    CursorIcon::Grab
                }
            }
            Tool::SelectText => CursorIcon::Text,
            t if t.is_markup() => CursorIcon::Text,
            _ => CursorIcon::Crosshair,
        };
        ui.ctx().set_cursor_icon(cursor);
    }

    // Middle mouse button always pans.
    if response.dragged_by(PointerButton::Middle) {
        let offset = tab.scroll_offset - response.drag_delta();
        tab.pending_scroll = Some(ScrollRequest::Offset(offset.max(Vec2::ZERO)));
        return;
    }

    let press = ui.input(|i| i.pointer.press_origin());

    if response.drag_started_by(PointerButton::Primary)
        && let Some(start) = press
    {
        let page = find_page(on_screen, start);
        tab.interaction = match (cx.tool, page) {
            (Tool::Hand, Some(pos)) => {
                let pdf = pos.to_pdf(start);
                let selected = tab.selected_annot;
                match selected {
                    Some(sel)
                        if sel.page == pos.page
                            && annotation_at(tab, pos.page, pdf, tol) == Some(sel.index)
                            && tab
                                .annotations(pos.page)
                                .get(sel.index)
                                .is_some_and(|a| a.kind.is_movable()) =>
                    {
                        Interaction::MoveAnnot { annot: sel, delta: Vec2::ZERO }
                    }
                    _ => Interaction::Pan,
                }
            }
            (Tool::Hand, None) => Interaction::Pan,
            (Tool::SelectText, Some(pos)) => {
                let pdf = pos.to_pdf(start);
                match tab.page_text(pos.page).and_then(|t| t.hit_test(pdf, 10.0 / tab.scale())) {
                    Some(anchor) => {
                        tab.selection = Some(TextSelection {
                            page: pos.page,
                            range: TextRange::new(anchor, anchor),
                        });
                        Interaction::SelectText { page: pos.page, anchor }
                    }
                    None => {
                        tab.selection = None;
                        Interaction::Pan
                    }
                }
            }
            (t, Some(pos)) if t.is_markup() => {
                let pdf = pos.to_pdf(start);
                let anchor = tab.page_text(pos.page).and_then(|t| t.hit_test(pdf, 10.0 / tab.scale()));
                let v = pos.to_view(start);
                Interaction::Markup { page: pos.page, anchor, start: v, current: v }
            }
            (t, Some(pos)) if t.is_shape() => {
                let v = pos.to_view(start);
                Interaction::Shape { page: pos.page, start: v, current: v, points: vec![v] }
            }
            _ => Interaction::None,
        };
    }

    if response.dragged_by(PointerButton::Primary)
        && let Some(cur) = response.interact_pointer_pos()
    {
        let delta = response.drag_delta();
        let scale = tab.scale();
        let mut interaction = std::mem::replace(&mut tab.interaction, Interaction::None);
        match &mut interaction {
            Interaction::Pan => {
                let offset = tab.scroll_offset - delta;
                tab.pending_scroll = Some(ScrollRequest::Offset(offset.max(Vec2::ZERO)));
            }
            Interaction::SelectText { page, anchor } => {
                if let Some(pos) = on_screen.iter().find(|p| p.page == *page) {
                    let pdf = pos.to_pdf(cur);
                    if let Some(c) = tab.page_text(*page).and_then(|t| t.hit_test(pdf, 40.0 / scale)) {
                        tab.selection = Some(TextSelection { page: *page, range: TextRange::new(*anchor, c) });
                    }
                }
            }
            Interaction::Markup { page, current, .. } => {
                if let Some(pos) = on_screen.iter().find(|p| p.page == *page) {
                    let clamped = cur.clamp(pos.rect.min, pos.rect.max);
                    *current = pos.to_view(clamped);
                }
            }
            Interaction::Shape { page, start, current, points } => {
                if let Some(pos) = on_screen.iter().find(|p| p.page == *page) {
                    let clamped = cur.clamp(pos.rect.min, pos.rect.max);
                    let mut v = pos.to_view(clamped);
                    if shift_down && matches!(cx.tool, Tool::Line | Tool::Arrow) {
                        v = snap_45(*start, v);
                    }
                    *current = v;
                    if cx.tool == Tool::Pencil
                        && points.last().is_none_or(|l| l.distance(v) * scale > 1.5)
                    {
                        points.push(v);
                    }
                }
            }
            Interaction::MoveAnnot { delta: d, .. } => {
                *d += delta;
            }
            Interaction::None => {}
        }
        tab.interaction = interaction;
    }

    if response.drag_stopped() {
        let interaction = std::mem::replace(&mut tab.interaction, Interaction::None);
        finish_drag(tab, cx, interaction, on_screen);
    }

    if response.clicked()
        && let Some(p) = response.interact_pointer_pos()
    {
        handle_click(tab, cx, on_screen, p);
    }

    if response.double_clicked()
        && let Some(p) = response.interact_pointer_pos()
        && let Some(pos) = find_page(on_screen, p)
    {
        let pdf = pos.to_pdf(p);
        match cx.tool {
            Tool::SelectText => {
                if let Some(text) = tab.page_text(pos.page)
                    && let Some(i) = text.hit_test(pdf, 4.0 / tab.scale())
                    && let Some(range) = text.word_at(i)
                {
                    tab.selection = Some(TextSelection { page: pos.page, range });
                }
            }
            Tool::Hand => {
                if let Some(index) = annotation_at(tab, pos.page, pdf, tol) {
                    let kind = tab.annotations(pos.page).get(index).map(|a| a.kind);
                    let annot = AnnotRef { page: pos.page, index };
                    tab.selected_annot = Some(annot);
                    if matches!(kind, Some(AnnotKind::FreeText | AnnotKind::Note)) {
                        cx.events.push(CanvasEvent::EditText(annot));
                    } else {
                        cx.events.push(CanvasEvent::ShowProperties);
                    }
                }
            }
            _ => {}
        }
    }

    if response.secondary_clicked()
        && let Some(p) = response.interact_pointer_pos()
        && let Some(pos) = find_page(on_screen, p)
    {
        let pdf = pos.to_pdf(p);
        if let Some(index) = annotation_at(tab, pos.page, pdf, tol) {
            tab.selected_annot = Some(AnnotRef { page: pos.page, index });
        }
    }

    let has_selection = tab.selection.is_some();
    let has_annot = tab.selected_annot.is_some();
    let t = cx.t;
    response.context_menu(|ui| {
        if has_selection {
            if ui.button(t.copy).clicked() {
                cx.events.push(CanvasEvent::CopySelection);
                ui.close();
            }
            ui.separator();
            for (tool, label) in [
                (Tool::Highlight, t.tool_highlight),
                (Tool::Underline, t.tool_underline),
                (Tool::StrikeOut, t.tool_strikeout),
                (Tool::Squiggly, t.tool_squiggly),
            ] {
                if ui.button(label).clicked() {
                    cx.events.push(CanvasEvent::MarkupSelection(tool));
                    ui.close();
                }
            }
        }
        if has_annot {
            if has_selection {
                ui.separator();
            }
            if ui.button(t.properties).clicked() {
                cx.events.push(CanvasEvent::ShowProperties);
                ui.close();
            }
            if ui.button(t.delete).clicked() {
                cx.events.push(CanvasEvent::DeleteSelected);
                ui.close();
            }
        }
        if !has_selection && !has_annot {
            ui.label(t.no_selection);
        }
    });

}

fn finish_drag(tab: &mut DocTab, cx: &mut CanvasCtx, interaction: Interaction, on_screen: &[PageOnScreen]) {
    let scale = tab.scale();
    match interaction {
        Interaction::Markup { page, anchor, start, current } => {
            let Some(pos) = on_screen.iter().find(|p| p.page == page) else {
                return;
            };
            let end_char = tab
                .page_text(page)
                .and_then(|t| t.hit_test(pos.geo.view_to_pdf(current), 8.0));
            let quads = match (anchor, end_char, tab.page_text(page)) {
                (Some(a), Some(b), Some(text)) => text.line_rects(TextRange::new(a, b)),
                _ if cx.tool == Tool::Highlight && start.distance(current) * scale > 4.0 => {
                    vec![pos.geo.view_rect_to_pdf(&RectF::from_points(start, current))]
                }
                _ => Vec::new(),
            };
            if quads.is_empty() {
                cx.events.push(CanvasEvent::Status(cx.t.no_text_here.into()));
                return;
            }
            if let Some(kind) = cx.tool.annot_kind() {
                let mut annot = NewAnnotation::new(kind, cx.style.clone());
                annot.quads = quads;
                annot.contents = match (anchor, end_char, tab.page_text(page)) {
                    (Some(a), Some(b), Some(text)) => text.text_in(TextRange::new(a, b)),
                    _ => String::new(),
                };
                cx.events.push(CanvasEvent::Create { page, annot: Box::new(annot) });
            }
        }
        Interaction::Shape { page, start, current, points } => {
            let Some(pos) = on_screen.iter().find(|p| p.page == page) else {
                return;
            };
            let geo = pos.geo;
            let big_enough = start.distance(current) * scale > 3.0;
            let mut annot = NewAnnotation::new(cx.tool.annot_kind().unwrap_or(AnnotKind::Square), cx.style.clone());
            match cx.tool {
                Tool::Rectangle | Tool::Ellipse if big_enough => {
                    annot.rect = geo.view_rect_to_pdf(&RectF::from_points(start, current));
                }
                Tool::TextBox => {
                    let rect = if big_enough {
                        RectF::from_points(start, current)
                    } else {
                        RectF::new(start.x, start.y, start.x + 180.0, start.y + 40.0)
                    };
                    cx.events.push(CanvasEvent::RequestText {
                        page,
                        rect: geo.view_rect_to_pdf(&rect),
                        kind: AnnotKind::FreeText,
                    });
                    return;
                }
                Tool::Line | Tool::Arrow if big_enough => {
                    annot.paths = vec![vec![geo.view_to_pdf(start), geo.view_to_pdf(current)]];
                    annot.arrow_end = cx.tool == Tool::Arrow;
                }
                Tool::Pencil if points.len() >= 2 => {
                    annot.paths = vec![points.iter().map(|p| geo.view_to_pdf(*p)).collect()];
                }
                _ => return,
            }
            cx.events.push(CanvasEvent::Create { page, annot: Box::new(annot) });
        }
        Interaction::MoveAnnot { annot, delta } => {
            if delta.length() < 1.0 {
                return;
            }
            if let Some(pos) = on_screen.iter().find(|p| p.page == annot.page) {
                let (dx, dy) = pos.geo.view_delta_to_pdf(delta.x / scale, delta.y / scale);
                cx.events.push(CanvasEvent::Move { annot, dx, dy });
            }
        }
        _ => {}
    }
}

fn handle_click(tab: &mut DocTab, cx: &mut CanvasCtx, on_screen: &[PageOnScreen], p: Pos2) {
    let Some(pos) = find_page(on_screen, p) else {
        tab.selected_annot = None;
        tab.selection = None;
        return;
    };
    let pdf = pos.to_pdf(p);
    let view = pos.to_view(p);
    let tol = 4.0 / tab.scale();
    match cx.tool {
        Tool::Hand => {
            if let Some(field) = tab.fields(pos.page).iter().rev().find(|f| f.rect.contains(pdf)).cloned()
                && !field.read_only
            {
                match field.kind {
                    FieldKind::Text { multiline, password } => {
                        tab.field_edit = Some(FieldEdit {
                            page: field.page,
                            annot_index: field.annot_index,
                            value: field.value.clone(),
                            multiline,
                            password,
                            rect: field.rect,
                            focus_requested: false,
                        });
                    }
                    FieldKind::CheckBox => cx.events.push(CanvasEvent::SetChecked {
                        page: field.page,
                        annot_index: field.annot_index,
                        checked: !field.checked,
                    }),
                    FieldKind::RadioButton => cx.events.push(CanvasEvent::SetChecked {
                        page: field.page,
                        annot_index: field.annot_index,
                        checked: true,
                    }),
                    FieldKind::ComboBox | FieldKind::ListBox if !field.options.is_empty() => {
                        cx.events.push(CanvasEvent::ChooseOption {
                            page: field.page,
                            annot_index: field.annot_index,
                            options: field.options.clone(),
                            current: field.value.clone(),
                        })
                    }
                    _ => cx.events.push(CanvasEvent::Status(cx.t.field_not_supported.into())),
                }
                return;
            }
            if let Some(link) = tab.links(pos.page).iter().find(|l| l.rect.contains(pdf)).cloned() {
                match link.target {
                    LinkTarget::Page(target) => tab.go_to_page(target),
                    LinkTarget::Uri(uri) => cx.events.push(CanvasEvent::OpenUri(uri)),
                    LinkTarget::Unsupported => {
                        cx.events.push(CanvasEvent::Status(cx.t.link_not_supported.into()))
                    }
                }
                return;
            }
            tab.selected_annot = annotation_at(tab, pos.page, pdf, tol).map(|index| AnnotRef { page: pos.page, index });
        }
        Tool::SelectText => {
            tab.selection = None;
        }
        Tool::Note => {
            let rect = RectF::new(view.x, view.y, view.x + 22.0, view.y + 22.0);
            cx.events.push(CanvasEvent::RequestText {
                page: pos.page,
                rect: pos.geo.view_rect_to_pdf(&rect),
                kind: AnnotKind::Note,
            });
        }
        Tool::TextBox => {
            let rect = RectF::new(view.x, view.y, view.x + 180.0, view.y + 40.0);
            cx.events.push(CanvasEvent::RequestText {
                page: pos.page,
                rect: pos.geo.view_rect_to_pdf(&rect),
                kind: AnnotKind::FreeText,
            });
        }
        Tool::Rectangle | Tool::Ellipse => {
            let rect = RectF::new(view.x, view.y, view.x + 100.0, view.y + 60.0);
            let mut annot = NewAnnotation::new(cx.tool.annot_kind().unwrap_or(AnnotKind::Square), cx.style.clone());
            annot.rect = pos.geo.view_rect_to_pdf(&rect);
            cx.events.push(CanvasEvent::Create { page: pos.page, annot: Box::new(annot) });
        }
        Tool::Stamp => {
            let label = win_ansi(&cx.stamp_label.to_uppercase());
            let w = (StdFont::HelveticaBold.text_width(&label, 20.0) + 28.0).max(80.0);
            let h = 36.0;
            let rect = RectF::new(view.x - w / 2.0, view.y - h / 2.0, view.x + w / 2.0, view.y + h / 2.0);
            let mut annot = NewAnnotation::new(AnnotKind::Stamp, cx.style.clone());
            annot.rect = pos.geo.view_rect_to_pdf(&rect);
            annot.label = cx.stamp_label.clone();
            annot.icon = Some(cx.stamp_icon.to_string());
            cx.events.push(CanvasEvent::Create { page: pos.page, annot: Box::new(annot) });
        }
        _ => {}
    }
}

fn draw_field_editor(ui: &mut Ui, tab: &mut DocTab, on_screen: &[PageOnScreen], cx: &mut CanvasCtx) {
    let Some(edit) = tab.field_edit.as_mut() else {
        return;
    };
    let Some(pos) = on_screen.iter().find(|p| p.page == edit.page) else {
        return;
    };
    let rect = pos.pdf_rect_to_screen(&edit.rect);
    let font_size = if edit.multiline {
        (12.0 * pos.scale).clamp(8.0, 48.0)
    } else {
        (rect.height() * 0.65).clamp(8.0, 48.0)
    };
    let mut text_edit = if edit.multiline {
        egui::TextEdit::multiline(&mut edit.value)
    } else {
        egui::TextEdit::singleline(&mut edit.value)
    }
    .font(egui::FontId::proportional(font_size))
    .password(edit.password)
    .desired_width(rect.width());
    if edit.multiline {
        text_edit = text_edit.desired_rows(((rect.height() / (font_size * 1.3)) as usize).max(1));
    }
    let response = ui.put(rect, text_edit);
    if !edit.focus_requested {
        response.request_focus();
        edit.focus_requested = true;
    }
    let cancel = ui.input(|i| i.key_pressed(Key::Escape));
    if cancel {
        tab.field_edit = None;
        return;
    }
    let enter = !edit.multiline && ui.input(|i| i.key_pressed(Key::Enter));
    if response.lost_focus() || enter {
        let edit = tab.field_edit.take().expect("field edit present");
        cx.events.push(CanvasEvent::CommitField {
            page: edit.page,
            annot_index: edit.annot_index,
            value: edit.value,
        });
    }
}
