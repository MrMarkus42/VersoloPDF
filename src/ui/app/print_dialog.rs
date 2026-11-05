//! The print dialog "Drucken" / "Alle drucken":
//! printer, page range, position and scaling,
//! printed content, live preview of the sheets.

use egui::{Context, RichText, Ui, vec2};

use super::dialogs::{Action, Outcome};
use super::page_dialogs::{PAPERS, paper_size, tr};
use super::preview::{doc_preview, page_strip};
use super::{Dialog, VersoloPdfApp};
use crate::Document;
use crate::import::PageOrder;
use crate::print::PrinterInfo;
use crate::print_layout::{Binding, CutPage, Duplex, Paper, PrintContent, PrintSettings, Scaling};
use crate::ui::pickers::{PageRange, RangeMode, Subset, parse_pages, preset_combo, section, segmented};

/// State of the print dialog.
#[derive(Debug, Clone, PartialEq)]
pub struct PrintState {
    pub settings: PrintSettings,
    pub printers: Vec<PrinterInfo>,
    pub printer: String,
    /// Driver settings of the printer (Windows DEVMODE).
    pub devmode: Option<Vec<u8>>,
    pub paper: Paper,
    pub range: PageRange,
    /// "Drucke Blätter"
    pub sheets: String,
    pub sheet_subset: Subset,
    pub sheets_reverse: bool,
    pub preview_sheet: usize,
    /// "Alle drucken": every open document.
    pub all_documents: bool,
    pub preset_name: String,
    pub error: Option<String>,
    /// Settings the shown sheets were made for.
    pub job_key: String,
    /// Pages cut because they reach beyond the printable area (page, cut
    /// width, cut height in points).
    pub cut: Vec<CutPage>,
}

fn paper_of(settings: &PrintSettings, printer: &str, devmode: Option<&[u8]>) -> Paper {
    let (w, h) = paper_size(&settings.paper).unwrap_or((595.28, 841.89));
    crate::print::printer_paper(printer, devmode, Paper::full(w, h))
}

fn paper_name(paper: &Paper) -> String {
    let mm = 25.4 / 72.0;
    let (w, h) = (paper.size.0 * mm, paper.size.1 * mm);
    let known = PAPERS.iter().find(|p| {
        ((p.2 - w).abs() < 1.5 && (p.3 - h).abs() < 1.5) || ((p.3 - w).abs() < 1.5 && (p.2 - h).abs() < 1.5)
    });
    match known {
        Some(p) => format!("{} {:.0} x {:.0} mm", p.1, w, h),
        None => format!("{w:.0} x {h:.0} mm"),
    }
}

/// Sheets chosen under "Drucke Blätter" (0-based).
/// Warning about pages that do not fit the paper; true if the user chose to
/// shrink them.
fn cut_warning(ui: &mut Ui, de: bool, cut: &[CutPage], paper: &Paper) -> bool {
    let mm = 25.4 / 72.0;
    let (w, h) = cut.iter().fold((0.0f32, 0.0f32), |(a, b), c| (a.max(c.1), b.max(c.2)));
    let pages: Vec<String> = cut.iter().take(8).map(|c| (c.0 + 1).to_string()).collect();
    let more = if cut.len() > 8 { " …" } else { "" };
    let lost = match (w * mm >= 1.0, h * mm >= 1.0) {
        (true, true) => {
            format!("{:.0} mm {} {:.0} mm", w * mm, tr(de, "in der Breite und", "in width and"), h * mm)
        }
        (true, false) => format!("{:.0} mm {}", w * mm, tr(de, "in der Breite", "in width")),
        _ => format!("{:.0} mm {}", h * mm, tr(de, "in der Höhe", "in height")),
    };
    let text = format!(
        "{} {}{more} {} ({}). {} {lost}.",
        if cut.len() == 1 { tr(de, "Seite", "Page") } else { tr(de, "Seiten", "Pages") },
        pages.join(", "),
        if cut.len() == 1 {
            tr(de, "ist größer als das Papier", "is larger than the paper")
        } else {
            tr(de, "sind größer als das Papier", "are larger than the paper")
        },
        paper_name(paper),
        tr(de, "Beim Drucken fehlen bis zu", "Printing cuts off up to"),
    );
    let amber = egui::Color32::from_rgb(190, 110, 0);
    egui::Frame::new()
        .stroke(egui::Stroke::new(1.0, amber))
        .corner_radius(3.0)
        .inner_margin(6)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.colored_label(amber, format!("⚠ {text}"));
            ui.button(tr(de, "Auf Druckbereich reduzieren", "Shrink to printable area"))
                .on_hover_text(tr(
                    de,
                    "Zu große Seiten werden verkleinert, kleinere bleiben unverändert.",
                    "Pages that are too large are shrunk, smaller ones stay as they are.",
                ))
                .clicked()
        })
        .inner
}

