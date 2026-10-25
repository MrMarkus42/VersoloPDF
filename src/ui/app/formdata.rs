//! Form fields, document-wide comment commands and redaction: creating
//! fields, reset, XFDF import/export, comment summary, "delete all comments",
//! search-and-redact and applying redactions.

use egui::{Context, Ui};

use super::commands::Cmd;
use super::dialogs::{Action, Outcome, buttons};
use super::{Dialog, MrPdfApp};
use crate::forms::NewFieldKind;
use crate::geometry::RectF;
use crate::pdf_edit::format_pdf_date;
use crate::report::Line;
use crate::ui::i18n::Language;
use crate::ui::tools::Tool;

/// Resolution of pages that are turned into images by redaction.
const REDACTION_DPI: f32 = 200.0;

/// Input of the dialog for new radio buttons, list and combo boxes.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldForm {
    pub page: usize,
    pub rect: RectF,
    pub tool: Tool,
    pub name: String,
    pub group: String,
    pub export: String,
    /// One option per line.
    pub options: String,
}

impl MrPdfApp {
    fn lang_de(&self) -> bool {
        self.settings.language == Language::German
    }

    /// Starts a form/comment command. Returns false if `cmd` is not one.
    pub(super) fn run_forms(&mut self, ctx: &Context, cmd: Cmd) -> bool {
        let de = self.lang_de();
        let lang = self.settings.language;
        match cmd {
            Cmd::ExportComments => self.export_xfdf(ctx, true),
            Cmd::ImportComments => self.import_xfdf(ctx, true),
            Cmd::ExportFormData => self.export_xfdf(ctx, false),
            Cmd::ImportFormData => self.import_xfdf(ctx, false),
            Cmd::SummarizeComments => self.summarize_comments(ctx),
            Cmd::DeleteAllComments => {
                let text = if de {
                    "Alle Kommentare des Dokuments löschen? Links und Formularfelder bleiben erhalten."
                } else {
                    "Delete all comments of the document? Links and form fields are kept."
                };
                self.dialog = Some(Dialog::Confirm { title: cmd.label(lang).into(), text: text.into(), cmd });
            }
            Cmd::ApplyRedactions => {
                let text = if de {
                    "Alle zum Schwärzen markierten Bereiche endgültig entfernen?\n\nBetroffene Seiten werden dabei in Bilder umgewandelt (ihr Text ist danach nicht mehr auswählbar). Die Datei wird ohne ältere Versionen neu geschrieben."
                } else {
                    "Permanently remove all areas marked for redaction?\n\nAffected pages are turned into images (their text can no longer be selected). The file is rewritten without older revisions."
                };
                self.dialog = Some(Dialog::Confirm { title: cmd.label(lang).into(), text: text.into(), cmd });
            }
            Cmd::SearchRedact => {
                let query = self.search_query.clone();
                self.dialog = Some(Dialog::SearchRedact { query, match_case: false, whole_word: false });
            }
            Cmd::ResetForm => {
                let text = if de {
                    "Alle Formularfelder auf ihre Standardwerte zurücksetzen?"
                } else {
                    "Reset all form fields to their default values?"
                };
                self.dialog = Some(Dialog::Confirm { title: cmd.label(lang).into(), text: text.into(), cmd });
            }
            _ => return false,
        }
        true
    }

    /// Runs a command after the user confirmed it.
    pub(super) fn run_confirmed(&mut self, ctx: &Context, cmd: Cmd) {
        let de = self.lang_de();
        match cmd {
            Cmd::DeleteAllComments => {
                let result = self.tab().map(|t| {
                    t.selected_annot = None;
                    t.doc.delete_all_comments()
                });
                self.after_count_status(
                    ctx,
                    result,
                    if de { "Gelöschte Kommentare" } else { "Deleted comments" },
                );
            }
            Cmd::ResetForm => self.edit(ctx, |tab| tab.doc.reset_form()),
            Cmd::BookmarkDeleteAll => self.edit(ctx, |tab| tab.doc.delete_all_bookmarks()),
            Cmd::ApplyRedactions => {
                let result = self.tab().map(|t| {
                    t.selected_annot = None;
                    t.doc.apply_redactions(REDACTION_DPI)
                });
                self.after_count_status(
                    ctx,
                    result,
                    if de { "Geschwärzte Seiten" } else { "Redacted pages" },
                );
            }
            _ => self.run(ctx, cmd),
        }
    }

