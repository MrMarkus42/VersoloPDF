//! "Einstellungen" (Strg+K):
//! 38 categories with a search box, OK / Übernehmen / Abbrechen; and
//! "Einstellungen verwalten" (export, import, reset).

use egui::{RichText, Ui, vec2};

use super::commands::Cmd;
use super::dialogs::{Action, Outcome};
use super::page_dialogs::tr;
use super::{Dialog, MrPdfApp};
use crate::ui::i18n::Language;
use crate::ui::pickers::{Unit, section};
use crate::ui::settings::{LinkPolicy, SaveFolder, Settings, url_host};
use crate::ui::tools::Tool;
use crate::ui::view::{PageLayout, ZoomMode};

/// State of the dialog (the edited options are in `MrPdfApp::settings_draft`).
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

    /// Why a category has no options in MrPDF (None: it has some).
    fn unavailable(self, de: bool) -> Option<&'static str> {
        Some(match self {
            Cat::License => tr(
                de,
                "MrPDF ist freie Software und braucht keine Lizenz.",
                "MrPDF is free software and needs no licence.",
            ),
            Cat::Measure3d | Cat::Media3d => {
                tr(de, "MrPDF zeigt keine 3D-Inhalte an.", "MrPDF does not show 3D content.")
            }
            Cat::Launch => tr(
                de,
                "MrPDF startet keine fremden Programme aus Dokumenten.",
                "MrPDF does not start other programs from documents.",
            ),
            Cat::FileAssoc => tr(
                de,
                "Das Standardprogramm für PDF-Dateien wird in den Einstellungen des Betriebssystems festgelegt.",
                "The default program for PDF files is set in the settings of the operating system.",
            ),
            Cat::InfoBar => tr(
                de,
                "MrPDF zeigt nur die Wiederherstellungsleiste (siehe „Dokumente“).",
                "MrPDF only shows the recovery bar (see \"Documents\").",
            ),
            Cat::Plugins => tr(de, "MrPDF hat keine Erweiterungen.", "MrPDF has no plug-ins."),
            Cat::ColorManagement => tr(
                de,
                "Farben werden ohne Farbprofile angezeigt.",
                "Colours are shown without colour profiles.",
            ),
            Cat::JavaScript => tr(
                de,
                "MrPDF führt kein JavaScript aus Dokumenten aus.",
                "MrPDF does not run JavaScript from documents.",
            ),
            Cat::Ai => tr(de, "MrPDF hat keinen KI-Dienst.", "MrPDF has no AI service."),
            Cat::Performance => tr(
                de,
                "Speicher und Rechenkerne werden automatisch verwaltet.",
                "Memory and processor cores are managed automatically.",
            ),
            Cat::Update => tr(
                de,
                "MrPDF sucht nicht selbst nach neuen Versionen.",
                "MrPDF does not look for new versions by itself.",
            ),
            Cat::Scanner => tr(de, "MrPDF kann nicht scannen.", "MrPDF cannot scan."),
            Cat::SharePoint | Cat::Places => tr(
                de,
                "MrPDF arbeitet nur mit Dateien auf diesem Computer.",
                "MrPDF only works with files on this computer.",
            ),
            Cat::Signatures => tr(
                de,
                "Digitale Signaturen sind in MrPDF noch nicht vorhanden.",
                "Digital signatures are not available in MrPDF yet.",
            ),
            Cat::SearchEngines => {
                tr(de, "MrPDF sucht nicht im Internet.", "MrPDF does not search the internet.")
            }
            Cat::Ocr => tr(de, "MrPDF hat keine Texterkennung.", "MrPDF has no OCR."),
            Cat::ReadAloud => tr(
                de,
                "Vorlesen ist in MrPDF noch nicht vorhanden.",
                "Read aloud is not available in MrPDF yet.",
            ),
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
    s.ui_scale = s.ui_scale.clamp(0.5, 2.0);
    s.recovery_minutes = s.recovery_minutes.clamp(1, 600);
    if s.snapshot_dpi != 0 {
        s.snapshot_dpi = s.snapshot_dpi.clamp(36, 1200);
    }
    s.trusted_hosts.truncate(200);
}

impl MrPdfApp {
    /// Opens "Einstellungen".
    pub fn open_settings(&mut self) {
        self.settings_draft = Some(Box::new(self.settings.clone()));
        self.dialog = Some(Dialog::Settings(Box::default()));
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
        ui.heading(tr(de, "Einstellungen", "Settings"));
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
                    for cat in Cat::ALL {
                        if !cat.matches(&query) {
                            continue;
                        }
                        let text = if cat.unavailable(de).is_some() {
                            RichText::new(cat.name(de)).weak()
                        } else {
                            RichText::new(cat.name(de))
                        };
                        if ui.selectable_label(s.page == cat, text).clicked() {
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
            .add_filter(tr(de, "MrPDF-Einstellungen", "MrPDF settings"), &["ron"])
            .set_file_name("MrPDF-Einstellungen.ron")
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
            .add_filter(tr(de, "MrPDF-Einstellungen", "MrPDF settings"), &["ron"])
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
                            (PageLayout::Facing, tr(de, "Doppelseiten", "Facing")),
                            (PageLayout::Cover, tr(de, "Doppelseiten mit Deckblatt", "Facing with cover")),
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
                    "MrPDF wandelt Bilder (PNG, JPEG, BMP, TIFF …), Text, Markdown, CSV und RTF in PDF um \
                     (Datei › Neues Dokument). Eigene Einstellungen gibt es in den jeweiligen Dialogen.",
                    "MrPDF converts images (PNG, JPEG, BMP, TIFF …), text, Markdown, CSV and RTF to PDF \
                     (File › New document). Their options are in the dialogs.",
                ),
            );
        }
        Cat::ConvertFrom => {
            hint(
                ui,
                tr(
                    de,
                    "MrPDF exportiert in Bilder, Word (DOCX) und Text (Datei › Exportieren). \
                     Die Einstellungen stehen in den jeweiligen Dialogen.",
                    "MrPDF exports to images, Word (DOCX) and text (File › Export). \
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
            });
        }
        Cat::Interface => {
            section(ui, tr(de, "Aussehen", "Appearance"), |ui| {
                ui.checkbox(
                    &mut d.follow_system_theme,
                    tr(de, "Dunkler Modus aus System", "Dark mode from system"),
                );
                ui.add_enabled(
                    !d.follow_system_theme,
                    egui::Checkbox::new(&mut d.dark_mode, tr(de, "Dunkler Modus", "Dark mode")),
                );
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
