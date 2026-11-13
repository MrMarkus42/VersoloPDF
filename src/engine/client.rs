//! The program side of the engine: one connection per document.
//!
//! In protected mode every document gets its own helper process (see
//! [`super::sandbox`]); a document can then not see another one, and a
//! crash or a hang ends only its helper. The helper is started again when
//! needed and gets the document once more. If no helper can be started,
//! the engine runs inside the program and [`protection`] says so.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use serde::de::DeserializeOwned;

use super::host::Host;
use super::proto::{Bytes, Hello, LoadInfo, PROTOCOL, Request, encode, read_answer};
use super::sandbox::{self, Helper};
use crate::error::{Error, Result};

/// The program that runs the helper; `None`: engine inside the program.
static HELPER_EXE: Mutex<Option<PathBuf>> = Mutex::new(None);
/// What the last helper reported.
static LAST: Mutex<Option<Protection>> = Mutex::new(None);

/// How a request may take before the helper is ended.
const ANSWER_LIMIT: Duration = Duration::from_secs(180);
/// How long the helper may take to start.
const START_LIMIT: Duration = Duration::from_secs(30);
/// Crashes of one document's helper before the document is given up.
const MAX_FAILURES: u32 = 3;

/// Whether documents are opened in protected mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Protection {
    /// Every document is read in a restricted helper; the text says how.
    Protected(String),
    /// A separate helper, but not all restrictions could be applied.
    Partial(String),
    /// PDFium runs inside the program; the text says why.
    Off(String),
}

/// The helper program next to the program `main_exe`: `versolopdf-engine`, a
/// program of its own that contains only the PDF engine (so it loads no window
/// or system libraries that a sandbox may refuse). Without it, the program
/// itself is the helper.
pub fn helper_program(main_exe: &std::path::Path) -> PathBuf {
    let name = if cfg!(windows) { "versolopdf-engine.exe" } else { "versolopdf-engine" };
    main_exe.parent().map(|d| d.join(name)).filter(|p| p.is_file()).unwrap_or_else(|| main_exe.to_path_buf())
}

/// Protected mode: documents are read by `exe` started as helper.
pub fn use_helper(exe: PathBuf) {
    if let Ok(mut h) = HELPER_EXE.lock() {
        *h = Some(exe);
    }
}

/// PDFium inside the program (tests, or when switched off).
pub fn use_in_process() {
    if let Ok(mut h) = HELPER_EXE.lock() {
        *h = None;
    }
    set_status(Protection::Off("Geschützter Modus ist ausgeschaltet.".into()));
}

/// The setting "Geschützter Modus" (on unless the user switched it off).
static ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Switches protected mode on or off for documents opened afterwards
/// (the helper program set with [`use_helper`] is kept).
pub fn set_enabled(on: bool) {
    let was = ENABLED.swap(on, std::sync::atomic::Ordering::Relaxed);
    if !on {
        set_status(Protection::Off("Geschützter Modus ist ausgeschaltet.".into()));
    } else if !was && let Ok(mut last) = LAST.lock() {
        *last = None;
    }
}

fn helper_exe() -> Option<PathBuf> {
    if !ENABLED.load(std::sync::atomic::Ordering::Relaxed) {
        return None;
    }
    HELPER_EXE.lock().ok().and_then(|h| h.clone())
}

fn set_status(p: Protection) {
    if let Ok(mut last) = LAST.lock() {
        *last = Some(p);
    }
}

/// The protection of the documents opened last.
pub fn protection() -> Protection {
    let last = LAST.lock().ok().and_then(|l| l.clone());
    match (last, helper_exe()) {
        (Some(p), _) => p,
        (None, Some(_)) => Protection::Protected("Noch kein Dokument geöffnet.".into()),
        (None, None) => Protection::Off("Geschützter Modus ist ausgeschaltet.".into()),
    }
}

enum Conn {
    Local(Box<Host>),
    Remote(Helper),
}

fn start_helper(exe: &std::path::Path) -> Result<Helper> {
    let fail = |e: String| Error::Pdfium(format!("PDF-Kern konnte nicht gestartet werden: {e}"));
    let mut helper = sandbox::spawn(exe).map_err(|e| fail(e.to_string()))?;
    let first = helper.receive(START_LIMIT).map_err(|e| fail(e.to_string()))?;
    let hello: Hello = read_answer(&first)?;
    if hello.protocol != PROTOCOL {
        return Err(fail(format!("protocol {}", hello.protocol)));
    }
    if hello.locked {
        log::info!("PDF-Kern geschützt gestartet: {}", hello.detail);
        set_status(Protection::Protected(hello.detail));
    } else {
        log::warn!("PDF engine helper without full restrictions: {}", hello.detail);
        set_status(Protection::Partial(hello.detail));
    }
    Ok(helper)
}

/// A helper started in advance (it has not seen any document yet), so a
/// document opens without waiting for a new process.
type Spare = (PathBuf, std::thread::JoinHandle<Result<Helper>>);
static SPARE: Mutex<Option<Spare>> = Mutex::new(None);

fn start_spare(exe: PathBuf) {
    let worker = exe.clone();
    if let Ok(handle) = std::thread::Builder::new()
        .name("versolopdf-engine-start".into())
        .spawn(move || start_helper(&worker))
        && let Ok(mut spare) = SPARE.lock()
    {
        *spare = Some((exe, handle));
    }
}

/// Whether a helper still answers (a waiting helper may have been ended).
fn alive(helper: &mut Helper) -> bool {
    // Without a document the helper answers with a fault – an answer.
    encode(&Request::Info)
        .is_ok_and(|frame| helper.send(&frame).is_ok() && helper.receive(Duration::from_secs(10)).is_ok())
}

