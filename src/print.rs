//! Printing.
//!
//! * Windows: the standard print dialog is shown and pages are rendered by
//!   PDFium and sent to the printer through GDI.
//! * Linux / macOS: the PDF is handed to the CUPS `lp` command.

#[cfg(unix)]
use crate::document::Document;
#[cfg(unix)]
use crate::error::{Error, Result};

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

/// Prints `pages` through CUPS. Returns the job description printed by `lp`.
#[cfg(unix)]
pub fn print_pages(doc: &mut Document, pages: &[usize], copies: u32) -> Result<String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    if pages.is_empty() {
        return Err(Error::Invalid("no pages selected".into()));
    }
    if !doc.permissions().print {
        return Err(Error::NotPermitted);
    }
    let bytes = doc.to_bytes()?;
    let mut child = Command::new("lp")
        .arg("-n")
        .arg(copies.clamp(1, 99).to_string())
        .arg("-P")
        .arg(format_ranges(pages))
        .arg("-t")
        .arg(sanitize_title(doc.display_name()))
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

#[cfg(unix)]
fn sanitize_title(name: &str) -> String {
    name.chars().filter(|c| !c.is_control()).take(100).collect::<String>()
}

#[cfg(windows)]
pub use windows_print::print_with_dialog;

#[cfg(windows)]
#[allow(unsafe_code)]
mod windows_print {
    //! GDI printing. All `unsafe` blocks only call documented Win32 APIs with
    //! correctly sized, initialised structures and check every result.

