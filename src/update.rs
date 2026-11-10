//! "Neue Version": looks for the newest VersoloPDF release on GitHub and, on
//! Windows and macOS, installs it.
//!
//! Like "Von URL öffnen", the system's `curl` program does the network work
//! (VersoloPDF has no own network or TLS library). The release package is only
//! taken from this project's GitHub releases, must carry a valid signature
//! of the project's update key (see [`verify_signature`]; the public key is
//! built into the program, the private key never leaves the developer's
//! machine), must match the SHA-256 checksum named in it and is unpacked
//! with the system's own program (`tar` on
//! Windows, `ditto` on macOS). The new program then installs itself
//! (`--install-update`, with administrator rights when the folder is
//! write-protected): on Windows it copies its files into the program
//! folder, the running files are renamed to "….old" first, which Windows
//! allows; on macOS the new bundle takes the place of the old one.
//! Afterwards the new version is started.

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
    /// Its signature ("….zip.sig").
    pub windows_sig: Option<Asset>,
    /// The macOS package ("…-macos.zip", the app for Intel and Apple chips).
    pub macos: Option<Asset>,
    pub macos_sha: Option<Asset>,
    pub macos_sig: Option<Asset>,
}

/// The package for this system and its checksum file, if the release has one.
pub fn package_of(release: &Release) -> Option<(&Asset, Option<&Asset>)> {
    if cfg!(windows) {
        release.windows.as_ref().map(|a| (a, release.windows_sha.as_ref()))
    } else if cfg!(target_os = "macos") {
        release.macos.as_ref().map(|a| (a, release.macos_sha.as_ref()))
    } else {
        None
    }
}

