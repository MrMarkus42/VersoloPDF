//! One program window: when VersoloPDF is started while it already runs (for
//! example by opening a second PDF in the Explorer), the new start hands its
//! files to the running program, which opens them as tabs, and ends.
//!
//! The running program receives the files through a channel of the
//! operating system that only the same user on the same computer can use:
//! a named pipe on Windows (remote clients rejected, other users may not
//! write to it) and a Unix socket in a private folder (mode 0700) of
//! VersoloPDF's data folder on Linux and macOS.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const MAGIC: &str = "VERSOLOPDF-OPEN-1";
/// At most this many files per message (protects against floods).
const MAX_FILES: usize = 200;
/// How long a start waits for the running program to confirm.
const TIMEOUT: Duration = Duration::from_secs(3);

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
    /// This is the only VersoloPDF: open the window.
    First(Option<Inbox>),
    /// The files were handed to the running VersoloPDF; this start ends.
    Forwarded,
}

/// Hands `files` to a running VersoloPDF, or prepares this start to receive the
/// files of later starts. `dir` is VersoloPDF's data folder.
pub fn start(dir: Option<&Path>, files: &[PathBuf]) -> Start {
    let Some(dir) = dir else { return Start::First(None) };
    if forward(dir, files).is_ok() {
        return Start::Forwarded;
    }
    Start::First(listen(dir).ok())
}

/// The message: marker line, one absolute path per line, empty line.
fn message(files: &[PathBuf]) -> String {
    let mut msg = format!("{MAGIC}\n");
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
    msg
}

/// Sends the files to the running program; Ok only if it confirmed them.
fn forward(dir: &Path, files: &[PathBuf]) -> std::io::Result<()> {
    let msg = message(files);
    allow_foreground();
    // In a thread: a program that hangs must not block this start.
    let (tx, rx) = channel();
    let dir = dir.to_path_buf();
    std::thread::spawn(move || {
        let result = (|| {
            let mut conn = transport::connect(&dir)?;
            conn.write_all(msg.as_bytes())?;
            conn.flush()?;
            let mut reply = String::new();
            BufReader::new(&mut conn).read_line(&mut reply)?;
            if reply.trim() == "OK" { Ok(()) } else { Err(std::io::ErrorKind::InvalidData.into()) }
        })();
        let _ = tx.send(result);
    });
    rx.recv_timeout(TIMEOUT).unwrap_or_else(|_| Err(std::io::ErrorKind::TimedOut.into()))
}

fn listen(dir: &Path) -> std::io::Result<Inbox> {
    let mut listener = transport::Listener::bind(dir)?;
    let (tx, rx) = channel();
    let waker: Arc<Mutex<Option<egui::Context>>> = Arc::default();
    let wake = waker.clone();
    std::thread::Builder::new().name("versolopdf-instance".into()).spawn(move || {
        loop {
            match listener.accept() {
                Ok(conn) => {
                    if receive(conn, &tx).is_ok()
                        && let Ok(w) = wake.lock()
                        && let Some(ctx) = w.as_ref()
                    {
                        ctx.request_repaint();
                    }
                }
                // Do not spin if the system refuses connections for a while.
                Err(_) => std::thread::sleep(Duration::from_millis(200)),
            }
        }
    })?;
    Ok(Inbox { rx, waker })
}

