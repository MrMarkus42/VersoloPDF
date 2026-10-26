//! The "Datei" backstage view: a full-window page with a command column on
//! the left, a list of sub-commands in the middle and their content on the
//! right.

use egui::{
    Align2, Color32, FontId, Frame, Margin, Panel, Rect, RichText, Sense, Stroke, Ui, Vec2, WidgetInfo,
    WidgetType, vec2,
};

use super::MrPdfApp;
use super::commands::{Cmd, Unavailable};
use crate::ui::i18n::Language;
use crate::ui::icons::{self, Icon};
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

/// What a sub-command in the middle list does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Run(Cmd),
    /// Shows the recent files on the right.
    Recent,
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
                    item("Letzte Dateien", "Recent files", Icon::Open, Recent),
                    item("Sitzungen", "Sessions", Icon::Thumbnails, Planned('H')),
                    item("Von URL öffnen", "Open from URL", Icon::Web, Planned('H')),
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
                    item("Mit leeren Seiten", "With blank pages", Icon::New, Run(Cmd::New)),
                    group("Dateien zusammenfassen", "Combine files", Icon::Merge, Run(Cmd::FromFiles)),
                    item("PDF-Portfolio", "PDF portfolio", Icon::Attachment, Not(Unavailable::Portfolio)),
                    group("Aus Text", "From text", Icon::TextFile, Planned('H')),
                    item("Aus Markdown", "From Markdown", Icon::TextFile, Planned('H')),
                    item("Aus CSV-Datei", "From CSV file", Icon::TextFile, Planned('H')),
                    item("Aus Rich-Text (RTF)", "From rich text (RTF)", Icon::TextFile, Planned('H')),
                    item("Aus Web-Seite", "From web page", Icon::Web, Not(Unavailable::WebImport)),
                    item("Aus E-Mail", "From e-mail", Icon::Mail, Not(Unavailable::WebImport)),
                    item("Aus Bilddateien", "From image files", Icon::Image, Run(Cmd::FromFiles)),
                    item("Von Scanner", "From scanner", Icon::Camera, Not(Unavailable::Scanner)),
                    group("Aus Zwischenablage", "From clipboard", Icon::Copy, Planned('H')),
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
        let mut selected_kind = if page == Page::Open { Some(Kind::Recent) } else { None };
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
                        Kind::Recent => (true, None),
                        Kind::Planned(step) => (false, Some(planned_hint(lang, step))),
                        Kind::Not(reason) => (false, Some(reason.reason(lang).to_string())),
                    };
                    let selected = selected_kind == Some(it.kind);
                    let response = list_row(ui, it.icon, label, enabled, selected);
                    if let Some(hint) = hint {
                        let _ = response.on_disabled_hover_text(format!("{label}\n{hint}"));
                    } else if response.clicked() {
                        match it.kind {
                            Kind::Run(cmd) => chosen = Some(cmd),
                            kind => selected_kind = Some(kind),
                        }
                    }
                }
            });
        if let Some(cmd) = chosen {
            self.backstage_run(&ctx, cmd);
            return;
        }

        // Content of the chosen sub-command.
        egui::CentralPanel::default().show(ui, |ui| {
            if selected_kind == Some(Kind::Recent) {
                self.recent_files_page(ui, de);
            }
        });
    }

    /// "Öffnen → Letzte Dateien": filterable list of recently used files.
    fn recent_files_page(&mut self, ui: &mut Ui, de: bool) {
        page_title(ui, if de { "Letzte Dateien" } else { "Recent files" });
        ui.add(
            egui::TextEdit::singleline(&mut self.recent_filter)
                .hint_text(if de { "Suchen…" } else { "Search…" })
                .desired_width(420.0),
        );
        ui.add_space(6.0);
        let filter = self.recent_filter.trim().to_lowercase();
        let files: Vec<_> = self
            .settings
            .recent_files
            .iter()
            .filter(|p| filter.is_empty() || p.to_string_lossy().to_lowercase().contains(&filter))
            .cloned()
            .collect();
        ui.label(
            RichText::new(format!(
                "{} ({})",
                if de { "Zuletzt geöffnete Dokumente" } else { "Recently opened documents" },
                files.len()
            ))
            .strong(),
        );
        if files.is_empty() {
            ui.label(RichText::new(if de { "(keine)" } else { "(none)" }).weak());
            return;
        }
        let mut open = None;
        let mut remove = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for path in files {
                let exists = path.exists();
                let name = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
                    if exists {
                        icons::paint(
                            ui.painter(),
                            rect,
                            Icon::New,
                            theme::icon_color(ui.visuals().dark_mode),
                        );
                    } else {
                        ui.painter().text(
                            rect.center(),
                            Align2::CENTER_CENTER,
                            "?",
                            FontId::proportional(16.0),
                            ui.visuals().weak_text_color(),
                        );
                    }
                    ui.vertical(|ui| {
                        let title = RichText::new(&name).strong();
                        let response = ui
                            .add_enabled(exists, egui::Button::new(title).frame(false))
                            .on_disabled_hover_text(if de {
                                "Datei nicht gefunden"
                            } else {
                                "File not found"
                            });
                        if response.clicked() {
                            open = Some(path.clone());
                        }
                        ui.label(RichText::new(path.display().to_string()).small().weak());
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let hint = if de { "Aus der Liste entfernen" } else { "Remove from list" };
                        if crate::ui::widgets::icon_button(ui, Icon::Close, hint, false, true)
                            .on_hover_text(hint)
                            .clicked()
                        {
                            remove = Some(path.clone());
                        }
                    });
                });
                ui.separator();
            }
        });
        if let Some(path) = remove {
            self.settings.recent_files.retain(|p| p != &path);
        }
        if let Some(path) = open {
            self.backstage = None;
            self.pending_open.push_back(path);
        }
    }
}
