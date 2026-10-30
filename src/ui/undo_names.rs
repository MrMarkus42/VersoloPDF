//! Readable names of the steps in the undo history ("Rückgängig: …").
//!
//! The document core labels each change with a short English phrase; the
//! table turns it into words for the user.

use crate::ui::i18n::Language;

const NAMES: &[(&str, &str)] = &[
    ("add bookmark", "Lesezeichen hinzufügen"),
    ("add bookmarks", "Lesezeichen hinzufügen"),
    ("add form field", "Formularfeld hinzufügen"),
    ("add image", "Bild hinzufügen"),
    ("add links", "Verweise hinzufügen"),
    ("add margins", "Ränder hinzufügen"),
    ("add media", "Medien hinzufügen"),
    ("add sound", "Audio hinzufügen"),
    ("apply redactions", "Schwärzungen anwenden"),
    ("attach file", "Datei anhängen"),
    ("bookmark zoom", "Zoom der Lesezeichen"),
    ("change stacking order", "Reihenfolge der Objekte ändern"),
    ("contents to images", "Inhalt in Bilder umwandeln"),
    ("convert to grayscale", "In Graustufen umwandeln"),
    ("create named destinations", "Benannte Ziele erstellen"),
    ("create web links", "Weblinks erstellen"),
    ("crop pages", "Seiten zuschneiden"),
    ("delete all comments", "Alle Anmerkungen löschen"),
    ("delete annotation", "Anmerkung löschen"),
    ("delete annotations", "Anmerkungen löschen"),
    ("delete attachment", "Anhang löschen"),
    ("delete bookmark", "Lesezeichen löschen"),
    ("delete bookmarks", "Lesezeichen löschen"),
    ("delete objects", "Objekte löschen"),
    ("delete pages", "Seiten löschen"),
    ("detect form fields", "Formularfelder erkennen"),
    ("document properties", "Dokumenteigenschaften"),
    ("duplicate pages", "Seiten duplizieren"),
    ("edit annotation", "Anmerkung bearbeiten"),
    ("edit bookmarks", "Lesezeichen bearbeiten"),
    ("edit text", "Text bearbeiten"),
    ("fill form", "Formular ausfüllen"),
    ("flatten", "Reduzieren"),
    ("format objects", "Objekte formatieren"),
    ("import comments", "Anmerkungen importieren"),
    ("import form data", "Formulardaten importieren"),
    ("insert page", "Seite einfügen"),
    ("insert pages", "Seiten einfügen"),
    ("insert table of contents", "Inhaltsverzeichnis einfügen"),
    ("links from bookmarks", "Verweise aus Lesezeichen"),
    ("mark for redaction", "Zum Schwärzen markieren"),
    ("merge duplicate bookmarks", "Doppelte Lesezeichen zusammenführen"),
    ("move objects", "Objekte verschieben"),
    ("move pages", "Seiten verschieben"),
    ("normalize pages", "Seiten vereinheitlichen"),
    ("overlay pages", "Seiten überlagern"),
    ("page labels", "Seitenbeschriftungen"),
    ("page margins", "Seitenränder"),
    ("page marks", "Seitenmarkierungen"),
    ("page transitions", "Seitenübergänge"),
    ("paste objects", "Objekte einfügen"),
    ("portfolio", "Portfolio"),
    ("recognize text", "Text erkennen"),
    ("recolor pages", "Seiten umfärben"),
    ("remove bookmark actions", "Aktionen der Lesezeichen entfernen"),
    ("remove cropped content", "Abgeschnittenen Inhalt entfernen"),
    ("remove duplicate bookmarks", "Doppelte Lesezeichen entfernen"),
    ("remove page marks", "Seitenmarkierungen entfernen"),
    ("remove web links", "Weblinks entfernen"),
    ("rename bookmark", "Lesezeichen umbenennen"),
    ("replace fonts", "Schriftarten ersetzen"),
    ("replace image", "Bild ersetzen"),
    ("replace pages", "Seiten ersetzen"),
    ("reset form", "Formular zurücksetzen"),
    ("resize pages", "Seitengröße ändern"),
    ("resolve named destinations", "Benannte Ziele auflösen"),
    ("reverse pages", "Seitenreihenfolge umkehren"),
    ("rotate pages", "Seiten drehen"),
    ("sort bookmarks", "Lesezeichen sortieren"),
    ("sort pages by bookmarks", "Seiten nach Lesezeichen sortieren"),
    ("split pages", "Seiten aufteilen"),
    ("swap pages", "Seiten tauschen"),
];

/// The name of an undo step in the user's language.
pub fn undo_name(label: &str, lang: Language) -> String {
    if lang == Language::English {
        let mut chars = label.chars();
        return chars.next().map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect());
    }
    if let Some((_, de)) = NAMES.iter().find(|(en, _)| *en == label) {
        return (*de).to_string();
    }
    // "add Highlight", "add FreeText", … (new comments).
    if label.starts_with("add ") {
        return "Anmerkung hinzufügen".to_string();
    }
    "Letzte Änderung".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(undo_name("delete pages", Language::German), "Seiten löschen");
        assert_eq!(undo_name("add Highlight", Language::German), "Anmerkung hinzufügen");
        assert_eq!(undo_name("something new", Language::German), "Letzte Änderung");
        assert_eq!(undo_name("delete pages", Language::English), "Delete pages");
    }
}
