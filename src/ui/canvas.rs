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
use crate::ui::pickers::Unit;
use crate::ui::tab::{
    AnnotRef, DocTab, FieldEdit, Grip, Guide, Interaction, MovePreview, ScrollRequest, Split, TextSelection,
};
use crate::ui::theme;
use crate::ui::tools::{Gesture, Tool};
use crate::ui::view::{
    Layout, LayoutOptions, MARGIN, MAX_ZOOM, MIN_ZOOM, PT_TO_SCREEN, PageLayout, ZoomMode, fit_page_zoom,
    fit_width_zoom, zoom_in, zoom_out,
};

/// Display options of the ribbon tab "Ansicht" used by the canvas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewOptions {
    /// Page display (the mode itself comes from the tab).
    pub layout: LayoutOptions,
    /// "Gitter": distance of the main lines in points and subdivisions.
    pub grid: Option<(f32, u32)>,
    /// "Hilfslinien" are shown (and can be moved).
    pub guides: bool,
    /// "Transparenzgitter" instead of white paper.
    pub transparency: bool,
    /// "Verweise hervorheben".
    pub highlight_links: bool,
    /// "Einrasten": (at the grid, at guides, at page objects); None: off.
    pub snap: Option<(bool, bool, bool)>,
    /// "Lineale".
    pub rulers: bool,
    pub unit: Unit,
    /// "Lupe": magnification (the area is marked on the page).
    pub loupe: Option<f32>,
}

impl Default for ViewOptions {
    fn default() -> Self {
        ViewOptions {
            layout: LayoutOptions::new(PageLayout::Continuous),
            grid: None,
            guides: true,
            transparency: false,
            highlight_links: false,
            snap: None,
            rulers: false,
            unit: Unit::Mm,
            loupe: None,
        }
    }
}

mod content;

