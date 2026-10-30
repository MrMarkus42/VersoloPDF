//! Ribbon tab "Formular": layout, "Felder hervorheben",
//! the form pane, "Formulare erkennen", form data by e-mail, "Formulare
//! ausfüllen" (one PDF per CSV row), image fields and the JavaScript list.

use std::path::PathBuf;

use egui::{Context, RichText, Ui};

use super::commands::{Cmd, Unavailable};
use super::dialogs::{Action, Outcome};
use super::view_cmds::ViewCmd;
use super::{Dialog, MrPdfApp, SidePanel};
use crate::form_fill;
use crate::ui::i18n::Language;
use crate::ui::icons::Icon;
use crate::ui::settings::FieldGroup;
use crate::ui::tab::AnnotRef;
use crate::ui::tools::Tool;
use crate::ui::widgets::{group, small_menu_button};

const UA: fn(Unavailable, &'static str, &'static str, Icon) -> Cmd = Cmd::Unavailable;

/// The commands; used as `Cmd::Form(..)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FormCmd {
    Highlight(FieldGroup),
    RequiredOnly,
    FormPane,
    DetectForms,
    EmailFormData,
    FillForms,
    DocJavaScript,
    DocActions,
    AllJavaScripts,
}

impl FormCmd {
    pub fn names(self) -> (&'static str, &'static str) {
        use FormCmd::*;
        match self {
            Highlight(g) => g.names(),
            RequiredOnly => ("Nur erforderliche Felder", "Required fields only"),
            FormPane => ("Formular", "Form"),
            DetectForms => ("Formulare erkennen", "Detect form fields"),
            EmailFormData => ("Formulardaten per E-Mail versenden", "Send form data by e-mail"),
            FillForms => ("Formulare ausfüllen", "Fill forms"),
            DocJavaScript => ("JavaScript", "JavaScript"),
            DocActions => ("Dokumentaktionen", "Document actions"),
            AllJavaScripts => ("Alle JavaScripte", "All JavaScripts"),
        }
    }

    pub fn icon(self) -> Icon {
        use FormCmd::*;
        match self {
            Highlight(_) | RequiredOnly => Icon::Form,
            FormPane => Icon::Sidebar,
            DetectForms => Icon::Search,
            EmailFormData => Icon::Mail,
            FillForms => Icon::Form,
            DocJavaScript | DocActions | AllJavaScripts => Icon::Console,
        }
    }

    pub fn shortcut(self) -> Option<(&'static str, &'static str)> {
        match self {
            FormCmd::FormPane => Some(("Strg+I", "Ctrl+I")),
            _ => None,
        }
    }
}

/// State of "PDF-Formulare ausfüllen".
#[derive(Debug, Clone, PartialEq)]
pub struct FillState {
    /// None: the active document.
    pub form: Option<PathBuf>,
    pub data: Option<PathBuf>,
    pub separator: char,
    pub rows: String,
    /// The CSV file; first row = column names.
    pub table: Vec<Vec<String>>,
    pub fields: Vec<String>,
    pub mapping: Vec<(String, usize)>,
    pub folder: Option<PathBuf>,
    pub pattern: String,
    pub open_folder: bool,
    pub error: Option<String>,
    /// Field and column chosen for "Hinzufügen".
    pub add: (usize, usize),
}

/// "Alle JavaScripte": (where, code) and the selected entry.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptList {
    pub scripts: Vec<(String, String)>,
    pub selected: usize,
}

fn tr(de: bool, g: &'static str, e: &'static str) -> &'static str {
    if de { g } else { e }
}

impl MrPdfApp {
    fn fc_de(&self) -> bool {
        self.settings.language == Language::German
    }

    pub(super) fn form_enabled(&self, cmd: FormCmd) -> bool {
        match cmd {
            FormCmd::Highlight(_) | FormCmd::RequiredOnly | FormCmd::FillForms => true,
            _ => !self.tabs.is_empty(),
        }
    }

    pub(super) fn form_selected(&self, cmd: FormCmd) -> bool {
        let s = &self.settings;
        match cmd {
            FormCmd::Highlight(g) => !s.highlight_off.contains(&g),
            FormCmd::RequiredOnly => s.highlight_required_only,
            FormCmd::FormPane => s.show_sidebar && self.side == SidePanel::Fields,
            _ => false,
        }
    }

