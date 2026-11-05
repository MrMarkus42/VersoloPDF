//! Dialogs of the ribbon tab "Umwandeln": "Text auf
//! Seiten erkennen" (OCR with Tesseract), "Schriften ersetzen" and "Seiten
//! neu einfärben".

use std::path::PathBuf;

use egui::{Color32, RichText, Ui};

use super::dialogs::{Action, Outcome};
use super::page_dialogs::{footer, tr};
use super::{Dialog, VersoloPdfApp};
use crate::font_replace::{STANDARD_FONTS, replaceable, suggestion};
use crate::ui::i18n::Language;
use crate::ui::pickers::{PageRange, RangeMode, section};

/// Download page of the Windows installer of Tesseract.
pub const TESSERACT_URL: &str = "https://github.com/UB-Mannheim/tesseract/wiki";

/// State of "Text auf Seiten erkennen".
#[derive(Debug, Clone, PartialEq)]
pub struct OcrState {
    pub range: PageRange,
    pub exe: Option<PathBuf>,
    /// Installed languages and the chosen ones.
    pub available: Vec<String>,
    pub chosen: Vec<String>,
    pub dpi: u32,
    pub skip_text: bool,
}

/// State of "Schriften ersetzen": (font name, kind, embedded, choice).
#[derive(Debug, Clone, PartialEq)]
pub struct FontsState {
    pub fonts: Vec<(String, String, bool, Option<String>)>,
}

/// State of "Seiten neu einfärben".
#[derive(Debug, Clone, PartialEq)]
pub struct RecolorState {
    pub background: Color32,
    pub text: Color32,
}

/// Ready-made colour pairs: (name de, name en, background, text).
const PRESETS: [(&str, &str, [u8; 3], [u8; 3]); 4] = [
    ("Dunkel", "Dark", [32, 33, 36], [232, 232, 232]),
    ("Sepia", "Sepia", [244, 236, 216], [70, 50, 30]),
    ("Hoher Kontrast", "High contrast", [0, 0, 0], [255, 255, 0]),
    ("Blau auf Weiß", "Blue on white", [255, 255, 255], [20, 50, 140]),
];

fn rgb(c: Color32) -> [u8; 3] {
    [c.r(), c.g(), c.b()]
}

impl VersoloPdfApp {
    fn cv_de(&self) -> bool {
        self.settings.language == Language::German
    }

    pub(super) fn open_recognize_text(&mut self) {
        let exe = crate::ocr::find_tesseract(self.settings.tesseract.as_deref());
        let available = exe.as_deref().map(crate::ocr::languages).unwrap_or_default();
        let chosen = self
            .settings
            .ocr_languages
            .split('+')
            .filter(|l| available.iter().any(|a| a == l))
            .map(str::to_string)
            .collect();
        self.dialog = Some(Dialog::RecognizeText(Box::new(OcrState {
            range: PageRange::all(),
            exe,
            available,
            chosen,
            dpi: 300,
            skip_text: true,
        })));
    }

