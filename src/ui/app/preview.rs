//! Page previews inside dialogs (with the page strip navigator below).

use egui::{Color32, Rect, Response, Sense, Stroke, StrokeKind, TextureHandle, Ui, Vec2, vec2};

use super::MrPdfApp;
use crate::Document;
use crate::geometry::{PageGeometry, PointF};

/// Number of independent previews a dialog can show at once.
pub const SLOTS: usize = 2;

/// Rendered preview kept between frames; re-rendered when the key changes.
pub struct PreviewCache {
    key: (u64, usize, u32, u32),
    texture: TextureHandle,
}

/// Where a preview was drawn (used for interactive previews, e.g. cropping).
#[allow(dead_code)]
pub struct PreviewArea {
    /// Screen rectangle of the page.
    pub rect: Rect,
    /// Geometry of the page (PDF <-> view at scale 1).
    pub geometry: PageGeometry,
    /// Screen pixels per view unit (point).
    pub scale: f32,
    pub response: Response,
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

impl MrPdfApp {
    /// Preview of `page` of the active document.
    pub(super) fn page_preview(
        &mut self,
        ui: &mut Ui,
        slot: usize,
        page: usize,
        max: Vec2,
    ) -> Option<PreviewArea> {
        let tab = self.tabs.get(self.active)?;
        doc_preview(ui, &mut self.preview[slot], &tab.doc, tab.id, page, max)
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
) -> Option<PreviewArea> {
    let geometry = *doc.geometry(page)?;
    let (vw, vh) = geometry.view_size();
    let (bg, response) = ui.allocate_exact_size(max, Sense::click_and_drag());
    ui.painter().rect_filled(bg, 0.0, Color32::from_gray(128));
    let margin = 12.0;
    let scale = ((max.x - 2.0 * margin) / vw.max(1.0)).min((max.y - 2.0 * margin) / vh.max(1.0)).max(0.01);
    let rect = Rect::from_center_size(bg.center(), vec2(vw * scale, vh * scale));
    let ppp = ui.ctx().pixels_per_point();
    let (pw, ph) =
        (((rect.width() * ppp).round() as u32).max(1), ((rect.height() * ppp).round() as u32).max(1));
    let key = (doc.revision().wrapping_mul(1_000_003) ^ salt, page, pw, ph);
    if cache.as_ref().is_none_or(|c| c.key != key) {
        *cache = match doc.render_page(page, pw, ph) {
            Ok(img) => {
                let color = egui::ColorImage::from_rgba_unmultiplied(
                    [img.width as usize, img.height as usize],
                    &img.rgba,
                );
                let texture = ui.ctx().load_texture("dialog-preview", color, egui::TextureOptions::LINEAR);
                Some(PreviewCache { key, texture })
            }
            Err(err) => {
                log::warn!("preview failed: {err}");
                None
            }
        };
    }
    ui.painter().rect_filled(rect.translate(vec2(2.0, 2.0)), 0.0, Color32::from_black_alpha(60));
    match cache {
        Some(c) => {
            let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            ui.painter().image(c.texture.id(), rect, uv, Color32::WHITE);
        }
        None => {
            ui.painter().rect_filled(rect, 0.0, Color32::WHITE);
        }
    }
    ui.painter().rect_stroke(rect, 0.0, Stroke::new(1.0, Color32::from_gray(90)), StrokeKind::Outside);
    Some(PreviewArea { rect, geometry, scale, response })
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