fn chosen_sheets(text: &str, subset: Subset, reverse: bool, count: usize) -> Option<Vec<usize>> {
    let text = if text.trim().is_empty() { "1-" } else { text };
    let mut sheets: Vec<usize> = parse_pages(text, count)?
        .into_iter()
        .filter(|s| match subset {
            Subset::All => true,
            Subset::Odd => s % 2 == 0,
            Subset::Even => s % 2 == 1,
        })
        .collect();
    if reverse {
        sheets.reverse();
    }
    (!sheets.is_empty()).then_some(sheets)
}

fn scaling_name(s: Scaling, de: bool) -> &'static str {
    match s {
        Scaling::None => tr(de, "Keine Größenanpassung", "No scaling"),
        Scaling::Fit => tr(de, "In Druckbereich einpassen", "Fit to printable area"),
        Scaling::Shrink => tr(de, "Auf Druckbereich reduzieren", "Shrink to printable area"),
        Scaling::Custom => tr(de, "Eigene Vergrößerung", "Custom zoom"),
        Scaling::TileLarge => tr(de, "Große Seiten teilen", "Tile large pages"),
        Scaling::TileAll => tr(de, "Alle Seiten teilen", "Tile all pages"),
        Scaling::NUp => tr(de, "Mehrere Seiten pro Blatt", "Several pages per sheet"),
        Scaling::Booklet => "Booklet",
    }
}

fn content_name(c: PrintContent, de: bool) -> (&'static str, &'static str) {
    match c {
        PrintContent::Document => (
            tr(de, "Dokument", "Document"),
            tr(de, "Seiteninhalt, Formularfelder", "Page content, form fields"),
        ),
        PrintContent::DocumentAndMarkup => (
            tr(de, "Dokument und Markierungen", "Document and markups"),
            tr(
                de,
                "Seiteninhalt, Formularfelder, Kommentare, Stempel",
                "Page content, form fields, comments, stamps",
            ),
        ),
        PrintContent::DocumentAndStamps => (
            tr(de, "Dokument und Stempel", "Document and stamps"),
            tr(de, "Seiteninhalt, Formularfelder, Stempel", "Page content, form fields, stamps"),
        ),
        PrintContent::FormData => (
            tr(de, "Nur Daten der Formularfelder", "Form field data only"),
            tr(de, "Formularfelder (nur Daten)", "Form fields (data only)"),
        ),
        PrintContent::MarkupOnly => (
            tr(de, "Nur Markierungen", "Markups only"),
            tr(de, "Kommentare und Stempel", "Comments and stamps"),
        ),
        PrintContent::Everything => (
            tr(de, "Gesamter Inhalt", "All content"),
            tr(
                de,
                "Alles, auch als „nicht drucken“ markierte Kommentare",
                "Everything, also comments marked \"do not print\"",
            ),
        ),
    }
}

impl VersoloPdfApp {
    fn pr_de(&self) -> bool {
        self.settings.language == crate::ui::i18n::Language::German
    }

