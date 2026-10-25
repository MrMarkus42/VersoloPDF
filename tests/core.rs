//! Integration tests of the document core against the real PDFium engine.

mod common;

use mrpdf::Document;
use mrpdf::error::Error;
use mrpdf::geometry::{PointF, RectF, Rotation};
use mrpdf::model::{AnnotKind, AnnotStyle, FieldKind, Metadata, Rgba, SearchOptions};
use mrpdf::pdf_edit::{AnnotUpdate, NewAnnotation};
use mrpdf::search::search_page;

fn open(pages: usize) -> Document {
    Document::from_bytes(common::text_pdf(pages), "test.pdf", None).unwrap()
}

fn style(color: Rgba) -> AnnotStyle {
    AnnotStyle { color, width: 4.0, author: "Tester".into(), ..AnnotStyle::default() }
}

#[test]
fn opens_and_reports_pages() {
    let _guard = common::lock();
    let doc = open(3);
    assert_eq!(doc.page_count(), 3);
    let g = doc.geometry(0).unwrap();
    let (w, h) = g.view_size();
    assert!((w - 595.0).abs() < 1.0 && (h - 842.0).abs() < 1.0, "{w}x{h}");
    assert!(!doc.is_modified());
    assert!(!doc.is_encrypted());
}

#[test]
fn garbage_is_rejected_without_panic() {
    let _guard = common::lock();
    for bytes in [
        Vec::new(),
        b"hello".to_vec(),
        b"%PDF-1.7\n1 0 obj << /Type /Catalog >> endobj\ntrailer << /Root 1 0 R >>".to_vec(),
        common::text_pdf(1)[..200].to_vec(),
    ] {
        let result = Document::from_bytes(bytes, "bad.pdf", None);
        assert!(result.is_err() || result.unwrap().page_count() <= 1);
    }
}

#[test]
fn renders_page_and_tiles() {
    let _guard = common::lock();
    let doc = open(1);
    let img = doc.render_page(0, 300, 424).unwrap();
    assert_eq!((img.width, img.height), (300, 424));
    assert_eq!(img.rgba.len(), 300 * 424 * 4);
    // The page is mostly white but contains dark text near the top.
    let corner = common::average(&img, 250, 400, 300, 424);
    assert!(corner.iter().all(|&c| c > 250.0), "{corner:?}");
    let has_dark = img.rgba.chunks(4).any(|p| p[0] < 80);
    assert!(has_dark);

    // A tile equals the corresponding area of the full render.
    let tile = doc.render_tile(0, 300, 424, 20, 40, 100, 50).unwrap();
    for y in 0..50 {
        for x in 0..100 {
            let a = ((y * 100 + x) * 4) as usize;
            let b = (((y + 40) * 300 + x + 20) * 4) as usize;
            for c in 0..3 {
                let d = i32::from(tile.rgba[a + c]) - i32::from(img.rgba[b + c]);
                assert!(d.abs() <= 40, "pixel mismatch at {x},{y}");
            }
        }
    }
    assert!(doc.render_page(0, 0, 10).is_err());
    assert!(doc.render_page(5, 10, 10).is_err());
}

#[test]
fn extracts_text_and_searches() {
    let _guard = common::lock();
    let doc = open(2);
    let text = doc.page_text(1).unwrap();
    let all = text.all_text();
    assert!(all.contains("Page 2 Hello World"), "{all}");
    assert!(all.contains("Grüße"), "{all}");
    let hits = search_page(1, &text, "hello", SearchOptions { match_case: false, whole_word: true });
    assert_eq!(hits.len(), 1);
    let r = hits[0].rects[0];
    // Text was placed at x=72, baseline y=700 with 24pt.
    assert!(r.x0 > 100.0 && r.x0 < 250.0 && r.y0 > 690.0 && r.y1 < 730.0, "{r:?}");
    assert!(hits[0].context.contains("Hello"));
}