    pub(super) fn recognize_text_dialog(
        &mut self,
        ui: &mut Ui,
        s: &mut OcrState,
    ) -> (Outcome, Option<Action>) {
        let de = self.cv_de();
        let (n, cur) = self.active_tab().map_or((0, 0), |t| (t.doc.page_count(), t.current_page));
        super::dialogs::set_title(ui, tr(de, "Text auf Seiten erkennen", "Recognize text"));
        ui.set_max_width(520.0);
        ui.label(
            RichText::new(tr(
                de,
                "Erkannter Text wird unsichtbar über die Seite gelegt: gescannte Seiten lassen sich danach durchsuchen und kopieren, ihr Aussehen bleibt gleich.",
                "Recognised text is laid invisibly over the page: scanned pages can then be searched and copied, they look the same.",
            ))
            .weak(),
        );
        match s.exe.clone() {
            None => {
                ui.add_space(6.0);
                ui.colored_label(
                    Color32::from_rgb(190, 90, 0),
                    tr(
                        de,
                        "Für die Texterkennung nutzt VersoloPDF das kostenlose Programm Tesseract (Open Source, Apache-Lizenz). Es wurde nicht gefunden.",
                        "For text recognition VersoloPDF uses the free program Tesseract (open source, Apache licence). It was not found.",
                    ),
                );
                ui.horizontal(|ui| {
                    ui.label(tr(de, "Download für Windows:", "Download for Windows:"));
                    ui.hyperlink_to(TESSERACT_URL, TESSERACT_URL);
                });
                ui.label(
                    RichText::new(tr(
                        de,
                        "Bei der Installation die Sprache „German“ mit auswählen.",
                        "Select the language \"German\" during installation if needed.",
                    ))
                    .small()
                    .weak(),
                );
                if ui
                    .button(tr(de, "Tesseract-Programm auswählen…", "Choose the Tesseract program…"))
                    .clicked()
                    && let Some(p) = rfd::FileDialog::new().pick_file()
                    && p.is_file()
                {
                    self.settings.tesseract = Some(p.clone());
                    s.available = crate::ocr::languages(&p);
                    s.exe = Some(p);
                }
            }
            Some(exe) => {
                ui.label(RichText::new(format!("Tesseract: {}", exe.display())).small().weak());
                s.range.ui(ui, de, n, cur, &[RangeMode::All, RangeMode::Current, RangeMode::Custom]);
                section(ui, tr(de, "Sprachen", "Languages"), |ui| {
                    if s.available.is_empty() {
                        ui.label(tr(de, "Keine Sprachen gefunden.", "No languages found."));
                    }
                    ui.horizontal_wrapped(|ui| {
                        let mut list = s.available.clone();
                        list.sort_by_key(|l| (!matches!(l.as_str(), "deu" | "eng"), l.clone()));
                        for lang in list.iter().filter(|l| *l != "osd").take(40) {
                            let mut on = s.chosen.contains(lang);
                            if ui.checkbox(&mut on, lang.as_str()).changed() {
                                s.chosen.retain(|l| l != lang);
                                if on {
                                    s.chosen.push(lang.clone());
                                }
                            }
                        }
                    });
                });
                ui.horizontal(|ui| {
                    ui.label(tr(de, "Auflösung:", "Resolution:"));
                    egui::ComboBox::from_id_salt("ocr-dpi").selected_text(format!("{} dpi", s.dpi)).show_ui(
                        ui,
                        |ui| {
                            for d in [200, 300, 400] {
                                ui.selectable_value(&mut s.dpi, d, format!("{d} dpi"));
                            }
                        },
                    );
                });
                ui.checkbox(
                    &mut s.skip_text,
                    tr(de, "Seiten mit Text überspringen", "Skip pages that have text"),
                );
                ui.label(
                    RichText::new(tr(
                        de,
                        "Das Erkennen dauert einige Sekunden je Seite.",
                        "Recognition takes a few seconds per page.",
                    ))
                    .small()
                    .weak(),
                );
            }
        }
        let pages = s.range.resolve(n, cur);
        let ready = s.exe.is_some() && !s.chosen.is_empty() && pages.is_some();
        let (ok, cancel, _) = footer(ui, de, ready, false, "OK");
        if cancel {
            return (Outcome::Close, None);
        }
        let (true, Some(pages), Some(exe)) = (ok, pages, s.exe.clone()) else { return (Outcome::Keep, None) };
        let languages = s.chosen.join("+");
        self.settings.ocr_languages = languages.clone();
        let (dpi, skip) = (s.dpi as f32, s.skip_text);
        (
            Outcome::Close,
            Some(Box::new(move |app, ctx| {
                let engine = crate::ocr::Engine { exe, languages };
                let result = app.tab().map(|t| {
                    let r = t.doc.recognize_text(&pages, &engine, dpi, skip);
                    t.after_edit();
                    r
                });
                let de = app.cv_de();
                match result {
                    Some(Ok(words)) => app.set_status(
                        ctx,
                        if de {
                            format!("Erkannte Wörter: {words}")
                        } else {
                            format!("Recognised words: {words}")
                        },
                    ),
                    Some(Err(err)) => app.show_error(&err),
                    None => {}
                }
            })),
        )
    }

    pub(super) fn open_replace_fonts(&mut self) {
        let Some(tab) = self.tab() else { return };
        match tab.doc.fonts() {
            Ok(fonts) => {
                let fonts = fonts.into_iter().map(|f| (f.name, f.kind, f.embedded, None)).collect();
                self.dialog = Some(Dialog::ReplaceFonts(Box::new(FontsState { fonts })));
            }
            Err(err) => self.show_error(&err),
        }
    }