    /// Opens "Drucken" (or "Alle drucken").
    pub(super) fn open_print_dialog_for(&mut self, all_documents: bool) {
        let Some(tab) = self.active_tab() else { return };
        if !tab.doc.permissions().print {
            self.show_error(&crate::Error::NotPermitted);
            return;
        }
        let printers = crate::print::printers();
        let printer = printers.first().map(|p| p.name.clone()).unwrap_or_default();
        let devmode = Self::devmode_for(&printer);
        let settings = PrintSettings::default();
        let paper = paper_of(&settings, &printer, devmode.as_deref());
        self.clear_previews();
        self.dialog_doc = None;
        self.dialog = Some(Dialog::Print(Box::new(PrintState {
            settings,
            printers,
            printer,
            devmode,
            paper,
            range: PageRange::all(),
            sheets: "1-".into(),
            sheet_subset: Subset::All,
            sheets_reverse: false,
            preview_sheet: 0,
            all_documents,
            preset_name: String::new(),
            error: None,
            job_key: String::new(),
            cut: Vec::new(),
        })));
    }

    #[cfg(windows)]
    fn devmode_for(printer: &str) -> Option<Vec<u8>> {
        crate::print::default_devmode(printer)
    }

    #[cfg(not(windows))]
    fn devmode_for(_printer: &str) -> Option<Vec<u8>> {
        None
    }

    /// The sheets to print for a document (content filter, flattening and
    /// placement on sheets) and the pages cut at the paper's edge.
    fn print_job(
        doc: &mut Document,
        pages: &[usize],
        s: &PrintSettings,
        paper: &Paper,
    ) -> crate::Result<(Document, Vec<CutPage>)> {
        let bytes = doc.to_bytes()?;
        let (filtered, annots) = crate::print_layout::filter_content(&bytes, None, s.content)?;
        let filtered = if annots {
            let mut d = Document::from_bytes(filtered, "print.pdf", None)?;
            let all: Vec<usize> = (0..d.page_count()).collect();
            d.flatten_pages(&all)?;
            d.to_bytes()?
        } else {
            filtered
        };
        let src = crate::import::Source::load(&filtered, None)?;
        let sizes = pages
            .iter()
            .map(|&p| Ok((p, crate::import::upright_size(&src, p)?)))
            .collect::<crate::Result<Vec<_>>>()?;
        let sheets = crate::print_layout::layout(&sizes, paper, s);
        let cut = crate::print_layout::cut_pages(&sizes, &sheets, s);
        let bytes = crate::import::compose_sheets(&src, &sheets)?;
        Ok((Document::from_bytes(bytes, doc.display_name(), None)?, cut))
    }

