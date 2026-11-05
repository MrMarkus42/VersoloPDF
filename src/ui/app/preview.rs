//! Page previews inside dialogs (with the page strip navigator below).
//!
//! Every preview can be zoomed: mouse wheel at the pointer, the small
//! buttons in the corner; dragging moves the zoomed page (with the right or
//! middle button in previews whose left button edits something), a double
//! click fits the page again.

use egui::{Color32, Pos2, Rect, Response, Sense, Stroke, StrokeKind, TextureHandle, Ui, Vec2, vec2};

use super::VersoloPdfApp;
use crate::Document;
use crate::geometry::{PageGeometry, PointF};

/// Number of independent previews a dialog can show at once.
pub const SLOTS: usize = 2;

/// Largest zoom factor (relative to the whole page).
const MAX_ZOOM: f32 = 8.0;

/// Rendered preview kept between frames, with its zoom.
pub struct PreviewCache {
    /// Document instance, revision ^ salt, page, pixel size of the picture.
    key: (u64, u64, usize, u32, u32),
    texture: Option<TextureHandle>,
    /// 1 = the whole page fits.
    zoom: f32,
    /// Point of the page (0..1 of width and height) in the middle.
    center: Vec2,
    /// The start view (see `PreviewOpts::focus`) is set.
    homed: bool,
}

impl Default for PreviewCache {
    fn default() -> Self {
        PreviewCache {
            key: (0, 0, usize::MAX, 0, 0),
            texture: None,
            zoom: 1.0,
            center: vec2(0.5, 0.5),
            homed: false,
        }
    }
}

/// How a preview behaves.
#[derive(Debug, Clone, Copy)]
pub struct PreviewOpts {
    /// German texts for the zoom buttons.
    pub de: bool,
    /// The dialog uses the left button in the preview (handles, lines): the
    /// zoomed page then moves with the right or middle button.
    pub interactive: bool,
    /// Zoom with wheel and buttons (off for small thumbnails).
    pub zoomable: bool,
    /// Start view: the page as wide as the preview, this height (0 = top,
    /// 1 = bottom) in the middle; for headers and footers. A double click
    /// returns to it. `None`: the whole page.
    pub focus: Option<f32>,
}

impl PreviewOpts {
    pub fn new(de: bool) -> Self {
        PreviewOpts { de, interactive: false, zoomable: true, focus: None }
    }

    pub fn focus(mut self, y: f32) -> Self {
        self.focus = Some(y);
        self
    }

    pub fn interactive(mut self) -> Self {
        self.interactive = true;
        self
    }

    pub fn fixed(mut self) -> Self {
        self.zoomable = false;
        self
    }
}

/// Where a preview was drawn (used for interactive previews, e.g. cropping).
#[allow(dead_code)]
pub struct PreviewArea {
    /// Screen rectangle of the page (larger than the preview when zoomed).
    pub rect: Rect,
    /// Geometry of the page (PDF <-> view at scale 1).
    pub geometry: PageGeometry,
    /// Screen pixels per view unit (point).
    pub scale: f32,
    pub response: Response,
    /// Painter clipped to the preview; overlays use it so a zoomed page
    /// does not spill over the dialog.
    pub painter: egui::Painter,
}

#[allow(dead_code)]
impl PreviewArea {
    /// Maps a PDF point to the screen.
    pub fn to_screen(&self, p: PointF) -> egui::Pos2 {
        let v = self.geometry.pdf_to_view(p);
        self.rect.min + vec2(v.x, v.y) * self.scale
    }

    /// Maps a screen position to PDF space.
    pub fn to_pdf(&self, pos: egui::Pos2) -> PointF {
        let v = (pos - self.rect.min) / self.scale;
        self.geometry.view_to_pdf(PointF::new(v.x, v.y))
    }
}

impl VersoloPdfApp {
    /// Preview of `page` of the active document.
    pub(super) fn page_preview(
        &mut self,
        ui: &mut Ui,
        slot: usize,
        page: usize,
        max: Vec2,
        interactive: bool,
    ) -> Option<PreviewArea> {
        let opts = PreviewOpts::new(self.settings.language == crate::ui::i18n::Language::German);
        let opts = if interactive { opts.interactive() } else { opts };
        let tab = self.tabs.get(self.active)?;
        doc_preview(ui, &mut self.preview[slot], &tab.doc, tab.id, page, max, opts)
    }

    /// Forgets rendered previews (when a dialog closes).
    pub(super) fn clear_previews(&mut self) {
        self.preview = Default::default();
    }
}

