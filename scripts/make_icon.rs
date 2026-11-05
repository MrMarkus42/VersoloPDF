//! Prepares the application icon from the drawing in `assets/versolopdf-icon-source.webp`:
//! crops it to the red rounded square, makes everything outside the square
//! transparent and writes `assets/versolopdf.png` (256 px) and `assets/versolopdf.ico`
//! (16 to 256 px).
//!
//! Run: `cargo run --release --example make_icon` (registered in Cargo.toml).

use image::codecs::ico::{IcoEncoder, IcoFrame};
use image::imageops::FilterType;
use image::{ExtendedColorType, Rgba, RgbaImage};

fn is_red(p: &Rgba<u8>) -> bool {
    p[0] > 110 && p[1] < 80 && p[2] < 80
}

fn main() {
    let src = image::open("assets/versolopdf-icon-source.webp").expect("icon source").to_rgba8();
    // Bounding box of the red square.
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for (x, y, p) in src.enumerate_pixels() {
        if is_red(p) {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    let side = (x1 - x0).max(y1 - y0) + 1;
    let mut square = image::imageops::crop_imm(&src, x0, y0, side, side).to_image();
    // Corner radius: distance from the corner to the first red pixel on the diagonal.
    let mut d = 0;
    while d < side / 2 && !is_red(square.get_pixel(d, d)) {
        d += 1;
    }
    let radius = d as f32 / (1.0 - std::f32::consts::FRAC_1_SQRT_2);
    let s = side as f32;
    for (x, y, p) in square.enumerate_pixels_mut() {
        let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
        let cx = fx.clamp(radius, s - radius);
        let cy = fy.clamp(radius, s - radius);
        let dist = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
        // Soft edge over one pixel.
        let alpha = (radius - dist + 0.5).clamp(0.0, 1.0);
        p[3] = (f32::from(p[3]) * alpha).round() as u8;
    }
    let big = image::imageops::resize(&square, 256, 256, FilterType::Lanczos3);
    big.save("assets/versolopdf.png").expect("write png");
    let frames: Vec<IcoFrame> = [16u32, 20, 24, 32, 40, 48, 64, 128, 256]
        .iter()
        .map(|&n| {
            let img: RgbaImage = image::imageops::resize(&square, n, n, FilterType::Lanczos3);
            IcoFrame::as_png(img.as_raw(), n, n, ExtendedColorType::Rgba8).expect("frame")
        })
        .collect();
    let file = std::fs::File::create("assets/versolopdf.ico").expect("ico file");
    IcoEncoder::new(file).encode_images(&frames).expect("write ico");
    println!("square {side} px at {x0},{y0}, corner radius {radius:.0} px");
}
