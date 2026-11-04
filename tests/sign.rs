//! Digital signatures: signing, checking, tampering, later changes and the
//! independent check with Poppler's `pdfsig` (when installed).

mod common;

use std::process::Command;

use lopdf::{Dictionary, IncrementalDocument, Object};
use mrpdf::document::Document;
use mrpdf::geometry::RectF;
use mrpdf::sign::cert::NewId;
use mrpdf::sign::cms::Check;
use mrpdf::sign::id::DigitalId;
use mrpdf::sign::pdf::{self, Place, SignOptions};

const NOW: i64 = 1_793_808_000; // 4.11.2026

fn id() -> DigitalId {
    DigitalId::create(&NewId {
        name: "Erika Mustermann",
        email: "erika@example.org",
        organisation: "Test GmbH",
        years: 3,
    })
    .unwrap()
}

fn opts(place: Place) -> SignOptions {
    SignOptions { place, reason: "Freigabe".into(), location: "Köln".into(), contact: String::new() }
}

fn visible() -> Place {
    Place::Rect { page: 0, rect: RectF::new(350.0, 60.0, 550.0, 120.0) }
}

/// Runs `pdfsig` on `bytes`; None when it is not installed.
fn pdfsig(bytes: &[u8], name: &str) -> Option<String> {
    let path = std::env::temp_dir().join(format!("mrpdf-sign-{}-{name}.pdf", std::process::id()));
    std::fs::write(&path, bytes).unwrap();
    let out = Command::new("pdfsig").arg(&path).output().ok();
    let _ = std::fs::remove_file(&path);
    out.map(|o| String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr))
}

fn opens(bytes: Vec<u8>) {
    let _g = common::lock();
    let doc = Document::from_bytes(bytes, "signiert.pdf", None).expect("signed file opens");
    assert!(doc.page_count() > 0);
}

#[test]
fn sign_visible_and_check() {
    let src = {
        let _g = common::lock();
        common::text_pdf(2)
    };
    let id = id();
    let signed = pdf::sign(&src, &id, &opts(visible()), NOW, 60).unwrap();
    // The original bytes stay untouched in front (incremental update).
    assert_eq!(&signed[..src.len()], &src[..]);
    let sigs = pdf::signatures(&signed);
    assert_eq!(sigs.len(), 1);
    let s = &sigs[0];
    assert_eq!(s.check, Check::Valid, "{s:?}");
    assert!(s.valid() && s.whole_file && s.self_signed);
    assert_eq!(s.signer, "Erika Mustermann");
    assert_eq!(s.reason, "Freigabe");
    assert_eq!(s.location, "Köln");
    assert_eq!(s.field, "Signatur1");
    assert_eq!(s.time, "04.11.2026 17:00:00");
    assert_eq!(s.sub_filter, "ETSI.CAdES.detached");
    // For looking at the appearance.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/ui-screenshots");
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join("signiert-beispiel.pdf"), &signed);
    if let Some(out) = pdfsig(&signed, "visible") {
        assert!(out.contains("Signature is Valid"), "pdfsig:\n{out}");
        assert!(out.contains("Erika Mustermann"), "pdfsig:\n{out}");
        assert!(out.contains("ETSI.CAdES.detached"), "pdfsig:\n{out}");
    }
    opens(signed);
}

#[test]
fn invisible_signature_on_form_and_second_signature() {
    let src = common::form_pdf();
    let first = pdf::sign(&src, &id(), &opts(Place::Invisible), NOW, 0).unwrap();
    let other =
        DigitalId::create(&NewId { name: "Max Muster", email: "", organisation: "", years: 1 }).unwrap();
    let both = pdf::sign(&first, &other, &opts(visible()), NOW + 60, 0).unwrap();
    let sigs = pdf::signatures(&both);
    assert_eq!(sigs.len(), 2, "{sigs:?}");
    assert!(sigs.iter().all(|s| s.valid()), "{sigs:?}");
    // The first covers only its revision, the second the whole file.
    assert!(!sigs[0].whole_file && sigs[1].whole_file);
    assert_eq!(sigs[0].signer, "Erika Mustermann");
    assert_eq!(sigs[1].signer, "Max Muster");
    assert_eq!(sigs[1].field, "Signatur2");
    // The form fields are still there, plus the two signatures.
    let fields = |bytes: &[u8]| {
        let doc = lopdf::Document::load_mem(bytes).unwrap();
        let catalog = doc.catalog().unwrap();
        let form = match catalog.get(b"AcroForm").unwrap() {
            Object::Reference(r) => doc.get_dictionary(*r).unwrap().clone(),
            o => o.as_dict().unwrap().clone(),
        };
        let n = doc.dereference(form.get(b"Fields").unwrap()).unwrap().1.as_array().unwrap().len();
        (n, form.get(b"SigFlags").and_then(Object::as_i64).ok())
    };
    assert_eq!(fields(&both), (fields(&src).0 + 2, Some(3)));
    if let Some(out) = pdfsig(&both, "two") {
        assert_eq!(out.matches("Signature is Valid").count(), 2, "pdfsig:\n{out}");
    }
    opens(both);
}

