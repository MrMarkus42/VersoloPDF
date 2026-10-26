//! Printing.
//!
//! MrPDF prepares the sheets itself (see [`crate::print_layout`]) and shows
//! its own print dialog. The finished sheets are sent to the printer:
//!
//! * Windows: rendered by PDFium and drawn through GDI on a printer device
//!   context; the printer driver's own settings dialog ("Einstellungen…")
//!   is available.
//! * Linux / macOS: handed to the CUPS `lp` command.

use crate::document::Document;
#[cfg(unix)]
use crate::error::Error;
use crate::error::Result;
#[cfg(unix)]
use crate::print_layout::{Duplex, Paper, PrintSettings};

/// Formats zero-based page indices as a 1-based range list (`1-3,5`).
pub fn format_ranges(pages: &[usize]) -> String {
    let mut sorted: Vec<usize> = pages.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let mut parts = Vec::new();
    let mut i = 0;
    while i < sorted.len() {
        let start = sorted[i];
        let mut end = start;
        while i + 1 < sorted.len() && sorted[i + 1] == end + 1 {
            i += 1;
            end = sorted[i];
        }
        parts.push(if start == end {
            format!("{}", start + 1)
        } else {
            format!("{}-{}", start + 1, end + 1)
        });
        i += 1;
    }
    parts.join(",")
}

/// A printer known to the system.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PrinterInfo {
    pub name: String,
    /// Driver / type.
    pub driver: String,
    pub ready: bool,
    pub is_default: bool,
}

fn sanitize_title(name: &str) -> String {
    name.chars().filter(|c| !c.is_control()).take(100).collect::<String>()
}

/// The sheets as one PDF; with `as_image` every sheet becomes a picture.
#[cfg_attr(windows, allow(dead_code))]
fn sheet_pdf(job: &Document, sheets: &[usize], as_image: bool) -> Result<Vec<u8>> {
    let bytes = job.extract_pages_to_bytes(sheets)?;
    if !as_image {
        return Ok(bytes);
    }
    let mut doc = Document::from_bytes(bytes, "print.pdf", None)?;
    let pages: Vec<usize> = (0..doc.page_count()).collect();
    doc.contents_to_images(&pages, 300.0)?;
    doc.to_bytes()
}

// ---------------------------------------------------------------------------
// CUPS

#[cfg(unix)]
fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(cmd).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Printers of the system (CUPS destinations).
#[cfg(unix)]
pub fn printers() -> Vec<PrinterInfo> {
    let default = run("lpstat", &["-d"])
        .and_then(|s| s.rsplit_once(':').map(|(_, name)| name.trim().to_string()))
        .unwrap_or_default();
    let mut out: Vec<PrinterInfo> = run("lpstat", &["-e"])
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|name| PrinterInfo {
            name: name.to_string(),
            driver: "CUPS".into(),
            ready: true,
            is_default: name == default,
        })
        .collect();
    out.sort_by(|a, b| b.is_default.cmp(&a.is_default).then(a.name.cmp(&b.name)));
    out
}

/// Prints the given sheets of `job` with `lp`. Returns the job description.
#[cfg(unix)]
pub fn print_job(
    printer: &str,
    _devmode: Option<&[u8]>,
    settings: &PrintSettings,
    job: &Document,
    sheets: &[usize],
    title: &str,
    _paper: &Paper,
) -> Result<String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    if sheets.is_empty() {
        return Err(Error::Invalid("no pages selected".into()));
    }
    let bytes = sheet_pdf(job, sheets, settings.as_image)?;
    let sides = match settings.duplex {
        Duplex::Auto => None,
        Duplex::Simplex => Some("sides=one-sided"),
        Duplex::LongEdge => Some("sides=two-sided-long-edge"),
        Duplex::ShortEdge => Some("sides=two-sided-short-edge"),
    };
    let mut cmd = Command::new("lp");
    if !printer.is_empty() {
        cmd.arg("-d").arg(printer);
    }
    cmd.arg("-n").arg(settings.copies.clamp(1, 999).to_string());
    cmd.arg("-o").arg(if settings.collate { "collate=true" } else { "collate=false" });
    if let Some(sides) = sides {
        cmd.arg("-o").arg(sides);
    }
    let mut child = cmd
        .arg("-t")
        .arg(sanitize_title(title))
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::Invalid(format!("lp: {e}")))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(&bytes).map_err(|e| Error::Invalid(format!("lp: {e}")))?;
    }
    let out = child.wait_with_output().map_err(|e| Error::Invalid(format!("lp: {e}")))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(Error::Invalid(String::from_utf8_lossy(&out.stderr).trim().to_string()))
    }
}