/// Reads one message and confirms it with "OK".
fn receive<S: Read + Write>(mut conn: S, tx: &Sender<Vec<PathBuf>>) -> std::io::Result<()> {
    let mut files = Vec::new();
    {
        let mut reader = BufReader::new(Read::take(&mut conn, 256 * 1024));
        let mut line = String::new();
        reader.read_line(&mut line)?;
        if line.trim_end() != MAGIC {
            return Err(std::io::ErrorKind::InvalidData.into());
        }
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
    }
    tx.send(files).map_err(|_| std::io::ErrorKind::BrokenPipe)?;
    conn.write_all(b"OK\n")?;
    conn.flush()
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

#[cfg(unix)]
mod transport {
    //! Unix socket in a private folder of VersoloPDF's data folder.

    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::{Path, PathBuf};

    fn socket_path(dir: &Path) -> PathBuf {
        dir.join("ipc").join("instance.sock")
    }

    pub fn connect(dir: &Path) -> std::io::Result<UnixStream> {
        let s = UnixStream::connect(socket_path(dir))?;
        s.set_read_timeout(Some(super::TIMEOUT))?;
        s.set_write_timeout(Some(super::TIMEOUT))?;
        Ok(s)
    }

    pub struct Listener(UnixListener);

    impl Listener {
        pub fn bind(dir: &Path) -> std::io::Result<Self> {
            let path = socket_path(dir);
            let folder = path.parent().unwrap_or(dir);
            // Only this user may enter the folder, so only this user can
            // reach the socket.
            std::fs::DirBuilder::new().recursive(true).mode(0o700).create(folder)?;
            std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o700))?;
            // A socket file of a program that has ended (it did not answer).
            let _ = std::fs::remove_file(&path);
            Ok(Self(UnixListener::bind(&path)?))
        }

        pub fn accept(&mut self) -> std::io::Result<UnixStream> {
            let (s, _) = self.0.accept()?;
            s.set_read_timeout(Some(super::TIMEOUT))?;
            s.set_write_timeout(Some(super::TIMEOUT))?;
            Ok(s)
        }
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod transport {
    //! Named pipe `\\.\pipe\VersoloPDF-<hash of the data folder>`. All `unsafe`
    //! blocks call documented Win32 functions with valid handles and
    //! buffers and check every result.

    use std::hash::{Hash, Hasher};
    use std::path::Path;

    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_BROKEN_PIPE, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, GENERIC_READ, GENERIC_WRITE,
        GetLastError, HANDLE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FlushFileBuffers, OPEN_EXISTING, PIPE_ACCESS_DUPLEX,
        ReadFile, WriteFile,
    };
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
        PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT, WaitNamedPipeW,
    };

    /// Name per user (the data folder lies in the user's profile).
    fn pipe_name(dir: &Path) -> Vec<u16> {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        dir.hash(&mut h);
        format!(r"\\.\pipe\VersoloPDF-{:016x}", h.finish()).encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn last_error() -> std::io::Error {
        // SAFETY: reads the calling thread's last error code.
        std::io::Error::from_raw_os_error(unsafe { GetLastError() } as i32)
    }

    /// One end of a connection; the server end is disconnected when dropped.
    pub struct Pipe {
        handle: HANDLE,
        server: bool,
    }

    // SAFETY: a pipe handle may be used from any thread; Pipe is used by
    // one thread at a time.
    unsafe impl Send for Pipe {}

    impl std::io::Read for Pipe {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let mut n = 0u32;
            let len = buf.len().min(u32::MAX as usize) as u32;
            // SAFETY: buf is valid for len bytes; synchronous call.
            let ok = unsafe { ReadFile(self.handle, buf.as_mut_ptr(), len, &mut n, std::ptr::null_mut()) };
            if ok == 0 {
                // SAFETY: see last_error.
                if unsafe { GetLastError() } == ERROR_BROKEN_PIPE {
                    return Ok(0);
                }
                return Err(last_error());
            }
            Ok(n as usize)
        }
    }

    impl std::io::Write for Pipe {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let mut n = 0u32;
            let len = buf.len().min(u32::MAX as usize) as u32;
            // SAFETY: buf is valid for len bytes; synchronous call.
            let ok = unsafe { WriteFile(self.handle, buf.as_ptr(), len, &mut n, std::ptr::null_mut()) };
            if ok == 0 { Err(last_error()) } else { Ok(n as usize) }
        }

        fn flush(&mut self) -> std::io::Result<()> {
            // SAFETY: valid handle.
            if unsafe { FlushFileBuffers(self.handle) } == 0 { Err(last_error()) } else { Ok(()) }
        }
    }

    impl Drop for Pipe {
        fn drop(&mut self) {
            // SAFETY: the handle is owned by this value and closed once.
            unsafe {
                if self.server {
                    DisconnectNamedPipe(self.handle);
                }
                CloseHandle(self.handle);
            }
        }
    }

    pub fn connect(dir: &Path) -> std::io::Result<Pipe> {
        let name = pipe_name(dir);
        for _ in 0..3 {
            // SAFETY: name is a zero-terminated UTF-16 string.
            let handle = unsafe {
                CreateFileW(
                    name.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    0,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    0,
                    std::ptr::null_mut(),
                )
            };
            if handle != INVALID_HANDLE_VALUE {
                return Ok(Pipe { handle, server: false });
            }
            // SAFETY: see last_error.
            if unsafe { GetLastError() } != ERROR_PIPE_BUSY {
                return Err(last_error());
            }
            // All instances busy: wait until one is free.
            // SAFETY: as above.
            unsafe { WaitNamedPipeW(name.as_ptr(), 1000) };
        }
        Err(std::io::ErrorKind::TimedOut.into())
    }

    /// The server side. One instance of the pipe always waits, so a new
    /// start never finds the name missing while VersoloPDF runs.
    pub struct Listener {
        name: Vec<u16>,
        next: HANDLE,
    }

    // SAFETY: the listener is moved once into its thread and used only there.
    unsafe impl Send for Listener {}

    fn create_instance(name: &[u16], first: bool) -> std::io::Result<HANDLE> {
        let open_mode = PIPE_ACCESS_DUPLEX | if first { FILE_FLAG_FIRST_PIPE_INSTANCE } else { 0 };
        // SAFETY: name is zero-terminated; default security (only the
        // creator and administrators may write).
        let handle = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                open_mode,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                64 * 1024,
                64 * 1024,
                0,
                std::ptr::null(),
            )
        };
        if handle == INVALID_HANDLE_VALUE { Err(last_error()) } else { Ok(handle) }
    }

    impl Listener {
        pub fn bind(dir: &Path) -> std::io::Result<Self> {
            let name = pipe_name(dir);
            // Fails if another program already owns the name.
            let next = create_instance(&name, true)?;
            Ok(Self { name, next })
        }

        pub fn accept(&mut self) -> std::io::Result<Pipe> {
            let handle = self.next;
            // SAFETY: valid pipe handle; synchronous wait for a client.
            let ok = unsafe { ConnectNamedPipe(handle, std::ptr::null_mut()) };
            // SAFETY: see last_error.
            let connected = ok != 0 || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
            // Keep one instance waiting before handing this one out.
            self.next = create_instance(&self.name, false)?;
            if !connected {
                drop(Pipe { handle, server: true });
                return Err(last_error());
            }
            Ok(Pipe { handle, server: true })
        }
    }

    impl Drop for Listener {
        fn drop(&mut self) {
            // SAFETY: the waiting instance is owned by the listener.
            unsafe { CloseHandle(self.next) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait_for(inbox: &Inbox) -> Option<Vec<PathBuf>> {
        for _ in 0..100 {
            if let Some(files) = inbox.take() {
                return Some(files);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        None
    }

    #[test]
    fn second_start_hands_files_to_the_first() {
        let dir = tempfile::tempdir().unwrap();
        let Start::First(Some(inbox)) = start(Some(dir.path()), &[]) else { panic!("first start") };
        let file = dir.path().join("a.pdf");
        std::fs::write(&file, b"%PDF").unwrap();
        assert!(matches!(start(Some(dir.path()), std::slice::from_ref(&file)), Start::Forwarded));
        let got = wait_for(&inbox).expect("files arrive");
        assert_eq!(got.len(), 1);
        assert!(got[0].ends_with("a.pdf"), "{got:?}");
        // A start without files only brings the window to the front.
        assert!(matches!(start(Some(dir.path()), &[]), Start::Forwarded));
        assert_eq!(wait_for(&inbox), Some(Vec::new()));
    }

    #[test]
    fn foreign_messages_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let Start::First(Some(inbox)) = start(Some(dir.path()), &[]) else { panic!("first start") };
        let mut conn = transport::connect(dir.path()).unwrap();
        conn.write_all(b"HELLO\n/etc/passwd\n\n").unwrap();
        conn.flush().unwrap();
        std::thread::sleep(Duration::from_millis(200));
        assert!(inbox.take().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn socket_folder_is_private_and_stale_sockets_are_replaced() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        // A socket file left behind by a program that has ended.
        let stale = std::os::unix::net::UnixListener::bind({
            std::fs::create_dir_all(dir.path().join("ipc")).unwrap();
            dir.path().join("ipc").join("instance.sock")
        })
        .unwrap();
        drop(stale);
        assert!(matches!(start(Some(dir.path()), &[]), Start::First(Some(_))));
        let mode = std::fs::metadata(dir.path().join("ipc")).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
    }
}
