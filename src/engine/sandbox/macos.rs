//! Restrictions of the engine helper on macOS: the macOS sandbox (Seatbelt),
//! entered by the helper itself after loading PDFium.

#![allow(unsafe_code)]

use std::ffi::{CStr, CString, c_char, c_int};

unsafe extern "C" {
    fn sandbox_init(profile: *const c_char, flags: u64, errorbuf: *mut *mut c_char) -> c_int;
    fn sandbox_free_error(errorbuf: *mut c_char);
}

/// Quotes a path for the profile language.
fn quoted(path: &str) -> String {
    format!("\"{}\"", path.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The rules: nothing is allowed except reading fonts and system files.
fn profile() -> String {
    let mut read = vec![
        "/System/Library/Fonts".to_string(),
        "/System/Library/Assets".to_string(),
        "/System/Library/AssetsV2".to_string(),
        "/Library/Fonts".to_string(),
        "/System/Library/Frameworks".to_string(),
        "/System/Library/PrivateFrameworks".to_string(),
        "/usr/lib".to_string(),
        "/usr/share".to_string(),
        "/private/var/db/timezone".to_string(),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        read.push(format!("{}/Library/Fonts", home.to_string_lossy()));
    }
    let paths: String = read.iter().map(|p| format!(" (subpath {})", quoted(p))).collect();
    format!(
        "(version 1)\n\
         (deny default)\n\
         (allow file-read*{paths})\n\
         (allow file-read-metadata (literal \"/\") (literal \"/private\") (literal \"/private/var\"))\n\
         (allow sysctl-read)\n\
         (allow mach-lookup (global-name \"com.apple.fonts\") (global-name \"com.apple.FontObjectsServer\"))\n"
    )
}

pub fn lockdown() -> Result<String, String> {
    let profile = CString::new(profile()).map_err(|e| e.to_string())?;
    let mut error: *mut c_char = std::ptr::null_mut();
    // SAFETY: `profile` is a valid C string; the error buffer is freed below.
    let result = unsafe { sandbox_init(profile.as_ptr(), 0, &mut error) };
    if result != 0 {
        let message = if error.is_null() {
            "sandbox_init failed".to_string()
        } else {
            // SAFETY: sandbox_init set a valid C string, freed once here.
            let text = unsafe { CStr::from_ptr(error) }.to_string_lossy().into_owned();
            unsafe { sandbox_free_error(error) };
            text
        };
        return Err(message);
    }
    Ok("macOS sandbox: files only fonts, no network, no new processes".into())
}
