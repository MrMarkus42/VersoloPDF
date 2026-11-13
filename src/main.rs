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
    // The PDF engine helper of a running VersoloPDF (protected mode): no
    // window, no files of its own, only requests on stdin/stdout.
    if std::env::args_os().nth(1).is_some_and(|a| a == versolopdf::engine::ENGINE_ARG) {
        std::process::exit(versolopdf::engine::run_helper());
    }
    // "Neue Version": the installed program checks a downloaded update and
    // puts it in place (possibly with administrator rights), then ends. No
    // window. See versolopdf::update::install_main.
    let args_os: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    if args_os.first().is_some_and(|a| a == versolopdf::update::INSTALL_ARG) {
        versolopdf::crashlog::init();
        std::process::exit(versolopdf::update::install_main(&args_os[1..]));
    }
    // The uninstaller: remove the file types this user registered.
    if args_os.first().is_some_and(|a| a == versolopdf::assoc::UNREGISTER_ARG) {
        versolopdf::assoc::unregister_all();
        std::process::exit(0);
    }
    // Leftovers of an earlier update (if the folder allows it).
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(PathBuf::from)) {
        versolopdf::update::remove_leftovers(&dir);
    }
    versolopdf::update::clean_old_downloads();
    // A moved portable copy: the file types this user gave it follow it.
    versolopdf::assoc::follow_moved_program();
    // Data of the former name "MrPDF" first, before anything is written.
    versolopdf::files::take_over_old_data();
    // Error log and crash reports ("Fehlerprotokoll").
    versolopdf::crashlog::init();
    let crash = versolopdf::crashlog::take_crash_notice();
    // macOS may run a downloaded app from a hidden read-only copy.
    #[cfg(target_os = "macos")]
    if let Ok(exe) = std::env::current_exe()
        && versolopdf::macos::is_translocated(&exe)
    {
        let original = exe.ancestors().nth(3).and_then(versolopdf::macos::original_bundle_path);
        log::info!(
            "macOS startet eine schreibgeschützte Kopie (App Translocation); Programm liegt in {original:?}"
        );
    }
    // Protected mode: documents are read by restricted helper processes
    // (this program started with --pdf-engine). VERSOLOPDF_ENGINE=inprocess
    // or the setting switch it off.
    let in_process = std::env::var("VERSOLOPDF_ENGINE").is_ok_and(|v| v.eq_ignore_ascii_case("inprocess"));
    match std::env::current_exe() {
        Ok(exe) if !in_process => versolopdf::engine::use_helper(versolopdf::engine::helper_program(&exe)),
        _ => versolopdf::engine::use_in_process(),
    }
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
    // PDFs opened in the Finder arrive as Apple Events, also at start.
    #[cfg(target_os = "macos")]
    versolopdf::macos::install_open_handler();

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
                #[cfg(target_os = "macos")]
                versolopdf::macos::set_waker(&cc.egui_ctx);
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
