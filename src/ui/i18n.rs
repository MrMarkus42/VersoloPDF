//! User interface texts (German and English).

use serde::{Deserialize, Serialize};

use crate::model::StampPreset;
use crate::ui::tools::Tool;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Language {
    German,
    English,
}

impl Language {
    pub const ALL: [Language; 2] = [Language::German, Language::English];

    pub fn name(self) -> &'static str {
        match self {
            Language::German => "Deutsch",
            Language::English => "English",
        }
    }

    pub fn strings(self) -> &'static Strings {
        match self {
            Language::German => &DE,
            Language::English => &EN,
        }
    }

    /// Picks the language from the environment (German unless an English
    /// or other non-German locale is configured).
    pub fn detect() -> Self {
        for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
            if let Ok(value) = std::env::var(var)
                && !value.is_empty()
            {
                return if value.to_ascii_lowercase().starts_with("de") {
                    Language::German
                } else {
                    Language::English
                };
            }
        }
        Language::German
    }
}

macro_rules! strings {
    ($($key:ident: $de:expr, $en:expr;)*) => {
        /// All translatable texts.
        pub struct Strings {
            $(pub $key: &'static str,)*
        }

        pub static DE: Strings = Strings { $($key: $de,)* };
        pub static EN: Strings = Strings { $($key: $en,)* };
    };
}

strings! {
    // Files
    menu_file: "Datei", "File";
    new_document: "Neues Dokument", "New document";
    open_file: "Öffnen", "Open";
    open: "Öffnen", "Open";
    recent_files: "Zuletzt verwendet", "Recent files";
    none: "(keine)", "(none)";
    save: "Speichern", "Save";
    save_as: "Speichern unter", "Save as";
    print: "Drucken", "Print";
    export_image: "Als Bild exportieren", "Export as image";
    export: "Exportieren", "Export";
    export_done: "Export abgeschlossen", "Export finished";
    doc_properties: "Dokumenteigenschaften", "Document properties";
    settings: "Einstellungen", "Settings";
    close: "Schließen", "Close";
    exit: "Beenden", "Exit";
    untitled: "Unbenannt.pdf", "Untitled.pdf";
    saved: "Gespeichert", "Saved";
    not_a_pdf: "Keine PDF-Datei", "Not a PDF file";
    welcome: "Öffnen Sie eine PDF-Datei oder ziehen Sie sie in dieses Fenster.", "Open a PDF file or drop it onto this window.";
    print_sent: "An den Drucker gesendet", "Sent to printer";
    print_hint: "Das Dokument wird an den Standarddrucker (CUPS) gesendet.", "The document is sent to the default printer (CUPS).";
    copies: "Exemplare", "Copies";

    // Editing
    undo: "Rückgängig", "Undo";
    redo: "Wiederholen", "Redo";
    copy: "Kopieren", "Copy";
    copied: "Text kopiert", "Text copied";
    delete: "Löschen", "Delete";
    properties: "Eigenschaften", "Properties";
    apply: "Übernehmen", "Apply";
    ok: "OK", "OK";
    cancel: "Abbrechen", "Cancel";
    insert: "Einfügen", "Insert";

    // Ribbon tabs
    tab_home: "Start", "Home";
    tab_comment: "Anmerkungen", "Comments";
    tab_organize: "Dokumentaufbau", "Document";
    tab_convert: "Umwandeln", "Convert";
    tab_share: "Teilen", "Share";
    tab_review: "Überprüfen", "Review";
    tab_accessibility: "Barrierefreiheit", "Accessibility";
    tab_bookmarks: "Lesezeichen", "Bookmarks";
    tab_form: "Formular", "Form";
    tab_view: "Ansicht", "View";
    tab_protect: "Schützen", "Protect";
    tab_help: "Hilfe", "Help";

    // Groups
    group_tools: "Werkzeuge", "Tools";
    group_zoom: "Zoom", "Zoom";
    group_comment: "Kommentieren", "Comment";
    group_pages: "Seiten", "Pages";
    group_find: "Suchen", "Find";
    group_text_markup: "Textmarkierung", "Text markup";
    group_text: "Text", "Text";
    group_drawing: "Zeichnen", "Drawing";
    group_stamps: "Stempel", "Stamps";
    group_manage: "Verwalten", "Manage";
    group_insert: "Einfügen", "Insert";
    group_order: "Reihenfolge", "Order";
    group_forms: "Formulare", "Forms";
    group_layout: "Seitenlayout", "Page layout";
    group_panels: "Bereiche", "Panes";
    group_appearance: "Darstellung", "Appearance";
    group_security: "Sicherheit", "Security";
    group_attachments: "Anhänge", "Attachments";
    password_protect: "Kennwortschutz", "Password protection";
    remove_protection: "Schutz entfernen", "Remove protection";
    protection_removed: "Kennwortschutz entfernt", "Password protection removed";
    protected_msg: "Kennwortschutz gesetzt – wirksam nach dem Speichern", "Password protection set – effective after saving";
    status_encrypted: "Dieses Dokument ist verschlüsselt.", "This document is encrypted.";
    status_not_encrypted: "Dieses Dokument ist nicht verschlüsselt.", "This document is not encrypted.";
    user_password: "Kennwort zum Öffnen", "Password to open";
    owner_password: "Berechtigungskennwort", "Permissions password";
    repeat_password: "Kennwort wiederholen", "Repeat password";
    passwords_differ: "Die Kennwörter stimmen nicht überein.", "The passwords do not match.";
    password_needed: "Bitte mindestens ein Kennwort eingeben.", "Please enter at least one password.";
    protect_hint: "Verschlüsselung mit AES-256. Ohne Berechtigungskennwort wird ein zufälliges erzeugt; die Einschränkungen gelten dann für alle.", "Encryption with AES-256. Without a permissions password a random one is generated; the restrictions then apply to everyone.";
    allow_print: "Drucken erlauben", "Allow printing";
    allow_copy: "Kopieren von Text erlauben", "Allow copying text";
    allow_modify: "Ändern erlauben", "Allow changes";
    allow_annotate: "Kommentieren erlauben", "Allow comments";
    allow_fill: "Formulare ausfüllen erlauben", "Allow filling forms";
    allow_assemble: "Seiten organisieren erlauben", "Allow organizing pages";
    attachments: "Anhänge", "Attachments";
    no_attachments: "Keine Dateianhänge", "No file attachments";
    save_attachment: "Speichern…", "Save…";
    add_attachment: "Datei anhängen…", "Attach file…";
    delete_attachment: "Anhang löschen", "Delete attachment";
    attachment_hint: "Anhänge werden aus Sicherheitsgründen nie direkt geöffnet, nur gespeichert.", "For security reasons attachments are never opened directly, only saved.";
    fullscreen: "Vollbild", "Full screen";
    replace_original: "Ersetzt wird", "Replacing";
    create_link: "Verweis erstellen", "Create link";
    link_to_url: "Web-Adresse öffnen", "Open web address";
    link_to_page: "Zu Seite springen", "Go to page";
    quick_start: "Schnellstart (Strg+.)", "Quick start (Ctrl+.)";
    hint_vertices: "Eckpunkte nacheinander anklicken; Doppelklick oder Eingabe beendet, Esc bricht ab.", "Click the corners one after another; double click or Enter finishes, Esc cancels.";
    hint_eraser: "Über Freihand-Zeichnungen ziehen, um sie zu löschen.", "Drag over freehand drawings to erase them.";
    hint_zoom: "Klicken vergrößert, Umschalt+Klick oder Rechtsklick verkleinert, Aufziehen zoomt auf den Bereich.", "Click zooms in, Shift+click or right click zooms out, dragging zooms to the area.";
    hint_select_objects: "Objekt anklicken zum Auswählen, ziehen zum Verschieben, Entf löscht.", "Click an object to select it, drag to move it, Del deletes it.";
    format_tab: "Format", "Format";
    no_command: "Kein passender Befehl", "No matching command";
    manual_text: "MrPDF ist ein freier PDF-Editor. Wählen Sie oben eine Registerkarte und darin ein Werkzeug. Die meisten Werkzeuge werden durch Klicken oder Aufziehen auf der Seite benutzt, Polygone durch Klicken der Eckpunkte (Doppelklick oder Eingabe beendet). Ausgewählte Kommentare ändern Sie in der Registerkarte Format oder rechts in den Eigenschaften. Strg+. öffnet die Befehlssuche.", "MrPDF is a free PDF editor. Choose a tab at the top and a tool in it. Most tools are used by clicking or dragging on the page, polygons by clicking the corners (double click or Enter finishes). Change selected comments in the Format tab or in the properties on the right. Ctrl+. opens the command search.";
    add_bookmark: "Lesezeichen hinzufügen", "Add bookmark";
    add_child_bookmark: "Unterlesezeichen hinzufügen", "Add child bookmark";
    rename: "Umbenennen", "Rename";
    bookmark_target: "Ziel: Seite", "Target: page";
    insert_image: "Bild", "Image";
    images: "Bilder", "Images";

    // Tools
    tool_hand: "Hand", "Hand";
    tool_select_text: "Text auswählen", "Select text";
    tool_highlight: "Hervorheben", "Highlight";
    tool_underline: "Unterstreichen", "Underline";
    tool_strikeout: "Durchstreichen", "Strike out";
    tool_squiggly: "Wellenlinie", "Squiggly";
    tool_note: "Notiz", "Note";
    tool_textbox: "Textfeld", "Text box";
    tool_rectangle: "Rechteck", "Rectangle";
    tool_ellipse: "Ellipse", "Ellipse";
    tool_line: "Linie", "Line";
    tool_arrow: "Pfeil", "Arrow";
    tool_pencil: "Freihand", "Pencil";
    tool_stamp: "Stempel", "Stamp";
    kind_polygon: "Polygon", "Polygon";
    kind_attachment: "Dateianhang", "File attachment";
    kind_other: "Kommentar", "Comment";

    hint_hand: "Ziehen zum Verschieben der Ansicht. Klicken wählt Kommentare aus, Doppelklick bearbeitet sie; Formularfelder lassen sich direkt ausfüllen.", "Drag to move the view. Click selects comments, double click edits them; form fields can be filled in directly.";
    hint_select: "Text mit gedrückter Maustaste markieren, Doppelklick wählt ein Wort. Strg+C kopiert.", "Drag over text to select it, double click selects a word. Ctrl+C copies.";
    hint_markup: "Mit der Maus über den Text ziehen.", "Drag over the text.";
    hint_click: "Auf die Seite klicken, um das Objekt zu platzieren.", "Click on the page to place the object.";
    hint_drag: "Auf der Seite aufziehen. Umschalt rastet Linien in 45°-Schritten ein.", "Drag on the page. Shift snaps lines to 45° steps.";

    // View
    fit_width: "Breite", "Fit width";
    fit_page: "Ganze Seite", "Fit page";
    actual_size: "Originalgröße", "Actual size";
    zoom_in: "Vergrößern", "Zoom in";
    zoom_out: "Verkleinern", "Zoom out";
    layout_single: "Einzelseite", "Single page";
    layout_continuous: "Fortlaufend", "Continuous";
    layout_facing: "Doppelseite", "Two pages";
    sidebar: "Seitenleiste", "Sidebar";
    dark_mode: "Dunkel", "Dark";
    language: "Sprache", "Language";
    first_page: "Erste Seite", "First page";
    previous_page: "Vorherige Seite", "Previous page";
    next_page: "Nächste Seite", "Next page";
    last_page: "Letzte Seite", "Last page";

    // Pages
    rotate_left: "Links drehen", "Rotate left";
    rotate_right: "Rechts drehen", "Rotate right";
    rotate_range: "Seiten drehen…", "Rotate pages…";
    insert_blank: "Leere Seite", "Blank page";
    insert_from_file: "Aus Datei", "From file";
    append_file: "Anhängen", "Append";
    delete_pages: "Seiten löschen", "Delete pages";
    extract_pages: "Seiten extrahieren", "Extract pages";
    extract_suffix: "Auszug", "extract";
    move_up: "Nach vorne", "Move up";
    move_down: "Nach hinten", "Move down";
    page: "Seite", "Page";
    pages: "Seiten", "Pages";
    page_range_label: "Seitenbereich (z. B. 1-3, 5):", "Page range (e.g. 1-3, 5):";
    invalid_range: "Ungültiger Seitenbereich", "Invalid page range";
    count: "Anzahl", "Count";
    paper_size: "Papierformat", "Paper size";
    same_as_current: "Wie aktuelle Seite", "Same as current page";
    orientation: "Ausrichtung", "Orientation";
    portrait: "Hochformat", "Portrait";
    landscape: "Querformat", "Landscape";
    position: "Position", "Position";
    before_current: "Vor der aktuellen Seite", "Before current page";
    after_current: "Nach der aktuellen Seite", "After current page";
    at_end: "Am Ende", "At the end";
    resolution: "Auflösung", "Resolution";
    format: "Format", "Format";

    // Comments
    comment_list: "Kommentarliste", "Comment list";
    flatten: "Abflachen", "Flatten";
    flatten_hint: "Kommentare und Formularfelder werden fest in den Seiteninhalt übernommen und sind danach nicht mehr bearbeitbar.", "Comments and form fields become part of the page content and can no longer be edited.";
    to_images_hint: "Der Seiteninhalt wird durch ein Bild ersetzt (200 dpi). Text ist danach nicht mehr auswählbar; Kommentare bleiben erhalten.", "The page content is replaced by an image (200 dpi). Text can no longer be selected; comments are kept.";
    delete_comment: "Kommentar löschen", "Delete comment";
    no_comments: "Keine Kommentare", "No comments";
    color: "Farbe", "Colour";
    fill: "Füllung", "Fill";
    line_width: "Linienstärke", "Line width";
    opacity: "Deckkraft", "Opacity";
    font_size: "Schriftgröße", "Font size";
    author: "Autor", "Author";
    author_name: "Autorname für Kommentare", "Author name for comments";
    author_hint: "Wird in neuen Kommentaren gespeichert.", "Stored in new comments.";
    text: "Text", "Text";
    comment_text: "Kommentartext", "Comment text";
    enter_text: "Text eingeben…", "Enter text…";
    shift_enter_hint: "Eingabe bestätigt, Umschalt+Eingabe beginnt eine neue Zeile.", "Enter confirms, Shift+Enter starts a new line.";
    not_restylable: "Das Aussehen dieses Kommentars kann nicht geändert werden.", "The appearance of this comment cannot be changed.";
    no_selection: "Keine Auswahl", "Nothing selected";
    no_text_here: "Hier ist kein Text", "No text here";
    no_content_here: "Hier ist kein Inhalt", "No content here";
    hint_correction: "Entf: löschen · Einfg: einfügen · Text tippen: ersetzen", "Del: delete · Ins: insert · type: replace";
    stamp_approved: "Genehmigt", "Approved";
    stamp_not_approved: "Nicht genehmigt", "Not approved";
    stamp_draft: "Entwurf", "Draft";
    stamp_confidential: "Vertraulich", "Confidential";
    stamp_final: "Endgültig", "Final";
    stamp_for_comment: "Zur Prüfung", "For comment";
    stamp_void: "Ungültig", "Void";

    // Forms
    highlight_fields: "Felder hervorheben", "Highlight fields";
    fill_forms: "Ausfüllen", "Fill in";
    forms_hint: "Mit dem Hand-Werkzeug in ein Feld klicken, um es auszufüllen.", "Click a field with the hand tool to fill it in.";
    choose_value: "Wert auswählen", "Choose a value";
    field_not_supported: "Dieser Feldtyp kann noch nicht bearbeitet werden", "This field type cannot be edited yet";

    // Sidebar
    thumbnails: "Seiten", "Pages";
    bookmarks: "Lesezeichen", "Bookmarks";
    comments: "Kommentare", "Comments";
    search: "Suchen", "Search";
    search_hint: "Suchbegriff", "Search term";
    match_case: "Groß-/Kleinschreibung", "Match case";
    whole_words: "Ganze Wörter", "Whole words";
    previous: "Vorheriger Treffer", "Previous match";
    next: "Nächster Treffer", "Next match";
    hits: "Treffer:", "Matches:";
    no_bookmarks: "Dieses Dokument hat keine Lesezeichen.", "This document has no bookmarks.";

    // Links
    open_link_title: "Externen Link öffnen?", "Open external link?";
    open_link_question: "Das Dokument möchte folgende Adresse öffnen. Nur fortfahren, wenn Sie der Quelle vertrauen:", "The document wants to open the following address. Only continue if you trust the source:";
    link_blocked: "Link blockiert (nicht unterstützte oder unsichere Adresse)", "Link blocked (unsupported or unsafe address)";
    link_not_supported: "Diese Link-Art wird nicht unterstützt", "This kind of link is not supported";

    // Dialogs
    password_title: "Kennwort erforderlich", "Password required";
    password: "Kennwort", "Password";
    unsaved_title: "Ungespeicherte Änderungen", "Unsaved changes";
    unsaved_doc: "Das Dokument", "The document";
    unsaved_changed: "wurde verändert.", "has been changed.";
    unsaved_question: "Möchten Sie die Änderungen vor dem Schließen des Dokuments speichern?",
        "Do you want to save the changes before closing the document?";
    yes: "Ja", "Yes";
    no: "Nein", "No";
    meta_title: "Titel", "Title";
    meta_subject: "Thema", "Subject";
    meta_keywords: "Stichwörter", "Keywords";
    meta_creator: "Erstellt mit", "Creator";
    meta_producer: "PDF-Erzeuger", "Producer";
    meta_created: "Erstellt", "Created";
    meta_modified: "Geändert", "Modified";
    file_size: "Dateigröße", "File size";
    security: "Sicherheit", "Security";
    encrypted: "Verschlüsselt", "Encrypted";
    not_encrypted: "Nicht verschlüsselt", "Not encrypted";
    allowed: "erlaubt", "allowed";
    not_allowed: "nicht erlaubt", "not allowed";
    perm_print: "Drucken", "Printing";
    perm_modify: "Ändern", "Changing";
    perm_copy: "Inhalt kopieren", "Copying content";
    perm_annotate: "Kommentieren", "Commenting";
    perm_forms: "Formulare ausfüllen", "Filling forms";
    save_properties: "Übernehmen", "Apply";
    about: "Über MrPDF", "About MrPDF";
    about_text: "Freier PDF-Betrachter und -Editor.", "Free PDF viewer and editor.";
    license_text: "Lizenz: MIT oder Apache-2.0 (nach Wahl). Copyright © Mr. M und Mitwirkende.", "License: MIT or Apache-2.0 (at your option). Copyright © Mr. M and contributors.";
    third_party_text: "Verwendet PDFium (BSD-3-Clause/Apache-2.0), egui/eframe, pdfium-render, lopdf und weitere Bibliotheken unter MIT-, Apache- oder BSD-Lizenzen. Details: THIRD_PARTY_NOTICES.md.", "Uses PDFium (BSD-3-Clause/Apache-2.0), egui/eframe, pdfium-render, lopdf and further libraries under MIT, Apache or BSD licenses. Details: THIRD_PARTY_NOTICES.md.";
    shortcuts_title: "Tastenkürzel", "Keyboard shortcuts";
    shortcuts_text: "Strg+O Öffnen · Strg+S Speichern · Strg+P Drucken · Strg+Z/Y Rückgängig/Wiederholen · Strg+F Suchen · F3 Weitersuchen · Strg+C Kopieren · Strg+A Alles auswählen · Entf Kommentar löschen · Strg+Plus/Minus Zoom · Strg+0 Ganze Seite · Strg+1 Originalgröße · Strg+2 Breite · Bild↑/↓ Blättern · H Hand · V Text auswählen · F4 Seitenleiste · Esc Auswahl aufheben", "Ctrl+O Open · Ctrl+S Save · Ctrl+P Print · Ctrl+Z/Y Undo/Redo · Ctrl+F Find · F3 Find next · Ctrl+C Copy · Ctrl+A Select all · Del Delete comment · Ctrl+Plus/Minus Zoom · Ctrl+0 Fit page · Ctrl+1 Actual size · Ctrl+2 Fit width · PgUp/PgDn Page · H Hand · V Select text · F4 Sidebar · Esc Clear selection";

    // Errors
    error: "Fehler", "Error";
    err_engine: "Die PDF-Bibliothek (PDFium) wurde nicht gefunden. Bitte installieren Sie MrPDF neu.", "The PDF library (PDFium) was not found. Please reinstall MrPDF.";
    err_password_required: "Das Dokument ist kennwortgeschützt.", "The document is password protected.";
    err_wrong_password: "Das Kennwort ist falsch.", "The password is incorrect.";
    err_invalid_pdf: "Die Datei ist keine gültige PDF-Datei oder beschädigt.", "The file is not a valid PDF document or is damaged.";
    err_too_large: "Die Datei ist zu groß.", "The file is too large.";
    err_io: "Fehler beim Zugriff auf", "Could not access";
    err_encrypted: "Diese Funktion ist für verschlüsselte Dokumente nicht verfügbar.", "This function is not available for encrypted documents.";
    err_not_permitted: "Das Dokument erlaubt diese Aktion nicht.", "The document does not permit this action.";
    err_generic: "Die Aktion ist fehlgeschlagen.", "The action failed.";
}

impl Strings {
    pub fn stamp_label(&self, preset: StampPreset) -> &'static str {
        match preset {
            StampPreset::Approved => self.stamp_approved,
            StampPreset::NotApproved => self.stamp_not_approved,
            StampPreset::Draft => self.stamp_draft,
            StampPreset::Confidential => self.stamp_confidential,
            StampPreset::Final => self.stamp_final,
            StampPreset::ForComment => self.stamp_for_comment,
            StampPreset::Void => self.stamp_void,
        }
    }

    pub fn tool_hint(&self, tool: Tool) -> &'static str {
        use crate::ui::tools::Gesture;
        match tool {
            Tool::Hand => self.hint_hand,
            Tool::SelectText => self.hint_select,
            Tool::Eraser => self.hint_eraser,
            Tool::Zoom => self.hint_zoom,
            t => match t.gesture() {
                Gesture::TextMarkup => self.hint_markup,
                Gesture::Click => self.hint_click,
                Gesture::Vertices => self.hint_vertices,
                Gesture::None => self.hint_select_objects,
                _ => self.hint_drag,
            },
        }
    }
}