#[test]
fn tampering_and_later_changes_show() {
    let src = {
        let _g = common::lock();
        common::text_pdf(1)
    };
    let signed = pdf::sign(&src, &id(), &opts(visible()), NOW, 0).unwrap();
    // Change one byte in the signed part (the version in the header).
    let mut bad = signed.clone();
    assert!(bad.starts_with(b"%PDF-1."));
    bad[7] = if bad[7] == b'4' { b'5' } else { b'4' };
    assert_eq!(pdf::signatures(&bad)[0].check, Check::DigestMismatch);
    if let Some(out) = pdfsig(&bad, "bad") {
        assert!(!out.contains("Signature is Valid"), "pdfsig:\n{out}");
    }
    // An incremental change after signing: still valid, but not the whole file.
    let prev = lopdf::Document::load_mem(&signed).unwrap();
    let mut inc = IncrementalDocument::create_from(signed.clone(), prev);
    let mut info = Dictionary::new();
    info.set("Title", Object::string_literal("geändert"));
    let info_id = inc.new_document.add_object(Object::Dictionary(info));
    inc.new_document.trailer.set("Info", Object::Reference(info_id));
    let mut later = Vec::new();
    inc.save_to(&mut later).unwrap();
    let s = &pdf::signatures(&later)[0];
    assert!(s.valid() && !s.whole_file, "{s:?}");
    assert_eq!(s.revision_end, signed.len());
}

#[test]
fn existing_empty_field_gets_signed() {
    let src = {
        let _g = common::lock();
        common::text_pdf(1)
    };
    let mut doc = lopdf::Document::load_mem(&src).unwrap();
    let page = *doc.get_pages().get(&1).unwrap();
    let mut w = Dictionary::new();
    w.set("Type", Object::Name(b"Annot".to_vec()));
    w.set("Subtype", Object::Name(b"Widget".to_vec()));
    w.set("FT", Object::Name(b"Sig".to_vec()));
    w.set("T", Object::string_literal("Unterschrift Kunde"));
    w.set("F", 4);
    w.set("P", Object::Reference(page));
    w.set("Rect", Object::Array(vec![100.into(), 100.into(), 300.into(), 150.into()]));
    let wid = doc.add_object(Object::Dictionary(w));
    doc.get_dictionary_mut(page).unwrap().set("Annots", Object::Array(vec![Object::Reference(wid)]));
    let mut form = Dictionary::new();
    form.set("Fields", Object::Array(vec![Object::Reference(wid)]));
    let form_id = doc.add_object(Object::Dictionary(form));
    doc.catalog_mut().unwrap().set("AcroForm", Object::Reference(form_id));
    let mut with_field = Vec::new();
    doc.save_to(&mut with_field).unwrap();

    assert_eq!(pdf::empty_signature_fields(&with_field), vec!["Unterschrift Kunde".to_string()]);
    let signed =
        pdf::sign(&with_field, &id(), &opts(Place::Field("Unterschrift Kunde".into())), NOW, 0).unwrap();
    assert!(pdf::empty_signature_fields(&signed).is_empty());
    let sigs = pdf::signatures(&signed);
    assert_eq!(sigs.len(), 1);
    assert!(sigs[0].valid());
    assert_eq!(sigs[0].field, "Unterschrift Kunde");
    // The field got an appearance.
    let d = lopdf::Document::load_mem(&signed).unwrap();
    assert!(d.get_dictionary(wid).unwrap().has(b"AP"));
    // Unknown field names are refused.
    assert!(pdf::sign(&with_field, &id(), &opts(Place::Field("gibt es nicht".into())), NOW, 0).is_err());
    if let Some(out) = pdfsig(&signed, "field") {
        assert!(out.contains("Signature is Valid"), "pdfsig:\n{out}");
    }
}

#[test]
fn encrypted_documents_are_refused() {
    let src = {
        let _g = common::lock();
        common::encrypted_pdf("", "eigner", lopdf::Permissions::all())
    };
    let err = pdf::sign(&src, &id(), &opts(Place::Invisible), NOW, 0).expect_err("refused");
    assert!(err.to_string().contains("Verschlüsselte"), "{err}");
}

/// An RSA ID made by OpenSSL (as from a certificate authority).
#[test]
fn rsa_pfx_from_openssl() {
    let dir = std::env::temp_dir().join(format!("mrpdf-rsa-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let run = |args: &[&str]| {
        Command::new("openssl").args(args).current_dir(&dir).output().is_ok_and(|o| o.status.success())
    };
    if !run(&[
        "req",
        "-x509",
        "-newkey",
        "rsa:2048",
        "-nodes",
        "-keyout",
        "k.pem",
        "-out",
        "c.pem",
        "-days",
        "30",
        "-subj",
        "/CN=RSA Pruefer/O=Test",
    ]) {
        eprintln!("openssl fehlt – Test übersprungen");
        return;
    }
    assert!(run(&[
        "pkcs12",
        "-export",
        "-inkey",
        "k.pem",
        "-in",
        "c.pem",
        "-out",
        "id.pfx",
        "-passout",
        "pass:Kennwort1"
    ]));
    let pfx = std::fs::read(dir.join("id.pfx")).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(DigitalId::from_pfx(&pfx, "falsch").is_err());
    let id = DigitalId::from_pfx(&pfx, "Kennwort1").unwrap();
    assert_eq!(id.cert.name, "RSA Pruefer");
    let src = {
        let _g = common::lock();
        common::text_pdf(1)
    };
    let signed = pdf::sign(&src, &id, &opts(visible()), NOW, 0).unwrap();
    let s = &pdf::signatures(&signed)[0];
    assert!(s.valid(), "{s:?}");
    if let Some(out) = pdfsig(&signed, "rsa") {
        assert!(out.contains("Signature is Valid"), "pdfsig:\n{out}");
    }
}
