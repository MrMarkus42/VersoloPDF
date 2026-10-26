//! Export dialogs of "Datei → Exportieren":
//! "Als Bilddateien exportieren" and "Einstellungen für Word-Umwandlung".

use std::path::PathBuf;

use egui::{RichText, Ui, vec2};

use super::commands::Cmd;
use super::dialogs::{Action, Outcome};
use super::page_dialogs::{footer, tr};
use super::preview::doc_preview;
use super::{Dialog, MrPdfApp};
use crate::export_image::{ExportMode, ImageExportOptions, ImageFormat};
use crate::ui::pickers::{PageRange, RangeMode, parse_pages, preset_combo, section};

/// State of "Als Bilddateien exportieren".
#[derive(Debug, Clone, PartialEq)]
pub struct ExportImagesState {
    pub options: ImageExportOptions,
    pub range: PageRange,
    pub folder: PathBuf,
    /// Horizontal and vertical resolution linked.
    pub linked: bool,
    pub preset_name: String,
}

/// The comma separated parts of a custom range (for "Jeden Seitenbereich …").
fn range_parts(range: &PageRange, count: usize) -> Vec<Vec<usize>> {
    if range.mode != RangeMode::Custom {
        return Vec::new();
    }
    range.text.split([',', ';']).filter_map(|part| parse_pages(part, count)).collect()
}

impl MrPdfApp {
    fn ex_de(&self) -> bool {
        self.settings.language == crate::ui::i18n::Language::German
    }

    /// Opens the export dialogs; false for other commands.
    pub(super) fn run_export(&mut self, cmd: Cmd) -> bool {
        let Some(tab) = self.active_tab() else { return false };
        match cmd {
            Cmd::ExportImages => {
                let folder = tab
                    .doc
                    .path()
                    .and_then(|p| p.parent())
                    .map(PathBuf::from)
                    .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
                self.clear_previews();
                self.dialog = Some(Dialog::ExportImages(Box::new(ExportImagesState {
                    options: ImageExportOptions::default(),
                    range: PageRange::all(),
                    folder,
                    linked: true,
                    preset_name: String::new(),
                })));
            }
            Cmd::ExportWord => self.dialog = Some(Dialog::ExportWord { range: PageRange::all() }),
            _ => return false,
        }
        true
    }

