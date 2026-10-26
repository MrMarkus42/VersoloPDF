//! The page dialogs of "Dokumentaufbau":
//! insert (file, blank, clipboard, images,
//! text, RTF), delete (pages, blank pages), extract, move, duplicate,
//! replace, rotate, reverse, normalize and find duplicates.

use std::path::{Path, PathBuf};

use egui::{Context, RichText, Ui, vec2};
use serde::{Deserialize, Serialize};

use super::commands::Cmd;
use super::dialogs::{Action, Outcome};
use super::preview::{doc_preview, page_strip};
use super::{Dialog, MrPdfApp, PaperChoice};
use crate::Document;
use crate::compose::{
    Align, Fit, ImageLayout, JoinMode, PageSizeMode, ParagraphMode, TextEncoding, TextFont, TextLayout,
};
use crate::document::{BookmarkCopy, ExtractOptions, Handling};
use crate::pdf_edit::RgbaImage;
use crate::ui::i18n::Language;
use crate::ui::icons::Icon;
use crate::ui::pickers::{
    PageRange, PresetAction, RangeMode, Target, TargetPage, Unit, length, preset_combo, section, segmented,
    unit_combo,
};
use crate::ui::widgets::{icon_button, small_button};

/// Image files accepted by "Bilder einfügen".
const IMAGE_EXT: [&str; 7] = ["png", "jpg", "jpeg", "gif", "bmp", "tif", "tiff"];
/// Upper bounds for file lists and text files.
const MAX_FILES: usize = 1000;
const MAX_TEXT_FILE: u64 = 32 * 1024 * 1024;

fn tr(de: bool, german: &'static str, english: &'static str) -> &'static str {
    if de { german } else { english }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Orientation {
    #[default]
    Keep,
    Portrait,
    Landscape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BlankSize {
    /// Size of the page next to the insert position.
    AtInsert,
    /// Size of a chosen page.
    #[default]
    FromPage,
    Standard,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BlankForm {
    pub size: BlankSize,
    pub ref_page: TargetPage,
    pub ref_custom: u32,
    pub paper: PaperChoice,
    /// Custom size in points.
    pub custom: (f32, f32),
    pub orientation: Orientation,
    pub count: u32,
    pub target: Target,
}

impl Default for BlankForm {
    fn default() -> Self {
        Self {
            size: BlankSize::FromPage,
            ref_page: TargetPage::Current,
            ref_custom: 1,
            paper: PaperChoice::A4,
            custom: (595.28, 841.89),
            orientation: Orientation::Keep,
            count: 1,
            target: Target::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ExtractMode {
    /// All pages into one file.
    #[default]
    OneFile,
    /// One file per page.
    PerPage,
    /// Open as new, unsaved document.
    NewDocument,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExtractForm {
    pub range: PageRange,
    pub mode: ExtractMode,
    pub file_name: String,
    pub folder: PathBuf,
    pub open_folder: bool,
    pub open_first: bool,
    pub delete_after: bool,
    pub options: ExtractOptions,
}

impl Default for ExtractForm {
    fn default() -> Self {
        Self {
            range: PageRange::current(),
            mode: ExtractMode::OneFile,
            file_name: "%[FileName]_%[Page]".into(),
            folder: PathBuf::new(),
            open_folder: true,
            open_first: false,
            delete_after: false,
            options: ExtractOptions::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RotateDir {
    #[default]
    Clockwise,
    CounterClockwise,
    Half,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum OrientationFilter {
    #[default]
    Any,
    Portrait,
    Landscape,
}

/// Which page dialog is open, with its input.
#[derive(Debug, Clone, PartialEq)]
pub enum PageDialog {
    Insert {
        path: Option<PathBuf>,
        pages: usize,
        range: PageRange,
        target: Target,
        error: Option<String>,
    },
    Blank(BlankForm),
    Clipboard {
        image: bool,
        target: Target,
    },
    Images {
        files: Vec<PathBuf>,
        selected: Option<usize>,
        target: Target,
        layout: ImageLayout,
    },
    Text {
        rtf: bool,
        files: Vec<PathBuf>,
        selected: Option<usize>,
        target: Target,
        layout: TextLayout,
    },
    Delete {
        range: PageRange,
        bookmarks: bool,
    },
    DeleteBlank {
        ignore_annots: bool,
        ignore_noise: bool,
        tolerance: f32,
        range: PageRange,
        review: bool,
    },
    /// Confirmation of found blank pages (page, delete?).
    BlankReview {
        pages: Vec<(usize, bool)>,
    },
    Extract(ExtractForm),
    Move {
        range: PageRange,
        target: Target,
    },
    Duplicate {
        range: PageRange,
        target: Target,
        copies: u32,
        collate: bool,
        comments: bool,
        forms: bool,
    },
    Replace {
        targets: PageRange,
        keep: bool,
        path: Option<PathBuf>,
        sources: PageRange,
        fit: bool,
        scale: bool,
        preview_target: usize,
        preview_source: usize,
    },
    Rotate {
        dir: RotateDir,
        range: PageRange,
        filter: OrientationFilter,
    },
    Reverse {
        range: PageRange,
    },
    Normalize {
        range: PageRange,
    },
    Duplicates {
        range: PageRange,
        ignore_orientation: bool,
        ignore_annots: bool,
    },
}

/// Open dialog plus the state of its "Einstellungen" view.
#[derive(Debug, Clone, PartialEq)]
pub struct PageDialogState {
    pub dialog: PageDialog,
    pub settings_open: bool,
    pub settings_page: usize,
    /// Copy restored when the settings view is cancelled.
    backup: Option<Box<PageDialog>>,
    pub preset_name: String,
}

impl PageDialogState {
    fn new(dialog: PageDialog) -> Self {
        Self { dialog, settings_open: false, settings_page: 0, backup: None, preset_name: String::new() }
    }
}

/// Footer with optional "Einstellungen" button; returns (ok, cancel, settings).
fn footer(
    ui: &mut Ui,
    de: bool,
    ok_enabled: bool,
    with_settings: bool,
    ok_label: &str,
) -> (bool, bool, bool) {
    let mut result = (false, false, false);
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if with_settings {
            result.2 =
                small_button(ui, Icon::Gear, tr(de, "Einstellungen", "Settings"), false, true).clicked();
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            result.1 = ui.button(tr(de, "Abbrechen", "Cancel")).clicked();
            result.0 =
                ui.add_enabled(ok_enabled, egui::Button::new(RichText::new(ok_label).strong())).clicked();
        });
    });
    let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
    (result.0, result.1 || esc, result.2)
}

fn paper_label(p: PaperChoice, de: bool) -> String {
    match p.size() {
        Some((w, h)) => {
            let name = match p {
                PaperChoice::A4 => "A4",
                PaperChoice::A3 => "A3",
                PaperChoice::A5 => "A5",
                PaperChoice::Letter => "Letter",
                PaperChoice::Legal => "Legal",
                PaperChoice::SameAsCurrent => "",
            };
            format!("{name} ({:.0} x {:.0} mm)", w / Unit::Mm.points(), h / Unit::Mm.points())
        }
        None => tr(de, "Wie aktuelle Seite", "Same as current").into(),
    }
}

fn paper_combo(ui: &mut Ui, de: bool, id: &str, paper: &mut PaperChoice) {
    egui::ComboBox::from_id_salt(id).selected_text(paper_label(*paper, de)).width(190.0).show_ui(ui, |ui| {
        for p in [PaperChoice::A4, PaperChoice::A3, PaperChoice::A5, PaperChoice::Letter, PaperChoice::Legal]
        {
            ui.selectable_value(paper, p, paper_label(p, de));
        }
    });
}

fn size_label(w: f32, h: f32, unit: Unit) -> String {
    let f = |v: f32| {
        let x = v / unit.points();
        if unit == Unit::Pt { format!("{x:.0}") } else { format!("{x:.1}").replace(".0", "") }
    };
    format!("({} x {} {})", f(w), f(h), unit.suffix())
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Replaces macros in output file names: %[FileName], %[Page], %[Pages],
/// %[Date]. Characters not allowed in file names are replaced.
pub fn expand_file_name(pattern: &str, stem: &str, page: usize, pages: &str, date: &str) -> String {
    let name = pattern
        .replace("%[FileName]", stem)
        .replace("%[Page]", &page.to_string())
        .replace("%[Pages]", pages)
        .replace("%[Date]", date);
    let clean: String =
        name.chars().map(|c| if "/\\:*?\"<>|".contains(c) || c.is_control() { '_' } else { c }).collect();
    let clean = clean.trim().trim_end_matches(".pdf").trim_end_matches(".PDF").to_string();
    let clean = if clean.is_empty() { "Seiten" } else { clean.as_str() };
    format!("{clean}.pdf")
}

/// A path that does not exist yet ("name (2).pdf" …).
fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    if !path.exists() {
        return path;
    }
    let stem = name.trim_end_matches(".pdf");
    (2..10_000).map(|i| dir.join(format!("{stem} ({i}).pdf"))).find(|p| !p.exists()).unwrap_or(path)
}

/// Macro menu "[···] ▾" next to file name and caption fields.
fn macro_menu(ui: &mut Ui, de: bool, text: &mut String, macros: &[(&str, &'static str, &'static str)]) {
    ui.menu_button("[···]", |ui| {
        for (code, german, english) in macros {
            if ui.button(format!("{} – {code}", tr(de, german, english))).clicked() {
                text.push_str(code);
                ui.close();
            }
        }
    })
    .response
    .on_hover_text(tr(de, "Platzhalter einfügen", "Insert placeholder"));
}

/// Toolbar + table of a file list dialog ("Dateien hinzufügen…").
fn file_list(
    ui: &mut Ui,
    de: bool,
    files: &mut Vec<PathBuf>,
    selected: &mut Option<usize>,
    extensions: &[&str],
    filter_name: &str,
) {
    ui.horizontal(|ui| {
        if small_button(ui, Icon::InsertFile, tr(de, "Dateien hinzufügen…", "Add files…"), false, true)
            .clicked()
            && let Some(paths) = rfd::FileDialog::new().add_filter(filter_name, extensions).pick_files()
        {
            files.extend(paths);
        }
        if small_button(ui, Icon::Open, tr(de, "Ordner hinzufügen…", "Add folder…"), false, true).clicked()
            && let Some(dir) = rfd::FileDialog::new().pick_folder()
            && let Ok(entries) = std::fs::read_dir(dir)
        {
            let mut found: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.is_file()
                        && p.extension()
                            .and_then(|e| e.to_str())
                            .is_some_and(|e| extensions.iter().any(|x| x.eq_ignore_ascii_case(e)))
                })
                .collect();
            found.sort();
            files.extend(found);
        }
        ui.separator();
        let sel = selected.filter(|i| *i < files.len());
        let n = files.len();
        for (label, tip, target) in [
            (Icon::First, tr(de, "Ganz nach oben", "To top"), sel.map(|_| 0)),
            (Icon::MoveUp, tr(de, "Nach oben", "Up"), sel.map(|i| i.saturating_sub(1))),
            (Icon::MoveDown, tr(de, "Nach unten", "Down"), sel.map(|i| (i + 1).min(n.saturating_sub(1)))),
            (Icon::Last, tr(de, "Ganz nach unten", "To bottom"), sel.map(|_| n.saturating_sub(1))),
        ] {
            if icon_button(ui, label, tip, false, target.is_some()).clicked()
                && let (Some(from), Some(to)) = (sel, target)
            {
                let item = files.remove(from);
                files.insert(to, item);
                *selected = Some(to);
            }
        }
        if small_button(ui, Icon::Delete, tr(de, "Entfernen", "Remove"), false, sel.is_some()).clicked()
            && let Some(i) = sel
        {
            files.remove(i);
            *selected = None;
        }
    });
    // Files dropped onto the window are added as well.
    let dropped: Vec<PathBuf> = ui.ctx().input(|i| {
        i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| p.is_absolute()).collect()
    });
    for p in dropped {
        if p.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| extensions.iter().any(|x| x.eq_ignore_ascii_case(e)))
        {
            files.push(p);
        }
    }
    files.truncate(MAX_FILES);
    egui::Frame::new().stroke(ui.visuals().widgets.noninteractive.bg_stroke).show(ui, |ui| {
        ui.set_min_size(vec2(ui.available_width(), 170.0));
        egui::ScrollArea::vertical().max_height(170.0).show(ui, |ui| {
            if files.is_empty() {
                ui.add_space(60.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new(tr(
                            de,
                            "Fügen Sie neue Dateien hinzu, indem Sie sie hier hinziehen. Sie können die Reihenfolge beliebig ändern.",
                            "Add new files by dragging them here. You can change the order as you like.",
                        ))
                        .weak(),
                    );
                });
                return;
            }
            egui::Grid::new("file-list").num_columns(5).striped(true).spacing(vec2(12.0, 3.0)).show(ui, |ui| {
                for h in [tr(de, "Titel", "Title"), tr(de, "Erw.", "Ext."), tr(de, "Ort", "Location"), tr(de, "Größe", "Size"), tr(de, "Geändert", "Modified")] {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                for (i, path) in files.iter().enumerate() {
                    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                    if ui.selectable_label(*selected == Some(i), stem).clicked() {
                        *selected = Some(i);
                    }
                    ui.label(path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default());
                    ui.label(path.parent().map(|p| p.display().to_string()).unwrap_or_default());
                    let meta = std::fs::metadata(path).ok();
                    ui.label(meta.as_ref().map(|m| format!("{} KB", m.len().div_ceil(1024))).unwrap_or_default());
                    let modified = meta
                        .and_then(|m| m.modified().ok())
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| crate::pdf_edit::format_pdf_date(&crate::pdf_edit::pdf_date(d.as_secs() as i64)))
                        .unwrap_or_default();
                    ui.label(modified);
                    ui.end_row();
                }
            });
        });
    });
}

/// Left list of settings pages; returns the chosen page.
fn settings_nav(ui: &mut Ui, page: &mut usize, names: &[&str]) {
    ui.vertical(|ui| {
        ui.set_width(170.0);
        for (i, name) in names.iter().enumerate() {
            if ui.selectable_label(*page == i, *name).clicked() {
                *page = i;
            }
        }
    });
}

fn margins_ui(ui: &mut Ui, de: bool, m: &mut [f32; 4], unit: Unit) {
    section(ui, tr(de, "Ränder", "Margins"), |ui| {
        egui::Grid::new("margins4").num_columns(4).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
            ui.label(tr(de, "Links:", "Left:"));
            length(ui, &mut m[0], unit, 0.0..=1000.0);
            ui.label(tr(de, "Rechts:", "Right:"));
            length(ui, &mut m[1], unit, 0.0..=1000.0);
            ui.end_row();
            ui.label(tr(de, "Oben:", "Top:"));
            length(ui, &mut m[2], unit, 0.0..=1000.0);
            ui.label(tr(de, "Unten:", "Bottom:"));
            length(ui, &mut m[3], unit, 0.0..=1000.0);
            ui.end_row();
        });
    });
}

