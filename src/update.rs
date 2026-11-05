//! "Neue Version": looks for the newest VersoloPDF release on GitHub and, on
//! Windows, installs it.
//!
//! Like "Von URL öffnen", the system's `curl` program does the network work
//! (VersoloPDF has no own network or TLS library). The release package is only
//! taken from this project's GitHub releases, must match its SHA-256
//! checksum and is unpacked with the `tar` program that is part of Windows.
//! The new program then copies itself into the program folder
//! (`--install-update`, with administrator rights when the folder is
//! write-protected); the running files are renamed to "….old" first, which
//! Windows allows. Afterwards the new version is started.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{Error, Result};

/// GitHub repository ("owner/name") whose releases are used.
pub const REPOSITORY: &str = "MrMarkus42/VersoloPDF";

/// Largest accepted release package.
const MAX_PACKAGE: u64 = 512 * 1024 * 1024;
/// Largest accepted answer of the GitHub API.
const MAX_ANSWER: usize = 2 * 1024 * 1024;

/// A downloadable file of a release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
    /// Lower-case hex SHA-256 from GitHub ("digest"), if given.
    pub sha256: Option<String>,
}

/// The newest release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// Version without a leading "v" ("0.2.0").
    pub version: String,
    /// Release notes (Markdown text as written on GitHub).
    pub notes: String,
    /// The release page on GitHub.
    pub page: String,
    /// The Windows package ("…-windows-x64.zip").
    pub windows: Option<Asset>,
    /// Its checksum file ("….zip.sha256"), used when GitHub gives no digest.
    pub windows_sha: Option<Asset>,
}

/// The running version.
pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// "v1.2.3" / "1.2" / "1.2.3-beta" as numbers (pre-release text ignored).
pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let text = text.trim().trim_start_matches(['v', 'V']);
    let core = text.split(['-', '+', ' ']).next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
    let major = parts.next()??;
    let minor = parts.next().unwrap_or(Some(0))?;
    let patch = parts.next().unwrap_or(Some(0))?;
    Some((major, minor, patch))
}

/// True if `candidate` is a higher version than `current`.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// A small JSON reader (enough for the GitHub answer; no extra library).

#[derive(Debug, Clone, PartialEq)]
enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    fn str(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }
}

struct Reader<'a> {
    s: &'a [u8],
    i: usize,
}

const MAX_DEPTH: usize = 64;

