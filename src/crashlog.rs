//! Error log and crash reports ("Fehlerprotokoll").
//!
//! The log goes to `<data dir>/logs/mrpdf-<date>.log` (and to the console
//! of debug builds): program start, every error shown to the user and the
//! warnings of MrPDF. A panic writes `absturz-<time>.txt` with version,
//! system, the place of the error, the backtrace and the last log lines;
//! the next start tells the user where it is. Document contents never go
//! into the log; file names are written without their folders.

use std::collections::VecDeque;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Log files older than this are removed at start.
const KEEP: Duration = Duration::from_secs(14 * 24 * 3600);
/// A log file is started anew above this size.
const MAX_SIZE: u64 = 5 * 1024 * 1024;
/// Log lines kept for the crash report.
const RECENT: usize = 200;
/// Marker of a crash not reported to the user yet.
const MARKER: &str = "letzter-absturz.txt";

/// Folder of the logs and crash reports.
pub fn log_dir() -> Option<PathBuf> {
    crate::files::data_dir().map(|d| d.join("logs"))
}

/// Date and time in UTC as "2026-11-04 16:03:12".
pub fn timestamp(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (days, rest) = (secs / 86_400, secs % 86_400);
    // Civil date from days since 1970-01-01 (H. Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}", rest / 3600, rest / 60 % 60, rest % 60)
}

/// Only the file name of a path (no folders with user names in the log).
pub fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(|| "?".into(), |n| n.to_string_lossy().into_owned())
}

struct Logger {
    file: Mutex<Option<File>>,
    recent: Mutex<VecDeque<String>>,
    /// Level for MrPDF's own messages; other crates only warn and above.
    own: log::LevelFilter,
    console: bool,
}

impl log::Log for Logger {
    fn enabled(&self, meta: &log::Metadata<'_>) -> bool {
        let limit = if meta.target().starts_with("mrpdf") { self.own } else { log::LevelFilter::Warn };
        meta.level() <= limit
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!(
            "{} UTC {:<5} {}: {}",
            timestamp(SystemTime::now()),
            record.level(),
            record.target(),
            record.args()
        );
        if self.console {
            eprintln!("{line}");
        }
        if let Ok(mut f) = self.file.lock()
            && let Some(f) = f.as_mut()
        {
            let _ = writeln!(f, "{line}");
        }
        if let Ok(mut r) = self.recent.lock() {
            if r.len() >= RECENT {
                r.pop_front();
            }
            r.push_back(line);
        }
    }

    fn flush(&self) {
        if let Ok(mut f) = self.file.lock()
            && let Some(f) = f.as_mut()
        {
            let _ = f.flush();
        }
    }
}

static LOGGER: std::sync::OnceLock<Logger> = std::sync::OnceLock::new();

/// Removes old log files and crash reports.
fn clean(dir: &Path) {
    let Ok(read) = std::fs::read_dir(dir) else { return };
    let now = SystemTime::now();
    for e in read.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name == MARKER || !(name.ends_with(".log") || name.ends_with(".txt")) {
            continue;
        }
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|t| now.duration_since(t).is_ok_and(|d| d > KEEP));
        if old {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Starts the log and the crash reports; returns the log file. The level
/// of MrPDF's messages comes from `MRPDF_LOG` (error, warn, info, debug,
/// trace), default info.
pub fn init() -> Option<PathBuf> {
    let own = std::env::var("MRPDF_LOG").ok().and_then(|v| v.parse().ok()).unwrap_or(log::LevelFilter::Info);
    let dir = log_dir();
    let mut path = None;
    let mut file = None;
    if let Some(dir) = &dir
        && std::fs::create_dir_all(dir).is_ok()
    {
        clean(dir);
        let p = dir.join(format!("mrpdf-{}.log", &timestamp(SystemTime::now())[..10]));
        let too_big = std::fs::metadata(&p).is_ok_and(|m| m.len() > MAX_SIZE);
        file = std::fs::OpenOptions::new()
            .create(true)
            .append(!too_big)
            .write(true)
            .truncate(too_big)
            .open(&p)
            .ok();
        path = Some(p);
    }
    let logger = LOGGER.get_or_init(|| Logger {
        file: Mutex::new(file),
        recent: Mutex::new(VecDeque::new()),
        own,
        console: cfg!(debug_assertions) || std::env::var_os("MRPDF_LOG").is_some(),
    });
    if log::set_logger(logger).is_ok() {
        log::set_max_level(own.max(log::LevelFilter::Warn));
    }
    install_panic_hook(dir);
    path
}

/// The crash report text.
fn report(info: &std::panic::PanicHookInfo<'_>, backtrace: &str) -> String {
    let message = info
        .payload()
        .downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(keine Meldung)".into());
    let place = info.location().map_or_else(|| "?".into(), |l| format!("{}:{}", l.file(), l.line()));
    let thread = std::thread::current().name().unwrap_or("?").to_string();
    let recent = LOGGER
        .get()
        .and_then(|l| l.recent.lock().ok().map(|r| r.iter().cloned().collect::<Vec<_>>().join("\n")))
        .unwrap_or_default();
    format!(
        "MrPDF Absturzbericht / crash report\n\
         ===================================\n\
         Zeit:     {} UTC\n\
         Version:  {}\n\
         System:   {} {}\n\
         Thread:   {thread}\n\
         Stelle:   {place}\n\
         Meldung:  {message}\n\n\
         Ablauf (backtrace):\n{backtrace}\n\n\
         Letzte Protokollzeilen:\n{recent}\n",
        timestamp(SystemTime::now()),
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
}

fn install_panic_hook(dir: Option<PathBuf>) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        let text = report(info, &backtrace);
        log::error!("Absturz: {}", text.lines().nth(7).unwrap_or_default());
        if let Some(dir) = &dir {
            let stamp = timestamp(SystemTime::now()).replace([' ', ':'], "-");
            let path = dir.join(format!("absturz-{stamp}.txt"));
            if std::fs::write(&path, &text).is_ok() {
                let _ = std::fs::write(dir.join(MARKER), path.to_string_lossy().as_bytes());
            }
        }
        if let Some(l) = LOGGER.get() {
            log::Log::flush(l);
        }
        previous(info);
    }));
}

/// The crash report of the last run not shown yet (and forgets it).
pub fn take_crash_notice() -> Option<PathBuf> {
    let marker = log_dir()?.join(MARKER);
    let text = std::fs::read_to_string(&marker).ok()?;
    let _ = std::fs::remove_file(&marker);
    let path = PathBuf::from(text.trim());
    path.is_file().then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_in_utc() {
        assert_eq!(timestamp(UNIX_EPOCH), "1970-01-01 00:00:00");
        // 2026-11-04 16:03:12 UTC
        assert_eq!(timestamp(UNIX_EPOCH + Duration::from_secs(1_793_808_192)), "2026-11-04 16:03:12");
        // Leap day.
        assert_eq!(timestamp(UNIX_EPOCH + Duration::from_secs(951_782_400)), "2000-02-29 00:00:00");
    }

    #[test]
    fn only_file_names() {
        assert_eq!(file_name(Path::new("/home/someone/Briefe/a.pdf")), "a.pdf");
    }
}
