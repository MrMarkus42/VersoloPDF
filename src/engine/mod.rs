//! The PDF engine (PDFium) and how VersoloPDF uses it.
//!
//! PDFium reads and draws the documents. It is written in C++ and handles
//! untrusted files, so VersoloPDF keeps it away from the rest of the
//! program ("geschützter Modus"): every document is read by a helper
//! process – the program itself started with [`ENGINE_ARG`] – that loads
//! PDFium and then locks itself up ([`sandbox`]): it cannot read the user's
//! files, use the network or start programs. The program sends it requests
//! ([`proto`]) and checks every answer. Only `host.rs` uses PDFium.

mod client;
mod host;
mod pdfium_lib;
pub mod proto;
mod sandbox;

pub use client::{EngineDoc, Protection, Source, protection, set_enabled, use_helper, use_in_process};
pub use host::{ENGINE_ARG, run_helper};
pub use pdfium_lib::{PDFIUM_DIR_ENV, library_search_dirs, pdfium};