    pub(super) fn run_form(&mut self, ctx: &Context, cmd: FormCmd) {
        let s = &mut self.settings;
        match cmd {
            FormCmd::Highlight(g) => {
                if s.highlight_off.contains(&g) {
                    s.highlight_off.retain(|x| *x != g);
                } else {
                    s.highlight_off.push(g);
                }
                s.highlight_fields = true;
            }
            FormCmd::RequiredOnly => {
                s.highlight_required_only = !s.highlight_required_only;
                s.highlight_fields = true;
            }
            FormCmd::FormPane => {
                if s.show_sidebar && self.side == SidePanel::Fields {
                    s.show_sidebar = false;
                } else {
                    s.show_sidebar = true;
                    self.side = SidePanel::Fields;
                }
            }
            FormCmd::DetectForms => self.detect_forms(ctx),
            FormCmd::EmailFormData => self.email_form_data(ctx),
            FormCmd::FillForms => self.open_fill_forms(),
            FormCmd::DocJavaScript | FormCmd::DocActions => {
                self.open_properties();
                if let Some(Dialog::Properties(state)) = &mut self.dialog {
                    state.page = if cmd == FormCmd::DocActions { 6 } else { 7 };
                }
            }
            FormCmd::AllJavaScripts => {
                let result = self.tab().map(|t| t.doc.all_javascripts());
                match result {
                    Some(Ok(scripts)) => {
                        self.dialog = Some(Dialog::Scripts(Box::new(ScriptList { scripts, selected: 0 })))
                    }
                    Some(Err(err)) => self.show_error(&err),
                    None => {}
                }
            }
        }
    }

    /// The ribbon tab "Formular".
    pub(super) fn ribbon_form(&mut self, ui: &mut Ui, ctx: &Context) {
        let de = self.fc_de();
        self.tools_group(ui, ctx);
        group(ui, tr(de, "Formularfelder", "Form fields"), |ui| {
            self.big(ui, ctx, Cmd::Tool(Tool::SelectFields));
            for tool in [
                Tool::FieldText,
                Tool::FieldButton,
                Tool::FieldCheckBox,
                Tool::FieldRadio,
                Tool::FieldList,
                Tool::FieldCombo,
                Tool::FieldSignature,
            ] {
                self.big(ui, ctx, Cmd::Tool(tool));
            }
            self.big(ui, ctx, UA(Unavailable::Barcodes, "Barcode", "Barcode", Icon::Barcode));
            self.big(ui, ctx, Cmd::Tool(Tool::FieldDate));
            self.big(ui, ctx, Cmd::Tool(Tool::FieldImage));
            ui.vertical(|ui| {
                self.highlight_menu(ui, ctx);
                self.small(ui, ctx, Cmd::View(ViewCmd::Grid));
                self.small(ui, ctx, Cmd::Form(FormCmd::FormPane));
            });
        });
        group(ui, tr(de, "Erkennen", "Detect"), |ui| {
            self.big(ui, ctx, Cmd::Form(FormCmd::DetectForms));
        });
        group(ui, tr(de, "Formulardaten", "Form data"), |ui| {
            self.big(ui, ctx, Cmd::ResetForm);
            self.smalls(
                ui,
                ctx,
                &[Cmd::ImportFormData, Cmd::ExportFormData, Cmd::Form(FormCmd::EmailFormData)],
            );
            self.big(ui, ctx, Cmd::Form(FormCmd::FillForms));
        });
        group(ui, "JavaScript", |ui| {
            self.big(
                ui,
                ctx,
                UA(Unavailable::JavaScript, "JavaScript-Konsole", "JavaScript console", Icon::Console),
            );
            self.big(
                ui,
                ctx,
                UA(Unavailable::JavaScript, "JavaScript-Einstellungen", "JavaScript settings", Icon::Gear),
            );
            self.smalls(
                ui,
                ctx,
                &[
                    Cmd::Form(FormCmd::DocJavaScript),
                    Cmd::Form(FormCmd::DocActions),
                    Cmd::Form(FormCmd::AllJavaScripts),
                ],
            );
        });
    }

