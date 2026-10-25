//! Left pane: thumbnails, bookmarks, comments and search.

use std::time::Duration;

use egui::{Color32, Panel, RichText, ScrollArea, Sense, Stroke, StrokeKind, Ui, vec2};

use super::{MrPdfApp, SidePanel};
use crate::model::{AnnotKind, Bookmark, SearchOptions};
use crate::ui::cache::Budget;
use crate::ui::i18n::Strings;
use crate::ui::icons::{self, Icon};
use crate::ui::tab::{AnnotRef, DocTab, ScrollRequest};
use crate::ui::theme;
use crate::ui::widgets::icon_button;

const THUMB_WIDTH: f32 = 120.0;

pub fn kind_icon(kind: AnnotKind) -> Icon {
    match kind {
        AnnotKind::Note => Icon::Note,
        AnnotKind::FreeText => Icon::TextBox,
        AnnotKind::Line => Icon::Line,
        AnnotKind::Square => Icon::Rectangle,
        AnnotKind::Circle => Icon::Ellipse,
        AnnotKind::Polygon | AnnotKind::Polyline => Icon::Polygon,
        AnnotKind::Highlight => Icon::Highlight,
        AnnotKind::Underline => Icon::Underline,
        AnnotKind::Squiggly => Icon::Squiggly,
        AnnotKind::StrikeOut => Icon::StrikeOut,
        AnnotKind::Stamp => Icon::Stamp,
        AnnotKind::Ink => Icon::Pencil,
        _ => Icon::Comments,
    }
}

