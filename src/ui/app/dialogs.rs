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

/// Height of the title bar of a dialog window.
const TITLE_BAR: f32 = 30.0;

fn title_id() -> Id {
    Id::new("mrpdf-dialog-title")
}

/// Sets the text of the dialog's title bar (drawn by `MrPdfApp::dialogs`).
pub(super) fn set_title(ui: &mut Ui, text: impl Into<String>) {
    let text: String = text.into();
    ui.ctx().data_mut(|d| d.insert_temp(title_id(), text));
}

/// Draws the title bar of a dialog window into `bar`; returns (drag
/// movement, close button clicked).
fn title_bar(ui: &mut Ui, bar: egui::Rect, title: &str) -> (Option<egui::Vec2>, bool) {
    let visuals = ui.visuals().clone();
    let drag = ui.interact(bar, Id::new("mrpdf-dialog-bar"), egui::Sense::drag());
    let fill = if visuals.dark_mode { egui::Color32::from_gray(48) } else { egui::Color32::from_gray(236) };
    let r = visuals.window_corner_radius;
    ui.painter().rect_filled(bar, egui::CornerRadius { nw: r.nw, ne: r.ne, sw: 0, se: 0 }, fill);
    ui.painter().line_segment(
        [bar.left_bottom(), bar.right_bottom()],
        egui::Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color),
    );
    let close_rect = egui::Rect::from_center_size(
        egui::pos2(bar.right() - TITLE_BAR / 2.0 - 3.0, bar.center().y),
        egui::vec2(TITLE_BAR + 6.0, TITLE_BAR - 6.0),
    );
    // Title text, wrapped before the close button.
    let galley = ui.painter().layout(
        title.to_string(),
        egui::FontId::proportional(14.0),
        visuals.strong_text_color(),
        (close_rect.left() - bar.left() - 16.0).max(20.0),
    );
    ui.painter().galley(
        egui::pos2(bar.left() + 12.0, bar.center().y - galley.size().y / 2.0),
        galley,
        visuals.strong_text_color(),
    );
    // Accessible name of the bar: the title (found by tests and screen readers).
    drag.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, title));
    let close = ui.interact(close_rect, Id::new("mrpdf-dialog-close"), egui::Sense::click());
    close.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "✕"));
    if close.hovered() {
        ui.painter().rect_filled(close_rect, 3.0, egui::Color32::from_rgb(196, 43, 28));
    }
    let x_color = if close.hovered() { egui::Color32::WHITE } else { visuals.text_color() };
    let c = close_rect.center();
    let stroke = egui::Stroke::new(1.4, x_color);
    ui.painter().line_segment([c + egui::vec2(-5.0, -5.0), c + egui::vec2(5.0, 5.0)], stroke);
    ui.painter().line_segment([c + egui::vec2(-5.0, 5.0), c + egui::vec2(5.0, -5.0)], stroke);
    if drag.hovered() || drag.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
    }
    let delta = drag.dragged().then(|| drag.drag_delta()).filter(|d| *d != egui::Vec2::ZERO);
    (delta, close.clicked())
}

pub(super) fn buttons(ui: &mut Ui, ok: &str, cancel: &str) -> (bool, bool) {
    buttons_if(ui, ok, cancel, true)
}