    /// "Felder hervorheben ▾": the button switches highlighting, the menu
    /// chooses the kinds.
    fn highlight_menu(&mut self, ui: &mut Ui, ctx: &Context) {
        let de = self.fc_de();
        let on = self.settings.highlight_fields;
        let resp =
            small_menu_button(ui, Icon::Form, tr(de, "Felder hervorheben", "Highlight fields"), true, on);
        let mut chosen = None;
        egui::Popup::menu(&resp).show(|ui| {
            ui.set_min_width(240.0);
            if self.menu_row(ui, Cmd::HighlightFields) {
                chosen = Some(Cmd::HighlightFields);
            }
            ui.separator();
            for g in FieldGroup::ALL {
                let cmd = Cmd::Form(FormCmd::Highlight(g));
                if self.menu_row(ui, cmd) {
                    chosen = Some(cmd);
                }
            }
            self.menu_row(ui, UA(Unavailable::Barcodes, "Barcodes", "Barcodes", Icon::Barcode));
            ui.separator();
            let cmd = Cmd::Form(FormCmd::RequiredOnly);
            if self.menu_row(ui, cmd) {
                chosen = Some(cmd);
            }
        });
        if let Some(cmd) = chosen {
            self.run(ctx, cmd);
        }
    }

    /// The "Formular" pane: every field with type and value; a click shows it.
    pub(super) fn fields_panel(&mut self, ui: &mut Ui) {
        let de = self.fc_de();
        let lang = self.settings.language;
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        let mut jump = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let mut any = false;
            for page in 0..tab.doc.page_count().min(2000) {
                let fields = tab.fields(page).to_vec();
                if fields.is_empty() {
                    continue;
                }
                any = true;
                ui.label(RichText::new(format!("{} {}", tr(de, "Seite", "Page"), page + 1)).weak());
                for f in fields {
                    let tool = match f.kind {
                        crate::model::FieldKind::Text { .. } => Tool::FieldText,
                        crate::model::FieldKind::CheckBox => Tool::FieldCheckBox,
                        crate::model::FieldKind::RadioButton => Tool::FieldRadio,
                        crate::model::FieldKind::ComboBox => Tool::FieldCombo,
                        crate::model::FieldKind::ListBox => Tool::FieldList,
                        crate::model::FieldKind::PushButton => Tool::FieldButton,
                        crate::model::FieldKind::Signature => Tool::FieldSignature,
                        crate::model::FieldKind::Unknown => Tool::SelectFields,
                    };
                    let selected = tab.selected_annot == Some(AnnotRef { page, index: f.annot_index });
                    let mut text = f.name.clone();
                    if !f.value.is_empty() {
                        text.push_str(&format!(" = {}", f.value));
                    }
                    if f.required {
                        text.push_str(" *");
                    }
                    let resp = ui
                        .horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                            crate::ui::icons::paint(ui.painter(), r, tool.icon(), ui.visuals().text_color());
                            ui.selectable_label(selected, text)
                        })
                        .inner
                        .on_hover_text(Cmd::Tool(tool).label(lang));
                    if resp.clicked() {
                        jump = Some(AnnotRef { page, index: f.annot_index });
                    }
                }
            }
            if !any {
                ui.label(
                    RichText::new(tr(
                        de,
                        "Das Dokument hat keine Formularfelder.",
                        "The document has no form fields.",
                    ))
                    .weak(),
                );
            }
        });
        if let Some(r) = jump {
            tab.selected_annot = Some(r);
            tab.jump_to_page(r.page);
        }
    }

    /// "Formulare erkennen" on all pages.
    fn detect_forms(&mut self, ctx: &Context) {
        let de = self.fc_de();
        let Some(tab) = self.tab() else { return };
        let mut found = Vec::new();
        for page in 0..tab.doc.page_count().min(2000) {
            if let Ok(list) = tab.doc.detect_fields(page) {
                found.extend(list.into_iter().map(|(r, k)| (page, r, k)));
            }
        }
        if found.is_empty() {
            self.set_status(ctx, tr(de, "Keine Formularfelder erkannt", "No form fields detected"));
            return;
        }
        let result = self.tab().map(|t| {
            let r = t.doc.create_detected_fields(&found);
            t.after_edit();
            r
        });
        match result {
            Some(Ok(n)) => {
                self.tool = Tool::SelectFields;
                let msg = if de { format!("Erkannte Felder: {n}") } else { format!("Detected fields: {n}") };
                self.set_status(ctx, msg);
            }
            Some(Err(err)) => self.show_error(&err),
            None => {}
        }
        ctx.request_repaint();
    }

    /// "Formulardaten per E-Mail versenden": XFDF in the temporary folder,
    /// then the mail program.
    fn email_form_data(&mut self, ctx: &Context) {
        let de = self.fc_de();
        let Some(tab) = self.tab() else { return };
        let stem = tab.doc.display_name().trim_end_matches(".pdf").trim_end_matches(".PDF").to_string();
        let dir = std::env::temp_dir().join("mrpdf-formdata");
        let path = dir.join(format!("{}.xfdf", stem.replace(['/', '\\', ':'], "_")));
        let result = std::fs::create_dir_all(&dir)
            .map_err(|e| crate::Error::Io { path: dir.clone(), source: e })
            .and_then(|()| tab.doc.export_form_data(&path));
        if let Err(err) = result {
            self.show_error(&err);
            return;
        }
        let subject = super::features::mail_subject(&self.settings.email_subject, &format!("{stem}.xfdf"));
        ctx.open_url(egui::OpenUrl::new_tab(format!("mailto:?subject={subject}")));
        let text = if de {
            format!(
                "Das E-Mail-Programm wurde geöffnet. Bitte hängen Sie die Formulardaten an:\n{}",
                path.display()
            )
        } else {
            format!("Your e-mail program was opened. Please attach the form data:\n{}", path.display())
        };
        self.info(Cmd::Form(FormCmd::EmailFormData).label(self.settings.language), text);
    }

    fn open_fill_forms(&mut self) {
        let fields = self.tab().and_then(|t| t.doc.field_values().ok()).unwrap_or_default();
        self.dialog = Some(Dialog::FillForms(Box::new(FillState {
            form: None,
            data: None,
            separator: ';',
            rows: String::new(),
            table: Vec::new(),
            fields: fields.into_iter().map(|(n, _)| n).collect(),
            mapping: Vec::new(),
            folder: self.active_tab().and_then(|t| t.doc.path()).and_then(|p| p.parent()).map(PathBuf::from),
            pattern: "%[FileName]_%[RowIndex]".into(),
            open_folder: true,
            error: None,
            add: (0, 0),
        })));
    }

    /// Reads the CSV file of the dialog (and maps columns automatically).
    pub fn fill_load_data(s: &mut FillState, path: PathBuf, guess: bool) {
        let text = std::fs::metadata(&path)
            .ok()
            .filter(|m| m.len() <= form_fill::MAX_DATA)
            .and_then(|_| std::fs::read(&path).ok())
            .map(|b| String::from_utf8(b.clone()).unwrap_or_else(|_| b.iter().map(|&c| c as char).collect()));
        match text {
            Some(text) => {
                if guess {
                    s.separator = form_fill::guess_separator(&text);
                }
                s.table = form_fill::parse_csv(&text, s.separator);
                s.data = Some(path);
                s.error = None;
                if let Some(header) = s.table.first() {
                    s.mapping = form_fill::auto_map(&s.fields, header);
                }
            }
            None => s.error = Some("CSV".into()),
        }
    }

    pub(super) fn fill_forms_dialog(&mut self, ui: &mut Ui, s: &mut FillState) -> (Outcome, Option<Action>) {
        let de = self.fc_de();
        super::dialogs::set_title(ui, tr(de, "PDF-Formulare ausfüllen", "Fill PDF forms"));
        let section = |ui: &mut Ui, title: &str| {
            ui.add_space(4.0);
            ui.label(RichText::new(title).strong());
        };
        section(ui, tr(de, "Quelle für PDF-Formular", "PDF form source"));
        ui.horizontal(|ui| {
            ui.label(tr(de, "Dokument:", "Document:"));
            let current = self.active_tab().map(|t| t.doc.display_name().to_string()).unwrap_or_default();
            let shown = s.form.as_ref().map_or(current, |p| p.display().to_string());
            ui.add(egui::Label::new(shown).truncate());
            if ui.button("…").on_hover_text(tr(de, "Formular wählen", "Choose form")).clicked()
                && let Some(p) = rfd::FileDialog::new().add_filter("PDF", &["pdf"]).pick_file()
            {
                if let Ok(mut doc) = crate::Document::open(&p, None) {
                    s.fields = doc.field_values().unwrap_or_default().into_iter().map(|(n, _)| n).collect();
                    if let Some(header) = s.table.first() {
                        s.mapping = form_fill::auto_map(&s.fields, header);
                    }
                }
                s.form = Some(p);
            }
        });
        section(ui, tr(de, "Quelle für Daten", "Data source"));
        egui::Grid::new("fill-data").num_columns(2).show(ui, |ui| {
            ui.label(tr(de, "Datei:", "File:"));
            ui.horizontal(|ui| {
                let shown = s.data.as_ref().map_or_else(
                    || tr(de, "Bitte eine CSV-Datei wählen", "Please choose a CSV file").to_string(),
                    |p| p.display().to_string(),
                );
                ui.add(egui::Label::new(shown).truncate());
                if ui.button("…").clicked()
                    && let Some(p) = rfd::FileDialog::new().add_filter("CSV", &["csv", "txt"]).pick_file()
                {
                    Self::fill_load_data(s, p, true);
                }
            });
            ui.end_row();
            ui.label(tr(de, "Trenner:", "Separator:"));
            let before = s.separator;
            egui::ComboBox::from_id_salt("fill-sep")
                .selected_text(match s.separator {
                    '\t' => tr(de, "Tabulator", "Tab").to_string(),
                    c => c.to_string(),
                })
                .show_ui(ui, |ui| {
                    for (c, l) in [(';', ";"), (',', ","), ('\t', tr(de, "Tabulator", "Tab"))] {
                        ui.selectable_value(&mut s.separator, c, l);
                    }
                });
            if s.separator != before
                && let Some(p) = s.data.clone()
            {
                Self::fill_load_data(s, p, false);
            }
            ui.end_row();
            ui.label(tr(de, "Zeile(n):", "Row(s):"));
            ui.add(
                egui::TextEdit::singleline(&mut s.rows).hint_text(tr(de, "Alle", "All")).desired_width(120.0),
            );
            ui.end_row();
        });
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_min_size(egui::vec2(460.0, 90.0));
            if s.table.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(30.0);
                    ui.label(
                        RichText::new(tr(
                            de,
                            "Keine gültige Datenquelle für eine Vorschau",
                            "No valid data source for a preview",
                        ))
                        .weak(),
                    );
                });
            } else {
                egui::ScrollArea::both().max_height(120.0).show(ui, |ui| {
                    egui::Grid::new("fill-preview").striped(true).show(ui, |ui| {
                        for (i, row) in s.table.iter().take(8).enumerate() {
                            for cell in row.iter().take(12) {
                                let t = RichText::new(cell.as_str());
                                ui.label(if i == 0 { t.strong() } else { t });
                            }
                            ui.end_row();
                        }
                    });
                });
            }
        });
        section(
            ui,
            tr(
                de,
                "Zuordnungen zwischen PDF-Formular und Datenquelle",
                "Mapping between PDF form and data source",
            ),
        );
        let header: Vec<String> = s.table.first().cloned().unwrap_or_default();
        ui.horizontal(|ui| {
            let field_name = s.fields.get(s.add.0).cloned().unwrap_or_default();
            egui::ComboBox::from_id_salt("fill-add-field").selected_text(field_name).width(150.0).show_ui(
                ui,
                |ui| {
                    for (i, f) in s.fields.iter().enumerate() {
                        ui.selectable_value(&mut s.add.0, i, f);
                    }
                },
            );
            let col_name = header.get(s.add.1).cloned().unwrap_or_default();
            egui::ComboBox::from_id_salt("fill-add-col").selected_text(col_name).width(130.0).show_ui(
                ui,
                |ui| {
                    for (i, h) in header.iter().enumerate() {
                        ui.selectable_value(&mut s.add.1, i, h);
                    }
                },
            );
            let can_add = s.add.0 < s.fields.len() && s.add.1 < header.len();
            if ui.add_enabled(can_add, egui::Button::new(tr(de, "Hinzufügen…", "Add…"))).clicked() {
                let field = s.fields[s.add.0].clone();
                s.mapping.retain(|(f, _)| *f != field);
                s.mapping.push((field, s.add.1));
            }
            if ui
                .add_enabled(
                    !header.is_empty(),
                    egui::Button::new(tr(de, "Automatisch ermitteln", "Detect automatically")),
                )
                .clicked()
            {
                s.mapping = form_fill::auto_map(&s.fields, &header);
            }
        });
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_min_size(egui::vec2(460.0, 70.0));
            let mut remove = None;
            egui::Grid::new("fill-map").striped(true).num_columns(3).show(ui, |ui| {
                ui.label(RichText::new(tr(de, "PDF-Formularfeld", "PDF form field")).weak());
                ui.label(RichText::new(tr(de, "Spalte für Daten", "Data column")).weak());
                ui.label("");
                ui.end_row();
                for (i, (field, col)) in s.mapping.iter().enumerate() {
                    ui.label(field.as_str());
                    ui.label(header.get(*col).map_or("?", String::as_str));
                    if ui.small_button(tr(de, "Entfernen", "Remove")).clicked() {
                        remove = Some(i);
                    }
                    ui.end_row();
                }
            });
            if let Some(i) = remove {
                s.mapping.remove(i);
            }
        });
        section(ui, tr(de, "Einstellungen für Ausgabe", "Output settings"));
        egui::Grid::new("fill-out").num_columns(2).show(ui, |ui| {
            ui.label(tr(de, "Ordner:", "Folder:"));
            ui.horizontal(|ui| {
                let shown = s.folder.as_ref().map_or_else(
                    || tr(de, "Bitte einen Ordner wählen", "Please choose a folder").to_string(),
                    |p| p.display().to_string(),
                );
                ui.add(egui::Label::new(shown).truncate());
                if ui.button("…").clicked()
                    && let Some(p) = rfd::FileDialog::new().pick_folder()
                {
                    s.folder = Some(p);
                }
            });
            ui.end_row();
            ui.label(tr(de, "Dateiname:", "File name:"));
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut s.pattern).desired_width(240.0));
                ui.menu_button("%", |ui| {
                    for p in ["%[FileName]", "%[RowIndex]"] {
                        if ui.button(p).clicked() {
                            s.pattern.push_str(p);
                            ui.close();
                        }
                    }
                });
            });
            ui.end_row();
        });
        ui.checkbox(
            &mut s.open_folder,
            tr(de, "Zielordner danach öffnen", "Open the target folder afterwards"),
        );
        if let Some(err) = &s.error {
            ui.colored_label(egui::Color32::from_rgb(200, 40, 40), err.as_str());
        }
        let data_rows = s.table.len().saturating_sub(1);
        let rows = form_fill::chosen_rows(&s.rows, data_rows);
        let ready =
            s.folder.is_some() && !s.mapping.is_empty() && rows.as_ref().is_some_and(|r| !r.is_empty());
        let (ok, cancel, _) = super::page_dialogs::footer(ui, de, ready, false, "OK");
        if cancel {
            return (Outcome::Close, None);
        }
        if !ok {
            return (Outcome::Keep, None);
        }
        let state = s.clone();
        let rows = rows.unwrap_or_default();
        (Outcome::Close, Some(Box::new(move |app, ctx| app.run_fill_forms(ctx, &state, &rows))))
    }

    /// Writes one filled PDF per data row (`rows`: 0 = first row below the
    /// column names).
    pub fn run_fill_forms(&mut self, ctx: &Context, s: &FillState, rows: &[usize]) {
        let de = self.fc_de();
        let (form, stem) = match &s.form {
            Some(p) => (
                crate::document::read_pdf_file(p),
                p.file_stem().map_or_else(|| "Formular".into(), |n| n.to_string_lossy().into_owned()),
            ),
            None => match self.tab() {
                Some(t) => {
                    let stem =
                        t.doc.display_name().trim_end_matches(".pdf").trim_end_matches(".PDF").to_string();
                    (t.doc.to_bytes(), stem)
                }
                None => return,
            },
        };
        let Some(folder) = s.folder.clone() else { return };
        // The first row of the file holds the column names.
        let table: Vec<Vec<String>> = s.table.iter().skip(1).cloned().collect();
        let result = form.and_then(|bytes| {
            form_fill::fill_all(&bytes, &stem, &table, &s.mapping, rows, &folder, &s.pattern)
        });
        match result {
            Ok(files) => {
                let msg = if de {
                    format!("Ausgefüllte Formulare: {}", files.len())
                } else {
                    format!("Filled forms: {}", files.len())
                };
                self.set_status(ctx, msg);
                if s.open_folder
                    && let Some(first) = files.first()
                {
                    super::features::reveal_in_file_manager(first);
                }
            }
            Err(err) => self.show_error(&err),
        }
    }

    pub(super) fn scripts_dialog(&mut self, ui: &mut Ui, s: &mut ScriptList) -> Outcome {
        let de = self.fc_de();
        super::dialogs::set_title(ui, tr(de, "Alle JavaScripte", "All JavaScripts"));
        ui.label(
            RichText::new(tr(
                de,
                "MrPDF führt kein JavaScript aus. Hier sehen Sie, welche Skripte das Dokument enthält.",
                "MrPDF does not run JavaScript. This shows which scripts the document contains.",
            ))
            .weak(),
        );
        if s.scripts.is_empty() {
            ui.label(tr(de, "Das Dokument enthält kein JavaScript.", "The document contains no JavaScript."));
        } else {
            ui.horizontal_top(|ui| {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    egui::ScrollArea::vertical().id_salt("js-list").max_height(320.0).show(ui, |ui| {
                        ui.set_width(220.0);
                        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                            for (i, (place, _)) in s.scripts.iter().enumerate() {
                                if ui.selectable_label(s.selected == i, place.as_str()).clicked() {
                                    s.selected = i;
                                }
                            }
                        });
                    });
                });
                if let Some((_, code)) = s.scripts.get(s.selected) {
                    let mut code = code.clone();
                    egui::ScrollArea::vertical().id_salt("js-code").max_height(320.0).show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut code)
                                .code_editor()
                                .interactive(true)
                                .desired_width(380.0)
                                .desired_rows(16),
                        );
                    });
                }
            });
        }
        let (ok, cancel, _) = super::page_dialogs::footer(ui, de, true, false, tr(de, "Schließen", "Close"));
        if ok || cancel { Outcome::Close } else { Outcome::Keep }
    }

    /// A push button was clicked while filling in: "Bild" fields load an image.
    pub(super) fn press_button(&mut self, ctx: &Context, page: usize, annot_index: usize) {
        let t = self.t();
        let extras =
            self.tab().and_then(|tab| tab.doc.field_extras(page, annot_index).ok()).unwrap_or_default();
        if !extras.image {
            self.set_status(ctx, t.field_not_supported);
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Bilder / Images", &["png", "jpg", "jpeg", "bmp", "gif", "tif", "tiff", "webp"])
            .pick_file()
        else {
            return;
        };
        match image::open(&path) {
            Ok(img) => {
                let rgba = img.to_rgba8();
                let image = crate::pdf_edit::RgbaImage {
                    width: rgba.width(),
                    height: rgba.height(),
                    rgba: rgba.into_raw(),
                };
                self.set_field_image(ctx, page, annot_index, &image);
            }
            Err(e) => self.show_error(&crate::Error::Invalid(e.to_string())),
        }
    }

    /// Shows `image` in a "Bild" field (also used by tests).
    pub fn set_field_image(
        &mut self,
        ctx: &Context,
        page: usize,
        annot_index: usize,
        image: &crate::pdf_edit::RgbaImage,
    ) {
        self.edit(ctx, |tab| tab.doc.set_button_image(page, annot_index, image));
    }
}