impl MrPdfApp {
    pub(super) fn sidebar(&mut self, ui: &mut Ui) {
        let t = self.t();
        Panel::left("side-strip")
            .resizable(false)
            .exact_size(34.0)
            .show(ui, |ui| {
                ui.add_space(4.0);
                for (panel, icon, label) in [
                    (SidePanel::Thumbnails, Icon::Thumbnails, t.thumbnails),
                    (SidePanel::Bookmarks, Icon::Bookmarks, t.bookmarks),
                    (SidePanel::Comments, Icon::Comments, t.comments),
                    (SidePanel::Search, Icon::Search, t.search),
                ] {
                    if icon_button(ui, icon, label, self.side == panel, true).clicked() {
                        self.side = panel;
                    }
                }
            });
        Panel::left("side-panel")
            .resizable(true)
            .default_size(200.0)
            .size_range(150.0..=480.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let title = match self.side {
                        SidePanel::Thumbnails => t.thumbnails,
                        SidePanel::Bookmarks => t.bookmarks,
                        SidePanel::Comments => t.comments,
                        SidePanel::Search => t.search,
                    };
                    ui.strong(title);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icon_button(ui, Icon::Close, t.close, false, true).clicked() {
                            self.settings.show_sidebar = false;
                        }
                    });
                });
                ui.separator();
                match self.side {
                    SidePanel::Thumbnails => self.thumbnails(ui),
                    SidePanel::Bookmarks => self.bookmarks_panel(ui),
                    SidePanel::Comments => self.comments_panel(ui),
                    SidePanel::Search => self.search_panel(ui),
                }
            });
    }

    fn thumbnails(&mut self, ui: &mut Ui) {
        let t = self.t();
        let ctx = ui.ctx().clone();
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        let count = tab.doc.page_count();
        let sizes = tab.page_sizes();
        let ppp = ctx.pixels_per_point();
        let mut budget = Budget::new(Duration::from_millis(25));
        let mut go_to = None;
        let mut move_req: Option<(usize, usize)> = None;
        let mut rotate: Option<(usize, i32)> = None;
        let mut delete: Option<usize> = None;
        let mut extract: Option<usize> = None;
        let row_heights: Vec<f32> = sizes
            .iter()
            .map(|(w, h)| THUMB_WIDTH * h / w.max(1.0) + 26.0)
            .collect();
        let current = tab.current_page;
        ScrollArea::vertical().id_salt(("thumbs", tab.id)).auto_shrink([false, false]).show_viewport(ui, |ui, viewport| {
            let full_w = ui.available_width();
            let mut y = 0.0;
            for page in 0..count {
                let h = row_heights[page];
                let visible = y + h >= viewport.min.y - 200.0 && y <= viewport.max.y + 200.0;
                let (rect, response) = ui.allocate_exact_size(vec2(full_w, h), Sense::click_and_drag());
                y += h + ui.spacing().item_spacing.y;
                if !visible {
                    continue;
                }
                let thumb_h = h - 26.0;
                let thumb = egui::Rect::from_min_size(
                    rect.center_top() + vec2(-THUMB_WIDTH / 2.0, 4.0),
                    vec2(THUMB_WIDTH, thumb_h),
                );
                let painter = ui.painter();
                if page == current {
                    painter.rect_filled(rect, 3.0, theme::ACCENT.gamma_multiply(0.2));
                } else if response.hovered() {
                    painter.rect_filled(rect, 3.0, ui.visuals().widgets.hovered.weak_bg_fill);
                }
                painter.rect_filled(thumb, 0.0, Color32::WHITE);
                let px = [((THUMB_WIDTH * ppp) as u32).max(1), ((thumb_h * ppp) as u32).max(1)];
                if let Some(img) = tab.textures.thumbnail(&ctx, &tab.doc, page, px, &mut budget, tab.id) {
                    painter.image(
                        img.tex.id(),
                        thumb,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
                let border = if page == current {
                    Stroke::new(2.0, theme::ACCENT)
                } else {
                    Stroke::new(1.0, Color32::from_gray(150))
                };
                painter.rect_stroke(thumb, 0.0, border, StrokeKind::Outside);
                let label = tab.doc.page_label(page).unwrap_or_else(|| (page + 1).to_string());
                painter.text(
                    egui::pos2(rect.center().x, rect.max.y - 10.0),
                    egui::Align2::CENTER_CENTER,
                    label,
                    egui::FontId::proportional(12.0),
                    ui.visuals().text_color(),
                );
                if response.clicked() {
                    go_to = Some(page);
                }
                // Drag & drop to reorder pages.
                response.dnd_set_drag_payload(page);
                if let Some(from) = response.dnd_release_payload::<usize>() {
                    let pointer_y = ui.input(|i| i.pointer.interact_pos()).map_or(rect.center().y, |p| p.y);
                    let to = if pointer_y > rect.center().y { page + 1 } else { page };
                    move_req = Some((*from, to));
                }
                if response.dnd_hover_payload::<usize>().is_some() {
                    let pointer_y = ui.input(|i| i.pointer.hover_pos()).map_or(rect.center().y, |p| p.y);
                    let line_y = if pointer_y > rect.center().y { rect.max.y } else { rect.min.y };
                    ui.painter().hline(rect.x_range(), line_y, Stroke::new(3.0, theme::ACCENT));
                }
                response.context_menu(|ui| {
                    if ui.button(t.rotate_left).clicked() {
                        rotate = Some((page, -90));
                        ui.close();
                    }
                    if ui.button(t.rotate_right).clicked() {
                        rotate = Some((page, 90));
                        ui.close();
                    }
                    if ui.button(t.extract_pages).clicked() {
                        extract = Some(page);
                        ui.close();
                    }
                    if ui.add_enabled(count > 1, egui::Button::new(t.delete_pages)).clicked() {
                        delete = Some(page);
                        ui.close();
                    }
                });
            }
        });
        if budget.pending {
            ctx.request_repaint();
        }
        if let Some(page) = go_to {
            tab.go_to_page(page);
        }
        if let Some((from, to)) = move_req
            && from != to
            && from + 1 != to
        {
            self.move_page_to(&ctx, from, to);
        }
        if let Some((page, delta)) = rotate {
            self.edit_pages(&ctx, |tab| tab.doc.rotate_pages(&[page], delta));
        }
        if let Some(page) = delete {
            self.edit_pages(&ctx, |tab| tab.doc.delete_pages(&[page]));
        }
        if let Some(page) = extract {
            self.apply_range(&ctx, super::RangePurpose::Extract, vec![page]);
        }
    }

    fn edit_pages(&mut self, ctx: &egui::Context, f: impl FnOnce(&mut DocTab) -> crate::Result<()>) {
        if let Some(tab) = self.tabs.get_mut(self.active) {
            let result = f(tab);
            tab.after_edit();
            if let Err(err) = result {
                let t = self.t();
                self.dialog = Some(super::Dialog::Message {
                    title: t.error.to_string(),
                    text: super::actions::error_text(t, &err),
                });
            }
            ctx.request_repaint();
        }
    }

    fn bookmarks_panel(&mut self, ui: &mut Ui) {
        let t = self.t();
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        let bookmarks = tab.bookmarks();
        if bookmarks.is_empty() {
            ui.label(RichText::new(t.no_bookmarks).weak());
            return;
        }
        let mut target = None;
        ScrollArea::vertical().id_salt(("bookmarks", tab.id)).auto_shrink([false, false]).show(ui, |ui| {
            fn tree(ui: &mut Ui, items: &[Bookmark], path: &mut Vec<usize>, target: &mut Option<usize>) {
                for (i, b) in items.iter().enumerate() {
                    path.push(i);
                    let title = if b.title.is_empty() { "–".to_string() } else { b.title.clone() };
                    if b.children.is_empty() {
                        if ui.selectable_label(false, title).clicked() {
                            *target = b.page;
                        }
                    } else {
                        let id = ui.make_persistent_id(("bm", path.clone()));
                        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false)
                            .show_header(ui, |ui| {
                                if ui.selectable_label(false, title).clicked() {
                                    *target = b.page;
                                }
                            })
                            .body(|ui| tree(ui, &b.children, path, target));
                    }
                    path.pop();
                }
            }
            tree(ui, &bookmarks, &mut Vec::new(), &mut target);
        });
        if let Some(page) = target {
            tab.go_to_page(page);
        }
    }

    fn comments_panel(&mut self, ui: &mut Ui) {
        let t = self.t();
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        let comments = tab.all_comments();
        if comments.is_empty() {
            ui.label(RichText::new(t.no_comments).weak());
            return;
        }
        ui.label(RichText::new(format!("{} {}", comments.len(), t.comments)).weak());
        let selected = tab.selected_annot;
        let mut clicked: Option<AnnotRef> = None;
        let dark = ui.visuals().dark_mode;
        ScrollArea::vertical().id_salt(("comments", tab.id)).auto_shrink([false, false]).show(ui, |ui| {
            let mut last_page = usize::MAX;
            for c in &comments {
                if c.page != last_page {
                    ui.add_space(4.0);
                    ui.label(RichText::new(format!("{} {}", t.page, c.page + 1)).strong());
                    last_page = c.page;
                }
                let r = AnnotRef { page: c.page, index: c.index };
                let is_sel = selected == Some(r);
                let frame = egui::Frame::new()
                    .inner_margin(4)
                    .corner_radius(3)
                    .fill(if is_sel { theme::ACCENT.gamma_multiply(0.2) } else { Color32::TRANSPARENT });
                let resp = frame
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                            icons::paint(ui.painter(), rect, kind_icon(c.kind), theme::icon_color(dark));
                            ui.label(RichText::new(kind_name(t, c.kind)).strong());
                            if !c.author.is_empty() {
                                ui.label(RichText::new(&c.author).weak());
                            }
                        });
                        if !c.contents.is_empty() {
                            let mut text: String = c.contents.chars().take(160).collect();
                            if c.contents.chars().count() > 160 {
                                text.push('…');
                            }
                            ui.label(text);
                        }
                        if !c.modified.is_empty() {
                            ui.label(RichText::new(&c.modified).small().weak());
                        }
                    })
                    .response
                    .interact(Sense::click());
                if resp.clicked() {
                    clicked = Some(r);
                }
            }
        });
        if let Some(r) = clicked {
            tab.selected_annot = Some(r);
            if let (Some(geo), Some(info)) = (tab.doc.geometry(r.page).copied(), comments.iter().find(|c| c.page == r.page && c.index == r.index)) {
                let v = geo.pdf_rect_to_view(&info.rect).center();
                tab.current_page = r.page;
                tab.pending_scroll = Some(ScrollRequest::Point(r.page, v));
            }
        }
    }

    fn search_panel(&mut self, ui: &mut Ui) {
        let t = self.t();
        let mut start = false;
        ui.horizontal(|ui| {
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.search_query)
                    .hint_text(t.search_hint)
                    .desired_width(ui.available_width() - 30.0),
            );
            if self.focus_search {
                edit.request_focus();
                self.focus_search = false;
            }
            if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                start = true;
            }
            if icon_button(ui, Icon::Search, t.search, false, true).clicked() {
                start = true;
            }
        });
        ui.checkbox(&mut self.search_case, t.match_case);
        ui.checkbox(&mut self.search_word, t.whole_words);
        let query = self.search_query.clone();
        let opts = SearchOptions {
            match_case: self.search_case,
            whole_word: self.search_word,
        };
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        if start {
            if tab.search.query == query && tab.search.options == Some(opts) && !tab.search.hits.is_empty() {
                tab.next_hit(true);
            } else {
                tab.start_search(query, opts);
            }
        }
        ui.horizontal(|ui| {
            let n = tab.search.hits.len();
            if icon_button(ui, Icon::Prev, t.previous, false, n > 0).clicked() {
                tab.next_hit(false);
            }
            if icon_button(ui, Icon::Next, t.next, false, n > 0).clicked() {
                tab.next_hit(true);
            }
            let status = if tab.search.is_running() {
                format!("{} {n} …", t.hits)
            } else if tab.search.query.is_empty() {
                String::new()
            } else {
                format!("{} {n}", t.hits)
            };
            ui.label(status);
        });
        ui.separator();
        let mut reveal = None;
        let current = tab.search.current;
        ScrollArea::vertical().id_salt(("search", tab.id)).auto_shrink([false, false]).show_rows(
            ui,
            34.0,
            tab.search.hits.len(),
            |ui, range| {
                for i in range {
                    let hit = &tab.search.hits[i];
                    let text = format!("{} {}: {}", t.page, hit.page + 1, hit.context);
                    let resp = ui.add_sized(
                        [ui.available_width(), 30.0],
                        egui::Button::selectable(current == Some(i), RichText::new(text).small()).wrap(),
                    );
                    if resp.clicked() {
                        reveal = Some(i);
                    }
                }
            },
        );
        if let Some(i) = reveal {
            tab.search.current = Some(i);
            tab.reveal_hit(i);
        }
    }
}

