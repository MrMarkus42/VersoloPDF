//! Dialogs of step C: "Seiten zusammenführen"
//! (several pages per sheet), "Dokument aufteilen", "Seiten aufteilen" and
//! "Seiten überlagern".

use std::path::PathBuf;

use egui::{Color32, Context, RichText, Stroke, Ui, vec2};

use super::commands::Cmd;
use super::dialogs::{Action, Outcome};
use super::page_dialogs::{
    align_combo, expand_file_name, file_name, footer, macro_menu, margins_ui, pages_text, paper_combo, tr,
    unique_path,
};
use super::preview::{doc_preview, page_strip};
use super::{Dialog, MrPdfApp, PaperChoice};
use crate::document::ExtractOptions;
use crate::import::{Blend, NUpLayout, OverlayOptions, PageOrder, SheetSize};
use crate::pages::SplitMethod;
use crate::ui::i18n::Language;
use crate::ui::icons::Icon;
use crate::ui::pickers::{PageRange, RangeMode, length, parse_pages, section, segmented, unit_combo_right};
use crate::ui::widgets::icon_button;

/// "Seiten pro Blatt" presets (columns, rows).
const GRIDS: [(u32, u32); 7] = [(1, 2), (2, 2), (2, 3), (2, 4), (3, 3), (3, 4), (4, 4)];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitBy {
    Bookmarks,
    Every,
    Size,
}

