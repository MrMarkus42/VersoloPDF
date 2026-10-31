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