#[test]
fn annotations_add_list_update_delete_undo() {
    let _guard = common::lock();
    let mut doc = open(1);
    let rev = doc.revision();
    let mut square = NewAnnotation::new(AnnotKind::Square, style(Rgba::RED));
    square.rect = RectF::new(100.0, 100.0, 200.0, 180.0);
    square.contents = "Ein Rechteck".into();
    doc.add_annotation(0, &square).unwrap();
    assert!(doc.revision() > rev);
    assert!(doc.is_modified());

    let mut hl = NewAnnotation::new(AnnotKind::Highlight, style(Rgba::YELLOW));
    hl.quads = vec![RectF::new(72.0, 695.0, 300.0, 722.0)];
    doc.add_annotation(0, &hl).unwrap();

    let mut ink = NewAnnotation::new(AnnotKind::Ink, style(Rgba::BLUE));
    ink.paths = vec![vec![PointF::new(300.0, 300.0), PointF::new(350.0, 320.0), PointF::new(400.0, 300.0)]];
    doc.add_annotation(0, &ink).unwrap();

    let mut line = NewAnnotation::new(AnnotKind::Line, style(Rgba::GREEN));
    line.paths = vec![vec![PointF::new(50.0, 50.0), PointF::new(250.0, 80.0)]];
    line.arrow_end = true;
    doc.add_annotation(0, &line).unwrap();

    let mut note = NewAnnotation::new(AnnotKind::Note, style(Rgba::YELLOW));
    note.rect = RectF::new(500.0, 780.0, 520.0, 800.0);
    note.contents = "Notiz mit Ümlaut".into();
    doc.add_annotation(0, &note).unwrap();

    let mut text = NewAnnotation::new(AnnotKind::FreeText, style(Rgba::BLUE));
    text.rect = RectF::new(300.0, 400.0, 500.0, 450.0);
    text.contents = "Textfeld (mit Klammern)".into();
    doc.add_annotation(0, &text).unwrap();

    let mut stamp = NewAnnotation::new(AnnotKind::Stamp, style(Rgba::GREEN));
    stamp.rect = RectF::new(350.0, 500.0, 500.0, 540.0);
    stamp.label = "Genehmigt".into();
    stamp.icon = Some("Approved".into());
    doc.add_annotation(0, &stamp).unwrap();

    let list = doc.annotations(0).unwrap();
    let kinds: Vec<AnnotKind> = list.iter().map(|a| a.kind).collect();
    assert_eq!(
        kinds,
        vec![
            AnnotKind::Square,
            AnnotKind::Highlight,
            AnnotKind::Ink,
            AnnotKind::Line,
            AnnotKind::Note,
            AnnotKind::FreeText,
            AnnotKind::Stamp
        ]
    );
    assert_eq!(list[0].contents, "Ein Rechteck");
    assert_eq!(list[0].author, "Tester");
    assert_eq!(list[4].contents, "Notiz mit Ümlaut");
    assert!(!list[0].modified.is_empty());
    let r = list[0].rect;
    assert!((r.x0 - 100.0).abs() < 0.5 && (r.y1 - 180.0).abs() < 0.5, "{r:?}");

    // The rectangle's border is visible in the rendering (red pixels at its edge).
    let img = doc.render_page(0, 595, 842).unwrap();
    let px = |x: u32, y: u32| {
        let i = ((y * img.width + x) * 4) as usize;
        [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]]
    };
    let edge = px(150, 842 - 101);
    assert!(edge[0] > 180 && edge[1] < 120, "{edge:?}");
    // Highlight multiplies yellow over white.
    // Sample right of the text (which ends near x=292) but inside the quad.
    let hl_px = px(297, 842 - 710);
    assert!(hl_px[2] < 150 && hl_px[0] > 200, "{hl_px:?}");

    let details = doc.annotation_details(0, 0).unwrap();
    assert_eq!(details.width, 4.0);
    assert!(details.restylable);

    // Update colour and move.
    doc.update_annotation(
        0,
        0,
        &AnnotUpdate {
            color: Some(Rgba::BLUE),
            translate: Some((10.0, -20.0)),
            contents: Some("Geändert".into()),
            ..AnnotUpdate::default()
        },
    )
    .unwrap();
    let a = &doc.annotations(0).unwrap()[0];
    assert_eq!(a.contents, "Geändert");
    assert!((a.rect.x0 - 110.0).abs() < 0.5 && (a.rect.y0 - 80.0).abs() < 0.5, "{:?}", a.rect);
    assert_eq!(doc.annotation_details(0, 0).unwrap().color, Some(Rgba::BLUE));

    doc.delete_annotation(0, 1).unwrap();
    assert_eq!(doc.annotations(0).unwrap().len(), 6);

    assert!(doc.undo().unwrap());
    assert_eq!(doc.annotations(0).unwrap().len(), 7);
    assert!(doc.redo().unwrap());
    assert_eq!(doc.annotations(0).unwrap().len(), 6);
    assert!(doc.delete_annotation(0, 99).is_err());
}

