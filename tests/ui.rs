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
        backstage_at_start: false,
        ..Settings::default()
    };
    harness_with(settings, files)
}

fn harness_with(settings: Settings, files: Vec<PathBuf>) -> Harness<'static, MrPdfApp> {
    Harness::builder().with_size(egui::vec2(1280.0, 820.0)).with_max_steps(64).wgpu().build_eframe(
        move |cc| {
            let mut app = MrPdfApp::with_settings(&cc.egui_ctx, settings, files);
            // Tests never touch the user's recovery folder.
            app.set_recovery_dir(None);
            app
        },
    )
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
    save(&mut h, "96-view-ribbon");
    h.get_by_label("Zwei Seiten fortlaufend").click();
    h.run_steps(8);
    assert_eq!(h.state().active_tab().unwrap().layout_mode, mrpdf::ui::view::PageLayout::TwoPagesContinuous);
    save(&mut h, "10-dark-two-pages");
}

#[test]
fn view_tab_features() {
    use mrpdf::ui::app::commands::Cmd;
    use mrpdf::ui::app::view_cmds::ViewCmd as V;
    use mrpdf::ui::view::PageLayout;
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let file = sample_file(&dir, 4);
    let other = dir.path().join("Anderes.pdf");
    std::fs::write(&other, common::text_pdf(1)).unwrap();
    let mut h = harness(vec![file.clone(), other.clone()], false);
    h.run_steps(12);
    h.state_mut().set_active_tab(0);
    h.run_steps(4);
    tab(&mut h, "Ansicht");
    let ctx = h.ctx.clone();
    let run = |h: &mut Harness<'_, MrPdfApp>, cmd: Cmd| {
        let ctx = h.ctx.clone();
        h.state_mut().run(&ctx, cmd);
        h.run_steps(4);
    };
    // Rulers, grid and the page size / pointer position.
    run(&mut h, Cmd::View(V::Rulers));
    run(&mut h, Cmd::View(V::Grid));
    assert!(h.state().settings().show_rulers && h.state().settings().show_grid);
    // A guide dragged out of the top ruler onto the page.
    let t = h.state().active_tab().unwrap();
    let page = t.ruler_page.unwrap().1;
    let view_top = (t.canvas_origin - t.content_shift + t.scroll_offset).y;
    let scale = t.scale();
    let x = page.center().x;
    drag(&mut h, egui::pos2(x, view_top - 9.0), egui::pos2(x, page.top() + 100.0));
    let guides = h.state().active_tab().unwrap().guides.clone();
    assert_eq!(guides.len(), 1, "guide created");
    assert!(!guides[0].vertical && (guides[0].pos - 100.0 / scale).abs() < 3.0, "{guides:?}");
    save(&mut h, "97-view-rulers-grid");
    // Two pages one spread at a time, right to left, cover alone.
    run(&mut h, Cmd::LayoutFacing);
    run(&mut h, Cmd::LayoutCover);
    run(&mut h, Cmd::View(V::RightToLeft));
    let tab = h.state().active_tab().unwrap();
    assert_eq!(tab.layout_mode, PageLayout::TwoPages);
    assert_eq!(tab.layout.slots.len(), 1, "cover page alone");
    h.state_mut().active_tab_mut().unwrap().go_to_page(1);
    h.run_steps(6);
    let slots = h.state().active_tab().unwrap().layout.slots.clone();
    assert_eq!(slots.len(), 2);
    assert_eq!(slots[0].page, 2, "right to left: page 3 on the left");
    save(&mut h, "98-view-two-pages-rtl");
    run(&mut h, Cmd::View(V::RightToLeft));
    run(&mut h, Cmd::LayoutCover);
    // Pages side by side.
    run(&mut h, Cmd::LayoutContinuous);
    run(&mut h, Cmd::View(V::SideBySide));
    assert!(h.state().active_tab().unwrap().layout.horizontal);
    save(&mut h, "99-view-side-by-side");
    run(&mut h, Cmd::View(V::StandardLayout));
    // Split view with two panes; a click activates the other pane.
    run(&mut h, Cmd::View(V::SplitVertical));
    assert_eq!(h.state().active_tab().unwrap().panes.len(), 2);
    save(&mut h, "100-view-split");
    run(&mut h, Cmd::View(V::SplitVertical));
    assert!(
        h.state().active_tab().unwrap().panes.is_empty() || h.state().active_tab().unwrap().panes.len() == 1
    );
    // Transparency grid, highlighted links, design "Schwarz".
    run(&mut h, Cmd::View(V::Transparency));
    assert!(h.state().active_tab().unwrap().transparent);
    run(&mut h, Cmd::View(V::Transparency));
    run(&mut h, Cmd::View(V::Design(mrpdf::ui::theme::Design::Black)));
    assert!(ctx.style_of(ctx.theme()).visuals.dark_mode);
    save(&mut h, "101-view-design-black");
    run(&mut h, Cmd::View(V::Design(mrpdf::ui::theme::Design::Standard)));
    // Loupe and pan & zoom windows.
    run(&mut h, Cmd::Tool(mrpdf::ui::tools::Tool::Loupe));
    run(&mut h, Cmd::View(V::PanZoom));
    let page = h.state().active_tab().unwrap().ruler_page.unwrap().1;
    h.hover_at(page.center());
    h.run_steps(6);
    assert!(h.state().active_tab().unwrap().loupe_at.is_some());
    assert!(h.query_all_by_label("Ausschnitt und Vergrößerung").count() >= 2, "window open");
    save(&mut h, "102-view-loupe-pan-zoom");
    // Rotate and reset the view.
    run(&mut h, Cmd::RotateViewRight);
    run(&mut h, Cmd::View(V::RotateReset));
    assert_eq!(h.state().active_tab().unwrap().view_rotation, mrpdf::geometry::Rotation::R0);
    // Tabs: sort, close the other, reopen it.
    run(&mut h, Cmd::View(V::SortTabs));
    assert!(h.state().active_tab().unwrap().doc.display_name().starts_with("Beispiel"), "stays active");
    h.state_mut().set_active_tab(0);
    assert!(h.state().active_tab().unwrap().doc.display_name().starts_with("Anderes"), "sorted first");
    h.state_mut().set_active_tab(1);
    run(&mut h, Cmd::View(V::CloseOthers));
    assert_eq!(h.state().tab_count(), 1);
    run(&mut h, Cmd::View(V::ReopenClosed));
    h.run_steps(6);
    assert_eq!(h.state().tab_count(), 2);
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
fn comment_tab_features() {
    use mrpdf::ui::tools::Tool;
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let file = sample_file(&dir, 1);
    let mut h = harness(vec![file], false);
    h.run_steps(10);
    let at = |h: &Harness<'_, MrPdfApp>, x: f32, y: f32| {
        h.state().active_tab().unwrap().screen_pos(0, mrpdf::geometry::PointF::new(x, y)).unwrap()
    };
    tab(&mut h, "Anmerkungen");
    save(&mut h, "anm-01-ribbon");

    // "Inhalt hervorheben": a click on the heading highlights the text object.
    h.get_by_label("Inhalt hervorheben").click();
    h.run_steps(2);
    let p = at(&h, 120.0, 708.0);
    click(&mut h, p);
    assert_eq!(kinds(&mut h), vec![AnnotKind::Highlight]);

    // The active tool has a Format tab with ready-made styles.
    tab(&mut h, "Format");
    save(&mut h, "anm-02-tool-format");
    h.get_by_label("Blau").click();
    h.run_steps(2);
    assert_eq!(
        h.state().settings().style(Tool::ContentHighlight).color,
        mrpdf::model::Rgba::rgb(30, 136, 229)
    );
    assert!(h.state().settings().keeps_tool(Tool::ContentHighlight));
    h.get_by_label("Auswahl beibehalten").click();
    h.run_steps(2);
    assert!(!h.state().settings().keeps_tool(Tool::ContentHighlight));

    // "Textkorrektur": select text, Delete strikes it out, typing replaces it.
    tab(&mut h, "Anmerkungen");
    h.get_by_label("Textkorrektur").click();
    h.run_steps(2);
    let (a, b) = (at(&h, 74.0, 604.0), at(&h, 160.0, 604.0));
    drag(&mut h, a, b);
    assert!(h.state().active_tab().unwrap().selection.is_some(), "text should stay selected");
    h.key_press(egui::Key::Delete);
    h.run_steps(4);
    assert_eq!(kinds(&mut h).last(), Some(&AnnotKind::StrikeOut));
    let (a, b) = (at(&h, 170.0, 604.0), at(&h, 260.0, 604.0));
    drag(&mut h, a, b);
    h.input_mut().events.push(Event::Text("neu".into()));
    h.run_steps(3);
    save(&mut h, "anm-03-text-correction");
    h.key_press(egui::Key::Enter);
    h.run_steps(4);
    let count = kinds(&mut h).len();
    assert_eq!(count, 3);
    {
        let tab = h.state_mut().active_tab_mut().unwrap();
        let last = tab.doc.annotations(0).unwrap().last().cloned().unwrap();
        assert_eq!(last.kind, AnnotKind::StrikeOut);
        assert!(last.contents.contains("neu"), "{}", last.contents);
    }

    // "Anzeigen": hide and show all comments, the menu, pop-ups.
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, egui::Key::Num8);
    h.run_steps(2);
    assert!(!h.state().active_tab().unwrap().show_annotations);
    h.key_press_modifiers(Modifiers::COMMAND, egui::Key::Num8);
    h.run_steps(2);
    assert!(h.state().active_tab().unwrap().show_annotations);
    h.get_by_label("Anzeigen").click();
    h.run_steps(3);
    assert!(h.query_by_label("Alle Pop-ups öffnen").is_some(), "show menu should be open");
    h.get_by_label_contains("Nach Typ").hover();
    h.run_steps(3);
    save(&mut h, "anm-04-show-menu");
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    quick(&mut h, "Alle Pop-ups öffnen");
    assert_eq!(h.state().active_tab().unwrap().open_popups.len(), count);
    save(&mut h, "anm-05-popups");
    quick(&mut h, "Alle Pop-ups schließen");
    assert!(h.state().active_tab().unwrap().open_popups.is_empty());

    // "Rechner": 12 + 3 = 15, inserted as text box.
    h.state_mut().active_tab_mut().unwrap().selected_annot = None;
    quick(&mut h, "Rechner");
    assert_eq!(h.state().open_calculators(), 1);
    for key in ["1", "2", "+", "3", "="] {
        h.get_all_by_label(key).last().unwrap().click();
        h.run_steps(2);
    }
    save(&mut h, "anm-06-calculator");
    h.get_all_by_label("Einfügen").last().unwrap().click();
    h.run_steps(4);
    {
        let tab = h.state_mut().active_tab_mut().unwrap();
        let last = tab.doc.annotations(0).unwrap().last().cloned().unwrap();
        assert_eq!(last.kind, AnnotKind::FreeText);
        assert!(last.contents.contains("15,00 ="), "{}", last.contents);
    }

    // "Medien": a video and a sound note are embedded and can be read back.
    let ctx = h.ctx.clone();
    let rect = mrpdf::geometry::RectF::new(300.0, 300.0, 460.0, 390.0);
    h.state_mut().add_media_bytes(&ctx, 0, rect, Tool::Video, "Film.mp4", b"not really a video");
    let mut wav =
        b"RIFF\x2c\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0\x40\x1f\0\0\x80\x3e\0\0\x02\0\x10\0data\x08\0\0\0"
            .to_vec();
    wav.extend_from_slice(&[1, 0, 2, 0, 3, 0, 4, 0]);
    let note = mrpdf::geometry::RectF::new(480.0, 300.0, 500.0, 320.0);
    h.state_mut().add_media_bytes(&ctx, 0, note, Tool::SoundNote, "Aufnahme.wav", &wav);
    h.run_steps(4);
    save(&mut h, "anm-08-media");
    {
        let tab = h.state_mut().active_tab_mut().unwrap();
        let annots = tab.doc.annotations(0).unwrap();
        let media: Vec<usize> =
            annots.iter().filter(|a| a.kind == AnnotKind::Media).map(|a| a.index).collect();
        assert_eq!(media.len(), 2);
        let (name, data) = tab.doc.media_file(0, media[0]).unwrap().unwrap();
        assert_eq!((name.as_str(), data.as_slice()), ("Film.mp4", &b"not really a video"[..]));
        let (name, data) = tab.doc.media_file(0, media[1]).unwrap().unwrap();
        assert_eq!(name, "sound.wav");
        assert_eq!(data, wav, "the recording comes back unchanged");
    }

    // "Formate für Anmerkungen".
    h.get_by_label("Formate für Anmerkungen").click();
    h.run_steps(3);
    save(&mut h, "anm-07-comment-styles");
    h.get_all_by_label("Schließen").last().unwrap().click();
    h.run_steps(2);
}