/// A helper for `exe`: the one started in advance, or a new one; then the
/// next one is started in the background.
fn take_helper(exe: &std::path::Path) -> Result<Helper> {
    let spare = SPARE.lock().ok().and_then(|mut s| s.take());
    let helper = match spare {
        Some((spare_exe, handle)) if spare_exe == exe => handle
            .join()
            .unwrap_or_else(|_| Err(Error::Pdfium("engine start".into())))
            .and_then(|mut h| if alive(&mut h) { Ok(h) } else { Err(Error::Pdfium("spare ended".into())) }),
        _ => start_helper(exe),
    };
    let helper = match helper {
        Ok(h) => h,
        // The one in advance may have failed (or ended): once more directly.
        Err(_) => start_helper(exe)?,
    };
    start_spare(exe.to_path_buf());
    Ok(helper)
}

fn connect() -> Conn {
    if let Some(exe) = helper_exe() {
        match take_helper(&exe) {
            Ok(h) => return Conn::Remote(h),
            Err(e) => {
                log::warn!("{e}; PDFium runs inside the program");
                set_status(Protection::Off(e.to_string()));
            }
        }
    }
    Conn::Local(Box::default())
}

#[derive(Default)]
struct Inner {
    conn: Option<Conn>,
    /// Changes were made in the engine that are not in the program's copy.
    dirty: bool,
    /// The engine has a document loaded.
    loaded: bool,
    failures: u32,
}

/// The engine of one document.
#[derive(Default)]
pub struct EngineDoc {
    inner: Mutex<Inner>,
}

/// The document as the program has it, for loading it again after a crash.
pub type Source<'a> = (&'a [u8], Option<&'a str>);

impl EngineDoc {
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads a document into the engine.
    pub fn load(&self, bytes: &[u8], password: Option<&str>) -> Result<LoadInfo> {
        let mut inner = self.inner.lock().map_err(|_| Error::Pdfium("engine lock".into()))?;
        inner.loaded = false;
        inner.dirty = false;
        let req = Request::Load { bytes: Bytes(bytes.to_vec()), password: password.map(str::to_string) };
        let info = inner.call(req, None, false)?;
        inner.loaded = true;
        Ok(info)
    }

    /// A request that does not change the document.
    pub fn read<T: DeserializeOwned>(&self, req: Request, source: Source<'_>) -> Result<T> {
        let mut inner = self.inner.lock().map_err(|_| Error::Pdfium("engine lock".into()))?;
        inner.call(req, Some(source), false)
    }

    /// A request that changes the document in the engine.
    pub fn edit<T: DeserializeOwned>(&self, req: Request, source: Source<'_>) -> Result<T> {
        let mut inner = self.inner.lock().map_err(|_| Error::Pdfium("engine lock".into()))?;
        let result = inner.call(req, Some(source), true);
        inner.dirty = true;
        result
    }

    /// A request in an engine of its own (new documents, other files).
    pub fn once<T: DeserializeOwned>(req: Request) -> Result<T> {
        let doc = EngineDoc::new();
        let mut inner = doc.inner.lock().map_err(|_| Error::Pdfium("engine lock".into()))?;
        inner.call(req, None, false)
    }

    /// True if the document's engine runs in a separate helper.
    pub fn is_separate(&self) -> bool {
        self.inner.lock().is_ok_and(|i| matches!(i.conn, Some(Conn::Remote(_))))
    }
}

const CRASHED: &str = "Der PDF-Kern ist bei diesem Dokument abgestürzt oder hat nicht geantwortet";

impl Inner {
    fn call<T: DeserializeOwned>(
        &mut self,
        req: Request,
        source: Option<Source<'_>>,
        edit: bool,
    ) -> Result<T> {
        if self.failures >= MAX_FAILURES {
            return Err(Error::Pdfium(format!("{CRASHED}; es wird nicht weiter verwendet.")));
        }
        if self.conn.is_none() {
            self.conn = Some(connect());
            // A new helper needs the document again.
            if self.loaded
                && let Some((bytes, password)) = source
            {
                let load =
                    Request::Load { bytes: Bytes(bytes.to_vec()), password: password.map(str::to_string) };
                self.call::<LoadInfo>(load, None, false)?;
            }
        }
        let frame = match self.conn.as_mut() {
            Some(Conn::Local(host)) => {
                let answer = match host.handle(req) {
                    Ok(a) => a,
                    Err(e) => super::proto::answer_err(&e.into()),
                };
                return read_answer(&answer);
            }
            Some(Conn::Remote(_)) => encode(&req)?,
            None => return Err(Error::Pdfium("no engine".into())),
        };
        let Some(Conn::Remote(helper)) = self.conn.as_mut() else {
            return Err(Error::Pdfium("no engine".into()));
        };
        let answer = helper.send(&frame).and_then(|()| helper.receive(ANSWER_LIMIT));
        match answer {
            Ok(a) => match read_answer::<T>(&a) {
                Ok(v) => Ok(v),
                // A fault the helper reported (wrong password, page …).
                Err(e) if a.first() == Some(&1) => Err(e),
                // Garbage: the helper is no longer trusted.
                Err(e) => {
                    self.conn = None;
                    self.failures += 1;
                    Err(e)
                }
            },
            Err(e) => {
                // The next request gets a new helper with the document.
                self.conn = None;
                self.failures += 1;
                log::warn!("PDF engine helper ended: {e}");
                let lost = self.dirty || edit;
                self.dirty = false;
                if lost {
                    Err(Error::Pdfium(format!("{CRASHED}; die letzte Änderung ging verloren.")))
                } else {
                    Err(Error::Pdfium(format!("{CRASHED}.")))
                }
            }
        }
    }
}
