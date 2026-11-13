//! Starting the engine helper with restrictions, and the restrictions the
//! helper puts on itself.
//!
//! * Windows: the helper runs in an AppContainer (no access to the user's
//!   files, no network) inside a job that allows no further processes, with
//!   hardening policies (no remote or dynamic code, strict handles).
//! * macOS: the helper locks itself into the macOS sandbox (Seatbelt) after
//!   loading PDFium: only fonts and system libraries can be read; no
//!   writing, network or new processes.
//! * Linux: Landlock allows reading only fonts; seccomp forbids network,
//!   new processes and debugging other processes.
//!
//! The helper always checks what is in force and reports it ([`lockdown`]).

use std::io::{Read, Write};
use std::path::Path;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

/// A running helper: requests go to `to`, answers come from a reader thread.
pub struct Helper {
    to: Box<dyn Write + Send>,
    frames: Receiver<std::io::Result<Option<Vec<u8>>>>,
    process: Box<dyn Process>,
}

/// Ending the helper process.
pub trait Process: Send {
    fn kill(&mut self);
    /// The exit code once the process has ended (for error messages).
    fn exit_code(&mut self) -> Option<i64> {
        None
    }
}

impl Helper {
    fn new(to: Box<dyn Write + Send>, mut from: Box<dyn Read + Send>, process: Box<dyn Process>) -> Self {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("versolopdf-engine-reader".into())
            .spawn(move || {
                loop {
                    let frame = super::proto::read_frame(&mut from);
                    let end = !matches!(frame, Ok(Some(_)));
                    if tx.send(frame).is_err() || end {
                        break;
                    }
                }
            })
            .ok();
        Self { to, frames: rx, process }
    }

    pub fn send(&mut self, frame: &[u8]) -> std::io::Result<()> {
        super::proto::write_frame(&mut self.to, frame)
    }

    /// The next frame; the helper is ended if it takes longer than `limit`.
    pub fn receive(&mut self, limit: Duration) -> std::io::Result<Vec<u8>> {
        match self.frames.recv_timeout(limit) {
            Ok(Ok(Some(frame))) => Ok(frame),
            Ok(Ok(None)) => Err(match self.process.exit_code() {
                Some(code) => std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    format!("the helper ended (exit code {code:#x})"),
                ),
                None => std::io::ErrorKind::UnexpectedEof.into(),
            }),
            Ok(Err(e)) => Err(e),
            Err(RecvTimeoutError::Timeout) => {
                self.process.kill();
                Err(std::io::ErrorKind::TimedOut.into())
            }
            Err(RecvTimeoutError::Disconnected) => Err(std::io::ErrorKind::BrokenPipe.into()),
        }
    }
}

impl Drop for Helper {
    fn drop(&mut self) {
        self.process.kill();
    }
}

/// Starts `exe` as engine helper with the restrictions of this system.
pub fn spawn(exe: &Path) -> std::io::Result<Helper> {
    #[cfg(windows)]
    {
        windows::spawn(exe)
    }
    #[cfg(not(windows))]
    {
        unix_spawn(exe)
    }
}

#[cfg(not(windows))]
struct UnixChild(std::process::Child);

#[cfg(not(windows))]
impl Process for UnixChild {
    fn kill(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[cfg(not(windows))]
fn unix_spawn(exe: &Path) -> std::io::Result<Helper> {
    use std::process::{Command, Stdio};
    let mut cmd = Command::new(exe);
    cmd.arg(super::host::ENGINE_ARG)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        // The helper gets no environment except where PDFium is.
        .env_clear();
    if let Some(dir) = std::env::var_os(super::pdfium_lib::PDFIUM_DIR_ENV) {
        cmd.env(super::pdfium_lib::PDFIUM_DIR_ENV, dir);
    }
    if let Some(home) = std::env::var_os("HOME") {
        // Only used to find the user's own fonts (~/.fonts, ~/Library/Fonts).
        cmd.env("HOME", home);
    }
    let mut child = cmd.spawn()?;
    let to = child.stdin.take().ok_or_else(|| std::io::Error::other("no pipe"))?;
    let from = child.stdout.take().ok_or_else(|| std::io::Error::other("no pipe"))?;
    Ok(Helper::new(Box::new(to), Box::new(from), Box::new(UnixChild(child))))
}

/// Called by the helper once PDFium is loaded: applies the restrictions.
/// Returns what is in force, or why nothing could be restricted.
pub fn lockdown() -> Result<String, String> {
    #[cfg(target_os = "linux")]
    {
        linux::lockdown()
    }
    #[cfg(target_os = "macos")]
    {
        macos::lockdown()
    }
    #[cfg(windows)]
    {
        windows::lockdown()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        Err("no restrictions on this system".into())
    }
}