#[test]
fn form_tab_features() {
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let file = sample_file(&dir, 1);
    let mut h = harness(vec![file], false);
    h.run_steps(10);
    let at = |h: &Harness<'_, MrPdfApp>, x: f32, y: f32| {
        h.state().active_tab().unwrap().screen_pos(0, mrpdf::geometry::PointF::new(x, y)).unwrap()
    };
    tab(&mut h, "Formular");
    save(&mut h, "frm-01-ribbon");

    // "Datum": draw the field, then type a date in any common way.
    h.get_by_label("Datum").click();
    h.run_steps(2);
    let (a, b) = (at(&h, 72.0, 480.0), at(&h, 200.0, 460.0));
    drag(&mut h, a, b);
    // "Bild": a button that shows an image.
    h.get_by_label("Bild").click();
    h.run_steps(2);
    let (a, b) = (at(&h, 300.0, 480.0), at(&h, 420.0, 380.0));
    drag(&mut h, a, b);
    let fields = h.state_mut().active_tab_mut().unwrap().doc.form_fields(0).unwrap();
    assert_eq!(fields.len(), 2, "{fields:?}");
    let image = fields[1].annot_index;
    let ctx = h.ctx.clone();
    let green = mrpdf::pdf_edit::RgbaImage { width: 1, height: 1, rgba: vec![30, 160, 60, 255] };
    h.state_mut().set_field_image(&ctx, 0, image, &green);
    h.get_by_label("Hand").click();
    h.run_steps(2);
    let p = at(&h, 130.0, 470.0);
    click(&mut h, p);
    h.run_steps(2);
    h.input_mut().events.push(Event::Text("3.4.26".into()));
    h.run_steps(2);
    h.key_press(egui::Key::Enter);
    h.run_steps(4);
    let fields = h.state_mut().active_tab_mut().unwrap().doc.form_fields(0).unwrap();
    assert_eq!(fields[0].value, "03.04.2026");
    save(&mut h, "frm-02-date-image");

    // "Felder hervorheben ▾" per kind.
    h.get_by_label("Felder hervorheben").click();
    h.run_steps(3);
    save(&mut h, "frm-03-highlight-menu");
    h.get_by_label("Textfelder").click();
    h.run_steps(3);
    assert!(h.state().settings().highlight_off.contains(&mrpdf::ui::settings::FieldGroup::TextFields));

    // The form pane (Ctrl+I) lists the fields.
    h.key_press_modifiers(Modifiers::COMMAND, egui::Key::I);
    h.run_steps(4);
    save(&mut h, "frm-04-form-pane");
    assert!(h.query_by_label_contains("= 03.04.2026").is_some(), "form pane should list the date field");

    // "Alle JavaScripte" shows the date format scripts.
    quick(&mut h, "Alle JavaScripte");
    assert!(h.query_by_label_contains("Format").is_some());
    save(&mut h, "frm-05-javascripts");
    h.get_all_by_label("Schließen").last().unwrap().click();
    h.run_steps(2);

    // "Formulare ausfüllen" dialog.
    quick(&mut h, "Formulare ausfüllen");
    assert!(h.query_by_label("Automatisch ermitteln").is_some());
    save(&mut h, "frm-06-fill-forms");
    h.get_all_by_label("Abbrechen").last().unwrap().click();
    h.run_steps(2);

    // "Formulare erkennen" finds nothing new on this page.
    tab(&mut h, "Formular");
    h.get_by_label("Formulare erkennen").click();
    h.run_steps(3);
    assert_eq!(h.state_mut().active_tab_mut().unwrap().doc.form_fields(0).unwrap().len(), 2);
}

