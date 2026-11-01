//! One program window: when MrPDF is started while it already runs (for
//! example by opening a second PDF in the Explorer), the new start hands its
//! files to the running program, which opens them as tabs, and ends.
//!
//! The running program listens on a local TCP port (127.0.0.1 only, so the
//! port is not reachable from the network). Port and a random token are
//! written to a file in MrPDF's data folder; a message must start with the
//! token, so other programs cannot send files by guessing the port.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const MAGIC: &str = "MRPDF-OPEN-1";
/// At most this many files per message (protects against floods).
const MAX_FILES: usize = 200;

/// Files sent by later starts; the waker repaints the window when they arrive.
pub struct Inbox {
    rx: Receiver<Vec<PathBuf>>,
    waker: Arc<Mutex<Option<egui::Context>>>,
}

impl Inbox {
    /// Lets arriving files wake up the window.
    pub fn set_context(&self, ctx: &egui::Context) {
        if let Ok(mut w) = self.waker.lock() {
            *w = Some(ctx.clone());
        }
    }

    /// Files that arrived since the last call (an empty list: a start
    /// without files, which only brings the window to the front).
    pub fn take(&self) -> Option<Vec<PathBuf>> {
        let mut all: Option<Vec<PathBuf>> = None;
        while let Ok(files) = self.rx.try_recv() {
            all.get_or_insert_with(Vec::new).extend(files);
        }
        all
    }
}

/// Result of [`start`].
pub enum Start {
    /// This is the only MrPDF: open the window.
    First(Option<Inbox>),
    /// The files were handed to the running MrPDF; this start ends.
    Forwarded,
}

fn info_file(dir: &Path) -> PathBuf {
    dir.join("instance.txt")
}

fn random_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut out = String::new();
    for i in 0..2u64 {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(i);
        h.write_u128(
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos(),
        );
        h.write_u32(std::process::id());
        out.push_str(&format!("{:016x}", h.finish()));
    }
    out
}

/// Hands `files` to a running MrPDF, or prepares this start to receive the
/// files of later starts. `dir` is MrPDF's data folder.
pub fn start(dir: Option<&Path>, files: &[PathBuf]) -> Start {
    let Some(dir) = dir else { return Start::First(None) };
    if forward(dir, files).is_ok() {
        return Start::Forwarded;
    }
    Start::First(listen(dir).ok())
}

/// Sends the files to the running program; Ok only if it confirmed them.
fn forward(dir: &Path, files: &[PathBuf]) -> std::io::Result<()> {
    let info = std::fs::read_to_string(info_file(dir))?;
    let mut parts = info.split_whitespace();
    let port: u16 = parts.next().and_then(|p| p.parse().ok()).ok_or(std::io::ErrorKind::InvalidData)?;
    let token = parts.next().ok_or(std::io::ErrorKind::InvalidData)?;
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(500))?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    let mut msg = format!("{MAGIC} {token}\n");
    for f in files.iter().take(MAX_FILES) {
        // Absolute paths: the running program has another working folder.
        let path = std::fs::canonicalize(f).map(crate::files::strip_verbatim).unwrap_or_else(|_| f.clone());
        let text = path.display().to_string();
        if !text.contains('\n') {
            msg.push_str(&text);
            msg.push('\n');
        }
    }
    msg.push('\n');
    allow_foreground();
    stream.write_all(msg.as_bytes())?;
    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply)?;
    if reply.trim() == "OK" { Ok(()) } else { Err(std::io::ErrorKind::InvalidData.into()) }
}

fn listen(dir: &Path) -> std::io::Result<Inbox> {
    std::fs::create_dir_all(dir)?;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let port = listener.local_addr()?.port();
    let token = random_token();
    std::fs::write(info_file(dir), format!("{port} {token}\n"))?;
    let (tx, rx) = channel();
    let waker: Arc<Mutex<Option<egui::Context>>> = Arc::default();
    let wake = waker.clone();
    std::thread::Builder::new().name("mrpdf-instance".into()).spawn(move || {
        for stream in listener.incoming().flatten() {
            if receive(stream, &token, &tx).is_ok()
                && let Ok(w) = wake.lock()
                && let Some(ctx) = w.as_ref()
            {
                ctx.request_repaint();
            }
        }
    })?;
    Ok(Inbox { rx, waker })
}

/// Reads one message: token line, file lines, empty line.
fn receive(stream: TcpStream, token: &str, tx: &Sender<Vec<PathBuf>>) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    let mut reader = BufReader::new(stream.try_clone()?).take(256 * 1024);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    if line.trim_end() != format!("{MAGIC} {token}") {
        return Err(std::io::ErrorKind::PermissionDenied.into());
    }
    let mut files = Vec::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let path = line.trim_end_matches(['\r', '\n']);
        if path.is_empty() {
            break;
        }
        if files.len() < MAX_FILES {
            files.push(PathBuf::from(path));
        }
    }
    tx.send(files).map_err(|_| std::io::ErrorKind::BrokenPipe)?;
    let mut stream = stream;
    stream.write_all(b"OK\n")
}

/// Windows only lets the program in front bring another window to the
/// front; the new start (which is in front) passes that right on.
#[allow(unsafe_code)]
fn allow_foreground() {
    #[cfg(windows)]
    // SAFETY: plain Win32 call without pointers.
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(
            windows_sys::Win32::UI::WindowsAndMessaging::ASFW_ANY,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_start_hands_files_to_the_first() {
        let dir = tempfile::tempdir().unwrap();
        let Start::First(Some(inbox)) = start(Some(dir.path()), &[]) else { panic!("first start") };
        let file = dir.path().join("a.pdf");
        std::fs::write(&file, b"%PDF").unwrap();
        assert!(matches!(start(Some(dir.path()), std::slice::from_ref(&file)), Start::Forwarded));
        let mut got = None;
        for _ in 0..50 {
            got = inbox.take();
            if got.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let got = got.expect("files arrive");
        assert_eq!(got.len(), 1);
        assert!(got[0].ends_with("a.pdf"), "{got:?}");
    }

    #[test]
    fn wrong_token_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let Start::First(Some(inbox)) = start(Some(dir.path()), &[]) else { panic!("first start") };
        let info = std::fs::read_to_string(info_file(dir.path())).unwrap();
        let port: u16 = info.split_whitespace().next().unwrap().parse().unwrap();
        let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        s.write_all(format!("{MAGIC} falsch\n/etc/passwd\n\n").as_bytes()).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        assert!(inbox.take().is_none());
    }

    #[test]
    fn stale_info_file_starts_a_new_window() {
        let dir = tempfile::tempdir().unwrap();
        // A port nobody listens on (bound and released at once).
        let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap().local_addr().unwrap().port();
        std::fs::write(info_file(dir.path()), format!("{port} abc\n")).unwrap();
        assert!(matches!(start(Some(dir.path()), &[]), Start::First(Some(_))));
    }
}
