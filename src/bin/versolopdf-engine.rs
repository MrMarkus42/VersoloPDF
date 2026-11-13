//! The PDF engine helper of VersoloPDF (protected mode): reads documents
//! with PDFium for the program, inside the sandbox. A program of its own, so
//! it contains nothing but the engine (no window, no other libraries).
//!
//! On Windows it is a window program without a console: a console would need
//! a second process (conhost), which the sandbox does not allow.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    std::process::exit(versolopdf::engine::run_helper());
}
