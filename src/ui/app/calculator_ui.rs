//! "Rechner" windows of the ribbon tab "Anmerkungen": a desk calculator
//! whose tape can be placed on the current page as a text box.

use egui::{Context, RichText, Ui, vec2};

use super::VersoloPdfApp;
use crate::calculator::{Calculator, Key, format_number};
use crate::geometry::RectF;
use crate::model::AnnotKind;
use crate::pdf_edit::NewAnnotation;
use crate::ui::i18n::Language;
use crate::ui::icons::Icon;
use crate::ui::tools::Tool;
use crate::ui::widgets::icon_button;

/// One open calculator.
pub struct CalcWindow {
    pub id: usize,
    pub calc: Calculator,
    /// "Bezeichnung für Rechnung": first line of the inserted text.
    pub title: String,
    /// "Vorschau": shows the text that "Einfügen" places.
    pub preview: bool,
    closed: bool,
}

const KEY: f32 = 30.0;

/// Keypad rows: (label, key, columns).
const ROWS: [&[(&str, Key, u8)]; 5] = [
    &[
        ("", Key::Backspace, 2),
        ("MR", Key::MemoryRecall, 1),
        ("MC", Key::MemoryClear, 1),
        ("MS", Key::MemoryStore, 1),
        ("M+", Key::MemoryAdd, 1),
        ("M-", Key::MemorySub, 1),
    ],
    &[
        ("×", Key::Mul, 1),
        ("±", Key::Negate, 1),
        ("7", Key::Digit(7), 1),
        ("8", Key::Digit(8), 1),
        ("9", Key::Digit(9), 1),
        ("TAX+", Key::TaxPlus, 1),
        ("TAX-", Key::TaxMinus, 1),
    ],
    &[
        ("÷", Key::Div, 1),
        ("%", Key::Percent, 1),
        ("4", Key::Digit(4), 1),
        ("5", Key::Digit(5), 1),
        ("6", Key::Digit(6), 1),
        ("−", Key::Sub, 1),
        ("GT", Key::GrandTotal, 1),
    ],
    &[
        ("C", Key::Clear, 1),
        ("√", Key::Sqrt, 1),
        ("1", Key::Digit(1), 1),
        ("2", Key::Digit(2), 1),
        ("3", Key::Digit(3), 1),
        ("+", Key::Add, 1),
        ("ST", Key::Subtotal, 1),
    ],
    &[
        ("CE", Key::ClearEntry, 1),
        ("1/x", Key::Recip, 1),
        ("0", Key::Digit(0), 1),
        ("00", Key::DoubleZero, 1),
        (",", Key::Point, 1),
        ("=", Key::Equals, 2),
    ],
];

fn keypad(ui: &mut Ui, calc: &mut Calculator, de: bool) {
    let gap = ui.spacing().item_spacing.x;
    ui.vertical(|ui| {
        for row in ROWS {
            ui.horizontal(|ui| {
                for (label, key, cols) in row {
                    let w = KEY * f32::from(*cols) + gap * f32::from(cols - 1);
                    let resp = ui.add_sized(vec2(w, KEY), egui::Button::new(*label));
                    if *key == Key::Backspace {
                        // Drawn as icon: the fonts have no backspace sign.
                        let color = ui.visuals().text_color();
                        let r = egui::Rect::from_center_size(resp.rect.center(), vec2(16.0, 16.0));
                        crate::ui::icons::paint(ui.painter(), r, Icon::Back, color);
                    }
                    let resp = if label.is_empty() {
                        resp.on_hover_text(if de { "Letzte Ziffer löschen" } else { "Delete last digit" })
                    } else {
                        resp
                    };
                    if resp.clicked() {
                        calc.press(*key);
                    }
                }
            });
        }
    });
}

impl VersoloPdfApp {
    fn calc_de(&self) -> bool {
        self.settings.language == Language::German
    }

    /// "Rechner": opens a new calculator window.
    pub(super) fn open_calculator(&mut self) {
        let mut calc = Calculator::default();
        calc.tax_rate = self.settings.tax_rate;
        self.calculators.push(CalcWindow {
            id: self.next_calculator,
            calc,
            title: String::new(),
            preview: false,
            closed: false,
        });
        self.next_calculator += 1;
    }

    /// Number of open calculator windows.
    pub fn open_calculators(&self) -> usize {
        self.calculators.len()
    }

    pub(super) fn close_calculators(&mut self) {
        self.calculators.clear();
    }

    /// Names of the open calculators (drop-down of "Rechner").
    pub(super) fn calculator_names(&self) -> Vec<(usize, String)> {
        let de = self.calc_de();
        self.calculators
            .iter()
            .map(|w| {
                let name = if w.title.trim().is_empty() {
                    format!("{} #{}", if de { "Rechner" } else { "Calculator" }, w.id)
                } else {
                    w.title.trim().to_string()
                };
                (w.id, name)
            })
            .collect()
    }