impl Reader<'_> {
    fn ws(&mut self) {
        while self.s.get(self.i).is_some_and(|c| matches!(c, b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: u8) -> Option<()> {
        self.ws();
        (self.s.get(self.i) == Some(&c)).then(|| self.i += 1)
    }

    fn word(&mut self, w: &[u8], v: Json) -> Option<Json> {
        self.s[self.i..].starts_with(w).then(|| {
            self.i += w.len();
            v
        })
    }

    fn value(&mut self, depth: usize) -> Option<Json> {
        if depth > MAX_DEPTH {
            return None;
        }
        self.ws();
        match *self.s.get(self.i)? {
            b'{' => {
                self.i += 1;
                let mut items = Vec::new();
                if self.eat(b'}').is_some() {
                    return Some(Json::Obj(items));
                }
                loop {
                    self.ws();
                    let key = self.string()?;
                    self.eat(b':')?;
                    items.push((key, self.value(depth + 1)?));
                    if self.eat(b',').is_none() {
                        self.eat(b'}')?;
                        return Some(Json::Obj(items));
                    }
                }
            }
            b'[' => {
                self.i += 1;
                let mut items = Vec::new();
                if self.eat(b']').is_some() {
                    return Some(Json::Arr(items));
                }
                loop {
                    items.push(self.value(depth + 1)?);
                    if self.eat(b',').is_none() {
                        self.eat(b']')?;
                        return Some(Json::Arr(items));
                    }
                }
            }
            b'"' => self.string().map(Json::Str),
            b't' => self.word(b"true", Json::Bool(true)),
            b'f' => self.word(b"false", Json::Bool(false)),
            b'n' => self.word(b"null", Json::Null),
            _ => {
                let start = self.i;
                while self
                    .s
                    .get(self.i)
                    .is_some_and(|c| matches!(c, b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9'))
                {
                    self.i += 1;
                }
                std::str::from_utf8(&self.s[start..self.i]).ok()?.parse().ok().map(Json::Num)
            }
        }
    }

    fn hex4(&mut self) -> Option<u32> {
        let h = self.s.get(self.i..self.i + 4)?;
        self.i += 4;
        u32::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok()
    }

    fn string(&mut self) -> Option<String> {
        if self.s.get(self.i) != Some(&b'"') {
            return None;
        }
        self.i += 1;
        let mut out = Vec::new();
        loop {
            let c = *self.s.get(self.i)?;
            self.i += 1;
            match c {
                b'"' => return String::from_utf8(out).ok(),
                b'\\' => {
                    let e = *self.s.get(self.i)?;
                    self.i += 1;
                    let ch = match e {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let mut code = self.hex4()?;
                            // A surrogate pair.
                            if (0xD800..0xDC00).contains(&code) && self.s[self.i..].starts_with(b"\\u") {
                                self.i += 2;
                                let low = self.hex4()?;
                                code = 0x10000 + ((code - 0xD800) << 10) + (low.checked_sub(0xDC00)? & 0x3FF);
                            }
                            char::from_u32(code).unwrap_or('\u{FFFD}')
                        }
                        _ => return None,
                    };
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
                _ => out.push(c),
            }
        }
    }
}

fn parse_json(text: &str) -> Option<Json> {
    let mut r = Reader { s: text.as_bytes(), i: 0 };
    let v = r.value(0)?;
    r.ws();
    (r.i == r.s.len()).then_some(v)
}

/// Reads GitHub's answer for "the latest release".
pub fn parse_release(text: &str) -> Result<Release> {
    let bad = || Error::Invalid("unexpected answer from GitHub".into());
    let json = parse_json(text).ok_or_else(bad)?;
    let tag = json.str("tag_name").ok_or_else(bad)?;
    let version = tag.trim().trim_start_matches(['v', 'V']).to_string();
    if parse_version(&version).is_none() {
        return Err(bad());
    }
    let mut release = Release {
        version,
        notes: json.str("body").unwrap_or_default().chars().take(20_000).collect(),
        page: json.str("html_url").unwrap_or_default().to_string(),
        windows: None,
        windows_sha: None,
    };
    if let Some(Json::Arr(assets)) = json.get("assets") {
        for a in assets {
            let (Some(name), Some(url)) = (a.str("name"), a.str("browser_download_url")) else { continue };
            if !is_release_url(url) {
                continue;
            }
            let size = match a.get("size") {
                Some(Json::Num(n)) if *n >= 0.0 => *n as u64,
                _ => 0,
            };
            let sha256 = a
                .str("digest")
                .and_then(|d| d.strip_prefix("sha256:"))
                .map(str::to_ascii_lowercase)
                .filter(|h| is_sha256_hex(h));
            let asset = Asset { name: name.to_string(), url: url.to_string(), size, sha256 };
            let lower = name.to_ascii_lowercase();
            if lower.ends_with("-windows-x64.zip") {
                release.windows = Some(asset);
            } else if lower.ends_with("-windows-x64.zip.sha256") {
                release.windows_sha = Some(asset);
            }
        }
    }
    Ok(release)
}

/// Only files of this project's releases on GitHub are downloaded.
fn is_release_url(url: &str) -> bool {
    url.starts_with(&format!("https://github.com/{REPOSITORY}/releases/download/"))
        && !url.chars().any(|c| c.is_control() || c.is_whitespace())
        && !url.contains("/../")
}

fn is_sha256_hex(h: &str) -> bool {
    h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit())
}

// ---------------------------------------------------------------------------
// Network (curl)

fn curl() -> Command {
    let mut cmd = Command::new("curl");
    cmd.args(["--silent", "--show-error", "--location"])
        .args(["--proto", "=https", "--proto-redir", "=https"])
        .args(["--max-redirs", "10", "--connect-timeout", "20"])
        .args(["--user-agent", "VersoloPDF-update-check"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window for the helper program.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

fn curl_error(output: &std::process::Output) -> Error {
    let message = String::from_utf8_lossy(&output.stderr).trim().chars().take(400).collect::<String>();
    Error::Invalid(if message.is_empty() { "download failed".into() } else { message })
}

/// Asks GitHub for the latest release. `Ok(None)`: the project has no
/// (public) release yet.
pub fn fetch_latest() -> Result<Option<Release>> {
    let url = format!("https://api.github.com/repos/{REPOSITORY}/releases/latest");
    let output = curl()
        .args(["--max-time", "30", "--header", "Accept: application/vnd.github+json"])
        .args(["--write-out", "\n%{http_code}"])
        .arg(url)
        .output()
        .map_err(|e| Error::Invalid(format!("curl: {e}")))?;
    if !output.status.success() {
        return Err(curl_error(&output));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let (body, code) = text.rsplit_once('\n').unwrap_or(("", text.as_ref()));
    match code.trim() {
        "200" if body.len() <= MAX_ANSWER => parse_release(body).map(Some),
        "404" => Ok(None),
        other => Err(Error::Invalid(format!("GitHub: HTTP {other}"))),
    }
}

fn sha256_file(path: &Path) -> Result<String> {
    use sha2::Digest;
    let mut file = std::fs::File::open(path).map_err(|e| Error::io(path, e))?;
    let mut hasher = sha2::Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = std::io::Read::read(&mut file, &mut buf).map_err(|e| Error::io(path, e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn download(asset: &Asset, dest: &Path, max: u64) -> Result<()> {
    let output = curl()
        .args(["--fail", "--max-time", "1800", "--max-filesize", &max.to_string()])
        .arg("--output")
        .arg(dest)
        .arg(&asset.url)
        .output()
        .map_err(|e| Error::Invalid(format!("curl: {e}")))?;
    if !output.status.success() {
        std::fs::remove_file(dest).ok();
        return Err(curl_error(&output));
    }
    Ok(())
}

/// Downloads the Windows package of `release` into `dir` and checks its
/// SHA-256 checksum (GitHub's digest or the release's ".sha256" file).
pub fn download_package(release: &Release, dir: &Path) -> Result<PathBuf> {
    let asset = release.windows.as_ref().ok_or_else(|| Error::Invalid("no Windows package".into()))?;
    std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
    let expected = match (&asset.sha256, &release.windows_sha) {
        (Some(h), _) => h.clone(),
        (None, Some(sha_file)) => {
            let path = dir.join("package.sha256");
            download(sha_file, &path, 4096)?;
            let text = std::fs::read_to_string(&path).map_err(|e| Error::io(&path, e))?;
            let hex = text.split_whitespace().next().unwrap_or_default().to_ascii_lowercase();
            if !is_sha256_hex(&hex) {
                return Err(Error::Invalid("invalid checksum file".into()));
            }
            hex
        }
        (None, None) => return Err(Error::Invalid("the release has no checksum".into())),
    };
    let dest = dir.join("VersoloPDF-update.zip");
    download(asset, &dest, MAX_PACKAGE)?;
    if sha256_file(&dest)? != expected {
        std::fs::remove_file(&dest).ok();
        return Err(Error::Invalid("checksum mismatch – the download was damaged or changed".into()));
    }
    Ok(dest)
}

/// The folder in `dir` (or below) that holds `VersoloPDF.exe`.
fn program_folder(dir: &Path, depth: usize) -> Option<PathBuf> {
    if dir.join("VersoloPDF.exe").is_file() {
        return Some(dir.to_path_buf());
    }
    if depth == 0 {
        return None;
    }
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.path().is_dir())
        .find_map(|e| program_folder(&e.path(), depth - 1))
}

/// True if new files can be written into `dir`.
pub fn folder_writable(dir: &Path) -> bool {
    let probe = dir.join(".versolopdf-update-probe");
    let ok = std::fs::write(&probe, b"x").is_ok();
    std::fs::remove_file(&probe).ok();
    ok
}

/// Unpacks the package; returns the folder with the new `VersoloPDF.exe`.
/// Windows only (uses the `tar.exe` of Windows).
pub fn unpack(zip: &Path) -> Result<PathBuf> {
    let work = zip.parent().ok_or_else(|| Error::Invalid("no folder".into()))?.join("files");
    std::fs::remove_dir_all(&work).ok();
    std::fs::create_dir_all(&work).map_err(|e| Error::io(&work, e))?;
    // tar.exe of Windows (also reads zip files), with its full path.
    let system = std::env::var_os("SystemRoot").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
    let mut tar = Command::new(system.join("System32").join("tar.exe"));
    tar.arg("-xf").arg(zip).arg("-C").arg(&work);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        tar.creation_flags(0x0800_0000);
    }
    let output = tar.output().map_err(|e| Error::Invalid(format!("tar: {e}")))?;
    if !output.status.success() {
        return Err(curl_error(&output));
    }
    program_folder(&work, 2).ok_or_else(|| Error::Invalid("VersoloPDF.exe not found in the package".into()))
}

/// Argument with which the new program copies itself into a folder.
pub const INSTALL_ARG: &str = "--install-update";
/// Argument with which the new version waits until the old one has closed
/// (followed by its process id).
pub const WAIT_ARG: &str = "--after-update";

/// Waits until the process `pid` has ended, at most `seconds`. Returns
/// false if it still runs.
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn wait_for_exit(pid: u32, seconds: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject};
    // SAFETY: plain calls; the handle is checked and closed once.
    unsafe {
        let handle = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if handle.is_null() {
            return true; // already gone
        }
        let done = WaitForSingleObject(handle, seconds.saturating_mul(1000)) == WAIT_OBJECT_0;
        CloseHandle(handle);
        done
    }
}

#[cfg(not(windows))]
pub fn wait_for_exit(pid: u32, seconds: u32) -> bool {
    let proc = PathBuf::from(format!("/proc/{pid}"));
    for _ in 0..seconds * 10 {
        if !proc.exists() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    !proc.exists()
}
/// Ending of files replaced by an update (removed by the next update).
const OLD: &str = "old";

/// All files below `dir` (relative paths), at most a few folders deep.
fn files_below(dir: &Path, rel: &Path, depth: usize, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir.join(rel))? {
        let entry = entry?;
        let path = rel.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            if depth > 0 {
                files_below(dir, &path, depth - 1, out)?;
            }
        } else {
            out.push(path);
        }
    }
    Ok(())
}

fn with_old_ending(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".");
    name.push(OLD);
    path.with_file_name(name)
}

/// Copies the program files of `source` into `target`. Files that exist
/// are first renamed to "….old": Windows allows that even for the running
/// program and its libraries, so the old version may still run. If
/// anything fails, the old files are put back. Returns the number of files.
pub fn install_files(source: &Path, target: &Path) -> Result<usize> {
    if !source.join("VersoloPDF.exe").is_file() && !source.join("versolopdf").is_file() {
        return Err(Error::Invalid("no program in the source folder".into()));
    }
    let mut files = Vec::new();
    files_below(source, Path::new(""), 4, &mut files).map_err(|e| Error::io(source, e))?;
    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut created: Vec<PathBuf> = Vec::new();
    let result = (|| -> Result<()> {
        for rel in &files {
            let (src, dest) = (source.join(rel), target.join(rel));
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
            }
            if dest.exists() {
                let old = with_old_ending(&dest);
                std::fs::remove_file(&old).ok();
                std::fs::rename(&dest, &old).map_err(|e| Error::io(&dest, e))?;
                moved.push((dest.clone(), old));
            }
            std::fs::copy(&src, &dest).map_err(|e| Error::io(&dest, e))?;
            created.push(dest);
        }
        Ok(())
    })();
    if let Err(err) = result {
        // Back to the old version.
        for f in &created {
            std::fs::remove_file(f).ok();
        }
        for (dest, old) in moved.iter().rev() {
            std::fs::rename(old, dest).ok();
        }
        return Err(err);
    }
    // Only the files renamed here are removed later (never other ".old" files).
    let list: String = moved
        .iter()
        .filter_map(|(_, old)| old.strip_prefix(target).ok())
        .map(|rel| format!("{}\n", rel.display()))
        .collect();
    let list_path = target.join(LEFTOVER_LIST);
    let mut all = std::fs::read_to_string(&list_path).unwrap_or_default();
    all.push_str(&list);
    std::fs::write(&list_path, all).map_err(|e| Error::io(&list_path, e))?;
    Ok(files.len())
}

/// List of the "….old" files an update left behind.
const LEFTOVER_LIST: &str = "update-old-files.txt";

/// Removes the "….old" files of an earlier update in `dir` (if allowed).
pub fn remove_leftovers(dir: &Path) {
    let list_path = dir.join(LEFTOVER_LIST);
    let Ok(list) = std::fs::read_to_string(&list_path) else { return };
    let mut left = String::new();
    for rel in list.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let path = dir.join(rel);
        // Only "….old" files inside the program folder.
        let safe = !rel.contains("..") && path.extension().is_some_and(|e| e == OLD);
        if safe && path.exists() && std::fs::remove_file(&path).is_err() {
            left.push_str(rel);
            left.push('\n');
        }
    }
    if left.is_empty() {
        std::fs::remove_file(&list_path).ok();
    } else {
        std::fs::write(&list_path, left).ok();
    }
}

/// Lets the new program (`new_exe`) copy itself into `target` and waits
/// until it is done. A write-protected folder (e.g. under "Programme")
/// makes Windows ask for administrator rights first.
pub fn run_installer(new_exe: &Path, target: &Path) -> Result<()> {
    if crate::mail::no_external() {
        return Err(Error::Invalid("Testbetrieb: keine Installation".into()));
    }
    let code = if folder_writable(target) {
        let mut cmd = Command::new(new_exe);
        cmd.arg(INSTALL_ARG).arg(target);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        cmd.status().map_err(|e| Error::Invalid(format!("{}: {e}", new_exe.display())))?.code().unwrap_or(1)
            as u32
    } else {
        elevated(new_exe, target)?
    };
    if code == 0 {
        Ok(())
    } else {
        Err(Error::Invalid(
            "Die neuen Dateien konnten nicht kopiert werden (Details im Fehlerprotokoll).".into(),
        ))
    }
}

/// Runs the installer with administrator rights (Windows asks the user).
#[cfg(windows)]
#[allow(unsafe_code)]
fn elevated(new_exe: &Path, target: &Path) -> Result<u32> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, INFINITE, WaitForSingleObject};
    use windows_sys::Win32::UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;
    let wide = |s: &std::ffi::OsStr| s.encode_wide().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let verb = wide(std::ffi::OsStr::new("runas"));
    let file = wide(new_exe.as_os_str());
    let params = wide(std::ffi::OsStr::new(&format!("{INSTALL_ARG} \"{}\"", target.display())));
    // SAFETY: plain C struct, all fields may be zero.
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS;
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.lpParameters = params.as_ptr();
    info.nShow = SW_HIDE;
    // SAFETY: `info` and the strings it points to live until the call returns.
    if unsafe { ShellExecuteExW(&mut info) } == 0 || info.hProcess.is_null() {
        let err = std::io::Error::last_os_error();
        // 1223: the user said no to the administrator question.
        return Err(Error::Invalid(if err.raw_os_error() == Some(1223) {
            "Die Installation wurde abgebrochen (keine Administratorrechte).".into()
        } else {
            format!("Installation nicht gestartet: {err}")
        }));
    }
    let mut code = 1u32;
    // SAFETY: valid process handle from ShellExecuteExW, closed once.
    unsafe {
        WaitForSingleObject(info.hProcess, INFINITE);
        GetExitCodeProcess(info.hProcess, &mut code);
        CloseHandle(info.hProcess);
    }
    Ok(code)
}

