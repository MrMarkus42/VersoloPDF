//! PDF portfolios ("PDF-Portfolio"): a PDF whose catalog has a
//! `/Collection` and whose embedded files are the actual content. The
//! cover page is an ordinary page shown by viewers without portfolio
//! support.

use std::path::Path;

use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document as LoDocument, Object, Stream, StringFormat};

use crate::pdf_edit::{Edit, decode_text_string, guarded, pdf_date, text_string};
use crate::{Error, Result};

/// Largest file taken into a portfolio.
pub const MAX_FILE: u64 = 256 * 1024 * 1024;
/// Largest number of files.
pub const MAX_FILES: usize = 1000;

/// One file of a portfolio.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub name: String,
    pub size: u64,
    /// "Geändert" as shown (empty if unknown).
    pub modified: String,
    pub description: String,
}

/// A file to put into a new portfolio.
#[derive(Debug, Clone)]
pub struct NewFile {
    pub name: String,
    pub data: Vec<u8>,
    /// Modification time (seconds since 1970).
    pub modified: Option<i64>,
}

impl NewFile {
    /// Reads a file from disk (size limited).
    pub fn read(path: &Path) -> Result<NewFile> {
        let meta = std::fs::metadata(path).map_err(|e| Error::io(path, e))?;
        if meta.len() > MAX_FILE {
            return Err(Error::FileTooLarge { size: meta.len(), limit: MAX_FILE });
        }
        let data = std::fs::read(path).map_err(|e| Error::io(path, e))?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| Error::Invalid("invalid file name".into()))?;
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64);
        Ok(NewFile { name, data, modified })
    }
}

/// Unique names in the order given ("a.pdf", "a (2).pdf", …).
fn unique_names(files: &[NewFile]) -> Vec<String> {
    let mut used: Vec<String> = Vec::new();
    for f in files {
        let mut name = f.name.clone();
        let mut n = 2;
        while used.iter().any(|u| u.eq_ignore_ascii_case(&name)) {
            name = match f.name.rsplit_once('.') {
                Some((stem, ext)) => format!("{stem} ({n}).{ext}"),
                None => format!("{} ({n})", f.name),
            };
            n += 1;
        }
        used.push(name);
    }
    used
}

/// Text lines of the cover page (WinAnsi; other characters become "?").
fn cover_lines(title: &str, names: &[String], de: bool) -> Vec<(f32, f32, String)> {
    let mut lines = vec![(20.0, 780.0, title.to_string())];
    lines.push((
        11.0,
        750.0,
        if de {
            "Dieses Dokument ist ein PDF-Portfolio. Die enthaltenen Dateien:".to_string()
        } else {
            "This document is a PDF portfolio. The files it contains:".to_string()
        },
    ));
    let mut y = 722.0;
    for name in names.iter().take(45) {
        lines.push((11.0, y, format!("- {name}")));
        y -= 15.0;
    }
    if names.len() > 45 {
        lines.push((11.0, y, format!("… (+{})", names.len() - 45)));
    }
    lines
}

