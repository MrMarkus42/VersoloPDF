//! "Per E-Mail senden": opens the user's default e-mail program with a new
//! message, the documents attached where the system allows it.
//!
//! On Windows this uses Simple MAPI (`MAPISendMailW` from MAPI32.dll), the
//! way Windows programs hand a file to Outlook, Thunderbird and others; the
//! message window opens and nothing is sent without the user. Without a
//! MAPI mail program (e.g. the new Outlook) the `mailto:` address goes to
//! the program registered for it (not to the web browser) and Explorer
//! shows the files to drag in. On macOS Apple Mail or Outlook get the
//! files attached (see `crate::macos`). Otherwise `xdg-open` / `open` gets
//! the `mailto:` address and the file manager shows the files.

use std::path::{Path, PathBuf};

/// Opens a new message with `subject` and `files` attached; returns at
/// once (the mail program may keep its window open for a long time).
/// `true` when the files are attached automatically (Windows with MAPI is
/// tried first; the result there is only known later), `false` when the
/// user has to attach them.
pub fn compose(subject: &str, files: Vec<PathBuf>) -> bool {
    if no_external() {
        return false;
    }
    let subject = subject.to_string();
    #[cfg(windows)]
    {
        std::thread::spawn(move || {
            if !windows::send_mapi(&subject, &files) {
                windows::open_mailto(&mailto(&subject));
                for f in &files {
                    reveal(f);
                }
            }
        });
        true
    }
    #[cfg(target_os = "macos")]
    if crate::macos::compose_mail(&files) {
        return true;
    }
    #[cfg(not(windows))]
    {
        let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        if let Err(err) = std::process::Command::new(opener).arg(mailto(&subject)).spawn() {
            log::warn!("could not open the e-mail program: {err}");
        }
        for f in &files {
            reveal(f);
        }
        false
    }
}

/// Automated tests set `VERSOLOPDF_NO_EXTERNAL`: then no mail program, file
/// manager or browser is started.
pub fn no_external() -> bool {
    std::env::var_os("VERSOLOPDF_NO_EXTERNAL").is_some()
}

/// `mailto:` address with the subject, percent-encoded.
pub fn mailto(subject: &str) -> String {
    format!("mailto:?subject={}", encode(subject))
}

/// Percent-encoding for a mailto address (at most 200 characters).
pub fn encode(text: &str) -> String {
    let mut out = String::new();
    for b in text.chars().filter(|c| !c.is_control()).take(200).collect::<String>().bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Shows a file in the system file manager.
fn reveal(path: &Path) {
    let result = if cfg!(windows) {
        let mut arg = std::ffi::OsString::from("/select,");
        arg.push(path.as_os_str());
        std::process::Command::new("explorer").arg(arg).spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg("-R").arg(path).spawn()
    } else {
        let dir = path.parent().unwrap_or(path);
        std::process::Command::new("xdg-open").arg(dir).spawn()
    };
    if let Err(err) = result {
        log::warn!("could not open the file manager: {err}");
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod windows {
    use std::path::PathBuf;

    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows_sys::Win32::System::Mapi::{
        LPMAPISENDMAILW, MAPI_DIALOG, MAPI_LOGON_UI, MapiFileDescW, MapiMessageW,
    };
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    /// UTF-16 with a closing zero.
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// A new message with the files attached in the default MAPI mail
    /// program. `false` when there is none or it failed (not when the user
    /// closed the message).
    pub fn send_mapi(subject: &str, files: &[PathBuf]) -> bool {
        // SAFETY: LoadLibraryW gets a zero-terminated UTF-16 name.
        let lib = unsafe { LoadLibraryW(wide("MAPI32.DLL").as_ptr()) };
        if lib.is_null() {
            return false;
        }
        // SAFETY: the name is zero-terminated ASCII; the result is checked.
        let Some(proc) = (unsafe { GetProcAddress(lib, c"MAPISendMailW".as_ptr().cast()) }) else {
            return false;
        };
        // SAFETY: MAPISendMailW has exactly the signature of LPMAPISENDMAILW.
        let send: LPMAPISENDMAILW = unsafe { std::mem::transmute(proc) };
        let Some(send) = send else { return false };

        // The strings must live until the call returns.
        let mut subject = wide(subject);
        let mut empty = wide("");
        let mut paths: Vec<Vec<u16>> = files.iter().map(|p| wide(&p.to_string_lossy())).collect();
        let mut names: Vec<Vec<u16>> = files
            .iter()
            .map(|p| wide(&p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()))
            .collect();
        let mut descs: Vec<MapiFileDescW> = paths
            .iter_mut()
            .zip(names.iter_mut())
            .map(|(path, name)| MapiFileDescW {
                // Not placed inside the text.
                nPosition: u32::MAX,
                lpszPathName: path.as_mut_ptr(),
                lpszFileName: name.as_mut_ptr(),
                ..MapiFileDescW::default()
            })
            .collect();
        let message = MapiMessageW {
            lpszSubject: subject.as_mut_ptr(),
            lpszNoteText: empty.as_mut_ptr(),
            nFileCount: descs.len() as u32,
            lpFiles: if descs.is_empty() { std::ptr::null_mut() } else { descs.as_mut_ptr() },
            ..MapiMessageW::default()
        };
        // SAFETY: all pointers in `message` point into the vectors above,
        // which outlive the call.
        let code = unsafe { send(0, 0, &message, MAPI_DIALOG | MAPI_LOGON_UI, 0) };
        // 0 = SUCCESS_SUCCESS, 1 = MAPI_USER_ABORT (message closed).
        if code > 1 {
            log::warn!("MAPISendMailW failed with code {code}");
        }
        code <= 1
    }

    /// Opens a `mailto:` address with the program Windows has for it.
    pub fn open_mailto(address: &str) {
        let (verb, file) = (wide("open"), wide(address));
        // SAFETY: zero-terminated UTF-16 strings; null parameters allowed.
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                file.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        // Values up to 32 are errors.
        if result as usize <= 32 {
            log::warn!("no program for mailto: ({})", result as usize);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn mailto_address() {
        assert_eq!(super::mailto("a b.pdf"), "mailto:?subject=a%20b.pdf");
        assert_eq!(super::encode("Datei: ä"), "Datei%3A%20%C3%A4");
        assert_eq!(super::encode("x&y=\n"), "x%26y%3D");
    }
}
