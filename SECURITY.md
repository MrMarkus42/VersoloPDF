# Security policy

## Reporting a vulnerability

Please report security problems privately through GitHub ("Security" tab of
the repository, "Report a vulnerability") instead of opening a public issue.
You will get an answer within a few days.

## Security design

VersoloPDF opens files from untrusted sources, so it is built defensively:

* **Memory safety** – the application is written in Rust. `unsafe` code is
  denied crate-wide; the only exceptions are small, reviewed modules that call
  operating system functions (Win32, macOS, Linux sandbox set-up).
* **Protected mode (sandbox)** – PDFium, the C++ engine that reads and draws
  the documents, does not run inside the program. Every open document gets
  its own helper process (`versolopdf-engine`, a small program of its own that
  contains only the PDF engine) that only
  receives that document and answers requests over a pipe; every answer is
  size-checked and decoded by safe Rust code (`src/engine/`). The helper is
  locked down before it sees a document:
  * Windows: AppContainer without capabilities (no access to the user's
    files, registry or the network; low integrity), a job object (no further
    processes, memory limit, no clipboard/desktop access, killed with the
    program) and hardening policies (no dynamic or remote code, strict
    handles, only system fonts, extension points disabled).
  * macOS: the macOS sandbox (Seatbelt) with a deny-by-default profile: only
    fonts and system libraries can be read; no writing, network or new
    processes.
  * Linux: Landlock (only font folders readable, nothing writable) and a
    seccomp filter (no sockets, no new processes, no ptrace, no mounts).
  A crashed or hanging helper (3 minutes) is ended and replaced; a document
  that crashes its helper repeatedly is given up. Tests (`tests/engine.rs`)
  check on every system that the helper cannot read a user file, write a
  file, connect to a server or start a program. The status is shown under
  Einstellungen › Sicherheit.
* **PDF parsing** – rendering, text extraction and form handling are done by
  PDFium, the engine used and fuzzed continuously by Google Chrome, inside the
  sandbox described above. Structural edits use `lopdf` (pure Rust) with a
  limit on decompressed stream sizes (protection against "decompression
  bombs"); parser panics are caught and turned into errors.
* **Signed updates** – the built-in update only installs packages that carry a
  valid [minisign](https://jedisct1.github.io/minisign/) signature (Ed25519)
  of the project's update key. The public key is compiled into the program
  (`keys/update-key.pub`), never read from a file or setting; the secret key
  is password-protected and never leaves the developer's own computer (not on
  GitHub, not in CI, not in a cloud folder). The signed comment names system
  and version (`versolopdf-windows 1.5.0`), and only a version newer than the
  running one is accepted, so an older or foreign package cannot be passed off
  as the new one. Packages are downloaded into a private folder of the user
  (`%LOCALAPPDATA%\VersoloPDF\updates`, `~/Library/Caches/VersoloPDF/updates`)
  and checked; then the **installed** program – never the downloaded one –
  installs the update: it opens the package so that nobody can change it,
  checks the signature again with its own built-in key, unpacks it into a
  folder next to the program folder (with the same protection) and replaces
  the program files. Only this step runs with administrator rights, and only
  where the program folder is protected (installation for all users); the new
  version is started without them. Nothing goes to the network before the user
  agreed once, and then only the project's release list is asked for.
  Versions up to 1.4 used an ECDSA P-256 signature (`.sig`); 1.5.0 carries both
  so that they can still update.
* **File association** – VersoloPDF registers itself for PDF and session files
  only when the user asks for it (installer option or *Einstellungen ›
  Dateizuordnung*), for the current user, and removes the registration again
  when uninstalled.
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
