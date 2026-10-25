//! "Dokumentaufbau": page structure commands and their dialogs.

use std::collections::HashMap;

use egui::{Context, RichText, Ui};

use super::actions::parse_page_range;
use super::commands::Cmd;
use super::dialogs::{Action, Outcome, buttons, paper_name};
use super::{Dialog, MrPdfApp, PaperChoice};
use crate::model::Rgba;
use crate::pages::{Anchor, LabelStyle, PageMark, TextMark, bates_number, expand_placeholders};
use crate::ui::i18n::Language;
use crate::ui::widgets::{from_color32, to_color32};

const MM: f32 = 72.0 / 25.4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkKind {
    Watermark,
    Background,
    HeaderFooter,
    Bates,
}

/// Input of the watermark / background / header-footer / Bates dialog.
#[derive(Debug, Clone, PartialEq)]
pub struct MarkForm {
    pub kind: MarkKind,
    pub range: String,
    pub text: String,
    pub font_size: f32,
    pub color: Rgba,
    pub opacity: f32,
    pub rotation: f32,
    pub behind: bool,
    pub fill: Rgba,
    /// Header left/centre/right, footer left/centre/right.
    pub texts: [String; 6],
    pub margin_mm: f32,
    pub prefix: String,
    pub start: u32,
    pub digits: u32,
    pub suffix: String,
    pub bates_anchor: Anchor,
    pub error: bool,
}

impl MarkForm {
    fn new(kind: MarkKind, pages: usize, german: bool) -> Self {
        Self {
            kind,
            range: format!("1-{pages}"),
            text: if german { "ENTWURF".into() } else { "DRAFT".into() },
            font_size: if kind == MarkKind::Watermark { 72.0 } else { 10.0 },
            color: if kind == MarkKind::Watermark { Rgba::rgb(200, 30, 30) } else { Rgba::BLACK },
            opacity: if kind == MarkKind::Watermark { 0.3 } else { 1.0 },
            rotation: 45.0,
            behind: false,
            fill: Rgba::rgb(255, 250, 225),
            texts: [
                String::new(),
                String::new(),
                String::new(),
                "<<file>>".into(),
                String::new(),
                if german {
                    "Seite <<page>> von <<pages>>".into()
                } else {
                    "Page <<page>> of <<pages>>".into()
                },
            ],
            margin_mm: 10.0,
            prefix: String::new(),
            start: 1,
            digits: 6,
            suffix: String::new(),
            bates_anchor: Anchor::BottomRight,
            error: false,
        }
    }
}

fn label_style_name(style: LabelStyle, de: bool) -> &'static str {
    match style {
        LabelStyle::Decimal => "1, 2, 3",
        LabelStyle::RomanLower => "i, ii, iii",
        LabelStyle::RomanUpper => "I, II, III",
        LabelStyle::LettersLower => "a, b, c",
        LabelStyle::LettersUpper => "A, B, C",
        LabelStyle::None => {
            if de {
                "(nur Präfix)"
            } else {
                "(prefix only)"
            }
        }
    }
}

/// Today's date for headers/footers.
fn today(de: bool) -> String {
    let d = crate::pdf_edit::pdf_date_now();
    let digits: String = d.chars().filter(|c| c.is_ascii_digit()).take(8).collect();
    if digits.len() < 8 {
        return String::new();
    }
    let (y, m, day) = (&digits[0..4], &digits[4..6], &digits[6..8]);
    if de { format!("{day}.{m}.{y}") } else { format!("{y}-{m}-{day}") }
}

impl MrPdfApp {
    fn de(&self) -> bool {
        self.settings.language == Language::German
    }

    fn page_count(&self) -> usize {
        self.active_tab().map_or(0, |t| t.doc.page_count())
    }

    fn current_page(&self) -> usize {
        self.active_tab().map_or(0, |t| t.current_page)
    }

