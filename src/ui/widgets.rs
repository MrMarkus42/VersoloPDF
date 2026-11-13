//! Ribbon style buttons and small helpers.

use egui::{
    Align2, Color32, FontId, Rect, Response, Role, Sense, Stroke, StrokeKind, TextWrapMode, Ui, Vec2,
    WidgetInfo, vec2,
};

use crate::model::Rgba;
use crate::ui::icons::{self, Icon};
use crate::ui::theme;

/// How the ribbon is fitted into a narrow window, like the ribbons of
/// office programs: first small buttons lose their labels, then groups
/// collapse from the right into one drop-down button each.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RibbonFit {
    /// Narrower large buttons and less space between groups.
    pub dense: bool,
    /// Small buttons show only their icon (label as tooltip).
    pub compact: bool,
    /// Groups with this index or higher are collapsed.
    pub collapse_from: usize,
    /// Index of the next group (counted while drawing).
    pub next_group: usize,
    /// Drawing inside the drop-down of a collapsed group (full size).
    pub in_popup: bool,
    /// First icon drawn in the current group (icon of its collapsed button).
    pub first_icon: Option<Icon>,
    /// Drawing the ribbon's commands: icon buttons are as high as small
    /// buttons, so three rows fit into the fixed ribbon height.
    pub in_ribbon: bool,
}

/// Height of the contents of a ribbon group: three rows of small buttons or
/// input fields, or a large button with a two-line label. Every tab has this
/// height, so the document does not move when the tab changes.
pub const RIBBON_CONTENT_HEIGHT: f32 = 70.0;
/// Height of a ribbon group with its caption.
pub const RIBBON_GROUP_HEIGHT: f32 = RIBBON_CONTENT_HEIGHT + 14.0;

impl RibbonFit {
    /// Everything at full size (also used outside the ribbon).
    pub const FULL: RibbonFit = RibbonFit {
        dense: false,
        compact: false,
        collapse_from: usize::MAX,
        next_group: 0,
        in_popup: false,
        first_icon: None,
        in_ribbon: false,
    };

    fn shrunk(self) -> bool {
        self.compact && !self.in_popup
    }

    fn tight(self) -> bool {
        self.dense && !self.in_popup
    }
}

fn fit_id() -> egui::Id {
    egui::Id::unique("versolopdf-ribbon-fit")
}

fn too_tall_id() -> egui::Id {
    egui::Id::unique("versolopdf-ribbon-too-tall")
}

/// Caption and height of the last ribbon group whose contents were taller
/// than the ribbon (and were cut off); `None` when all fit. For tests.
pub fn ribbon_group_too_tall(ctx: &egui::Context) -> Option<String> {
    ctx.data(|d| d.get_temp(too_tall_id()))
}

/// Forgets [`ribbon_group_too_tall`] before the ribbon is drawn again.
pub fn reset_ribbon_group_too_tall(ctx: &egui::Context) {
    ctx.data_mut(|d| d.remove::<String>(too_tall_id()));
}

fn command_ran_id() -> egui::Id {
    egui::Id::unique("versolopdf-ribbon-command-ran")
}

pub fn ribbon_fit(ctx: &egui::Context) -> RibbonFit {
    ctx.data(|d| d.get_temp(fit_id())).unwrap_or(RibbonFit::FULL)
}

pub fn set_ribbon_fit(ctx: &egui::Context, fit: RibbonFit) {
    ctx.data_mut(|d| d.insert_temp(fit_id(), fit));
}

/// Called when a command runs (closes the drop-down of a collapsed group).
pub fn note_command_ran(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(command_ran_id(), true));
}

fn note_icon(ui: &Ui, icon: Icon) {
    let mut fit = ribbon_fit(ui.ctx());
    if fit.first_icon.is_none() && fit != RibbonFit::FULL {
        fit.first_icon = Some(icon);
        set_ribbon_fit(ui.ctx(), fit);
    }
}

/// Background of a ribbon button: soft tint on hover, stronger when pressed,
/// accent fill with outline when selected (active tool, open panel).
fn button_frame(ui: &Ui, rect: Rect, response: &Response, selected: bool) {
    let hovered = response.hovered();
    let (fill, stroke) = if response.is_pointer_button_down_on() {
        (theme::ACCENT.gamma_multiply(0.32), Stroke::new(1.0, theme::ACCENT.gamma_multiply(0.8)))
    } else if selected {
        let fill = theme::ACCENT.gamma_multiply(if hovered { 0.26 } else { 0.18 });
        (fill, Stroke::new(1.2, theme::ACCENT.gamma_multiply(0.85)))
    } else if hovered {
        (theme::ACCENT.gamma_multiply(0.1), Stroke::new(1.0, theme::ACCENT.gamma_multiply(0.3)))
    } else {
        return;
    };
    ui.painter().rect(rect, 5.0, fill, stroke, StrokeKind::Inside);
}

