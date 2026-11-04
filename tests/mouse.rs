//! Mouse tour: clicks every button of every ribbon tab (and every entry of
//! the drop-down menus they open) with the mouse, in a fresh window with a
//! sample document each time, and checks what happens:
//!
//! * a tool: it is used on the page (drag, click or vertices) and a new
//!   comment must appear (text asked for is typed in),
//! * a dialog: OK is clicked and no error may follow,
//! * otherwise the effect on the document is noted.
//!
//! Buttons acting outside MrPDF (printing, e-mail, files, web pages) are
//! not clicked. The report goes to `target/mouse/report.md`.
//!
//! Run: `MRPDF_MOUSE_TABS=Start,Anmerkungen cargo test --release --test mouse
//! -- --ignored --nocapture` (all tabs without the variable).

mod common;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;

use egui::accesskit::Role;
use egui::{Event, Key, Modifiers, PointerButton, Pos2, Rect, pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use mrpdf::geometry::RectF;
use mrpdf::model::{AnnotKind, AnnotStyle, Rgba};
use mrpdf::pdf_edit::NewAnnotation;
use mrpdf::ui::MrPdfApp;
use mrpdf::ui::app::Dialog;
use mrpdf::ui::i18n::Language;
use mrpdf::ui::settings::Settings;
use mrpdf::ui::tools::{Gesture, Tool};

const TABS: [&str; 12] = [
    "Start",
    "Ansicht",
    "Anmerkungen",
    "Schützen",
    "Formular",
    "Dokumentaufbau",
    "Umwandeln",
    "Teilen",
    "Überprüfen",
    "Barrierefreiheit",
    "Lesezeichen",
    "Hilfe",
];

/// Buttons that act outside MrPDF or would end the test.
const NOT_CLICKED: [&str; 20] = [
    "Drucken",
    "Alle drucken",
    "E-Mail",
    "Per E-Mail",
    "Alle Dokumente per E-Mail",
    "Formulardaten per E-Mail",
    "Ordner öffnen",
    "Handbuch",
    "Support",
    "Problem melden",
    "Nach Updates suchen",
    "Nach neuer Version suchen",
    "Startseite",
    "Homepage",
    "Dateizuordnung",
    "Vorlesen",
    "Beenden",
    "Umbenennen",
    "Verschieben in",
    "In den Papierkorb",
];

fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/mouse");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn harness(file: PathBuf) -> Harness<'static, MrPdfApp> {
    let settings = Settings {
        language: Language::German,
        author: "Tester".into(),
        backstage_at_start: false,
        ..Settings::default()
    };
    let mut h =
        Harness::builder().with_size(egui::vec2(1280.0, 820.0)).with_max_steps(64).wgpu().build_eframe(
            move |cc| {
                let mut app = MrPdfApp::with_settings(&cc.egui_ctx, settings, vec![file]);
                app.set_recovery_dir(None);
                app
            },
        );
    h.run_steps(6);
    h
}

fn input(h: &mut Harness<'_, MrPdfApp>, e: Event) {
    h.input_mut().events.push(e);
}

fn button(pos: Pos2, pressed: bool) -> Event {
    Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
}

fn click(h: &mut Harness<'_, MrPdfApp>, at: Pos2) {
    input(h, Event::PointerMoved(at));
    h.step();
    input(h, button(at, true));
    h.step();
    input(h, button(at, false));
    h.run_steps(5);
}

fn drag(h: &mut Harness<'_, MrPdfApp>, from: Pos2, to: Pos2) {
    input(h, Event::PointerMoved(from));
    h.step();
    input(h, button(from, true));
    h.step();
    for i in 1..=8 {
        input(h, Event::PointerMoved(from + (to - from) * (i as f32 / 8.0)));
        h.step();
    }
    input(h, button(to, false));
    h.run_steps(5);
}

fn key(h: &mut Harness<'_, MrPdfApp>, k: Key) {
    h.key_press(k);
    h.run_steps(3);
}

/// Vertical extent of the ribbon (below the tab row with "Datei", above
/// the document tabs), in any look.
fn ribbon_band(h: &Harness<'_, MrPdfApp>) -> (f32, f32) {
    let top = h.query_all_by_label("Datei").map(|n| n.rect().max.y).fold(f32::INFINITY, f32::min);
    let top = if top.is_finite() { top + 4.0 } else { 55.0 };
    // The document tab strip starts below the ribbon (the close buttons of
    // the tabs and panels are the first "Schließen" below it).
    let strip = h
        .query_all_by_label("Schließen")
        .map(|n| n.rect().min.y)
        .filter(|y| *y > top + 40.0)
        .fold(f32::INFINITY, f32::min);
    (top, strip.min(top + 130.0))
}

/// Enabled buttons with a name; with `ribbon` only those in the ribbon.
fn all_buttons(h: &Harness<'_, MrPdfApp>, ribbon: bool) -> Vec<(String, Rect)> {
    let (top, bottom) = ribbon_band(h);
    h.get_all_by_role(Role::Button)
        .filter(|n| !n.accesskit_node().is_disabled())
        .filter_map(|n| {
            let label = n.accesskit_node().label()?.trim().to_string();
            let r = n.rect();
            let in_ribbon = r.min.y > top && r.max.y < bottom;
            (!label.is_empty() && (!ribbon || in_ribbon)).then_some((label, r))
        })
        .collect()
}

/// A place on page 1 given as it was on the screen of the classic look
/// (1280 × 820, page at 97 %): the same spot of the page in any look.
fn at(h: &Harness<'_, MrPdfApp>, x: f32, y: f32) -> Pos2 {
    const LEFT: f32 = 257.0;
    const TOP: f32 = 218.0;
    const SCALE: f32 = 770.0 / 595.28;
    to_screen(h, (x - LEFT) / SCALE, 841.89 - (y - TOP) / SCALE)
}

fn buttons(h: &Harness<'_, MrPdfApp>) -> Vec<(String, Rect)> {
    all_buttons(h, true)
}

