//! "Einstellungen für Excel-Umwandlung" and "… PowerPoint-Umwandlung"
//! (ribbon tab "Umwandeln").

use egui::{RichText, Ui};

use super::commands::Cmd;
use super::dialogs::{Action, Outcome};
use super::page_dialogs::{footer, tr};
use super::{Dialog, VersoloPdfApp};
use crate::office::{ExcelOptions, NumberFormat, PowerPointOptions};
use crate::ui::i18n::Language;
use crate::ui::pickers::{PageRange, RangeMode, section};

/// State of the Excel dialog.
#[derive(Debug, Clone, PartialEq)]
pub struct ExcelState {
    pub range: PageRange,
    /// "Lokale Spracheinstellungen verwenden" (false: own separators).
    pub local: bool,
    pub decimal: char,
    pub thousands: Option<char>,
    pub detect_tables: bool,
    pub sheet_per_page: bool,
    pub row_per_paragraph: bool,
    pub ocr: bool,
    pub protect: Protect,
}

/// State of the PowerPoint dialog.
#[derive(Debug, Clone, PartialEq)]
pub struct PowerPointState {
    pub range: PageRange,
    pub whole_page_image: bool,
    pub dpi: u32,
    pub comments: bool,
    pub links: bool,
    /// "Modus: In Kurven umwandeln".
    pub curves: bool,
    pub ocr: bool,
    pub protect: Protect,
}

/// State of the Word dialog.
#[derive(Debug, Clone, PartialEq)]
pub struct WordState {
    pub range: PageRange,
    /// "Seitenlayout erhalten" (false: "Textfluss erhalten").
    pub keep_layout: bool,
    pub tables: bool,
    pub images: bool,
    pub ocr: bool,
    pub protect: Protect,
}

/// "Vor Änderung mit Kennwort schützen".
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Protect {
    pub on: bool,
    pub password: String,
}

impl Protect {
    pub fn password(&self) -> Option<String> {
        (self.on && !self.password.is_empty()).then(|| self.password.clone())
    }

    pub fn ready(&self) -> bool {
        !self.on || !self.password.is_empty()
    }

    pub fn ui(&mut self, ui: &mut Ui, de: bool) {
        ui.horizontal(|ui| {
            ui.checkbox(
                &mut self.on,
                tr(de, "Vor Änderung mit Kennwort schützen:", "Protect from changes with password:"),
            );
            ui.add_enabled(
                self.on,
                egui::TextEdit::singleline(&mut self.password).password(true).desired_width(160.0),
            )
            .on_hover_text(tr(
                de,
                "Die Datei lässt sich ohne Kennwort nur lesen. Sie wird nicht verschlüsselt.",
                "Without the password the file can only be read. It is not encrypted.",
            ));
        });
    }
}

const DPIS: [u32; 5] = [72, 96, 150, 200, 300];

impl VersoloPdfApp {
    fn od_de(&self) -> bool {
        self.settings.language == Language::German
    }

    fn local_numbers(&self) -> NumberFormat {
        if self.od_de() { NumberFormat::GERMAN } else { NumberFormat::ENGLISH }
    }

    pub(super) fn open_word_export(&mut self) {
        self.dialog = Some(Dialog::ExportWord(Box::new(WordState {
            range: PageRange::all(),
            keep_layout: false,
            tables: true,
            images: true,
            ocr: false,
            protect: Protect::default(),
        })));
    }

