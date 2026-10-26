//! The document view: draws pages and handles the interactive tools.

use egui::{
    Color32, CursorIcon, Key, PointerButton, Pos2, Rect, ScrollArea, Sense, Stroke, StrokeKind, Ui, Vec2,
    pos2, vec2,
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
use crate::ui::tools::{Gesture, Tool};
use crate::ui::view::{
    Layout, MARGIN, MAX_ZOOM, MIN_ZOOM, PT_TO_SCREEN, PageLayout, ZoomMode, fit_page_zoom, fit_width_zoom,
    zoom_in, zoom_out,
};

/// Requests from the canvas that the application has to carry out.
#[derive(Debug, Clone, PartialEq)]
pub enum CanvasEvent {
    Create {
        page: usize,
        annot: Box<NewAnnotation>,
    },
    /// Ask for a text and then create `template` with it as contents.
    RequestText {
        page: usize,
        tool: Tool,
        template: Box<NewAnnotation>,
    },
    EditText(AnnotRef),
    Move {
        annot: AnnotRef,
        dx: f32,
        dy: f32,
    },
    OpenUri(String),
    CommitField {
        page: usize,
        annot_index: usize,
        value: String,
    },
    SetChecked {
        page: usize,
        annot_index: usize,
        checked: bool,
    },
    ChooseOption {
        page: usize,
        annot_index: usize,
        options: Vec<String>,
        current: String,
    },
    MarkupSelection(Tool),
    CopySelection,
    DeleteSelected,
    ShowProperties,
    Status(String),
    /// Copy the given area (PDF space) as image to the clipboard.
    Snapshot {
        page: usize,
        rect: RectF,
    },
    /// Delete the given annotations (eraser).
    Erase(Vec<AnnotRef>),
    CreateLink {
        page: usize,
        rect: RectF,
    },
    CreateField {
        page: usize,
        rect: RectF,
        tool: Tool,
    },
    AttachFile {
        page: usize,
        rect: RectF,
    },
    /// The crop tool drew a rectangle.
    CropPage {
        page: usize,
        rect: RectF,
    },
}

pub struct CanvasCtx<'a> {
    pub tool: Tool,
    pub style: AnnotStyle,
    pub stamp_label: String,
    pub stamp_icon: &'static str,
    pub highlight_fields: bool,
    /// Use a decimal comma in measurement labels.
    pub decimal_comma: bool,
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
    fn view_at(&self, p: Pos2) -> PointF {
        PointF::new((p.x - self.rect.min.x) / self.scale, (p.y - self.rect.min.y) / self.scale)
    }

    fn pdf_at(&self, p: Pos2) -> PointF {
        self.geo.view_to_pdf(self.view_at(p))
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

    fn view_rect_to_pdf(&self, a: PointF, b: PointF) -> RectF {
        self.geo.view_rect_to_pdf(&RectF::from_points(a, b))
    }
}

const PT_TO_MM: f32 = 25.4 / 72.0;

/// Formats a measurement (`value` in mm or mm²).
pub fn format_measure(value: f32, area: bool, decimal_comma: bool) -> String {
    let s = if area {
        if value >= 10_000.0 { format!("{:.2} cm²", value / 100.0) } else { format!("{value:.1} mm²") }
    } else if value >= 1000.0 {
        format!("{:.1} cm", value / 10.0)
    } else {
        format!("{value:.1} mm")
    };
    if decimal_comma { s.replace('.', ",") } else { s }
}

fn path_length(points: &[PointF]) -> f32 {
    points.windows(2).map(|w| w[0].distance(w[1])).sum()
}

fn polygon_area(points: &[PointF]) -> f32 {
    let n = points.len();
    (0..n)
        .map(|i| points[i].x * points[(i + 1) % n].y - points[(i + 1) % n].x * points[i].y)
        .sum::<f32>()
        .abs()
        / 2.0
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
    let facing = matches!(tab.layout_mode, PageLayout::Facing | PageLayout::Cover);
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

    let mut area = ScrollArea::both().id_salt(("doc", tab.id)).auto_shrink([false, false]);
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

    // Single page layout: the mouse wheel flips pages at the top/bottom edge.
    if tab.layout_mode == PageLayout::Single
        && let Some(pointer) = ui.input(|i| i.pointer.hover_pos())
        && output.inner_rect.contains(pointer)
    {
        let (dy, now) = ui.input(|i| (i.smooth_scroll_delta.y, i.time));
        let max_y = (output.content_size.y - output.inner_rect.height()).max(0.0);
        let count = tab.doc.page_count();
        if now - tab.last_flip > 0.35 {
            if dy < 0.0 && output.state.offset.y >= max_y - 1.0 && tab.current_page + 1 < count {
                tab.go_to_page(tab.current_page + 1);
                tab.last_flip = now;
            } else if dy > 0.0 && output.state.offset.y <= 1.0 && tab.current_page > 0 {
                tab.go_to_page(tab.current_page - 1);
                tab.last_flip = now;
            }
        }
    }
}

fn draw_and_interact(ui: &mut Ui, tab: &mut DocTab, cx: &mut CanvasCtx, viewport: Rect, ctx: &egui::Context) {
    let layout_size = tab.layout.size;
    let size = vec2(layout_size.x.max(viewport.width()), layout_size.y.max(viewport.height()));
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let shift = vec2(
        ((size.x - layout_size.x) / 2.0).max(0.0),
        if tab.layout_mode == PageLayout::Single { ((size.y - layout_size.y) / 2.0).max(0.0) } else { 0.0 },
    );
    tab.content_shift = shift;
    tab.viewport = viewport;
    let origin = rect.min + shift;
    tab.canvas_origin = origin;
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
    let opts = tab.render_options();
    let mut on_screen: Vec<PageOnScreen> = Vec::new();

    let slots = tab.layout.slots.clone();
    for slot in &slots {
        let screen = slot.rect.translate(origin.to_vec2());
        let Some(geo) = tab.display_geometry(slot.page) else {
            continue;
        };
        let pos = PageOnScreen { page: slot.page, rect: screen, geo, scale };
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
        if let Some(img) = tab.textures.page(ctx, &tab.doc, slot.page, base_px, cx.budget, tab.id, opts) {
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
                if let Some(tile) = tab.textures.tile(
                    ctx,
                    &tab.doc,
                    slot.page,
                    [px_w, px_h],
                    [ax, ay, aw, ah],
                    cx.budget,
                    tab.id,
                    opts,
                ) {
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
        draw_overlays(&painter, tab, &pos, cx, response.hover_pos());
    }

    handle_input(ui, tab, cx, &response, &on_screen);
    draw_field_editor(ui, tab, &on_screen, cx);
    tab.textures.evict();
}

fn style_stroke(cx: &CanvasCtx, scale: f32) -> Stroke {
    let c = cx.style.color;
    Stroke::new((cx.style.width * scale).max(1.0), Color32::from_rgb(c.r, c.g, c.b))
}

fn draw_overlays(
    painter: &egui::Painter,
    tab: &mut DocTab,
    pos: &PageOnScreen,
    cx: &CanvasCtx,
    hover: Option<Pos2>,
) {
    let page = pos.page;
    if cx.highlight_fields || cx.tool == Tool::SelectFields || cx.tool.is_field() {
        for f in tab.fields(page).iter() {
            if !f.read_only && f.kind != FieldKind::PushButton {
                painter.rect_filled(
                    pos.pdf_rect_to_screen(&f.rect),
                    0.0,
                    Color32::from_rgba_unmultiplied(120, 160, 255, 50),
                );
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
        Interaction::Markup { page: p, anchor: Some(a), current, .. } if *p == page => tab
            .page_text(page)
            .and_then(|t| t.hit_test(pos.geo.view_to_pdf(*current), 8.0).map(|c| TextRange::new(*a, c))),
        _ => tab.selection.filter(|s| s.page == page).map(|s| s.range),
    };
    if let (Some(range), Some(text)) = (selection, tab.page_text(page)) {
        for r in text.line_rects(range) {
            painter.rect_filled(
                pos.pdf_rect_to_screen(&r),
                0.0,
                Color32::from_rgba_unmultiplied(40, 110, 230, 70),
            );
        }
    }
    // Selected annotation or form field.
    if let Some(sel) = tab.selected_annot.filter(|s| s.page == page)
        && let Some(a) = tab.annotations(page).get(sel.index)
    {
        let mut r = pos.pdf_rect_to_screen(&a.rect).expand(2.0);
        if let Interaction::MoveAnnot { annot, delta } = &interaction
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
    // Eraser hits.
    if let Interaction::Erase { page: p, hits } = &interaction
        && *p == page
    {
        let annots = tab.annotations(page);
        for i in hits {
            if let Some(a) = annots.get(*i) {
                painter.rect_stroke(
                    pos.pdf_rect_to_screen(&a.rect),
                    0.0,
                    Stroke::new(2.0, Color32::RED),
                    StrokeKind::Outside,
                );
            }
        }
    }
    // Shape preview.
    if let Interaction::Shape { page: p, start, current, points } = &interaction
        && *p == page
    {
        let stroke = style_stroke(cx, pos.scale);
        let a = pos.view_to_screen(*start);
        let b = pos.view_to_screen(*current);
        let thin = Stroke::new(1.0, theme::ACCENT);
        match cx.tool {
            Tool::Ellipse => {
                let r = Rect::from_two_pos(a, b);
                painter.add(egui::Shape::ellipse_stroke(r.center(), r.size() / 2.0, stroke));
            }
            Tool::Line | Tool::Arrow | Tool::MeasureDistance => {
                painter.line_segment([a, b], stroke);
                if cx.tool == Tool::MeasureDistance {
                    let d = pos.geo.view_to_pdf(*start).distance(pos.geo.view_to_pdf(*current)) * PT_TO_MM;
                    painter.text(
                        b + vec2(8.0, -8.0),
                        egui::Align2::LEFT_BOTTOM,
                        format_measure(d, false, cx.decimal_comma),
                        egui::FontId::proportional(13.0),
                        stroke.color,
                    );
                }
            }
            Tool::Callout => {
                painter.line_segment([a, b], stroke);
                let r = callout_box(*start, *current);
                painter.rect_stroke(
                    Rect::from_min_max(
                        pos.view_to_screen(PointF::new(r.x0, r.y0)),
                        pos.view_to_screen(PointF::new(r.x1, r.y1)),
                    ),
                    0.0,
                    stroke,
                    StrokeKind::Middle,
                );
            }
            Tool::Pencil | Tool::Signature => {
                let pts: Vec<Pos2> = points.iter().map(|p| pos.view_to_screen(*p)).collect();
                painter.add(egui::Shape::line(pts, stroke));
            }
            Tool::Zoom | Tool::Snapshot | Tool::Link => {
                painter.rect_stroke(Rect::from_two_pos(a, b), 0.0, thin, StrokeKind::Middle);
            }
            Tool::AreaHighlight => {
                painter.rect_filled(
                    Rect::from_two_pos(a, b),
                    0.0,
                    Color32::from_rgba_unmultiplied(255, 230, 0, 90),
                );
            }
            _ => {
                painter.rect_stroke(Rect::from_two_pos(a, b), 0.0, stroke, StrokeKind::Middle);
            }
        }
    }
    // Polygon in progress.
    if !tab.polygon.is_empty() && tab.polygon_page == page {
        let stroke = style_stroke(cx, pos.scale);
        let mut pts: Vec<Pos2> = tab.polygon.iter().map(|p| pos.view_to_screen(*p)).collect();
        if let Some(h) = hover.filter(|h| pos.rect.contains(*h)) {
            pts.push(h);
        }
        painter.add(egui::Shape::line(pts.clone(), stroke));
        for p in &pts {
            painter.circle_filled(*p, 3.0, theme::ACCENT);
        }
    }
}

/// Text box of a callout whose leader goes from `tip` to `knee` (view space).
fn callout_box(tip: PointF, knee: PointF) -> RectF {
    let (w, h) = (150.0, 40.0);
    if knee.x >= tip.x {
        RectF::new(knee.x, knee.y - h / 2.0, knee.x + w, knee.y + h / 2.0)
    } else {
        RectF::new(knee.x - w, knee.y - h / 2.0, knee.x, knee.y + h / 2.0)
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

/// Topmost form field widget at `pdf` on `page`.
fn widget_at(tab: &mut DocTab, page: usize, pdf: PointF) -> Option<usize> {
    tab.annotations(page)
        .iter()
        .rev()
        .find(|a| a.kind == AnnotKind::Widget && a.rect.contains(pdf))
        .map(|a| a.index)
}

/// Topmost ink annotation at `pdf` (for the eraser).
fn ink_at(tab: &mut DocTab, page: usize, pdf: PointF, tolerance: f32) -> Option<usize> {
    tab.annotations(page)
        .iter()
        .rev()
        .find(|a| a.kind == AnnotKind::Ink && a.rect.inflate(tolerance).contains(pdf))
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

fn cursor_for(tab: &mut DocTab, cx: &CanvasCtx, pos: &PageOnScreen, pdf: PointF) -> CursorIcon {
    let tol = 4.0 / tab.scale();
    match cx.tool {
        Tool::Hand => {
            let over_link = tab.links(pos.page).iter().any(|l| l.rect.contains(pdf));
            let over_field = tab.fields(pos.page).iter().any(|f| !f.read_only && f.rect.contains(pdf));
            if matches!(tab.interaction, Interaction::Pan) {
                CursorIcon::Grabbing
            } else if over_link || over_field {
                CursorIcon::PointingHand
            } else if annotation_at(tab, pos.page, pdf, tol).is_some() {
                CursorIcon::Move
            } else {
                CursorIcon::Grab
            }
        }
        Tool::SelectAnnot => {
            if annotation_at(tab, pos.page, pdf, tol).is_some() {
                CursorIcon::Move
            } else {
                CursorIcon::Default
            }
        }
        Tool::SelectFields => {
            if widget_at(tab, pos.page, pdf).is_some() {
                CursorIcon::Move
            } else {
                CursorIcon::Default
            }
        }
        Tool::SelectText => CursorIcon::Text,
        Tool::Zoom => CursorIcon::ZoomIn,
        t if t.is_markup() => CursorIcon::Text,
        _ => CursorIcon::Crosshair,
    }
}

fn handle_input(
    ui: &mut Ui,
    tab: &mut DocTab,
    cx: &mut CanvasCtx,
    response: &egui::Response,
    on_screen: &[PageOnScreen],
) {
    let shift_down = ui.input(|i| i.modifiers.shift);
    let tol = 4.0 / tab.scale();

    if let Some(h) = response.hover_pos()
        && let Some(pos) = find_page(on_screen, h)
    {
        let cursor = cursor_for(tab, cx, &pos, pos.pdf_at(h));
        ui.ctx().set_cursor_icon(cursor);
    }

    // Polygon tools: Enter finishes, Escape cancels.
    if !tab.polygon.is_empty() {
        if ui.input(|i| i.key_pressed(Key::Enter)) {
            finish_polygon(tab, cx, on_screen);
        } else if ui.input(|i| i.key_pressed(Key::Escape)) {
            tab.polygon.clear();
        }
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
        tab.interaction = start_drag(tab, cx, find_page(on_screen, start), start, tol);
    }

    if response.dragged_by(PointerButton::Primary)
        && let Some(cur) = response.interact_pointer_pos()
    {
        continue_drag(tab, cx, on_screen, cur, response.drag_delta(), shift_down, tol);
    }

    if response.drag_stopped() {
        let interaction = std::mem::replace(&mut tab.interaction, Interaction::None);
        finish_drag(tab, cx, interaction, on_screen);
    }

    if response.clicked()
        && let Some(p) = response.interact_pointer_pos()
    {
        handle_click(tab, cx, on_screen, p, shift_down);
    }

    if response.double_clicked()
        && let Some(p) = response.interact_pointer_pos()
        && let Some(pos) = find_page(on_screen, p)
    {
        handle_double_click(tab, cx, &pos, p, on_screen, tol);
    }

    if response.secondary_clicked()
        && let Some(p) = response.interact_pointer_pos()
        && let Some(pos) = find_page(on_screen, p)
    {
        if cx.tool == Tool::Zoom {
            tab.zoom = zoom_out(tab.zoom);
            tab.zoom_mode = ZoomMode::Custom;
            tab.pending_scroll = Some(ScrollRequest::Point(pos.page, pos.view_at(p)));
        } else if let Some(index) = annotation_at(tab, pos.page, pos.pdf_at(p), tol) {
            tab.selected_annot = Some(AnnotRef { page: pos.page, index });
        }
    }

    context_menu(tab, cx, response);
}

fn start_drag(
    tab: &mut DocTab,
    cx: &CanvasCtx,
    page: Option<PageOnScreen>,
    start: Pos2,
    tol: f32,
) -> Interaction {
    let Some(pos) = page else {
        return if cx.tool == Tool::Hand { Interaction::Pan } else { Interaction::None };
    };
    let pdf = pos.pdf_at(start);
    let v = pos.view_at(start);
    match cx.tool {
        Tool::Hand | Tool::SelectAnnot | Tool::SelectFields => {
            let hit = if cx.tool == Tool::SelectFields {
                widget_at(tab, pos.page, pdf)
            } else {
                annotation_at(tab, pos.page, pdf, tol)
            };
            let movable = |tab: &mut DocTab, i: usize| {
                cx.tool == Tool::SelectFields
                    || tab.annotations(pos.page).get(i).is_some_and(|a| a.kind.is_movable())
            };
            match hit {
                Some(i)
                    if movable(tab, i)
                        && (cx.tool != Tool::Hand
                            || tab.selected_annot == Some(AnnotRef { page: pos.page, index: i })) =>
                {
                    let annot = AnnotRef { page: pos.page, index: i };
                    tab.selected_annot = Some(annot);
                    Interaction::MoveAnnot { annot, delta: Vec2::ZERO }
                }
                _ if cx.tool == Tool::Hand => Interaction::Pan,
                _ => Interaction::None,
            }
        }
        Tool::SelectText => match tab.page_text(pos.page).and_then(|t| t.hit_test(pdf, 10.0 / tab.scale())) {
            Some(anchor) => {
                tab.selection = Some(TextSelection { page: pos.page, range: TextRange::new(anchor, anchor) });
                Interaction::SelectText { page: pos.page, anchor }
            }
            None => {
                tab.selection = None;
                Interaction::Pan
            }
        },
        Tool::Eraser => {
            Interaction::Erase { page: pos.page, hits: ink_at(tab, pos.page, pdf, tol).into_iter().collect() }
        }
        t => match t.gesture() {
            Gesture::TextMarkup => {
                let anchor = tab.page_text(pos.page).and_then(|t| t.hit_test(pdf, 10.0 / tab.scale()));
                Interaction::Markup { page: pos.page, anchor, start: v, current: v }
            }
            Gesture::Rect | Gesture::Line | Gesture::Freehand => {
                Interaction::Shape { page: pos.page, start: v, current: v, points: vec![v] }
            }
            _ => Interaction::None,
        },
    }
}

fn continue_drag(
    tab: &mut DocTab,
    cx: &CanvasCtx,
    on_screen: &[PageOnScreen],
    cur: Pos2,
    delta: Vec2,
    shift_down: bool,
    tol: f32,
) {
    let scale = tab.scale();
    let mut interaction = std::mem::replace(&mut tab.interaction, Interaction::None);
    match &mut interaction {
        Interaction::Pan => {
            let offset = tab.scroll_offset - delta;
            tab.pending_scroll = Some(ScrollRequest::Offset(offset.max(Vec2::ZERO)));
        }
        Interaction::SelectText { page, anchor } => {
            if let Some(pos) = on_screen.iter().find(|p| p.page == *page) {
                let pdf = pos.pdf_at(cur);
                if let Some(c) = tab.page_text(*page).and_then(|t| t.hit_test(pdf, 40.0 / scale)) {
                    tab.selection = Some(TextSelection { page: *page, range: TextRange::new(*anchor, c) });
                }
            }
        }
        Interaction::Markup { page, current, .. } => {
            if let Some(pos) = on_screen.iter().find(|p| p.page == *page) {
                *current = pos.view_at(cur.clamp(pos.rect.min, pos.rect.max));
            }
        }
        Interaction::Shape { page, start, current, points } => {
            if let Some(pos) = on_screen.iter().find(|p| p.page == *page) {
                let mut v = pos.view_at(cur.clamp(pos.rect.min, pos.rect.max));
                if shift_down && cx.tool.gesture() == Gesture::Line {
                    v = snap_45(*start, v);
                }
                *current = v;
                if cx.tool.gesture() == Gesture::Freehand
                    && points.last().is_none_or(|l| l.distance(v) * scale > 1.5)
                {
                    points.push(v);
                }
            }
        }
        Interaction::Erase { page, hits } => {
            if let Some(pos) = on_screen.iter().find(|p| p.page == *page)
                && let Some(i) = ink_at(tab, *page, pos.pdf_at(cur), tol)
                && !hits.contains(&i)
            {
                hits.push(i);
            }
        }
        Interaction::MoveAnnot { delta: d, .. } => *d += delta,
        Interaction::None => {}
    }
    tab.interaction = interaction;
}

fn new_annot(cx: &CanvasCtx, kind: AnnotKind) -> NewAnnotation {
    NewAnnotation::new(kind, cx.style.clone())
}

fn finish_drag(tab: &mut DocTab, cx: &mut CanvasCtx, interaction: Interaction, on_screen: &[PageOnScreen]) {
    let scale = tab.scale();
    match interaction {
        Interaction::Markup { page, anchor, start, current } => {
            let Some(pos) = on_screen.iter().find(|p| p.page == page) else { return };
            let end_char = tab.page_text(page).and_then(|t| t.hit_test(pos.geo.view_to_pdf(current), 8.0));
            let (quads, text) = match (anchor, end_char, tab.page_text(page)) {
                (Some(a), Some(b), Some(text)) => {
                    (text.line_rects(TextRange::new(a, b)), text.text_in(TextRange::new(a, b)))
                }
                _ if cx.tool == Tool::Highlight && start.distance(current) * scale > 4.0 => {
                    (vec![pos.view_rect_to_pdf(start, current)], String::new())
                }
                _ => (Vec::new(), String::new()),
            };
            if quads.is_empty() {
                cx.events.push(CanvasEvent::Status(cx.t.no_text_here.into()));
                return;
            }
            let Some(kind) = cx.tool.annot_kind() else { return };
            let mut annot = new_annot(cx, kind);
            annot.quads = quads;
            if cx.tool == Tool::Replace {
                // The replacement text is asked for; the struck-out text is kept as subject.
                annot.label = text;
                cx.events.push(CanvasEvent::RequestText {
                    page,
                    tool: Tool::Replace,
                    template: Box::new(annot),
                });
            } else {
                annot.contents = text;
                cx.events.push(CanvasEvent::Create { page, annot: Box::new(annot) });
            }
        }
        Interaction::Shape { page, start, current, points } => {
            let Some(pos) = on_screen.iter().find(|p| p.page == page) else { return };
            finish_shape(tab, cx, pos, start, current, points, scale);
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
        Interaction::Erase { page, hits } if !hits.is_empty() => {
            cx.events
                .push(CanvasEvent::Erase(hits.into_iter().map(|index| AnnotRef { page, index }).collect()));
        }
        _ => {}
    }
}

/// Default size (view space) of objects placed by a click.
fn default_size(tool: Tool) -> (f32, f32) {
    match tool {
        Tool::TextBox => (180.0, 40.0),
        Tool::FieldText | Tool::FieldCombo => (150.0, 20.0),
        Tool::FieldCheckBox | Tool::FieldRadio => (14.0, 14.0),
        Tool::FieldList => (150.0, 60.0),
        Tool::FieldButton => (80.0, 22.0),
        Tool::FieldSignature => (180.0, 50.0),
        _ => (100.0, 60.0),
    }
}

fn finish_shape(
    tab: &mut DocTab,
    cx: &mut CanvasCtx,
    pos: &PageOnScreen,
    start: PointF,
    current: PointF,
    points: Vec<PointF>,
    scale: f32,
) {
    let page = pos.page;
    let geo = pos.geo;
    let big_enough = start.distance(current) * scale > 3.0;
    let tool = cx.tool;
    let (a, b) = if big_enough || tool.gesture() != Gesture::Rect {
        (start, current)
    } else {
        let (w, h) = default_size(tool);
        (start, PointF::new(start.x + w, start.y + h))
    };
    let rect = pos.view_rect_to_pdf(a, b);
    match tool {
        Tool::Zoom => {
            if big_enough {
                let r = RectF::from_points(start, current);
                let vw = tab.viewport.width().max(50.0);
                let vh = tab.viewport.height().max(50.0);
                tab.zoom = (vw / (r.width() * PT_TO_SCREEN))
                    .min(vh / (r.height() * PT_TO_SCREEN))
                    .clamp(MIN_ZOOM, MAX_ZOOM);
                tab.zoom_mode = ZoomMode::Custom;
                tab.pending_scroll = Some(ScrollRequest::Point(page, r.center()));
            }
        }
        Tool::Snapshot if big_enough => cx.events.push(CanvasEvent::Snapshot { page, rect }),
        Tool::Crop if big_enough => cx.events.push(CanvasEvent::CropPage { page, rect }),
        Tool::Link if big_enough => cx.events.push(CanvasEvent::CreateLink { page, rect }),
        Tool::AreaHighlight if big_enough => {
            let mut annot = new_annot(cx, AnnotKind::Highlight);
            annot.quads = vec![rect];
            cx.events.push(CanvasEvent::Create { page, annot: Box::new(annot) });
        }
        Tool::Redact if big_enough => {
            let mut annot = new_annot(cx, AnnotKind::Redact);
            annot.rect = rect;
            cx.events.push(CanvasEvent::Create { page, annot: Box::new(annot) });
        }
        Tool::Rectangle | Tool::Ellipse => {
            let mut annot = new_annot(cx, tool.annot_kind().unwrap_or(AnnotKind::Square));
            annot.rect = rect;
            cx.events.push(CanvasEvent::Create { page, annot: Box::new(annot) });
        }
        Tool::TextBox => {
            let mut annot = new_annot(cx, AnnotKind::FreeText);
            annot.rect = rect;
            cx.events.push(CanvasEvent::RequestText { page, tool, template: Box::new(annot) });
        }
        t if t.is_field() => cx.events.push(CanvasEvent::CreateField { page, rect, tool: t }),
        Tool::Line | Tool::Arrow | Tool::MeasureDistance if big_enough => {
            let (p0, p1) = (geo.view_to_pdf(start), geo.view_to_pdf(current));
            let mut annot = new_annot(cx, AnnotKind::Line);
            annot.paths = vec![vec![p0, p1]];
            annot.arrow_end = tool == Tool::Arrow;
            if tool == Tool::MeasureDistance {
                annot.measure_label = format_measure(p0.distance(p1) * PT_TO_MM, false, cx.decimal_comma);
            }
            cx.events.push(CanvasEvent::Create { page, annot: Box::new(annot) });
        }
        Tool::Callout if big_enough => {
            let boxed = callout_box(start, current);
            let mut annot = new_annot(cx, AnnotKind::FreeText);
            annot.rect = geo.view_rect_to_pdf(&boxed);
            annot.leader = vec![geo.view_to_pdf(start), geo.view_to_pdf(current)];
            cx.events.push(CanvasEvent::RequestText { page, tool, template: Box::new(annot) });
        }
        Tool::Pencil | Tool::Signature if points.len() >= 2 => {
            let mut annot = new_annot(cx, AnnotKind::Ink);
            annot.paths = vec![points.iter().map(|p| geo.view_to_pdf(*p)).collect()];
            cx.events.push(CanvasEvent::Create { page, annot: Box::new(annot) });
        }
        _ => {}
    }
}

fn finish_polygon(tab: &mut DocTab, cx: &mut CanvasCtx, on_screen: &[PageOnScreen]) {
    let page = tab.polygon_page;
    let mut view = std::mem::take(&mut tab.polygon);
    view.dedup_by(|a, b| a.distance(*b) < 2.0);
    let Some(pos) = on_screen.iter().find(|p| p.page == page) else { return };
    let points: Vec<PointF> = view.iter().map(|p| pos.geo.view_to_pdf(*p)).collect();
    let tool = cx.tool;
    let closed = matches!(tool, Tool::Polygon | Tool::Cloud | Tool::MeasureArea);
    if points.len() < if closed { 3 } else { 2 } {
        return;
    }
    let mut annot = new_annot(cx, tool.annot_kind().unwrap_or(AnnotKind::Polygon));
    annot.cloudy = tool == Tool::Cloud;
    match tool {
        Tool::MeasurePerimeter => {
            annot.measure_label = format_measure(path_length(&points) * PT_TO_MM, false, cx.decimal_comma);
        }
        Tool::MeasureArea => {
            annot.measure_label =
                format_measure(polygon_area(&points) * PT_TO_MM * PT_TO_MM, true, cx.decimal_comma);
        }
        _ => {}
    }
    annot.paths = vec![points];
    cx.events.push(CanvasEvent::Create { page, annot: Box::new(annot) });
}

fn handle_click(tab: &mut DocTab, cx: &mut CanvasCtx, on_screen: &[PageOnScreen], p: Pos2, shift: bool) {
    let Some(pos) = find_page(on_screen, p) else {
        tab.selected_annot = None;
        tab.selection = None;
        return;
    };
    let pdf = pos.pdf_at(p);
    let view = pos.view_at(p);
    let tol = 4.0 / tab.scale();
    let tool = cx.tool;
    match tool {
        Tool::Hand => click_hand(tab, cx, &pos, pdf, tol),
        Tool::SelectAnnot => {
            tab.selected_annot =
                annotation_at(tab, pos.page, pdf, tol).map(|index| AnnotRef { page: pos.page, index });
        }
        Tool::SelectFields => {
            tab.selected_annot =
                widget_at(tab, pos.page, pdf).map(|index| AnnotRef { page: pos.page, index });
        }
        Tool::SelectText => tab.selection = None,
        Tool::Zoom => {
            tab.zoom = if shift { zoom_out(tab.zoom) } else { zoom_in(tab.zoom) };
            tab.zoom_mode = ZoomMode::Custom;
            tab.pending_scroll = Some(ScrollRequest::Point(pos.page, view));
        }
        Tool::Eraser => {
            if let Some(index) = ink_at(tab, pos.page, pdf, tol) {
                cx.events.push(CanvasEvent::Erase(vec![AnnotRef { page: pos.page, index }]));
            }
        }
        Tool::Note | Tool::Caret | Tool::Typewriter => {
            let (w, h) = match tool {
                Tool::Note => (22.0, 22.0),
                Tool::Caret => (14.0, 12.0),
                _ => (10.0, 10.0),
            };
            let mut annot = new_annot(cx, tool.annot_kind().unwrap_or(AnnotKind::Note));
            annot.rect = pos.view_rect_to_pdf(view, PointF::new(view.x + w, view.y + h));
            cx.events.push(CanvasEvent::RequestText { page: pos.page, tool, template: Box::new(annot) });
        }
        Tool::FileAttach => {
            let rect = pos.view_rect_to_pdf(view, PointF::new(view.x + 16.0, view.y + 22.0));
            cx.events.push(CanvasEvent::AttachFile { page: pos.page, rect });
        }
        Tool::Stamp => {
            let label = win_ansi(&cx.stamp_label.to_uppercase());
            let w = (StdFont::HelveticaBold.text_width(&label, 20.0) + 28.0).max(80.0);
            let h = 36.0;
            let mut annot = new_annot(cx, AnnotKind::Stamp);
            annot.rect = pos.view_rect_to_pdf(
                PointF::new(view.x - w / 2.0, view.y - h / 2.0),
                PointF::new(view.x + w / 2.0, view.y + h / 2.0),
            );
            annot.label = cx.stamp_label.clone();
            annot.icon = Some(cx.stamp_icon.to_string());
            cx.events.push(CanvasEvent::Create { page: pos.page, annot: Box::new(annot) });
        }
        t if t.gesture() == Gesture::Vertices => {
            if tab.polygon_page != pos.page {
                tab.polygon.clear();
                tab.polygon_page = pos.page;
            }
            tab.polygon.push(view);
        }
        t if t.gesture() == Gesture::Rect
            && !matches!(t, Tool::Snapshot | Tool::Link | Tool::Redact | Tool::AreaHighlight) =>
        {
            finish_shape(tab, cx, &pos, view, view, Vec::new(), tab.scale());
        }
        _ => {}
    }
}

fn click_hand(tab: &mut DocTab, cx: &mut CanvasCtx, pos: &PageOnScreen, pdf: PointF, tol: f32) {
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
            LinkTarget::Page(target) => tab.jump_to_page(target),
            LinkTarget::Uri(uri) => cx.events.push(CanvasEvent::OpenUri(uri)),
            LinkTarget::Unsupported => cx.events.push(CanvasEvent::Status(cx.t.link_not_supported.into())),
        }
        return;
    }
    tab.selected_annot =
        annotation_at(tab, pos.page, pdf, tol).map(|index| AnnotRef { page: pos.page, index });
}

fn handle_double_click(
    tab: &mut DocTab,
    cx: &mut CanvasCtx,
    pos: &PageOnScreen,
    p: Pos2,
    on_screen: &[PageOnScreen],
    tol: f32,
) {
    let pdf = pos.pdf_at(p);
    match cx.tool {
        Tool::SelectText => {
            if let Some(text) = tab.page_text(pos.page)
                && let Some(i) = text.hit_test(pdf, 4.0 / tab.scale())
                && let Some(range) = text.word_at(i)
            {
                tab.selection = Some(TextSelection { page: pos.page, range });
            }
        }
        Tool::Hand | Tool::SelectAnnot => {
            if let Some(index) = annotation_at(tab, pos.page, pdf, tol) {
                let kind = tab.annotations(pos.page).get(index).map(|a| a.kind);
                let annot = AnnotRef { page: pos.page, index };
                tab.selected_annot = Some(annot);
                if matches!(kind, Some(AnnotKind::FreeText | AnnotKind::Note | AnnotKind::Caret)) {
                    cx.events.push(CanvasEvent::EditText(annot));
                } else {
                    cx.events.push(CanvasEvent::ShowProperties);
                }
            }
        }
        t if t.gesture() == Gesture::Vertices => {
            // Only a real double click on the same spot finishes (two quick clicks
            // on different corners are two vertices).
            let n = tab.polygon.len();
            if n >= 2 && tab.polygon[n - 1].distance(tab.polygon[n - 2]) * tab.scale() < 6.0 {
                finish_polygon(tab, cx, on_screen);
            }
        }
        _ => {}
    }
}

fn context_menu(tab: &mut DocTab, cx: &mut CanvasCtx, response: &egui::Response) {
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
    if ui.input(|i| i.key_pressed(Key::Escape)) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measurements_are_formatted() {
        assert_eq!(format_measure(12.34, false, true), "12,3 mm");
        assert_eq!(format_measure(1234.0, false, false), "123.4 cm");
        assert_eq!(format_measure(50_000.0, true, true), "500,00 cm²");
        assert_eq!(format_measure(99.0, true, false), "99.0 mm²");
    }

    #[test]
    fn geometry_helpers() {
        let square =
            [PointF::new(0.0, 0.0), PointF::new(10.0, 0.0), PointF::new(10.0, 10.0), PointF::new(0.0, 10.0)];
        assert_eq!(polygon_area(&square), 100.0);
        assert_eq!(path_length(&square), 30.0);
        let b = callout_box(PointF::new(0.0, 0.0), PointF::new(50.0, 50.0));
        assert_eq!(b.x0, 50.0);
        let b = callout_box(PointF::new(100.0, 0.0), PointF::new(50.0, 50.0));
        assert_eq!(b.x1, 50.0);
    }
}
