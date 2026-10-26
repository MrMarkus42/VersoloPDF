//! File operations of the "Datei" menu: rename, move, recycle bin, and
//! recovery copies of unsaved documents.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Characters not allowed in file names on Windows (and `/` everywhere).
const FORBIDDEN: &[char] = &['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

/// Checks a new file name (without folder) and adds `.pdf` if missing.
pub fn clean_file_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.len() > 240
        || name.chars().any(|c| c.is_control() || FORBIDDEN.contains(&c))
    {
        return Err(Error::Invalid("invalid file name".into()));
    }
    let mut name = name.trim_end_matches('.').to_string();
    if !name.to_ascii_lowercase().ends_with(".pdf") {
        name.push_str(".pdf");
    }
    Ok(name)
}

fn already_exists(path: &Path) -> Error {
    Error::io(path, std::io::Error::new(std::io::ErrorKind::AlreadyExists, "file already exists"))
}

/// Moves `from` to `to`; falls back to copy + delete across drives.
fn move_path(from: &Path, to: &Path) -> Result<()> {
    if to.exists() {
        return Err(already_exists(to));
    }
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    std::fs::copy(from, to).map_err(|e| Error::io(to, e))?;
    if let Err(e) = std::fs::remove_file(from) {
        std::fs::remove_file(to).ok();
        return Err(Error::io(from, e));
    }
    Ok(())
}

/// "Umbenennen": gives the file a new name in the same folder.
pub fn rename_file(from: &Path, new_name: &str) -> Result<PathBuf> {
    let name = clean_file_name(new_name)?;
    let to = from.with_file_name(name);
    if to == from {
        return Ok(to);
    }
    // Only a different spelling (Windows file names ignore case).
    let same_file = to.to_string_lossy().eq_ignore_ascii_case(&from.to_string_lossy());
    if same_file {
        std::fs::rename(from, &to).map_err(|e| Error::io(from, e))?;
    } else {
        move_path(from, &to)?;
    }
    Ok(to)
}

/// "Datei verschieben": moves the file into `folder`.
pub fn move_file(from: &Path, folder: &Path) -> Result<PathBuf> {
    let name = from.file_name().ok_or_else(|| Error::Invalid("no file name".into()))?;
    let to = folder.join(name);
    if to == from {
        return Ok(to);
    }
    move_path(from, &to)?;
    Ok(to)
}

/// Local date and time `(YYYY, MM, DD, hh, mm, ss)` as strings.
fn now_parts() -> Option<[String; 6]> {
    let d = crate::pdf_edit::pdf_date_now();
    let digits: String = d.chars().filter(|c| c.is_ascii_digit()).take(14).collect();
    (digits.len() == 14).then(|| {
        [&digits[0..4], &digits[4..6], &digits[6..8], &digits[8..10], &digits[10..12], &digits[12..14]]
            .map(str::to_string)
    })
}

/// "In den Papierkorb werfen": moves the file to the recycle bin of the
/// system (Windows: via PowerShell, macOS: via Finder, Linux: freedesktop
/// trash in the home folder).
pub fn move_to_trash(path: &Path) -> Result<()> {
    if !path.is_file() {
        return Err(Error::io(path, std::io::Error::new(std::io::ErrorKind::NotFound, "file not found")));
    }
    let path = path.canonicalize().map_err(|e| Error::io(path, e))?;
    trash_impl(&path)?;
    if path.exists() {
        return Err(Error::Invalid("the file could not be moved to the recycle bin".into()));
    }
    Ok(())
}

#[cfg(windows)]
fn trash_impl(path: &Path) -> Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // The path is passed in an environment variable, never as script text.
    let script = "Add-Type -AssemblyName Microsoft.VisualBasic; \
                  [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteFile($env:MRPDF_TRASH_PATH, 'OnlyErrorDialogs', 'SendToRecycleBin')";
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script])
        .env("MRPDF_TRASH_PATH", path)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| Error::Invalid(format!("powershell: {e}")))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(Error::Invalid(String::from_utf8_lossy(&output.stderr).trim().chars().take(400).collect()))
    }
}

#[cfg(target_os = "macos")]
fn trash_impl(path: &Path) -> Result<()> {
    let output = std::process::Command::new("osascript")
        .args([
            "-e",
            "on run argv",
            "-e",
            "tell application \"Finder\" to delete (POSIX file (item 1 of argv))",
        ])
        .args(["-e", "end run"])
        .arg(path)
        .output()
        .map_err(|e| Error::Invalid(format!("osascript: {e}")))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(Error::Invalid(String::from_utf8_lossy(&output.stderr).trim().chars().take(400).collect()))
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn trash_impl(path: &Path) -> Result<()> {
    let home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .ok_or_else(|| Error::Invalid("no home folder".into()))?;
    freedesktop_trash(path, &home.join("Trash"))
}

/// Moves `path` into a freedesktop.org trash folder (`files/` + `info/`).
#[cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]
fn freedesktop_trash(path: &Path, trash: &Path) -> Result<()> {
    let files = trash.join("files");
    let info = trash.join("info");
    for dir in [&files, &info] {
        std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.clone(), String::new()),
    };
    let mut candidate = name.clone();
    let mut n = 2;
    while files.join(&candidate).exists() || info.join(format!("{candidate}.trashinfo")).exists() {
        candidate = format!("{stem}.{n}{ext}");
        n += 1;
    }
    let encoded: String = path
        .to_string_lossy()
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    let date =
        now_parts().map(|[y, mo, d, h, mi, s]| format!("{y}-{mo}-{d}T{h}:{mi}:{s}")).unwrap_or_default();
    let info_file = info.join(format!("{candidate}.trashinfo"));
    std::fs::write(&info_file, format!("[Trash Info]\nPath={encoded}\nDeletionDate={date}\n"))
        .map_err(|e| Error::io(&info_file, e))?;
    if let Err(e) = std::fs::rename(path, files.join(&candidate)) {
        std::fs::remove_file(&info_file).ok();
        return Err(Error::io(path, e));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Recovery copies ("Informationen zum Wiederherstellen speichern")

/// Description of a recovery copy (stored next to it as RON).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RecoveryEntry {
    /// Name shown in the tab.
    pub name: String,
    /// File the document belongs to (None: never saved).
    pub original: Option<PathBuf>,
    /// Date and time of the copy, for display.
    pub saved: String,
}

/// Folder of the recovery copies in the user's data folder.
pub fn default_recovery_dir() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from).map(|p| p.join("MrPDF"))
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support/MrPDF"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .map(|p| p.join("mrpdf"))
    };
    base.map(|p| p.join("recovery"))
}