/// Buttons that are new compared with `before` (the entries of a menu that
/// just opened).
fn new_buttons(h: &Harness<'_, MrPdfApp>, before: &[(String, Rect)]) -> Vec<(String, Rect)> {
    all_buttons(h, false)
        .into_iter()
        .filter(|(l, r)| !before.iter().any(|(bl, br)| bl == l && br.center().distance(r.center()) < 2.0))
        .collect()
}

/// State used to see what a click did.
#[derive(Clone, PartialEq)]
struct Snap {
    revision: u64,
    pages: usize,
    comments: usize,
    tool: Tool,
    zoom: String,
    /// Hash of the window below the ribbon (pages, panels, status bar).
    picture: u64,
}

/// Hash of the rendered window below the ribbon, pointer moved away so no
/// hover effect or tooltip counts as a change.
fn picture(h: &mut Harness<'_, MrPdfApp>) -> u64 {
    use std::hash::{Hash, Hasher};
    h.event(egui::Event::PointerGone);
    h.run_steps(3);
    let Ok(img) = h.render() else { return 0 };
    let ppp = img.width() as f32 / h.ctx.content_rect().width();
    let below_ribbon = ribbon_band(h).1 + 5.0;
    let top = (below_ribbon * ppp) as usize * img.width() as usize * 4;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    img.as_raw()[top.min(img.as_raw().len())..].hash(&mut hasher);
    hasher.finish()
}

fn snap(h: &mut Harness<'_, MrPdfApp>) -> Snap {
    let picture = picture(h);
    let tool = h.state().tool();
    let tab = h.state_mut().active_tab_mut();
    let (revision, pages, comments, zoom) = match tab {
        Some(t) => {
            let comments = (0..t.doc.page_count()).map(|p| t.annotations(p).len()).sum();
            (t.doc.revision(), t.doc.page_count(), comments, format!("{:.3}", t.zoom))
        }
        None => (0, 0, 0, String::new()),
    };
    Snap { revision, pages, comments, tool, zoom, picture }
}

fn dialog_name(h: &Harness<'_, MrPdfApp>) -> Option<String> {
    let d = h.state().dialog()?;
    let text = format!("{d:?}");
    Some(text.split([' ', '{', '(']).next().unwrap_or("").to_string())
}

fn error_text(h: &Harness<'_, MrPdfApp>) -> Option<String> {
    match h.state().dialog() {
        Some(Dialog::Message { title, text }) if title == "Fehler" => Some(text.replace('\n', " ")),
        _ => None,
    }
}

/// Uses the active tool on the page; returns a description.
fn use_tool(h: &mut Harness<'_, MrPdfApp>, tool: Tool) -> String {
    let before = snap(h);
    match tool.gesture() {
        // Over the first text line of the sample page.
        Gesture::TextMarkup => {
            let (a, b) = (at(h, 352.0, 392.0), at(h, 612.0, 392.0));
            drag(h, a, b)
        }
        Gesture::Rect | Gesture::Line | Gesture::Freehand => {
            let (a, b) = (at(h, 450.0, 600.0), at(h, 650.0, 700.0));
            drag(h, a, b)
        }
        Gesture::Click => {
            let p = at(h, 700.0, 650.0);
            click(h, p)
        }
        Gesture::Vertices => {
            let corners = [at(h, 450.0, 600.0), at(h, 650.0, 600.0), at(h, 550.0, 710.0)];
            for p in corners {
                click(h, p);
            }
            key(h, Key::Enter);
        }
        Gesture::None => return "Werkzeug ohne Zeichnen".into(),
    }
    // Text asked for: type it and confirm.
    if matches!(h.state().dialog(), Some(Dialog::TextInput { .. })) {
        input(h, Event::Text("Maustest".into()));
        h.run_steps(2);
        if let Some(ok) = h.query_all_by_label("OK").last() {
            ok.click();
        }
        h.run_steps(5);
    }
    let after = snap(h);
    if let Some(e) = error_text(h) {
        return format!("**Fehler beim Zeichnen**: {e}");
    }
    if after.comments > before.comments {
        "zeichnet ok".into()
    } else if after.revision != before.revision {
        "Dokument geändert".into()
    } else if let Some(d) = dialog_name(h) {
        format!("öffnet {d}")
    } else {
        "**nichts entstanden**".into()
    }
}

/// What a click on `target` did; may continue with OK or a tool use.
fn outcome(h: &mut Harness<'_, MrPdfApp>, before: &Snap, shots: &str) -> String {
    if let Some(e) = error_text(h) {
        return format!("**Fehlermeldung**: {e}");
    }
    if let Some(d) = dialog_name(h) {
        if let Ok(img) = h.render() {
            let _ = img.save(out_dir().join(format!("{shots}-dialog.png")));
        }
        if d == "Print" {
            return "Dialog Print (Drucken nicht geklickt)".into();
        }
        let oks: Vec<Rect> = h
            .get_all_by_role(Role::Button)
            .filter(|n| n.accesskit_node().label().as_deref() == Some("OK"))
            .map(|n| (n.rect(), n.accesskit_node().is_disabled()))
            .filter(|(_, disabled)| !disabled)
            .map(|(r, _)| r)
            .collect();
        let Some(ok) = oks.last().copied() else { return format!("Dialog {d} (kein OK frei)") };
        let mid = snap(h);
        click(h, ok.center());
        h.run_steps(5);
        let after = snap(h);
        if let Some(e) = error_text(h) {
            return format!("Dialog {d} → OK → **Fehlermeldung**: {e}");
        }
        let still = dialog_name(h);
        let effect = if after.revision != mid.revision {
            "Dokument geändert"
        } else if still.as_deref() == Some(d.as_str()) {
            "**Dialog bleibt offen**"
        } else {
            "ohne Änderung"
        };
        return format!("Dialog {d} → OK → {effect}");
    }
    let now = snap(h);
    if now.tool != before.tool {
        let tool = now.tool;
        // Tools whose result is not a comment.
        match tool {
            Tool::Snapshot
            | Tool::SelectText
            | Tool::SelectAnnot
            | Tool::SelectFields
            | Tool::EditText
            | Tool::EditObjects
            | Tool::Hand
            | Tool::Loupe
            | Tool::DynamicZoom
            | Tool::Eraser => {
                return format!("Werkzeug {tool:?} gewählt");
            }
            Tool::Zoom => {
                let p = at(h, 640.0, 450.0);
                click(h, p);
                let changed = snap(h).zoom != now.zoom;
                return format!(
                    "Werkzeug Zoom: {}",
                    if changed { "vergrößert" } else { "**vergrößert nicht**" }
                );
            }
            _ => {}
        }
        let used = use_tool(h, tool);
        if let Ok(img) = h.render() {
            let _ = img.save(out_dir().join(format!("{shots}-tool.png")));
        }
        return format!("Werkzeug {tool:?}: {used}");
    }
    if now.revision != before.revision {
        format!(
            "Dokument geändert (Seiten {} → {}, Kommentare {} → {})",
            before.pages, now.pages, before.comments, now.comments
        )
    } else if now.zoom != before.zoom {
        "Ansicht geändert".into()
    } else if now.picture != before.picture {
        "Anzeige geändert".into()
    } else {
        "keine sichtbare Wirkung".into()
    }
}

