//! Quick access toolbar and ribbon.

use egui::{Frame, Margin, Panel, RichText, Ui};

use super::{Dialog, InsertPosition, MrPdfApp, PaperChoice, RangePurpose, RibbonTab};
use crate::model::StampPreset;
use crate::ui::i18n::Language;
use crate::ui::icons::Icon;
use crate::ui::theme;
use crate::ui::tools::Tool;
use crate::ui::view::{PageLayout, ZoomMode};
use crate::ui::widgets::{big_button, group, icon_button, small_button};

impl MrPdfApp {
    pub(super) fn ribbon_ui(&mut self, ui: &mut Ui) {
        let t = self.t();
        let dark = self.settings.dark_mode;
        let ctx = ui.ctx().clone();
        Panel::top("ribbon")
            .frame(Frame::new().fill(theme::ribbon_background(dark)).inner_margin(Margin::symmetric(6, 3)))
            .show(ui, |ui| {
                // Row 1: file menu, quick access toolbar, ribbon tabs.
                ui.horizontal(|ui| {
                    self.file_menu(ui);
                    ui.separator();
                    let has_doc = !self.tabs.is_empty();
                    let can_undo = self.active_tab().and_then(|t| t.doc.can_undo()).is_some();
                    let can_redo = self.active_tab().and_then(|t| t.doc.can_redo()).is_some();
                    if icon_button(ui, Icon::Open, t.open_file, false, true).clicked() {
                        self.open_dialog();
                    }
                    if icon_button(ui, Icon::Save, t.save, false, has_doc).clicked() {
                        self.save(&ctx, None);
                    }
                    if icon_button(ui, Icon::Print, t.print, false, has_doc).clicked() {
                        self.open_print_dialog(&ctx);
                    }
                    if icon_button(ui, Icon::Undo, t.undo, false, can_undo).clicked() {
                        self.undo(&ctx);
                    }
                    if icon_button(ui, Icon::Redo, t.redo, false, can_redo).clicked() {
                        self.redo(&ctx);
                    }
                    ui.separator();
                    for (tab, label) in [
                        (RibbonTab::Home, t.tab_home),
                        (RibbonTab::Comment, t.tab_comment),
                        (RibbonTab::Organize, t.tab_organize),
                        (RibbonTab::Form, t.tab_form),
                        (RibbonTab::Protect, t.tab_protect),
                        (RibbonTab::View, t.tab_view),
                        (RibbonTab::Help, t.tab_help),
                    ] {
                        let text = RichText::new(label).size(13.0);
                        if ui.selectable_label(self.ribbon == tab, text).clicked() {
                            self.ribbon = tab;
                        }
                    }
                });
                ui.add_space(2.0);
                egui::ScrollArea::horizontal()
                    .id_salt("ribbon-scroll")
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.set_min_height(82.0);
                            match self.ribbon {
                                RibbonTab::Home => self.ribbon_home(ui),
                                RibbonTab::Comment => self.ribbon_comment(ui),
                                RibbonTab::Organize => self.ribbon_organize(ui),
                                RibbonTab::Form => self.ribbon_form(ui),
                                RibbonTab::Protect => self.ribbon_protect(ui),
                                RibbonTab::View => self.ribbon_view(ui),
                                RibbonTab::Help => self.ribbon_help(ui),
                            }
                        });
                    });
            });
    }

    fn file_menu(&mut self, ui: &mut Ui) {
        let t = self.t();
        let ctx = ui.ctx().clone();
        let has_doc = !self.tabs.is_empty();
        let button = egui::Button::new(RichText::new(t.menu_file).color(egui::Color32::WHITE).strong())
            .fill(theme::BRAND);
        egui::containers::menu::MenuButton::from_button(button).ui(ui, |ui| {
            ui.set_min_width(220.0);
            if ui.button(t.new_document).clicked() {
                self.new_document(&ctx);
                ui.close();
            }
            if ui.button(format!("{}…", t.open_file)).clicked() {
                ui.close();
                self.open_dialog();
            }
            ui.menu_button(t.recent_files, |ui| {
                if self.settings.recent_files.is_empty() {
                    ui.label(t.none);
                }
                for path in self.settings.recent_files.clone() {
                    if ui.button(path.display().to_string()).clicked() {
                        self.pending_open.push_back(path);
                        ui.close();
                    }
                }
            });
            ui.separator();
            if ui.add_enabled(has_doc, egui::Button::new(t.save)).clicked() {
                ui.close();
                self.save(&ctx, None);
            }
            if ui.add_enabled(has_doc, egui::Button::new(format!("{}…", t.save_as))).clicked() {
                ui.close();
                self.save_as(&ctx, None);
            }
            if ui.add_enabled(has_doc, egui::Button::new(format!("{}…", t.print))).clicked() {
                ui.close();
                self.open_print_dialog(&ctx);
            }
            if ui.add_enabled(has_doc, egui::Button::new(format!("{}…", t.export_image))).clicked() {
                ui.close();
                if let Some(tab) = self.active_tab() {
                    self.dialog = Some(Dialog::ExportImage {
                        range: (tab.current_page + 1).to_string(),
                        dpi: 150,
                        jpeg: false,
                        error: false,
                    });
                }
            }
            ui.separator();
            if ui.add_enabled(has_doc, egui::Button::new(format!("{}…", t.doc_properties))).clicked() {
                ui.close();
                self.open_properties();
            }
            if ui.button(format!("{}…", t.settings)).clicked() {
                ui.close();
                self.dialog = Some(Dialog::Settings { author: self.settings.author.clone() });
            }
            ui.separator();
            if ui.add_enabled(has_doc, egui::Button::new(t.close)).clicked() {
                ui.close();
                self.request_close_tab(&ctx, self.active);
            }
            if ui.button(t.exit).clicked() {
                ui.close();
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    }

    fn tool_button(&mut self, ui: &mut Ui, tool: Tool, label: &str, big: bool) {
        let enabled = !self.tabs.is_empty();
        let selected = self.tool == tool;
        let response = if big {
            big_button(ui, tool.icon(), label, selected, enabled)
        } else {
            small_button(ui, tool.icon(), label, selected, enabled)
        };
        if response.clicked() {
            self.select_tool(tool);
        }
    }

    fn zoom_group(&mut self, ui: &mut Ui) {
        let t = self.t();
        let has_doc = !self.tabs.is_empty();
        let mode = self.active_tab().map(|t| t.zoom_mode);
        group(ui, t.group_zoom, |ui| {
            if big_button(ui, Icon::FitWidth, t.fit_width, mode == Some(ZoomMode::FitWidth), has_doc)
                .clicked()
            {
                self.set_zoom_mode(ZoomMode::FitWidth);
            }
            if big_button(ui, Icon::FitPage, t.fit_page, mode == Some(ZoomMode::FitPage), has_doc).clicked() {
                self.set_zoom_mode(ZoomMode::FitPage);
            }
            ui.vertical(|ui| {
                if small_button(ui, Icon::ActualSize, t.actual_size, false, has_doc).clicked() {
                    self.set_zoom(1.0);
                }
                if small_button(ui, Icon::ZoomIn, t.zoom_in, false, has_doc).clicked() {
                    self.zoom_step(true);
                }
                if small_button(ui, Icon::ZoomOut, t.zoom_out, false, has_doc).clicked() {
                    self.zoom_step(false);
                }
            });
        });
    }

    fn ribbon_home(&mut self, ui: &mut Ui) {
        let t = self.t();
        let ctx = ui.ctx().clone();
        let has_doc = !self.tabs.is_empty();
        group(ui, t.group_tools, |ui| {
            self.tool_button(ui, Tool::Hand, t.tool_hand, true);
            self.tool_button(ui, Tool::SelectText, t.tool_select_text, true);
        });
        self.zoom_group(ui);
        group(ui, t.group_comment, |ui| {
            self.tool_button(ui, Tool::Highlight, t.tool_highlight, true);
            self.tool_button(ui, Tool::Note, t.tool_note, true);
            self.tool_button(ui, Tool::TextBox, t.tool_textbox, true);
            ui.vertical(|ui| {
                self.tool_button(ui, Tool::Underline, t.tool_underline, false);
                self.tool_button(ui, Tool::StrikeOut, t.tool_strikeout, false);
                self.tool_button(ui, Tool::Stamp, t.tool_stamp, false);
            });
        });
        group(ui, t.group_pages, |ui| {
            if big_button(ui, Icon::RotateLeft, t.rotate_left, false, has_doc).clicked() {
                self.rotate_current(&ctx, -90);
            }
            if big_button(ui, Icon::RotateRight, t.rotate_right, false, has_doc).clicked() {
                self.rotate_current(&ctx, 90);
            }
        });
        group(ui, t.group_find, |ui| {
            if big_button(ui, Icon::Search, t.search, false, has_doc).clicked() {
                self.settings.show_sidebar = true;
                self.side = super::SidePanel::Search;
                self.focus_search = true;
            }
            if big_button(ui, Icon::Copy, t.copy, false, has_doc).clicked() {
                self.copy_selection(&ctx);
            }
        });
    }

    fn ribbon_comment(&mut self, ui: &mut Ui) {
        let t = self.t();
        let ctx = ui.ctx().clone();
        let has_doc = !self.tabs.is_empty();
        group(ui, t.group_text_markup, |ui| {
            self.tool_button(ui, Tool::Highlight, t.tool_highlight, true);
            ui.vertical(|ui| {
                self.tool_button(ui, Tool::Underline, t.tool_underline, false);
                self.tool_button(ui, Tool::StrikeOut, t.tool_strikeout, false);
                self.tool_button(ui, Tool::Squiggly, t.tool_squiggly, false);
            });
        });
        group(ui, t.group_text, |ui| {
            self.tool_button(ui, Tool::Note, t.tool_note, true);
            self.tool_button(ui, Tool::TextBox, t.tool_textbox, true);
        });
        group(ui, t.group_drawing, |ui| {
            ui.vertical(|ui| {
                self.tool_button(ui, Tool::Rectangle, t.tool_rectangle, false);
                self.tool_button(ui, Tool::Ellipse, t.tool_ellipse, false);
                self.tool_button(ui, Tool::Pencil, t.tool_pencil, false);
            });
            ui.vertical(|ui| {
                self.tool_button(ui, Tool::Line, t.tool_line, false);
                self.tool_button(ui, Tool::Arrow, t.tool_arrow, false);
            });
        });
        group(ui, t.group_stamps, |ui| {
            self.tool_button(ui, Tool::Stamp, t.tool_stamp, true);
            ui.vertical(|ui| {
                let current = self.settings.stamp;
                egui::ComboBox::from_id_salt("stamp-preset")
                    .selected_text(t.stamp_label(current))
                    .width(130.0)
                    .show_ui(ui, |ui| {
                        for preset in StampPreset::ALL {
                            if ui.selectable_label(current == preset, t.stamp_label(preset)).clicked() {
                                self.settings.stamp = preset;
                                let mut style = self.settings.style(Tool::Stamp);
                                style.color = preset.color();
                                self.settings.set_style(Tool::Stamp, style);
                                self.select_tool(Tool::Stamp);
                            }
                        }
                    });
            });
        });
        group(ui, t.group_manage, |ui| {
            let comments_open = self.settings.show_sidebar && self.side == super::SidePanel::Comments;
            if big_button(ui, Icon::Comments, t.comment_list, comments_open, has_doc).clicked() {
                if comments_open {
                    self.settings.show_sidebar = false;
                } else {
                    self.settings.show_sidebar = true;
                    self.side = super::SidePanel::Comments;
                }
            }
            if big_button(ui, Icon::Flatten, t.flatten, false, has_doc).clicked()
                && let Some(tab) = self.active_tab()
            {
                self.dialog = Some(Dialog::PageRange {
                    purpose: RangePurpose::Flatten,
                    text: format!("1-{}", tab.doc.page_count()),
                    error: false,
                });
            }
            let has_sel = self.active_tab().is_some_and(|t| t.selected_annot.is_some());
            if big_button(ui, Icon::Delete, t.delete_comment, false, has_sel).clicked() {
                self.delete_selected_annotation(&ctx);
            }
        });
    }

    fn ribbon_organize(&mut self, ui: &mut Ui) {
        let t = self.t();
        let ctx = ui.ctx().clone();
        let has_doc = !self.tabs.is_empty();
        let (cur, count) = self.active_tab().map_or((0, 0), |t| (t.current_page, t.doc.page_count()));
        group(ui, t.group_insert, |ui| {
            if big_button(ui, Icon::InsertPage, t.insert_blank, false, has_doc).clicked() {
                self.dialog = Some(Dialog::InsertBlank {
                    count: 1,
                    paper: PaperChoice::SameAsCurrent,
                    landscape: false,
                    position: InsertPosition::After,
                });
            }
            if big_button(ui, Icon::InsertFile, t.insert_from_file, false, has_doc).clicked() {
                self.insert_from_file(&ctx, InsertPosition::After);
            }
            if big_button(ui, Icon::InsertFile, t.append_file, false, has_doc).clicked() {
                self.insert_from_file(&ctx, InsertPosition::End);
            }
        });
        group(ui, t.group_pages, |ui| {
            if big_button(ui, Icon::DeletePage, t.delete_pages, false, has_doc && count > 1).clicked() {
                self.dialog = Some(Dialog::PageRange {
                    purpose: RangePurpose::Delete,
                    text: (cur + 1).to_string(),
                    error: false,
                });
            }
            if big_button(ui, Icon::ExtractPages, t.extract_pages, false, has_doc).clicked() {
                self.dialog = Some(Dialog::PageRange {
                    purpose: RangePurpose::Extract,
                    text: (cur + 1).to_string(),
                    error: false,
                });
            }
            ui.vertical(|ui| {
                if small_button(ui, Icon::RotateLeft, t.rotate_left, false, has_doc).clicked() {
                    self.rotate_current(&ctx, -90);
                }
                if small_button(ui, Icon::RotateRight, t.rotate_right, false, has_doc).clicked() {
                    self.rotate_current(&ctx, 90);
                }
                if small_button(ui, Icon::RotateRight, t.rotate_range, false, has_doc).clicked() {
                    self.dialog = Some(Dialog::PageRange {
                        purpose: RangePurpose::Rotate(90),
                        text: format!("1-{count}"),
                        error: false,
                    });
                }
            });
        });
        group(ui, t.group_order, |ui| {
            if big_button(ui, Icon::MoveUp, t.move_up, false, has_doc && cur > 0).clicked() {
                self.move_current_page(&ctx, true);
            }
            if big_button(ui, Icon::MoveDown, t.move_down, false, has_doc && cur + 1 < count).clicked() {
                self.move_current_page(&ctx, false);
            }
        });
    }

    fn ribbon_form(&mut self, ui: &mut Ui) {
        let t = self.t();
        let has_doc = !self.tabs.is_empty();
        group(ui, t.group_forms, |ui| {
            let on = self.settings.highlight_fields;
            if big_button(ui, Icon::Form, t.highlight_fields, on, has_doc).clicked() {
                self.settings.highlight_fields = !on;
            }
            self.tool_button(ui, Tool::Hand, t.fill_forms, true);
        });
        ui.label(RichText::new(t.forms_hint).weak());
    }

    fn ribbon_protect(&mut self, ui: &mut Ui) {
        let t = self.t();
        let ctx = ui.ctx().clone();
        let (has_doc, encrypted, full) = self
            .active_tab()
            .map_or((false, false, false), |tab| (true, tab.doc.is_encrypted(), tab.doc.has_full_access()));
        group(ui, t.group_security, |ui| {
            if big_button(ui, Icon::Lock, t.password_protect, false, has_doc && full).clicked() {
                self.dialog = Some(Dialog::Protect {
                    opts: crate::security::ProtectOptions::default(),
                    repeat: String::new(),
                    error: None,
                });
            }
            if big_button(ui, Icon::Unlock, t.remove_protection, false, encrypted && full).clicked() {
                self.edit(&ctx, |tab| tab.doc.unprotect());
                self.set_status(&ctx, t.protection_removed);
            }
            if big_button(ui, Icon::Info, t.properties, false, has_doc).clicked() {
                self.open_properties();
            }
        });
        group(ui, t.group_attachments, |ui| {
            if big_button(ui, Icon::Attachment, t.attachments, false, has_doc).clicked() {
                self.settings.show_sidebar = true;
                self.side = super::SidePanel::Attachments;
            }
        });
        ui.label(RichText::new(if encrypted { t.status_encrypted } else { t.status_not_encrypted }).weak());
    }

    fn ribbon_view(&mut self, ui: &mut Ui) {
        let t = self.t();
        let has_doc = !self.tabs.is_empty();
        let layout = self.active_tab().map(|t| t.layout_mode);
        group(ui, t.group_layout, |ui| {
            for (mode, icon, label) in [
                (PageLayout::Single, Icon::SinglePage, t.layout_single),
                (PageLayout::Continuous, Icon::Continuous, t.layout_continuous),
                (PageLayout::Facing, Icon::TwoPages, t.layout_facing),
            ] {
                if big_button(ui, icon, label, layout == Some(mode), has_doc).clicked() {
                    self.set_layout(mode);
                }
            }
        });
        self.zoom_group(ui);
        group(ui, t.group_panels, |ui| {
            let side = self.settings.show_sidebar;
            if big_button(ui, Icon::Sidebar, t.sidebar, side, true).clicked() {
                self.settings.show_sidebar = !side;
            }
            let full = ui.input(|i| i.viewport().fullscreen.unwrap_or(false));
            if big_button(ui, Icon::Fullscreen, t.fullscreen, full, true).clicked() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Fullscreen(!full));
            }
            let props = self.settings.show_properties;
            if big_button(ui, Icon::Properties, t.properties, props, true).clicked() {
                self.settings.show_properties = !props;
            }
        });
        group(ui, t.group_appearance, |ui| {
            let dark = self.settings.dark_mode;
            if big_button(ui, Icon::Theme, t.dark_mode, dark, true).clicked() {
                self.settings.dark_mode = !dark;
            }
            ui.vertical(|ui| {
                ui.label(t.language);
                let current = self.settings.language;
                egui::ComboBox::from_id_salt("language").selected_text(current.name()).show_ui(ui, |ui| {
                    for lang in Language::ALL {
                        if ui.selectable_label(current == lang, lang.name()).clicked() {
                            self.settings.language = lang;
                        }
                    }
                });
            });
        });
    }

    fn ribbon_help(&mut self, ui: &mut Ui) {
        let t = self.t();
        group(ui, t.tab_help, |ui| {
            if big_button(ui, Icon::Info, t.about, false, true).clicked() {
                self.dialog = Some(Dialog::About);
            }
        });
        ui.vertical(|ui| {
            ui.label(RichText::new(t.shortcuts_title).strong());
            ui.label(RichText::new(t.shortcuts_text).small());
        });
    }
}
