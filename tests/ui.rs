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
    // "Drehen ▾" opens a drop-down menu.
    h.get_by_label("Drehen").click();
    h.run_steps(3);
    save(&mut h, "08b-rotate-menu");
    h.get_by_label("90° im Uhrzeigersinn").click();
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

#[test]
fn document_structure_ui() {
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let file = sample_file(&dir, 3);
    let mut h = harness(vec![file], false);
    h.run_steps(10);
    tab(&mut h, "Dokumentaufbau");
    save(&mut h, "19-document-ribbon");

    // Watermark with the default text through its dialog.
    quick(&mut h, "Wasserzeichen");
    assert!(h.state().dialog().is_some());
    save(&mut h, "20-watermark-dialog");
    h.get_by_label("OK").click();
    h.run_steps(6);
    {
        let tab = h.state_mut().active_tab_mut().unwrap();
        assert!(tab.doc.page_text(0).unwrap().all_text().contains("ENTWURF"));
    }
    save(&mut h, "21-watermark");

    // Header/footer with page numbers.
    quick(&mut h, "Kopf- und Fußzeilen");
    h.get_by_label("OK").click();
    h.run_steps(6);
    {
        let tab = h.state_mut().active_tab_mut().unwrap();
        assert!(tab.doc.page_text(2).unwrap().all_text().contains("Seite 3 von 3"));
    }

    // Reverse page order (dialog with page range, all pages preselected).
    quick(&mut h, "Seitenreihenfolge umkehren");
    h.get_all_by_label("OK").last().unwrap().click();
    h.run_steps(6);
    let tab = h.state_mut().active_tab_mut().unwrap();
    assert!(tab.doc.page_text(0).unwrap().all_text().contains("Page 3"));
}

#[test]
fn form_designer_and_comment_commands() {
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let file = sample_file(&dir, 1);
    let mut h = harness(vec![file], false);
    h.run_steps(10);
    let at = |h: &Harness<'_, MrPdfApp>, x: f32, y: f32| {
        h.state().active_tab().unwrap().screen_pos(0, mrpdf::geometry::PointF::new(x, y)).unwrap()
    };
    let fields = |h: &mut Harness<'_, MrPdfApp>| {
        let tab = h.state_mut().active_tab_mut().unwrap();
        tab.doc.form_fields(0).unwrap().into_iter().map(|f| f.kind).collect::<Vec<_>>()
    };
    tab(&mut h, "Formular");

    // Check box: drawn directly with an automatic name.
    h.get_all_by_label("Auswahlfeld (Häkchen)").next().unwrap().click();
    h.run_steps(3);
    let (a, b) = (at(&h, 100.0, 700.0), at(&h, 116.0, 684.0));
    drag(&mut h, a, b);
    h.run_steps(4);
    assert_eq!(fields(&mut h), vec![mrpdf::model::FieldKind::CheckBox]);

    // Combo box asks for its items first.
    h.get_all_by_label("Menü").next().unwrap().click();
    h.run_steps(3);
    let (a, b) = (at(&h, 100.0, 650.0), at(&h, 250.0, 630.0));
    drag(&mut h, a, b);
    h.run_steps(4);
    assert!(h.state().dialog().is_some(), "options dialog");
    save(&mut h, "22-new-field-dialog");
    h.get_by_label("OK").click();
    h.run_steps(6);
    assert!(fields(&mut h).contains(&mrpdf::model::FieldKind::ComboBox));

    // Text field, then the field selection tool selects it.
    h.get_all_by_label("Text").next().unwrap().click();
    h.run_steps(3);
    let (a, b) = (at(&h, 100.0, 600.0), at(&h, 300.0, 580.0));
    drag(&mut h, a, b);
    h.run_steps(4);
    assert_eq!(fields(&mut h).len(), 3);
    h.get_all_by_label("Felder auswählen").next().unwrap().click();
    h.run_steps(3);
    let p = at(&h, 200.0, 590.0);
    click(&mut h, p);
    assert!(h.state().active_tab().unwrap().selected_annot.is_some(), "field selected");
    save(&mut h, "23-form-designer");

    // Comment summary opens a new tab; deleting all comments asks first.
    {
        let tab = h.state_mut().active_tab_mut().unwrap();
        let mut note = mrpdf::pdf_edit::NewAnnotation::new(AnnotKind::Note, Default::default());
        note.rect = mrpdf::geometry::RectF::new(400.0, 700.0, 420.0, 720.0);
        note.contents = "Notiz".into();
        tab.doc.add_annotation(0, &note).unwrap();
        tab.after_edit();
    }
    quick(&mut h, "Anmerkungen zusammenfassen");
    assert_eq!(h.state().tab_count(), 2);
    {
        let tab = h.state_mut().active_tab_mut().unwrap();
        assert!(tab.doc.page_text(0).unwrap().all_text().contains("Notiz"));
    }
    save(&mut h, "24-comment-summary");
    h.state_mut().set_active_tab(0);
    h.run_steps(3);
    quick(&mut h, "Anmerkungen löschen");
    assert!(h.state().dialog().is_some(), "confirmation");
    h.get_by_label("OK").click();
    h.run_steps(4);
    assert!(!kinds(&mut h).contains(&AnnotKind::Note));
    assert_eq!(fields(&mut h).len(), 3, "form fields are kept");
}

