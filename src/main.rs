// Hide the console window of release builds on Windows.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::path::PathBuf;

use mrpdf::ui::MrPdfApp;

fn app_icon() -> egui::IconData {
    // A simple generated icon: red rounded square with a white "M".
    const N: u32 = 64;
    let mut rgba = vec![0u8; (N * N * 4) as usize];
    for y in 0..N {
        for x in 0..N {
            let i = ((y * N + x) * 4) as usize;
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let r = 12.0;
            let cx = fx.clamp(r, N as f32 - r);
            let cy = fy.clamp(r, N as f32 - r);
            let inside = (fx - cx).powi(2) + (fy - cy).powi(2) <= r * r;
            if !inside {
                continue;
            }
            // Strokes of the "M".
            let in_m = (14..=50).contains(&y)
                && ((14..=21).contains(&x)
                    || (43..=50).contains(&x)
                    || ((x as i32 - y as i32 + 2).abs() <= 3 && (16..=33).contains(&x))
                    || ((x as i32 + y as i32 - 64 - 2).abs() <= 3 && (31..=48).contains(&x)))
                && y <= 50;
            let (r, g, b) = if in_m && y < 51 { (255, 255, 255) } else { (200, 35, 45) };
            rgba[i] = r;
            rgba[i + 1] = g;
            rgba[i + 2] = b;
            rgba[i + 3] = 255;
        }
    }
    egui::IconData { rgba, width: N, height: N }
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

    let start = |renderer, files: Vec<PathBuf>| {
        eframe::run_native(
            "MrPDF",
            options(renderer),
            Box::new(move |cc| Ok(Box::new(MrPdfApp::new(cc, files)))),
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
