//! Tour through every command of the command search ("Schnellstart").
//!
//! Each command runs in a fresh window with a sample document (3 pages, a
//! selected comment). The tour notes crashes, error messages, slow commands
//! and the dialog that opens, saves a picture of every dialog and writes
//! `target/tour/report.md`. Commands that act outside VersoloPDF (e-mail, web
//! pages, moving or deleting files, the registry, speech output, ending
//! the program) are left out and checked by hand.
//!
//! Run: `cargo test --release --test tour -- --ignored --nocapture`.

mod common;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::time::Instant;

use egui_kittest::Harness;
use versolopdf::geometry::RectF;
use versolopdf::model::{AnnotKind, AnnotStyle, Rgba};
use versolopdf::pdf_edit::NewAnnotation;
use versolopdf::ui::VersoloPdfApp;
use versolopdf::ui::app::Dialog;
use versolopdf::ui::app::commands::{Cmd, SEARCHABLE};
use versolopdf::ui::i18n::Language;
use versolopdf::ui::settings::Settings;

/// Commands with effects outside VersoloPDF.
fn outside(cmd: Cmd) -> bool {
    use versolopdf::ui::app::form_cmds::FormCmd;
    use versolopdf::ui::app::view_cmds::ViewCmd;
    matches!(
        cmd,
        Cmd::Email
            | Cmd::EmailAll
            | Cmd::Form(FormCmd::EmailFormData)
            | Cmd::RenameFile
            | Cmd::MoveFile
            | Cmd::TrashFile
            | Cmd::OpenFolder
            | Cmd::Manual
            | Cmd::Support
            | Cmd::ReportProblem
            | Cmd::CheckUpdate
            | Cmd::Homepage
            | Cmd::FileAssociation
            | Cmd::View(ViewCmd::ReadPage | ViewCmd::ReadToEnd | ViewCmd::ReadSelection)
    )
}

/// Window size: `VERSOLOPDF_TOUR_SIZE=1024x700` checks a small screen.
fn tour_size() -> egui::Vec2 {
    let size = std::env::var("VERSOLOPDF_TOUR_SIZE").unwrap_or_default();
    let mut parts = size.split('x').filter_map(|p| p.parse::<f32>().ok());
    match (parts.next(), parts.next()) {
        (Some(w), Some(h)) => egui::vec2(w, h),
        _ => egui::vec2(1280.0, 820.0),
    }
}

fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/tour");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn harness(file: PathBuf) -> Harness<'static, VersoloPdfApp> {
    let settings = Settings {
        language: Language::German,
        author: "Tester".into(),
        backstage_at_start: false,
        ..Settings::default()
    };
    let mut h = Harness::builder().with_size(tour_size()).with_max_steps(64).wgpu().build_eframe(move |cc| {
        let mut app = VersoloPdfApp::with_settings(&cc.egui_ctx, settings, vec![file]);
        app.set_recovery_dir(None);
        app
    });
    h.run_steps(6);
    // A comment to work with, selected.
    let tab = h.state_mut().active_tab_mut().expect("document open");
    let style = AnnotStyle { color: Rgba::RED, width: 2.0, author: "Tester".into(), ..AnnotStyle::default() };
    let mut square = NewAnnotation::new(AnnotKind::Square, style);
    square.rect = RectF::new(100.0, 100.0, 200.0, 180.0);
    tab.doc.add_annotation(0, &square).unwrap();
    tab.selected_annot = Some(versolopdf::ui::tab::AnnotRef { page: 0, index: 0 });
    h.run_steps(3);
    h
}

/// Name of a dialog (its variant).
fn dialog_name(d: &Dialog) -> String {
    let text = format!("{d:?}");
    text.split([' ', '{', '(']).next().unwrap_or("").to_string()
}

#[test]
#[ignore = "long; run explicitly"]
fn command_tour() {
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let dir_out = out_dir();
    let mut lines = vec!["| Nr | Befehl | Ergebnis | Dauer |".to_string(), "|---|---|---|---|".to_string()];
    let (mut crashes, mut errors, mut slow, mut too_big) = (0, 0, 0, 0);
    for (i, &cmd) in SEARCHABLE.iter().enumerate() {
        let name = cmd.label(Language::German).to_string();
        if outside(cmd) {
            lines.push(format!("| {i} | {name} | übersprungen (wirkt außerhalb) | |"));
            continue;
        }
        // A fresh copy of the sample for every command.
        let file = dir.path().join(format!("Beispiel{i}.pdf"));
        std::fs::write(&file, common::text_pdf(3)).unwrap();
        let mut h = harness(file);
        let ctx = h.ctx.clone();
        if !h.state().cmd_enabled(cmd) {
            lines.push(format!("| {i} | {name} | gesperrt | |"));
            continue;
        }
        let start = Instant::now();
        let result = catch_unwind(AssertUnwindSafe(|| {
            h.state_mut().run(&ctx, cmd);
            h.run_steps(6);
        }));
        let took = start.elapsed();
        let dauer = format!("{:.2} s", took.as_secs_f64());
        if took.as_secs_f64() > 2.0 {
            slow += 1;
        }
        let outcome = match result {
            Err(panic) => {
                crashes += 1;
                let msg = panic
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_default();
                format!("**ABSTURZ**: {}", msg.replace('\n', " ").replace('|', "/"))
            }
            Ok(()) => match h.state().dialog().cloned() {
                Some(Dialog::Message { title, text }) if title == "Fehler" => {
                    errors += 1;
                    format!("**Fehlermeldung**: {}", text.replace('\n', " ").replace('|', "/"))
                }
                Some(Dialog::Message { title, text }) => {
                    format!("Meldung „{title}“: {}", text.replace('\n', " ").replace('|', "/"))
                }
                Some(d) => {
                    let dn = dialog_name(&d);
                    if let Ok(img) = h.render() {
                        let _ = img.save(dir_out.join(format!("{i:03}-{dn}.png")));
                    }
                    // The dialog must fit into the window.
                    let screen = h.ctx.content_rect();
                    let area = h.ctx.memory(|m| m.area_rect(egui::Id::new("versolopdf-dialog")));
                    match area {
                        Some(r) if !screen.expand(1.0).contains_rect(r) => {
                            too_big += 1;
                            format!(
                                "**Dialog {dn} zu groß**: {:.0}×{:.0} bei Fenster {:.0}×{:.0}",
                                r.width(),
                                r.height(),
                                screen.width(),
                                screen.height()
                            )
                        }
                        _ => format!("Dialog {dn}"),
                    }
                }
                None => "ok".to_string(),
            },
        };
        println!("{i:3} {name}: {outcome} ({dauer})");
        lines.push(format!("| {i} | {name} | {outcome} | {dauer} |"));
    }
    let summary = format!(
        "# Befehls-Rundgang\n\n{} Befehle, {crashes} Abstürze, {errors} Fehlermeldungen, {slow} langsamer als 2 s, {too_big} Dialoge zu groß.\n\n",
        SEARCHABLE.len()
    );
    std::fs::write(dir_out.join("report.md"), summary + &lines.join("\n") + "\n").unwrap();
    println!("Abstürze: {crashes}, Fehlermeldungen: {errors}, langsam: {slow}, zu groß: {too_big}");
}