    use windows_sys::Win32::Foundation::{GlobalFree, HGLOBAL};
    use windows_sys::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteDC, GetDeviceCaps, HDC, HORZRES,
        LOGPIXELSX, LOGPIXELSY, SRCCOPY, StretchDIBits, VERTRES,
    };
    use windows_sys::Win32::Storage::Xps::{AbortDoc, DOCINFOW, EndDoc, EndPage, StartDocW, StartPage};
    use windows_sys::Win32::UI::Controls::Dialogs::{
        PD_ALLPAGES, PD_NOSELECTION, PD_PAGENUMS, PD_RETURNDC, PD_USEDEVMODECOPIESANDCOLLATE, PRINTDLGW,
        PrintDlgW,
    };

    use crate::document::Document;
    use crate::error::{Error, Result};

    /// Upper limit of the effective print resolution (memory protection).
    const MAX_DPI: f32 = 300.0;

    struct Dc(HDC);

    impl Drop for Dc {
        fn drop(&mut self) {
            // SAFETY: the handle was returned by PrintDlgW and is deleted once.
            unsafe { DeleteDC(self.0) };
        }
    }

    fn free(handle: HGLOBAL) {
        if !handle.is_null() {
            // SAFETY: handles returned by PrintDlgW are owned by the caller.
            unsafe { GlobalFree(handle) };
        }
    }

    /// Shows the system print dialog and prints. Returns `Ok(false)` if the
    /// user cancelled.
    pub fn print_with_dialog(doc: &Document) -> Result<bool> {
        let count = doc.page_count();
        if count == 0 {
            return Ok(false);
        }
        if !doc.permissions().print {
            return Err(Error::NotPermitted);
        }
        let max_page = u16::try_from(count).unwrap_or(u16::MAX);
        // SAFETY: PRINTDLGW is a plain C struct; zero is a valid initial state.
        let mut pd: PRINTDLGW = unsafe { std::mem::zeroed() };
        pd.lStructSize = std::mem::size_of::<PRINTDLGW>() as u32;
        pd.Flags = PD_RETURNDC | PD_ALLPAGES | PD_NOSELECTION | PD_USEDEVMODECOPIESANDCOLLATE;
        pd.nMinPage = 1;
        pd.nMaxPage = max_page;
        pd.nFromPage = 1;
        pd.nToPage = max_page;
        pd.nCopies = 1;
        // SAFETY: `pd` is fully initialised and lives for the duration of the call.
        let ok = unsafe { PrintDlgW(&mut pd) };
        free(pd.hDevMode);
        free(pd.hDevNames);
        if ok == 0 {
            if !pd.hDC.is_null() {
                drop(Dc(pd.hDC));
            }
            return Ok(false);
        }
        if pd.hDC.is_null() {
            return Err(Error::Invalid("no printer device".into()));
        }
        let dc = Dc(pd.hDC);
        let (from, to) = if pd.Flags & PD_PAGENUMS != 0 {
            (usize::from(pd.nFromPage.max(1)) - 1, usize::from(pd.nToPage).min(count) - 1)
        } else {
            (0, count - 1)
        };
        print_range(doc, &dc, from, to)?;
        Ok(true)
    }

    fn print_range(doc: &Document, dc: &Dc, from: usize, to: usize) -> Result<()> {
        let title: Vec<u16> = doc.display_name().encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: DOCINFOW is a plain C struct; zero is a valid initial state.
        let mut info: DOCINFOW = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<DOCINFOW>() as i32;
        info.lpszDocName = title.as_ptr();
        // SAFETY: `info` and `title` outlive the call; dc is a valid printer DC.
        if unsafe { StartDocW(dc.0, &info) } <= 0 {
            return Err(Error::Invalid("StartDoc failed".into()));
        }
        let result = (|| -> Result<()> {
            // SAFETY: querying device capabilities of a valid DC.
            let (res_x, res_y, dpi_x, dpi_y) = unsafe {
                (
                    GetDeviceCaps(dc.0, HORZRES as i32),
                    GetDeviceCaps(dc.0, VERTRES as i32),
                    GetDeviceCaps(dc.0, LOGPIXELSX as i32),
                    GetDeviceCaps(dc.0, LOGPIXELSY as i32),
                )
            };
            if res_x <= 0 || res_y <= 0 || dpi_x <= 0 || dpi_y <= 0 {
                return Err(Error::Invalid("invalid printer resolution".into()));
            }
            for page in from..=to {
                let geo = doc.geometry(page).ok_or(Error::PageOutOfRange(page))?;
                let (w_pt, h_pt) = geo.view_size();
                // Target size on paper in device pixels, shrunk to the printable area.
                let mut dev_w = w_pt / 72.0 * dpi_x as f32;
                let mut dev_h = h_pt / 72.0 * dpi_y as f32;
                let fit = (res_x as f32 / dev_w).min(res_y as f32 / dev_h).min(1.0);
                dev_w *= fit;
                dev_h *= fit;
                // Bitmap resolution capped to MAX_DPI.
                let bmp_scale = (MAX_DPI / dpi_x.max(dpi_y) as f32).min(1.0);
                let bw = ((dev_w * bmp_scale).round() as u32).clamp(1, crate::document::MAX_BITMAP_EDGE);
                let bh = ((dev_h * bmp_scale).round() as u32).clamp(1, crate::document::MAX_BITMAP_EDGE);
                let img = doc.render_page(page, bw, bh)?;
                let mut bgra = img.rgba;
                for px in bgra.as_chunks_mut::<4>().0 {
                    px.swap(0, 2);
                }
                // SAFETY: plain C struct, zero initialised and then filled in.
                let mut bmi: BITMAPINFO = unsafe { std::mem::zeroed() };
                bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
                bmi.bmiHeader.biWidth = bw as i32;
                bmi.bmiHeader.biHeight = -(bh as i32); // top-down rows
                bmi.bmiHeader.biPlanes = 1;
                bmi.bmiHeader.biBitCount = 32;
                bmi.bmiHeader.biCompression = BI_RGB;
                let x = ((res_x as f32 - dev_w) / 2.0) as i32;
                let y = ((res_y as f32 - dev_h) / 2.0) as i32;
                // SAFETY: valid DC; `bgra` holds bw*bh*4 bytes as described by `bmi`.
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
                        bw as i32,
                        bh as i32,
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
        result
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
}
