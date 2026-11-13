//! Helpers that build test documents at runtime (no binary fixtures needed).

#![allow(dead_code)]

use std::sync::{Mutex, MutexGuard};

use lopdf::{Dictionary, Object, Stream, dictionary};
use pdfium_render::prelude::*;

/// PDFium is not thread safe; tests that use it hold this lock. Documents
/// are read in protected mode, by the program built for the tests as
/// restricted helper (VERSOLOPDF_ENGINE=inprocess: inside the tests).
pub fn lock() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if std::env::var("VERSOLOPDF_ENGINE").is_ok_and(|v| v == "inprocess") {
        versolopdf::engine::use_in_process();
    } else {
        // Test programs copied to another machine name the program there.
        let exe = std::env::var_os("VERSOLOPDF_ENGINE_EXE")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(env!("CARGO_BIN_EXE_versolopdf-engine")));
        versolopdf::engine::use_helper(exe);
    }
    guard
}

/// A PDF with one A4 page per entry of `pages`: its text lines (14 pt, from
/// the top) and, if `box_on` names the page, a filled blue rectangle.
#[allow(dead_code)]
pub fn lines_pdf(pages: &[&[&str]], box_on: Option<usize>) -> Vec<u8> {
    use pdfium_render::prelude::{PdfColor, PdfRect};
    let pdfium = versolopdf::engine::pdfium().expect("PDFium must be available for tests");
    let mut doc = pdfium.create_new_pdf().unwrap();
    let font = doc.fonts_mut().helvetica();
    for (i, lines) in pages.iter().enumerate() {
        let mut page = doc.pages_mut().create_page_at_end(PdfPagePaperSize::a4()).unwrap();
        for (k, line) in lines.iter().enumerate() {
            page.objects_mut()
                .create_text_object(
                    PdfPoints::new(72.0),
                    PdfPoints::new(760.0 - 30.0 * k as f32),
                    *line,
                    font,
                    PdfPoints::new(14.0),
                )
                .unwrap();
        }
        if box_on == Some(i) {
            page.objects_mut()
                .create_path_object_rect(
                    PdfRect::new_from_values(200.0, 300.0, 300.0, 400.0),
                    None,
                    None,
                    Some(PdfColor::new(30, 60, 200, 255)),
                )
                .unwrap();
        }
    }
    doc.save_to_bytes().unwrap()
}

/// A PDF with `pages` A4 pages, each containing the line `Page N Hello World`.
pub fn text_pdf(pages: usize) -> Vec<u8> {
    let pdfium = versolopdf::engine::pdfium().expect("PDFium must be available for tests");
    let mut doc = pdfium.create_new_pdf().unwrap();
    let font = doc.fonts_mut().helvetica();
    for i in 0..pages {
        let mut page = doc.pages_mut().create_page_at_end(PdfPagePaperSize::a4()).unwrap();
        page.objects_mut()
            .create_text_object(
                PdfPoints::new(72.0),
                PdfPoints::new(700.0),
                format!("Page {} Hello World", i + 1),
                font,
                PdfPoints::new(24.0),
            )
            .unwrap();
        page.objects_mut()
            .create_text_object(
                PdfPoints::new(72.0),
                PdfPoints::new(600.0),
                "Zweite Zeile mit Umlauten: Grüße",
                font,
                PdfPoints::new(14.0),
            )
            .unwrap();
    }
    doc.save_to_bytes().unwrap()
}

/// A one page text PDF encrypted with RC4-128 (lopdf).
pub fn encrypted_pdf(user: &str, owner: &str, permissions: lopdf::Permissions) -> Vec<u8> {
    let bytes = text_pdf(1);
    let mut doc = lopdf::Document::load_mem(&bytes).unwrap();
    let version = lopdf::EncryptionVersion::V2 {
        document: &doc,
        owner_password: owner,
        user_password: user,
        key_length: 128,
        permissions,
    };
    let state = lopdf::EncryptionState::try_from(version).unwrap();
    doc.encrypt(&state).unwrap();
    let mut out = Vec::new();
    doc.save_to(&mut out).unwrap();
    out
}

