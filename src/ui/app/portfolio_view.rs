//! PDF portfolios: the file list shown instead of the pages, and
//! "PDF-Portfolio erstellen" (Datei › Neues Dokument).

use std::path::PathBuf;

use egui::{Context, RichText, Ui};

use super::dialogs::{Action, Outcome};
use super::page_dialogs::tr;
use super::{Dialog, VersoloPdfApp};
use crate::portfolio::{Entry, NewFile};
use crate::ui::i18n::Language;

/// State of "PDF-Portfolio erstellen".
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NewPortfolioState {
    pub files: Vec<PathBuf>,
    pub selected: Option<usize>,
    pub error: Option<String>,
}

fn size_text(bytes: u64, de: bool) -> String {
    let s = match bytes {
        b if b >= 1024 * 1024 => format!("{:.1} MB", b as f64 / 1048576.0),
        b if b >= 1024 => format!("{:.1} KB", b as f64 / 1024.0),
        b => format!("{b} Byte"),
    };
    if de { s.replace('.', ",") } else { s }
}

/// Files directly inside a folder (sorted, at most `MAX_FILES`).
fn folder_files(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_file()).collect())
        .unwrap_or_default();
    files.sort();
    files.truncate(crate::portfolio::MAX_FILES);
    files
}

/// Files that VersoloPDF can show itself (others are only saved).
fn openable_here(name: &str) -> bool {
    super::page_dialogs::openable(std::path::Path::new(name))
}

impl VersoloPdfApp {
    fn pf_de(&self) -> bool {
        self.settings.language == Language::German
    }

    /// The files of the portfolio in tab `index` (cached per revision).
    fn portfolio_files(&mut self, index: usize) -> Vec<Entry> {
        let Some(tab) = self.tabs.get_mut(index) else { return Vec::new() };
        let rev = tab.doc.revision();
        if let Some((r, list)) = self.portfolio_cache.get(&tab.id)
            && *r == rev
        {
            return list.clone();
        }
        let list = tab.doc.portfolio_entries().unwrap_or_default();
        self.portfolio_cache.insert(tab.id, (rev, list.clone()));
        list
    }