/// One split line of "Seiten aufteilen": vertical lines cut the width.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SplitLine {
    pub vertical: bool,
    /// Position as fraction (0..1) from the left / top.
    pub at: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LayoutDialog {
    NUp {
        layout: NUpLayout,
        range: PageRange,
        reverse: bool,
        sheet: usize,
        sheets: String,
    },
    SplitDocument {
        by: SplitBy,
        level: u32,
        every: u32,
        mb: f32,
        pattern: String,
        folder: PathBuf,
        open_folder: bool,
        overwrite: bool,
        options: ExtractOptions,
        show_settings: bool,
    },
    SplitPages {
        lines: Vec<SplitLine>,
        selected: Option<usize>,
        from_top: bool,
        from_right: bool,
        rtl: bool,
        method: SplitMethod,
        range: PageRange,
        page: usize,
        dragging: Option<usize>,
    },
    Overlay {
        path: Option<PathBuf>,
        sources: PageRange,
        targets: PageRange,
        new_layer: bool,
        layer_name: String,
        opts: OverlayOptions,
        page: usize,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutState {
    pub dialog: LayoutDialog,
    pub preset_name: String,
}

/// Every dialog shows the target pages as "Seitenbereich" with these modes.
const ALL_CUSTOM: [RangeMode; 2] = [RangeMode::All, RangeMode::Custom];

impl MrPdfApp {
    fn ld_de(&self) -> bool {
        self.settings.language == Language::German
    }

    /// Opens a dialog of step C. Returns false if `cmd` has none.
    pub(super) fn run_layout_dialog(&mut self, cmd: Cmd) -> bool {
        let folder = self
            .active_tab()
            .and_then(|t| t.doc.path())
            .and_then(|p| p.parent())
            .map(std::path::Path::to_path_buf)
            .unwrap_or_default();
        let dialog = match cmd {
            Cmd::NUp => LayoutDialog::NUp {
                layout: NUpLayout::default(),
                range: PageRange::all(),
                reverse: false,
                sheet: 0,
                sheets: "1-".into(),
            },
            Cmd::SplitDocument => LayoutDialog::SplitDocument {
                by: SplitBy::Every,
                level: 1,
                every: 1,
                mb: 1.0,
                pattern: "%[FileName]_%[Page]".into(),
                folder,
                open_folder: true,
                overwrite: false,
                options: ExtractOptions::default(),
                show_settings: false,
            },
            Cmd::SplitPages => LayoutDialog::SplitPages {
                lines: Vec::new(),
                selected: None,
                from_top: false,
                from_right: false,
                rtl: false,
                method: SplitMethod::CropBox,
                range: PageRange::all(),
                page: self.active_tab().map_or(0, |t| t.current_page),
                dragging: None,
            },
            Cmd::Overlay => LayoutDialog::Overlay {
                path: None,
                sources: PageRange::all(),
                targets: PageRange::all(),
                new_layer: false,
                layer_name: if self.ld_de() { "Neue Ebene".into() } else { "New layer".into() },
                opts: OverlayOptions::default(),
                page: self.active_tab().map_or(0, |t| t.current_page),
            },
            _ => return false,
        };
        self.clear_previews();
        self.dialog_doc = None;
        self.overlay_source = None;
        self.dialog = Some(Dialog::Layout(Box::new(LayoutState { dialog, preset_name: String::new() })));
        true
    }

    pub(super) fn layout_dialog(
        &mut self,
        ui: &mut Ui,
        state: &mut LayoutState,
    ) -> (Outcome, Option<Action>) {
        let de = self.ld_de();
        let lang = self.settings.language;
        let unit = self.settings.unit;
        let (n, cur) = self.active_tab().map_or((0, 0), |t| (t.doc.page_count(), t.current_page));
        let mut outcome = Outcome::Keep;
        let mut action: Option<Action> = None;
        match &mut state.dialog {
            LayoutDialog::NUp { layout, range, reverse, sheet, sheets } => {
                ui.horizontal(|ui| {
                    super::dialogs::set_title(ui, Cmd::NUp.label(lang));
                    unit_combo_right(ui, de, &mut self.settings.unit);
                });
                let pages = range.resolve(n, cur).map(|mut p| {
                    if *reverse {
                        p.reverse();
                    }
                    p
                });
                let per = layout.per_sheet();
                let total_sheets = pages.as_ref().map_or(0, |p| p.len().div_ceil(per));
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(430.0);
                        section(ui, tr(de, "Seiten", "Pages"), |ui| {
                            egui::Grid::new("nup-sheet").num_columns(2).spacing(vec2(8.0, 6.0)).show(
                                ui,
                                |ui| {
                                    ui.label(tr(de, "Typ:", "Type:"));
                                    segmented(
                                        ui,
                                        &mut layout.sheet_mode,
                                        &[
                                            (SheetSize::Compute, tr(de, "Berechnen", "Compute").into()),
                                            (SheetSize::Standard, "Standard".into()),
                                            (SheetSize::Custom, tr(de, "Eigene", "Custom").into()),
                                        ],
                                    );
                                    ui.end_row();
                                    match layout.sheet_mode {
                                        SheetSize::Standard => {
                                            ui.label(tr(de, "Größe:", "Size:"));
                                            let mut paper = paper_of(layout.sheet);
                                            paper_combo(ui, de, "nup-paper", &mut paper);
                                            layout.sheet = paper.size().unwrap_or(layout.sheet);
                                            ui.end_row();
                                        }
                                        SheetSize::Custom => {
                                            ui.label(tr(de, "Größe:", "Size:"));
                                            ui.horizontal(|ui| {
                                                length(ui, &mut layout.sheet.0, unit, 36.0..=14_400.0);
                                                ui.label("x");
                                                length(ui, &mut layout.sheet.1, unit, 36.0..=14_400.0);
                                            });
                                            ui.end_row();
                                        }
                                        SheetSize::Compute => {}
                                    }
                                    if layout.sheet_mode != SheetSize::Compute {
                                        ui.label(tr(de, "Ausrichtung:", "Orientation:"));
                                        segmented(
                                            ui,
                                            &mut layout.landscape,
                                            &[
                                                (false, tr(de, "Hochformat", "Portrait").into()),
                                                (true, tr(de, "Querformat", "Landscape").into()),
                                            ],
                                        );
                                        ui.end_row();
                                    }
                                },
                            );
                        });
                        margins_ui(ui, de, &mut layout.margins, unit);
                        range.ui(ui, de, n, cur, &crate::ui::pickers::PageRange::ALL_MODES);
                        ui.checkbox(reverse, tr(de, "Reihenfolge umkehren", "Reverse order"));
                        section(ui, tr(de, "Position und Skalierung", "Position and scaling"), |ui| {
                            egui::Grid::new("nup-pos").num_columns(4).spacing(vec2(8.0, 6.0)).show(
                                ui,
                                |ui| {
                                    ui.label(tr(de, "Typ:", "Type:"));
                                    egui::ComboBox::from_id_salt("nup-type")
                                        .selected_text(tr(
                                            de,
                                            "Mehrere Seiten pro Blatt",
                                            "Several pages per sheet",
                                        ))
                                        .show_ui(ui, |ui| {
                                            let _ = ui.selectable_label(
                                                true,
                                                tr(de, "Mehrere Seiten pro Blatt", "Several pages per sheet"),
                                            );
                                        });
                                    ui.label("");
                                    ui.label("");
                                    ui.end_row();
                                    ui.label(tr(de, "Seiten pro Blatt:", "Pages per sheet:"));
                                    let label = |c: u32, r: u32| format!("{} ({c}/{r})", c * r);
                                    let current =
                                        GRIDS.iter().find(|g| **g == (layout.columns, layout.rows)).copied();
                                    egui::ComboBox::from_id_salt("nup-grid")
                                        .selected_text(
                                            current
                                                .map_or(tr(de, "Eigene", "Custom").to_string(), |(c, r)| {
                                                    label(c, r)
                                                }),
                                        )
                                        .show_ui(ui, |ui| {
                                            for (c, r) in GRIDS {
                                                if ui
                                                    .selectable_label(current == Some((c, r)), label(c, r))
                                                    .clicked()
                                                {
                                                    layout.columns = c;
                                                    layout.rows = r;
                                                }
                                            }
                                        });
                                    ui.horizontal(|ui| {
                                        ui.add(egui::DragValue::new(&mut layout.columns).range(1..=16));
                                        ui.label(tr(de, "auf", "by"));
                                        ui.add(egui::DragValue::new(&mut layout.rows).range(1..=16));
                                    });
                                    ui.label("");
                                    ui.end_row();
                                    ui.label(tr(de, "Reihenfolge:", "Order:"));
                                    egui::ComboBox::from_id_salt("nup-order")
                                        .selected_text(match layout.order {
                                            PageOrder::Horizontal => tr(de, "Horizontal", "Horizontal"),
                                            PageOrder::Vertical => tr(de, "Vertikal", "Vertical"),
                                        })
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(
                                                &mut layout.order,
                                                PageOrder::Horizontal,
                                                tr(de, "Horizontal", "Horizontal"),
                                            );
                                            ui.selectable_value(
                                                &mut layout.order,
                                                PageOrder::Vertical,
                                                tr(de, "Vertikal", "Vertical"),
                                            );
                                        });
                                    ui.checkbox(&mut layout.reverse_order, tr(de, "Umkehren", "Reverse"));
                                    ui.label("");
                                    ui.end_row();
                                    ui.label(tr(de, "Horiz. Ausrichtung:", "Horiz. alignment:"));
                                    align_combo(ui, de, "nup-h", &mut layout.h_align, false);
                                    ui.label(tr(de, "Vert. Ausrichtung:", "Vert. alignment:"));
                                    align_combo(ui, de, "nup-v", &mut layout.v_align, true);
                                    ui.end_row();
                                    ui.label(tr(de, "Horiz. Abstand:", "Horiz. gap:"));
                                    length(ui, &mut layout.h_gap, unit, 0.0..=500.0);
                                    ui.label(tr(de, "Vert. Abstand:", "Vert. gap:"));
                                    length(ui, &mut layout.v_gap, unit, 0.0..=500.0);
                                    ui.end_row();
                                },
                            );
                            ui.checkbox(
                                &mut layout.auto_rotate,
                                tr(
                                    de,
                                    "Seiten in Zelle automatisch drehen",
                                    "Rotate pages in cells automatically",
                                ),
                            );
                            ui.checkbox(
                                &mut layout.border,
                                tr(de, "Seitenrand drucken", "Print page border"),
                            );
                            ui.checkbox(
                                &mut layout.no_enlarge,
                                tr(de, "Seiten nicht vergrößern", "Do not enlarge pages"),
                            );
                        });
                    });
                    ui.vertical(|ui| {
                        ui.set_width(380.0);
                        let page_size = pages
                            .as_ref()
                            .and_then(|p| p.first())
                            .and_then(|p| self.active_tab().and_then(|t| t.doc.geometry(*p)))
                            .map_or((595.0, 842.0), |g| g.view_size());
                        let (sw, sh) = layout.sheet_size(page_size);
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!(
                                    "{}: {} x {}",
                                    tr(de, "Papier", "Paper"),
                                    unit.format(sw),
                                    unit.format(sh)
                                ))
                                .strong(),
                            );
                        });
                        *sheet = (*sheet).min(total_sheets.saturating_sub(1));
                        // Preview of the current sheet only (rendered on demand).
                        if let Some(pages) = &pages {
                            let chunk: Vec<usize> =
                                pages.chunks(per).nth(*sheet).map(|c| c.to_vec()).unwrap_or_default();
                            let key = format!("nup {chunk:?} {layout:?}");
                            if !chunk.is_empty() && self.dialog_doc.as_ref().is_none_or(|(k, _)| *k != key) {
                                let doc = self.tab().map(|t| t.doc.nup_document(&chunk, layout));
                                match doc {
                                    Some(Ok(d)) => self.dialog_doc = Some((key, d)),
                                    Some(Err(e)) => log::warn!("n-up preview: {e}"),
                                    None => {}
                                }
                            }
                            if let Some((_, d)) = &self.dialog_doc {
                                doc_preview(ui, &mut self.preview[0], d, 0x40b, 0, vec2(380.0, 420.0));
                            }
                            let first = chunk.first().map_or(0, |p| p + 1);
                            let last = chunk.last().map_or(0, |p| p + 1);
                            ui.label(format!(
                                "{}: {}   {}: {first}-{last}",
                                tr(de, "Blatt", "Sheet"),
                                *sheet + 1,
                                tr(de, "Seiten", "Pages")
                            ));
                        }
                        page_strip(ui, sheet, total_sheets);
                        ui.horizontal(|ui| {
                            ui.label(tr(de, "Blätter:", "Sheets:"));
                            ui.add(egui::TextEdit::singleline(sheets).desired_width(80.0));
                            let chosen = parse_pages(sheets, total_sheets).map_or(0, |s| s.len());
                            ui.label(if de {
                                format!("({chosen} ausgewählt, {total_sheets} insg.)")
                            } else {
                                format!("({chosen} selected, {total_sheets} total)")
                            });
                        });
                    });
                });
                let selected_sheets = parse_pages(sheets, total_sheets);
                let (ok, cancel, _) =
                    footer(ui, de, pages.is_some() && selected_sheets.is_some(), false, "OK");
                if ok && let (Some(pages), Some(sel)) = (pages, selected_sheets) {
                    let chosen: Vec<usize> = pages
                        .chunks(per)
                        .enumerate()
                        .filter(|(i, _)| sel.contains(i))
                        .flat_map(|(_, c)| c.to_vec())
                        .collect();
                    let layout = layout.clone();
                    action = Some(Box::new(move |app, ctx| {
                        let result = app.tab().map(|t| t.doc.nup_document(&chosen, &layout));
                        match result {
                            Some(Ok(doc)) => {
                                app.add_tab(doc);
                                ctx.request_repaint();
                            }
                            Some(Err(err)) => app.show_error(&err),
                            None => {}
                        }
                    }));
                    outcome = Outcome::Close;
                } else if cancel {
                    outcome = Outcome::Close;
                }
            }
            LayoutDialog::SplitDocument {
                by,
                level,
                every,
                mb,
                pattern,
                folder,
                open_folder,
                overwrite,
                options,
                show_settings,
            } => {
                super::dialogs::set_title(ui, Cmd::SplitDocument.label(lang));
                let has_bookmarks = self.active_tab().is_some_and(|t| !t.doc.bookmarks().is_empty());
                section(ui, tr(de, "Methode zum Aufteilen", "Split method"), |ui| {
                    ui.add_enabled_ui(has_bookmarks, |ui| {
                        ui.horizontal(|ui| {
                            ui.radio_value(
                                by,
                                SplitBy::Bookmarks,
                                tr(de, "Nach Lesezeichen der tiefsten Ebene:", "By bookmarks down to level:"),
                            );
                            ui.add(egui::DragValue::new(level).range(1..=10));
                        });
                    });
                    ui.horizontal(|ui| {
                        ui.radio_value(by, SplitBy::Every, tr(de, "Alle", "Every"));
                        ui.add(egui::DragValue::new(every).range(1..=10_000));
                        ui.label(tr(de, "Seite(n)", "page(s)"));
                    });
                    ui.horizontal(|ui| {
                        ui.radio_value(by, SplitBy::Size, tr(de, "Nach Größe:", "By size:"));
                        ui.add(egui::DragValue::new(mb).range(0.1..=4096.0).speed(0.1).suffix(" MB"));
                    });
                });
                section(ui, tr(de, "Ausgabe", "Output"), |ui| {
                    ui.label(tr(de, "Dateiname:", "File name:"));
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(pattern).desired_width(320.0));
                        macro_menu(
                            ui,
                            de,
                            pattern,
                            &[
                                ("%[FileName]", "Dateiname", "File name"),
                                ("%[Page]", "Erste Seite des Teils", "First page of the part"),
                                ("%[Pages]", "Seitenbereich", "Page range"),
                                ("%[Part]", "Nummer des Teils", "Part number"),
                                ("%[Bookmark]", "Lesezeichen", "Bookmark"),
                                ("%[Date]", "Datum", "Date"),
                            ],
                        );
                    });
                    ui.label(tr(de, "Zielordner:", "Target folder:"));
                    ui.horizontal(|ui| {
                        let mut text = folder.display().to_string();
                        if ui.add(egui::TextEdit::singleline(&mut text).desired_width(320.0)).changed() {
                            *folder = PathBuf::from(text);
                        }
                        if icon_button(
                            ui,
                            Icon::Open,
                            tr(de, "Ordner auswählen", "Choose folder"),
                            false,
                            true,
                        )
                        .clicked()
                            && let Some(dir) = rfd::FileDialog::new().pick_folder()
                        {
                            *folder = dir;
                        }
                    });
                    ui.checkbox(
                        open_folder,
                        tr(
                            de,
                            "Öffne Zielordner und wähle Dateien aus",
                            "Open target folder and select files",
                        ),
                    );
                    ui.checkbox(
                        overwrite,
                        tr(de, "Existierende Dateien überschreiben", "Overwrite existing files"),
                    );
                });
                let folder_ok = folder.is_dir();
                if !folder_ok {
                    ui.colored_label(
                        Color32::from_rgb(200, 40, 40),
                        tr(
                            de,
                            "Bitte einen vorhandenen Zielordner wählen.",
                            "Please choose an existing target folder.",
                        ),
                    );
                }
                if *show_settings {
                    section(ui, tr(de, "Einstellungen", "Settings"), |ui| {
                        super::page_dialogs::extract_options_ui(ui, de, options)
                    });
                }
                let (ok, cancel, settings) = footer(ui, de, folder_ok && n > 0, true, "OK");
                if settings {
                    *show_settings = !*show_settings;
                }
                if ok {
                    let (by, level, every, mb) = (*by, *level as usize, *every as usize, *mb);
                    let (pattern, folder, open_folder, overwrite, options) =
                        (pattern.clone(), folder.clone(), *open_folder, *overwrite, *options);
                    action = Some(Box::new(move |app, ctx| {
                        app.split_document_parts(
                            ctx,
                            by,
                            level,
                            every,
                            mb,
                            &pattern,
                            &folder,
                            open_folder,
                            overwrite,
                            &options,
                        )
                    }));
                    outcome = Outcome::Close;
                } else if cancel {
                    outcome = Outcome::Close;
                }
            }
            LayoutDialog::SplitPages {
                lines,
                selected,
                from_top,
                from_right,
                rtl,
                method,
                range,
                page,
                dragging,
            } => {
                super::dialogs::set_title(ui, Cmd::SplitPages.label(lang));
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(390.0);
                        ui.checkbox(from_top, tr(de, "Werte von Oben berechnen", "Measure from the top"));
                        ui.checkbox(
                            from_right,
                            tr(de, "Werte von Rechts berechnen", "Measure from the right"),
                        );
                        ui.horizontal(|ui| {
                            if ui.button(tr(de, "Senkrecht teilen", "Split vertically")).clicked() {
                                lines.push(SplitLine { vertical: true, at: 0.5 });
                                *selected = Some(lines.len() - 1);
                            }
                            if ui.button(tr(de, "Waagerecht teilen", "Split horizontally")).clicked() {
                                lines.push(SplitLine { vertical: false, at: 0.5 });
                                *selected = Some(lines.len() - 1);
                            }
                            if icon_button(
                                ui,
                                Icon::Delete,
                                tr(de, "Trennlinie löschen", "Delete split line"),
                                false,
                                selected.is_some(),
                            )
                            .clicked()
                                && let Some(i) = selected.take()
                                && i < lines.len()
                            {
                                lines.remove(i);
                            }
                            if icon_button(
                                ui,
                                Icon::Eraser,
                                tr(de, "Alle Trennlinien löschen", "Delete all split lines"),
                                false,
                                !lines.is_empty(),
                            )
                            .clicked()
                            {
                                lines.clear();
                                *selected = None;
                            }
                        });
                        egui::Frame::new().stroke(ui.visuals().widgets.noninteractive.bg_stroke).show(
                            ui,
                            |ui| {
                                ui.set_min_size(vec2(ui.available_width(), 140.0));
                                if lines.is_empty() {
                                    ui.add_space(50.0);
                                    ui.vertical_centered(|ui| {
                                        ui.label(RichText::new(tr(de, "Keine Aufteilung", "No split")).weak())
                                    });
                                }
                                for (i, line) in lines.iter_mut().enumerate() {
                                    ui.horizontal(|ui| {
                                        let name = if line.vertical {
                                            tr(de, "Senkrecht", "Vertical")
                                        } else {
                                            tr(de, "Waagerecht", "Horizontal")
                                        };
                                        if ui.selectable_label(*selected == Some(i), name).clicked() {
                                            *selected = Some(i);
                                        }
                                        // Shown from the chosen edge, stored from left/top.
                                        let flip = if line.vertical { *from_right } else { !*from_top };
                                        let mut pct =
                                            if flip { 100.0 - line.at * 100.0 } else { line.at * 100.0 };
                                        if ui
                                            .add(
                                                egui::DragValue::new(&mut pct)
                                                    .range(1.0..=99.0)
                                                    .speed(0.5)
                                                    .suffix(" %"),
                                            )
                                            .changed()
                                        {
                                            line.at = if flip { 1.0 - pct / 100.0 } else { pct / 100.0 };
                                        }
                                    });
                                }
                            },
                        );
                        section(ui, tr(de, "Einstellungen", "Settings"), |ui| {
                            ui.checkbox(
                                rtl,
                                tr(de, "Von Rechts nach Links aufteilen", "Split right to left"),
                            );
                            ui.label(tr(de, "Methode zum Aufteilen:", "Split method:"));
                            egui::ComboBox::from_id_salt("split-method")
                                .selected_text(match method {
                                    SplitMethod::CropBox => tr(
                                        de,
                                        "Den sichtbaren Bereich (Crop-Box) der Seite(n) festlegen",
                                        "Set the visible area (crop box)",
                                    ),
                                    SplitMethod::MediaBox => tr(
                                        de,
                                        "Die Seitengröße (Media-Box) festlegen",
                                        "Set the page size (media box)",
                                    ),
                                })
                                .width(370.0)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        method,
                                        SplitMethod::CropBox,
                                        tr(
                                            de,
                                            "Den sichtbaren Bereich (Crop-Box) der Seite(n) festlegen",
                                            "Set the visible area (crop box)",
                                        ),
                                    );
                                    ui.selectable_value(
                                        method,
                                        SplitMethod::MediaBox,
                                        tr(
                                            de,
                                            "Die Seitengröße (Media-Box) festlegen",
                                            "Set the page size (media box)",
                                        ),
                                    );
                                });
                            let mut keep = *method == SplitMethod::CropBox;
                            if ui
                                .checkbox(
                                    &mut keep,
                                    tr(de, "Originalinhalt erhalten", "Keep original content"),
                                )
                                .changed()
                            {
                                *method = if keep { SplitMethod::CropBox } else { SplitMethod::MediaBox };
                            }
                        });
                        range.ui(ui, de, n, cur, &crate::ui::pickers::PageRange::ALL_MODES);
                    });
                    ui.vertical(|ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!(
                                    "{} {}",
                                    tr(de, "Vorschau von Seite:", "Preview of page:"),
                                    *page + 1
                                ))
                                .weak(),
                            )
                            .wrap_mode(egui::TextWrapMode::Extend),
                        );
                        *page = (*page).min(n.saturating_sub(1));
                        if let Some(area) = self.page_preview(ui, 0, *page, vec2(380.0, 440.0)) {
                            let rect = area.rect;
                            let painter = ui.painter();
                            for (i, line) in lines.iter().enumerate() {
                                let color = if *selected == Some(i) {
                                    Color32::from_rgb(230, 60, 40)
                                } else {
                                    Color32::from_rgb(40, 110, 220)
                                };
                                let (a, b) = if line.vertical {
                                    let x = rect.left() + line.at * rect.width();
                                    (egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom()))
                                } else {
                                    let y = rect.top() + line.at * rect.height();
                                    (egui::pos2(rect.left(), y), egui::pos2(rect.right(), y))
                                };
                                painter.line_segment([a, b], Stroke::new(2.0, color));
                            }
                            // Lines can be dragged in the preview.
                            let r = &area.response;
                            if r.drag_started()
                                && let Some(pos) = r.interact_pointer_pos()
                            {
                                *dragging = lines.iter().position(|l| {
                                    if l.vertical {
                                        (rect.left() + l.at * rect.width() - pos.x).abs() < 6.0
                                    } else {
                                        (rect.top() + l.at * rect.height() - pos.y).abs() < 6.0
                                    }
                                });
                                if dragging.is_some() {
                                    *selected = *dragging;
                                }
                            }
                            if let (Some(i), Some(pos)) = (*dragging, r.interact_pointer_pos())
                                && r.dragged()
                                && let Some(l) = lines.get_mut(i)
                            {
                                l.at = if l.vertical {
                                    (pos.x - rect.left()) / rect.width()
                                } else {
                                    (pos.y - rect.top()) / rect.height()
                                }
                                .clamp(0.01, 0.99);
                            }
                            if r.drag_stopped() {
                                *dragging = None;
                            }
                        }
                        page_strip(ui, page, n);
                    });
                });
                let sel = range.resolve(n, cur);
                let (ok, cancel, _) = footer(ui, de, sel.is_some() && !lines.is_empty(), false, "OK");
                if ok && let Some(pages) = sel {
                    let xs: Vec<f32> = lines.iter().filter(|l| l.vertical).map(|l| l.at).collect();
                    let ys: Vec<f32> = lines.iter().filter(|l| !l.vertical).map(|l| l.at).collect();
                    let (rtl, method) = (*rtl, *method);
                    action = Some(Box::new(move |app, ctx| {
                        app.edit(ctx, |tab| tab.doc.split_pages_at(&pages, &xs, &ys, rtl, method))
                    }));
                    outcome = Outcome::Close;
                } else if cancel {
                    outcome = Outcome::Close;
                }
            }
            LayoutDialog::Overlay { path, sources, targets, new_layer, layer_name, opts, page } => {
                super::dialogs::set_title(ui, tr(de, "Seiten überlagern", "Overlay pages"));
                // Load the chosen source once.
                if let Some(p) = path.clone()
                    && self.overlay_source.as_ref().is_none_or(|(sp, _)| *sp != p)
                {
                    match crate::document::read_pdf_file(&p)
                        .and_then(|b| crate::import::Source::load(&b, None))
                    {
                        Ok(src) => self.overlay_source = Some((p, src)),
                        Err(err) => {
                            self.show_error(&err);
                            *path = None;
                        }
                    }
                }
                let source_pages = self.overlay_source.as_ref().map_or(0, |(_, s)| s.page_count());
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(430.0);
                        section(ui, tr(de, "Quelle und Seitenbereich", "Source and page range"), |ui| {
                            ui.horizontal(|ui| {
                                ui.label(tr(de, "Datei:", "File:"));
                                let shown = path.as_deref().map(file_name).unwrap_or_else(|| {
                                    tr(
                                        de,
                                        "Klicken Sie hier, um die Datei auszuwählen…",
                                        "Click here to choose the file…",
                                    )
                                    .into()
                                });
                                let clicked = ui.button(shown).clicked()
                                    | icon_button(
                                        ui,
                                        Icon::Open,
                                        tr(de, "Datei auswählen", "Choose file"),
                                        false,
                                        true,
                                    )
                                    .clicked();
                                if clicked
                                    && let Some(p) =
                                        rfd::FileDialog::new().add_filter("PDF", &["pdf", "PDF"]).pick_file()
                                {
                                    *path = Some(p);
                                }
                            });
                            ui.add_enabled_ui(path.is_some(), |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(tr(de, "Seiten", "Pages"));
                                    segmented(
                                        ui,
                                        &mut sources.mode,
                                        &[
                                            (RangeMode::All, tr(de, "Alle", "All").into()),
                                            (RangeMode::Custom, tr(de, "Bereich", "Range").into()),
                                        ],
                                    );
                                    if sources.mode == RangeMode::Custom {
                                        ui.add(
                                            egui::TextEdit::singleline(&mut sources.text).desired_width(90.0),
                                        );
                                        ui.label(format!("/{source_pages}"));
                                    }
                                });
                            });
                        });
                        section(ui, tr(de, "Einstellungen", "Settings"), |ui| {
                            ui.radio_value(
                                new_layer,
                                false,
                                tr(de, "Keine neue Ebene erstellen", "Do not create a new layer"),
                            );
                            ui.horizontal(|ui| {
                                ui.radio_value(
                                    new_layer,
                                    true,
                                    tr(de, "Eine neue Ebene erstellen:", "Create a new layer:"),
                                );
                                ui.add_enabled(
                                    *new_layer,
                                    egui::TextEdit::singleline(layer_name).desired_width(150.0),
                                );
                            });
                            ui.add_enabled_ui(false, |ui| {
                                let mut off = false;
                                ui.radio_value(&mut off, true, tr(de, "Zu Ebene hinzufügen", "Add to layer"));
                                ui.radio_value(
                                    &mut off,
                                    true,
                                    tr(
                                        de,
                                        "Entsprechende Ebenen aus der Quelle kopieren",
                                        "Copy the corresponding layers of the source",
                                    ),
                                );
                            });
                        });
                        targets.ui_titled(
                            ui,
                            de,
                            n,
                            cur,
                            &ALL_CUSTOM,
                            tr(de, "Seitenbereich im Ziel", "Page range in the target"),
                        );
                        ui.horizontal(|ui| {
                            ui.label(tr(de, "Quelle wiederholen:", "Repeat source:"));
                            egui::ComboBox::from_id_salt("ov-repeat")
                                .selected_text(if opts.repeat {
                                    tr(de, "Ja", "Yes")
                                } else {
                                    tr(de, "Nein", "No")
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut opts.repeat, false, tr(de, "Nein", "No"));
                                    ui.selectable_value(&mut opts.repeat, true, tr(de, "Ja", "Yes"));
                                });
                        });
                        section(ui, tr(de, "Aussehen", "Appearance"), |ui| {
                            egui::Grid::new("ov-look").num_columns(4).spacing(vec2(8.0, 6.0)).show(
                                ui,
                                |ui| {
                                    ui.label(tr(de, "Drehen:", "Rotate:"));
                                    ui.add(
                                        egui::DragValue::new(&mut opts.rotation)
                                            .range(-360.0..=360.0)
                                            .suffix("°"),
                                    );
                                    ui.label(tr(de, "Deckkraft:", "Opacity:"));
                                    let mut pct = opts.opacity * 100.0;
                                    if ui
                                        .add(egui::DragValue::new(&mut pct).range(0.0..=100.0).suffix(" %"))
                                        .changed()
                                    {
                                        opts.opacity = pct / 100.0;
                                    }
                                    ui.end_row();
                                    ui.label(tr(de, "Skalieren:", "Scale:"));
                                    let mut pct = opts.scale * 100.0;
                                    if ui
                                        .add(egui::DragValue::new(&mut pct).range(1.0..=1000.0).suffix(" %"))
                                        .changed()
                                    {
                                        opts.scale = pct / 100.0;
                                    }
                                    ui.checkbox(
                                        &mut opts.relative,
                                        tr(de, "Relativ zur Seite", "Relative to page"),
                                    );
                                    ui.label("");
                                    ui.end_row();
                                    ui.label(tr(de, "Mischen:", "Blend:"));
                                    egui::ComboBox::from_id_salt("ov-blend")
                                        .selected_text(blend_name(opts.blend, de))
                                        .show_ui(ui, |ui| {
                                            for b in Blend::ALL {
                                                ui.selectable_value(&mut opts.blend, b, blend_name(b, de));
                                            }
                                        });
                                    ui.end_row();
                                },
                            );
                        });
                        section(ui, tr(de, "Einfügen", "Placement"), |ui| {
                            egui::Grid::new("ov-place").num_columns(4).spacing(vec2(8.0, 6.0)).show(
                                ui,
                                |ui| {
                                    ui.label(tr(de, "Horiz. verschieben:", "Move horizontally:"));
                                    length(ui, &mut opts.dx, unit, -5000.0..=5000.0);
                                    ui.label(tr(de, "von:", "from:"));
                                    align_combo(ui, de, "ov-h", &mut opts.h_from, false);
                                    ui.end_row();
                                    ui.label(tr(de, "Vert. verschieben:", "Move vertically:"));
                                    length(ui, &mut opts.dy, unit, -5000.0..=5000.0);
                                    ui.label(tr(de, "von:", "from:"));
                                    align_combo(ui, de, "ov-v", &mut opts.v_from, true);
                                    ui.end_row();
                                },
                            );
                            ui.checkbox(&mut opts.behind, tr(de, "Im Hintergrund", "In the background"));
                        });
                    });
                    ui.vertical(|ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!(
                                    "{} {}",
                                    tr(de, "Vorschau von Seite:", "Preview of page:"),
                                    *page + 1
                                ))
                                .weak(),
                            )
                            .wrap_mode(egui::TextWrapMode::Extend),
                        );
                        *page = (*page).min(n.saturating_sub(1));
                        let first_source = sources.resolve(source_pages, 0).and_then(|s| s.first().copied());
                        let mut shown = false;
                        if let (Some((_, src)), Some(sp)) = (&self.overlay_source, first_source) {
                            let index =
                                targets.resolve(n, cur).and_then(|t| t.iter().position(|p| p == page));
                            let source_page = match (index, sources.resolve(source_pages, 0)) {
                                (Some(i), Some(s)) if opts.repeat => s[i % s.len()],
                                (Some(i), Some(s)) => s.get(i).copied().unwrap_or(sp),
                                _ => sp,
                            };
                            let key = format!("ov {page} {source_page} {opts:?}");
                            if self.dialog_doc.as_ref().is_none_or(|(k, _)| *k != key) {
                                let doc = self
                                    .active_tab()
                                    .map(|t| t.doc.overlay_preview(*page, src, source_page, opts));
                                if let Some(Ok(d)) = doc {
                                    self.dialog_doc = Some((key, d));
                                }
                            }
                            if let Some((_, d)) = &self.dialog_doc {
                                doc_preview(ui, &mut self.preview[0], d, 0x0e1, 0, vec2(380.0, 470.0));
                                shown = true;
                            }
                        }
                        if !shown {
                            self.page_preview(ui, 0, *page, vec2(380.0, 470.0));
                        }
                        page_strip(ui, page, n);
                    });
                });
                let t_sel = targets.resolve(n, cur);
                let s_sel = sources.resolve(source_pages, 0);
                let (ok, cancel, _) =
                    footer(ui, de, path.is_some() && t_sel.is_some() && s_sel.is_some(), false, "OK");
                if ok && let (Some(t), Some(s), Some(p)) = (t_sel, s_sel, path.clone()) {
                    let mut o = opts.clone();
                    o.layer = new_layer.then(|| layer_name.clone());
                    action = Some(Box::new(move |app, ctx| {
                        app.edit(ctx, |tab| tab.doc.overlay_from(&t, &p, &s, &o).map(|_| ()))
                    }));
                    outcome = Outcome::Close;
                } else if cancel {
                    outcome = Outcome::Close;
                }
            }
        }
        if matches!(outcome, Outcome::Close) {
            self.clear_previews();
            self.dialog_doc = None;
            self.overlay_source = None;
        }
        (outcome, action)
    }

    #[allow(clippy::too_many_arguments)]
    fn split_document_parts(
        &mut self,
        ctx: &Context,
        by: SplitBy,
        level: usize,
        every: usize,
        mb: f32,
        pattern: &str,
        folder: &std::path::Path,
        open_folder: bool,
        overwrite: bool,
        options: &ExtractOptions,
    ) {
        let de = self.ld_de();
        let Some(tab) = self.active_tab() else { return };
        let stem = tab.doc.display_name().trim_end_matches(".pdf").trim_end_matches(".PDF").to_string();
        let date = super::structure::today(false);
        let result = (|| -> crate::Result<Vec<PathBuf>> {
            let parts: Vec<(Vec<usize>, String)> = match by {
                SplitBy::Every => {
                    tab.doc.parts_every(every).into_iter().map(|p| (p, String::new())).collect()
                }
                SplitBy::Size => tab
                    .doc
                    .parts_by_size((mb * 1024.0 * 1024.0) as usize)?
                    .into_iter()
                    .map(|p| (p, String::new()))
                    .collect(),
                SplitBy::Bookmarks => tab.doc.parts_by_bookmarks(level),
            };
            let mut files = Vec::new();
            for (i, (pages, title)) in parts.iter().enumerate() {
                let bytes = tab.doc.extract_pages_with(pages, options)?;
                let pattern = pattern.replace("%[Part]", &(i + 1).to_string()).replace("%[Bookmark]", title);
                let name = expand_file_name(&pattern, &stem, pages[0] + 1, &pages_text(pages), &date);
                let path = if overwrite { folder.join(&name) } else { unique_path(folder, &name) };
                crate::document::write_atomic(&path, &bytes)?;
                files.push(path);
            }
            Ok(files)
        })();
        match result {
            Ok(files) => {
                if open_folder && let Some(first) = files.first() {
                    super::features::reveal_in_file_manager(first);
                }
                self.set_status(
                    ctx,
                    if de {
                        format!("{} Dateien geschrieben", files.len())
                    } else {
                        format!("{} files written", files.len())
                    },
                );
            }
            Err(err) => self.show_error(&err),
        }
    }
}

fn paper_of(size: (f32, f32)) -> PaperChoice {
    [PaperChoice::A4, PaperChoice::A3, PaperChoice::A5, PaperChoice::Letter, PaperChoice::Legal]
        .into_iter()
        .find(|p| p.size().is_some_and(|s| (s.0 - size.0).abs() < 1.0 && (s.1 - size.1).abs() < 1.0))
        .unwrap_or(PaperChoice::A4)
}

fn blend_name(b: Blend, de: bool) -> &'static str {
    match b {
        Blend::Normal => "Normal",
        Blend::Multiply => tr(de, "Multiplizieren", "Multiply"),
        Blend::Screen => tr(de, "Negativ multiplizieren", "Screen"),
        Blend::Darken => tr(de, "Abdunkeln", "Darken"),
        Blend::Lighten => tr(de, "Aufhellen", "Lighten"),
        Blend::Overlay => tr(de, "Ineinanderkopieren", "Overlay"),
        Blend::Difference => tr(de, "Differenz", "Difference"),
    }
}