fn skipped(label: &str) -> bool {
    NOT_CLICKED.iter().any(|s| label.starts_with(s))
}

/// A fresh window with the sample; a selected comment for the comment
/// commands.
fn fresh(dir: &std::path::Path, n: usize) -> Harness<'static, MrPdfApp> {
    let file = dir.join(format!("Beispiel{n}.pdf"));
    std::fs::write(&file, common::text_pdf(3)).unwrap();
    let mut h = harness(file);
    let tab = h.state_mut().active_tab_mut().expect("document open");
    let style = AnnotStyle { color: Rgba::RED, width: 2.0, author: "Tester".into(), ..AnnotStyle::default() };
    let mut square = NewAnnotation::new(AnnotKind::Square, style);
    square.rect = RectF::new(100.0, 100.0, 200.0, 180.0);
    tab.doc.add_annotation(0, &square).unwrap();
    h.run_steps(3);
    h
}

fn open_tab(h: &mut Harness<'_, MrPdfApp>, tab: &str) {
    if let Some(n) = h.get_all_by_label(tab).next() {
        n.click();
    }
    h.run_steps(4);
}

#[test]
#[ignore = "long; run explicitly"]
fn mouse_tour() {
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let wanted: Vec<String> = std::env::var("MRPDF_MOUSE_TABS")
        .map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default();
    let mut lines =
        vec!["| Reiter | Knopf | Eintrag | Ergebnis |".to_string(), "|---|---|---|---|".to_string()];
    let mut problems = 0;
    let mut n = 0;
    for tab in TABS.iter().filter(|t| wanted.is_empty() || wanted.iter().any(|w| w == *t)) {
        let mut h = fresh(dir.path(), n);
        open_tab(&mut h, tab);
        let list = buttons(&h);
        drop(h);
        for (i, (label, _)) in list.iter().enumerate() {
            if skipped(label) {
                lines.push(format!("| {tab} | {label} | | nicht geklickt (wirkt außerhalb) |"));
                continue;
            }
            // The button itself.
            n += 1;
            let mut h = fresh(dir.path(), n);
            open_tab(&mut h, tab);
            let Some((_, rect)) = buttons(&h).get(i).cloned() else { continue };
            let before = snap(&mut h);
            let base = all_buttons(&h, false);
            let shots = format!("{}-{i:02}", tab.to_lowercase());
            let result = catch_unwind(AssertUnwindSafe(|| {
                click(&mut h, rect.center());
                let menu = new_buttons(&h, &base);
                let opened_menu =
                    egui::Popup::is_any_open(&h.ctx) && !menu.is_empty() && h.state().dialog().is_none();
                if opened_menu {
                    if let Ok(img) = h.render() {
                        let _ = img.save(out_dir().join(format!("{shots}-menu.png")));
                    }
                    (format!("Menü mit {} Einträgen", menu.len()), menu.into_iter().map(|(l, _)| l).collect())
                } else {
                    (outcome(&mut h, &before, &shots), Vec::new())
                }
            }));
            let (text, entries) = match result {
                Ok(r) => r,
                Err(_) => ("**ABSTURZ**".to_string(), Vec::new()),
            };
            if text.contains("**") {
                problems += 1;
            }
            println!("{tab} / {label}: {text}");
            lines.push(format!("| {tab} | {label} | | {} |", text.replace('|', "/")));
            // Every entry of the drop-down menu.
            for (j, entry) in entries.iter().enumerate() {
                if skipped(entry) {
                    lines.push(format!("| {tab} | {label} | {entry} | nicht geklickt (wirkt außerhalb) |"));
                    continue;
                }
                n += 1;
                let mut h = fresh(dir.path(), n);
                open_tab(&mut h, tab);
                let Some((_, rect)) = buttons(&h).get(i).cloned() else { continue };
                let base = all_buttons(&h, false);
                // Picture before the menu opens; the open menu covers the page.
                let pic = picture(&mut h);
                click(&mut h, rect.center());
                let Some((_, item)) = new_buttons(&h, &base).get(j).cloned() else {
                    lines.push(format!("| {tab} | {label} | {entry} | Eintrag nicht wiedergefunden |"));
                    continue;
                };
                let before = Snap { picture: pic, ..snap(&mut h) };
                let shots = format!("{}-{i:02}-{j:02}", tab.to_lowercase());
                let result = catch_unwind(AssertUnwindSafe(|| {
                    click(&mut h, item.center());
                    outcome(&mut h, &before, &shots)
                }));
                let text = result.unwrap_or_else(|_| "**ABSTURZ**".to_string());
                if text.contains("**") {
                    problems += 1;
                }
                println!("{tab} / {label} / {entry}: {text}");
                lines.push(format!("| {tab} | {label} | {entry} | {} |", text.replace('|', "/")));
            }
        }
    }
    let head = format!("# Maus-Rundgang\n\n{n} Klickfolgen, {problems} auffällig (fett markiert).\n\n");
    std::fs::write(out_dir().join("report.md"), head + &lines.join("\n") + "\n").unwrap();
    println!("Klickfolgen: {n}, auffällig: {problems}");
}

