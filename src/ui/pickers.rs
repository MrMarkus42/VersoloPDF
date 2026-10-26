//! Reusable dialog building blocks
//! for the page dialogs: segmented buttons, page range
//! ("Seitenbereich"), insert target ("Ziel"), length units and presets.

use egui::{Color32, Frame, Margin, RichText, Stroke, Ui, vec2};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::ui::settings::Settings;

/// Upper bound for stored presets per dialog.
const MAX_PRESETS: usize = 50;

/// A row of joined toggle buttons; returns true if the value changed.
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, String)]) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for (v, label) in options {
            let selected = *value == *v;
            let text =
                if selected { RichText::new(label).color(Color32::WHITE) } else { RichText::new(label) };
            let mut button = egui::Button::new(text).corner_radius(0.0).selected(selected);
            if selected {
                button = button.fill(Color32::from_gray(95));
            }
            if ui.add(button).clicked() && !selected {
                *value = *v;
                changed = true;
            }
        }
    });
    changed
}

/// A titled group box (grey caption bar, light border).
pub fn section(ui: &mut Ui, title: impl Into<RichText>, add: impl FnOnce(&mut Ui)) {
    let stroke = ui.visuals().widgets.noninteractive.bg_stroke;
    Frame::new().stroke(Stroke::new(1.0, stroke.color)).corner_radius(2.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        Frame::new().fill(ui.visuals().faint_bg_color).inner_margin(Margin::symmetric(6, 3)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(title.into().weak());
        });
        Frame::new().inner_margin(Margin::symmetric(8, 6)).show(ui, add);
    });
    ui.add_space(4.0);
}

// ---------------------------------------------------------------------------
// Page range

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RangeMode {
    #[default]
    All,
    Current,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Subset {
    #[default]
    All,
    Odd,
    Even,
}

/// "Seitenbereich": all / current / custom range, filtered to odd or even.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PageRange {
    pub mode: RangeMode,
    pub text: String,
    pub subset: Subset,
}

/// Parses "1-3, 7, 10-" (1-based, open ends allowed) into page indices.
pub fn parse_pages(text: &str, page_count: usize) -> Option<Vec<usize>> {
    let mut pages = Vec::new();
    for part in text.split([',', ';']) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (a, b) = match part.split_once('-') {
            Some((a, b)) => {
                let a = if a.trim().is_empty() { 1 } else { a.trim().parse::<usize>().ok()? };
                let b = if b.trim().is_empty() { page_count } else { b.trim().parse::<usize>().ok()? };
                (a, b)
            }
            None => {
                let n = part.parse::<usize>().ok()?;
                (n, n)
            }
        };
        if a == 0 || b == 0 || a > b || b > page_count {
            return None;
        }
        pages.extend((a - 1)..b);
    }
    pages.sort_unstable();
    pages.dedup();
    (!pages.is_empty()).then_some(pages)
}

impl PageRange {
    pub fn all() -> Self {
        Self::default()
    }

    pub fn current() -> Self {
        Self { mode: RangeMode::Current, ..Self::default() }
    }

    /// The selected page indices (sorted), or `None` if the custom range is
    /// invalid or nothing is left after filtering.
    pub fn resolve(&self, page_count: usize, current: usize) -> Option<Vec<usize>> {
        if page_count == 0 {
            return None;
        }
        let pages = match self.mode {
            RangeMode::All => (0..page_count).collect(),
            RangeMode::Current => vec![current.min(page_count - 1)],
            RangeMode::Custom => parse_pages(&self.text, page_count)?,
        };
        let pages: Vec<usize> = pages
            .into_iter()
            .filter(|p| match self.subset {
                Subset::All => true,
                // Odd/even refer to page numbers (1-based).
                Subset::Odd => p % 2 == 0,
                Subset::Even => p % 2 == 1,
            })
            .collect();
        (!pages.is_empty()).then_some(pages)
    }

    /// Draws the "Seitenbereich" box offering `modes`.
    /// All modes offered: "Alle", "Aktuell", "Bereich".
    pub const ALL_MODES: [RangeMode; 3] = [RangeMode::All, RangeMode::Current, RangeMode::Custom];

