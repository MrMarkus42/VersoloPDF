//! Digital signatures: signing, checking, tampering, later changes and the
//! independent check with Poppler's `pdfsig` (when installed).

mod common;

use std::process::Command;

use lopdf::{Dictionary, IncrementalDocument, Object};
use versolopdf::document::Document;
use versolopdf::geometry::RectF;
use versolopdf::sign::cert::NewId;
use versolopdf::sign::cms::Check;
use versolopdf::sign::id::DigitalId;
use versolopdf::sign::pdf::{self, Place, SignOptions};

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
    SignOptions { place, reason: "Freigabe".into(), location: "Köln".into(), ..SignOptions::default() }
}

/// A time stamp authority made with a self-made ID (no network).
fn fake_stamp(hash: &[u8]) -> versolopdf::Result<Vec<u8>> {
    use versolopdf::sign::{cert, cms, der};
    let tsa =
        DigitalId::create(&NewId { name: "Test-Zeitstempeldienst", email: "", organisation: "", years: 1 })?;
    let imprint = der::seq(&[&der::alg(cert::SHA256, true), &der::octets(hash)]);
    let info = der::seq(&[
        &der::small_int(1),
        &der::oid("1.2.3.4.1"),
        &imprint,
        &der::small_int(7),
        &der::tlv(der::GENERALIZED_TIME, b"20261105101500Z"),
    ]);
    cms::encapsulated(&tsa, cms::TST_INFO, &info)
}

#[test]
fn certification_and_time_stamps() {
    let src = {
        let _g = common::lock();
        common::text_pdf(1)
    };
    // Certify: DocMDP level in the signature and the catalogue.
    let certified =
        pdf::sign(&src, &id(), &SignOptions { certify: Some(2), ..opts(visible()) }, NOW, 0).unwrap();
    let sigs = pdf::signatures(&certified);
    assert_eq!(sigs.len(), 1);
    assert!(sigs[0].valid() && sigs[0].certified == Some(2), "{sigs:?}");
    let doc = lopdf::Document::load_mem(&certified).unwrap();
    assert!(doc.catalog().unwrap().get(b"Perms").is_ok(), "/Perms /DocMDP");
    // Only as the first signature.
    let again =
        pdf::sign(&certified, &id(), &SignOptions { certify: Some(1), ..opts(Place::Invisible) }, NOW, 0);
    assert!(again.is_err());
    if let Some(out) = pdfsig(&certified, "certified") {
        assert!(out.contains("Signature is Valid"), "pdfsig:\n{out}");
    }

    // A signature with a time stamp of its signature value.
    let stamped = pdf::sign_with_stamper(&src, &id(), &opts(Place::Invisible), &fake_stamp, NOW).unwrap();
    let s = &pdf::signatures(&stamped)[0];
    assert!(s.valid(), "{s:?}");
    let t = s.time_stamp.as_ref().expect("time stamp read back");
    assert_eq!(t.check, Check::Valid);
    assert_eq!(t.time, "05.11.2026 10:15:00 UTC");
    assert_eq!(t.authority, "Test-Zeitstempeldienst");
    if let Some(out) = pdfsig(&stamped, "stamped") {
        assert!(out.contains("Signature is Valid"), "pdfsig:\n{out}");
    }

    // A document time stamp after the signature.
    let doc_stamped = pdf::time_stamp_document_with(&stamped, &fake_stamp).unwrap();
    let sigs = pdf::signatures(&doc_stamped);
    assert_eq!(sigs.len(), 2, "{sigs:?}");
    let ts = &sigs[1];
    assert!(ts.doc_time_stamp && ts.valid() && ts.whole_file, "{ts:?}");
    assert_eq!(ts.sub_filter, "ETSI.RFC3161");
    assert_eq!(ts.time, "05.11.2026 10:15:00 UTC");
    assert_eq!(ts.field, "Zeitstempel1");
    assert!(sigs[0].valid() && !sigs[0].whole_file);
    // A changed byte in the stamped part is found.
    let mut bad = doc_stamped.clone();
    bad[7] = if bad[7] == b'4' { b'5' } else { b'4' };
    assert_eq!(pdf::signatures(&bad)[1].check, Check::DigestMismatch);
    opens(doc_stamped);
}

fn visible() -> Place {
    Place::Rect { page: 0, rect: RectF::new(350.0, 60.0, 550.0, 120.0) }
}

/// Runs `pdfsig` on `bytes`; None when it is not installed.
fn pdfsig(bytes: &[u8], name: &str) -> Option<String> {
    let path = std::env::temp_dir().join(format!("versolopdf-sign-{}-{name}.pdf", std::process::id()));
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

/// A real time stamp authority over the network (run by hand:
/// `cargo test --test sign real_time_stamp -- --ignored`).
#[test]
#[ignore = "needs the internet"]
fn real_time_stamp_authority() {
    let src = {
        let _g = common::lock();
        common::text_pdf(1)
    };
    let url = versolopdf::sign::tsa::DEFAULT_URL;
    let stamped =
        pdf::sign(&src, &id(), &SignOptions { time_stamp: Some(url.into()), ..opts(visible()) }, NOW, 0)
            .unwrap();
    let s = &pdf::signatures(&stamped)[0];
    let t = s.time_stamp.as_ref().expect("time stamp");
    assert!(s.valid() && t.check == Check::Valid, "{s:?}");
    eprintln!("Zeitstempel: {} von {}", t.time, t.authority);
    let doc = pdf::time_stamp_document(&stamped, url).unwrap();
    let ts = &pdf::signatures(&doc)[1];
    assert!(ts.doc_time_stamp && ts.valid(), "{ts:?}");
    if let Some(out) = pdfsig(&doc, "real-tsa") {
        eprintln!("{out}");
        // Older pdfsig versions do not check document time stamps (ETSI.RFC3161).
        assert!(out.matches("Signature is Valid").count() >= 1, "pdfsig:\n{out}");
    }
}

/// An RSA ID made by OpenSSL (as from a certificate authority).
#[test]
fn rsa_pfx_from_openssl() {
    let dir = std::env::temp_dir().join(format!("versolopdf-rsa-{}", std::process::id()));
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
