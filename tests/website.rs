//! Screenshots for the project web site (`website/img`). Not part of the
//! normal test run:
//!
//! ```text
//! VERSOLOPDF_SCREENSHOT_DIR=website/img cargo test --test website -- --ignored
//! ```
//!
//! The sample document is generated here (a made-up annual report), so no
//! real file appears on the web site.

mod common;

use std::path::{Path, PathBuf};

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use pdfium_render::prelude::*;
use versolopdf::geometry::{PointF, RectF};
use versolopdf::model::{AnnotKind, AnnotStyle, Rgba};
use versolopdf::pdf_edit::NewAnnotation;
use versolopdf::ui::VersoloPdfApp;
use versolopdf::ui::app::commands::Cmd;
use versolopdf::ui::i18n::Language;
use versolopdf::ui::settings::Settings;

fn out_dir() -> PathBuf {
    let dir = std::env::var_os("VERSOLOPDF_SCREENSHOT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/website-screenshots"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn save(h: &mut Harness<'_, VersoloPdfApp>, name: &str) {
    let img = h.render().expect("rendering");
    img.save(out_dir().join(format!("{name}.png"))).unwrap();
}

fn harness(files: Vec<PathBuf>, dark: bool, language: Language) -> Harness<'static, VersoloPdfApp> {
    let settings = Settings {
        language,
        author: "Mr. M".into(),
        dark_mode: dark,
        backstage_at_start: false,
        ..Settings::default()
    };
    Harness::builder().with_size(egui::vec2(1360.0, 860.0)).with_max_steps(64).wgpu().build_eframe(
        move |cc| {
            let mut app = VersoloPdfApp::with_settings(&cc.egui_ctx, settings, files);
            app.set_recovery_dir(None);
            app
        },
    )
}

fn tab(h: &mut Harness<'_, VersoloPdfApp>, label: &str) {
    h.get_all_by_label(label).next().unwrap().click();
    h.run_steps(4);
}

fn quick(h: &mut Harness<'_, VersoloPdfApp>, query: &str) {
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Period);
    h.run_steps(3);
    h.input_mut().events.push(egui::Event::Text(query.into()));
    h.run_steps(2);
    h.key_press(egui::Key::Enter);
    h.run_steps(8);
}

fn fit_page(h: &mut Harness<'_, VersoloPdfApp>) {
    h.state_mut().active_tab_mut().unwrap().zoom_mode = versolopdf::ui::view::ZoomMode::FitPage;
    h.run_steps(8);
}

/// A registration form with labels; the fields are added by `add_form_fields`.
fn form_page_pdf() -> Vec<u8> {
    let pdfium = versolopdf::engine::pdfium().unwrap();
    let mut doc = pdfium.create_new_pdf().unwrap();
    let regular = doc.fonts_mut().helvetica();
    let bold = doc.fonts_mut().helvetica_bold();
    let mut page = doc.pages_mut().create_page_at_end(PdfPagePaperSize::a4()).unwrap();
    let objects = page.objects_mut();
    objects
        .create_path_object_rect(
            PdfRect::new_from_values(762.0, 0.0, 842.0, 595.0),
            None,
            None,
            Some(PdfColor::new(224, 242, 246, 255)),
        )
        .unwrap();
    objects
        .create_path_object_rect(
            PdfRect::new_from_values(762.0, 0.0, 766.0, 595.0),
            None,
            None,
            Some(PdfColor::new(TEAL.0, TEAL.1, TEAL.2, 255)),
        )
        .unwrap();
    let mut t = |x: f32, y: f32, s: &str, font: PdfFontToken, size: f32| {
        objects
            .create_text_object(PdfPoints::new(x), PdfPoints::new(y), s, font, PdfPoints::new(size))
            .unwrap();
    };
    t(56.0, 806.0, "Anmeldung Sommerfest", bold, 24.0);
    t(56.0, 782.0, "Grüne Stadtwerke Musterstadt · Samstag, 4. Juli 2026", regular, 12.0);
    t(
        56.0,
        720.0,
        "Bitte füllen Sie das Formular aus und senden Sie es bis zum 15. Juni zurück.",
        regular,
        11.0,
    );
    for (y, label) in [
        (676.0, "Name:"),
        (636.0, "E-Mail:"),
        (596.0, "Anreisedatum:"),
        (556.0, "Teilnahme:"),
        (516.0, "Essenswunsch:"),
        (476.0, "Mitbringen:"),
        (436.0, "Anmerkungen:"),
    ] {
        t(56.0, y, label, bold, 11.0);
    }
    t(200.0, 556.0, "mit Begleitung", regular, 11.0);
    t(340.0, 556.0, "allein", regular, 11.0);
    t(220.0, 476.0, "Ich bringe einen Kuchen für das Buffet mit", regular, 11.0);
    doc.save_to_bytes().unwrap()
}

