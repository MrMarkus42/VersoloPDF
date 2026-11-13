//! The PDF engine helper of VersoloPDF (protected mode): reads documents
//! with PDFium for the program, inside the sandbox. A program of its own, so
//! it contains nothing but the engine (no window, no other libraries).
fn main() {
    std::process::exit(versolopdf::engine::run_helper());
}