/// A page with a blue square on layer "Grundriss" and an (empty) second
/// layer "Bemaßung" that starts hidden.
pub fn layered_pdf() -> Vec<u8> {
    use lopdf::Object;
    let mut doc = lopdf::Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let ocg1 = doc.add_object(dictionary! { "Type" => "OCG", "Name" => Object::string_literal("Grundriss") });
    // PDFDocEncoding: "ß" is the byte 0xDF.
    let ocg2 = doc.add_object(
        dictionary! { "Type" => "OCG", "Name" => Object::string_literal(b"Bema\xDFung".to_vec()) },
    );
    let content = b"/OC /oc1 BDC 0 0 1 rg 100 600 200 200 re f EMC\n".to_vec();
    let content_id = doc.add_object(Stream::new(Dictionary::new(), content));
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        "Contents" => content_id,
        "Resources" => dictionary! { "Properties" => dictionary! { "oc1" => ocg1 } },
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1 }),
    );
    let catalog = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
        "OCProperties" => dictionary! {
            "OCGs" => vec![ocg1.into(), ocg2.into()],
            "D" => dictionary! { "Order" => vec![ocg1.into(), ocg2.into()], "OFF" => vec![ocg2.into()] },
        },
    });
    doc.trailer.set("Root", catalog);
    let mut out = Vec::new();
    doc.save_to(&mut out).unwrap();
    out
}

/// A one page PDF with a text field `name` and a check box `agree`.
pub fn form_pdf() -> Vec<u8> {
    let mut doc = lopdf::Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let page_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let text_field = doc.add_object(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Widget",
        "FT" => "Tx",
        "T" => Object::string_literal("name"),
        "V" => Object::string_literal(""),
        "Rect" => vec![100.into(), 700.into(), 300.into(), 720.into()],
        "DA" => Object::string_literal("/Helv 12 Tf 0 g"),
        "F" => 4,
        "P" => page_id,
    });
    let on = doc.add_object(Stream::new(
        dictionary! {"Type" => "XObject", "Subtype" => "Form", "BBox" => vec![0.into(), 0.into(), 12.into(), 12.into()]},
        b"0 g 2 2 8 8 re f".to_vec(),
    ));
    let off = doc.add_object(Stream::new(
        dictionary! {"Type" => "XObject", "Subtype" => "Form", "BBox" => vec![0.into(), 0.into(), 12.into(), 12.into()]},
        b"0 G 0.5 0.5 11 11 re S".to_vec(),
    ));
    let checkbox = doc.add_object(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Widget",
        "FT" => "Btn",
        "T" => Object::string_literal("agree"),
        "V" => "Off",
        "AS" => "Off",
        "Rect" => vec![100.into(), 650.into(), 112.into(), 662.into()],
        "AP" => dictionary! { "N" => dictionary! { "Yes" => on, "Off" => off } },
        "F" => 4,
        "P" => page_id,
    });
    let content = doc.add_object(Stream::new(Dictionary::new(), b"".to_vec()));
    doc.objects.insert(
        page_id,
        Object::Dictionary(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            "Contents" => content,
            "Annots" => vec![text_field.into(), checkbox.into()],
            "Resources" => dictionary! {},
        }),
    );
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1,
        }),
    );
    let catalog = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
        "AcroForm" => dictionary! {
            "Fields" => vec![text_field.into(), checkbox.into()],
            "DA" => Object::string_literal("/Helv 0 Tf 0 g"),
            "DR" => dictionary! { "Font" => dictionary! { "Helv" => font_id } },
        },
    });
    doc.trailer.set("Root", catalog);
    let mut out = Vec::new();
    doc.save_to(&mut out).unwrap();
    out
}

/// Average colour of a rectangular pixel area of an RGBA image.
pub fn average(img: &versolopdf::document::RenderedImage, x0: u32, y0: u32, x1: u32, y1: u32) -> [f32; 3] {
    let mut sum = [0f64; 3];
    let mut n = 0f64;
    for y in y0..y1.min(img.height) {
        for x in x0..x1.min(img.width) {
            let i = ((y * img.width + x) * 4) as usize;
            for (c, s) in sum.iter_mut().enumerate() {
                *s += f64::from(img.rgba[i + c]);
            }
            n += 1.0;
        }
    }
    [(sum[0] / n) as f32, (sum[1] / n) as f32, (sum[2] / n) as f32]
}
