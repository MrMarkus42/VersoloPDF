//! The "Datei" backstage view: a full-window page with a command column on
//! the left, a list of sub-commands in the middle and their content on the
//! right.

use std::path::{Path, PathBuf};

use egui::{
    Align2, Color32, FontId, Frame, Margin, Panel, Rect, RichText, Sense, Stroke, Ui, Vec2, WidgetInfo,
    WidgetType, vec2,
};

use super::commands::{Cmd, Unavailable};
use super::page_dialogs::{NewKind, TextKind};
use super::{Dialog, MrPdfApp};
use crate::ui::i18n::Language;
use crate::ui::icons::{self, Icon};
use crate::ui::settings::Session;
use crate::ui::theme;

/// Entries of the left column that open a list of sub-commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Open,
    SaveAs,
    New,
    Export,
    Share,
    ManageSettings,
}

impl Page {
    fn names(self) -> (&'static str, &'static str) {
        match self {
            Page::Open => ("Öffnen", "Open"),
            Page::SaveAs => ("Speichern unter", "Save as"),
            Page::New => ("Neues Dokument", "New document"),
            Page::Export => ("Exportieren", "Export"),
            Page::Share => ("Teilen", "Share"),
            Page::ManageSettings => ("Einstellungen verwalten", "Manage settings"),
        }
    }

    fn needs_document(self) -> bool {
        matches!(self, Page::SaveAs | Page::Export | Page::Share)
    }
}

/// One entry of the left column.
#[derive(Debug, Clone, Copy)]
enum Left {
    Page(Page),
    Run(Cmd),
    /// Planned in a later step of docs/DATEI.md (German, English, step).
    Planned(&'static str, &'static str, char),
    Separator,
}

const LEFT: &[Left] = &[
    Left::Page(Page::Open),
    Left::Separator,
    Left::Run(Cmd::Save),
    Left::Page(Page::SaveAs),
    Left::Planned("Optimiert speichern", "Save optimized", 'J'),
    Left::Separator,
    Left::Page(Page::New),
    Left::Separator,
    Left::Run(Cmd::Print),
    Left::Planned("Alle drucken", "Print all", 'K'),
    Left::Page(Page::Export),
    Left::Page(Page::Share),
    Left::Separator,
    Left::Planned("Zurücksetzen", "Revert", 'I'),
    Left::Separator,
    Left::Run(Cmd::DocProperties),
    Left::Planned("Speicherverbrauch untersuchen", "Examine space usage", 'J'),
    Left::Separator,
    Left::Planned("Umbenennen", "Rename", 'I'),
    Left::Planned("Datei verschieben", "Move file", 'I'),
    Left::Separator,
    Left::Planned("In den Papierkorb werfen", "Move to recycle bin", 'I'),
    Left::Separator,
    Left::Run(Cmd::Close),
    Left::Run(Cmd::CloseAll),
    Left::Separator,
    Left::Run(Cmd::Settings),
    Left::Page(Page::ManageSettings),
    Left::Separator,
    Left::Run(Cmd::Exit),
];

/// Content shown right of the "Öffnen" list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenView {
    Recent,
    Sessions,
}

/// What a sub-command in the middle list does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Run(Cmd),
    /// Shows a page of "Öffnen" on the right.
    View(OpenView),
    /// Opens a "Neues Dokument" dialog.
    New(NewKind),
    /// "Von URL öffnen".
    Url,
    /// Planned in a later step of docs/DATEI.md.
    Planned(char),
    /// Offered by other editors, not by MrPDF.
    Not(Unavailable),
}

#[derive(Debug, Clone, Copy)]
struct Item {
    de: &'static str,
    en: &'static str,
    icon: Icon,
    kind: Kind,
    /// Draw a separator line above this item.
    group: bool,
}

const fn item(de: &'static str, en: &'static str, icon: Icon, kind: Kind) -> Item {
    Item { de, en, icon, kind, group: false }
}