fn add_form_fields(h: &mut Harness<'_, VersoloPdfApp>) {
    use versolopdf::forms::{ButtonAction, NewFieldKind};
    use versolopdf::model::FieldKind;
    let tab = h.state_mut().active_tab_mut().unwrap();
    let r = |x: f32, y: f32, w: f32, hh: f32| RectF::new(x, y, x + w, y + hh);
    let d = &mut tab.doc;
    d.create_field(0, r(180.0, 670.0, 300.0, 22.0), &NewFieldKind::Text { multiline: false }, "Name")
        .unwrap();
    d.create_field(0, r(180.0, 630.0, 300.0, 22.0), &NewFieldKind::Text { multiline: false }, "EMail")
        .unwrap();
    d.create_field(
        0,
        r(180.0, 590.0, 120.0, 22.0),
        &NewFieldKind::Date { format: "dd.mm.yyyy".into() },
        "Datum",
    )
    .unwrap();
    for (x, export) in [(180.0, "Begleitung"), (320.0, "Allein")] {
        let kind = NewFieldKind::Radio { group: "Teilnahme".into(), export: export.into() };
        d.create_field(0, r(x, 552.0, 14.0, 14.0), &kind, "").unwrap();
    }
    let options = vec!["Vegetarisch".to_string(), "Vegan".into(), "Mit Fleisch".into()];
    d.create_field(0, r(180.0, 510.0, 180.0, 22.0), &NewFieldKind::ComboBox { options }, "Essen").unwrap();
    d.create_field(0, r(180.0, 472.0, 14.0, 14.0), &NewFieldKind::CheckBox, "Kuchen").unwrap();
    d.create_field(0, r(180.0, 360.0, 300.0, 80.0), &NewFieldKind::Text { multiline: true }, "Anmerkungen")
        .unwrap();
    d.create_field(
        0,
        r(180.0, 310.0, 120.0, 26.0),
        &NewFieldKind::Button { caption: "Zurücksetzen".into(), action: ButtonAction::ResetForm },
        "",
    )
    .unwrap();
    d.create_field(
        0,
        r(330.0, 290.0, 150.0, 48.0),
        &NewFieldKind::Signature { label: "Unterschrift".into() },
        "",
    )
    .unwrap();
    let fields = d.form_fields(0).unwrap();
    let find = |name: &str| fields.iter().find(|f| f.name == name).unwrap().annot_index;
    d.set_text_field(0, find("Name"), "Erika Mustermann").unwrap();
    d.set_text_field(0, find("EMail"), "erika@example.org").unwrap();
    d.set_text_field(0, find("Datum"), "04.07.2026").unwrap();
    d.set_choice(0, find("Essen"), "Vegetarisch").unwrap();
    d.set_checked(0, find("Kuchen"), true).unwrap();
    let radio = fields.iter().find(|f| f.kind == FieldKind::RadioButton).unwrap().annot_index;
    d.set_checked(0, radio, true).unwrap();
    tab.after_edit();
    h.run_steps(8);
}

fn run(h: &mut Harness<'_, VersoloPdfApp>, cmd: Cmd) {
    let ctx = h.ctx.clone();
    h.state_mut().run(&ctx, cmd);
    h.run_steps(6);
}

const TEAL: (u8, u8, u8) = (14, 116, 144);