    pub(super) fn word_dialog(&mut self, ui: &mut Ui, s: &mut WordState) -> (Outcome, Option<Action>) {
        let de = self.od_de();
        let (n, cur) = self.active_tab().map_or((0, 0), |t| (t.doc.page_count(), t.current_page));
        super::dialogs::set_title(
            ui,
            tr(de, "Einstellungen für Word-Umwandlung", "Word conversion settings"),
        );
        s.range.ui(ui, de, n, cur, &[RangeMode::All, RangeMode::Current, RangeMode::Custom]);
        section(ui, tr(de, "Einstellungen für Layout", "Layout settings"), |ui| {
            ui.horizontal(|ui| {
                ui.radio_value(&mut s.keep_layout, true, tr(de, "Seitenlayout erhalten", "Keep page layout"));
                ui.radio_value(&mut s.keep_layout, false, tr(de, "Textfluss erhalten", "Keep text flow"));
            });
            ui.label(
                RichText::new(if s.keep_layout {
                    tr(
                        de,
                        "Jede Seite wird eine Word-Seite gleicher Größe: Grafiken als Bild im Hintergrund, der Text in Rahmen an der Originalstelle mit Originalschrift.",
                        "Every page becomes a Word page of the same size: graphics as picture behind, the text in frames at the original place with the original font.",
                    )
                } else {
                    tr(
                        de,
                        "Der Text fließt; Überschriften werden an Lesezeichen erkannt, Tabellen an ihren Spalten.",
                        "The text flows; headings are recognised by bookmarks, tables by their columns.",
                    )
                })
                .small()
                .weak(),
            );
            ui.add_enabled_ui(!s.keep_layout, |ui| {
                ui.checkbox(&mut s.tables, tr(de, "Tabellen erkennen", "Detect tables"));
                ui.checkbox(&mut s.images, tr(de, "Bilder übernehmen", "Transfer pictures"));
            });
        });
        self.ocr_option(ui, &mut s.ocr);
        s.protect.ui(ui, de);
        let pages = s.range.resolve(n, cur);
        let (ok, cancel, _) = footer(ui, de, pages.is_some() && s.protect.ready(), false, "OK");
        if cancel {
            return (Outcome::Close, None);
        }
        let (true, Some(pages)) = (ok, pages) else { return (Outcome::Keep, None) };
        let o = crate::office::WordOptions {
            keep_layout: s.keep_layout,
            tables: s.tables,
            images: s.images,
            dpi: 150.0,
            ocr: if s.ocr { self.ocr_engine() } else { None },
        };
        let password = s.protect.password();
        (
            Outcome::Close,
            Some(Box::new(move |app, ctx| {
                app.save_office(ctx, Cmd::ExportWord, "docx", "Word", |doc, path| {
                    crate::office::export_word(doc, path, &pages, &o, password.as_deref())
                });
            })),
        )
    }

    pub(super) fn open_excel_export(&mut self) {
        let n = self.local_numbers();
        self.dialog = Some(Dialog::ExportExcel(Box::new(ExcelState {
            range: PageRange::all(),
            local: true,
            decimal: n.decimal,
            thousands: n.thousands,
            detect_tables: true,
            sheet_per_page: true,
            row_per_paragraph: false,
            ocr: false,
            protect: Protect::default(),
        })));
    }

    pub(super) fn open_powerpoint_export(&mut self) {
        self.dialog = Some(Dialog::ExportPowerPoint(Box::new(PowerPointState {
            range: PageRange::all(),
            whole_page_image: false,
            dpi: 150,
            comments: true,
            links: true,
            curves: false,
            ocr: false,
            protect: Protect::default(),
        })));
    }

    /// The OCR program with the chosen languages, if installed.
    pub(super) fn ocr_engine(&self) -> Option<crate::ocr::Engine> {
        crate::ocr::find_tesseract(self.settings.tesseract.as_deref())
            .map(|exe| crate::ocr::Engine { exe, languages: self.settings.ocr_languages.clone() })
    }

    /// "Beschädigten Text mit Texterkennung verbessern": only with Tesseract.
    pub(super) fn ocr_option(&self, ui: &mut Ui, on: &mut bool) {
        let de = self.od_de();
        let available = crate::ocr::find_tesseract(self.settings.tesseract.as_deref()).is_some();
        let label = tr(de, "Beschädigten Text mit Texterkennung verbessern", "Improve damaged text with OCR");
        let resp = ui.add_enabled(available, egui::Checkbox::new(on, label));
        if !available {
            resp.on_disabled_hover_text(tr(
                de,
                "Dafür wird das kostenlose Programm Tesseract benötigt (siehe „Text auf Seiten erkennen“).",
                "This needs the free program Tesseract (see \"Recognize text\").",
            ));
            *on = false;
        }
    }

    /// Asks for the target file and writes it with `write`.
    pub(super) fn save_office(
        &mut self,
        ctx: &egui::Context,
        cmd: Cmd,
        ext: &str,
        filter: &str,
        write: impl FnOnce(&mut crate::Document, &std::path::Path) -> crate::Result<()>,
    ) {
        let lang = self.settings.language;
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        let stem = tab.doc.display_name().trim_end_matches(".pdf").trim_end_matches(".PDF").to_string();
        let Some(path) = rfd::FileDialog::new()
            .set_title(cmd.label(lang))
            .add_filter(filter, &[ext])
            .set_file_name(format!("{stem}.{ext}"))
            .save_file()
        else {
            return;
        };
        match write(&mut tab.doc, &path) {
            Ok(()) => {
                let msg = if lang == Language::German { "Exportiert: " } else { "Exported: " };
                self.set_status(ctx, format!("{msg}{}", path.display()));
            }
            Err(err) => self.show_error(&err),
        }
    }

