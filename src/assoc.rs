//! "Dateizuordnung": makes VersoloPDF the program that opens PDF files (and
//! its own session files) on a double click.
//!
//! * Windows: VersoloPDF registers itself for the current user
//!   (`HKEY_CURRENT_USER\Software\Classes`, listed in "Standard-Apps").
//!   Windows does not let programs take over a file type the user has
//!   chosen a program for; then its settings page is opened, where one click
//!   finishes the change.
//! * macOS: Launch Services is told to use VersoloPDF (macOS may ask once).
//! * Linux: a `.desktop` entry and the default in `mimeapps.list`
//!   (freedesktop.org standard).
//!
//! Nothing here runs without the user asking for it (Einstellungen ›
//! Dateizuordnung), and automated tests never touch the real system: they
//! use [`Places`] in a test folder or test registry key.

use crate::error::{Error, Result};

/// A kind of file VersoloPDF can be the program for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Kind {
    /// File ending without the dot.
    pub ext: &'static str,
    pub mime: &'static str,
    /// macOS type identifier.
    pub uti: &'static str,
    /// Windows class name.
    pub prog_id: &'static str,
    pub german: &'static str,
    pub english: &'static str,
}

pub const PDF: Kind = Kind {
    ext: "pdf",
    mime: "application/pdf",
    uti: "com.adobe.pdf",
    prog_id: "VersoloPDF.Document",
    german: "PDF-Dokumente (.pdf)",
    english: "PDF documents (.pdf)",
};

pub const SESSION: Kind = Kind {
    ext: "versolopdfsession",
    mime: "application/x-versolopdf-session",
    uti: "io.github.mrmarkus42.versolopdf.session",
    prog_id: "VersoloPDF.Session",
    german: "VersoloPDF-Sitzungen (.versolopdfsession)",
    english: "VersoloPDF sessions (.versolopdfsession)",
};

/// The kinds offered on this system (Linux knows no session type).
pub fn kinds() -> &'static [Kind] {
    if cfg!(any(windows, target_os = "macos")) { &[PDF, SESSION] } else { &[PDF] }
}

/// Who opens a kind of file now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// VersoloPDF.
    Default,
    /// VersoloPDF is offered ("Öffnen mit"), another program is the default.
    Offered,
    /// VersoloPDF is not registered for it.
    Off,
}

/// What [`set`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Done,
    /// Windows asks the user to confirm in its settings (opened).
    ConfirmInSettings,
}

/// Argument with which the uninstaller removes the registration.
pub const UNREGISTER_ARG: &str = "--unregister-file-types";

/// Where the registration is written: the real places of this user, or a
/// test folder / test registry key.
#[derive(Debug, Clone)]
pub struct Places {
    /// Registry path below HKEY_CURRENT_USER that holds "Classes"
    /// ("Software" for the real one).
    #[cfg(windows)]
    pub base: String,
    /// `$XDG_DATA_HOME` and `$XDG_CONFIG_HOME`.
    #[cfg(all(unix, not(target_os = "macos")))]
    pub data: std::path::PathBuf,
    #[cfg(all(unix, not(target_os = "macos")))]
    pub config: std::path::PathBuf,
    /// True for the user's real settings.
    pub real: bool,
}

impl Places {
    /// The places the system uses for the current user.
    pub fn system() -> Option<Places> {
        #[cfg(windows)]
        {
            Some(Places { base: "Software".into(), real: true })
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let home = std::env::var_os("HOME").map(std::path::PathBuf::from)?;
            let env = |name: &str, fallback: &str| {
                std::env::var_os(name)
                    .map(std::path::PathBuf::from)
                    .filter(|p| p.is_absolute())
                    .unwrap_or_else(|| home.join(fallback))
            };
            Some(Places {
                data: env("XDG_DATA_HOME", ".local/share"),
                config: env("XDG_CONFIG_HOME", ".config"),
                real: true,
            })
        }
        #[cfg(target_os = "macos")]
        {
            Some(Places { real: true })
        }
    }
}

fn exe() -> Result<std::path::PathBuf> {
    std::env::current_exe().map_err(|e| Error::Invalid(format!("program file unknown: {e}")))
}

