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

/// The modern look is on (set by [`apply`]).
static MODERN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn modern() -> bool {
    MODERN.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn canvas_background(dark: bool) -> Color32 {
    match (dark, modern()) {
        (true, _) => Color32::from_gray(48),
        // A calm light grey: the white page stands out without glare.
        (false, true) => Color32::from_gray(214),
        (false, false) => Color32::from_gray(128),
    }
}

/// The system's user interface font (Segoe UI on Windows), if found; it
/// is read from the system, not shipped.
fn system_font() -> Option<Vec<u8>> {
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if cfg!(windows) {
        let dir = std::env::var_os("WINDIR").map_or_else(|| "C:\\Windows".into(), std::path::PathBuf::from);
        candidates.push(dir.join("Fonts").join("segoeui.ttf"));
    } else if cfg!(target_os = "macos") {
        candidates.push("/System/Library/Fonts/Supplemental/Arial.ttf".into());
    } else {
        for p in [
            "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
            "/usr/share/fonts/noto/NotoSans-Regular.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        ] {
            candidates.push(p.into());
        }
    }
    candidates.iter().find_map(|p| std::fs::read(p).ok())
}

/// Fonts for the look: the system font first (egui's fonts stay as
/// fallback for symbols), or egui's own fonts.
pub fn apply_fonts(ctx: &Context, modern: bool) {
    let mut fonts = egui::FontDefinitions::default();
    if modern && let Some(data) = system_font() {
        fonts.font_data.insert("system-ui".into(), std::sync::Arc::new(egui::FontData::from_owned(data)));
        if let Some(list) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            list.insert(0, "system-ui".into());
        }
    }
    // The key symbols of the Mac (⌘ ⇧ ⌥ ⌃), which neither the system font
    // (Arial) nor egui's fonts contain.
    if cfg!(target_os = "macos")
        && let Ok(data) = std::fs::read("/System/Library/Fonts/Apple Symbols.ttf")
    {
        fonts.font_data.insert("mac-symbols".into(), std::sync::Arc::new(egui::FontData::from_owned(data)));
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            if let Some(list) = fonts.families.get_mut(&family) {
                list.push("mac-symbols".into());
            }
        }
    }
    ctx.set_fonts(fonts);
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

/// Applies the widget colours (`dark`: dark mode or a dark design) and,
/// with `modern`, the modern look.
pub fn apply(ctx: &Context, dark: bool, design: Design, modern: bool) {
    MODERN.store(modern, std::sync::atomic::Ordering::Relaxed);
    apply_classic(ctx, dark, design);
    if !modern {
        return;
    }
    ctx.all_styles_mut(|s| {
        use egui::{FontFamily::Proportional, FontId, TextStyle};
        s.text_styles = [
            (TextStyle::Small, FontId::new(12.0, Proportional)),
            (TextStyle::Body, FontId::new(14.0, Proportional)),
            (TextStyle::Button, FontId::new(14.0, Proportional)),
            (TextStyle::Heading, FontId::new(19.0, Proportional)),
            (TextStyle::Monospace, FontId::new(13.0, egui::FontFamily::Monospace)),
        ]
        .into();
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(10.0, 5.0);
        s.spacing.interact_size = egui::vec2(40.0, 28.0);
        s.spacing.menu_margin = egui::Margin::same(8);
        let v = &mut s.visuals;
        // No size change on hover: neighbours must not move.
        for w in [&mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
            w.expansion = 0.0;
        }
        for w in [
            &mut v.widgets.noninteractive,
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.corner_radius = CornerRadius::same(5);
        }
        if !dark {
            // Calm, clearly readable: dark text, soft surfaces.
            v.panel_fill = if design == Design::White { Color32::WHITE } else { Color32::from_gray(246) };
            v.window_fill = Color32::WHITE;
            v.faint_bg_color = Color32::from_gray(242);
            v.widgets.noninteractive.fg_stroke.color = Color32::from_gray(30);
            v.widgets.inactive.fg_stroke.color = Color32::from_gray(25);
            v.widgets.inactive.weak_bg_fill = Color32::from_gray(238);
            v.widgets.inactive.bg_fill = Color32::from_gray(238);
            v.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_gray(205));
            v.widgets.hovered.weak_bg_fill = Color32::from_rgb(226, 234, 248);
            v.widgets.hovered.bg_fill = Color32::from_rgb(226, 234, 248);
            v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT.gamma_multiply(0.6));
            v.weak_text_color = Some(Color32::from_gray(90));
        }
    });
}

fn apply_classic(ctx: &Context, dark: bool, design: Design) {
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