/// Screen position of a PDF point of page 0.
fn to_screen(h: &Harness<'_, MrPdfApp>, x: f32, y: f32) -> Pos2 {
    let tab = h.state().active_tab().unwrap();
    let slot = tab.layout.slots.iter().find(|s| s.page == 0).unwrap();
    let g = tab.display_geometry(0).unwrap();
    let v = g.pdf_to_view(mrpdf::geometry::PointF::new(x, y));
    slot.rect.min + tab.canvas_origin.to_vec2() + egui::vec2(v.x, v.y) * tab.scale()
}

fn first_rect(h: &mut Harness<'_, MrPdfApp>) -> Option<RectF> {
    let tab = h.state_mut().active_tab_mut()?;
    tab.annotations(0).first().map(|a| a.rect.normalized())
}

/// Polygon tools ("Wolke", "Vieleck", "Streckenzug") drawn the way people
/// click: a slow double click at the end, clicks that slip by a few pixels,
/// a click back on the first corner.
#[test]
#[ignore = "long; run explicitly"]
fn polygon_gestures() {
    use mrpdf::ui::app::commands::Cmd;
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let mut failed = Vec::new();
    let mut n = 0;
    for tool in [Tool::Cloud, Tool::Polygon, Tool::Polyline] {
        for way in ["langsamer Doppelklick", "verrutschte Klicks", "Klick auf ersten Punkt"] {
            if way == "Klick auf ersten Punkt" && tool == Tool::Polyline {
                continue;
            }
            n += 1;
            let file = dir.path().join(format!("P{n}.pdf"));
            std::fs::write(&file, common::text_pdf(1)).unwrap();
            let mut h = harness(file);
            let ctx = h.ctx.clone();
            h.state_mut().run(&ctx, Cmd::Tool(tool));
            h.run_steps(3);
            let corners = [at(&h, 450.0, 600.0), at(&h, 650.0, 600.0), at(&h, 550.0, 710.0)];
            for p in corners {
                if way == "verrutschte Klicks" {
                    drag(&mut h, p, p + egui::vec2(9.0, 7.0));
                } else {
                    click(&mut h, p);
                }
                h.run_steps(30);
            }
            match way {
                "Klick auf ersten Punkt" => click(&mut h, corners[0] + egui::vec2(2.0, 1.0)),
                _ => {
                    // Second click of a slow double click on the last corner.
                    let last = if way == "verrutschte Klicks" {
                        corners[2] + egui::vec2(9.0, 7.0)
                    } else {
                        corners[2]
                    };
                    click(&mut h, last);
                }
            }
            h.run_steps(5);
            let count = h.state_mut().active_tab_mut().unwrap().annotations(0).len();
            let ok = count == 1;
            println!("{tool:?} / {way}: {count} Anmerkung(en)");
            if let Ok(img) = h.render() {
                let _ = img.save(out_dir().join(format!("polygon-{n}.png")));
            }
            if !ok {
                failed.push(format!("{tool:?} / {way}"));
            }
        }
    }
    assert!(failed.is_empty(), "nicht gezeichnet: {failed:?}");
}

/// Background colour on a page that covers its background (first real test
/// file, `MRPDF_TESTFILES`): the dialog switches to "Über den Seiteninhalt
/// legen" by itself and OK colours the white parts of the page.
#[test]
#[ignore = "needs MRPDF_TESTFILES"]
fn background_on_covered_page() {
    use mrpdf::ui::app::commands::Cmd;
    let Some(dir) = std::env::var_os("MRPDF_TESTFILES") else { return };
    let file = PathBuf::from(dir).join("test1.pdf");
    let _guard = common::lock();
    let mut h = harness(file);
    let ctx = h.ctx.clone();
    let before = h.state_mut().active_tab_mut().unwrap().doc.render_page(0, 100, 140).unwrap();
    h.state_mut().run(&ctx, Cmd::Background);
    h.run_steps(10);
    let check = h
        .get_all_by_label("Über den Seiteninhalt legen (multiplizieren)")
        .next()
        .map(|n| n.accesskit_node().toggled());
    println!("Schalter: {check:?}");
    if let Ok(img) = h.render() {
        let _ = img.save(out_dir().join("covered-dialog.png"));
    }
    assert_eq!(check, Some(Some(egui::accesskit::Toggled::True)), "Schalter nicht von selbst an");
    let ok = h.get_all_by_label("OK").last().map(|n| n.rect().center()).unwrap();
    click(&mut h, ok);
    h.run_steps(10);
    let tab = h.state_mut().active_tab_mut().unwrap();
    let img = tab.doc.render_page(0, 100, 140).unwrap();
    // White pixels that took the (light) colour.
    let white = |p: &[u8]| p[0] > 245 && p[1] > 245 && p[2] > 245;
    let was_white = before.rgba.chunks(4).filter(|p| white(p)).count();
    let tinted = before.rgba.chunks(4).zip(img.rgba.chunks(4)).filter(|(a, b)| white(a) && a != b).count();
    println!("eingefärbt: {tinted} von {was_white} weißen Pixeln");
    assert!(tinted * 10 > was_white * 9, "Seite nicht eingefärbt");
}

