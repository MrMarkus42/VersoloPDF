//! "Formulare ausfüllen": fills a PDF form once per row of a CSV file and
//! writes one PDF per row.

use std::path::{Path, PathBuf};

use crate::document::{Document, write_atomic};
use crate::error::{Error, Result};

/// Largest data file that is read.
pub const MAX_DATA: u64 = 64 * 1024 * 1024;
/// Most rows that are filled in one run.
pub const MAX_ROWS: usize = 10_000;

/// Splits CSV text into rows of cells (quotes with "" escapes, CR/LF).
pub fn parse_csv(text: &str, sep: char) -> Vec<Vec<String>> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    cell.push('"');
                    chars.next();
                }
                '"' => quoted = false,
                _ => cell.push(c),
            }
        } else if c == '"' && cell.is_empty() {
            quoted = true;
        } else if c == sep {
            row.push(std::mem::take(&mut cell));
        } else if c == '\n' || c == '\r' {
            if c == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            row.push(std::mem::take(&mut cell));
            if row.iter().any(|c| !c.is_empty()) {
                rows.push(std::mem::take(&mut row));
            }
            row.clear();
        } else {
            cell.push(c);
        }
    }
    row.push(cell);
    if row.iter().any(|c| !c.is_empty()) {
        rows.push(row);
    }
    rows
}

/// Guesses the separator from the first line (`;`, `,`, tab).
pub fn guess_separator(text: &str) -> char {
    let first = text.lines().next().unwrap_or_default();
    [';', ',', '\t'].into_iter().max_by_key(|s| first.matches(*s).count()).unwrap_or(';')
}

/// Data rows chosen with "Zeile(n)" (1-based, e.g. "1-3, 5"; empty = all).
pub fn chosen_rows(spec: &str, count: usize) -> Option<Vec<usize>> {
    if spec.trim().is_empty()
        || spec.trim().eq_ignore_ascii_case("alle")
        || spec.trim().eq_ignore_ascii_case("all")
    {
        return Some((0..count).collect());
    }
    let mut out = Vec::new();
    for part in spec.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (a, b) = match part.split_once('-') {
            Some((a, b)) => (a.trim().parse::<usize>().ok()?, b.trim().parse::<usize>().ok()?),
            None => {
                let n = part.parse::<usize>().ok()?;
                (n, n)
            }
        };
        if a == 0 || b < a {
            return None;
        }
        out.extend((a..=b.min(count)).map(|n| n - 1));
    }
    out.dedup();
    Some(out)
}

/// Pairs each form field with the column of the same name (ignoring case,
/// spaces and underscores): "Automatisch ermitteln".
pub fn auto_map(fields: &[String], header: &[String]) -> Vec<(String, usize)> {
    let norm =
        |s: &str| s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect::<String>();
    fields
        .iter()
        .filter_map(|f| header.iter().position(|h| norm(h) == norm(f)).map(|col| (f.clone(), col)))
        .collect()
}

/// Output file name: `%[FileName]` (form name without extension) and
/// `%[RowIndex]` (1-based) replaced, unsafe characters removed.
pub fn output_name(pattern: &str, form_stem: &str, row: usize) -> String {
    let name = pattern.replace("%[FileName]", form_stem).replace("%[RowIndex]", &(row + 1).to_string());
    let clean: String = name
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .take(150)
        .collect();
    let clean = clean.trim().trim_matches('.').to_string();
    let clean = if clean.is_empty() { format!("{form_stem}_{}", row + 1) } else { clean };
    if clean.to_ascii_lowercase().ends_with(".pdf") { clean } else { format!("{clean}.pdf") }
}

/// Fills `form` (PDF bytes) for each chosen row and writes the files into
/// `folder`. Returns the written paths.
pub fn fill_all(
    form: &[u8],
    form_stem: &str,
    table: &[Vec<String>],
    mapping: &[(String, usize)],
    rows: &[usize],
    folder: &Path,
    pattern: &str,
) -> Result<Vec<PathBuf>> {
    if rows.len() > MAX_ROWS {
        return Err(Error::Invalid(format!("at most {MAX_ROWS} rows")));
    }
    let mut written = Vec::new();
    for &row in rows {
        let Some(cells) = table.get(row) else { continue };
        let values: Vec<(String, String)> = mapping
            .iter()
            .map(|(field, col)| (field.clone(), cells.get(*col).cloned().unwrap_or_default()))
            .collect();
        let mut doc = Document::from_bytes(form.to_vec(), "form.pdf", None)?;
        doc.set_values_by_name(&values)?;
        let path = folder.join(output_name(pattern, form_stem, row));
        write_atomic(&path, &doc.to_bytes()?)?;
        written.push(path);
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_rows_and_names() {
        let rows =
            parse_csv("\u{feff}Name;Ort\r\n\"Müller; Anna\";\"Bad \"\"Kissingen\"\"\"\n\nBerg;Ulm", ';');
        assert_eq!(
            rows,
            vec![vec!["Name", "Ort"], vec!["Müller; Anna", "Bad \"Kissingen\""], vec!["Berg", "Ulm"]]
        );
        assert_eq!(guess_separator("a,b,c\n1;2"), ',');
        assert_eq!(chosen_rows("", 3), Some(vec![0, 1, 2]));
        assert_eq!(chosen_rows("2-5, 1", 3), Some(vec![1, 2, 0]));
        assert_eq!(chosen_rows("x", 3), None);
        let map = auto_map(
            &["first_name".into(), "Ort".into(), "PLZ".into()],
            &["First Name".into(), "ort".into()],
        );
        assert_eq!(map, vec![("first_name".to_string(), 0), ("Ort".to_string(), 1)]);
        assert_eq!(output_name("%[FileName]_%[RowIndex]", "Antrag", 0), "Antrag_1.pdf");
        assert_eq!(output_name("../a:b", "x", 4), "_a_b.pdf");
        assert_eq!(output_name("", "Antrag", 2), "Antrag_3.pdf");
    }
}