    pub(super) fn calculator_windows(&mut self, ctx: &Context) {
        let de = self.calc_de();
        let has_doc = !self.tabs.is_empty();
        let mut insert = None;
        for w in &mut self.calculators {
            let mut open = true;
            let title = format!("{} #{}", if de { "Rechner" } else { "Calculator" }, w.id);
            egui::Window::new(title)
                .id(egui::Id::new(("calculator", w.id)))
                .open(&mut open)
                .resizable(false)
                .collapsible(false)
                .default_pos(ctx.content_rect().center() - vec2(240.0, 130.0))
                .show(ctx, |ui| {
                    ui.horizontal_top(|ui| {
                        ui.vertical(|ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(w.calc.display(de)).size(22.0).strong().monospace(),
                                )
                                .halign(egui::Align::RIGHT),
                            );
                            ui.add_space(4.0);
                            keypad(ui, &mut w.calc, de);
                            ui.add_space(4.0);
                            let memory = format_number(w.calc.memory, 2);
                            let memory = if de { memory.replace('.', ",") } else { memory };
                            ui.label(
                                RichText::new(format!(
                                    "{} = {memory}",
                                    if de { "Speicher" } else { "Memory" }
                                ))
                                .strong(),
                            );
                        });
                        ui.add_space(10.0);
                        ui.vertical(|ui| {
                            ui.set_width(250.0);
                            ui.add(
                                egui::TextEdit::singleline(&mut w.title)
                                    .hint_text(if de {
                                        "Bezeichnung für Rechnung"
                                    } else {
                                        "Title of the calculation"
                                    })
                                    .desired_width(f32::INFINITY),
                            );
                            ui.horizontal(|ui| {
                                let undo = if de { "Rückgängig" } else { "Undo" };
                                if icon_button(ui, Icon::Undo, undo, false, !w.calc.tape.is_empty()).clicked()
                                {
                                    w.calc.undo();
                                }
                                let redo = if de { "Wiederholen" } else { "Redo" };
                                if icon_button(ui, Icon::Redo, redo, false, w.calc.can_redo()).clicked() {
                                    w.calc.redo();
                                }
                                if ui
                                    .add_enabled(
                                        !w.calc.tape.is_empty(),
                                        egui::Button::new(if de { "Liste leeren" } else { "Clear list" }),
                                    )
                                    .clicked()
                                {
                                    w.calc.clear_tape();
                                }
                            });
                            egui::Frame::group(ui.style()).show(ui, |ui| {
                                egui::ScrollArea::vertical()
                                    .max_height(150.0)
                                    .min_scrolled_height(150.0)
                                    .stick_to_bottom(true)
                                    .show(ui, |ui| {
                                        ui.set_min_width(230.0);
                                        egui::Grid::new(("tape", w.id)).striped(true).num_columns(3).show(
                                            ui,
                                            |ui| {
                                                ui.label(RichText::new("#").weak());
                                                ui.label(
                                                    RichText::new(if de { "Menge" } else { "Amount" }).weak(),
                                                );
                                                ui.label(
                                                    RichText::new(if de { "Rechenart" } else { "Operation" })
                                                        .weak(),
                                                );
                                                ui.end_row();
                                                for (i, line) in w.calc.tape.iter().enumerate() {
                                                    let n = format_number(line.value, 2);
                                                    ui.label(format!("{}", i + 1));
                                                    ui.label(
                                                        RichText::new(if de {
                                                            n.replace('.', ",")
                                                        } else {
                                                            n
                                                        })
                                                        .monospace(),
                                                    );
                                                    ui.label(line.op.symbol());
                                                    ui.end_row();
                                                }
                                            },
                                        );
                                    });
                            });
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut w.preview, if de { "Vorschau" } else { "Preview" });
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let ok = has_doc && !w.calc.tape.is_empty();
                                    if ui
                                        .add_enabled(
                                            ok,
                                            egui::Button::new(if de { "Einfügen" } else { "Insert" }),
                                        )
                                        .clicked()
                                    {
                                        insert = Some(w.calc.tape_text(&w.title, de));
                                    }
                                });
                            });
                            if w.preview {
                                ui.label(RichText::new(w.calc.tape_text(&w.title, de)).monospace().weak());
                            }
                        });
                    });
                });
            if !open {
                w.closed = true;
            }
        }
        self.calculators.retain(|w| !w.closed);
        if let Some(text) = insert {
            self.insert_calculation(ctx, text);
        }
    }

    /// Places the tape as a text box at the top left of the current page.
    pub(super) fn insert_calculation(&mut self, ctx: &Context, text: String) {
        let mut style = self.settings.style(Tool::Typewriter);
        style.width = 0.0;
        style.fill = None;
        let size = style.font_size.max(4.0);
        let width = text
            .lines()
            .map(|l| crate::appearance::StdFont::Helvetica.text_width(&crate::appearance::win_ansi(l), size))
            .fold(10.0, f32::max)
            + 6.0;
        let height = text.lines().count().max(1) as f32 * size * 1.2 + 4.0;
        let Some(tab) = self.active_tab() else { return };
        let page = tab.current_page;
        let Some(crop) = tab.doc.geometry(page).map(|g| g.crop.normalized()) else { return };
        let mut annot = NewAnnotation::new(AnnotKind::FreeText, style);
        let (x, top) = (crop.x0 + 36.0, crop.y1 - 36.0);
        annot.rect = RectF::new(x, top - height, x + width, top);
        annot.contents = text;
        self.edit(ctx, |tab| tab.doc.add_annotation(page, &annot));
    }
}
