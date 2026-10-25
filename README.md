# MrPDF

A free, open-source PDF viewer and editor for Windows, macOS and Linux,
written in Rust. Its user interface follows the familiar ribbon layout of
desktop PDF editors.

*Deutsche Beschreibung weiter unten.*

## Features

* **Viewing** – fast rendering with PDFium, tabs for several documents,
  continuous / single / two-page layouts, zoom (fit width, fit page, 5 %–6400 %,
  Ctrl + mouse wheel), sharp tiles at very high zoom levels, dark mode
* **Navigation** – page thumbnails, bookmarks (outline), links, page
  number entry, keyboard shortcuts
* **Text** – select and copy text, full-text search with match case / whole
  words and a result list
* **Comments** – highlight, underline, strike-out, squiggly, sticky notes,
  text boxes, rectangles, ellipses, lines, arrows, freehand drawing and
  stamps; colours, fill, line width, opacity and font size; moving, editing
  and deleting; comment list; flattening
* **Forms** – fill in text fields, check boxes and radio buttons
* **Pages** – rotate, delete, insert blank pages, insert / append other PDFs,
  extract pages, reorder (buttons or drag & drop in the thumbnails)
* **Documents** – document properties (title, author, …), export pages as
  PNG/JPEG, printing, unlimited undo/redo (up to 50 steps), safe saving
* **German and English** user interface

All annotations are written with their own appearance streams, so they look
the same in every PDF viewer. Edits are stored as incremental updates and keep
the original file structure intact.

## Building

Requirements: Rust 1.88 or newer.

```bash
scripts/fetch-pdfium.sh          # downloads and verifies PDFium
cargo run --release -- file.pdf
```

The PDFium library (`pdfium.dll`, `libpdfium.so` or `libpdfium.dylib`) must be
placed next to the executable for distribution.

Cross-compiling for Windows on Linux (MinGW):

```bash
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu
```

### Tests

```bash
cargo test                  # unit, integration and headless UI tests
python3 scripts/check-licenses.py
python3 scripts/check-release-age.py
```

The UI tests render screenshots into `target/ui-screenshots/`.

## Licence

MrPDF is dual-licensed under the [MIT](LICENSE-MIT) or
[Apache 2.0](LICENSE-APACHE) licence, at your option. All dependencies use
permissive licences (see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md));
PDFium is licensed under BSD-3-Clause / Apache-2.0. MrPDF is an independent
project and not affiliated with any commercial PDF software vendor.

See [SECURITY.md](SECURITY.md) for the security design and how to report
vulnerabilities.

---

## Deutsch

MrPDF ist ein freier PDF-Betrachter und -Editor für Windows, macOS und Linux.
Die Oberfläche orientiert sich am bekannten Menüband-Layout gängiger
PDF-Editoren.

**Funktionen:** Anzeigen mit mehreren Tabs und verschiedenen Seitenlayouts,
Zoom, Miniaturansichten, Lesezeichen, Links, Text markieren und kopieren,
Volltextsuche, Kommentare (Hervorheben, Unterstreichen, Durchstreichen,
Wellenlinie, Notizen, Textfelder, Rechtecke, Ellipsen, Linien, Pfeile,
Freihand, Stempel), Formulare ausfüllen, Seiten drehen/löschen/einfügen/
extrahieren/umsortieren, Dokumente zusammenfügen, Dokumenteigenschaften,
Export als Bild, Drucken, Rückgängig/Wiederholen, deutsche und englische
Oberfläche.

**Bauen:** `scripts/fetch-pdfium.sh` lädt die PDF-Bibliothek (mit
Prüfsummenkontrolle), danach `cargo run --release`.

**Lizenz:** MIT oder Apache-2.0 (nach Wahl). Alle verwendeten Bibliotheken
stehen unter freizügigen Lizenzen.