/// Splits a paragraph into lines of at most `width` characters.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = vec![String::new()];
    for word in text.split_whitespace() {
        let last = lines.last_mut().unwrap();
        if !last.is_empty() && last.len() + word.len() + 1 > width {
            lines.push(word.to_string());
        } else {
            if !last.is_empty() {
                last.push(' ');
            }
            last.push_str(word);
        }
    }
    lines
}

/// A three-page sample report with a chart, figures and a table.
fn report_pdf() -> Vec<u8> {
    let pdfium = versolopdf::engine::pdfium().unwrap();
    let mut doc = pdfium.create_new_pdf().unwrap();
    let regular = doc.fonts_mut().helvetica();
    let bold = doc.fonts_mut().helvetica_bold();
    let rgb = |(r, g, b): (u8, u8, u8)| PdfColor::new(r, g, b, 255);
    let paragraphs: [(&str, &[&str]); 3] = [
        (
            "1. Überblick",
            &[
                "Das Jahr 2026 stand ganz im Zeichen der Energiewende vor Ort. Mit drei neuen Solarparks und dem Ausbau des Wärmenetzes konnten die Grünen Stadtwerke Musterstadt den Anteil erneuerbarer Energie auf 68 Prozent steigern.",
                "Gleichzeitig sanken die Kosten für unsere Kundinnen und Kunden: Der Grundpreis blieb stabil, der Arbeitspreis ging im zweiten Halbjahr um vier Prozent zurück.",
            ],
        ),
        (
            "2. Projekte",
            &[
                "Solarpark Nord: 12 Megawatt Leistung auf einer ehemaligen Deponie, in Betrieb seit April. Wärmenetz Altstadt: 4,8 Kilometer neue Leitungen, 310 Gebäude angeschlossen.",
                "Ladeinfrastruktur: 140 öffentliche Ladepunkte, davon 26 Schnelllader. Digitale Zähler: 18.500 Haushalte können ihren Verbrauch jetzt in der App verfolgen.",
            ],
        ),
        (
            "3. Ausblick",
            &[
                "Für 2027 planen wir zwei Windenergieanlagen im Gewerbegebiet Süd und einen Großwärmespeicher. Bis 2030 soll die Versorgung vollständig klimaneutral sein.",
                "Wir danken allen Mitarbeiterinnen und Mitarbeitern sowie unseren Partnern für die engagierte Zusammenarbeit in diesem besonderen Jahr.",
            ],
        ),
    ];
    for (index, (heading, texts)) in paragraphs.iter().enumerate() {
        let mut page = doc.pages_mut().create_page_at_end(PdfPagePaperSize::a4()).unwrap();
        let objects = page.objects_mut();
        // Coloured header band with the title.
        objects
            .create_path_object_rect(
                PdfRect::new_from_values(762.0, 0.0, 842.0, 595.0),
                None,
                None,
                Some(rgb((224, 242, 246))),
            )
            .unwrap();
        objects
            .create_path_object_rect(
                PdfRect::new_from_values(762.0, 0.0, 766.0, 595.0),
                None,
                None,
                Some(rgb(TEAL)),
            )
            .unwrap();
        let mut text = |x: f32, y: f32, s: &str, font: PdfFontToken, size: f32| {
            objects
                .create_text_object(PdfPoints::new(x), PdfPoints::new(y), s, font, PdfPoints::new(size))
                .unwrap();
        };
        text(56.0, 806.0, "Jahresbericht 2026", bold, 24.0);
        text(56.0, 782.0, "Grüne Stadtwerke Musterstadt", regular, 12.0);
        text(500.0, 782.0, &format!("Seite {}", index + 1), regular, 10.0);
        text(56.0, 720.0, heading, bold, 17.0);
        let mut y = 690.0;
        for paragraph in texts.iter() {
            for line in wrap(paragraph, 84) {
                text(56.0, y, &line, regular, 11.0);
                y -= 16.0;
            }
            y -= 10.0;
        }
        if index == 0 {
            // Key figures.
            for (k, (value, label)) in
                [("68 %", "erneuerbare Energie"), ("-4 %", "Arbeitspreis"), ("310", "neue Anschlüsse")]
                    .iter()
                    .enumerate()
            {
                let x = 56.0 + k as f32 * 166.0;
                objects
                    .create_path_object_rect(
                        PdfRect::new_from_values(470.0, x, 540.0, x + 150.0),
                        Some(rgb((190, 222, 230))),
                        Some(PdfPoints::new(1.0)),
                        Some(rgb((246, 251, 252))),
                    )
                    .unwrap();
                let mut t = |x: f32, y: f32, s: &str, font: PdfFontToken, size: f32| {
                    objects
                        .create_text_object(
                            PdfPoints::new(x),
                            PdfPoints::new(y),
                            s,
                            font,
                            PdfPoints::new(size),
                        )
                        .unwrap();
                };
                t(x + 14.0, 508.0, value, bold, 22.0);
                t(x + 14.0, 484.0, label, regular, 10.0);
            }
            // Bar chart: share of renewable energy.
            let mut t = |x: f32, y: f32, s: &str, font: PdfFontToken, size: f32| {
                objects
                    .create_text_object(PdfPoints::new(x), PdfPoints::new(y), s, font, PdfPoints::new(size))
                    .unwrap();
            };
            t(56.0, 430.0, "Anteil erneuerbarer Energie", bold, 12.0);
            let bars = [("2022", 31.0), ("2023", 39.0), ("2024", 47.0), ("2025", 56.0), ("2026", 68.0)];
            for (k, (year, value)) in bars.iter().enumerate() {
                let x = 80.0 + k as f32 * 90.0;
                let h = value * 3.2;
                let color = if k == bars.len() - 1 { TEAL } else { (126, 186, 201) };
                objects
                    .create_path_object_rect(
                        PdfRect::new_from_values(180.0, x, 180.0 + h, x + 50.0),
                        None,
                        None,
                        Some(rgb(color)),
                    )
                    .unwrap();
                let mut t = |x: f32, y: f32, s: &str, font: PdfFontToken, size: f32| {
                    objects
                        .create_text_object(
                            PdfPoints::new(x),
                            PdfPoints::new(y),
                            s,
                            font,
                            PdfPoints::new(size),
                        )
                        .unwrap();
                };
                t(x + 10.0, 164.0, year, regular, 10.0);
                t(x + 12.0, 186.0 + h, &format!("{value:.0} %"), bold, 10.0);
            }
            objects
                .create_path_object_line(
                    PdfPoints::new(66.0),
                    PdfPoints::new(180.0),
                    PdfPoints::new(530.0),
                    PdfPoints::new(180.0),
                    rgb((120, 120, 120)),
                    PdfPoints::new(0.8),
                )
                .unwrap();
        }
        if index == 1 {
            // A small table.
            let rows = [
                ("Projekt", "Investition", "Status"),
                ("Solarpark Nord", "9,4 Mio. €", "in Betrieb"),
                ("Wärmenetz Altstadt", "6,1 Mio. €", "in Betrieb"),
                ("Ladepunkte", "2,3 Mio. €", "laufend"),
                ("Windenergie Süd", "14,0 Mio. €", "in Planung"),
            ];
            for (r, (a, b, c)) in rows.iter().enumerate() {
                let y = 500.0 - r as f32 * 26.0;
                if r == 0 {
                    objects
                        .create_path_object_rect(
                            PdfRect::new_from_values(y - 8.0, 56.0, y + 18.0, 540.0),
                            None,
                            None,
                            Some(rgb((224, 242, 246))),
                        )
                        .unwrap();
                }
                let font = if r == 0 { bold } else { regular };
                let mut t = |x: f32, s: &str| {
                    objects
                        .create_text_object(
                            PdfPoints::new(x),
                            PdfPoints::new(y),
                            s,
                            font,
                            PdfPoints::new(11.0),
                        )
                        .unwrap();
                };
                t(66.0, a);
                t(280.0, b);
                t(420.0, c);
                objects
                    .create_path_object_line(
                        PdfPoints::new(56.0),
                        PdfPoints::new(y - 8.0),
                        PdfPoints::new(540.0),
                        PdfPoints::new(y - 8.0),
                        rgb((200, 210, 214)),
                        PdfPoints::new(0.6),
                    )
                    .unwrap();
            }
        }
    }
    doc.save_to_bytes().unwrap()
}