#[test]
fn search_and_redact() {
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let file = sample_file(&dir, 1);
    let mut h = harness(vec![file], false);
    h.run_steps(10);
    tab(&mut h, "Schützen");
    quick(&mut h, "Suchen und schwärzen");
    assert!(h.state().dialog().is_some());
    h.input_mut().events.push(Event::Text("Hello".into()));
    h.run_steps(2);
    h.key_press(egui::Key::Enter);
    h.run_steps(4);
    assert_eq!(kinds(&mut h), vec![AnnotKind::Redact]);
    save(&mut h, "25-redaction-marked");
    quick(&mut h, "Übernehmen");
    assert!(h.state().dialog().is_some(), "confirmation");
    h.get_by_label("OK").click();
    h.run_steps(6);
    assert!(kinds(&mut h).is_empty());
    let tab = h.state_mut().active_tab_mut().unwrap();
    assert!(!tab.doc.page_text(0).unwrap().all_text().contains("Hello"));
    save(&mut h, "26-redacted");
}

#[test]
fn bookmark_tools_ui() {
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let file = sample_file(&dir, 3);
    let mut h = harness(vec![file], false);
    h.run_steps(10);
    tab(&mut h, "Lesezeichen");
    quick(&mut h, "Aus Seitentext");
    assert_eq!(h.state().active_tab().unwrap().doc.bookmarks().len(), 3);
    quick(&mut h, "Inhaltsverzeichnis erstellen");
    h.run_steps(6);
    let tab = h.state().active_tab().unwrap();
    assert_eq!(tab.doc.page_count(), 4);
    save(&mut h, "27-table-of-contents");
}

#[test]
fn review_tools_ui() {
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let file = sample_file(&dir, 2);
    let mut h = harness(vec![file], false);
    h.run_steps(10);
    tab(&mut h, "Überprüfen");
    quick(&mut h, "Wörter zählen");
    match h.state().dialog() {
        Some(mrpdf::ui::app::Dialog::Message { text, .. }) => assert!(text.contains("Wörter: 18"), "{text}"),
        other => panic!("unexpected dialog {other:?}"),
    }
    save(&mut h, "28-word-count");
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    quick(&mut h, "Barrierefreiheit prüfen");
    assert_eq!(h.state().tab_count(), 2);
    let report = h.state().active_tab().unwrap().doc.page_text(0).unwrap().all_text();
    assert!(report.contains("Prüfungen bestanden"), "{report}");
    save(&mut h, "29-accessibility-report");
    // Colours: confirmation, then gray.
    h.state_mut().set_active_tab(0);
    h.run_steps(3);
    quick(&mut h, "Farben umwandeln");
    h.get_by_label("OK").click();
    h.run_steps(4);
    assert!(h.state().active_tab().unwrap().doc.is_modified());
}