    /// Starts a "Dokumentaufbau" command. Returns false if `cmd` is not one.
    pub(super) fn run_structure(&mut self, ctx: &Context, cmd: Cmd) -> bool {
        let n = self.page_count();
        let cur = self.current_page();
        let de = self.de();
        match cmd {
            Cmd::MergeFiles => self.merge_files(ctx),
            Cmd::SplitDocument => self.dialog = Some(Dialog::Split { every: 1 }),
            Cmd::DuplicatePages => {
                self.dialog = Some(Dialog::PageRange {
                    purpose: super::RangePurpose::Duplicate,
                    text: (cur + 1).to_string(),
                    error: false,
                })
            }
            Cmd::ReplacePages => {
                self.dialog =
                    Some(Dialog::Replace { range: (cur + 1).to_string(), source_start: 1, error: false })
            }
            Cmd::SwapPages => {
                self.dialog = Some(Dialog::Swap { a: cur as u32 + 1, b: (cur as u32 + 2).min(n as u32) })
            }
            Cmd::ReversePages => self.edit(ctx, |tab| tab.doc.reverse_pages()),
            Cmd::NormalizePages => {
                self.dialog = Some(Dialog::Resize {
                    range: format!("1-{n}"),
                    paper: PaperChoice::SameAsCurrent,
                    landscape: false,
                    fit: true,
                    error: false,
                })
            }
            Cmd::ResizePages => {
                self.dialog = Some(Dialog::Resize {
                    range: (cur + 1).to_string(),
                    paper: PaperChoice::A4,
                    landscape: false,
                    fit: true,
                    error: false,
                })
            }
            Cmd::FindDuplicatePages => {
                let result = self.active_tab().map(|t| t.doc.find_duplicate_pages());
                match result {
                    Some(Ok(groups)) if groups.is_empty() => {
                        let t = self.t();
                        let text = if de {
                            "Es wurden keine doppelten Seiten gefunden."
                        } else {
                            "No duplicate pages found."
                        };
                        self.dialog = Some(Dialog::Message {
                            title: Cmd::FindDuplicatePages.label(self.settings.language).into(),
                            text: text.into(),
                        });
                        let _ = t;
                    }
                    Some(Ok(groups)) => self.dialog = Some(Dialog::Duplicates { groups }),
                    Some(Err(err)) => self.show_error(&err),
                    None => {}
                }
            }
            Cmd::CropPages => {
                self.dialog =
                    Some(Dialog::Margins { range: format!("1-{n}"), mm: [10.0; 4], crop: true, error: false })
            }
            Cmd::Margins => {
                self.dialog = Some(Dialog::Margins {
                    range: format!("1-{n}"),
                    mm: [10.0; 4],
                    crop: false,
                    error: false,
                })
            }
            Cmd::Watermark => {
                self.dialog = Some(Dialog::PageMark(Box::new(MarkForm::new(MarkKind::Watermark, n, de))))
            }
            Cmd::Background => {
                self.dialog = Some(Dialog::PageMark(Box::new(MarkForm::new(MarkKind::Background, n, de))))
            }
            Cmd::HeaderFooter => {
                self.dialog = Some(Dialog::PageMark(Box::new(MarkForm::new(MarkKind::HeaderFooter, n, de))))
            }
            Cmd::Bates => {
                self.dialog = Some(Dialog::PageMark(Box::new(MarkForm::new(MarkKind::Bates, n, de))))
            }
            Cmd::PageLabels => {
                self.dialog = Some(Dialog::PageLabels {
                    start: cur as u32 + 1,
                    style: LabelStyle::Decimal,
                    prefix: String::new(),
                    first: 1,
                })
            }
            _ => return false,
        }
        true
    }

    fn merge_files(&mut self, ctx: &Context) {
        let title = Cmd::MergeFiles.label(self.settings.language);
        let Some(files) =
            rfd::FileDialog::new().set_title(title).add_filter("PDF", &["pdf", "PDF"]).pick_files()
        else {
            return;
        };
        let name = if self.de() { "Zusammengeführt.pdf" } else { "Merged.pdf" };
        match crate::Document::merge_files(&files, name) {
            Ok(doc) => {
                self.add_tab(doc);
                ctx.request_repaint();
            }
            Err(err) => self.show_error(&err),
        }
    }

    fn split_document(&mut self, ctx: &Context, every: usize) {
        let Some(tab) = self.active_tab() else { return };
        let stem = tab.doc.display_name().trim_end_matches(".pdf").trim_end_matches(".PDF").to_string();
        let Some(dir) =
            rfd::FileDialog::new().set_title(Cmd::SplitDocument.label(self.settings.language)).pick_folder()
        else {
            return;
        };
        let result = self.active_tab().map(|t| t.doc.split(every, &dir, &stem));
        match result {
            Some(Ok(files)) => {
                let msg = if self.de() {
                    format!("{} Dateien geschrieben", files.len())
                } else {
                    format!("{} files written", files.len())
                };
                self.set_status(ctx, msg);
            }
            Some(Err(err)) => self.show_error(&err),
            None => {}
        }
    }