#[test]
fn convert_tab_dialogs() {
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let file = sample_file(&dir, 2);
    let mut h = harness(vec![file], false);
    h.run_steps(10);
    tab(&mut h, "Umwandeln");
    save(&mut h, "umw-01-ribbon");
    h.get_by_label("In MS Excel").click();
    h.run_steps(3);
    assert!(h.query_by_label("Tabellen erkennen").is_some());
    save(&mut h, "umw-02-excel");
    h.get_all_by_label("Abbrechen").last().unwrap().click();
    h.run_steps(2);
    h.get_by_label("In MS PowerPoint").click();
    h.run_steps(3);
    assert!(h.query_by_label("Verweise einschließen").is_some());
    save(&mut h, "umw-03-powerpoint");
    h.get_all_by_label("Abbrechen").last().unwrap().click();
    h.run_steps(2);
    h.get_by_label("In MS Word").click();
    h.run_steps(3);
    assert!(h.query_by_label("Bilder übernehmen").is_some());
    h.get_by_label("Seitenlayout erhalten").click();
    h.run_steps(2);
    save(&mut h, "umw-04-word");
    h.get_all_by_label("Abbrechen").last().unwrap().click();
    h.run_steps(2);
    h.get_by_label("Text auf Seiten erkennen").click();
    h.run_steps(3);
    save(&mut h, "umw-05-ocr");
    h.get_all_by_label("Abbrechen").last().unwrap().click();
    h.run_steps(2);
    h.get_by_label("Schriften ersetzen").click();
    h.run_steps(3);
    h.get_by_label("Vorschläge übernehmen").click();
    h.run_steps(2);
    save(&mut h, "umw-06-fonts");
    h.get_all_by_label("OK").last().unwrap().click();
    h.run_steps(4);
    let fonts = h.state_mut().active_tab_mut().unwrap().doc.fonts().unwrap();
    assert!(
        fonts.iter().all(|f| mrpdf::font_replace::STANDARD_FONTS.contains(&f.name.as_str())),
        "{fonts:?}"
    );
    quick(&mut h, "Seiten neu einfärben");
    h.get_all_by_label_contains("Dunkel").last().expect("preset button").click();
    h.run_steps(2);
    save(&mut h, "umw-07-recolor");
    h.get_all_by_label("OK").last().unwrap().click();
    h.run_steps(6);
    save(&mut h, "umw-08-recolored");
    assert!(h.state().active_tab().unwrap().doc.can_undo().is_some());
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
    h.get_by_label("Oval").click();
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
    use mrpdf::ui::app::Dialog;
    use mrpdf::ui::app::bookmark_dialogs::BookmarkDialog;
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let file = sample_file(&dir, 3);
    let mut h = harness(vec![file], false);
    h.run_steps(10);
    let ok = |h: &mut Harness<'_, MrPdfApp>| {
        h.get_all_by_label("OK").last().unwrap().click();
        h.run_steps(6);
    };
    tab(&mut h, "Lesezeichen");
    save(&mut h, "les-01-ribbon");

    // "Aus Seitentext": the headings are suggested as a rule.
    quick(&mut h, "Aus Seitentext");
    save(&mut h, "les-02-from-text");
    h.get_by_label("Hinzufügen…").click();
    h.run_steps(3);
    save(&mut h, "les-03-rule");
    h.get_by_label("Formatierung").click();
    h.run_steps(3);
    save(&mut h, "les-04-rule-format");
    h.get_all_by_label("Abbrechen").last().unwrap().click();
    h.run_steps(3);
    ok(&mut h);
    assert_eq!(h.state().active_tab().unwrap().doc.bookmarks().len(), 3);

    // "Inhaltsverzeichnis erstellen".
    quick(&mut h, "Inhaltsverzeichnis erstellen");
    save(&mut h, "les-05-toc-dialog");
    h.get_all_by_label("Erstellen").last().unwrap().click();
    h.run_steps(6);
    assert_eq!(h.state().active_tab().unwrap().doc.page_count(), 4);
    save(&mut h, "27-table-of-contents");

    // "Überprüfen": all bookmarks are fine.
    quick(&mut h, "Überprüfen");
    h.get_all_by_label("Überprüfen").last().unwrap().click();
    h.run_steps(3);
    match h.state().dialog() {
        Some(Dialog::Bookmark(d)) => match d.as_ref() {
            BookmarkDialog::Check(s) => assert_eq!(s.findings.as_ref().map(Vec::len), Some(0)),
            other => panic!("unexpected dialog {other:?}"),
        },
        other => panic!("unexpected dialog {other:?}"),
    }
    save(&mut h, "les-06-check");
    h.key_press(egui::Key::Escape);
    h.run_steps(3);

    // "Vergrößerung ändern".
    quick(&mut h, "Vergrößerung ändern");
    save(&mut h, "les-07-zoom");
    ok(&mut h);
    assert!(h.state().dialog().is_none());

    // "Aktionen entfernen" lists the action types.
    quick(&mut h, "Aktionen entfernen");
    assert!(h.query_by_label("Web-Seite öffnen").is_some());
    save(&mut h, "les-08-remove-actions");
    h.key_press(egui::Key::Escape);
    h.run_steps(3);

    // "Aus Inhaltsverzeichnis": the contents page made above.
    quick(&mut h, "Aus Inhaltsverzeichnis");
    ok(&mut h);
    let tab = h.state().active_tab().unwrap();
    assert_eq!(tab.doc.bookmarks().len(), 6, "three more from the contents page");
    assert_eq!(tab.doc.bookmarks()[3].page, Some(1));
}

