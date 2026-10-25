# Security policy

## Reporting a vulnerability

Please report security problems privately to **mr.m@users.noreply.github.com** instead of
opening a public issue. You will get an answer within a few days.

## Security design

MrPDF opens files from untrusted sources, so it is built defensively:

* **Memory safety** – the application is written in Rust. `unsafe` code is
  denied crate-wide; the only exception is the small, reviewed Windows printing
  module that calls Win32 APIs.
* **PDF parsing** – rendering, text extraction and form handling are done by
  PDFium, the engine used and fuzzed continuously by Google Chrome. Structural
  edits use `lopdf` (pure Rust) with a limit on decompressed stream sizes
  (protection against "decompression bombs"); parser panics are caught and
  turned into errors.
* **No active content** – JavaScript, launch actions, embedded executables and
  remote content are never executed. PDFium is built without V8/XFA.
* **Links** – only `http`, `https` and `mailto` links can be opened, and only
  after the user confirmed the exact address.
* **Library loading** – PDFium is loaded from an absolute path next to the
  executable only, never through the system search path (no DLL hijacking).
* **Resource limits** – files above 2 GiB are refused, bitmaps are limited in
  size and undo history is capped in memory.
* **Safe saving** – documents are written to a temporary file and atomically
  renamed, so a failure never destroys the original.
* **Supply chain** – dependencies are pinned in `Cargo.lock`, only releases
  older than 14 days are accepted (`scripts/check-release-age.py`), licences
  are checked (`scripts/check-licenses.py`), and the PDFium download is
  verified against pinned SHA-256 checksums (`scripts/fetch-pdfium.sh`).
* **Imported files** – XFDF (comments, form data) is parsed with `quick-xml`,
  which never loads external entities or DTDs; files are size-limited. Images
  are decoded with size and memory limits; bookmark lists are size-limited.
* **Redaction** – applying redactions replaces every affected page by an
  image with the marked areas painted black, removes the redaction marks and
  rewrites the whole file without its revision history, dropping objects that
  are no longer used. Removed text therefore cannot be recovered from the
  saved file. (Text in other places, e.g. comments, bookmarks or metadata, is
  not touched and should be checked separately.)
