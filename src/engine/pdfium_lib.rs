//! Loading and initialising the PDFium rendering engine.
//!
//! PDFium is shipped as a shared library next to the executable. To avoid
//! DLL-hijacking style attacks the library is only ever loaded from an
//! explicit, absolute path – never through the operating system's default
//! search order (which on Windows includes the current working directory).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use pdfium_render::prelude::*;

use crate::error::{Error, Result};

static PDFIUM: OnceLock<Pdfium> = OnceLock::new();

/// Environment variable that may point to a directory containing the PDFium
/// library. Intended for development and packaging; it must be absolute.
pub const PDFIUM_DIR_ENV: &str = "VERSOLOPDF_PDFIUM_DIR";

/// Returns the process-wide PDFium instance, initialising it on first use.
pub fn pdfium() -> Result<&'static Pdfium> {
    if let Some(pdfium) = PDFIUM.get() {
        return Ok(pdfium);
    }
    let bindings = bind()?;
    // A concurrent initialisation would only happen in tests; `get_or_init`
    // keeps the first instance and drops the surplus bindings safely.
    Ok(PDFIUM.get_or_init(|| Pdfium::new(bindings)))
}

/// Candidate directories that are searched for the PDFium library, in order.
pub fn library_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Some(dir) = std::env::var_os(PDFIUM_DIR_ENV).map(PathBuf::from) {
        if dir.is_absolute() {
            dirs.push(dir);
        } else {
            log::warn!("ignoring {PDFIUM_DIR_ENV}: path is not absolute");
        }
    }

    if let Some(exe_dir) = std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf)) {
        dirs.push(exe_dir.clone());
        // Linux packages (`/usr/lib/versolopdf`) and macOS app bundles.
        dirs.push(exe_dir.join("../lib/versolopdf"));
        dirs.push(exe_dir.join("../Frameworks"));
    }

    // Development builds: use the copy fetched by `scripts/fetch-pdfium.sh`.
    if cfg!(debug_assertions) {
        let vendor = Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/pdfium");
        let platform = platform_dir_name();
        dirs.push(vendor.join(platform).join("lib"));
        dirs.push(vendor.join(platform).join("bin"));
    }

    dirs
}

fn platform_dir_name() -> &'static str {
    if cfg!(all(target_os = "windows", target_arch = "aarch64")) {
        "win-arm64"
    } else if cfg!(target_os = "windows") {
        "win-x64"
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "mac-arm64"
    } else if cfg!(target_os = "macos") {
        "mac-x64"
    } else if cfg!(target_arch = "aarch64") {
        "linux-arm64"
    } else {
        "linux-x64"
    }
}

fn bind() -> Result<Box<dyn PdfiumLibraryBindings>> {
    let mut tried = Vec::new();
    for dir in library_search_dirs() {
        let candidate = Pdfium::pdfium_platform_library_name_at_path(&dir);
        if !candidate.is_file() {
            tried.push(candidate.display().to_string());
            continue;
        }
        match Pdfium::bind_to_library(&candidate) {
            Ok(bindings) => {
                log::info!("loaded PDFium from {}", candidate.display());
                return Ok(bindings);
            }
            Err(err) => {
                log::warn!("could not load {}: {err}", candidate.display());
                tried.push(candidate.display().to_string());
            }
        }
    }
    Err(Error::EngineUnavailable(tried.join(", ")))
}