#[test]
fn annotation_on_rotated_page_lands_where_drawn() {
    let _guard = common::lock();
    let mut doc = open(1);
    doc.rotate_pages(&[0], 90).unwrap();
    let g = *doc.geometry(0).unwrap();
    assert_eq!(g.rotation, Rotation::R90);
    let (w, h) = g.view_size();
    assert!((w - 842.0).abs() < 1.0 && (h - 595.0).abs() < 1.0);

    // Draw a filled square at view position (100..150, 50..100).
    let view = RectF::new(100.0, 50.0, 150.0, 100.0);
    let mut sq = NewAnnotation::new(
        AnnotKind::Square,
        AnnotStyle { color: Rgba::BLACK, fill: Some(Rgba::BLACK), ..AnnotStyle::default() },
    );
    sq.rect = g.view_rect_to_pdf(&view);
    doc.add_annotation(0, &sq).unwrap();

    let img = doc.render_page(0, 842, 595).unwrap();
    let inside = common::average(&img, 110, 60, 140, 90);
    let outside = common::average(&img, 200, 60, 230, 90);
    assert!(inside[0] < 30.0, "inside {inside:?}");
    assert!(outside[0] > 220.0, "outside {outside:?}");
}

#[test]
fn page_operations() {
    let _guard = common::lock();
    let mut doc = open(3);
    doc.insert_blank_page(1, 300.0, 400.0).unwrap();
    assert_eq!(doc.page_count(), 4);
    let (w, h) = doc.geometry(1).unwrap().view_size();
    assert_eq!((w.round(), h.round()), (300.0, 400.0));

    doc.delete_pages(&[1]).unwrap();
    assert_eq!(doc.page_count(), 3);
    assert!(doc.delete_pages(&[0, 1, 2]).is_err(), "must keep one page");

    // Move page 3 to the front.
    doc.move_pages(&[2], 0).unwrap();
    let first = doc.page_text(0).unwrap().all_text();
    assert!(first.contains("Page 3"), "{first}");
    let second = doc.page_text(1).unwrap().all_text();
    assert!(second.contains("Page 1"), "{second}");
    assert!(doc.undo().unwrap());
    assert!(doc.page_text(0).unwrap().all_text().contains("Page 1"));

    let extracted = doc.extract_pages_to_bytes(&[2, 0]).unwrap();
    let other = Document::from_bytes(extracted, "x.pdf", None).unwrap();
    assert_eq!(other.page_count(), 2);
    assert!(other.page_text(0).unwrap().all_text().contains("Page 3"));

    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src.pdf");
    std::fs::write(&src, common::text_pdf(2)).unwrap();
    assert_eq!(doc.insert_pages_from(3, &src, None).unwrap(), 2);
    assert_eq!(doc.page_count(), 5);

    doc.rotate_pages(&[0, 1], -90).unwrap();
    assert_eq!(doc.geometry(0).unwrap().rotation, Rotation::R270);
}

#[test]
fn save_and_reopen_preserves_edits() {
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.pdf");
    let mut doc = open(2);
    let mut sq = NewAnnotation::new(AnnotKind::Circle, style(Rgba::RED));
    sq.rect = RectF::new(10.0, 10.0, 60.0, 60.0);
    doc.add_annotation(1, &sq).unwrap();
    doc.rotate_pages(&[0], 180).unwrap();
    doc.set_metadata(&Metadata {
        title: "Titel äöü".into(), author: "Mr. M".into(), ..Metadata::default()
    })
    .unwrap();
    doc.save_as(&path).unwrap();
    assert!(!doc.is_modified());

    let re = Document::open(&path, None).unwrap();
    assert_eq!(re.page_count(), 2);
    assert_eq!(re.geometry(0).unwrap().rotation, Rotation::R180);
    assert_eq!(re.annotations(1).unwrap()[0].kind, AnnotKind::Circle);
    let meta = re.metadata();
    assert_eq!(meta.title, "Titel äöü");
    assert_eq!(meta.author, "Mr. M");
}

