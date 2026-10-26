//! Ribbon style buttons and small helpers.

use egui::{
    Align2, Color32, FontId, Rect, Response, Sense, Stroke, StrokeKind, TextWrapMode, Ui, Vec2, WidgetInfo,
    WidgetType, vec2,
};

use crate::model::Rgba;
use crate::ui::icons::{self, Icon};
use crate::ui::theme;

fn button_frame(ui: &Ui, rect: Rect, response: &Response, selected: bool) {
    let visuals = ui.visuals();
    let (fill, stroke) = if selected {
        (theme::ACCENT.gamma_multiply(0.22), Stroke::new(1.0, theme::ACCENT.gamma_multiply(0.7)))
    } else if response.is_pointer_button_down_on() {
        (visuals.widgets.active.weak_bg_fill, visuals.widgets.active.bg_stroke)
    } else if response.hovered() {
        (visuals.widgets.hovered.weak_bg_fill, visuals.widgets.hovered.bg_stroke)
    } else {
        return;
    };
    ui.painter().rect(rect, 3.0, fill, stroke, StrokeKind::Inside);
}

fn icon_tint(ui: &Ui, enabled: bool) -> Color32 {
    let c = theme::icon_color(ui.visuals().dark_mode);
    if enabled { c } else { c.gamma_multiply(0.35) }
}

/// Large ribbon button: icon on top, label below.
pub fn big_button(ui: &mut Ui, icon: Icon, label: &str, selected: bool, enabled: bool) -> Response {
    // Wide enough for the longest word so labels only wrap between words.
    let font = FontId::proportional(11.5);
    let widest_word = label
        .split_whitespace()
        .map(|w| ui.painter().layout_no_wrap(w.to_string(), font.clone(), Color32::WHITE).size().x)
        .fold(0.0, f32::max);
    let size = vec2((widest_word + 10.0).max(54.0), 62.0);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    response.widget_info(|| WidgetInfo::selected(WidgetType::Button, enabled, selected, label));
    if ui.is_rect_visible(rect) {
        button_frame(ui, rect, &response, selected);
        let icon_rect = Rect::from_center_size(rect.center_top() + vec2(0.0, 18.0), Vec2::splat(26.0));
        let painter = ui.painter_at(rect);
        let mut alpha_painter = painter.clone();
        if !enabled {
            alpha_painter.multiply_opacity(0.4);
        }
        icons::paint(&alpha_painter, icon_rect, icon, icon_tint(ui, enabled));
        let text_color = if enabled { ui.visuals().text_color() } else { ui.visuals().weak_text_color() };
        let galley = ui.painter().layout(label.to_string(), font, text_color, size.x - 4.0);
        let pos = rect.center_top() + vec2(-galley.size().x / 2.0, 35.0);
        ui.painter().galley(pos, galley, text_color);
    }
    response
}

fn paint_chevron(ui: &Ui, center: egui::Pos2, color: Color32) {
    let w = 3.5;
    ui.painter().line(
        vec![center + vec2(-w, -1.5), center + vec2(0.0, 2.0), center + vec2(w, -1.5)],
        Stroke::new(1.3, color),
    );
}

