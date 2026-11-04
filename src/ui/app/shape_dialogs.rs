//! Dialogs of step D: "Seiten zuschneiden" with
//! draggable handles, "Größe ändern" and "Ränder einrichten", plus the
//! commands "Abgeschnittene Inhalte entfernen" and "Weiße Ränder
//! abschneiden".

use egui::{Color32, Context, Rect, RichText, Sense, Stroke, StrokeKind, Ui, vec2};
use serde::{Deserialize, Serialize};

use super::commands::{Cmd, Side};
use super::dialogs::{Action, Outcome};
use super::page_dialogs::{footer, paper_combo, tr};
use super::preview::{doc_preview, page_strip};
use super::{Dialog, MrPdfApp, PaperChoice};
use crate::pages::{BoxKind, CropOptions, Margins, ResizeSpec, SizeMode};
use crate::ui::i18n::Language;
use crate::ui::pickers::{
    PageRange, Target, TargetPage, length, preset_combo, section, segmented, unit_combo_right,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CropMethod {
    /// "Manuelle Einstellungen für Ränder".
    #[default]
    Manual,
    /// Remove the white margins of every page.
    White,
}

/// Input of "Seiten zuschneiden".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CropForm {
    pub method: CropMethod,
    pub kind: BoxKind,
    /// Display margins left, right, top, bottom (points).
    pub margins: Margins,
    pub keep_aspect: bool,
    pub remove_content: bool,
    pub remove_comments: bool,
    pub remove_fields: bool,
    pub fit_media: bool,
    pub normalize: bool,
    pub same_size: bool,
    pub consider_annots: bool,
}

