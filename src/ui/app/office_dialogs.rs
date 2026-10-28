//! "Einstellungen für Excel-Umwandlung" and "… PowerPoint-Umwandlung"
//! (ribbon tab "Umwandeln").

use egui::{RichText, Ui};

use super::commands::Cmd;
use super::dialogs::{Action, Outcome};
use super::page_dialogs::{footer, tr};
use super::{Dialog, MrPdfApp};
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
}

/// State of the PowerPoint dialog.
#[derive(Debug, Clone, PartialEq)]
pub struct PowerPointState {
    pub range: PageRange,
    pub whole_page_image: bool,
    pub dpi: u32,
    pub comments: bool,
    pub links: bool,
}

const DPIS: [u32; 5] = [72, 96, 150, 200, 300];

fn not_offered(ui: &mut Ui, de: bool, label: &str) {
    let _ = ui.add_enabled(false, egui::Checkbox::new(&mut false, label)).on_disabled_hover_text(tr(
        de,
        "Texterkennung (OCR) ist in MrPDF noch nicht enthalten.",
        "Text recognition (OCR) is not included in MrPDF yet.",
    ));
}

impl MrPdfApp {
    fn od_de(&self) -> bool {
        self.settings.language == Language::German
    }

    fn local_numbers(&self) -> NumberFormat {
        if self.od_de() { NumberFormat::GERMAN } else { NumberFormat::ENGLISH }
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
        })));
    }

    pub(super) fn open_powerpoint_export(&mut self) {
        self.dialog = Some(Dialog::ExportPowerPoint(Box::new(PowerPointState {
            range: PageRange::all(),
            whole_page_image: false,
            dpi: 150,
            comments: true,
            links: true,
        })));
    }

    /// Asks for the target file and writes it with `write`.
    fn save_office(
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
            egui::ComboBox::from_id_salt("excel-sheet")
                .selected_text(if s.sheet_per_page {
                    tr(de, "Für jede Seite", "For each page")
                } else {
                    tr(de, "Eins für das Dokument", "One for the document")
                })
                .width(180.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut s.sheet_per_page,
                        true,
                        tr(de, "Für jede Seite", "For each page"),
                    );
                    ui.selectable_value(
                        &mut s.sheet_per_page,
                        false,
                        tr(de, "Eins für das Dokument", "One for the document"),
                    );
                });
            ui.end_row();
            ui.label(tr(de, "Neue Zeile in der Tabelle nutzen pro:", "Use a new table row per:"));
            egui::ComboBox::from_id_salt("excel-row")
                .selected_text(if s.row_per_paragraph {
                    tr(de, "Absatz", "Paragraph")
                } else {
                    tr(de, "Zeile", "Line")
                })
                .width(180.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut s.row_per_paragraph, true, tr(de, "Absatz", "Paragraph"));
                    ui.selectable_value(&mut s.row_per_paragraph, false, tr(de, "Zeile", "Line"));
                });
            ui.end_row();
        });
        not_offered(
            ui,
            de,
            tr(de, "Beschädigten Text mit Texterkennung verbessern", "Improve damaged text with OCR"),
        );
        let pages = s.range.resolve(n, cur);
        let (ok, cancel, _) = footer(ui, de, pages.is_some(), false, "OK");
        if cancel {
            return (Outcome::Close, None);
        }
        let (true, Some(pages)) = (ok, pages) else { return (Outcome::Keep, None) };
        let o = ExcelOptions {
            numbers: NumberFormat { decimal: s.decimal, thousands: s.thousands.filter(|t| *t != s.decimal) },
            detect_tables: s.detect_tables,
            sheet_per_page: s.sheet_per_page,
            row_per_paragraph: s.row_per_paragraph,
        };
        (
            Outcome::Close,
            Some(Box::new(move |app, ctx| {
                app.save_office(ctx, Cmd::ExportExcel, "xlsx", "Excel", |doc, path| {
                    crate::office::export_excel(doc, path, &pages, &o)
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
        ui.label(
            RichText::new(tr(
                de,
                "Bilder, Grafiken und Farbverläufe kommen als Bild auf die Folie, der Text bleibt bearbeitbar. Ist die Option oben aktiv, wird die ganze Seite ein Bild.",
                "Pictures, graphics and gradients become a picture on the slide, the text stays editable. With the option above the whole page becomes a picture.",
            ))
            .small()
            .weak(),
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
                    let mut mode = 0;
                    egui::ComboBox::from_id_salt("ppt-mode")
                        .selected_text(tr(de, "Bearbeitbarkeit von Text erhalten", "Keep text editable"))
                        .width(240.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut mode, 0, tr(de, "Bearbeitbarkeit von Text erhalten", "Keep text editable"));
                            let _ = ui
                                .add_enabled(false, egui::Button::new(tr(de, "In Kurven umwandeln", "Convert to curves")).frame(false))
                                .on_disabled_hover_text(tr(de, "Text in Kurven umwandeln kann MrPDF nicht.", "MrPDF cannot convert text into curves."));
                        });
                });
                ui.label(
                    RichText::new(tr(
                        de,
                        "Die Textfelder verwenden Arial in der Größe des Originals; die genaue Schrift wird nicht eingebettet.",
                        "The text boxes use Arial at the original size; the exact font is not embedded.",
                    ))
                    .small()
                    .weak(),
                );
            });
            not_offered(
                ui,
                de,
                tr(de, "Beschädigten Text mit Texterkennung verbessern", "Improve damaged text with OCR"),
            );
        });
        ui.checkbox(&mut s.comments, tr(de, "Kommentare einschließen", "Include comments"));
        ui.checkbox(&mut s.links, tr(de, "Verweise einschließen", "Include links"));
        let pages = s.range.resolve(n, cur);
        let (ok, cancel, _) = footer(ui, de, pages.is_some(), false, "OK");
        if cancel {
            return (Outcome::Close, None);
        }
        let (true, Some(pages)) = (ok, pages) else { return (Outcome::Keep, None) };
        let o = PowerPointOptions {
            whole_page_image: s.whole_page_image,
            dpi: s.dpi as f32,
            comments: s.comments,
            links: s.links,
        };
        (
            Outcome::Close,
            Some(Box::new(move |app, ctx| {
                app.save_office(ctx, Cmd::ExportPowerPoint, "pptx", "PowerPoint", |doc, path| {
                    crate::office::export_powerpoint(doc, path, &pages, &o)
                });
            })),
        )
    }
}