/// The signature file of the package for this system.
pub fn signature_of(release: &Release) -> Option<&Asset> {
    if cfg!(windows) {
        release.windows_sig.as_ref()
    } else if cfg!(target_os = "macos") {
        release.macos_sig.as_ref()
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Signatures of release packages
//
// A ".sig" file next to each package says (text, one item per line):
//
//     versolopdf-signature-v1
//     file: VersoloPDF-1.4.0-windows-x64.zip
//     sha256: <64 hex digits>
//     signature: <128 hex digits: ECDSA P-256 r‖s over SIGNED_PREFIX + file + "\n" + sha256 + "\n">
//
// The file name is signed too, so an older (also signed) version cannot be
// passed off as a newer one, and a Windows package not as a Mac one.

/// Public keys whose signatures are accepted (SEC1, uncompressed, hex). A
/// second key can be added before the first one is retired.
pub const UPDATE_KEYS: &[&str] = &[
    // Created 2026-11-10; the private key is kept by the developer.
    "04add7026fa7c4ca6b96cff63700c8c781ac0f7383bd15799272925734bcb780c2691bfb43399b14d3dd1b2cfaea1f53be034ba2b1f924853c13776d6a9916e38c",
];

const SIGNATURE_HEADER: &str = "versolopdf-signature-v1";
const SIGNED_PREFIX: &str = "versolopdf-update-v1\n";

/// The text that is signed for `file` with checksum `sha256` (lower-case hex).
pub fn signed_message(file: &str, sha256: &str) -> String {
    format!("{SIGNED_PREFIX}{file}\n{sha256}\n")
}

/// The content of a ".sig" file.
pub fn signature_file(file: &str, sha256: &str, signature: &[u8]) -> String {
    let hex: String = signature.iter().map(|b| format!("{b:02x}")).collect();
    format!("{SIGNATURE_HEADER}\nfile: {file}\nsha256: {sha256}\nsignature: {hex}\n")
}

fn from_hex(hex: &str) -> Option<Vec<u8>> {
    let hex = hex.trim();
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok()).collect()
}

/// Checks a ".sig" file against the package name `file` with the keys
/// `keys`; returns the signed SHA-256 checksum (lower-case hex).
pub fn verify_signature(text: &str, file: &str, keys: &[&str]) -> Result<String> {
    use p256::ecdsa::signature::Verifier;
    let bad = |why: &str| Error::Invalid(format!("Update-Signatur ungültig: {why}"));
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    if lines.next() != Some(SIGNATURE_HEADER) {
        return Err(bad("unbekanntes Format"));
    }
    let mut value = |key: &str| -> Result<String> {
        lines
            .next()
            .and_then(|l| l.strip_prefix(key))
            .map(|v| v.trim().to_string())
            .ok_or_else(|| bad("unvollständig"))
    };
    let signed_file = value("file:")?;
    let sha256 = value("sha256:")?.to_ascii_lowercase();
    let signature = value("signature:")?;
    if signed_file != file {
        return Err(bad("sie gehört zu einer anderen Datei"));
    }
    if !is_sha256_hex(&sha256) {
        return Err(bad("Prüfsumme fehlt"));
    }
    let signature = from_hex(&signature)
        .and_then(|b| p256::ecdsa::Signature::from_slice(&b).ok())
        .ok_or_else(|| bad("Unterschrift unlesbar"))?;
    let message = signed_message(&signed_file, &sha256);
    let trusted = keys.iter().filter_map(|k| from_hex(k)).any(|k| {
        p256::ecdsa::VerifyingKey::from_sec1_bytes(&k)
            .is_ok_and(|key| key.verify(message.as_bytes(), &signature).is_ok())
    });
    if !trusted {
        return Err(bad("sie stammt nicht vom Herausgeber von VersoloPDF"));
    }
    Ok(sha256)
}

/// Signs a package (used by the release script `examples/sign_release.rs`):
/// returns the content of its ".sig" file. `secret` is the 32-byte private
/// key.
pub fn sign_package(secret: &[u8], file: &str, sha256: &str) -> Result<String> {
    use p256::ecdsa::signature::Signer;
    let key =
        p256::ecdsa::SigningKey::from_slice(secret).map_err(|_| Error::Invalid("invalid key".into()))?;
    let signature: p256::ecdsa::Signature = key.sign(signed_message(file, sha256).as_bytes());
    Ok(signature_file(file, sha256, &signature.to_bytes()))
}

/// The public key (SEC1 uncompressed hex) of a private key.
pub fn public_key_hex(secret: &[u8]) -> Result<String> {
    let key =
        p256::ecdsa::SigningKey::from_slice(secret).map_err(|_| Error::Invalid("invalid key".into()))?;
    Ok(key.verifying_key().to_encoded_point(false).as_bytes().iter().map(|b| format!("{b:02x}")).collect())
}

/// The SHA-256 checksum of a file (lower-case hex).
pub fn file_sha256(path: &Path) -> Result<String> {
    sha256_file(path)
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
        windows_sig: None,
        macos: None,
        macos_sha: None,
        macos_sig: None,
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
            } else if lower.ends_with("-windows-x64.zip.sig") {
                release.windows_sig = Some(asset);
            } else if lower.ends_with("-macos.zip") {
                release.macos = Some(asset);
            } else if lower.ends_with("-macos.zip.sha256") {
                release.macos_sha = Some(asset);
            } else if lower.ends_with("-macos.zip.sig") {
                release.macos_sig = Some(asset);
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

/// Shown when a release carries no signature.
pub const UNSIGNED: &str = "Dieses Update ist nicht signiert und wird deshalb nicht installiert. Bitte etwas später erneut versuchen oder die neue Version selbst von der Projektseite laden.";

/// Downloads the package of `release` for this system into `dir`. It is only
/// kept if its signature (".sig") is valid for this package and its
/// SHA-256 checksum is the signed one (and GitHub's digest, if given).
pub fn download_package(release: &Release, dir: &Path) -> Result<PathBuf> {
    let (asset, _) =
        package_of(release).ok_or_else(|| Error::Invalid("no package for this system".into()))?;
    let sig_asset = signature_of(release).ok_or_else(|| Error::Invalid(UNSIGNED.into()))?;
    // The package name must belong to this release's version.
    if !asset.name.contains(&format!("-{}-", release.version)) {
        return Err(Error::Invalid("the package does not belong to this release".into()));
    }
    std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
    let sig_path = dir.join("package.sig");
    download(sig_asset, &sig_path, 4096)?;
    let text = std::fs::read_to_string(&sig_path).map_err(|e| Error::io(&sig_path, e))?;
    std::fs::remove_file(&sig_path).ok();
    let expected = verify_signature(&text, &asset.name, UPDATE_KEYS)?;
    if asset.sha256.as_ref().is_some_and(|h| *h != expected) {
        return Err(Error::Invalid("checksum mismatch – the release was changed".into()));
    }
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

/// Name of the program bundle on macOS.
pub const MAC_APP: &str = "VersoloPDF.app";

/// Where an update is installed: on Windows the program folder, on macOS
/// the program bundle ("…/VersoloPDF.app"). None when the program does not
/// run from such a place (e.g. a development build).
pub fn install_target() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    if cfg!(target_os = "macos") {
        // …/VersoloPDF.app/Contents/MacOS/VersoloPDF
        let bundle = exe.parent()?.parent()?.parent()?;
        if bundle.extension().is_none_or(|e| e != "app") {
            return None;
        }
        // Started from macOS' hidden read-only copy: install where the
        // user put the app.
        #[cfg(target_os = "macos")]
        if let Some(original) = crate::macos::original_bundle_path(bundle) {
            return Some(original);
        }
        Some(bundle.to_path_buf())
    } else if cfg!(windows) {
        exe.parent().map(PathBuf::from)
    } else {
        None
    }
}

/// The program file inside an installation (`install_target`) or an
/// unpacked package.
pub fn program_exe(dir: &Path) -> PathBuf {
    if cfg!(target_os = "macos") { dir.join("Contents/MacOS/VersoloPDF") } else { dir.join("VersoloPDF.exe") }
}

/// The folder that has to be writable to install into `target`.
pub fn install_folder(target: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        target.parent().unwrap_or(target).to_path_buf()
    } else {
        target.to_path_buf()
    }
}

/// The bundle `VersoloPDF.app` in `dir` (or one folder below).
fn mac_bundle(dir: &Path) -> Option<PathBuf> {
    let direct = dir.join(MAC_APP);
    if program_exe(&direct).is_file() {
        return Some(direct);
    }
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path().join(MAC_APP)).find(|p| program_exe(p).is_file())
}

