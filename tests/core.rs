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
    let tile =
        doc.render_tile(0, 300, 424, 20, 40, 100, 50, mrpdf::document::RenderOptions::default()).unwrap();
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
fn comment_filter_and_content_objects() {
    use mrpdf::document::RenderOptions;
    use mrpdf::model::AnnotFilter;
    let _guard = common::lock();
    let mut doc = open(1);
    for (x, author) in [(100.0, "Tester"), (300.0, "Other")] {
        let mut sq = NewAnnotation::new(
            AnnotKind::Square,
            AnnotStyle {
                color: Rgba::BLACK,
                fill: Some(Rgba::BLACK),
                author: author.into(),
                ..AnnotStyle::default()
            },
        );
        sq.rect = RectF::new(x, 100.0, x + 50.0, 150.0);
        doc.add_annotation(0, &sq).unwrap();
    }
    let render = |doc: &Document, filter: u32| {
        doc.render_tile(0, 595, 842, 0, 0, 595, 842, RenderOptions { filter, ..RenderOptions::default() })
            .unwrap()
    };
    // PDF y 100..150 is image row 692..742.
    let dark = |img: &mrpdf::document::RenderedImage, x: u32| {
        common::average(img, x + 10, 702, x + 40, 732)[0] < 60.0
    };
    assert_eq!(doc.filter_key(), 0);
    let img = render(&doc, 0);
    assert!(dark(&img, 100) && dark(&img, 300));

    doc.set_annot_filter(AnnotFilter { hidden_authors: vec!["Other".into()], ..AnnotFilter::default() });
    let key = doc.filter_key();
    assert_ne!(key, 0);
    let img = render(&doc, key);
    assert!(dark(&img, 100) && !dark(&img, 300), "only the other author's square is hidden");
    // Without the key (printing, export) nothing is filtered, and the
    // hidden flags of the document are restored after rendering.
    let img = render(&doc, 0);
    assert!(dark(&img, 100) && dark(&img, 300));
    doc.set_annot_filter(AnnotFilter { hidden_kinds: vec![AnnotKind::Square], ..AnnotFilter::default() });
    let img = render(&doc, doc.filter_key());
    assert!(!dark(&img, 100) && !dark(&img, 300));
    doc.set_annot_filter(AnnotFilter::default());
    assert_eq!(doc.filter_key(), 0);
    assert!(!doc.is_modified() || doc.can_undo().is_some(), "the filter is no document change");

    // "Inhalt hervorheben": the heading's text object.
    let r = doc.object_at(0, PointF::new(100.0, 708.0)).unwrap().expect("text object");
    assert!(r.x0 < 80.0 && r.x1 > 200.0 && r.y0 > 690.0 && r.y1 < 730.0, "{r:?}");
    assert!(doc.object_at(0, PointF::new(500.0, 300.0)).unwrap().is_none());
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

#[test]
fn view_rotation_matches_geometry() {
    let _guard = common::lock();
    use mrpdf::document::RenderOptions;
    use mrpdf::geometry::PageGeometry;
    let mut doc = open(1);
    // A black square at a known PDF position.
    let mut sq = NewAnnotation::new(
        AnnotKind::Square,
        AnnotStyle { color: Rgba::BLACK, fill: Some(Rgba::BLACK), ..AnnotStyle::default() },
    );
    sq.rect = RectF::new(100.0, 600.0, 160.0, 660.0);
    doc.add_annotation(0, &sq).unwrap();
    for rotation in [Rotation::R90, Rotation::R180, Rotation::R270] {
        let base = *doc.geometry(0).unwrap();
        let geo = PageGeometry::new(base.crop, base.rotation.rotated_by(rotation.degrees()));
        let (w, h) = geo.view_size();
        let img = doc
            .render_tile(
                0,
                w as u32,
                h as u32,
                0,
                0,
                w as u32,
                h as u32,
                RenderOptions { rotation, annotations: true, ..RenderOptions::default() },
            )
            .unwrap();
        let v = geo.pdf_rect_to_view(&sq.rect);
        let c = v.center();
        let inside = common::average(&img, c.x as u32 - 5, c.y as u32 - 5, c.x as u32 + 5, c.y as u32 + 5);
        assert!(inside[0] < 40.0, "{rotation:?}: {inside:?}");
        // Without annotations the spot is white.
        let plain = doc
            .render_tile(
                0,
                w as u32,
                h as u32,
                0,
                0,
                w as u32,
                h as u32,
                RenderOptions { rotation, annotations: false, ..RenderOptions::default() },
            )
            .unwrap();
        let spot = common::average(&plain, c.x as u32 - 5, c.y as u32 - 5, c.x as u32 + 5, c.y as u32 + 5);
        assert!(spot[0] > 220.0, "{rotation:?}: {spot:?}");
    }
}

#[test]
fn document_structure_operations() {
    let _guard = common::lock();
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.pdf");
    let b = dir.path().join("b.pdf");
    std::fs::write(&a, common::text_pdf(2)).unwrap();
    std::fs::write(&b, common::text_pdf(3)).unwrap();

    let merged = Document::merge_files(&[a.clone(), b.clone()], "m.pdf").unwrap();
    assert_eq!(merged.page_count(), 5);
    let parts = merged.split(2, dir.path(), "teil").unwrap();
    assert_eq!(parts.len(), 3);
    assert_eq!(Document::open(&parts[2], None).unwrap().page_count(), 1);

    let mut doc = open(3);
    let text = |d: &Document, p: usize| d.page_text(p).unwrap().all_text();
    doc.duplicate_pages(&[1]).unwrap();
    assert_eq!(doc.page_count(), 4);
    assert!(text(&doc, 2).contains("Page 2"));
    let groups = doc.find_duplicate_pages().unwrap();
    assert_eq!(groups, vec![vec![1, 2]]);

    doc.swap_pages(0, 3).unwrap();
    assert!(text(&doc, 0).contains("Page 3"));
    doc.reverse_pages().unwrap();
    assert!(text(&doc, 3).contains("Page 3"));

    let mut doc = open(3);
    doc.replace_pages(&[1], &b, 2).unwrap();
    assert_eq!(doc.page_count(), 3);
    assert!(text(&doc, 1).contains("Page 3"), "{}", text(&doc, 1));
}

#[test]
fn page_shape_operations() {
    let _guard = common::lock();
    let mut doc = open(2);
    doc.resize_pages(&[0], (419.53, 595.28), true).unwrap();
    let (w, h) = doc.geometry(0).unwrap().view_size();
    assert!((w - 419.5).abs() < 1.0 && (h - 595.3).abs() < 1.0, "{w}x{h}");
    assert!(doc.page_text(0).unwrap().all_text().contains("Page 1"));
    // The text was scaled and moved with the page.
    let hits = mrpdf::search::search_page(
        0,
        &doc.page_text(0).unwrap(),
        "Hello",
        SearchOptions { match_case: true, whole_word: false },
    );
    assert!(hits[0].rects[0].x1 < 419.0);

    doc.crop_pages(&[1], [50.0, 50.0, 50.0, 50.0]).unwrap();
    let (w, h) = doc.geometry(1).unwrap().view_size();
    assert!((w - 495.0).abs() < 1.0 && (h - 742.0).abs() < 1.0, "{w}x{h}");
    doc.add_margins(&[1], [20.0, 20.0, 20.0, 20.0]).unwrap();
    let (w, _) = doc.geometry(1).unwrap().view_size();
    assert!((w - 535.0).abs() < 1.0, "{w}");
    assert!(doc.crop_pages(&[1], [400.0, 0.0, 400.0, 0.0]).is_err());
}

#[test]
fn page_marks_and_labels() {
    use mrpdf::pages::{Anchor, LabelStyle, PageMark, TextMark};
    let _guard = common::lock();
    let mut doc = open(3);
    let mark = |text: &str, anchor: Anchor| TextMark {
        text: text.into(),
        anchor,
        font_size: 12.0,
        color: Rgba::rgb(200, 0, 0),
        opacity: 1.0,
        rotation: 0.0,
        margin: 24.0,
        bold: false,
    };
    let footer = PageMark::Texts {
        marks: vec![mark("Seite <<page>> von <<pages>>", Anchor::BottomCenter)],
        behind: false,
    };
    doc.add_page_marks(&[0, 1, 2], &footer, &|t, p| mrpdf::pages::expand_placeholders(t, p, 3, "", "x.pdf"))
        .unwrap();
    assert!(doc.page_text(1).unwrap().all_text().contains("Seite 2 von 3"));

    let mut wm = mark("ENTWURF", Anchor::Center);
    wm.font_size = 80.0;
    wm.rotation = 45.0;
    wm.opacity = 0.4;
    wm.bold = true;
    doc.add_page_marks(&[0], &PageMark::Texts { marks: vec![wm], behind: false }, &|t, _| t.to_string())
        .unwrap();
    assert!(doc.page_text(0).unwrap().all_text().contains("ENTWURF"));

    let bates = PageMark::Texts { marks: vec![mark("", Anchor::BottomRight)], behind: false };
    doc.add_page_marks(&[0, 1], &bates, &|_, p| mrpdf::pages::bates_number("ABC", 1 + p as u64, 6, ""))
        .unwrap();
    assert!(doc.page_text(1).unwrap().all_text().contains("ABC000002"));

    doc.add_page_marks(&[2], &PageMark::Fill { color: Rgba::rgb(220, 240, 255) }, &|t, _| t.to_string())
        .unwrap();
    let img = doc.render_page(2, 100, 141).unwrap();
    let corner = common::average(&img, 2, 2, 6, 6);
    assert!(corner[0] < 235.0 && corner[2] > 245.0, "{corner:?}");
    assert!(
        doc.page_text(2).unwrap().all_text().contains("Page 3"),
        "content stays visible above the background"
    );

    doc.set_page_labels(0, LabelStyle::RomanLower, "", 1).unwrap();
    doc.set_page_labels(2, LabelStyle::Decimal, "A-", 1).unwrap();
    assert_eq!(doc.page_label(0).as_deref(), Some("i"));
    assert_eq!(doc.page_label(1).as_deref(), Some("ii"));
    assert_eq!(doc.page_label(2).as_deref(), Some("A-1"));
}

fn temp_file(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("mrpdf-test-{}-{name}", std::process::id()))
}