    fn after_count_status(&mut self, ctx: &Context, result: Option<crate::Result<usize>>, label: &str) {
        if let Some(tab) = self.tab() {
            tab.after_edit();
        }
        match result {
            Some(Ok(n)) => self.set_status(ctx, format!("{label}: {n}")),
            Some(Err(err)) => self.show_error(&err),
            None => {}
        }
        ctx.request_repaint();
    }

    fn xfdf_name(&self) -> String {
        let stem = self
            .active_tab()
            .map(|t| t.doc.display_name().trim_end_matches(".pdf").trim_end_matches(".PDF").to_string())
            .unwrap_or_default();
        format!("{stem}.xfdf")
    }

    fn export_xfdf(&mut self, ctx: &Context, comments: bool) {
        if self.active_tab().is_none() {
            return;
        }
        let cmd = if comments { Cmd::ExportComments } else { Cmd::ExportFormData };
        let Some(path) = rfd::FileDialog::new()
            .set_title(cmd.label(self.settings.language))
            .add_filter("XFDF", &["xfdf"])
            .set_file_name(self.xfdf_name())
            .save_file()
        else {
            return;
        };
        let result = self.tab().map(|t| {
            if comments { t.doc.export_comments(&path) } else { t.doc.export_form_data(&path).map(|()| 0) }
        });
        match result {
            Some(Ok(_)) => {
                let msg = if self.lang_de() { "Exportiert: " } else { "Exported: " };
                self.set_status(ctx, format!("{msg}{}", path.display()));
            }
            Some(Err(err)) => self.show_error(&err),
            None => {}
        }
    }

    fn import_xfdf(&mut self, ctx: &Context, comments: bool) {
        if self.active_tab().is_none() {
            return;
        }
        let cmd = if comments { Cmd::ImportComments } else { Cmd::ImportFormData };
        let Some(path) = rfd::FileDialog::new()
            .set_title(cmd.label(self.settings.language))
            .add_filter("XFDF", &["xfdf", "xml"])
            .pick_file()
        else {
            return;
        };
        let author = self.settings.author.clone();
        let result = self.tab().map(|t| {
            if comments { t.doc.import_comments(&path, &author) } else { t.doc.import_form_data(&path) }
        });
        let de = self.lang_de();
        let label = match (comments, de) {
            (true, true) => "Importierte Kommentare",
            (true, false) => "Imported comments",
            (false, true) => "Ausgefüllte Felder",
            (false, false) => "Filled fields",
        };
        self.after_count_status(ctx, result, label);
    }

