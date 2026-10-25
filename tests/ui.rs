//! End-to-end tests of the user interface (headless, rendered with wgpu).
//!
//! Screenshots of every step are written to `target/ui-screenshots/` so the
//! look of the application can be reviewed without a display.

mod common;

use std::path::PathBuf;

use egui::{Event, Modifiers, PointerButton, Pos2, pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use mrpdf::model::AnnotKind;
use mrpdf::ui::MrPdfApp;
use mrpdf::ui::i18n::Language;
use mrpdf::ui::settings::Settings;

fn screenshot_dir() -> PathBuf {
    let dir = std::env::var_os("MRPDF_SCREENSHOT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/ui-screenshots"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn save(harness: &mut Harness<'_, MrPdfApp>, name: &str) {
    match harness.render() {
        Ok(img) => img.save(screenshot_dir().join(format!("{name}.png"))).unwrap(),
        Err(err) => eprintln!("rendering {name} failed: {err}"),
    }
}

fn harness(files: Vec<PathBuf>, dark: bool) -> Harness<'static, MrPdfApp> {
    let settings = Settings {
        language: Language::German,
        author: "Tester".into(),
        dark_mode: dark,
        ..Settings::default()
    };
    Harness::builder()
        .with_size(egui::vec2(1280.0, 820.0))
        .with_max_steps(64)
        .wgpu()
        .build_eframe(move |cc| MrPdfApp::with_settings(&cc.egui_ctx, settings, files))
}

fn sample_file(dir: &tempfile::TempDir, pages: usize) -> PathBuf {
    let path = dir.path().join("Beispiel.pdf");
    std::fs::write(&path, common::text_pdf(pages)).unwrap();
    path
}

fn drag(harness: &mut Harness<'_, MrPdfApp>, from: Pos2, to: Pos2) {
    let input = |h: &mut Harness<'_, MrPdfApp>, e: Event| h.input_mut().events.push(e);
    input(harness, Event::PointerMoved(from));
    harness.step();
    input(
        harness,
        Event::PointerButton {
            pos: from,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        },
    );
    harness.step();
    for i in 1..=8 {
        let t = i as f32 / 8.0;
        input(harness, Event::PointerMoved(from + (to - from) * t));
        harness.step();
    }
    input(
        harness,
        Event::PointerButton {
            pos: to,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        },
    );
    harness.run_steps(4);
}

fn click(harness: &mut Harness<'_, MrPdfApp>, at: Pos2) {
    let input = |h: &mut Harness<'_, MrPdfApp>, e: Event| h.input_mut().events.push(e);
    input(harness, Event::PointerMoved(at));
    harness.step();
    input(
        harness,
        Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        },
    );
    harness.step();
    input(
        harness,
        Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        },
    );
    harness.run_steps(4);
}

#[test]
fn start_page() {
    let _guard = common::lock();
    let mut h = harness(Vec::new(), false);
    h.run_steps(5);
    assert_eq!(h.state().tab_count(), 0);
    h.get_by_label("Neues Dokument").click();
    h.run_steps(5);
    assert_eq!(h.state().tab_count(), 1);
    save(&mut h, "01-new-document");
}

#[test]
fn open_view_and_annotate() {
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let file = sample_file(&dir, 3);
    let mut h = harness(vec![file], false);
    h.run_steps(10);
    assert_eq!(h.state().tab_count(), 1);
    save(&mut h, "02-document-open");

    // Comment ribbon and rectangle tool.
    h.get_by_label("Kommentar").click();
    h.run_steps(3);
    save(&mut h, "03-comment-ribbon");
    h.get_by_label("Rechteck").click();
    h.run_steps(3);
    assert_eq!(h.state().tool(), mrpdf::ui::tools::Tool::Rectangle);
    drag(&mut h, pos2(560.0, 360.0), pos2(720.0, 460.0));
    h.run_steps(5);
    let tab = h.state_mut().active_tab_mut().unwrap();
    let annots = tab.doc.annotations(tab.current_page).unwrap();
    assert_eq!(annots.len(), 1, "rectangle should be created");
    assert_eq!(annots[0].kind, AnnotKind::Square);
    save(&mut h, "04-rectangle");

    // A note via the text dialog.
    h.get_by_label("Notiz").click();
    h.run_steps(3);
    click(&mut h, pos2(640.0, 520.0));
    assert!(h.state().dialog().is_some(), "note dialog expected");
    h.input_mut().events.push(Event::Text("Bitte prüfen".into()));
    h.run_steps(3);
    save(&mut h, "05-note-dialog");
    h.key_press(egui::Key::Enter);
    h.run_steps(5);
    assert!(h.state().dialog().is_none());
    let tab = h.state_mut().active_tab_mut().unwrap();
    let page = tab.current_page;
    let annots = tab.doc.annotations(page).unwrap();
    assert!(annots.iter().any(|a| a.kind == AnnotKind::Note && a.contents == "Bitte prüfen"), "{annots:?}");
    save(&mut h, "06-note-created");

    // Undo removes the note again.
    h.key_press_modifiers(Modifiers::COMMAND, egui::Key::Z);
    h.run_steps(4);
    let tab = h.state_mut().active_tab_mut().unwrap();
    assert_eq!(tab.doc.annotations(page).unwrap().len(), 1);
}