#[test]
fn organize_ribbon_menus() {
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let file = sample_file(&dir, 2);
    let mut h = harness(vec![file], false);
    h.run_steps(10);
    tab(&mut h, "Dokumentaufbau");
    h.get_by_label("Einfügen").click();
    h.run_steps(3);
    // The insert menu lists the entries; scanning is offered but disabled.
    for label in [
        "Seiten einfügen…",
        "Leere Seiten einfügen…",
        "Bilder einfügen…",
        "Gescannte Seiten einfügen…",
        "RTF einfügen…",
    ] {
        assert!(h.query_by_label(label).is_some(), "{label} missing");
    }
    save(&mut h, "30-insert-menu");
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    // The page mark group lies beyond the visible ribbon width.
    h.get_by_label("Wasserzeichen").scroll_to_me();
    h.run_steps(3);
    h.get_by_label("Wasserzeichen").click();
    h.run_steps(3);
    assert!(h.query_by_label("Verwalten…").is_some());
    save(&mut h, "31-watermark-menu");
}

#[test]
fn page_dialogs_ui() {
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let file = sample_file(&dir, 3);
    let mut h = harness(vec![file], false);
    h.run_steps(10);
    let count = |h: &Harness<'_, MrPdfApp>| h.state().active_tab().unwrap().doc.page_count();
    let ok = |h: &mut Harness<'_, MrPdfApp>| {
        h.get_all_by_label("OK").last().unwrap().click();
        h.run_steps(6);
    };

    // Blank pages: dialog, two pages after the current one.
    quick(&mut h, "Leere Seiten einfügen");
    assert!(h.state().dialog().is_some());
    save(&mut h, "32-insert-blank-dialog");
    ok(&mut h);
    assert_eq!(count(&h), 4);

    // Duplicate the current page once (after the last page).
    quick(&mut h, "Seiten duplizieren");
    save(&mut h, "33-duplicate-dialog");
    ok(&mut h);
    assert_eq!(count(&h), 5);

    // Rotate all pages clockwise.
    quick(&mut h, "Seiten drehen");
    save(&mut h, "34-rotate-dialog");
    ok(&mut h);
    let rotations: Vec<_> =
        (0..5).map(|p| h.state().active_tab().unwrap().doc.geometry(p).unwrap().rotation).collect();
    assert!(rotations.iter().all(|r| *r == mrpdf::geometry::Rotation::R90));

    // Normalize removes the rotation again without visual change.
    quick(&mut h, "Seiten normalisieren");
    save(&mut h, "35-normalize-dialog");
    ok(&mut h);
    assert_eq!(
        h.state().active_tab().unwrap().doc.geometry(0).unwrap().rotation,
        mrpdf::geometry::Rotation::R0
    );

    // Delete blank pages (the inserted one) and show the duplicates.
    quick(&mut h, "Leere Seiten löschen");
    save(&mut h, "36-delete-blank-dialog");
    ok(&mut h);
    assert_eq!(count(&h), 4, "the blank page is gone");
    quick(&mut h, "Duplikate finden");
    ok(&mut h);
    assert!(matches!(h.state().dialog(), Some(mrpdf::ui::app::Dialog::Duplicates { .. })));
    h.key_press(egui::Key::Escape);
    h.run_steps(3);

    // Delete the current page.
    quick(&mut h, "Seiten löschen");
    save(&mut h, "37-delete-dialog");
    ok(&mut h);
    assert_eq!(count(&h), 3);

    // Replace dialog with two previews (no source chosen yet).
    quick(&mut h, "Seiten ersetzen");
    save(&mut h, "38-replace-dialog");
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    quick(&mut h, "Seiten extrahieren");
    save(&mut h, "39-extract-dialog");
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(h.state().dialog().is_none());

    // Image insertion with its settings pages.
    quick(&mut h, "Bilder einfügen");
    save(&mut h, "40-images-dialog");
    h.get_by_label("Einstellungen").click();
    h.run_steps(3);
    h.get_by_label("Bildposition").click();
    h.run_steps(3);
    save(&mut h, "41-images-settings");
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(h.state().dialog().is_none());
}