/// "Dokumente vergleichen" with the mouse: dialog, OK, both documents side
/// by side with the differences marked, "Nächster", "Vergleich beenden".
/// Screenshots in target/mouse/compare-*.png.
#[test]
#[ignore = "long; run explicitly"]
fn compare_documents() {
    use mrpdf::compare::DiffKind;
    use mrpdf::ui::app::commands::Cmd;
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("Vertrag-alt.pdf");
    let new = dir.path().join("Vertrag-neu.pdf");
    std::fs::write(
        &old,
        common::lines_pdf(
            &[
                &[
                    "Vertrag über Leistungen",
                    "Der Preis beträgt 100 Euro pro Monat.",
                    "Laufzeit zwölf Monate.",
                ],
                &["Seite zwei bleibt gleich.", "Mit einem zweiten Satz."],
            ],
            None,
        ),
    )
    .unwrap();
    std::fs::write(
        &new,
        common::lines_pdf(
            &[
                &["Vertrag über Leistungen", "Der Preis beträgt 120 Euro pro Monat inklusive Steuer."],
                &["Eine ganz neue Seite mit Anlagen."],
                &["Seite zwei bleibt gleich.", "Mit einem zweiten Satz."],
            ],
            Some(2),
        ),
    )
    .unwrap();
    // The new document first, then the old one: the old one is active.
    let mut h = harness(new);
    let ctx = h.ctx.clone();
    h.state_mut().open_path(&ctx, old, None);
    h.run_steps(5);
    h.state_mut().run(&ctx, Cmd::CompareDocuments);
    h.run_steps(5);
    if let Ok(img) = h.render() {
        let _ = img.save(out_dir().join("compare-1-dialog.png"));
    }
    assert!(matches!(h.state().dialog(), Some(Dialog::Compare(_))), "Dialog fehlt");
    let ok = h.get_all_by_label("OK").last().map(|n| n.rect().center()).expect("OK");
    click(&mut h, ok);
    for _ in 0..200 {
        h.step();
        if h.state().comparison().is_some_and(|c| !c.running()) {
            break;
        }
    }
    h.run_steps(5);
    let c = h.state().comparison().expect("kein Vergleich");
    assert!(!c.running(), "Berechnung endet nicht");
    let diffs = c.diffs.clone();
    let kinds: Vec<DiffKind> = diffs.iter().map(|d| d.kind).collect();
    for d in &diffs {
        println!("{:?}: {:?} | {:?} → {:?}", d.kind, d.pages(), d.old_text, d.new_text);
    }
    if let Ok(img) = h.render() {
        let _ = img.save(out_dir().join("compare-2-result.png"));
    }
    let mut failed = Vec::new();
    let has = |k: DiffKind, old: &str, new: &str| {
        diffs.iter().any(|d| d.kind == k && d.old_text.contains(old) && d.new_text.contains(new))
    };
    if !has(DiffKind::Changed, "100", "120") {
        failed.push("100 → 120 fehlt");
    }
    if !has(DiffKind::Inserted, "", "Steuer") && !has(DiffKind::Changed, "", "Steuer") {
        failed.push("„inklusive Steuer“ fehlt");
    }
    if !has(DiffKind::Deleted, "Laufzeit", "") {
        failed.push("„Laufzeit zwölf Monate“ nicht als gelöscht");
    }
    // The text of the new page is part of "Seite eingefügt", not a change.
    if diffs.iter().any(|d| d.new_text.contains("Anlagen")) {
        failed.push("Text der neuen Seite einzeln aufgeführt");
    }
    if !kinds.contains(&DiffKind::PageInserted) {
        failed.push("eingefügte Seite fehlt");
    }
    if !kinds.contains(&DiffKind::Graphic) {
        failed.push("Grafik fehlt");
    }
    let marked = h.state().active_tab().is_some_and(|t| !t.compare_marks.is_empty());
    if !marked {
        failed.push("keine Markierungen im Dokument");
    }
    // Scrolling the old document (left) takes the new one along.
    let before = h.state().compared_scroll();
    let p = at(&h, 420.0, 500.0);
    input(&mut h, Event::PointerMoved(p));
    h.step();
    input(
        &mut h,
        Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -400.0),
            modifiers: Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        },
    );
    h.run_steps(30);
    let after = h.state().compared_scroll();
    println!("Blättern: vorher {before:?}, nachher {after:?}");
    match (before, after) {
        (Some(b), Some(a)) if a.0 > b.0 + 50.0 && a.1 > b.1 + 50.0 => {}
        _ => failed.push("rechtes Dokument blättert nicht mit"),
    }
    // "Nächster" selects the first difference.
    let next = h.get_all_by_label("Nächster ▶").next().map(|n| n.rect().center());
    if let Some(p) = next {
        click(&mut h, p);
        h.run_steps(5);
    }
    let selected = h.state().comparison().and_then(|c| c.selected);
    println!("ausgewählt: {selected:?}");
    if selected != Some(0) {
        failed.push("„Nächster“ wählt nichts");
    }
    if let Ok(img) = h.render() {
        let _ = img.save(out_dir().join("compare-3-selected.png"));
    }
    let end = h.get_all_by_label("Vergleich beenden").next().map(|n| n.rect().center());
    if let Some(p) = end {
        click(&mut h, p);
        h.run_steps(5);
    }
    if h.state().comparison().is_some() || h.state().active_tab().is_some_and(|t| !t.compare_marks.is_empty())
    {
        failed.push("„Vergleich beenden“ räumt nicht auf");
    }
    assert!(failed.is_empty(), "{failed:?}");
}