/// Unpacks the package; returns the folder with the new program (Windows)
/// or the new bundle (macOS).
pub fn unpack(zip: &Path) -> Result<PathBuf> {
    let work = zip.parent().ok_or_else(|| Error::Invalid("no folder".into()))?.join("files");
    std::fs::remove_dir_all(&work).ok();
    std::fs::create_dir_all(&work).map_err(|e| Error::io(&work, e))?;
    if cfg!(target_os = "macos") {
        // ditto keeps the bundle intact (permissions, signature).
        let output = Command::new("/usr/bin/ditto")
            .args(["-x", "-k"])
            .arg(zip)
            .arg(&work)
            .output()
            .map_err(|e| Error::Invalid(format!("ditto: {e}")))?;
        if !output.status.success() {
            return Err(curl_error(&output));
        }
        return mac_bundle(&work)
            .ok_or_else(|| Error::Invalid(format!("{MAC_APP} not found in the package")));
    }
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

#[cfg(unix)]
#[allow(unsafe_code)]
pub fn wait_for_exit(pid: u32, seconds: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else { return true };
    // SAFETY: signal 0 only asks whether the process exists.
    let alive = || unsafe { libc::kill(pid, 0) } == 0;
    for _ in 0..seconds * 10 {
        if !alive() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    !alive()
}

/// Replaces the bundle `target` by a copy of the bundle `source` (macOS).
/// The new bundle is copied next to the old one first; the old one is only
/// removed once the new one is in place (a running program keeps working,
/// its files stay open). Returns the number of files.
pub fn install_bundle(source: &Path, target: &Path) -> Result<usize> {
    if !program_exe(source).is_file() {
        return Err(Error::Invalid("no program in the new bundle".into()));
    }
    let folder = target.parent().ok_or_else(|| Error::Invalid("no folder".into()))?;
    let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| MAC_APP.into());
    let fresh = folder.join(format!(".{name}.new"));
    let old = folder.join(format!(".{name}.old"));
    std::fs::remove_dir_all(&fresh).ok();
    std::fs::remove_dir_all(&old).ok();
    copy_tree(source, &fresh, 0).map_err(|e| {
        std::fs::remove_dir_all(&fresh).ok();
        Error::io(&fresh, e)
    })?;
    let mut files = Vec::new();
    files_below(&fresh, Path::new(""), 8, &mut files).map_err(|e| Error::io(&fresh, e))?;
    let had_old = target.exists();
    if had_old && let Err(e) = std::fs::rename(target, &old) {
        std::fs::remove_dir_all(&fresh).ok();
        return Err(Error::io(target, e));
    }
    if let Err(e) = std::fs::rename(&fresh, target) {
        // Back to the old version.
        if had_old {
            std::fs::rename(&old, target).ok();
        }
        std::fs::remove_dir_all(&fresh).ok();
        return Err(Error::io(target, e));
    }
    std::fs::remove_dir_all(&old).ok();
    Ok(files.len())
}

/// Copies a folder with its files, links and permissions.
fn copy_tree(from: &Path, to: &Path, depth: usize) -> std::io::Result<()> {
    if depth > 12 {
        return Err(std::io::ErrorKind::InvalidInput.into());
    }
    std::fs::create_dir_all(to)?;
    std::fs::set_permissions(to, std::fs::metadata(from)?.permissions())?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let (src, dest) = (entry.path(), to.join(entry.file_name()));
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(std::fs::read_link(&src)?, &dest)?;
        } else if kind.is_dir() {
            copy_tree(&src, &dest, depth + 1)?;
        } else {
            // std::fs::copy keeps the permissions (the program stays executable).
            std::fs::copy(&src, &dest)?;
        }
    }
    Ok(())
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