    /// The file list of a portfolio (shown instead of the pages).
    pub(super) fn portfolio_ui(&mut self, ui: &mut Ui, index: usize) {
        let de = self.pf_de();
        let ctx = ui.ctx().clone();
        let files = self.portfolio_files(index);
        let sel = self.tabs[index].portfolio_sel.filter(|s| *s < files.len());
        let mut open = None;
        let mut save = None;
        let mut remove = None;
        egui::Frame::new().inner_margin(egui::Margin::same(12)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading(tr(de, "PDF-Portfolio", "PDF portfolio"));
                ui.label(
                    RichText::new(if de {
                        format!("{} Dateien", files.len())
                    } else {
                        format!("{} files", files.len())
                    })
                    .weak(),
                );
            });
            ui.horizontal(|ui| {
                if ui.button(tr(de, "Dateien hinzufügen…", "Add files…")).clicked() {
                    self.portfolio_add(&ctx, index, false);
                }
                if ui.button(tr(de, "Ordner hinzufügen…", "Add folder…")).clicked() {
                    self.portfolio_add(&ctx, index, true);
                }
                ui.separator();
                if ui.add_enabled(sel.is_some(), egui::Button::new(tr(de, "Öffnen", "Open"))).clicked() {
                    open = sel;
                }
                if ui
                    .add_enabled(sel.is_some(), egui::Button::new(tr(de, "Speichern unter…", "Save as…")))
                    .clicked()
                {
                    save = sel;
                }
                if ui.add_enabled(sel.is_some(), egui::Button::new(tr(de, "Entfernen", "Remove"))).clicked() {
                    remove = sel;
                }
                ui.separator();
                if ui.button(tr(de, "Deckblatt anzeigen", "Show cover page")).clicked() {
                    self.tabs[index].show_cover = true;
                }
            });
            ui.add_space(6.0);
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                egui::Grid::new(("portfolio", self.tabs[index].id))
                    .num_columns(4)
                    .striped(true)
                    .min_col_width(90.0)
                    .spacing(egui::vec2(16.0, 6.0))
                    .show(ui, |ui| {
                        for h in [
                            tr(de, "Name", "Name"),
                            tr(de, "Größe", "Size"),
                            tr(de, "Geändert", "Modified"),
                            tr(de, "Beschreibung", "Description"),
                        ] {
                            ui.label(RichText::new(h).strong());
                        }
                        ui.end_row();
                        for (i, f) in files.iter().enumerate() {
                            let r = ui.selectable_label(sel == Some(i), &f.name);
                            if r.clicked() {
                                self.tabs[index].portfolio_sel = Some(i);
                            }
                            if r.double_clicked() {
                                open = Some(i);
                            }
                            ui.label(size_text(f.size, de));
                            ui.label(&f.modified);
                            ui.label(&f.description);
                            ui.end_row();
                        }
                    });
                if files.is_empty() {
                    ui.label(
                        RichText::new(tr(
                            de,
                            "Das Portfolio enthält keine Dateien.",
                            "The portfolio contains no files.",
                        ))
                        .weak(),
                    );
                }
            });
        });
        if let Some(i) = open.and_then(|i| files.get(i)) {
            self.portfolio_open(&ctx, index, &i.name.clone());
        }
        if let Some(i) = save.and_then(|i| files.get(i)) {
            self.portfolio_save(&ctx, index, &i.name.clone());
        }
        if let Some(i) = remove.and_then(|i| files.get(i)) {
            let name = i.name.clone();
            if let Some(a) = self.tabs[index].doc.attachment_index(&name) {
                let previous = self.active;
                self.active = index;
                self.edit(&ctx, |tab| tab.doc.delete_attachment(a));
                self.active = previous;
                self.tabs[index].portfolio_sel = None;
            }
        }
    }

    /// "Dateien hinzufügen…" / "Ordner hinzufügen…".
    pub(super) fn portfolio_add(&mut self, ctx: &Context, index: usize, folder: bool) {
        let paths = if folder {
            rfd::FileDialog::new().pick_folder().map(|d| folder_files(&d)).unwrap_or_default()
        } else {
            rfd::FileDialog::new().pick_files().unwrap_or_default()
        };
        if paths.is_empty() {
            return;
        }
        let previous = self.active;
        self.active = index;
        for path in paths {
            match NewFile::read(&path) {
                Ok(f) => self.edit(ctx, |tab| tab.doc.add_attachment_bytes(&f.name, &f.data)),
                Err(err) => self.show_error(&err),
            }
        }
        self.active = previous.min(self.tabs.len().saturating_sub(1));
    }

    /// "Öffnen": PDFs and files VersoloPDF can convert open in a new tab (from a
    /// temporary copy); nothing is ever executed.
    fn portfolio_open(&mut self, ctx: &Context, index: usize, name: &str) {
        if !openable_here(name) {
            self.portfolio_save(ctx, index, name);
            return;
        }
        let Some(a) = self.tabs[index].doc.attachment_index(name) else { return };
        let dir = std::env::temp_dir().join(format!("versolopdf-portfolio-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        // Only the file name part is used (no folders from the PDF).
        let safe: String = std::path::Path::new(name)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "datei.pdf".into());
        let dest = dir.join(safe);
        match self.tabs[index].doc.save_attachment(a, &dest) {
            Ok(()) => self.pending_open.push_back(dest),
            Err(err) => self.show_error(&err),
        }
    }

    fn portfolio_save(&mut self, _ctx: &Context, index: usize, name: &str) {
        let Some(a) = self.tabs[index].doc.attachment_index(name) else { return };
        let Some(dest) = rfd::FileDialog::new().set_file_name(name).save_file() else { return };
        if let Err(err) = self.tabs[index].doc.save_attachment(a, &dest) {
            self.show_error(&err);
        }
    }

    /// Opens "PDF-Portfolio erstellen".
    pub(super) fn open_new_portfolio(&mut self) {
        self.dialog = Some(Dialog::NewPortfolio(Box::default()));
    }

    pub(super) fn new_portfolio_dialog(
        &mut self,
        ui: &mut Ui,
        s: &mut NewPortfolioState,
    ) -> (Outcome, Option<Action>) {
        let de = self.pf_de();
        super::dialogs::set_title(ui, tr(de, "PDF-Portfolio erstellen", "Create PDF portfolio"));
        ui.set_min_width(620.0);
        ui.horizontal(|ui| {
            if ui.button(tr(de, "Dateien hinzufügen…", "Add files…")).clicked()
                && let Some(paths) = rfd::FileDialog::new().pick_files()
            {
                s.files.extend(paths);
            }
            if ui.button(tr(de, "Ordner hinzufügen…", "Add folder…")).clicked()
                && let Some(dir) = rfd::FileDialog::new().pick_folder()
            {
                s.files.extend(folder_files(&dir));
            }
            let resp = ui.button(tr(de, "Dateiliste ⏷", "File list ⏷"));
            egui::Popup::menu(&resp).show(|ui| {
                if ui
                    .add_enabled(
                        !s.files.is_empty(),
                        egui::Button::new(tr(
                            de,
                            "Alle Dateien in Dateiliste speichern…",
                            "Save all files to file list…",
                        )),
                    )
                    .clicked()
                    && let Some(path) = rfd::FileDialog::new().add_filter("Text", &["txt"]).save_file()
                {
                    let text: String = s.files.iter().map(|p| format!("{}\n", p.display())).collect();
                    if let Err(e) = std::fs::write(&path, text) {
                        s.error = Some(e.to_string());
                    }
                    ui.close();
                }
                if ui.button(tr(de, "Dateiliste laden…", "Load file list…")).clicked()
                    && let Some(path) = rfd::FileDialog::new().add_filter("Text", &["txt"]).pick_file()
                {
                    match std::fs::read_to_string(&path) {
                        Ok(text) if text.len() < 1024 * 1024 => s
                            .files
                            .extend(text.lines().map(str::trim).filter(|l| !l.is_empty()).map(PathBuf::from)),
                        Ok(_) => {
                            s.error = Some(tr(de, "Die Liste ist zu groß.", "The list is too large.").into())
                        }
                        Err(e) => s.error = Some(e.to_string()),
                    }
                    ui.close();
                }
            });
            ui.separator();
            let sel = s.selected.filter(|i| *i < s.files.len());
            let n = s.files.len();
            let mv = |s: &mut NewPortfolioState, from: usize, to: usize| {
                let f = s.files.remove(from);
                s.files.insert(to, f);
                s.selected = Some(to);
            };
            if ui
                .add_enabled(sel.is_some_and(|i| i > 0), egui::Button::new("⏶"))
                .on_hover_text(tr(de, "Nach oben", "Move up"))
                .clicked()
            {
                let i = sel.unwrap_or(0);
                mv(s, i, i - 1);
            }
            if ui
                .add_enabled(sel.is_some_and(|i| i + 1 < n), egui::Button::new("⏷"))
                .on_hover_text(tr(de, "Nach unten", "Move down"))
                .clicked()
            {
                let i = sel.unwrap_or(0);
                mv(s, i, i + 1);
            }
            if ui.add_enabled(sel.is_some(), egui::Button::new(tr(de, "Entfernen", "Remove"))).clicked() {
                s.files.remove(sel.unwrap_or(0));
                s.selected = None;
            }
            if ui.add_enabled(n > 0, egui::Button::new(tr(de, "Leeren", "Clear"))).clicked() {
                s.files.clear();
                s.selected = None;
            }
        });
        s.files.dedup();
        s.files.truncate(crate::portfolio::MAX_FILES);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_min_size(egui::vec2(600.0, 220.0));
            egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                egui::Grid::new("new-portfolio")
                    .num_columns(4)
                    .striped(true)
                    .spacing(egui::vec2(14.0, 4.0))
                    .show(ui, |ui| {
                        for h in [
                            tr(de, "Titel", "Title"),
                            tr(de, "Speicherort", "Location"),
                            tr(de, "Typ", "Type"),
                            tr(de, "Größe", "Size"),
                        ] {
                            ui.label(RichText::new(h).weak());
                        }
                        ui.end_row();
                        for (i, p) in s.files.iter().enumerate() {
                            let name =
                                p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                            if ui.selectable_label(s.selected == Some(i), name).clicked() {
                                s.selected = Some(i);
                            }
                            ui.label(p.parent().map(|d| d.display().to_string()).unwrap_or_default());
                            ui.label(
                                p.extension().map(|e| e.to_string_lossy().to_uppercase()).unwrap_or_default(),
                            );
                            ui.label(
                                std::fs::metadata(p).map(|m| size_text(m.len(), de)).unwrap_or_default(),
                            );
                            ui.end_row();
                        }
                    });
                if s.files.is_empty() {
                    ui.label(
                        RichText::new(tr(
                            de,
                            "Fügen Sie Dateien hinzu. Die Reihenfolge lässt sich mit den Pfeilen ändern.",
                            "Add files. The order can be changed with the arrows.",
                        ))
                        .weak(),
                    );
                }
            });
        });
        ui.label(if de {
            format!("{} Einträge", s.files.len())
        } else {
            format!("{} entries", s.files.len())
        });
        if let Some(e) = &s.error {
            ui.colored_label(egui::Color32::from_rgb(200, 40, 40), e);
        }
        let (ok, cancel, _) = super::page_dialogs::footer(ui, de, !s.files.is_empty(), false, "OK");
        if cancel {
            return (Outcome::Close, None);
        }
        if !ok {
            return (Outcome::Keep, None);
        }
        let mut files = Vec::new();
        for p in &s.files {
            match NewFile::read(p) {
                Ok(f) => files.push(f),
                Err(e) => {
                    s.error = Some(format!("{}: {e}", p.display()));
                    return (Outcome::Keep, None);
                }
            }
        }
        let title = tr(de, "PDF-Portfolio", "PDF portfolio").to_string();
        (Outcome::Close, Some(Box::new(move |app, ctx| app.finish_new_portfolio(ctx, files, &title))))
    }

    fn finish_new_portfolio(&mut self, ctx: &Context, files: Vec<NewFile>, title: &str) {
        let de = self.pf_de();
        let result = crate::portfolio::create(&files, title, de)
            .and_then(|bytes| crate::Document::new_from_pdf(bytes, &[], &format!("{title}.pdf")));
        match result {
            Ok(doc) => {
                self.add_tab(doc);
                ctx.request_repaint();
            }
            Err(err) => self.show_error(&err),
        }
    }
}