#[test]
fn combine_split_overlay_ui() {
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let file = sample_file(&dir, 7);
    let mut h = harness(vec![file], false);
    h.run_steps(10);
    let ok = |h: &mut Harness<'_, MrPdfApp>| {
        h.get_all_by_label("OK").last().unwrap().click();
        h.run_steps(8);
    };

    // N-up: 7 pages, 6 per sheet -> new document with 2 sheets.
    quick(&mut h, "Seiten zusammenführen");
    h.run_steps(4);
    save(&mut h, "42-nup-dialog");
    ok(&mut h);
    assert_eq!(h.state().tab_count(), 2);
    assert_eq!(h.state().active_tab().unwrap().doc.page_count(), 2);
    save(&mut h, "43-nup-result");
    h.state_mut().set_active_tab(0);
    h.run_steps(3);

    // Split every page in the middle.
    quick(&mut h, "Seiten aufteilen");
    h.get_by_label("Senkrecht teilen").click();
    h.run_steps(4);
    save(&mut h, "44-split-pages-dialog");
    ok(&mut h);
    assert_eq!(h.state().active_tab().unwrap().doc.page_count(), 14);
    assert!(h.state_mut().active_tab_mut().unwrap().doc.undo().unwrap());

    // Split the document into files of three pages.
    quick(&mut h, "Dokument aufteilen");
    save(&mut h, "45-split-document-dialog");
    let before = std::fs::read_dir(dir.path()).unwrap().count();
    h.get_by_label("Öffne Zielordner und wähle Dateien aus").click();
    h.run_steps(2);
    ok(&mut h);
    let after = std::fs::read_dir(dir.path()).unwrap().count();
    assert_eq!(after - before, 7, "one file per page by default");

    // Overlay dialog with preview.
    quick(&mut h, "Überlagern");
    h.run_steps(3);
    save(&mut h, "46-overlay-dialog");
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
}

#[test]
fn page_shape_ui() {
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let file = sample_file(&dir, 2);
    let mut h = harness(vec![file], false);
    h.run_steps(10);
    let size =
        |h: &Harness<'_, MrPdfApp>| h.state().active_tab().unwrap().doc.geometry(0).unwrap().view_size();
    let ok = |h: &mut Harness<'_, MrPdfApp>| {
        h.get_all_by_label("OK").last().unwrap().click();
        h.run_steps(8);
    };

    // Crop dialog: white margins of the current page via the button.
    quick(&mut h, "Seiten zuschneiden");
    h.get_by_label("Auf weiße Ränder setzen").click();
    h.run_steps(4);
    save(&mut h, "47-crop-dialog");
    ok(&mut h);
    assert!(size(&h).0 < 500.0, "cropped: {:?}", size(&h));
    assert!(h.state_mut().active_tab_mut().unwrap().doc.undo().unwrap());

    // Trim white margins from the menu (all pages).
    quick(&mut h, "Weiße Ränder abschneiden");
    assert!(size(&h).0 < 500.0);
    assert!(h.state_mut().active_tab_mut().unwrap().doc.undo().unwrap());

    // Resize: A5 by default? Standard A4 -> choose scaling 50 %.
    quick(&mut h, "Größe ändern");
    h.get_by_label("Skalierung").click();
    h.run_steps(3);
    save(&mut h, "48-resize-dialog");
    ok(&mut h);
    // Scaling defaults to 100 %: size stays.
    assert!((size(&h).0 - 595.3).abs() < 1.0);

    // Margins: dialog with mirror option.
    quick(&mut h, "Ränder");
    save(&mut h, "49-margins-dialog");
    h.key_press(egui::Key::Escape);
    h.run_steps(3);

    // Crop tool: drag a rectangle on page 1.
    quick(&mut h, "Seite zuschneiden");
    assert_eq!(h.state().tool(), mrpdf::ui::tools::Tool::Crop);
    let at = |h: &Harness<'_, MrPdfApp>, x: f32, y: f32| {
        h.state().active_tab().unwrap().screen_pos(0, mrpdf::geometry::PointF::new(x, y)).unwrap()
    };
    let (a, b) = (at(&h, 60.0, 760.0), at(&h, 400.0, 560.0));
    drag(&mut h, a, b);
    h.run_steps(6);
    let (w, hh) = size(&h);
    assert!((w - 340.0).abs() < 3.0 && (hh - 200.0).abs() < 3.0, "{w}x{hh}");
    save(&mut h, "50-crop-tool");
}

