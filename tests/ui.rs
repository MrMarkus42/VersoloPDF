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

/// Clicks a ribbon tab (tabs come before groups with the same caption).
fn tab(harness: &mut Harness<'_, MrPdfApp>, label: &str) {
    harness.get_all_by_label(label).next().unwrap().click();
    harness.run_steps(3);
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
    tab(&mut h, "Anmerkungen");
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
    h.get_by_label("Kommentar").click();
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

    tab(&mut h, "Dokumentaufbau");
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
    tab(&mut h, "Ansicht");
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

    let at = |h: &Harness<'_, MrPdfApp>, x: f32, y: f32| {
        h.state().active_tab().unwrap().screen_pos(0, mrpdf::geometry::PointF::new(x, y)).unwrap()
    };
    let field = at(&h, 200.0, 710.0);
    click(&mut h, field);
    h.run_steps(3);
    assert!(h.state().active_tab().unwrap().field_edit.is_some(), "field editor expected");
    h.input_mut().events.push(Event::Text("Erika Musterfrau".into()));
    h.run_steps(2);
    save(&mut h, "12-form-editing");
    h.key_press(egui::Key::Enter);
    h.run_steps(5);
    let check = at(&h, 106.0, 656.0);
    click(&mut h, check);
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
    tab(&mut h, "Anmerkungen");
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
    tab(&mut h, "Schützen");
    h.run_steps(3);
    save(&mut h, "14-protect-ribbon");
    h.get_by_label("Sicherheitseinstellungen").click();
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

/// Manual check with a real document: `MRPDF_SAMPLE=/path/file.pdf cargo test --test ui -- --ignored`
#[test]
#[ignore]
fn real_world_sample() {
    let _guard = common::lock();
    let Some(path) = std::env::var_os("MRPDF_SAMPLE").map(PathBuf::from) else { return };
    let name = path.file_stem().unwrap().to_string_lossy().into_owned();
    let mut h = harness(vec![path], false);
    h.run_steps(15);
    save(&mut h, &format!("sample-{name}-1"));
    h.get_all_by_label("Lesezeichen").next().unwrap().click();
    h.run_steps(5);
    tab(&mut h, "Ansicht");
    h.run_steps(2);
    h.get_by_label("Doppelseite").click();
    h.run_steps(10);
    save(&mut h, &format!("sample-{name}-2"));

    // Fill in the first text field (if any) through the document API.
    let tab = h.state_mut().active_tab_mut().unwrap();
    let field = tab
        .doc
        .form_fields(0)
        .unwrap()
        .into_iter()
        .find(|f| matches!(f.kind, mrpdf::model::FieldKind::Text { .. }));
    if let Some(field) = field {
        tab.doc.set_text_field(0, field.annot_index, "MrPDF Test 123").unwrap();
        tab.after_edit();
        let value = tab
            .doc
            .form_fields(0)
            .unwrap()
            .into_iter()
            .find(|f| f.annot_index == field.annot_index)
            .unwrap()
            .value;
        assert_eq!(value, "MrPDF Test 123");
        h.get_by_label("Einzelseite").click();
        h.run_steps(10);
        save(&mut h, &format!("sample-{name}-3"));
    }
}

/// Selects a tool through the quick start search (works for every command).
fn quick(harness: &mut Harness<'_, MrPdfApp>, query: &str) {
    harness.key_press_modifiers(Modifiers::COMMAND, egui::Key::Period);
    harness.run_steps(3);
    harness.input_mut().events.push(Event::Text(query.into()));
    harness.run_steps(2);
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
}

fn kinds(harness: &mut Harness<'_, MrPdfApp>) -> Vec<AnnotKind> {
    let tab = harness.state_mut().active_tab_mut().unwrap();
    tab.doc.annotations(0).unwrap().iter().map(|a| a.kind).collect()
}