/// Who opens `kind` now (on the real system).
pub fn state(kind: &Kind) -> State {
    match Places::system() {
        Some(places) => sys::state(&places, kind),
        None => State::Off,
    }
}

/// Makes VersoloPDF the program for `kinds` (`on`), or removes it.
pub fn set(kinds: &[Kind], on: bool) -> Result<Outcome> {
    if crate::mail::no_external() {
        return Err(Error::Invalid("Testbetrieb: Dateizuordnung wird nicht geändert".into()));
    }
    let places = Places::system().ok_or_else(|| Error::Invalid("home folder unknown".into()))?;
    let exe = exe()?;
    set_in(&places, &exe, kinds, on)
}

/// [`set`] with explicit places and program file (tests).
pub fn set_in(places: &Places, exe: &std::path::Path, kinds: &[Kind], on: bool) -> Result<Outcome> {
    let outcome = if on { sys::register(places, exe, kinds)? } else { sys::unregister(places, kinds)? };
    log::info!(
        "Dateizuordnung {}: {}",
        if on { "gesetzt" } else { "entfernt" },
        kinds.iter().map(|k| k.ext).collect::<Vec<_>>().join(", ")
    );
    Ok(outcome)
}

/// For the uninstaller: removes everything VersoloPDF registered itself.
pub fn unregister_all() {
    if let Some(places) = Places::system() {
        let _ = sys::unregister(&places, kinds());
    }
}

/// After the program was moved (portable copy): points an existing
/// registration of this user to the new place. Never registers anew.
pub fn follow_moved_program() {
    if crate::mail::no_external() {
        return;
    }
    let (Some(places), Ok(exe)) = (Places::system(), exe()) else { return };
    let registered: Vec<Kind> = kinds()
        .iter()
        .copied()
        .filter(|k| sys::registered_program(&places, k).is_some_and(|p| p != exe))
        .collect();
    if !registered.is_empty() {
        log::info!("Programm verschoben: Dateizuordnung zeigt jetzt auf {}", exe.display());
        let _ = sys::register(&places, &exe, &registered);
    }
}

// ---------------------------------------------------------------------------
// Windows

