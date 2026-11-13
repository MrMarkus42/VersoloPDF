//! Before/after pictures of the look: the same scenes with the classic and
//! the modern look (and the modern one at 125 % interface size). Pictures
//! go to `VERSOLOPDF_SHOT_DIR` (default target/look). Run on Windows to see the
//! system font: `look-tests.exe --ignored --nocapture`.

mod common;

use std::path::PathBuf;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use versolopdf::ui::VersoloPdfApp;
use versolopdf::ui::app::commands::Cmd;
use versolopdf::ui::i18n::Language;
use versolopdf::ui::settings::Settings;

fn out_dir() -> PathBuf {
    let dir = std::env::var_os("VERSOLOPDF_SHOT_DIR")
        .map_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/look"), PathBuf::from);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn harness(files: Vec<PathBuf>, modern: bool, scale: f32) -> Harness<'static, VersoloPdfApp> {
    let settings = Settings {
        language: Language::German,
        author: "Tester".into(),
        backstage_at_start: false,
        modern_look: modern,
        ui_scale: scale,
        ..Settings::default()
    };
    let mut h =
        Harness::builder().with_size(egui::vec2(1280.0, 800.0)).with_max_steps(64).wgpu().build_eframe(
            move |cc| {
                let mut app = VersoloPdfApp::with_settings(&cc.egui_ctx, settings, files);
                app.set_recovery_dir(None);
                app
            },
        );
    h.run_steps(10);
    h
}

fn shot(h: &mut Harness<'_, VersoloPdfApp>, name: &str) {
    h.event(egui::Event::PointerGone);
    h.run_steps(4);
    match h.render() {
        Ok(img) => {
            let path = out_dir().join(format!("{name}.png"));
            img.save(&path).unwrap();
            println!("{}", path.display());
        }
        Err(e) => panic!("render failed: {e}"),
    }
}

#[test]
#[ignore = "pictures; run explicitly"]
fn before_after() {
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let sample = dir.path().join("Beispiel.pdf");
    std::fs::write(&sample, common::text_pdf(3)).unwrap();
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
    for (variant, modern, scale) in
        [("1-vorher", false, 1.0), ("2-nachher", true, 1.0), ("3-nachher-125", true, 1.25)]
    {
        // Main window with a document and the properties.
        let mut h = harness(vec![sample.clone()], modern, scale);
        shot(&mut h, &format!("{variant}-hauptfenster"));
        drop(h);
        // A large dialog.
        let mut h = harness(vec![sample.clone()], modern, scale);
        let ctx = h.ctx.clone();
        h.state_mut().run(&ctx, Cmd::HeaderFooter);
        h.run_steps(8);
        shot(&mut h, &format!("{variant}-dialog"));
        drop(h);
        // The comparison side by side.
        let mut h = harness(vec![new.clone(), old.clone()], modern, scale);
        let ctx = h.ctx.clone();
        h.state_mut().run(&ctx, Cmd::CompareDocuments);
        h.run_steps(5);
        // In a small window OK may lie below the visible part: scroll the
        // dialog down first, as a person would, then click OK.
        h.event(egui::Event::PointerMoved(egui::pos2(640.0, 400.0)));
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -3000.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
        });
        h.run_steps(20);
        let ok = h.query_all_by_label("OK").last().map(|n| n.rect().center());
        if let Some(p) = ok {
            for (pressed, step) in [(true, true), (false, false)] {
                h.event(egui::Event::PointerMoved(p));
                h.event(egui::Event::PointerButton {
                    pos: p,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                });
                if step {
                    h.step();
                }
            }
        }
        for _ in 0..200 {
            h.step();
            if h.state().comparison().is_some_and(|c| !c.running()) {
                break;
            }
        }
        h.run_steps(5);
        shot(&mut h, &format!("{variant}-vergleich"));
    }
}
