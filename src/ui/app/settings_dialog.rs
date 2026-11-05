//! "Einstellungen" (Strg+K):
//! the categories with options, a search box, OK / Übernehmen / Abbrechen; and
//! "Einstellungen verwalten" (export, import, reset).

use egui::{RichText, Ui, vec2};

use super::commands::Cmd;
use super::dialogs::{Action, Outcome};
use super::page_dialogs::tr;
use super::{Dialog, VersoloPdfApp};
use crate::ui::i18n::Language;
use crate::ui::pickers::{Unit, section};
use crate::ui::settings::{LinkPolicy, SaveFolder, Settings, url_host};
use crate::ui::tools::Tool;
use crate::ui::view::{PageLayout, ZoomMode};

/// State of the dialog (the edited options are in `VersoloPdfApp::settings_draft`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SettingsPage {
    pub page: Cat,
    pub search: String,
    /// Input of "Webseite hinzufügen" on the page "Sicherheit".
    pub new_host: String,
}

/// The categories in a fixed order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Cat {
    #[default]
    General,
    Documents,
    License,
    PageDisplay,
    PageText,
    Measure3d,
    Comments,
    Launch,
    Accessibility,
    Snapshots,
    FileAssoc,
    InfoBar,
    Printing,
    Email,
    Plugins,
    ColorManagement,
    Forms,
    Identity,
    ConvertTo,
    JavaScript,
    Ai,
    Performance,
    Measure,
    Media3d,
    Update,
    Interface,
    ConvertFrom,
    Scanner,
    SharePoint,
    Security,
    Signatures,
    Places,
    Language,
    SearchEngines,
    Ocr,
    FullScreen,
    ReadAloud,
    Tools,
}

impl Cat {
    pub const ALL: [Cat; 38] = [
        Cat::General,
        Cat::Documents,
        Cat::License,
        Cat::PageDisplay,
        Cat::PageText,
        Cat::Measure3d,
        Cat::Comments,
        Cat::Launch,
        Cat::Accessibility,
        Cat::Snapshots,
        Cat::FileAssoc,
        Cat::InfoBar,
        Cat::Printing,
        Cat::Email,
        Cat::Plugins,
        Cat::ColorManagement,
        Cat::Forms,
        Cat::Identity,
        Cat::ConvertTo,
        Cat::JavaScript,
        Cat::Ai,
        Cat::Performance,
        Cat::Measure,
        Cat::Media3d,
        Cat::Update,
        Cat::Interface,
        Cat::ConvertFrom,
        Cat::Scanner,
        Cat::SharePoint,
        Cat::Security,
        Cat::Signatures,
        Cat::Places,
        Cat::Language,
        Cat::SearchEngines,
        Cat::Ocr,
        Cat::FullScreen,
        Cat::ReadAloud,
        Cat::Tools,
    ];