    /// Draws the print dialog.
    pub(super) fn print_dialog(&mut self, ui: &mut Ui, state: &mut PrintState) -> (Outcome, Option<Action>) {
        let de = self.pr_de();
        let (n, cur) = self.active_tab().map_or((0, 0), |t| (t.doc.page_count(), t.current_page));
        ui.set_max_width(840.0);
        super::dialogs::set_title(
            ui,
            if state.all_documents {
                tr(de, "Alle drucken", "Print all")
            } else {
                tr(de, "Drucken", "Print")
            },
        );
        let mut reload_paper = false;
        ui.horizontal_top(|ui| {
            // Left: settings.
            ui.vertical(|ui| {
                ui.set_width(450.0);
                section(ui, tr(de, "Drucker", "Printer"), |ui| {
                    egui::Grid::new("print-printer").num_columns(2).spacing(vec2(8.0, 5.0)).show(ui, |ui| {
                        ui.label(tr(de, "Name:", "Name:"));
                        ui.horizontal(|ui| {
                            let shown = if state.printer.is_empty() { tr(de, "(Standarddrucker)", "(default printer)").to_string() } else { state.printer.clone() };
                            egui::ComboBox::from_id_salt("print-printer-name").selected_text(shown).width(230.0).show_ui(ui, |ui| {
                                for p in &state.printers {
                                    if ui.selectable_label(p.name == state.printer, p.name.as_str()).clicked() && p.name != state.printer {
                                        state.printer = p.name.clone();
                                        state.devmode = Self::devmode_for(&state.printer);
                                        reload_paper = true;
                                    }
                                }
                            });
                            #[cfg(windows)]
                            if ui.button(tr(de, "Einstellungen…", "Settings…")).clicked()
                                && let Some(dm) = crate::print::printer_properties(&state.printer, state.devmode.as_deref())
                            {
                                state.devmode = Some(dm);
                                reload_paper = true;
                            }
                            #[cfg(target_os = "macos")]
                            {
                                let _ = ui.add_enabled(false, egui::Button::new(tr(de, "Einstellungen…", "Settings…"))).on_disabled_hover_text(tr(
                                    de,
                                    "Nach „Drucken“ öffnet sich der Druckdialog von macOS mit allen Einstellungen des Druckers (z. B. beidseitig, Papierfach).",
                                    "After \"Print\" the macOS print dialog opens with all settings of the printer (e.g. two-sided, paper tray).",
                                ));
                            }
                            #[cfg(all(unix, not(target_os = "macos")))]
                            {
                                let _ = ui.add_enabled(false, egui::Button::new(tr(de, "Einstellungen…", "Settings…"))).on_disabled_hover_text(tr(
                                    de,
                                    "Die Druckereinstellungen werden hier über das System (CUPS) verwaltet.",
                                    "Printer settings are managed by the system (CUPS) here.",
                                ));
                            }
                        });
                        ui.end_row();
                        let info = state.printers.iter().find(|p| p.name == state.printer);
                        ui.label(tr(de, "Status:", "Status:"));
                        ui.label(match info {
                            Some(p) if p.ready => tr(de, "Bereit", "Ready"),
                            Some(_) => tr(de, "Nicht bereit", "Not ready"),
                            None => tr(de, "Kein Drucker gefunden", "No printer found"),
                        });
                        ui.end_row();
                        ui.label(tr(de, "Typ:", "Type:"));
                        ui.label(info.map(|p| p.driver.clone()).unwrap_or_default());
                        ui.end_row();
                        ui.label(tr(de, "Kopien:", "Copies:"));
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(&mut state.settings.copies).range(1..=999));
                            ui.checkbox(&mut state.settings.collate, tr(de, "Sammeln", "Collate"));
                            ui.label(tr(de, "Duplex:", "Duplex:"));
                            let names = [
                                (Duplex::Auto, tr(de, "Auto (Ohne)", "Auto (none)")),
                                (Duplex::Simplex, tr(de, "Ohne", "None")),
                                (Duplex::LongEdge, tr(de, "Lange Seite", "Long edge")),
                                (Duplex::ShortEdge, tr(de, "Kurze Seite", "Short edge")),
                            ];
                            let current = names.iter().find(|n| n.0 == state.settings.duplex).map_or("", |n| n.1);
                            egui::ComboBox::from_id_salt("print-duplex").selected_text(current).width(100.0).show_ui(ui, |ui| {
                                for (v, t) in names {
                                    ui.selectable_value(&mut state.settings.duplex, v, t);
                                }
                            });
                        });
                        ui.end_row();
                        #[cfg(not(windows))]
                        {
                            ui.label(tr(de, "Papier:", "Paper:"));
                            let before = state.settings.paper.clone();
                            super::page_dialogs::paper_list_combo(ui, de, "print-paper", &mut state.settings.paper, crate::ui::pickers::Unit::Mm);
                            reload_paper |= before != state.settings.paper;
                            ui.end_row();
                        }
                    });
                });
                state.range.ui(ui, de, n, cur, &[RangeMode::All, RangeMode::Current, RangeMode::Custom]);
                ui.checkbox(&mut state.settings.reverse, tr(de, "Reihenfolge umkehren", "Reverse order"));
                let s = &mut state.settings;
                section(ui, tr(de, "Position und Skalierung", "Position and scaling"), |ui| {
                    ui.horizontal(|ui| {
                        ui.label(tr(de, "Typ:", "Type:"));
                        egui::ComboBox::from_id_salt("print-scaling").selected_text(scaling_name(s.scaling, de)).width(240.0).show_ui(ui, |ui| {
                            for v in [Scaling::None, Scaling::Fit, Scaling::Shrink, Scaling::Custom, Scaling::TileLarge, Scaling::TileAll, Scaling::NUp, Scaling::Booklet] {
                                ui.selectable_value(&mut s.scaling, v, scaling_name(v, de));
                            }
                        });
                    });
                    match s.scaling {
                        Scaling::Custom | Scaling::TileLarge | Scaling::TileAll => {
                            ui.horizontal(|ui| {
                                ui.label(tr(de, "Vergrößerung:", "Zoom:"));
                                ui.add(egui::DragValue::new(&mut s.zoom).range(1.0..=1000.0).suffix(" %"));
                                if s.scaling != Scaling::Custom {
                                    ui.label(tr(de, "Überlappung:", "Overlap:"));
                                    let mut mm = s.tile_overlap * 25.4 / 72.0;
                                    if ui.add(egui::DragValue::new(&mut mm).range(0.0..=50.0).suffix(" mm")).changed() {
                                        s.tile_overlap = mm * 72.0 / 25.4;
                                    }
                                }
                            });
                        }
                        Scaling::NUp => {
                            ui.horizontal(|ui| {
                                ui.label(tr(de, "Seiten pro Blatt:", "Pages per sheet:"));
                                ui.add(egui::DragValue::new(&mut s.columns).range(1..=8));
                                ui.label("x");
                                ui.add(egui::DragValue::new(&mut s.rows).range(1..=8));
                                segmented(
                                    ui,
                                    &mut s.order,
                                    &[(PageOrder::Horizontal, tr(de, "Horizontal", "Horizontal").into()), (PageOrder::Vertical, tr(de, "Vertikal", "Vertical").into())],
                                );
                            });
                            ui.checkbox(&mut s.border, tr(de, "Seitenrahmen drucken", "Print page borders"));
                        }
                        Scaling::Booklet => {
                            ui.horizontal(|ui| {
                                ui.label(tr(de, "Bindung:", "Binding:"));
                                segmented(ui, &mut s.binding, &[(Binding::Left, tr(de, "Links", "Left").into()), (Binding::Right, tr(de, "Rechts", "Right").into())]);
                            });
                            ui.label(RichText::new(tr(de, "Beidseitig über die kurze Seite drucken und in der Mitte falten.", "Print double-sided (short edge) and fold in the middle.")).small().weak());
                        }
                        _ => {}
                    }
                    let simple = matches!(s.scaling, Scaling::None | Scaling::Fit | Scaling::Shrink | Scaling::Custom);
                    ui.add_enabled(simple || s.scaling == Scaling::NUp, egui::Checkbox::new(&mut s.auto_rotate, tr(de, "Seiten automatisch drehen", "Rotate pages automatically")));
                    ui.add_enabled(simple || s.scaling == Scaling::TileLarge, egui::Checkbox::new(&mut s.auto_center, tr(de, "Seiten automatisch zentrieren", "Center pages automatically")));
                    ui.add_enabled(simple, egui::Checkbox::new(&mut s.paper_from_page, tr(de, "Papiergröße anhand der Dokumentseitengröße wählen", "Choose paper size by page size")));
                    ui.add_enabled(matches!(s.scaling, Scaling::Fit | Scaling::Shrink), egui::Checkbox::new(&mut s.ignore_margins, tr(de, "Druckerrandeinstellungen ignorieren", "Ignore printer margins")));
                });
                section(ui, tr(de, "Erweiterte Druckeinstellungen", "Advanced print settings"), |ui| {
                    ui.horizontal(|ui| {
                        ui.label(tr(de, "Drucken:", "Print:"));
                        egui::ComboBox::from_id_salt("print-content").selected_text(content_name(s.content, de).0).width(260.0).show_ui(ui, |ui| {
                            for v in [PrintContent::Document, PrintContent::DocumentAndMarkup, PrintContent::DocumentAndStamps, PrintContent::FormData, PrintContent::MarkupOnly, PrintContent::Everything] {
                                let (name, detail) = content_name(v, de);
                                ui.selectable_value(&mut s.content, v, name).on_hover_text(detail);
                            }
                        });
                    });
                    #[cfg(windows)]
                    {
                        let mut always = true;
                        let _ = ui.add_enabled(false, egui::Checkbox::new(&mut always, tr(de, "Als Bild drucken", "Print as image"))).on_disabled_hover_text(tr(
                            de,
                            "Unter Windows druckt VersoloPDF die Seiten immer als Bild.",
                            "On Windows VersoloPDF always prints pages as images.",
                        ));
                    }
                    #[cfg(not(windows))]
                    ui.checkbox(&mut s.as_image, tr(de, "Als Bild drucken", "Print as image"));
                });
            });
            ui.add_space(8.0);
            // Right: preview.
            ui.vertical(|ui| {
                ui.set_width(330.0);
                ui.label(RichText::new(format!("{} {}", tr(de, "Papier:", "Paper:"), paper_name(&state.paper))).weak());
                let pages = self.tabs.get(self.active).and_then(|t| state.range.resolve(t.doc.page_count(), t.current_page));
                let key = format!(
                    "print-{}-{:?}-{:?}-{:?}",
                    self.tabs.get(self.active).map_or(0, |t| t.doc.revision() ^ t.id.rotate_left(32)),
                    pages,
                    state.settings,
                    state.paper
                );
                // The sheets are made again when a setting changes.
                if state.job_key != key {
                    state.job_key = key.clone();
                    let job = match (pages.as_ref(), self.tabs.get_mut(self.active)) {
                        (Some(p), Some(tab)) => Self::print_job(&mut tab.doc, p, &state.settings, &state.paper),
                        _ => Err(crate::Error::Invalid(tr(de, "Keine Seiten ausgewählt.", "No pages selected.").into())),
                    };
                    match job {
                        Ok((doc, cut)) => {
                            state.error = None;
                            state.cut = cut;
                            self.dialog_doc = Some((key, doc));
                        }
                        Err(err) => {
                            state.error = Some(super::actions::error_text(self.t(), &err));
                            state.cut.clear();
                            self.dialog_doc = None;
                        }
                    }
                }
                let count = self.dialog_doc.as_ref().map_or(0, |(_, d)| d.page_count());
                state.preview_sheet = state.preview_sheet.min(count.saturating_sub(1));
                if let Some((_, doc)) = &self.dialog_doc
                    && count > 0
                {
                    let area =
                        doc_preview(ui, &mut self.preview[0], doc, 0x9e1, state.preview_sheet, vec2(330.0, 380.0), super::preview::PreviewOpts::new(de));
                    // The printable area of the paper, so cut parts are visible.
                    let sheet_is_paper = doc
                        .geometry(state.preview_sheet)
                        .is_some_and(|g| (g.crop.width() - state.paper.size.0).abs() < 1.0);
                    let p = state.paper.printable;
                    let has_border = p.x0 > 0.5 || p.y0 > 0.5 || p.x1 < state.paper.size.0 - 0.5;
                    if let Some(area) = area
                        && sheet_is_paper
                        && has_border
                    {
                        let r = egui::Rect::from_two_pos(
                            area.to_screen(crate::geometry::PointF::new(p.x0, p.y0)),
                            area.to_screen(crate::geometry::PointF::new(p.x1, p.y1)),
                        );
                        let pts = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
                        let stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(220, 60, 60));
                        area.painter.extend(egui::Shape::dashed_line(&pts, stroke, 4.0, 3.0));
                    }
                } else {
                    let (rect, _) = ui.allocate_exact_size(vec2(330.0, 380.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 0.0, egui::Color32::from_gray(128));
                }
                if let Some(e) = &state.error {
                    ui.colored_label(egui::Color32::from_rgb(200, 40, 40), e.as_str());
                }
                if state.paper.printable != crate::geometry::RectF::new(0.0, 0.0, state.paper.size.0, state.paper.size.1) {
                    ui.label(RichText::new(tr(de, "Gestrichelt: bedruckbarer Bereich des Druckers", "Dashed: printable area of the printer")).small().weak());
                }
                if !state.cut.is_empty() && cut_warning(ui, de, &state.cut, &state.paper) {
                    state.settings.scaling = Scaling::Shrink;
                    state.settings.ignore_margins = false;
                }
                ui.label(RichText::new(format!("{} {} / {count}", tr(de, "Blatt", "Sheet"), state.preview_sheet + 1)).small());
                page_strip(ui, &mut state.preview_sheet, count);
                ui.horizontal(|ui| {
                    ui.label(tr(de, "Drucke Blätter:", "Print sheets:"));
                    ui.add(egui::TextEdit::singleline(&mut state.sheets).desired_width(80.0));
                    let chosen = chosen_sheets(&state.sheets, state.sheet_subset, state.sheets_reverse, count).map_or(0, |s| s.len());
                    ui.label(format!("({chosen} {} {count} {})", tr(de, "ausgewählt,", "selected,"), tr(de, "insg.", "total")));
                });
                ui.horizontal(|ui| {
                    ui.label(tr(de, "Teilmenge:", "Subset:"));
                    segmented(
                        ui,
                        &mut state.sheet_subset,
                        &[
                            (Subset::All, tr(de, "Alle", "All").into()),
                            (Subset::Odd, tr(de, "Ungerade", "Odd").into()),
                            (Subset::Even, tr(de, "Gerade", "Even").into()),
                        ],
                    );
                    ui.checkbox(&mut state.sheets_reverse, tr(de, "Umkehren", "Reverse"));
                });
            });
        });
        if reload_paper {
            state.paper = paper_of(&state.settings, &state.printer, state.devmode.as_deref());
        }
        ui.separator();
        let mut print = false;
        let mut cancel = false;
        ui.horizontal(|ui| {
            let mut name = std::mem::take(&mut state.preset_name);
            let _ = preset_combo(ui, de, "print", &mut state.settings, &mut self.settings, &mut name);
            state.preset_name = name;
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                cancel = ui.button(tr(de, "Abbrechen", "Cancel")).clicked();
                let count = self.dialog_doc.as_ref().map_or(0, |(_, d)| d.page_count());
                let ok = state.error.is_none()
                    && chosen_sheets(&state.sheets, state.sheet_subset, state.sheets_reverse, count)
                        .is_some();
                print = ui
                    .add_enabled(ok, egui::Button::new(RichText::new(tr(de, "Drucken", "Print")).strong()))
                    .clicked();
            });
        });
        if cancel || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            return (Outcome::Close, None);
        }
        if !print {
            return (Outcome::Keep, None);
        }
        let state = state.clone();
        (Outcome::Close, Some(Box::new(move |app, ctx| app.run_print(ctx, &state))))
    }

    /// Sends the job(s) to the printer.
    fn run_print(&mut self, ctx: &Context, state: &PrintState) {
        let de = self.pr_de();
        let mut done = Vec::new();
        let indices: Vec<usize> =
            if state.all_documents { (0..self.tabs.len()).collect() } else { vec![self.active] };
        for i in indices {
            let tab = &mut self.tabs[i];
            if !tab.doc.permissions().print {
                continue;
            }
            let pages = if state.all_documents {
                Some((0..tab.doc.page_count()).collect::<Vec<_>>())
            } else {
                state.range.resolve(tab.doc.page_count(), tab.current_page)
            };
            let Some(pages) = pages else { continue };
            let title = tab.doc.display_name().to_string();
            let result =
                Self::print_job(&mut tab.doc, &pages, &state.settings, &state.paper).and_then(|(job, _)| {
                    let sheets = chosen_sheets(
                        &state.sheets,
                        state.sheet_subset,
                        state.sheets_reverse,
                        job.page_count(),
                    )
                    .ok_or_else(|| crate::Error::Invalid("no sheets selected".into()))?;
                    crate::print::print_job(
                        &state.printer,
                        state.devmode.as_deref(),
                        &state.settings,
                        &job,
                        &sheets,
                        &title,
                        &state.paper,
                    )
                });
            match result {
                // Cancelled in the system's print dialog.
                Ok(d) if d == crate::print::CANCELLED => {}
                Ok(_) => done.push(title),
                Err(err) => {
                    self.show_error(&err);
                    return;
                }
            }
        }
        if !done.is_empty() {
            let msg = if de { "An den Drucker gesendet: " } else { "Sent to the printer: " };
            self.set_status(ctx, format!("{msg}{}", done.join(", ")));
        }
        self.clear_previews();
        self.dialog_doc = None;
    }
}