#[test]
fn page_marks_ui() {
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let file = sample_file(&dir, 2);
    let mut h = harness(vec![file], false);
    h.run_steps(10);
    let text = |h: &mut Harness<'_, MrPdfApp>, p: usize| {
        h.state_mut().active_tab_mut().unwrap().doc.page_text(p).unwrap().all_text()
    };
    let ok = |h: &mut Harness<'_, MrPdfApp>| {
        h.get_all_by_label("OK").last().unwrap().click();
        h.run_steps(8);
    };

    quick(&mut h, "Wasserzeichen");
    h.run_steps(4);
    save(&mut h, "51-watermark-dialog");
    ok(&mut h);
    assert!(text(&mut h, 1).contains("ENTWURF"));

    quick(&mut h, "Kopf- und Fußzeilen");
    h.run_steps(4);
    save(&mut h, "52-header-footer-dialog");
    ok(&mut h);
    assert!(text(&mut h, 1).contains("Seite 2 von 2"));

    quick(&mut h, "Paginierung");
    h.run_steps(4);
    save(&mut h, "53-bates-dialog");
    ok(&mut h);
    assert!(text(&mut h, 1).contains("000002"));

    // The manager lists the watermark; "Alle entfernen" asks first.
    quick(&mut h, "Wasserzeichen verwalten");
    h.run_steps(4);
    save(&mut h, "54-watermark-manager");
    h.get_all_by_label("Schließen").last().unwrap().click();
    h.run_steps(3);
    quick(&mut h, "Alle Wasserzeichen entfernen");
    assert!(h.state().dialog().is_some(), "confirmation");
    ok(&mut h);
    assert!(!text(&mut h, 1).contains("ENTWURF"));
    assert!(text(&mut h, 1).contains("Seite 2 von 2"), "other marks stay");

    quick(&mut h, "Hintergrund");
    h.run_steps(4);
    save(&mut h, "55-background-dialog");
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    quick(&mut h, "Paginierung zu mehreren Dateien");
    h.run_steps(3);
    save(&mut h, "56-bates-files-dialog");
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
}

#[test]
fn page_labels_ui() {
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let file = sample_file(&dir, 12);
    let mut h = harness(vec![file], false);
    h.run_steps(10);
    quick(&mut h, "Seiten nummerieren");
    h.run_steps(3);
    // A second range from page 4 (via "Neuer Bereich…" between page 1 and the end).
    h.get_by_label("Neuer Bereich…").click();
    h.run_steps(3);
    save(&mut h, "57-page-labels-dialog");
    h.get_all_by_label("OK").last().unwrap().click();
    h.run_steps(6);
    let tab = h.state_mut().active_tab_mut().unwrap();
    let ranges = tab.doc.page_label_ranges().unwrap();
    assert_eq!(ranges.len(), 2, "{ranges:?}");
    assert_eq!(ranges[1].start, 6);
    assert_eq!(tab.doc.page_label(7).as_deref(), Some("2"));
}

#[test]
fn other_tools_menu() {
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let file = sample_file(&dir, 1);
    let mut h = harness(vec![file], false);
    h.run_steps(10);
    tab(&mut h, "Dokumentaufbau");
    h.get_all_by_label("Andere Werkzeuge").next().unwrap().click();
    h.run_steps(3);
    for label in [
        "Tatsächliche Größe",
        "An Seitengröße anpassen",
        "An Breite anpassen",
        "An Inhalt anpassen",
        "Vergrößerung",
    ] {
        assert!(h.query_by_label(label).is_some(), "{label} missing");
    }
    save(&mut h, "58-other-tools-menu");
    let before = h.state().active_tab().unwrap().zoom;
    h.get_by_label("An Inhalt anpassen").click();
    h.run_steps(6);
    let after = h.state().active_tab().unwrap().zoom;
    assert!(after > before * 1.3, "zoomed to the printed area: {before} -> {after}");
    // Strg+1 returns to 100 %.
    h.key_press_modifiers(Modifiers::COMMAND, egui::Key::Num1);
    h.run_steps(3);
    assert!((h.state().active_tab().unwrap().zoom - 1.0).abs() < 1e-3);
}