    pub(super) fn excel_dialog(&mut self, ui: &mut Ui, s: &mut ExcelState) -> (Outcome, Option<Action>) {
        let de = self.od_de();
        let (n, cur) = self.active_tab().map_or((0, 0), |t| (t.doc.page_count(), t.current_page));
        super::dialogs::set_title(
            ui,
            tr(de, "Einstellungen für Excel-Umwandlung", "Excel conversion settings"),
        );
        s.range.ui(ui, de, n, cur, &[RangeMode::All, RangeMode::Current, RangeMode::Custom]);
        section(ui, tr(de, "Einstellungen für Zahlen", "Number settings"), |ui| {
            if ui
                .radio_value(
                    &mut s.local,
                    true,
                    tr(de, "Lokale Spracheinstellungen verwenden", "Use local language settings"),
                )
                .clicked()
            {
                let n = self.local_numbers();
                s.decimal = n.decimal;
                s.thousands = n.thousands;
            }
            ui.radio_value(
                &mut s.local,
                false,
                tr(
                    de,
                    "Eigene Dezimal- und Tausendertrennzeichen verwenden:",
                    "Use own decimal and thousands separators:",
                ),
            );
            ui.add_enabled_ui(!s.local, |ui| {
                egui::Grid::new("excel-sep").num_columns(2).show(ui, |ui| {
                    ui.label(tr(de, "Dezimaltrennzeichen:", "Decimal separator:"));
                    egui::ComboBox::from_id_salt("excel-dec").selected_text(s.decimal.to_string()).show_ui(
                        ui,
                        |ui| {
                            for c in [',', '.'] {
                                ui.selectable_value(&mut s.decimal, c, c.to_string());
                            }
                        },
                    );
                    ui.end_row();
                    ui.label(tr(de, "Tausendertrennzeichen:", "Thousands separator:"));
                    let shown = match s.thousands {
                        None => tr(de, "(keines)", "(none)").to_string(),
                        Some(' ') => tr(de, "Leerzeichen", "Space").to_string(),
                        Some(c) => c.to_string(),
                    };
                    egui::ComboBox::from_id_salt("excel-thou").selected_text(shown).show_ui(ui, |ui| {
                        ui.selectable_value(&mut s.thousands, None, tr(de, "(keines)", "(none)"));
                        for c in ['.', ',', '\''] {
                            ui.selectable_value(&mut s.thousands, Some(c), c.to_string());
                        }
                        ui.selectable_value(&mut s.thousands, Some(' '), tr(de, "Leerzeichen", "Space"));
                    });
                    ui.end_row();
                });
            });
        });
        ui.checkbox(&mut s.detect_tables, tr(de, "Tabellen erkennen", "Detect tables"));
        egui::Grid::new("excel-opts").num_columns(2).show(ui, |ui| {
            ui.label(tr(de, "Arbeitsblatt erstellen:", "Create worksheet:"));
            let per_page = tr(de, "Für jede Seite", "For each page");
            let one = tr(de, "Eins für das Dokument", "One for the document");
            egui::ComboBox::from_id_salt("excel-sheet")
                .selected_text(if s.sheet_per_page { per_page } else { one })
                .width(180.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut s.sheet_per_page, true, per_page);
                    ui.selectable_value(&mut s.sheet_per_page, false, one);
                });
            ui.end_row();
            ui.label(tr(de, "Neue Zeile in der Tabelle nutzen pro:", "Use a new table row per:"));
            let (para, line) = (tr(de, "Absatz", "Paragraph"), tr(de, "Zeile", "Line"));
            egui::ComboBox::from_id_salt("excel-row")
                .selected_text(if s.row_per_paragraph { para } else { line })
                .width(180.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut s.row_per_paragraph, true, para);
                    ui.selectable_value(&mut s.row_per_paragraph, false, line);
                });
            ui.end_row();
        });
        self.ocr_option(ui, &mut s.ocr);
        s.protect.ui(ui, de);
        let pages = s.range.resolve(n, cur);
        let (ok, cancel, _) = footer(ui, de, pages.is_some() && s.protect.ready(), false, "OK");
        if cancel {
            return (Outcome::Close, None);
        }
        let (true, Some(pages)) = (ok, pages) else { return (Outcome::Keep, None) };
        let o = ExcelOptions {
            numbers: NumberFormat { decimal: s.decimal, thousands: s.thousands.filter(|t| *t != s.decimal) },
            detect_tables: s.detect_tables,
            sheet_per_page: s.sheet_per_page,
            row_per_paragraph: s.row_per_paragraph,
            ocr: if s.ocr { self.ocr_engine() } else { None },
        };
        let password = s.protect.password();
        (
            Outcome::Close,
            Some(Box::new(move |app, ctx| {
                app.save_office(ctx, Cmd::ExportExcel, "xlsx", "Excel", |doc, path| {
                    crate::office::export_excel_protected(doc, path, &pages, &o, password.as_deref())
                });
            })),
        )
    }

    pub(super) fn powerpoint_dialog(
        &mut self,
        ui: &mut Ui,
        s: &mut PowerPointState,
    ) -> (Outcome, Option<Action>) {
        let de = self.od_de();
        let (n, cur) = self.active_tab().map_or((0, 0), |t| (t.doc.page_count(), t.current_page));
        super::dialogs::set_title(
            ui,
            tr(de, "Einstellungen für PowerPoint-Umwandlung", "PowerPoint conversion settings"),
        );
        s.range.ui(ui, de, n, cur, &[RangeMode::All, RangeMode::Current, RangeMode::Custom]);
        ui.checkbox(
            &mut s.whole_page_image,
            tr(
                de,
                "Gesamten Seiteninhalt in ein Bild umwandeln",
                "Convert the whole page content into one picture",
            ),
        );
        ui.horizontal(|ui| {
            ui.label(tr(de, "Auflösung:", "Resolution:"));
            egui::ComboBox::from_id_salt("ppt-dpi").selected_text(format!("{} dpi", s.dpi)).show_ui(
                ui,
                |ui| {
                    for d in DPIS {
                        ui.selectable_value(&mut s.dpi, d, format!("{d} dpi"));
                    }
                },
            );
        });
        section(ui, tr(de, "Einstellungen zur Textumwandlung", "Text conversion settings"), |ui| {
            ui.add_enabled_ui(!s.whole_page_image, |ui| {
                ui.horizontal(|ui| {
                    ui.label(tr(de, "Modus:", "Mode:"));
                    let editable = tr(de, "Bearbeitbarkeit von Text erhalten", "Keep text editable");
                    let curves = tr(de, "In Kurven umwandeln", "Convert to curves");
                    egui::ComboBox::from_id_salt("ppt-mode")
                        .selected_text(if s.curves { curves } else { editable })
                        .width(240.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut s.curves, false, editable);
                            ui.selectable_value(&mut s.curves, true, curves);
                        });
                });
                ui.label(
                    RichText::new(if s.curves {
                        tr(
                            de,
                            "Der Text wird zu Formen mit den Umrissen der Originalschrift (nicht mehr bearbeitbar, sieht aber überall gleich aus).",
                            "The text becomes shapes with the outlines of the original font (no longer editable, but looks the same everywhere).",
                        )
                    } else {
                        tr(
                            de,
                            "Die Textfelder verwenden die Originalschrift, Größe, Farbe, fett und kursiv. Ist die Schrift auf dem Rechner nicht installiert, ersetzt PowerPoint sie.",
                            "The text boxes use the original font, size, colour, bold and italic. If the font is not installed, PowerPoint replaces it.",
                        )
                    })
                    .small()
                    .weak(),
                );
            });
            self.ocr_option(ui, &mut s.ocr);
        });
        ui.checkbox(&mut s.comments, tr(de, "Kommentare einschließen", "Include comments"));
        ui.checkbox(&mut s.links, tr(de, "Verweise einschließen", "Include links"));
        s.protect.ui(ui, de);
        let pages = s.range.resolve(n, cur);
        let (ok, cancel, _) = footer(ui, de, pages.is_some() && s.protect.ready(), false, "OK");
        if cancel {
            return (Outcome::Close, None);
        }
        let (true, Some(pages)) = (ok, pages) else { return (Outcome::Keep, None) };
        let o = PowerPointOptions {
            ocr: if s.ocr { self.ocr_engine() } else { None },
            whole_page_image: s.whole_page_image,
            dpi: s.dpi as f32,
            comments: s.comments,
            links: s.links,
            curves: s.curves,
        };
        let password = s.protect.password();
        (
            Outcome::Close,
            Some(Box::new(move |app, ctx| {
                app.save_office(ctx, Cmd::ExportPowerPoint, "pptx", "PowerPoint", |doc, path| {
                    crate::office::export_powerpoint_protected(doc, path, &pages, &o, password.as_deref())
                });
            })),
        )
    }
}
