//! Tests of the macOS system functions. They only run on a Mac (GitHub
//! Actions, `.github/workflows/macos.yml`).
//!
//! The keychain test creates its own keychain, puts it into the user's
//! search list for the test and restores the list afterwards; it is
//! `#[ignore]`d so a developer's Mac is only changed on request:
//! `cargo test --test macos -- --ignored`.
#![cfg(target_os = "macos")]

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use versolopdf::sign::cert::{self, NewId};
use versolopdf::sign::id::DigitalId;
use versolopdf::sign::pdf::{self, Place, SignOptions};

#[test]
fn vision_recognises_rendered_text() {
    let _guard = common::lock();
    let bytes = common::lines_pdf(&[&["Rechnung Nummer 4711", "Versand nach Hamburg"]], None);
    let mut doc = versolopdf::Document::from_bytes(bytes, "ocr.pdf", None).unwrap();
    let exe = versolopdf::ocr::find_tesseract(None).unwrap();
    assert!(versolopdf::ocr::is_vision(&exe), "macOS uses its own text recognition");
    let languages = versolopdf::ocr::languages(&exe);
    assert!(languages.iter().any(|l| l == "deu") && languages.iter().any(|l| l == "eng"), "{languages:?}");
    let engine = versolopdf::ocr::Engine { exe, languages: "deu+eng".into() };
    let words = doc.ocr_words(0, &engine, 200.0).unwrap();
    let text: Vec<&str> = words.iter().map(|(t, _)| t.as_str()).collect();
    for expected in ["Rechnung", "4711", "Hamburg"] {
        assert!(text.contains(&expected), "{expected} not in {text:?}");
    }
    // The boxes lie where the text was drawn (72 pt from the left, top lines).
    let (_, r) = words.iter().find(|(t, _)| t == "Rechnung").unwrap();
    let r = r.normalized();
    assert!((60.0..90.0).contains(&r.x0) && r.y0 > 700.0 && r.y1 < 790.0, "{r:?}");
    // "Text auf Seiten erkennen" makes the page searchable.
    let found = doc.recognize_text(&[0], &engine, 200.0, false).unwrap();
    assert!(found >= 6, "{found}");
}

fn run(cmd: &str, args: &[&str]) -> String {
    let out = Command::new(cmd).args(args).output().unwrap();
    assert!(out.status.success(), "{cmd} {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Puts the user's keychain search list back when dropped.
struct SearchList(Vec<String>, PathBuf);

impl Drop for SearchList {
    fn drop(&mut self) {
        let mut args = vec!["list-keychains", "-d", "user", "-s"];
        args.extend(self.0.iter().map(String::as_str));
        let _ = Command::new("security").args(&args).status();
        let _ = Command::new("security").arg("delete-keychain").arg(&self.1).status();
    }
}

fn add_keychain(path: &Path) -> SearchList {
    let p = path.to_str().unwrap();
    run("security", &["create-keychain", "-p", "versolo-test", p]);
    run("security", &["set-keychain-settings", p]);
    run("security", &["unlock-keychain", "-p", "versolo-test", p]);
    let old: Vec<String> = run("security", &["list-keychains", "-d", "user"])
        .lines()
        .map(|l| l.trim().trim_matches('"').to_string())
        .filter(|l| !l.is_empty())
        .collect();
    let guard = SearchList(old.clone(), path.to_path_buf());
    let mut args = vec!["list-keychains", "-d", "user", "-s", p];
    args.extend(old.iter().map(String::as_str));
    run("security", &args);
    guard
}

#[test]
#[ignore = "changes the keychain search list for a moment; run with --ignored (CI)"]
fn keychain_id_signs_a_pdf() {
    let dir = tempfile::tempdir().unwrap();
    let id = DigitalId::create(&NewId { name: "Schlüsselbund Test", email: "", organisation: "", years: 1 })
        .unwrap();
    let pfx = dir.path().join("id.p12");
    std::fs::write(&pfx, id.to_pfx("p12-test").unwrap()).unwrap();
    let keychain = dir.path().join("versolo-test.keychain-db");
    let _list = add_keychain(&keychain);
    let kc = keychain.to_str().unwrap();
    // -A: every program may use the key without asking (only in this test keychain).
    run("security", &["import", pfx.to_str().unwrap(), "-k", kc, "-P", "p12-test", "-A"]);
    run(
        "security",
        &["set-key-partition-list", "-S", "apple-tool:,apple:,unsigned:", "-s", "-k", "versolo-test", kc],
    );

    let ids = versolopdf::sign::id::system_ids();
    let (name, der) = ids.iter().find(|(_, der)| *der == id.cert.der).expect("the ID is listed");
    assert!(name.contains("Schlüsselbund Test"), "{name}");
    let system = DigitalId::from_system(der).unwrap();

    // The keychain signs; the signature fits the certificate.
    let digest = cert::digest(cert::SHA256, &[b"VersoloPDF"]).unwrap();
    let (sig, _) = system.key.sign_sha256(&digest).unwrap();
    assert!(cert::verify(&system.cert, cert::SHA256, &digest, &sig).unwrap());

    // A whole PDF signature.
    let src = {
        let _guard = common::lock();
        common::text_pdf(1)
    };
    let opts = SignOptions { place: Place::Invisible, reason: "Test".into(), ..SignOptions::default() };
    let signed = pdf::sign(&src, &system, &opts, 1_793_000_000, 60).unwrap();
    let sigs = pdf::signatures(&signed);
    assert_eq!(sigs.len(), 1);
    assert!(sigs[0].valid(), "{:?}", sigs[0]);
}

#[test]
fn open_files_start_empty() {
    assert!(versolopdf::macos::take_opened_files().is_empty());
}
