//! Document tab bar and status bar.

use egui::{Panel, RichText, Ui};

use super::MrPdfApp;
use crate::ui::icons::Icon;
use crate::ui::theme;
use crate::ui::view::{self, ZoomMode};
use crate::ui::widgets::icon_button;

impl MrPdfApp {
    pub(super) fn tab_bar(&mut self, ui: &mut Ui) {
        if self.tabs.is_empty() {
            return;
        }
        let t = self.t();
        let ctx = ui.ctx().clone();
        let mut close = None;
        Panel::top("doc-tabs").show(ui, |ui| {
            egui::ScrollArea::horizontal()
                .id_salt("doc-tabs-scroll")
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for (i, tab) in self.tabs.iter().enumerate() {
                            let selected = i == self.active;
                            let frame = egui::Frame::new()
                                .inner_margin(egui::Margin::symmetric(8, 3))
                                .corner_radius(egui::CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 })
                                .fill(if selected {
                                    ui.visuals().extreme_bg_color
                                } else {
                                    ui.visuals().faint_bg_color
                                })
                                .stroke(egui::Stroke::new(
                                    1.0,
                                    if selected {
                                        theme::ACCENT
                                    } else {
                                        ui.visuals().widgets.noninteractive.bg_stroke.color
                                    },
                                ));
                            frame.show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    let title = RichText::new(tab.title());
                                    let title = if selected { title.strong() } else { title };
                                    let label = ui.add(egui::Label::new(title).sense(egui::Sense::click()));
                                    let label = match tab.doc.path() {
                                        Some(p) => label.on_hover_text(p.display().to_string()),
                                        None => label,
                                    };
                                    if label.clicked() {
                                        self.active = i;
                                    }
                                    if label.middle_clicked() {
                                        close = Some(i);
                                    }
                                    if icon_button(ui, Icon::Close, t.close, false, true).clicked() {
                                        close = Some(i);
                                    }
                                });
                            });
                        }
                    });
                });
        });
        if let Some(i) = close {
            self.request_close_tab(&ctx, i);
        }
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
                        .add(egui::TextEdit::singleline(&mut self.goto_text).id(goto_id).desired_width(42.0));
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
                    if let Some(geo) = tab.doc.geometry(cur) {
                        let (w, h) = geo.view_size();
                        ui.label(
                            RichText::new(format!("{:.0} × {:.0} mm", w / 72.0 * 25.4, h / 72.0 * 25.4))
                                .weak(),
                        );
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.label(message);
                    });
                });
            });
        });
    }
}
