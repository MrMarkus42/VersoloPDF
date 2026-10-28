//! The spell checker with the German Hunspell dictionary, if installed
//! (skipped otherwise; MrPDF ships no dictionary).

use std::path::Path;

#[test]
fn german_dictionary() {
    let dic = Path::new("/usr/share/hunspell/de_DE.dic");
    if !dic.is_file() {
        eprintln!("no German dictionary installed, skipped");
        return;
    }
    let d = mrpdf::spell::Dictionary::load(dic).unwrap();
    assert!(d.len() > 50_000);
    for ok in [
        "Haus",
        "Häuser",
        "Häusern",
        "Haustür",
        "Rechtschreibprüfung",
        "Lebensmittel",
        "Versicherung",
        "Wohnen",
        "gelesen",
        "Oktober",
        "Beispielseite",
        "unklar",
        "Grüße",
        "zum",
        "Testen",
        "der",
        "Umwandlung",
    ] {
        assert!(d.check(ok), "{ok} should be right");
    }
    for wrong in ["Hasu", "Rechtschreibprüfnug", "Versicherrung", "Okotber", "gelessen"] {
        assert!(!d.check(wrong), "{wrong} should be wrong");
    }
    let s = d.suggest("Okotber", 5);
    assert!(s.contains(&"Oktober".to_string()), "{s:?}");
}