/// "Dokumente vergleichen" with the real test files (`MRPDF_TESTFILES`):
/// each file against a copy without its second page. Exactly one
/// difference ("Seite gelöscht") may come out; the time is printed.
#[test]
#[ignore = "needs MRPDF_TESTFILES"]
fn compare_real_files() {
    use mrpdf::compare::DiffKind;
    use mrpdf::ui::app::commands::Cmd;
    let Some(dir) = std::env::var_os("MRPDF_TESTFILES") else { return };
    let _guard = common::lock();
    let tmp = tempfile::tempdir().unwrap();
    let mut files: Vec<PathBuf> =
        std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok().map(|e| e.path())).collect();
    files.sort();
    let mut failed = Vec::new();
    for path in files.into_iter().filter(|p| p.extension().is_some_and(|e| e == "pdf")) {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let mut doc = mrpdf::Document::open(&path, None).unwrap();
        let pages = doc.page_count();
        if pages < 3 {
            continue;
        }
        doc.delete_pages(&[1]).unwrap();
        let copy = tmp.path().join(format!("ohne-seite-2-{name}"));
        std::fs::write(&copy, doc.to_bytes().unwrap()).unwrap();
        drop(doc);
        // The copy first, then the original: the original is the old one.
        let mut h = harness(copy);
        let ctx = h.ctx.clone();
        h.state_mut().open_path(&ctx, path.clone(), None);
        h.run_steps(5);
        h.state_mut().run(&ctx, Cmd::CompareDocuments);
        h.run_steps(5);
        let ok = h.get_all_by_label("OK").last().map(|n| n.rect().center()).unwrap();
        let t = std::time::Instant::now();
        click(&mut h, ok);
        let mut steps = 0;
        while h.state().comparison().is_none_or(|c| c.running()) && steps < 100_000 {
            h.step();
            steps += 1;
        }
        let secs = t.elapsed().as_secs_f32();
        let c = h.state().comparison().unwrap();
        let kinds: Vec<DiffKind> = c.diffs.iter().map(|d| d.kind).collect();
        println!("{name}: {pages} Seiten, {secs:.1} s, Unterschiede: {kinds:?}");
        for d in c.diffs.iter().take(5) {
            println!("   {:?} {:?}", d.kind, d.pages());
        }
        if kinds != [DiffKind::PageDeleted] || c.diffs[0].pages() != (Some(1), None) {
            failed.push(format!("{name}: {kinds:?}"));
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// Hash of the pixels in `r` (screen points).
fn region_hash(h: &mut Harness<'_, MrPdfApp>, r: Rect) -> u64 {
    use std::hash::{Hash, Hasher};
    h.event(egui::Event::PointerGone);
    h.run_steps(3);
    let Ok(img) = h.render() else { return 0 };
    let ppp = img.width() as f32 / h.ctx.content_rect().width();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let (x0, x1) = ((r.min.x * ppp) as u32, ((r.max.x * ppp) as u32).min(img.width()));
    let (y0, y1) = ((r.min.y * ppp) as u32, ((r.max.y * ppp) as u32).min(img.height()));
    for y in y0..y1 {
        for x in x0..x1 {
            img.get_pixel(x, y).0.hash(&mut hasher);
        }
    }
    hasher.finish()
}

/// Screen area of the dialog preview: below the "Vorschau …" heading.
fn preview_rect(h: &Harness<'_, MrPdfApp>) -> Rect {
    let text = |n: &egui_kittest::Node<'_>| {
        let a = n.accesskit_node();
        a.label().or_else(|| a.value()).unwrap_or_default().to_string()
    };
    let label = h
        .get_all_by_role(Role::Label)
        .find(|n| text(n).starts_with("Vorschau"))
        .map(|n| n.rect())
        .unwrap_or_else(|| {
            let all: Vec<String> = h.get_all_by_role(Role::Label).map(|n| text(&n)).collect();
            panic!("Vorschau-Überschrift fehlt; Beschriftungen: {all:?}")
        });
    Rect::from_min_size(pos2(label.min.x, label.max.y + 6.0), egui::vec2(760.0, 120.0))
}

/// Dialog previews follow the settings and can be zoomed: background
/// colour, header text, mouse wheel, double click. Screenshots in
/// target/mouse/preview-*.png.
#[test]
#[ignore = "long; run explicitly"]
fn dialog_previews() {
    use mrpdf::ui::app::commands::Cmd;
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let mut failed = Vec::new();
    let shot = |h: &mut Harness<'_, MrPdfApp>, name: &str| {
        if let Ok(img) = h.render() {
            let _ = img.save(out_dir().join(format!("preview-{name}.png")));
        }
    };

    // Header and footer: typed text appears in the preview.
    let file = dir.path().join("K.pdf");
    std::fs::write(&file, common::text_pdf(2)).unwrap();
    let mut h = harness(file);
    let ctx = h.ctx.clone();
    h.state_mut().run(&ctx, Cmd::HeaderFooter);
    h.run_steps(5);
    let area = preview_rect(&h);
    let before = region_hash(&mut h, area);
    // The first text field of the dialog (not "Schnellstart" at the top).
    let field = h
        .get_all_by_role(Role::MultilineTextInput)
        .chain(h.get_all_by_role(Role::TextInput))
        .map(|n| n.rect().center())
        .filter(|p| p.y > 100.0 && p.y < 780.0)
        .min_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)));
    if let Some(f) = field {
        click(&mut h, f);
        input(&mut h, Event::Text("Kopfzeile Maustest".into()));
        h.run_steps(8);
    }
    shot(&mut h, "header");
    let after = region_hash(&mut h, area);
    println!("Kopf-/Fußzeile: Textfeld {field:?}, Vorschau geändert: {}", before != after);
    if field.is_none() || before == after {
        failed.push("Kopf-/Fußzeile: Vorschau folgt dem Text nicht");
    }
    // Zoom: wheel over the preview, then double click back to the page.
    let centre = area.min + egui::vec2(120.0, 60.0);
    input(&mut h, Event::PointerMoved(centre));
    h.step();
    input(
        &mut h,
        Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, 240.0),
            modifiers: Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        },
    );
    // egui spreads the wheel over several frames.
    h.run_steps(60);
    shot(&mut h, "zoomed");
    let zoomed = region_hash(&mut h, area);
    println!("Zoom: Vorschau geändert: {}", zoomed != after);
    if zoomed == after {
        failed.push("Vorschau: Mausrad zoomt nicht");
    }
    input(&mut h, Event::PointerMoved(centre));
    h.step();
    // Both clicks in one frame: a test step lasts a quarter second, longer
    // than egui allows between the clicks of a double click.
    for _ in 0..2 {
        input(&mut h, button(centre, true));
        input(&mut h, button(centre, false));
    }
    h.step();
    h.run_steps(8);
    shot(&mut h, "dblclick");
    let back = region_hash(&mut h, area);
    println!("Doppelklick: wieder eingepasst: {}", back == after);
    if back != after {
        failed.push("Vorschau: Doppelklick passt nicht wieder ein");
    }
    drop(h);

    // Watermark: typed text appears in the preview.
    let file = dir.path().join("W.pdf");
    std::fs::write(&file, common::text_pdf(1)).unwrap();
    let mut h = harness(file);
    let ctx = h.ctx.clone();
    h.state_mut().run(&ctx, Cmd::Watermark);
    h.run_steps(5);
    let area = preview_rect(&h);
    let before = region_hash(&mut h, area);
    let field = h
        .get_all_by_role(Role::MultilineTextInput)
        .chain(h.get_all_by_role(Role::TextInput))
        .map(|n| n.rect().center())
        .filter(|p| p.y > 100.0 && p.y < 780.0)
        .min_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)));
    if let Some(f) = field {
        click(&mut h, f);
        input(&mut h, Event::Text("ENTWURF".into()));
        h.run_steps(8);
    }
    shot(&mut h, "watermark");
    let after = region_hash(&mut h, area);
    println!("Wasserzeichen: Vorschau geändert: {}", before != after);
    if before == after {
        failed.push("Wasserzeichen: Vorschau folgt dem Text nicht");
    }
    drop(h);

    // Background colour: the picked colour appears in the preview.
    let file = dir.path().join("B.pdf");
    std::fs::write(&file, common::text_pdf(1)).unwrap();
    let mut h = harness(file);
    let ctx = h.ctx.clone();
    h.state_mut().run(&ctx, Cmd::Background);
    h.run_steps(5);
    let area = preview_rect(&h);
    let before = region_hash(&mut h, area);
    // The colour button sits right of the "Farbe:" radio button.
    let radio = h.get_all_by_label("Farbe:").next().map(|n| n.rect()).expect("Farbe:");
    let button = h
        .get_all_by_role(Role::Button)
        .map(|n| n.rect())
        .filter(|r| (r.center().y - radio.center().y).abs() < 8.0 && r.min.x > radio.max.x - 2.0)
        .min_by(|a, b| a.min.x.total_cmp(&b.min.x));
    let at = button.map_or(radio.right_center() + egui::vec2(20.0, 0.0), |r| r.center());
    click(&mut h, at);
    h.run_steps(5);
    // Into the saturation/value square of the picker, below the button.
    click(&mut h, at + egui::vec2(120.0, 200.0));
    h.run_steps(5);
    let rev = h.state_mut().active_tab_mut().unwrap().doc.revision();
    // A click outside (on the dialog title) closes the picker.
    let title = h.get_all_by_label("Hintergrund hinzufügen").next().map(|n| n.rect().center()).unwrap();
    click(&mut h, title);
    h.run_steps(5);
    shot(&mut h, "background");
    let after = region_hash(&mut h, area);
    println!("Hintergrund: Vorschau geändert: {}", before != after);
    if before == after {
        failed.push("Hintergrund: Vorschau folgt der Farbe nicht");
    }
    let ok = h.get_all_by_label("OK").last().map(|n| n.rect().center());
    if let Some(ok) = ok {
        click(&mut h, ok);
    }
    h.run_steps(8);
    let changed = h.state_mut().active_tab_mut().unwrap().doc.revision() != rev;
    println!("Hintergrund: Dokument geändert: {changed}");
    if !changed {
        failed.push("Hintergrund: OK ändert das Dokument nicht");
    }
    assert!(failed.is_empty(), "{failed:?}");
}