    fn apply_mark(&mut self, ctx: &Context, form: &MarkForm, pages: Vec<usize>) {
        let total = self.page_count();
        let file = self.active_tab().map(|t| t.doc.display_name().to_string()).unwrap_or_default();
        let date = today(self.de());
        let mark = match form.kind {
            MarkKind::Background => PageMark::Fill { color: form.fill },
            MarkKind::Watermark => PageMark::Texts {
                marks: vec![TextMark {
                    text: form.text.clone(),
                    anchor: Anchor::Center,
                    font_size: form.font_size,
                    color: form.color,
                    opacity: form.opacity,
                    rotation: form.rotation,
                    margin: 0.0,
                    bold: true,
                }],
                behind: form.behind,
            },
            MarkKind::HeaderFooter => {
                let anchors = [
                    Anchor::TopLeft,
                    Anchor::TopCenter,
                    Anchor::TopRight,
                    Anchor::BottomLeft,
                    Anchor::BottomCenter,
                    Anchor::BottomRight,
                ];
                PageMark::Texts {
                    marks: form
                        .texts
                        .iter()
                        .zip(anchors)
                        .filter(|(text, _)| !text.trim().is_empty())
                        .map(|(text, anchor)| TextMark {
                            text: text.clone(),
                            anchor,
                            font_size: form.font_size,
                            color: form.color,
                            opacity: 1.0,
                            rotation: 0.0,
                            margin: form.margin_mm * MM,
                            bold: false,
                        })
                        .collect(),
                    behind: false,
                }
            }
            MarkKind::Bates => PageMark::Texts {
                marks: vec![TextMark {
                    text: String::new(),
                    anchor: form.bates_anchor,
                    font_size: form.font_size,
                    color: form.color,
                    opacity: 1.0,
                    rotation: 0.0,
                    margin: form.margin_mm * MM,
                    bold: false,
                }],
                behind: false,
            },
        };
        let position: HashMap<usize, usize> = pages.iter().enumerate().map(|(i, p)| (*p, i)).collect();
        let (kind, prefix, start, digits, suffix) =
            (form.kind, form.prefix.clone(), form.start, form.digits as usize, form.suffix.clone());
        let text_for = move |text: &str, page: usize| -> String {
            if kind == MarkKind::Bates {
                let n = u64::from(start) + position.get(&page).copied().unwrap_or(0) as u64;
                bates_number(&prefix, n, digits, &suffix)
            } else {
                expand_placeholders(text, page, total, &date, &file)
            }
        };
        self.edit(ctx, |tab| tab.doc.add_page_marks(&pages, &mark, &text_for));
    }