fn style(color: Rgba) -> AnnotStyle {
    AnnotStyle { color, author: "Mr. M".into(), ..AnnotStyle::default() }
}

/// Comments as a reviewer would add them.
fn annotate(h: &mut Harness<'_, VersoloPdfApp>) {
    let tab = h.state_mut().active_tab_mut().unwrap();
    let mut hl = NewAnnotation::new(AnnotKind::Highlight, style(Rgba { r: 255, g: 226, b: 52, a: 255 }));
    // Second line of the first paragraph.
    hl.quads = vec![RectF::new(54.0, 671.0, 470.0, 686.0)];
    tab.doc.add_annotation(0, &hl).unwrap();
    let mut note = NewAnnotation::new(AnnotKind::Note, style(Rgba { r: 255, g: 200, b: 40, a: 255 }));
    note.rect = RectF::new(548.0, 676.0, 568.0, 696.0);
    note.contents = "Bitte mit den Zahlen aus dem Controlling abgleichen.".into();
    tab.doc.add_annotation(0, &note).unwrap();
    let mut circle = NewAnnotation::new(AnnotKind::Circle, style(Rgba::RED));
    circle.rect = RectF::new(418.0, 166.0, 548.0, 420.0);
    circle.contents = "Rekordwert!".into();
    tab.doc.add_annotation(0, &circle).unwrap();
    let mut text = NewAnnotation::new(AnnotKind::FreeText, style(Rgba::RED));
    text.rect = RectF::new(380.0, 430.0, 548.0, 456.0);
    text.contents = "Rekordwert – Ziel übertroffen".into();
    text.style.font_size = 11.0;
    tab.doc.add_annotation(0, &text).unwrap();
    let mut stamp = NewAnnotation::new(AnnotKind::Stamp, style(Rgba { r: 22, g: 140, b: 60, a: 255 }));
    stamp.rect = RectF::new(400.0, 40.0, 548.0, 82.0);
    stamp.label = "Genehmigt".into();
    stamp.icon = Some("Approved".into());
    tab.doc.add_annotation(0, &stamp).unwrap();
    let mut arrow = NewAnnotation::new(AnnotKind::Line, style(Rgba::BLUE));
    arrow.paths = vec![vec![PointF::new(300.0, 120.0), PointF::new(396.0, 62.0)]];
    arrow.arrow_end = true;
    tab.doc.add_annotation(0, &arrow).unwrap();
    tab.after_edit();
    h.run_steps(8);
}