/// Draws `page` of `doc` into a grey area of `max`; `salt` separates
/// documents with the same revision number.
pub fn doc_preview(
    ui: &mut Ui,
    cache: &mut Option<PreviewCache>,
    doc: &Document,
    salt: u64,
    page: usize,
    max: Vec2,
    opts: PreviewOpts,
) -> Option<PreviewArea> {
    let geometry = *doc.geometry(page)?;
    let (vw, vh) = geometry.view_size();
    let (bg, response) = ui.allocate_exact_size(max, Sense::click_and_drag());
    let painter = ui.painter_at(bg);
    painter.rect_filled(bg, 0.0, Color32::from_gray(128));
    let margin = 12.0;
    let fit = ((max.x - 2.0 * margin) / vw.max(1.0)).min((max.y - 2.0 * margin) / vh.max(1.0)).max(0.01);
    let c = cache.get_or_insert_with(PreviewCache::default);
    let size_at = |zoom: f32| vec2(vw * fit * zoom, vh * fit * zoom);
    let zoomable = opts.zoomable && max.x >= 160.0;
    // The start view: whole page, or full width around `focus`.
    let home = |c: &mut PreviewCache| match opts.focus {
        Some(y) if zoomable => {
            c.zoom = ((max.x - 2.0 * margin) / (vw * fit)).clamp(1.0, MAX_ZOOM);
            c.center = vec2(0.5, y);
        }
        _ => {
            c.zoom = 1.0;
            c.center = vec2(0.5, 0.5);
        }
    };
    if !c.homed {
        home(c);
        c.homed = true;
    }
    if zoomable {
        // Wheel: zoom around the pointer.
        let wheel = if response.hovered() { ui.input(|i| i.smooth_scroll_delta.y) } else { 0.0 };
        if wheel != 0.0 {
            let ptr = response.hover_pos().unwrap_or(bg.center());
            zoom_at(c, bg, ptr, c.zoom * (wheel / 200.0).exp(), size_at);
            // The dialog must not scroll at the same time.
            ui.input_mut(|i| i.smooth_scroll_delta = Vec2::ZERO);
        }
        let pan = response.dragged_by(egui::PointerButton::Middle)
            || response.dragged_by(egui::PointerButton::Secondary)
            || (!opts.interactive && response.dragged_by(egui::PointerButton::Primary));
        if pan && c.zoom > 1.0 {
            c.center -= response.drag_delta() / size_at(c.zoom);
        }
        if response.double_clicked() {
            home(c);
        }
    } else {
        c.zoom = 1.0;
    }
    // Keep the page in view: centred where it is smaller than the area.
    let size = size_at(c.zoom);
    let half = vec2(bg.width() / 2.0 / size.x, bg.height() / 2.0 / size.y);
    c.center.x = if half.x >= 0.5 { 0.5 } else { c.center.x.clamp(half.x, 1.0 - half.x) };
    c.center.y = if half.y >= 0.5 { 0.5 } else { c.center.y.clamp(half.y, 1.0 - half.y) };
    let rect = Rect::from_min_size(bg.center() - c.center * size, size);

    // The picture is rendered at 1, 2, 4 or 8 times the fitted size, so
    // zooming with the wheel does not render every frame.
    let ppp = ui.ctx().pixels_per_point();
    let step = c.zoom.log2().ceil().exp2().clamp(1.0, MAX_ZOOM);
    let full = size_at(1.0) * step * ppp;
    let cap = (4096.0 / full.x.max(full.y)).min(1.0);
    let (pw, ph) = (((full.x * cap).round() as u32).max(1), ((full.y * cap).round() as u32).max(1));
    let key = (doc.instance(), doc.revision().wrapping_mul(1_000_003) ^ salt, page, pw, ph);
    if c.key != key {
        c.key = key;
        c.texture = match doc.render_page(page, pw, ph) {
            Ok(img) => {
                let color = egui::ColorImage::from_rgba_unmultiplied(
                    [img.width as usize, img.height as usize],
                    &img.rgba,
                );
                Some(ui.ctx().load_texture("dialog-preview", color, egui::TextureOptions::LINEAR))
            }
            Err(err) => {
                log::warn!("preview failed: {err}");
                None
            }
        };
    }
    painter.rect_filled(rect.translate(vec2(2.0, 2.0)), 0.0, Color32::from_black_alpha(60));
    match &c.texture {
        Some(t) => {
            let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            painter.image(t.id(), rect, uv, Color32::WHITE);
        }
        None => {
            painter.rect_filled(rect, 0.0, Color32::WHITE);
        }
    }
    painter.rect_stroke(rect, 0.0, Stroke::new(1.0, Color32::from_gray(90)), StrokeKind::Outside);
    if zoomable {
        zoom_buttons(ui, &painter, bg, c, opts.de, size_at);
    }
    Some(PreviewArea { rect, geometry, scale: fit * c.zoom, response, painter })
}