    /// Creates a PDF listing all comments and opens it in a new tab.
    fn summarize_comments(&mut self, ctx: &Context) {
        let de = self.lang_de();
        let t = self.t();
        let Some(tab) = self.tab() else { return };
        let name = tab.doc.display_name().to_string();
        let records = match tab.doc.comments() {
            Ok(r) => r,
            Err(err) => {
                self.show_error(&err);
                return;
            }
        };
        let mut lines = vec![
            Line::title(if de { "Kommentarübersicht" } else { "Comment summary" }),
            Line::small(format!("{name} · {} {}", records.len(), if de { "Kommentare" } else { "comments" })),
        ];
        let mut last_page = None;
        for r in &records {
            if last_page != Some(r.page) {
                last_page = Some(r.page);
                lines.push(Line::heading(format!("{} {}", if de { "Seite" } else { "Page" }, r.page + 1)));
            }
            let mut head = super::sidebar::kind_name(t, r.kind).to_string();
            if !r.author.is_empty() {
                head.push_str(" – ");
                head.push_str(&r.author);
            }
            if !r.date.is_empty() {
                head.push_str(" – ");
                head.push_str(&format_pdf_date(&r.date));
            }
            let mut line = Line { bold: true, space_before: 4.0, ..Line::new(head) };
            line.swatch = Some(r.color.unwrap_or(crate::model::Rgba::rgb(255, 210, 0)));
            lines.push(line);
            if !r.contents.trim().is_empty() {
                lines.push(Line::new(r.contents.trim()).indent(14.0));
            }
        }
        if records.is_empty() {
            lines.push(Line::new(if de {
                "Das Dokument enthält keine Kommentare."
            } else {
                "The document has no comments."
            }));
        }
        let title = if de { "Kommentarübersicht" } else { "Comment summary" };
        let result = crate::report::build_pdf(title, &lines)
            .and_then(|bytes| crate::Document::from_bytes(bytes, &format!("{title} – {name}"), None));
        match result {
            Ok(doc) => {
                self.add_tab(doc);
                ctx.request_repaint();
            }
            Err(err) => self.show_error(&err),
        }
    }

    /// A field tool drew `rect`: create the field (asking for options first
    /// where needed).
    pub(super) fn create_field(&mut self, ctx: &Context, page: usize, rect: RectF, tool: Tool) {
        if rect.width().abs() < 4.0 || rect.height().abs() < 4.0 {
            return;
        }
        let de = self.lang_de();
        let kind = match tool {
            Tool::FieldText => NewFieldKind::Text { multiline: rect.height().abs() > 40.0 },
            Tool::FieldCheckBox => NewFieldKind::CheckBox,
            Tool::FieldButton => NewFieldKind::ResetButton {
                caption: if de { "Zurücksetzen".into() } else { "Reset".into() },
            },
            Tool::FieldSignature => {
                NewFieldKind::Signature { label: if de { "Unterschrift".into() } else { "Signature".into() } }
            }
            Tool::FieldRadio | Tool::FieldList | Tool::FieldCombo => {
                let (group, n) = self.last_radio.clone();
                self.dialog = Some(Dialog::NewField(Box::new(FieldForm {
                    page,
                    rect,
                    tool,
                    name: String::new(),
                    group,
                    export: format!("Option{}", n + 1),
                    options: "Option 1\nOption 2\nOption 3".into(),
                })));
                return;
            }
            _ => return,
        };
        self.add_field(ctx, page, rect, kind, String::new());
    }

    fn add_field(&mut self, ctx: &Context, page: usize, rect: RectF, kind: NewFieldKind, name: String) {
        let Some(tab) = self.tab() else { return };
        let result = tab.doc.create_field(page, rect, &kind, &name);
        tab.after_edit();
        match result {
            Ok(name) => {
                let msg = if self.lang_de() { "Feld angelegt: " } else { "Field created: " };
                self.set_status(ctx, format!("{msg}{name}"));
            }
            Err(err) => self.show_error(&err),
        }
        ctx.request_repaint();
    }

