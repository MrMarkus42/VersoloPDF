//! Robustness against damaged or hostile files: hundreds of mutated PDFs are
//! opened, rendered, searched and edited. Errors are fine, crashes are not.

mod common;

use versolopdf::Document;
use versolopdf::geometry::RectF;
use versolopdf::model::{AnnotKind, AnnotStyle};
use versolopdf::pdf_edit::{self, NewAnnotation};

/// Small deterministic pseudo random generator (no external crates needed).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn mutate(base: &[u8], rng: &mut Lcg) -> Vec<u8> {
    let mut bytes = base.to_vec();
    match rng.below(4) {
        0 => {
            // Flip random bytes.
            for _ in 0..1 + rng.below(16) {
                let i = rng.below(bytes.len());
                bytes[i] = rng.next() as u8;
            }
        }
        1 => {
            // Truncate.
            let len = rng.below(bytes.len());
            bytes.truncate(len);
        }
        2 => {
            // Duplicate a chunk somewhere else.
            let start = rng.below(bytes.len());
            let len = rng.below(256).min(bytes.len() - start);
            let chunk = bytes[start..start + len].to_vec();
            let at = rng.below(bytes.len());
            bytes.splice(at..at, chunk);
        }
        _ => {
            // Replace numbers with extreme values.
            let text = String::from_utf8_lossy(&bytes).into_owned();
            let huge = ["99999999999", "-1", "0", "2147483648", "1e38"][rng.below(5)];
            let mut out = String::new();
            for (i, part) in text.split(' ').enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                if part.chars().all(|c| c.is_ascii_digit()) && !part.is_empty() && rng.below(8) == 0 {
                    out.push_str(huge);
                } else {
                    out.push_str(part);
                }
            }
            bytes = out.into_bytes();
        }
    }
    bytes
}

#[test]
fn mutated_documents_never_crash() {
    let _guard = common::lock();
    let bases = [common::text_pdf(2), common::form_pdf()];
    let mut rng = Lcg(0x5EED_1234);
    let mut opened = 0;
    for round in 0..300 {
        let base = &bases[round % bases.len()];
        let bytes = mutate(base, &mut rng);
        if let Ok(mut doc) = Document::from_bytes(bytes.clone(), "fuzz.pdf", None) {
            opened += 1;
            for page in 0..doc.page_count().min(2) {
                let _ = doc.render_page(page, 120, 160);
                let _ = doc.page_text(page);
                let _ = doc.annotations(page);
                let _ = doc.links(page);
                let _ = doc.form_fields(page);
            }
            let _ = doc.bookmarks();
            let _ = doc.metadata();
            let mut annot = NewAnnotation::new(AnnotKind::Square, AnnotStyle::default());
            annot.rect = RectF::new(10.0, 10.0, 50.0, 50.0);
            let _ = doc.add_annotation(0, &annot);
            let _ = doc.undo();
        }
        // The structural editor must cope with anything, too.
        let _ = pdf_edit::read_metadata(&bytes, None);
        let _ = pdf_edit::reorder_pages(&bytes, None, &[1, 0]);
    }
    assert!(opened > 30, "most mutations should still open ({opened})");
}

/// The same with a real document: the smallest PDF of the folder named in
/// `VERSOLOPDF_TESTFILES` (real files are not part of the repository).
#[test]
#[ignore = "needs VERSOLOPDF_TESTFILES"]
fn mutated_real_document_never_crashes() {
    let _guard = common::lock();
    let dir = std::env::var_os("VERSOLOPDF_TESTFILES").expect("VERSOLOPDF_TESTFILES");
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")))
        .collect();
    files.sort_by_key(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(u64::MAX));
    let base = std::fs::read(&files[0]).unwrap();
    let mut rng = Lcg(0xC0FF_EE01);
    let (mut opened, mut slow) = (0, 0);
    for _ in 0..200 {
        let bytes = mutate(&base, &mut rng);
        let start = std::time::Instant::now();
        if let Ok(mut doc) = Document::from_bytes(bytes.clone(), "fuzz.pdf", None) {
            opened += 1;
            let n = doc.page_count();
            for page in [0, n / 2, n.saturating_sub(1)] {
                let _ = doc.render_page(page, 200, 280);
                let _ = doc.page_text(page);
                let _ = doc.annotations(page);
            }
            let _ = doc.bookmarks();
            let mut annot = NewAnnotation::new(AnnotKind::Square, AnnotStyle::default());
            annot.rect = RectF::new(10.0, 10.0, 50.0, 50.0);
            let _ = doc.add_annotation(0, &annot);
            let _ = doc.rotate_pages(&[0], 90);
            let _ = doc.undo_steps(2);
            let _ = doc.to_bytes();
        }
        let _ = pdf_edit::read_metadata(&bytes, None);
        if start.elapsed().as_secs_f64() > 5.0 {
            slow += 1;
        }
    }
    println!("geöffnet: {opened} von 200, langsamer als 5 s: {slow}");
    assert_eq!(slow, 0, "a damaged file must not hang");
}