#[test]
fn edit_page_content_ui() {
    use mrpdf::ui::tools::Tool;
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let file = sample_file(&dir, 1);
    let mut h = harness(vec![file], false);
    h.run_steps(10);
    let at = |h: &Harness<'_, MrPdfApp>, x: f32, y: f32| {
        h.state().active_tab().unwrap().screen_pos(0, mrpdf::geometry::PointF::new(x, y)).unwrap()
    };
    let objects =
        |h: &mut Harness<'_, MrPdfApp>| h.state_mut().active_tab_mut().unwrap().doc.page_objects(0).unwrap();
    tab(&mut h, "Start");
    save(&mut h, "obj-01-start");

    // "Objekte bearbeiten": click the heading, drag it 50 pt down.
    quick(&mut h, "Objekte bearbeiten");
    assert_eq!(h.state().tool(), Tool::EditObjects);
    let p = at(&h, 150.0, 710.0);
    click(&mut h, p);
    h.run_steps(3);
    assert_eq!(h.state().active_tab().unwrap().content.selected, vec![0]);
    save(&mut h, "obj-02-selected");
    let (a, b) = (at(&h, 150.0, 710.0), at(&h, 150.0, 660.0));
    drag(&mut h, a, b);
    h.run_steps(4);
    let y = objects(&mut h)[0].origin().y;
    assert!((y - 650.0).abs() < 2.0, "moved to {y}");
    assert_eq!(h.state().active_tab().unwrap().content.selected, vec![0], "stays selected");

    // The contextual tab "Anordnen".
    tab(&mut h, "Anordnen");
    save(&mut h, "obj-03-arrange");
    tab(&mut h, "Format");
    save(&mut h, "obj-04-format");

    // Strg+D duplicates, Entf deletes the copy.
    h.key_press_modifiers(Modifiers::COMMAND, egui::Key::D);
    h.run_steps(4);
    assert_eq!(objects(&mut h).len(), 3);
    h.key_press(egui::Key::Delete);
    h.run_steps(4);
    assert_eq!(objects(&mut h).len(), 2);

    // Resize with the bottom right handle, then rotate with the round handle.
    let p = at(&h, 150.0, 660.0);
    click(&mut h, p);
    h.run_steps(3);
    let r = objects(&mut h)[0].rect;
    let corner = at(&h, r.x1, r.y0);
    let further = corner + egui::vec2(60.0, 20.0);
    drag(&mut h, corner, further);
    h.run_steps(4);
    let wider = objects(&mut h)[0].rect;
    assert!(wider.width() > r.width() + 40.0, "{r:?} -> {wider:?}");
    let top = at(&h, (wider.x0 + wider.x1) / 2.0, wider.y1);
    let knob = top - egui::vec2(0.0, 22.0);
    let side = knob + egui::vec2(120.0, 120.0);
    drag(&mut h, knob, side);
    h.run_steps(4);
    let m = objects(&mut h)[0].matrix.0;
    assert!(m[1].abs() > 0.1, "rotated: {m:?}");
    save(&mut h, "obj-05-rotated");
    // Undo both, step by step.
    h.key_press_modifiers(Modifiers::COMMAND, egui::Key::Z);
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, egui::Key::Z);
    h.run_steps(3);
    assert!((objects(&mut h)[0].rect.width() - r.width()).abs() < 1.0);

    // "Hinzufügen › Text hinzufügen": new text in the editor.
    tab(&mut h, "Start");
    h.get_by_label("Hinzufügen").click();
    h.run_steps(2);
    h.get_by_label("Text hinzufügen").click();
    h.run_steps(4);
    assert_eq!(h.state().tool(), Tool::EditText);
    h.state_mut().active_tab_mut().unwrap().content.text.as_mut().unwrap().text = "Hinzugefügt".into();
    h.key_press_modifiers(Modifiers::COMMAND, egui::Key::Enter);
    h.run_steps(4);
    assert!(h.state().active_tab().unwrap().doc.page_text(0).unwrap().all_text().contains("Hinzugefügt"));
    assert_eq!(objects(&mut h).len(), 3);

    // "Text bearbeiten": click the second line, replace its text.
    quick(&mut h, "Text bearbeiten");
    assert_eq!(h.state().tool(), Tool::EditText);
    let p = at(&h, 120.0, 604.0);
    click(&mut h, p);
    h.run_steps(4);
    assert!(h.state().active_tab().unwrap().content.text.is_some(), "editor open");
    // Typing goes into the editor; the test replaces the whole text.
    h.input_mut().events.push(Event::Text("!".into()));
    h.run_steps(2);
    assert!(h.state().active_tab().unwrap().content.text.as_ref().unwrap().text.ends_with('!'));
    h.state_mut().active_tab_mut().unwrap().content.text.as_mut().unwrap().text =
        "Neuer Inhalt der Zeile".into();
    h.run_steps(3);
    save(&mut h, "obj-06-text-editor");
    h.key_press_modifiers(Modifiers::COMMAND, egui::Key::Enter);
    h.run_steps(6);
    let text = h.state().active_tab().unwrap().doc.page_text(0).unwrap().all_text();
    assert!(text.contains("Neuer Inhalt der Zeile") && !text.contains("Zweite Zeile"), "{text}");
    save(&mut h, "obj-07-text-applied");
    assert!(h.state().active_tab().unwrap().doc.is_modified());
}

