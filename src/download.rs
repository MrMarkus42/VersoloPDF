//! "Von URL öffnen": downloads a PDF with the system's `curl` program
//! (part of Windows 10/11, macOS and most Linux systems), so VersoloPDF needs no
//! own network or TLS library.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{Error, Result};

/// Largest accepted download.
pub const MAX_DOWNLOAD: u64 = 256 * 1024 * 1024;

/// Only plain http(s) addresses without spaces or control characters.
pub fn valid_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    let host = lower.strip_prefix("https://").or_else(|| lower.strip_prefix("http://"));
    host.is_some_and(|h| !h.is_empty() && !h.starts_with('/'))
        && url.len() <= 4096
        && !url.chars().any(|c| c.is_control() || c.is_whitespace())
}

/// File name for the downloaded document: the last path segment of the
/// address (only safe characters), always ending in `.pdf`.
pub fn file_name_for(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or("");
    let path = path.trim_end_matches('/');
    // "https://host" has no file name.
    let last = if path.matches('/').count() <= 2 { "" } else { path.rsplit('/').next().unwrap_or("") };
    let mut name: String = last
        .chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' })
        .take(100)
        .collect();
    name = name.trim_matches('.').to_string();
    if name.is_empty() {
        name = "Download".into();
    }
    if !name.to_ascii_lowercase().ends_with(".pdf") {
        name.push_str(".pdf");
    }
    name
}

/// Downloads `url` into a new file in `dir` and returns its path. Fails if
/// the server does not answer with a PDF file.
pub fn download_pdf(url: &str, dir: &Path) -> Result<PathBuf> {
    if !valid_url(url) {
        return Err(Error::Invalid("invalid address".into()));
    }
    std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
    let name = file_name_for(url);
    let mut dest = dir.join(&name);
    let mut n = 2;
    while dest.exists() {
        dest = dir.join(format!("{} ({n}).pdf", name.trim_end_matches(".pdf")));
        n += 1;
    }
    let mut cmd = Command::new("curl");
    cmd.args(["--fail", "--silent", "--show-error", "--location"])
        .args(["--proto", "=http,https", "--proto-redir", "=http,https"])
        .args(["--max-redirs", "10", "--connect-timeout", "20", "--max-time", "300"])
        .args(["--max-filesize", &MAX_DOWNLOAD.to_string()])
        .arg("--output")
        .arg(&dest)
        .arg(url);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window for the helper program.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let output = cmd.output().map_err(|e| Error::Invalid(format!("curl: {e}")))?;
    if !output.status.success() {
        std::fs::remove_file(&dest).ok();
        let message = String::from_utf8_lossy(&output.stderr).trim().chars().take(400).collect::<String>();
        return Err(Error::Invalid(if message.is_empty() { "download failed".into() } else { message }));
    }
    // The PDF header must appear within the first KB.
    let mut head = Vec::with_capacity(1024);
    let file = std::fs::File::open(&dest).map_err(|e| Error::io(&dest, e))?;
    std::io::Read::read_to_end(&mut std::io::Read::take(file, 1024), &mut head)
        .map_err(|e| Error::io(&dest, e))?;
    if !head.windows(5).any(|w| w == b"%PDF-") {
        std::fs::remove_file(&dest).ok();
        return Err(Error::InvalidFormat);
    }
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_checked() {
        assert!(valid_url("https://example.org/a.pdf"));
        assert!(valid_url("http://example.org"));
        assert!(!valid_url("file:///etc/passwd"));
        assert!(!valid_url("https://exa mple.org"));
        assert!(!valid_url("-o /tmp/x"));
        assert!(!valid_url("https://"));
    }

    #[test]
    fn file_names_are_safe() {
        assert_eq!(file_name_for("https://x.org/docs/Report%202.pdf?x=1"), "Report_202.pdf");
        assert_eq!(file_name_for("https://x.org/"), "Download.pdf");
        assert_eq!(file_name_for("https://x.org"), "Download.pdf");
        assert_eq!(file_name_for("https://x.org/a/..\\..\\evil"), "_.._evil.pdf");
        assert_eq!(file_name_for("https://x.org/a/b.PDF"), "b.PDF");
    }
}