/// Large ribbon button that opens a drop-down menu (label with ▾ below).
pub fn big_menu_button(ui: &mut Ui, icon: Icon, label: &str, enabled: bool, open: bool) -> Response {
    let font = FontId::proportional(11.5);
    let widest_word = label
        .split_whitespace()
        .map(|w| ui.painter().layout_no_wrap(w.to_string(), font.clone(), Color32::WHITE).size().x)
        .fold(0.0, f32::max);
    let size = vec2((widest_word + 10.0).max(54.0), 64.0);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, label));
    if ui.is_rect_visible(rect) {
        button_frame(ui, rect, &response, open);
        let icon_rect = Rect::from_center_size(rect.center_top() + vec2(0.0, 17.0), Vec2::splat(26.0));
        let mut painter = ui.painter_at(rect);
        if !enabled {
            painter.multiply_opacity(0.4);
        }
        icons::paint(&painter, icon_rect, icon, icon_tint(ui, enabled));
        let text_color = if enabled { ui.visuals().text_color() } else { ui.visuals().weak_text_color() };
        let galley = ui.painter().layout(label.to_string(), font, text_color, size.x - 4.0);
        let pos = rect.center_top() + vec2(-galley.size().x / 2.0, 33.0);
        let text_bottom = pos.y + galley.size().y;
        ui.painter().galley(pos, galley, text_color);
        paint_chevron(
            ui,
            egui::pos2(rect.center().x, (text_bottom + 4.0).min(rect.bottom() - 3.0)),
            text_color,
        );
    }
    response
}

/// Small ribbon button with ▾ that opens a drop-down menu.
pub fn small_menu_button(ui: &mut Ui, icon: Icon, label: &str, enabled: bool, open: bool) -> Response {
    let font = FontId::proportional(12.5);
    let text_w = ui.painter().layout_no_wrap(label.to_string(), font.clone(), Color32::WHITE).size().x;
    let size = vec2(text_w + 42.0, 21.0);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, label));
    if ui.is_rect_visible(rect) {
        button_frame(ui, rect, &response, open);
        let mut painter = ui.painter_at(rect);
        if !enabled {
            painter.multiply_opacity(0.4);
        }
        icons::paint(
            &painter,
            Rect::from_min_size(rect.min + vec2(3.0, 2.5), Vec2::splat(16.0)),
            icon,
            icon_tint(ui, enabled),
        );
        let color = if enabled { ui.visuals().text_color() } else { ui.visuals().weak_text_color() };
        ui.painter().text(rect.left_center() + vec2(24.0, 0.0), Align2::LEFT_CENTER, label, font, color);
        paint_chevron(ui, egui::pos2(rect.right() - 8.0, rect.center().y), color);
    }
    response
}

/// One row of a drop-down menu: icon, label, optional shortcut and ▸ for
/// sub menus.
pub fn menu_item(
    ui: &mut Ui,
    icon: Icon,
    label: &str,
    shortcut: Option<&str>,
    enabled: bool,
    checked: bool,
    submenu: bool,
) -> Response {
    let font = FontId::proportional(13.0);
    let text_w = ui.painter().layout_no_wrap(label.to_string(), font.clone(), Color32::WHITE).size().x;
    let sc_w = shortcut.map_or(0.0, |s| {
        ui.painter().layout_no_wrap(s.to_string(), FontId::proportional(12.0), Color32::WHITE).size().x + 24.0
    });
    // Rows share one width (popups offer a large available width).
    let width = (text_w + sc_w + 52.0).max(ui.available_width().min(330.0)).max(220.0);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(vec2(width, 26.0), sense);
    response.widget_info(|| WidgetInfo::selected(WidgetType::Button, enabled, checked, label));
    if ui.is_rect_visible(rect) {
        if enabled && response.hovered() {
            ui.painter().rect_filled(rect, 2.0, theme::ACCENT.gamma_multiply(0.18));
        }
        let mut painter = ui.painter_at(rect);
        if !enabled {
            painter.multiply_opacity(0.4);
        }
        if checked {
            ui.painter().text(
                rect.left_center() + vec2(4.0, 0.0),
                Align2::LEFT_CENTER,
                "✔",
                FontId::proportional(11.0),
                ui.visuals().text_color(),
            );
        }
        icons::paint(
            &painter,
            Rect::from_min_size(rect.min + vec2(16.0, 4.0), Vec2::splat(18.0)),
            icon,
            icon_tint(ui, enabled),
        );
        let color = if enabled { ui.visuals().text_color() } else { ui.visuals().weak_text_color() };
        ui.painter().text(rect.left_center() + vec2(42.0, 0.0), Align2::LEFT_CENTER, label, font, color);
        if let Some(sc) = shortcut {
            ui.painter().text(
                rect.right_center() - vec2(10.0, 0.0),
                Align2::RIGHT_CENTER,
                sc.to_uppercase(),
                FontId::proportional(11.5),
                ui.visuals().weak_text_color(),
            );
        }
        if submenu {
            ui.painter().text(
                rect.right_center() - vec2(8.0, 0.0),
                Align2::RIGHT_CENTER,
                "›",
                FontId::proportional(15.0),
                color,
            );
        }
    }
    response
}

