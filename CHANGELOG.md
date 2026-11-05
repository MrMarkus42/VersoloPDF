# Changelog

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