    pub fn ui(&mut self, ui: &mut Ui, de: bool, page_count: usize, current: usize, modes: &[RangeMode]) {
        self.ui_titled(ui, de, page_count, current, modes, if de { "Seitenbereich" } else { "Page range" });
    }

    pub fn ui_titled(
        &mut self,
        ui: &mut Ui,
        de: bool,
        page_count: usize,
        current: usize,
        modes: &[RangeMode],
        title: &str,
    ) {
        if !modes.contains(&self.mode) {
            self.mode = modes.first().copied().unwrap_or_default();
        }
        let selected = self.resolve(page_count, current).map_or(0, |p| p.len());
        let header = if de {
            format!("{title}: {selected} von {page_count} Seiten ausgewählt")
        } else {
            format!("{title}: {selected} of {page_count} pages selected")
        };
        section(ui, header, |ui| {
            egui::Grid::new(ui.id().with("range")).num_columns(2).spacing(vec2(8.0, 6.0)).show(ui, |ui| {
                ui.label(if de { "Seiten" } else { "Pages" });
                ui.horizontal(|ui| {
                    let options: Vec<(RangeMode, String)> = modes
                        .iter()
                        .map(|m| match m {
                            RangeMode::All => (*m, if de { "Alle" } else { "All" }.to_string()),
                            RangeMode::Current => {
                                (*m, format!("{}: {}", if de { "Aktuell" } else { "Current" }, current + 1))
                            }
                            RangeMode::Custom => (*m, if de { "Bereich" } else { "Range" }.to_string()),
                        })
                        .collect();
                    if segmented(ui, &mut self.mode, &options)
                        && self.mode == RangeMode::Custom
                        && self.text.is_empty()
                    {
                        self.text = (current + 1).to_string();
                    }
                    if self.mode == RangeMode::Custom {
                        let valid = parse_pages(&self.text, page_count).is_some();
                        let edit = egui::TextEdit::singleline(&mut self.text)
                            .desired_width(120.0)
                            .hint_text("1-3, 7, 10-")
                            .text_color_opt((!valid).then_some(Color32::from_rgb(200, 40, 40)));
                        ui.add(edit).on_hover_text(if de {
                            "Seitenzahlen und Bereiche, durch Komma getrennt, z. B. 1-3, 7, 10-"
                        } else {
                            "Page numbers and ranges separated by commas, e.g. 1-3, 7, 10-"
                        });
                        ui.label(format!("/{page_count}"));
                    }
                });
                ui.end_row();
                ui.label(if de { "Teilmenge" } else { "Subset" });
                segmented(
                    ui,
                    &mut self.subset,
                    &[
                        (Subset::All, if de { "Alle" } else { "All" }.into()),
                        (Subset::Odd, if de { "Ungerade" } else { "Odd" }.into()),
                        (Subset::Even, if de { "Gerade" } else { "Even" }.into()),
                    ],
                );
                ui.end_row();
            });
        });
    }
}

// ---------------------------------------------------------------------------
// Target

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TargetPage {
    First,
    Current,
    #[default]
    Last,
    Custom,
}

/// "Ziel": insert before or after the first / current / last / a given page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    pub after: bool,
    pub page: TargetPage,
    /// 1-based page number for `TargetPage::Custom`.
    pub custom: u32,
}

impl Default for Target {
    fn default() -> Self {
        Self { after: true, page: TargetPage::Last, custom: 1 }
    }
}

impl Target {
    pub fn after_current() -> Self {
        Self { after: true, page: TargetPage::Current, custom: 1 }
    }

    /// The reference page (0-based).
    pub fn page_index(&self, page_count: usize, current: usize) -> usize {
        let last = page_count.saturating_sub(1);
        match self.page {
            TargetPage::First => 0,
            TargetPage::Current => current.min(last),
            TargetPage::Last => last,
            TargetPage::Custom => (self.custom.max(1) as usize - 1).min(last),
        }
    }

    /// Index at which new pages are inserted (0 ..= page_count).
    pub fn insert_index(&self, page_count: usize, current: usize) -> usize {
        if page_count == 0 {
            return 0;
        }
        self.page_index(page_count, current) + usize::from(self.after)
    }