    /// (German, English, search words).
    fn names(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Cat::General => ("Allgemein", "General", "tab tabs leiste dateimenü start backstage dialoge"),
            Cat::Documents => (
                "Dokumente",
                "Documents",
                "sitzung wiederherstellen zuletzt speichern unter ordner wiederherstellung minuten recent session recovery folder",
            ),
            Cat::License => ("Lizenz", "License", "lizenz schlüssel license key"),
            Cat::PageDisplay => (
                "Seitenanzeige",
                "Page display",
                "seite layout zoom vergrößerung ansicht letzte seitenleiste page view sidebar",
            ),
            Cat::PageText => ("Seitentext", "Page text", "text kopieren links copy whitespace ligatur"),
            Cat::Measure3d => ("3D-Messung", "3D measurement", "3d messung"),
            Cat::Comments => {
                ("Anmerkungen", "Comments", "kommentar anmerkung auswählen autor annotation select author")
            }
            Cat::Launch => ("Anwendungen starten", "Launch applications", "anwendung programm application"),
            Cat::Accessibility => (
                "Barrierefreiheit",
                "Accessibility",
                "farben ersetzen kontrast hintergrund text colours contrast",
            ),
            Cat::Snapshots => (
                "Bildschirmfotos",
                "Snapshots",
                "bildschirmfoto auflösung graustufen snapshot resolution gray",
            ),
            Cat::FileAssoc => ("Dateizuordnung", "File association", "standard anwendung default"),
            Cat::InfoBar => ("Dokument-Infoleiste", "Document info bar", "infoleiste info bar"),
            Cat::Printing => ("Drucken", "Printing", "drucker treiber printer driver"),
            Cat::Email => ("E-Mails versenden", "Sending e-mail", "mail betreff subject"),
            Cat::Plugins => ("Erweiterungen", "Plug-ins", "erweiterung plugin"),
            Cat::ColorManagement => ("Farbverwaltung", "Colour management", "farbprofil icc profile cms"),
            Cat::Forms => ("Formulare", "Forms", "formular felder hervorheben form fields highlight"),
            Cat::Identity => (
                "Identität",
                "Identity",
                "name titel organisation abteilung e-mail autor title organization department author",
            ),
            Cat::ConvertTo => {
                ("In PDF umwandeln", "Convert to PDF", "umwandeln bild text rtf import convert")
            }
            Cat::JavaScript => ("JavaScript", "JavaScript", "javascript skript konsole script console"),
            Cat::Ai => ("KI-Assistent", "AI assistant", "ki ai assistent"),
            Cat::Performance => ("Leistung", "Performance", "speicher ram cpu memory"),
            Cat::Measure => ("Messen", "Measurement", "einheit maß gitter unit grid ruler"),
            Cat::Media3d => ("Multimedia 3D", "Multimedia 3D", "3d multimedia"),
            Cat::Update => ("Neue Version", "Updates", "update version aktualisierung"),
            Cat::Interface => (
                "Oberfläche anpassen",
                "Customize UI",
                "dunkel dark modus system größe skalierung aussehen theme scale",
            ),
            Cat::ConvertFrom => ("PDF umwandeln in", "Convert PDF to", "export word text bild export"),
            Cat::Scanner => ("Scanner", "Scanner", "scanner scannen scan"),
            Cat::SharePoint => ("SharePoint", "SharePoint", "sharepoint"),
            Cat::Security => (
                "Sicherheit",
                "Security",
                "webseite link vertrauensliste erlauben blockieren trust web page allow block",
            ),
            Cat::Signatures => ("Signaturen", "Signatures", "signatur signature zertifikat certificate"),
            Cat::Places => ("Speicherorte", "Places", "speicherort cloud cookies"),
            Cat::Language => ("Sprache", "Language", "sprache deutsch englisch language german english"),
            Cat::SearchEngines => ("Suchmaschinen", "Search engines", "suchmaschine search engine"),
            Cat::Ocr => ("Texterkennung", "OCR", "texterkennung ocr"),
            Cat::FullScreen => ("Vollbild", "Full screen", "vollbild esc full screen"),
            Cat::ReadAloud => ("Vorlesen", "Read aloud", "vorlesen sprache stimme read aloud voice tts"),
            Cat::Tools => ("Werkzeuge", "Tools", "werkzeug hand standard letztes tool default last"),
        }
    }

    pub fn name(self, de: bool) -> &'static str {
        let (g, e, _) = self.names();
        if de { g } else { e }
    }

    /// Why a category has no options in VersoloPDF (None: it has some).
    fn unavailable(self, de: bool) -> Option<&'static str> {
        Some(match self {
            Cat::License => tr(
                de,
                "VersoloPDF ist freie Software und braucht keine Lizenz.",
                "VersoloPDF is free software and needs no licence.",
            ),
            Cat::Measure3d | Cat::Media3d => {
                tr(de, "VersoloPDF zeigt keine 3D-Inhalte an.", "VersoloPDF does not show 3D content.")
            }
            Cat::Launch => tr(
                de,
                "VersoloPDF startet keine fremden Programme aus Dokumenten.",
                "VersoloPDF does not start other programs from documents.",
            ),
            Cat::FileAssoc => tr(
                de,
                "Das Standardprogramm für PDF-Dateien wird in den Einstellungen des Betriebssystems festgelegt.",
                "The default program for PDF files is set in the settings of the operating system.",
            ),
            Cat::InfoBar => tr(
                de,
                "VersoloPDF zeigt nur die Wiederherstellungsleiste (siehe „Dokumente“).",
                "VersoloPDF only shows the recovery bar (see \"Documents\").",
            ),
            Cat::Plugins => tr(de, "VersoloPDF hat keine Erweiterungen.", "VersoloPDF has no plug-ins."),
            Cat::ColorManagement => tr(
                de,
                "Farben werden ohne Farbprofile angezeigt.",
                "Colours are shown without colour profiles.",
            ),
            Cat::JavaScript => tr(
                de,
                "VersoloPDF führt kein JavaScript aus Dokumenten aus.",
                "VersoloPDF does not run JavaScript from documents.",
            ),
            Cat::Ai => tr(de, "VersoloPDF hat keinen KI-Dienst.", "VersoloPDF has no AI service."),
            Cat::Performance => tr(
                de,
                "Speicher und Rechenkerne werden automatisch verwaltet.",
                "Memory and processor cores are managed automatically.",
            ),
            Cat::Scanner => tr(de, "VersoloPDF kann nicht scannen.", "VersoloPDF cannot scan."),
            Cat::SharePoint | Cat::Places => tr(
                de,
                "VersoloPDF arbeitet nur mit Dateien auf diesem Computer.",
                "VersoloPDF only works with files on this computer.",
            ),
            Cat::SearchEngines => {
                tr(de, "VersoloPDF sucht nicht im Internet.", "VersoloPDF does not search the internet.")
            }
            Cat::Ocr => tr(de, "VersoloPDF hat keine Texterkennung.", "VersoloPDF has no OCR."),
            _ => return None,
        })
    }

    fn matches(self, query: &str) -> bool {
        let (g, e, words) = self.names();
        query.is_empty()
            || g.to_lowercase().contains(query)
            || e.to_lowercase().contains(query)
            || words.contains(query)
    }
}

/// Tools offered as "Standardwerkzeug".
const START_TOOLS: [Tool; 5] = [Tool::Hand, Tool::SelectText, Tool::SelectAnnot, Tool::Zoom, Tool::Snapshot];

/// Colour schemes of "Farben im Dokument ersetzen": (name, background, text).
const SCHEMES: [(&str, &str, [u8; 3], [u8; 3]); 4] = [
    ("Weiß auf Schwarz", "White on black", [0, 0, 0], [255, 255, 255]),
    ("Gelb auf Schwarz", "Yellow on black", [0, 0, 0], [255, 255, 0]),
    ("Grün auf Schwarz", "Green on black", [0, 0, 0], [0, 255, 0]),
    ("Schwarz auf Beige", "Black on beige", [245, 236, 214], [0, 0, 0]),
];

fn combo<T: PartialEq + Copy>(ui: &mut Ui, id: &str, value: &mut T, options: &[(T, &str)], width: f32) {
    let current = options.iter().find(|o| o.0 == *value).map_or("", |o| o.1);
    egui::ComboBox::from_id_salt(id).selected_text(current).width(width).show_ui(ui, |ui| {
        for (v, name) in options {
            ui.selectable_value(value, *v, *name);
        }
    });
}

fn hint(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).small().weak());
}

/// Removes control characters and limits the length of a text field.
fn clean(text: &str, max: usize) -> String {
    text.chars().filter(|c| !c.is_control()).take(max).collect()
}