#[cfg(windows)]
#[allow(unsafe_code)]
mod sys {
    use std::path::{Path, PathBuf};

    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CLASSES_ROOT, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_NONE,
        REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteKeyValueW,
        RegDeleteTreeW, RegGetValueW, RegSetValueExW,
    };

    use super::{Kind, Outcome, Places, State};
    use crate::error::{Error, Result};

    const APP: &str = "VersoloPDF";
    const EXE_NAME: &str = "VersoloPDF.exe";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn fail(what: &str, code: u32) -> Error {
        Error::Invalid(format!("Registry ({what}): {}", std::io::Error::from_raw_os_error(code as i32)))
    }

    struct Key(HKEY);

    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: a key opened by RegCreateKeyExW, closed once.
            unsafe { RegCloseKey(self.0) };
        }
    }

    impl Key {
        fn create(path: &str) -> Result<Key> {
            let mut key: HKEY = std::ptr::null_mut();
            let p = wide(path);
            // SAFETY: valid strings and out pointer.
            let code = unsafe {
                RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    p.as_ptr(),
                    0,
                    std::ptr::null(),
                    REG_OPTION_NON_VOLATILE,
                    KEY_SET_VALUE | KEY_QUERY_VALUE,
                    std::ptr::null(),
                    &mut key,
                    std::ptr::null_mut(),
                )
            };
            if code != ERROR_SUCCESS {
                return Err(fail(path, code));
            }
            Ok(Key(key))
        }

        fn set(&self, name: &str, value: &str) -> Result<()> {
            let (n, v) = (wide(name), wide(value));
            // SAFETY: `v` holds len*2 bytes including the final 0.
            let code = unsafe {
                RegSetValueExW(self.0, n.as_ptr(), 0, REG_SZ, v.as_ptr().cast(), (v.len() * 2) as u32)
            };
            if code != ERROR_SUCCESS {
                return Err(fail(name, code));
            }
            Ok(())
        }

        fn set_empty(&self, name: &str) -> Result<()> {
            let n = wide(name);
            // SAFETY: no data.
            let code = unsafe { RegSetValueExW(self.0, n.as_ptr(), 0, REG_NONE, std::ptr::null(), 0) };
            if code != ERROR_SUCCESS {
                return Err(fail(name, code));
            }
            Ok(())
        }
    }

    fn get(root: HKEY, path: &str, name: &str) -> Option<String> {
        let (p, n) = (wide(path), wide(name));
        let mut buf = vec![0u16; 1024];
        let mut size = (buf.len() * 2) as u32;
        // SAFETY: `buf` holds `size` bytes.
        let code = unsafe {
            RegGetValueW(
                root,
                p.as_ptr(),
                n.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buf.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if code != ERROR_SUCCESS {
            return None;
        }
        let len = (size as usize / 2).min(buf.len());
        let text = String::from_utf16_lossy(&buf[..len]);
        Some(text.trim_end_matches('\0').to_string())
    }

    fn delete_tree(path: &str) {
        let p = wide(path);
        // SAFETY: valid string.
        unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, p.as_ptr()) };
    }

    fn delete_value(path: &str, name: &str) {
        let (p, n) = (wide(path), wide(name));
        // SAFETY: valid strings.
        unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, p.as_ptr(), n.as_ptr()) };
    }

    fn command(exe: &Path) -> String {
        format!("\"{}\" \"%1\"", exe.display())
    }

    fn label(kind: &Kind) -> &'static str {
        if crate::ui::i18n::Language::detect() == crate::ui::i18n::Language::German {
            kind.german
        } else {
            kind.english
        }
    }

    /// Tells Explorer that file types changed.
    fn notify(places: &Places) {
        if !places.real {
            return;
        }
        use windows_sys::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};
        // SAFETY: no items are passed.
        unsafe {
            SHChangeNotify(SHCNE_ASSOCCHANGED as i32, SHCNF_IDLIST, std::ptr::null(), std::ptr::null())
        };
    }

    /// The program Windows' choice of the user names for `.ext`, if any.
    fn user_choice(ext: &str) -> Option<String> {
        let base = format!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\.{ext}");
        // Newer Windows 11 versions keep the choice in "UserChoiceLatest".
        get(HKEY_CURRENT_USER, &format!(r"{base}\UserChoiceLatest"), "ProgId")
            .or_else(|| get(HKEY_CURRENT_USER, &format!(r"{base}\UserChoice"), "ProgId"))
    }

    fn is_ours(prog_id: &str, kind: &Kind) -> bool {
        prog_id.eq_ignore_ascii_case(kind.prog_id)
            || prog_id.eq_ignore_ascii_case(&format!(r"Applications\{EXE_NAME}"))
    }

    pub fn registered_program(places: &Places, kind: &Kind) -> Option<PathBuf> {
        let cmd = get(
            HKEY_CURRENT_USER,
            &format!(r"{}\Classes\{}\shell\open\command", places.base, kind.prog_id),
            "",
        )?;
        let exe = cmd.strip_prefix('"')?.split('"').next()?;
        Some(PathBuf::from(exe))
    }

    pub fn state(places: &Places, kind: &Kind) -> State {
        let classes = format!(r"{}\Classes", places.base);
        if places.real {
            if let Some(choice) = user_choice(kind.ext) {
                if is_ours(&choice, kind) {
                    return State::Default;
                }
            } else if get(HKEY_CLASSES_ROOT, &format!(".{}", kind.ext), "").is_some_and(|p| is_ours(&p, kind))
            {
                return State::Default;
            }
            if get(HKEY_CLASSES_ROOT, &format!(r"{}\shell\open\command", kind.prog_id), "").is_some() {
                return State::Offered;
            }
            return State::Off;
        }
        match get(HKEY_CURRENT_USER, &format!(r"{classes}\.{}", kind.ext), "") {
            Some(p) if is_ours(&p, kind) => State::Default,
            _ if registered_program(places, kind).is_some() => State::Offered,
            _ => State::Off,
        }
    }

    pub fn register(places: &Places, exe: &Path, kinds: &[Kind]) -> Result<Outcome> {
        let classes = format!(r"{}\Classes", places.base);
        let icon = format!("\"{}\",0", exe.display());
        let caps = format!(r"{}\{APP}\Capabilities", places.base);
        let cap = Key::create(&caps)?;
        cap.set("ApplicationName", APP)?;
        cap.set(
            "ApplicationDescription",
            if crate::ui::i18n::Language::detect() == crate::ui::i18n::Language::German {
                "Kostenloser PDF-Betrachter und -Editor"
            } else {
                "Free PDF viewer and editor"
            },
        )?;
        let assoc = Key::create(&format!(r"{caps}\FileAssociations"))?;
        for kind in kinds {
            let prog = Key::create(&format!(r"{classes}\{}", kind.prog_id))?;
            prog.set("", label(kind))?;
            Key::create(&format!(r"{classes}\{}\DefaultIcon", kind.prog_id))?.set("", &icon)?;
            Key::create(&format!(r"{classes}\{}\shell\open\command", kind.prog_id))?
                .set("", &command(exe))?;
            Key::create(&format!(r"{classes}\.{}\OpenWithProgids", kind.ext))?.set_empty(kind.prog_id)?;
            // Used by Windows when the user has not chosen a program.
            Key::create(&format!(r"{classes}\.{}", kind.ext))?.set("", kind.prog_id)?;
            Key::create(&format!(r"{classes}\Applications\{EXE_NAME}\SupportedTypes"))?
                .set(&format!(".{}", kind.ext), "")?;
            assoc.set(&format!(".{}", kind.ext), kind.prog_id)?;
        }
        let app = Key::create(&format!(r"{classes}\Applications\{EXE_NAME}"))?;
        app.set("FriendlyAppName", APP)?;
        Key::create(&format!(r"{classes}\Applications\{EXE_NAME}\shell\open\command"))?
            .set("", &command(exe))?;
        Key::create(&format!(r"{}\RegisteredApplications", places.base))?.set(APP, &caps)?;
        notify(places);
        // A program the user chose stays until the user changes it.
        let blocked =
            places.real && kinds.iter().any(|k| user_choice(k.ext).is_some_and(|c| !is_ours(&c, k)));
        if blocked {
            open_default_apps();
            return Ok(Outcome::ConfirmInSettings);
        }
        Ok(Outcome::Done)
    }

    pub fn unregister(places: &Places, kinds: &[Kind]) -> Result<Outcome> {
        let classes = format!(r"{}\Classes", places.base);
        for kind in kinds {
            delete_tree(&format!(r"{classes}\{}", kind.prog_id));
            delete_value(&format!(r"{classes}\.{}\OpenWithProgids", kind.ext), kind.prog_id);
            if get(HKEY_CURRENT_USER, &format!(r"{classes}\.{}", kind.ext), "")
                .is_some_and(|p| is_ours(&p, kind))
            {
                delete_value(&format!(r"{classes}\.{}", kind.ext), "");
            }
            delete_value(
                &format!(r"{classes}\Applications\{EXE_NAME}\SupportedTypes"),
                &format!(".{}", kind.ext),
            );
            delete_value(
                &format!(r"{}\{APP}\Capabilities\FileAssociations", places.base),
                &format!(".{}", kind.ext),
            );
        }
        let left =
            super::kinds().iter().any(|k| !kinds.contains(k) && registered_program(places, k).is_some());
        if !left {
            delete_tree(&format!(r"{classes}\Applications\{EXE_NAME}"));
            delete_tree(&format!(r"{}\{APP}\Capabilities", places.base));
            delete_value(&format!(r"{}\RegisteredApplications", places.base), APP);
        }
        notify(places);
        Ok(Outcome::Done)
    }

    /// Windows' settings page "Standard-Apps" for VersoloPDF.
    fn open_default_apps() {
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let (verb, uri) = (wide("open"), wide(&format!("ms-settings:defaultapps?registeredAppUser={APP}")));
        // SAFETY: valid strings; no window handle.
        unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                uri.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
    }
}