    pub fn ui(&mut self, ui: &mut Ui, de: bool, page_count: usize, current: usize) {
        section(ui, if de { "Ziel" } else { "Target" }, |ui| {
            egui::Grid::new(ui.id().with("target")).num_columns(2).spacing(vec2(8.0, 6.0)).show(ui, |ui| {
                ui.label(if de { "Bereich" } else { "Position" });
                segmented(
                    ui,
                    &mut self.after,
                    &[
                        (false, if de { "Vor" } else { "Before" }.into()),
                        (true, if de { "Nach" } else { "After" }.into()),
                    ],
                );
                ui.end_row();
                ui.label(if de { "Seite" } else { "Page" });
                ui.horizontal(|ui| {
                    segmented(
                        ui,
                        &mut self.page,
                        &[
                            (TargetPage::First, if de { "Erste" } else { "First" }.into()),
                            (
                                TargetPage::Current,
                                format!("{}: {}", if de { "Aktuell" } else { "Current" }, current + 1),
                            ),
                            (
                                TargetPage::Last,
                                format!("{}: {}", if de { "Letzte" } else { "Last" }, page_count),
                            ),
                            (TargetPage::Custom, if de { "Eigene" } else { "Custom" }.into()),
                        ],
                    );
                    if self.page == TargetPage::Custom {
                        ui.add(egui::DragValue::new(&mut self.custom).range(1..=page_count.max(1) as u32));
                    }
                });
                ui.end_row();
            });
        });
    }
}

// ---------------------------------------------------------------------------
// Units

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Unit {
    #[default]
    Mm,
    Cm,
    Inch,
    Pt,
}

impl Unit {
    pub const ALL: [Unit; 4] = [Unit::Mm, Unit::Cm, Unit::Inch, Unit::Pt];

    /// Points per unit.
    pub fn points(self) -> f32 {
        match self {
            Unit::Mm => 72.0 / 25.4,
            Unit::Cm => 72.0 / 2.54,
            Unit::Inch => 72.0,
            Unit::Pt => 1.0,
        }
    }

    pub fn suffix(self) -> &'static str {
        match self {
            Unit::Mm => "mm",
            Unit::Cm => "cm",
            Unit::Inch => "in",
            Unit::Pt => "pt",
        }
    }

    pub fn format(self, points: f32) -> String {
        let v = points / self.points();
        let decimals = if self == Unit::Pt { 1 } else { 2 };
        format!("{v:.decimals$} {}", self.suffix())
    }
}

/// "Einheit: mm ▾".
pub fn unit_combo(ui: &mut Ui, de: bool, unit: &mut Unit) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(if de { "Einheit:" } else { "Unit:" }).weak());
        egui::ComboBox::from_id_salt(ui.id().with("unit")).selected_text(unit.suffix()).width(50.0).show_ui(
            ui,
            |ui| {
                for u in Unit::ALL {
                    ui.selectable_value(unit, u, u.suffix());
                }
            },
        );
    });
}

/// A length entry: the value is stored in points and shown in `unit`.
pub fn length(
    ui: &mut Ui,
    points: &mut f32,
    unit: Unit,
    range: std::ops::RangeInclusive<f32>,
) -> egui::Response {
    let mut v = *points / unit.points();
    let lo = *range.start() / unit.points();
    let hi = *range.end() / unit.points();
    let resp = ui.add(
        egui::DragValue::new(&mut v)
            .range(lo..=hi)
            .speed(if unit == Unit::Pt { 1.0 } else { 0.1 })
            .max_decimals(2)
            .suffix(format!(" {}", unit.suffix())),
    );
    if resp.changed() {
        *points = v * unit.points();
    }
    resp
}

// ---------------------------------------------------------------------------
// Presets ("Einstellungen: <Benutzerdefiniert> ▾")

/// Stored presets: (dialog key, name, serialized form).
pub type PresetStore = Vec<(String, String, String)>;

/// Result of the preset drop-down.
pub enum PresetAction {
    None,
    Loaded,
    Saved,
}