/// Sets the zoom, keeping the page point under `at` where it is.
fn zoom_at(c: &mut PreviewCache, bg: Rect, at: Pos2, zoom: f32, size_at: impl Fn(f32) -> Vec2) {
    let old = size_at(c.zoom);
    let min = bg.center() - c.center * old;
    let u = (at - min) / old;
    c.zoom = zoom.clamp(1.0, MAX_ZOOM);
    let new = size_at(c.zoom);
    c.center = (bg.center() - (at - u * new)) / new;
}

/// "−", "+" and the zoom factor (click: fit) in the upper right corner.
fn zoom_buttons(
    ui: &Ui,
    painter: &egui::Painter,
    bg: Rect,
    c: &mut PreviewCache,
    de: bool,
    size_at: impl Fn(f32) -> Vec2 + Copy,
) {
    let font = egui::FontId::proportional(12.0);
    let label = format!("{:.0} %", c.zoom * 100.0);
    let text_w = |t: &str| ui.painter().layout_no_wrap(t.to_string(), font.clone(), Color32::BLACK).size().x;
    let h = (ui.fonts_mut(|f| f.row_height(&font)) + 4.0).max(18.0);
    let widths = [h, h, (text_w(&label) + 12.0).max(44.0)];
    let mut x = bg.right() - 4.0 - widths.iter().sum::<f32>() - 4.0;
    let items: [(&str, &str); 3] = [
        ("−", if de { "Verkleinern" } else { "Zoom out" }),
        ("+", if de { "Vergrößern" } else { "Zoom in" }),
        (
            label.as_str(),
            if de {
                "Ganze Seite. Mausrad zoomt, Ziehen verschiebt, Doppelklick passt die Seite ein."
            } else {
                "Whole page. The mouse wheel zooms, dragging moves, a double click fits the page."
            },
        ),
    ];
    for (i, ((text, tip), w)) in items.into_iter().zip(widths).enumerate() {
        let r = Rect::from_min_size(egui::pos2(x, bg.top() + 4.0), vec2(w, h));
        x += w + 2.0;
        let resp = ui.interact(
            r,
            ui.id().with(("preview-zoom", bg.min.x as i32, bg.min.y as i32, i)),
            Sense::click(),
        );
        let fill =
            if resp.hovered() { Color32::from_white_alpha(250) } else { Color32::from_white_alpha(215) };
        painter.rect(r, 3.0, fill, Stroke::new(1.0, Color32::from_gray(110)), StrokeKind::Inside);
        painter.text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(12.0),
            Color32::from_gray(30),
        );
        let clicked = resp.on_hover_text(tip).clicked();
        if clicked {
            match i {
                0 => zoom_at(c, bg, bg.center(), c.zoom / 1.5, size_at),
                1 => zoom_at(c, bg, bg.center(), c.zoom * 1.5, size_at),
                _ => c.zoom = 1.0,
            }
        }
    }
}

/// Page strip "1 2 3 … ‹ › |‹ ›| [n]" below previews. `page` is 0-based.
pub fn page_strip(ui: &mut Ui, page: &mut usize, count: usize) -> bool {
    let before = *page;
    if count == 0 {
        return false;
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        // A window of up to 15 page buttons around the current page.
        let window = 15usize.min(count);
        let start = page.saturating_sub(window / 2).min(count - window);
        for p in start..start + window {
            let selected = p == *page;
            let text = egui::RichText::new((p + 1).to_string()).size(11.0);
            let text = if selected { text.color(Color32::WHITE) } else { text };
            let mut b = egui::Button::new(text).corner_radius(0.0).min_size(vec2(18.0, 18.0));
            if selected {
                b = b.fill(Color32::from_gray(95));
            }
            if ui.add(b).clicked() {
                *page = p;
            }
        }
        ui.add_space(6.0);
        if ui.small_button("‹").clicked() {
            *page = page.saturating_sub(1);
        }
        if ui.small_button("›").clicked() {
            *page = (*page + 1).min(count - 1);
        }
        if ui.small_button("|‹").clicked() {
            *page = 0;
        }
        if ui.small_button("›|").clicked() {
            *page = count - 1;
        }
        ui.add_space(6.0);
        let mut n = *page + 1;
        if ui.add(egui::DragValue::new(&mut n).range(1..=count)).changed() {
            *page = n - 1;
        }
    });
    *page != before
}