#[test]
fn form_fields_can_be_created_reset_and_exchanged() {
    use mrpdf::forms::NewFieldKind;
    let _guard = common::lock();
    let mut doc = open(1);
    let r = |x: f32, y: f32, w: f32, h: f32| RectF::new(x, y, x + w, y + h);
    assert_eq!(
        doc.create_field(0, r(100.0, 700.0, 200.0, 20.0), &NewFieldKind::Text { multiline: false }, "")
            .unwrap(),
        "Text1"
    );
    assert_eq!(
        doc.create_field(0, r(100.0, 660.0, 200.0, 20.0), &NewFieldKind::Text { multiline: false }, "")
            .unwrap(),
        "Text2"
    );
    doc.create_field(0, r(100.0, 620.0, 14.0, 14.0), &NewFieldKind::CheckBox, "agree").unwrap();
    for (i, x) in [100.0, 130.0].iter().enumerate() {
        let kind = NewFieldKind::Radio { group: "colour".into(), export: format!("c{i}") };
        assert_eq!(doc.create_field(0, r(*x, 580.0, 14.0, 14.0), &kind, "").unwrap(), "colour");
    }
    let options = vec!["rot".to_string(), "grün".to_string()];
    doc.create_field(
        0,
        r(100.0, 540.0, 120.0, 20.0),
        &NewFieldKind::ComboBox { options: options.clone() },
        "",
    )
    .unwrap();
    doc.create_field(0, r(100.0, 460.0, 120.0, 60.0), &NewFieldKind::ListBox { options }, "").unwrap();
    doc.create_field(
        0,
        r(300.0, 460.0, 80.0, 24.0),
        &NewFieldKind::ResetButton { caption: "Reset".into() },
        "",
    )
    .unwrap();
    doc.create_field(0, r(300.0, 380.0, 160.0, 50.0), &NewFieldKind::Signature { label: "Sign".into() }, "")
        .unwrap();

    let fields = doc.form_fields(0).unwrap();
    let kinds: Vec<_> = fields.iter().map(|f| f.kind.clone()).collect();
    assert_eq!(fields.len(), 9, "{fields:#?}");
    assert!(kinds.contains(&FieldKind::CheckBox));
    assert_eq!(kinds.iter().filter(|k| **k == FieldKind::RadioButton).count(), 2);
    assert!(kinds.contains(&FieldKind::ComboBox) && kinds.contains(&FieldKind::ListBox));
    assert!(kinds.contains(&FieldKind::PushButton) && kinds.contains(&FieldKind::Signature));

    // The new fields can be filled through the normal API.
    let text1 = fields.iter().find(|f| f.name == "Text1").unwrap().annot_index;
    doc.set_text_field(0, text1, "Hallo").unwrap();
    let radio = fields.iter().filter(|f| f.kind == FieldKind::RadioButton).nth(1).unwrap().annot_index;
    doc.set_checked(0, radio, true).unwrap();
    let values = doc.field_values().unwrap();
    assert!(values.contains(&("Text1".into(), "Hallo".into())), "{values:?}");
    assert!(values.contains(&("colour".into(), "c1".into())), "{values:?}");

    // XFDF export, reset, import.
    let path = temp_file("fields.xfdf");
    doc.export_form_data(&path).unwrap();
    doc.reset_form().unwrap();
    let values = doc.field_values().unwrap();
    assert!(values.contains(&("Text1".into(), String::new())), "{values:?}");
    assert!(values.contains(&("colour".into(), "Off".into())), "{values:?}");
    assert_eq!(doc.import_form_data(&path).unwrap(), values.len());
    let values = doc.field_values().unwrap();
    assert!(values.contains(&("Text1".into(), "Hallo".into())), "{values:?}");
    assert!(values.contains(&("colour".into(), "c1".into())), "{values:?}");
    std::fs::remove_file(&path).ok();

    // Survives saving and reloading.
    let re = Document::from_bytes(doc.to_bytes().unwrap(), "f.pdf", None).unwrap();
    assert_eq!(re.form_fields(0).unwrap().len(), 9);
    assert_eq!(re.form_fields(0).unwrap().iter().find(|f| f.name == "Text1").unwrap().value, "Hallo");
}