/// Draws the preset drop-down for the dialog `key`. Loading replaces `form`.
pub fn preset_combo<T: Serialize + DeserializeOwned>(
    ui: &mut Ui,
    de: bool,
    key: &str,
    form: &mut T,
    settings: &mut Settings,
    name_buffer: &mut String,
) -> PresetAction {
    let mut action = PresetAction::None;
    let names: Vec<String> = settings.presets.iter().filter(|p| p.0 == key).map(|p| p.1.clone()).collect();
    let custom = if de { "<Benutzerdefiniert>" } else { "<Custom>" };
    ui.horizontal(|ui| {
        ui.label(if de { "Einstellungen:" } else { "Settings:" });
        egui::ComboBox::from_id_salt(("preset", key)).selected_text(custom).width(150.0).show_ui(ui, |ui| {
            for name in &names {
                if ui.selectable_label(false, name).clicked()
                    && let Some((_, _, data)) = settings.presets.iter().find(|p| p.0 == key && &p.1 == name)
                    && let Ok(loaded) = ron::from_str::<T>(data)
                {
                    *form = loaded;
                    action = PresetAction::Loaded;
                }
            }
            if !names.is_empty() {
                ui.separator();
            }
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(name_buffer).hint_text("Name").desired_width(110.0));
                let can_save = !name_buffer.trim().is_empty();
                if ui
                    .add_enabled(can_save, egui::Button::new(if de { "Speichern" } else { "Save" }))
                    .on_hover_text(if de { "Eigene Einstellungen speichern" } else { "Save own settings" })
                    .clicked()
                    && let Ok(data) = ron::to_string(form)
                {
                    let name: String = name_buffer.trim().chars().take(60).collect();
                    settings.presets.retain(|p| !(p.0 == key && p.1 == name));
                    if settings.presets.iter().filter(|p| p.0 == key).count() < MAX_PRESETS {
                        settings.presets.push((key.to_string(), name, data));
                    }
                    name_buffer.clear();
                    action = PresetAction::Saved;
                }
            });
            if !names.is_empty() {
                ui.menu_button(if de { "Löschen" } else { "Delete" }, |ui| {
                    for name in &names {
                        if ui.button(name).clicked() {
                            settings.presets.retain(|p| !(p.0 == key && &p.1 == name));
                            ui.close();
                        }
                    }
                });
            }
        });
    });
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_and_subsets() {
        let mut r = PageRange::all();
        assert_eq!(r.resolve(5, 2), Some(vec![0, 1, 2, 3, 4]));
        r.subset = Subset::Odd;
        assert_eq!(r.resolve(5, 2), Some(vec![0, 2, 4]));
        r.subset = Subset::Even;
        assert_eq!(r.resolve(5, 2), Some(vec![1, 3]));
        let r = PageRange { mode: RangeMode::Custom, text: "2-3, 5-".into(), subset: Subset::All };
        assert_eq!(r.resolve(6, 0), Some(vec![1, 2, 4, 5]));
        let r = PageRange { mode: RangeMode::Custom, text: "9".into(), subset: Subset::All };
        assert_eq!(r.resolve(6, 0), None);
        assert_eq!(PageRange::current().resolve(6, 3), Some(vec![3]));
        let r = PageRange { mode: RangeMode::Current, subset: Subset::Even, ..PageRange::default() };
        assert_eq!(r.resolve(6, 0), None, "page 1 is odd");
    }

    #[test]
    fn target_insert_index() {
        let t = Target { after: false, page: TargetPage::First, custom: 1 };
        assert_eq!(t.insert_index(10, 4), 0);
        assert_eq!(Target::after_current().insert_index(10, 4), 5);
        assert_eq!(Target::default().insert_index(10, 4), 10);
        let t = Target { after: false, page: TargetPage::Custom, custom: 99 };
        assert_eq!(t.insert_index(10, 4), 9);
        assert_eq!(Target::default().insert_index(0, 0), 0);
    }

    #[test]
    fn units_convert() {
        assert!((Unit::Mm.points() * 25.4 - 72.0).abs() < 1e-4);
        assert_eq!(Unit::Mm.format(72.0), "25.40 mm");
        assert_eq!(Unit::Pt.format(10.0), "10.0 pt");
    }

    #[test]
    fn presets_round_trip_through_ron() {
        let r = PageRange { mode: RangeMode::Custom, text: "1-2".into(), subset: Subset::Even };
        let s = ron::to_string(&r).unwrap();
        assert_eq!(ron::from_str::<PageRange>(&s).unwrap(), r);
    }
}