const fn group(de: &'static str, en: &'static str, icon: Icon, kind: Kind) -> Item {
    Item { de, en, icon, kind, group: true }
}

fn items(page: Page) -> &'static [Item] {
    use Kind::*;
    match page {
        Page::Open => {
            const {
                &[
                    item("Letzte Dateien", "Recent files", Icon::Open, View(OpenView::Recent)),
                    item("Sitzungen", "Sessions", Icon::Thumbnails, View(OpenView::Sessions)),
                    item("Von URL öffnen", "Open from URL", Icon::Web, Url),
                    item("Speicherort hinzufügen", "Add place", Icon::Cloud, Not(Unavailable::Cloud)),
                    item("Homepage", "Homepage", Icon::Web, Run(Cmd::Homepage)),
                    item("Lokale Datei", "Local file", Icon::Open, Run(Cmd::Open)),
                ]
            }
        }
        Page::SaveAs => {
            const {
                &[
                    item("Speicherort hinzufügen", "Add place", Icon::Cloud, Not(Unavailable::Cloud)),
                    item("Homepage", "Homepage", Icon::Web, Run(Cmd::Homepage)),
                    item("Lokale Datei", "Local file", Icon::SaveAs, Run(Cmd::SaveAs)),
                ]
            }
        }
        Page::New => {
            const {
                &[
                    item("Mit leeren Seiten", "With blank pages", Icon::New, New(NewKind::Blank)),
                    group("Dateien zusammenfassen", "Combine files", Icon::Merge, Run(Cmd::FromFiles)),
                    item("PDF-Portfolio", "PDF portfolio", Icon::Attachment, Not(Unavailable::Portfolio)),
                    group("Aus Text", "From text", Icon::TextFile, New(NewKind::Text(TextKind::Plain))),
                    item(
                        "Aus Markdown",
                        "From Markdown",
                        Icon::TextFile,
                        New(NewKind::Text(TextKind::Markdown)),
                    ),
                    item("Aus CSV-Datei", "From CSV file", Icon::TextFile, New(NewKind::Text(TextKind::Csv))),
                    item(
                        "Aus Rich-Text (RTF)",
                        "From rich text (RTF)",
                        Icon::TextFile,
                        New(NewKind::Text(TextKind::Rtf)),
                    ),
                    item("Aus Web-Seite", "From web page", Icon::Web, Not(Unavailable::WebImport)),
                    item("Aus E-Mail", "From e-mail", Icon::Mail, Not(Unavailable::WebImport)),
                    item("Aus Bilddateien", "From image files", Icon::Image, New(NewKind::Images)),
                    item("Von Scanner", "From scanner", Icon::Camera, Not(Unavailable::Scanner)),
                    group("Aus Zwischenablage", "From clipboard", Icon::Copy, New(NewKind::Clipboard)),
                ]
            }
        }
        Page::Export => {
            const {
                &[
                    item("In Bilder", "To images", Icon::Image, Run(Cmd::ExportImages)),
                    item(
                        "In Microsoft-Word-Dokument",
                        "To Microsoft Word document",
                        Icon::WordFile,
                        Run(Cmd::ExportWord),
                    ),
                    item(
                        "In Microsoft-Excel-Tabelle",
                        "To Microsoft Excel sheet",
                        Icon::Export,
                        Not(Unavailable::Office),
                    ),
                    item(
                        "In Microsoft-PowerPoint-Präsentation",
                        "To Microsoft PowerPoint presentation",
                        Icon::Export,
                        Not(Unavailable::Office),
                    ),
                    item("In Text", "To text", Icon::TextFile, Run(Cmd::ExportText)),
                ]
            }
        }
        Page::Share => {
            const {
                &[
                    item("Per E-Mail senden", "Send by e-mail", Icon::Mail, Run(Cmd::Email)),
                    item(
                        "Alle Dokumente per E-Mail senden",
                        "Send all documents by e-mail",
                        Icon::Mail,
                        Planned('K'),
                    ),
                    group("Zurückgeben", "Check in", Icon::Cloud, Not(Unavailable::Cloud)),
                    item("Ausleihen", "Check out", Icon::Cloud, Not(Unavailable::Cloud)),
                    item(
                        "Ausleihen rückgängig machen",
                        "Discard check out",
                        Icon::Cloud,
                        Not(Unavailable::Cloud),
                    ),
                    item("Bearbeitung zulassen", "Enable editing", Icon::Cloud, Not(Unavailable::Cloud)),
                ]
            }
        }
        Page::ManageSettings => {
            const {
                &[
                    item("Einstellungen exportieren", "Export settings", Icon::Export, Planned('M')),
                    item("Einstellungen importieren", "Import settings", Icon::Import, Planned('M')),
                    item("Einstellungen zurücksetzen", "Reset settings", Icon::Undo, Planned('M')),
                ]
            }
        }
    }
}