/// Accent bar under a selected large button, so the active tool stands out.
fn selected_bar(ui: &Ui, rect: Rect) {
    let bar = Rect::from_min_max(
        egui::pos2(rect.left() + 10.0, rect.bottom() - 3.5),
        egui::pos2(rect.right() - 10.0, rect.bottom() - 1.5),
    );
    ui.painter().rect_filled(bar, 1.0, theme::ACCENT);
}

/// Font and minimum width of large buttons (narrower in a dense ribbon).
fn big_metrics(ui: &Ui) -> (FontId, f32) {
    if ribbon_fit(ui.ctx()).tight() {
        (FontId::proportional(11.0), 44.0)
    } else {
        (FontId::proportional(11.5), 54.0)
    }
}

/// Width and label of a large ribbon button: wide enough for the longest
/// word (labels only wrap between words) and for at most two lines, so all
/// large buttons have the same height.
fn big_label(ui: &Ui, label: &str, color: Color32) -> (f32, std::sync::Arc<egui::Galley>) {
    let (font, min_w) = big_metrics(ui);
    let widest_word = label
        .split_whitespace()
        .map(|w| ui.painter().layout_no_wrap(w.to_string(), font.clone(), Color32::WHITE).size().x)
        .fold(0.0, f32::max);
    let mut width = (widest_word + 10.0).max(min_w);
    loop {
        let galley = ui.painter().layout(label.to_string(), font.clone(), color, width - 4.0);
        if galley.rows.len() <= 2 || width > 400.0 {
            return (width, galley);
        }
        width += 4.0;
    }
}

fn icon_tint(ui: &Ui, enabled: bool) -> Color32 {
    let c = theme::icon_color(ui.visuals().dark_mode);
    // Disabled buttons also lower the painter's opacity; the faded ink makes
    // the icon's topic colours grey as well.
    if enabled { c } else { c.gamma_multiply(0.7) }
}