#[cfg(not(windows))]
fn elevated(_new_exe: &Path, target: &Path) -> Result<u32> {
    Err(Error::Invalid(format!("{} ist schreibgeschützt.", target.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_compared() {
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("0.2"), Some((0, 2, 0)));
        assert_eq!(parse_version("1.0.0-beta.1"), Some((1, 0, 0)));
        assert_eq!(parse_version("neu"), None);
        assert!(is_newer("v0.2.0", "0.1.0"));
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.0.9", "0.1.0"));
        assert!(!is_newer("kaputt", "0.1.0"));
    }

    #[test]
    fn release_answer_is_read() {
        let json = r#"{
            "tag_name": "v0.2.0", "html_url": "https://github.com/MrMarkus42/VersoloPDF/releases/tag/v0.2.0",
            "body": "Neu: \u00c4nderungen \ud83d\ude00\nZeile 2", "draft": false, "id": 12,
            "assets": [
              {"name": "VersoloPDF-0.2.0-windows-x64.zip", "size": 12345,
               "browser_download_url": "https://github.com/MrMarkus42/VersoloPDF/releases/download/v0.2.0/VersoloPDF-0.2.0-windows-x64.zip",
               "digest": "sha256:ABCDEF0123456789abcdef0123456789abcdef0123456789abcdef0123456789"},
              {"name": "VersoloPDF-0.2.0-windows-x64.zip.sha256", "size": 99,
               "browser_download_url": "https://github.com/MrMarkus42/VersoloPDF/releases/download/v0.2.0/VersoloPDF-0.2.0-windows-x64.zip.sha256",
               "digest": null},
              {"name": "evil-windows-x64.zip", "size": 1,
               "browser_download_url": "https://example.org/evil-windows-x64.zip"}
            ]
        }"#;
        let r = parse_release(json).unwrap();
        assert_eq!(r.version, "0.2.0");
        assert_eq!(r.notes, "Neu: Änderungen 😀\nZeile 2");
        let w = r.windows.unwrap();
        assert_eq!(w.size, 12345);
        assert_eq!(
            w.sha256.as_deref(),
            Some("abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789")
        );
        assert!(w.url.starts_with("https://github.com/MrMarkus42/VersoloPDF/releases/download/"));
        assert!(r.windows_sha.is_some());
        assert!(parse_release("{\"message\": \"Not Found\"}").is_err());
        assert!(parse_release("{\"tag_name\": ").is_err());
        assert!(parse_json(&"[".repeat(500)).is_none(), "deep nesting is refused");
    }

    #[test]
    fn files_are_installed_and_old_ones_kept_aside() {
        let base = std::env::temp_dir().join(format!("versolopdf-install-{}", std::process::id()));
        let (src, dst) = (base.join("neu"), base.join("Programme"));
        std::fs::create_dir_all(src.join("licenses")).unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        std::fs::write(src.join("VersoloPDF.exe"), b"neu").unwrap();
        std::fs::write(src.join("pdfium.dll"), b"neu-dll").unwrap();
        std::fs::write(src.join("licenses").join("a.txt"), b"lizenz").unwrap();
        std::fs::write(dst.join("VersoloPDF.exe"), b"alt").unwrap();
        std::fs::write(dst.join("eigene.txt"), b"bleibt").unwrap();
        std::fs::write(dst.join("fremd.txt.old"), b"bleibt auch").unwrap();
        assert_eq!(install_files(&src, &dst).unwrap(), 3);
        assert_eq!(std::fs::read(dst.join("VersoloPDF.exe")).unwrap(), b"neu");
        assert_eq!(std::fs::read(dst.join("VersoloPDF.exe.old")).unwrap(), b"alt");
        assert_eq!(std::fs::read(dst.join("licenses").join("a.txt")).unwrap(), b"lizenz");
        assert_eq!(std::fs::read(dst.join("eigene.txt")).unwrap(), b"bleibt");
        // A second update replaces the leftover; the clean-up removes it.
        std::fs::write(src.join("VersoloPDF.exe"), b"neuer").unwrap();
        install_files(&src, &dst).unwrap();
        assert_eq!(std::fs::read(dst.join("VersoloPDF.exe.old")).unwrap(), b"neu");
        remove_leftovers(&dst);
        assert!(!dst.join("VersoloPDF.exe.old").exists() && dst.join("VersoloPDF.exe").exists());
        // Other ".old" files are never touched.
        assert!(dst.join("fremd.txt.old").exists());
        assert!(!dst.join(LEFTOVER_LIST).exists());
        // Without a program in the source nothing happens.
        assert!(install_files(&dst.join("licenses"), &dst).is_err());
        std::fs::remove_dir_all(&base).ok();
    }
}