#[test]
fn insert_pages_source_list() {
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let first = sample_file(&dir, 2);
    let second = dir.path().join("Zweites.pdf");
    std::fs::write(&second, common::text_pdf(3)).unwrap();
    let mut h = harness(vec![first, second], false);
    h.run_steps(10);
    assert_eq!(h.state().tab_count(), 2);
    h.state_mut().set_active_tab(0);
    h.run_steps(3);
    tab(&mut h, "Dokumentaufbau");
    h.get_by_label("Einfügen").click();
    h.run_steps(3);
    h.get_by_label("Seiten einfügen…").click();
    h.run_steps(3);
    assert!(matches!(h.state().dialog(), Some(mrpdf::ui::app::Dialog::Pages(_))));
    h.get_by_role(egui::accesskit::Role::ComboBox).click();
    h.run_steps(3);
    save(&mut h, "les-04-insert-source");
    // The document tab carries the same name; the list entry comes last.
    h.get_all_by_label("Zweites.pdf").last().unwrap().click();
    h.run_steps(3);
    h.get_by_label("OK").click();
    h.run_steps(4);
    assert_eq!(h.state().active_tab().unwrap().doc.page_count(), 5, "2 + 3 pages of the open document");
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
fn spell_check_and_comment_menus() {
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let file = sample_file(&dir, 2);
    // A tiny Hunspell dictionary (MrPDF ships none).
    std::fs::write(dir.path().join("mini.aff"), "SET UTF-8\nTRY estinrdaS\n").unwrap();
    std::fs::write(dir.path().join("mini.dic"), "5\nDies\nist\nein\nTest\nNotiz\n").unwrap();
    let settings = Settings {
        language: Language::German,
        backstage_at_start: false,
        spell_dictionary: Some(dir.path().join("mini.dic")),
        ..Settings::default()
    };
    let mut h = harness_with(settings, vec![file]);
    h.run_steps(10);
    let notes = |h: &mut Harness<'_, MrPdfApp>| -> Vec<String> {
        let tab = h.state_mut().active_tab_mut().unwrap();
        tab.doc
            .annotations(0)
            .unwrap()
            .into_iter()
            .filter(|a| a.kind.is_comment())
            .map(|a| a.contents)
            .collect()
    };
    {
        let tab = h.state_mut().active_tab_mut().unwrap();
        for (i, text) in ["Dies ist ein Tset", "Notiz"].into_iter().enumerate() {
            let mut note = mrpdf::pdf_edit::NewAnnotation::new(AnnotKind::Note, Default::default());
            let x = 400.0 + 40.0 * i as f32;
            note.rect = mrpdf::geometry::RectF::new(x, 700.0, x + 20.0, 720.0);
            note.contents = text.into();
            tab.doc.add_annotation(0, &note).unwrap();
        }
        tab.after_edit();
    }
    tab(&mut h, "Überprüfen");
    save(&mut h, "rev-01-ribbon");
    h.get_by_label("Rechtschreibprüfung").click();
    h.run_steps(3);
    h.get_by_label("Prüfung starten").click();
    h.run_steps(4);
    match h.state().dialog() {
        Some(mrpdf::ui::app::Dialog::SpellCheck(s)) => {
            let words: Vec<&str> = s.findings.as_ref().unwrap().iter().map(|f| f.word.as_str()).collect();
            assert_eq!(words, ["Tset"]);
            assert_eq!(s.replacement, "Test");
        }
        other => panic!("unexpected dialog {other:?}"),
    }
    save(&mut h, "rev-02-spell-check");
    h.get_all_by_label("Ändern").last().unwrap().click();
    h.run_steps(4);
    assert!(notes(&mut h).contains(&"Dies ist ein Test".to_string()), "{:?}", notes(&mut h));
    h.get_all_by_label("Schließen").last().unwrap().click();
    h.run_steps(3);
    assert!(h.state().dialog().is_none());

    // "Anmerkungen löschen ▾" > "Anmerkungen dieser Seite löschen".
    h.get_by_label("Anmerkungen löschen").click();
    h.run_steps(3);
    save(&mut h, "rev-03-delete-menu");
    h.get_by_label("Anmerkungen dieser Seite löschen").click();
    h.run_steps(4);
    assert!(notes(&mut h).is_empty());
    {
        let tab = h.state_mut().active_tab_mut().unwrap();
        assert!(tab.doc.undo().unwrap());
        tab.after_edit();
    }
    h.run_steps(3);
    assert_eq!(notes(&mut h).len(), 2);

    // "Anmerkungen einbetten ▾" > "Alle Anmerkungen einbetten".
    h.get_by_label("Anmerkungen einbetten").click();
    h.run_steps(3);
    h.get_by_label("Alle Anmerkungen einbetten").click();
    h.run_steps(6);
    assert!(notes(&mut h).is_empty(), "flattened");
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

#[test]
fn open_other_file_types() {
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let md = dir.path().join("Notizen.md");
    std::fs::write(&md, "# Überschrift\n\nText mit **fett**.\n\n- Punkt\n").unwrap();
    let csv = dir.path().join("Tabelle.csv");
    std::fs::write(&csv, "Name;Wert\nA;1\nB;2\n").unwrap();
    let mut h = harness(vec![md, csv], false);
    h.run_steps(12);
    assert_eq!(h.state().tab_count(), 2);
    let tab = h.state().active_tab().unwrap();
    assert_eq!(tab.doc.display_name(), "Tabelle.pdf");
    assert!(tab.doc.page_text(0).unwrap().all_text().contains("Wert"));
    assert!(tab.doc.is_modified(), "converted documents are unsaved");
    save(&mut h, "67-csv-document");
    h.state_mut().set_active_tab(0);
    h.run_steps(3);
    let text = h.state().active_tab().unwrap().doc.page_text(0).unwrap().all_text();
    assert!(text.contains("Überschrift") && text.contains("Punkt"), "{text}");
}

#[test]
fn file_operations_and_recovery() {
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let file = sample_file(&dir, 2);
    let recovery = dir.path().join("recovery");
    let mut h = harness(vec![file.clone()], false);
    h.state_mut().set_recovery_dir(Some(recovery.clone()));
    h.run_steps(10);
    // Umbenennen: type a new name into the focused field.
    open_backstage(&mut h);
    h.get_by_label("Umbenennen").click();
    h.run_steps(4);
    assert!(matches!(h.state().dialog(), Some(mrpdf::ui::app::Dialog::RenameFile { .. })));
    // One frame: delete the old name, type the new one.
    for _ in 0.."Beispiel".len() {
        h.input_mut().events.push(Event::Key {
            key: egui::Key::Backspace,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        });
    }
    h.input_mut().events.push(Event::Text("Umbenannt".into()));
    h.run_steps(3);
    save(&mut h, "68-rename-dialog");
    h.get_by_label("OK").click();
    h.run_steps(4);
    let renamed = dir.path().join("Umbenannt.pdf");
    assert!(renamed.exists() && !file.exists());
    assert_eq!(h.state().active_tab().unwrap().doc.path(), Some(renamed.as_path()));
    assert_eq!(h.state().active_tab().unwrap().doc.display_name(), "Umbenannt.pdf");
    // Zurücksetzen discards a change after a confirmation.
    h.state_mut().active_tab_mut().unwrap().doc.rotate_pages(&[0], 90).unwrap();
    open_backstage(&mut h);
    h.get_by_label("Zurücksetzen").click();
    h.run_steps(3);
    assert!(matches!(h.state().dialog(), Some(mrpdf::ui::app::Dialog::Confirm { .. })));
    h.get_by_label("OK").click();
    h.run_steps(4);
    assert!(!h.state().active_tab().unwrap().doc.is_modified());
    let (w, hgt) = h.state().active_tab().unwrap().doc.geometry(0).unwrap().view_size();
    assert!(w < hgt, "rotation undone");
    // A recovery copy of the changed document …
    h.state_mut().active_tab_mut().unwrap().doc.rotate_pages(&[1], 90).unwrap();
    h.state_mut().write_recovery_now();
    assert_eq!(mrpdf::files::list_recovery(&recovery).len(), 1);
    // … is offered by the next start (as after a crash).
    drop(h);
    let mut h = harness(Vec::new(), false);
    h.state_mut().set_recovery_dir(Some(recovery.clone()));
    h.run_steps(5);
    save(&mut h, "69-recovery-bar");
    h.get_by_label("Wiederherstellen").click();
    h.run_steps(6);
    assert_eq!(h.state().tab_count(), 1);
    let tab = h.state().active_tab().unwrap();
    assert!(tab.doc.is_modified());
    assert_eq!(tab.doc.path(), Some(renamed.as_path()), "saving writes the original file");
    let (w, hgt) = tab.doc.geometry(1).unwrap().view_size();
    assert!(w > hgt, "the change is restored");
    assert!(mrpdf::files::list_recovery(&recovery).is_empty());
}

#[test]
fn optimize_and_space_usage() {
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let file = sample_file(&dir, 3);
    let mut h = harness(vec![file], false);
    h.run_steps(10);
    open_backstage(&mut h);
    h.get_by_label("Optimiert speichern").click();
    h.run_steps(4);
    assert!(matches!(h.state().dialog(), Some(mrpdf::ui::app::Dialog::Optimize(_))));
    save(&mut h, "70-optimize-images");
    for (page, shot) in [("Schriftarten", "71-optimize-fonts"), ("Aufräumen", "72-optimize-cleanup")] {
        h.get_by_label(page).click();
        h.run_steps(3);
        save(&mut h, shot);
    }
    // "Platzverbrauch untersuchen…" shows the report.
    h.get_by_label("Platzverbrauch untersuchen…").click();
    h.run_steps(4);
    assert!(matches!(h.state().dialog(), Some(mrpdf::ui::app::Dialog::SpaceUsage(_))));
    assert!(h.query_all_by_label("Seiteninhalt").next().is_some());
    save(&mut h, "73-space-usage");
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    // Optimize into a new file; the tab shows it afterwards.
    let out = dir.path().join("Optimiert.pdf");
    let options = mrpdf::optimize::OptimizeOptions { metadata: true, ..Default::default() };
    let ctx = h.ctx.clone();
    h.state_mut().save_optimized_to(&ctx, &out, &options);
    h.run_steps(4);
    assert!(out.exists());
    let tab = h.state().active_tab().unwrap();
    assert_eq!(tab.doc.path(), Some(out.as_path()));
    assert_eq!(tab.doc.page_count(), 3);
    assert!(tab.doc.page_text(2).unwrap().all_text().contains("Page 3"));
    assert!(matches!(h.state().dialog(), Some(mrpdf::ui::app::Dialog::Message { .. })));
    save(&mut h, "74-optimize-result");
}

#[test]
fn print_and_export_dialogs() {
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let file = sample_file(&dir, 3);
    let mut h = harness(vec![file], false);
    h.run_steps(10);
    h.key_press_modifiers(Modifiers::COMMAND, egui::Key::P);
    h.run_steps(5);
    assert!(matches!(h.state().dialog(), Some(mrpdf::ui::app::Dialog::Print(_))));
    assert!(h.query_all_by_label("Position und Skalierung").next().is_some());
    save(&mut h, "75-print-dialog");
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(h.state().dialog().is_none());
    // Exportieren → In Bilder: one multi-page TIFF next to the document.
    open_backstage(&mut h);
    h.get_by_label("Exportieren").click();
    h.run_steps(3);
    h.get_by_label("In Bilder").click();
    h.run_steps(5);
    assert!(matches!(h.state().dialog(), Some(mrpdf::ui::app::Dialog::ExportImages(_))));
    save(&mut h, "76-export-images");
    h.get_by_label("Öffne Zielordner und wähle Dateien aus").click();
    h.run_steps(2);
    h.get_by_label("OK").click();
    h.run_steps(6);
    let tif = dir.path().join("Beispiel_1.tif");
    assert!(tif.exists(), "TIFF written");
    let data = std::fs::read(&tif).unwrap();
    assert_eq!(&data[..4], b"II*\0");
    // Word settings.
    open_backstage(&mut h);
    h.get_by_label("Exportieren").click();
    h.run_steps(3);
    h.get_by_label("In Microsoft-Word-Dokument").click();
    h.run_steps(4);
    assert!(matches!(h.state().dialog(), Some(mrpdf::ui::app::Dialog::ExportWord { .. })));
    save(&mut h, "77-export-word");
}

#[test]
fn document_properties_dialog() {
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let file = sample_file(&dir, 2);
    let mut h = harness(vec![file], false);
    h.run_steps(10);
    h.key_press_modifiers(Modifiers::COMMAND, egui::Key::D);
    h.run_steps(4);
    assert!(matches!(h.state().dialog(), Some(mrpdf::ui::app::Dialog::Properties(_))));
    assert!(h.query_all_by_label("Beispiel.pdf").next().is_some(), "file name shown");
    save(&mut h, "78-properties-description");
    for (page, shot) in [
        ("Schriftarten", "79-properties-fonts"),
        ("Sicherheit", "80-properties-security"),
        ("Ansicht beim Öffnen", "81-properties-view"),
        ("Eigene", "82-properties-custom"),
        ("Erweitert", "83-properties-advanced"),
        ("Aktionen", "84-properties-actions"),
        ("JavaScript", "85-properties-javascript"),
    ] {
        h.get_all_by_label(page).next().unwrap().click();
        h.run_steps(3);
        save(&mut h, shot);
    }
    // Custom property: add, then OK writes it into the document.
    h.get_by_label("Eigene").click();
    h.run_steps(3);
    h.get_by_label("Hinzufügen…").click();
    h.run_steps(3);
    assert!(h.query_by_label("Übernehmen").is_some());
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(h.state().dialog().is_none(), "Esc cancels");
    assert!(!h.state().active_tab().unwrap().doc.is_modified());
    // A change on "Ansicht beim Öffnen" is written with OK.
    h.key_press_modifiers(Modifiers::COMMAND, egui::Key::D);
    h.run_steps(4);
    h.get_by_label("Ansicht beim Öffnen").click();
    h.run_steps(3);
    h.get_by_label("Fenster auf Bildschirm zentrieren").click();
    h.run_steps(3);
    h.get_by_label("OK").click();
    h.run_steps(4);
    let tab = h.state_mut().active_tab_mut().unwrap();
    assert!(tab.doc.is_modified());
    assert!(tab.doc.properties().unwrap().center_window);
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
    // Pinning moves the entry to "Angeheftete Dokumente".
    h.get_by_label("Anheften").click();
    h.run_steps(4);
    assert_eq!(h.state().settings().pinned_files.len(), 1);
    assert!(h.query_all_by_label("Lösen").next().is_some());
    save(&mut h, "63-backstage-pinned");
    // Entries can be removed from the list.
    h.get_by_label("Aus der Liste entfernen").click();
    h.run_steps(3);
    assert!(h.query_all_by_label("Beispiel").next().is_none());
    assert!(h.state().settings().pinned_files.is_empty());
    // Sessions page.
    h.get_by_label("Sitzungen").click();
    h.run_steps(3);
    assert!(h.query_all_by_label("Aktuelle Sitzung speichern").next().is_some());
    save(&mut h, "64-backstage-sessions");
    // "Von URL öffnen" asks for the address.
    h.get_by_label("Von URL öffnen").click();
    h.run_steps(3);
    assert!(matches!(h.state().dialog(), Some(mrpdf::ui::app::Dialog::OpenUrl { .. })));
    save(&mut h, "65-open-url");
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    open_backstage(&mut h);
    // Disabled entries do nothing.
    h.get_by_label("Speicherort hinzufügen").click();
    h.run_steps(3);
    assert!(h.state().backstage_open() && h.state().dialog().is_none());
    // "Einstellungen verwalten": "zurücksetzen" asks first.
    h.get_by_label("Einstellungen verwalten").click();
    h.run_steps(3);
    assert!(h.query_by_label("Einstellungen exportieren").is_some());
    assert!(h.query_by_label("Einstellungen importieren").is_some());
    save(&mut h, "86-backstage-manage-settings");
    h.get_by_label("Einstellungen zurücksetzen").click();
    h.run_steps(3);
    assert!(matches!(h.state().dialog(), Some(mrpdf::ui::app::Dialog::Confirm { .. })));
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    if !h.state().backstage_open() {
        open_backstage(&mut h);
    }
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
    assert!(!h.state().backstage_open(), "the dialog replaces the backstage");
    assert!(h.query_all_by_label("Neues leeres Dokument").next().is_some());
    h.get_by_label("Querformat").click();
    h.run_steps(3);
    save(&mut h, "66-new-blank-dialog");
    h.get_by_label("OK").click();
    h.run_steps(5);
    assert_eq!(h.state().tab_count(), 2);
    let (w, hgt) = h.state().active_tab().unwrap().doc.geometry(0).unwrap().view_size();
    assert!(w > hgt, "landscape A4: {w} x {hgt}");
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
    assert!(matches!(h.state().dialog(), Some(mrpdf::ui::app::Dialog::Properties(_))));
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

#[test]
fn settings_dialog() {
    use mrpdf::ui::app::Dialog;
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let file = sample_file(&dir, 3);
    let mut h = harness(vec![file.clone()], false);
    h.run_steps(10);
    h.key_press_modifiers(Modifiers::COMMAND, egui::Key::K);
    h.run_steps(3);
    assert!(matches!(h.state().dialog(), Some(Dialog::Settings(_))));
    // All 38 categories are listed; "Allgemein" is shown first.
    for label in ["Dokumente", "Seitenanzeige", "Identität", "Sicherheit", "Vollbild", "Vorlesen"] {
        assert!(h.query_all_by_label(label).next().is_some(), "{label} missing");
    }
    save(&mut h, "87-settings-general");
    // A checkbox changes only the draft; "Übernehmen" applies it and keeps the dialog.
    h.get_by_label("Tab-Leiste verbergen, wenn nur ein Dokument geöffnet ist").click();
    h.run_steps(2);
    assert!(!h.state().settings().hide_single_tab);
    h.get_by_label("Übernehmen").click();
    h.run_steps(3);
    assert!(h.state().settings().hide_single_tab);
    assert!(matches!(h.state().dialog(), Some(Dialog::Settings(_))));
    // Identity page (further down in the scrolled list).
    h.get_by_label("Identität").scroll_to_me();
    h.run_steps(6);
    h.get_by_label("Identität").click();
    h.run_steps(3);
    assert!(h.query_by_label("Organisation:").is_some(), "identity page shown");
    save(&mut h, "88-settings-identity");
    h.state_mut().settings_draft_mut().unwrap().identity.organisation = "Firma\u{7}".into();
    h.get_by_label("Sicherheit").scroll_to_me();
    h.run_steps(6);
    h.get_by_label("Sicherheit").click();
    h.run_steps(3);
    assert!(h.query_by_label("Vertrauensliste").is_some(), "security page shown");
    save(&mut h, "89-settings-security");
    h.get_by_label("Lizenz").scroll_to_me();
    h.run_steps(6);
    h.get_by_label("Lizenz").click();
    h.run_steps(3);
    assert!(h.query_by_label("❌ MrPDF ist freie Software und braucht keine Lizenz.").is_some());
    // "Farben im Dokument ersetzen" and OK.
    let draft = h.state_mut().settings_draft_mut().unwrap();
    draft.recolor = true;
    draft.recolor_background = [0, 0, 0];
    draft.recolor_text = [255, 255, 0];
    h.get_by_label("OK").click();
    h.run_steps(5);
    assert!(h.state().dialog().is_none());
    assert_eq!(h.state().settings().identity.organisation, "Firma", "control characters removed");
    assert_eq!(h.state().active_tab().unwrap().recolor, Some(([0, 0, 0], [255, 255, 0])));
    save(&mut h, "90-settings-recolor");
    // Abbrechen discards the changes.
    h.key_press_modifiers(Modifiers::COMMAND, egui::Key::K);
    h.run_steps(3);
    h.state_mut().settings_draft_mut().unwrap().author = "Jemand".into();
    h.get_by_label("Abbrechen").click();
    h.run_steps(3);
    assert!(h.state().dialog().is_none());
    assert_eq!(h.state().settings().author, "Tester");
    // The page shown last is restored when the document is opened again.
    h.state_mut().active_tab_mut().unwrap().go_to_page(2);
    h.run_steps(4);
    h.key_press_modifiers(Modifiers::COMMAND, egui::Key::W);
    h.run_steps(4);
    assert_eq!(h.state().tab_count(), 0);
    let ctx = h.ctx.clone();
    h.state_mut().open_path(&ctx, file.clone(), None);
    h.run_steps(6);
    assert_eq!(h.state().active_tab().unwrap().current_page, 2);
}

#[test]
fn narrow_window_and_movable_dialog() {
    use mrpdf::ui::app::Dialog;
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let file = sample_file(&dir, 2);
    let mut h = harness(vec![file], false);
    h.run_steps(10);
    assert!(h.query_by_label("»").is_none(), "all tabs fit at full width");
    // Medium width: small buttons lose their labels, groups collapse from the right.
    h.set_size(egui::vec2(900.0, 700.0));
    h.run_steps(16);
    save(&mut h, "91-ribbon-900");
    // The ribbon fits: its right-most group is a drop-down button now.
    assert!(h.get_by_label("Verweise").rect().right() <= 900.0, "ribbon fits");
    assert!(h.query_by_label("»").is_some(), "the last tabs are in the » menu");
    // Narrow: tabs that do not fit are in the "»" menu.
    h.set_size(egui::vec2(640.0, 600.0));
    h.run_steps(16);
    save(&mut h, "92-ribbon-640");
    h.get_by_label("»").click();
    h.run_steps(3);
    save(&mut h, "93-ribbon-more-tabs");
    h.get_by_label("Hilfe").click();
    h.run_steps(4);
    assert!(h.query_by_label("»").is_some());
    // A collapsed group opens as drop-down; a command in it runs and closes it.
    tab(&mut h, "Start");
    h.run_steps(16);
    h.get_by_label("Verweise").click();
    h.run_steps(3);
    save(&mut h, "94-ribbon-collapsed-group");
    h.get_by_label("Verweis erstellen").click();
    h.run_steps(3);
    assert_eq!(h.state().tool(), mrpdf::ui::tools::Tool::Link);
    // Dialogs are windows: drag the title bar, close with ✕.
    h.set_size(egui::vec2(1280.0, 820.0));
    h.run_steps(16);
    h.key_press_modifiers(Modifiers::COMMAND, egui::Key::D);
    h.run_steps(4);
    assert!(matches!(h.state().dialog(), Some(Dialog::Properties(_))));
    let bar = h.get_by_label("Dokumenteigenschaften").rect();
    let from = bar.left_center() + egui::vec2(60.0, 0.0);
    drag(&mut h, from, from + egui::vec2(-150.0, -60.0));
    let moved = h.get_by_label("Dokumenteigenschaften").rect();
    // A drag starts after a few pixels of movement.
    assert!((moved.left() - (bar.left() - 150.0)).abs() < 8.0, "{bar:?} -> {moved:?}");
    assert!((moved.top() - (bar.top() - 60.0)).abs() < 8.0, "{bar:?} -> {moved:?}");
    save(&mut h, "95-dialog-moved");
    h.get_by_label("✕").click();
    h.run_steps(3);
    assert!(h.state().dialog().is_none());
    // The next dialog opens in the middle again.
    h.key_press_modifiers(Modifiers::COMMAND, egui::Key::D);
    h.run_steps(4);
    let again = h.get_by_label("Dokumenteigenschaften").rect();
    assert!((again.left() - bar.left()).abs() < 2.0);
}

#[test]
fn transitions_tab_windows_and_portfolio() {
    use mrpdf::transitions::{Direction, Speed, Style, Transition};
    use mrpdf::ui::app::Dialog;
    use mrpdf::ui::app::commands::Cmd;
    use mrpdf::ui::app::view_cmds::ViewCmd as V;
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let file = sample_file(&dir, 3);
    let other = dir.path().join("Zweites.pdf");
    std::fs::write(&other, common::text_pdf(1)).unwrap();
    let mut h = harness(vec![file, other], false);
    h.run_steps(12);
    h.state_mut().set_active_tab(0);
    h.run_steps(3);
    let run = |h: &mut Harness<'_, MrPdfApp>, cmd: Cmd| {
        let ctx = h.ctx.clone();
        h.state_mut().run(&ctx, cmd);
        h.run_steps(4);
    };
    // Seitenübergänge: dialog, then a wipe on page 2 written into the PDF.
    run(&mut h, Cmd::View(V::Transitions));
    assert!(matches!(h.state().dialog(), Some(Dialog::Transitions(_))));
    save(&mut h, "103-transitions-dialog");
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    let wipe = Transition {
        style: Style::Wipe,
        direction: Some(Direction::Right),
        speed: Speed::Slow,
        advance: None,
    };
    h.state_mut().active_tab_mut().unwrap().doc.set_transitions(&[1], wipe, false).unwrap();
    let read = h.state_mut().active_tab_mut().unwrap().doc.transitions().unwrap();
    assert_eq!(read[1].style, Style::Wipe);
    assert_eq!(read[0].style, Style::None);
    // Vollbild: page by page; the arrow key shows page 2 with the wipe.
    run(&mut h, Cmd::Fullscreen);
    assert_eq!(h.state().presentation_page(), Some(0));
    save(&mut h, "104-presentation");
    h.key_press(egui::Key::ArrowRight);
    h.step();
    h.step();
    assert_eq!(h.state().presentation_page(), Some(1));
    save(&mut h, "105-presentation-wipe");
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert_eq!(h.state().presentation_page(), None);
    assert_eq!(h.state().active_tab().unwrap().current_page, 1, "the view keeps the page");
    // Tab groups: the active document into a new group beside the other.
    run(&mut h, Cmd::View(V::MoveTabNewVertical));
    let groups: Vec<u32> = (0..2).map(|i| h.state().tab_at(i).unwrap().group).collect();
    assert_ne!(groups[0], groups[1]);
    save(&mut h, "106-tab-groups");
    run(&mut h, Cmd::View(V::MergeGroups));
    assert!((0..2).all(|i| h.state().tab_at(i).unwrap().group == 0));
    // A document in its own window, then back.
    run(&mut h, Cmd::View(V::MoveTabNewWindow));
    assert!((0..2).any(|i| h.state().tab_at(i).unwrap().window != 0));
    save(&mut h, "107-tab-window");
    run(&mut h, Cmd::View(V::MergeWindows));
    let w = h.state().active_tab().unwrap().window;
    assert!((0..2).all(|i| h.state().tab_at(i).unwrap().window == w));
    // Mehrere Tabs verschieben.
    run(&mut h, Cmd::View(V::MoveTabs));
    assert!(matches!(h.state().dialog(), Some(Dialog::MoveTabs(_))));
    save(&mut h, "108-move-tabs");
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    // A portfolio shows its file list; a PDF in it opens in a new tab.
    let files = vec![
        mrpdf::portfolio::NewFile { name: "Bericht.pdf".into(), data: common::text_pdf(2), modified: None },
        mrpdf::portfolio::NewFile { name: "Notiz.txt".into(), data: b"Hallo".to_vec(), modified: None },
    ];
    let mappe = dir.path().join("Mappe.pdf");
    std::fs::write(&mappe, mrpdf::portfolio::create(&files, "Mappe", true).unwrap()).unwrap();
    let ctx = h.ctx.clone();
    h.state_mut().open_path(&ctx, mappe, None);
    h.run_steps(6);
    assert!(h.query_by_label("Bericht.pdf").is_some(), "file list shown");
    save(&mut h, "109-portfolio");
    let before = h.state().tab_count();
    h.get_by_label("Bericht.pdf").click();
    h.run_steps(2);
    // The "Öffnen" in the row of the file list (not the toolbar's or the start page's).
    let list_y = h.get_by_label("Bericht.pdf").rect().top();
    h.get_all_by_label("Öffnen")
        .find(|n| (n.rect().top() - list_y).abs() < 80.0 && n.rect().top() < list_y)
        .unwrap()
        .click();
    h.run_steps(8);
    save(&mut h, "109b-portfolio-open");
    assert_eq!(h.state().tab_count(), before + 1, "embedded PDF opened: {:?}", h.state().dialog());
    // "PDF-Portfolio erstellen".
    run(&mut h, Cmd::View(V::NewPortfolio));
    assert!(matches!(h.state().dialog(), Some(Dialog::NewPortfolio(_))));
    save(&mut h, "110-new-portfolio");
}

#[test]
fn backstage_and_tool_at_start() {
    let _guard = common::lock();
    let settings = Settings {
        language: Language::German,
        default_tool: mrpdf::ui::tools::Tool::SelectText,
        ..Settings::default()
    };
    let mut h = harness_with(settings, Vec::new());
    h.run_steps(5);
    // "Das Dateimenü beim Start anzeigen" is on by default.
    assert!(h.state().backstage_open());
    assert_eq!(h.state().tool(), mrpdf::ui::tools::Tool::SelectText);
}
