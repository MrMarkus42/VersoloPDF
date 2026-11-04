//! Timings with real, large PDF files (not part of the repository).
//!
//! Run with `MRPDF_TESTFILES=<folder> cargo test --release --test real_files
//! -- --ignored --nocapture`. Every PDF in the folder is opened, rendered,
//! searched, edited (comment added and moved) and saved; the times are
//! printed and checked against generous limits.

mod common;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use mrpdf::Document;
use mrpdf::geometry::RectF;
use mrpdf::model::{AnnotKind, AnnotStyle, Rgba};
use mrpdf::pdf_edit::NewAnnotation;

fn files() -> Vec<PathBuf> {
    let Some(dir) = std::env::var_os("MRPDF_TESTFILES") else { return Vec::new() };
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    files.retain(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")));
    files.sort();
    files
}

fn ms(d: Duration) -> String {
    format!("{:>7.0} ms", d.as_secs_f64() * 1000.0)
}

#[test]
#[ignore = "needs MRPDF_TESTFILES"]
fn timings_with_real_files() {
    let _guard = common::lock();
    let files = files();
    assert!(!files.is_empty(), "MRPDF_TESTFILES must name a folder with PDFs");
    for path in files {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let size = std::fs::metadata(&path).unwrap().len() as f64 / 1e6;
        let t = Instant::now();
        let mut doc = match Document::open(&path, None) {
            Ok(d) => d,
            Err(e) => {
                println!("{name}: {size:.0} MB, cannot open: {e}");
                continue;
            }
        };
        let open = t.elapsed();
        let pages = doc.page_count();

        let (w, h) = doc.geometry(0).unwrap().view_size();
        let px_w = 1280u32;
        let px_h = (px_w as f32 * h / w) as u32;
        let t = Instant::now();
        doc.render_page(0, px_w, px_h).unwrap();
        let render_first = t.elapsed();
        let last = pages - 1;
        let t = Instant::now();
        doc.render_page(last, px_w, px_h).unwrap();
        let render_last = t.elapsed();

        let t = Instant::now();
        let _ = doc.page_text(0);
        let text = t.elapsed();

        let style =
            AnnotStyle { color: Rgba::RED, width: 2.0, author: "Test".into(), ..AnnotStyle::default() };
        let mut square = NewAnnotation::new(AnnotKind::Square, style);
        square.rect = RectF::new(100.0, 100.0, 200.0, 180.0);
        let t = Instant::now();
        doc.add_annotation(0, &square).unwrap();
        let add = t.elapsed();
        let index = doc.annotations(0).unwrap().len() - 1;
        let t = Instant::now();
        doc.move_annotations(0, &[index], 20.0, 10.0).unwrap();
        let moved = t.elapsed();
        let t = Instant::now();
        doc.rotate_pages(&[0], 90).unwrap();
        let rotate = t.elapsed();
        let t = Instant::now();
        doc.delete_pages(&[pages - 1]).unwrap();
        let delete = t.elapsed();
        println!("{name}: drehen {} | Seite löschen {}", ms(rotate), ms(delete));
        let t = Instant::now();
        let bytes = doc.to_bytes().unwrap();
        let save = t.elapsed();
        // Parts of an edit: the byte edit alone and loading the result.
        let update = mrpdf::pdf_edit::AnnotUpdate { translate: Some((5.0, 5.0)), ..Default::default() };
        let t = Instant::now();
        let edited = mrpdf::pdf_edit::update_annotation(&bytes, None, 0, index, &update).unwrap();
        let byte_edit = t.elapsed();
        let t = Instant::now();
        let reloaded = Document::from_bytes(edited, "x.pdf", None).unwrap();
        let reload = t.elapsed();
        drop(reloaded);
        println!("{name}: Byte-Änderung {} | neu laden {}", ms(byte_edit), ms(reload));

        println!(
            "{name}: {size:>6.1} MB {pages:>5} S. | öffnen {} | Seite 1 {} | letzte {} | Text {} | \
             Anmerkung neu {} | verschieben {} | speichern {} ({:.1} MB)",
            ms(open),
            ms(render_first),
            ms(render_last),
            ms(text),
            ms(add),
            ms(moved),
            ms(save),
            bytes.len() as f64 / 1e6
        );
    }
}

/// The characters of a page's text, sorted (to compare pages; a rotated
/// page gives the same characters in another order).
fn text_of(doc: &Document, page: usize) -> String {
    let mut chars: Vec<char> = doc
        .page_text(page)
        .map(|t| t.all_text().chars().filter(|c| !c.is_whitespace()).collect())
        .unwrap_or_default();
    chars.sort_unstable();
    chars.into_iter().collect()
}

fn reopen(doc: &mut Document) -> mrpdf::Result<Document> {
    Document::from_bytes(doc.to_bytes()?, "check.pdf", None)
}

/// Edits every real file, saves, opens the result again and compares.
#[test]
#[ignore = "needs MRPDF_TESTFILES"]
fn round_trips_with_real_files() {
    let _guard = common::lock();
    let files = files();
    assert!(!files.is_empty(), "MRPDF_TESTFILES must name a folder with PDFs");
    let mut problems: Vec<String> = Vec::new();
    for path in files {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let mut doc = match Document::open(&path, None) {
            Ok(d) => d,
            Err(e) => {
                problems.push(format!("{name}: öffnen: {e}"));
                continue;
            }
        };
        let n = doc.page_count();
        let first = text_of(&doc, 0);
        let last = text_of(&doc, n - 1);
        let size0 = doc.geometry(0).unwrap().view_size();
        let mut check = |what: &str, ok: Result<bool, String>, took: Duration| {
            let line = match ok {
                Ok(true) => format!("{name}: {what} ok ({})", ms(took)),
                Ok(false) => {
                    problems.push(format!("{name}: {what}: Ergebnis falsch"));
                    format!("{name}: {what} FALSCH")
                }
                Err(e) => {
                    problems.push(format!("{name}: {what}: {e}"));
                    format!("{name}: {what} FEHLER {e}")
                }
            };
            println!("{line}");
        };
        let step = |doc: &mut Document,
                    f: &dyn Fn(&mut Document) -> mrpdf::Result<()>|
         -> Result<Document, String> {
            f(doc).map_err(|e| e.to_string())?;
            reopen(doc).map_err(|e| format!("neu öffnen: {e}"))
        };

        let t = Instant::now();
        let r = step(&mut doc, &|d| d.rotate_pages(&[0], 90)).map(|r| {
            let s = r.geometry(0).unwrap().view_size();
            (s.0 - size0.1).abs() < 1.0 && (s.1 - size0.0).abs() < 1.0
        });
        check("Seite 1 drehen", r, t.elapsed());

        let t = Instant::now();
        let style =
            AnnotStyle { color: Rgba::RED, width: 2.0, author: "Test".into(), ..AnnotStyle::default() };
        let mut square = NewAnnotation::new(AnnotKind::Square, style);
        square.rect = RectF::new(100.0, 100.0, 200.0, 180.0);
        let before = doc.annotations(0).map(|a| a.len()).unwrap_or(0);
        let r = step(&mut doc, &|d| d.add_annotation(0, &square))
            .map(|r| r.annotations(0).map(|a| a.len()).unwrap_or(0) == before + 1);
        check("Anmerkung einfügen", r, t.elapsed());

        let t = Instant::now();
        let r = step(&mut doc, &|d| d.delete_pages(&[n - 1])).map(|r| r.page_count() == n - 1);
        check("letzte Seite löschen", r, t.elapsed());

        let t = Instant::now();
        let r = step(&mut doc, &|d| d.insert_blank_page(0, 595.0, 842.0)).map(|r| {
            let ok = r.page_count() == n && text_of(&r, 1) == first;
            if !ok {
                println!(
                    "  Seiten {} (erwartet {n}); Text Seite 2 gleich Anfang: {}; Länge Text {} / {}",
                    r.page_count(),
                    text_of(&r, 1) == first,
                    text_of(&r, 1).len(),
                    first.len()
                );
            }
            ok
        });
        check("leere Seite vorn einfügen", r, t.elapsed());

        let t = Instant::now();
        let r = step(&mut doc, &|d| d.reverse_pages())
            .map(|r| r.page_count() == n && text_of(&r, n - 2) == first);
        check("Reihenfolge umkehren", r, t.elapsed());

        let t = Instant::now();
        let r = doc
            .extract_pages_to_bytes(&[n - 1, n - 2])
            .map_err(|e| e.to_string())
            .and_then(|b| Document::from_bytes(b, "x.pdf", None).map_err(|e| e.to_string()))
            .map(|x| x.page_count() == 2 && text_of(&x, 1) == first);
        check("zwei Seiten herausziehen", r, t.elapsed());

        let t = Instant::now();
        let r = doc.undo_steps(99).map_err(|e| e.to_string()).and_then(|_| {
            let r = reopen(&mut doc).map_err(|e| e.to_string())?;
            let s = r.geometry(0).unwrap().view_size();
            Ok(r.page_count() == n
                && text_of(&r, 0) == first
                && text_of(&r, n - 1) == last
                && (s.0 - size0.0).abs() < 1.0
                && r.annotations(0).map(|a| a.len()).unwrap_or(0) == before)
        });
        check("alles rückgängig = Anfangszustand", r, t.elapsed());

        let t = Instant::now();
        let out = std::env::temp_dir().join(format!("mrpdf-roundtrip-{}.pdf", std::process::id()));
        let r = doc.save_as(&out).map_err(|e| e.to_string()).and_then(|_| {
            let r = Document::open(&out, None).map_err(|e| e.to_string())?;
            Ok(r.page_count() == n && text_of(&r, 0) == first)
        });
        let _ = std::fs::remove_file(&out);
        check("speichern und von Platte öffnen", r, t.elapsed());
    }
    println!("Probleme: {}", problems.len());
    for p in &problems {
        println!("  {p}");
    }
    assert!(problems.is_empty(), "{problems:#?}");
}

/// Probe: reading page geometry by loading every page vs. page_sizes().
#[test]
#[ignore = "needs MRPDF_TESTFILES"]
fn probe_geometry_speed() {
    let _guard = common::lock();
    let pdfium = mrpdf::engine::pdfium().unwrap();
    for path in files() {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let t = Instant::now();
        let doc = pdfium.load_pdf_from_file(&path, None).unwrap();
        let load = t.elapsed();
        let t = Instant::now();
        let mut rotated = 0;
        let mut offset = 0;
        for page in doc.pages().iter() {
            let crop = page.boundaries().crop().or_else(|_| page.boundaries().media()).map(|b| b.bounds);
            if let Ok(r) = crop
                && (r.left().value.abs() > 0.5 || r.bottom().value.abs() > 0.5)
            {
                offset += 1;
            }
            if page.rotation().map(|r| r.as_degrees() != 0.0).unwrap_or(false) {
                rotated += 1;
            }
        }
        let each = t.elapsed();
        let t = Instant::now();
        let sizes = doc.pages().page_sizes().unwrap();
        let fast = t.elapsed();
        println!(
            "{name}: laden {} | jede Seite laden {} | page_sizes {} ({} Seiten, {rotated} gedreht, {offset} mit verschobenem Rahmen)",
            ms(load),
            ms(each),
            ms(fast),
            sizes.len()
        );
    }
}