// ---------------------------------------------------------------------------
// macOS

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod sys {
    use std::path::{Path, PathBuf};

    use objc2_core_foundation::{CFRetained, CFString};

    use super::{Kind, Outcome, Places, State};
    use crate::error::{Error, Result};

    const BUNDLE_ID: &str = "io.github.mrmarkus42.versolopdf";
    /// kLSRolesAll.
    const ROLES_ALL: u32 = 0xFFFF_FFFF;

    #[link(name = "CoreServices", kind = "framework")]
    unsafe extern "C" {
        fn LSSetDefaultRoleHandlerForContentType(
            content_type: *const CFString,
            role: u32,
            handler: *const CFString,
        ) -> i32;
        fn LSCopyDefaultRoleHandlerForContentType(content_type: *const CFString, role: u32) -> *mut CFString;
    }

    fn handler(kind: &Kind) -> Option<String> {
        let uti = CFString::from_str(kind.uti);
        // SAFETY: a valid CFString; the result is owned (+1) or null.
        let raw = unsafe { LSCopyDefaultRoleHandlerForContentType(&*uti, ROLES_ALL) };
        let owned = std::ptr::NonNull::new(raw).map(|p| unsafe { CFRetained::from_raw(p) })?;
        Some(owned.to_string())
    }

    fn set_handler(kind: &Kind, bundle: &str) -> Result<()> {
        let (uti, id) = (CFString::from_str(kind.uti), CFString::from_str(bundle));
        // SAFETY: valid CFStrings.
        let status = unsafe { LSSetDefaultRoleHandlerForContentType(&*uti, ROLES_ALL, &*id) };
        if status != 0 {
            return Err(Error::Invalid(format!("Launch Services: Fehler {status}")));
        }
        Ok(())
    }

    pub fn registered_program(_places: &Places, _kind: &Kind) -> Option<PathBuf> {
        // Launch Services finds the app by its identifier wherever it is.
        None
    }

    pub fn state(_places: &Places, kind: &Kind) -> State {
        match handler(kind) {
            Some(h) if h.eq_ignore_ascii_case(BUNDLE_ID) => State::Default,
            // The app declares its document types itself.
            _ => State::Offered,
        }
    }

    pub fn register(_places: &Places, _exe: &Path, kinds: &[Kind]) -> Result<Outcome> {
        for kind in kinds {
            set_handler(kind, BUNDLE_ID)?;
        }
        Ok(Outcome::Done)
    }

    pub fn unregister(_places: &Places, kinds: &[Kind]) -> Result<Outcome> {
        for kind in kinds {
            // PDF files go back to Preview; other files have no other program.
            if *kind == super::PDF && state(_places, kind) == State::Default {
                set_handler(kind, "com.apple.Preview")?;
            }
        }
        Ok(Outcome::Done)
    }
}