/// Large ribbon button: icon on top, label below.
pub fn big_button(ui: &mut Ui, icon: Icon, label: &str, selected: bool, enabled: bool) -> Response {
    note_icon(ui, icon);
    // Wide enough for the longest word so labels only wrap between words.
    let text_color = if enabled { ui.visuals().text_color() } else { ui.visuals().weak_text_color() };
    let (width, galley) = big_label(ui, label, text_color);
    // Tall enough for two label lines in any font.
    let size = vec2(width, (31.0 + galley.size().y + 3.0).max(56.0));
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    response.widget_info(|| WidgetInfo::selected(Role::Button, enabled, selected, label));
    if ui.is_rect_visible(rect) {
        button_frame(ui, rect, &response, selected);
        if selected {
            selected_bar(ui, rect);
        }
        let icon_rect = Rect::from_center_size(rect.center_top() + vec2(0.0, 16.0), Vec2::splat(24.0));
        let painter = ui.painter_at(rect);
        let mut alpha_painter = painter.clone();
        if !enabled {
            alpha_painter.multiply_opacity(0.4);
        }
        icons::paint(&alpha_painter, icon_rect, icon, icon_tint(ui, enabled));
        let pos = rect.center_top() + vec2(-galley.size().x / 2.0, 31.0);
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

/// Large ribbon button that opens a drop-down menu (label with ⏷ below).
pub fn big_menu_button(ui: &mut Ui, icon: Icon, label: &str, enabled: bool, open: bool) -> Response {
    note_icon(ui, icon);
    let text_color = if enabled { ui.visuals().text_color() } else { ui.visuals().weak_text_color() };
    let (width, galley) = big_label(ui, label, text_color);
    // Room for the label lines and the arrow below them in any font.
    let size = vec2(width, (30.0 + galley.size().y + 8.0).max(58.0));
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    response.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, label));
    if ui.is_rect_visible(rect) {
        button_frame(ui, rect, &response, open);
        let icon_rect = Rect::from_center_size(rect.center_top() + vec2(0.0, 15.0), Vec2::splat(24.0));
        let mut painter = ui.painter_at(rect);
        if !enabled {
            painter.multiply_opacity(0.4);
        }
        icons::paint(&painter, icon_rect, icon, icon_tint(ui, enabled));
        let pos = rect.center_top() + vec2(-galley.size().x / 2.0, 30.0);
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

/// Small ribbon button with ⏷ that opens a drop-down menu.
pub fn small_menu_button(ui: &mut Ui, icon: Icon, label: &str, enabled: bool, open: bool) -> Response {
    note_icon(ui, icon);
    let shrunk = ribbon_fit(ui.ctx()).shrunk();
    let font = FontId::proportional(12.5);
    let text_w = ui.painter().layout_no_wrap(label.to_string(), font.clone(), Color32::WHITE).size().x;
    let size = if shrunk { vec2(34.0, 20.0) } else { vec2(text_w + 42.0, 20.0) };
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    response.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, label));
    if ui.is_rect_visible(rect) {
        button_frame(ui, rect, &response, open);
        let mut painter = ui.painter_at(rect);
        if !enabled {
            painter.multiply_opacity(0.4);
        }
        icons::paint(
            &painter,
            Rect::from_min_size(rect.min + vec2(3.0, 2.0), Vec2::splat(16.0)),
            icon,
            icon_tint(ui, enabled),
        );
        let color = if enabled { ui.visuals().text_color() } else { ui.visuals().weak_text_color() };
        if !shrunk {
            ui.painter().text(rect.left_center() + vec2(24.0, 0.0), Align2::LEFT_CENTER, label, font, color);
        }
        paint_chevron(ui, egui::pos2(rect.right() - 8.0, rect.center().y), color);
    }
    if shrunk { response.on_hover_text(label) } else { response }
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
    response.widget_info(|| WidgetInfo::selected(Role::Button, enabled, checked, label));
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
    note_icon(ui, icon);
    // In a narrow ribbon only the icon is shown; callers add the label as tooltip.
    let shrunk = ribbon_fit(ui.ctx()).shrunk();
    let font = FontId::proportional(12.5);
    let text_w = ui.painter().layout_no_wrap(label.to_string(), font.clone(), Color32::WHITE).size().x;
    let size = if shrunk { vec2(24.0, 20.0) } else { vec2(text_w + 30.0, 20.0) };
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    response.widget_info(|| WidgetInfo::selected(Role::Button, enabled, selected, label));
    if ui.is_rect_visible(rect) {
        button_frame(ui, rect, &response, selected);
        let icon_rect = Rect::from_min_size(rect.min + vec2(3.0, 2.0), Vec2::splat(16.0));
        let painter = ui.painter_at(rect);
        let mut painter = painter;
        if !enabled {
            painter.multiply_opacity(0.4);
        }
        icons::paint(&painter, icon_rect, icon, icon_tint(ui, enabled));
        let color = if enabled { ui.visuals().text_color() } else { ui.visuals().weak_text_color() };
        if !shrunk {
            ui.painter().text(rect.left_center() + vec2(24.0, 0.0), Align2::LEFT_CENTER, label, font, color);
        }
    }
    response
}

/// Square icon-only button with tooltip.
pub fn icon_button(ui: &mut Ui, icon: Icon, tooltip: &str, selected: bool, enabled: bool) -> Response {
    icon_button_named(ui, icon, tooltip, tooltip, selected, enabled)
}

/// Icon-only button whose name (for screen readers) differs from its
/// longer tooltip.
pub fn icon_button_named(
    ui: &mut Ui,
    icon: Icon,
    name: &str,
    tooltip: &str,
    selected: bool,
    enabled: bool,
) -> Response {
    note_icon(ui, icon);
    let size = if ribbon_fit(ui.ctx()).in_ribbon { vec2(24.0, 20.0) } else { Vec2::splat(24.0) };
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    response.widget_info(|| WidgetInfo::selected(Role::Button, enabled, selected, name));
    if ui.is_rect_visible(rect) {
        button_frame(ui, rect, &response, selected);
        let painter = ui.painter_at(rect);
        let mut painter = painter;
        if !enabled {
            painter.multiply_opacity(0.4);
        }
        let icon_rect = Rect::from_center_size(rect.center(), Vec2::splat(16.0));
        icons::paint(&painter, icon_rect, icon, icon_tint(ui, enabled));
    }
    response.on_hover_text(tooltip)
}