fn planned_hint(lang: Language, step: char) -> String {
    if lang == Language::German {
        format!("In Vorbereitung (Schritt {step}).")
    } else {
        format!("In preparation (step {step}).")
    }
}

/// Colours of the left column.
const COLUMN_TEXT: Color32 = Color32::WHITE;
const COLUMN_DISABLED: Color32 = Color32::from_rgb(232, 160, 165);

/// A full-width row of the left column.
fn column_row(ui: &mut Ui, label: &str, enabled: bool, selected: bool) -> egui::Response {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 23.0), sense);
    response.widget_info(|| WidgetInfo::selected(WidgetType::Button, enabled, selected, label));
    if ui.is_rect_visible(rect) {
        if selected {
            ui.painter().rect_filled(rect, 0.0, Color32::from_black_alpha(70));
        } else if enabled && response.hovered() {
            ui.painter().rect_filled(rect, 0.0, Color32::from_black_alpha(35));
        }
        ui.painter().text(
            rect.left_center() + vec2(10.0, 0.0),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(13.0),
            if enabled { COLUMN_TEXT } else { COLUMN_DISABLED },
        );
    }
    response
}

/// A row of the middle list: large icon and label.
fn list_row(ui: &mut Ui, icon: Icon, label: &str, enabled: bool, selected: bool) -> egui::Response {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), sense);
    response.widget_info(|| WidgetInfo::selected(WidgetType::Button, enabled, selected, label));
    if ui.is_rect_visible(rect) {
        let visuals = ui.visuals();
        if selected {
            ui.painter().rect(
                rect,
                2.0,
                theme::ACCENT.gamma_multiply(0.3),
                Stroke::new(1.0, theme::ACCENT.gamma_multiply(0.7)),
                egui::StrokeKind::Inside,
            );
        } else if enabled && response.hovered() {
            ui.painter().rect_filled(rect, 2.0, visuals.widgets.hovered.weak_bg_fill);
        }
        let tint = theme::icon_color(visuals.dark_mode);
        let mut painter = ui.painter_at(rect);
        if !enabled {
            painter.multiply_opacity(0.4);
        }
        let icon_rect = Rect::from_center_size(rect.left_center() + vec2(20.0, 0.0), Vec2::splat(26.0));
        icons::paint(&painter, icon_rect, icon, tint);
        let color = if enabled { visuals.text_color() } else { visuals.weak_text_color() };
        ui.painter().text(
            rect.left_center() + vec2(42.0, 0.0),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(13.0),
            color,
        );
    }
    response
}

/// Big light title of a page ("Öffnen", "Neues Dokument", …).
fn page_title(ui: &mut Ui, text: &str) {
    ui.add_space(8.0);
    ui.label(RichText::new(text).size(26.0).color(ui.visuals().weak_text_color()));
    ui.add_space(8.0);
}

impl MrPdfApp {
    /// Whether the "Datei" backstage is shown instead of the documents.
    pub fn backstage_open(&self) -> bool {
        self.backstage.is_some()
    }

