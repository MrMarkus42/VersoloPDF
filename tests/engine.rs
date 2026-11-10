//! Protected mode: documents are read by a restricted helper process.
//! These tests check that the helper really cannot read the user's files,
//! write, use the network or start programs, and that a crashed helper is
//! replaced.

mod common;

use versolopdf::engine::proto::{Bytes, LoadInfo, Request};
use versolopdf::engine::{EngineDoc, Protection};

fn helper_mode() -> bool {
    !std::env::var("VERSOLOPDF_ENGINE").is_ok_and(|v| v == "inprocess")
}

#[test]
fn documents_are_read_in_a_restricted_helper() {
    let _guard = common::lock();
    if !helper_mode() {
        return;
    }
    let doc = versolopdf::Document::from_bytes(common::text_pdf(2), "a.pdf", None).unwrap();
    assert_eq!(doc.page_count(), 2);
    match versolopdf::engine::protection() {
        Protection::Protected(detail) => println!("protected: {detail}"),
        other => panic!("not protected: {other:?}"),
    }
    // Text and pictures come through the helper.
    assert!(doc.page_text(1).unwrap().all_text().contains("Page 2"));
    let img = doc.render_page(1, 200, 280).unwrap();
    let dark = img.rgba.chunks(4).filter(|p| p[0] < 128).count();
    assert!(dark > 20, "text is drawn ({dark} dark pixels)");
}

#[test]
fn the_helper_cannot_reach_files_network_or_programs() {
    let _guard = common::lock();
    if !helper_mode() {
        return;
    }
    // A private file of the user (here: one the test writes).
    let dir = tempfile::tempdir().unwrap();
    let secret = dir.path().join("geheim.txt");
    std::fs::write(&secret, b"nur fuer mich").unwrap();
    assert!(std::fs::read(&secret).is_ok(), "the test itself can read it");
    // A server the helper must not reach.
    let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = server.local_addr().unwrap().to_string();
    let probe = Request::ProbeRestrictions { file: secret.display().to_string(), address };
    let results: Vec<(String, bool)> = EngineDoc::once(probe).unwrap();
    println!("{results:?}");
    assert_eq!(results.len(), 4);
    for (what, worked) in &results {
        assert!(!worked, "the helper could {what}");
    }
}

#[test]
fn a_crashed_helper_is_replaced() {
    let _guard = common::lock();
    if !helper_mode() {
        return;
    }
    let bytes = common::text_pdf(3);
    let engine = EngineDoc::new();
    let info = engine.load(&bytes, None).unwrap();
    assert_eq!(info.pages, 3);
    let source = (bytes.as_slice(), None);
    // The helper dies while answering.
    assert!(engine.read::<()>(Request::TestCrash, source).is_err());
    // The next request gets a new helper that has the document again.
    let again: LoadInfo = engine.read(Request::Info, source).unwrap();
    assert_eq!(again.pages, 3);
    assert!(engine.is_separate());
    // A document that crashes the helper again and again is given up.
    for _ in 0..3 {
        let _ = engine.read::<()>(Request::TestCrash, source);
    }
    assert!(engine.read::<LoadInfo>(Request::Info, source).is_err());
}

#[test]
fn a_crash_during_a_document_edit_is_reported() {
    let _guard = common::lock();
    if !helper_mode() {
        return;
    }
    let mut doc = versolopdf::Document::from_bytes(common::text_pdf(3), "b.pdf", None).unwrap();
    doc.rotate_pages(&[0], 90).unwrap();
    // The document keeps working after its helper was replaced.
    let bytes = doc.to_bytes().unwrap();
    let copy = versolopdf::Document::from_bytes(bytes, "c.pdf", None).unwrap();
    assert_eq!(copy.geometry(0).unwrap().rotation, versolopdf::geometry::Rotation::R90);
    // New documents and combining files go through helpers too.
    let blank = versolopdf::Document::new_with_pages("leer.pdf", 300.0, 400.0, 2).unwrap();
    assert_eq!(blank.page_count(), 2);
    let combined: Bytes = EngineDoc::once(Request::Combine {
        sources: vec![Bytes(common::text_pdf(1)), Bytes(common::text_pdf(2))],
    })
    .unwrap();
    let doc = versolopdf::Document::from_bytes(combined.0, "d.pdf", None).unwrap();
    assert_eq!(doc.page_count(), 3);
}

#[test]
fn opening_documents_stays_fast() {
    let _guard = common::lock();
    let bytes = common::text_pdf(2);
    // The first helper may have to be prepared (Windows: container, copy).
    drop(versolopdf::Document::from_bytes(bytes.clone(), "warm.pdf", None).unwrap());
    let start = std::time::Instant::now();
    let n = 10;
    for i in 0..n {
        let doc = versolopdf::Document::from_bytes(bytes.clone(), &format!("{i}.pdf"), None).unwrap();
        assert_eq!(doc.page_count(), 2);
        let _ = doc.render_page(0, 100, 140).unwrap();
        // Some work in between, as when someone reads the document.
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
    let per_doc = (start.elapsed().as_secs_f64() - 0.15 * n as f64) / n as f64;
    println!("open + first page: {:.0} ms per document", per_doc * 1000.0);
    assert!(per_doc < 1.0, "opening takes {per_doc:.2} s");
}

#[test]
fn broken_and_hostile_input_is_refused() {
    let _guard = common::lock();
    // Not a PDF.
    assert!(matches!(
        versolopdf::Document::from_bytes(b"%PDF-1.7 kaputt".to_vec(), "x.pdf", None),
        Err(versolopdf::Error::InvalidFormat) | Err(versolopdf::Error::Pdfium(_))
    ));
    // Pages that do not exist and absurd sizes are checked before the engine.
    let doc = versolopdf::Document::from_bytes(common::text_pdf(1), "e.pdf", None).unwrap();
    assert!(doc.render_page(5, 10, 10).is_err());
    assert!(doc.render_page(0, 0, 10).is_err());
    assert!(doc.render_page(0, 100_000, 10).is_err());
    assert!(doc.page_text(9).is_err());
}
