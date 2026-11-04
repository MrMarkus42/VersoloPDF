// Hide the console window of release builds on Windows.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::path::PathBuf;

use mrpdf::ui::MrPdfApp;

/// Window and taskbar icon (the same picture as the program file's icon).
fn app_icon() -> egui::IconData {
    let (width, height, rgba) = mrpdf::ui::icons::logo_rgba();
    egui::IconData { rgba, width, height }
}

fn options(renderer: eframe::Renderer) -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("MrPDF")
            .with_app_id("mrpdf")
            .with_inner_size([1280.0, 860.0])
            .with_min_inner_size([640.0, 420.0])
            .with_drag_and_drop(true)
            .with_icon(app_icon()),
        renderer,
        ..Default::default()
    }
}

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    // "--opengl" (or MRPDF_RENDERER=opengl) uses OpenGL instead of the
    // default DirectX/Vulkan/Metal drawing through wgpu.
    let opengl = args.iter().any(|a| a == "--opengl")
        || std::env::var("MRPDF_RENDERER").is_ok_and(|v| v.eq_ignore_ascii_case("opengl"));
    let files: Vec<PathBuf> = args.iter().map(PathBuf::from).filter(|p| p.is_file()).collect();

    // One program window: a running MrPDF opens the files as tabs instead
    // ("--new-window" starts a separate window anyway).
    let inbox = if args.iter().any(|a| a == "--new-window") {
        None
    } else {
        match mrpdf::instance::start(mrpdf::files::data_dir().as_deref(), &files) {
            mrpdf::instance::Start::Forwarded => return Ok(()),
            mrpdf::instance::Start::First(inbox) => inbox,
        }
    };
    mrpdf::document::clean_old_undo_files();
    let inbox = std::sync::Arc::new(std::sync::Mutex::new(inbox));

    let start = |renderer, files: Vec<PathBuf>| {
        let inbox = inbox.clone();
        eframe::run_native(
            "MrPDF",
            options(renderer),
            Box::new(move |cc| {
                let mut app = MrPdfApp::new(cc, files);
                if let Some(inbox) = inbox.lock().ok().and_then(|mut i| i.take()) {
                    app.set_inbox(&cc.egui_ctx, inbox);
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