pub fn kind_name_for_tool(t: &Strings, tool: crate::ui::tools::Tool) -> &'static str {
    use crate::ui::tools::Tool;
    match tool {
        Tool::Hand => t.tool_hand,
        Tool::SelectText => t.tool_select_text,
        Tool::Highlight => t.tool_highlight,
        Tool::Underline => t.tool_underline,
        Tool::StrikeOut => t.tool_strikeout,
        Tool::Squiggly => t.tool_squiggly,
        Tool::Note => t.tool_note,
        Tool::TextBox => t.tool_textbox,
        Tool::Rectangle => t.tool_rectangle,
        Tool::Ellipse => t.tool_ellipse,
        Tool::Line => t.tool_line,
        Tool::Arrow => t.tool_arrow,
        Tool::Pencil => t.tool_pencil,
        Tool::Stamp => t.tool_stamp,
    }
}

pub fn kind_name(t: &Strings, kind: AnnotKind) -> &'static str {
    match kind {
        AnnotKind::Note => t.tool_note,
        AnnotKind::FreeText => t.tool_textbox,
        AnnotKind::Line => t.tool_line,
        AnnotKind::Square => t.tool_rectangle,
        AnnotKind::Circle => t.tool_ellipse,
        AnnotKind::Polygon | AnnotKind::Polyline => t.kind_polygon,
        AnnotKind::Highlight => t.tool_highlight,
        AnnotKind::Underline => t.tool_underline,
        AnnotKind::Squiggly => t.tool_squiggly,
        AnnotKind::StrikeOut => t.tool_strikeout,
        AnnotKind::Stamp => t.tool_stamp,
        AnnotKind::Ink => t.tool_pencil,
        AnnotKind::FileAttachment => t.kind_attachment,
        _ => t.kind_other,
    }
}