/// The tools that need text under the mouse: "Textkorrektur" (select,
/// then Entf strikes out, typing replaces) and "Inhalt hervorheben" (click
/// on text).
#[test]
#[ignore = "long; run explicitly"]
fn text_tools() {
    use mrpdf::ui::app::commands::Cmd;
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let count = |h: &mut Harness<'_, MrPdfApp>| h.state_mut().active_tab_mut().unwrap().annotations(0).len();
    let cases: [(&str, Tool, Option<Event>); 3] = [
        (
            "Textkorrektur + Entf",
            Tool::TextCorrection,
            Some(Event::Key {
                key: Key::Delete,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            }),
        ),
        ("Textkorrektur + Tippen", Tool::TextCorrection, Some(Event::Text("Neu".into()))),
        ("Inhalt hervorheben", Tool::ContentHighlight, None),
    ];
    let mut failed = Vec::new();
    for (n, (name, tool, then)) in cases.into_iter().enumerate() {
        let file = dir.path().join(format!("T{n}.pdf"));
        std::fs::write(&file, common::text_pdf(1)).unwrap();
        let mut h = harness(file);
        let ctx = h.ctx.clone();
        h.state_mut().run(&ctx, Cmd::Tool(tool));
        h.run_steps(3);
        let before = count(&mut h);
        let line = (at(&h, 352.0, 392.0), at(&h, 612.0, 392.0));
        match then {
            Some(e) => {
                drag(&mut h, line.0, line.1);
                input(&mut h, e);
                h.run_steps(5);
                // Typing opens the replacement text, prefilled: confirm it.
                if matches!(h.state().dialog(), Some(Dialog::TextInput { .. })) {
                    if let Some(ok) = h.query_all_by_label("OK").last() {
                        ok.click();
                    }
                    h.run_steps(5);
                }
            }
            None => {
                let p = at(&h, 450.0, 392.0);
                click(&mut h, p)
            }
        }
        let after = count(&mut h);
        let kinds: Vec<String> = h
            .state_mut()
            .active_tab_mut()
            .unwrap()
            .annotations(0)
            .iter()
            .map(|a| format!("{:?}", a.kind))
            .collect();
        println!("{name}: {before} → {after} {kinds:?}");
        if let Ok(img) = h.render() {
            let _ = img.save(out_dir().join(format!("text-{n}.png")));
        }
        if after <= before {
            failed.push(name);
        }
    }
    assert!(failed.is_empty(), "nichts entstanden: {failed:?}");
}

