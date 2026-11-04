//! Dialogs of step E: adding, editing, managing
//! and removing watermarks, backgrounds, headers/footers and Bates numbers,
//! and Bates numbering of several files.

use std::path::{Path, PathBuf};

use egui::{Color32, Context, RichText, Stroke, Ui, vec2};

use super::commands::Cmd;
use super::dialogs::{Action, Outcome};
use super::page_dialogs::{align_combo, file_name, footer, macro_menu, pages_text, tr, unique_path};
use super::preview::{doc_preview, page_strip};
use super::structure::MarkKind;
use super::{Dialog, MrPdfApp};
use crate::Document;
use crate::compose::{Align, TextFont};
use crate::marks::{BatesFormat, FontSpec, ImageData, Kind, MarkDef, Source, Visibility};
use crate::ui::i18n::Language;
use crate::ui::icons::Icon;
use crate::ui::pickers::{PageRange, length, preset_combo, section, segmented, unit_combo_right};
use crate::ui::widgets::{from_color32, icon_button, small_button, to_color32};

/// Image files and PDFs accepted as watermark/background.
const IMAGE_EXT: [&str; 15] = [
    "png", "jpg", "jpeg", "gif", "bmp", "tif", "tiff", "webp", "pbm", "pgm", "ppm", "pnm", "tga", "ico",
    "pdf",
];
const MM: f32 = 72.0 / 25.4;

fn kind_of(k: MarkKind) -> Kind {
    match k {
        MarkKind::Watermark => Kind::Watermark,
        MarkKind::Background => Kind::Background,
        MarkKind::HeaderFooter => Kind::HeaderFooter,
        MarkKind::Bates => Kind::Bates,
    }
}

fn kind_title(k: Kind, de: bool) -> (&'static str, &'static str) {
    match k {
        Kind::Watermark => (
            tr(de, "Wasserzeichen hinzufügen", "Add watermark"),
            tr(de, "Wasserzeichen verwalten", "Manage watermarks"),
        ),
        Kind::Background => (
            tr(de, "Hintergrund hinzufügen", "Add background"),
            tr(de, "Hintergründe verwalten", "Manage backgrounds"),
        ),
        Kind::HeaderFooter => (
            tr(de, "Kopf- und Fußzeile hinzufügen", "Add header and footer"),
            tr(de, "Kopf- & Fußzeilen verwalten", "Manage headers & footers"),
        ),
        Kind::Bates => (
            tr(de, "Paginierung hinzufügen", "Add Bates numbering"),
            tr(de, "Paginierung verwalten", "Manage Bates numbering"),
        ),
    }
}

