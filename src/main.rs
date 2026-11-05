// Hide the console window of release builds on Windows.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::path::PathBuf;

use versolopdf::ui::VersoloPdfApp;

/// Window and taskbar icon (the same picture as the program file's icon).
fn app_icon() -> egui::IconData {
    let (width, height, rgba) = versolopdf::ui::icons::logo_rgba();
    egui::IconData { rgba, width, height }
}

fn options(renderer: eframe::Renderer) -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("VersoloPDF")
            .with_app_id("versolopdf")
            .with_inner_size([1280.0, 860.0])
            .with_min_inner_size([640.0, 420.0])
            .with_drag_and_drop(true)
            .with_icon(app_icon()),
        renderer,
        ..Default::default()
    }
}

fn main() -> eframe::Result {
    // "Neue Version": this (new) program copies itself into the program
    // folder of the running old version, then ends. No window.
    let mut raw = std::env::args_os().skip(1);
    if raw.next().is_some_and(|a| a == versolopdf::update::INSTALL_ARG) {
        let code = match (raw.next(), std::env::current_exe()) {
            (Some(target), Ok(exe)) => {
                let source = exe.parent().map(PathBuf::from).unwrap_or_default();
                match versolopdf::update::install_files(&source, &PathBuf::from(target)) {
                    Ok(_) => 0,
                    Err(err) => {
                        versolopdf::crashlog::init();
                        log::error!("Update nicht installiert: {err}");
                        1
                    }
                }
            }
            _ => 2,
        };
        std::process::exit(code);
    }
    // Leftovers of an earlier update (if the folder allows it).
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(PathBuf::from)) {
        versolopdf::update::remove_leftovers(&dir);
    }
    // Data of the former name "MrPDF" first, before anything is written.
    versolopdf::files::take_over_old_data();
    // Error log and crash reports ("Fehlerprotokoll").
    versolopdf::crashlog::init();
    let crash = versolopdf::crashlog::take_crash_notice();
    // For testing the crash report only.
    if std::env::var_os("VERSOLOPDF_TEST_CRASH").is_some() {
        panic!("Testabsturz (VERSOLOPDF_TEST_CRASH gesetzt)");
    }

    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    // "--opengl" (or VERSOLOPDF_RENDERER=opengl) uses OpenGL instead of the
    // default DirectX/Vulkan/Metal drawing through wgpu.
    let opengl = args.iter().any(|a| a == "--opengl")
        || std::env::var("VERSOLOPDF_RENDERER").is_ok_and(|v| v.eq_ignore_ascii_case("opengl"));
    let files: Vec<PathBuf> = args.iter().map(PathBuf::from).filter(|p| p.is_file()).collect();

    // Started by an update: first wait until the old version has closed
    // (it may still ask about unsaved changes). If it stays open, this one
    // becomes a separate window.
    let mut separate = args.iter().any(|a| a == "--new-window");
    if let Some(pos) = args.iter().position(|a| a == versolopdf::update::WAIT_ARG)
        && let Some(pid) = args.get(pos + 1).and_then(|p| p.to_str()).and_then(|p| p.parse::<u32>().ok())
    {
        separate |= !versolopdf::update::wait_for_exit(pid, 120);
    }

    // One program window: a running VersoloPDF opens the files as tabs instead
    // ("--new-window" starts a separate window anyway).
    let inbox = if separate {
        None
    } else {
        match versolopdf::instance::start(versolopdf::files::data_dir().as_deref(), &files) {
            versolopdf::instance::Start::Forwarded => return Ok(()),
            versolopdf::instance::Start::First(inbox) => inbox,
        }
    };
    versolopdf::document::clean_old_undo_files();
    let inbox = std::sync::Arc::new(std::sync::Mutex::new(inbox));

    let start = |renderer: eframe::Renderer, files: Vec<PathBuf>| {
        log::info!(
            "VersoloPDF {} gestartet ({} {}, Grafik: {renderer:?}, {} Datei(en))",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
            files.len()
        );
        let inbox = inbox.clone();
        let crash = crash.clone();
        eframe::run_native(
            "VersoloPDF",
            options(renderer),
            Box::new(move |cc| {
                let mut app = VersoloPdfApp::new(cc, files);
                if let Some(inbox) = inbox.lock().ok().and_then(|mut i| i.take()) {
                    app.set_inbox(&cc.egui_ctx, inbox);
                }
                if let Some(report) = crash {
                    app.show_crash_notice(&report);
                }
                Ok(Box::new(app))
            }),
        )
    };
    if opengl {
        return start(eframe::Renderer::Glow, files);
    }
    match start(eframe::Renderer::Wgpu, files.clone()) {
        // No usable graphics driver for wgpu: fall back to OpenGL.
        Err(err @ eframe::Error::Wgpu(_)) => {
            log::warn!("wgpu not available ({err}), using OpenGL");
            start(eframe::Renderer::Glow, files)
        }
        result => result,
    }
}
