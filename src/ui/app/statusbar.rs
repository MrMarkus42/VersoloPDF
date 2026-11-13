//! Document tab bar and status bar.

use egui::{Panel, RichText, Ui};

use super::VersoloPdfApp;
use crate::ui::icons::Icon;
use crate::ui::view::{self, ZoomMode};
use crate::ui::widgets::icon_button;

impl VersoloPdfApp {
    pub(super) fn tab_bar(&mut self, ui: &mut Ui) {
        // "Tab-Leiste verbergen, wenn nur ein Dokument geöffnet ist"; with
        // several tab groups every group has its own bar.
        let main_tabs = self.tabs.iter().filter(|t| t.window == 0).count();
        if main_tabs == 0
            || (main_tabs == 1 && self.settings.hide_single_tab)
            || self.window_groups(0).len() > 1
        {
            return;
        }
        Panel::top("doc-tabs").show(ui, |ui| self.main_tab_strip(ui));
    }

    pub(super) fn status_bar(&mut self, ui: &mut Ui) {
        let t = self.t();
        let now = ui.input(|i| i.time);
        Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                // Left: status message or tool hint.
                let message = match &self.status {
                    Some((text, at)) if now - at < 6.0 => text.clone(),
                    _ => String::new(),
                };
                if !message.is_empty() {
                    ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
                }
                let Some(tab) = self.tabs.get_mut(self.active) else {
                    ui.label(message);
                    return;
                };
                let count = tab.doc.page_count();
                let cur = tab.current_page;
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Right: zoom controls.
                    let mut zoom_pct = tab.zoom * 100.0;
                    if ui
                        .add(
                            egui::DragValue::new(&mut zoom_pct)
                                .range(view::MIN_ZOOM * 100.0..=view::MAX_ZOOM * 100.0)
                                .suffix(" %")
                                .speed(1.0)
                                .max_decimals(0),
                        )
                        .changed()
                    {
                        tab.zoom = zoom_pct / 100.0;
                        tab.zoom_mode = ZoomMode::Custom;
                    }
                    if icon_button(ui, Icon::ZoomIn, t.zoom_in, false, true).clicked() {
                        tab.zoom = view::zoom_in(tab.zoom);
                        tab.zoom_mode = ZoomMode::Custom;
                    }
                    if icon_button(ui, Icon::ZoomOut, t.zoom_out, false, true).clicked() {
                        tab.zoom = view::zoom_out(tab.zoom);
                        tab.zoom_mode = ZoomMode::Custom;
                    }
                    if icon_button(ui, Icon::FitPage, t.fit_page, tab.zoom_mode == ZoomMode::FitPage, true)
                        .clicked()
                    {
                        tab.zoom_mode = ZoomMode::FitPage;
                    }
                    if icon_button(ui, Icon::FitWidth, t.fit_width, tab.zoom_mode == ZoomMode::FitWidth, true)
                        .clicked()
                    {
                        tab.zoom_mode = ZoomMode::FitWidth;
                    }
                    ui.separator();
                    // Page navigation.
                    if icon_button(ui, Icon::Last, t.last_page, false, cur + 1 < count).clicked() {
                        tab.go_to_page(count - 1);
                    }
                    if icon_button(ui, Icon::Next, t.next_page, false, cur + 1 < count).clicked() {
                        tab.go_to_page(cur + 1);
                    }
                    ui.label(format!("/ {count}"));
                    let goto_id = ui.make_persistent_id("goto-page");
                    let editing = ui.memory(|m| m.has_focus(goto_id));
                    if !editing {
                        self.goto_text = (cur + 1).to_string();
                    }
                    let resp = ui
                        .add(egui::TextEdit::singleline(&mut self.goto_text).id(goto_id).desired_width(42.0))
                        .accessible_name(if self.settings.language == crate::ui::i18n::Language::German {
                            "Seite"
                        } else {
                            "Page"
                        });
                    if resp.lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter))
                        && let Ok(n) = self.goto_text.trim().parse::<usize>()
                        && (1..=count).contains(&n)
                    {
                        tab.go_to_page(n - 1);
                    }
                    if icon_button(ui, Icon::Prev, t.previous_page, false, cur > 0).clicked() {
                        tab.go_to_page(cur - 1);
                    }
                    if icon_button(ui, Icon::First, t.first_page, false, cur > 0).clicked() {
                        tab.go_to_page(0);
                    }
                    ui.separator();
                    // "Seitenmaße/Position": page size and pointer position.
                    if self.settings.show_page_info {
                        let unit = self.settings.unit;
                        let de = self.settings.language == crate::ui::i18n::Language::German;
                        let num = |pt: f32| {
                            let s = format!("{:.2}", pt / unit.points());
                            if de { s.replace('.', ",") } else { s }
                        };
                        let (page, at) = match tab.pointer {
                            Some((p, v)) => (p, Some(v)),
                            None => (cur, None),
                        };
                        if let Some(geo) = tab.display_geometry(page) {
                            let (w, h) = geo.view_size();
                            let u = unit.suffix();
                            let pos = match at {
                                Some(v) => format!("   X: {} {u}  Y: {} {u}", num(v.x), num(v.y)),
                                None => String::new(),
                            };
                            let text = if de {
                                format!("B: {} {u}  H: {} {u}{pos}", num(w), num(h))
                            } else {
                                format!("W: {} {u}  H: {} {u}{pos}", num(w), num(h))
                            };
                            ui.label(RichText::new(text).weak());
                        }
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.label(message);
                    });
                });
            });
        });
    }
}