    /// Renders the dialogs of this module.
    pub(super) fn structure_dialog(&mut self, ui: &mut Ui, dialog: &mut Dialog) -> (Outcome, Option<Action>) {
        let t = self.t();
        let de = self.de();
        let lang = self.settings.language;
        let n = self.page_count();
        let red = egui::Color32::from_rgb(200, 40, 40);
        let mut outcome = Outcome::Keep;
        let mut action: Option<Action> = None;
        match dialog {
            Dialog::Split { every } => {
                ui.heading(Cmd::SplitDocument.label(lang));
                ui.horizontal(|ui| {
                    ui.label(if de { "Seiten je Datei:" } else { "Pages per file:" });
                    ui.add(egui::DragValue::new(every).range(1..=n.max(1) as u32));
                });
                let (ok, cancel) = buttons(ui, t.ok, t.cancel);
                if ok {
                    let every = *every as usize;
                    action = Some(Box::new(move |app, ctx| app.split_document(ctx, every)));
                    outcome = Outcome::Close;
                } else if cancel {
                    outcome = Outcome::Close;
                }
            }
            Dialog::Replace { range, source_start, error } => {
                ui.heading(Cmd::ReplacePages.label(lang));
                ui.label(t.page_range_label);
                ui.text_edit_singleline(range);
                ui.horizontal(|ui| {
                    ui.label(if de { "Ab Seite der anderen Datei:" } else { "From page of the other file:" });
                    ui.add(egui::DragValue::new(source_start).range(1..=9999));
                });
                if *error {
                    ui.colored_label(red, t.invalid_range);
                }
                let (ok, cancel) = buttons(ui, t.ok, t.cancel);
                if ok {
                    match parse_page_range(range, n) {
                        Some(pages) => {
                            let start = source_start.saturating_sub(1) as usize;
                            action = Some(Box::new(move |app, ctx| {
                                let title = Cmd::ReplacePages.label(app.settings.language);
                                if let Some(path) = rfd::FileDialog::new()
                                    .set_title(title)
                                    .add_filter("PDF", &["pdf", "PDF"])
                                    .pick_file()
                                {
                                    app.edit(ctx, |tab| tab.doc.replace_pages(&pages, &path, start));
                                }
                            }));
                            outcome = Outcome::Close;
                        }
                        None => *error = true,
                    }
                } else if cancel {
                    outcome = Outcome::Close;
                }
            }
            Dialog::Swap { a, b } => {
                ui.heading(Cmd::SwapPages.label(lang));
                ui.horizontal(|ui| {
                    ui.label(t.page);
                    ui.add(egui::DragValue::new(a).range(1..=n.max(1) as u32));
                    ui.label(if de { "mit Seite" } else { "with page" });
                    ui.add(egui::DragValue::new(b).range(1..=n.max(1) as u32));
                });
                let (ok, cancel) = buttons(ui, t.ok, t.cancel);
                if ok {
                    let (a, b) = (*a as usize - 1, *b as usize - 1);
                    action = Some(Box::new(move |app, ctx| app.edit(ctx, |tab| tab.doc.swap_pages(a, b))));
                    outcome = Outcome::Close;
                } else if cancel {
                    outcome = Outcome::Close;
                }
            }
            Dialog::Resize { range, paper, landscape, fit, error } => {
                ui.heading(Cmd::ResizePages.label(lang));
                egui::Grid::new("resize").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    ui.label(t.page_range_label);
                    ui.text_edit_singleline(range);
                    ui.end_row();
                    ui.label(t.paper_size);
                    egui::ComboBox::from_id_salt("resize-paper")
                        .selected_text(paper_name(t, *paper))
                        .show_ui(ui, |ui| {
                            for p in [
                                PaperChoice::SameAsCurrent,
                                PaperChoice::A4,
                                PaperChoice::A3,
                                PaperChoice::A5,
                                PaperChoice::Letter,
                                PaperChoice::Legal,
                            ] {
                                ui.selectable_value(paper, p, paper_name(t, p));
                            }
                        });
                    ui.end_row();
                    ui.label(t.orientation);
                    ui.horizontal(|ui| {
                        ui.radio_value(landscape, false, t.portrait);
                        ui.radio_value(landscape, true, t.landscape);
                    });
                    ui.end_row();
                });
                ui.checkbox(
                    fit,
                    if de { "Inhalt an die neue Größe anpassen" } else { "Scale content to the new size" },
                );
                if *error {
                    ui.colored_label(red, t.invalid_range);
                }
                let (ok, cancel) = buttons(ui, t.ok, t.cancel);
                if ok {
                    match parse_page_range(range, n) {
                        Some(pages) => {
                            let (paper, landscape, fit) = (*paper, *landscape, *fit);
                            action = Some(Box::new(move |app, ctx| {
                                let size = paper.size().unwrap_or_else(|| {
                                    app.active_tab()
                                        .and_then(|t| t.doc.geometry(t.current_page).map(|g| g.view_size()))
                                        .unwrap_or((595.28, 841.89))
                                });
                                let (w, h) = size;
                                let size = if landscape == (w > h) { (w, h) } else { (h, w) };
                                app.edit(ctx, |tab| tab.doc.resize_pages(&pages, size, fit));
                            }));
                            outcome = Outcome::Close;
                        }
                        None => *error = true,
                    }
                } else if cancel {
                    outcome = Outcome::Close;
                }
            }
            Dialog::Margins { range, mm, crop, error } => {
                ui.heading(if *crop { Cmd::CropPages.label(lang) } else { Cmd::Margins.label(lang) });
                ui.label(t.page_range_label);
                ui.text_edit_singleline(range);
                let names = if de {
                    ["Links", "Unten", "Rechts", "Oben"]
                } else {
                    ["Left", "Bottom", "Right", "Top"]
                };
                egui::Grid::new("margins").num_columns(2).show(ui, |ui| {
                    for (i, name) in names.iter().enumerate() {
                        ui.label(*name);
                        ui.add(egui::DragValue::new(&mut mm[i]).range(0.0..=500.0).suffix(" mm").speed(0.5));
                        ui.end_row();
                    }
                });
                if *error {
                    ui.colored_label(red, t.invalid_range);
                }
                let (ok, cancel) = buttons(ui, t.ok, t.cancel);
                if ok {
                    match parse_page_range(range, n) {
                        Some(pages) => {
                            let pts = mm.map(|v| v * MM);
                            let crop = *crop;
                            action = Some(Box::new(move |app, ctx| {
                                app.edit(ctx, |tab| {
                                    if crop {
                                        tab.doc.crop_pages(&pages, pts)
                                    } else {
                                        tab.doc.add_margins(&pages, pts)
                                    }
                                })
                            }));
                            outcome = Outcome::Close;
                        }
                        None => *error = true,
                    }
                } else if cancel {
                    outcome = Outcome::Close;
                }
            }
            Dialog::PageMark(form) => {
                let title = match form.kind {
                    MarkKind::Watermark => Cmd::Watermark,
                    MarkKind::Background => Cmd::Background,
                    MarkKind::HeaderFooter => Cmd::HeaderFooter,
                    MarkKind::Bates => Cmd::Bates,
                };
                ui.heading(title.label(lang));
                egui::Grid::new("mark").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    ui.label(t.page_range_label);
                    ui.text_edit_singleline(&mut form.range);
                    ui.end_row();
                    match form.kind {
                        MarkKind::Watermark => {
                            ui.label(t.text);
                            ui.text_edit_singleline(&mut form.text);
                            ui.end_row();
                            ui.label(if de { "Drehung" } else { "Rotation" });
                            ui.add(egui::Slider::new(&mut form.rotation, -90.0..=90.0).suffix("°"));
                            ui.end_row();
                            ui.label(t.opacity);
                            let mut pct = form.opacity * 100.0;
                            if ui
                                .add(egui::Slider::new(&mut pct, 5.0..=100.0).suffix(" %").max_decimals(0))
                                .changed()
                            {
                                form.opacity = pct / 100.0;
                            }
                            ui.end_row();
                        }
                        MarkKind::Background => {
                            ui.label(t.color);
                            let mut c = to_color32(form.fill);
                            if ui.color_edit_button_srgba(&mut c).changed() {
                                form.fill = from_color32(c);
                            }
                            ui.end_row();
                        }
                        MarkKind::HeaderFooter => {
                            let labels = if de {
                                [
                                    "Kopf links",
                                    "Kopf Mitte",
                                    "Kopf rechts",
                                    "Fuß links",
                                    "Fuß Mitte",
                                    "Fuß rechts",
                                ]
                            } else {
                                [
                                    "Header left",
                                    "Header centre",
                                    "Header right",
                                    "Footer left",
                                    "Footer centre",
                                    "Footer right",
                                ]
                            };
                            for (i, label) in labels.iter().enumerate() {
                                ui.label(*label);
                                ui.text_edit_singleline(&mut form.texts[i]);
                                ui.end_row();
                            }
                        }
                        MarkKind::Bates => {
                            ui.label(if de { "Präfix" } else { "Prefix" });
                            ui.text_edit_singleline(&mut form.prefix);
                            ui.end_row();
                            ui.label(if de { "Startnummer" } else { "Start number" });
                            ui.add(egui::DragValue::new(&mut form.start).range(0..=999_999_999));
                            ui.end_row();
                            ui.label(if de { "Stellen" } else { "Digits" });
                            ui.add(egui::DragValue::new(&mut form.digits).range(1..=12));
                            ui.end_row();
                            ui.label("Suffix");
                            ui.text_edit_singleline(&mut form.suffix);
                            ui.end_row();
                            ui.label("Position");
                            ui.horizontal(|ui| {
                                for (anchor, name) in [
                                    (Anchor::BottomLeft, if de { "unten links" } else { "bottom left" }),
                                    (Anchor::BottomCenter, if de { "unten Mitte" } else { "bottom centre" }),
                                    (Anchor::BottomRight, if de { "unten rechts" } else { "bottom right" }),
                                    (Anchor::TopRight, if de { "oben rechts" } else { "top right" }),
                                ] {
                                    ui.radio_value(&mut form.bates_anchor, anchor, name);
                                }
                            });
                            ui.end_row();
                        }
                    }
                    if form.kind != MarkKind::Background {
                        ui.label(t.font_size);
                        ui.add(egui::DragValue::new(&mut form.font_size).range(4.0..=300.0).suffix(" pt"));
                        ui.end_row();
                        ui.label(t.color);
                        let mut c = to_color32(form.color);
                        if ui.color_edit_button_srgba(&mut c).changed() {
                            form.color = from_color32(c);
                        }
                        ui.end_row();
                    }
                    if matches!(form.kind, MarkKind::HeaderFooter | MarkKind::Bates) {
                        ui.label(if de { "Randabstand" } else { "Margin" });
                        ui.add(egui::DragValue::new(&mut form.margin_mm).range(0.0..=100.0).suffix(" mm"));
                        ui.end_row();
                    }
                });
                if form.kind == MarkKind::Watermark {
                    ui.checkbox(
                        &mut form.behind,
                        if de { "Hinter dem Seiteninhalt" } else { "Behind the page content" },
                    );
                }
                if form.kind == MarkKind::HeaderFooter {
                    ui.label(
                        RichText::new(if de {
                            "Platzhalter: <<page>> Seite, <<pages>> Seitenzahl, <<date>> Datum, <<file>> Dateiname"
                        } else {
                            "Placeholders: <<page>> page, <<pages>> page count, <<date>> date, <<file>> file name"
                        })
                        .small()
                        .weak(),
                    );
                }
                if form.error {
                    ui.colored_label(red, t.invalid_range);
                }
                let (ok, cancel) = buttons(ui, t.ok, t.cancel);
                if ok {
                    match parse_page_range(&form.range, n) {
                        Some(pages) => {
                            let form = (**form).clone();
                            action = Some(Box::new(move |app, ctx| app.apply_mark(ctx, &form, pages)));
                            outcome = Outcome::Close;
                        }
                        None => form.error = true,
                    }
                } else if cancel {
                    outcome = Outcome::Close;
                }
            }
            Dialog::PageLabels { start, style, prefix, first } => {
                ui.heading(Cmd::PageLabels.label(lang));
                egui::Grid::new("labels").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    ui.label(if de { "Ab Seite" } else { "From page" });
                    ui.add(egui::DragValue::new(start).range(1..=n.max(1) as u32));
                    ui.end_row();
                    ui.label(if de { "Format" } else { "Style" });
                    egui::ComboBox::from_id_salt("label-style")
                        .selected_text(label_style_name(*style, de))
                        .show_ui(ui, |ui| {
                            for s in [
                                LabelStyle::Decimal,
                                LabelStyle::RomanLower,
                                LabelStyle::RomanUpper,
                                LabelStyle::LettersLower,
                                LabelStyle::LettersUpper,
                                LabelStyle::None,
                            ] {
                                ui.selectable_value(style, s, label_style_name(s, de));
                            }
                        });
                    ui.end_row();
                    ui.label(if de { "Präfix" } else { "Prefix" });
                    ui.text_edit_singleline(prefix);
                    ui.end_row();
                    ui.label(if de { "Beginnen mit" } else { "Start with" });
                    ui.add(egui::DragValue::new(first).range(1..=99_999));
                    ui.end_row();
                });
                let (ok, cancel) = buttons(ui, t.ok, t.cancel);
                if ok {
                    let (start, style, prefix, first) = (*start as usize - 1, *style, prefix.clone(), *first);
                    action = Some(Box::new(move |app, ctx| {
                        app.edit(ctx, |tab| tab.doc.set_page_labels(start, style, &prefix, first))
                    }));
                    outcome = Outcome::Close;
                } else if cancel {
                    outcome = Outcome::Close;
                }
            }
            Dialog::Duplicates { groups } => {
                ui.heading(Cmd::FindDuplicatePages.label(lang));
                for g in groups.iter() {
                    let pages: Vec<String> = g.iter().map(|p| (p + 1).to_string()).collect();
                    ui.label(format!(
                        "{}: {}",
                        if de { "Gleiche Seiten" } else { "Identical pages" },
                        pages.join(", ")
                    ));
                }
                let (delete, cancel) =
                    buttons(ui, if de { "Duplikate löschen" } else { "Delete duplicates" }, t.cancel);
                if delete {
                    let doomed: Vec<usize> = groups.iter().flat_map(|g| g.iter().skip(1).copied()).collect();
                    action =
                        Some(Box::new(move |app, ctx| app.edit(ctx, |tab| tab.doc.delete_pages(&doomed))));
                    outcome = Outcome::Close;
                } else if cancel {
                    outcome = Outcome::Close;
                }
            }
            _ => outcome = Outcome::Close,
        }
        (outcome, action)
    }
}