    /// Opens or closes the backstage ("Datei" button).
    pub(super) fn toggle_backstage(&mut self) {
        self.backstage = match self.backstage {
            Some(_) => None,
            None => Some(Page::Open),
        };
        self.recent_filter.clear();
    }

    pub(super) fn close_backstage(&mut self) {
        self.backstage = None;
    }

    fn page_enabled(&self, page: Page) -> bool {
        !page.needs_document() || self.active_tab().is_some()
    }

    /// Runs a command chosen in the backstage and returns to the document.
    fn backstage_run(&mut self, ctx: &egui::Context, cmd: Cmd) {
        self.backstage = None;
        self.run(ctx, cmd);
    }

    /// Draws the backstage into the area below the tab row.
    pub(super) fn backstage_ui(&mut self, ui: &mut Ui) {
        let Some(page) = self.backstage else { return };
        let ctx = ui.ctx().clone();
        let lang = self.settings.language;
        let de = lang == Language::German;
        let pick = |names: (&'static str, &'static str)| if de { names.0 } else { names.1 };

        // Left column.
        let mut chosen: Option<Cmd> = None;
        let mut new_page: Option<Page> = None;
        Panel::left("backstage-column")
            .resizable(false)
            .exact_size(200.0)
            .frame(Frame::new().fill(theme::BRAND).inner_margin(Margin::symmetric(0, 8)))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                for entry in LEFT {
                    match *entry {
                        Left::Separator => {
                            let (rect, _) =
                                ui.allocate_exact_size(vec2(ui.available_width(), 7.0), Sense::hover());
                            ui.painter().hline(
                                rect.x_range().shrink(6.0),
                                rect.center().y,
                                Stroke::new(1.0, Color32::from_white_alpha(60)),
                            );
                        }
                        Left::Page(p) => {
                            let enabled = self.page_enabled(p);
                            if column_row(ui, pick(p.names()), enabled, p == page).clicked() {
                                new_page = Some(p);
                            }
                        }
                        Left::Run(cmd) => {
                            let label = cmd.label(lang);
                            let response = column_row(ui, label, self.cmd_enabled(cmd), false);
                            let response = match cmd.shortcut() {
                                Some(keys) => response.on_hover_text(format!("{label} ({})", pick(keys))),
                                None => response,
                            };
                            if response.clicked() {
                                chosen = Some(cmd);
                            }
                        }
                        Left::Planned(de_label, en_label, step) => {
                            let label = if de { de_label } else { en_label };
                            let _ = column_row(ui, label, false, false)
                                .on_disabled_hover_text(format!("{label}\n{}", planned_hint(lang, step)));
                        }
                    }
                }
            });
        if let Some(cmd) = chosen {
            self.backstage_run(&ctx, cmd);
            return;
        }
        if let Some(p) = new_page {
            self.backstage = Some(p);
            return;
        }

        // Middle list.
        let dark = ui.visuals().dark_mode;
        let list_fill = if dark { Color32::from_gray(44) } else { Color32::from_gray(244) };
        let mut clicked: Option<Kind> = None;
        let open_view = self.open_view;
        Panel::left("backstage-list")
            .resizable(false)
            .exact_size(300.0)
            .frame(Frame::new().fill(list_fill).inner_margin(Margin::symmetric(6, 4)))
            .show(ui, |ui| {
                page_title(ui, pick(page.names()));
                ui.spacing_mut().item_spacing.y = 2.0;
                for it in items(page) {
                    if it.group {
                        ui.separator();
                    }
                    let label = if de { it.de } else { it.en };
                    let (enabled, hint) = match it.kind {
                        Kind::Run(cmd) => (self.cmd_enabled(cmd), None),
                        Kind::View(_) | Kind::New(_) | Kind::Url => (true, None),
                        Kind::Planned(step) => (false, Some(planned_hint(lang, step))),
                        Kind::Not(reason) => (false, Some(reason.reason(lang).to_string())),
                    };
                    let selected = page == Page::Open && it.kind == Kind::View(open_view);
                    let response = list_row(ui, it.icon, label, enabled, selected);
                    if let Some(hint) = hint {
                        let _ = response.on_disabled_hover_text(format!("{label}\n{hint}"));
                    } else if response.clicked() {
                        clicked = Some(it.kind);
                    }
                }
            });
        match clicked {
            Some(Kind::Run(cmd)) => {
                self.backstage_run(&ctx, cmd);
                return;
            }
            Some(Kind::View(view)) => self.open_view = view,
            Some(Kind::New(kind)) => {
                self.backstage = None;
                self.open_new_document_dialog(kind);
                return;
            }
            Some(Kind::Url) => {
                self.backstage = None;
                self.dialog = Some(Dialog::OpenUrl { url: String::new() });
                return;
            }
            _ => {}
        }

        // Content of the chosen sub-command.
        egui::CentralPanel::default().show(ui, |ui| {
            if page == Page::Open {
                match self.open_view {
                    OpenView::Recent => self.recent_files_page(ui, de),
                    OpenView::Sessions => self.sessions_page(ui, de),
                }
            }
        });
    }

    /// First-page picture of a recent file, rendered once and cached (at
    /// most one new file per frame so the list stays responsive).
    fn recent_thumb(
        &mut self,
        ctx: &egui::Context,
        path: &Path,
        budget: &mut bool,
    ) -> Option<egui::TextureHandle> {
        if let Some(entry) = self.recent_thumbs.get(path) {
            return entry.clone();
        }
        if !*budget {
            ctx.request_repaint();
            return None;
        }
        *budget = false;
        let texture = crate::Document::open(path, None).ok().and_then(|doc| {
            let geometry = doc.geometry(0)?;
            let (w, h) = geometry.view_size();
            let scale = (THUMB.y * 2.0 / h.max(1.0)).min(THUMB.x * 2.0 / w.max(1.0));
            let (pw, ph) = (((w * scale) as u32).max(1), ((h * scale) as u32).max(1));
            let img = doc.render_page(0, pw, ph).ok()?;
            let color = egui::ColorImage::from_rgba_unmultiplied(
                [img.width as usize, img.height as usize],
                &img.rgba,
            );
            Some(ctx.load_texture("recent-thumb", color, egui::TextureOptions::LINEAR))
        });
        self.recent_thumbs.insert(path.to_path_buf(), texture.clone());
        texture
    }

    /// One row of the recent files list. Returns (open, pin toggle, remove).
    fn recent_row(
        &mut self,
        ui: &mut Ui,
        de: bool,
        path: &Path,
        pinned: bool,
        budget: &mut bool,
    ) -> (bool, bool, bool) {
        let exists = path.exists();
        let name = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let thumb = if exists { self.recent_thumb(ui.ctx(), path, budget) } else { None };
        let mut result = (false, false, false);
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(THUMB, Sense::hover());
            match (&thumb, exists) {
                (Some(texture), _) => {
                    let size = texture.size_vec2();
                    let scale = (THUMB.x / size.x).min(THUMB.y / size.y);
                    let r = Rect::from_center_size(rect.center(), size * scale);
                    let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                    ui.painter().image(texture.id(), r, uv, Color32::WHITE);
                    ui.painter().rect_stroke(
                        r,
                        0.0,
                        Stroke::new(1.0, Color32::from_gray(150)),
                        egui::StrokeKind::Outside,
                    );
                }
                (None, true) => {
                    icons::paint(
                        ui.painter(),
                        rect.shrink(4.0),
                        Icon::New,
                        theme::icon_color(ui.visuals().dark_mode),
                    );
                }
                (None, false) => {
                    ui.painter().circle_stroke(
                        rect.center(),
                        10.0,
                        Stroke::new(1.0, ui.visuals().weak_text_color()),
                    );
                    ui.painter().text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        "?",
                        FontId::proportional(14.0),
                        ui.visuals().weak_text_color(),
                    );
                }
            }
            ui.vertical(|ui| {
                let title = RichText::new(&name).strong();
                let response = ui
                    .add_enabled(exists, egui::Button::new(title).frame(false))
                    .on_disabled_hover_text(if de { "Datei nicht gefunden" } else { "File not found" });
                result.0 = response.clicked();
                ui.label(RichText::new(path.display().to_string()).small().weak());
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let hint = if de { "Aus der Liste entfernen" } else { "Remove from list" };
                result.2 = crate::ui::widgets::icon_button(ui, Icon::Close, hint, false, true)
                    .on_hover_text(hint)
                    .clicked();
                let pin = match (pinned, de) {
                    (false, true) => "Anheften",
                    (false, false) => "Pin",
                    (true, true) => "Lösen",
                    (true, false) => "Unpin",
                };
                result.1 = crate::ui::widgets::icon_button(ui, Icon::Bookmarks, pin, pinned, true)
                    .on_hover_text(pin)
                    .clicked();
            });
        });
        ui.separator();
        result
    }

    /// "Öffnen → Letzte Dateien": pinned and recent documents with search.
    fn recent_files_page(&mut self, ui: &mut Ui, de: bool) {
        page_title(ui, if de { "Letzte Dateien" } else { "Recent files" });
        ui.add(
            egui::TextEdit::singleline(&mut self.recent_filter)
                .hint_text(if de { "Suchen…" } else { "Search…" })
                .desired_width(420.0),
        );
        ui.add_space(6.0);
        let filter = self.recent_filter.trim().to_lowercase();
        let matches = |p: &PathBuf| filter.is_empty() || p.to_string_lossy().to_lowercase().contains(&filter);
        let pinned: Vec<PathBuf> =
            self.settings.pinned_files.iter().filter(|p| matches(p)).cloned().collect();
        let unpinned: Vec<PathBuf> = self
            .settings
            .recent_files
            .iter()
            .filter(|p| !self.settings.pinned_files.contains(p) && matches(p))
            .cloned()
            .collect();
        let mut open = None;
        let mut pin = None;
        let mut remove = None;
        let mut budget = true;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .max_height(ui.available_height() - 70.0)
            .show(ui, |ui| {
                for (is_pinned, list, de_title, en_title) in [
                    (true, &pinned, "Angeheftete Dokumente", "Pinned documents"),
                    (false, &unpinned, "Nicht angeheftete Dokumente", "Unpinned documents"),
                ] {
                    let title = format!("{} ({})", if de { de_title } else { en_title }, list.len());
                    egui::CollapsingHeader::new(RichText::new(title).strong())
                        .id_salt(("recent-group", is_pinned))
                        .default_open(true)
                        .show(ui, |ui| {
                            for path in list {
                                let (o, p, r) = self.recent_row(ui, de, path, is_pinned, &mut budget);
                                if o {
                                    open = Some(path.clone());
                                }
                                if p {
                                    pin = Some(path.clone());
                                }
                                if r {
                                    remove = Some(path.clone());
                                }
                            }
                        });
                }
            });
        ui.separator();
        ui.horizontal(|ui| {
            if ui
                .button(if de { "Nicht gefundene Dateien entfernen" } else { "Remove missing files" })
                .clicked()
            {
                self.settings.recent_files.retain(|p| p.exists());
                self.settings.pinned_files.retain(|p| p.exists());
            }
            if ui
                .button(if de {
                    "Verlauf löschen (ohne angeheftete)"
                } else {
                    "Clear history (keep pinned)"
                })
                .clicked()
            {
                self.settings.recent_files.clear();
                self.recent_thumbs.clear();
            }
        });
        if let Some(path) = pin {
            self.settings.toggle_pin(&path);
        }
        if let Some(path) = remove {
            self.settings.recent_files.retain(|p| p != &path);
            self.settings.pinned_files.retain(|p| p != &path);
            self.recent_thumbs.remove(&path);
        }
        if let Some(path) = open {
            self.backstage = None;
            self.pending_open.push_back(path);
        }
    }

    /// The open documents that are saved to disk as a session.
    pub(super) fn current_session(&self, name: String) -> Option<Session> {
        let mut active = 0;
        let mut files = Vec::new();
        for (i, tab) in self.tabs.iter().enumerate() {
            if let Some(path) = tab.doc.path() {
                if i == self.active {
                    active = files.len();
                }
                files.push(path.to_path_buf());
            }
        }
        (!files.is_empty()).then(|| Session { name, files, active, saved: now_text(self.settings.language) })
    }

    /// Opens the documents of a session (missing files are skipped).
    fn open_session(&mut self, session: &Session) {
        let files: Vec<PathBuf> = session.files.iter().filter(|p| p.exists()).cloned().collect();
        let missing = session.files.len() - files.len();
        self.backstage = None;
        self.pending_open.extend(files);
        if missing > 0 {
            let de = self.settings.language == Language::German;
            self.dialog = Some(Dialog::Message {
                title: if de { "Sitzung" } else { "Session" }.into(),
                text: if de {
                    format!("{missing} Dokument(e) der Sitzung wurden nicht gefunden.")
                } else {
                    format!("{missing} document(s) of the session were not found.")
                },
            });
        }
    }

    /// "Öffnen → Sitzungen": last session, saved sessions, session files.
    fn sessions_page(&mut self, ui: &mut Ui, de: bool) {
        page_title(ui, if de { "Sitzungen" } else { "Sessions" });
        let tr = |g: &'static str, e: &'static str| if de { g } else { e };
        let mut open: Option<Session> = None;
        let mut delete = None;
        let summary = |s: &Session| {
            format!(
                "{} {}, {}: {}",
                tr("Erstellt", "Created"),
                s.saved,
                tr("Dokumente", "Documents"),
                s.files.len()
            )
        };
        if !self.settings.last_session.files.is_empty() {
            let last = self.settings.last_session.clone();
            ui.horizontal(|ui| {
                if ui.button(RichText::new(tr("Letzte Sitzung", "Last session")).strong()).clicked() {
                    open = Some(last.clone());
                }
                ui.label(RichText::new(summary(&last)).weak());
            });
        }
        ui.add_space(6.0);
        ui.label(RichText::new(tr("Gespeicherte Sitzungen", "Saved sessions")).strong());
        if self.settings.sessions.is_empty() {
            ui.label(RichText::new(tr("(keine)", "(none)")).weak());
        }
        for (i, session) in self.settings.sessions.iter().enumerate() {
            ui.horizontal(|ui| {
                if ui
                    .button(session.name.as_str())
                    .on_hover_text(tr("Sitzung öffnen", "Open session"))
                    .clicked()
                {
                    open = Some(session.clone());
                }
                ui.label(RichText::new(summary(session)).weak());
                let hint = tr("Sitzung löschen", "Delete session");
                if crate::ui::widgets::icon_button(ui, Icon::Close, hint, false, true)
                    .on_hover_text(hint)
                    .clicked()
                {
                    delete = Some(i);
                }
            });
        }
        ui.separator();
        let current = self.current_session(String::new());
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.session_name)
                    .hint_text(tr("Name der Sitzung", "Session name"))
                    .desired_width(220.0),
            );
            let name = self.session_name.trim().to_string();
            if ui
                .add_enabled(current.is_some() && !name.is_empty(), egui::Button::new(tr("Aktuelle Sitzung speichern", "Save current session")))
                .on_disabled_hover_text(tr(
                    "Es muss mindestens ein gespeichertes Dokument geöffnet sein und ein Name eingegeben werden.",
                    "At least one saved document must be open and a name entered.",
                ))
                .clicked()
                && let Some(mut session) = current.clone()
            {
                session.name = name;
                self.settings.store_session(session);
                self.session_name.clear();
            }
        });
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    current.is_some(),
                    egui::Button::new(tr(
                        "Aktuelle Sitzung in eine Datei speichern…",
                        "Save current session to a file…",
                    )),
                )
                .clicked()
                && let Some(session) = current.clone()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter(tr("MrPDF-Sitzung", "MrPDF session"), &[SESSION_EXT])
                    .set_file_name(format!("{}.{SESSION_EXT}", tr("Sitzung", "Session")))
                    .save_file()
                && let Err(err) = crate::ui::settings::write_session(&path, &session)
            {
                self.show_error(&err);
            }
            if ui.button(tr("Sitzung aus einer Datei öffnen…", "Open session from a file…")).clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter(tr("MrPDF-Sitzung", "MrPDF session"), &[SESSION_EXT])
                    .pick_file()
            {
                match crate::ui::settings::read_session(&path) {
                    Ok(session) => open = Some(session),
                    Err(err) => self.show_error(&err),
                }
            }
        });
        ui.add_space(6.0);
        ui.checkbox(
            &mut self.settings.restore_session,
            tr("Letzte Sitzung beim Start wiederherstellen", "Restore the last session at start"),
        );
        if let Some(i) = delete {
            self.settings.sessions.remove(i);
        }
        if let Some(session) = open {
            self.open_session(&session);
        }
    }

    /// Starts "Von URL öffnen" in the background.
    pub(super) fn start_download(&mut self, ctx: &egui::Context, url: String) {
        self.settings.url_history.retain(|u| u != &url);
        self.settings.url_history.insert(0, url.clone());
        self.settings.url_history.truncate(20);
        let (tx, rx) = std::sync::mpsc::channel();
        let dir = std::env::temp_dir().join("mrpdf-downloads");
        let address = url.clone();
        let repaint = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(crate::download::download_pdf(&address, &dir));
            repaint.request_repaint();
        });
        self.download = Some((url, rx));
        let de = self.settings.language == Language::German;
        self.set_status(ctx, if de { "Wird heruntergeladen …" } else { "Downloading …" });
    }

    /// Opens a finished download.
    pub(super) fn poll_download(&mut self, ctx: &egui::Context) {
        let Some((url, rx)) = &self.download else { return };
        match rx.try_recv() {
            Ok(Ok(path)) => {
                self.download = None;
                self.open_path(ctx, path.clone(), None);
                // The temporary copy is not a "recent file".
                self.settings
                    .recent_files
                    .retain(|p| p.canonicalize().ok().as_deref() != path.canonicalize().ok().as_deref());
            }
            Ok(Err(err)) => {
                let url = url.clone();
                self.download = None;
                let de = self.settings.language == Language::German;
                let reason = match err {
                    crate::Error::InvalidFormat => {
                        if de {
                            "Die Adresse liefert keine PDF-Datei.".to_string()
                        } else {
                            "The address does not deliver a PDF file.".to_string()
                        }
                    }
                    other => other.to_string(),
                };
                self.dialog = Some(Dialog::Message {
                    title: if de { "Von URL öffnen" } else { "Open from URL" }.into(),
                    text: format!("{url}\n\n{reason}"),
                });
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(200))
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.download = None,
        }
    }
}

/// Size of the pictures in the recent files list.
const THUMB: Vec2 = Vec2::new(30.0, 40.0);
/// File extension of session files.
const SESSION_EXT: &str = "mrpdfsession";

/// Current date and time for display ("26.10.2026, 15:31").
fn now_text(lang: Language) -> String {
    let d = crate::pdf_edit::pdf_date_now();
    let digits: String = d.chars().filter(|c| c.is_ascii_digit()).take(12).collect();
    if digits.len() < 12 {
        return String::new();
    }
    let (y, m, day, h, min) = (&digits[0..4], &digits[4..6], &digits[6..8], &digits[8..10], &digits[10..12]);
    if lang == Language::German {
        format!("{day}.{m}.{y}, {h}:{min}")
    } else {
        format!("{y}-{m}-{day} {h}:{min}")
    }
}