/// Builds a new portfolio with a cover page and `files` embedded.
pub fn create(files: &[NewFile], title: &str, de: bool) -> Result<Vec<u8>> {
    if files.is_empty() {
        return Err(Error::Invalid("no files selected".into()));
    }
    if files.len() > MAX_FILES {
        return Err(Error::Invalid("too many files".into()));
    }
    let names = unique_names(files);
    let mut doc = LoDocument::with_version("1.7");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(Dictionary::from_iter(vec![
        ("Type", Object::Name(b"Font".to_vec())),
        ("Subtype", Object::Name(b"Type1".to_vec())),
        ("BaseFont", Object::Name(b"Helvetica".to_vec())),
        ("Encoding", Object::Name(b"WinAnsiEncoding".to_vec())),
    ]));
    let mut ops = Vec::new();
    for (size, y, text) in cover_lines(title, &names, de) {
        ops.push(Operation::new("BT", vec![]));
        ops.push(Operation::new("Tf", vec![Object::Name(b"F1".to_vec()), Object::Real(size)]));
        ops.push(Operation::new("Td", vec![Object::Real(56.0), Object::Real(y)]));
        let bytes = crate::appearance::win_ansi(&text);
        ops.push(Operation::new("Tj", vec![Object::String(bytes, StringFormat::Literal)]));
        ops.push(Operation::new("ET", vec![]));
    }
    let content = Content { operations: ops }.encode().map_err(|e| Error::Structure(e.to_string()))?;
    let content_id = doc.add_object(Stream::new(Dictionary::new(), content));
    let mut fonts = Dictionary::new();
    fonts.set("F1", Object::Reference(font_id));
    let mut resources = Dictionary::new();
    resources.set("Font", Object::Dictionary(fonts));
    let page_id = doc.add_object(Dictionary::from_iter(vec![
        ("Type", Object::Name(b"Page".to_vec())),
        ("Parent", Object::Reference(pages_id)),
        ("MediaBox", Object::Array(vec![0.into(), 0.into(), Object::Real(595.0), Object::Real(842.0)])),
        ("Contents", Object::Reference(content_id)),
        ("Resources", Object::Dictionary(resources)),
    ]));
    doc.objects.insert(
        pages_id,
        Object::Dictionary(Dictionary::from_iter(vec![
            ("Type", Object::Name(b"Pages".to_vec())),
            ("Kids", Object::Array(vec![Object::Reference(page_id)])),
            ("Count", Object::Integer(1)),
        ])),
    );
    // Embedded files; the name tree is sorted by name.
    let mut entries: Vec<(String, Object)> = Vec::new();
    for (f, name) in files.iter().zip(&names) {
        let mut params = Dictionary::new();
        params.set("Size", Object::Integer(f.data.len() as i64));
        if let Some(t) = f.modified {
            params.set("ModDate", Object::String(pdf_date(t).into_bytes(), StringFormat::Literal));
        }
        let mut sd = Dictionary::new();
        sd.set("Type", Object::Name(b"EmbeddedFile".to_vec()));
        sd.set("Params", Object::Dictionary(params));
        let mut stream = Stream::new(sd, f.data.clone());
        let _ = stream.compress();
        let ef_id = doc.add_object(stream);
        let mut ef = Dictionary::new();
        ef.set("F", Object::Reference(ef_id));
        ef.set("UF", Object::Reference(ef_id));
        let spec = Dictionary::from_iter(vec![
            ("Type", Object::Name(b"Filespec".to_vec())),
            ("F", text_string(name)),
            ("UF", text_string(name)),
            ("EF", Object::Dictionary(ef)),
        ]);
        let spec_id = doc.add_object(spec);
        entries.push((name.clone(), Object::Reference(spec_id)));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let names_array: Vec<Object> = entries.into_iter().flat_map(|(n, r)| [text_string(&n), r]).collect();
    let mut embedded = Dictionary::new();
    embedded.set("Names", Object::Array(names_array));
    let mut names_dict = Dictionary::new();
    names_dict.set("EmbeddedFiles", Object::Dictionary(embedded));
    let mut collection = Dictionary::new();
    collection.set("Type", Object::Name(b"Collection".to_vec()));
    collection.set("View", Object::Name(b"D".to_vec()));
    let catalog_id = doc.add_object(Dictionary::from_iter(vec![
        ("Type", Object::Name(b"Catalog".to_vec())),
        ("Pages", Object::Reference(pages_id)),
        ("Names", Object::Dictionary(names_dict)),
        ("Collection", Object::Dictionary(collection)),
        ("PageMode", Object::Name(b"UseAttachments".to_vec())),
    ]));
    doc.trailer.set("Root", Object::Reference(catalog_id));
    let mut info = Dictionary::new();
    info.set("Title", text_string(title));
    info.set("Producer", text_string("MrPDF"));
    let info_id = doc.add_object(info);
    doc.trailer.set("Info", Object::Reference(info_id));
    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(|e| Error::Structure(e.to_string()))?;
    Ok(out)
}

fn catalog(edit: &Edit) -> Result<Dictionary> {
    let id = edit.prev().trailer.get(b"Root")?.as_reference()?;
    Ok(edit.object(id)?.as_dict()?.clone())
}

/// True if the document is a portfolio.
pub fn is_portfolio(bytes: &[u8], password: Option<&str>) -> bool {
    guarded(|| {
        let edit = Edit::open(bytes, password)?;
        Ok(catalog(&edit)?.has(b"Collection"))
    })
    .unwrap_or(false)
}

/// Marks a document as portfolio (or removes the mark).
pub fn set_portfolio(bytes: &[u8], password: Option<&str>, on: bool) -> Result<Vec<u8>> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let id = edit.prev().trailer.get(b"Root")?.as_reference()?;
        let cat = edit.dict_mut(id)?;
        if on {
            let mut collection = Dictionary::new();
            collection.set("Type", Object::Name(b"Collection".to_vec()));
            collection.set("View", Object::Name(b"D".to_vec()));
            cat.set("Collection", Object::Dictionary(collection));
        } else {
            cat.remove(b"Collection");
        }
        edit.finish()
    })
}

