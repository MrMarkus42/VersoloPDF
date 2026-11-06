# Changelog

## Unreleased

* Forms: **properties of form fields** in the right pane – name, tooltip
  ("QuickInfo"), required, read only; for text fields a text given in
  advance, multi-line, password, maximum length, font (Helvetica, Helvetica
  bold, Times, Courier), size (or automatic), colour, alignment, border and
  background; for lists and combo boxes the items and the preselected one.
  Changes apply at once and are one undo step each. A new field is selected
  with its properties shown; a double click in "Felder auswählen" shows them
  too
* Forms: **"Ausfüllen"** on the Form tab switches to filling in (hand tool,
  fields highlighted); **Tab / Shift+Tab** go to the next / previous text
  field; the text selection tool fills in fields too. "Formulare ausfüllen"
  (one PDF per CSV row) is now called "Serienformulare aus CSV"
* Start › **"Auswahl"**: one click gives the selection pointer (the arrow
  below opens the menu as before); a frame takes the page content and also
  comments and form fields, which then move and are deleted together.
  "Objekte bearbeiten" works the same way (upper part: the tool, arrow: the
  filter)
* Start › Hinzufügen › **Text hinzufügen**: the text is typed in place and
  then stays selected – it can be moved, resized and turned at once. The
  right pane shows font, size, bold/italic, alignment and colour (also for
  any selected text), position and size, and turning by 90° or any angle
* The search pane no longer grows wider by itself
* The "Schreibmaschine" tool shows its font size in the right pane

## 1.3.1 – 2026-11-06

* macOS: updates install again when macOS runs VersoloPDF from a hidden
  read-only copy ("App Translocation", for a downloaded app that was not
  moved with the Finder): the update goes to the place where the app
  really is. Before, it failed with "Read-only file system" even after the
  administrator password. If that place cannot be found, VersoloPDF says
  how to move the app into "Programme".

## 1.3.0 – 2026-11-06

* macOS: the menu bar of the Mac (VersoloPDF, Ablage, Bearbeiten,
  Darstellung, Dokument, Kommentare, Fenster, Hilfe) with VersoloPDF's
  commands, the usual Mac keys (⌘, for the settings, ⌘Q, ⌃⌘F), "Dienste",
  hiding the program and the search field in "Hilfe"; entries that are not
  possible right now are greyed out, switched on ones are ticked
* macOS: shortcuts are shown with ⌘ ⇧ ⌥ instead of "Strg"
* macOS: "Per E-Mail senden" works again with Apple Mail – the files are
  handed to Mail, which starts a new message with them attached (before,
  Mail did not open)

## 1.2.0 – 2026-11-05

* **VersoloPDF for macOS**: one app for Apple chips and Intel (macOS 11 or
  newer), built and tested on a Mac by GitHub Actions
  * printing through the macOS print dialog, with the printer and copies
    chosen in VersoloPDF preselected and all options of the printer
  * "Per E-Mail senden" opens a new message in Apple Mail or Outlook with the
    files attached
  * "Vorlesen" uses the voice of macOS, with the chosen volume
  * text recognition with the recognition built into macOS (no extra
    program needed; Tesseract can still be chosen)
  * PDFs opened in the Finder (double click, "Öffnen mit", Dock) open as
    tabs, also while VersoloPDF already runs
  * new versions install themselves (an administrator's password is asked
    for if the Applications folder is protected)
  * signing with digital IDs from the keychain; the private key stays there
* `scripts/fetch-pdfium.sh` also runs with the bash of macOS

## 1.1.2 – 2026-11-05

* Updates install reliably: the new version copies itself into the program
  folder (no console windows any more); a protected folder such as
  "C:\Programme" makes Windows ask for administrator rights once. Files of
  the running version are put aside and removed by the next start.
* Help › "Check for updates" opens the update page and starts the search
  instead of opening the web site

## 1.1.1 – 2026-11-05

Security and maintenance update:

* PDFium (the PDF engine) updated from chromium/7881 (June 2026) to
  chromium/8111 (October 2026) with the security fixes of these months
* lopdf (reads and writes the PDF structure) updated to 0.46
* About 60 further libraries updated to their newest versions that are at
  least 14 days old; all licences checked again
* The release checklist now always includes checking PDFium and all
  libraries for newer versions

## 1.1.0 – 2026-11-05

* **New name: VersoloPDF** (formerly MrPDF). Settings, digital IDs, recovery
  copies and the error log of the old name are taken over at the first
  start; sessions saved as `.mrpdfsession` still open.
* Certify documents (first signature with the allowed later changes:
  none, filling in forms and signing, or also comments)