#[test]
fn new_comment_tools() {
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let file = sample_file(&dir, 1);
    let mut h = harness(vec![file], false);
    h.run_steps(10);
    let at = |h: &Harness<'_, MrPdfApp>, x: f32, y: f32| {
        h.state().active_tab().unwrap().screen_pos(0, mrpdf::geometry::PointF::new(x, y)).unwrap()
    };

    // Typewriter: click, type, Enter.
    quick(&mut h, "Schreibmaschine");
    assert_eq!(h.state().tool(), mrpdf::ui::tools::Tool::Typewriter);
    let p = at(&h, 100.0, 500.0);
    click(&mut h, p);
    h.input_mut().events.push(Event::Text("Getippt".into()));
    h.run_steps(2);
    h.key_press(egui::Key::Enter);
    h.run_steps(4);
    assert_eq!(kinds(&mut h).last(), Some(&AnnotKind::FreeText));

    // Cloud polygon by clicking corners, Enter finishes.
    quick(&mut h, "Wolke");
    for (x, y) in [(300.0, 700.0), (450.0, 700.0), (450.0, 600.0), (300.0, 600.0)] {
        let p = at(&h, x, y);
        click(&mut h, p);
    }
    assert_eq!(h.state().tool(), mrpdf::ui::tools::Tool::Cloud);
    assert_eq!(h.state().active_tab().unwrap().polygon.len(), 4, "vertices");
    h.key_press(egui::Key::Enter);
    h.run_steps(4);
    assert_eq!(kinds(&mut h).last(), Some(&AnnotKind::Polygon));

    // Distance measurement.
    quick(&mut h, "Entfernung");
    let (a, b) = (at(&h, 100.0, 560.0), at(&h, 280.0, 560.0));
    drag(&mut h, a, b);
    h.run_steps(3);
    {
        let tab = h.state_mut().active_tab_mut().unwrap();
        let last = tab.doc.annotations(0).unwrap().last().cloned().unwrap();
        assert_eq!(last.kind, AnnotKind::Line);
        assert!(last.contents.ends_with("mm"), "{}", last.contents);
    }

    // Callout: drag from the tip to the box position, then type.
    quick(&mut h, "Beschreibung");
    let (a, b) = (at(&h, 100.0, 780.0), at(&h, 160.0, 800.0));
    drag(&mut h, a, b);
    h.input_mut().events.push(Event::Text("Hinweis".into()));
    h.run_steps(2);
    h.key_press(egui::Key::Enter);
    h.run_steps(4);
    assert_eq!(kinds(&mut h).last(), Some(&AnnotKind::FreeText));

    // Freehand drawing, then the eraser removes it again.
    quick(&mut h, "Stift");
    let (a, b) = (at(&h, 400.0, 480.0), at(&h, 500.0, 520.0));
    drag(&mut h, a, b);
    h.run_steps(3);
    let before = kinds(&mut h).len();
    assert_eq!(kinds(&mut h).last(), Some(&AnnotKind::Ink));
    quick(&mut h, "Radiergummi");
    let (a, b) = (at(&h, 390.0, 485.0), at(&h, 510.0, 515.0));
    drag(&mut h, a, b);
    h.run_steps(3);
    assert_eq!(kinds(&mut h).len(), before - 1, "eraser should remove the ink");

    // Area highlight and caret.
    quick(&mut h, "Bereich hervorheben");
    let (a, b) = (at(&h, 60.0, 470.0), at(&h, 200.0, 455.0));
    drag(&mut h, a, b);
    h.run_steps(3);
    assert_eq!(kinds(&mut h).last(), Some(&AnnotKind::Highlight));
    quick(&mut h, "Text einfügen");
    let p = at(&h, 250.0, 530.0);
    click(&mut h, p);
    h.input_mut().events.push(Event::Text("ergänzt".into()));
    h.run_steps(2);
    h.key_press(egui::Key::Enter);
    h.run_steps(4);
    assert_eq!(kinds(&mut h).last(), Some(&AnnotKind::Caret));
    save(&mut h, "17-new-tools");

    // View rotation and history.
    quick(&mut h, "Ansicht rechts drehen");
    assert_eq!(h.state().active_tab().unwrap().view_rotation, mrpdf::geometry::Rotation::R90);
    save(&mut h, "18-view-rotated");
}