/// The files of a portfolio (any document with embedded files), in the
/// order of the name tree.
pub fn entries(bytes: &[u8], password: Option<&str>) -> Result<Vec<Entry>> {
    guarded(|| {
        let edit = Edit::open(bytes, password)?;
        let cat = catalog(&edit)?;
        let mut out = Vec::new();
        let Ok(names) = cat.get(b"Names") else { return Ok(out) };
        let names = edit.resolve(names)?.as_dict()?;
        let Ok(tree) = names.get(b"EmbeddedFiles") else { return Ok(out) };
        let mut stack = vec![edit.resolve(tree)?.clone()];
        let mut budget = 10_000;
        while let Some(node) = stack.pop() {
            budget -= 1;
            if budget == 0 {
                break;
            }
            let Ok(node) = node.as_dict() else { continue };
            if let Ok(list) = node.get(b"Names") {
                let list = edit.resolve(list)?.as_array()?;
                for pair in list.as_chunks::<2>().0 {
                    let key = pair[0].as_str().map(decode_text_string).unwrap_or_default();
                    let Ok(spec) = edit.resolve(&pair[1]).and_then(|o| Ok(o.as_dict()?)) else { continue };
                    out.push(entry_of(&edit, spec, key));
                }
            }
            if let Ok(kids) = node.get(b"Kids") {
                let kids = edit.resolve(kids)?.as_array()?;
                // Reversed so the stack pops them in order.
                for kid in kids.iter().rev() {
                    stack.push(edit.resolve(kid)?.clone());
                }
            }
        }
        Ok(out)
    })
}

fn entry_of(edit: &Edit, spec: &Dictionary, key: String) -> Entry {
    let text = |k: &[u8]| spec.get(k).ok().and_then(|o| o.as_str().ok()).map(decode_text_string);
    let name = text(b"UF").or_else(|| text(b"F")).unwrap_or(key);
    let description = text(b"Desc").unwrap_or_default();
    let mut size = 0;
    let mut modified = String::new();
    if let Some(ef) = spec.get(b"EF").ok().and_then(|o| edit.resolve(o).ok())
        && let Ok(ef) = ef.as_dict()
        && let Ok(stream_ref) = ef.get(b"UF").or_else(|_| ef.get(b"F"))
        && let Ok(Object::Stream(stream)) = edit.resolve(stream_ref)
    {
        let params =
            stream.dict.get(b"Params").ok().and_then(|p| edit.resolve(p).ok()).and_then(|p| p.as_dict().ok());
        size = params
            .and_then(|p| p.get(b"Size").ok())
            .and_then(|s| s.as_i64().ok())
            .map(|s| s.max(0) as u64)
            .unwrap_or(stream.content.len() as u64);
        if let Some(raw) = params.and_then(|p| p.get(b"ModDate").ok()).and_then(|d| d.as_str().ok()) {
            modified = crate::pdf_edit::format_pdf_date(&String::from_utf8_lossy(raw));
        }
    }
    Entry { name, size, modified, description }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_read_back() {
        let files = vec![
            NewFile { name: "b.txt".into(), data: b"Hallo".to_vec(), modified: Some(1_700_000_000) },
            NewFile { name: "a.pdf".into(), data: vec![1, 2, 3], modified: None },
            NewFile { name: "b.txt".into(), data: b"zwei".to_vec(), modified: None },
        ];
        let bytes = create(&files, "Mappe", true).unwrap();
        assert!(is_portfolio(&bytes, None));
        let e = entries(&bytes, None).unwrap();
        let names: Vec<&str> = e.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["a.pdf", "b (2).txt", "b.txt"]);
        assert_eq!(e[2].size, 5);
        assert!(!e[2].modified.is_empty());
        let plain = set_portfolio(&bytes, None, false).unwrap();
        assert!(!is_portfolio(&plain, None));
        assert!(create(&[], "x", true).is_err());
    }
}