#[test]
fn forms_can_be_filled() {
    let _guard = common::lock();
    let mut doc = Document::from_bytes(common::form_pdf(), "form.pdf", None).unwrap();
    let fields = doc.form_fields(0).unwrap();
    assert_eq!(fields.len(), 2, "{fields:?}");
    assert_eq!(fields[0].name, "name");
    assert!(matches!(fields[0].kind, FieldKind::Text { .. }));
    assert_eq!(fields[1].kind, FieldKind::CheckBox);
    assert!(!fields[1].checked);

    doc.set_text_field(0, 0, "Max Mustermann").unwrap();
    doc.set_checked(0, 1, true).unwrap();
    let fields = doc.form_fields(0).unwrap();
    assert_eq!(fields[0].value, "Max Mustermann");
    assert!(fields[1].checked);

    // The new values are visible: dark text pixels inside the text field
    // (PDF 100..300 x 700..720) and a filled check box (100..112 x 650..662).
    let img = doc.render_page(0, 595, 842).unwrap();
    let dark = |x0: u32, y0: u32, x1: u32, y1: u32| {
        (y0..y1)
            .flat_map(|y| (x0..x1).map(move |x| (x, y)))
            .filter(|&(x, y)| img.rgba[((y * 595 + x) * 4) as usize] < 100)
            .count()
    };
    assert!(dark(102, 842 - 718, 298, 842 - 702) > 50, "text not rendered");
    assert!(dark(103, 842 - 659, 109, 842 - 653) > 20, "check mark not rendered");

    let bytes = doc.to_bytes().unwrap();
    let re = Document::from_bytes(bytes, "form.pdf", None).unwrap();
    let fields = re.form_fields(0).unwrap();
    assert_eq!(fields[0].value, "Max Mustermann");
    assert!(fields[1].checked);

    // Unticking works as well and wrong value types are rejected.
    doc.set_checked(0, 1, false).unwrap();
    assert!(!doc.form_fields(0).unwrap()[1].checked);
    assert!(doc.set_checked(0, 0, true).is_err());
}

#[test]
fn export_image() {
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let doc = open(1);
    let png = dir.path().join("p.png");
    doc.export_page_image(0, 36.0, &png).unwrap();
    let img = image::open(&png).unwrap();
    assert_eq!((img.width(), img.height()), (298, 421));
    let jpg = dir.path().join("p.jpg");
    doc.export_page_image(0, 36.0, &jpg).unwrap();
    assert!(image::open(&jpg).is_ok());
}

#[test]
fn missing_file_is_io_error() {
    let _guard = common::lock();
    let err = Document::open(std::path::Path::new("/nonexistent/file.pdf"), None).err().unwrap();
    assert!(matches!(err, Error::Io { .. }));
}

#[test]
fn encrypted_documents_stay_encrypted() {
    let _guard = common::lock();
    let bytes = common::encrypted_pdf("geheim", "owner-pw", lopdf::Permissions::all());
    assert!(matches!(Document::from_bytes(bytes.clone(), "e.pdf", None), Err(Error::PasswordRequired)));
    assert!(matches!(
        Document::from_bytes(bytes.clone(), "e.pdf", Some("falsch")),
        Err(Error::WrongPassword)
    ));
    let mut doc = Document::from_bytes(bytes, "e.pdf", Some("geheim")).unwrap();
    assert!(doc.is_encrypted());
    assert!(doc.page_text(0).unwrap().all_text().contains("Hello"));

    let mut sq = NewAnnotation::new(AnnotKind::Square, style(Rgba::RED));
    sq.rect = RectF::new(50.0, 50.0, 150.0, 150.0);
    doc.add_annotation(0, &sq).unwrap();
    // An engine edit followed by a structural edit.
    doc.rotate_pages(&[0], 90).unwrap();
    sq.rect = RectF::new(200.0, 50.0, 300.0, 150.0);
    doc.add_annotation(0, &sq).unwrap();

    let out = doc.to_bytes().unwrap();
    assert!(
        matches!(Document::from_bytes(out.clone(), "e.pdf", None), Err(Error::PasswordRequired)),
        "saved file must still be protected"
    );
    let re = Document::from_bytes(out, "e.pdf", Some("geheim")).unwrap();
    assert_eq!(re.annotations(0).unwrap().len(), 2);
    assert_eq!(re.geometry(0).unwrap().rotation, Rotation::R90);
}

#[test]
fn document_permissions_are_respected() {
    let _guard = common::lock();
    // Empty user password: opens without prompt, but only printing is allowed.
    let bytes = common::encrypted_pdf("", "owner-pw", lopdf::Permissions::PRINTABLE);
    let mut doc = Document::from_bytes(bytes, "r.pdf", None).unwrap();
    let perms = doc.permissions();
    assert!(perms.encrypted && !perms.annotate && !perms.assemble);
    let mut sq = NewAnnotation::new(AnnotKind::Square, style(Rgba::RED));
    sq.rect = RectF::new(50.0, 50.0, 150.0, 150.0);
    assert!(matches!(doc.add_annotation(0, &sq), Err(Error::NotPermitted)));
    assert!(matches!(doc.rotate_pages(&[0], 90), Err(Error::NotPermitted)));
}