/// Small ribbon button with icon and label in one row.
pub fn small_button(ui: &mut Ui, icon: Icon, label: &str, selected: bool, enabled: bool) -> Response {
    let font = FontId::proportional(12.5);
    let text_w = ui.painter().layout_no_wrap(label.to_string(), font.clone(), Color32::WHITE).size().x;
    let size = vec2(text_w + 30.0, 21.0);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    response.widget_info(|| WidgetInfo::selected(WidgetType::Button, enabled, selected, label));
    if ui.is_rect_visible(rect) {
        button_frame(ui, rect, &response, selected);
        let icon_rect = Rect::from_min_size(rect.min + vec2(3.0, 2.5), Vec2::splat(16.0));
        let painter = ui.painter_at(rect);
        let mut painter = painter;
        if !enabled {
            painter.multiply_opacity(0.4);
        }
        icons::paint(&painter, icon_rect, icon, icon_tint(ui, enabled));
        let color = if enabled { ui.visuals().text_color() } else { ui.visuals().weak_text_color() };
        ui.painter().text(rect.left_center() + vec2(24.0, 0.0), Align2::LEFT_CENTER, label, font, color);
    }
    response
}

/// Square icon-only button with tooltip.
pub fn icon_button(ui: &mut Ui, icon: Icon, tooltip: &str, selected: bool, enabled: bool) -> Response {
    let size = Vec2::splat(24.0);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    response.widget_info(|| WidgetInfo::selected(WidgetType::Button, enabled, selected, tooltip));
    if ui.is_rect_visible(rect) {
        button_frame(ui, rect, &response, selected);
        let painter = ui.painter_at(rect);
        let mut painter = painter;
        if !enabled {
            painter.multiply_opacity(0.4);
        }
        icons::paint(&painter, rect.shrink(4.0), icon, icon_tint(ui, enabled));
    }
    response.on_hover_text(tooltip)
}

/// A ribbon group: contents on top, caption below, separator on the right.
pub fn group(ui: &mut Ui, caption: &str, add: impl FnOnce(&mut Ui)) {
    ui.vertical(|ui| {
        ui.set_min_height(80.0);
        ui.horizontal(|ui| {
            ui.set_min_height(64.0);
            add(ui);
        });
        ui.add(
            egui::Label::new(egui::RichText::new(caption).size(10.5).weak()).wrap_mode(TextWrapMode::Extend),
        );
    });
    ui.add(egui::Separator::default().vertical().spacing(10.0));
}

/// A colour swatch; returns the click response.
pub fn swatch(ui: &mut Ui, color: Rgba, selected: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::click());
    let c = Color32::from_rgb(color.r, color.g, color.b);
    ui.painter().rect_filled(rect.shrink(2.0), 2.0, c);
    let stroke =
        if selected { Stroke::new(2.0, theme::ACCENT) } else { Stroke::new(1.0, Color32::from_gray(140)) };
    ui.painter().rect_stroke(rect.shrink(1.0), 2.0, stroke, StrokeKind::Inside);
    response.on_hover_text(color.to_hex())
}

pub fn to_color32(c: Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}

pub fn from_color32(c: Color32) -> Rgba {
    let [r, g, b, _] = c.to_srgba_unmultiplied();
    Rgba::rgb(r, g, b)
}