// ---------------------------------------------------------------------------
// Linux and other Unix systems (freedesktop.org)

#[cfg(all(unix, not(target_os = "macos")))]
mod sys {
    use std::path::{Path, PathBuf};

    use super::{Kind, Outcome, Places, State};
    use crate::error::{Error, Result};

    const DESKTOP: &str = "versolopdf.desktop";
    const SECTION: &str = "[Default Applications]";

    fn desktop_file(places: &Places) -> PathBuf {
        places.data.join("applications").join(DESKTOP)
    }

    fn mimeapps(places: &Places) -> PathBuf {
        places.config.join("mimeapps.list")
    }

    /// The program a `.desktop` file starts (its Exec line, first word).
    pub fn registered_program(places: &Places, kind: &Kind) -> Option<PathBuf> {
        let text = std::fs::read_to_string(desktop_file(places)).ok()?;
        let has_kind =
            text.lines().any(|l| l.starts_with("MimeType=") && l.split([';', '=']).any(|m| m == kind.mime));
        if !has_kind {
            return None;
        }
        let exec = text.lines().find_map(|l| l.strip_prefix("Exec="))?;
        let exe = exec
            .strip_prefix('"')
            .and_then(|e| e.split('"').next())
            .unwrap_or_else(|| exec.split(' ').next().unwrap_or(""));
        Some(PathBuf::from(exe))
    }

