//! Modal dialogs.

use egui::{Context, Id, Modal, RichText, Ui};

use super::actions::parse_page_range;
use super::{Dialog, MrPdfApp, RangePurpose};

/// Work to do after the modal closed (needs `&mut MrPdfApp`).
pub(super) type Action = Box<dyn FnOnce(&mut MrPdfApp, &Context)>;

pub(super) enum Outcome {
    Keep,
    Close,
}

pub(super) fn buttons(ui: &mut Ui, ok: &str, cancel: &str) -> (bool, bool) {
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

impl MrPdfApp {
    pub(super) fn dialogs(&mut self, ctx: &Context) {
        let Some(mut dialog) = self.dialog.take() else { return };
        let t = self.t();
        let mut outcome = Outcome::Keep;
        // Deferred actions that need `&mut self` after the modal closed.
        let mut action: Option<Action> = None;

        Modal::new(Id::new("mrpdf-dialog")).show(ctx, |ui| {
            ui.set_max_width(460.0);
            match &mut dialog {
                Dialog::Message { title, text } => {
                    ui.heading(title.as_str());
                    ui.label(text.as_str());
                    ui.add_space(8.0);
                    if ui.button(t.ok).clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Escape))
                    {
                        outcome = Outcome::Close;
                    }
                }
                Dialog::Password { path, password, wrong } => {
                    ui.heading(t.password_title);
                    ui.label(path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
                    if *wrong {
                        ui.colored_label(egui::Color32::from_rgb(200, 40, 40), t.err_wrong_password);
                    }
                    let edit =
                        ui.add(egui::TextEdit::singleline(password).password(true).hint_text(t.password));
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
                    // Task dialog: page picture, question and one
                    // full-width button per answer.
                    let (id, then) = (*tab_id, *then);
                    let tab = self.tabs.iter().find(|tab| tab.id == id);
                    let name = tab.map(|tab| tab.doc.display_name().to_string()).unwrap_or_default();
                    ui.horizontal_top(|ui| {
                        if let Some(tab) = tab {
                            super::preview::doc_preview(
                                ui,
                                &mut self.preview[0],
                                &tab.doc,
                                tab.id,
                                tab.current_page,
                                egui::vec2(96.0, 128.0),
                            );
                        }
                        ui.vertical(|ui| {
                            ui.set_width(330.0);
                            ui.label(
                                RichText::new(format!("{} „{name}“ {}", t.unsaved_doc, t.unsaved_changed))
                                    .size(15.0)
                                    .color(ui.visuals().strong_text_color()),
                            );
                            ui.label(t.unsaved_question);
                            ui.add_space(6.0);
                            let answer = |ui: &mut Ui, text: &str| {
                                ui.add_sized(
                                    [ui.available_width(), 28.0],
                                    egui::Button::new(format!("›  {text}"))
                                        .wrap_mode(egui::TextWrapMode::Truncate),
                                )
                                .clicked()
                            };
                            if answer(ui, t.yes) {
                                action =
                                    Some(Box::new(move |app, ctx| app.after_unsaved(ctx, id, then, true)));
                                outcome = Outcome::Close;
                            }
                            if answer(ui, t.no) {
                                action =
                                    Some(Box::new(move |app, ctx| app.after_unsaved(ctx, id, then, false)));
                                outcome = Outcome::Close;
                            }
                            if answer(ui, t.cancel) || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                outcome = Outcome::Close;
                            }
                        });
                    });
                }
                Dialog::TextInput { page, tool, template, text, edit } => {
                    ui.heading(super::commands::Cmd::Tool(*tool).label(self.settings.language));
                    if *tool == crate::ui::tools::Tool::Replace
                        && let Some(original) =
                            template.as_ref().map(|t| t.label.clone()).filter(|s| !s.is_empty())
                    {
                        ui.label(RichText::new(format!("{}: „{original}“", t.replace_original)).weak());
                    }
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
                        let text = text.trim_end_matches('\n').to_string();
                        let (page, tool) = (*page, *tool);
                        match (edit.take(), template.take()) {
                            (Some(annot), _) => {
                                action =
                                    Some(Box::new(move |app, ctx| app.finish_text_edit(ctx, annot, text)));
                            }
                            (None, Some(template)) => {
                                action = Some(Box::new(move |app, ctx| {
                                    app.finish_text_template(ctx, page, tool, template, text)
                                }));
                            }
                            (None, None) => {}
                        }
                        outcome = Outcome::Close;
                    } else if cancel {
                        outcome = Outcome::Close;
                    }
                }
                Dialog::Link { page, rect, url, target_page, to_page } => {
                    ui.heading(t.create_link);
                    ui.radio_value(to_page, false, t.link_to_url);
                    ui.add_enabled(!*to_page, egui::TextEdit::singleline(url).desired_width(360.0));
                    ui.radio_value(to_page, true, t.link_to_page);
                    let count = self.active_tab().map_or(1, |tab| tab.doc.page_count()) as u32;
                    ui.add_enabled(*to_page, egui::DragValue::new(target_page).range(1..=count.max(1)));
                    let valid = *to_page || super::actions::is_safe_uri(url);
                    if !valid {
                        ui.colored_label(egui::Color32::from_rgb(200, 40, 40), t.link_blocked);
                    }
                    let (ok, cancel) = buttons(ui, t.ok, t.cancel);
                    if ok && valid {
                        let spec = if *to_page {
                            crate::pdf_edit::LinkSpec::Page(target_page.saturating_sub(1) as usize)
                        } else {
                            crate::pdf_edit::LinkSpec::Uri(url.trim().to_string())
                        };
                        let (page, rect) = (*page, *rect);
                        action = Some(Box::new(move |app, ctx| app.create_link(ctx, page, rect, spec)));
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
                Dialog::PageRange { purpose, text, error } => {
                    let title = match purpose {
                        RangePurpose::Extract => t.extract_pages,
                        RangePurpose::Flatten => t.flatten,
                        RangePurpose::ToImages => {
                            super::commands::Cmd::ContentsToImages.label(self.settings.language)
                        }
                    };
                    ui.heading(title);
                    if *purpose == RangePurpose::Flatten {
                        ui.label(RichText::new(t.flatten_hint).weak());
                    }
                    if *purpose == RangePurpose::ToImages {
                        ui.label(RichText::new(t.to_images_hint).weak());
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
                                action =
                                    Some(Box::new(move |app, ctx| app.export_images(ctx, pages, dpi, jpeg)));
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
                        action = Some(Box::new(move |app, ctx| {
                            app.edit(ctx, |tab| tab.doc.set_choice(page, index, &value))
                        }));
                        outcome = Outcome::Close;
                    } else if cancel {
                        outcome = Outcome::Close;
                    }
                }
                Dialog::Protect { opts, repeat, error } => {
                    ui.heading(t.password_protect);
                    egui::Grid::new("protect").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                        ui.label(t.user_password);
                        ui.add(egui::TextEdit::singleline(&mut opts.user_password).password(true));
                        ui.end_row();
                        ui.label(t.repeat_password);
                        ui.add(egui::TextEdit::singleline(repeat).password(true));
                        ui.end_row();
                        ui.label(t.owner_password);
                        ui.add(egui::TextEdit::singleline(&mut opts.owner_password).password(true));
                        ui.end_row();
                    });
                    ui.checkbox(&mut opts.allow_print, t.allow_print);
                    ui.checkbox(&mut opts.allow_copy, t.allow_copy);
                    ui.checkbox(&mut opts.allow_modify, t.allow_modify);
                    ui.checkbox(&mut opts.allow_annotate, t.allow_annotate);
                    ui.checkbox(&mut opts.allow_fill_forms, t.allow_fill);
                    ui.checkbox(&mut opts.allow_assemble, t.allow_assemble);
                    ui.label(RichText::new(t.protect_hint).small().weak());
                    if let Some(msg) = error {
                        ui.colored_label(egui::Color32::from_rgb(200, 40, 40), *msg);
                    }
                    let (ok, cancel) = buttons(ui, t.ok, t.cancel);
                    if ok {
                        if opts.user_password != *repeat {
                            *error = Some(t.passwords_differ);
                        } else if opts.user_password.is_empty() && opts.owner_password.is_empty() {
                            *error = Some(t.password_needed);
                        } else {
                            let opts = opts.clone();
                            action = Some(Box::new(move |app, ctx| {
                                app.edit(ctx, |tab| tab.doc.protect(&opts));
                                let t = app.t();
                                app.set_status(ctx, t.protected_msg);
                            }));
                            outcome = Outcome::Close;
                        }
                    } else if cancel {
                        outcome = Outcome::Close;
                    }
                }
                Dialog::BookmarkTitle { parent, path, page, title } => {
                    ui.heading(if path.is_some() { t.rename } else { t.add_bookmark });
                    let edit = ui.add(egui::TextEdit::singleline(title).desired_width(320.0));
                    edit.request_focus();
                    if path.is_none() {
                        ui.label(RichText::new(format!("{} {}", t.bookmark_target, *page + 1)).weak());
                    }
                    let (ok, cancel) = buttons(ui, t.ok, t.cancel);
                    if ok && !title.trim().is_empty() {
                        let title = title.trim().to_string();
                        let (parent, path, page) = (parent.clone(), path.clone(), *page);
                        action = Some(Box::new(move |app, ctx| match (parent, path) {
                            (_, Some(path)) => app.edit(ctx, |tab| tab.doc.rename_bookmark(&path, &title)),
                            (Some(parent), None) => {
                                app.edit(ctx, |tab| tab.doc.add_bookmark(&parent, &title, page))
                            }
                            (None, None) => {}
                        }));
                        outcome = Outcome::Close;
                    } else if cancel {
                        outcome = Outcome::Close;
                    }
                }
                other @ (Dialog::Swap { .. } | Dialog::Duplicates { .. }) => {
                    let (o, a) = self.structure_dialog(ui, other);
                    outcome = o;
                    action = a;
                }
                other @ (Dialog::Confirm { .. } | Dialog::NewField(_) | Dialog::SearchRedact { .. }) => {
                    let (o, a) = self.forms_dialog(ui, other);
                    outcome = o;
                    action = a;
                }
                other @ (Dialog::BookmarkEveryNth { .. }
                | Dialog::BookmarkReplace { .. }
                | Dialog::BookmarkSort { .. }) => {
                    let (o, a) = self.outline_dialog(ui, other);
                    outcome = o;
                    action = a;
                }
                Dialog::Pages(state) => {
                    let (o, a) = self.page_dialog(ui, state);
                    outcome = o;
                    action = a;
                }
                Dialog::Layout(state) => {
                    let (o, a) = self.layout_dialog(ui, state);
                    outcome = o;
                    action = a;
                }
                Dialog::Shape(state) => {
                    let (o, a) = self.shape_dialog(ui, state);
                    outcome = o;
                    action = a;
                }
                Dialog::Marks(state) => {
                    let (o, a) = self.marks_dialog(ui, state);
                    outcome = o;
                    action = a;
                }
                Dialog::Labels(state) => {
                    let (o, a) = self.labels_dialog(ui, state);
                    outcome = o;
                    action = a;
                }
                Dialog::About => {
                    ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(48.0, 48.0), egui::Sense::hover());
                        crate::ui::icons::paint(
                            ui.painter(),
                            rect,
                            crate::ui::icons::Icon::Logo,
                            crate::ui::theme::BRAND,
                        );
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
                Dialog::Optimize(state) => {
                    let (o, a) = self.optimize_dialog(ui, state);
                    outcome = o;
                    action = a;
                }
                Dialog::SpaceUsage(usage) => {
                    let (o, a) = self.space_usage_dialog(ui, usage);
                    outcome = o;
                    action = a;
                }
                Dialog::RenameFile { name, error } => {
                    let (o, a) = self.rename_dialog(ui, name, error);
                    outcome = o;
                    action = a;
                }
                Dialog::OpenUrl { url } => {
                    let de = self.settings.language == crate::ui::i18n::Language::German;
                    ui.heading(if de { "Web-Seite öffnen" } else { "Open web page" });
                    ui.label(if de { "URL eingeben:" } else { "Enter URL:" });
                    ui.horizontal(|ui| {
                        let edit = ui
                            .add(egui::TextEdit::singleline(url).desired_width(380.0).hint_text("https://…"));
                        if url.is_empty() {
                            edit.request_focus();
                        }
                        egui::ComboBox::from_id_salt("url-history").selected_text("").width(24.0).show_ui(
                            ui,
                            |ui| {
                                for entry in &self.settings.url_history {
                                    if ui.selectable_label(false, entry.as_str()).clicked() {
                                        *url = entry.clone();
                                    }
                                }
                            },
                        );
                    });
                    let address = url.trim().to_string();
                    let valid = crate::download::valid_url(&address);
                    if !address.is_empty() && !valid {
                        ui.colored_label(
                            egui::Color32::from_rgb(200, 40, 40),
                            if de {
                                "Bitte eine Adresse eingeben, die mit https:// oder http:// beginnt."
                            } else {
                                "Please enter an address starting with https:// or http://."
                            },
                        );
                    }
                    let (ok, cancel) = buttons(ui, t.ok, t.cancel);
                    if ok && valid {
                        action = Some(Box::new(move |app, ctx| app.start_download(ctx, address)));
                        outcome = Outcome::Close;
                    } else if cancel {
                        outcome = Outcome::Close;
                    }
                }
            }
        });

        if matches!(outcome, Outcome::Keep) {
            self.dialog = Some(dialog);
        } else if matches!(dialog, Dialog::Unsaved { .. }) {
            self.clear_previews();
        }
        if let Some(action) = action {
            action(self, ctx);
        }
    }
}