* Time stamps (RFC 3161): signatures can get a time stamp of a time stamp
  authority, and "Protect › Time stamp" adds a document time stamp; only a
  hash value is sent. "Check signatures" shows certifications and time
  stamps. Settings › Signatures: the authority and the default.
* Barcode form field: a QR code of any text or web address (own encoder,
  error correction level M, checked with an independent reader)
* Layers: the "Ebenen" pane switches the layers of a document (saved as the
  document's default view, undoable)
* Snapping to objects: drawing snaps to the edges of text, images and
  graphics (View › Snap › To objects)
* Split pages at guides: the split dialog takes the guides as split lines
* Read aloud in the Accessibility tab
* Greyed-out placeholders removed: cloud and SharePoint places, JavaScript
  console and settings, web page and e-mail to PDF, scanning and scan
  cleanup, panes and settings categories without function, thin lines,
  word highlighting while reading aloud

## 1.0.0 – 2026-11-05

First public release. Everything below is new in this version:

* Viewing (tabs, layouts, zoom, high-zoom tiles, dark mode, full screen)
* Navigation (thumbnails with drag & drop, bookmarks incl. editing, links)
* Text selection, copy and full-text search
* Comments: text markup, notes, text boxes, shapes, lines/arrows, freehand,
  stamps, image stamps; editing, moving, deleting, comment list, flattening
* Forms: text fields, check boxes, radio buttons, choice fields
* Pages: rotate, delete, insert blank/from file, extract, reorder
* Security: AES-256 password protection, permissions, attachments
* Document properties, image export, printing, undo/redo
* German and English user interface
* Ribbon with 13 tabs in the familiar layout, quick command search (Ctrl+.)
* More comment tools: typewriter, callout, polygon, cloud, eraser,
  measurements, file attachment, area highlight, replace/insert text;
  comment summary, XFDF import/export, delete all comments
* Form designer: create, move and delete fields; reset; XFDF form data
* Document structure: merge, split, duplicate, replace, swap, reverse,
  find duplicates, crop, resize, margins, watermark, background,
  header/footer, Bates numbering, page labels
* Page dialogs: page range / target / unit pickers,
  presets and live previews; insert from clipboard, images, text and RTF;
  delete blank pages; several pages per sheet; split documents and pages;
  overlay pages; crop with handles, remove cropped content, trim white
  margins; page marks that can be removed and managed later; Bates for several
  files; page label ruler; "Fit content" (Ctrl+3)
* Secure redaction (mark, search and redact, apply)
* Bookmark tools and table of contents
* Review and convert: word count, document comparison, accessibility report,
  new document from files, contents to images, grayscale, text/Word export
* File backstage: recent files with pins, sessions, recovery, PDF optimizer
  (images, fonts, embedding all or only form fields / markup comments,
  hidden layers, clean-up), space usage, own print dialog, document
  properties, settings dialog
* Images in GIF, TIFF, WebP, PBM/PGM/PPM, TGA and ICO can be opened and
  inserted; page export also as GIF, WebP and PBM/PGM/PPM
* Digital signatures: sign with a digital ID (.pfx/.p12 or a new self-signed
  one), visible, invisible or into an existing signature field (PAdES,
  appended as incremental update); check signatures, hint when opening a
  signed document and when saving breaks signatures
* Keyboard operation of the ribbon with key tips (Alt or F10)
* Error log and crash reports ("Hilfe › Fehlerprotokoll öffnen")
* Modern look as default (system font, outline icons, fixed dialog buttons)
* Compacter ribbon that can be collapsed and pinned as in office programs
  (corner button, double click on a tab, Ctrl+F1); a collapsed ribbon shows
  a tab's commands over the document until one is used
* The ribbon has the same height on every tab and in every window width (the
  document no longer jumps); the tab names no longer move on hover
* "Move to: New window" starts a separate VersoloPDF with the full ribbon for the
  document (unsaved changes are asked about first)
* Form buttons: name, caption and action (reset form, open a web address,
  go to a page, next/previous page) are chosen when the button is drawn, and
  a click while filling in runs the action; umlauts in field captions and
  list entries are written correctly (they appeared as garbage before)
* Saving own dialog settings: the list stays open while the name is typed;
  Enter saves, ✕ deletes an entry
* Settings › Signatures: defaults for reason, location, contact and the
  corner of a visible signature
* Settings › Updates: looks for the newest release on GitHub (by hand or once
  a day at start); on Windows it downloads the package, checks its SHA-256
  checksum, installs it and restarts VersoloPDF