/// Shown when macOS runs VersoloPDF from a hidden read-only copy and its
/// real place is unknown.
pub const TRANSLOCATED: &str = "macOS startet VersoloPDF aus einer schreibgeschützten Kopie, weil das Programm noch nicht mit dem Finder verschoben wurde. Bitte VersoloPDF beenden, VersoloPDF.app im Finder in den Ordner „Programme“ ziehen, von dort starten und das Update erneut versuchen.";

/// Lets the new program (`new_exe`) copy itself into `target` and waits
/// until it is done. A write-protected folder (e.g. under "Programme")
/// makes Windows ask for administrator rights first.
pub fn run_installer(new_exe: &Path, target: &Path) -> Result<()> {
    if crate::mail::no_external() {
        return Err(Error::Invalid("Testbetrieb: keine Installation".into()));
    }
    if target.to_string_lossy().contains("/AppTranslocation/") {
        return Err(Error::Invalid(TRANSLOCATED.into()));
    }
    let code = if folder_writable(&install_folder(target)) {
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

/// Runs the installer as administrator: macOS asks for an administrator's
/// password. The paths are passed as arguments, never as script text.
#[cfg(target_os = "macos")]
fn elevated(new_exe: &Path, target: &Path) -> Result<u32> {
    let output = Command::new("/usr/bin/osascript")
        .args(["-e", "on run argv"])
        .args([
            "-e",
            "do shell script (quoted form of item 1 of argv) & \" --install-update \" & (quoted form of item 2 of argv) with administrator privileges",
        ])
        .args(["-e", "end run"])
        .arg(new_exe)
        .arg(target)
        .output()
        .map_err(|e| Error::Invalid(format!("osascript: {e}")))?;
    if output.status.success() {
        return Ok(0);
    }
    let text = String::from_utf8_lossy(&output.stderr);
    // -128: the user cancelled the password question.
    if text.contains("-128") {
        return Err(Error::Invalid("Die Installation wurde abgebrochen (keine Administratorrechte).".into()));
    }
    Ok(1)
}

#[cfg(all(unix, not(target_os = "macos")))]
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
        assert!(r.macos.is_none());
        let mac = json.replace("windows-x64", "macos");
        let r = parse_release(&mac).unwrap();
        assert!(r.macos.is_some() && r.macos_sha.is_some() && r.windows.is_none());
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

    #[cfg(unix)]
    #[test]
    fn bundle_is_replaced_as_a_whole() {
        use std::os::unix::fs::PermissionsExt;
        let base = std::env::temp_dir().join(format!("versolopdf-bundle-{}", std::process::id()));
        let (src, apps) = (base.join("neu").join(MAC_APP), base.join("Programme"));
        let target = apps.join(MAC_APP);
        for (dir, text) in [(&src, "neu"), (&target, "alt")] {
            let exe = program_exe(dir);
            std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
            std::fs::write(&exe, text).unwrap();
            std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        std::fs::write(target.join("alt-nur.txt"), b"weg").unwrap();
        std::os::unix::fs::symlink("Contents", src.join("Verweis")).unwrap();
        assert!(install_bundle(&src, &target).unwrap() >= 1);
        let exe = program_exe(&target);
        assert_eq!(std::fs::read(&exe).unwrap(), b"neu");
        assert_eq!(std::fs::metadata(&exe).unwrap().permissions().mode() & 0o111, 0o111, "stays executable");
        assert!(!target.join("alt-nur.txt").exists(), "the old bundle is gone");
        assert!(std::fs::symlink_metadata(target.join("Verweis")).unwrap().file_type().is_symlink());
        let left: Vec<_> = std::fs::read_dir(&apps).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(left, vec![std::ffi::OsString::from(MAC_APP)], "no helper folders left");
        // Without a program in the new bundle nothing happens.
        assert!(install_bundle(&base.join("leer"), &target).is_err());
        assert_eq!(std::fs::read(&exe).unwrap(), b"neu");
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn signatures_are_checked() {
        let secret = [7u8; 32];
        let other = [9u8; 32];
        let public = public_key_hex(&secret).unwrap();
        let keys = [public.as_str()];
        let file = "VersoloPDF-9.9.9-windows-x64.zip";
        let sha = "ab".repeat(32);
        let sig = sign_package(&secret, file, &sha).unwrap();
        assert_eq!(verify_signature(&sig, file, &keys).unwrap(), sha);
        // Another file (older version, other system) is refused.
        assert!(verify_signature(&sig, "VersoloPDF-9.9.8-windows-x64.zip", &keys).is_err());
        assert!(verify_signature(&sig, "VersoloPDF-9.9.9-macos.zip", &keys).is_err());
        // A changed checksum or a foreign key is refused.
        let changed = sig.replace(&sha, &"cd".repeat(32));
        assert!(verify_signature(&changed, file, &keys).is_err());
        let foreign = sign_package(&other, file, &sha).unwrap();
        assert!(verify_signature(&foreign, file, &keys).is_err());
        // Broken files.
        assert!(verify_signature("", file, &keys).is_err());
        assert!(verify_signature(&sig.replace("signature: ", "signature: zz"), file, &keys).is_err());
        assert!(verify_signature(&sig, file, &[]).is_err());
        // A second key is accepted next to the first.
        let both = [public_key_hex(&other).unwrap(), public.clone()];
        let both: Vec<&str> = both.iter().map(String::as_str).collect();
        assert!(verify_signature(&sig, file, &both).is_ok());
    }

    #[test]
    fn the_built_in_key_is_valid() {
        assert!(!UPDATE_KEYS.is_empty());
        for key in UPDATE_KEYS {
            let bytes = from_hex(key).expect("hex");
            assert!(p256::ecdsa::VerifyingKey::from_sec1_bytes(&bytes).is_ok(), "{key}");
        }
    }

    #[test]
    fn signatures_are_found_in_the_release() {
        let base = "https://github.com/MrMarkus42/VersoloPDF/releases/download/v1.4.0/";
        let json = format!(
            r#"{{"tag_name": "v1.4.0", "assets": [
              {{"name": "VersoloPDF-1.4.0-windows-x64.zip", "browser_download_url": "{base}VersoloPDF-1.4.0-windows-x64.zip"}},
              {{"name": "VersoloPDF-1.4.0-windows-x64.zip.sig", "browser_download_url": "{base}VersoloPDF-1.4.0-windows-x64.zip.sig"}},
              {{"name": "VersoloPDF-1.4.0-macos.zip.sig", "browser_download_url": "{base}VersoloPDF-1.4.0-macos.zip.sig"}}
            ]}}"#
        );
        let r = parse_release(&json).unwrap();
        assert!(r.windows_sig.is_some() && r.macos_sig.is_some());
        // Without a signature nothing is downloaded.
        let mut unsigned = r.clone();
        unsigned.windows_sig = None;
        unsigned.macos_sig = None;
        if package_of(&unsigned).is_some() {
            let err =
                download_package(&unsigned, &std::env::temp_dir().join("versolopdf-unsigned")).unwrap_err();
            assert!(err.to_string().contains("nicht signiert"), "{err}");
        }
    }

    #[test]
    fn hidden_read_only_copy_is_not_written() {
        let hidden = Path::new("/private/var/folders/x/T/AppTranslocation/ABC/d/VersoloPDF.app");
        let err = run_installer(Path::new("/tmp/neu"), hidden).unwrap_err().to_string();
        // (in tests VERSOLOPDF_NO_EXTERNAL may stop it first)
        assert!(err.contains("Programme") || err.contains("Testbetrieb"), "{err}");
    }

    #[test]
    fn mac_bundle_is_found_in_the_package() {
        let base = std::env::temp_dir().join(format!("versolopdf-pkg-{}", std::process::id()));
        let app = base.join("VersoloPDF-9.9.9-macos").join(MAC_APP);
        std::fs::create_dir_all(program_exe(&app).parent().unwrap()).unwrap();
        std::fs::write(program_exe(&app), b"x").unwrap();
        assert_eq!(mac_bundle(&base), Some(app));
        assert_eq!(mac_bundle(&base.join("nichts")), None);
        std::fs::remove_dir_all(&base).ok();
    }
}