impl Default for CropForm {
    fn default() -> Self {
        Self {
            method: CropMethod::Manual,
            kind: BoxKind::Crop,
            margins: [0.0; 4],
            keep_aspect: false,
            remove_content: false,
            remove_comments: false,
            remove_fields: false,
            fit_media: false,
            normalize: false,
            same_size: false,
            consider_annots: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ResizeKind {
    /// Size of a chosen page ("Nach Seite").
    ByPage,
    #[default]
    Standard,
    /// "Skalierung" in percent.
    Scale,
    /// "Wachstum": add width and height.
    Grow,
    Custom,
}

/// Input of "Seitengröße ändern".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ResizeForm {
    pub kind: ResizeKind,
    pub ref_page: TargetPage,
    pub ref_custom: u32,
    pub paper: PaperChoice,
    pub scale: f32,
    pub grow: (f32, f32),
    pub custom: (f32, f32),
    /// 0 = keep, 1 = portrait, 2 = landscape.
    pub orientation: u8,
    /// 0 = none, 1 = portrait pages, 2 = landscape pages.
    pub skip: u8,
    pub normalize: bool,
    pub fit: bool,
    pub keep_aspect: bool,
    /// Anchor cell 0..9 (row by row from the top left).
    pub anchor: u8,
    pub offset: (f32, f32),
}

impl Default for ResizeForm {
    fn default() -> Self {
        Self {
            kind: ResizeKind::Standard,
            ref_page: TargetPage::Current,
            ref_custom: 1,
            paper: PaperChoice::A4,
            scale: 100.0,
            grow: (0.0, 0.0),
            custom: (595.28, 841.89),
            orientation: 0,
            skip: 0,
            normalize: true,
            fit: false,
            keep_aspect: true,
            anchor: 4,
            offset: (0.0, 0.0),
        }
    }
}

/// Input of "Ränder einrichten".
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MarginsForm {
    /// Left, right, top, bottom (points); negative values shrink the page.
    pub margins: Margins,
    pub mirror: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ShapeDialog {
    Crop { form: CropForm, range: PageRange, page: usize, drag: Option<u8> },
    Resize { form: ResizeForm, range: PageRange, page: usize },
    Margins { form: MarginsForm, range: PageRange },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShapeState {
    pub dialog: ShapeDialog,
    pub preset_name: String,
}

/// 3 × 3 anchor grid ("Inhalte ausrichten").
fn anchor_grid(ui: &mut Ui, anchor: &mut u8) {
    let size = 14.0;
    let (rect, response) = ui.allocate_exact_size(vec2(size * 3.0 + 8.0, size * 3.0 + 8.0), Sense::click());
    for i in 0..9u8 {
        let (c, r) = (f32::from(i % 3), f32::from(i / 3));
        let cell = Rect::from_min_size(rect.min + vec2(c * (size + 4.0), r * (size + 4.0)), vec2(size, size));
        let fill = if *anchor == i { Color32::from_gray(95) } else { ui.visuals().extreme_bg_color };
        ui.painter().rect(cell, 2.0, fill, Stroke::new(1.0, Color32::from_gray(140)), StrokeKind::Inside);
        if response.clicked()
            && let Some(pos) = response.interact_pointer_pos()
            && cell.expand(2.0).contains(pos)
        {
            *anchor = i;
        }
    }
}

impl MrPdfApp {
    fn sd_de(&self) -> bool {
        self.settings.language == Language::German
    }

    /// Handles the commands of step D. Returns false for other commands.
    pub(super) fn run_shape(&mut self, ctx: &Context, cmd: Cmd) -> bool {
        let Some(tab) = self.active_tab() else { return false };
        let (n, cur) = (tab.doc.page_count(), tab.current_page);
        let de = self.sd_de();
        let dialog = match cmd {
            Cmd::CropPages => ShapeDialog::Crop {
                form: CropForm::default(),
                range: PageRange::current(),
                page: cur,
                drag: None,
            },
            Cmd::ResizePages => {
                ShapeDialog::Resize { form: ResizeForm::default(), range: PageRange::all(), page: cur }
            }
            Cmd::Margins => ShapeDialog::Margins { form: MarginsForm::default(), range: PageRange::all() },
            Cmd::RemoveCropped => {
                let pages: Vec<usize> = (0..n).collect();
                let result = self.tab().map(|t| t.doc.remove_outside(&pages, true, true, true));
                if let Some(tab) = self.tab() {
                    tab.after_edit();
                }
                match result {
                    Some(Ok(k)) => self.set_status(
                        ctx,
                        if de { format!("Entfernte Inhalte: {k}") } else { format!("Removed objects: {k}") },
                    ),
                    Some(Err(err)) => self.show_error(&err),
                    None => {}
                }
                return true;
            }
            Cmd::TrimWhite(side) => {
                self.trim_white(ctx, (0..n).collect(), side);
                return true;
            }
            _ => return false,
        };
        self.clear_previews();
        self.dialog_doc = None;
        self.dialog = Some(Dialog::Shape(Box::new(ShapeState { dialog, preset_name: String::new() })));
        true
    }

    fn trim_white(&mut self, ctx: &Context, pages: Vec<usize>, side: Side) {
        let de = self.sd_de();
        let Some(tab) = self.active_tab() else { return };
        let mut items = Vec::new();
        for p in pages {
            match tab.doc.white_margins(p, false) {
                Ok(m) => {
                    let keep = |s: Side| side == Side::All || side == s;
                    let m = [
                        if keep(Side::Left) { m[0] } else { 0.0 },
                        if keep(Side::Right) { m[1] } else { 0.0 },
                        if keep(Side::Top) { m[2] } else { 0.0 },
                        if keep(Side::Bottom) { m[3] } else { 0.0 },
                    ];
                    if m.iter().any(|v| *v > 0.5) {
                        items.push((p, m));
                    }
                }
                Err(err) => {
                    self.show_error(&err);
                    return;
                }
            }
        }
        let count = items.len();
        if count > 0 {
            self.edit(ctx, |tab| tab.doc.crop_pages_with(&items, CropOptions::default()));
        }
        self.set_status(
            ctx,
            if de { format!("Zugeschnittene Seiten: {count}") } else { format!("Cropped pages: {count}") },
        );
    }

    pub(super) fn shape_dialog(&mut self, ui: &mut Ui, state: &mut ShapeState) -> (Outcome, Option<Action>) {
        let de = self.sd_de();
        let lang = self.settings.language;
        let unit = self.settings.unit;
        let (n, cur) = self.active_tab().map_or((0, 0), |t| (t.doc.page_count(), t.current_page));
        let mut outcome = Outcome::Keep;
        let mut action: Option<Action> = None;
        match &mut state.dialog {
            ShapeDialog::Crop { form, range, page, drag } => {
                super::dialogs::set_title(ui, Cmd::CropPages.label(lang));
                *page = (*page).min(n.saturating_sub(1));
                let page_size = self
                    .active_tab()
                    .and_then(|t| t.doc.geometry(*page))
                    .map_or((595.0, 842.0), |g| g.view_size());
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(440.0);
                        section(ui, tr(de, "Ränder zuschneiden", "Crop margins"), |ui| {
                            ui.horizontal(|ui| unit_combo_right(ui, de, &mut self.settings.unit));
                            egui::Grid::new("crop-head").num_columns(2).spacing(vec2(8.0, 6.0)).show(
                                ui,
                                |ui| {
                                    ui.label(tr(de, "Methode:", "Method:"));
                                    egui::ComboBox::from_id_salt("crop-method")
                                        .selected_text(match form.method {
                                            CropMethod::Manual => {
                                                tr(de, "Manuelle Einstellungen für Ränder", "Manual margins")
                                            }
                                            CropMethod::White => tr(
                                                de,
                                                "Weiße Ränder entfernen (jede Seite)",
                                                "Remove white margins (every page)",
                                            ),
                                        })
                                        .width(280.0)
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(
                                                &mut form.method,
                                                CropMethod::Manual,
                                                tr(de, "Manuelle Einstellungen für Ränder", "Manual margins"),
                                            );
                                            ui.selectable_value(
                                                &mut form.method,
                                                CropMethod::White,
                                                tr(
                                                    de,
                                                    "Weiße Ränder entfernen (jede Seite)",
                                                    "Remove white margins (every page)",
                                                ),
                                            );
                                        });
                                    ui.end_row();
                                    ui.label(tr(de, "Box:", "Box:"));
                                    egui::ComboBox::from_id_salt("crop-box")
                                        .selected_text(form.kind.label())
                                        .show_ui(ui, |ui| {
                                            for k in BoxKind::ALL {
                                                ui.selectable_value(&mut form.kind, k, k.label());
                                            }
                                        });
                                    ui.end_row();
                                },
                            );
                            ui.add_enabled_ui(form.method == CropMethod::Manual, |ui| {
                                let before = form.margins;
                                egui::Grid::new("crop-margins").num_columns(4).spacing(vec2(10.0, 6.0)).show(
                                    ui,
                                    |ui| {
                                        ui.label(tr(de, "Links:", "Left:"));
                                        length(ui, &mut form.margins[0], unit, -2000.0..=page_size.0);
                                        ui.label(tr(de, "Rechts:", "Right:"));
                                        length(ui, &mut form.margins[1], unit, -2000.0..=page_size.0);
                                        ui.end_row();
                                        ui.label(tr(de, "Oben:", "Top:"));
                                        length(ui, &mut form.margins[2], unit, -2000.0..=page_size.1);
                                        ui.label(tr(de, "Unten:", "Bottom:"));
                                        length(ui, &mut form.margins[3], unit, -2000.0..=page_size.1);
                                        ui.end_row();
                                    },
                                );
                                ui.checkbox(
                                    &mut form.keep_aspect,
                                    tr(de, "Seitenverhältnis beibehalten", "Keep aspect ratio"),
                                );
                                if form.keep_aspect && form.margins != before {
                                    keep_aspect(&mut form.margins, before, page_size);
                                }
                                ui.horizontal(|ui| {
                                    if ui
                                        .button(tr(de, "Auf Null setzen", "Set to zero"))
                                        .on_hover_text(tr(
                                            de,
                                            "Die ganze Seite (Media-Box) zeigen",
                                            "Show the whole page (media box)",
                                        ))
                                        .clicked()
                                    {
                                        form.margins = self
                                            .active_tab()
                                            .and_then(|t| t.doc.media_margins(*page).ok())
                                            .unwrap_or([0.0; 4]);
                                    }
                                    if ui.button(tr(de, "Auf Original setzen", "Set to original")).clicked() {
                                        form.margins = [0.0; 4];
                                    }
                                    if ui
                                        .button(tr(de, "Auf weiße Ränder setzen", "Set to white margins"))
                                        .clicked()
                                        && let Some(m) = self.active_tab().and_then(|t| {
                                            t.doc.white_margins(*page, form.consider_annots).ok()
                                        })
                                    {
                                        form.margins = m;
                                    }
                                });
                            });
                            ui.separator();
                            ui.checkbox(
                                &mut form.remove_content,
                                tr(
                                    de,
                                    "Inhalte außerhalb der Crop-Box entfernen",
                                    "Remove content outside the crop box",
                                ),
                            );
                            ui.checkbox(
                                &mut form.remove_comments,
                                tr(
                                    de,
                                    "Anmerkungen außerhalb der Crop-Box entfernen",
                                    "Remove comments outside the crop box",
                                ),
                            );
                            ui.checkbox(
                                &mut form.remove_fields,
                                tr(
                                    de,
                                    "Formularfelder außerhalb der Crop-Box entfernen",
                                    "Remove form fields outside the crop box",
                                ),
                            );
                            ui.add_enabled_ui(form.kind == BoxKind::Crop, |ui| {
                                ui.checkbox(
                                    &mut form.fit_media,
                                    tr(
                                        de,
                                        "Seitengröße an die Crop-Box anpassen",
                                        "Adapt page size to the crop box",
                                    ),
                                );
                                ui.add_enabled_ui(form.fit_media, |ui| {
                                    ui.indent("norm", |ui| {
                                        ui.checkbox(
                                            &mut form.normalize,
                                            tr(
                                                de,
                                                "Media-Box normalisieren auf Startkoordinate (0;0)",
                                                "Normalize media box to origin (0;0)",
                                            ),
                                        )
                                    });
                                });
                            });
                            ui.separator();
                            ui.add_enabled_ui(range.resolve(n, cur).is_some_and(|p| p.len() > 1), |ui| {
                                ui.checkbox(
                                    &mut form.same_size,
                                    tr(
                                        de,
                                        "Alle geänderten Seiten sollen dieselbe Größe haben",
                                        "All changed pages get the same size",
                                    ),
                                );
                            });
                            ui.checkbox(
                                &mut form.consider_annots,
                                tr(
                                    de,
                                    "Anmerkungen und Formularfelder berücksichtigen",
                                    "Consider comments and form fields",
                                ),
                            );
                        });
                        range.ui(ui, de, n, cur, &PageRange::ALL_MODES);
                    });
                    ui.vertical(|ui| {
                        ui.set_width(360.0);
                        let margins = if form.method == CropMethod::White {
                            self.active_tab()
                                .and_then(|t| t.doc.white_margins(*page, form.consider_annots).ok())
                                .unwrap_or([0.0; 4])
                        } else {
                            form.margins
                        };
                        if let Some(area) = self.page_preview(ui, 0, *page, vec2(360.0, 460.0), true) {
                            let r = area.rect;
                            let s = area.scale;
                            let crop = Rect::from_min_max(
                                r.min + vec2(margins[0] * s, margins[2] * s),
                                r.max - vec2(margins[1] * s, margins[3] * s),
                            );
                            let shade = Color32::from_black_alpha(90);
                            let painter = &area.painter;
                            for part in [
                                Rect::from_min_max(r.min, egui::pos2(r.max.x, crop.min.y)),
                                Rect::from_min_max(egui::pos2(r.min.x, crop.max.y), r.max),
                                Rect::from_min_max(
                                    egui::pos2(r.min.x, crop.min.y),
                                    egui::pos2(crop.min.x, crop.max.y),
                                ),
                                Rect::from_min_max(
                                    egui::pos2(crop.max.x, crop.min.y),
                                    egui::pos2(r.max.x, crop.max.y),
                                ),
                            ] {
                                if part.is_positive() {
                                    painter.rect_filled(part.intersect(r), 0.0, shade);
                                }
                            }
                            painter.rect_stroke(
                                crop,
                                0.0,
                                Stroke::new(1.0, Color32::from_rgb(30, 30, 30)),
                                StrokeKind::Middle,
                            );
                            // Eight handles: corners and edge centres.
                            let handles = [
                                crop.left_top(),
                                crop.center_top(),
                                crop.right_top(),
                                crop.right_center(),
                                crop.right_bottom(),
                                crop.center_bottom(),
                                crop.left_bottom(),
                                crop.left_center(),
                            ];
                            for h in handles {
                                let hr = Rect::from_center_size(h, vec2(9.0, 9.0));
                                painter.rect(
                                    hr,
                                    1.0,
                                    Color32::from_rgb(250, 220, 90),
                                    Stroke::new(1.0, Color32::from_rgb(120, 100, 30)),
                                    StrokeKind::Inside,
                                );
                            }
                            if form.method == CropMethod::Manual {
                                let resp = &area.response;
                                if resp.drag_started()
                                    && let Some(pos) = resp.interact_pointer_pos()
                                {
                                    *drag =
                                        handles.iter().position(|h| h.distance(pos) < 9.0).map(|i| i as u8);
                                }
                                if let Some(i) = *drag
                                    && resp.dragged()
                                {
                                    let d = resp.drag_delta() / s;
                                    let before = form.margins;
                                    let m = &mut form.margins;
                                    // Which margins each handle moves.
                                    let (l, rr, t, b) = match i {
                                        0 => (true, false, true, false),
                                        1 => (false, false, true, false),
                                        2 => (false, true, true, false),
                                        3 => (false, true, false, false),
                                        4 => (false, true, false, true),
                                        5 => (false, false, false, true),
                                        6 => (true, false, false, true),
                                        _ => (true, false, false, false),
                                    };
                                    if l {
                                        m[0] += d.x;
                                    }
                                    if rr {
                                        m[1] -= d.x;
                                    }
                                    if t {
                                        m[2] += d.y;
                                    }
                                    if b {
                                        m[3] -= d.y;
                                    }
                                    for v in m.iter_mut() {
                                        *v = v.max(0.0);
                                    }
                                    if form.keep_aspect {
                                        keep_aspect(&mut form.margins, before, page_size);
                                    }
                                }
                                if resp.drag_stopped() {
                                    *drag = None;
                                }
                            }
                            let (w, h) = (
                                page_size.0 - margins[0] - margins[1],
                                page_size.1 - margins[2] - margins[3],
                            );
                            ui.label(format!(
                                "{} {}: {}: {} x {}",
                                tr(de, "Seite", "Page"),
                                *page + 1,
                                tr(de, "Beschnittene Seitengröße", "Cropped page size"),
                                unit.format(w.max(0.0)),
                                unit.format(h.max(0.0))
                            ));
                        }
                        page_strip(ui, page, n);
                    });
                });
                let mut name = std::mem::take(&mut state.preset_name);
                preset_combo(ui, de, "crop", form, &mut self.settings, &mut name);
                state.preset_name = name;
                let sel = range.resolve(n, cur);
                let (ok, cancel, _) = footer(ui, de, sel.is_some(), false, "OK");
                if ok && let Some(pages) = sel {
                    let form = form.clone();
                    action = Some(Box::new(move |app, ctx| app.apply_crop(ctx, &pages, &form)));
                    outcome = Outcome::Close;
                } else if cancel {
                    outcome = Outcome::Close;
                }
            }
            ShapeDialog::Resize { form, range, page } => {
                super::dialogs::set_title(ui, tr(de, "Seitengröße ändern", "Change page size"));
                *page = (*page).min(n.saturating_sub(1));
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(440.0);
                        range.ui(ui, de, n, cur, &PageRange::ALL_MODES);
                        section(ui, tr(de, "Papiergröße", "Paper size"), |ui| {
                            ui.horizontal(|ui| unit_combo_right(ui, de, &mut self.settings.unit));
                            egui::Grid::new("resize-size").num_columns(2).spacing(vec2(8.0, 6.0)).show(
                                ui,
                                |ui| {
                                    ui.label(tr(de, "Typ:", "Type:"));
                                    segmented(
                                        ui,
                                        &mut form.kind,
                                        &[
                                            (ResizeKind::ByPage, tr(de, "Nach Seite", "By page").into()),
                                            (ResizeKind::Standard, "Standard".into()),
                                            (ResizeKind::Scale, tr(de, "Skalierung", "Scaling").into()),
                                            (ResizeKind::Grow, tr(de, "Wachstum", "Growth").into()),
                                            (ResizeKind::Custom, tr(de, "Eigene", "Custom").into()),
                                        ],
                                    );
                                    ui.end_row();
                                    match form.kind {
                                        ResizeKind::ByPage => {
                                            ui.label(tr(de, "Seite:", "Page:"));
                                            ui.horizontal(|ui| {
                                                segmented(
                                                    ui,
                                                    &mut form.ref_page,
                                                    &[
                                                        (TargetPage::First, tr(de, "Erste", "First").into()),
                                                        (
                                                            TargetPage::Current,
                                                            format!(
                                                                "{}: {}",
                                                                tr(de, "Aktuell", "Current"),
                                                                cur + 1
                                                            ),
                                                        ),
                                                        (
                                                            TargetPage::Last,
                                                            format!("{}: {}", tr(de, "Letzte", "Last"), n),
                                                        ),
                                                        (
                                                            TargetPage::Custom,
                                                            tr(de, "Eigene", "Custom").into(),
                                                        ),
                                                    ],
                                                );
                                                if form.ref_page == TargetPage::Custom {
                                                    ui.add(
                                                        egui::DragValue::new(&mut form.ref_custom)
                                                            .range(1..=n.max(1) as u32),
                                                    );
                                                }
                                            });
                                        }
                                        ResizeKind::Standard => {
                                            ui.label(tr(de, "Größe:", "Size:"));
                                            paper_combo(ui, de, "resize-paper", &mut form.paper);
                                        }
                                        ResizeKind::Scale => {
                                            ui.label(tr(de, "Skalierung:", "Scaling:"));
                                            ui.add(
                                                egui::DragValue::new(&mut form.scale)
                                                    .range(1.0..=1000.0)
                                                    .suffix(" %"),
                                            );
                                        }
                                        ResizeKind::Grow => {
                                            ui.label(tr(de, "Wachstum:", "Growth:"));
                                            ui.horizontal(|ui| {
                                                length(ui, &mut form.grow.0, unit, -5000.0..=5000.0);
                                                ui.label("x");
                                                length(ui, &mut form.grow.1, unit, -5000.0..=5000.0);
                                            });
                                        }
                                        ResizeKind::Custom => {
                                            ui.label(tr(de, "Größe:", "Size:"));
                                            ui.horizontal(|ui| {
                                                length(ui, &mut form.custom.0, unit, 3.0..=14_400.0);
                                                ui.label("x");
                                                length(ui, &mut form.custom.1, unit, 3.0..=14_400.0);
                                            });
                                        }
                                    }
                                    ui.end_row();
                                    ui.label(tr(de, "Ausrichtung:", "Orientation:"));
                                    segmented(
                                        ui,
                                        &mut form.orientation,
                                        &[
                                            (0, tr(de, "Beibehalten", "Keep").into()),
                                            (1, tr(de, "Hochformat", "Portrait").into()),
                                            (2, tr(de, "Querformat", "Landscape").into()),
                                        ],
                                    );
                                    ui.end_row();
                                    ui.label(tr(de, "Überspringen:", "Skip:"));
                                    let names = [
                                        tr(de, "nicht festgelegt", "not set"),
                                        tr(de, "Seiten im Hochformat", "Portrait pages"),
                                        tr(de, "Seiten im Querformat", "Landscape pages"),
                                    ];
                                    egui::ComboBox::from_id_salt("resize-skip")
                                        .selected_text(names[usize::from(form.skip.min(2))])
                                        .show_ui(ui, |ui| {
                                            for (i, name) in names.iter().enumerate() {
                                                ui.selectable_value(&mut form.skip, i as u8, *name);
                                            }
                                        });
                                    ui.end_row();
                                },
                            );
                            ui.checkbox(
                                &mut form.normalize,
                                tr(
                                    de,
                                    "Media-Box normalisieren auf Startkoordinate (0;0)",
                                    "Normalize media box to origin (0;0)",
                                ),
                            );
                        });
                        section(ui, tr(de, "Inhalte", "Contents"), |ui| {
                            ui.checkbox(
                                &mut form.fit,
                                tr(
                                    de,
                                    "Inhalte an neue Seitengröße anpassen",
                                    "Fit contents to the new page size",
                                ),
                            );
                            ui.add_enabled_ui(form.fit, |ui| {
                                ui.indent("fit", |ui| {
                                    ui.checkbox(
                                        &mut form.keep_aspect,
                                        tr(
                                            de,
                                            "Seitenverhältnis der Inhalte beibehalten",
                                            "Keep aspect ratio of the contents",
                                        ),
                                    );
                                    ui.add_enabled(
                                        false,
                                        egui::Checkbox::new(
                                            &mut true,
                                            tr(
                                                de,
                                                "Anmerkungen und Formularfelder anpassen",
                                                "Adapt comments and form fields",
                                            ),
                                        ),
                                    );
                                });
                            });
                            ui.horizontal(|ui| {
                                anchor_grid(ui, &mut form.anchor);
                                egui::Grid::new("resize-offset").num_columns(2).spacing(vec2(8.0, 6.0)).show(
                                    ui,
                                    |ui| {
                                        ui.label(tr(de, "Horiz. verschieben:", "Move horizontally:"));
                                        length(ui, &mut form.offset.0, unit, -5000.0..=5000.0);
                                        ui.end_row();
                                        ui.label(tr(de, "Vert. verschieben:", "Move vertically:"));
                                        length(ui, &mut form.offset.1, unit, -5000.0..=5000.0);
                                        ui.end_row();
                                    },
                                );
                                if ui.button("↺").on_hover_text(tr(de, "Zurücksetzen", "Reset")).clicked()
                                {
                                    form.anchor = 4;
                                    form.offset = (0.0, 0.0);
                                }
                            });
                        });
                    });
                    ui.vertical(|ui| {
                        ui.set_width(360.0);
                        let spec = self.resize_spec(form, n, cur, *page);
                        let key = format!("resize {page} {spec:?}");
                        if self.dialog_doc.as_ref().is_none_or(|(k, _)| *k != key)
                            && let Some(Ok(doc)) =
                                self.active_tab().map(|t| t.doc.resize_preview(*page, &spec))
                        {
                            self.dialog_doc = Some((key, doc));
                        }
                        if let Some((_, d)) = &self.dialog_doc {
                            let (w, h) = d.geometry(0).map_or((0.0, 0.0), |g| g.view_size());
                            ui.label(
                                RichText::new(format!(
                                    "{} {}: {} x {}",
                                    tr(de, "Seite", "Page"),
                                    *page + 1,
                                    unit.format(w),
                                    unit.format(h)
                                ))
                                .weak(),
                            );
                            doc_preview(
                                ui,
                                &mut self.preview[0],
                                d,
                                0x5e2,
                                0,
                                vec2(360.0, 440.0),
                                super::preview::PreviewOpts::new(de),
                            );
                        }
                        page_strip(ui, page, n);
                    });
                });
                let mut name = std::mem::take(&mut state.preset_name);
                preset_combo(ui, de, "resize", form, &mut self.settings, &mut name);
                state.preset_name = name;
                let sel = range.resolve(n, cur);
                let (ok, cancel, _) = footer(ui, de, sel.is_some(), false, "OK");
                if ok && let Some(pages) = sel {
                    let spec = self.resize_spec(form, n, cur, cur);
                    action = Some(Box::new(move |app, ctx| {
                        app.edit(ctx, |tab| tab.doc.resize_pages_with(&pages, &spec).map(|_| ()))
                    }));
                    outcome = Outcome::Close;
                } else if cancel {
                    outcome = Outcome::Close;
                }
            }
            ShapeDialog::Margins { form, range } => {
                super::dialogs::set_title(ui, tr(de, "Ränder einrichten", "Page margins"));
                range.ui(ui, de, n, cur, &PageRange::ALL_MODES);
                section(ui, tr(de, "Ränder", "Margins"), |ui| {
                    ui.horizontal(|ui| unit_combo_right(ui, de, &mut self.settings.unit));
                    egui::Grid::new("margins-page").num_columns(4).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
                        ui.label(tr(de, "Oben:", "Top:"));
                        length(ui, &mut form.margins[2], unit, -2000.0..=2000.0);
                        ui.label(tr(de, "Unten:", "Bottom:"));
                        length(ui, &mut form.margins[3], unit, -2000.0..=2000.0);
                        ui.end_row();
                        ui.label(tr(de, "Links:", "Left:"));
                        length(ui, &mut form.margins[0], unit, -2000.0..=2000.0);
                        ui.label(tr(de, "Rechts:", "Right:"));
                        length(ui, &mut form.margins[1], unit, -2000.0..=2000.0);
                        ui.end_row();
                    });
                    ui.checkbox(
                        &mut form.mirror,
                        tr(
                            de,
                            "Horizontale Ränder bei geraden Seiten spiegeln",
                            "Mirror horizontal margins on even pages",
                        ),
                    );
                    ui.label(RichText::new(tr(de, "Positive Werte vergrößern die Seite, negative verkleinern sie. Der Seiteninhalt bleibt unverändert.", "Positive values enlarge the page, negative values make it smaller. The page content stays unchanged.")).weak().small());
                });
                let mut name = std::mem::take(&mut state.preset_name);
                preset_combo(ui, de, "margins", form, &mut self.settings, &mut name);
                state.preset_name = name;
                let sel = range.resolve(n, cur);
                let (ok, cancel, _) = footer(ui, de, sel.is_some(), false, "OK");
                if ok && let Some(pages) = sel {
                    let form = form.clone();
                    let items: Vec<(usize, Margins)> = pages
                        .iter()
                        .map(|&p| {
                            let m = form.margins;
                            // Even page numbers (2, 4, …) get left and right swapped.
                            if form.mirror && p % 2 == 1 { (p, [m[1], m[0], m[2], m[3]]) } else { (p, m) }
                        })
                        .collect();
                    action =
                        Some(Box::new(move |app, ctx| app.edit(ctx, |tab| tab.doc.add_page_margins(&items))));
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

    fn resize_spec(&self, form: &ResizeForm, n: usize, cur: usize, page: usize) -> ResizeSpec {
        let page_size = |p: usize| {
            self.active_tab().and_then(|t| t.doc.geometry(p)).map_or((595.28, 841.89), |g| g.view_size())
        };
        let mode = match form.kind {
            ResizeKind::ByPage => {
                let t = Target { after: false, page: form.ref_page, custom: form.ref_custom };
                let (w, h) = page_size(t.page_index(n, cur));
                SizeMode::Fixed(w, h)
            }
            ResizeKind::Standard => {
                let (w, h) = form.paper.size().unwrap_or((595.28, 841.89));
                // A standard size follows the orientation of each page unless set.
                let (pw, ph) = page_size(page);
                if form.orientation == 0 && pw > ph { SizeMode::Fixed(h, w) } else { SizeMode::Fixed(w, h) }
            }
            ResizeKind::Scale => SizeMode::Scale(form.scale / 100.0),
            ResizeKind::Grow => SizeMode::Grow(form.grow.0, form.grow.1),
            ResizeKind::Custom => SizeMode::Fixed(form.custom.0, form.custom.1),
        };
        let (c, r) = (f32::from(form.anchor % 3), f32::from(form.anchor / 3));
        ResizeSpec {
            mode,
            orientation: form.orientation,
            skip: form.skip,
            fit: form.fit,
            keep_aspect: form.keep_aspect,
            // Grid row 0 is the top: anchor y counts from the bottom.
            anchor: (c / 2.0, 1.0 - r / 2.0),
            offset: (form.offset.0, -form.offset.1),
            normalize: form.normalize,
        }
    }

    fn apply_crop(&mut self, ctx: &Context, pages: &[usize], form: &CropForm) {
        let Some(tab) = self.active_tab() else { return };
        let items: crate::Result<Vec<(usize, Margins)>> = pages
            .iter()
            .map(|&p| match form.method {
                CropMethod::Manual => Ok((p, form.margins)),
                CropMethod::White => tab.doc.white_margins(p, form.consider_annots).map(|m| (p, m)),
            })
            .collect();
        let items = match items {
            Ok(i) => i,
            Err(err) => {
                self.show_error(&err);
                return;
            }
        };
        let opts = CropOptions { kind: form.kind, fit_media: form.fit_media, same_size: form.same_size };
        let form = form.clone();
        let pages = pages.to_vec();
        self.edit(ctx, |tab| {
            tab.doc.crop_pages_with(&items, opts)?;
            if form.remove_content || form.remove_comments || form.remove_fields {
                tab.doc.remove_outside(
                    &pages,
                    form.remove_content,
                    form.remove_comments,
                    form.remove_fields,
                )?;
            }
            if form.fit_media && form.normalize {
                tab.doc.normalize_pages(&pages)?;
            }
            Ok(())
        });
    }
}

/// Adjusts margins so the cropped area keeps the page's aspect ratio; the
/// dimension that was not edited follows.
fn keep_aspect(m: &mut Margins, before: Margins, page: (f32, f32)) {
    let ratio = page.0 / page.1.max(1.0);
    let changed_width = m[0] != before[0] || m[1] != before[1];
    let w = page.0 - m[0] - m[1];
    let h = page.1 - m[2] - m[3];
    if changed_width {
        let target_h = w / ratio;
        let extra = (h - target_h) / 2.0;
        m[2] = (m[2] + extra).max(0.0);
        m[3] = (m[3] + extra).max(0.0);
    } else {
        let target_w = h * ratio;
        let extra = (w - target_w) / 2.0;
        m[0] = (m[0] + extra).max(0.0);
        m[1] = (m[1] + extra).max(0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::keep_aspect;

    #[test]
    fn aspect_follows_the_edited_side() {
        let mut m = [50.0, 50.0, 0.0, 0.0];
        keep_aspect(&mut m, [0.0; 4], (600.0, 800.0));
        // Width 500 -> height 666.7: 66.7 split over top and bottom.
        assert!((m[2] - 66.67).abs() < 0.1 && (m[3] - 66.67).abs() < 0.1, "{m:?}");
    }
}