#[test]
fn comments_are_exchanged_summarized_and_deleted() {
    let _guard = common::lock();
    let mut doc = open(2);
    let mut note = NewAnnotation::new(AnnotKind::Note, style(Rgba::rgb(255, 200, 0)));
    note.rect = RectF::new(100.0, 700.0, 120.0, 720.0);
    note.contents = "Bitte prüfen (wichtig)".into();
    doc.add_annotation(0, &note).unwrap();
    let mut square = NewAnnotation::new(AnnotKind::Square, style(Rgba::rgb(200, 0, 0)));
    square.rect = RectF::new(100.0, 400.0, 300.0, 500.0);
    doc.add_annotation(1, &square).unwrap();

    let path = temp_file("comments.xfdf");
    assert_eq!(doc.export_comments(&path).unwrap(), 2);
    let xml = std::fs::read_to_string(&path).unwrap();
    assert!(xml.contains("<text") && xml.contains("<square"), "{xml}");

    let summary = mrpdf::report::build_pdf(
        "Summary",
        &doc.comments()
            .unwrap()
            .iter()
            .map(|c| mrpdf::report::Line::new(format!("{} {}", c.page + 1, c.contents)))
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let summary = Document::from_bytes(summary, "s.pdf", None).unwrap();
    assert!(summary.page_text(0).unwrap().all_text().contains("Bitte prüfen (wichtig)"));

    assert_eq!(doc.delete_all_comments().unwrap(), 2);
    assert!(doc.comments().unwrap().is_empty());
    assert_eq!(doc.import_comments(&path, "Import").unwrap(), 2);
    let comments = doc.comments().unwrap();
    assert_eq!(comments.len(), 2);
    assert_eq!(comments[0].contents, "Bitte prüfen (wichtig)");
    assert_eq!(comments[1].page, 1);
    assert!(doc.undo().unwrap());
    assert!(doc.comments().unwrap().is_empty());
    std::fs::remove_file(&path).ok();
}

#[test]
fn table_of_contents_pages_link_to_targets() {
    let _guard = common::lock();
    let mut doc = open(3);
    let lines: Vec<_> = (0..3)
        .map(|p| mrpdf::report::Line {
            link: Some(p),
            right: (p + 2).to_string(),
            ..mrpdf::report::Line::new(format!("Kapitel {p}"))
        })
        .collect();
    let mut bytes = doc.to_bytes().unwrap();
    let (new, inserted) = mrpdf::report::insert_front(&bytes, None, &lines).unwrap();
    bytes = new;
    assert_eq!(inserted, 1);
    let doc = Document::from_bytes(bytes, "toc.pdf", None).unwrap();
    assert_eq!(doc.page_count(), 4);
    assert!(doc.page_text(0).unwrap().all_text().contains("Kapitel 2"));
    let links = doc.links(0).unwrap();
    assert_eq!(links.len(), 3, "{links:?}");
    assert!(links.iter().any(|l| matches!(l.target, mrpdf::model::LinkTarget::Page(3))), "{links:?}");
}

#[test]
fn redaction_removes_content_for_good() {
    let _guard = common::lock();
    let mut doc = open(3);
    // Looks for the text in all (decompressed) streams, literal or hex-encoded.
    let contains = |pdf: &[u8], text: &str| {
        let doc = lopdf::Document::load_mem(pdf).unwrap();
        let hex: String = text.bytes().map(|b| format!("{b:02x}")).collect();
        doc.objects.values().any(|o| match o {
            lopdf::Object::Stream(s) => {
                let data = s.decompressed_content().unwrap_or_else(|_| s.content.clone());
                let lower = String::from_utf8_lossy(&data).to_lowercase();
                lower.contains(&text.to_lowercase()) || lower.replace(char::is_whitespace, "").contains(&hex)
            }
            _ => false,
        })
    };
    assert!(contains(&doc.to_bytes().unwrap(), "Page 2"), "test PDF should contain the text");

    let style = AnnotStyle { color: Rgba::RED, fill: Some(Rgba::BLACK), width: 1.0, ..AnnotStyle::default() };
    let opts = SearchOptions { match_case: true, whole_word: false };
    assert_eq!(doc.mark_for_redaction("Page 2", opts, &style).unwrap(), 1);
    let marked = doc.annotations(1).unwrap();
    assert_eq!(marked.iter().filter(|a| a.kind == AnnotKind::Redact).count(), 1);
    let rect = marked.iter().find(|a| a.kind == AnnotKind::Redact).unwrap().rect;

    assert_eq!(doc.apply_redactions(100.0).unwrap(), 1);
    // Page 2 has no text left, the other pages are untouched.
    assert!(doc.page_text(1).unwrap().all_text().trim().is_empty());
    assert!(doc.page_text(0).unwrap().all_text().contains("Page 1"));
    assert!(doc.page_text(2).unwrap().all_text().contains("Page 3"));
    assert!(doc.annotations(1).unwrap().iter().all(|a| a.kind != AnnotKind::Redact));
    let bytes = doc.to_bytes().unwrap();
    assert!(!contains(&bytes, "Page 2"), "redacted text must not remain in the file");
    assert!(!bytes.windows(5).any(|w| w == b"/Prev"), "no older revisions");

    // The area is black, the rest of the line is still visible as image.
    let img = doc.render_page(1, 595, 842).unwrap();
    let c = rect.center();
    let px =
        common::average(&img, c.x as u32 - 2, 842 - c.y as u32 - 2, c.x as u32 + 2, 842 - c.y as u32 + 2);
    assert!(px.iter().all(|v| *v < 30.0), "{px:?}");
    // "World" (x 200..280, baseline 700) is still there at its old place.
    let dark = (842 - 724..842 - 700)
        .flat_map(|y| (200..280).map(move |x| (x, y)))
        .filter(|&(x, y)| img.rgba[((y * 595 + x) * 4) as usize] < 100)
        .count();
    assert!(dark > 100, "text outside the redaction must stay visible ({dark})");
    let re = Document::from_bytes(bytes, "r.pdf", None).unwrap();
    assert_eq!(re.page_count(), 3);

    assert!(doc.undo().unwrap());
    assert!(doc.page_text(1).unwrap().all_text().contains("Page 2"));
}

#[test]
fn bookmark_tools() {
    use mrpdf::outline::NewBookmark;
    let _guard = common::lock();
    let mut doc = open(3);
    let nb = |title: &str, page: usize, level: usize| NewBookmark { title: title.into(), page, level };

    // Headings (24 pt lines) become bookmarks.
    let headings = doc.heading_bookmarks().unwrap();
    assert_eq!(
        headings.iter().map(|b| b.title.as_str()).collect::<Vec<_>>(),
        ["Page 1 Hello World", "Page 2 Hello World", "Page 3 Hello World"]
    );

    let items =
        [nb("kapitel 10", 2, 0), nb("unterpunkt", 2, 1), nb("kapitel 2", 1, 0), nb("kapitel 2", 1, 0)];
    assert_eq!(doc.append_bookmarks(&items).unwrap(), 4);
    let titles = |doc: &Document| doc.bookmarks().iter().map(|b| b.title.clone()).collect::<Vec<_>>();
    assert_eq!(titles(&doc), ["kapitel 10", "kapitel 2", "kapitel 2"]);
    assert_eq!(doc.bookmarks()[0].children[0].title, "unterpunkt");

    assert_eq!(doc.remove_duplicate_bookmarks().unwrap(), 1);
    assert_eq!(doc.map_bookmark_titles(mrpdf::outline::title_case).unwrap(), 3);
    doc.sort_bookmarks(false).unwrap();
    assert_eq!(titles(&doc), ["Kapitel 2", "Kapitel 10"]);
    assert_eq!(doc.bookmarks()[1].children[0].title, "Unterpunkt", "children move with their parent");

    // Text export can be read back as a bookmark list.
    let text = doc.export_bookmarks(false);
    assert_eq!(text, "Kapitel 2\t2\nKapitel 10\t3\n\tUnterpunkt\t3\n");
    let parsed = mrpdf::outline::parse_bookmark_list(&text, 3);
    assert_eq!(parsed[2], nb("Unterpunkt", 2, 1));
    assert!(doc.export_bookmarks(true).contains("<li>Kapitel 10 <span class=\"page\">3</span>"));

    // Table of contents in front, with working links.
    assert_eq!(doc.insert_toc("Inhalt").unwrap(), 1);
    assert_eq!(doc.page_count(), 4);
    let toc = doc.page_text(0).unwrap().all_text();
    assert!(toc.contains("Inhalt") && toc.contains("Kapitel 10"), "{toc}");
    assert!(toc.contains('4'), "page numbers include the new page: {toc}");
    assert_eq!(doc.links(0).unwrap().len(), 3);
    assert_eq!(doc.bookmarks()[1].page, Some(3), "bookmarks still point to their pages");

    doc.delete_all_bookmarks().unwrap();
    assert!(doc.bookmarks().is_empty());
}

#[test]
fn review_and_conversion() {
    let _guard = common::lock();
    let mut doc = open(2);
    let count = doc.word_count().unwrap();
    assert_eq!((count.pages, count.words), (2, 18), "{count:?}");

    // Comparison: the longer document adds the lines of page 3.
    let longer = open(3);
    let changes = mrpdf::compare::diff(&doc.text_lines().unwrap(), &longer.text_lines().unwrap());
    let added: Vec<_> = changes
        .iter()
        .filter_map(|c| match c {
            mrpdf::compare::Change::Added(p, t) => Some((*p, t.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(added.len(), 2, "{changes:?}");
    assert!(added.iter().all(|(p, _)| *p == 2));

    // Accessibility facts of a plain generated PDF.
    let s = doc.accessibility_structure().unwrap();
    assert!(!s.tagged && s.language.is_none());
    assert!(doc.pages_without_text().unwrap().is_empty());

    // Text and Word export.
    let txt = temp_file("export.txt");
    doc.export_text(&txt).unwrap();
    let text = std::fs::read_to_string(&txt).unwrap();
    assert!(text.contains("Page 2 Hello World") && text.contains('\u{c}'), "{text}");
    let docx = temp_file("export.docx");
    doc.export_word(&docx).unwrap();
    let bytes = std::fs::read(&docx).unwrap();
    assert_eq!(&bytes[..2], b"PK");
    let xml = String::from_utf8_lossy(&bytes);
    assert!(
        xml.contains("Heading1\"/></w:pPr><w:r><w:t xml:space=\"preserve\">Page 1 Hello World"),
        "headings are styled"
    );
    std::fs::remove_file(&txt).ok();
    std::fs::remove_file(&docx).ok();

    // Grayscale: a red image stamp and a red rectangle turn gray.
    let png = temp_file("red.png");
    image::RgbImage::from_pixel(40, 40, image::Rgb([220, 20, 20])).save(&png).unwrap();
    doc.add_image_stamp(0, &png, &style(Rgba::RED)).unwrap();
    let mut rect = NewAnnotation::new(AnnotKind::Square, style(Rgba::rgb(0, 0, 255)));
    rect.rect = RectF::new(300.0, 300.0, 400.0, 400.0);
    doc.add_annotation(1, &rect).unwrap();
    doc.to_grayscale().unwrap();
    let is_gray = |img: &mrpdf::document::RenderedImage| {
        img.rgba.as_chunks::<4>().0.iter().all(|p| p[0].abs_diff(p[1]) < 8 && p[1].abs_diff(p[2]) < 8)
    };
    assert!(is_gray(&doc.render_page(0, 298, 421).unwrap()), "page 1 should be gray");
    assert!(is_gray(&doc.render_page(1, 298, 421).unwrap()), "page 2 should be gray");
    let page1 = doc.render_page(0, 298, 421).unwrap();
    assert!(page1.rgba.as_chunks::<4>().0.iter().any(|p| p[0] < 200), "stamp still visible");

    // Contents to images: text is gone, comments stay.
    assert_eq!(doc.contents_to_images(&[1], 72.0).unwrap(), 1);
    assert!(doc.page_text(1).unwrap().all_text().trim().is_empty());
    assert!(doc.page_text(0).unwrap().all_text().contains("Page 1"));
    assert!(doc.annotations(1).unwrap().iter().any(|a| a.kind == AnnotKind::Square));

    // New document from an image and a PDF.
    let pdf = temp_file("part.pdf");
    std::fs::write(&pdf, common::text_pdf(2)).unwrap();
    let merged = Document::from_files(&[png.clone(), pdf.clone()], "neu.pdf").unwrap();
    assert_eq!(merged.page_count(), 3);
    let (w, _) = merged.geometry(0).unwrap().view_size();
    assert!((w - 40.0 * 72.0 / 150.0).abs() < 0.5, "image page size {w}");
    std::fs::remove_file(&png).ok();
    std::fs::remove_file(&pdf).ok();
}

fn page_texts(doc: &Document) -> Vec<String> {
    (0..doc.page_count())
        .map(|p| doc.page_text(p).unwrap().all_text().lines().next().unwrap_or("").trim().to_string())
        .collect()
}

#[test]
fn page_dialog_operations() {
    use mrpdf::outline::NewBookmark;
    let _guard = common::lock();
    let src = temp_file("source.pdf");
    std::fs::write(&src, common::text_pdf(4)).unwrap();
    assert_eq!(Document::file_page_count(&src, None).unwrap(), 4);

    // Insert pages 2 and 4 of the source after page 1.
    let mut doc = open(2);
    assert_eq!(doc.insert_pages_selected(1, &src, None, &[1, 3]).unwrap(), 2);
    assert_eq!(
        page_texts(&doc),
        ["Page 1 Hello World", "Page 2 Hello World", "Page 4 Hello World", "Page 2 Hello World"]
    );

    // Blank pages: three at the front, then detection.
    doc.insert_blank_pages(0, 300.0, 400.0, 3).unwrap();
    assert_eq!(doc.page_count(), 7);
    assert_eq!(doc.blank_pages(&(0..7).collect::<Vec<_>>(), false, false, 10.0).unwrap(), vec![0, 1, 2]);

    // Delete two blank pages and the bookmark pointing to one of them.
    doc.append_bookmarks(&[
        NewBookmark { title: "leer".into(), page: 0, level: 0 },
        NewBookmark { title: "Text".into(), page: 3, level: 0 },
    ])
    .unwrap();
    doc.delete_pages_with_bookmarks(&[0, 1], true).unwrap();
    assert_eq!(doc.page_count(), 5);
    assert_eq!(doc.bookmarks().iter().map(|b| b.title.as_str()).collect::<Vec<_>>(), ["Text"]);
    doc.delete_pages(&[0]).unwrap();

    // Generated text pages with a bookmark.
    let (pdf, marks) = mrpdf::compose::text_to_pdf(
        &[("notiz.txt".into(), "Eingefuegter Text".into())],
        &mrpdf::compose::TextLayout { bookmark_per_file: true, ..Default::default() },
    )
    .unwrap();
    assert_eq!(doc.insert_pdf_bytes(4, pdf, &marks).unwrap(), 1);
    assert_eq!(page_texts(&doc)[4], "Eingefuegter Text");
    assert!(doc.bookmarks().iter().any(|b| b.title == "notiz" && b.page == Some(4)));

    // Duplicate pages 1-2 twice, collated and not collated.
    let mut d = open(2);
    assert_eq!(d.duplicate_pages_to(&[0, 1], 2, 2, true, true, true).unwrap(), 4);
    assert_eq!(
        page_texts(&d)[2..],
        ["Page 1 Hello World", "Page 2 Hello World", "Page 1 Hello World", "Page 2 Hello World"]
    );
    let mut d = open(2);
    d.duplicate_pages_to(&[0, 1], 2, 2, false, true, true).unwrap();
    assert_eq!(
        page_texts(&d)[2..],
        ["Page 1 Hello World", "Page 1 Hello World", "Page 2 Hello World", "Page 2 Hello World"]
    );
    assert!(d.undo().unwrap());
    assert_eq!(d.page_count(), 2, "one undo step");

    // Reverse a range only.
    let mut d = open(4);
    d.reverse_page_range(&[1, 2, 3]).unwrap();
    assert_eq!(
        page_texts(&d),
        ["Page 1 Hello World", "Page 4 Hello World", "Page 3 Hello World", "Page 2 Hello World"]
    );

    // Replace page 2 by page 3 of the source and keep its note.
    let mut d = open(3);
    let mut note = NewAnnotation::new(AnnotKind::Note, style(Rgba::RED));
    note.rect = RectF::new(50.0, 50.0, 70.0, 70.0);
    d.add_annotation(1, &note).unwrap();
    let opts =
        mrpdf::import::ReplaceOptions { keep_annotations: true, fit_target: true, scale_content: true };
    assert_eq!(d.replace_pages_from(&[1], &src, &[2], opts).unwrap(), 1);
    assert_eq!(page_texts(&d), ["Page 1 Hello World", "Page 3 Hello World", "Page 3 Hello World"]);
    assert!(d.annotations(1).unwrap().iter().any(|a| a.kind == AnnotKind::Note));
    let opts = mrpdf::import::ReplaceOptions { keep_annotations: false, ..opts };
    d.replace_pages_from(&[1], &src, &[3], opts).unwrap();
    assert!(d.annotations(1).unwrap().is_empty());
    assert_eq!(page_texts(&d)[1], "Page 4 Hello World");

    // Normalize: a rotated page loses /Rotate but looks the same.
    let mut d = open(1);
    d.rotate_pages(&[0], 90).unwrap();
    let before = d.render_page(0, 120, 85).unwrap();
    assert_eq!(d.normalize_pages(&[0]).unwrap(), 1);
    let g = d.geometry(0).unwrap();
    assert_eq!(g.rotation, Rotation::R0);
    let (w, h) = g.view_size();
    assert!(w > h, "still landscape: {w}x{h}");
    let after = d.render_page(0, 120, 85).unwrap();
    let diff = before.rgba.iter().zip(&after.rgba).filter(|(a, b)| a.abs_diff(**b) > 40).count();
    assert!(diff < before.rgba.len() / 100, "look changed: {diff}");
    assert_eq!(d.normalize_pages(&[0]).unwrap(), 0, "nothing left to do");

    // Duplicates, also when one copy is rotated.
    let mut d = open(2);
    d.duplicate_pages_to(&[0], 2, 1, true, true, true).unwrap();
    d.rotate_pages(&[2], 180).unwrap();
    assert!(d.find_duplicate_pages_in(&[0, 1, 2], false, false).unwrap().is_empty());
    assert_eq!(d.find_duplicate_pages_in(&[0, 1, 2], true, false).unwrap(), vec![vec![0, 2]]);

    // Extract with related bookmarks only, comments removed.
    let mut d = open(3);
    d.append_bookmarks(&[
        NewBookmark { title: "eins".into(), page: 0, level: 0 },
        NewBookmark { title: "drei".into(), page: 2, level: 0 },
    ])
    .unwrap();
    d.add_annotation(2, &note).unwrap();
    let opts = mrpdf::document::ExtractOptions {
        comments: mrpdf::document::Handling::Remove,
        forms: mrpdf::document::Handling::Copy,
        bookmarks: mrpdf::document::BookmarkCopy::Related,
    };
    let out = Document::from_bytes(d.extract_pages_with(&[1, 2], &opts).unwrap(), "x.pdf", None).unwrap();
    assert_eq!(out.page_count(), 2);
    let marks = out.bookmarks();
    assert_eq!(marks.len(), 1);
    assert_eq!((marks[0].title.as_str(), marks[0].page), ("drei", Some(1)));
    assert!(out.annotations(1).unwrap().is_empty());
    std::fs::remove_file(&src).ok();
}

#[test]
fn combine_split_and_overlay() {
    use mrpdf::import::{NUpLayout, OverlayOptions};
    use mrpdf::outline::NewBookmark;
    let _guard = common::lock();

    // Six pages on one A4 sheet (2 x 3), seven pages need two sheets.
    let mut doc = open(7);
    let nup = doc.nup_document(&(0..6).collect::<Vec<_>>(), &NUpLayout::default()).unwrap();
    assert_eq!(nup.page_count(), 1);
    let text = nup.page_text(0).unwrap().all_text();
    for i in 1..=6 {
        assert!(text.contains(&format!("Page {i} Hello")), "{text}");
    }
    let (w, h) = nup.geometry(0).unwrap().view_size();
    assert!((w - 595.3).abs() < 1.0 && (h - 841.9).abs() < 1.0);
    let layout = NUpLayout { columns: 2, rows: 2, landscape: true, border: true, ..NUpLayout::default() };
    let nup = doc.nup_document(&(0..7).collect::<Vec<_>>(), &layout).unwrap();
    assert_eq!(nup.page_count(), 2);
    assert!(nup.geometry(0).unwrap().view_size().0 > 800.0, "landscape sheet");

    // Parts for "Dokument aufteilen".
    assert_eq!(doc.parts_every(3), vec![vec![0, 1, 2], vec![3, 4, 5], vec![6]]);
    let single = doc.extract_pages_to_bytes(&[0]).unwrap().len();
    let parts = doc.parts_by_size(single * 2 + 10).unwrap();
    assert!(parts.len() >= 3 && parts.iter().all(|p| !p.is_empty()));
    assert_eq!(parts.concat(), (0..7).collect::<Vec<_>>());
    doc.append_bookmarks(&[
        NewBookmark { title: "A".into(), page: 2, level: 0 },
        NewBookmark { title: "A1".into(), page: 3, level: 1 },
        NewBookmark { title: "B".into(), page: 5, level: 0 },
    ])
    .unwrap();
    let parts = doc.parts_by_bookmarks(1);
    assert_eq!(
        parts,
        vec![(vec![0, 1], String::new()), (vec![2, 3, 4], "A".into()), (vec![5, 6], "B".into())]
    );
    assert_eq!(doc.parts_by_bookmarks(2).len(), 4);

    // Split pages into a left and a right half.
    let mut d = open(2);
    d.split_pages_at(&[0], &[0.5], &[], false, mrpdf::pages::SplitMethod::CropBox).unwrap();
    assert_eq!(d.page_count(), 3);
    let (w0, _) = d.geometry(0).unwrap().view_size();
    let (w1, _) = d.geometry(1).unwrap().view_size();
    assert!((w0 - 297.6).abs() < 1.0 && (w1 - 297.6).abs() < 1.0, "{w0} {w1}");
    assert!(d.page_text(2).unwrap().all_text().contains("Page 2"), "order kept");
    let reopened = Document::from_bytes(d.to_bytes().unwrap(), "s.pdf", None).unwrap();
    assert_eq!(reopened.page_count(), 3);

    // Overlay page 3 of another PDF on all pages, on a new layer.
    let src = temp_file("overlay.pdf");
    std::fs::write(&src, common::text_pdf(3)).unwrap();
    let mut d = open(2);
    let opts = OverlayOptions {
        opacity: 0.5,
        layer: Some("Overlay".into()),
        repeat: true,
        ..OverlayOptions::default()
    };
    assert_eq!(d.overlay_from(&[0, 1], &src, &[2], &opts).unwrap(), 2);
    let t = d.page_text(1).unwrap().all_text();
    assert!(t.contains("Page 2") && t.contains("Page 3"), "{t}");
    let bytes = d.to_bytes().unwrap();
    assert!(bytes.windows(12).any(|w| w == b"OCProperties"));
    // Without repeat only as many pages as source pages are changed.
    let mut d = open(3);
    let opts = OverlayOptions { repeat: false, ..OverlayOptions::default() };
    assert_eq!(d.overlay_from(&[0, 1, 2], &src, &[0, 1], &opts).unwrap(), 2);
    std::fs::remove_file(&src).ok();
}

#[test]
fn page_shape_dialog_operations() {
    use mrpdf::pages::{CropOptions, ResizeSpec, SizeMode};
    let _guard = common::lock();
    let size = |d: &Document, p: usize| d.geometry(p).unwrap().view_size();

    // White margins: the text starts 72 pt from the left.
    let mut d = open(2);
    let m = d.white_margins(0, false).unwrap();
    assert!((m[0] - 72.0).abs() < 4.0, "{m:?}");
    assert!(m[2] > 90.0 && m[2] < 140.0, "top {m:?}");
    d.crop_pages_with(&[(0, m)], CropOptions::default()).unwrap();
    let (w, h) = size(&d, 0);
    assert!(w < 595.0 - 72.0 && h < 842.0 - 90.0, "{w}x{h}");
    assert!(d.page_text(0).unwrap().all_text().contains("Page 1"));
    // Cropped areas are white margins only: trimming again changes little.
    let again = d.white_margins(0, false).unwrap();
    assert!(again.iter().all(|v| *v < 4.0), "{again:?}");

    // Cut the first text line off and remove the hidden content.
    let mut d = open(1);
    d.crop_pages_with(&[(0, [0.0, 0.0, 200.0, 0.0])], CropOptions::default()).unwrap();
    assert!(d.page_text(0).unwrap().all_text().contains("Page 1"), "still in the file");
    assert!(d.remove_outside(&[0], true, true, true).unwrap() >= 1);
    let text = d.page_text(0).unwrap().all_text();
    assert!(!text.contains("Page 1") && text.contains("Zweite"), "{text}");

    // Same size for all changed pages; fit the media box.
    let mut d = open(2);
    let opts = CropOptions { same_size: true, fit_media: true, ..CropOptions::default() };
    d.crop_pages_with(&[(0, [100.0, 100.0, 100.0, 100.0]), (1, [10.0, 10.0, 10.0, 10.0])], opts).unwrap();
    let (w0, h0) = size(&d, 0);
    let (w1, h1) = size(&d, 1);
    assert!((w0 - w1).abs() < 0.5 && (h0 - h1).abs() < 0.5, "{w0}x{h0} vs {w1}x{h1}");

    // Crop tool rectangle.
    let mut d = open(1);
    d.crop_to_rect(0, RectF::new(50.0, 500.0, 350.0, 760.0)).unwrap();
    let (w, h) = size(&d, 0);
    assert!((w - 300.0).abs() < 0.5 && (h - 260.0).abs() < 0.5, "{w}x{h}");

    // Resize: fixed A5 with scaled content, scale 50 %, grow, skip portrait.
    let mut d = open(2);
    let spec = ResizeSpec {
        mode: SizeMode::Fixed(419.53, 595.28),
        orientation: 0,
        skip: 0,
        fit: true,
        keep_aspect: true,
        anchor: (0.5, 0.5),
        offset: (0.0, 0.0),
        normalize: true,
    };
    assert_eq!(d.resize_pages_with(&[0], &spec).unwrap(), 1);
    let (w, h) = size(&d, 0);
    assert!((w - 419.5).abs() < 0.5 && (h - 595.3).abs() < 0.5);
    assert!(d.page_text(0).unwrap().all_text().contains("Page 1"));
    let spec2 = ResizeSpec { mode: SizeMode::Scale(0.5), ..spec };
    d.resize_pages_with(&[1], &spec2).unwrap();
    assert!((size(&d, 1).0 - 297.6).abs() < 0.5);
    let spec3 = ResizeSpec { mode: SizeMode::Grow(20.0, 10.0), fit: false, ..spec };
    d.resize_pages_with(&[1], &spec3).unwrap();
    assert!((size(&d, 1).0 - 317.6).abs() < 0.5 && (size(&d, 1).1 - 431.0).abs() < 0.5);
    let skip = ResizeSpec { skip: 1, ..spec3 };
    assert_eq!(d.resize_pages_with(&[0, 1], &skip).unwrap(), 0, "both pages are portrait");
    let preview = d.resize_preview(0, &spec2).unwrap();
    assert!((preview.geometry(0).unwrap().view_size().0 - 209.8).abs() < 0.5);

    // Margins: 20 pt everywhere, mirrored on page 2.
    let mut d = open(2);
    d.add_page_margins(&[(0, [20.0, 20.0, 20.0, 20.0]), (1, [10.0, 30.0, 0.0, 0.0])]).unwrap();
    assert!((size(&d, 0).0 - 635.3).abs() < 0.5 && (size(&d, 0).1 - 881.9).abs() < 0.5);
    assert!((size(&d, 1).0 - 635.3).abs() < 0.5);
}

/// A one-page PDF with body text and a watermark artifact as other
/// programs write it.
fn foreign_watermark_pdf() -> Vec<u8> {
    use lopdf::{Document as Lo, Object, Stream, dictionary};
    let mut doc = Lo::with_version("1.7");
    let pages_id = doc.new_object_id();
    let font =
        doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" });
    let content = b"BT /F1 24 Tf 72 700 Td (Body text) Tj ET\n/Artifact <</Type /Pagination /Subtype /Watermark>> BDC\nBT /F1 40 Tf 100 400 Td (FREMD) Tj ET\nEMC\n".to_vec();
    let content_id = doc.add_object(Stream::new(lopdf::Dictionary::new(), content));
    let page = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages_id, "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        "Contents" => content_id, "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    let mut out = Vec::new();
    doc.save_to(&mut out).unwrap();
    out
}

#[test]
fn managed_page_marks() {
    use mrpdf::compose::Align;
    use mrpdf::marks::{BatesFormat, FontSpec, ImageData, Kind, MarkDef, Source};
    let _guard = common::lock();
    let mut d = open(2);
    let text = |d: &Document, p: usize| d.page_text(p).unwrap().all_text();

    let watermark = MarkDef {
        kind: Kind::Watermark,
        source: Source::Text {
            text: "ENTWURF".into(),
            font: FontSpec { size: 60.0, ..FontSpec::default() },
            align: Align::Center,
        },
        rotation: 45.0,
        opacity: 0.3,
        pages: vec![0, 1],
        ..MarkDef::default()
    };
    let id = d.add_mark(&watermark, None).unwrap();
    assert!(text(&d, 1).contains("ENTWURF"));
    let header = MarkDef {
        kind: Kind::HeaderFooter,
        source: Source::HeaderFooter {
            texts: [
                "%[FileName]".into(),
                String::new(),
                String::new(),
                String::new(),
                "Seite %[Page] von %[Pages]".into(),
                String::new(),
            ],
            font: FontSpec::default(),
            margins: [36.0; 4],
            shrink: true,
        },
        pages: vec![0, 1],
        ..MarkDef::default()
    };
    let header_id = d.add_mark(&header, None).unwrap();
    assert!(text(&d, 1).contains("Seite 2 von 2"), "{}", text(&d, 1));
    let bates = MarkDef {
        kind: Kind::Bates,
        source: Source::HeaderFooter {
            texts: [
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                "%[Bates]".into(),
            ],
            font: FontSpec::default(),
            margins: [20.0; 4],
            shrink: false,
        },
        bates: BatesFormat { prefix: "AB".into(), start: 7, digits: 4, suffix: String::new() },
        pages: vec![0, 1],
        ..MarkDef::default()
    };
    d.add_mark(&bates, None).unwrap();
    assert!(text(&d, 1).contains("AB0008"));

    // The marks are listed and survive saving.
    let reopened = Document::from_bytes(d.to_bytes().unwrap(), "m.pdf", None).unwrap();
    let mut reopened = reopened;
    let marks = reopened.marks().unwrap();
    assert_eq!(marks.len(), 3);
    assert_eq!(marks[0].id, id);
    assert_eq!(marks[0].def, watermark);

    // Edit the header in one undo step.
    let mut changed = header.clone();
    if let Source::HeaderFooter { texts, .. } = &mut changed.source {
        texts[4] = "S. %[Page]".into();
    }
    d.update_mark(&header_id, &changed, None).unwrap();
    assert!(text(&d, 1).contains("S. 2") && !text(&d, 1).contains("Seite 2 von 2"));
    assert!(d.undo().unwrap());
    assert!(text(&d, 1).contains("Seite 2 von 2"));

    // Removing the watermarks leaves the other marks.
    assert_eq!(d.remove_marks(Kind::Watermark, None, false).unwrap(), 2);
    assert!(!text(&d, 0).contains("ENTWURF"));
    assert!(text(&d, 0).contains("Seite 1 von 2"));
    assert_eq!(d.marks().unwrap().len(), 2);

    // Image watermark with preview.
    let img = mrpdf::pdf_edit::RgbaImage { width: 20, height: 10, rgba: [255, 0, 0, 255].repeat(200) };
    let pic = MarkDef {
        source: Source::Image { name: "rot.png".into() },
        pages: vec![0],
        scale: 0.5,
        ..MarkDef::default()
    };
    let preview = d.mark_preview(0, &pic, Some(ImageData::Pixels(img.clone()))).unwrap();
    let rendered = preview.render_page(0, 100, 141).unwrap();
    assert!(rendered.rgba.as_chunks::<4>().0.iter().any(|p| p[0] > 200 && p[1] < 60), "red image visible");
    d.add_mark(&pic, Some(ImageData::Pixels(img))).unwrap();
    let pic_id = d.marks().unwrap().last().unwrap().id.clone();
    let moved = MarkDef { h_from: Align::Start, ..pic.clone() };
    d.update_mark(&pic_id, &moved, None).unwrap();

    // Watermarks of other programs are removed with "Alle entfernen".
    let mut f = Document::from_bytes(foreign_watermark_pdf(), "f.pdf", None).unwrap();
    assert!(text(&f, 0).contains("FREMD"));
    assert_eq!(f.remove_marks(Kind::Watermark, None, true).unwrap(), 1);
    let t = text(&f, 0);
    assert!(!t.contains("FREMD") && t.contains("Body text"), "{t}");
}

#[test]
fn page_label_ranges() {
    use mrpdf::pages::{LabelRange, LabelStyle};
    let _guard = common::lock();
    let mut d = open(6);
    assert_eq!(d.page_label_ranges().unwrap().len(), 1);
    let ranges = vec![
        LabelRange { start: 0, style: LabelStyle::RomanLower, prefix: String::new(), first: 1 },
        LabelRange { start: 2, style: LabelStyle::LettersUpper, prefix: "A-".into(), first: 26 },
        LabelRange { start: 4, style: LabelStyle::Decimal, prefix: String::new(), first: 1 },
    ];
    d.set_page_label_ranges(&ranges).unwrap();
    assert_eq!(d.page_label_ranges().unwrap(), ranges);
    assert_eq!(d.page_label(1).as_deref(), Some("ii"));
    assert_eq!(d.page_label(3).as_deref(), Some("A-AA"));
    assert_eq!(d.page_label(5).as_deref(), Some("2"));
    assert_eq!(LabelStyle::RomanUpper.format(1994), "MCMXCIV");
    d.set_page_label_ranges(&[LabelRange {
        start: 0,
        style: LabelStyle::Decimal,
        prefix: String::new(),
        first: 1,
    }])
    .unwrap();
    assert_eq!(d.page_label_ranges().unwrap().len(), 1);
}