/// Width of the rulers in points.
pub const RULER: f32 = 18.0;
/// Size of the loupe picture in points.
pub const LOUPE_SIZE: Vec2 = vec2(320.0, 220.0);

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
    /// New size or point of a comment (its handles); `contents` is the new
    /// value of a measurement.
    Reshape {
        annot: AnnotRef,
        reshape: crate::pdf_edit::Reshape,
        contents: Option<String>,
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
    /// Ask for a media file and embed it in `rect` (media tools).
    AddMedia {
        page: usize,
        rect: RectF,
        tool: Tool,
    },
    /// Open the file of a media annotation with the system's program.
    PlayMedia(AnnotRef),
    /// A push button was clicked while filling in ("Bild" fields).
    PressButton {
        page: usize,
        annot_index: usize,
    },
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
    /// "Objekte bearbeiten" / "Text bearbeiten".
    Content(crate::ui::content_state::ContentOp),
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
    pub view: ViewOptions,
    /// "Exklusiver Modus" (see `Settings::exclusive_mode`).
    pub exclusive: bool,
    /// "Felder hervorheben ▾": kinds not highlighted, only required fields.
    pub highlight_off: Vec<crate::ui::settings::FieldGroup>,
    pub highlight_required_only: bool,
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

    fn pdf_to_screen(&self, p: PointF) -> Pos2 {
        self.view_to_screen(self.geo.pdf_to_view(p))
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
        ScrollRequest::Page(p) if tab.layout.horizontal => tab
            .layout
            .slot(*p)
            .map(|s| vec2((s.rect.min.x + tab.content_shift.x - MARGIN / 2.0).max(0.0), tab.scroll_offset.y)),
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

/// Draws the document: the rulers, and one view or the panes of a split
/// window ("Teilen").
pub fn show_document(ui: &mut Ui, tab: &mut DocTab, cx: &mut CanvasCtx) {
    tab.textures.begin_frame();
    tab.pointer = None;
    let full = ui.available_rect_before_wrap();
    ui.allocate_rect(full, Sense::hover());
    let r = if cx.view.rulers { RULER } else { 0.0 };
    let area = Rect::from_min_max(full.min + vec2(r, r), full.max);
    let panes = pane_rects(area, tab.split);
    let mut activate = None;
    let pressed = ui.input(|i| i.pointer.any_pressed().then(|| i.pointer.interact_pos()).flatten());
    for (i, rect) in panes.iter().enumerate() {
        let mut child = ui.new_child(
            egui::UiBuilder::new().max_rect(*rect).layout(egui::Layout::top_down(egui::Align::Min)),
        );
        child.set_clip_rect(rect.intersect(ui.clip_rect()));
        tab.drawing_pane = i;
        if i == tab.active_pane || tab.split == Split::None {
            show(&mut child, tab, cx);
        } else {
            let mut v = tab.panes[i].clone();
            tab.swap_view(&mut v);
            show(&mut child, tab, cx);
            tab.swap_view(&mut v);
            tab.panes[i] = v;
        }
        if pressed.is_some_and(|p| rect.contains(p)) && i != tab.active_pane {
            activate = Some(i);
        }
    }
    tab.drawing_pane = tab.active_pane;
    // Dividers between the panes; the active pane has a frame.
    if panes.len() > 1 {
        let painter = ui.painter_at(area);
        let line = Stroke::new(3.0, ui.visuals().widgets.noninteractive.bg_stroke.color);
        for rect in &panes {
            painter.rect_stroke(*rect, 0.0, line, StrokeKind::Inside);
        }
        if let Some(rect) = panes.get(tab.active_pane) {
            painter.rect_stroke(*rect, 0.0, Stroke::new(1.5, theme::ACCENT), StrokeKind::Inside);
        }
    }
    if let Some(i) = activate {
        tab.activate_pane(i);
    }
    if cx.view.rulers {
        rulers(ui, tab, cx, full, area);
    }
    tab.textures.evict();
}

/// The rectangles of the panes of a split window.
fn pane_rects(area: Rect, split: Split) -> Vec<Rect> {
    let (l, r) = (area.left(), area.right());
    let (t, b) = (area.top(), area.bottom());
    let (cx, cy) = (area.center().x, area.center().y);
    match split {
        Split::None => vec![area],
        Split::Horizontal => {
            vec![Rect::from_min_max(pos2(l, t), pos2(r, cy)), Rect::from_min_max(pos2(l, cy), pos2(r, b))]
        }
        Split::Vertical => {
            vec![Rect::from_min_max(pos2(l, t), pos2(cx, b)), Rect::from_min_max(pos2(cx, t), pos2(r, b))]
        }
        Split::Grid => vec![
            Rect::from_min_max(pos2(l, t), pos2(cx, cy)),
            Rect::from_min_max(pos2(cx, t), pos2(r, cy)),
            Rect::from_min_max(pos2(l, cy), pos2(cx, b)),
            Rect::from_min_max(pos2(cx, cy), pos2(r, b)),
        ],
    }
}

/// Distance of the labelled ruler ticks in `unit` for a scale (screen
/// points per unit): at least 50 points apart.
fn ruler_step(points_per_unit: f32) -> f32 {
    [1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0]
        .into_iter()
        .find(|s| s * points_per_unit >= 50.0)
        .unwrap_or(1000.0)
}

/// The rulers ("Lineale") at the top and left; dragging from a ruler
/// creates a guide.
fn rulers(ui: &mut Ui, tab: &mut DocTab, cx: &mut CanvasCtx, full: Rect, area: Rect) {
    let top = Rect::from_min_max(pos2(area.left(), full.top()), pos2(full.right(), area.top()));
    let left = Rect::from_min_max(pos2(full.left(), area.top()), pos2(area.left(), full.bottom()));
    let corner = Rect::from_min_max(full.min, area.min);
    let visuals = ui.visuals().clone();
    let bg = if visuals.dark_mode { Color32::from_gray(40) } else { Color32::from_gray(236) };
    let fg = visuals.text_color();
    let painter = ui.painter_at(full);
    for r in [top, left, corner] {
        painter.rect_filled(r, 0.0, bg);
    }
    painter.line_segment([top.left_bottom(), top.right_bottom()], Stroke::new(1.0, fg.gamma_multiply(0.5)));
    painter.line_segment([left.right_top(), left.right_bottom()], Stroke::new(1.0, fg.gamma_multiply(0.5)));
    let Some((_, page_rect)) = tab.ruler_page else { return };
    let unit_pt = cx.view.unit.points();
    let ppu = tab.scale() * unit_pt;
    let step = ruler_step(ppu);
    let font = egui::FontId::proportional(9.5);
    let tick = Stroke::new(1.0, fg.gamma_multiply(0.7));
    let decimals = cx.view.unit == Unit::Inch && step < 1.0;
    // Top ruler.
    let p = painter.with_clip_rect(top);
    let first = ((top.left() - page_rect.left()) / ppu / step).floor() as i64;
    let last = ((top.right() - page_rect.left()) / ppu / step).ceil() as i64;
    for k in first..=last.min(first + 2000) {
        let v = k as f32 * step;
        let x = page_rect.left() + v * ppu;
        p.line_segment([pos2(x, top.bottom() - 8.0), pos2(x, top.bottom())], tick);
        for m in 1..10 {
            let xm = x + m as f32 * step * ppu / 10.0;
            let len = if m == 5 { 5.0 } else { 3.0 };
            p.line_segment([pos2(xm, top.bottom() - len), pos2(xm, top.bottom())], tick);
        }
        let label = if decimals { format!("{v:.1}") } else { format!("{v:.0}") };
        p.text(pos2(x + 2.0, top.top() + 1.0), egui::Align2::LEFT_TOP, label, font.clone(), fg);
    }
    // Left ruler.
    let p = painter.with_clip_rect(left);
    let first = ((left.top() - page_rect.top()) / ppu / step).floor() as i64;
    let last = ((left.bottom() - page_rect.top()) / ppu / step).ceil() as i64;
    for k in first..=last.min(first + 2000) {
        let v = k as f32 * step;
        let y = page_rect.top() + v * ppu;
        p.line_segment([pos2(left.right() - 8.0, y), pos2(left.right(), y)], tick);
        for m in 1..10 {
            let ym = y + m as f32 * step * ppu / 10.0;
            let len = if m == 5 { 5.0 } else { 3.0 };
            p.line_segment([pos2(left.right() - len, ym), pos2(left.right(), ym)], tick);
        }
        let label = if decimals { format!("{v:.1}") } else { format!("{v:.0}") };
        p.text(pos2(left.left() + 1.0, y + 2.0), egui::Align2::LEFT_TOP, label, font.clone(), fg);
    }
    // Pointer position on the rulers.
    if let Some(h) = ui.input(|i| i.pointer.hover_pos()).filter(|h| area.contains(*h)) {
        let marker = Stroke::new(1.0, theme::ACCENT);
        painter.line_segment([pos2(h.x, top.top()), pos2(h.x, top.bottom())], marker);
        painter.line_segment([pos2(left.left(), h.y), pos2(left.right(), h.y)], marker);
    }
    // Dragging out of a ruler creates a guide; back onto the ruler removes it.
    if !cx.view.guides {
        return;
    }
    let scale = tab.scale();
    for (rect, vertical, id) in [(top, false, "ruler-top"), (left, true, "ruler-left")] {
        let response = ui.interact(rect, ui.id().with(id), Sense::drag());
        if response.hovered() {
            ui.ctx().set_cursor_icon(if vertical {
                CursorIcon::ResizeHorizontal
            } else {
                CursorIcon::ResizeVertical
            });
        }
        if response.drag_started() {
            tab.guides.push(Guide { vertical, pos: 0.0 });
            tab.interaction = Interaction::Guide { index: tab.guides.len() - 1 };
        }
        if let Interaction::Guide { index } = tab.interaction
            && (response.dragged() || response.drag_stopped())
            && let Some(p) = response.interact_pointer_pos()
            && let Some(g) = tab.guides.get_mut(index)
        {
            g.pos = if vertical { (p.x - page_rect.left()) / scale } else { (p.y - page_rect.top()) / scale };
            if response.drag_stopped() {
                if !area.contains(p) {
                    tab.guides.remove(index);
                }
                tab.interaction = Interaction::None;
            }
        }
    }
}

/// "Einrasten": moves a view point onto the nearest grid line, guide or
/// edge of a page object within a few screen points.
fn snap_point(tab: &DocTab, cx: &CanvasCtx, page: usize, v: PointF) -> PointF {
    let Some((to_grid, to_guides, to_objects)) = cx.view.snap else { return v };
    let reach = 6.0 / tab.scale();
    let mut x = (v.x, reach);
    let mut y = (v.y, reach);
    let mut consider = |vertical: bool, pos: f32| {
        let (cur, value) = if vertical { (&mut x, v.x) } else { (&mut y, v.y) };
        let d = (value - pos).abs();
        if d < cur.1 {
            *cur = (pos, d);
        }
    };
    if to_grid {
        let (main, sub) = cx.view.grid.unwrap_or((10.0 * 72.0 / 25.4, 10));
        let step = main / sub.max(1) as f32;
        consider(true, (v.x / step).round() * step);
        consider(false, (v.y / step).round() * step);
    }
    if to_guides && cx.view.guides {
        for g in &tab.guides {
            consider(g.vertical, g.pos);
        }
    }
    // Edges of text, images and graphics (read when the drag started).
    if to_objects && let (Some(objects), Some(geo)) = (tab.cached_objects(page), tab.doc.geometry(page)) {
        for o in objects.iter().filter(|o| !o.invisible) {
            let a = geo.pdf_to_view(PointF::new(o.rect.x0, o.rect.y0));
            let b = geo.pdf_to_view(PointF::new(o.rect.x1, o.rect.y1));
            consider(true, a.x);
            consider(true, b.x);
            consider(false, a.y);
            consider(false, b.y);
        }
    }
    PointF::new(x.0, y.0)
}

/// Checkerboard texture for "Transparenzgitter" (repeated).
fn checker_texture(ctx: &egui::Context) -> egui::TextureHandle {
    let id = egui::Id::new("versolopdf-checker");
    if let Some(tex) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return tex;
    }
    let mut img = egui::ColorImage::new([16, 16], vec![Color32::WHITE; 256]);
    for y in 0..16 {
        for x in 0..16 {
            if (x / 8 + y / 8) % 2 == 1 {
                img.pixels[y * 16 + x] = Color32::from_gray(204);
            }
        }
    }
    let options =
        egui::TextureOptions { wrap_mode: egui::TextureWrapMode::Repeat, ..egui::TextureOptions::NEAREST };
    let tex = ctx.load_texture("versolopdf-checker", img, options);
    ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
    tex
}

pub fn show(ui: &mut Ui, tab: &mut DocTab, cx: &mut CanvasCtx) {
    let ctx = ui.ctx().clone();
    let avail = ui.available_size();
    let sizes = tab.page_sizes();
    if sizes.is_empty() {
        return;
    }
    let facing = tab.layout_mode.two_up();
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

    let opts = LayoutOptions { mode: tab.layout_mode, ..cx.view.layout };
    tab.layout = Layout::compute(&sizes, tab.zoom, opts, tab.current_page);

    let mut area = ScrollArea::both().id_salt(("doc", tab.id, tab.drawing_pane)).auto_shrink([false, false]);
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

    // Page by page layouts: the mouse wheel flips pages at the top/bottom edge.
    if tab.layout_mode.paged()
        && let Some(pointer) = ui.input(|i| i.pointer.hover_pos())
        && output.inner_rect.contains(pointer)
    {
        let (dy, now) = ui.input(|i| (i.smooth_scroll_delta.y, i.time));
        let max_y = (output.content_size.y - output.inner_rect.height()).max(0.0);
        let count = tab.doc.page_count();
        let shown_last = tab.layout.slots.iter().map(|s| s.page).max().unwrap_or(tab.current_page);
        let shown_first = tab.layout.slots.iter().map(|s| s.page).min().unwrap_or(tab.current_page);
        if now - tab.last_flip > 0.35 {
            if dy < 0.0 && output.state.offset.y >= max_y - 1.0 && shown_last + 1 < count {
                tab.go_to_page(shown_last + 1);
                tab.last_flip = now;
            } else if dy > 0.0 && output.state.offset.y <= 1.0 && shown_first > 0 {
                tab.go_to_page(shown_first - 1);
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
        if tab.layout_mode.paged() || tab.layout.horizontal {
            ((size.y - layout_size.y) / 2.0).max(0.0)
        } else {
            0.0
        },
    );
    tab.content_shift = shift;
    tab.viewport = viewport;
    let origin = rect.min + shift;
    tab.canvas_origin = origin;
    let painter = ui.painter_at(ui.clip_rect());
    painter.rect_filled(ui.clip_rect(), 0.0, theme::canvas_background(ui.visuals().dark_mode));

    // Track the current page in scrolling layouts.
    if !tab.layout_mode.paged()
        && let Some(p) = tab.layout.page_at(viewport.center() - shift)
        && tab.pending_scroll.is_none()
    {
        tab.current_page = p;
    }
    let hover = response.hover_pos();
    let mut ruler_page = None;

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
        // The rulers refer to the page under the pointer, else the current page.
        if hover.is_some_and(|h| screen.contains(h))
            || (ruler_page.is_none() && slot.page == tab.current_page)
        {
            ruler_page = Some((slot.page, screen));
        }
        if !screen.intersects(visible_screen.expand(200.0)) {
            continue;
        }
        // Page shadow and paper (or the transparency grid).
        painter.rect_filled(screen.translate(vec2(2.0, 3.0)), 1.0, Color32::from_black_alpha(60));
        if cx.view.transparency {
            let tex = checker_texture(ctx);
            let uv = Rect::from_min_size(Pos2::ZERO, screen.size() / 16.0);
            painter.image(tex.id(), screen, uv, Color32::WHITE);
        } else {
            painter.rect_filled(screen, 0.0, Color32::WHITE);
        }

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
        draw_move_preview(
            &painter.with_clip_rect(screen.intersect(visible_screen)),
            tab,
            &pos,
            opts,
            [px_w, px_h],
        );
        painter.rect_stroke(screen, 0.0, Stroke::new(1.0, Color32::from_gray(150)), StrokeKind::Outside);
        if let Some((main, sub)) = cx.view.grid {
            draw_grid(&painter.with_clip_rect(screen.intersect(visible_screen)), screen, scale, main, sub);
        }
        draw_overlays(&painter, tab, &pos, cx, response.hover_pos());
    }
    if tab.drawing_pane == tab.active_pane {
        tab.ruler_page = ruler_page;
    }

    if matches!(cx.tool, Tool::EditText | Tool::EditObjects) {
        content::interact(ui, tab, cx, &response, &on_screen);
    } else {
        handle_input(ui, tab, cx, &response, &on_screen);
    }
    draw_field_editor(ui, tab, &on_screen, cx);
}

/// "Gitter": main lines every `main` points, `sub` subdivisions.
fn draw_grid(painter: &egui::Painter, page: Rect, scale: f32, main: f32, sub: u32) {
    let sub = sub.max(1);
    let step = main / sub as f32;
    if step * scale < 3.0 {
        return;
    }
    let clip = painter.clip_rect();
    let minor = Stroke::new(1.0, Color32::from_rgba_unmultiplied(80, 120, 220, 40));
    let major = Stroke::new(1.0, Color32::from_rgba_unmultiplied(80, 120, 220, 110));
    let lines = |horizontal: bool| {
        let len = if horizontal { page.height() } else { page.width() } / scale;
        let n = (len / step).floor() as u32;
        for k in 0..=n.min(5000) {
            let off = k as f32 * step * scale;
            let stroke = if k % sub == 0 { major } else { minor };
            if horizontal {
                let y = page.top() + off;
                if y >= clip.top() - 1.0 && y <= clip.bottom() + 1.0 {
                    painter.line_segment([pos2(page.left(), y), pos2(page.right(), y)], stroke);
                }
            } else {
                let x = page.left() + off;
                if x >= clip.left() - 1.0 && x <= clip.right() + 1.0 {
                    painter.line_segment([pos2(x, page.top()), pos2(x, page.bottom())], stroke);
                }
            }
        }
    };
    lines(true);
    lines(false);
}

/// While comments are dragged, shows them at the new place right away: the
/// old place shows the page without them, the comments alone follow the
/// pointer. The document itself changes when the drag ends.
fn draw_move_preview(
    painter: &egui::Painter,
    tab: &mut DocTab,
    pos: &PageOnScreen,
    opts: crate::document::RenderOptions,
    full_px: [u32; 2],
) {
    let Interaction::MoveAnnot { annot, delta } = tab.interaction else {
        tab.move_preview = None;
        return;
    };
    if annot.page != pos.page || delta == Vec2::ZERO {
        return;
    }
    let key = (annot, tab.doc.page_revision(pos.page), full_px);
    if tab.move_preview.as_ref().is_none_or(|m| m.key != key) {
        let parts = move_preview_parts(painter.ctx(), tab, pos, opts, full_px, annot);
        tab.move_preview = Some(MovePreview { key, parts });
    }
    let Some(preview) = &tab.move_preview else { return };
    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    for (area, base, alone) in &preview.parts {
        let r = area.translate(pos.rect.min.to_vec2());
        painter.image(base.id(), r, uv, Color32::WHITE);
        painter.image(alone.id(), r.translate(delta), uv, Color32::WHITE);
    }
}

/// Renders the areas of the dragged comments with and without comments;
/// the pixels that differ are the comments.
fn move_preview_parts(
    ctx: &egui::Context,
    tab: &mut DocTab,
    pos: &PageOnScreen,
    opts: crate::document::RenderOptions,
    full_px: [u32; 2],
    annot: AnnotRef,
) -> Vec<(Rect, egui::TextureHandle, egui::TextureHandle)> {
    let annots = tab.annotations(pos.page);
    let ppp = full_px[0] as f32 / pos.rect.width().max(1.0);
    let bare = crate::document::RenderOptions { annotations: false, ..opts };
    let mut parts = Vec::new();
    for sel in moving_annots(tab, annot) {
        let Some(a) = annots.get(sel.index) else { continue };
        let src = pos.pdf_rect_to_screen(&a.rect).expand(4.0).intersect(pos.rect);
        let x = ((src.min.x - pos.rect.min.x) * ppp).max(0.0) as u32;
        let y = ((src.min.y - pos.rect.min.y) * ppp).max(0.0) as u32;
        let w = ((src.width() * ppp) as u32).min(full_px[0].saturating_sub(x));
        let h = ((src.height() * ppp) as u32).min(full_px[1].saturating_sub(y));
        // Very large areas would take too long; the frame alone still shows the move.
        if w == 0 || h == 0 || u64::from(w) * u64::from(h) > 8_000_000 {
            continue;
        }
        let (Ok(with), Ok(without)) = (
            tab.doc.render_tile(pos.page, full_px[0], full_px[1], x, y, w, h, opts),
            tab.doc.render_tile(pos.page, full_px[0], full_px[1], x, y, w, h, bare),
        ) else {
            continue;
        };
        let mut alone = with.rgba.clone();
        for (px, base) in alone.chunks_mut(4).zip(without.rgba.chunks(4)) {
            let diff: i32 = (0..3).map(|c| (i32::from(px[c]) - i32::from(base[c])).abs()).sum();
            // Soft edges; tiny differences (text smoothing) stay invisible.
            px[3] = ((diff - 60) * 255 / 120).clamp(0, 255) as u8;
        }
        let size = [w as usize, h as usize];
        let opts_tex = egui::TextureOptions::LINEAR;
        let base_tex = ctx.load_texture(
            "move-preview-base",
            egui::ColorImage::from_rgba_unmultiplied(size, &without.rgba),
            opts_tex,
        );
        let alone_tex = ctx.load_texture(
            "move-preview",
            egui::ColorImage::from_rgba_unmultiplied(size, &alone),
            opts_tex,
        );
        let area =
            Rect::from_min_size(pos2(x as f32 / ppp, y as f32 / ppp), vec2(w as f32 / ppp, h as f32 / ppp));
        parts.push((area, base_tex, alone_tex));
    }
    parts
}
/// Handles of the selected comment on this page (screen positions): box
/// handles for comments with an area, points for lines and polygons.
fn grips(tab: &mut DocTab, pos: &PageOnScreen) -> Vec<(Grip, Pos2)> {
    let Some(sel) = tab.selected_annot.filter(|s| s.page == pos.page) else { return Vec::new() };
    if tab.selected_annots().len() > 1 {
        return Vec::new();
    }
    let Some(a) = tab.annotations(pos.page).get(sel.index).cloned() else { return Vec::new() };
    match a.kind {
        AnnotKind::Square | AnnotKind::Circle | AnnotKind::FreeText | AnnotKind::Stamp | AnnotKind::Ink => {
            let r = pos.pdf_rect_to_screen(&a.rect);
            box_grips(r).into_iter().enumerate().map(|(i, p)| (Grip::Box(i as u8), p)).collect()
        }
        AnnotKind::Line | AnnotKind::Polygon | AnnotKind::Polyline => tab
            .selected_details()
            .map(|d| {
                d.points.iter().enumerate().map(|(i, p)| (Grip::Point(i), pos.pdf_to_screen(*p))).collect()
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// The eight box handles, clockwise from the top left corner.
fn box_grips(r: Rect) -> [Pos2; 8] {
    [
        r.left_top(),
        r.center_top(),
        r.right_top(),
        r.right_center(),
        r.right_bottom(),
        r.center_bottom(),
        r.left_bottom(),
        r.left_center(),
    ]
}

/// The handle under the screen point `p`.
fn grip_at(tab: &mut DocTab, pos: &PageOnScreen, p: Pos2) -> Option<Grip> {
    grips(tab, pos).into_iter().find(|(_, g)| g.distance(p) <= 6.0).map(|(g, _)| g)
}

/// The box after dragging handle `grip` to `to`.
fn reshaped_box(r: Rect, grip: u8, to: Pos2) -> Rect {
    let (mut x0, mut y0, mut x1, mut y1) = (r.min.x, r.min.y, r.max.x, r.max.y);
    if matches!(grip, 0 | 6 | 7) {
        x0 = to.x;
    }
    if matches!(grip, 2..=4) {
        x1 = to.x;
    }
    if matches!(grip, 0..=2) {
        y0 = to.y;
    }
    if matches!(grip, 4..=6) {
        y1 = to.y;
    }
    Rect::from_two_pos(pos2(x0, y0), pos2(x1, y1))
}

fn grip_cursor(grip: Grip) -> CursorIcon {
    match grip {
        Grip::Box(0 | 4) => CursorIcon::ResizeNwSe,
        Grip::Box(2 | 6) => CursorIcon::ResizeNeSw,
        Grip::Box(1 | 5) => CursorIcon::ResizeVertical,
        Grip::Box(_) => CursorIcon::ResizeHorizontal,
        Grip::Point(_) => CursorIcon::Crosshair,
    }
}

/// The comments that move with `annot`: the whole selection if `annot` is
/// part of it, else `annot` alone.
fn moving_annots(tab: &DocTab, annot: AnnotRef) -> Vec<AnnotRef> {
    let selected = tab.selected_annots();
    if selected.contains(&annot) { selected } else { vec![annot] }
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
    // "Verweise hervorheben".
    if cx.view.highlight_links {
        for l in tab.links(page).iter() {
            let r = pos.pdf_rect_to_screen(&l.rect);
            painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(40, 110, 230, 35));
            painter.rect_stroke(r, 0.0, Stroke::new(1.0, theme::ACCENT), StrokeKind::Outside);
        }
    }
    // Guides ("Hilfslinien").
    if cx.view.guides {
        let stroke = Stroke::new(1.0, Color32::from_rgb(0, 170, 220));
        for g in &tab.guides {
            if g.vertical {
                let x = pos.rect.left() + g.pos * pos.scale;
                if x >= pos.rect.left() && x <= pos.rect.right() {
                    painter.line_segment([pos2(x, pos.rect.top()), pos2(x, pos.rect.bottom())], stroke);
                }
            } else {
                let y = pos.rect.top() + g.pos * pos.scale;
                if y >= pos.rect.top() && y <= pos.rect.bottom() {
                    painter.line_segment([pos2(pos.rect.left(), y), pos2(pos.rect.right(), y)], stroke);
                }
            }
        }
    }
    // The area shown in the loupe.
    if let Some(factor) = cx.view.loupe
        && let Some((p, at)) = tab.loupe_at
        && p == page
    {
        // The loupe shows LOUPE_SIZE screen points at `factor` × 100 %.
        let half = LOUPE_SIZE / (2.0 * factor * PT_TO_SCREEN) * pos.scale;
        let c = pos.view_to_screen(at);
        painter.rect_stroke(
            Rect::from_center_size(c, half * 2.0),
            0.0,
            Stroke::new(1.5, Color32::from_rgb(220, 60, 40)),
            StrokeKind::Middle,
        );
    }
    let designing = cx.tool == Tool::SelectFields || cx.tool.is_field();
    if cx.highlight_fields || designing {
        for f in tab.fields(page).iter() {
            let group = crate::ui::settings::FieldGroup::of(&f.kind);
            let shown = designing
                || (!f.read_only
                    && group.is_some_and(|g| !cx.highlight_off.contains(&g))
                    && (f.required || !cx.highlight_required_only));
            if shown {
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
    // Differences of "Dokumente vergleichen": tinted areas with a frame,
    // the chosen one stronger.
    for m in tab.compare_marks.iter().filter(|m| m.page == page) {
        let c = crate::ui::tab::CompareMark::color(m.kind);
        let r = pos.pdf_rect_to_screen(&m.rect).expand(if m.caret { 0.0 } else { 1.5 });
        let alpha = if m.selected { 90 } else { 45 };
        if m.caret {
            painter.rect_filled(r.expand2(egui::vec2(1.0, 2.0)), 0.0, c);
        } else {
            painter.rect_filled(r, 2.0, Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), alpha));
            painter.rect_stroke(
                r,
                2.0,
                Stroke::new(if m.selected { 2.5 } else { 1.0 }, c),
                StrokeKind::Outside,
            );
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
    // Selected annotations or form field (moved along while dragged).
    let moving = match &interaction {
        Interaction::MoveAnnot { annot, delta } => {
            moving_annots(tab, *annot).into_iter().map(|a| (a, *delta)).collect()
        }
        _ => Vec::new(),
    };
    let annots = tab.annotations(page);
    for sel in tab.selected_annots().into_iter().filter(|s| s.page == page) {
        let Some(a) = annots.get(sel.index) else { continue };
        let mut r = pos.pdf_rect_to_screen(&a.rect).expand(2.0);
        if let Some((_, delta)) = moving.iter().find(|(m, _)| *m == sel) {
            r = r.translate(*delta);
        }
        let accent = theme::ACCENT;
        painter.rect_stroke(r, 0.0, Stroke::new(1.5, accent), StrokeKind::Outside);
    }
    // Handles of a single selected comment: squares change the size, circles
    // move a point. Comments without handles (text markup, note symbols)
    // only show the frame.
    if !matches!(interaction, Interaction::MoveAnnot { .. }) {
        for (grip, p) in grips(tab, pos) {
            let stroke = Stroke::new(1.2, theme::ACCENT);
            match grip {
                Grip::Box(_) => {
                    painter.rect(
                        Rect::from_center_size(p, vec2(7.0, 7.0)),
                        1.0,
                        Color32::WHITE,
                        stroke,
                        StrokeKind::Middle,
                    );
                }
                Grip::Point(_) => {
                    painter.circle(p, 4.0, Color32::WHITE, stroke);
                }
            }
        }
    }
    // The new shape while a handle is dragged.
    if let Interaction::ReshapeAnnot { annot, grip, current } = &interaction
        && annot.page == page
        && let Some(a) = tab.annotations(page).get(annot.index).cloned()
    {
        let stroke = Stroke::new(1.2, theme::ACCENT);
        match grip {
            Grip::Box(i) => {
                let r = reshaped_box(pos.pdf_rect_to_screen(&a.rect), *i, *current);
                let pts = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
                painter.extend(egui::Shape::dashed_line(&pts, stroke, 5.0, 3.0));
            }
            Grip::Point(i) => {
                let mut pts: Vec<Pos2> = tab
                    .selected_details()
                    .map(|d| d.points.iter().map(|p| pos.pdf_to_screen(*p)).collect())
                    .unwrap_or_default();
                if let Some(p) = pts.get_mut(*i) {
                    *p = *current;
                }
                if a.kind == AnnotKind::Polygon
                    && let Some(first) = pts.first().copied()
                {
                    pts.push(first);
                }
                painter.extend(egui::Shape::dashed_line(&pts, stroke, 5.0, 3.0));
            }
        }
    }
    // Selection rectangle.
    if let Interaction::Marquee { page: p, start, current, .. } = &interaction
        && *p == page
    {
        let r = Rect::from_two_pos(pos.view_to_screen(*start), pos.view_to_screen(*current));
        painter.rect_filled(r, 0.0, theme::ACCENT.gamma_multiply(0.12));
        let pts = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
        painter.extend(egui::Shape::dashed_line(&pts, Stroke::new(1.2, theme::ACCENT), 5.0, 3.0));
    }
    // Open pop-ups ("Alle Pop-ups öffnen"): author and text next to the comment.
    let popups: Vec<usize> = tab.open_popups.iter().filter(|r| r.page == page).map(|r| r.index).collect();
    for index in popups {
        let Some(a) = tab.annotations(page).get(index).cloned() else { continue };
        let anchor = pos.pdf_rect_to_screen(&a.rect);
        let font = egui::FontId::proportional(12.0);
        let text = if a.contents.trim().is_empty() { "…".to_string() } else { a.contents.clone() };
        let galley = painter.layout(text, font.clone(), Color32::from_gray(20), 220.0);
        let title = painter.layout_no_wrap(a.author.clone(), font, Color32::from_gray(40));
        let size = vec2(galley.size().x.max(title.size().x).max(120.0) + 12.0, galley.size().y + 30.0);
        let r = Rect::from_min_size(anchor.right_top() + vec2(8.0, 0.0), size);
        painter.rect_filled(r, 3.0, Color32::from_rgb(255, 250, 200));
        painter.rect_stroke(r, 3.0, Stroke::new(1.0, Color32::from_rgb(190, 170, 60)), StrokeKind::Inside);
        painter.line_segment(
            [anchor.right_top(), r.left_top()],
            Stroke::new(1.0, Color32::from_rgb(190, 170, 60)),
        );
        painter.galley(r.min + vec2(6.0, 4.0), title, Color32::from_gray(40));
        painter.galley(r.min + vec2(6.0, 22.0), galley, Color32::from_gray(20));
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

/// Topmost visible comment annotation at `pdf` on `page`.
fn annotation_at(tab: &mut DocTab, page: usize, pdf: PointF, tolerance: f32) -> Option<usize> {
    let filter = tab.doc.annot_filter().clone();
    tab.annotations(page)
        .iter()
        .rev()
        .find(|a| {
            a.kind.is_comment()
                && a.rect.inflate(tolerance).contains(pdf)
                && !filter.hides(page, a.index, a.kind, &a.author, false)
        })
        .map(|a| a.index)
}

/// "Exklusiver Modus": a comment of the active tool's type at `pdf`.
fn own_kind_at(tab: &mut DocTab, cx: &CanvasCtx, page: usize, pdf: PointF, tol: f32) -> Option<usize> {
    let kind = cx.tool.annot_kind().filter(|_| cx.exclusive)?;
    let index = annotation_at(tab, page, pdf, tol)?;
    tab.annotations(page).get(index).is_some_and(|a| a.kind == kind).then_some(index)
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
        Tool::Zoom | Tool::Loupe => CursorIcon::ZoomIn,
        Tool::DynamicZoom => CursorIcon::ResizeVertical,
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
        let cursor = match (grip_at(tab, &pos, h), guide_at(tab, cx, &pos, h)) {
            (Some(grip), _) => grip_cursor(grip),
            (None, Some(i)) if tab.guides[i].vertical => CursorIcon::ResizeHorizontal,
            (None, Some(_)) => CursorIcon::ResizeVertical,
            (None, None) => cursor_for(tab, cx, &pos, pos.pdf_at(h)),
        };
        ui.ctx().set_cursor_icon(cursor);
        // "Seitenmaße/Position" and the loupe follow the pointer.
        tab.pointer = Some((pos.page, pos.view_at(h)));
        if cx.tool == Tool::Loupe {
            tab.loupe_at = Some((pos.page, pos.view_at(h)));
        }
    }

    // Moving a guide; dropped outside the page it is removed.
    if let Interaction::Guide { index } = tab.interaction {
        if response.dragged()
            && let Some(cur) = response.interact_pointer_pos()
            && let Some(pos) = on_screen.iter().find(|p| p.rect.expand(40.0).contains(cur))
            && let Some(g) = tab.guides.get_mut(index)
        {
            let v = pos.view_at(cur);
            g.pos = if g.vertical { v.x } else { v.y };
        }
        if response.drag_stopped() {
            let inside = response.interact_pointer_pos().is_some_and(|p| find_page(on_screen, p).is_some());
            if !inside && index < tab.guides.len() {
                tab.guides.remove(index);
            }
            tab.interaction = Interaction::None;
        }
        if response.dragged() || response.drag_stopped() {
            return;
        }
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
        tab.interaction = start_drag(tab, cx, find_page(on_screen, start), start, tol, shift_down);
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

/// Index of the guide near the screen point `p` (only with tools that do
/// not draw).
fn guide_at(tab: &DocTab, cx: &CanvasCtx, pos: &PageOnScreen, p: Pos2) -> Option<usize> {
    if !cx.view.guides || !matches!(cx.tool, Tool::Hand | Tool::SelectAnnot | Tool::SelectText) {
        return None;
    }
    tab.guides.iter().position(|g| {
        if g.vertical {
            (pos.rect.left() + g.pos * pos.scale - p.x).abs() <= 3.0
        } else {
            (pos.rect.top() + g.pos * pos.scale - p.y).abs() <= 3.0
        }
    })
}

/// Changes the zoom keeping the content under the screen point `anchor`.
fn zoom_at(tab: &mut DocTab, zoom: f32, anchor: Pos2) {
    let old = tab.scale();
    tab.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
    tab.zoom_mode = ZoomMode::Custom;
    let factor = tab.scale() / old;
    let content_pt = anchor - tab.canvas_origin;
    let offset = content_pt * factor - (content_pt + tab.content_shift - tab.scroll_offset);
    tab.pending_scroll = Some(ScrollRequest::Offset(offset.max(Vec2::ZERO)));
}

fn start_drag(
    tab: &mut DocTab,
    cx: &CanvasCtx,
    page: Option<PageOnScreen>,
    start: Pos2,
    tol: f32,
    shift_down: bool,
) -> Interaction {
    if cx.tool == Tool::DynamicZoom {
        return Interaction::DynamicZoom { anchor: start };
    }
    let Some(pos) = page else {
        return if cx.tool == Tool::Hand { Interaction::Pan } else { Interaction::None };
    };
    if let Some(index) = guide_at(tab, cx, &pos, start) {
        return Interaction::Guide { index };
    }
    // A handle of the selected comment: change its size or a point.
    if let Some(grip) = grip_at(tab, &pos, start)
        && let Some(annot) = tab.selected_annot
    {
        return Interaction::ReshapeAnnot { annot, grip, current: start };
    }
    let pdf = pos.pdf_at(start);
    let v = pos.view_at(start);
    if cx.view.snap.is_some_and(|s| s.2) {
        // Reads the page objects once; dragging then snaps to their edges.
        let _ = tab.objects(pos.page);
    }
    let snapped = snap_point(tab, cx, pos.page, v);
    // While a polygon is being drawn, a press on an existing comment is
    // another corner, not a move.
    if tab.polygon.is_empty()
        && let Some(i) = own_kind_at(tab, cx, pos.page, pdf, tol)
        && tab.annotations(pos.page).get(i).is_some_and(|a| a.kind.is_movable())
    {
        let annot = AnnotRef { page: pos.page, index: i };
        // Dragging a part of the selection moves all of it.
        if !tab.selected_annots().contains(&annot) {
            tab.selected_annot = Some(annot);
        }
        return Interaction::MoveAnnot { annot, delta: Vec2::ZERO };
    }
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
                    if !tab.selected_annots().contains(&annot) {
                        tab.selected_annot = Some(annot);
                    }
                    Interaction::MoveAnnot { annot, delta: Vec2::ZERO }
                }
                _ if cx.tool == Tool::Hand => Interaction::Pan,
                // "Anmerkungen bearbeiten": a rectangle selects the comments in it.
                None if cx.tool == Tool::SelectAnnot => {
                    Interaction::Marquee { page: pos.page, start: v, current: v, add: shift_down }
                }
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
            Gesture::Rect | Gesture::Line => {
                Interaction::Shape { page: pos.page, start: snapped, current: snapped, points: vec![snapped] }
            }
            Gesture::Freehand => Interaction::Shape { page: pos.page, start: v, current: v, points: vec![v] },
            Gesture::Vertices => Interaction::Vertex { page: pos.page, start: v, current: v },
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
                } else if cx.tool.gesture() != Gesture::Freehand {
                    v = snap_point(tab, cx, *page, v);
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
        Interaction::Marquee { page, current, .. } | Interaction::Vertex { page, current, .. } => {
            if let Some(pos) = on_screen.iter().find(|p| p.page == *page) {
                *current = pos.view_at(cur.clamp(pos.rect.min, pos.rect.max));
            }
        }
        Interaction::ReshapeAnnot { annot, current, .. } => {
            if let Some(pos) = on_screen.iter().find(|p| p.page == annot.page) {
                *current = cur.clamp(pos.rect.min, pos.rect.max);
            }
        }
        Interaction::DynamicZoom { anchor } => {
            // Up zooms in, down zooms out.
            let factor = (-delta.y * 0.01).exp();
            let anchor = *anchor;
            zoom_at(tab, tab.zoom * factor, anchor);
        }
        Interaction::Guide { .. } | Interaction::None => {}
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
            if cx.tool == Tool::TextCorrection
                && let (Some(a), Some(b)) = (anchor, end_char)
            {
                // The keys decide what happens with the selection.
                tab.selected_annot = None;
                tab.selection = Some(TextSelection { page, range: TextRange::new(a, b) });
                cx.events.push(CanvasEvent::Status(cx.t.hint_correction.into()));
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
        Interaction::ReshapeAnnot { annot, grip, current } => {
            let Some(pos) = on_screen.iter().find(|p| p.page == annot.page) else { return };
            let Some(a) = tab.annotations(annot.page).get(annot.index).cloned() else { return };
            let reshape = match grip {
                Grip::Box(i) => {
                    let r = reshaped_box(pos.pdf_rect_to_screen(&a.rect), i, current);
                    if r.width() < 4.0 || r.height() < 4.0 {
                        return;
                    }
                    crate::pdf_edit::Reshape::Rect(
                        pos.view_rect_to_pdf(pos.view_at(r.min), pos.view_at(r.max)),
                    )
                }
                Grip::Point(index) => crate::pdf_edit::Reshape::Point { index, to: pos.pdf_at(current) },
            };
            // A measurement shows its new value.
            let contents = tab.selected_details().filter(|d| d.measure).and_then(|d| {
                let mut pts = d.points.clone();
                if let (Grip::Point(i), crate::pdf_edit::Reshape::Point { to, .. }) = (grip, reshape) {
                    *pts.get_mut(i)? = to;
                }
                Some(match a.kind {
                    AnnotKind::Line if pts.len() >= 2 => {
                        format_measure(pts[0].distance(pts[1]) * PT_TO_MM, false, cx.decimal_comma)
                    }
                    AnnotKind::Polyline => {
                        format_measure(path_length(&pts) * PT_TO_MM, false, cx.decimal_comma)
                    }
                    AnnotKind::Polygon => {
                        format_measure(polygon_area(&pts) * PT_TO_MM * PT_TO_MM, true, cx.decimal_comma)
                    }
                    _ => return None,
                })
            });
            cx.events.push(CanvasEvent::Reshape { annot, reshape, contents });
        }
        Interaction::Vertex { page, start, current } => {
            if tab.polygon.is_empty() || tab.polygon_page != page {
                add_vertex(tab, cx, on_screen, page, start);
            }
            add_vertex(tab, cx, on_screen, page, current);
        }
        Interaction::Marquee { page, start, current, add } => {
            let Some(pos) = on_screen.iter().find(|p| p.page == page) else { return };
            if start.distance(current) * scale < 3.0 {
                return;
            }
            let area = pos.view_rect_to_pdf(start, current).normalized();
            let mut chosen = if add { tab.selected_annots() } else { Vec::new() };
            chosen.retain(|a| a.page == page);
            for (index, a) in tab.annotations(page).iter().enumerate() {
                // Comments whose centre lies in the rectangle.
                let r = AnnotRef { page, index };
                if area.contains(a.rect.normalized().center()) && a.kind.is_movable() && !chosen.contains(&r)
                {
                    chosen.push(r);
                }
            }
            tab.select_annots(chosen);
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
        Tool::FieldDate => (100.0, 20.0),
        Tool::FieldBarcode => (80.0, 80.0),
        Tool::FieldImage => (120.0, 90.0),
        Tool::Video | Tool::Model3d => (160.0, 90.0),
        Tool::AudioClip => (120.0, 40.0),
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
        Tool::Video | Tool::AudioClip | Tool::Model3d => {
            cx.events.push(CanvasEvent::AddMedia { page, rect, tool })
        }
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

/// One more corner for the polygon tools. A click on the last corner (a
/// double click, however slow) finishes; for closed shapes a click on the
/// first corner too.
fn add_vertex(tab: &mut DocTab, cx: &mut CanvasCtx, on_screen: &[PageOnScreen], page: usize, view: PointF) {
    if tab.polygon_page != page {
        tab.polygon.clear();
        tab.polygon_page = page;
    }
    let scale = tab.scale();
    let closed = matches!(cx.tool, Tool::Polygon | Tool::Cloud | Tool::MeasureArea);
    let need = if closed { 3 } else { 2 };
    let near = |a: PointF| a.distance(view) * scale < 6.0;
    let n = tab.polygon.len();
    let on_last = n > 0 && near(tab.polygon[n - 1]);
    if n >= need && (on_last || (closed && near(tab.polygon[0]))) {
        finish_polygon(tab, cx, on_screen);
    } else if !on_last {
        tab.polygon.push(view);
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
    if tab.polygon.is_empty()
        && let Some(index) = own_kind_at(tab, cx, pos.page, pdf, tol)
    {
        tab.selected_annot = Some(AnnotRef { page: pos.page, index });
        return;
    }
    match tool {
        Tool::Hand => click_hand(tab, cx, &pos, pdf, tol),
        Tool::ContentHighlight => match tab.doc.object_at(pos.page, pdf) {
            Ok(Some(rect)) => {
                let mut annot = new_annot(cx, AnnotKind::Highlight);
                annot.quads = vec![rect];
                cx.events.push(CanvasEvent::Create { page: pos.page, annot: Box::new(annot) });
            }
            _ => cx.events.push(CanvasEvent::Status(cx.t.no_content_here.into())),
        },
        Tool::SelectAnnot => {
            let hit = annotation_at(tab, pos.page, pdf, tol).map(|index| AnnotRef { page: pos.page, index });
            match hit {
                // Shift+click adds a comment to the selection or removes it.
                Some(r) if shift => {
                    let mut chosen = tab.selected_annots();
                    chosen.retain(|a| a.page == r.page);
                    if let Some(i) = chosen.iter().position(|a| *a == r) {
                        chosen.remove(i);
                    } else {
                        chosen.push(r);
                    }
                    tab.select_annots(chosen);
                }
                _ => tab.select_annots(hit.into_iter().collect()),
            }
        }
        Tool::SelectFields => {
            tab.selected_annot =
                widget_at(tab, pos.page, pdf).map(|index| AnnotRef { page: pos.page, index });
        }
        // Form fields can be filled in with the text tool too.
        Tool::SelectText => {
            if !fill_field_at(tab, cx, &pos, pdf) {
                tab.selection = None;
            }
        }
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
        Tool::SoundNote => {
            let rect = pos.view_rect_to_pdf(view, PointF::new(view.x + 20.0, view.y + 20.0));
            cx.events.push(CanvasEvent::AddMedia { page: pos.page, rect, tool });
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
        t if t.gesture() == Gesture::Vertices => add_vertex(tab, cx, on_screen, pos.page, view),
        t if t.gesture() == Gesture::Rect
            && !matches!(t, Tool::Snapshot | Tool::Link | Tool::Redact | Tool::AreaHighlight) =>
        {
            finish_shape(tab, cx, &pos, view, view, Vec::new(), tab.scale());
        }
        _ => {}
    }
}

fn click_hand(tab: &mut DocTab, cx: &mut CanvasCtx, pos: &PageOnScreen, pdf: PointF, tol: f32) {
    if fill_field_at(tab, cx, pos, pdf) {
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

/// Filling in: a click on a form field edits, ticks or presses it.
/// Returns false if there is no (writable) field at `pdf`.
fn fill_field_at(tab: &mut DocTab, cx: &mut CanvasCtx, pos: &PageOnScreen, pdf: PointF) -> bool {
    if let Some(field) = tab.fields(pos.page).iter().rev().find(|f| f.rect.contains(pdf)).cloned()
        && !field.read_only
    {
        match field.kind {
            FieldKind::Text { .. } => tab.field_edit = field_editor(&field),
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
            FieldKind::PushButton => {
                cx.events.push(CanvasEvent::PressButton { page: field.page, annot_index: field.annot_index })
            }
            _ => cx.events.push(CanvasEvent::Status(cx.t.field_not_supported.into())),
        }
        return true;
    }
    false
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
                } else if kind == Some(AnnotKind::Media) {
                    cx.events.push(CanvasEvent::PlayMedia(annot));
                } else {
                    cx.events.push(CanvasEvent::ShowProperties);
                }
            }
        }
        // Form designer: a double click shows the field's properties.
        Tool::SelectFields => {
            if let Some(index) = widget_at(tab, pos.page, pdf) {
                tab.selected_annot = Some(AnnotRef { page: pos.page, index });
                cx.events.push(CanvasEvent::ShowProperties);
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
    // Tab (Shift+Tab) goes on to the next (previous) text field.
    let (tab_key, back) = ui.input(|i| (i.key_pressed(Key::Tab) && !i.modifiers.command, i.modifiers.shift));
    if response.lost_focus() || enter || tab_key {
        let edit = tab.field_edit.take().expect("field edit present");
        let (page, index) = (edit.page, edit.annot_index);
        cx.events.push(CanvasEvent::CommitField { page, annot_index: index, value: edit.value });
        if tab_key && let Some(next) = next_text_field(tab, page, index, back) {
            if next.page != page {
                tab.jump_to_page(next.page);
            }
            tab.field_edit = field_editor(&next);
        }
    }
}

/// The editor for a text field (None for other kinds).
fn field_editor(field: &crate::model::FormFieldInfo) -> Option<FieldEdit> {
    match field.kind {
        FieldKind::Text { multiline, password } if !field.read_only => Some(FieldEdit {
            page: field.page,
            annot_index: field.annot_index,
            value: field.value.clone(),
            multiline,
            password,
            rect: field.rect,
            focus_requested: false,
        }),
        _ => None,
    }
}

/// The text field after (before) widget `index` on `page`, in the order of
/// the pages and of the fields on each page; wraps around.
fn next_text_field(
    tab: &mut DocTab,
    page: usize,
    index: usize,
    back: bool,
) -> Option<crate::model::FormFieldInfo> {
    let mut list = Vec::new();
    for p in 0..tab.doc.page_count().min(2000) {
        list.extend(
            tab.fields(p)
                .iter()
                .filter(|f| matches!(f.kind, FieldKind::Text { .. }) && !f.read_only)
                .cloned(),
        );
    }
    let at = list.iter().position(|f| f.page == page && f.annot_index == index)?;
    let n = list.len();
    let next = if back { (at + n - 1) % n } else { (at + 1) % n };
    (next != at).then(|| list[next].clone())
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
