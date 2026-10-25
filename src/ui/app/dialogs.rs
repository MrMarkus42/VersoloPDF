//! Modal dialogs.

use egui::{Context, Id, Modal, RichText, Ui};

use super::actions::parse_page_range;
use super::{Dialog, InsertPosition, MrPdfApp, PaperChoice, RangePurpose};
use crate::model::AnnotKind;
use crate::ui::i18n::Strings;

enum Outcome {
    Keep,
    Close,
}

fn buttons(ui: &mut Ui, ok: &str, cancel: &str) -> (bool, bool) {
    let mut result = (false, false);
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        result.0 = ui.button(RichText::new(ok).strong()).clicked();
        result.1 = ui.button(cancel).clicked();
    });
    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);
    let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
    (result.0 || enter, result.1 || esc)
}

fn paper_name(t: &Strings, p: PaperChoice) -> &'static str {
    match p {
        PaperChoice::SameAsCurrent => t.same_as_current,
        PaperChoice::A4 => "A4",
        PaperChoice::A3 => "A3",
        PaperChoice::A5 => "A5",
        PaperChoice::Letter => "Letter",
        PaperChoice::Legal => "Legal",
    }
}

impl MrPdfApp {
    pub(super) fn dialogs(&mut self, ctx: &Context) {
        let Some(mut dialog) = self.dialog.take() else { return };
        let t = self.t();
        let mut outcome = Outcome::Keep;
        // Deferred actions that need `&mut self` after the modal closed.
        let mut action: Option<Box<dyn FnOnce(&mut MrPdfApp, &Context)>> = None;

        Modal::new(Id::new("mrpdf-dialog")).show(ctx, |ui| {
            ui.set_max_width(460.0);
            match &mut dialog {
                Dialog::Message { title, text } => {
                    ui.heading(title.as_str());
                    ui.label(text.as_str());
                    ui.add_space(8.0);
                    if ui.button(t.ok).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Escape)) {
                        outcome = Outcome::Close;
                    }
                }
                Dialog::Password { path, password, wrong } => {
                    ui.heading(t.password_title);
                    ui.label(path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
                    if *wrong {
                        ui.colored_label(egui::Color32::from_rgb(200, 40, 40), t.err_wrong_password);
                    }
                    let edit = ui.add(egui::TextEdit::singleline(password).password(true).hint_text(t.password));
                    edit.request_focus();
                    let (ok, cancel) = buttons(ui, t.open, t.cancel);
                    if ok {
                        let path = path.clone();
                        let pw = std::mem::take(password);
                        action = Some(Box::new(move |app, ctx| app.open_path(ctx, path, Some(pw))));
                        outcome = Outcome::Close;
                    } else if cancel {
                        outcome = Outcome::Close;
                    }
                }
                Dialog::Unsaved { tab_id, then } => {
                    let name = self
                        .tabs
                        .iter()
                        .find(|tab| tab.id == *tab_id)
                        .map(|tab| tab.doc.display_name().to_string())
                        .unwrap_or_default();
                    ui.heading(t.unsaved_title);
                    ui.label(format!("{} „{name}“?", t.unsaved_question));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let (id, then) = (*tab_id, *then);
                        if ui.button(RichText::new(t.save).strong()).clicked() {
                            action = Some(Box::new(move |app, ctx| app.after_unsaved(ctx, id, then, true)));
                            outcome = Outcome::Close;
                        }
                        if ui.button(t.dont_save).clicked() {
                            action = Some(Box::new(move |app, ctx| app.after_unsaved(ctx, id, then, false)));
                            outcome = Outcome::Close;
                        }
                        if ui.button(t.cancel).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            outcome = Outcome::Close;
                        }
                    });
                }
                Dialog::TextInput { page, rect, kind, text, edit } => {
                    ui.heading(if *kind == AnnotKind::Note { t.tool_note } else { t.tool_textbox });
                    let resp = ui.add(
                        egui::TextEdit::multiline(text)
                            .desired_rows(5)
                            .desired_width(400.0)
                            .hint_text(t.enter_text),
                    );
                    if !resp.has_focus() && text.is_empty() {
                        resp.request_focus();
                    }
                    ui.label(RichText::new(t.shift_enter_hint).small().weak());
                    let (ok, cancel) = buttons(ui, t.ok, t.cancel);
                    if ok {
                        let (page, rect, kind, edit) = (*page, *rect, *kind, *edit);
                        let text = text.trim_end_matches('\n').to_string();
                        action = Some(Box::new(move |app, ctx| app.finish_text_input(ctx, page, rect, kind, text, edit)));
                        outcome = Outcome::Close;
                    } else if cancel {
                        outcome = Outcome::Close;
                    }
                }
                Dialog::ConfirmLink { uri } => {
                    ui.heading(t.open_link_title);
                    ui.label(t.open_link_question);
                    ui.add(egui::Label::new(RichText::new(uri.as_str()).monospace()).wrap());
                    let (ok, cancel) = buttons(ui, t.open, t.cancel);
                    if ok {
                        ctx.open_url(egui::OpenUrl::new_tab(uri.clone()));
                        outcome = Outcome::Close;
                    } else if cancel {
                        outcome = Outcome::Close;
                    }
                }
                Dialog::Properties { meta } => {
                    ui.heading(t.doc_properties);
                    egui::Grid::new("props").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                        for (label, value) in [
                            (t.meta_title, &mut meta.title),
                            (t.author, &mut meta.author),
                            (t.meta_subject, &mut meta.subject),
                            (t.meta_keywords, &mut meta.keywords),
                        ] {
                            ui.label(label);
                            ui.add(egui::TextEdit::singleline(value).desired_width(280.0));
                            ui.end_row();
                        }
                        for (label, value) in [
                            (t.meta_creator, &meta.creator),
                            (t.meta_producer, &meta.producer),
                            (t.meta_created, &meta.created),
                            (t.meta_modified, &meta.modified),
                        ] {
                            ui.label(label);
                            ui.label(RichText::new(value.as_str()).weak());
                            ui.end_row();
                        }
                        if let Some(tab) = self.tabs.get(self.active) {
                            let perms = tab.doc.permissions();
                            ui.label(t.pages);
                            ui.label(tab.doc.page_count().to_string());
                            ui.end_row();
                            ui.label(t.file_size);
                            ui.label(format!("{:.1} KB", tab.doc.file_size() as f64 / 1024.0));
                            ui.end_row();
                            ui.label(t.security);
                            ui.label(if perms.encrypted { t.encrypted } else { t.not_encrypted });
                            ui.end_row();
                            let yes_no = |b: bool| if b { t.allowed } else { t.not_allowed };
                            for (label, value) in [
                                (t.perm_print, perms.print),
                                (t.perm_modify, perms.modify),
                                (t.perm_copy, perms.copy),
                                (t.perm_annotate, perms.annotate),
                                (t.perm_forms, perms.fill_forms),
                            ] {
                                ui.label(label);
                                ui.label(yes_no(value));
                                ui.end_row();
                            }
                        }
                    });
                    let (ok, cancel) = buttons(ui, t.save_properties, t.cancel);
                    if ok {
                        let meta = meta.clone();
                        action = Some(Box::new(move |app, ctx| {
                            let changed = app.active_tab().is_some_and(|tab| {
                                let cur = tab.doc.metadata();
                                cur.title != meta.title
                                    || cur.author != meta.author
                                    || cur.subject != meta.subject
                                    || cur.keywords != meta.keywords
                            });
                            if changed {
                                app.edit(ctx, |tab| tab.doc.set_metadata(&meta));
                            }
                        }));
                        outcome = Outcome::Close;
                    } else if cancel {
                        outcome = Outcome::Close;
                    }
                }
                Dialog::InsertBlank { count, paper, landscape, position } => {
                    ui.heading(t.insert_blank);
                    egui::Grid::new("insert").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                        ui.label(t.count);
                        ui.add(egui::DragValue::new(count).range(1..=500));
                        ui.end_row();
                        ui.label(t.paper_size);
                        egui::ComboBox::from_id_salt("paper").selected_text(paper_name(t, *paper)).show_ui(ui, |ui| {
                            for p in [
                                PaperChoice::SameAsCurrent,
                                PaperChoice::A4,
                                PaperChoice::A3,
                                PaperChoice::A5,
                                PaperChoice::Letter,
                                PaperChoice::Legal,
                            ] {
                                ui.selectable_value(paper, p, paper_name(t, p));
                            }
                        });
                        ui.end_row();
                        ui.label(t.orientation);
                        ui.horizontal(|ui| {
                            ui.radio_value(landscape, false, t.portrait);
                            ui.radio_value(landscape, true, t.landscape);
                        });
                        ui.end_row();
                        ui.label(t.position);
                        ui.vertical(|ui| {
                            ui.radio_value(position, InsertPosition::Before, t.before_current);
                            ui.radio_value(position, InsertPosition::After, t.after_current);
                            ui.radio_value(position, InsertPosition::End, t.at_end);
                        });
                        ui.end_row();
                    });
                    let (ok, cancel) = buttons(ui, t.insert, t.cancel);
                    if ok {
                        let (c, p, l, pos) = (*count, *paper, *landscape, *position);
                        action = Some(Box::new(move |app, ctx| app.insert_blank(ctx, c, p, l, pos)));
                        outcome = Outcome::Close;
                    } else if cancel {
                        outcome = Outcome::Close;
                    }
                }
                Dialog::PageRange { purpose, text, error } => {
                    let title = match purpose {
                        RangePurpose::Extract => t.extract_pages,
                        RangePurpose::Delete => t.delete_pages,
                        RangePurpose::Rotate(_) => t.rotate_range,
                        RangePurpose::Flatten => t.flatten,
                    };
                    ui.heading(title);
                    if let RangePurpose::Rotate(delta) = purpose {
                        ui.horizontal(|ui| {
                            ui.radio_value(delta, -90, t.rotate_left);
                            ui.radio_value(delta, 90, t.rotate_right);
                            ui.radio_value(delta, 180, "180°");
                        });
                    }
                    if *purpose == RangePurpose::Flatten {
                        ui.label(RichText::new(t.flatten_hint).weak());
                    }
                    ui.label(t.page_range_label);
                    ui.add(egui::TextEdit::singleline(text).hint_text("1-3, 5, 8-"));
                    if *error {
                        ui.colored_label(egui::Color32::from_rgb(200, 40, 40), t.invalid_range);
                    }
                    let (ok, cancel) = buttons(ui, t.ok, t.cancel);
                    if ok {
                        let count = self.active_tab().map_or(0, |tab| tab.doc.page_count());
                        match parse_page_range(text, count) {
                            Some(pages) => {
                                let purpose = *purpose;
                                action = Some(Box::new(move |app, ctx| app.apply_range(ctx, purpose, pages)));
                                outcome = Outcome::Close;
                            }
                            None => *error = true,
                        }
                    } else if cancel {
                        outcome = Outcome::Close;
                    }
                }
                Dialog::ExportImage { range, dpi, jpeg, error } => {
                    ui.heading(t.export_image);
                    egui::Grid::new("export").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                        ui.label(t.page_range_label);
                        ui.text_edit_singleline(range);
                        ui.end_row();
                        ui.label(t.resolution);
                        ui.add(egui::DragValue::new(dpi).range(36..=600).suffix(" dpi"));
                        ui.end_row();
                        ui.label(t.format);
                        ui.horizontal(|ui| {
                            ui.radio_value(jpeg, false, "PNG");
                            ui.radio_value(jpeg, true, "JPEG");
                        });
                        ui.end_row();
                    });
                    if *error {
                        ui.colored_label(egui::Color32::from_rgb(200, 40, 40), t.invalid_range);
                    }
                    let (ok, cancel) = buttons(ui, t.export, t.cancel);
                    if ok {
                        let count = self.active_tab().map_or(0, |tab| tab.doc.page_count());
                        match parse_page_range(range, count) {
                            Some(pages) => {
                                let (dpi, jpeg) = (*dpi, *jpeg);
                                action = Some(Box::new(move |app, ctx| app.export_images(ctx, pages, dpi, jpeg)));
                                outcome = Outcome::Close;
                            }
                            None => *error = true,
                        }
                    } else if cancel {
                        outcome = Outcome::Close;
                    }
                }
                Dialog::Settings { author } => {
                    ui.heading(t.settings);
                    ui.label(t.author_name);
                    ui.text_edit_singleline(author);
                    ui.label(RichText::new(t.author_hint).small().weak());
                    let (ok, cancel) = buttons(ui, t.ok, t.cancel);
                    if ok {
                        self.settings.author = author.chars().filter(|c| !c.is_control()).take(128).collect();
                        outcome = Outcome::Close;
                    } else if cancel {
                        outcome = Outcome::Close;
                    }
                }
                Dialog::Print { range, copies, error } => {
                    ui.heading(t.print);
                    egui::Grid::new("print").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                        ui.label(t.page_range_label);
                        ui.text_edit_singleline(range);
                        ui.end_row();
                        ui.label(t.copies);
                        ui.add(egui::DragValue::new(copies).range(1..=99));
                        ui.end_row();
                    });
                    ui.label(RichText::new(t.print_hint).small().weak());
                    if *error {
                        ui.colored_label(egui::Color32::from_rgb(200, 40, 40), t.invalid_range);
                    }
                    let (ok, cancel) = buttons(ui, t.print, t.cancel);
                    if ok {
                        let count = self.active_tab().map_or(0, |tab| tab.doc.page_count());
                        match parse_page_range(range, count) {
                            Some(pages) => {
                                let copies = *copies;
                                action = Some(Box::new(move |app, ctx| app.print(ctx, pages, copies)));
                                outcome = Outcome::Close;
                            }
                            None => *error = true,
                        }
                    } else if cancel {
                        outcome = Outcome::Close;
                    }
                }
                Dialog::Choice { page, annot_index, options, selected } => {
                    ui.heading(t.choose_value);
                    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                        for option in options.iter() {
                            ui.selectable_value(selected, option.clone(), option.as_str());
                        }
                    });
                    let (ok, cancel) = buttons(ui, t.ok, t.cancel);
                    if ok {
                        let (page, index, value) = (*page, *annot_index, selected.clone());
                        action = Some(Box::new(move |app, ctx| app.edit(ctx, |tab| tab.doc.set_choice(page, index, &value))));
                        outcome = Outcome::Close;
                    } else if cancel {
                        outcome = Outcome::Close;
                    }
                }
                Dialog::About => {
                    ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(48.0, 48.0), egui::Sense::hover());
                        crate::ui::icons::paint(ui.painter(), rect, crate::ui::icons::Icon::Logo, crate::ui::theme::BRAND);
                        ui.vertical(|ui| {
                            ui.heading(format!("MrPDF {}", env!("CARGO_PKG_VERSION")));
                            ui.label(t.about_text);
                        });
                    });
                    ui.separator();
                    ui.label(t.license_text);
                    ui.label(RichText::new(t.third_party_text).small());
                    ui.add_space(8.0);
                    if ui.button(t.ok).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        outcome = Outcome::Close;
                    }
                }
            }
        });

        if matches!(outcome, Outcome::Keep) {
            self.dialog = Some(dialog);
        }
        if let Some(action) = action {
            action(self, ctx);
        }
    }
}
