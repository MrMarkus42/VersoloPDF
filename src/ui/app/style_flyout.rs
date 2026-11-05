//! Style presets of the comment tools: holding the mouse button down on a
//! tool button (marked with a small arrow in its lower right corner) opens
//! a palette of ready-made styles.

use egui::{Color32, Context, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

use super::{Dialog, VersoloPdfApp};
use crate::model::{AnnotStyle, Rgba};
use crate::ui::i18n::Language;
use crate::ui::tools::Tool;

/// How a tool's sample is drawn in the palette.
#[derive(Clone, Copy, PartialEq)]
enum Look {
    /// Coloured block behind text (highlights).
    Marker,
    /// Text with a coloured line (underline, strike out, …).
    TextLine,
    /// Coloured text (typewriter, text box, callout).
    Text,
    /// Speech bubble (comment, attachment, audio).
    Bubble,
    /// Line (line, arrow, pencil, measuring).
    Stroke,
    /// Closed shape: filled presets and "… Rahmen" outlines.
    Shape,
}

fn look(tool: Tool) -> Option<Look> {
    use Tool::*;
    Some(match tool {
        Highlight | AreaHighlight | ContentHighlight => Look::Marker,
        Underline | StrikeOut | Squiggly | Replace | Caret => Look::TextLine,
        Typewriter | TextBox | Callout => Look::Text,
        Note | FileAttach | SoundNote => Look::Bubble,
        Line | Arrow | Polyline | Pencil | MeasureDistance | MeasurePerimeter => Look::Stroke,
        Rectangle | Ellipse | Polygon | Cloud | MeasureArea => Look::Shape,
        _ => return None,
    })
}

/// Whether the tool's button gets the hold-down palette.
pub fn has_presets(tool: Tool) -> bool {
    look(tool).is_some()
}

/// One entry of the palette: names and colour; `filled` for closed shapes.
struct Preset {
    de: String,
    en: String,
    color: Rgba,
    filled: bool,
}

const COLORS: [(&str, &str, [u8; 3]); 6] = [
    ("Rot", "Red", [229, 57, 53]),
    ("Violett", "Violet", [156, 39, 176]),
    ("Indigo", "Indigo", [63, 81, 181]),
    ("Blau", "Blue", [33, 150, 243]),
    ("Grün", "Green", [67, 160, 71]),
    ("Gelb", "Yellow", [253, 216, 53]),
];

fn presets(tool: Tool) -> Vec<Preset> {
    let look = look(tool);
    // Markers and bubbles end with grey, the others with black.
    let last = if matches!(look, Some(Look::Marker | Look::Bubble)) {
        ("Grau", "Grey", [158, 158, 158])
    } else {
        ("Schwarz", "Black", [0, 0, 0])
    };
    let rgba = |c: [u8; 3]| Rgba::rgb(c[0], c[1], c[2]);
    let mut out: Vec<Preset> = COLORS
        .iter()
        .chain(std::iter::once(&last))
        .map(|(de, en, c)| Preset { de: (*de).into(), en: (*en).into(), color: rgba(*c), filled: true })
        .collect();
    if look == Some(Look::Shape) {
        let frames = [
            ("Roter", "Red"),
            ("Violetter", "Violet"),
            ("Indigo", "Indigo"),
            ("Blauer", "Blue"),
            ("Grüner", "Green"),
            ("Gelber", "Yellow"),
            ("Schwarzer", "Black"),
        ];
        for ((de, en), (_, _, c)) in
            frames.iter().zip(COLORS.iter().chain(std::iter::once(&("", "", [0, 0, 0]))))
        {
            out.push(Preset {
                de: format!("{de} Rahmen"),
                en: format!("{en} frame"),
                color: rgba(*c),
                filled: false,
            });
        }
    } else {
        for p in &mut out {
            p.filled = false;
        }
    }
    out
}

/// The tool's style with a preset applied.
fn apply(mut style: AnnotStyle, tool: Tool, preset: &Preset) -> AnnotStyle {
    style.color = preset.color;
    if look(tool) == Some(Look::Shape) {
        style.fill = preset.filled.then_some(preset.color);
    }
    style
}

fn matches(style: &AnnotStyle, tool: Tool, preset: &Preset) -> bool {
    let fill_ok = look(tool) != Some(Look::Shape) || style.fill.is_some() == preset.filled;
    style.color == preset.color && fill_ok
}

fn color32(c: Rgba) -> Color32 {
    Color32::from_rgb(c.r, c.g, c.b)
}

/// Draws the sample of a preset into `r`.
fn sample(ui: &Ui, r: Rect, look: Look, preset: &Preset) {
    let p = ui.painter();
    let c = color32(preset.color);
    let ink = ui.visuals().text_color();
    let font = egui::FontId::proportional(r.height() * 0.5);
    let center = r.center();
    match look {
        Look::Marker => {
            p.rect_filled(r.shrink(4.0), 5.0, c.gamma_multiply(0.85));
            p.text(center, egui::Align2::CENTER_CENTER, "T", font, Color32::from_gray(40));
        }
        Look::TextLine => {
            p.text(center - vec2(0.0, 4.0), egui::Align2::CENTER_CENTER, "T", font, ink);
            let y = r.bottom() - 9.0;
            p.line_segment([pos2(r.left() + 14.0, y), pos2(r.right() - 14.0, y)], Stroke::new(2.5, c));
        }
        Look::Text => {
            p.text(center, egui::Align2::CENTER_CENTER, "T", font, c);
        }
        Look::Bubble => {
            let b = Rect::from_min_max(r.min + vec2(8.0, 6.0), r.max - vec2(8.0, 14.0));
            p.rect(b, 6.0, c, Stroke::new(1.0, ink.gamma_multiply(0.6)), StrokeKind::Inside);
            let tail = vec![
                pos2(b.left() + 10.0, b.bottom()),
                pos2(b.left() + 22.0, b.bottom()),
                pos2(b.left() + 8.0, r.bottom() - 6.0),
            ];
            p.add(egui::Shape::convex_polygon(tail, c, Stroke::NONE));
            for i in 0..3 {
                let y = b.top() + 7.0 + i as f32 * 6.0;
                p.line_segment(
                    [pos2(b.left() + 8.0, y), pos2(b.right() - 8.0, y)],
                    Stroke::new(1.0, Color32::from_black_alpha(140)),
                );
            }
        }
        Look::Stroke => {
            p.line_segment(
                [r.left_bottom() + vec2(10.0, -8.0), r.right_top() + vec2(-10.0, 8.0)],
                Stroke::new(2.5, c),
            );
        }
        Look::Shape => {
            let s = r.shrink2(vec2(12.0, 8.0));
            if preset.filled {
                p.rect_filled(s, 2.0, c);
            } else {
                p.rect_stroke(s, 2.0, Stroke::new(2.0, c), StrokeKind::Inside);
            }
        }
    }
}

impl VersoloPdfApp {
    /// Adds the hold-down palette to a tool button: draws the corner arrow,
    /// opens the palette after a long press and returns whether the click
    /// should still run the tool (not after a long press).
    pub(super) fn style_flyout(&mut self, ui: &mut Ui, ctx: &Context, resp: &Response, tool: Tool) -> bool {
        let Some(look) = look(tool) else { return true };
        let enabled = !self.tabs.is_empty();
        // The corner arrow.
        let r = resp.rect;
        let tip = r.right_bottom() - vec2(2.5, 2.5);
        let c = ui.visuals().text_color().gamma_multiply(if enabled { 0.75 } else { 0.3 });
        ui.painter().add(egui::Shape::convex_polygon(
            vec![tip, tip - vec2(0.0, 5.0), tip - vec2(5.0, 0.0)],
            c,
            Stroke::NONE,
        ));
        let popup_id = resp.id.with("style-presets");
        if resp.is_pointer_button_down_on() && enabled {
            let held = ui.input(|i| i.pointer.press_start_time().is_some_and(|t| i.time - t > 0.4));
            if held {
                egui::Popup::open_id(ctx, popup_id);
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
        }
        let open = egui::Popup::is_id_open(ctx, popup_id);
        let de = self.settings.language == Language::German;
        let current = self.settings.style(tool);
        let list = presets(tool);
        let mut chosen = None;
        let mut palette = false;
        let shown = egui::Popup::from_response(resp)
            .id(popup_id)
            // Open only while remembered as open (after a long press). It
            // stays open when the button is released and closes at the next
            // press outside (below).
            .open_memory(None)
            .close_behavior(egui::PopupCloseBehavior::IgnoreClicks)
            .show(|ui| {
                egui::Grid::new(popup_id.with("grid")).spacing([4.0, 4.0]).show(ui, |ui| {
                    for (i, preset) in list.iter().enumerate() {
                        let name = if de { &preset.de } else { &preset.en };
                        let (rect, tile) = ui.allocate_exact_size(vec2(92.0, 70.0), Sense::click());
                        tile.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
                        let selected = matches(&current, tool, preset);
                        let fill = if tile.hovered() || selected {
                            crate::ui::theme::ACCENT.gamma_multiply(if selected { 0.22 } else { 0.12 })
                        } else {
                            Color32::TRANSPARENT
                        };
                        ui.painter().rect(
                            rect,
                            4.0,
                            fill,
                            Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
                            StrokeKind::Inside,
                        );
                        sample(
                            ui,
                            Rect::from_min_size(rect.min + vec2(14.0, 4.0), vec2(64.0, 44.0)),
                            look,
                            preset,
                        );
                        ui.painter().text(
                            Pos2::new(rect.center().x, rect.bottom() - 10.0),
                            egui::Align2::CENTER_CENTER,
                            name,
                            egui::FontId::proportional(11.0),
                            ui.visuals().text_color(),
                        );
                        if tile.clicked() {
                            chosen = Some(i);
                        }
                        if i % 5 == 4 {
                            ui.end_row();
                        }
                    }
                });
                ui.separator();
                let label = if de { "Formatpalette für Anmerkungen…" } else { "Comment style palette…" };
                if ui.button(label).clicked() {
                    palette = true;
                }
            });
        if let Some(shown) = shown {
            let outside = ctx.input(|i| {
                i.pointer.any_pressed()
                    && i.pointer.interact_pos().is_some_and(|p| !shown.response.rect.contains(p))
            });
            if outside || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                egui::Popup::close_id(ctx, popup_id);
            }
        }
        if let Some(i) = chosen {
            egui::Popup::close_id(ctx, popup_id);
            let style = apply(current.clone(), tool, &list[i]);
            self.settings.set_style(tool, style);
            self.select_tool(tool);
        }
        if palette {
            egui::Popup::close_id(ctx, popup_id);
            self.dialog = Some(Dialog::CommentStyles(tool));
        }
        !open
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_have_the_expected_styles() {
        let names = |t: Tool| presets(t).iter().map(|p| p.de.clone()).collect::<Vec<_>>();
        assert_eq!(names(Tool::Typewriter), ["Rot", "Violett", "Indigo", "Blau", "Grün", "Gelb", "Schwarz"]);
        assert_eq!(names(Tool::Highlight).last().unwrap(), "Grau");
        let shapes = presets(Tool::Rectangle);
        assert_eq!(shapes.len(), 14);
        assert_eq!(shapes[7].de, "Roter Rahmen");
        assert!(shapes[0].filled && !shapes[7].filled);
        assert!(!has_presets(Tool::Eraser) && !has_presets(Tool::Hand));

        let style = apply(AnnotStyle::default(), Tool::Rectangle, &shapes[3]);
        assert_eq!(style.fill, Some(style.color));
        let frame = apply(style, Tool::Rectangle, &shapes[10]);
        assert_eq!(frame.fill, None);
        assert!(matches(&frame, Tool::Rectangle, &shapes[10]));
    }
}