/// Paper of the printer: without a driver interface the chosen standard
/// paper without margins.
#[cfg(unix)]
pub fn printer_paper(_printer: &str, _devmode: Option<&[u8]>, fallback: Paper) -> Paper {
    fallback
}

// ---------------------------------------------------------------------------
// Windows

#[cfg(windows)]
pub use windows_print::{default_devmode, print_job, printer_paper, printer_properties, printers};

#[cfg(windows)]
#[allow(unsafe_code)]
mod windows_print {
    //! GDI printing. All `unsafe` blocks only call documented Win32 APIs with
    //! correctly sized, initialised structures and check every result.

    use windows_sys::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateDCW, CreateICW, DEVMODEW, DIB_RGB_COLORS, DM_COLLATE,
        DM_COPIES, DM_DUPLEX, DM_IN_BUFFER, DM_IN_PROMPT, DM_ORIENTATION, DM_OUT_BUFFER, DM_PAPERLENGTH,
        DM_PAPERWIDTH, DMCOLLATE_FALSE, DMCOLLATE_TRUE, DMDUP_HORIZONTAL, DMDUP_SIMPLEX, DMDUP_VERTICAL,
        DMORIENT_LANDSCAPE, DMORIENT_PORTRAIT, DeleteDC, GetDeviceCaps, HDC, HORZRES, LOGPIXELSX, LOGPIXELSY,
        PHYSICALHEIGHT, PHYSICALOFFSETX, PHYSICALOFFSETY, PHYSICALWIDTH, ResetDCW, SRCCOPY, StretchDIBits,
        VERTRES,
    };
    use windows_sys::Win32::Graphics::Printing::{
        ClosePrinter, DocumentPropertiesW, EnumPrintersW, GetDefaultPrinterW, OpenPrinterW,
        PRINTER_ENUM_CONNECTIONS, PRINTER_ENUM_LOCAL, PRINTER_HANDLE, PRINTER_INFO_2W, PRINTER_STATUS_ERROR,
        PRINTER_STATUS_OFFLINE, PRINTER_STATUS_PAUSED,
    };
    use windows_sys::Win32::Storage::Xps::{AbortDoc, DOCINFOW, EndDoc, EndPage, StartDocW, StartPage};

    use super::{PrinterInfo, sanitize_title};
    use crate::document::Document;
    use crate::error::{Error, Result};
    use crate::geometry::RectF;
    use crate::print_layout::{Duplex, Paper, PrintSettings};

    /// Upper limit of the effective print resolution (memory protection).
    const MAX_DPI: f32 = 300.0;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Reads a zero terminated UTF-16 string.
    fn from_wide(p: *const u16) -> String {
        if p.is_null() {
            return String::new();
        }
        let mut len = 0;
        // SAFETY: Windows returns zero terminated strings; the scan stops at 0.
        unsafe {
            while *p.add(len) != 0 && len < 32_768 {
                len += 1;
            }
            String::from_utf16_lossy(std::slice::from_raw_parts(p, len))
        }
    }

    struct Dc(HDC);

    impl Drop for Dc {
        fn drop(&mut self) {
            // SAFETY: the handle was created by CreateDCW/CreateICW and is deleted once.
            unsafe { DeleteDC(self.0) };
        }
    }

    struct Printer(PRINTER_HANDLE);

    impl Printer {
        fn open(name: &str) -> Option<Self> {
            let name = wide(name);
            let mut handle = PRINTER_HANDLE::default();
            // SAFETY: `name` is zero terminated, `handle` receives the result.
            let ok = unsafe { OpenPrinterW(name.as_ptr(), &mut handle, std::ptr::null()) };
            (ok != 0 && !handle.Value.is_null()).then_some(Self(handle))
        }
    }

    impl Drop for Printer {
        fn drop(&mut self) {
            // SAFETY: the handle was opened by OpenPrinterW and is closed once.
            unsafe { ClosePrinter(self.0) };
        }
    }

    /// A DEVMODEW buffer with correct alignment (stored as bytes elsewhere).
    struct DevMode(Vec<u64>);

    impl DevMode {
        fn from_bytes(bytes: &[u8]) -> Option<Self> {
            if bytes.len() < std::mem::size_of::<DEVMODEW>() {
                return None;
            }
            let mut buf = vec![0u64; bytes.len().div_ceil(8)];
            // SAFETY: the buffer holds at least `bytes.len()` bytes.
            unsafe {
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf.as_mut_ptr().cast::<u8>(), bytes.len())
            };
            Some(Self(buf))
        }

        fn ptr(&self) -> *const DEVMODEW {
            self.0.as_ptr().cast()
        }

        fn ptr_mut(&mut self) -> *mut DEVMODEW {
            self.0.as_mut_ptr().cast()
        }

        fn size(&self) -> usize {
            // SAFETY: the buffer starts with a DEVMODEW written by the driver.
            let dm = unsafe { &*self.ptr() };
            usize::from(dm.dmSize) + usize::from(dm.dmDriverExtra)
        }

        fn bytes(&self) -> Vec<u8> {
            let len = self.size().min(self.0.len() * 8);
            // SAFETY: `len` is within the buffer.
            unsafe { std::slice::from_raw_parts(self.0.as_ptr().cast::<u8>(), len).to_vec() }
        }

        /// Copies, collation, duplex and (optionally) a paper size.
        fn apply(&mut self, s: &PrintSettings, paper_mm10: Option<(i16, i16)>) {
            // SAFETY: the buffer starts with a valid DEVMODEW; the union member
            // for printers (Anonymous1) is the one used by print drivers.
            unsafe {
                let dm = &mut *self.ptr_mut();
                dm.Anonymous1.Anonymous1.dmCopies = s.copies.clamp(1, 999) as i16;
                dm.dmCollate = if s.collate { DMCOLLATE_TRUE } else { DMCOLLATE_FALSE };
                dm.dmFields |= DM_COPIES | DM_COLLATE;
                let duplex = match s.duplex {
                    Duplex::Auto => None,
                    Duplex::Simplex => Some(DMDUP_SIMPLEX),
                    Duplex::LongEdge => Some(DMDUP_VERTICAL),
                    Duplex::ShortEdge => Some(DMDUP_HORIZONTAL),
                };
                if let Some(d) = duplex {
                    dm.dmDuplex = d;
                    dm.dmFields |= DM_DUPLEX;
                }
                if let Some((w, h)) = paper_mm10 {
                    dm.Anonymous1.Anonymous1.dmPaperWidth = w;
                    dm.Anonymous1.Anonymous1.dmPaperLength = h;
                    dm.Anonymous1.Anonymous1.dmOrientation = DMORIENT_PORTRAIT as i16;
                    dm.dmFields |= DM_PAPERWIDTH | DM_PAPERLENGTH | DM_ORIENTATION;
                }
            }
        }

        fn orientation(&mut self, landscape: bool) {
            // SAFETY: as in `apply`.
            unsafe {
                let dm = &mut *self.ptr_mut();
                dm.Anonymous1.Anonymous1.dmOrientation =
                    if landscape { DMORIENT_LANDSCAPE as i16 } else { DMORIENT_PORTRAIT as i16 };
                dm.dmFields |= DM_ORIENTATION;
            }
        }
    }

    fn default_printer_name() -> String {
        let mut len = 0u32;
        // SAFETY: first call asks for the needed length.
        unsafe { GetDefaultPrinterW(std::ptr::null_mut(), &mut len) };
        if len == 0 || len > 4096 {
            return String::new();
        }
        let mut buf = vec![0u16; len as usize];
        // SAFETY: the buffer has `len` characters.
        if unsafe { GetDefaultPrinterW(buf.as_mut_ptr(), &mut len) } == 0 {
            return String::new();
        }
        from_wide(buf.as_ptr())
    }

    /// Local and connected printers; the default printer first.
    pub fn printers() -> Vec<PrinterInfo> {
        let flags = PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS;
        let (mut needed, mut count) = (0u32, 0u32);
        // SAFETY: first call asks for the buffer size.
        unsafe {
            EnumPrintersW(flags, std::ptr::null(), 2, std::ptr::null_mut(), 0, &mut needed, &mut count)
        };
        if needed == 0 || needed > 16 * 1024 * 1024 {
            return Vec::new();
        }
        let mut buf = vec![0u64; (needed as usize).div_ceil(8)];
        // SAFETY: the buffer has `needed` bytes and is 8-byte aligned.
        let ok = unsafe {
            EnumPrintersW(
                flags,
                std::ptr::null(),
                2,
                buf.as_mut_ptr().cast(),
                needed,
                &mut needed,
                &mut count,
            )
        };
        if ok == 0 {
            return Vec::new();
        }
        let default = default_printer_name();
        // SAFETY: the driver filled `count` PRINTER_INFO_2W structures.
        let infos =
            unsafe { std::slice::from_raw_parts(buf.as_ptr().cast::<PRINTER_INFO_2W>(), count as usize) };
        let mut out: Vec<PrinterInfo> = infos
            .iter()
            .map(|p| {
                let name = from_wide(p.pPrinterName);
                PrinterInfo {
                    is_default: name == default,
                    driver: from_wide(p.pDriverName),
                    ready: p.Status & (PRINTER_STATUS_ERROR | PRINTER_STATUS_OFFLINE | PRINTER_STATUS_PAUSED)
                        == 0,
                    name,
                }
            })
            .collect();
        out.sort_by(|a, b| b.is_default.cmp(&a.is_default).then(a.name.cmp(&b.name)));
        out
    }

    fn document_properties(name: &str, input: Option<&DevMode>, prompt: bool) -> Option<Vec<u8>> {
        let printer = Printer::open(name)?;
        let wname = wide(name);
        // SAFETY: size query with null buffers.
        let size = unsafe {
            DocumentPropertiesW(
                std::ptr::null_mut(),
                printer.0,
                wname.as_ptr(),
                std::ptr::null_mut(),
                std::ptr::null(),
                0,
            )
        };
        if size <= 0 || size > 1024 * 1024 {
            return None;
        }
        let mut out = DevMode(vec![0u64; (size as usize).div_ceil(8)]);
        let mut mode = DM_OUT_BUFFER;
        let in_ptr = match input {
            Some(dm) => {
                mode |= DM_IN_BUFFER;
                dm.ptr()
            }
            None => std::ptr::null(),
        };
        if prompt {
            mode |= DM_IN_PROMPT;
        }
        // SAFETY: the output buffer has `size` bytes; the input (if any) is a
        // DEVMODEW of this printer.
        let result = unsafe {
            DocumentPropertiesW(std::ptr::null_mut(), printer.0, wname.as_ptr(), out.ptr_mut(), in_ptr, mode)
        };
        // IDOK = 1; IDCANCEL = 2 (only with a prompt).
        (result == 1).then(|| out.bytes())
    }

    /// The printer's current default settings (DEVMODE bytes).
    pub fn default_devmode(printer: &str) -> Option<Vec<u8>> {
        document_properties(printer, None, false)
    }

    /// Shows the driver's settings dialog; returns the new settings or None
    /// if cancelled.
    pub fn printer_properties(printer: &str, devmode: Option<&[u8]>) -> Option<Vec<u8>> {
        let input = devmode.and_then(DevMode::from_bytes);
        document_properties(printer, input.as_ref(), true)
    }

    fn create_dc(printer: &str, dm: Option<&DevMode>, info_only: bool) -> Option<Dc> {
        let driver = wide("WINSPOOL");
        let device = wide(printer);
        let pdm = dm.map_or(std::ptr::null(), DevMode::ptr);
        // SAFETY: all strings are zero terminated; pdm is null or a DEVMODEW.
        let hdc = unsafe {
            if info_only {
                CreateICW(driver.as_ptr(), device.as_ptr(), std::ptr::null(), pdm)
            } else {
                CreateDCW(driver.as_ptr(), device.as_ptr(), std::ptr::null(), pdm)
            }
        };
        (!hdc.is_null()).then_some(Dc(hdc))
    }

    struct Caps {
        dpi: (f32, f32),
        /// Paper in device pixels.
        physical: (f32, f32),
        offset: (f32, f32),
        printable: (f32, f32),
    }

    fn caps(dc: &Dc) -> Option<Caps> {
        // SAFETY: querying device capabilities of a valid DC.
        let v = |i: u32| unsafe { GetDeviceCaps(dc.0, i as i32) } as f32;
        let caps = Caps {
            dpi: (v(LOGPIXELSX), v(LOGPIXELSY)),
            physical: (v(PHYSICALWIDTH), v(PHYSICALHEIGHT)),
            offset: (v(PHYSICALOFFSETX), v(PHYSICALOFFSETY)),
            printable: (v(HORZRES), v(VERTRES)),
        };
        (caps.dpi.0 > 0.0 && caps.dpi.1 > 0.0 && caps.physical.0 > 0.0 && caps.physical.1 > 0.0)
            .then_some(caps)
    }

    /// Paper size and printable area of the printer with its settings.
    pub fn printer_paper(printer: &str, devmode: Option<&[u8]>, fallback: Paper) -> Paper {
        let dm = devmode.and_then(DevMode::from_bytes);
        let Some(dc) = create_dc(printer, dm.as_ref(), true) else { return fallback };
        let Some(c) = caps(&dc) else { return fallback };
        let pt = |px: f32, dpi: f32| px / dpi * 72.0;
        let (w, h) = (pt(c.physical.0, c.dpi.0), pt(c.physical.1, c.dpi.1));
        let x0 = pt(c.offset.0, c.dpi.0);
        let top = pt(c.offset.1, c.dpi.1);
        let (pw, ph) = (pt(c.printable.0, c.dpi.0), pt(c.printable.1, c.dpi.1));
        Paper { size: (w, h), printable: RectF::new(x0, h - top - ph, x0 + pw, h - top) }
    }

    /// Prints the given sheets of `job`.
    #[allow(clippy::too_many_arguments)]
    pub fn print_job(
        printer: &str,
        devmode: Option<&[u8]>,
        settings: &PrintSettings,
        job: &Document,
        sheets: &[usize],
        title: &str,
        paper: &Paper,
    ) -> Result<String> {
        if sheets.is_empty() {
            return Err(Error::Invalid("no pages selected".into()));
        }
        let mut dm = devmode
            .and_then(DevMode::from_bytes)
            .or_else(|| super::default_devmode(printer).as_deref().and_then(DevMode::from_bytes))
            .ok_or_else(|| Error::Invalid(format!("printer not available: {printer}")))?;
        dm.apply(settings, None);
        let dc =
            create_dc(printer, Some(&dm), false).ok_or_else(|| Error::Invalid("CreateDC failed".into()))?;
        let name = wide(&sanitize_title(title));
        // SAFETY: DOCINFOW is a plain C struct; zero is a valid initial state.
        let mut info: DOCINFOW = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<DOCINFOW>() as i32;
        info.lpszDocName = name.as_ptr();
        // SAFETY: `info` and `name` outlive the call; dc is a valid printer DC.
        if unsafe { StartDocW(dc.0, &info) } <= 0 {
            return Err(Error::Invalid("StartDoc failed".into()));
        }
        let paper_landscape = paper.size.0 > paper.size.1;
        let result = (|| -> Result<()> {
            for &sheet in sheets {
                let geo = job.geometry(sheet).ok_or(Error::PageOutOfRange(sheet))?;
                let (sw, sh) = geo.view_size();
                let own_size = settings.paper_from_page;
                if own_size {
                    // Paper of the sheet's size (tenths of a millimetre).
                    let to = |pt: f32| ((pt / 72.0 * 254.0).round() as i32).clamp(100, 32_000) as i16;
                    let (a, b) = (sw.min(sh), sw.max(sh));
                    dm.apply(settings, Some((to(a), to(b))));
                    dm.orientation(sw > sh);
                    // SAFETY: valid DC and DEVMODEW.
                    unsafe { ResetDCW(dc.0, dm.ptr()) };
                }
                let c = caps(&dc).ok_or_else(|| Error::Invalid("invalid printer resolution".into()))?;
                // Landscape sheets on portrait paper (booklets) are turned.
                let turn = !own_size && (sw > sh) != paper_landscape && (sw - sh).abs() > 1.0;
                let (pw_pt, ph_pt) = if turn { (sh, sw) } else { (sw, sh) };
                let dev_w = pw_pt / 72.0 * c.dpi.0;
                let dev_h = ph_pt / 72.0 * c.dpi.1;
                let bmp_scale = (MAX_DPI / c.dpi.0.max(c.dpi.1)).min(1.0);
                let edge = crate::document::MAX_BITMAP_EDGE;
                let (bw, bh) = (
                    ((sw / 72.0 * c.dpi.0 * bmp_scale).round() as u32).clamp(1, edge),
                    ((sh / 72.0 * c.dpi.1 * bmp_scale).round() as u32).clamp(1, edge),
                );
                let opts = crate::document::RenderOptions { printing: true, ..Default::default() };
                let img = job.render_tile(sheet, bw, bh, 0, 0, bw, bh, opts)?;
                let mut rgba = image::RgbaImage::from_raw(img.width, img.height, img.rgba)
                    .ok_or_else(|| Error::Invalid("bitmap size mismatch".into()))?;
                if turn {
                    rgba = image::imageops::rotate270(&rgba);
                }
                let (iw, ih) = rgba.dimensions();
                let mut bgra = rgba.into_raw();
                for px in bgra.as_chunks_mut::<4>().0 {
                    px.swap(0, 2);
                }
                // SAFETY: plain C struct, zero initialised and then filled in.
                let mut bmi: BITMAPINFO = unsafe { std::mem::zeroed() };
                bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
                bmi.bmiHeader.biWidth = iw as i32;
                bmi.bmiHeader.biHeight = -(ih as i32); // top-down rows
                bmi.bmiHeader.biPlanes = 1;
                bmi.bmiHeader.biBitCount = 32;
                bmi.bmiHeader.biCompression = BI_RGB;
                // Device origin is the printable area's corner.
                let (x, y) = (-c.offset.0 as i32, -c.offset.1 as i32);
                // SAFETY: valid DC; `bgra` holds iw*ih*4 bytes as described by `bmi`.
                unsafe {
                    if StartPage(dc.0) <= 0 {
                        return Err(Error::Invalid("StartPage failed".into()));
                    }
                    let lines = StretchDIBits(
                        dc.0,
                        x,
                        y,
                        dev_w as i32,
                        dev_h as i32,
                        0,
                        0,
                        iw as i32,
                        ih as i32,
                        bgra.as_ptr().cast(),
                        &bmi,
                        DIB_RGB_COLORS,
                        SRCCOPY,
                    );
                    if EndPage(dc.0) <= 0 || lines == 0 {
                        return Err(Error::Invalid("printing a page failed".into()));
                    }
                }
            }
            Ok(())
        })();
        // SAFETY: finishing or aborting the document started above.
        unsafe {
            if result.is_ok() {
                EndDoc(dc.0);
            } else {
                AbortDoc(dc.0);
            }
        }
        result.map(|()| format!("{} → {printer}", sheets.len()))
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn printers_can_be_listed() {
            // Works with and without installed printers.
            for p in super::printers() {
                assert!(!p.name.is_empty());
            }
        }

        #[test]
        fn paper_of_the_default_printer() {
            // Only reads settings; nothing is printed.
            let Some(p) = super::printers().into_iter().find(|p| p.is_default && p.ready) else { return };
            let dm = super::default_devmode(&p.name);
            assert!(
                dm.as_ref().is_some_and(|d| d.len() >= std::mem::size_of::<super::DEVMODEW>()),
                "{}",
                p.name
            );
            let fallback = crate::print_layout::Paper::full(1.0, 1.0);
            let paper = super::printer_paper(&p.name, dm.as_deref(), fallback);
            assert!(paper.size.0 > 100.0 && paper.size.1 > 100.0, "{} {paper:?}", p.name);
            let r = paper.printable;
            assert!(
                r.x0 >= 0.0 && r.y0 >= -0.5 && r.x1 <= paper.size.0 + 0.5 && r.y1 <= paper.size.1 + 0.5,
                "{paper:?}"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_are_compressed() {
        assert_eq!(format_ranges(&[0, 1, 2, 4, 6, 7]), "1-3,5,7-8");
        assert_eq!(format_ranges(&[3]), "4");
        assert_eq!(format_ranges(&[2, 0, 1, 1]), "1-3");
        assert_eq!(format_ranges(&[]), "");
    }

    #[test]
    fn titles_are_cleaned() {
        assert_eq!(sanitize_title("a\nb"), "ab");
    }
}