fn safe_id(id: &str) -> Result<&str> {
    if id.is_empty() || id.len() > 64 || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(Error::Invalid("invalid recovery id".into()));
    }
    Ok(id)
}

/// Writes (or replaces) the recovery copy `id`.
pub fn write_recovery(dir: &Path, id: &str, entry: &RecoveryEntry, pdf: &[u8]) -> Result<()> {
    let id = safe_id(id)?;
    std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
    let text = ron::to_string(entry).map_err(|e| Error::Invalid(e.to_string()))?;
    // PDF first: an entry without its PDF is ignored by `list_recovery`.
    for (ext, data) in [("pdf", pdf), ("ron", text.as_bytes())] {
        let tmp = dir.join(format!("{id}.{ext}.tmp"));
        let target = dir.join(format!("{id}.{ext}"));
        std::fs::write(&tmp, data).map_err(|e| Error::io(&tmp, e))?;
        std::fs::rename(&tmp, &target).map_err(|e| Error::io(&target, e))?;
    }
    Ok(())
}

/// Deletes the recovery copy `id` (if any).
pub fn remove_recovery(dir: &Path, id: &str) {
    if let Ok(id) = safe_id(id) {
        for ext in ["ron", "pdf"] {
            std::fs::remove_file(dir.join(format!("{id}.{ext}"))).ok();
        }
    }
}

/// All complete recovery copies: (id, entry).
pub fn list_recovery(dir: &Path) -> Vec<(String, RecoveryEntry)> {
    let Ok(read) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<(String, RecoveryEntry)> = read
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let id = path.file_name()?.to_str()?.strip_suffix(".ron")?.to_string();
            safe_id(&id).ok()?;
            if !dir.join(format!("{id}.pdf")).is_file() || e.metadata().ok()?.len() > 64 * 1024 {
                return None;
            }
            let entry: RecoveryEntry = ron::from_str(&std::fs::read_to_string(&path).ok()?).ok()?;
            Some((id, entry))
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out.truncate(100);
    out
}

/// The PDF of the recovery copy `id`.
pub fn read_recovery(dir: &Path, id: &str) -> Result<Vec<u8>> {
    let path = dir.join(format!("{}.pdf", safe_id(id)?));
    std::fs::read(&path).map_err(|e| Error::io(&path, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mrpdf-files-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn names_are_checked() {
        assert_eq!(clean_file_name(" Bericht ").unwrap(), "Bericht.pdf");
        assert_eq!(clean_file_name("a.PDF").unwrap(), "a.PDF");
        for bad in ["", "..", "a/b", "a\\b", "x:y", "a?", "a\nb"] {
            assert!(clean_file_name(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn rename_and_move() {
        let dir = temp("rename");
        let a = dir.join("a.pdf");
        std::fs::write(&a, b"%PDF-1.7").unwrap();
        std::fs::write(dir.join("taken.pdf"), b"x").unwrap();
        assert!(rename_file(&a, "taken").is_err(), "existing files are not overwritten");
        let b = rename_file(&a, "b").unwrap();
        assert_eq!(b, dir.join("b.pdf"));
        assert!(!a.exists() && b.exists());
        let sub = dir.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let moved = move_file(&b, &sub).unwrap();
        assert_eq!(moved, sub.join("b.pdf"));
        assert_eq!(std::fs::read(&moved).unwrap(), b"%PDF-1.7");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn freedesktop_trash_layout() {
        let dir = temp("trash");
        let trash = dir.join("Trash");
        for _ in 0..2 {
            let f = dir.join("weg damit.pdf");
            std::fs::write(&f, b"x").unwrap();
            freedesktop_trash(&f, &trash).unwrap();
            assert!(!f.exists());
        }
        assert!(trash.join("files/weg damit.pdf").exists());
        assert!(trash.join("files/weg damit.2.pdf").exists());
        let info = std::fs::read_to_string(trash.join("info/weg damit.pdf.trashinfo")).unwrap();
        assert!(info.starts_with("[Trash Info]\nPath=/") && info.contains("weg%20damit.pdf"), "{info}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn recovery_copies() {
        let dir = temp("recovery");
        let entry =
            RecoveryEntry { name: "a.pdf".into(), original: Some("/x/a.pdf".into()), saved: "1".into() };
        write_recovery(&dir, "12-3", &entry, b"%PDF-1.7 a").unwrap();
        write_recovery(&dir, "12-4", &RecoveryEntry::default(), b"%PDF b").unwrap();
        assert!(write_recovery(&dir, "../evil", &entry, b"x").is_err());
        let list = list_recovery(&dir);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0], ("12-3".to_string(), entry));
        assert_eq!(read_recovery(&dir, "12-3").unwrap(), b"%PDF-1.7 a");
        remove_recovery(&dir, "12-3");
        assert_eq!(list_recovery(&dir).len(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }
}