    /// The default program for `mime` in mimeapps.list.
    fn default_for(places: &Places, mime: &str) -> Option<String> {
        let text = std::fs::read_to_string(mimeapps(places)).ok()?;
        let mut in_section = false;
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                in_section = line == SECTION;
            } else if in_section
                && let Some((key, value)) = line.split_once('=')
                && key.trim() == mime
            {
                return value.split(';').next().map(|v| v.trim().to_string());
            }
        }
        None
    }

    /// Sets (Some) or removes (None) the default for `mime`, keeping
    /// everything else in the file.
    fn set_default(places: &Places, mime: &str, value: Option<&str>) -> Result<()> {
        let path = mimeapps(places);
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let mut out: Vec<String> = Vec::new();
        let (mut in_section, mut seen_section, mut done) = (false, false, false);
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('[') {
                if in_section
                    && !done
                    && let Some(v) = value
                {
                    out.push(format!("{mime}={v};"));
                    done = true;
                }
                in_section = trimmed == SECTION;
                seen_section |= in_section;
            } else if in_section && trimmed.split_once('=').is_some_and(|(k, _)| k.trim() == mime) {
                if let Some(v) = value
                    && !done
                {
                    out.push(format!("{mime}={v};"));
                    done = true;
                }
                continue;
            }
            out.push(line.to_string());
        }
        if let Some(v) = value
            && !done
        {
            if !seen_section {
                if out.last().is_some_and(|l| !l.trim().is_empty()) {
                    out.push(String::new());
                }
                out.push(SECTION.to_string());
            }
            out.push(format!("{mime}={v};"));
        }
        std::fs::create_dir_all(&places.config).map_err(|e| Error::io(&places.config, e))?;
        let mut all = out.join("\n");
        all.push('\n');
        std::fs::write(&path, all).map_err(|e| Error::io(&path, e))
    }

    pub fn state(places: &Places, kind: &Kind) -> State {
        if registered_program(places, kind).is_none() {
            State::Off
        } else if default_for(places, kind.mime).as_deref() == Some(DESKTOP) {
            State::Default
        } else {
            State::Offered
        }
    }

    pub fn register(places: &Places, exe: &Path, kinds: &[Kind]) -> Result<Outcome> {
        let file = desktop_file(places);
        let dir = file.parent().unwrap_or(&places.data).to_path_buf();
        std::fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
        let mimes: String = super::kinds()
            .iter()
            .filter(|k| kinds.contains(k) || registered_program(places, k).is_some())
            .map(|k| format!("{};", k.mime))
            .collect();
        let text = format!(
            "[Desktop Entry]\nType=Application\nName=VersoloPDF\nComment=PDF viewer and editor\nExec=\"{}\" %F\nIcon=versolopdf\nTerminal=false\nCategories=Office;Viewer;\nMimeType={mimes}\n",
            exe.display()
        );
        std::fs::write(&file, text).map_err(|e| Error::io(&file, e))?;
        for kind in kinds {
            set_default(places, kind.mime, Some(DESKTOP))?;
        }
        Ok(Outcome::Done)
    }

    pub fn unregister(places: &Places, kinds: &[Kind]) -> Result<Outcome> {
        for kind in kinds {
            if default_for(places, kind.mime).as_deref() == Some(DESKTOP) {
                set_default(places, kind.mime, None)?;
            }
        }
        let left: Vec<Kind> = super::kinds()
            .iter()
            .copied()
            .filter(|k| !kinds.contains(k) && registered_program(places, k).is_some())
            .collect();
        let file = desktop_file(places);
        if left.is_empty() {
            std::fs::remove_file(&file).ok();
        }
        Ok(Outcome::Done)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn linux_registration_is_written_and_removed() {
        let dir = tempfile::tempdir().unwrap();
        let places = Places { data: dir.path().join("data"), config: dir.path().join("config"), real: false };
        let exe = std::path::Path::new("/opt/Versolo PDF/versolopdf");
        // Other programs' entries stay untouched.
        std::fs::create_dir_all(&places.config).unwrap();
        std::fs::write(
            places.config.join("mimeapps.list"),
            "[Added Associations]\napplication/pdf=evince.desktop;\n\n[Default Applications]\ntext/plain=gedit.desktop;\napplication/pdf=evince.desktop;\n",
        )
        .unwrap();
        assert_eq!(sys::state(&places, &PDF), State::Off);
        assert_eq!(set_in(&places, exe, &[PDF], true).unwrap(), Outcome::Done);
        assert_eq!(sys::state(&places, &PDF), State::Default);
        assert_eq!(sys::registered_program(&places, &PDF).as_deref(), Some(exe));
        let list = std::fs::read_to_string(places.config.join("mimeapps.list")).unwrap();
        assert!(list.contains("text/plain=gedit.desktop;"), "{list}");
        assert!(list.contains("application/pdf=versolopdf.desktop;"), "{list}");
        assert!(list.contains("[Added Associations]\napplication/pdf=evince.desktop;"), "{list}");
        assert_eq!(list.matches("application/pdf=").count(), 2, "{list}");
        // Removing: the default goes, the other program's entries stay.
        set_in(&places, exe, &[PDF], false).unwrap();
        assert_eq!(sys::state(&places, &PDF), State::Off);
        let list = std::fs::read_to_string(places.config.join("mimeapps.list")).unwrap();
        assert!(!list.contains("versolopdf"), "{list}");
        assert!(list.contains("text/plain=gedit.desktop;"), "{list}");
        // Without any mimeapps.list the section is created.
        std::fs::remove_file(places.config.join("mimeapps.list")).unwrap();
        set_in(&places, exe, &[PDF], true).unwrap();
        let list = std::fs::read_to_string(places.config.join("mimeapps.list")).unwrap();
        assert_eq!(list, "[Default Applications]\napplication/pdf=versolopdf.desktop;\n");
        // A program chosen elsewhere makes it "offered" only.
        std::fs::write(
            places.config.join("mimeapps.list"),
            "[Default Applications]\napplication/pdf=okular.desktop;\n",
        )
        .unwrap();
        assert_eq!(sys::state(&places, &PDF), State::Offered);
    }

    #[cfg(windows)]
    #[test]
    fn windows_registration_is_written_and_removed() {
        // A test key below HKEY_CURRENT_USER, never the real "Software\Classes".
        let base = format!(r"Software\VersoloPDF-Test-{}", std::process::id());
        let places = Places { base: base.clone(), real: false };
        let exe = std::path::Path::new(r"C:\Programme\Versolo PDF\VersoloPDF.exe");
        assert_eq!(sys::state(&places, &PDF), State::Off);
        assert_eq!(set_in(&places, exe, &[PDF, SESSION], true).unwrap(), Outcome::Done);
        assert_eq!(sys::state(&places, &PDF), State::Default);
        assert_eq!(sys::state(&places, &SESSION), State::Default);
        assert_eq!(sys::registered_program(&places, &PDF).as_deref(), Some(exe));
        set_in(&places, exe, &[SESSION], false).unwrap();
        assert_eq!(sys::state(&places, &SESSION), State::Off);
        assert_eq!(sys::state(&places, &PDF), State::Default);
        set_in(&places, exe, &[PDF], false).unwrap();
        assert_eq!(sys::state(&places, &PDF), State::Off);
        // Clean up the test key.
        let p: Vec<u16> = base.encode_utf16().chain(std::iter::once(0)).collect();
        #[allow(unsafe_code)]
        // SAFETY: valid string.
        unsafe {
            windows_sys::Win32::System::Registry::RegDeleteTreeW(
                windows_sys::Win32::System::Registry::HKEY_CURRENT_USER,
                p.as_ptr(),
            )
        };
    }

    #[test]
    fn tests_never_change_the_real_system() {
        // VERSOLOPDF_NO_EXTERNAL is set for the UI tests; then nothing happens.
        if crate::mail::no_external() {
            assert!(set(&[PDF], true).is_err());
        }
    }
}
