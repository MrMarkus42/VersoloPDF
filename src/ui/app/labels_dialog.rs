//! "Seitennummerierung" (page labels) with the range ruler:
//! double click on the bar adds a range, the handles move range starts.

use egui::{Align2, Color32, FontId, Rect, RichText, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

use super::commands::Cmd;
use super::dialogs::{Action, Outcome};
use super::page_dialogs::{footer, tr};
use super::{Dialog, MrPdfApp};
use crate::pages::{LabelRange, LabelStyle};
use crate::ui::i18n::Language;
use crate::ui::icons::Icon;
use crate::ui::widgets::icon_button;

/// Upper bound of undo steps inside the dialog.
const MAX_UNDO: usize = 100;

#[derive(Debug, Clone, PartialEq)]
pub struct LabelsState {
    pub ranges: Vec<LabelRange>,
    pub original: Vec<LabelRange>,
    pub selected: usize,
    pub zoom: f32,
    pub undo: Vec<Vec<LabelRange>>,
    pub redo: Vec<Vec<LabelRange>>,
    pub dragging: Option<usize>,
}

fn style_name(s: LabelStyle, de: bool) -> &'static str {
    match s {
        LabelStyle::Decimal => "1, 2, 3 …",
        LabelStyle::RomanLower => "i, ii, iii …",
        LabelStyle::RomanUpper => "I, II, III …",
        LabelStyle::LettersLower => "a, b, c …",
        LabelStyle::LettersUpper => "A, B, C …",
        LabelStyle::None => tr(de, "(kein Format, nur Präfix)", "(no style, prefix only)"),
    }
}

/// Labels of the first pages for the preview line.
fn preview(ranges: &[LabelRange], n: usize) -> Vec<(String, bool)> {
    let label = |p: usize| {
        let r = ranges.iter().rev().find(|r| r.start <= p).unwrap_or(&ranges[0]);
        r.label(p)
    };
    let mut out = Vec::new();
    for (i, r) in ranges.iter().enumerate().take(4) {
        let end = ranges.get(i + 1).map_or(n, |x| x.start);
        let len = end.saturating_sub(r.start);
        let shown: Vec<usize> = if len <= 5 {
            (r.start..end).collect()
        } else {
            vec![r.start, r.start + 1, r.start + 2, usize::MAX, end - 2, end - 1]
        };
        for p in shown {
            out.push(if p == usize::MAX { ("…".into(), i % 2 == 0) } else { (label(p), i % 2 == 0) });
        }
    }
    out
}

impl MrPdfApp {
    /// Opens the page label dialog for `Cmd::PageLabels`.
    pub(super) fn run_labels(&mut self, cmd: Cmd) -> bool {
        if cmd != Cmd::PageLabels {
            return false;
        }
        let Some(tab) = self.tab() else { return false };
        let ranges = match tab.doc.page_label_ranges() {
            Ok(r) => r,
            Err(err) => {
                self.show_error(&err);
                return true;
            }
        };
        self.dialog = Some(Dialog::Labels(Box::new(LabelsState {
            original: ranges.clone(),
            ranges,
            selected: 0,
            zoom: 1.0,
            undo: Vec::new(),
            redo: Vec::new(),
            dragging: None,
        })));
        true
    }