    /// "Als Bilddateien exportieren".
    pub(super) fn export_images_dialog(
        &mut self,
        ui: &mut Ui,
        state: &mut ExportImagesState,
    ) -> (Outcome, Option<Action>) {
        let de = self.ex_de();
        let (n, cur) = self.active_tab().map_or((0, 0), |t| (t.doc.page_count(), t.current_page));
        ui.set_max_width(800.0);
        ui.heading(tr(de, "Als Bilddateien exportieren", "Export as image files"));
        let o = &mut state.options;
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(460.0);
                state.range.ui(ui, de, n, cur, &[RangeMode::All, RangeMode::Current, RangeMode::Custom]);
                section(ui, tr(de, "Speichern als", "Save as"), |ui| {
                    egui::Grid::new("export-img").num_columns(2).spacing(vec2(8.0, 5.0)).show(ui, |ui| {
                        ui.label(tr(de, "Bildformat:", "Image format:"));
                        ui.horizontal(|ui| {
                            egui::ComboBox::from_id_salt("export-format")
                                .selected_text(o.format.label())
                                .width(260.0)
                                .show_ui(ui, |ui| {
                                    for f in ImageFormat::ALL {
                                        ui.selectable_value(&mut o.format, f, f.label());
                                    }
                                    for other in
                                        ["GIF", "WebP", "JPEG2000", "JBIG2", "PCX/DCX", "PBM/PGM/PPM"]
                                    {
                                        let _ = ui
                                            .add_enabled(false, egui::Button::new(other).frame(false))
                                            .on_disabled_hover_text(tr(
                                                de,
                                                "Dieses Format wird von MrPDF nicht geschrieben.",
                                                "MrPDF does not write this format.",
                                            ));
                                    }
                                });
                        });
                        ui.end_row();
                        match o.format {
                            ImageFormat::Jpeg => {
                                ui.label(tr(de, "Qualität:", "Quality:"));
                                ui.add(egui::Slider::new(&mut o.jpeg_quality, 1..=100));
                                ui.end_row();
                            }
                            ImageFormat::Tiff => {
                                ui.label(tr(de, "Kompression:", "Compression:"));
                                ui.horizontal(|ui| {
                                    ui.radio_value(&mut o.tiff_zip, true, "ZIP");
                                    ui.radio_value(&mut o.tiff_zip, false, tr(de, "Keine", "None"));
                                });
                                ui.end_row();
                            }
                            _ => {}
                        }
                        ui.label(tr(de, "Zielordner:", "Target folder:"));
                        ui.horizontal(|ui| {
                            let mut text = state.folder.display().to_string();
                            if ui.add(egui::TextEdit::singleline(&mut text).desired_width(260.0)).changed() {
                                state.folder = PathBuf::from(text);
                            }
                            if ui
                                .button("…")
                                .on_hover_text(tr(de, "Ordner wählen", "Choose folder"))
                                .clicked()
                                && let Some(dir) =
                                    rfd::FileDialog::new().set_directory(&state.folder).pick_folder()
                            {
                                state.folder = dir;
                            }
                        });
                        ui.end_row();
                        ui.label(tr(de, "Dateiname:", "File name:"));
                        ui.horizontal(|ui| {
                            ui.add(egui::TextEdit::singleline(&mut o.file_name).desired_width(220.0));
                            ui.menu_button(tr(de, "Makros", "Macros"), |ui| {
                                for (name, value) in [
                                    (tr(de, "Dateiname", "File name"), "%[FileName]"),
                                    (tr(de, "Automatisch nummerieren", "Auto number"), "%[AutoNumber]"),
                                    (tr(de, "Seitenzahl", "Page number"), "%[Page]"),
                                    (tr(de, "Seiten", "Pages"), "%[Pages]"),
                                ] {
                                    if ui.button(name).clicked() {
                                        o.file_name.push_str(value);
                                        ui.close();
                                    }
                                }
                            });
                        });
                        ui.end_row();
                        ui.label(tr(de, "Exportmodus:", "Export mode:"));
                        let multi = o.format.multi_page();
                        let names = [
                            (
                                ExportMode::OneFile,
                                tr(
                                    de,
                                    "Alle Seiten in eine Mehr-Seiten-Bilddatei speichern",
                                    "All pages into one multi-page image",
                                ),
                            ),
                            (
                                ExportMode::PerRange,
                                tr(
                                    de,
                                    "Jeden Seitenbereich in eine eigene Mehr-Seiten-Bilddatei speichern",
                                    "Each page range into its own multi-page image",
                                ),
                            ),
                            (
                                ExportMode::PerPage,
                                tr(
                                    de,
                                    "Jede Seite in eine eigene Bilddatei speichern",
                                    "Each page into its own image file",
                                ),
                            ),
                        ];
                        let mode = o.effective_mode();
                        let current = names.iter().find(|x| x.0 == mode).map_or("", |x| x.1);
                        egui::ComboBox::from_id_salt("export-mode")
                            .selected_text(current)
                            .width(300.0)
                            .show_ui(ui, |ui| {
                                for (v, t) in names {
                                    ui.add_enabled_ui(multi || v == ExportMode::PerPage, |ui| {
                                        ui.selectable_value(&mut o.mode, v, t);
                                    });
                                }
                            });
                        ui.end_row();
                    });
                    ui.checkbox(
                        &mut o.open_folder,
                        tr(de, "Öffne Zielordner und wähle Dateien aus", "Open the target folder"),
                    );
                });
                section(ui, tr(de, "Grafik", "Graphics"), |ui| {
                    egui::Grid::new("export-gfx").num_columns(4).spacing(vec2(8.0, 5.0)).show(ui, |ui| {
                        ui.label(tr(de, "Vergrößerung:", "Zoom:"));
                        ui.add(egui::DragValue::new(&mut o.zoom).range(1.0..=6400.0).suffix(" %"));
                        ui.label(tr(de, "Horiz. Auflösung:", "Horiz. resolution:"));
                        if ui
                            .add(egui::DragValue::new(&mut o.dpi.0).range(9.0..=2400.0).suffix(" dpi"))
                            .changed()
                            && state.linked
                        {
                            o.dpi.1 = o.dpi.0;
                        }
                        ui.end_row();
                        ui.label(tr(de, "Hintergrund:", "Background:"));
                        let mut color =
                            egui::Color32::from_rgb(o.background[0], o.background[1], o.background[2]);
                        if ui.color_edit_button_srgba(&mut color).changed() {
                            o.background = [color.r(), color.g(), color.b()];
                        }
                        ui.label(tr(de, "Vert. Auflösung:", "Vert. resolution:"));
                        ui.horizontal(|ui| {
                            if ui
                                .add(egui::DragValue::new(&mut o.dpi.1).range(9.0..=2400.0).suffix(" dpi"))
                                .changed()
                                && state.linked
                            {
                                o.dpi.0 = o.dpi.1;
                            }
                            ui.checkbox(&mut state.linked, tr(de, "verbunden", "linked"));
                        });
                        ui.end_row();
                    });
                });
            });
            ui.add_space(8.0);
            ui.vertical(|ui| {
                ui.set_width(280.0);
                ui.label(RichText::new(o.format.label()).weak());
                if let Some(tab) = self.tabs.get(self.active) {
                    let page = state.range.resolve(n, cur).and_then(|p| p.first().copied()).unwrap_or(cur);
                    doc_preview(ui, &mut self.preview[0], &tab.doc, tab.id, page, vec2(280.0, 330.0));
                    if let Some(geo) = tab.doc.geometry(page) {
                        let (w, h) = o.pixel_size(geo.view_size());
                        ui.label(RichText::new(format!("{w} x {h} px")).small());
                    }
                }
            });
        });
        let pages = state.range.resolve(n, cur);
        ui.horizontal(|ui| {
            let mut name = std::mem::take(&mut state.preset_name);
            let _ = preset_combo(ui, de, "export-images", &mut state.options, &mut self.settings, &mut name);
            state.preset_name = name;
        });
        let (ok, cancel, _) = footer(ui, de, pages.is_some(), false, "OK");
        if cancel {
            return (Outcome::Close, None);
        }
        let (true, Some(pages)) = (ok, pages) else { return (Outcome::Keep, None) };
        let groups = crate::export_image::groups(
            &pages,
            &range_parts(&state.range, n),
            state.options.effective_mode(),
        );
        let (folder, options) = (state.folder.clone(), state.options.clone());
        (
            Outcome::Close,
            Some(Box::new(move |app, ctx| {
                let Some(tab) = app.tabs.get(app.active) else { return };
                let result = std::fs::create_dir_all(&folder)
                    .map_err(|e| crate::Error::io(&folder, e))
                    .and_then(|()| crate::export_image::export(&tab.doc, &groups, &folder, &options));
                match result {
                    Ok(files) => {
                        let de = app.ex_de();
                        let msg = if de { "Exportierte Bilder: " } else { "Exported images: " };
                        app.set_status(ctx, format!("{msg}{}", files.len()));
                        if options.open_folder
                            && let Some(first) = files.first()
                        {
                            super::features::reveal_in_file_manager(first);
                        }
                    }
                    Err(err) => app.show_error(&err),
                }
            })),
        )
    }

    /// "Einstellungen für Word-Umwandlung".
    pub(super) fn export_word_dialog(
        &mut self,
        ui: &mut Ui,
        range: &mut PageRange,
    ) -> (Outcome, Option<Action>) {
        let de = self.ex_de();
        let (n, cur) = self.active_tab().map_or((0, 0), |t| (t.doc.page_count(), t.current_page));
        ui.heading(tr(de, "Einstellungen für Word-Umwandlung", "Word conversion settings"));
        range.ui(ui, de, n, cur, &[RangeMode::All, RangeMode::Current, RangeMode::Custom]);
        section(ui, tr(de, "Einstellungen für Layout", "Layout settings"), |ui| {
            let mut flow = true;
            ui.horizontal(|ui| {
                let _ = ui
                    .add_enabled(
                        false,
                        egui::RadioButton::new(false, tr(de, "Seitenlayout erhalten", "Keep page layout")),
                    )
                    .on_disabled_hover_text(tr(
                        de,
                        "MrPDF überträgt den Text mit Überschriften, aber nicht die genaue Seitengestaltung.",
                        "MrPDF transfers the text with headings, but not the exact page layout.",
                    ));
                ui.radio_value(&mut flow, true, tr(de, "Textfluss erhalten", "Keep text flow"));
            });
        });
        ui.label(
            RichText::new(tr(
                de,
                "Überschriften werden an der Schriftgröße erkannt. Bilder, Tabellen und Kommentare werden nicht übernommen.",
                "Headings are recognised by their font size. Images, tables and comments are not transferred.",
            ))
            .small()
            .weak(),
        );
        let pages = range.resolve(n, cur);
        let (ok, cancel, _) = footer(ui, de, pages.is_some(), false, "OK");
        if cancel {
            return (Outcome::Close, None);
        }
        let (true, Some(pages)) = (ok, pages) else { return (Outcome::Keep, None) };
        (
            Outcome::Close,
            Some(Box::new(move |app, ctx| {
                let Some(tab) = app.tabs.get(app.active) else { return };
                let stem =
                    tab.doc.display_name().trim_end_matches(".pdf").trim_end_matches(".PDF").to_string();
                let Some(path) = rfd::FileDialog::new()
                    .set_title(Cmd::ExportWord.label(app.settings.language))
                    .add_filter("Word", &["docx"])
                    .set_file_name(format!("{stem}.docx"))
                    .save_file()
                else {
                    return;
                };
                let tab = &app.tabs[app.active];
                match tab.doc.export_word_pages(&path, &pages) {
                    Ok(()) => {
                        let msg = if app.ex_de() { "Exportiert: " } else { "Exported: " };
                        app.set_status(ctx, format!("{msg}{}", path.display()));
                    }
                    Err(err) => app.show_error(&err),
                }
            })),
        )
    }
}
