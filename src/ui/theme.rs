//! Colours and widget styling.

use egui::{Color32, Context, CornerRadius, Stroke, Visuals};

pub const ACCENT: Color32 = Color32::from_rgb(30, 100, 220);
pub const BRAND: Color32 = Color32::from_rgb(200, 35, 45);

pub fn canvas_background(dark: bool) -> Color32 {
    if dark {
        Color32::from_gray(48)
    } else {
        Color32::from_gray(128)
    }
}

pub fn ribbon_background(dark: bool) -> Color32 {
    if dark {
        Color32::from_gray(38)
    } else {
        Color32::from_gray(250)
    }
}

pub fn icon_color(dark: bool) -> Color32 {
    if dark {
        Color32::from_gray(220)
    } else {
        Color32::from_gray(50)
    }
}

pub fn apply(ctx: &Context, dark: bool) {
    let mut visuals = if dark { Visuals::dark() } else { Visuals::light() };
    visuals.selection.bg_fill = ACCENT.gamma_multiply(if dark { 0.7 } else { 0.35 });
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    visuals.hyperlink_color = ACCENT;
    if !dark {
        visuals.panel_fill = Color32::from_gray(240);
        visuals.window_fill = Color32::from_gray(252);
        visuals.extreme_bg_color = Color32::WHITE;
    }
    for w in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        w.corner_radius = CornerRadius::same(3);
    }
    ctx.set_visuals(visuals);
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(6.0, 4.0);
        s.spacing.button_padding = egui::vec2(6.0, 3.0);
        s.interaction.tooltip_delay = 0.4;
    });
}