/// Checks and normalises edited options before they are used.
fn sanitize(s: &mut Settings) {
    s.author = clean(&s.author, 128);
    s.identity.title = clean(&s.identity.title, 128);
    s.identity.organisation = clean(&s.identity.organisation, 128);
    s.identity.department = clean(&s.identity.department, 128);
    s.identity.email = clean(&s.identity.email, 128);
    s.email_subject = clean(&s.email_subject, 200);
    s.sign_reason = clean(&s.sign_reason, 200);
    s.sign_location = clean(&s.sign_location, 200);
    s.sign_contact = clean(&s.sign_contact, 200);
    s.sign_corner = s.sign_corner.min(3);
    s.tsa_url = clean(s.tsa_url.trim(), 300);
    if !crate::download::valid_url(&s.tsa_url) {
        s.tsa_url = crate::sign::tsa::DEFAULT_URL.into();
    }
    s.ui_scale = s.ui_scale.clamp(0.5, 2.0);
    s.recovery_minutes = s.recovery_minutes.clamp(1, 600);
    if s.snapshot_dpi != 0 {
        s.snapshot_dpi = s.snapshot_dpi.clamp(36, 1200);
    }
    s.trusted_hosts.truncate(200);
    s.limit_view_values();
}

impl VersoloPdfApp {
    /// Opens "Einstellungen".
    pub fn open_settings(&mut self) {
        self.open_settings_at(Cat::General);
    }

    /// Opens "Einstellungen" at a category.
    pub fn open_settings_at(&mut self, page: Cat) {
        self.settings_draft = Some(Box::new(self.settings.clone()));
        self.dialog = Some(Dialog::Settings(Box::new(SettingsPage { page, ..SettingsPage::default() })));
    }

    /// The options being edited in "Einstellungen" (for tests).
    pub fn settings_draft_mut(&mut self) -> Option<&mut Settings> {
        self.settings_draft.as_deref_mut()
    }

    /// Takes the edited options over; the history of the running program
    /// (recent files, sessions …) is kept.
    fn apply_draft(&mut self) {
        let Some(draft) = self.settings_draft.as_deref() else { return };
        let mut next = draft.clone();
        sanitize(&mut next);
        let old = std::mem::replace(&mut self.settings, next);
        self.settings.keep_history(old);
        // Changes of the recent list made in the dialog ("Liste leeren").
        self.settings.recent_files.clone_from(&draft.recent_files);
        self.settings.pinned_files.clone_from(&draft.pinned_files);
    }

    /// Draws "Einstellungen".
    pub(super) fn settings_dialog(&mut self, ui: &mut Ui, s: &mut SettingsPage) -> (Outcome, Option<Action>) {
        let de = self.settings.language == Language::German;
        let mut draft = self.settings_draft.take().unwrap_or_else(|| Box::new(self.settings.clone()));
        ui.set_max_width(780.0);
        super::dialogs::set_title(ui, tr(de, "Einstellungen", "Settings"));
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(200.0);
                ui.add(
                    egui::TextEdit::singleline(&mut s.search)
                        .hint_text(tr(de, "Suchen…", "Search…"))
                        .desired_width(196.0),
                );
                let query = s.search.trim().to_lowercase();
                egui::ScrollArea::vertical().id_salt("settings-cats").max_height(400.0).show(ui, |ui| {
                    // Full column width, so the scroll bar sits at the edge.
                    ui.set_min_width(ui.available_width());
                    // Categories without options in VersoloPDF are not listed.
                    for cat in Cat::ALL {
                        if !cat.matches(&query) || cat.unavailable(de).is_some() {
                            continue;
                        }
                        if ui.selectable_label(s.page == cat, RichText::new(cat.name(de))).clicked() {
                            s.page = cat;
                        }
                    }
                });
            });
            ui.add_space(8.0);
            ui.vertical(|ui| {
                ui.set_width(540.0);
                ui.set_min_height(420.0);
                ui.strong(s.page.name(de));
                ui.add_space(4.0);
                egui::ScrollArea::vertical().id_salt("settings-page").max_height(400.0).show(ui, |ui| {
                    if let Some(reason) = s.page.unavailable(de) {
                        ui.label(format!("❌ {reason}"));
                    } else if s.page == Cat::Update {
                        // Needs the program state (running check, found release).
                        self.update_page(ui, de, &mut draft);
                    } else {
                        settings_page(ui, de, s, &mut draft);
                    }
                });
            });
        });

        // Footer: OK / Übernehmen / Abbrechen.
        let (mut ok, mut apply, mut cancel) = (false, false, false);
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                cancel = ui.button(tr(de, "Abbrechen", "Cancel")).clicked();
                apply = ui.button(tr(de, "Übernehmen", "Apply")).clicked();
                ok = ui.button(RichText::new("OK").strong()).clicked();
            });
        });
        cancel |= ui.input(|i| i.key_pressed(egui::Key::Escape));
        self.settings_draft = Some(draft);
        if ok || apply {
            self.apply_draft();
        }
        if ok || cancel {
            self.settings_draft = None;
            return (Outcome::Close, None);
        }
        (Outcome::Keep, None)
    }

    /// "Einstellungen exportieren": writes the options to a RON file.
    pub(super) fn export_settings(&mut self, ctx: &egui::Context) {
        let de = self.settings.language == Language::German;
        let Some(path) = rfd::FileDialog::new()
            .set_title(Cmd::ExportSettings.label(self.settings.language))
            .add_filter(tr(de, "VersoloPDF-Einstellungen", "VersoloPDF settings"), &["ron"])
            .set_file_name("VersoloPDF-Einstellungen.ron")
            .save_file()
        else {
            return;
        };
        match self.settings.export(&path) {
            Ok(()) => {
                let msg = if de { "Einstellungen exportiert: " } else { "Settings exported: " };
                self.set_status(ctx, format!("{msg}{}", path.display()));
            }
            Err(err) => self.show_error(&err),
        }
    }

    /// "Einstellungen importieren": reads options written by the export.
    pub(super) fn import_settings(&mut self, ctx: &egui::Context) {
        let de = self.settings.language == Language::German;
        let Some(path) = rfd::FileDialog::new()
            .set_title(Cmd::ImportSettings.label(self.settings.language))
            .add_filter(tr(de, "VersoloPDF-Einstellungen", "VersoloPDF settings"), &["ron"])
            .pick_file()
        else {
            return;
        };
        match Settings::import(&path) {
            Ok(mut next) => {
                sanitize(&mut next);
                let old = std::mem::replace(&mut self.settings, next);
                self.settings.keep_history(old);
                let msg = if de { "Einstellungen importiert" } else { "Settings imported" };
                self.set_status(ctx, msg);
            }
            Err(err) => self.show_error(&err),
        }
    }

    /// "Einstellungen zurücksetzen" (after the confirmation).
    pub(super) fn reset_settings(&mut self, ctx: &egui::Context) {
        self.settings.reset_options();
        let de = self.settings.language == Language::German;
        self.set_status(ctx, if de { "Einstellungen zurückgesetzt" } else { "Settings reset" });
    }
}

