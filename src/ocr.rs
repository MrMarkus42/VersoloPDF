//! "Text auf Seiten erkennen" (OCR). VersoloPDF uses the free program Tesseract
//! (Apache-2.0) if it is installed: each page is rendered, Tesseract finds
//! the words and their places, and VersoloPDF adds them as an invisible text
//! layer over the page, so scanned pages can be searched and copied.

use std::path::{Path, PathBuf};
use std::process::Command;

use lopdf::{Dictionary, Object};

use crate::appearance::{StdFont, num, win_ansi};
use crate::error::{Error, Result};
use crate::geometry::RectF;
use crate::pdf_edit::{Edit, guarded};

/// The installed program and the languages to recognise ("deu+eng").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Engine {
    pub exe: PathBuf,
    pub languages: String,
}

/// A recognised word: text, box in image pixels (left, top, width,
/// height) and confidence 0..100.
#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub text: String,
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
    pub confidence: f32,
}

/// Places where Tesseract is usually installed.
const CANDIDATES: [&str; 6] = [
    r"C:\Program Files\Tesseract-OCR\tesseract.exe",
    r"C:\Program Files (x86)\Tesseract-OCR\tesseract.exe",
    "/usr/bin/tesseract",
    "/usr/local/bin/tesseract",
    "/opt/homebrew/bin/tesseract",
    "/opt/local/bin/tesseract",
];

/// Finds the Tesseract program: the configured path, the search path or
/// the usual install folders.
pub fn find_tesseract(configured: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = configured.filter(|p| p.is_file()) {
        return Some(p.to_path_buf());
    }
    let exe = if cfg!(windows) { "tesseract.exe" } else { "tesseract" };
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let p = dir.join(exe);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    CANDIDATES.iter().map(PathBuf::from).find(|p| p.is_file())
}

fn command(exe: &Path) -> Command {
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut c = Command::new(exe);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: no console window pops up.
        c.creation_flags(0x0800_0000);
    }
    c
}

/// Installed recognition languages ("deu", "eng", …).
pub fn languages(exe: &Path) -> Vec<String> {
    let Ok(out) = command(exe).arg("--list-langs").output() else { return Vec::new() };
    let text = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    text.lines()
        .skip_while(|l| !l.to_ascii_lowercase().contains("languages"))
        .skip(1)
        .map(str::trim)
        .filter(|l| !l.is_empty() && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .map(str::to_string)
        .collect()
}

/// Language codes passed on the command line: letters, digits, `_`, `+`.
pub fn clean_languages(langs: &str) -> String {
    let l: String =
        langs.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '+')).take(60).collect();
    if l.is_empty() { "eng".into() } else { l }
}

/// Reads Tesseract's TSV output (level 5 = words).
pub fn parse_tsv(tsv: &str) -> Vec<Word> {
    let mut out = Vec::new();
    for line in tsv.lines().skip(1) {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 12 || cols[0] != "5" {
            continue;
        }
        let n = |i: usize| cols[i].trim().parse::<f32>().ok();
        let text = cols[11..].join("\t").trim().to_string();
        if let (Some(left), Some(top), Some(width), Some(height), Some(confidence)) =
            (n(6), n(7), n(8), n(9), n(10))
            && !text.is_empty()
            && confidence >= 0.0
        {
            out.push(Word { text, left, top, width, height, confidence });
        }
    }
    out
}

/// Runs Tesseract on a PNG file.
pub fn recognize(exe: &Path, png: &Path, langs: &str) -> Result<Vec<Word>> {
    let out = command(exe)
        .arg(png)
        .arg("stdout")
        .arg("-l")
        .arg(clean_languages(langs))
        .arg("tsv")
        .output()
        .map_err(|e| Error::Io { path: exe.to_path_buf(), source: e })?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or("Tesseract").to_string();
        return Err(Error::Invalid(msg));
    }
    Ok(parse_tsv(&String::from_utf8_lossy(&out.stdout)))
}

/// Adds `words` (PDF space boxes) as invisible text to `page`.
pub fn add_text_layer(
    bytes: &[u8],
    password: Option<&str>,
    page: usize,
    words: &[(String, RectF)],
) -> Result<Vec<u8>> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let page_id = edit.page_id(page)?;
        let mut font = Dictionary::new();
        font.set("Type", Object::Name(b"Font".to_vec()));
        font.set("Subtype", Object::Name(b"Type1".to_vec()));
        font.set("BaseFont", Object::Name(b"Helvetica".to_vec()));
        font.set("Encoding", Object::Name(b"WinAnsiEncoding".to_vec()));
        let font_id = edit.add(Object::Dictionary(font));
        crate::pages::add_resource(&mut edit, page_id, b"Font", "FOcr", Object::Reference(font_id))?;
        // Text render mode 3: invisible, but searchable and selectable.
        let mut content = String::from("BT 3 Tr\n");
        for (text, r) in words {
            let r = r.normalized();
            let bytes = win_ansi(text);
            let size = (r.height() * 0.85).clamp(1.0, 500.0);
            let natural = StdFont::Helvetica.text_width(&bytes, size).max(0.1);
            let scale = (r.width() / natural * 100.0).clamp(10.0, 1000.0);
            let mut escaped = String::new();
            for b in &bytes {
                match b {
                    b'(' | b')' | b'\\' => {
                        escaped.push('\\');
                        escaped.push(*b as char);
                    }
                    32..=126 => escaped.push(*b as char),
                    _ => escaped.push_str(&format!("\\{b:03o}")),
                }
            }
            content.push_str(&format!(
                "/FOcr {} Tf {} Tz 1 0 0 1 {} {} Tm ({escaped} ) Tj\n",
                num(size),
                num(scale),
                num(r.x0),
                num(r.y0 + r.height() * 0.2)
            ));
        }
        content.push_str("ET\n");
        crate::pages::add_content(&mut edit, page_id, content.into_bytes(), false)?;
        edit.finish()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tsv_words_and_languages() {
        let tsv = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n\
                   1\t1\t0\t0\t0\t0\t0\t0\t600\t800\t-1\t\n\
                   5\t1\t1\t1\t1\t1\t100\t50\t80\t20\t96.5\tHallo\n\
                   5\t1\t1\t1\t1\t2\t190\t50\t60\t20\t91\tWelt\n\
                   5\t1\t1\t1\t1\t3\t260\t50\t10\t20\t95\t \n";
        let words = parse_tsv(tsv);
        assert_eq!(words.len(), 2);
        assert_eq!(
            words[0],
            Word {
                text: "Hallo".into(),
                left: 100.0,
                top: 50.0,
                width: 80.0,
                height: 20.0,
                confidence: 96.5
            }
        );
        assert_eq!(clean_languages("deu+eng; rm -rf"), "deu+engrmrf");
        assert_eq!(clean_languages(""), "eng");
    }
}