#[test]
fn password_protection_round_trip() {
    let _guard = common::lock();
    let mut doc = open(1);
    let opts = mrpdf::security::ProtectOptions {
        user_password: "lesen".into(),
        owner_password: "admin".into(),
        allow_copy: false,
        ..Default::default()
    };
    doc.protect(&opts).unwrap();
    assert!(doc.is_encrypted());
    assert!(doc.has_full_access(), "the session continues with owner rights");
    let bytes = doc.to_bytes().unwrap();
    assert!(matches!(Document::from_bytes(bytes.clone(), "p.pdf", None), Err(Error::PasswordRequired)));

    let user = Document::from_bytes(bytes.clone(), "p.pdf", Some("lesen")).unwrap();
    let perms = user.permissions();
    assert!(!perms.copy && perms.print, "{perms:?}");
    assert!(user.page_text(0).unwrap().all_text().contains("Hello"));
    assert!(!user.has_full_access());

    let mut owner = Document::from_bytes(bytes, "p.pdf", Some("admin")).unwrap();
    assert!(owner.has_full_access());
    owner.unprotect().unwrap();
    let plain = Document::from_bytes(owner.to_bytes().unwrap(), "p.pdf", None).unwrap();
    assert!(!plain.is_encrypted());
    assert!(plain.page_text(0).unwrap().all_text().contains("Hello"));
}

#[test]
fn attachments_can_be_added_saved_and_removed() {
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("notiz.txt");
    std::fs::write(&src, "Inhalt des Anhangs").unwrap();
    let mut doc = open(1);
    assert!(doc.attachments().is_empty());
    doc.add_attachment(&src).unwrap();
    assert_eq!(doc.attachments(), vec![("notiz.txt".to_string(), 18)]);
    let out = dir.path().join("kopie.txt");
    doc.save_attachment(0, &out).unwrap();
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "Inhalt des Anhangs");
    doc.delete_attachment(0).unwrap();
    assert!(doc.attachments().is_empty());
    doc.undo().unwrap();
    assert_eq!(doc.attachments().len(), 1);
}

#[test]
fn bookmarks_can_be_edited() {
    let _guard = common::lock();
    let mut doc = open(3);
    assert!(doc.bookmarks().is_empty());
    doc.add_bookmark(&[], "Kapitel 1", 0).unwrap();
    doc.add_bookmark(&[], "Kapitel 2", 2).unwrap();
    doc.add_bookmark(&[0], "Abschnitt 1.1", 1).unwrap();
    let b = doc.bookmarks();
    assert_eq!(b.len(), 2);
    assert_eq!(b[0].title, "Kapitel 1");
    assert_eq!(b[0].children[0].title, "Abschnitt 1.1");
    assert_eq!(b[0].children[0].page, Some(1));
    assert_eq!(b[1].page, Some(2));

    doc.rename_bookmark(&[1], "Zusammenfassung").unwrap();
    doc.delete_bookmark(&[0]).unwrap();
    let b = doc.bookmarks();
    assert_eq!(b.len(), 1);
    assert_eq!(b[0].title, "Zusammenfassung");
    assert!(doc.delete_bookmark(&[5]).is_err());

    let re = Document::from_bytes(doc.to_bytes().unwrap(), "b.pdf", None).unwrap();
    assert_eq!(re.bookmarks()[0].title, "Zusammenfassung");
}

#[test]
fn image_stamp_is_rendered() {
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("signatur.png");
    // Blue square with a transparent border.
    let img = image::RgbaImage::from_fn(100, 100, |x, y| {
        if (10..90).contains(&x) && (10..90).contains(&y) {
            image::Rgba([0, 0, 255, 255])
        } else {
            image::Rgba([0, 0, 0, 0])
        }
    });
    img.save(&png).unwrap();
    let mut doc = open(1);
    doc.add_image_stamp(0, &png, &AnnotStyle::default()).unwrap();
    let annots = doc.annotations(0).unwrap();
    assert_eq!(annots.len(), 1);
    assert_eq!(annots[0].kind, AnnotKind::Stamp);
    let page = doc.render_page(0, 595, 842).unwrap();
    let centre = common::average(&page, 290, 413, 305, 428);
    assert!(centre[2] > 200.0 && centre[0] < 60.0, "{centre:?}");
    // The transparent border keeps the white page visible.
    let r = annots[0].rect;
    let edge_x = (r.x0 + 2.0) as u32;
    let edge_y = (842.0 - r.y1 + 2.0) as u32;
    let edge = common::average(&page, edge_x, edge_y, edge_x + 3, edge_y + 3);
    assert!(edge.iter().all(|&c| c > 200.0), "{edge:?}");

    let bad = dir.path().join("kaputt.png");
    std::fs::write(&bad, b"not an image").unwrap();
    assert!(doc.add_image_stamp(0, &bad, &AnnotStyle::default()).is_err());
}