fn open_backstage(h: &mut Harness<'_, MrPdfApp>) {
    h.get_by_label("Datei").click();
    h.run_steps(4);
    assert!(h.state().backstage_open());
}

#[test]
fn file_backstage() {
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let file = sample_file(&dir, 2);
    let mut h = harness(vec![file], false);
    h.run_steps(10);
    open_backstage(&mut h);
    // Left column; later steps are visible but disabled.
    for label in [
        "Speichern",
        "Optimiert speichern",
        "Drucken",
        "Alle drucken",
        "Exportieren",
        "Teilen",
        "Dokumenteigenschaften",
        "Alle schließen",
        "Einstellungen verwalten",
        "Beenden",
    ] {
        assert!(h.query_all_by_label(label).next().is_some(), "{label} missing");
    }
    // "Öffnen" shows the recent files; the opened sample is listed.
    assert!(h.query_all_by_label("Letzte Dateien").next().is_some());
    assert!(h.query_all_by_label("Beispiel").next().is_some());
    save(&mut h, "59-backstage-open");
    // Entries can be removed from the list.
    h.get_by_label("Aus der Liste entfernen").click();
    h.run_steps(3);
    assert!(h.query_all_by_label("Beispiel").next().is_none());
    // Disabled entries do nothing.
    h.get_by_label("Optimiert speichern").click();
    h.run_steps(3);
    assert!(h.state().backstage_open());
    // Sub-page lists.
    h.get_by_label("Exportieren").click();
    h.run_steps(3);
    assert!(h.query_by_label("In Microsoft-Word-Dokument").is_some());
    save(&mut h, "60-backstage-export");
    h.get_all_by_label("Neues Dokument").next().unwrap().click();
    h.run_steps(3);
    assert!(h.query_by_label("Aus Markdown").is_some());
    save(&mut h, "61-backstage-new");
    h.get_by_label("Mit leeren Seiten").click();
    h.run_steps(5);
    assert_eq!(h.state().tab_count(), 2);
    assert!(!h.state().backstage_open(), "a new document leaves the backstage");
    // Esc and a ribbon tab leave the backstage.
    open_backstage(&mut h);
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(!h.state().backstage_open());
    open_backstage(&mut h);
    tab(&mut h, "Ansicht");
    assert!(!h.state().backstage_open());
    // Strg+K opens the settings, Strg+D the document properties.
    h.key_press_modifiers(Modifiers::COMMAND, egui::Key::K);
    h.run_steps(3);
    assert!(matches!(h.state().dialog(), Some(mrpdf::ui::app::Dialog::Settings { .. })));
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    h.key_press_modifiers(Modifiers::COMMAND, egui::Key::D);
    h.run_steps(3);
    assert!(matches!(h.state().dialog(), Some(mrpdf::ui::app::Dialog::Properties { .. })));
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    // "Alle schließen" asks for the changed document, "Nein" closes all.
    h.state_mut().set_active_tab(0);
    h.state_mut().active_tab_mut().unwrap().doc.rotate_pages(&[0], 90).unwrap();
    open_backstage(&mut h);
    h.get_by_label("Alle schließen").click();
    h.run_steps(4);
    assert!(matches!(h.state().dialog(), Some(mrpdf::ui::app::Dialog::Unsaved { .. })));
    save(&mut h, "62-close-all-question");
    h.get_by_label("›  Abbrechen").click();
    h.run_steps(3);
    assert_eq!(h.state().tab_count(), 2, "cancel keeps the documents");
    open_backstage(&mut h);
    h.get_by_label("Alle schließen").click();
    h.run_steps(4);
    h.get_by_label("›  Nein").click();
    h.run_steps(5);
    // The new blank document is unsaved too: second question.
    assert_eq!(h.state().tab_count(), 1);
    assert!(matches!(h.state().dialog(), Some(mrpdf::ui::app::Dialog::Unsaved { .. })));
    h.get_by_label("›  Nein").click();
    h.run_steps(5);
    assert_eq!(h.state().tab_count(), 0);
    assert!(h.state().dialog().is_none());
}