/// The options of one category.
fn settings_page(ui: &mut Ui, de: bool, s: &mut SettingsPage, d: &mut Settings) {
    match s.page {
        Cat::General => {
            section(ui, tr(de, "Tabs und Fenster", "Tabs and windows"), |ui| {
                ui.checkbox(
                    &mut d.hide_single_tab,
                    tr(
                        de,
                        "Tab-Leiste verbergen, wenn nur ein Dokument geöffnet ist",
                        "Hide the tab bar when only one document is open",
                    ),
                );
            });
            section(ui, tr(de, "Start", "Start"), |ui| {
                ui.checkbox(
                    &mut d.backstage_at_start,
                    tr(de, "Das Dateimenü beim Start anzeigen", "Show the file menu at start"),
                );
                hint(
                    ui,
                    tr(
                        de,
                        "Nur wenn beim Start keine Datei geöffnet wird.",
                        "Only when no file is opened at start.",
                    ),
                );
            });
            section(ui, tr(de, "Dialoge", "Dialogs"), |ui| {
                ui.add_enabled(
                    false,
                    egui::Checkbox::new(
                        &mut true,
                        tr(
                            de,
                            "Systemdialoge zum Öffnen und Speichern verwenden",
                            "Use the system dialogs for opening and saving",
                        ),
                    ),
                );
            });
        }
        Cat::Documents => {
            section(ui, tr(de, "Öffnen", "Opening"), |ui| {
                ui.checkbox(
                    &mut d.restore_session,
                    tr(
                        de,
                        "Letzte Sitzung beim Start wiederherstellen (wenn keine Dateien geöffnet werden)",
                        "Restore the last session at start (when no files are opened)",
                    ),
                );
                ui.horizontal(|ui| {
                    let n = d.recent_files.len();
                    ui.label(if de {
                        format!("Zuletzt verwendete Dokumente: {n}")
                    } else {
                        format!("Recent documents: {n}")
                    });
                    if ui
                        .add_enabled(n > 0, egui::Button::new(tr(de, "Liste leeren", "Clear list")))
                        .clicked()
                    {
                        d.recent_files.clear();
                        d.pinned_files.clear();
                    }
                });
            });
            section(ui, tr(de, "Speichern", "Saving"), |ui| {
                ui.horizontal(|ui| {
                    ui.label(tr(de, "Zielordner für „Speichern unter“:", "Folder for \"Save as\":"));
                    combo(
                        ui,
                        "set-save-folder",
                        &mut d.save_as_folder,
                        &[
                            (SaveFolder::Last, tr(de, "Letzter Ordner", "Last folder")),
                            (SaveFolder::Document, tr(de, "Ordner des Dokuments", "Folder of the document")),
                        ],
                        180.0,
                    );
                });
                ui.horizontal(|ui| {
                    ui.checkbox(
                        &mut d.recovery,
                        tr(
                            de,
                            "Informationen zum Wiederherstellen speichern alle",
                            "Save recovery information every",
                        ),
                    );
                    ui.add_enabled(d.recovery, egui::DragValue::new(&mut d.recovery_minutes).range(1..=600));
                    ui.label(tr(de, "Minute(n)", "minute(s)"));
                });
                ui.checkbox(
                    &mut d.recovery_bar,
                    tr(
                        de,
                        "Wiederherstellungsleiste anzeigen, wenn Daten verfügbar sind",
                        "Show the recovery bar when data is available",
                    ),
                );
            });
        }
        Cat::PageDisplay => {
            section(ui, tr(de, "Beim Öffnen", "When opening"), |ui| {
                ui.checkbox(
                    &mut d.restore_last_page,
                    tr(
                        de,
                        "Zuletzt angezeigte Seite des Dokuments wiederherstellen",
                        "Restore the page of the document shown last",
                    ),
                );
                egui::Grid::new("set-view").num_columns(2).spacing(vec2(10.0, 4.0)).show(ui, |ui| {
                    ui.label(tr(de, "Standardseitenlayout:", "Default page layout:"));
                    combo(
                        ui,
                        "set-layout",
                        &mut d.layout,
                        &[
                            (PageLayout::Single, tr(de, "Einzelne Seite", "Single page")),
                            (PageLayout::Continuous, tr(de, "Fortlaufend", "Continuous")),
                            (PageLayout::TwoPages, tr(de, "Zwei Seiten", "Two pages")),
                            (
                                PageLayout::TwoPagesContinuous,
                                tr(de, "Zwei Seiten fortlaufend", "Two pages continuous"),
                            ),
                        ],
                        200.0,
                    );
                    ui.end_row();
                    ui.label(tr(de, "Standardvergrößerung:", "Default zoom:"));
                    combo(
                        ui,
                        "set-zoom",
                        &mut d.zoom_mode,
                        &[
                            (ZoomMode::FitPage, tr(de, "Ganze Seite", "Fit page")),
                            (ZoomMode::FitWidth, tr(de, "Seitenbreite", "Fit width")),
                            (ZoomMode::Custom, tr(de, "Tatsächliche Größe", "Actual size")),
                        ],
                        200.0,
                    );
                    ui.end_row();
                });
            });
            section(ui, tr(de, "Fenster", "Window"), |ui| {
                ui.checkbox(&mut d.show_sidebar, tr(de, "Seitenleiste anzeigen", "Show the sidebar"));
                ui.checkbox(&mut d.show_properties, tr(de, "Eigenschaften anzeigen", "Show the properties"));
            });
        }
        Cat::PageText => {
            hint(
                ui,
                tr(
                    de,
                    "Beim Kopieren steht ein Leerzeichen zwischen Wörtern und ein Zeilenumbruch nach jeder Textzeile. \
                     Web-Adressen im Text werden nicht als Verweise erkannt.",
                    "When copying, words are separated by one space and every text line ends with a line break. \
                     Web addresses in the text are not turned into links.",
                ),
            );
        }
        Cat::Comments => {
            section(ui, tr(de, "Erstellen", "Creating"), |ui| {
                ui.checkbox(
                    &mut d.auto_select_new,
                    tr(
                        de,
                        "Neu erstellte Anmerkung automatisch auswählen",
                        "Select a new comment automatically",
                    ),
                );
                hint(
                    ui,
                    tr(
                        de,
                        "Als Autor wird der Name aus „Identität“ eingetragen.",
                        "The name from \"Identity\" is used as author.",
                    ),
                );
            });
        }
        Cat::Accessibility => {
            section(ui, tr(de, "Dokumentfarben", "Document colours"), |ui| {
                ui.checkbox(
                    &mut d.recolor,
                    tr(de, "Farben im Dokument ersetzen", "Replace document colours"),
                );
                ui.add_enabled_ui(d.recolor, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(tr(de, "Farbschema:", "Colour scheme:"));
                        for (g, e, bg, fg) in SCHEMES {
                            if ui.button(tr(de, g, e)).clicked() {
                                d.recolor_background = bg;
                                d.recolor_text = fg;
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(tr(de, "Hintergrund:", "Background:"));
                        ui.color_edit_button_srgb(&mut d.recolor_background);
                        ui.add_space(12.0);
                        ui.label(tr(de, "Text:", "Text:"));
                        ui.color_edit_button_srgb(&mut d.recolor_text);
                    });
                });
                hint(
                    ui,
                    tr(
                        de,
                        "Nur die Anzeige ändert sich; Dokument, Druck und Export bleiben unverändert.",
                        "Only the display changes; the document, printing and export stay unchanged.",
                    ),
                );
            });
        }
        Cat::Snapshots => {
            section(ui, tr(de, "Bild", "Image"), |ui| {
                ui.horizontal(|ui| {
                    let mut own = d.snapshot_dpi != 0;
                    if ui
                        .checkbox(&mut own, tr(de, "Eigene Auflösung verwenden:", "Use own resolution:"))
                        .changed()
                    {
                        d.snapshot_dpi = if own { 150 } else { 0 };
                    }
                    if own {
                        ui.add(egui::DragValue::new(&mut d.snapshot_dpi).range(36..=1200).suffix(" dpi"));
                    }
                });
                hint(
                    ui,
                    tr(
                        de,
                        "Ohne eigene Auflösung: doppelte Bildschirmauflösung (192 dpi).",
                        "Without own resolution: twice the screen resolution (192 dpi).",
                    ),
                );
                ui.checkbox(&mut d.snapshot_gray, tr(de, "In Graustufen kopieren", "Copy in greyscale"));
            });
        }
        Cat::Printing => {
            hint(
                ui,
                tr(
                    de,
                    "Drucker und Druckertreiber kommen vom Betriebssystem. Papier, Seiten und Größe werden im Dialog \
                     „Drucken“ gewählt.",
                    "Printers and printer drivers come from the operating system. Paper, pages and size are chosen \
                     in the \"Print\" dialog.",
                ),
            );
        }
        Cat::Email => {
            section(ui, tr(de, "Versand", "Sending"), |ui| {
                ui.label(tr(de, "Methode: Standard-E-Mail-Programm", "Method: default e-mail program"));
                ui.horizontal(|ui| {
                    ui.label(tr(de, "Standardbetreff:", "Default subject:"));
                    ui.add(egui::TextEdit::singleline(&mut d.email_subject).desired_width(260.0));
                    if ui
                        .button("%[FileName]")
                        .on_hover_text(tr(de, "Dateiname einfügen", "Insert file name"))
                        .clicked()
                    {
                        d.email_subject.push_str("%[FileName]");
                    }
                });
                hint(
                    ui,
                    tr(
                        de,
                        "%[FileName] wird durch den Dateinamen ersetzt.",
                        "%[FileName] is replaced by the file name.",
                    ),
                );
            });
        }
        Cat::Signatures => {
            section(
                ui,
                tr(de, "Vorgaben für „Dokument signieren“", "Defaults of \"Sign document\""),
                |ui| {
                    egui::Grid::new("sign-defaults").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                        for (label, value) in [
                            (tr(de, "Grund:", "Reason:"), &mut d.sign_reason),
                            (tr(de, "Ort:", "Location:"), &mut d.sign_location),
                            (tr(de, "Kontakt:", "Contact:"), &mut d.sign_contact),
                        ] {
                            ui.label(label);
                            ui.add(egui::TextEdit::singleline(value).desired_width(280.0));
                            ui.end_row();
                        }
                        ui.label(tr(de, "Sichtbare Signatur:", "Visible signature:"));
                        let corners: Vec<(u8, &str)> = super::sign_dialog::corner_names(de)
                            .into_iter()
                            .enumerate()
                            .map(|(i, n)| (i as u8, n))
                            .collect();
                        combo(ui, "sign-corner", &mut d.sign_corner, &corners, 180.0);
                        ui.end_row();
                    });
                    hint(
                        ui,
                        tr(
                            de,
                            "Diese Werte stehen beim Signieren schon im Dialog und lassen sich dort für jede Signatur ändern.",
                            "These values are filled into the signing dialog and can be changed there for each signature.",
                        ),
                    );
                },
            );
            section(ui, tr(de, "Digitale IDs", "Digital IDs"), |ui| match crate::sign::id::ids_dir() {
                Some(dir) => {
                    let count = crate::sign::id::list(&dir).len();
                    ui.label(if de {
                        format!("Gespeicherte IDs: {count}")
                    } else {
                        format!("Stored IDs: {count}")
                    });
                    ui.horizontal(|ui| {
                        ui.label(tr(de, "Ordner:", "Folder:"));
                        ui.add(
                            egui::Label::new(RichText::new(dir.display().to_string()).monospace())
                                .selectable(true),
                        );
                    });
                    hint(
                        ui,
                        tr(
                            de,
                            "Neue IDs legt „Dokument signieren“ an. Eine vorhandene .pfx- oder .p12-Datei kann dort ebenfalls gewählt werden.",
                            "\"Sign document\" creates new IDs. An existing .pfx or .p12 file can be chosen there as well.",
                        ),
                    );
                }
                None => {
                    ui.label(tr(
                        de,
                        "Kein Ordner für digitale IDs gefunden.",
                        "No folder for digital IDs found.",
                    ));
                }
            });
            section(ui, tr(de, "Zeitstempel", "Time stamps"), |ui| {
                ui.checkbox(
                    &mut d.sign_time_stamp,
                    tr(de, "Signaturen mit einem Zeitstempel versehen", "Add a time stamp to signatures"),
                );
                ui.horizontal(|ui| {
                    ui.label(tr(de, "Zeitstempeldienst:", "Time stamp authority:"));
                    ui.add(egui::TextEdit::singleline(&mut d.tsa_url).desired_width(280.0));
                    if ui.button(tr(de, "Standard", "Default")).clicked() {
                        d.tsa_url = crate::sign::tsa::DEFAULT_URL.into();
                    }
                });
                hint(
                    ui,
                    tr(
                        de,
                        "Ein Zeitstempeldienst (RFC 3161) bestätigt unabhängig, wann signiert wurde. Gesendet wird nur ein Prüfwert, nie der Inhalt des Dokuments. „Schützen › Zeitstempel“ versieht ein Dokument auch ohne Signatur mit einem Zeitstempel.",
                        "A time stamp authority (RFC 3161) confirms independently when a document was signed. Only a hash value is sent, never the content of the document. \"Protect › Time stamp\" adds a time stamp without a signature.",
                    ),
                );
            });
            section(ui, tr(de, "Prüfen", "Checking"), |ui| {
                hint(
                    ui,
                    tr(
                        de,
                        "Signaturen, Zertifizierungen und Zeitstempel prüft „Schützen › Signaturen überprüfen“. Ob einem Aussteller vertraut werden kann, wird nicht geprüft.",
                        "Signatures, certifications and time stamps are checked with \"Protect › Check signatures\". Whether an issuer can be trusted is not checked.",
                    ),
                );
            });
        }
        Cat::Forms => {
            section(ui, tr(de, "Hervorheben", "Highlighting"), |ui| {
                ui.checkbox(
                    &mut d.highlight_fields,
                    tr(de, "Formularfelder hervorheben", "Highlight form fields"),
                );
            });
        }
        Cat::Identity => {
            egui::Grid::new("set-identity").num_columns(2).spacing(vec2(10.0, 4.0)).show(ui, |ui| {
                let rows: [(&str, &str, &mut String); 5] = [
                    ("Name:", "Name:", &mut d.author),
                    ("Titel:", "Title:", &mut d.identity.title),
                    ("Organisation:", "Organization:", &mut d.identity.organisation),
                    ("Abteilung:", "Department:", &mut d.identity.department),
                    ("E-Mail-Adresse:", "E-mail address:", &mut d.identity.email),
                ];
                for (g, e, value) in rows {
                    ui.label(tr(de, g, e));
                    ui.add(egui::TextEdit::singleline(value).desired_width(300.0).char_limit(128));
                    ui.end_row();
                }
            });
            hint(
                ui,
                tr(
                    de,
                    "Der Name steht als Autor in neuen Kommentaren. Die Angaben bleiben auf diesem Computer.",
                    "The name is the author of new comments. The details stay on this computer.",
                ),
            );
        }
        Cat::ConvertTo => {
            hint(
                ui,
                tr(
                    de,
                    "VersoloPDF wandelt Bilder (PNG, JPEG, BMP, TIFF …), Text, Markdown, CSV und RTF in PDF um \
                     (Datei › Neues Dokument). Eigene Einstellungen gibt es in den jeweiligen Dialogen.",
                    "VersoloPDF converts images (PNG, JPEG, BMP, TIFF …), text, Markdown, CSV and RTF to PDF \
                     (File › New document). Their options are in the dialogs.",
                ),
            );
        }
        Cat::ConvertFrom => {
            hint(
                ui,
                tr(
                    de,
                    "VersoloPDF exportiert in Bilder, Word (DOCX) und Text (Datei › Exportieren). \
                     Die Einstellungen stehen in den jeweiligen Dialogen.",
                    "VersoloPDF exports to images, Word (DOCX) and text (File › Export). \
                     Their options are in the dialogs.",
                ),
            );
        }
        Cat::Measure => {
            section(ui, tr(de, "Maßeinheit", "Unit"), |ui| {
                ui.horizontal(|ui| {
                    ui.label(tr(de, "Einheit in Seitendialogen:", "Unit of page dialogs:"));
                    let options: Vec<(Unit, &str)> = Unit::ALL.iter().map(|u| (*u, u.suffix())).collect();
                    combo(ui, "set-unit", &mut d.unit, &options, 80.0);
                });
                ui.checkbox(&mut d.show_rulers, tr(de, "Lineale anzeigen", "Show rulers"));
            });
            section(ui, tr(de, "Gitter und Hilfslinien", "Grid and guides"), |ui| {
                ui.horizontal(|ui| {
                    ui.label(tr(de, "Abstand der Gitterlinien:", "Grid spacing:"));
                    ui.add(egui::DragValue::new(&mut d.grid_mm).range(1.0..=200.0).speed(0.5).suffix(" mm"));
                    ui.label(tr(de, "Unterteilung:", "Subdivisions:"));
                    ui.add(egui::DragValue::new(&mut d.grid_subdivisions).range(1..=20));
                });
                ui.checkbox(&mut d.show_grid, tr(de, "Gitter anzeigen", "Show grid"));
                ui.checkbox(&mut d.show_guides, tr(de, "Hilfslinien anzeigen", "Show guides"));
                hint(
                    ui,
                    tr(
                        de,
                        "Hilfslinien werden aus den Linealen auf die Seite gezogen und zum Entfernen zurückgezogen.",
                        "Guides are dragged from the rulers onto the page and dragged back to remove them.",
                    ),
                );
            });
            section(ui, tr(de, "Einrasten", "Snap"), |ui| {
                ui.checkbox(&mut d.snap, tr(de, "Einrasten beim Zeichnen", "Snap while drawing"));
                ui.add_enabled_ui(d.snap, |ui| {
                    ui.checkbox(&mut d.snap_grid, tr(de, "Am Gitter", "To grid"));
                    ui.checkbox(&mut d.snap_guides, tr(de, "An Hilfslinien", "To guides"));
                    ui.checkbox(
                        &mut d.snap_objects,
                        tr(
                            de,
                            "An Objekten (Texte, Bilder, Grafiken)",
                            "To objects (text, images, graphics)",
                        ),
                    );
                });
            });
        }
        Cat::ReadAloud => {
            section(ui, tr(de, "Sprachausgabe", "Speech output"), |ui| {
                hint(
                    ui,
                    tr(
                        de,
                        "VersoloPDF verwendet die Sprachausgabe des Betriebssystems.",
                        "VersoloPDF uses the speech output of the operating system.",
                    ),
                );
                egui::Grid::new("set-read").num_columns(2).spacing(vec2(10.0, 4.0)).show(ui, |ui| {
                    ui.label(tr(de, "Lautstärke:", "Volume:"));
                    ui.add(egui::Slider::new(&mut d.read_volume, 0..=100).suffix(" %"));
                    ui.end_row();
                    ui.label(tr(de, "Tempo:", "Speed:"));
                    ui.add(egui::Slider::new(&mut d.read_rate, -10..=10));
                    ui.end_row();
                });
                ui.checkbox(&mut d.read_fields, tr(de, "Formularfelder vorlesen", "Read form fields"));
                ui.checkbox(&mut d.read_annots, tr(de, "Anmerkungen vorlesen", "Read comments"));
                ui.checkbox(&mut d.read_follow, tr(de, "Automatisch blättern", "Turn pages automatically"));
            });
        }
        Cat::Interface => {
            section(ui, tr(de, "Design", "Design"), |ui| {
                for design in crate::ui::theme::Design::ALL {
                    let (g, e) = design.names();
                    ui.radio_value(&mut d.design, design, tr(de, g, e));
                }
            });
            section(ui, tr(de, "Aussehen", "Appearance"), |ui| {
                ui.checkbox(
                    &mut d.follow_system_theme,
                    tr(de, "Dunkler Modus aus System", "Dark mode from system"),
                );
                ui.add_enabled(
                    !d.follow_system_theme,
                    egui::Checkbox::new(&mut d.dark_mode, tr(de, "Dunkler Modus", "Dark mode")),
                );
                ui.checkbox(&mut d.modern_look, tr(de, "Moderne Darstellung", "Modern look")).on_hover_text(tr(
                    de,
                    "Systemschrift in gut lesbarer Größe, mehr Abstand, ruhigere Farben. Ausgeschaltet: die frühere, kompaktere Darstellung.",
                    "System font in a readable size, more room, calmer colours. Off: the former, more compact look.",
                ));
            });
            section(ui, tr(de, "Größe", "Size"), |ui| {
                ui.horizontal(|ui| {
                    ui.label(tr(de, "Größe der Benutzeroberfläche:", "Size of the user interface:"));
                    let mut pct = (d.ui_scale * 100.0).round();
                    if ui.add(egui::Slider::new(&mut pct, 50.0..=200.0).step_by(10.0).suffix(" %")).changed()
                    {
                        d.ui_scale = pct / 100.0;
                    }
                });
            });
        }
        Cat::Security => {
            section(ui, tr(de, "Öffnen und Ausführen", "Opening and running"), |ui| {
                ui.horizontal(|ui| {
                    ui.label(tr(de, "Webseite vom Dokument geöffnet:", "Web page opened by the document:"));
                    combo(
                        ui,
                        "set-links",
                        &mut d.link_policy,
                        &[
                            (LinkPolicy::Ask, tr(de, "Benutzer fragen", "Ask the user")),
                            (LinkPolicy::Allow, tr(de, "Immer erlauben", "Always allow")),
                            (LinkPolicy::Block, tr(de, "Immer blockieren", "Always block")),
                        ],
                        160.0,
                    );
                });
                hint(
                    ui,
                    tr(
                        de,
                        "Nur Adressen mit http, https und mailto werden geöffnet.",
                        "Only http, https and mailto addresses are opened.",
                    ),
                );
            });
            section(ui, tr(de, "Vertrauensliste", "Trust list"), |ui| {
                let mut remove = None;
                for (i, host) in d.trusted_hosts.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(host.as_str());
                        if ui.small_button(tr(de, "Entfernen", "Remove")).clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    d.trusted_hosts.remove(i);
                }
                if d.trusted_hosts.is_empty() {
                    hint(ui, tr(de, "Keine Webseiten in der Liste.", "No web sites in the list."));
                }
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut s.new_host)
                            .hint_text("example.org")
                            .desired_width(220.0)
                            .char_limit(253),
                    );
                    let host = url_host(&format!("https://{}", s.new_host.trim()));
                    let valid = host.as_ref().is_some_and(|h| {
                        h.contains('.') && h.chars().all(|c| c.is_ascii_alphanumeric() || "-.".contains(c))
                    });
                    if ui.add_enabled(valid, egui::Button::new(tr(de, "Hinzufügen", "Add"))).clicked()
                        && let Some(host) = host
                    {
                        if !d.trusted_hosts.contains(&host) {
                            d.trusted_hosts.push(host);
                        }
                        s.new_host.clear();
                    }
                });
            });
        }
        Cat::Language => {
            for lang in Language::ALL {
                ui.radio_value(&mut d.language, lang, lang.name());
            }
        }
        Cat::FullScreen => {
            section(ui, tr(de, "Vollbild", "Full screen"), |ui| {
                ui.checkbox(
                    &mut d.esc_exits_fullscreen,
                    tr(de, "Beenden durch Drücken der ESC-Taste", "Leave by pressing ESC"),
                );
                hint(
                    ui,
                    tr(de, "F11 schaltet das Vollbild ein und aus.", "F11 switches full screen on and off."),
                );
                ui.checkbox(
                    &mut d.fs_nav_bar,
                    tr(de, "Navigationsleiste anzeigen", "Show the navigation bar"),
                );
                ui.checkbox(
                    &mut d.fs_loop,
                    tr(de, "Nach letzter Seite wiederholen", "Repeat after the last page"),
                );
                ui.horizontal(|ui| {
                    let mut auto = d.fs_auto_seconds > 0;
                    if ui
                        .checkbox(&mut auto, tr(de, "Automatisch weiter nach", "Advance automatically after"))
                        .changed()
                    {
                        d.fs_auto_seconds = if auto { 5 } else { 0 };
                    }
                    ui.add_enabled(auto, egui::DragValue::new(&mut d.fs_auto_seconds).range(1..=3600));
                    ui.label(tr(de, "Sekunden", "seconds"));
                });
                ui.horizontal(|ui| {
                    ui.label(tr(de, "Hintergrund:", "Background:"));
                    ui.color_edit_button_srgb(&mut d.fs_background);
                });
            });
            section(ui, tr(de, "Übergang", "Transition"), |ui| {
                ui.checkbox(
                    &mut d.fs_always_default,
                    tr(de, "Immer den Standardübergang verwenden", "Always use the default transition"),
                );
                ui.horizontal(|ui| {
                    use crate::transitions::Style;
                    ui.label(tr(de, "Standardübergang:", "Default transition:"));
                    let (g, e) = d.fs_default_transition.style.names();
                    egui::ComboBox::from_id_salt("set-fs-trans")
                        .selected_text(tr(de, g, e))
                        .width(200.0)
                        .show_ui(ui, |ui| {
                            for s in Style::ALL.into_iter().skip(1) {
                                let (g, e) = s.names();
                                if ui
                                    .selectable_label(d.fs_default_transition.style == s, tr(de, g, e))
                                    .clicked()
                                {
                                    d.fs_default_transition.style = s;
                                    d.fs_default_transition.direction = None;
                                }
                            }
                        });
                });
                let dirs = d.fs_default_transition.style.directions();
                if !dirs.is_empty() {
                    ui.horizontal(|ui| {
                        ui.label(tr(de, "Richtung:", "Direction:"));
                        let shown = d.fs_default_transition.direction_or_default();
                        let text = shown.map_or("", |x| tr(de, x.names().0, x.names().1));
                        egui::ComboBox::from_id_salt("set-fs-dir").selected_text(text).width(200.0).show_ui(
                            ui,
                            |ui| {
                                for x in dirs {
                                    let (g, e) = x.names();
                                    ui.selectable_value(
                                        &mut d.fs_default_transition.direction,
                                        Some(*x),
                                        tr(de, g, e),
                                    );
                                }
                            },
                        );
                    });
                }
            });
        }
        Cat::Tools => {
            section(ui, tr(de, "Werkzeuge", "Tools"), |ui| {
                ui.horizontal(|ui| {
                    ui.label(tr(de, "Standardwerkzeug:", "Default tool:"));
                    let lang = if de { Language::German } else { Language::English };
                    let options: Vec<(Tool, &str)> =
                        START_TOOLS.iter().map(|t| (*t, Cmd::Tool(*t).label(lang))).collect();
                    combo(ui, "set-tool", &mut d.default_tool, &options, 200.0);
                });
                ui.checkbox(
                    &mut d.remember_tool,
                    tr(
                        de,
                        "Zuletzt verwendetes Werkzeug beim nächsten Start wieder auswählen",
                        "Select the last used tool again at the next start",
                    ),
                );
            });
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_and_search() {
        let names: std::collections::HashSet<_> = Cat::ALL.iter().map(|c| c.name(true)).collect();
        assert_eq!(names.len(), 38);
        let found: Vec<Cat> = Cat::ALL.into_iter().filter(|c| c.matches("dunkel")).collect();
        assert_eq!(found, vec![Cat::Interface]);
        assert!(Cat::Identity.matches("organisation"));
        assert!(Cat::Security.matches("vertrauensliste"));
        assert!(Cat::License.unavailable(true).is_some());
        assert!(Cat::General.unavailable(true).is_none());
    }

    #[test]
    fn sanitize_limits_values() {
        let mut s = Settings {
            ui_scale: 9.0,
            recovery_minutes: 0,
            snapshot_dpi: 5,
            email_subject: "a\nb".into(),
            ..Settings::default()
        };
        sanitize(&mut s);
        assert_eq!(s.ui_scale, 2.0);
        assert_eq!(s.recovery_minutes, 1);
        assert_eq!(s.snapshot_dpi, 36);
        assert_eq!(s.email_subject, "ab");
    }
}