    pub(super) fn labels_dialog(&mut self, ui: &mut Ui, s: &mut LabelsState) -> (Outcome, Option<Action>) {
        let de = self.settings.language == Language::German;
        let n = self.active_tab().map_or(1, |t| t.doc.page_count()).max(1);
        let mut outcome = Outcome::Keep;
        let mut action: Option<Action> = None;
        ui.set_max_width(760.0);
        ui.heading(tr(de, "Seitennummerierung", "Page numbering"));
        ui.label(tr(
            de,
            "Sie können neue Bereiche mit einem Doppelklick auf die Leiste hinzufügen oder bestehende Bereiche durch Verschieben der Regler ändern.",
            "Double-click the bar to add new ranges or move the handles to change existing ranges.",
        ));
        let before = s.ranges.clone();
        ui.horizontal(|ui| {
            if icon_button(ui, Icon::Undo, tr(de, "Rückgängig", "Undo"), false, !s.undo.is_empty()).clicked()
                && let Some(prev) = s.undo.pop()
            {
                s.redo.push(std::mem::replace(&mut s.ranges, prev));
            }
            if icon_button(ui, Icon::Redo, tr(de, "Wiederholen", "Redo"), false, !s.redo.is_empty()).clicked()
                && let Some(next) = s.redo.pop()
            {
                s.undo.push(std::mem::replace(&mut s.ranges, next));
            }
            if ui.button(tr(de, "Neuer Bereich…", "New range…")).clicked() {
                let cur = s.ranges[s.selected.min(s.ranges.len() - 1)].clone();
                let next_start = s.ranges.get(s.selected + 1).map_or(n, |r| r.start);
                let start = ((cur.start + next_start) / 2).max(cur.start + 1);
                if start < n && !s.ranges.iter().any(|r| r.start == start) {
                    s.ranges.push(LabelRange { start, ..cur });
                    s.ranges.sort_by_key(|r| r.start);
                    s.selected = s.ranges.iter().position(|r| r.start == start).unwrap_or(0);
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add(egui::Slider::new(&mut s.zoom, 0.25..=4.0).show_value(false));
                ui.label(tr(de, "Vergrößerung:", "Zoom:"));
            });
        });

        // The ruler: one tick per page, ranges in alternating colours.
        // At zoom 1 the whole document fills the bar.
        let fit = ((ui.available_width() - 20.0) / n as f32).clamp(4.0, 60.0);
        let px = (fit * s.zoom).clamp(3.0, 120.0);
        let width = (n as f32 * px + 20.0).max(ui.available_width());
        egui::ScrollArea::horizontal().max_width(ui.available_width()).show(ui, |ui| {
            let (rect, resp) = ui.allocate_exact_size(vec2(width, 62.0), Sense::click_and_drag());
            let painter = ui.painter();
            let bar = Rect::from_min_size(rect.min + vec2(10.0, 6.0), vec2(n as f32 * px, 22.0));
            painter.rect(
                bar,
                2.0,
                ui.visuals().extreme_bg_color,
                Stroke::new(1.0, Color32::from_gray(150)),
                StrokeKind::Inside,
            );
            let x_of = |p: usize| bar.left() + p as f32 * px;
            for (i, r) in s.ranges.iter().enumerate() {
                let end = s.ranges.get(i + 1).map_or(n, |x| x.start);
                let seg = Rect::from_min_max(pos2(x_of(r.start), bar.top()), pos2(x_of(end), bar.bottom()));
                let color = if i == s.selected {
                    Color32::from_rgb(250, 225, 160)
                } else if i % 2 == 0 {
                    Color32::from_gray(235)
                } else {
                    Color32::from_gray(215)
                };
                painter.rect_filled(seg.shrink(1.0), 1.0, color);
            }
            // Ticks with labels every few pages.
            let step = ((40.0 / px).ceil() as usize).max(1);
            for p in 0..=n {
                let x = x_of(p);
                let long = p % step == 0;
                painter.line_segment(
                    [pos2(x, bar.bottom()), pos2(x, bar.bottom() - if long { 8.0 } else { 4.0 })],
                    Stroke::new(1.0, Color32::from_gray(120)),
                );
                if long && p < n {
                    let range = s.ranges.iter().rev().find(|r| r.start <= p).unwrap_or(&s.ranges[0]);
                    painter.text(
                        pos2(x + 2.0, bar.center().y - 3.0),
                        Align2::LEFT_CENTER,
                        range.label(p),
                        FontId::proportional(10.0),
                        ui.visuals().text_color(),
                    );
                }
            }
            // Handles (triangles) below the start of every range but the first.
            let mut handle_rects = Vec::new();
            for (i, r) in s.ranges.iter().enumerate() {
                let x = x_of(r.start);
                let top = bar.bottom() + 2.0;
                let color =
                    if i == s.selected { Color32::from_rgb(220, 120, 30) } else { Color32::from_gray(110) };
                painter.add(egui::Shape::convex_polygon(
                    vec![pos2(x, top), pos2(x - 6.0, top + 12.0), pos2(x + 6.0, top + 12.0)],
                    color,
                    Stroke::NONE,
                ));
                handle_rects.push((i, Rect::from_center_size(pos2(x, top + 7.0), vec2(14.0, 16.0))));
            }
            let page_at = |x: f32| (((x - bar.left()) / px).floor().max(0.0) as usize).min(n - 1);
            if resp.drag_started()
                && let Some(pos) = resp.interact_pointer_pos()
            {
                s.dragging = handle_rects.iter().find(|(i, r)| *i > 0 && r.contains(pos)).map(|(i, _)| *i);
                if let Some(i) = s.dragging {
                    s.selected = i;
                }
            }
            if let (Some(i), Some(pos)) = (s.dragging, resp.interact_pointer_pos())
                && resp.dragged()
            {
                let lo = s.ranges[i - 1].start + 1;
                let hi = s.ranges.get(i + 1).map_or(n, |r| r.start).saturating_sub(1);
                let p = (((pos.x - bar.left()) / px).round().max(0.0) as usize).clamp(lo, hi.max(lo));
                s.ranges[i].start = p;
            }
            if resp.drag_stopped() {
                s.dragging = None;
            }
            if resp.clicked()
                && let Some(pos) = resp.interact_pointer_pos()
            {
                let p = page_at(pos.x);
                s.selected = s.ranges.iter().rposition(|r| r.start <= p).unwrap_or(0);
            }
            if resp.double_clicked()
                && let Some(pos) = resp.interact_pointer_pos()
            {
                let p = page_at(pos.x);
                if p > 0 && !s.ranges.iter().any(|r| r.start == p) {
                    let base =
                        s.ranges.iter().rev().find(|r| r.start <= p).cloned().unwrap_or(s.ranges[0].clone());
                    s.ranges.push(LabelRange { start: p, first: 1, ..base });
                    s.ranges.sort_by_key(|r| r.start);
                    s.selected = s.ranges.iter().position(|r| r.start == p).unwrap_or(0);
                }
            }
        });

        let sel = s.selected.min(s.ranges.len() - 1);
        s.selected = sel;
        ui.horizontal(|ui| {
            ui.label(tr(de, "Startseite:", "Start page:"));
            let lo = if sel == 0 { 1 } else { s.ranges[sel - 1].start + 2 };
            let hi = s.ranges.get(sel + 1).map_or(n, |r| r.start);
            let mut start = s.ranges[sel].start + 1;
            if ui.add_enabled(sel > 0, egui::DragValue::new(&mut start).range(lo..=hi.max(lo))).changed() {
                s.ranges[sel].start = start - 1;
            }
            if ui.add_enabled(sel > 0, egui::Button::new(tr(de, "Löschen…", "Delete…"))).clicked() {
                s.ranges.remove(sel);
                s.selected = sel - 1;
            }
            if ui
                .button(tr(de, "Zurücksetzen…", "Reset…"))
                .on_hover_text(tr(
                    de,
                    "Fortlaufende Seitenzahlen ab 1 für das ganze Dokument",
                    "Plain numbers from 1 for the whole document",
                ))
                .clicked()
            {
                s.ranges = vec![LabelRange {
                    start: 0,
                    style: LabelStyle::Decimal,
                    prefix: String::new(),
                    first: 1,
                }];
                s.selected = 0;
            }
        });
        let sel = s.selected.min(s.ranges.len() - 1);
        ui.separator();
        ui.horizontal_top(|ui| {
            egui::Grid::new("labels-form").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
                let r = &mut s.ranges[sel];
                ui.label(tr(de, "Format:", "Format:"));
                egui::ComboBox::from_id_salt("label-format")
                    .selected_text(style_name(r.style, de))
                    .width(200.0)
                    .show_ui(ui, |ui| {
                        for st in [
                            LabelStyle::Decimal,
                            LabelStyle::RomanLower,
                            LabelStyle::RomanUpper,
                            LabelStyle::LettersLower,
                            LabelStyle::LettersUpper,
                            LabelStyle::None,
                        ] {
                            ui.selectable_value(&mut r.style, st, style_name(st, de));
                        }
                    });
                ui.end_row();
                ui.label(tr(de, "Präfix:", "Prefix:"));
                ui.add(egui::TextEdit::singleline(&mut r.prefix).desired_width(200.0));
                ui.end_row();
                ui.label(tr(de, "Beginne Nummerierung bei:", "Start numbering at:"));
                ui.add(egui::DragValue::new(&mut r.first).range(1..=1_000_000));
                ui.end_row();
            });
            ui.add_space(30.0);
            ui.vertical(|ui| {
                ui.label(tr(de, "Vorschau:", "Preview:"));
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let items = preview(&s.ranges, n);
                    for (i, (text, even)) in items.iter().enumerate() {
                        let rt = RichText::new(text);
                        ui.label(if *even { rt.strong() } else { rt });
                        if i + 1 < items.len() {
                            ui.label(", ");
                        }
                    }
                });
            });
        });
        if s.ranges != before {
            s.undo.push(before);
            if s.undo.len() > MAX_UNDO {
                s.undo.remove(0);
            }
            s.redo.clear();
        }
        let (ok, cancel, _) = footer(ui, de, true, false, "OK");
        if ok {
            let ranges = s.ranges.clone();
            if ranges != s.original {
                action = Some(Box::new(move |app, ctx| {
                    app.edit(ctx, |tab| tab.doc.set_page_label_ranges(&ranges))
                }));
            }
            outcome = Outcome::Close;
        } else if cancel {
            outcome = Outcome::Close;
        }
        (outcome, action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_shows_ranges() {
        let ranges = vec![
            LabelRange { start: 0, style: LabelStyle::RomanLower, prefix: String::new(), first: 1 },
            LabelRange { start: 3, style: LabelStyle::Decimal, prefix: "S-".into(), first: 1 },
        ];
        let p: Vec<String> = preview(&ranges, 20).into_iter().map(|x| x.0).collect();
        assert_eq!(p, ["i", "ii", "iii", "S-1", "S-2", "S-3", "…", "S-16", "S-17"]);
    }
}