fn align_combo(ui: &mut Ui, de: bool, id: &str, value: &mut Align, vertical: bool) {
    let names = if vertical {
        [tr(de, "Oben", "Top"), tr(de, "Mittig", "Middle"), tr(de, "Unten", "Bottom")]
    } else {
        [tr(de, "Links", "Left"), tr(de, "Zentriert", "Centred"), tr(de, "Rechts", "Right")]
    };
    let idx = |a: Align| match a {
        Align::Start => 0,
        Align::Center => 1,
        Align::End => 2,
    };
    egui::ComboBox::from_id_salt(id).selected_text(names[idx(*value)]).show_ui(ui, |ui| {
        for (a, n) in [Align::Start, Align::Center, Align::End].into_iter().zip(names) {
            ui.selectable_value(value, a, n);
        }
    });
}

fn decode_files(
    files: &[PathBuf],
    rtf: bool,
    encoding: TextEncoding,
) -> crate::Result<Vec<(String, String)>> {
    let mut docs = Vec::new();
    for path in files {
        let len = std::fs::metadata(path).map_err(|e| crate::Error::io(path, e))?.len();
        if len > MAX_TEXT_FILE {
            return Err(crate::Error::FileTooLarge { size: len, limit: MAX_TEXT_FILE });
        }
        let bytes = std::fs::read(path).map_err(|e| crate::Error::io(path, e))?;
        let text = if rtf {
            crate::compose::rtf_to_text(&bytes)?
        } else {
            crate::compose::decode_text(&bytes, encoding)
        };
        docs.push((file_name(path), text));
    }
    Ok(docs)
}

fn load_images(files: &[PathBuf]) -> crate::Result<Vec<(String, RgbaImage)>> {
    files.iter().map(|p| Ok((file_name(p), crate::document::load_image(p)?))).collect()
}

/// Reads text and image from the system clipboard.
pub fn read_clipboard() -> (Option<String>, Option<RgbaImage>) {
    let Ok(mut clip) = arboard::Clipboard::new() else { return (None, None) };
    let text =
        clip.get_text().ok().filter(|t| !t.trim().is_empty()).map(|t| t.chars().take(5_000_000).collect());
    let image = clip.get_image().ok().and_then(|img| {
        let (w, h) = (img.width as u32, img.height as u32);
        (w > 0 && h > 0 && w <= 12_000 && h <= 12_000 && img.bytes.len() == (w * h * 4) as usize)
            .then(|| RgbaImage { width: w, height: h, rgba: img.bytes.into_owned() })
    });
    (text, image)
}

impl MrPdfApp {
    fn pd_de(&self) -> bool {
        self.settings.language == Language::German
    }

    fn doc_info(&self) -> (usize, usize) {
        self.active_tab().map_or((0, 0), |t| (t.doc.page_count(), t.current_page))
    }

    fn open_pages(&mut self, dialog: PageDialog) {
        self.clear_previews();
        self.dialog_doc = None;
        self.dialog = Some(Dialog::Pages(Box::new(PageDialogState::new(dialog))));
    }

    /// Opens the page dialog of `cmd`. Returns false if `cmd` has none.
    pub(super) fn run_page_dialog(&mut self, cmd: Cmd) -> bool {
        let (n, cur) = self.doc_info();
        let dialog = match cmd {
            Cmd::InsertFromFile => PageDialog::Insert {
                path: None,
                pages: 0,
                range: PageRange::all(),
                target: Target::after_current(),
                error: None,
            },
            Cmd::InsertBlank => {
                PageDialog::Blank(BlankForm { target: Target::after_current(), ..BlankForm::default() })
            }
            Cmd::InsertClipboard => {
                let (text, image) = read_clipboard();
                if text.is_none() && image.is_none() {
                    let de = self.pd_de();
                    self.dialog = Some(Dialog::Message {
                        title: cmd.label(self.settings.language).into(),
                        text: tr(
                            de,
                            "Die Zwischenablage enthält keinen Text und kein Bild.",
                            "The clipboard contains no text and no image.",
                        )
                        .into(),
                    });
                    return true;
                }
                let image_first = image.is_some() && text.is_none();
                self.clipboard = (text, image);
                PageDialog::Clipboard { image: image_first, target: Target::after_current() }
            }
            Cmd::InsertImages => PageDialog::Images {
                files: Vec::new(),
                selected: None,
                target: Target::after_current(),
                layout: ImageLayout::default(),
            },
            Cmd::InsertText | Cmd::InsertRtf => PageDialog::Text {
                rtf: cmd == Cmd::InsertRtf,
                files: Vec::new(),
                selected: None,
                target: Target::after_current(),
                layout: TextLayout::default(),
            },
            Cmd::DeletePages => PageDialog::Delete { range: PageRange::current(), bookmarks: false },
            Cmd::DeleteBlankPages => PageDialog::DeleteBlank {
                ignore_annots: false,
                ignore_noise: false,
                tolerance: 10.0,
                range: PageRange::all(),
                review: false,
            },
            Cmd::ExtractPages => {
                let folder = self
                    .active_tab()
                    .and_then(|t| t.doc.path())
                    .and_then(|p| p.parent())
                    .map(Path::to_path_buf)
                    .unwrap_or_default();
                PageDialog::Extract(ExtractForm { folder, ..ExtractForm::default() })
            }
            Cmd::MovePages => PageDialog::Move {
                range: PageRange::current(),
                target: Target { after: true, page: TargetPage::Last, custom: n as u32 },
            },
            Cmd::DuplicatePages => PageDialog::Duplicate {
                range: PageRange::current(),
                target: Target::default(),
                copies: 1,
                collate: true,
                comments: true,
                forms: true,
            },
            Cmd::ReplacePages => PageDialog::Replace {
                targets: PageRange::current(),
                keep: true,
                path: None,
                sources: PageRange::all(),
                fit: false,
                scale: true,
                preview_target: cur,
                preview_source: 0,
            },
            Cmd::RotatePages => PageDialog::Rotate {
                dir: RotateDir::Clockwise,
                range: PageRange::all(),
                filter: OrientationFilter::Any,
            },
            Cmd::ReversePages => PageDialog::Reverse { range: PageRange::all() },
            Cmd::NormalizePages => PageDialog::Normalize { range: PageRange::all() },
            Cmd::FindDuplicatePages => PageDialog::Duplicates {
                range: PageRange::all(),
                ignore_orientation: true,
                ignore_annots: false,
            },
            _ => return false,
        };
        self.open_pages(dialog);
        true
    }