/// Default definition of a new mark.
fn default_def(kind: Kind, pages: usize, de: bool) -> MarkDef {
    let all: Vec<usize> = (0..pages).collect();
    match kind {
        Kind::Watermark => MarkDef {
            kind,
            source: Source::Text {
                text: tr(de, "ENTWURF", "DRAFT").into(),
                font: FontSpec {
                    size: 72.0,
                    color: crate::model::Rgba::rgb(200, 30, 30),
                    bold: true,
                    ..FontSpec::default()
                },
                align: Align::Start,
            },
            rotation: 45.0,
            opacity: 0.25,
            pages: all,
            ..MarkDef::default()
        },
        Kind::Background => MarkDef {
            kind,
            source: Source::Color { color: crate::model::Rgba::rgb(255, 250, 225) },
            behind: true,
            pages: all,
            ..MarkDef::default()
        },
        Kind::HeaderFooter => MarkDef {
            kind,
            source: Source::HeaderFooter {
                texts: [
                    String::new(),
                    String::new(),
                    String::new(),
                    "%[FileName]".into(),
                    String::new(),
                    tr(de, "Seite %[Page] von %[Pages]", "Page %[Page] of %[Pages]").into(),
                ],
                font: FontSpec { size: 10.0, ..FontSpec::default() },
                margins: [12.7 * MM; 4],
                shrink: false,
            },
            pages: all,
            ..MarkDef::default()
        },
        Kind::Bates => MarkDef {
            kind,
            source: Source::HeaderFooter {
                texts: [
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    "%[Bates]".into(),
                ],
                font: FontSpec { size: 10.0, ..FontSpec::default() },
                margins: [12.7 * MM; 4],
                shrink: false,
            },
            pages: all,
            ..MarkDef::default()
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatermarkType {
    Text,
    File,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MarksDialog {
    Edit {
        def: MarkDef,
        /// Id of the mark being edited.
        editing: Option<String>,
        range: PageRange,
        page: usize,
        /// Image or PDF file chosen for an image mark.
        file: Option<PathBuf>,
        file_page: u32,
        /// Back to the manager after OK/Cancel.
        back_to_manage: bool,
    },
    Manage {
        kind: Kind,
        selected: Option<usize>,
    },
    BatesFiles {
        files: Vec<PathBuf>,
        selected: Option<usize>,
        folder: PathBuf,
        range_names: bool,
        pattern: String,
        overwrite: bool,
        open_folder: bool,
        log: bool,
        log_path: PathBuf,
        def: MarkDef,
        show_settings: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct MarksState {
    pub dialog: MarksDialog,
    pub preset_name: String,
}

fn font_menu(ui: &mut Ui, de: bool, font: &mut FontSpec) {
    ui.menu_button(tr(de, "Schriftart…", "Font…"), |ui| {
        ui.set_min_width(220.0);
        segmented(
            ui,
            &mut font.font,
            &[(TextFont::Helvetica, "Helvetica".into()), (TextFont::Courier, "Courier".into())],
        );
        ui.checkbox(&mut font.bold, tr(de, "Fett", "Bold"));
        ui.horizontal(|ui| {
            ui.label(tr(de, "Größe:", "Size:"));
            ui.add(egui::DragValue::new(&mut font.size).range(2.0..=400.0).suffix(" pt"));
        });
        ui.horizontal(|ui| {
            ui.label(tr(de, "Farbe:", "Colour:"));
            let mut c = to_color32(font.color);
            if ui.color_edit_button_srgba(&mut c).changed() {
                font.color = from_color32(c);
            }
        });
    });
}

/// True when fewer than half of the white pixels of `before` (RGBA) changed
/// in `after`: the page covers a background put behind it.
fn background_hidden(before: &[u8], after: &[u8]) -> bool {
    let white = |p: &[u8]| p[0] > 245 && p[1] > 245 && p[2] > 245;
    let (mut total, mut changed) = (0usize, 0usize);
    for (a, b) in before.chunks(4).zip(after.chunks(4)) {
        if white(a) {
            total += 1;
            changed += usize::from(a != b);
        }
    }
    total > 0 && changed * 2 < total
}

/// Dashed guides for the header and footer margins.
fn margin_guides(area: &super::preview::PreviewArea, margins: &[f32; 4]) {
    let r = area.rect;
    let s = area.scale;
    let guide = Stroke::new(1.0, Color32::from_rgb(40, 90, 220));
    let xs = [r.left() + margins[0] * s, r.right() - margins[1] * s];
    let ys = [r.top() + margins[2] * s, r.bottom() - margins[3] * s];
    for x in xs {
        area.painter.add(egui::Shape::dashed_line(
            &[egui::pos2(x, r.top()), egui::pos2(x, r.bottom())],
            guide,
            4.0,
            3.0,
        ));
    }
    for y in ys {
        area.painter.add(egui::Shape::dashed_line(
            &[egui::pos2(r.left(), y), egui::pos2(r.right(), y)],
            guide,
            4.0,
            3.0,
        ));
    }
}

const MACROS: [(&str, &str, &str); 6] = [
    ("%[Page]", "Seitenzahl", "Page number"),
    ("%[Pages]", "Seitenanzahl", "Page count"),
    ("%[Date]", "Datum", "Date"),
    ("%[Time]", "Uhrzeit", "Time"),
    ("%[FileName]", "Dateiname", "File name"),
    ("%[Bates]", "Paginierungsnummer", "Bates number"),
];

impl MrPdfApp {
    fn md_de(&self) -> bool {
        self.settings.language == Language::German
    }

    fn open_marks(&mut self, dialog: MarksDialog) {
        self.clear_previews();
        self.dialog_doc = None;
        self.mark_covered = (false, false);
        self.dialog = Some(Dialog::Marks(Box::new(MarksState { dialog, preset_name: String::new() })));
    }

    fn mark_editor(&mut self, kind: Kind, back_to_manage: bool) {
        let (n, cur) = self.active_tab().map_or((0, 0), |t| (t.doc.page_count(), t.current_page));
        let def = default_def(kind, n, self.md_de());
        self.open_marks(MarksDialog::Edit {
            def,
            editing: None,
            range: PageRange::all(),
            page: cur,
            file: None,
            file_page: 1,
            back_to_manage,
        });
    }

    /// Handles the page mark commands. Returns false for other commands.
    pub(super) fn run_marks(&mut self, cmd: Cmd) -> bool {
        if self.active_tab().is_none() {
            return false;
        }
        let de = self.md_de();
        let lang = self.settings.language;
        match cmd {
            Cmd::Watermark => self.mark_editor(Kind::Watermark, false),
            Cmd::Background => self.mark_editor(Kind::Background, false),
            Cmd::HeaderFooter => self.mark_editor(Kind::HeaderFooter, false),
            Cmd::Bates => self.mark_editor(Kind::Bates, false),
            Cmd::ManageMarks(k) => self.open_marks(MarksDialog::Manage { kind: kind_of(k), selected: None }),
            Cmd::RemoveMarks(k) => {
                let text = match k {
                    MarkKind::Watermark => tr(
                        de,
                        "Sind Sie sicher, dass Sie alle Wasserzeichen aus dem Dokument entfernen möchten?",
                        "Do you really want to remove all watermarks from the document?",
                    ),
                    MarkKind::Background => tr(
                        de,
                        "Sind Sie sicher, dass Sie alle Hintergründe aus dem Dokument entfernen möchten?",
                        "Do you really want to remove all backgrounds from the document?",
                    ),
                    MarkKind::HeaderFooter => tr(
                        de,
                        "Sind Sie sicher, dass Sie alle Kopf- und Fußzeilen aus dem Dokument entfernen möchten?",
                        "Do you really want to remove all headers and footers from the document?",
                    ),
                    MarkKind::Bates => tr(
                        de,
                        "Sind Sie sicher, dass Sie alle Paginierungen aus dem Dokument entfernen möchten?",
                        "Do you really want to remove all Bates numbers from the document?",
                    ),
                };
                self.dialog = Some(Dialog::Confirm { title: cmd.label(lang).into(), text: text.into(), cmd });
            }
            Cmd::BatesFiles => {
                let folder = self
                    .active_tab()
                    .and_then(|t| t.doc.path())
                    .and_then(Path::parent)
                    .map(Path::to_path_buf)
                    .unwrap_or_default();
                self.open_marks(MarksDialog::BatesFiles {
                    files: Vec::new(),
                    selected: None,
                    log_path: folder.join("Paginierung.txt"),
                    folder,
                    range_names: false,
                    pattern: "%[FileName]".into(),
                    overwrite: false,
                    open_folder: false,
                    log: false,
                    def: default_def(Kind::Bates, 0, de),
                    show_settings: false,
                });
            }
            _ => return false,
        }
        true
    }

    /// "Alle entfernen" after the confirmation.
    pub(super) fn remove_all_marks(&mut self, ctx: &Context, k: MarkKind) {
        let de = self.md_de();
        let result = self.tab().map(|t| t.doc.remove_marks(kind_of(k), None, true));
        if let Some(tab) = self.tab() {
            tab.after_edit();
        }
        match result {
            Some(Ok(n)) => self.set_status(
                ctx,
                if de { format!("Entfernte Markierungen: {n}") } else { format!("Removed marks: {n}") },
            ),
            Some(Err(err)) => self.show_error(&err),
            None => {}
        }
    }

    pub(super) fn marks_dialog(&mut self, ui: &mut Ui, state: &mut MarksState) -> (Outcome, Option<Action>) {
        let de = self.md_de();
        let unit = self.settings.unit;
        let (n, cur) = self.active_tab().map_or((0, 0), |t| (t.doc.page_count(), t.current_page));
        let mut outcome = Outcome::Keep;
        let mut action: Option<Action> = None;
        match &mut state.dialog {
            MarksDialog::Edit { def, editing, range, page, file, file_page, back_to_manage } => {
                let kind = def.kind;
                let title = if editing.is_some() { kind_title(kind, de).1 } else { kind_title(kind, de).0 };
                super::dialogs::set_title(ui, title);
                *page = (*page).min(n.saturating_sub(1));
                let header_like = matches!(kind, Kind::HeaderFooter | Kind::Bates);
                // Keep the dialog within the window.
                ui.set_max_width(if header_like { 800.0 } else { 820.0 });
                if header_like {
                    self.header_footer_editor(ui, def, unit);
                }
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(if header_like { 760.0 } else { 440.0 });
                        if !header_like {
                            self.source_editor(ui, def, file, file_page, unit);
                            self.appearance_editor(ui, def);
                            section(ui, tr(de, "Einfügen", "Placement"), |ui| {
                                ui.horizontal(|ui| unit_combo_right(ui, de, &mut self.settings.unit));
                                egui::Grid::new("mark-place").num_columns(4).spacing(vec2(8.0, 6.0)).show(
                                    ui,
                                    |ui| {
                                        ui.label(tr(de, "Horiz. verschieben:", "Move horizontally:"));
                                        length(ui, &mut def.dx, unit, -5000.0..=5000.0);
                                        ui.label(tr(de, "von:", "from:"));
                                        align_combo(ui, de, "mark-h", &mut def.h_from, false);
                                        ui.end_row();
                                        ui.label(tr(de, "Vert. verschieben:", "Move vertically:"));
                                        length(ui, &mut def.dy, unit, -5000.0..=5000.0);
                                        ui.label(tr(de, "von:", "from:"));
                                        align_combo(ui, de, "mark-v", &mut def.v_from, true);
                                        ui.end_row();
                                    },
                                );
                                if kind == Kind::Watermark {
                                    ui.checkbox(
                                        &mut def.behind,
                                        tr(de, "Im Hintergrund", "In the background"),
                                    );
                                }
                            });
                        }
                        range.ui(ui, de, n, cur, &PageRange::ALL_MODES);
                    });
                    if !header_like {
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
                            self.mark_preview_ui(
                                ui,
                                def,
                                file.as_deref(),
                                *file_page,
                                *page,
                                vec2(340.0, 460.0),
                            );
                            page_strip(ui, page, n);
                        });
                    }
                });
                if header_like {
                    ui.horizontal(|ui| {
                        ui.label(tr(de, "Vorschau", "Preview"));
                        let mut p = *page + 1;
                        if ui.add(egui::DragValue::new(&mut p).range(1..=n.max(1))).changed() {
                            *page = p - 1;
                        }
                        ui.label(format!("/{n}"));
                    });
                    // A smaller preview on small screens, so OK stays in view.
                    let height = (ui.ctx().content_rect().height() - 620.0).clamp(120.0, 300.0);
                    self.mark_preview_ui(ui, def, None, 1, *page, vec2(760.0, height));
                }
                let mut name = std::mem::take(&mut state.preset_name);
                let key = format!("mark-{kind:?}");
                preset_combo(ui, de, &key, def, &mut self.settings, &mut name);
                state.preset_name = name;
                let pages = range.resolve(n, cur);
                let source_ok =
                    !matches!(def.source, Source::Image { .. }) || file.is_some() || editing.is_some();
                let (ok, cancel, _) = footer(ui, de, pages.is_some() && source_ok, false, "OK");
                if ok && let Some(pages) = pages {
                    let mut def = def.clone();
                    def.pages = pages;
                    let (editing, file, file_page, back) =
                        (editing.clone(), file.clone(), *file_page, *back_to_manage);
                    action = Some(Box::new(move |app, ctx| {
                        let image = match &file {
                            Some(p) => match load_image_data(p, file_page) {
                                Ok(i) => Some(i),
                                Err(err) => {
                                    app.show_error(&err);
                                    return;
                                }
                            },
                            None => None,
                        };
                        app.edit(ctx, |tab| match &editing {
                            Some(id) => tab.doc.update_mark(id, &def, image),
                            None => tab.doc.add_mark(&def, image).map(|_| ()),
                        });
                        if back {
                            app.open_marks(MarksDialog::Manage { kind: def.kind, selected: None });
                        }
                    }));
                    outcome = Outcome::Close;
                } else if cancel {
                    if *back_to_manage {
                        let kind = def.kind;
                        action = Some(Box::new(move |app, _| {
                            app.open_marks(MarksDialog::Manage { kind, selected: None })
                        }));
                    }
                    outcome = Outcome::Close;
                }
            }
            MarksDialog::Manage { kind, selected } => {
                let kind = *kind;
                super::dialogs::set_title(ui, kind_title(kind, de).1);
                let marks: Vec<_> = self
                    .tab()
                    .and_then(|t| t.doc.marks().ok())
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|m| m.def.kind == kind)
                    .collect();
                let sel = selected.filter(|i| *i < marks.len());
                let mut todo: Option<Action> = None;
                ui.horizontal(|ui| {
                    if small_button(ui, Icon::InsertPage, tr(de, "Hinzufügen", "Add"), false, true).clicked()
                    {
                        todo = Some(Box::new(move |app, _| app.mark_editor(kind, true)));
                    }
                    if small_button(ui, Icon::Gear, tr(de, "Bearbeiten…", "Edit…"), false, sel.is_some())
                        .clicked()
                        && let Some(i) = sel
                    {
                        let m = marks[i].clone();
                        todo = Some(Box::new(move |app, _| app.edit_existing_mark(m)));
                    }
                    let ids: Vec<String> = marks.iter().map(|m| m.id.clone()).collect();
                    for (icon, tip, delta) in [
                        (Icon::MoveUp, tr(de, "Nach oben", "Up"), -1i32),
                        (Icon::MoveDown, tr(de, "Nach unten", "Down"), 1),
                    ] {
                        let target = sel
                            .and_then(|i| usize::try_from(i as i32 + delta).ok())
                            .filter(|t| *t < marks.len());
                        if icon_button(ui, icon, tip, false, target.is_some()).clicked()
                            && let (Some(i), Some(t)) = (sel, target)
                        {
                            let mut order = ids.clone();
                            order.swap(i, t);
                            *selected = Some(t);
                            todo = Some(Box::new(move |app, ctx| {
                                app.edit(ctx, |tab| tab.doc.reorder_marks(kind, &order))
                            }));
                        }
                    }
                    if small_button(ui, Icon::Delete, tr(de, "Entfernen", "Remove"), false, sel.is_some())
                        .clicked()
                        && let Some(i) = sel
                    {
                        let id = vec![marks[i].id.clone()];
                        *selected = None;
                        todo = Some(Box::new(move |app, ctx| {
                            app.edit(ctx, |tab| tab.doc.remove_marks(kind, Some(&id), false).map(|_| ()))
                        }));
                    }
                    if small_button(
                        ui,
                        Icon::Delete,
                        tr(de, "Alle entfernen", "Remove all"),
                        false,
                        !marks.is_empty(),
                    )
                    .clicked()
                    {
                        *selected = None;
                        todo = Some(Box::new(move |app, ctx| {
                            app.edit(ctx, |tab| tab.doc.remove_marks(kind, None, true).map(|_| ()))
                        }));
                    }
                });
                egui::Frame::new().stroke(ui.visuals().widgets.noninteractive.bg_stroke).show(ui, |ui| {
                    ui.set_min_size(vec2(560.0, 300.0));
                    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                        egui::Grid::new("marks-list")
                            .num_columns(3)
                            .striped(true)
                            .spacing(vec2(16.0, 4.0))
                            .show(ui, |ui| {
                                for h in
                                    ["Nr.", tr(de, "Beschreibung", "Description"), tr(de, "Seiten", "Pages")]
                                {
                                    ui.label(RichText::new(h).strong());
                                }
                                ui.end_row();
                                for (i, m) in marks.iter().enumerate() {
                                    if ui.selectable_label(sel == Some(i), (i + 1).to_string()).clicked() {
                                        *selected = Some(i);
                                    }
                                    let text = m.def.describe();
                                    if ui
                                        .selectable_label(
                                            sel == Some(i),
                                            if text.is_empty() { "–".into() } else { text },
                                        )
                                        .double_clicked()
                                    {
                                        let m = m.clone();
                                        todo = Some(Box::new(move |app, _| app.edit_existing_mark(m)));
                                    }
                                    ui.label(pages_text(&m.def.pages));
                                    ui.end_row();
                                }
                            });
                    });
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            sel.is_some(),
                            egui::Button::new(tr(de, "Seiten auswählen", "Select pages")),
                        )
                        .clicked()
                        && let Some(i) = sel
                    {
                        let m = marks[i].clone();
                        todo = Some(Box::new(move |app, _| app.edit_existing_mark(m)));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(tr(de, "Schließen", "Close")).clicked()
                            || ui.input(|i| i.key_pressed(egui::Key::Escape))
                        {
                            outcome = Outcome::Close;
                        }
                    });
                });
                if let Some(t) = todo {
                    action = Some(t);
                    // Dialog changes (editor) replace this one; list changes keep it.
                }
            }
            MarksDialog::BatesFiles {
                files,
                selected,
                folder,
                range_names,
                pattern,
                overwrite,
                open_folder,
                log,
                log_path,
                def,
                show_settings,
            } => {
                super::dialogs::set_title(
                    ui,
                    tr(de, "Paginierung in Dateien hinzufügen", "Add Bates numbering to files"),
                );
                super::page_dialogs::file_list(ui, de, files, selected, &["pdf"], "PDF");
                ui.label(format!("{} {}", files.len(), tr(de, "Einträge", "entries")));
                section(ui, tr(de, "Ausgabe", "Output"), |ui| {
                    ui.horizontal(|ui| {
                        ui.label(tr(de, "Zielordner:", "Target folder:"));
                        let mut text = folder.display().to_string();
                        if ui.add(egui::TextEdit::singleline(&mut text).desired_width(360.0)).changed() {
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
                    ui.radio_value(
                        range_names,
                        true,
                        tr(
                            de,
                            "Den Seitenbereich im Dokument als Namen verwenden (z. B. 000100-000115.pdf)",
                            "Use the page range as name (e.g. 000100-000115.pdf)",
                        ),
                    );
                    ui.horizontal(|ui| {
                        ui.radio_value(range_names, false, tr(de, "Dateiname:", "File name:"));
                        ui.add_enabled(
                            !*range_names,
                            egui::TextEdit::singleline(pattern).desired_width(260.0),
                        );
                    });
                    ui.checkbox(
                        overwrite,
                        tr(de, "Existierende Dateien überschreiben", "Overwrite existing files"),
                    );
                    ui.checkbox(
                        open_folder,
                        tr(
                            de,
                            "Nach der Nummerierung den Ordner mit dem Ergebnis öffnen",
                            "Open the result folder afterwards",
                        ),
                    );
                    ui.horizontal(|ui| {
                        ui.checkbox(log, tr(de, "Protokoll erstellen", "Write log"));
                        let mut text = log_path.display().to_string();
                        if ui
                            .add_enabled(*log, egui::TextEdit::singleline(&mut text).desired_width(300.0))
                            .changed()
                        {
                            *log_path = PathBuf::from(text);
                        }
                    });
                });
                if *show_settings {
                    self.header_footer_editor(ui, def, unit);
                }
                let folder_ok = folder.is_dir();
                let (ok, cancel, settings) = footer(ui, de, !files.is_empty() && folder_ok, true, "OK");
                if settings {
                    *show_settings = !*show_settings;
                }
                if ok {
                    let job = BatesJob {
                        files: files.clone(),
                        folder: folder.clone(),
                        range_names: *range_names,
                        pattern: pattern.clone(),
                        overwrite: *overwrite,
                        open_folder: *open_folder,
                        log: log.then(|| log_path.clone()),
                        def: def.clone(),
                    };
                    action = Some(Box::new(move |app, ctx| app.bates_files(ctx, &job)));
                    outcome = Outcome::Close;
                } else if cancel {
                    outcome = Outcome::Close;
                }
            }
        }
        if matches!(outcome, Outcome::Close) {
            self.clear_previews();
            self.dialog_doc = None;
        }
        (outcome, action)
    }

    fn edit_existing_mark(&mut self, m: crate::marks::StoredMark) {
        let n = self.active_tab().map_or(0, |t| t.doc.page_count());
        let range = if m.def.pages.len() == n {
            PageRange::all()
        } else {
            PageRange {
                mode: crate::ui::pickers::RangeMode::Custom,
                text: pages_text(&m.def.pages),
                ..PageRange::default()
            }
        };
        let page = m.def.pages.first().copied().unwrap_or(0);
        self.open_marks(MarksDialog::Edit {
            def: m.def,
            editing: Some(m.id),
            range,
            page,
            file: None,
            file_page: 1,
            back_to_manage: true,
        });
    }

    /// "Quelle" of watermarks and backgrounds.
    fn source_editor(
        &mut self,
        ui: &mut Ui,
        def: &mut MarkDef,
        file: &mut Option<PathBuf>,
        file_page: &mut u32,
        _unit: crate::ui::pickers::Unit,
    ) {
        let de = self.md_de();
        section(ui, tr(de, "Quelle", "Source"), |ui| {
            if def.kind == Kind::Background {
                let is_color = matches!(def.source, Source::Color { .. });
                ui.horizontal(|ui| {
                    if ui.radio(is_color, tr(de, "Farbe:", "Colour:")).clicked() && !is_color {
                        def.source = Source::Color { color: crate::model::Rgba::rgb(255, 250, 225) };
                    }
                    if let Source::Color { color } = &mut def.source {
                        let mut c = to_color32(*color);
                        if ui.color_edit_button_srgba(&mut c).changed() {
                            *color = from_color32(c);
                        }
                    }
                });
                if is_color {
                    // Pages that cover their background (scans, own white
                    // area): the colour goes over the content, once
                    // switched on by itself.
                    let (covered, switched) = self.mark_covered;
                    if covered && !switched {
                        def.multiply = true;
                        self.mark_covered.1 = true;
                    }
                    ui.checkbox(
                        &mut def.multiply,
                        tr(de, "Über den Seiteninhalt legen (multiplizieren)", "Lay over the page content (multiply)"),
                    )
                    .on_hover_text(tr(
                        de,
                        "Für Seiten, die ihren Hintergrund verdecken, z. B. Scans: Weiß wird zur gewählten Farbe, Text und dunkle Stellen bleiben erhalten; Bilder werden mit eingefärbt.",
                        "For pages that cover their background, e.g. scans: white becomes the chosen colour, text and dark parts stay; pictures are tinted too.",
                    ));
                    if covered {
                        ui.label(
                            RichText::new(tr(
                                de,
                                "Diese Seite verdeckt ihren Hintergrund vollständig (z. B. ein Scan oder eine eigene weiße Fläche). Hinter dem Inhalt wäre die Farbe nicht zu sehen.",
                                "This page covers its background completely (e.g. a scan or its own white area). Behind the content the colour would not show.",
                            ))
                            .small()
                            .color(Color32::from_rgb(170, 90, 0)),
                        );
                    }
                }
                ui.horizontal(|ui| {
                    if ui.radio(!is_color, tr(de, "Bild:", "Image:")).clicked() && is_color {
                        def.source = Source::Image { name: String::new() };
                    }
                    self.file_picker(ui, de, def, file, file_page);
                });
                return;
            }
            let mut ty = if matches!(def.source, Source::Image { .. }) {
                WatermarkType::File
            } else {
                WatermarkType::Text
            };
            ui.horizontal(|ui| {
                ui.label(tr(de, "Typ:", "Type:"));
                let before = ty;
                egui::ComboBox::from_id_salt("wm-type")
                    .selected_text(if ty == WatermarkType::Text {
                        "Text"
                    } else {
                        tr(de, "Bild / PDF-Datei", "Image / PDF file")
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut ty, WatermarkType::Text, "Text");
                        ui.selectable_value(
                            &mut ty,
                            WatermarkType::File,
                            tr(de, "Bild / PDF-Datei", "Image / PDF file"),
                        );
                    });
                if ty != before {
                    def.source = match ty {
                        WatermarkType::Text => Source::Text {
                            text: tr(de, "ENTWURF", "DRAFT").into(),
                            font: FontSpec { size: 72.0, ..FontSpec::default() },
                            align: Align::Start,
                        },
                        WatermarkType::File => Source::Image { name: String::new() },
                    };
                }
            });
            match &mut def.source {
                Source::Text { text, font, align } => {
                    ui.horizontal(|ui| {
                        ui.label("Text:");
                        ui.add(egui::TextEdit::multiline(text).desired_rows(2).desired_width(300.0));
                        macro_menu(ui, de, text, &MACROS[..5]);
                    });
                    ui.horizontal(|ui| {
                        font_menu(ui, de, font);
                        ui.label(tr(de, "Ausrichtung:", "Alignment:"));
                        align_combo(ui, de, "wm-align", align, false);
                    });
                }
                _ => {
                    ui.horizontal(|ui| self.file_picker(ui, de, def, file, file_page));
                }
            }
        });
    }

    fn file_picker(
        &mut self,
        ui: &mut Ui,
        de: bool,
        def: &mut MarkDef,
        file: &mut Option<PathBuf>,
        file_page: &mut u32,
    ) {
        let shown = file.as_deref().map(file_name).or_else(|| match &def.source {
            Source::Image { name } if !name.is_empty() => Some(name.clone()),
            _ => None,
        });
        let label = shown.unwrap_or_else(|| {
            tr(de, "Klicken Sie hier, um die Datei auszuwählen…", "Click here to choose the file…").into()
        });
        let clicked = ui.button(label).clicked()
            | icon_button(ui, Icon::Open, tr(de, "Datei auswählen", "Choose file"), false, true).clicked();
        if clicked
            && let Some(p) = rfd::FileDialog::new()
                .add_filter(tr(de, "Bilder und PDF", "Images and PDF"), &IMAGE_EXT)
                .pick_file()
        {
            def.source = Source::Image { name: p.display().to_string() };
            *file = Some(p);
            *file_page = 1;
        }
        if file.as_ref().is_some_and(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf"))) {
            ui.label(tr(de, "Seite:", "Page:"));
            ui.add(egui::DragValue::new(file_page).range(1..=9999));
        }
    }

    fn appearance_editor(&mut self, ui: &mut Ui, def: &mut MarkDef) {
        let de = self.md_de();
        section(ui, tr(de, "Aussehen", "Appearance"), |ui| {
            egui::Grid::new("mark-look").num_columns(4).spacing(vec2(8.0, 6.0)).show(ui, |ui| {
                ui.label(tr(de, "Drehen:", "Rotate:"));
                ui.add(egui::DragValue::new(&mut def.rotation).range(-360.0..=360.0).suffix("°"));
                ui.label(tr(de, "Deckkraft:", "Opacity:"));
                let mut pct = def.opacity * 100.0;
                if ui.add(egui::DragValue::new(&mut pct).range(0.0..=100.0).suffix(" %")).changed() {
                    def.opacity = pct / 100.0;
                }
                ui.end_row();
                ui.label(tr(de, "Skalieren:", "Scale:"));
                let mut pct = def.scale * 100.0;
                if ui.add(egui::DragValue::new(&mut pct).range(1.0..=1000.0).suffix(" %")).changed() {
                    def.scale = pct / 100.0;
                }
                ui.checkbox(&mut def.relative, tr(de, "Relativ zur Seite", "Relative to page"));
                ui.label("");
                ui.end_row();
                ui.label(tr(de, "Sichtbar auf:", "Visible on:"));
                let names = [
                    (Visibility::Always, tr(de, "Ansicht, Drucken, Exportieren", "View, print, export")),
                    (Visibility::ViewOnly, tr(de, "Nur Ansicht", "View only")),
                    (Visibility::PrintOnly, tr(de, "Nur Drucken", "Print only")),
                ];
                egui::ComboBox::from_id_salt("mark-vis")
                    .selected_text(names.iter().find(|x| x.0 == def.visibility).map_or("", |x| x.1))
                    .width(220.0)
                    .show_ui(ui, |ui| {
                        for (v, name) in names {
                            ui.selectable_value(&mut def.visibility, v, name);
                        }
                    });
                ui.end_row();
            });
        });
    }

    /// Six text boxes, margins, font and (for Bates) the number format.
    fn header_footer_editor(&mut self, ui: &mut Ui, def: &mut MarkDef, unit: crate::ui::pickers::Unit) {
        let de = self.md_de();
        let is_bates = def.kind == Kind::Bates;
        let Source::HeaderFooter { texts, font, margins, shrink } = &mut def.source else { return };
        let labels = [
            tr(de, "Text links in der Kopfzeile:", "Header left:"),
            tr(de, "Text mittig in der Kopfzeile:", "Header centre:"),
            tr(de, "Text rechts in der Kopfzeile:", "Header right:"),
            tr(de, "Text links in der Fußzeile:", "Footer left:"),
            tr(de, "Text mittig in der Fußzeile:", "Footer centre:"),
            tr(de, "Text rechts in der Fußzeile:", "Footer right:"),
        ];
        egui::Grid::new("hf-texts").num_columns(3).spacing(vec2(10.0, 4.0)).show(ui, |ui| {
            for row in 0..2 {
                for col in 0..3 {
                    let i = row * 3 + col;
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(labels[i]);
                            macro_menu(ui, de, &mut texts[i], &MACROS);
                        });
                        ui.add(egui::TextEdit::multiline(&mut texts[i]).desired_rows(2).desired_width(240.0));
                    });
                }
                ui.end_row();
            }
        });
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
            ui.set_width(380.0);
            section(ui, tr(de, "Ränder", "Margins"), |ui| {
                crate::ui::pickers::unit_combo(ui, de, &mut self.settings.unit);
                egui::Grid::new("hf-margins").num_columns(4).spacing(vec2(8.0, 6.0)).show(ui, |ui| {
                    ui.label(tr(de, "Links:", "Left:"));
                    length(ui, &mut margins[0], unit, 0.0..=500.0);
                    ui.label(tr(de, "Oben:", "Top:"));
                    length(ui, &mut margins[2], unit, 0.0..=500.0);
                    ui.end_row();
                    ui.label(tr(de, "Rechts:", "Right:"));
                    length(ui, &mut margins[1], unit, 0.0..=500.0);
                    ui.label(tr(de, "Unten:", "Bottom:"));
                    length(ui, &mut margins[3], unit, 0.0..=500.0);
                    ui.end_row();
                });
            });
            });
            ui.vertical(|ui| {
            ui.set_width(380.0);
            section(ui, tr(de, "Aussehen", "Appearance"), |ui| {
                ui.checkbox(shrink, tr(de, "Seiteninhalte verkleinern, um das Überschreiben von Text und Bildern zu verhindern", "Shrink page contents so text and images are not overwritten"));
                font_menu(ui, de, font);
            });
            });
        });
        if is_bates {
            section(ui, tr(de, "Paginierungsnummer (%[Bates])", "Bates number (%[Bates])"), |ui| {
                let b: &mut BatesFormat = &mut def.bates;
                ui.horizontal(|ui| {
                    ui.label(tr(de, "Präfix:", "Prefix:"));
                    ui.add(egui::TextEdit::singleline(&mut b.prefix).desired_width(80.0));
                    ui.label(tr(de, "Startnummer:", "Start:"));
                    ui.add(egui::DragValue::new(&mut b.start).range(0..=999_999_999));
                    ui.label(tr(de, "Stellen:", "Digits:"));
                    ui.add(egui::DragValue::new(&mut b.digits).range(1..=12));
                    ui.label(tr(de, "Suffix:", "Suffix:"));
                    ui.add(egui::TextEdit::singleline(&mut b.suffix).desired_width(80.0));
                    ui.label(RichText::new(b.number(0)).weak());
                });
            });
        }
    }

    /// Live preview of a mark on `page`.
    fn mark_preview_ui(
        &mut self,
        ui: &mut Ui,
        def: &MarkDef,
        file: Option<&Path>,
        file_page: u32,
        page: usize,
        size: egui::Vec2,
    ) {
        let file = file.map(Path::to_path_buf).or_else(|| match &def.source {
            Source::Image { name } if !name.is_empty() => Some(PathBuf::from(name)),
            _ => None,
        });
        let key = format!("mark {page} {def:?} {file:?} {file_page}");
        if self.dialog_doc.as_ref().is_none_or(|(k, _)| *k != key) {
            let image = match (&def.source, &file) {
                (Source::Image { .. }, Some(p)) => load_image_data(p, file_page).ok(),
                _ => None,
            };
            let doc = match (&def.source, image) {
                (Source::Image { .. }, None) => self.active_tab().map(|t| {
                    t.doc.extract_pages_to_bytes(&[page]).and_then(|b| Document::from_bytes(b, "p.pdf", None))
                }),
                (_, image) => self.active_tab().map(|t| t.doc.mark_preview(page, def, image)),
            };
            if let Some(Ok(d)) = doc {
                // A colour background behind content that covers the page
                // hardly shows: compare small pictures.
                if def.kind == Kind::Background && matches!(def.source, Source::Color { .. }) && !def.multiply
                {
                    let small = |doc: &Document, p: usize| doc.render_page(p, 60, 85).ok().map(|i| i.rgba);
                    if let (Some(a), Some(b)) =
                        (self.active_tab().and_then(|t| small(&t.doc, page)), small(&d, 0))
                        && background_hidden(&a, &b)
                    {
                        self.mark_covered.0 = true;
                    }
                }
                self.dialog_doc = Some((key, d));
            }
        }
        let de = self.md_de();
        let opts = super::preview::PreviewOpts::new(de);
        let Some((_, d)) = &self.dialog_doc else { return };
        let Source::HeaderFooter { margins, .. } = &def.source else {
            doc_preview(ui, &mut self.preview[0], d, 0x3a7, 0, size, opts);
            return;
        };
        // Headers and footers are small on a whole page: the top and the
        // bottom of the page side by side, as wide as each half.
        let half = vec2((size.x - 8.0) / 2.0, size.y);
        let [top, bottom] = &mut self.preview;
        ui.horizontal(|ui| {
            for (cache, focus, name) in
                [(top, 0.0, tr(de, "Kopfzeile", "Header")), (bottom, 1.0, tr(de, "Fußzeile", "Footer"))]
            {
                let Some(area) = doc_preview(ui, cache, d, 0x3a7, 0, half, opts.focus(focus)) else {
                    continue;
                };
                margin_guides(&area, margins);
                // Name of the part on a small label in the corner.
                let bg = area.response.rect;
                let font = egui::FontId::proportional(12.0);
                let galley = ui.painter().layout_no_wrap(name.to_string(), font, Color32::from_gray(30));
                let r = egui::Rect::from_min_size(
                    bg.left_top() + vec2(4.0, 4.0),
                    galley.size() + vec2(10.0, 4.0),
                );
                area.painter.rect(
                    r,
                    3.0,
                    Color32::from_white_alpha(215),
                    Stroke::new(1.0, Color32::from_gray(110)),
                    egui::StrokeKind::Inside,
                );
                area.painter.galley(r.min + vec2(5.0, 2.0), galley, Color32::from_gray(30));
            }
        });
    }

    fn bates_files(&mut self, ctx: &Context, job: &BatesJob) {
        let de = self.md_de();
        let mut offset = 0u64;
        let mut written = Vec::new();
        let mut log = String::new();
        for path in &job.files {
            let result = (|| -> crate::Result<(PathBuf, String, String)> {
                let mut doc = Document::open(path, None)?;
                let n = doc.page_count();
                let mut def = job.def.clone();
                def.pages = (0..n).collect();
                doc.add_mark_with(&def, None, None, offset)?;
                let first = job.def.bates.number(offset);
                let last = job.def.bates.number(offset + n.saturating_sub(1) as u64);
                let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                let name = if job.range_names {
                    format!("{first}-{last}.pdf")
                } else {
                    super::page_dialogs::expand_file_name(
                        &job.pattern,
                        &stem,
                        1,
                        "",
                        &super::structure::today(false),
                    )
                };
                let name: String =
                    name.chars().map(|c| if "/\\:*?\"<>|".contains(c) { '_' } else { c }).collect();
                let target =
                    if job.overwrite { job.folder.join(&name) } else { unique_path(&job.folder, &name) };
                let bytes = doc.to_bytes()?;
                crate::document::write_atomic(&target, &bytes)?;
                offset += n as u64;
                Ok((target, first, last))
            })();
            match result {
                Ok((target, first, last)) => {
                    log.push_str(&format!("{} -> {} ({first} - {last})\n", path.display(), target.display()));
                    written.push(target);
                }
                Err(err) => {
                    log.push_str(&format!("{}: {err}\n", path.display()));
                    self.show_error(&err);
                }
            }
        }
        if let Some(log_path) = &job.log
            && let Err(err) = crate::document::write_atomic(log_path, log.as_bytes())
        {
            self.show_error(&err);
        }
        if job.open_folder
            && let Some(first) = written.first()
        {
            super::features::reveal_in_file_manager(first);
        }
        self.set_status(
            ctx,
            if de {
                format!("Nummerierte Dateien: {}", written.len())
            } else {
                format!("Numbered files: {}", written.len())
            },
        );
    }
}

/// Input of "Paginierung in Dateien hinzufügen".
pub struct BatesJob {
    files: Vec<PathBuf>,
    folder: PathBuf,
    range_names: bool,
    pattern: String,
    overwrite: bool,
    open_folder: bool,
    log: Option<PathBuf>,
    def: MarkDef,
}

/// Image data of an image file or a page of a PDF file.
fn load_image_data(path: &Path, page: u32) -> crate::Result<ImageData> {
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")) {
        let src = crate::import::Source::load(&crate::document::read_pdf_file(path)?, None)?;
        let p = (page.max(1) as usize - 1).min(src.page_count().saturating_sub(1));
        Ok(ImageData::PdfPage(Box::new(src), p))
    } else {
        Ok(ImageData::Pixels(crate::document::load_image(path)?))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn hidden_background() {
        let white = [255u8, 255, 255, 255];
        let tint = [255u8, 200, 120, 255];
        let page = [white, white, white, white].concat();
        assert!(super::background_hidden(&page, &page));
        assert!(super::background_hidden(&page, &[tint, white, white, white].concat()));
        assert!(!super::background_hidden(&page, &[tint, tint, white, white].concat()));
    }
}
