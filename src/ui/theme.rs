//! Colours and widget styling.

use egui::{Color32, Context, CornerRadius, Stroke, Visuals};
use serde::{Deserialize, Serialize};

pub const ACCENT: Color32 = Color32::from_rgb(30, 100, 220);
pub const BRAND: Color32 = Color32::from_rgb(200, 35, 45);

/// "Design" of the user interface (Ansicht › Design).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Design {
    #[default]
    Standard,
    /// "Weiß".
    White,
    /// "Dunkles Grau".
    DarkGray,
    /// "Schwarz".
    Black,
}

impl Design {
    pub const ALL: [Design; 4] = [Design::Standard, Design::White, Design::DarkGray, Design::Black];

    /// Dark designs always use dark widgets.
    pub fn is_dark(self) -> bool {
        matches!(self, Design::DarkGray | Design::Black)
    }

    pub fn names(self) -> (&'static str, &'static str) {
        match self {
            Design::Standard => ("Standard", "Standard"),
            Design::White => ("Weiß", "White"),
            Design::DarkGray => ("Dunkles Grau", "Dark gray"),
            Design::Black => ("Schwarz", "Black"),
        }
    }
}

pub fn canvas_background(dark: bool) -> Color32 {
    if dark { Color32::from_gray(48) } else { Color32::from_gray(128) }
}

pub fn ribbon_background(dark: bool, design: Design) -> Color32 {
    match (dark, design) {
        (true, Design::Black) => Color32::from_gray(16),
        (true, _) => Color32::from_gray(38),
        (false, Design::White) => Color32::WHITE,
        (false, _) => Color32::from_gray(250),
    }
}

pub fn icon_color(dark: bool) -> Color32 {
    if dark { Color32::from_gray(220) } else { Color32::from_gray(50) }
}

/// Applies the widget colours (`dark`: dark mode or a dark design).
pub fn apply(ctx: &Context, dark: bool, design: Design) {
    let mut visuals = if dark { Visuals::dark() } else { Visuals::light() };
    visuals.selection.bg_fill = ACCENT.gamma_multiply(if dark { 0.7 } else { 0.35 });
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    visuals.hyperlink_color = ACCENT;
    if !dark {
        visuals.panel_fill = Color32::from_gray(240);
        visuals.window_fill = Color32::from_gray(252);
        visuals.extreme_bg_color = Color32::WHITE;
        // Visible borders for text fields and buttons on light backgrounds.
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_gray(195));
        if design == Design::White {
            visuals.panel_fill = Color32::WHITE;
            visuals.window_fill = Color32::WHITE;
            visuals.faint_bg_color = Color32::from_gray(248);
        }
    } else if design == Design::Black {
        visuals.panel_fill = Color32::from_gray(10);
        visuals.window_fill = Color32::from_gray(20);
        visuals.extreme_bg_color = Color32::BLACK;
        visuals.faint_bg_color = Color32::from_gray(24);
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