    /// Draws a page dialog; returns the outcome and the work to do.
    pub(super) fn page_dialog(
        &mut self,
        ui: &mut Ui,
        state: &mut PageDialogState,
    ) -> (Outcome, Option<Action>) {
        let de = self.pd_de();
        let lang = self.settings.language;
        let (n, cur) = self.doc_info();
        let unit = self.settings.unit;
        let mut outcome = Outcome::Keep;
        let mut action: Option<Action> = None;
        let close = |outcome: &mut Outcome| *outcome = Outcome::Close;

        // The settings view replaces the dialog body until OK/Cancel.
        if state.settings_open {
            let (ok, cancel) = self.settings_view(ui, state);
            if ok {
                state.settings_open = false;
                state.backup = None;
            } else if cancel {
                if let Some(b) = state.backup.take() {
                    state.dialog = *b;
                }
                state.settings_open = false;
            }
            return (Outcome::Keep, None);
        }

        let mut open_settings = false;
        match &mut state.dialog {
            PageDialog::Insert { path, pages, range, target, error } => {
                ui.heading(Cmd::InsertFromFile.label(lang));
                section(ui, tr(de, "Quelle", "Source"), |ui| {
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
                        let mut chosen: Option<PathBuf> = None;
                        egui::ComboBox::from_id_salt("insert-source")
                            .selected_text(shown)
                            .width(300.0)
                            .show_ui(ui, |ui| {
                                for recent in
                                    self.settings.recent_files.iter().filter(|p| {
                                        p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
                                    })
                                {
                                    if ui
                                        .selectable_label(
                                            path.as_ref() == Some(recent),
                                            recent.display().to_string(),
                                        )
                                        .clicked()
                                    {
                                        chosen = Some(recent.clone());
                                    }
                                }
                            });
                        if icon_button(ui, Icon::Open, tr(de, "Datei auswählen", "Choose file"), false, true)
                            .clicked()
                        {
                            chosen = rfd::FileDialog::new().add_filter("PDF", &["pdf", "PDF"]).pick_file();
                        }
                        if let Some(p) = chosen {
                            match Document::file_page_count(&p, None) {
                                Ok(count) => {
                                    *pages = count;
                                    *error = None;
                                }
                                Err(e) => {
                                    *pages = 0;
                                    *error = Some(e.to_string());
                                }
                            }
                            *path = Some(p);
                        }
                    });
                    if let Some(e) = error {
                        ui.colored_label(egui::Color32::from_rgb(200, 40, 40), e.as_str());
                    }
                });
                range.ui(ui, de, *pages, 0, &[RangeMode::All, RangeMode::Custom]);
                target.ui(ui, de, n, cur);
                let selection = range.resolve(*pages, 0);
                let (ok, cancel, _) = footer(ui, de, path.is_some() && selection.is_some(), false, "OK");
                if ok && let (Some(p), Some(sel)) = (path.clone(), selection) {
                    let at = target.insert_index(n, cur);
                    action = Some(Box::new(move |app, ctx| {
                        app.edit(ctx, |tab| tab.doc.insert_pages_selected(at, &p, None, &sel).map(|_| ()))
                    }));
                    close(&mut outcome);
                } else if cancel {
                    close(&mut outcome);
                }
            }
            PageDialog::Blank(form) => {
                ui.horizontal(|ui| {
                    ui.heading(Cmd::InsertBlank.label(lang));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.horizontal(|ui| unit_combo(ui, de, &mut self.settings.unit));
                    });
                });
                let size = self.blank_size(form, n, cur);
                section(ui, tr(de, "Seiten", "Pages"), |ui| {
                    egui::Grid::new("blank").num_columns(2).spacing(vec2(8.0, 6.0)).show(ui, |ui| {
                        ui.label(tr(de, "Typ:", "Type:"));
                        segmented(
                            ui,
                            &mut form.size,
                            &[
                                (
                                    BlankSize::AtInsert,
                                    tr(de, "Nach Position des Einfügens", "By insert position").into(),
                                ),
                                (BlankSize::FromPage, tr(de, "Nach Seite", "By page").into()),
                                (BlankSize::Standard, "Standard".into()),
                                (BlankSize::Custom, tr(de, "Eigene", "Custom").into()),
                            ],
                        );
                        ui.end_row();
                        match form.size {
                            BlankSize::FromPage => {
                                ui.label(tr(de, "Seite:", "Page:"));
                                ui.horizontal(|ui| {
                                    segmented(
                                        ui,
                                        &mut form.ref_page,
                                        &[
                                            (TargetPage::First, tr(de, "Erste", "First").into()),
                                            (
                                                TargetPage::Current,
                                                format!("{}: {}", tr(de, "Aktuell", "Current"), cur + 1),
                                            ),
                                            (
                                                TargetPage::Last,
                                                format!("{}: {}", tr(de, "Letzte", "Last"), n),
                                            ),
                                            (TargetPage::Custom, tr(de, "Eigene", "Custom").into()),
                                        ],
                                    );
                                    if form.ref_page == TargetPage::Custom {
                                        ui.add(
                                            egui::DragValue::new(&mut form.ref_custom)
                                                .range(1..=n.max(1) as u32),
                                        );
                                    }
                                    ui.label(size_label(size.0, size.1, unit));
                                });
                                ui.end_row();
                            }
                            BlankSize::Standard => {
                                ui.label(tr(de, "Größe:", "Size:"));
                                paper_combo(ui, de, "blank-paper", &mut form.paper);
                                ui.end_row();
                            }
                            BlankSize::Custom => {
                                ui.label(tr(de, "Größe:", "Size:"));
                                ui.horizontal(|ui| {
                                    length(ui, &mut form.custom.0, unit, 10.0..=14_400.0);
                                    ui.label("x");
                                    length(ui, &mut form.custom.1, unit, 10.0..=14_400.0);
                                });
                                ui.end_row();
                            }
                            BlankSize::AtInsert => {
                                ui.label("");
                                ui.label(size_label(size.0, size.1, unit));
                                ui.end_row();
                            }
                        }
                        ui.label(tr(de, "Ausrichtung:", "Orientation:"));
                        segmented(
                            ui,
                            &mut form.orientation,
                            &[
                                (Orientation::Keep, tr(de, "Beibehalten", "Keep").into()),
                                (Orientation::Portrait, tr(de, "Hochformat", "Portrait").into()),
                                (Orientation::Landscape, tr(de, "Querformat", "Landscape").into()),
                            ],
                        );
                        ui.end_row();
                        ui.label(tr(de, "Anzahl:", "Count:"));
                        ui.add(egui::DragValue::new(&mut form.count).range(1..=1000));
                        ui.end_row();
                    });
                });
                form.target.ui(ui, de, n, cur);
                let mut name = std::mem::take(&mut state.preset_name);
                let preset = ui
                    .horizontal(|ui| {
                        preset_combo(ui, de, "insert-blank", form, &mut self.settings, &mut name)
                    })
                    .inner;
                state.preset_name = name;
                let _ = matches!(preset, PresetAction::None);
                let (ok, cancel, _) = footer(ui, de, n > 0, false, "OK");
                if ok {
                    let (w, h) = self.blank_size(form, n, cur);
                    let at = form.target.insert_index(n, cur);
                    let count = form.count as usize;
                    action = Some(Box::new(move |app, ctx| {
                        app.edit(ctx, |tab| tab.doc.insert_blank_pages(at, w, h, count))
                    }));
                    close(&mut outcome);
                } else if cancel {
                    close(&mut outcome);
                }
            }
            PageDialog::Clipboard { image, target } => {
                ui.heading(tr(
                    de,
                    "Einfügen von Seiten aus der Zwischenablage",
                    "Insert pages from the clipboard",
                ));
                ui.label(tr(de, "Datentypen in der Zwischenablage:", "Data types in the clipboard:"));
                let has_text = self.clipboard.0.is_some();
                let has_image = self.clipboard.1.is_some();
                egui::Frame::new().stroke(ui.visuals().widgets.noninteractive.bg_stroke).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    if has_text && ui.selectable_label(!*image, tr(de, "Text", "Text")).clicked() {
                        *image = false;
                    }
                    if has_image && ui.selectable_label(*image, tr(de, "Bild", "Image")).clicked() {
                        *image = true;
                    }
                });
                // Preview of the generated page.
                let key = format!("clip-{image}");
                if self.dialog_doc.as_ref().is_none_or(|(k, _)| *k != key) {
                    let doc = self
                        .clipboard_pdf(*image)
                        .and_then(|b| Document::from_bytes(b, "clip.pdf", None))
                        .ok();
                    self.dialog_doc = doc.map(|d| (key.clone(), d));
                }
                ui.label(RichText::new(tr(de, "Vorschau: 1 von 1", "Preview: 1 of 1")).weak());
                if let Some((_, doc)) = &self.dialog_doc {
                    doc_preview(ui, &mut self.preview[0], doc, 0xc1b, 0, vec2(400.0, 300.0));
                }
                target.ui(ui, de, n, cur);
                let (ok, cancel, _) = footer(ui, de, self.dialog_doc.is_some(), false, "OK");
                if ok {
                    let at = target.insert_index(n, cur);
                    let img = *image;
                    action = Some(Box::new(move |app, ctx| {
                        let bytes = app.clipboard_pdf(img);
                        app.edit(ctx, |tab| tab.doc.insert_pdf_bytes(at, bytes?, &[]).map(|_| ()));
                    }));
                    close(&mut outcome);
                } else if cancel {
                    close(&mut outcome);
                }
            }
            PageDialog::Images { files, selected, target, layout } => {
                ui.heading(tr(de, "Bilder in PDF umwandeln", "Convert images to PDF"));
                file_list(ui, de, files, selected, &IMAGE_EXT, tr(de, "Bilder", "Images"));
                target.ui(ui, de, n, cur);
                let (ok, cancel, settings) = footer(ui, de, !files.is_empty(), true, "OK");
                open_settings = settings;
                if ok {
                    let files = files.clone();
                    let at = target.insert_index(n, cur);
                    let layout = layout.clone();
                    action = Some(Box::new(move |app, ctx| {
                        let result = load_images(&files)
                            .and_then(|imgs| crate::compose::images_to_pdf(&imgs, &layout));
                        app.edit(ctx, |tab| {
                            let (pdf, marks) = result?;
                            tab.doc.insert_pdf_bytes(at, pdf, &marks).map(|_| ())
                        });
                    }));
                    close(&mut outcome);
                } else if cancel {
                    close(&mut outcome);
                }
            }
            PageDialog::Text { rtf, files, selected, target, layout } => {
                let rtf = *rtf;
                ui.heading(if rtf {
                    tr(de, "RTF in PDF umwandeln", "Convert RTF to PDF")
                } else {
                    tr(de, "Textdateien in PDF umwandeln", "Convert text files to PDF")
                });
                let (ext, filter): (&[&str], &str) = if rtf {
                    (&["rtf"], "RTF")
                } else {
                    (&["txt", "text", "log", "csv", "md"], tr(de, "Textdateien", "Text files"))
                };
                file_list(ui, de, files, selected, ext, filter);
                if !rtf {
                    section(ui, tr(de, "Eigenschaften der Quelldateien", "Source file properties"), |ui| {
                        egui::Grid::new("text-props").num_columns(2).spacing(vec2(8.0, 6.0)).show(ui, |ui| {
                            ui.label(tr(de, "Text-Encoding:", "Text encoding:"));
                            egui::ComboBox::from_id_salt("enc")
                                .selected_text(match layout.encoding {
                                    TextEncoding::Auto => {
                                        tr(de, "Automatisch erkennen", "Detect automatically")
                                    }
                                    TextEncoding::Utf8 => "UTF-8",
                                    TextEncoding::Windows1252 => "Windows-1252 (Westeuropa)",
                                    TextEncoding::Utf16Le => "UTF-16 LE",
                                    TextEncoding::Utf16Be => "UTF-16 BE",
                                })
                                .width(240.0)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut layout.encoding,
                                        TextEncoding::Auto,
                                        tr(de, "Automatisch erkennen", "Detect automatically"),
                                    );
                                    ui.selectable_value(&mut layout.encoding, TextEncoding::Utf8, "UTF-8");
                                    ui.selectable_value(
                                        &mut layout.encoding,
                                        TextEncoding::Windows1252,
                                        "Windows-1252 (Westeuropa)",
                                    );
                                    ui.selectable_value(
                                        &mut layout.encoding,
                                        TextEncoding::Utf16Le,
                                        "UTF-16 LE",
                                    );
                                    ui.selectable_value(
                                        &mut layout.encoding,
                                        TextEncoding::Utf16Be,
                                        "UTF-16 BE",
                                    );
                                });
                            ui.end_row();
                            ui.label(tr(de, "Modus für neue Absätze:", "New paragraph mode:"));
                            egui::ComboBox::from_id_salt("para")
                                .selected_text(match layout.paragraphs {
                                    ParagraphMode::EachLine => {
                                        tr(de, "Jeder Zeilenumbruch", "Every line break")
                                    }
                                    ParagraphMode::EmptyLine => {
                                        tr(de, "Nur bei Leerzeilen", "Only at empty lines")
                                    }
                                })
                                .width(240.0)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut layout.paragraphs,
                                        ParagraphMode::EachLine,
                                        tr(de, "Jeder Zeilenumbruch", "Every line break"),
                                    );
                                    ui.selectable_value(
                                        &mut layout.paragraphs,
                                        ParagraphMode::EmptyLine,
                                        tr(de, "Nur bei Leerzeilen", "Only at empty lines"),
                                    );
                                });
                            ui.end_row();
                            ui.label(tr(de, "Modus zum Einfügen der Dateien:", "File insert mode:"));
                            egui::ComboBox::from_id_salt("join")
                                .selected_text(match layout.join {
                                    JoinMode::NewPage => {
                                        tr(de, "Jede Datei auf neuer Seite", "Each file on a new page")
                                    }
                                    JoinMode::Continuous => tr(de, "Fortlaufend", "Continuous"),
                                })
                                .width(240.0)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut layout.join,
                                        JoinMode::NewPage,
                                        tr(de, "Jede Datei auf neuer Seite", "Each file on a new page"),
                                    );
                                    ui.selectable_value(
                                        &mut layout.join,
                                        JoinMode::Continuous,
                                        tr(de, "Fortlaufend", "Continuous"),
                                    );
                                });
                            ui.end_row();
                        });
                    });
                }
                target.ui(ui, de, n, cur);
                let (ok, cancel, settings) = footer(ui, de, !files.is_empty(), true, "OK");
                open_settings = settings;
                if ok {
                    let (files, layout) = (files.clone(), layout.clone());
                    let at = target.insert_index(n, cur);
                    action = Some(Box::new(move |app, ctx| {
                        let result = decode_files(&files, rtf, layout.encoding)
                            .and_then(|docs| crate::compose::text_to_pdf(&docs, &layout));
                        app.edit(ctx, |tab| {
                            let (pdf, marks) = result?;
                            tab.doc.insert_pdf_bytes(at, pdf, &marks).map(|_| ())
                        });
                    }));
                    close(&mut outcome);
                } else if cancel {
                    close(&mut outcome);
                }
            }
            PageDialog::Delete { range, bookmarks } => {
                ui.heading(Cmd::DeletePages.label(lang));
                range.ui(ui, de, n, cur, &[RangeMode::Current, RangeMode::Custom]);
                ui.checkbox(bookmarks, tr(de, "Zugehörige Lesezeichen löschen", "Delete related bookmarks"));
                let sel = range.resolve(n, cur).filter(|p| p.len() < n);
                if range.resolve(n, cur).is_some_and(|p| p.len() >= n) {
                    ui.colored_label(
                        egui::Color32::from_rgb(200, 40, 40),
                        tr(
                            de,
                            "Mindestens eine Seite muss erhalten bleiben.",
                            "At least one page must remain.",
                        ),
                    );
                }
                let (ok, cancel, _) = footer(ui, de, sel.is_some(), false, "OK");
                if ok && let Some(pages) = sel {
                    let bm = *bookmarks;
                    action = Some(Box::new(move |app, ctx| {
                        app.edit(ctx, |tab| tab.doc.delete_pages_with_bookmarks(&pages, bm))
                    }));
                    close(&mut outcome);
                } else if cancel {
                    close(&mut outcome);
                }
            }
            PageDialog::DeleteBlank { ignore_annots, ignore_noise, tolerance, range, review } => {
                ui.heading(Cmd::DeleteBlankPages.label(lang));
                section(ui, tr(de, "Einstellungen", "Settings"), |ui| {
                    ui.label(tr(de, "Diese Seiten löschen:", "Delete these pages:"));
                    egui::ComboBox::from_id_salt("blank-mode")
                        .selected_text(tr(
                            de,
                            "Ohne Inhalt im Seitenbereich",
                            "Without content in the page area",
                        ))
                        .width(300.0)
                        .show_ui(ui, |ui| {
                            let _ = ui.selectable_label(
                                true,
                                tr(de, "Ohne Inhalt im Seitenbereich", "Without content in the page area"),
                            );
                        });
                    ui.checkbox(ignore_annots, tr(de, "Anmerkungen ignorieren", "Ignore comments"));
                    ui.checkbox(
                        ignore_noise,
                        tr(
                            de,
                            "Fehlerpixel vom Scannen am Seitenrand ignorieren",
                            "Ignore scan noise at the page border",
                        ),
                    );
                    ui.add_enabled_ui(*ignore_noise, |ui| {
                        ui.horizontal(|ui| {
                            ui.add(egui::Slider::new(tolerance, 0.0..=100.0).suffix(" ‰").text(tr(
                                de,
                                "Fehlertoleranz",
                                "Tolerance",
                            )));
                        });
                    });
                });
                range.ui(ui, de, n, cur, &[RangeMode::All, RangeMode::Custom]);
                ui.checkbox(
                    review,
                    tr(
                        de,
                        "Vor dem Löschen einen Dialog mit den leeren Seiten anzeigen",
                        "Show the blank pages before deleting",
                    ),
                );
                let sel = range.resolve(n, cur);
                let (ok, cancel, _) = footer(ui, de, sel.is_some(), false, "OK");
                if ok && let Some(pages) = sel {
                    let (ia, inz, tol, rev) = (*ignore_annots, *ignore_noise, *tolerance, *review);
                    action = Some(Box::new(move |app, ctx| app.delete_blank(ctx, &pages, ia, inz, tol, rev)));
                    close(&mut outcome);
                } else if cancel {
                    close(&mut outcome);
                }
            }
            PageDialog::BlankReview { pages } => {
                ui.heading(tr(de, "Leere Seiten", "Blank pages"));
                ui.label(tr(de, "Diese Seiten werden gelöscht:", "These pages will be deleted:"));
                egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                    for (page, keep) in pages.iter_mut() {
                        ui.checkbox(keep, format!("{} {}", tr(de, "Seite", "Page"), *page + 1));
                    }
                });
                let chosen: Vec<usize> = pages.iter().filter(|p| p.1).map(|p| p.0).collect();
                let (ok, cancel, _) = footer(
                    ui,
                    de,
                    !chosen.is_empty() && chosen.len() < n,
                    false,
                    tr(de, "Löschen", "Delete"),
                );
                if ok {
                    action =
                        Some(Box::new(move |app, ctx| app.edit(ctx, |tab| tab.doc.delete_pages(&chosen))));
                    close(&mut outcome);
                } else if cancel {
                    close(&mut outcome);
                }
            }
            PageDialog::Extract(form) => {
                ui.heading(Cmd::ExtractPages.label(lang));
                form.range.ui(ui, de, n, cur, &PageRange::ALL_MODES);
                section(ui, tr(de, "Ausgabe", "Output"), |ui| {
                    ui.horizontal(|ui| {
                        ui.label(tr(de, "Art:", "Kind:"));
                        egui::ComboBox::from_id_salt("extract-mode")
                            .selected_text(match form.mode {
                                ExtractMode::OneFile => tr(
                                    de,
                                    "Alle extrahierten Seiten in eine Datei speichern",
                                    "Save all extracted pages into one file",
                                ),
                                ExtractMode::PerPage => tr(
                                    de,
                                    "Jede Seite in eine eigene Datei speichern",
                                    "Save each page into its own file",
                                ),
                                ExtractMode::NewDocument => {
                                    tr(de, "In einem neuen Dokument öffnen", "Open in a new document")
                                }
                            })
                            .width(330.0)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut form.mode,
                                    ExtractMode::OneFile,
                                    tr(
                                        de,
                                        "Alle extrahierten Seiten in eine Datei speichern",
                                        "Save all extracted pages into one file",
                                    ),
                                );
                                ui.selectable_value(
                                    &mut form.mode,
                                    ExtractMode::PerPage,
                                    tr(
                                        de,
                                        "Jede Seite in eine eigene Datei speichern",
                                        "Save each page into its own file",
                                    ),
                                );
                                ui.selectable_value(
                                    &mut form.mode,
                                    ExtractMode::NewDocument,
                                    tr(de, "In einem neuen Dokument öffnen", "Open in a new document"),
                                );
                            });
                    });
                    ui.add_enabled_ui(form.mode != ExtractMode::NewDocument, |ui| {
                        ui.label(tr(de, "Dateiname:", "File name:"));
                        ui.horizontal(|ui| {
                            ui.add(egui::TextEdit::singleline(&mut form.file_name).desired_width(320.0));
                            macro_menu(
                                ui,
                                de,
                                &mut form.file_name,
                                &[
                                    ("%[FileName]", "Dateiname", "File name"),
                                    ("%[Page]", "Seitenzahl", "Page number"),
                                    ("%[Pages]", "Seitenbereich", "Page range"),
                                    ("%[Date]", "Datum", "Date"),
                                ],
                            );
                        });
                        ui.label(tr(de, "Zielordner:", "Target folder:"));
                        ui.horizontal(|ui| {
                            let mut text = form.folder.display().to_string();
                            if ui.add(egui::TextEdit::singleline(&mut text).desired_width(320.0)).changed() {
                                form.folder = PathBuf::from(text);
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
                                form.folder = dir;
                            }
                        });
                        ui.checkbox(
                            &mut form.open_folder,
                            tr(
                                de,
                                "Öffne Zielordner und wähle Dateien aus",
                                "Open target folder and select files",
                            ),
                        );
                        ui.checkbox(
                            &mut form.open_first,
                            tr(de, "Erstes Ergebnis-Dokument öffnen", "Open first result document"),
                        );
                    });
                });
                ui.checkbox(
                    &mut form.delete_after,
                    tr(de, "Seiten nach dem Extrahieren löschen", "Delete pages after extracting"),
                );
                let sel = form.range.resolve(n, cur);
                let folder_ok = form.mode == ExtractMode::NewDocument || form.folder.is_dir();
                if !folder_ok {
                    ui.colored_label(
                        egui::Color32::from_rgb(200, 40, 40),
                        tr(
                            de,
                            "Bitte einen vorhandenen Zielordner wählen.",
                            "Please choose an existing target folder.",
                        ),
                    );
                }
                let (ok, cancel, settings) = footer(ui, de, sel.is_some() && folder_ok, true, "OK");
                open_settings = settings;
                if ok && let Some(pages) = sel {
                    let form = form.clone();
                    action = Some(Box::new(move |app, ctx| app.extract(ctx, &pages, &form)));
                    close(&mut outcome);
                } else if cancel {
                    close(&mut outcome);
                }
            }
            PageDialog::Move { range, target } => {
                ui.heading(Cmd::MovePages.label(lang));
                range.ui(ui, de, n, cur, &PageRange::ALL_MODES);
                target.ui(ui, de, n, cur);
                let sel = range.resolve(n, cur).filter(|p| p.len() < n);
                let (ok, cancel, _) = footer(ui, de, sel.is_some(), false, "OK");
                if ok && let Some(pages) = sel {
                    let to = target.insert_index(n, cur);
                    action =
                        Some(Box::new(move |app, ctx| app.edit(ctx, |tab| tab.doc.move_pages(&pages, to))));
                    close(&mut outcome);
                } else if cancel {
                    close(&mut outcome);
                }
            }
            PageDialog::Duplicate { range, target, copies, collate, comments, forms } => {
                ui.heading(Cmd::DuplicatePages.label(lang));
                range.ui(ui, de, n, cur, &PageRange::ALL_MODES);
                target.ui(ui, de, n, cur);
                ui.horizontal(|ui| {
                    ui.label(tr(de, "Anzahl der Kopien:", "Number of copies:"));
                    ui.add(egui::DragValue::new(copies).range(1..=100));
                    ui.add_enabled(*copies > 1, egui::Checkbox::new(collate, tr(de, "Sammeln", "Collate")));
                });
                let sel = range.resolve(n, cur);
                let (ok, cancel, settings) = footer(ui, de, sel.is_some(), true, "OK");
                open_settings = settings;
                if ok && let Some(pages) = sel {
                    let at = target.insert_index(n, cur);
                    let (copies, collate) = (*copies as usize, *collate);
                    let (comments, forms) = (*comments, *forms);
                    action = Some(Box::new(move |app, ctx| {
                        app.edit(ctx, |tab| {
                            tab.doc
                                .duplicate_pages_to(&pages, at, copies, collate, comments, forms)
                                .map(|_| ())
                        })
                    }));
                    close(&mut outcome);
                } else if cancel {
                    close(&mut outcome);
                }
            }
            PageDialog::Replace {
                targets,
                keep,
                path,
                sources,
                fit,
                scale,
                preview_target,
                preview_source,
            } => {
                ui.heading(Cmd::ReplacePages.label(lang));
                let source_key = path.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
                if path.is_some() && self.dialog_doc.as_ref().is_none_or(|(k, _)| *k != source_key) {
                    match path.as_deref().map(|p| Document::open(p, None)) {
                        Some(Ok(doc)) => self.dialog_doc = Some((source_key.clone(), doc)),
                        Some(Err(err)) => {
                            self.show_error(&err);
                            *path = None;
                        }
                        None => {}
                    }
                }
                let source_pages = self.dialog_doc.as_ref().map_or(0, |(_, d)| d.page_count());
                ui.columns(2, |cols| {
                    let ui = &mut cols[0];
                    ui.label(RichText::new(tr(de, "Zieldokument:", "Target document:")).strong());
                    ui.label(self.active_tab().map(|t| t.doc.display_name().to_string()).unwrap_or_default());
                    if let Some(sel) = targets.resolve(n, cur)
                        && !sel.contains(preview_target)
                    {
                        *preview_target = sel[0];
                    }
                    *preview_target = (*preview_target).min(n.saturating_sub(1));
                    self.page_preview(ui, 0, *preview_target, vec2(ui.available_width(), 300.0));
                    page_strip(ui, preview_target, n);
                    targets.ui_titled(
                        ui,
                        de,
                        n,
                        cur,
                        &PageRange::ALL_MODES,
                        tr(de, "Seiten, die ersetzt werden sollen", "Pages to be replaced"),
                    );
                    ui.checkbox(
                        keep,
                        tr(
                            de,
                            "Anmerkungen und andere interaktive Funktionen behalten",
                            "Keep comments and other interactive features",
                        ),
                    )
                    .on_hover_text(tr(
                        de,
                        "Kommentare, Links und Formularfelder der ersetzten Seiten bleiben erhalten.",
                        "Comments, links and form fields of the replaced pages are kept.",
                    ));
                    let ui = &mut cols[1];
                    ui.label(RichText::new(tr(de, "Quelldokument:", "Source document:")).strong());
                    ui.horizontal(|ui| {
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
                            *preview_source = 0;
                        }
                    });
                    if let Some((_, doc)) = &self.dialog_doc {
                        if let Some(sel) = sources.resolve(source_pages, 0)
                            && !sel.contains(preview_source)
                        {
                            *preview_source = sel[0];
                        }
                        doc_preview(
                            ui,
                            &mut self.preview[1],
                            doc,
                            0x5ce,
                            (*preview_source).min(source_pages.saturating_sub(1)),
                            vec2(ui.available_width(), 300.0),
                        );
                        page_strip(ui, preview_source, source_pages);
                    } else {
                        let (rect, _) =
                            ui.allocate_exact_size(vec2(ui.available_width(), 300.0), egui::Sense::hover());
                        ui.painter().rect_filled(rect, 0.0, egui::Color32::from_gray(128));
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "PDF",
                            egui::FontId::proportional(28.0),
                            egui::Color32::from_gray(200),
                        );
                    }
                    ui.add_enabled_ui(path.is_some(), |ui| {
                        sources.ui_titled(
                            ui,
                            de,
                            source_pages,
                            0,
                            &[RangeMode::All, RangeMode::Custom],
                            tr(de, "Neue Seiten aus Datei", "New pages from file"),
                        );
                        ui.checkbox(
                            fit,
                            tr(
                                de,
                                "Seitengröße an das Zieldokument anpassen",
                                "Adapt page size to the target document",
                            ),
                        )
                        .on_hover_text(tr(
                            de,
                            "Die neuen Seiten übernehmen die Größe der ersetzten Seiten.",
                            "The new pages take the size of the replaced pages.",
                        ));
                        ui.add_enabled(
                            *fit,
                            egui::Checkbox::new(scale, tr(de, "Inhalte skalieren", "Scale contents")),
                        );
                    });
                });
                let t_sel = targets.resolve(n, cur);
                let s_sel = sources.resolve(source_pages, 0);
                let valid = matches!((&t_sel, &s_sel), (Some(t), Some(s)) if !t.is_empty() && !s.is_empty());
                if let (Some(t), Some(s)) = (&t_sel, &s_sel)
                    && t.len() != s.len()
                {
                    ui.label(
                        RichText::new(if de {
                            format!("Es werden {} Seite(n) ersetzt.", t.len().min(s.len()))
                        } else {
                            format!("{} page(s) will be replaced.", t.len().min(s.len()))
                        })
                        .weak(),
                    );
                }
                let (ok, cancel, _) = footer(ui, de, valid, false, "OK");
                if ok && let (Some(t), Some(s), Some(p)) = (t_sel, s_sel, path.clone()) {
                    let opts = crate::import::ReplaceOptions {
                        keep_annotations: *keep,
                        fit_target: *fit,
                        scale_content: *scale,
                    };
                    action = Some(Box::new(move |app, ctx| {
                        app.edit(ctx, |tab| tab.doc.replace_pages_from(&t, &p, &s, opts).map(|_| ()))
                    }));
                    close(&mut outcome);
                } else if cancel {
                    close(&mut outcome);
                }
            }
            PageDialog::Rotate { dir, range, filter } => {
                ui.heading(Cmd::RotatePages.label(lang));
                ui.horizontal(|ui| {
                    ui.label(tr(de, "Richtung:", "Direction:"));
                    let names = [
                        (RotateDir::Clockwise, tr(de, "90° im Uhrzeigersinn", "90° clockwise")),
                        (
                            RotateDir::CounterClockwise,
                            tr(de, "90° gegen den Uhrzeigersinn", "90° counter-clockwise"),
                        ),
                        (RotateDir::Half, "180°"),
                    ];
                    egui::ComboBox::from_id_salt("rotate-dir")
                        .selected_text(names.iter().find(|x| x.0 == *dir).map_or("", |x| x.1))
                        .width(230.0)
                        .show_ui(ui, |ui| {
                            for (d, name) in names {
                                ui.selectable_value(dir, d, name);
                            }
                        });
                });
                range.ui(ui, de, n, cur, &PageRange::ALL_MODES);
                let names = [
                    (
                        OrientationFilter::Any,
                        tr(de, "Seiten mit beliebiger Ausrichtung", "Pages of any orientation"),
                    ),
                    (OrientationFilter::Portrait, tr(de, "Nur Seiten im Hochformat", "Portrait pages only")),
                    (
                        OrientationFilter::Landscape,
                        tr(de, "Nur Seiten im Querformat", "Landscape pages only"),
                    ),
                ];
                egui::ComboBox::from_id_salt("rotate-filter")
                    .selected_text(names.iter().find(|x| x.0 == *filter).map_or("", |x| x.1))
                    .width(260.0)
                    .show_ui(ui, |ui| {
                        for (f, name) in names {
                            ui.selectable_value(filter, f, name);
                        }
                    });
                let sel = range.resolve(n, cur).map(|pages| {
                    let tab = self.active_tab();
                    pages
                        .into_iter()
                        .filter(|p| {
                            let (w, h) =
                                tab.and_then(|t| t.doc.geometry(*p)).map_or((1.0, 1.0), |g| g.view_size());
                            match filter {
                                OrientationFilter::Any => true,
                                OrientationFilter::Portrait => h >= w,
                                OrientationFilter::Landscape => w > h,
                            }
                        })
                        .collect::<Vec<_>>()
                });
                let (ok, cancel, _) =
                    footer(ui, de, sel.as_ref().is_some_and(|s| !s.is_empty()), false, "OK");
                if ok && let Some(pages) = sel {
                    let delta = match dir {
                        RotateDir::Clockwise => 90,
                        RotateDir::CounterClockwise => -90,
                        RotateDir::Half => 180,
                    };
                    action = Some(Box::new(move |app, ctx| {
                        app.edit(ctx, |tab| tab.doc.rotate_pages(&pages, delta))
                    }));
                    close(&mut outcome);
                } else if cancel {
                    close(&mut outcome);
                }
            }
            PageDialog::Reverse { range } => {
                ui.heading(Cmd::ReversePages.label(lang));
                range.ui(ui, de, n, cur, &PageRange::ALL_MODES);
                let sel = range.resolve(n, cur).filter(|p| p.len() > 1);
                let (ok, cancel, _) = footer(ui, de, sel.is_some(), false, "OK");
                if ok && let Some(pages) = sel {
                    action = Some(Box::new(move |app, ctx| {
                        app.edit(ctx, |tab| tab.doc.reverse_page_range(&pages))
                    }));
                    close(&mut outcome);
                } else if cancel {
                    close(&mut outcome);
                }
            }
            PageDialog::Normalize { range } => {
                ui.heading(Cmd::NormalizePages.label(lang));
                ui.label(tr(de, "Es werden unnötiger Seitenversatz und Drehungen entfernt, das Aussehen der Seite ändert sich nicht.", "Unnecessary page offsets and rotations are removed; the look of the page does not change."));
                range.ui(ui, de, n, cur, &PageRange::ALL_MODES);
                let sel = range.resolve(n, cur);
                let (ok, cancel, _) = footer(ui, de, sel.is_some(), false, "OK");
                if ok && let Some(pages) = sel {
                    action = Some(Box::new(move |app, ctx| {
                        let result = app.tab().map(|t| t.doc.normalize_pages(&pages));
                        let de = app.pd_de();
                        if let Some(tab) = app.tab() {
                            tab.after_edit();
                        }
                        match result {
                            Some(Ok(k)) => app.set_status(
                                ctx,
                                if de {
                                    format!("Normalisierte Seiten: {k}")
                                } else {
                                    format!("Normalized pages: {k}")
                                },
                            ),
                            Some(Err(err)) => app.show_error(&err),
                            None => {}
                        }
                    }));
                    close(&mut outcome);
                } else if cancel {
                    close(&mut outcome);
                }
            }
            PageDialog::Duplicates { range, ignore_orientation, ignore_annots } => {
                ui.heading(tr(de, "Doppelte Seiten finden", "Find duplicate pages"));
                range.ui(ui, de, n, cur, &[RangeMode::All, RangeMode::Custom]);
                section(ui, tr(de, "Einstellungen", "Settings"), |ui| {
                    ui.checkbox(
                        ignore_orientation,
                        tr(de, "Seitenorientierung ignorieren", "Ignore page orientation"),
                    );
                    ui.checkbox(
                        ignore_annots,
                        tr(
                            de,
                            "Anmerkungen und Formularfelder ignorieren",
                            "Ignore comments and form fields",
                        ),
                    );
                });
                let sel = range.resolve(n, cur).filter(|p| p.len() > 1);
                let (ok, cancel, _) = footer(ui, de, sel.is_some(), false, "OK");
                if ok && let Some(pages) = sel {
                    let (io, ia) = (*ignore_orientation, *ignore_annots);
                    action = Some(Box::new(move |app, _ctx| app.show_duplicates(&pages, io, ia)));
                    close(&mut outcome);
                } else if cancel {
                    close(&mut outcome);
                }
            }
        }
        if open_settings {
            state.backup = Some(Box::new(state.dialog.clone()));
            state.settings_open = true;
            state.settings_page = 0;
        }
        if matches!(outcome, Outcome::Close) {
            self.clear_previews();
            self.dialog_doc = None;
        }
        (outcome, action)
    }

    /// Size (points) of new blank pages.
    fn blank_size(&self, form: &BlankForm, n: usize, cur: usize) -> (f32, f32) {
        let page_size = |p: usize| {
            self.active_tab().and_then(|t| t.doc.geometry(p)).map_or((595.28, 841.89), |g| g.view_size())
        };
        let (w, h) = match form.size {
            BlankSize::AtInsert => {
                let at = form.target.page_index(n, cur);
                page_size(at)
            }
            BlankSize::FromPage => {
                let t = Target { after: false, page: form.ref_page, custom: form.ref_custom };
                page_size(t.page_index(n, cur))
            }
            BlankSize::Standard => form.paper.size().unwrap_or((595.28, 841.89)),
            BlankSize::Custom => form.custom,
        };
        match form.orientation {
            Orientation::Keep => (w, h),
            Orientation::Portrait => (w.min(h), w.max(h)),
            Orientation::Landscape => (w.max(h), w.min(h)),
        }
    }

    /// PDF with one page built from the clipboard text or image.
    fn clipboard_pdf(&self, image: bool) -> crate::Result<Vec<u8>> {
        if image {
            let img = self
                .clipboard
                .1
                .clone()
                .ok_or_else(|| crate::Error::Invalid("no image in the clipboard".into()))?;
            let layout = ImageLayout { margins: [28.35; 4], ..ImageLayout::default() };
            Ok(crate::compose::images_to_pdf(&[("clipboard.png".into(), img)], &layout)?.0)
        } else {
            let text = self
                .clipboard
                .0
                .clone()
                .ok_or_else(|| crate::Error::Invalid("no text in the clipboard".into()))?;
            let layout = TextLayout { font: TextFont::Courier, ..TextLayout::default() };
            Ok(crate::compose::text_to_pdf(&[("clipboard.txt".into(), text)], &layout)?.0)
        }
    }

    fn delete_blank(
        &mut self,
        ctx: &Context,
        pages: &[usize],
        ignore_annots: bool,
        ignore_noise: bool,
        tolerance: f32,
        review: bool,
    ) {
        let de = self.pd_de();
        let Some(tab) = self.active_tab() else { return };
        let n = tab.doc.page_count();
        let found = match tab.doc.blank_pages(pages, ignore_annots, ignore_noise, tolerance) {
            Ok(f) => f,
            Err(err) => {
                self.show_error(&err);
                return;
            }
        };
        if found.is_empty() {
            self.dialog = Some(Dialog::Message {
                title: Cmd::DeleteBlankPages.label(self.settings.language).into(),
                text: tr(de, "Es wurden keine leeren Seiten gefunden.", "No blank pages were found.").into(),
            });
            return;
        }
        if review || found.len() >= n {
            self.open_pages(PageDialog::BlankReview {
                pages: found.into_iter().map(|p| (p, true)).collect(),
            });
            return;
        }
        let count = found.len();
        self.edit(ctx, |tab| tab.doc.delete_pages(&found));
        self.set_status(
            ctx,
            if de {
                format!("Gelöschte leere Seiten: {count}")
            } else {
                format!("Deleted blank pages: {count}")
            },
        );
    }

    fn show_duplicates(&mut self, pages: &[usize], ignore_orientation: bool, ignore_annots: bool) {
        let de = self.pd_de();
        let result = self
            .active_tab()
            .map(|t| t.doc.find_duplicate_pages_in(pages, ignore_orientation, ignore_annots));
        match result {
            Some(Ok(groups)) if groups.is_empty() => {
                self.dialog = Some(Dialog::Message {
                    title: tr(de, "Doppelte Seiten finden", "Find duplicate pages").into(),
                    text: tr(de, "Es wurden keine doppelten Seiten gefunden.", "No duplicate pages found.")
                        .into(),
                });
            }
            Some(Ok(groups)) => self.dialog = Some(Dialog::Duplicates { groups }),
            Some(Err(err)) => self.show_error(&err),
            None => {}
        }
    }

    fn extract(&mut self, ctx: &Context, pages: &[usize], form: &ExtractForm) {
        let de = self.pd_de();
        let Some(tab) = self.active_tab() else { return };
        let stem = tab.doc.display_name().trim_end_matches(".pdf").trim_end_matches(".PDF").to_string();
        let date = super::structure::today(false);
        let range_text = pages_text(pages);
        if form.mode == ExtractMode::NewDocument {
            let result = tab
                .doc
                .extract_pages_with(pages, &form.options)
                .and_then(|bytes| Document::from_bytes(bytes, &format!("{stem} ({range_text}).pdf"), None));
            match result {
                Ok(doc) => {
                    self.add_tab(doc);
                    ctx.request_repaint();
                }
                Err(err) => self.show_error(&err),
            }
            return;
        }
        let result = (|| -> crate::Result<Vec<PathBuf>> {
            match form.mode {
                ExtractMode::NewDocument => Ok(Vec::new()),
                ExtractMode::OneFile => {
                    let bytes = tab.doc.extract_pages_with(pages, &form.options)?;
                    let name = expand_file_name(&form.file_name, &stem, pages[0] + 1, &range_text, &date);
                    let path = unique_path(&form.folder, &name);
                    crate::document::write_atomic(&path, &bytes)?;
                    Ok(vec![path])
                }
                ExtractMode::PerPage => {
                    let mut out = Vec::new();
                    for &p in pages {
                        let bytes = tab.doc.extract_pages_with(&[p], &form.options)?;
                        let name =
                            expand_file_name(&form.file_name, &stem, p + 1, &(p + 1).to_string(), &date);
                        let path = unique_path(&form.folder, &name);
                        crate::document::write_atomic(&path, &bytes)?;
                        out.push(path);
                    }
                    Ok(out)
                }
            }
        })();
        let files = match result {
            Ok(f) => f,
            Err(err) => {
                self.show_error(&err);
                return;
            }
        };
        if form.delete_after {
            self.edit(ctx, |tab| tab.doc.delete_pages(pages));
        }
        if let Some(first) = files.first() {
            if form.open_folder {
                super::features::reveal_in_file_manager(first);
            }
            if form.open_first {
                self.pending_open.push_back(first.clone());
            }
            self.set_status(
                ctx,
                if de {
                    format!("Extrahiert: {} Datei(en)", files.len())
                } else {
                    format!("Extracted: {} file(s)", files.len())
                },
            );
        }
    }

    /// The "Einstellungen" view of a page dialog; returns (ok, cancel).
    fn settings_view(&mut self, ui: &mut Ui, state: &mut PageDialogState) -> (bool, bool) {
        let de = self.pd_de();
        let unit = self.settings.unit;
        let page = &mut state.settings_page;
        match &mut state.dialog {
            PageDialog::Images { layout, .. } => {
                ui.heading(tr(
                    de,
                    "Einstellungen für Bild-zu-PDF-Umwandlung",
                    "Settings for image to PDF conversion",
                ));
                let names = [
                    tr(de, "Seitenlayout", "Page layout"),
                    tr(de, "Bildposition", "Image position"),
                    tr(de, "Bildbeschreibung", "Image caption"),
                    tr(de, "Bildverarbeitung", "Image processing"),
                    tr(de, "Bildkomprimierung", "Image compression"),
                    tr(de, "Bildnachbearbeitung", "Image post-processing"),
                ];
                ui.horizontal_top(|ui| {
                    settings_nav(ui, page, &names);
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.set_min_width(440.0);
                        match *page {
                            0 => {
                                ui.horizontal(|ui| {
                                    ui.label(tr(de, "Typ:", "Type:"));
                                    segmented(ui, &mut layout.page_mode, &[
                                        (PageSizeMode::ByImage, tr(de, "Nach Bild", "By image").into()),
                                        (PageSizeMode::Standard, "Standard".into()),
                                        (PageSizeMode::Custom, tr(de, "Eigene", "Custom").into()),
                                    ]);
                                    unit_combo(ui, de, &mut self.settings.unit);
                                });
                                ui.add_enabled_ui(layout.page_mode != PageSizeMode::ByImage, |ui| {
                                    egui::Grid::new("img-page").num_columns(2).spacing(vec2(8.0, 6.0)).show(ui, |ui| {
                                        ui.label(tr(de, "Größe:", "Size:"));
                                        if layout.page_mode == PageSizeMode::Custom {
                                            ui.horizontal(|ui| {
                                                length(ui, &mut layout.page_size.0, unit, 36.0..=14_400.0);
                                                ui.label("x");
                                                length(ui, &mut layout.page_size.1, unit, 36.0..=14_400.0);
                                            });
                                        } else {
                                            let mut paper = [PaperChoice::A4, PaperChoice::A3, PaperChoice::A5, PaperChoice::Letter, PaperChoice::Legal]
                                                .into_iter()
                                                .find(|p| p.size().is_some_and(|s| (s.0 - layout.page_size.0).abs() < 1.0 && (s.1 - layout.page_size.1).abs() < 1.0))
                                                .unwrap_or(PaperChoice::A4);
                                            paper_combo(ui, de, "img-paper", &mut paper);
                                            layout.page_size = paper.size().unwrap_or(layout.page_size);
                                        }
                                        ui.end_row();
                                        ui.label(tr(de, "Ausrichtung:", "Orientation:"));
                                        segmented(ui, &mut layout.landscape, &[(false, tr(de, "Hochformat", "Portrait").into()), (true, tr(de, "Querformat", "Landscape").into())]);
                                        ui.end_row();
                                        ui.label(tr(de, "Seitenausrichtung:", "Page orientation:"));
                                        egui::ComboBox::from_id_salt("img-rot").selected_text(if layout.auto_rotate { tr(de, "Automatisch nach Bild", "Automatic by image") } else { tr(de, "Nicht drehen", "Do not rotate") }).show_ui(ui, |ui| {
                                            ui.selectable_value(&mut layout.auto_rotate, false, tr(de, "Nicht drehen", "Do not rotate"));
                                            ui.selectable_value(&mut layout.auto_rotate, true, tr(de, "Automatisch nach Bild", "Automatic by image"));
                                        });
                                        ui.end_row();
                                    });
                                });
                                margins_ui(ui, de, &mut layout.margins, unit);
                            }
                            1 => {
                                section(ui, tr(de, "Layoutregeln", "Layout rules"), |ui| {
                                    egui::Grid::new("img-grid").num_columns(4).spacing(vec2(8.0, 6.0)).show(ui, |ui| {
                                        ui.label(tr(de, "Spalten:", "Columns:"));
                                        ui.add(egui::DragValue::new(&mut layout.columns).range(1..=20));
                                        ui.label(tr(de, "Horizontaler Abstand:", "Horizontal gap:"));
                                        ui.add_enabled_ui(layout.columns > 1, |ui| length(ui, &mut layout.h_gap, unit, 0.0..=500.0));
                                        ui.end_row();
                                        ui.label(tr(de, "Vertikale Anzahl:", "Rows:"));
                                        ui.add(egui::DragValue::new(&mut layout.rows).range(1..=20));
                                        ui.label(tr(de, "Vertikaler Abstand:", "Vertical gap:"));
                                        ui.add_enabled_ui(layout.rows > 1, |ui| length(ui, &mut layout.v_gap, unit, 0.0..=500.0));
                                        ui.end_row();
                                    });
                                });
                                section(ui, tr(de, "Bilder ausrichten", "Align images"), |ui| {
                                    egui::Grid::new("img-align").num_columns(4).spacing(vec2(8.0, 6.0)).show(ui, |ui| {
                                        ui.label(tr(de, "Einpassen:", "Fit:"));
                                        let names = [(Fit::Shrink, tr(de, "Auf Zelle reduzieren", "Shrink to cell")), (Fit::Fill, tr(de, "An Zelle anpassen", "Fit to cell")), (Fit::Original, tr(de, "Originalgröße", "Original size"))];
                                        egui::ComboBox::from_id_salt("img-fit").selected_text(names.iter().find(|x| x.0 == layout.fit).map_or("", |x| x.1)).show_ui(ui, |ui| {
                                            for (f, name) in names {
                                                ui.selectable_value(&mut layout.fit, f, name);
                                            }
                                        });
                                        ui.label(tr(de, "Horiz. Ausrichtung:", "Horiz. alignment:"));
                                        align_combo(ui, de, "img-h", &mut layout.h_align, false);
                                        ui.end_row();
                                        ui.checkbox(&mut layout.keep_aspect, tr(de, "Festes Seitenverhältnis", "Keep aspect ratio"));
                                        ui.label("");
                                        ui.label(tr(de, "Vert. Ausrichtung:", "Vert. alignment:"));
                                        align_combo(ui, de, "img-v", &mut layout.v_align, true);
                                        ui.end_row();
                                    });
                                });
                            }
                            2 => {
                                section(ui, tr(de, "Bildbeschreibung", "Image caption"), |ui| {
                                    let mut on = layout.caption.is_some();
                                    if ui.checkbox(&mut on, tr(de, "Eigene Beschreibung für jedes Bild benutzen", "Use a caption for every image")).changed() {
                                        layout.caption = on.then(|| "%[FileName]".to_string());
                                    }
                                    ui.add_enabled_ui(on, |ui| {
                                        let mut text = layout.caption.clone().unwrap_or_else(|| "%[FileName]".into());
                                        ui.horizontal(|ui| {
                                            ui.add(egui::TextEdit::singleline(&mut text).desired_width(300.0));
                                            macro_menu(ui, de, &mut text, &[("%[FileName]", "Dateiname", "File name"), ("%[FullName]", "Dateiname mit Endung", "File name with extension"), ("%[Index]", "Laufende Nummer", "Running number")]);
                                        });
                                        if on {
                                            layout.caption = Some(text);
                                        }
                                        ui.horizontal(|ui| {
                                            ui.label(tr(de, "Schriftgröße:", "Font size:"));
                                            ui.add(egui::DragValue::new(&mut layout.caption_size).range(4.0..=72.0).suffix(" pt"));
                                        });
                                        ui.checkbox(&mut layout.caption_above, tr(de, "Beschreibung über Bild einfügen", "Place caption above the image"));
                                    });
                                    ui.checkbox(&mut layout.bookmarks, tr(de, "Lesezeichen mit der Beschreibung als Namen für jedes Bild hinzufügen", "Add a bookmark named like the caption for every image"));
                                });
                            }
                            3 => {
                                section(ui, tr(de, "Herunterrechnen", "Downsampling"), |ui| {
                                    let mut on = layout.downsample_above.is_some();
                                    ui.checkbox(&mut on, tr(de, "Bilder mit hoher Auflösung herunterrechnen", "Downsample high-resolution images"));
                                    let mut above = layout.downsample_above.unwrap_or(225.0);
                                    ui.add_enabled_ui(on, |ui| {
                                        egui::Grid::new("img-ds").num_columns(2).show(ui, |ui| {
                                            ui.label(tr(de, "Auf:", "To:"));
                                            ui.add(egui::DragValue::new(&mut layout.downsample_to).range(36.0..=1200.0).suffix(tr(de, " Punkte pro Zoll", " dpi")));
                                            ui.end_row();
                                            ui.label(tr(de, "für Bilder über:", "for images above:"));
                                            ui.add(egui::DragValue::new(&mut above).range(36.0..=2400.0).suffix(tr(de, " Punkte pro Zoll", " dpi")));
                                            ui.end_row();
                                        });
                                    });
                                    layout.downsample_above = on.then_some(above.max(layout.downsample_to));
                                    ui.horizontal(|ui| {
                                        ui.label(tr(de, "Auflösung der Bilder (für „Nach Bild“):", "Image resolution (for \"By image\"):"));
                                        ui.add(egui::DragValue::new(&mut layout.image_dpi).range(36.0..=2400.0).suffix(" dpi"));
                                    });
                                });
                                ui.label(RichText::new(tr(de, "Farbumwandlung beim Einfügen ist nicht enthalten; dafür gibt es „Umwandeln > Farben umwandeln“.", "Colour conversion while inserting is not included; use \"Convert > Convert colours\".")).weak().small());
                            }
                            4 => {
                                section(ui, tr(de, "Echtfarben und Graustufen", "True colour and greyscale"), |ui| {
                                    let mut lossless = layout.jpeg_quality.is_none();
                                    let mut q = layout.jpeg_quality.unwrap_or(75);
                                    ui.horizontal(|ui| {
                                        ui.label(tr(de, "JPEG-Qualität:", "JPEG quality:"));
                                        ui.add_enabled(!lossless, egui::DragValue::new(&mut q).range(1..=100));
                                        ui.checkbox(&mut lossless, tr(de, "Verlustfrei", "Lossless"));
                                    });
                                    layout.jpeg_quality = (!lossless).then_some(q);
                                });
                                ui.label(RichText::new(tr(de, "Indizierte und einfarbige Bilder werden verlustfrei (Flate) gespeichert; JBIG2 wird nicht verwendet.", "Indexed and monochrome images are stored lossless (Flate); JBIG2 is not used.")).weak().small());
                            }
                            _ => {
                                ui.add_enabled_ui(false, |ui| {
                                    let mut off = false;
                                    ui.checkbox(&mut off, tr(de, "Begradigen", "Deskew"));
                                    ui.checkbox(&mut off, tr(de, "Bildfehler korrigieren", "Correct image defects"));
                                    ui.checkbox(&mut off, tr(de, "Texterkennung ausführen", "Run text recognition"));
                                });
                                ui.label(RichText::new(tr(de, "Bildnachbearbeitung und Texterkennung (OCR) sind in MrPDF nicht enthalten.", "Image post-processing and text recognition (OCR) are not included in MrPDF.")).weak());
                            }
                        }
                    });
                });
            }
            PageDialog::Text { layout, .. } => {
                ui.heading(tr(de, "Einstellungen", "Settings"));
                let names = [
                    tr(de, "Seitenlayout", "Page layout"),
                    tr(de, "Textformat", "Text format"),
                    tr(de, "Kopfzeile", "Header"),
                ];
                ui.horizontal_top(|ui| {
                    settings_nav(ui, page, &names);
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.set_min_width(440.0);
                        match *page {
                            0 => {
                                ui.horizontal(|ui| {
                                    ui.label(tr(de, "Größe:", "Size:"));
                                    let mut paper = [
                                        PaperChoice::A4,
                                        PaperChoice::A3,
                                        PaperChoice::A5,
                                        PaperChoice::Letter,
                                        PaperChoice::Legal,
                                    ]
                                    .into_iter()
                                    .find(|p| {
                                        p.size().is_some_and(|s| (s.0 - layout.page_size.0).abs() < 1.0)
                                    })
                                    .unwrap_or(PaperChoice::A4);
                                    paper_combo(ui, de, "txt-paper", &mut paper);
                                    layout.page_size = paper.size().unwrap_or(layout.page_size);
                                    unit_combo(ui, de, &mut self.settings.unit);
                                });
                                ui.horizontal(|ui| {
                                    ui.label(tr(de, "Ausrichtung:", "Orientation:"));
                                    segmented(
                                        ui,
                                        &mut layout.landscape,
                                        &[
                                            (false, tr(de, "Hochformat", "Portrait").into()),
                                            (true, tr(de, "Querformat", "Landscape").into()),
                                        ],
                                    );
                                });
                                margins_ui(ui, de, &mut layout.margins, unit);
                            }
                            1 => {
                                section(ui, tr(de, "Textformat", "Text format"), |ui| {
                                    egui::Grid::new("txt-format")
                                        .num_columns(2)
                                        .spacing(vec2(8.0, 6.0))
                                        .show(ui, |ui| {
                                            ui.label(tr(de, "Schriftart:", "Font:"));
                                            segmented(
                                                ui,
                                                &mut layout.font,
                                                &[
                                                    (TextFont::Helvetica, "Helvetica".into()),
                                                    (TextFont::Courier, "Courier".into()),
                                                ],
                                            );
                                            ui.end_row();
                                            ui.label(tr(de, "Schriftgröße:", "Font size:"));
                                            ui.add(
                                                egui::DragValue::new(&mut layout.font_size)
                                                    .range(4.0..=72.0)
                                                    .suffix(" pt"),
                                            );
                                            ui.end_row();
                                            ui.label(tr(de, "Zeilenabstand:", "Line spacing:"));
                                            ui.add(
                                                egui::DragValue::new(&mut layout.line_spacing)
                                                    .range(0.8..=4.0)
                                                    .speed(0.05),
                                            );
                                            ui.end_row();
                                        });
                                });
                                section(ui, tr(de, "Spalten", "Columns"), |ui| {
                                    egui::Grid::new("txt-cols").num_columns(2).spacing(vec2(8.0, 6.0)).show(
                                        ui,
                                        |ui| {
                                            ui.label(tr(de, "Anzahl der Spalten:", "Number of columns:"));
                                            ui.add(egui::DragValue::new(&mut layout.columns).range(1..=6));
                                            ui.end_row();
                                            ui.label(tr(
                                                de,
                                                "Abstand zwischen Spalten:",
                                                "Gap between columns:",
                                            ));
                                            length(ui, &mut layout.column_gap, unit, 0.0..=300.0);
                                            ui.end_row();
                                        },
                                    );
                                    ui.checkbox(
                                        &mut layout.column_line,
                                        tr(de, "Linie zwischen Spalten", "Line between columns"),
                                    );
                                    ui.add_enabled_ui(layout.column_line, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(tr(de, "Dicke:", "Width:"));
                                            length(ui, &mut layout.line_width, unit, 0.1..=20.0);
                                        });
                                    });
                                });
                            }
                            _ => {
                                let mut on = layout.header.is_some();
                                if ui
                                    .checkbox(&mut on, tr(de, "Titelkopfzeile benutzen", "Use title header"))
                                    .changed()
                                {
                                    layout.header = on.then(|| "%[FileName]".to_string());
                                }
                                ui.add_enabled_ui(on, |ui| {
                                    let mut text =
                                        layout.header.clone().unwrap_or_else(|| "%[FileName]".into());
                                    ui.horizontal(|ui| {
                                        ui.add(egui::TextEdit::singleline(&mut text).desired_width(300.0));
                                        macro_menu(
                                            ui,
                                            de,
                                            &mut text,
                                            &[
                                                ("%[FileName]", "Dateiname", "File name"),
                                                (
                                                    "%[FullName]",
                                                    "Dateiname mit Endung",
                                                    "File name with extension",
                                                ),
                                            ],
                                        );
                                    });
                                    if on {
                                        layout.header = Some(text);
                                    }
                                    ui.horizontal(|ui| {
                                        ui.label(tr(de, "Schriftgröße:", "Font size:"));
                                        ui.add(
                                            egui::DragValue::new(&mut layout.header_size)
                                                .range(4.0..=72.0)
                                                .suffix(" pt"),
                                        );
                                    });
                                    ui.checkbox(
                                        &mut layout.bookmark_per_file,
                                        tr(
                                            de,
                                            "Ein Lesezeichen für jede Datei erstellen",
                                            "Create a bookmark for every file",
                                        ),
                                    );
                                    ui.checkbox(
                                        &mut layout.header_line,
                                        tr(de, "Linie einfügen", "Insert line"),
                                    );
                                    ui.checkbox(
                                        &mut layout.header_line_above,
                                        tr(
                                            de,
                                            "Linie über Kopfzeile einfügen",
                                            "Insert line above the header",
                                        ),
                                    );
                                });
                            }
                        }
                    });
                });
            }
            PageDialog::Extract(form) => {
                ui.heading(tr(de, "Einstellungen", "Settings"));
                let handling = |ui: &mut Ui, id: &str, label: &str, value: &mut Handling| {
                    ui.label(label);
                    let names = [
                        (Handling::Copy, tr(de, "Kopieren", "Copy")),
                        (Handling::Flatten, tr(de, "Einbetten", "Flatten")),
                        (Handling::Remove, tr(de, "Nicht kopieren", "Do not copy")),
                    ];
                    egui::ComboBox::from_id_salt(id)
                        .selected_text(names.iter().find(|x| x.0 == *value).map_or("", |x| x.1))
                        .width(200.0)
                        .show_ui(ui, |ui| {
                            for (h, name) in names {
                                ui.selectable_value(value, h, name);
                            }
                        });
                    ui.end_row();
                };
                egui::Grid::new("extract-settings").num_columns(2).spacing(vec2(8.0, 8.0)).show(ui, |ui| {
                    handling(
                        ui,
                        "ex-comments",
                        tr(de, "Kommentare:", "Comments:"),
                        &mut form.options.comments,
                    );
                    handling(
                        ui,
                        "ex-forms",
                        tr(de, "Formularfelder:", "Form fields:"),
                        &mut form.options.forms,
                    );
                    ui.label(tr(de, "Lesezeichen:", "Bookmarks:"));
                    let names = [
                        (BookmarkCopy::All, tr(de, "Alle kopieren", "Copy all")),
                        (BookmarkCopy::Related, tr(de, "Zugehörige kopieren", "Copy related")),
                        (BookmarkCopy::None, tr(de, "Nicht kopieren", "Do not copy")),
                    ];
                    egui::ComboBox::from_id_salt("ex-bm")
                        .selected_text(
                            names.iter().find(|x| x.0 == form.options.bookmarks).map_or("", |x| x.1),
                        )
                        .width(200.0)
                        .show_ui(ui, |ui| {
                            for (b, name) in names {
                                ui.selectable_value(&mut form.options.bookmarks, b, name);
                            }
                        });
                    ui.end_row();
                });
                ui.label(RichText::new(tr(de, "Ebenen und Sprungmarken werden mit den Seiten übernommen, soweit sie zu ihnen gehören.", "Layers and named destinations are taken over with the pages they belong to.")).weak().small());
            }
            PageDialog::Duplicate { comments, forms, .. } => {
                ui.heading(tr(de, "Einstellungen", "Settings"));
                egui::Grid::new("dup-settings").num_columns(2).spacing(vec2(8.0, 8.0)).show(ui, |ui| {
                    for (label, value, id) in [
                        (tr(de, "Kommentare:", "Comments:"), comments, "dup-c"),
                        (tr(de, "Formularfelder:", "Form fields:"), forms, "dup-f"),
                    ] {
                        ui.label(label);
                        egui::ComboBox::from_id_salt(id)
                            .selected_text(if *value {
                                tr(de, "Kopieren", "Copy")
                            } else {
                                tr(de, "Nicht kopieren", "Do not copy")
                            })
                            .width(200.0)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(value, true, tr(de, "Kopieren", "Copy"));
                                ui.selectable_value(value, false, tr(de, "Nicht kopieren", "Do not copy"));
                            });
                        ui.end_row();
                    }
                });
            }
            _ => {
                ui.label(tr(de, "Keine Einstellungen.", "No settings."));
            }
        }
        ui.add_space(8.0);
        let mut result = (false, false);
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                result.1 = ui.button(tr(de, "Abbrechen", "Cancel")).clicked();
                result.0 = ui.button(RichText::new("OK").strong()).clicked();
            });
        });
        (result.0, result.1 || ui.input(|i| i.key_pressed(egui::Key::Escape)))
    }
}

/// "1-3,7" for a sorted page list (1-based).
fn pages_text(pages: &[usize]) -> String {
    let mut parts = Vec::new();
    let mut i = 0;
    while i < pages.len() {
        let start = pages[i];
        let mut end = start;
        while i + 1 < pages.len() && pages[i + 1] == end + 1 {
            i += 1;
            end = pages[i];
        }
        parts.push(if start == end { (start + 1).to_string() } else { format!("{}-{}", start + 1, end + 1) });
        i += 1;
    }
    parts.join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_are_expanded_and_cleaned() {
        assert_eq!(expand_file_name("%[FileName]_%[Page]", "Bericht", 3, "3", "2026-10-26"), "Bericht_3.pdf");
        assert_eq!(expand_file_name("a/b:%[Pages]", "x", 1, "1-3,7", ""), "a_b_1-3,7.pdf");
        assert_eq!(expand_file_name("  ", "x", 1, "", ""), "Seiten.pdf");
        assert_eq!(pages_text(&[0, 1, 2, 6, 8, 9]), "1-3,7,9-10");
    }
}