#[test]
fn search_and_sidebar_panels() {
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let file = sample_file(&dir, 4);
    let mut h = harness(vec![file], false);
    h.run_steps(10);
    h.key_press_modifiers(Modifiers::COMMAND, egui::Key::F);
    h.run_steps(3);
    h.input_mut().events.push(Event::Text("Hello".into()));
    h.run_steps(2);
    h.key_press(egui::Key::Enter);
    h.run_steps(20);
    let hits = h.state().active_tab().unwrap().search.hits.len();
    assert_eq!(hits, 4, "one hit per page expected");
    save(&mut h, "07-search");

    h.get_by_label("Organisieren").click();
    h.run_steps(3);
    save(&mut h, "08-organize-ribbon");
    h.get_by_label("Rechts drehen").click();
    h.run_steps(6);
    let tab = h.state().active_tab().unwrap();
    let page = tab.current_page;
    assert_eq!(tab.doc.geometry(page).unwrap().rotation, mrpdf::geometry::Rotation::R90);
    save(&mut h, "09-rotated");
}

#[test]
fn dark_mode_and_view_ribbon() {
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let file = sample_file(&dir, 2);
    let mut h = harness(vec![file], true);
    h.run_steps(10);
    h.get_by_label("Ansicht").click();
    h.run_steps(3);
    h.get_by_label("Doppelseite").click();
    h.run_steps(8);
    save(&mut h, "10-dark-two-pages");
}

#[test]
fn form_filling() {
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Formular.pdf");
    std::fs::write(&path, common::form_pdf()).unwrap();
    let mut h = harness(vec![path], false);
    h.run_steps(10);
    save(&mut h, "11-form");
    let tab = h.state().active_tab().unwrap();
    assert_eq!(tab.doc.form_fields(0).unwrap().len(), 2);

    // The layout is deterministic: the text field is drawn at about (515, 347).
    click(&mut h, pos2(515.0, 347.0));
    h.run_steps(3);
    assert!(h.state().active_tab().unwrap().field_edit.is_some(), "field editor expected");
    h.input_mut().events.push(Event::Text("Erika Musterfrau".into()));
    h.run_steps(2);
    save(&mut h, "12-form-editing");
    h.key_press(egui::Key::Enter);
    h.run_steps(5);
    click(&mut h, pos2(394.0, 417.0));
    h.run_steps(5);
    let tab = h.state().active_tab().unwrap();
    let fields = tab.doc.form_fields(0).unwrap();
    assert_eq!(fields[0].value, "Erika Musterfrau");
    assert!(fields[1].checked, "check box should be ticked");
    save(&mut h, "13-form-filled");
}

#[test]
fn delete_key_removes_selected_annotation() {
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let file = sample_file(&dir, 1);
    let mut h = harness(vec![file], false);
    h.run_steps(10);
    h.get_by_label("Kommentar").click();
    h.run_steps(2);
    h.get_by_label("Ellipse").click();
    h.run_steps(2);
    drag(&mut h, pos2(500.0, 300.0), pos2(650.0, 420.0));
    h.run_steps(4);
    let count = |h: &mut Harness<'_, MrPdfApp>| {
        let tab = h.state_mut().active_tab_mut().unwrap();
        tab.doc.annotations(0).unwrap().len()
    };
    assert_eq!(count(&mut h), 1);
    assert!(h.state().active_tab().unwrap().selected_annot.is_some());
    h.key_press(egui::Key::Delete);
    h.run_steps(4);
    assert_eq!(count(&mut h), 0);
}

#[test]
fn protect_ribbon_and_attachments() {
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let file = sample_file(&dir, 1);
    let mut h = harness(vec![file], false);
    h.run_steps(10);
    h.get_by_label("Schützen").click();
    h.run_steps(3);
    save(&mut h, "14-protect-ribbon");
    h.get_by_label("Kennwortschutz").click();
    h.run_steps(3);
    assert!(h.state().dialog().is_some());
    save(&mut h, "15-protect-dialog");
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(h.state().dialog().is_none());
    h.get_all_by_label("Anhänge").next().unwrap().click();
    h.run_steps(3);
    save(&mut h, "16-attachments");
}