fn sample(dir: &Path) -> PathBuf {
    let path = dir.join("Jahresbericht 2026.pdf");
    std::fs::write(&path, report_pdf()).unwrap();
    path
}

#[test]
#[ignore = "screenshots for the web site, run by hand"]
fn website_screenshots() {
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let report = sample(dir.path());

    // Light: the document with comments, ribbon "Anmerkungen".
    let mut h = harness(vec![report.clone()], false, Language::German);
    h.run_steps(16);
    annotate(&mut h);
    fit_page(&mut h);
    tab(&mut h, "Anmerkungen");
    save(&mut h, "comments");
    tab(&mut h, "Start");
    save(&mut h, "hero");
    // Page organisation: watermark dialog with its live preview.
    tab(&mut h, "Dokumentaufbau");
    quick(&mut h, "Wasserzeichen");
    save(&mut h, "organize");
    h.key_press(egui::Key::Escape);
    h.run_steps(4);
    // Digital signature.
    tab(&mut h, "Schützen");
    run(&mut h, Cmd::SignDocument);
    save(&mut h, "sign");
    drop(h);

    // Dark mode, English interface.
    let mut h = harness(vec![report.clone()], true, Language::English);
    h.run_steps(16);
    annotate(&mut h);
    fit_page(&mut h);
    tab(&mut h, "View");
    save(&mut h, "dark-english");
    drop(h);

    // Forms.
    let form = dir.path().join("Anmeldung Sommerfest.pdf");
    std::fs::write(&form, form_page_pdf()).unwrap();
    let mut h = harness(vec![form], false, Language::German);
    h.run_steps(16);
    add_form_fields(&mut h);
    fit_page(&mut h);
    tab(&mut h, "Formular");
    save(&mut h, "forms");
}