/// A ribbon tab name. Unlike egui's frameless buttons (whose text moves by a
/// pixel when the hover frame appears) its size and text never move.
pub fn ribbon_tab(ui: &mut Ui, text: egui::RichText, selected: bool) -> Response {
    let pad = ui.spacing().button_padding;
    let galley = egui::WidgetText::from(text).into_galley(
        ui,
        Some(TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Button,
    );
    let (rect, response) = ui.allocate_exact_size(galley.size() + 2.0 * pad, Sense::click());
    let enabled = ui.is_enabled();
    response.widget_info(|| WidgetInfo::selected(Role::Button, enabled, selected, galley.text()));
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact_selectable(&response, selected);
        let radius = ui.visuals().widgets.hovered.corner_radius;
        if selected {
            ui.painter().rect_filled(rect, radius, ui.visuals().selection.bg_fill);
        } else if response.hovered() || response.has_focus() {
            ui.painter().rect_filled(rect, radius, ui.visuals().widgets.hovered.weak_bg_fill);
        }
        ui.painter().galley(rect.min + pad, galley, visuals.text_color());
    }
    response
}

/// A ribbon group: contents on top, caption below, separator on the right.
///
/// In a narrow window the ribbon may collapse the group into one large
/// button with the group's name; its drop-down shows the full group.
pub fn group(ui: &mut Ui, caption: &str, add: impl FnOnce(&mut Ui)) {
    let ctx = ui.ctx().clone();
    let mut fit = ribbon_fit(&ctx);
    let index = fit.next_group;
    fit.next_group += 1;
    let icon_id = egui::Id::unique(("versolopdf-ribbon-group-icon", caption));
    if !fit.in_popup && index >= fit.collapse_from {
        set_ribbon_fit(&ctx, fit);
        let icon = ctx.data(|d| d.get_temp::<Icon>(icon_id)).unwrap_or(Icon::Gear);
        ui.vertical(|ui| {
            ui.set_min_height(RIBBON_GROUP_HEIGHT);
            let response = ui
                .push_id(("ribbon-group", caption), |ui| big_menu_button(ui, icon, caption, true, false))
                .inner;
            egui::Popup::menu(&response).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(
                |ui| {
                    let outer = ribbon_fit(ui.ctx());
                    set_ribbon_fit(ui.ctx(), RibbonFit { in_popup: true, ..outer });
                    ui.ctx().data_mut(|d| d.insert_temp(command_ran_id(), false));
                    ui.horizontal(|ui| {
                        ui.set_min_height(RIBBON_CONTENT_HEIGHT);
                        add(ui);
                    });
                    set_ribbon_fit(ui.ctx(), outer);
                    if ui.ctx().data(|d| d.get_temp::<bool>(command_ran_id())).unwrap_or(false) {
                        ui.close();
                    }
                },
            );
        });
        group_separator(ui);
        return;
    }
    fit.first_icon = None;
    set_ribbon_fit(&ctx, fit);
    ui.vertical(|ui| {
        ui.set_min_height(RIBBON_GROUP_HEIGHT);
        let height = ui
            .horizontal(|ui| {
                ui.set_min_height(RIBBON_CONTENT_HEIGHT);
                add(ui);
            })
            .response
            .rect
            .height();
        // Taller contents would be cut off (the ribbon has a fixed height).
        if height > RIBBON_CONTENT_HEIGHT + 0.5 && !fit.in_popup && !ui.is_sizing_pass() {
            ctx.data_mut(|d| d.insert_temp(too_tall_id(), format!("{caption} ({height:.0})")));
        }
        // Caption centred under the group (by indenting it: a centred layout
        // would take the whole ribbon width).
        let content_w = ui.min_rect().width();
        let caption_w = ui
            .painter()
            .layout_no_wrap(caption.to_string(), FontId::proportional(10.5), Color32::WHITE)
            .size()
            .x;
        ui.horizontal(|ui| {
            ui.add_space(((content_w - caption_w) / 2.0).max(0.0));
            ui.add(
                egui::Label::new(egui::RichText::new(caption).size(10.5).weak())
                    .wrap_mode(TextWrapMode::Extend)
                    .selectable(false),
            );
        });
    });
    // Remember the group's first icon for its collapsed button.
    if let Some(icon) = ribbon_fit(&ctx).first_icon
        && !fit.in_popup
    {
        ctx.data_mut(|d| d.insert_temp(icon_id, icon));
    }
    group_separator(ui);
}

/// Thin line between ribbon groups, with more air in a wide ribbon.
fn group_separator(ui: &mut Ui) {
    let spacing = if ribbon_fit(ui.ctx()).tight() { 8.0 } else { 14.0 };
    ui.add(egui::Separator::default().vertical().spacing(spacing).shrink(6.0));
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