/// For every comment tool: create, select by click, move, resize at the
/// corner handle, delete with Entf, undo. Report in target/mouse/handling.md.
#[test]
#[ignore = "long; run explicitly"]
fn annotation_handling() {
    use mrpdf::ui::app::commands::Cmd;
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let tools = [
        Tool::Typewriter,
        Tool::TextBox,
        Tool::Callout,
        Tool::Note,
        Tool::Highlight,
        Tool::AreaHighlight,
        Tool::Underline,
        Tool::StrikeOut,
        Tool::Squiggly,
        Tool::Caret,
        Tool::Replace,
        Tool::Line,
        Tool::Arrow,
        Tool::Ellipse,
        Tool::Rectangle,
        Tool::Cloud,
        Tool::Polyline,
        Tool::Polygon,
        Tool::Pencil,
        Tool::Stamp,
        Tool::MeasureDistance,
        Tool::MeasurePerimeter,
        Tool::MeasureArea,
    ];
    let mut lines = vec![
        "| Werkzeug | anlegen | anklicken | verschieben | Größe ändern | Entf | Strg+Z |".to_string(),
        "|---|---|---|---|---|---|---|".to_string(),
    ];
    for (n, tool) in tools.iter().enumerate() {
        let file = dir.path().join(format!("H{n}.pdf"));
        std::fs::write(&file, common::text_pdf(1)).unwrap();
        let mut h = harness(file);
        let ctx = h.ctx.clone();
        h.state_mut().run(&ctx, Cmd::Tool(*tool));
        h.run_steps(3);
        let created = use_tool(&mut h, *tool);
        let Some(r0) = first_rect(&mut h) else {
            lines.push(format!("| {tool:?} | **{created}** | | | | | |"));
            continue;
        };
        let ok = |b: bool| if b { "ok" } else { "**nein**" };
        // Select: "Anmerkungen bearbeiten", click on the comment's centre,
        // then on its left edge (unfilled shapes are hit at their outline).
        h.state_mut().run(&ctx, Cmd::Tool(Tool::SelectAnnot));
        h.run_steps(3);
        let centre = to_screen(&h, (r0.x0 + r0.x1) / 2.0, (r0.y0 + r0.y1) / 2.0);
        let edge = to_screen(&h, r0.x0 + 1.0, (r0.y0 + r0.y1) / 2.0);
        let mut grip = centre;
        for p in [centre, edge] {
            click(&mut h, p);
            if h.state().active_tab().unwrap().selected_annot.is_some() {
                grip = p;
                break;
            }
        }
        let selected = h.state().active_tab().unwrap().selected_annot.is_some();
        // Move by 40/30 screen points.
        drag(&mut h, grip, grip + egui::vec2(40.0, 30.0));
        let r1 = first_rect(&mut h).unwrap_or(r0);
        let moved = (r1.x0 - r0.x0).abs() > 10.0 && (r1.y0 - r0.y0).abs() > 10.0;
        // Handles: the lower right corner of comments with an area, the
        // first point of lines and polygons; none for text markup and notes.
        let p = to_screen(&h, (r1.x0 + r1.x1) / 2.0, (r1.y0 + r1.y1) / 2.0);
        // Select again where the comment is now (centre, else its edge).
        click(&mut h, p);
        if h.state().active_tab().unwrap().selected_annot.is_none() {
            let e = to_screen(&h, r1.x0 + 1.0, (r1.y0 + r1.y1) / 2.0);
            click(&mut h, e);
        }
        let kind = h.state_mut().active_tab_mut().unwrap().annotations(0)[0].kind;
        let text_before = h.state_mut().active_tab_mut().unwrap().annotations(0)[0].contents.clone();
        let points =
            h.state_mut().active_tab_mut().unwrap().selected_details().map(|d| d.points).unwrap_or_default();
        let size_text = match kind {
            AnnotKind::Square
            | AnnotKind::Circle
            | AnnotKind::FreeText
            | AnnotKind::Stamp
            | AnnotKind::Ink => {
                let corner = to_screen(&h, r1.x1, r1.y0);
                drag(&mut h, corner, corner + egui::vec2(50.0, 40.0));
                let r2 = first_rect(&mut h).unwrap_or(r1);
                if r2.width() > r1.width() + 10.0
                    && r2.height() > r1.height() + 10.0
                    && (r2.x0 - r1.x0).abs() < 2.0
                {
                    "ok (Ecke)".to_string()
                } else {
                    format!("**nein** {r1:?} → {r2:?}")
                }
            }
            AnnotKind::Line | AnnotKind::Polygon | AnnotKind::Polyline => match points.first() {
                Some(p0) => {
                    let at = to_screen(&h, p0.x, p0.y);
                    drag(&mut h, at, at + egui::vec2(-40.0, 30.0));
                    let after = h
                        .state_mut()
                        .active_tab_mut()
                        .unwrap()
                        .selected_details()
                        .map(|d| d.points)
                        .unwrap_or_default();
                    let moved = after.first().is_some_and(|q| (q.x - p0.x).abs() > 10.0);
                    let others_kept =
                        after.iter().skip(1).zip(points.iter().skip(1)).all(|(a, b)| a.distance(*b) < 0.5);
                    let text_after =
                        h.state_mut().active_tab_mut().unwrap().annotations(0)[0].contents.clone();
                    let measure = if text_before.contains("mm") || text_before.contains("cm") {
                        if text_after != text_before { ", Messwert neu" } else { ", **Messwert alt**" }
                    } else {
                        ""
                    };
                    if moved && others_kept {
                        format!("ok (Punkt){measure}")
                    } else {
                        format!("**nein** {points:?} → {after:?}")
                    }
                }
                None => "**keine Punkte**".to_string(),
            },
            _ => "keine Griffe (richtig)".to_string(),
        };
        let r2 = first_rect(&mut h).unwrap_or(r1);
        if let Ok(img) = h.render() {
            let _ = img.save(out_dir().join(format!("handling-{tool:?}.png")));
        }
        // Delete and undo.
        let count =
            |h: &mut Harness<'_, MrPdfApp>| h.state_mut().active_tab_mut().unwrap().annotations(0).len();
        let p = to_screen(&h, (r2.x0 + r2.x1) / 2.0, (r2.y0 + r2.y1) / 2.0);
        click(&mut h, p);
        if h.state().active_tab().unwrap().selected_annot.is_none() {
            let p = to_screen(&h, r2.x0 + 1.0, (r2.y0 + r2.y1) / 2.0);
            click(&mut h, p);
        }
        key(&mut h, Key::Delete);
        let deleted = count(&mut h) == 0;
        h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
        h.run_steps(4);
        let back = count(&mut h) == 1;
        let line = format!(
            "| {tool:?} | {created} | {} | {} | {size_text} | {} | {} |",
            ok(selected),
            ok(moved),
            ok(deleted),
            ok(back)
        );
        println!("{line}");
        lines.push(line);
    }
    std::fs::write(
        out_dir().join("handling.md"),
        "# Umgang mit Anmerkungen\n\n".to_string() + &lines.join("\n") + "\n",
    )
    .unwrap();
}