    pub(super) fn replace_fonts_dialog(
        &mut self,
        ui: &mut Ui,
        s: &mut FontsState,
    ) -> (Outcome, Option<Action>) {
        let de = self.cv_de();
        super::dialogs::set_title(ui, tr(de, "Schriften ersetzen", "Replace fonts"));
        ui.label(
            RichText::new(tr(
                de,
                "Ersetzt Schriften durch eine der Standardschriften, die jedes PDF-Programm hat. Die Abstände bleiben; die eingebettete Schrift entfällt.",
                "Replaces fonts with one of the standard fonts every PDF program has. Spacing stays; the embedded font is dropped.",
            ))
            .weak(),
        );
        if ui.button(tr(de, "Vorschläge übernehmen", "Use suggestions")).clicked() {
            for f in &mut s.fonts {
                if replaceable(&f.1) {
                    f.3 = Some(suggestion(&f.0).to_string());
                }
            }
        }
        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            egui::Grid::new("fonts").striped(true).num_columns(4).show(ui, |ui| {
                for h in [tr(de, "Schrift", "Font"), tr(de, "Art", "Type"), tr(de, "Eingebettet", "Embedded"), tr(de, "Ersetzen durch", "Replace with")] {
                    ui.label(RichText::new(h).weak());
                }
                ui.end_row();
                for (i, (name, kind, embedded, choice)) in s.fonts.iter_mut().enumerate() {
                    ui.label(name.as_str());
                    ui.label(kind.as_str());
                    ui.label(if *embedded { tr(de, "ja", "yes") } else { tr(de, "nein", "no") });
                    let none = tr(de, "(nicht ersetzen)", "(keep)");
                    ui.add_enabled_ui(replaceable(kind), |ui| {
                        egui::ComboBox::from_id_salt(("font-choice", i))
                            .selected_text(choice.as_deref().unwrap_or(none))
                            .width(180.0)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(choice, None, none);
                                for f in STANDARD_FONTS {
                                    ui.selectable_value(choice, Some(f.to_string()), f);
                                }
                            });
                    })
                    .response
                    .on_disabled_hover_text(tr(
                        de,
                        "Zusammengesetzte Schriften (Type 0, Type 3) lassen sich nicht ersetzen, ohne dass falsche Zeichen erscheinen.",
                        "Composite fonts (Type 0, Type 3) cannot be replaced without showing wrong characters.",
                    ));
                    ui.end_row();
                }
            });
        });
        let map: Vec<(String, String)> =
            s.fonts.iter().filter_map(|(n, _, _, c)| c.as_ref().map(|c| (n.clone(), c.clone()))).collect();
        let (ok, cancel, _) = footer(ui, de, !map.is_empty(), false, "OK");
        if cancel {
            return (Outcome::Close, None);
        }
        if !ok {
            return (Outcome::Keep, None);
        }
        (
            Outcome::Close,
            Some(Box::new(move |app, ctx| {
                let result = app.tab().map(|t| {
                    let r = t.doc.replace_fonts(&map);
                    t.after_edit();
                    r
                });
                let de = app.cv_de();
                match result {
                    Some(Ok(n)) => app.set_status(
                        ctx,
                        if de { format!("Ersetzte Schriften: {n}") } else { format!("Replaced fonts: {n}") },
                    ),
                    Some(Err(err)) => app.show_error(&err),
                    None => {}
                }
            })),
        )
    }

    pub(super) fn open_recolor(&mut self) {
        let [b, t] = [PRESETS[1].2, PRESETS[1].3];
        self.dialog = Some(Dialog::Recolor(Box::new(RecolorState {
            background: Color32::from_rgb(b[0], b[1], b[2]),
            text: Color32::from_rgb(t[0], t[1], t[2]),
        })));
    }

    pub(super) fn recolor_dialog(&mut self, ui: &mut Ui, s: &mut RecolorState) -> (Outcome, Option<Action>) {
        let de = self.cv_de();
        super::dialogs::set_title(ui, tr(de, "Seiten neu einfärben", "Recolor pages"));
        ui.label(
            RichText::new(tr(
                de,
                "Dunkle Teile bekommen die Textfarbe, helle Teile und das Papier die Hintergrundfarbe. Die Änderung wird im Dokument gespeichert (anders als „Dokumentfarben ersetzen“ in Ansicht). Farbverläufe behalten ihre Farben.",
                "Dark parts get the text colour, light parts and the paper the background colour. The change is saved in the document (unlike \"Replace document colours\" in View). Gradients keep their colours.",
            ))
            .weak(),
        );
        ui.horizontal(|ui| {
            for (dn, en, b, t) in PRESETS {
                let label = RichText::new(tr(de, dn, en)).color(Color32::from_rgb(t[0], t[1], t[2]));
                if ui.add(egui::Button::new(label).fill(Color32::from_rgb(b[0], b[1], b[2]))).clicked() {
                    s.background = Color32::from_rgb(b[0], b[1], b[2]);
                    s.text = Color32::from_rgb(t[0], t[1], t[2]);
                }
            }
        });
        egui::Grid::new("recolor").num_columns(2).show(ui, |ui| {
            ui.label(tr(de, "Hintergrund:", "Background:"));
            ui.color_edit_button_srgba(&mut s.background);
            ui.end_row();
            ui.label(tr(de, "Text:", "Text:"));
            ui.color_edit_button_srgba(&mut s.text);
            ui.end_row();
        });
        let (rect, _) = ui.allocate_exact_size(egui::vec2(260.0, 40.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 4.0, s.background);
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            tr(de, "Beispieltext", "Sample text"),
            egui::FontId::proportional(15.0),
            s.text,
        );
        let (ok, cancel, _) = footer(ui, de, true, false, "OK");
        if cancel {
            return (Outcome::Close, None);
        }
        if !ok {
            return (Outcome::Keep, None);
        }
        let (bg, fg) = (rgb(s.background), rgb(s.text));
        (Outcome::Close, Some(Box::new(move |app, ctx| app.edit(ctx, |tab| tab.doc.recolor_pages(bg, fg)))))
    }
}