/// Like [`buttons`], with OK (and Enter) only active while `enabled`.
pub(super) fn buttons_if(ui: &mut Ui, ok: &str, cancel: &str, enabled: bool) -> (bool, bool) {
    let mut result = (false, false);
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        result.0 = ui.add_enabled(enabled, egui::Button::new(RichText::new(ok).strong())).clicked();
        result.1 = ui.button(cancel).clicked();
    });
    let enter = enabled && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);
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

        // The dialog is a window: title bar (drag to move, ✕ to close) and
        // content. A different dialog opens in the middle again.
        let area_id = Id::new("mrpdf-dialog");
        let kind = std::mem::discriminant(&dialog);
        let pos = self.dialog_pos.filter(|(k, _)| *k == kind).map(|(_, p)| p);
        let area = match pos {
            Some(pos) => egui::Area::new(area_id)
                .kind(egui::UiKind::Modal)
                .sense(egui::Sense::hover())
                .order(egui::Order::Foreground)
                .interactable(true)
                .fixed_pos(pos),
            None => Modal::default_area(area_id),
        };
        let frame = egui::Frame::window(&ctx.style_of(ctx.theme())).inner_margin(0);
        let mut moved: Option<egui::Vec2> = None;
        let mut closed = false;
        ctx.data_mut(|d| d.remove::<String>(title_id()));
        let modal =
            Modal::new(area_id).area(area).frame(frame).backdrop_color(egui::Color32::from_black_alpha(40));
        // No dialog may be taller than the window: higher contents scroll.
        let max_height = (ctx.content_rect().height() - TITLE_BAR - 60.0).max(200.0);
        modal.show(ctx, |ui| {
            let bar_top = ui.cursor().min;
            ui.add_space(TITLE_BAR);
            let content =
                egui::Frame::new().inner_margin(egui::Margin { left: 14, right: 14, top: 10, bottom: 12 });
            content.show(ui, |ui| {
                // A visible scroll bar, so the user sees there is more below.
                ui.style_mut().spacing.scroll.floating = false;
                let scroll = egui::ScrollArea::vertical()
                    .id_salt("dialog-scroll")
                    .max_height(max_height)
                    // Use the window's height before scrolling (the dialog area
                    // would otherwise offer only half of it).
                    .min_scrolled_height(max_height);
                scroll.show(ui, |ui| {
                    ui.set_max_width(460.0);
                    match &mut dialog {
                        Dialog::Message { title, text } => {
                            super::dialogs::set_title(ui, title.as_str());
                            ui.label(text.as_str());
                            ui.add_space(8.0);
                            if ui.button(t.ok).clicked()
                                || ui.input(|i| {
                                    i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Escape)
                                })
                            {
                                outcome = Outcome::Close;
                            }
                        }
                        Dialog::Password { path, password, wrong } => {
                            super::dialogs::set_title(ui, t.password_title);
                            ui.label(
                                path.file_name()
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_default(),
                            );
                            if *wrong {
                                ui.colored_label(egui::Color32::from_rgb(200, 40, 40), t.err_wrong_password);
                            }
                            let edit = ui.add(
                                egui::TextEdit::singleline(password).password(true).hint_text(t.password),
                            );
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
                                        super::preview::PreviewOpts::new(true).fixed(),
                                    );
                                }
                                ui.vertical(|ui| {
                                    ui.set_width(330.0);
                                    ui.label(
                                        RichText::new(format!(
                                            "{} „{name}“ {}",
                                            t.unsaved_doc, t.unsaved_changed
                                        ))
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
                                        action = Some(Box::new(move |app, ctx| {
                                            app.after_unsaved(ctx, id, then, true)
                                        }));
                                        outcome = Outcome::Close;
                                    }
                                    if answer(ui, t.no) {
                                        action = Some(Box::new(move |app, ctx| {
                                            app.after_unsaved(ctx, id, then, false)
                                        }));
                                        outcome = Outcome::Close;
                                    }
                                    if answer(ui, t.cancel) || ui.input(|i| i.key_pressed(egui::Key::Escape))
                                    {
                                        outcome = Outcome::Close;
                                    }
                                });
                            });
                        }
                        Dialog::TextInput { page, tool, template, text, edit } => {
                            super::dialogs::set_title(
                                ui,
                                super::commands::Cmd::Tool(*tool).label(self.settings.language),
                            );
                            if *tool == crate::ui::tools::Tool::Replace
                                && let Some(original) =
                                    template.as_ref().map(|t| t.label.clone()).filter(|s| !s.is_empty())
                            {
                                ui.label(
                                    RichText::new(format!("{}: „{original}“", t.replace_original)).weak(),
                                );
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
                                        action = Some(Box::new(move |app, ctx| {
                                            app.finish_text_edit(ctx, annot, text)
                                        }));
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
                            super::dialogs::set_title(ui, t.create_link);
                            ui.radio_value(to_page, false, t.link_to_url);
                            ui.add_enabled(!*to_page, egui::TextEdit::singleline(url).desired_width(360.0));
                            ui.radio_value(to_page, true, t.link_to_page);
                            let count = self.active_tab().map_or(1, |tab| tab.doc.page_count()) as u32;
                            ui.add_enabled(
                                *to_page,
                                egui::DragValue::new(target_page).range(1..=count.max(1)),
                            );
                            let valid = *to_page || super::actions::is_safe_uri(url);
                            if !valid {
                                ui.colored_label(egui::Color32::from_rgb(200, 40, 40), t.link_blocked);
                            }
                            let (ok, cancel) = buttons_if(ui, t.ok, t.cancel, valid);
                            if ok {
                                let spec = if *to_page {
                                    crate::pdf_edit::LinkSpec::Page(target_page.saturating_sub(1) as usize)
                                } else {
                                    crate::pdf_edit::LinkSpec::Uri(url.trim().to_string())
                                };
                                let (page, rect) = (*page, *rect);
                                action =
                                    Some(Box::new(move |app, ctx| app.create_link(ctx, page, rect, spec)));
                                outcome = Outcome::Close;
                            } else if cancel {
                                outcome = Outcome::Close;
                            }
                        }
                        Dialog::ConfirmLink { uri, trust } => {
                            super::dialogs::set_title(ui, t.open_link_title);
                            ui.label(t.open_link_question);
                            ui.add(egui::Label::new(RichText::new(uri.as_str()).monospace()).wrap());
                            let host = crate::ui::settings::url_host(uri);
                            if let Some(host) = &host {
                                let de = self.settings.language == crate::ui::i18n::Language::German;
                                let text = if de {
                                    format!("Diese Webseite immer erlauben ({host})")
                                } else {
                                    format!("Always allow this web site ({host})")
                                };
                                ui.checkbox(trust, text);
                            }
                            let (ok, cancel) = buttons(ui, t.open, t.cancel);
                            if ok {
                                if *trust
                                    && let Some(host) = host
                                    && !self.settings.trusted_hosts.contains(&host)
                                {
                                    self.settings.trusted_hosts.push(host);
                                }
                                ctx.open_url(egui::OpenUrl::new_tab(uri.clone()));
                                outcome = Outcome::Close;
                            } else if cancel {
                                outcome = Outcome::Close;
                            }
                        }
                        Dialog::Properties(state) => {
                            let (o, a) = self.properties_dialog(ui, state);
                            outcome = o;
                            action = a;
                        }
                        Dialog::PageRange { purpose, text, error } => {
                            let title = match purpose {
                                RangePurpose::Extract => t.extract_pages,
                                RangePurpose::Flatten => t.flatten,
                                RangePurpose::ToImages => {
                                    super::commands::Cmd::ContentsToImages.label(self.settings.language)
                                }
                            };
                            super::dialogs::set_title(ui, title);
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
                                        action = Some(Box::new(move |app, ctx| {
                                            app.apply_range(ctx, purpose, pages)
                                        }));
                                        outcome = Outcome::Close;
                                    }
                                    None => *error = true,
                                }
                            } else if cancel {
                                outcome = Outcome::Close;
                            }
                        }
                        Dialog::ExportImages(state) => {
                            let (o, a) = self.export_images_dialog(ui, state);
                            outcome = o;
                            action = a;
                        }
                        Dialog::ExportWord(state) => {
                            let (o, a) = self.word_dialog(ui, state);
                            outcome = o;
                            action = a;
                        }
                        Dialog::RecognizeText(state) => {
                            let (o, a) = self.recognize_text_dialog(ui, state);
                            outcome = o;
                            action = a;
                        }
                        Dialog::ReplaceFonts(state) => {
                            let (o, a) = self.replace_fonts_dialog(ui, state);
                            outcome = o;
                            action = a;
                        }
                        Dialog::Recolor(state) => {
                            let (o, a) = self.recolor_dialog(ui, state);
                            outcome = o;
                            action = a;
                        }
                        Dialog::ExportExcel(state) => {
                            let (o, a) = self.excel_dialog(ui, state);
                            outcome = o;
                            action = a;
                        }
                        Dialog::ExportPowerPoint(state) => {
                            let (o, a) = self.powerpoint_dialog(ui, state);
                            outcome = o;
                            action = a;
                        }
                        Dialog::Transitions(state) => {
                            let (o, a) = self.transitions_dialog(ui, state);
                            outcome = o;
                            action = a;
                        }
                        Dialog::MoveTabs(state) => {
                            let (o, a) = self.move_tabs_dialog(ui, state);
                            outcome = o;
                            action = a;
                        }
                        Dialog::CommentStyles(tool) => {
                            outcome = self.comment_styles_dialog(ui, tool);
                        }
                        Dialog::FillForms(state) => {
                            let (o, a) = self.fill_forms_dialog(ui, state);
                            outcome = o;
                            action = a;
                        }
                        Dialog::Scripts(list) => {
                            outcome = self.scripts_dialog(ui, list);
                        }
                        Dialog::SpellCheck(state) => {
                            outcome = self.spell_dialog(ui, state);
                        }
                        Dialog::NewPortfolio(state) => {
                            let (o, a) = self.new_portfolio_dialog(ui, state);
                            outcome = o;
                            action = a;
                        }
                        Dialog::Settings(state) => {
                            let (o, a) = self.settings_dialog(ui, state);
                            outcome = o;
                            action = a;
                        }
                        Dialog::Print(state) => {
                            let (o, a) = self.print_dialog(ui, state);
                            outcome = o;
                            action = a;
                        }
                        Dialog::Choice { page, annot_index, options, selected } => {
                            super::dialogs::set_title(ui, t.choose_value);
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
                            super::dialogs::set_title(ui, t.password_protect);
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
                            super::dialogs::set_title(
                                ui,
                                if path.is_some() { t.rename } else { t.add_bookmark },
                            );
                            let edit = ui.add(egui::TextEdit::singleline(title).desired_width(320.0));
                            edit.request_focus();
                            if path.is_none() {
                                ui.label(
                                    RichText::new(format!("{} {}", t.bookmark_target, *page + 1)).weak(),
                                );
                            }
                            let (ok, cancel) = buttons_if(ui, t.ok, t.cancel, !title.trim().is_empty());
                            if ok {
                                let title = title.trim().to_string();
                                let (parent, path, page) = (parent.clone(), path.clone(), *page);
                                action = Some(Box::new(move |app, ctx| match (parent, path) {
                                    (_, Some(path)) => {
                                        app.edit(ctx, |tab| tab.doc.rename_bookmark(&path, &title))
                                    }
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
                        other @ (Dialog::Confirm { .. }
                        | Dialog::NewField(_)
                        | Dialog::SearchRedact { .. }) => {
                            let (o, a) = self.forms_dialog(ui, other);
                            outcome = o;
                            action = a;
                        }
                        Dialog::Bookmark(state) => {
                            let (o, a) = self.bookmark_dialog(ui, state);
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
                        Dialog::Compare(state) => {
                            let (o, a) = self.compare_dialog(ui, state);
                            outcome = o;
                            action = a;
                        }
                        Dialog::About => {
                            set_title(
                                ui,
                                if self.settings.language == crate::ui::i18n::Language::German {
                                    "Über MrPDF"
                                } else {
                                    "About MrPDF"
                                },
                            );
                            ui.horizontal(|ui| {
                                let (rect, _) =
                                    ui.allocate_exact_size(egui::vec2(48.0, 48.0), egui::Sense::hover());
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
                            super::dialogs::set_title(
                                ui,
                                if de { "Web-Seite öffnen" } else { "Open web page" },
                            );
                            ui.label(if de { "URL eingeben:" } else { "Enter URL:" });
                            ui.horizontal(|ui| {
                                let edit = ui.add(
                                    egui::TextEdit::singleline(url)
                                        .desired_width(380.0)
                                        .hint_text("https://…"),
                                );
                                if url.is_empty() {
                                    edit.request_focus();
                                }
                                egui::ComboBox::from_id_salt("url-history")
                                    .selected_text("")
                                    .width(24.0)
                                    .show_ui(ui, |ui| {
                                        for entry in &self.settings.url_history {
                                            if ui.selectable_label(false, entry.as_str()).clicked() {
                                                *url = entry.clone();
                                            }
                                        }
                                    });
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
                            let (ok, cancel) = buttons_if(ui, t.ok, t.cancel, valid);
                            if ok {
                                action = Some(Box::new(move |app, ctx| app.start_download(ctx, address)));
                                outcome = Outcome::Close;
                            } else if cancel {
                                outcome = Outcome::Close;
                            }
                        }
                    }
                });
            });
            let bar = egui::Rect::from_min_size(bar_top, egui::vec2(ui.min_rect().width(), TITLE_BAR));
            let title = ui.ctx().data(|d| d.get_temp::<String>(title_id())).unwrap_or_else(|| "MrPDF".into());
            (moved, closed) = title_bar(ui, bar, &title);
        });

        if let Some(delta) = moved
            && let Some(rect) = ctx.memory(|m| m.area_rect(area_id))
        {
            // The title bar always stays reachable on the screen.
            let screen = ctx.content_rect();
            let min = rect.min + delta;
            let x = min.x.clamp(screen.left() - rect.width() + 80.0, screen.right() - 80.0);
            let y = min.y.clamp(screen.top(), (screen.bottom() - TITLE_BAR).max(screen.top()));
            self.dialog_pos = Some((kind, egui::pos2(x, y)));
        }
        if closed {
            // ✕ works like "Abbrechen".
            outcome = Outcome::Close;
            action = None;
            if matches!(dialog, Dialog::Settings(_)) {
                self.settings_draft = None;
            }
        }

        if matches!(outcome, Outcome::Keep) {
            self.dialog = Some(dialog);
        } else {
            self.dialog_pos = None;
            if matches!(dialog, Dialog::Unsaved { .. }) {
                self.clear_previews();
            }
        }
        if let Some(action) = action {
            action(self, ctx);
        }
    }
}