    pub(super) fn forms_dialog(&mut self, ui: &mut Ui, dialog: &mut Dialog) -> (Outcome, Option<Action>) {
        let t = self.t();
        let de = self.lang_de();
        let lang = self.settings.language;
        let mut outcome = Outcome::Keep;
        let mut action: Option<Action> = None;
        match dialog {
            Dialog::Confirm { title, text, cmd } => {
                ui.heading(title.as_str());
                ui.label(text.as_str());
                let (ok, cancel) = buttons(ui, t.ok, t.cancel);
                if ok {
                    let cmd = *cmd;
                    action = Some(Box::new(move |app, ctx| app.run_confirmed(ctx, cmd)));
                    outcome = Outcome::Close;
                } else if cancel {
                    outcome = Outcome::Close;
                }
            }
            Dialog::NewField(form) => {
                ui.heading(Cmd::Tool(form.tool).label(lang));
                egui::Grid::new("new-field").num_columns(2).show(ui, |ui| {
                    if form.tool == Tool::FieldRadio {
                        ui.label(if de { "Gruppe:" } else { "Group:" });
                        ui.text_edit_singleline(&mut form.group);
                        ui.end_row();
                        ui.label(if de { "Exportwert:" } else { "Export value:" });
                        ui.text_edit_singleline(&mut form.export);
                        ui.end_row();
                    } else {
                        ui.label(if de {
                            "Feldname (leer = automatisch):"
                        } else {
                            "Field name (empty = automatic):"
                        });
                        ui.text_edit_singleline(&mut form.name);
                        ui.end_row();
                    }
                });
                if form.tool != Tool::FieldRadio {
                    ui.label(if de { "Einträge (einer pro Zeile):" } else { "Items (one per line):" });
                    ui.add(egui::TextEdit::multiline(&mut form.options).desired_rows(5));
                }
                ui.add_space(8.0);
                let mut ok = false;
                let mut cancel = false;
                ui.horizontal(|ui| {
                    ok = ui.button(egui::RichText::new(t.ok).strong()).clicked();
                    cancel = ui.button(t.cancel).clicked();
                });
                // Enter belongs to the multi-line option list here.
                cancel |= ui.input(|i| i.key_pressed(egui::Key::Escape));
                if ok {
                    let f = (**form).clone();
                    let options: Vec<String> = f
                        .options
                        .lines()
                        .map(str::trim)
                        .filter(|l| !l.is_empty())
                        .map(str::to_string)
                        .collect();
                    let kind = match f.tool {
                        Tool::FieldRadio => {
                            NewFieldKind::Radio { group: f.group.clone(), export: f.export.clone() }
                        }
                        Tool::FieldList => NewFieldKind::ListBox { options },
                        _ => NewFieldKind::ComboBox { options },
                    };
                    action = Some(Box::new(move |app, ctx| {
                        if f.tool == Tool::FieldRadio {
                            let n = if app.last_radio.0 == f.group { app.last_radio.1 + 1 } else { 1 };
                            app.last_radio = (f.group.clone(), n);
                        }
                        app.add_field(ctx, f.page, f.rect, kind, f.name.trim().to_string());
                    }));
                    outcome = Outcome::Close;
                } else if cancel {
                    outcome = Outcome::Close;
                }
            }
            Dialog::SearchRedact { query, match_case, whole_word } => {
                ui.heading(Cmd::SearchRedact.label(lang));
                ui.label(if de { "Suchbegriff:" } else { "Search for:" });
                ui.text_edit_singleline(query).request_focus();
                ui.checkbox(match_case, if de { "Groß-/Kleinschreibung beachten" } else { "Match case" });
                ui.checkbox(whole_word, if de { "Nur ganze Wörter" } else { "Whole words only" });
                ui.label(
                    egui::RichText::new(if de {
                        "Alle Fundstellen werden zum Schwärzen markiert. Erst „Übernehmen“ entfernt sie endgültig."
                    } else {
                        "All hits are marked for redaction. Only \"Apply\" removes them for good."
                    })
                    .weak()
                    .small(),
                );
                let (ok, cancel) = buttons(ui, t.ok, t.cancel);
                if ok && !query.trim().is_empty() {
                    let query = query.clone();
                    let opts =
                        crate::model::SearchOptions { match_case: *match_case, whole_word: *whole_word };
                    action = Some(Box::new(move |app, ctx| {
                        let style = app.settings.style(Tool::Redact);
                        let result = app.tab().map(|t| t.doc.mark_for_redaction(&query, opts, &style));
                        let label =
                            if app.lang_de() { "Zum Schwärzen markiert" } else { "Marked for redaction" };
                        app.after_count_status(ctx, result, label);
                    }));
                    outcome = Outcome::Close;
                } else if cancel {
                    outcome = Outcome::Close;
                }
            }
            _ => outcome = Outcome::Close,
        }
        (outcome, action)
    }
}
