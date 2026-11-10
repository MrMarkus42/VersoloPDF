//! An open PDF document: rendering, inspection and editing.
//!
//! The document in the PDF engine (see [`crate::engine`]; in protected mode
//! a separate, restricted helper process) is always the authoritative
//! state. Edits that PDFium supports natively (page rotation, page
//! insertion/removal) are applied there directly; structural edits
//! (annotations, page order, metadata) are applied to the serialised bytes
//! by [`crate::pdf_edit`] and the result is loaded again. Every edit records
//! an undo snapshot.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;

use crate::engine::EngineDoc;
use crate::engine::proto::{Bytes, DrawSpec, LoadInfo, Picture, Request};
use crate::error::{Error, Result};
use crate::forms::{self, FieldValue};
use crate::geometry::{PageGeometry, PointF, RectF, Rotation};
use crate::model::{
    AnnotFilter, AnnotKind, AnnotStyle, AnnotationInfo, Bookmark, FormFieldInfo, LinkInfo, Metadata,
    Permissions, SearchOptions,
};
use crate::outline;
use crate::pages;
use crate::pdf_edit::{self, AnnotDetails, AnnotUpdate, NewAnnotation};
use crate::security::{self, ProtectOptions, SecurityInfo};
use crate::text::{CharInfo, PageText};
use crate::xfdf;

mod bookmarks;
mod content;
pub use bookmarks::{LinkOptions, TextBookmarkOptions, TocLevel, TocOptions, TocPageNumbers};
pub use content::{ClipItem, ObjectClip, PathSeg};

/// Files larger than this are refused to protect against memory exhaustion.
pub const MAX_FILE_SIZE: u64 = 1 << 31;

/// Largest bitmap edge length that will be rendered.
pub const MAX_BITMAP_EDGE: u32 = 16_384;

const MAX_UNDO_STEPS: usize = 50;
const MAX_UNDO_BYTES: usize = 512 * 1024 * 1024;
/// Undo steps moved into temporary files may take this much disk space.
const MAX_UNDO_DISK: usize = 4 * 1024 * 1024 * 1024;
const MAX_BOOKMARKS: usize = 20_000;
const MAX_BOOKMARK_DEPTH: usize = 32;

/// Display options for rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderOptions {
    /// Additional clockwise rotation (view rotation).
    pub rotation: Rotation,
    /// Draw annotations and form fields.
    pub annotations: bool,
    /// Page background (RGB) behind transparent content.
    pub background: [u8; 3],
    /// Render as for a printer (annotations without the print flag hidden).
    pub printing: bool,
    /// "Farben im Dokument ersetzen": (background, text) colours; light
    /// areas become the background colour, dark areas the text colour.
    pub recolor: Option<([u8; 3], [u8; 3])>,
    /// No paper colour behind the content ("Transparenzgitter").
    pub transparent: bool,
    /// Apply the document's comment filter (see
    /// [`Document::set_annot_filter`]); the value is its serial number so
    /// render caches notice changes. 0 = no filter.
    pub filter: u32,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            rotation: Rotation::R0,
            annotations: true,
            background: [255, 255, 255],
            printing: false,
            recolor: None,
            transparent: false,
            filter: 0,
        }
    }
}

/// Maps the brightness of every pixel between `text` (dark) and
/// `background` (light); used for "Farben im Dokument ersetzen".
pub fn recolor_rgba(rgba: &mut [u8], background: [u8; 3], text: [u8; 3]) {
    for px in rgba.as_chunks_mut::<4>().0 {
        let l = (299 * px[0] as u32 + 587 * px[1] as u32 + 114 * px[2] as u32) / 1000;
        for c in 0..3 {
            let (fg, bg) = (text[c] as i32, background[c] as i32);
            px[c] = (fg + (bg - fg) * l as i32 / 255).clamp(0, 255) as u8;
        }
    }
}

/// What happens with comments or form fields when pages are extracted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Handling {
    #[default]
    Copy,
    /// Burn into the page content ("Einbetten").
    Flatten,
    Remove,
}

/// Which bookmarks go into extracted files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum BookmarkCopy {
    All,
    /// Only bookmarks pointing to extracted pages ("Zugehörige kopieren").
    #[default]
    Related,
    None,
}

/// Options of [`Document::extract_pages_with`] ("Einstellungen" of the
/// extract dialog).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct ExtractOptions {
    pub comments: Handling,
    pub forms: Handling,
    pub bookmarks: BookmarkCopy,
}

/// Result of [`Document::word_count`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WordCount {
    pub pages: usize,
    pub words: usize,
    pub characters: usize,
    pub characters_no_spaces: usize,
}

/// A rendered page (or part of a page) as straight RGBA pixels.
#[derive(Debug, Clone)]
pub struct RenderedImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Folder for undo steps moved out of memory.
fn spill_dir() -> PathBuf {
    std::env::temp_dir().join("versolopdf-undo")
}

/// Removes undo files left behind by earlier runs (for example after a
/// crash). Files still in use stay: Windows refuses to delete open files,
/// and on other systems the running program reads them through its open
/// handle.
pub fn clean_old_undo_files() {
    if let Ok(entries) = std::fs::read_dir(spill_dir()) {
        for entry in entries.flatten() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// An undo step written to a temporary file; the file is deleted with it.
struct SpillFile {
    file: Option<std::fs::File>,
    path: PathBuf,
    len: usize,
}

impl SpillFile {
    fn write(bytes: &[u8]) -> std::io::Result<Self> {
        use std::io::Write;
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = spill_dir();
        std::fs::create_dir_all(&dir)?;
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = dir.join(format!("{}-{n}.pdf", std::process::id()));
        let file = std::fs::OpenOptions::new().read(true).write(true).create_new(true).open(&path)?;
        let spill = Self { file: Some(file), path, len: bytes.len() };
        // On an error `spill` is dropped, which deletes the half-written file.
        spill.file.as_ref().map_or(Ok(()), |mut f| f.write_all(bytes))?;
        Ok(spill)
    }

    fn read(&self) -> std::io::Result<Vec<u8>> {
        use std::io::{Read, Seek, SeekFrom};
        let mut file = self.file.as_ref().ok_or(std::io::ErrorKind::NotFound)?;
        file.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::with_capacity(self.len);
        file.read_to_end(&mut bytes)?;
        Ok(bytes)
    }
}

impl Drop for SpillFile {
    fn drop(&mut self) {
        // Close first: Windows cannot delete an open file.
        self.file = None;
        let _ = std::fs::remove_file(&self.path);
    }
}

/// The document of an undo step: in memory, or in a temporary file once the
/// steps in memory exceed [`MAX_UNDO_BYTES`].
enum Stored {
    Memory(Vec<u8>),
    Disk(SpillFile),
}

impl Stored {
    fn read(&self) -> Result<Vec<u8>> {
        match self {
            Stored::Memory(b) => Ok(b.clone()),
            Stored::Disk(f) => f.read().map_err(|e| Error::io(&f.path, e)),
        }
    }

    fn memory(&self) -> usize {
        match self {
            Stored::Memory(b) => b.len(),
            Stored::Disk(_) => 0,
        }
    }

    fn disk(&self) -> usize {
        match self {
            Stored::Memory(_) => 0,
            Stored::Disk(f) => f.len,
        }
    }
}

struct Snapshot {
    label: String,
    data: Stored,
}

impl Snapshot {
    fn new(label: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self { label: label.into(), data: Stored::Memory(bytes) }
    }
}

struct History {
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// Limits (fields so tests can use small ones).
    memory_limit: usize,
    disk_limit: usize,
}

impl Default for History {
    fn default() -> Self {
        Self { undo: Vec::new(), redo: Vec::new(), memory_limit: MAX_UNDO_BYTES, disk_limit: MAX_UNDO_DISK }
    }
}

impl History {
    fn push_undo(&mut self, snapshot: Snapshot) {
        self.undo.push(snapshot);
        self.redo.clear();
        self.trim();
    }

    /// Keeps the memory use of the steps below [`MAX_UNDO_BYTES`] by moving
    /// the oldest steps into temporary files; only steps beyond
    /// [`MAX_UNDO_STEPS`], or beyond [`MAX_UNDO_DISK`] on disk, are dropped.
    fn trim(&mut self) {
        while self.undo.len() > MAX_UNDO_STEPS {
            self.undo.remove(0);
        }
        let memory = |h: &History| h.undo.iter().chain(&h.redo).map(|s| s.data.memory()).sum::<usize>();
        // Oldest first: the undo steps from the bottom, then the redo steps
        // from the bottom (the furthest away).
        let mut next = 0;
        while memory(self) > self.memory_limit {
            let total = self.undo.len() + self.redo.len();
            let Some(i) = (next..total).find(|&i| {
                let s = if i < self.undo.len() { &self.undo[i] } else { &self.redo[i - self.undo.len()] };
                matches!(s.data, Stored::Memory(_))
            }) else {
                break;
            };
            next = i + 1;
            let slot =
                if i < self.undo.len() { &mut self.undo[i] } else { &mut self.redo[i - self.undo.len()] };
            let Stored::Memory(bytes) = &slot.data else { continue };
            match SpillFile::write(bytes) {
                Ok(file) => slot.data = Stored::Disk(file),
                // No room on disk: drop the oldest step as before.
                Err(_) => {
                    if !self.undo.is_empty() {
                        self.undo.remove(0);
                    }
                    next = 0;
                    if self.undo.is_empty() {
                        break;
                    }
                }
            }
        }
        let disk = |h: &History| h.undo.iter().chain(&h.redo).map(|s| s.data.disk()).sum::<usize>();
        while disk(self) > self.disk_limit && self.undo.len() > 1 {
            self.undo.remove(0);
        }
    }
}

/// Source of [`Document::instance`].
static NEXT_INSTANCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub struct Document {
    /// The document in the PDF engine.
    engine: EngineDoc,
    /// Page count, sizes and protection as the engine reported them.
    info: LoadInfo,
    /// Serialised state; stale while `engine_dirty` is set.
    bytes: Vec<u8>,
    engine_dirty: bool,
    /// The layers of `bytes` (None: not read yet). Edits through PDFium
    /// (comments, fields) never change layers, so this stays valid until
    /// the bytes are replaced.
    layers_cache: Option<Vec<crate::layers::Layer>>,
    path: Option<PathBuf>,
    display_name: String,
    password: Option<String>,
    geometry: Vec<PageGeometry>,
    history: History,
    revision: u64,
    instance: u64,
    /// Revision of the last change that may affect every page; changes of
    /// single pages (comments) are kept in page_revs so the other pages
    /// keep their rendered pictures.
    structure_rev: u64,
    page_revs: HashMap<usize, u64>,
    /// Set while an edit only touches this page.
    local_page: Option<usize>,
    modified: bool,
    /// Security handler details of encrypted documents.
    security: Option<SecurityInfo>,
    /// Display filter for comments and its serial number.
    annot_filter: AnnotFilter,
    filter_serial: u32,
}

/// "1,3,4" (1-based) for the pages `pages` (0-based).
fn page_spec(pages: &[usize]) -> String {
    pages.iter().map(|p| (p + 1).to_string()).collect::<Vec<_>>().join(",")
}

/// Width and height of an A4 page in points.
const A4: (f32, f32) = (595.2756, 841.8898);

/// Most pages a document may have.
const MAX_PAGES: usize = 200_000;

/// The engine's facts about a document, checked: they come from another
/// process and must be plausible.
fn checked(mut info: LoadInfo) -> Result<LoadInfo> {
    if info.pages > MAX_PAGES {
        return Err(Error::Invalid(format!("too many pages ({})", info.pages)));
    }
    let sane = |v: f32| v.is_finite() && (0.0..=1.0e6).contains(&v);
    if info.sizes.len() != info.pages || info.sizes.iter().any(|(w, h)| !sane(*w) || !sane(*h)) {
        // Every page is then asked for on its own.
        info.sizes.clear();
    }
    Ok(info)
}

/// Reads a file with a size limit.
pub fn read_pdf_file(path: &Path) -> Result<Vec<u8>> {
    let meta = fs::metadata(path).map_err(|e| Error::io(path, e))?;
    if meta.len() > MAX_FILE_SIZE {
        return Err(Error::FileTooLarge { size: meta.len(), limit: MAX_FILE_SIZE });
    }
    fs::read(path).map_err(|e| Error::io(path, e))
}

/// Writes `bytes` to `path` atomically (temporary file + rename), so a
/// failure never leaves a half-written document behind.
/// Reads an XFDF file (size-limited, UTF-8).
fn read_xfdf(path: &Path) -> Result<String> {
    let len = fs::metadata(path).map_err(|e| Error::io(path, e))?.len();
    if len > xfdf::MAX_XFDF as u64 {
        return Err(Error::FileTooLarge { size: len, limit: xfdf::MAX_XFDF as u64 });
    }
    String::from_utf8(fs::read(path).map_err(|e| Error::io(path, e))?)
        .map_err(|_| Error::Invalid("XFDF file is not UTF-8".into()))
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let file_name =
        path.file_name().ok_or_else(|| Error::Invalid("invalid file name".into()))?.to_string_lossy();
    let nonce =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let tmp = dir.join(format!(".{file_name}.{}-{nonce:x}.versolopdf-tmp", std::process::id()));
    let result = (|| -> std::io::Result<()> {
        // create_new refuses to follow a pre-existing file or symlink.
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, path)
    })();
    if let Err(err) = result {
        let _ = fs::remove_file(&tmp);
        return Err(Error::io(path, err));
    }
    Ok(())
}

impl Document {
    /// Opens a PDF file.
    pub fn open(path: &Path, password: Option<&str>) -> Result<Self> {
        let bytes = read_pdf_file(path)?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "document.pdf".into());
        let mut doc = Self::from_bytes(bytes, &name, password)?;
        doc.path = Some(path.to_path_buf());
        Ok(doc)
    }

    /// Opens a PDF from memory.
    pub fn from_bytes(bytes: Vec<u8>, display_name: &str, password: Option<&str>) -> Result<Self> {
        let engine = EngineDoc::new();
        let info = checked(engine.load(&bytes, password)?)?;
        let mut doc = Self {
            engine,
            info,
            bytes,
            engine_dirty: false,
            layers_cache: None,
            path: None,
            display_name: display_name.to_string(),
            password: password.map(str::to_string),
            geometry: Vec::new(),
            history: History::default(),
            revision: 0,
            instance: NEXT_INSTANCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            structure_rev: 0,
            page_revs: HashMap::new(),
            local_page: None,
            modified: false,
            security: None,
            annot_filter: AnnotFilter::default(),
            filter_serial: 0,
        };
        doc.refresh_security();
        doc.refresh_geometry();
        Ok(doc)
    }

    /// Creates a new document with one empty A4 page.
    pub fn new_blank(display_name: &str) -> Result<Self> {
        let bytes: Bytes = EngineDoc::once(Request::CreateBlank { sizes: vec![A4] })?;
        let mut doc = Self::from_bytes(bytes.0, display_name, None)?;
        doc.modified = true;
        Ok(doc)
    }

    /// Creates a new, unsaved document with `count` empty pages of
    /// `width` × `height` points ("Neues leeres Dokument").
    pub fn new_with_pages(display_name: &str, width: f32, height: f32, count: usize) -> Result<Self> {
        if !(1.0..=14_400.0).contains(&width)
            || !(1.0..=14_400.0).contains(&height)
            || !(1..=1000).contains(&count)
        {
            return Err(Error::Invalid("invalid page size or count".into()));
        }
        let bytes: Bytes = EngineDoc::once(Request::CreateBlank { sizes: vec![(width, height); count] })?;
        let mut doc = Self::from_bytes(bytes.0, display_name, None)?;
        doc.modified = true;
        Ok(doc)
    }

    /// Creates a new, unsaved document from generated PDF bytes plus
    /// (page, title) bookmarks.
    pub fn new_from_pdf(bytes: Vec<u8>, bookmarks: &[(usize, String)], display_name: &str) -> Result<Self> {
        let mut doc = Self::from_bytes(bytes, display_name, None)?;
        if !bookmarks.is_empty() {
            let items: Vec<outline::NewBookmark> = bookmarks
                .iter()
                .map(|(p, t)| outline::NewBookmark { title: t.clone(), page: *p, level: 0 })
                .collect();
            doc.append_bookmarks(&items)?;
            // The bookmarks belong to the new document, not to its history.
            doc.history = History::default();
        }
        doc.modified = true;
        Ok(doc)
    }

    /// A request to the engine that does not change the document.
    fn ask<T: DeserializeOwned>(&self, req: Request) -> Result<T> {
        self.engine.read(req, (&self.bytes, self.password.as_deref()))
    }

    /// Page count of a PDF (in an engine of its own).
    fn count_pages(bytes: Vec<u8>, password: Option<&str>) -> Result<usize> {
        let info: LoadInfo =
            EngineDoc::once(Request::Load { bytes: Bytes(bytes), password: password.map(str::to_string) })?;
        Ok(info.pages)
    }

    /// Reads visible area and rotation of every page. Loading every page with
    /// PDFium takes seconds for thousands of pages, so the values are read
    /// from the file and only pages whose size disagrees with PDFium's
    /// (cheap) page sizes are loaded.
    fn refresh_geometry(&mut self) {
        let count = self.info.pages;
        let sizes = self.info.sizes.clone();
        // The file must be current to read the boxes from it.
        let boxes = match self.sync_bytes() {
            Ok(()) => pdf_edit::page_boxes(&self.bytes, self.password.as_deref()).unwrap_or_default(),
            Err(_) => Vec::new(),
        };
        let geometry: Vec<PageGeometry> = (0..count)
            .map(|i| {
                let quick =
                    boxes.get(i).map(|&(crop, rot)| PageGeometry::new(crop, Rotation::from_degrees(rot)));
                match (quick, sizes.get(i)) {
                    (Some(g), Some(size))
                        if (g.view_size().0 - size.0).abs() < 0.5
                            && (g.view_size().1 - size.1).abs() < 0.5 =>
                    {
                        g
                    }
                    _ => self.loaded_geometry(i),
                }
            })
            .collect();
        self.geometry = geometry;
    }

    /// Visible area and rotation of one page, as the engine reads it.
    fn loaded_geometry(&self, index: usize) -> PageGeometry {
        match self.ask::<(RectF, Rotation)>(Request::Geometry { page: index }) {
            Ok((crop, rotation)) => PageGeometry::new(crop, rotation),
            Err(_) => PageGeometry::new(RectF::new(0.0, 0.0, 612.0, 792.0), Rotation::R0),
        }
    }

    // -----------------------------------------------------------------
    // Properties

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    pub fn page_count(&self) -> usize {
        self.geometry.len()
    }

    pub fn geometry(&self, page: usize) -> Option<&PageGeometry> {
        self.geometry.get(page)
    }

    /// Increases with every change; caches compare against it.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Different for every document loaded in this process, so caches can
    /// tell a re-created document (same revision) from the old one.
    pub fn instance(&self) -> u64 {
        self.instance
    }

    /// Revision of the last change that affects how page looks (for
    /// rendered pictures): comment edits on other pages do not count.
    pub fn page_revision(&self, page: usize) -> u64 {
        self.page_revs.get(&page).copied().unwrap_or(0).max(self.structure_rev)
    }

    /// Records a new revision for the pages the change affects.
    fn bump_revision(&mut self) {
        self.revision += 1;
        match self.local_page.take() {
            Some(page) => {
                self.page_revs.insert(page, self.revision);
            }
            None => {
                self.structure_rev = self.revision;
                self.page_revs.clear();
            }
        }
    }

    /// Runs an edit that only changes page (its comments).
    fn page_edit<T>(&mut self, page: usize, f: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        self.local_page = Some(page);
        let result = f(self);
        // A failed edit may have reloaded the whole document.
        if result.is_err() || self.local_page.is_some() {
            self.local_page = None;
            if result.is_err() {
                self.structure_rev = self.revision;
                self.page_revs.clear();
            }
        }
        result
    }

    pub fn is_modified(&self) -> bool {
        self.modified
    }

    /// Display filter for comments ("Anmerkungen > Anzeigen").
    pub fn annot_filter(&self) -> &AnnotFilter {
        &self.annot_filter
    }

    pub fn set_annot_filter(&mut self, filter: AnnotFilter) {
        if filter != self.annot_filter {
            self.annot_filter = filter;
            self.filter_serial = self.filter_serial.wrapping_add(1).max(1);
        }
    }

    /// Value for [`RenderOptions::filter`]: 0 if nothing is filtered.
    pub fn filter_key(&self) -> u32 {
        if self.annot_filter.is_empty() { 0 } else { self.filter_serial }
    }

    pub fn is_encrypted(&self) -> bool {
        self.password.is_some() || self.security.is_some() || !self.info.unprotected
    }

    fn refresh_security(&mut self) {
        let encrypted = !self.info.unprotected;
        self.security =
            if encrypted { security::security_info(&self.bytes, self.password.as_deref()) } else { None };
    }
    pub fn can_undo(&self) -> Option<&str> {
        self.history.undo.last().map(|s| s.label.as_str())
    }

    pub fn can_redo(&self) -> Option<&str> {
        self.history.redo.last().map(|s| s.label.as_str())
    }

    pub fn page_label(&self, index: usize) -> Option<String> {
        if index >= self.page_count() {
            return None;
        }
        self.ask::<Option<String>>(Request::PageLabel { page: index }).ok().flatten()
    }

    // -----------------------------------------------------------------
    // Rendering

    /// Renders a whole page into a bitmap of the given size.
    pub fn render_page(&self, index: usize, width: u32, height: u32) -> Result<RenderedImage> {
        self.render_tile(index, width, height, 0, 0, width, height, RenderOptions::default())
    }

    /// Renders the part `(x, y, w, h)` of the page scaled to `full_w × full_h`.
    #[allow(clippy::too_many_arguments)]
    pub fn render_tile(
        &self,
        index: usize,
        full_w: u32,
        full_h: u32,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        opts: RenderOptions,
    ) -> Result<RenderedImage> {
        let limit = MAX_BITMAP_EDGE * 8;
        if w == 0 || h == 0 || w > MAX_BITMAP_EDGE || h > MAX_BITMAP_EDGE || full_w > limit || full_h > limit
        {
            return Err(Error::Invalid(format!("invalid render size {w}x{h}")));
        }
        if index >= self.page_count() {
            return Err(Error::PageOutOfRange(index));
        }
        // The comment filter hides comments only for this rendering.
        let filter =
            (opts.filter != 0 && opts.annotations && !opts.printing).then(|| self.annot_filter.clone());
        let spec = DrawSpec {
            rotation: opts.rotation,
            annotations: opts.annotations,
            background: opts.background,
            printing: opts.printing,
            transparent: opts.transparent,
        };
        let picture: Picture = self.ask(Request::Render {
            page: index,
            full: (full_w, full_h),
            part: (x, y, w, h),
            spec,
            filter,
        })?;
        Self::finish_picture(picture, (w, h), opts.recolor)
    }

    /// Checks a rendered picture from the engine and applies the colours.
    fn finish_picture(
        picture: Picture,
        (w, h): (u32, u32),
        recolor: Option<([u8; 3], [u8; 3])>,
    ) -> Result<RenderedImage> {
        let mut rgba = picture.rgba.0;
        if picture.width != w || picture.height != h || rgba.len() != (w as usize) * (h as usize) * 4 {
            return Err(Error::Pdfium("the PDF engine sent a picture of the wrong size".into()));
        }
        if let Some((background, text)) = recolor {
            recolor_rgba(&mut rgba, background, text);
        }
        Ok(RenderedImage { width: w, height: h, rgba })
    }

    /// Recognises the words of a page with Tesseract: (text, box in PDF
    /// space). `dpi` is the resolution of the picture handed over.
    pub fn ocr_words(
        &self,
        index: usize,
        engine: &crate::ocr::Engine,
        dpi: f32,
    ) -> Result<Vec<(String, RectF)>> {
        let geo = *self.geometry(index).ok_or(Error::PageOutOfRange(index))?;
        let (w, h) = geo.view_size();
        let scale = (dpi / 72.0).clamp(1.0, 600.0 / 72.0);
        let px = |v: f32| ((v * scale).round() as u32).clamp(1, MAX_BITMAP_EDGE);
        let opts = RenderOptions { annotations: false, ..RenderOptions::default() };
        let img = self.render_tile(index, px(w), px(h), 0, 0, px(w), px(h), opts)?;
        let dir = std::env::temp_dir().join("versolopdf-ocr");
        fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
        let png = dir.join(format!("{}-{index}.png", std::process::id()));
        image::RgbaImage::from_raw(img.width, img.height, img.rgba)
            .ok_or_else(|| Error::Invalid("image".into()))?
            .save(&png)
            .map_err(|e| Error::Invalid(e.to_string()))?;
        let words = crate::ocr::recognize(&engine.exe, &png, &engine.languages);
        let _ = fs::remove_file(&png);
        Ok(words?
            .into_iter()
            .filter(|w| w.confidence >= 30.0)
            .map(|w| {
                let view = RectF::new(
                    w.left / scale,
                    w.top / scale,
                    (w.left + w.width) / scale,
                    (w.top + w.height) / scale,
                );
                (w.text, geo.view_rect_to_pdf(&view))
            })
            .collect())
    }

    /// Page text built from recognised words (for exports of scanned
    /// pages: "Beschädigten Text mit Texterkennung verbessern").
    pub fn ocr_page_text(&self, index: usize, engine: &crate::ocr::Engine) -> Result<PageText> {
        let mut out = PageText::default();
        for (text, r) in self.ocr_words(index, engine, 300.0)? {
            let r = r.normalized();
            let n = text.chars().count().max(1) as f32;
            for (i, ch) in text.chars().enumerate() {
                let x0 = r.x0 + r.width() * i as f32 / n;
                out.chars.push(CharInfo {
                    ch,
                    rect: RectF::new(x0, r.y0, x0 + r.width() / n, r.y1),
                    generated: false,
                });
            }
            out.chars.push(CharInfo { ch: ' ', rect: RectF::default(), generated: true });
        }
        Ok(out)
    }

    /// "Text auf Seiten erkennen": adds an invisible text layer to `pages`
    /// (one undo step). Pages that already have text are skipped when
    /// `skip_text` is set. Returns the number of recognised words.
    pub fn recognize_text(
        &mut self,
        pages: &[usize],
        engine: &crate::ocr::Engine,
        dpi: f32,
        skip_text: bool,
    ) -> Result<usize> {
        self.require(self.permissions().modify)?;
        let mut layers = Vec::new();
        for &page in pages {
            self.check_page(page)?;
            if skip_text
                && self
                    .page_text(page)?
                    .chars
                    .iter()
                    .filter(|c| !c.ch.is_whitespace() && !c.generated)
                    .count()
                    > 20
            {
                continue;
            }
            let words = self.ocr_words(page, engine, dpi)?;
            if !words.is_empty() {
                layers.push((page, words));
            }
        }
        let count = layers.iter().map(|(_, w)| w.len()).sum();
        if count > 0 {
            self.edit_bytes("recognize text", |b, pw| {
                let mut bytes = b.to_vec();
                for (page, words) in &layers {
                    bytes = crate::ocr::add_text_layer(&bytes, pw, *page, words)?;
                }
                Ok(bytes)
            })?;
        }
        Ok(count)
    }

    /// The embedded TrueType/OpenType font programs of a page by font name
    /// (without subset prefix), for "In Kurven umwandeln".
    pub fn font_programs(&mut self, index: usize) -> Result<HashMap<String, Vec<u8>>> {
        self.sync_bytes()?;
        let bytes = self.bytes.clone();
        let password = self.password.clone();
        pdf_edit::guarded(|| {
            let edit = pdf_edit::Edit::open(&bytes, password.as_deref())?;
            let mut out = HashMap::new();
            let mut node = Some(edit.page_id(index)?);
            let mut fonts = None;
            for _ in 0..32 {
                let Some(id) = node else { break };
                let dict = edit.object(id)?.as_dict()?;
                if let Ok(res) = dict.get(b"Resources") {
                    fonts = edit.resolve(res)?.as_dict()?.get(b"Font").ok().cloned();
                    break;
                }
                node = dict.get(b"Parent").ok().and_then(|p| p.as_reference().ok());
            }
            let Some(fonts) = fonts else { return Ok(out) };
            for (_, font) in edit.resolve(&fonts)?.as_dict()?.iter() {
                let Ok(font) = edit.resolve(font).and_then(|f| Ok(f.as_dict()?)) else { continue };
                let name = font
                    .get(b"BaseFont")
                    .ok()
                    .and_then(|n| n.as_name().ok())
                    .map(|n| String::from_utf8_lossy(n).into_owned());
                let Some(name) = name else { continue };
                // Type0 fonts keep the descriptor in the descendant font.
                let mut desc_holder = font.clone();
                if let Ok(desc) = font.get(b"DescendantFonts")
                    && let Some(first) =
                        edit.resolve(desc).ok().and_then(|d| d.as_array().ok()).and_then(|a| a.first())
                    && let Ok(d) = edit.resolve(first).and_then(|d| Ok(d.as_dict()?.clone()))
                {
                    desc_holder = d;
                }
                let descriptor = desc_holder
                    .get(b"FontDescriptor")
                    .ok()
                    .and_then(|d| edit.resolve(d).ok())
                    .and_then(|d| d.as_dict().ok())
                    .cloned();
                let Some(descriptor) = descriptor else { continue };
                for key in [&b"FontFile2"[..], b"FontFile3"] {
                    if let Ok(file) = descriptor.get(key)
                        && let Ok(stream) = edit.resolve(file).and_then(|s| Ok(s.as_stream()?.clone()))
                    {
                        let data = stream.decompressed_content().unwrap_or_else(|_| stream.content.clone());
                        out.insert(crate::outlines::base_name(&name).to_string(), data);
                        break;
                    }
                }
            }
            Ok(out)
        })
    }

    /// Renders a page into `width` × `height` pixels for an export. With
    /// `without_text` the text objects are left out (they become editable
    /// text elsewhere); the document itself is not changed.
    pub fn render_background(
        &mut self,
        index: usize,
        width: u32,
        height: u32,
        without_text: bool,
        annotations: bool,
    ) -> Result<RenderedImage> {
        let opts = RenderOptions { annotations, ..RenderOptions::default() };
        if !without_text {
            return self.render_tile(index, width, height, 0, 0, width, height, opts);
        }
        self.check_page(index)?;
        let picture: Picture =
            self.ask(Request::RenderWithoutText { page: index, size: (width, height), annotations })?;
        Self::finish_picture(picture, (width, height), None)
    }

    // -----------------------------------------------------------------
    // Content inspection

    pub fn page_text(&self, index: usize) -> Result<PageText> {
        self.check_page(index)?;
        self.ask(Request::PageText { page: index })
    }

    /// Like [`Self::page_text`] plus font and colour of every character
    /// (for exports that keep the look of the text).
    pub fn page_text_styled(&self, index: usize) -> Result<PageText> {
        self.check_page(index)?;
        let text: PageText = self.ask(Request::PageTextStyled { page: index })?;
        // Every character must have its style.
        if text.styles.len() != text.chars.len()
            || text.styles.iter().any(|s| s.font as usize >= text.fonts.len().max(1))
        {
            return Err(Error::Pdfium("inconsistent text from the PDF engine".into()));
        }
        Ok(text)
    }

    pub fn bookmarks(&self) -> Vec<Bookmark> {
        fn trim(items: &mut Vec<Bookmark>, depth: usize, budget: &mut usize) {
            items.truncate(*budget);
            *budget -= items.len();
            for b in items.iter_mut() {
                if depth >= MAX_BOOKMARK_DEPTH {
                    b.children.clear();
                }
                trim(&mut b.children, depth + 1, budget);
            }
        }
        let mut out: Vec<Bookmark> = self.ask(Request::Bookmarks).unwrap_or_default();
        let mut budget = MAX_BOOKMARKS;
        trim(&mut out, 0, &mut budget);
        out
    }

    pub fn links(&self, index: usize) -> Result<Vec<LinkInfo>> {
        self.check_page(index)?;
        self.ask(Request::Links { page: index })
    }

    pub fn annotations(&self, index: usize) -> Result<Vec<AnnotationInfo>> {
        self.check_page(index)?;
        let mut list: Vec<AnnotationInfo> = self.ask(Request::Annotations { page: index })?;
        // The page is the program's, whatever the engine says.
        for a in &mut list {
            a.page = index;
        }
        Ok(list)
    }

    /// Bounds of the smallest page object (text run, image, graphic) at
    /// `point` ("Inhalt hervorheben"). Objects covering most of the page,
    /// such as backgrounds, are skipped.
    pub fn object_at(&self, index: usize, point: crate::geometry::PointF) -> Result<Option<RectF>> {
        self.check_page(index)?;
        self.ask(Request::ObjectAt { page: index, point })
    }

    pub fn annotation_details(&mut self, page: usize, index: usize) -> Result<AnnotDetails> {
        self.sync_bytes()?;
        pdf_edit::annotation_details(&self.bytes, self.password.as_deref(), page, index)
    }

    pub fn form_fields(&self, index: usize) -> Result<Vec<FormFieldInfo>> {
        self.check_page(index)?;
        let mut list: Vec<FormFieldInfo> = self.ask(Request::FormFields { page: index })?;
        for f in &mut list {
            f.page = index;
        }
        Ok(list)
    }

    pub fn metadata(&self) -> Metadata {
        self.ask(Request::Metadata).unwrap_or_default()
    }

    /// Effective permissions. PDFium (via pdfium-render) does not know
    /// revision 5/6 handlers and checks the wrong bit for copying, so the
    /// `/P` bits are evaluated here.
    pub fn permissions(&self) -> Permissions {
        let encrypted = self.is_encrypted();
        let all = Permissions {
            encrypted,
            print: true,
            modify: true,
            copy: true,
            annotate: true,
            fill_forms: true,
            assemble: true,
        };
        let Some(info) = self.security else { return all };
        if info.owner == Some(true) {
            return all;
        }
        if info.owner.is_none() {
            // Revisions 2-4: PDFium grants every right after unlocking with
            // the owner password; detect that case.
            let pdfium_all = self.info.all_rights;
            let restricted = !(info.can_print()
                && info.can_modify()
                && info.can_annotate()
                && info.can_assemble()
                && info.can_fill_forms());
            if pdfium_all && restricted {
                return all;
            }
        }
        Permissions {
            encrypted,
            print: info.can_print(),
            modify: info.can_modify(),
            copy: info.can_copy(),
            annotate: info.can_annotate(),
            fill_forms: info.can_fill_forms(),
            assemble: info.can_assemble(),
        }
    }
    pub fn file_size(&self) -> usize {
        self.bytes.len()
    }

    // -----------------------------------------------------------------
    // State management

    /// Makes `self.bytes` reflect the PDFium document.
    fn sync_bytes(&mut self) -> Result<()> {
        if self.engine_dirty {
            let saved: Bytes = self.ask(Request::Save)?;
            self.bytes = saved.0;
            self.engine_dirty = false;
        }
        Ok(())
    }

    /// Returns the current serialised document.
    pub fn to_bytes(&mut self) -> Result<Vec<u8>> {
        self.sync_bytes()?;
        Ok(self.bytes.clone())
    }

    /// The current pages as a PDF with the password needed to read it
    /// (for inserting them into another document).
    pub fn snapshot(&mut self) -> Result<(Vec<u8>, Option<String>)> {
        Ok((self.to_bytes()?, self.password.clone()))
    }

    fn reload(&mut self, bytes: Vec<u8>) -> Result<()> {
        self.info = checked(self.engine.load(&bytes, self.password.as_deref())?)?;
        self.bytes = bytes;
        self.engine_dirty = false;
        self.layers_cache = None;
        // Comment edits keep every page's size and rotation; reading them
        // again takes seconds in documents with thousands of pages.
        if self.local_page.is_none() || self.geometry.len() != self.info.pages {
            self.refresh_geometry();
        }
        self.bump_revision();
        Ok(())
    }

    fn mark_changed(&mut self) {
        self.bump_revision();
        self.modified = true;
    }

    /// Applies a byte-level transformation (see [`pdf_edit`]).
    fn edit_bytes(
        &mut self,
        label: &str,
        f: impl FnOnce(&[u8], Option<&str>) -> Result<Vec<u8>>,
    ) -> Result<()> {
        self.sync_bytes()?;
        let new_bytes = f(&self.bytes, self.password.as_deref())?;
        let old = self.bytes.clone();
        self.reload(new_bytes)?;
        self.history.push_undo(Snapshot::new(label, old));
        self.modified = true;
        Ok(())
    }

    /// Applies an edit in the engine.
    fn edit_engine(&mut self, label: &str, req: Request) -> Result<()> {
        self.edit_engine_with::<()>(label, req)
    }

    /// Applies an edit in the engine that answers a value.
    fn edit_engine_with<T: DeserializeOwned>(&mut self, label: &str, req: Request) -> Result<T> {
        self.sync_bytes()?;
        let snapshot = self.bytes.clone();
        let result = self.engine.edit::<T>(req, (&snapshot, self.password.as_deref()));
        let result = result.and_then(|v| {
            self.info = checked(self.ask(Request::Info)?)?;
            Ok(v)
        });
        match result {
            Ok(v) => {
                self.engine_dirty = true;
                self.history.push_undo(Snapshot::new(label, snapshot));
                self.refresh_geometry();
                self.mark_changed();
                Ok(v)
            }
            Err(err) => {
                // Discard a possibly half-applied change.
                self.reload(snapshot)?;
                Err(err)
            }
        }
    }

    pub fn undo(&mut self) -> Result<bool> {
        self.undo_steps(1)
    }

    pub fn redo(&mut self) -> Result<bool> {
        self.redo_steps(1)
    }

    /// Labels of the steps undo would take back, the most recent first.
    pub fn undo_list(&self) -> Vec<&str> {
        self.history.undo.iter().rev().map(|s| s.label.as_str()).collect()
    }

    /// Labels of the steps redo would repeat, the next one first.
    pub fn redo_list(&self) -> Vec<&str> {
        self.history.redo.iter().rev().map(|s| s.label.as_str()).collect()
    }

    /// Takes back the last `n` steps at once (the document is loaded once).
    pub fn undo_steps(&mut self, n: usize) -> Result<bool> {
        self.travel(n, false)
    }

    /// Repeats the next `n` undone steps at once.
    pub fn redo_steps(&mut self, n: usize) -> Result<bool> {
        self.travel(n, true)
    }

    fn travel(&mut self, n: usize, forward: bool) -> Result<bool> {
        let available = if forward { self.history.redo.len() } else { self.history.undo.len() };
        let n = n.min(available);
        if n == 0 {
            return Ok(false);
        }
        self.sync_bytes()?;
        // Read the target first: if that fails, nothing has changed yet.
        let from = if forward { &self.history.redo } else { &self.history.undo };
        let target = from[from.len() - n].data.read()?;
        let mut current = Stored::Memory(self.bytes.clone());
        for _ in 0..n {
            let (from, to) = if forward {
                (&mut self.history.redo, &mut self.history.undo)
            } else {
                (&mut self.history.undo, &mut self.history.redo)
            };
            let Some(snapshot) = from.pop() else { break };
            to.push(Snapshot { label: snapshot.label, data: current });
            current = snapshot.data;
        }
        drop(current);
        self.reload(target)?;
        self.history.trim();
        self.modified = true;
        Ok(true)
    }

    /// Saves to the current file.
    /// "Zurücksetzen": discards all changes and reloads the file from disk.
    pub fn revert(&mut self) -> Result<()> {
        let path = self.path.clone().ok_or_else(|| Error::Invalid("document has no file name yet".into()))?;
        let bytes = read_pdf_file(&path)?;
        self.reload(bytes)?;
        self.history = History::default();
        self.refresh_security();
        self.modified = false;
        Ok(())
    }

    /// Points the document to its file after it was renamed or moved on disk.
    pub fn set_file_path(&mut self, path: &Path) {
        self.path = Some(path.to_path_buf());
        if let Some(name) = path.file_name() {
            self.display_name = name.to_string_lossy().into_owned();
        }
    }

    /// A document restored from a recovery copy: unsaved, but "Speichern"
    /// writes to the original file again.
    pub fn recovered(bytes: Vec<u8>, display_name: &str, original: Option<&Path>) -> Result<Self> {
        let mut doc = Self::from_bytes(bytes, display_name, None)?;
        doc.path = original.map(Path::to_path_buf);
        doc.modified = true;
        Ok(doc)
    }

    pub fn save(&mut self) -> Result<()> {
        let path = self.path.clone().ok_or_else(|| Error::Invalid("document has no file name yet".into()))?;
        self.save_as(&path)
    }

    pub fn save_as(&mut self, path: &Path) -> Result<()> {
        self.sync_bytes()?;
        write_atomic(path, &self.bytes)?;
        self.path = Some(path.to_path_buf());
        if let Some(name) = path.file_name() {
            self.display_name = name.to_string_lossy().into_owned();
        }
        self.modified = false;
        Ok(())
    }

    // -----------------------------------------------------------------
    // Annotations

    fn require(&self, allowed: bool) -> Result<()> {
        if allowed { Ok(()) } else { Err(Error::NotPermitted) }
    }

    pub fn add_annotation(&mut self, page: usize, annot: &NewAnnotation) -> Result<()> {
        self.check_page(page)?;
        self.require(self.permissions().annotate)?;
        let label = format!("add {}", annot.kind.pdf_subtype());
        self.page_edit(page, |d| d.edit_bytes(&label, |b, pw| pdf_edit::add_annotation(b, pw, page, annot)))
    }

    /// "Video/Audio/3D hinzufügen": embeds a media file in `rect`.
    pub fn add_media(
        &mut self,
        page: usize,
        rect: RectF,
        kind: crate::media::MediaKind,
        name: &str,
        data: &[u8],
        author: &str,
    ) -> Result<()> {
        self.check_page(page)?;
        self.require(self.permissions().annotate)?;
        self.edit_bytes("add media", |b, pw| {
            crate::media::add_media(b, pw, page, rect, kind, name, data, author)
        })
    }

    /// "Audio" note from a WAV recording.
    pub fn add_sound(
        &mut self,
        page: usize,
        rect: RectF,
        name: &str,
        wav: &[u8],
        author: &str,
    ) -> Result<()> {
        self.check_page(page)?;
        self.require(self.permissions().annotate)?;
        self.edit_bytes("add sound", |b, pw| crate::media::add_sound(b, pw, page, rect, name, wav, author))
    }

    /// The embedded file of a media annotation (for playing it).
    pub fn media_file(&mut self, page: usize, index: usize) -> Result<Option<(String, Vec<u8>)>> {
        self.sync_bytes()?;
        crate::media::media_file(&self.bytes, self.password.as_deref(), page, index)
    }

    pub fn update_annotation(&mut self, page: usize, index: usize, update: &AnnotUpdate) -> Result<()> {
        self.check_page(page)?;
        self.require(self.permissions().annotate)?;
        self.page_edit(page, |d| {
            d.edit_bytes("edit annotation", |b, pw| pdf_edit::update_annotation(b, pw, page, index, update))
        })
    }

    /// Moves several comments of one page in one step (one undo step).
    pub fn move_annotations(&mut self, page: usize, indices: &[usize], dx: f32, dy: f32) -> Result<()> {
        self.check_page(page)?;
        self.require(self.permissions().annotate)?;
        let update = AnnotUpdate { translate: Some((dx, dy)), ..AnnotUpdate::default() };
        let label = if indices.len() == 1 { "move annotation" } else { "move annotations" };
        self.page_edit(page, |d| {
            d.edit_bytes(label, |b, pw| {
                let mut bytes = b.to_vec();
                for &index in indices {
                    bytes = pdf_edit::update_annotation(&bytes, pw, page, index, &update)?;
                }
                Ok(bytes)
            })
        })
    }

    pub fn delete_annotations(&mut self, page: usize, indices: &[usize]) -> Result<()> {
        self.check_page(page)?;
        self.require(self.permissions().annotate)?;
        self.page_edit(page, |d| {
            d.edit_bytes("delete annotations", |b, pw| pdf_edit::delete_annotations(b, pw, page, indices))
        })
    }

    pub fn add_links(&mut self, page: usize, links: &[(RectF, pdf_edit::LinkSpec)]) -> Result<()> {
        self.check_page(page)?;
        self.require(self.permissions().modify)?;
        if links.is_empty() {
            return Ok(());
        }
        self.edit_bytes("add links", |b, pw| pdf_edit::add_links(b, pw, page, links))
    }

    /// Creates links for all web addresses found in the page texts.
    /// Returns the number of new links.
    pub fn create_web_links(&mut self) -> Result<usize> {
        self.require(self.permissions().modify)?;
        let mut per_page = Vec::new();
        for page in 0..self.page_count() {
            let existing = self.links(page)?;
            let text = self.page_text(page)?;
            let links: Vec<(RectF, pdf_edit::LinkSpec)> = text
                .find_urls()
                .into_iter()
                .flat_map(|(range, url)| {
                    text.line_rects(range).into_iter().map(move |r| (r, pdf_edit::LinkSpec::Uri(url.clone())))
                })
                .filter(|(r, _)| !existing.iter().any(|l| l.rect.intersects(r)))
                .collect();
            if !links.is_empty() {
                per_page.push((page, links));
            }
        }
        let count = per_page.iter().map(|(_, l)| l.len()).sum();
        if count == 0 {
            return Ok(0);
        }
        self.edit_bytes("create web links", |b, pw| {
            let mut bytes = b.to_vec();
            for (page, links) in &per_page {
                bytes = pdf_edit::add_links(&bytes, pw, *page, links)?;
            }
            Ok(bytes)
        })?;
        Ok(count)
    }

    /// Removes all web links; returns how many were removed.
    pub fn remove_web_links(&mut self) -> Result<usize> {
        self.require(self.permissions().modify)?;
        let mut removed = 0;
        self.edit_bytes("remove web links", |b, pw| {
            let (bytes, n) = pdf_edit::remove_web_links(b, pw)?;
            removed = n;
            Ok(bytes)
        })?;
        Ok(removed)
    }

    pub fn delete_annotation(&mut self, page: usize, index: usize) -> Result<()> {
        self.check_page(page)?;
        self.require(self.permissions().annotate)?;
        self.page_edit(page, |d| {
            d.edit_bytes("delete annotation", |b, pw| pdf_edit::delete_annotation(b, pw, page, index))
        })
    }

    /// Converts all annotations and form fields of the given pages into
    /// regular page content.
    pub fn flatten_pages(&mut self, pages: &[usize]) -> Result<()> {
        for &p in pages {
            self.check_page(p)?;
        }
        self.require(self.permissions().modify)?;
        self.edit_engine("flatten", Request::Flatten { pages: pages.to_vec() })
    }

    /// Inserts an image file (PNG/JPEG) as stamp in the middle of page.
    pub fn add_image_stamp(&mut self, page: usize, image_path: &Path, style: &AnnotStyle) -> Result<()> {
        self.check_page(page)?;
        self.require(self.permissions().annotate)?;
        let image = load_image(image_path)?;
        let geo = *self.geometry(page).ok_or(Error::PageOutOfRange(page))?;
        let (vw, vh) = geo.view_size();
        let w = (vw * 0.4).min(220.0);
        let h = w * image.height as f32 / image.width as f32;
        let view = RectF::new((vw - w) / 2.0, (vh - h) / 2.0, (vw + w) / 2.0, (vh + h) / 2.0);
        let rect = geo.view_rect_to_pdf(&view);
        self.edit_bytes("add image", |b, pw| pdf_edit::add_image_stamp(b, pw, page, rect, &image, style))
    }

    // -----------------------------------------------------------------
    // Document structure

    /// Creates a new document from several PDF files (in the given order).
    pub fn merge_files(paths: &[PathBuf], display_name: &str) -> Result<Document> {
        if paths.is_empty() {
            return Err(Error::Invalid("no files selected".into()));
        }
        let sources = paths.iter().map(|p| read_pdf_file(p).map(Bytes)).collect::<Result<Vec<_>>>()?;
        let bytes: Bytes = EngineDoc::once(Request::Combine { sources })?;
        let mut doc = Self::from_bytes(bytes.0, display_name, None)?;
        doc.modified = true;
        Ok(doc)
    }

    /// Writes the document in parts of every pages into dir. Returns the
    /// written files.
    pub fn split(&self, every: usize, dir: &Path, stem: &str) -> Result<Vec<PathBuf>> {
        if every == 0 {
            return Err(Error::Invalid("invalid part size".into()));
        }
        let n = self.page_count();
        let mut files = Vec::new();
        for (part, start) in (0..n).step_by(every).enumerate() {
            let pages: Vec<usize> = (start..(start + every).min(n)).collect();
            let path = dir.join(format!("{stem}-{}.pdf", part + 1));
            self.extract_pages(&pages, &path)?;
            files.push(path);
        }
        Ok(files)
    }

    /// Inserts copies of pages directly after the last of them.
    pub fn duplicate_pages(&mut self, pages: &[usize]) -> Result<()> {
        let mut pages = pages.to_vec();
        pages.sort_unstable();
        pages.dedup();
        let Some(&last) = pages.last() else { return Ok(()) };
        self.check_page(last)?;
        self.require(self.permissions().assemble)?;
        self.edit_engine("duplicate pages", Request::DuplicatePages { spec: page_spec(&pages), at: last + 1 })
    }

    /// Replaces pages (consecutive) by the same number of pages of another
    /// PDF, starting at source_start there.
    pub fn replace_pages(&mut self, pages: &[usize], source: &Path, source_start: usize) -> Result<()> {
        let mut pages = pages.to_vec();
        pages.sort_unstable();
        pages.dedup();
        let Some(&last) = pages.last() else { return Ok(()) };
        self.check_page(last)?;
        self.require(self.permissions().assemble)?;
        let source = Bytes(read_pdf_file(source)?);
        let count = pages.len();
        self.edit_engine("replace pages", Request::ReplacePages { pages, source, start: source_start, count })
    }

    pub fn swap_pages(&mut self, a: usize, b: usize) -> Result<()> {
        self.check_page(a)?;
        self.check_page(b)?;
        self.require(self.permissions().assemble)?;
        if a == b {
            return Ok(());
        }
        let mut order: Vec<usize> = (0..self.page_count()).collect();
        order.swap(a, b);
        self.edit_bytes("swap pages", |bytes, pw| pdf_edit::reorder_pages(bytes, pw, &order))
    }

    pub fn reverse_pages(&mut self) -> Result<()> {
        self.require(self.permissions().assemble)?;
        let order: Vec<usize> = (0..self.page_count()).rev().collect();
        self.edit_bytes("reverse pages", |bytes, pw| pdf_edit::reorder_pages(bytes, pw, &order))
    }

    /// Groups of pages that look identical (same text and same rendering).
    pub fn find_duplicate_pages(&self) -> Result<Vec<Vec<usize>>> {
        use std::collections::HashMap;
        use std::hash::{Hash, Hasher};
        let mut groups: HashMap<u64, Vec<usize>> = HashMap::new();
        for page in 0..self.page_count() {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            self.page_text(page)?.all_text().hash(&mut hasher);
            let (w, h) = self.geometry(page).map_or((1.0, 1.0), |g| g.view_size());
            let img = self.render_page(page, 48, ((48.0 * h / w.max(1.0)) as u32).clamp(1, 400))?;
            // Coarse grey levels make the comparison robust against tiny differences.
            img.rgba
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| (u16::from(p[0]) + u16::from(p[1]) + u16::from(p[2])) / 96)
                .collect::<Vec<_>>()
                .hash(&mut hasher);
            groups.entry(hasher.finish()).or_default().push(page);
        }
        let mut out: Vec<Vec<usize>> = groups.into_values().filter(|g| g.len() > 1).collect();
        out.sort();
        Ok(out)
    }

    /// Scales pages to a new size (points, displayed orientation).
    pub fn resize_pages(&mut self, pages: &[usize], size: (f32, f32), fit: bool) -> Result<()> {
        for &p in pages {
            self.check_page(p)?;
        }
        self.require(self.permissions().assemble)?;
        self.edit_bytes("resize pages", |b, pw| pages::resize_pages(b, pw, pages, size, fit))
    }

    /// Cuts margins (left, bottom, right, top; points) off the visible area.
    pub fn crop_pages(&mut self, pages: &[usize], margins: [f32; 4]) -> Result<()> {
        for &p in pages {
            self.check_page(p)?;
        }
        self.require(self.permissions().assemble)?;
        let neg = margins.map(|m| -m.max(0.0));
        self.edit_bytes("crop pages", |b, pw| pages::change_margins(b, pw, pages, neg, true))
    }

    /// Adds white margins around the pages (points).
    pub fn add_margins(&mut self, pages: &[usize], margins: [f32; 4]) -> Result<()> {
        for &p in pages {
            self.check_page(p)?;
        }
        self.require(self.permissions().assemble)?;
        let pos = margins.map(|m| m.max(0.0));
        self.edit_bytes("add margins", |b, pw| pages::change_margins(b, pw, pages, pos, false))
    }

    /// Adds watermarks, backgrounds, headers/footers or Bates numbers.
    pub fn add_page_marks(
        &mut self,
        pages: &[usize],
        mark: &pages::PageMark,
        text_for: &dyn Fn(&str, usize) -> String,
    ) -> Result<()> {
        for &p in pages {
            self.check_page(p)?;
        }
        self.require(self.permissions().modify)?;
        self.edit_bytes("page marks", |b, pw| pages::add_page_marks(b, pw, pages, mark, text_for))
    }

    pub fn set_page_labels(
        &mut self,
        start: usize,
        style: pages::LabelStyle,
        prefix: &str,
        first: u32,
    ) -> Result<()> {
        self.check_page(start)?;
        self.require(self.permissions().modify)?;
        self.edit_bytes("page labels", |b, pw| pages::set_page_labels(b, pw, start, style, prefix, first))
    }

    /// Page transitions of all pages ("Seitenübergänge").
    pub fn transitions(&mut self) -> Result<Vec<crate::transitions::Transition>> {
        self.sync_bytes()?;
        crate::transitions::read(&self.bytes, self.password.as_deref())
    }

    /// Sets the transition of `pages` (one undo step).
    pub fn set_transitions(
        &mut self,
        pages: &[usize],
        t: crate::transitions::Transition,
        random: bool,
    ) -> Result<()> {
        for &p in pages {
            self.check_page(p)?;
        }
        self.require(self.permissions().modify)?;
        self.edit_bytes("page transitions", |b, pw| crate::transitions::write(b, pw, pages, t, random))
    }

    /// All page label ranges ("Seiten nummerieren").
    pub fn page_label_ranges(&mut self) -> Result<Vec<pages::LabelRange>> {
        self.sync_bytes()?;
        pages::read_page_labels(&self.bytes, self.password.as_deref())
    }

    /// Replaces all page label ranges.
    pub fn set_page_label_ranges(&mut self, ranges: &[pages::LabelRange]) -> Result<()> {
        self.require(self.permissions().modify)?;
        if ranges.iter().any(|r| r.start >= self.page_count().max(1)) {
            return Err(Error::Invalid("label range outside the document".into()));
        }
        self.edit_bytes("page labels", |b, pw| pages::write_page_labels(b, pw, ranges))
    }

    // -----------------------------------------------------------------
    // Bookmarks

    pub fn add_bookmark(&mut self, parent: &[usize], title: &str, page: usize) -> Result<()> {
        self.check_page(page)?;
        self.require(self.permissions().modify)?;
        self.edit_bytes("add bookmark", |b, pw| outline::add_bookmark(b, pw, parent, title, page))
    }

    pub fn rename_bookmark(&mut self, path: &[usize], title: &str) -> Result<()> {
        self.require(self.permissions().modify)?;
        self.edit_bytes("rename bookmark", |b, pw| outline::rename_bookmark(b, pw, path, title))
    }

    pub fn delete_bookmark(&mut self, path: &[usize]) -> Result<()> {
        self.require(self.permissions().modify)?;
        self.edit_bytes("delete bookmark", |b, pw| outline::delete_bookmark(b, pw, path))
    }

    /// Rewrites all bookmark titles; returns the number of changed titles.
    pub fn map_bookmark_titles(&mut self, f: impl Fn(&str) -> String) -> Result<usize> {
        self.require(self.permissions().modify)?;
        let mut changed = 0;
        self.edit_bytes("edit bookmarks", |b, pw| {
            let (bytes, n) = outline::map_titles(b, pw, f)?;
            changed = n;
            Ok(bytes)
        })?;
        Ok(changed)
    }

    pub fn sort_bookmarks(&mut self, by_page: bool) -> Result<()> {
        self.require(self.permissions().modify)?;
        self.edit_bytes("sort bookmarks", |b, pw| outline::sort(b, pw, by_page))
    }

    pub fn remove_duplicate_bookmarks(&mut self) -> Result<usize> {
        self.require(self.permissions().modify)?;
        let mut removed = 0;
        self.edit_bytes("remove duplicate bookmarks", |b, pw| {
            let (bytes, n) = outline::remove_duplicates(b, pw)?;
            removed = n;
            Ok(bytes)
        })?;
        Ok(removed)
    }

    pub fn delete_all_bookmarks(&mut self) -> Result<()> {
        self.require(self.permissions().modify)?;
        self.edit_bytes("delete bookmarks", outline::delete_all)
    }

    pub fn append_bookmarks(&mut self, items: &[outline::NewBookmark]) -> Result<usize> {
        self.require(self.permissions().modify)?;
        if items.is_empty() {
            return Ok(0);
        }
        self.edit_bytes("add bookmarks", |b, pw| outline::append(b, pw, items))?;
        Ok(items.len())
    }

    /// Suggests bookmarks from headings: lines set clearly larger than the
    /// body text. Larger sizes give higher levels (at most three).
    pub fn heading_bookmarks(&self) -> Result<Vec<outline::NewBookmark>> {
        let mut lines = Vec::new();
        for page in 0..self.page_count() {
            for (text, height) in self.page_text(page)?.lines() {
                lines.push((page, text, height));
            }
        }
        // Body size = the most frequent (rounded) line height.
        let mut counts: BTreeMap<i32, usize> = BTreeMap::new();
        for (_, text, h) in &lines {
            *counts.entry((h * 2.0).round() as i32).or_default() += text.chars().count();
        }
        let Some((&body, _)) = counts.iter().max_by_key(|(_, n)| **n) else { return Ok(Vec::new()) };
        let body = body as f32 / 2.0;
        let mut sizes: Vec<i32> = lines
            .iter()
            .filter(|(_, _, h)| *h >= body * 1.25)
            .map(|(_, _, h)| (h * 2.0).round() as i32)
            .collect();
        sizes.sort_unstable_by(|a, b| b.cmp(a));
        sizes.dedup();
        Ok(lines
            .into_iter()
            .filter(|(_, text, h)| *h >= body * 1.25 && (2..=120).contains(&text.chars().count()))
            .map(|(page, title, h)| {
                let rank = sizes.iter().position(|s| *s == (h * 2.0).round() as i32).unwrap_or(0);
                outline::NewBookmark { title, page, level: rank.min(2) }
            })
            .collect())
    }

    /// Bookmarks from the text of all highlight comments.
    pub fn bookmarks_from_highlights(&mut self) -> Result<usize> {
        let mut items = Vec::new();
        for c in self.comments()? {
            if c.kind != AnnotKind::Highlight {
                continue;
            }
            let areas = if c.quads.is_empty() { vec![c.rect] } else { c.quads.clone() };
            let mut title = self.page_text(c.page)?.text_in_rects(&areas);
            if title.is_empty() {
                title = c.contents.trim().to_string();
            }
            if !title.is_empty() {
                items.push(outline::NewBookmark {
                    title: title.chars().take(200).collect(),
                    page: c.page,
                    level: 0,
                });
            }
        }
        self.append_bookmarks(&items)
    }

    /// Inserts a table of contents built from the bookmarks in front of the
    /// first page. Returns the number of inserted pages.
    pub fn insert_toc(&mut self, heading: &str) -> Result<usize> {
        use crate::report::Line;
        self.require(self.permissions().modify && self.permissions().assemble)?;
        fn walk(items: &[Bookmark], depth: usize, out: &mut Vec<(String, Option<usize>, usize)>) {
            for b in items {
                out.push((b.title.clone(), b.page, depth));
                walk(&b.children, depth + 1, out);
            }
        }
        let mut entries = Vec::new();
        walk(&self.bookmarks(), 0, &mut entries);
        if entries.is_empty() {
            return Err(Error::Invalid("the document has no bookmarks".into()));
        }
        let size = self.geometry(0).map_or(crate::report::A4, |g| g.view_size());
        let make = |offset: usize| {
            let mut lines = vec![Line::title(heading)];
            for (title, page, depth) in &entries {
                lines.push(Line {
                    bold: *depth == 0,
                    indent: *depth as f32 * 14.0,
                    space_before: if *depth == 0 { 4.0 } else { 0.0 },
                    link: *page,
                    right: page.map(|p| (p + offset + 1).to_string()).unwrap_or_default(),
                    ..Line::new(title.clone())
                });
            }
            lines
        };
        // The page numbers shown include the inserted pages themselves.
        let count = crate::report::layout(&make(0), size).len();
        let lines = make(count);
        let mut inserted = 0;
        self.edit_bytes("insert table of contents", |b, pw| {
            let (bytes, n) = crate::report::insert_front(b, pw, &lines)?;
            inserted = n;
            Ok(bytes)
        })?;
        Ok(inserted)
    }

    /// The bookmarks as indented plain text or as an HTML list.
    pub fn export_bookmarks(&self, html: bool) -> String {
        fn esc(s: &str) -> String {
            s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
        }
        fn walk(items: &[Bookmark], depth: usize, html: bool, out: &mut String) {
            if html {
                out.push_str(&format!("{}<ul>\n", "  ".repeat(depth)));
            }
            for b in items {
                let page = b.page.map(|p| (p + 1).to_string()).unwrap_or_default();
                if html {
                    out.push_str(&format!(
                        "{}  <li>{} <span class=\"page\">{}</span>",
                        "  ".repeat(depth),
                        esc(&b.title),
                        page
                    ));
                    if !b.children.is_empty() {
                        out.push('\n');
                        walk(&b.children, depth + 2, html, out);
                        out.push_str(&"  ".repeat(depth + 1));
                    }
                    out.push_str("</li>\n");
                } else {
                    out.push_str(&format!("{}{}\t{}\n", "\t".repeat(depth), b.title, page));
                    walk(&b.children, depth + 1, html, out);
                }
            }
            if html {
                out.push_str(&format!("{}</ul>\n", "  ".repeat(depth)));
            }
        }
        let mut body = String::new();
        walk(&self.bookmarks(), 0, html, &mut body);
        if html {
            format!(
                "<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"><title>{}</title>\n<style>body{{font-family:sans-serif}} .page{{color:#777;margin-left:1em}}</style></head>\n<body>\n<h1>{}</h1>\n{body}</body></html>\n",
                esc(&self.display_name),
                esc(&self.display_name)
            )
        } else {
            body
        }
    }

    // -----------------------------------------------------------------
    // Forms

    pub fn set_text_field(&mut self, page: usize, annot_index: usize, value: &str) -> Result<()> {
        self.set_field(page, annot_index, &FieldValue::Text(value.to_string()))
    }

    pub fn set_checked(&mut self, page: usize, annot_index: usize, checked: bool) -> Result<()> {
        self.set_field(page, annot_index, &FieldValue::Checked(checked))
    }

    pub fn set_choice(&mut self, page: usize, annot_index: usize, value: &str) -> Result<()> {
        self.set_field(page, annot_index, &FieldValue::Choice(value.to_string()))
    }

    fn set_field(&mut self, page: usize, annot_index: usize, value: &FieldValue) -> Result<()> {
        self.check_page(page)?;
        if !self.permissions().fill_forms {
            return Err(Error::NotPermitted);
        }
        self.edit_bytes("fill form", |b, pw| forms::set_field_value(b, pw, page, annot_index, value))
    }

    /// Creates a new form field; returns the (unique) name it got.
    pub fn create_field(
        &mut self,
        page: usize,
        rect: RectF,
        kind: &forms::NewFieldKind,
        name: &str,
    ) -> Result<String> {
        self.check_page(page)?;
        self.require(self.permissions().modify)?;
        let mut used = String::new();
        self.edit_bytes("add form field", |b, pw| {
            let (bytes, name) = forms::create_field(b, pw, page, rect, kind, name)?;
            used = name;
            Ok(bytes)
        })?;
        Ok(used)
    }

    /// Date format, image button and required flag of a widget.
    pub fn field_extras(&mut self, page: usize, annot_index: usize) -> Result<forms::FieldExtras> {
        self.sync_bytes()?;
        forms::field_extras(&self.bytes, self.password.as_deref(), page, annot_index)
    }

    /// Properties of a form field (form designer).
    pub fn field_props(&mut self, page: usize, annot_index: usize) -> Result<forms::FieldProps> {
        self.check_page(page)?;
        self.sync_bytes()?;
        forms::field_props(&self.bytes, self.password.as_deref(), page, annot_index)
    }

    /// Changes the properties of a form field (one undo step).
    pub fn set_field_props(
        &mut self,
        page: usize,
        annot_index: usize,
        props: &forms::FieldProps,
    ) -> Result<()> {
        self.check_page(page)?;
        self.require(self.permissions().modify)?;
        self.edit_bytes("form field properties", |b, pw| {
            forms::set_field_props(b, pw, page, annot_index, props)
        })
    }

    /// "Bild" field: shows `image` in the button.
    pub fn set_button_image(
        &mut self,
        page: usize,
        annot_index: usize,
        image: &pdf_edit::RgbaImage,
    ) -> Result<()> {
        self.check_page(page)?;
        if !self.permissions().fill_forms {
            return Err(Error::NotPermitted);
        }
        self.edit_bytes("fill form", |b, pw| forms::set_button_image(b, pw, page, annot_index, image))
    }

    /// "Formulare erkennen": places for fields on `page` – runs of
    /// underscores and lines become text fields, small squares check boxes.
    pub fn detect_fields(&self, page_index: usize) -> Result<Vec<(RectF, forms::NewFieldKind)>> {
        self.check_page(page_index)?;
        let paths: Vec<RectF> = self.ask(Request::PathBounds { page: page_index })?;
        let mut found: Vec<(RectF, forms::NewFieldKind)> = Vec::new();
        let text_field = || forms::NewFieldKind::Text { multiline: false };
        // Underscore runs ("Name: ________").
        let text = self.page_text(page_index)?;
        let mut run: Option<RectF> = None;
        let flush = |run: &mut Option<RectF>, found: &mut Vec<(RectF, forms::NewFieldKind)>| {
            if let Some(r) = run.take()
                && r.width() >= 30.0
            {
                let h = r.height().clamp(10.0, 20.0) + 4.0;
                found.push((RectF::new(r.x0, r.y0, r.x1, r.y0 + h), text_field()));
            }
        };
        for c in &text.chars {
            if c.ch == '_' && c.rect.width() > 0.0 {
                run = Some(match run {
                    Some(r) if (c.rect.x0 - r.x1).abs() < 3.0 && (c.rect.y0 - r.y0).abs() < 2.0 => {
                        RectF::new(r.x0, r.y0.min(c.rect.y0), c.rect.x1, r.y1.max(c.rect.y1))
                    }
                    _ => {
                        flush(&mut run, &mut found);
                        c.rect
                    }
                });
            } else {
                flush(&mut run, &mut found);
            }
        }
        flush(&mut run, &mut found);
        // Drawn lines and boxes.
        for r in paths.into_iter().take(20_000) {
            let (w, h) = (r.width(), r.height());
            let overlaps = |a: &RectF| found.iter().any(|(b, _)| a.intersects(b));
            if (6.0..=20.0).contains(&w) && (w - h).abs() <= 2.0 {
                let cell = RectF::new(r.x0, r.y0, r.x1, r.y1);
                if !overlaps(&cell) {
                    found.push((cell, forms::NewFieldKind::CheckBox));
                }
            } else if w >= 40.0 && h <= 2.5 {
                let cell = RectF::new(r.x0, r.y1, r.x1, r.y1 + 16.0);
                if !overlaps(&cell) {
                    found.push((cell, text_field()));
                }
            }
        }
        // Not where a field already is.
        if let Ok(fields) = self.form_fields(page_index) {
            found.retain(|(r, _)| !fields.iter().any(|f| f.rect.intersects(r)));
        }
        found.truncate(500);
        Ok(found)
    }

    /// Creates the fields found by [`Self::detect_fields`] (one undo step).
    /// Returns the number of new fields.
    pub fn create_detected_fields(
        &mut self,
        fields: &[(usize, RectF, forms::NewFieldKind)],
    ) -> Result<usize> {
        self.require(self.permissions().modify)?;
        if fields.is_empty() {
            return Ok(0);
        }
        for (page, _, _) in fields {
            self.check_page(*page)?;
        }
        self.edit_bytes("detect form fields", |b, pw| {
            let mut bytes = b.to_vec();
            for (page, rect, kind) in fields {
                bytes = forms::create_field(&bytes, pw, *page, *rect, kind, "")?.0;
            }
            Ok(bytes)
        })?;
        Ok(fields.len())
    }

    /// "Alle JavaScripte" (for display only).
    pub fn all_javascripts(&mut self) -> Result<Vec<(String, String)>> {
        self.sync_bytes()?;
        crate::doc_props::all_javascripts(&self.bytes, self.password.as_deref())
    }

    /// Sets fields by full name (used by "Formulare ausfüllen").
    pub fn set_values_by_name(&mut self, values: &[(String, String)]) -> Result<usize> {
        self.require(self.permissions().fill_forms)?;
        let mut count = 0;
        self.edit_bytes("fill form", |b, pw| {
            let (bytes, n) = forms::set_values_by_name(b, pw, values)?;
            count = n;
            Ok(bytes)
        })?;
        Ok(count)
    }

    pub fn field_values(&mut self) -> Result<Vec<(String, String)>> {
        self.sync_bytes()?;
        forms::field_values(&self.bytes, self.password.as_deref())
    }

    /// "Ebenen": the layers of the document.
    pub fn layers(&mut self) -> Result<Vec<crate::layers::Layer>> {
        if let Some(cached) = &self.layers_cache {
            return Ok(cached.clone());
        }
        // Reading the whole file takes long: only when the bytes mention
        // layers, or when the catalogue may hide in a compressed object
        // stream (files up to 32 MB).
        const LARGE: usize = 32 * 1024 * 1024;
        let has = |needle: &[u8]| self.bytes.windows(needle.len()).any(|w| w == needle);
        let worth = has(b"/OCProperties") || (self.bytes.len() <= LARGE && has(b"/ObjStm"));
        let found = if worth {
            crate::layers::layers(&self.bytes, self.password.as_deref()).unwrap_or_default()
        } else {
            Vec::new()
        };
        self.layers_cache = Some(found.clone());
        Ok(found)
    }

    /// Shows or hides a layer (in the default view saved with the file).
    pub fn set_layer_visible(&mut self, index: usize, visible: bool) -> Result<()> {
        let label = if visible { "Ebene einblenden" } else { "Ebene ausblenden" };
        self.edit_bytes(label, |b, p| crate::layers::set_visible(b, p, index, visible))
    }

    pub fn reset_form(&mut self) -> Result<()> {
        self.require(self.permissions().fill_forms)?;
        self.edit_bytes("reset form", forms::reset_form)
    }

    /// Writes the form data as XFDF.
    pub fn export_form_data(&mut self, dest: &Path) -> Result<()> {
        let values = self.field_values()?;
        write_atomic(dest, xfdf::export_fields(&values, &self.display_name).as_bytes())
    }

    /// Fills the form from an XFDF file. Returns the number of fields set.
    pub fn import_form_data(&mut self, source: &Path) -> Result<usize> {
        self.require(self.permissions().fill_forms)?;
        let values = xfdf::import_fields(&read_xfdf(source)?)?;
        let mut count = 0;
        self.edit_bytes("import form data", |b, pw| {
            let (bytes, n) = forms::set_values_by_name(b, pw, &values)?;
            count = n;
            Ok(bytes)
        })?;
        Ok(count)
    }

    // -----------------------------------------------------------------
    // Redaction

    /// Marks every occurrence of `query` for redaction. Returns the number
    /// of hits.
    pub fn mark_for_redaction(
        &mut self,
        query: &str,
        opts: SearchOptions,
        style: &AnnotStyle,
    ) -> Result<usize> {
        self.require(self.permissions().annotate)?;
        let mut items = Vec::new();
        for page in 0..self.page_count() {
            let text = self.page_text(page)?;
            for hit in crate::search::search_page(page, &text, query, opts) {
                for rect in hit.rects {
                    let mut annot = NewAnnotation::new(AnnotKind::Redact, style.clone());
                    annot.rect = rect.inflate(1.0);
                    items.push((page, annot));
                }
            }
        }
        if items.is_empty() {
            return Ok(0);
        }
        self.edit_bytes("mark for redaction", |b, pw| pdf_edit::add_annotations(b, pw, &items))?;
        Ok(items.len())
    }

    /// Renders pages (without comments) as RGB images with marks painted
    /// black, ready to replace the page content (see [crate::redact::apply]).
    fn rasterize(
        &self,
        marks: &BTreeMap<usize, Vec<RectF>>,
        dpi: f32,
    ) -> Result<Vec<crate::redact::PageImage>> {
        let mut images = Vec::new();
        for (&page, rects) in marks {
            let geo = *self.geometry(page).ok_or(Error::PageOutOfRange(page))?;
            let (vw, vh) = geo.view_size();
            let edge = MAX_BITMAP_EDGE as f32;
            let scale = (dpi.clamp(50.0, 600.0) / 72.0).min(edge / vw.max(1.0)).min(edge / vh.max(1.0));
            let (w, h) = (((vw * scale).round() as u32).max(1), ((vh * scale).round() as u32).max(1));
            let opts = RenderOptions { annotations: false, ..RenderOptions::default() };
            let img = self.render_tile(page, w, h, 0, 0, w, h, opts)?;
            let mut rgb: Vec<u8> =
                img.rgba.as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]).collect();
            for rect in rects {
                let v = geo.pdf_rect_to_view(rect);
                let x0 = ((v.x0 * scale).floor().max(0.0) as u32).min(w);
                let x1 = ((v.x1 * scale).ceil().max(0.0) as u32).min(w);
                let y0 = ((v.y0 * scale).floor().max(0.0) as u32).min(h);
                let y1 = ((v.y1 * scale).ceil().max(0.0) as u32).min(h);
                for y in y0..y1 {
                    let row = (y * w) as usize * 3;
                    rgb[row + x0 as usize * 3..row + x1 as usize * 3].fill(0);
                }
            }
            // Image unit square -> PDF space: bottom-left, bottom-right, top-left.
            let o = geo.view_to_pdf(PointF::new(0.0, vh));
            let px = geo.view_to_pdf(PointF::new(vw, vh));
            let py = geo.view_to_pdf(PointF::new(0.0, 0.0));
            images.push(crate::redact::PageImage {
                page,
                width: w,
                height: h,
                rgb,
                matrix: [px.x - o.x, px.y - o.y, py.x - o.x, py.y - o.y, o.x, o.y],
            });
        }
        Ok(images)
    }

    /// Applies all redaction marks: affected pages become images with the
    /// marked areas blacked out (`dpi` sets their resolution) and the file is
    /// rewritten without history. Returns the number of affected pages.
    pub fn apply_redactions(&mut self, dpi: f32) -> Result<usize> {
        self.require(self.has_full_access())?;
        self.sync_bytes()?;
        let marks = crate::redact::redaction_marks(&self.bytes, self.password.as_deref())?;
        if marks.is_empty() {
            return Ok(0);
        }
        let images = self.rasterize(&marks, dpi)?;
        let count = images.len();
        self.edit_bytes("apply redactions", |b, pw| crate::redact::apply(b, pw, &images))?;
        Ok(count)
    }

    // -----------------------------------------------------------------
    // Review and conversion

    /// Words and characters of the document text.
    pub fn word_count(&self) -> Result<WordCount> {
        let mut count = WordCount { pages: self.page_count(), ..WordCount::default() };
        for page in 0..self.page_count() {
            let text = self.page_text(page)?.all_text();
            count.words += text.split_whitespace().count();
            count.characters += text.chars().filter(|c| !c.is_control()).count();
            count.characters_no_spaces +=
                text.chars().filter(|c| !c.is_whitespace() && !c.is_control()).count();
        }
        Ok(count)
    }

    /// All text lines as (page, text).
    pub fn text_lines(&self) -> Result<Vec<(usize, String)>> {
        let mut out = Vec::new();
        for page in 0..self.page_count() {
            out.extend(self.page_text(page)?.lines().into_iter().map(|(t, _)| (page, t)));
        }
        Ok(out)
    }

    /// Pages without any text (typically scans that need OCR).
    pub fn pages_without_text(&self) -> Result<Vec<usize>> {
        let mut out = Vec::new();
        for page in 0..self.page_count() {
            if self.page_text(page)?.all_text().trim().is_empty() {
                out.push(page);
            }
        }
        Ok(out)
    }

    pub fn accessibility_structure(&mut self) -> Result<crate::accessibility::StructureInfo> {
        self.sync_bytes()?;
        crate::accessibility::check_structure(&self.bytes, self.password.as_deref())
    }

    pub fn export_text(&self, dest: &Path) -> Result<()> {
        let pages: Vec<String> =
            (0..self.page_count()).map(|p| self.page_text(p).map(|t| t.all_text())).collect::<Result<_>>()?;
        write_atomic(dest, crate::convert::text_export(&pages).as_bytes())
    }

    /// Writes the text as a Word document; headings are recognised by size.
    pub fn export_word(&self, dest: &Path) -> Result<()> {
        self.export_word_pages(dest, &(0..self.page_count()).collect::<Vec<_>>())
    }

    /// Writes the text of `selection` as a Word document, with the pictures
    /// of the pages where they stand.
    pub fn export_word_pages(&self, dest: &Path, selection: &[usize]) -> Result<()> {
        self.export_word_with(dest, selection, true)
    }

    /// Word export; `images`: also transfer the pictures of the pages.
    pub fn export_word_with(&self, dest: &Path, selection: &[usize], images: bool) -> Result<()> {
        for &page in selection {
            self.check_page(page)?;
        }
        let o =
            crate::office::WordOptions { keep_layout: false, tables: true, images, dpi: 150.0, ocr: None };
        crate::office::export_word_flow(self, dest, selection, &o, None)
    }

    /// The pictures on a page as PNG with their place (PDF space); tiny
    /// pictures are skipped.
    pub fn page_images(&self, index: usize) -> Result<Vec<(RectF, Vec<u8>)>> {
        self.check_page(index)?;
        let list: Vec<(RectF, Bytes)> = self.ask(Request::PageImages { page: index })?;
        Ok(list.into_iter().take(200).map(|(r, b)| (r, b.0)).collect())
    }

    /// "Seiten neu einfärben": dark parts become `text`, light parts and the
    /// paper `background` (RGB).
    pub fn recolor_pages(&mut self, background: [u8; 3], text: [u8; 3]) -> Result<()> {
        self.require(self.has_full_access())?;
        self.edit_bytes("recolor pages", |b, pw| crate::convert::recolor(b, pw, background, text))
    }

    /// "Schriften ersetzen": (font name, standard font) pairs. Returns the
    /// number of fonts replaced.
    pub fn replace_fonts(&mut self, map: &[(String, String)]) -> Result<usize> {
        self.require(self.permissions().modify)?;
        let mut count = 0;
        self.edit_bytes("replace fonts", |b, pw| {
            let (bytes, n) = crate::font_replace::replace_fonts(b, pw, map)?;
            count = n;
            Ok(bytes)
        })?;
        Ok(count)
    }

    pub fn to_grayscale(&mut self) -> Result<()> {
        self.require(self.has_full_access())?;
        self.edit_bytes("convert to grayscale", crate::convert::to_grayscale)
    }

    /// Turns the content of `pages` into images (comments stay editable).
    pub fn contents_to_images(&mut self, pages: &[usize], dpi: f32) -> Result<usize> {
        self.require(self.has_full_access())?;
        self.sync_bytes()?;
        let marks: BTreeMap<usize, Vec<RectF>> =
            pages.iter().filter(|p| **p < self.page_count()).map(|p| (*p, Vec::new())).collect();
        let images = self.rasterize(&marks, dpi)?;
        let count = images.len();
        self.edit_bytes("contents to images", |b, pw| crate::redact::apply(b, pw, &images))?;
        Ok(count)
    }

    /// A new document from PDF and image files (images at 150 dpi).
    pub fn from_files(paths: &[PathBuf], display_name: &str) -> Result<Document> {
        if paths.is_empty() {
            return Err(Error::Invalid("no files selected".into()));
        }
        let mut sources = Vec::new();
        for path in paths {
            let is_pdf = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf"));
            sources.push(Bytes(if is_pdf {
                read_pdf_file(path)?
            } else {
                crate::convert::image_page_pdf(&load_image(path)?, 150.0)?
            }));
        }
        let bytes: Bytes = EngineDoc::once(Request::Combine { sources })?;
        let mut doc = Self::from_bytes(bytes.0, display_name, None)?;
        doc.modified = true;
        Ok(doc)
    }

    // -----------------------------------------------------------------
    // Comments as a whole

    /// All comments of the document in page order.
    pub fn comments(&mut self) -> Result<Vec<pdf_edit::AnnotRecord>> {
        self.sync_bytes()?;
        pdf_edit::read_annotations(&self.bytes, self.password.as_deref())
    }

    pub fn export_comments(&mut self, dest: &Path) -> Result<usize> {
        let records = self.comments()?;
        write_atomic(dest, xfdf::export_comments(&records, &self.display_name).as_bytes())?;
        Ok(records.len())
    }

    /// Imports comments from an XFDF file. Returns the number added.
    pub fn import_comments(&mut self, source: &Path, default_author: &str) -> Result<usize> {
        self.require(self.permissions().annotate)?;
        let pages = self.page_count();
        let items: Vec<_> = xfdf::import_comments(&read_xfdf(source)?, default_author)?
            .into_iter()
            .filter(|(p, _)| *p < pages)
            .collect();
        if items.is_empty() {
            return Ok(0);
        }
        self.edit_bytes("import comments", |b, pw| pdf_edit::add_annotations(b, pw, &items))?;
        Ok(items.len())
    }

    /// Deletes every comment (links and form fields stay). Returns the count.
    pub fn delete_all_comments(&mut self) -> Result<usize> {
        self.require(self.permissions().annotate)?;
        let records = self.comments()?;
        if records.is_empty() {
            return Ok(0);
        }
        let mut by_page: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for r in &records {
            by_page.entry(r.page).or_default().push(r.index);
        }
        self.edit_bytes("delete all comments", |b, pw| {
            let mut current = b.to_vec();
            for (page, indices) in &by_page {
                current = pdf_edit::delete_annotations(&current, pw, *page, indices)?;
            }
            Ok(current)
        })?;
        Ok(records.len())
    }
    // -----------------------------------------------------------------
    // Security

    /// Encrypts the document with AES-256. The undo history is cleared
    /// because older snapshots use different passwords.
    pub fn protect(&mut self, opts: &ProtectOptions) -> Result<()> {
        self.require(self.has_full_access())?;
        self.sync_bytes()?;
        let (bytes, owner) = security::protect(&self.bytes, self.password.as_deref(), opts)?;
        let previous = self.password.replace(owner);
        if let Err(err) = self.reload(bytes) {
            self.password = previous;
            return Err(err);
        }
        self.history = History::default();
        self.refresh_security();
        self.modified = true;
        Ok(())
    }

    /// Removes password protection (requires full access).
    pub fn unprotect(&mut self) -> Result<()> {
        self.require(self.has_full_access())?;
        self.sync_bytes()?;
        let bytes = security::remove_protection(&self.bytes, self.password.as_deref())?;
        let previous = self.password.take();
        if let Err(err) = self.reload(bytes) {
            self.password = previous;
            return Err(err);
        }
        self.history = History::default();
        self.refresh_security();
        self.modified = true;
        Ok(())
    }

    /// True if the document is unencrypted or was opened with full rights.
    pub fn has_full_access(&self) -> bool {
        let p = self.permissions();
        p.print && p.modify && p.copy && p.annotate && p.fill_forms && p.assemble
    }

    // -----------------------------------------------------------------
    // Attachments

    /// Embedded files as (name, size in bytes).
    pub fn attachments(&self) -> Vec<(String, usize)> {
        self.ask(Request::Attachments).unwrap_or_default()
    }

    /// True if the document is a PDF portfolio.
    pub fn is_portfolio(&mut self) -> bool {
        if self.sync_bytes().is_err() {
            return false;
        }
        crate::portfolio::is_portfolio(&self.bytes, self.password.as_deref())
    }

    /// The files of a portfolio (embedded files with size and date).
    pub fn portfolio_entries(&mut self) -> Result<Vec<crate::portfolio::Entry>> {
        self.sync_bytes()?;
        crate::portfolio::entries(&self.bytes, self.password.as_deref())
    }

    /// Marks the document as portfolio (or not).
    pub fn set_portfolio(&mut self, on: bool) -> Result<()> {
        self.require(self.permissions().modify)?;
        self.edit_bytes("portfolio", |b, pw| crate::portfolio::set_portfolio(b, pw, on))
    }

    /// Index of the embedded file `name` (for saving or deleting it).
    pub fn attachment_index(&self, name: &str) -> Option<usize> {
        self.attachments().iter().position(|(n, _)| n == name)
    }

    /// Embeds a file read into memory (portfolios).
    pub fn add_attachment_bytes(&mut self, name: &str, bytes: &[u8]) -> Result<()> {
        self.require(self.permissions().modify)?;
        if bytes.len() as u64 > crate::portfolio::MAX_FILE {
            return Err(Error::FileTooLarge { size: bytes.len() as u64, limit: crate::portfolio::MAX_FILE });
        }
        let req = Request::AddAttachment { name: name.to_string(), bytes: Bytes(bytes.to_vec()) };
        self.edit_engine("attach file", req)
    }

    /// Writes an embedded file to disk. Attachments are never opened or executed.
    pub fn save_attachment(&self, index: usize, dest: &Path) -> Result<()> {
        let index = u16::try_from(index).map_err(|_| Error::Invalid("attachment index".into()))?;
        let bytes: Bytes = self.ask(Request::Attachment { index })?;
        write_atomic(dest, &bytes.0)
    }

    pub fn add_attachment(&mut self, source: &Path) -> Result<()> {
        self.require(self.permissions().modify)?;
        let bytes = read_pdf_file(source)?;
        let name = source
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| Error::Invalid("invalid file name".into()))?;
        self.edit_engine("attach file", Request::AddAttachment { name, bytes: Bytes(bytes) })
    }

    pub fn delete_attachment(&mut self, index: usize) -> Result<()> {
        self.require(self.permissions().modify)?;
        let index = u16::try_from(index).map_err(|_| Error::Invalid("attachment index".into()))?;
        self.edit_engine("delete attachment", Request::DeleteAttachment { index })
    }

    // -----------------------------------------------------------------
    // Pages

    fn check_page(&self, page: usize) -> Result<()> {
        if page < self.page_count() { Ok(()) } else { Err(Error::PageOutOfRange(page)) }
    }

    /// Rotates the given pages by `delta` degrees (multiple of 90).
    pub fn rotate_pages(&mut self, pages: &[usize], delta: i32) -> Result<()> {
        for &p in pages {
            self.check_page(p)?;
        }
        self.require(self.permissions().assemble)?;
        let rotations: Vec<(usize, Rotation)> =
            pages.iter().map(|&p| (p, self.geometry[p].rotation.rotated_by(delta))).collect();
        self.edit_engine("rotate pages", Request::RotatePages { pages: rotations })
    }

    pub fn delete_pages(&mut self, pages: &[usize]) -> Result<()> {
        let mut pages: Vec<usize> = pages.to_vec();
        pages.sort_unstable();
        pages.dedup();
        for &p in &pages {
            self.check_page(p)?;
        }
        if pages.len() >= self.page_count() {
            return Err(Error::Invalid("a document must keep at least one page".into()));
        }
        self.require(self.permissions().assemble)?;
        self.edit_engine("delete pages", Request::DeletePages { pages })
    }

    /// Inserts an empty page of the given size (points) before `at`.
    pub fn insert_blank_page(&mut self, at: usize, width: f32, height: f32) -> Result<()> {
        if at > self.page_count() || !(1.0..=14_400.0).contains(&width) || !(1.0..=14_400.0).contains(&height)
        {
            return Err(Error::Invalid("invalid page size or position".into()));
        }
        self.require(self.permissions().assemble)?;
        self.edit_engine("insert page", Request::InsertBlank { at, size: (width, height), count: 1 })
    }

    /// Inserts all pages of another PDF before `at`.
    pub fn insert_pages_from(&mut self, at: usize, source: &Path, password: Option<&str>) -> Result<usize> {
        if at > self.page_count() {
            return Err(Error::PageOutOfRange(at));
        }
        self.require(self.permissions().assemble)?;
        self.insert_pages_with(at, read_pdf_file(source)?, password, None)
    }

    /// Inserts pages of another PDF (all, or `pages`) before `at` as one
    /// undo step; returns their number (0: nothing changed).
    fn insert_pages_with(
        &mut self,
        at: usize,
        bytes: Vec<u8>,
        password: Option<&str>,
        pages: Option<&[usize]>,
    ) -> Result<usize> {
        if pages.is_some_and(<[usize]>::is_empty) {
            return Ok(0);
        }
        let req = Request::InsertPages {
            at,
            source: Bytes(bytes),
            password: password.map(str::to_string),
            spec: pages.map(page_spec),
        };
        let n = self.edit_engine_with::<usize>("insert pages", req)?;
        if n == 0 {
            // Nothing was inserted: no undo step.
            self.history.undo.pop();
        }
        Ok(n)
    }

    /// Writes the given pages into a new PDF file.
    pub fn extract_pages(&self, pages: &[usize], dest: &Path) -> Result<()> {
        let bytes = self.extract_pages_to_bytes(pages)?;
        write_atomic(dest, &bytes)
    }

    pub fn extract_pages_to_bytes(&self, pages: &[usize]) -> Result<Vec<u8>> {
        if pages.is_empty() {
            return Err(Error::Invalid("no pages selected".into()));
        }
        for &p in pages {
            self.check_page(p)?;
        }
        let bytes: Bytes = self.ask(Request::ExtractPages { spec: page_spec(pages) })?;
        Ok(bytes.0)
    }

    /// Moves `pages` so that they start at position `to` (index in the
    /// document *before* the move).
    pub fn move_pages(&mut self, pages: &[usize], to: usize) -> Result<()> {
        self.require(self.permissions().assemble)?;
        let n = self.page_count();
        let order = move_order(n, pages, to)?;
        if order.iter().enumerate().all(|(i, &p)| i == p) {
            return Ok(());
        }
        self.edit_bytes("move pages", |b, pw| pdf_edit::reorder_pages(b, pw, &order))
    }

    // -----------------------------------------------------------------
    // Page dialogs of "Dokumentaufbau"

    /// Number of pages of a PDF file (for the insert dialogs).
    pub fn file_page_count(path: &Path, password: Option<&str>) -> Result<usize> {
        Self::count_pages(read_pdf_file(path)?, password)
    }

    /// Inserts the given pages (0-based) of another PDF before `at`.
    pub fn insert_pages_selected(
        &mut self,
        at: usize,
        source: &Path,
        password: Option<&str>,
        pages: &[usize],
    ) -> Result<usize> {
        self.insert_pages_from_bytes(at, read_pdf_file(source)?, password, pages)
    }

    /// Like `insert_pages_selected`, from a PDF in memory (an open document).
    pub fn insert_pages_from_bytes(
        &mut self,
        at: usize,
        bytes: Vec<u8>,
        password: Option<&str>,
        pages: &[usize],
    ) -> Result<usize> {
        if at > self.page_count() {
            return Err(Error::PageOutOfRange(at));
        }
        self.require(self.permissions().assemble)?;
        self.insert_pages_with(at, bytes, password, Some(pages))
    }

    /// Inserts all pages of a generated PDF before `at` and adds bookmarks
    /// `(page in that PDF, title)`. Returns the number of pages.
    pub fn insert_pdf_bytes(
        &mut self,
        at: usize,
        bytes: Vec<u8>,
        bookmarks: &[(usize, String)],
    ) -> Result<usize> {
        if at > self.page_count() {
            return Err(Error::PageOutOfRange(at));
        }
        self.require(self.permissions().assemble)?;
        let count = self.insert_pages_with(at, bytes, None, None)?;
        if count == 0 {
            return Ok(0);
        }
        if !bookmarks.is_empty() && self.permissions().modify {
            let items: Vec<outline::NewBookmark> = bookmarks
                .iter()
                .map(|(p, t)| outline::NewBookmark { title: t.clone(), page: at + p, level: 0 })
                .collect();
            self.append_bookmarks(&items)?;
        }
        Ok(count)
    }

    /// Inserts `count` empty pages of `width` × `height` points before `at`.
    pub fn insert_blank_pages(&mut self, at: usize, width: f32, height: f32, count: usize) -> Result<()> {
        if at > self.page_count()
            || !(1.0..=14_400.0).contains(&width)
            || !(1.0..=14_400.0).contains(&height)
            || !(1..=1000).contains(&count)
        {
            return Err(Error::Invalid("invalid page size, count or position".into()));
        }
        self.require(self.permissions().assemble)?;
        self.edit_engine("insert pages", Request::InsertBlank { at, size: (width, height), count })
    }

    /// Deletes pages, optionally together with the bookmarks pointing to them.
    pub fn delete_pages_with_bookmarks(&mut self, pages: &[usize], bookmarks: bool) -> Result<()> {
        if bookmarks && self.permissions().modify {
            self.sync_bytes()?;
            let (bytes, removed) = outline::remove_for_pages(&self.bytes, self.password.as_deref(), pages)?;
            if removed > 0 {
                let old = std::mem::replace(&mut self.bytes, bytes.clone());
                self.reload(bytes)?;
                self.history.push_undo(Snapshot::new("delete bookmarks", old));
            }
        }
        self.delete_pages(pages)
    }

    /// Pages among `pages` that look empty. `tolerance` is the allowed share
    /// of dark pixels in per mille when `ignore_noise` is set (scans).
    pub fn blank_pages(
        &self,
        pages: &[usize],
        ignore_annotations: bool,
        ignore_noise: bool,
        tolerance: f32,
    ) -> Result<Vec<usize>> {
        let mut out = Vec::new();
        for &page in pages {
            self.check_page(page)?;
            let (w, h) = self.geometry(page).map_or((1.0, 1.0), |g| g.view_size());
            let pw = 120u32;
            let ph = ((pw as f32 * h / w.max(1.0)) as u32).clamp(1, 1200);
            let opts = RenderOptions { annotations: !ignore_annotations, ..RenderOptions::default() };
            let img = self.render_tile(page, pw, ph, 0, 0, pw, ph, opts)?;
            // Scanners leave dark specks at the border: skip 5 % on each side.
            let border = if ignore_noise { (pw / 20, ph / 20) } else { (0, 0) };
            let mut dark = 0u32;
            let mut total = 0u32;
            for y in border.1..ph - border.1 {
                for x in border.0..pw - border.0 {
                    let i = ((y * pw + x) * 4) as usize;
                    let lum = (u32::from(img.rgba[i]) * 3
                        + u32::from(img.rgba[i + 1]) * 6
                        + u32::from(img.rgba[i + 2]))
                        / 10;
                    total += 1;
                    if lum < 200 {
                        dark += 1;
                    }
                }
            }
            let share = dark as f32 / total.max(1) as f32 * 1000.0;
            let limit = if ignore_noise { tolerance.clamp(0.0, 100.0) } else { 0.5 };
            let has_text = !self.page_text(page)?.all_text().trim().is_empty();
            if share <= limit && (!has_text || ignore_noise) {
                out.push(page);
            }
        }
        Ok(out)
    }

    /// Inserts `copies` copies of `pages` before `at`. With `collate` the
    /// whole set is repeated (1,2,3,1,2,3), else each page (1,1,2,2,3,3).
    /// Without `comments`/`forms` the copies lose those annotations.
    pub fn duplicate_pages_to(
        &mut self,
        pages: &[usize],
        at: usize,
        copies: usize,
        collate: bool,
        comments: bool,
        forms: bool,
    ) -> Result<usize> {
        if pages.is_empty() || !(1..=100).contains(&copies) || at > self.page_count() {
            return Err(Error::Invalid("invalid pages, copies or position".into()));
        }
        for &p in pages {
            self.check_page(p)?;
        }
        self.require(self.permissions().assemble)?;
        let order: Vec<usize> = if collate {
            (0..copies).flat_map(|_| pages.iter().copied()).collect()
        } else {
            pages.iter().flat_map(|p| std::iter::repeat_n(*p, copies)).collect()
        };
        self.edit_engine("duplicate pages", Request::DuplicatePages { spec: page_spec(&order), at })?;
        if !comments || !forms {
            for page in at..at + order.len() {
                let indices: Vec<usize> = self
                    .annotations(page)?
                    .iter()
                    .enumerate()
                    .filter(|(_, a)| {
                        let widget = a.kind == AnnotKind::Widget;
                        (widget && !forms) || (!widget && a.kind != AnnotKind::Link && !comments)
                    })
                    .map(|(i, _)| i)
                    .collect();
                if !indices.is_empty() {
                    self.edit_bytes("duplicate pages", |b, pw| {
                        pdf_edit::delete_annotations(b, pw, page, &indices)
                    })?;
                    // One undo step for the whole operation.
                    self.history.undo.pop();
                }
            }
        }
        Ok(order.len())
    }

    /// Replaces `targets[i]` by page `sources[i]` of another PDF, keeping the
    /// target page objects (see [`crate::import::replace_pages`]).
    pub fn replace_pages_from(
        &mut self,
        targets: &[usize],
        source: &Path,
        sources: &[usize],
        opts: crate::import::ReplaceOptions,
    ) -> Result<usize> {
        for &p in targets {
            self.check_page(p)?;
        }
        self.require(self.permissions().assemble)?;
        let src = crate::import::Source::load(&read_pdf_file(source)?, None)?;
        let n = targets.len().min(sources.len());
        if n == 0 {
            return Err(Error::Invalid("no pages to replace".into()));
        }
        if sources.iter().any(|&p| p >= src.page_count()) {
            return Err(Error::Invalid("the other document has not enough pages".into()));
        }
        let (t, s) = (&targets[..n], &sources[..n]);
        self.edit_bytes("replace pages", |b, pw| crate::import::replace_pages(b, pw, t, &src, s, opts))?;
        Ok(n)
    }

    /// Reverses the order of `pages` among themselves.
    pub fn reverse_page_range(&mut self, pages: &[usize]) -> Result<()> {
        self.require(self.permissions().assemble)?;
        let mut sorted = pages.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        for &p in &sorted {
            self.check_page(p)?;
        }
        let mut order: Vec<usize> = (0..self.page_count()).collect();
        for (slot, page) in sorted.iter().zip(sorted.iter().rev()) {
            order[*slot] = *page;
        }
        self.edit_bytes("reverse pages", |bytes, pw| pdf_edit::reorder_pages(bytes, pw, &order))
    }

    /// Removes page offsets and rotation without changing the look.
    pub fn normalize_pages(&mut self, pages: &[usize]) -> Result<usize> {
        for &p in pages {
            self.check_page(p)?;
        }
        self.require(self.permissions().modify)?;
        let mut changed = 0;
        self.edit_bytes("normalize pages", |b, pw| {
            let (bytes, n) = pages::normalize_pages(b, pw, pages)?;
            changed = n;
            Ok(bytes)
        })?;
        if changed == 0 {
            self.history.undo.pop();
        }
        Ok(changed)
    }

    /// Groups of identical-looking pages among `pages`.
    pub fn find_duplicate_pages_in(
        &self,
        pages: &[usize],
        ignore_orientation: bool,
        ignore_annotations: bool,
    ) -> Result<Vec<Vec<usize>>> {
        use std::collections::HashMap;
        use std::hash::{Hash, Hasher};
        const W: u32 = 48;
        let mut groups: HashMap<u64, Vec<usize>> = HashMap::new();
        for &page in pages {
            self.check_page(page)?;
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            self.page_text(page)?.all_text().hash(&mut hasher);
            let geo = self.geometry(page).copied().ok_or(Error::PageOutOfRange(page))?;
            let (w, h) = geo.view_size();
            // Ignoring the orientation: undo the page rotation while rendering,
            // so the same content gives the same picture.
            let (rotation, rw, rh) = if ignore_orientation {
                let undo = Rotation::from_degrees(360 - geo.rotation.degrees());
                if geo.rotation.is_quarter() { (undo, h, w) } else { (undo, w, h) }
            } else {
                (Rotation::R0, w, h)
            };
            let ph = ((W as f32 * rh / rw.max(1.0)) as u32).clamp(1, 400);
            let opts =
                RenderOptions { annotations: !ignore_annotations, rotation, ..RenderOptions::default() };
            let img = self.render_tile(page, W, ph, 0, 0, W, ph, opts)?;
            // Coarse grey levels make the comparison robust against tiny differences.
            img.rgba
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| (u16::from(p[0]) + u16::from(p[1]) + u16::from(p[2])) / 96)
                .collect::<Vec<u16>>()
                .hash(&mut hasher);
            groups.entry(hasher.finish()).or_default().push(page);
        }
        let mut out: Vec<Vec<usize>> = groups.into_values().filter(|g| g.len() > 1).collect();
        out.sort();
        Ok(out)
    }

    /// Writes `pages` into a new PDF with the given handling of comments,
    /// form fields and bookmarks.
    pub fn extract_pages_with(&self, pages: &[usize], opts: &ExtractOptions) -> Result<Vec<u8>> {
        let bytes = self.extract_pages_to_bytes(pages)?;
        if opts.comments == Handling::Copy
            && opts.forms == Handling::Copy
            && opts.bookmarks == BookmarkCopy::None
        {
            return Ok(bytes);
        }
        let mut doc = Self::from_bytes(bytes, "extract.pdf", None)?;
        let n = doc.page_count();
        if opts.comments == Handling::Flatten || opts.forms == Handling::Flatten {
            doc.flatten_pages(&(0..n).collect::<Vec<_>>())?;
        }
        if opts.comments == Handling::Remove || opts.forms == Handling::Remove {
            for page in 0..n {
                let indices: Vec<usize> = doc
                    .annotations(page)?
                    .iter()
                    .enumerate()
                    .filter(|(_, a)| match a.kind {
                        AnnotKind::Widget => opts.forms == Handling::Remove,
                        AnnotKind::Link => false,
                        _ => opts.comments == Handling::Remove,
                    })
                    .map(|(i, _)| i)
                    .collect();
                if !indices.is_empty() {
                    doc.delete_annotations(page, &indices)?;
                }
            }
        }
        if opts.bookmarks != BookmarkCopy::None {
            fn walk(items: &[Bookmark], level: usize, out: &mut Vec<(String, Option<usize>, usize)>) {
                for b in items {
                    out.push((b.title.clone(), b.page, level));
                    walk(&b.children, level + 1, out);
                }
            }
            let mut flat = Vec::new();
            walk(&self.bookmarks(), 0, &mut flat);
            let items: Vec<outline::NewBookmark> = flat
                .into_iter()
                .filter_map(|(title, page, level)| {
                    let new_page = page.and_then(|p| pages.iter().position(|x| *x == p));
                    match (opts.bookmarks, new_page) {
                        (BookmarkCopy::Related, None) => None,
                        (_, Some(p)) => Some(outline::NewBookmark { title, page: p, level }),
                        // Without a target in the new file the bookmark keeps only its title.
                        (_, None) => Some(outline::NewBookmark { title, page: usize::MAX, level }),
                    }
                })
                .collect();
            doc.append_bookmarks(&items)?;
        }
        doc.to_bytes()
    }

    // -----------------------------------------------------------------
    // Managed page marks (step E, see crate::marks)

    fn mark_context(&self) -> crate::marks::MarkContext {
        let now = pdf_edit::pdf_date_now();
        let digits: String = now.chars().filter(|c| c.is_ascii_digit()).take(14).collect();
        let (date, time) = if digits.len() >= 12 {
            (
                format!("{}.{}.{}", &digits[6..8], &digits[4..6], &digits[0..4]),
                format!("{}:{}", &digits[8..10], &digits[10..12]),
            )
        } else {
            (String::new(), String::new())
        };
        crate::marks::MarkContext { file_name: self.display_name.clone(), date, time, ..Default::default() }
    }

    /// The managed marks of the document.
    pub fn marks(&mut self) -> Result<Vec<crate::marks::StoredMark>> {
        self.sync_bytes()?;
        crate::marks::list(&self.bytes, self.password.as_deref())
    }

    /// Adds a mark; returns its id.
    pub fn add_mark(
        &mut self,
        def: &crate::marks::MarkDef,
        image: Option<crate::marks::ImageData>,
    ) -> Result<String> {
        self.add_mark_with(def, None, image, 0)
    }

    /// Adds a mark with Bates numbers continuing at `bates_offset`.
    pub fn add_mark_with(
        &mut self,
        def: &crate::marks::MarkDef,
        id: Option<&str>,
        image: Option<crate::marks::ImageData>,
        bates_offset: u64,
    ) -> Result<String> {
        self.require(self.permissions().modify)?;
        let mut ctx = self.mark_context();
        ctx.bates_offset = bates_offset;
        let mut new_id = String::new();
        self.edit_bytes("page marks", |b, pw| {
            let (bytes, id) = crate::marks::add(b, pw, def, id, image, &ctx)?;
            new_id = id;
            Ok(bytes)
        })?;
        Ok(new_id)
    }

    /// Removes marks of `kind` (all, or the given ids). With `foreign` also
    /// matching marks of other programs. Returns the number of removed marks.
    pub fn remove_marks(
        &mut self,
        kind: crate::marks::Kind,
        ids: Option<&[String]>,
        foreign: bool,
    ) -> Result<usize> {
        self.require(self.permissions().modify)?;
        let filter = crate::marks::RemoveFilter { kind: Some(kind), ids, foreign };
        let mut removed = 0;
        self.edit_bytes("remove page marks", |b, pw| {
            let (bytes, n) = crate::marks::remove(b, pw, &filter)?;
            removed = n;
            Ok(bytes)
        })?;
        Ok(removed)
    }

    /// Replaces a mark by a new definition (keeping its id and image unless a
    /// new image is given).
    pub fn update_mark(
        &mut self,
        id: &str,
        def: &crate::marks::MarkDef,
        image: Option<crate::marks::ImageData>,
    ) -> Result<()> {
        let stored = self
            .marks()?
            .into_iter()
            .find(|m| m.id == id)
            .ok_or_else(|| Error::Invalid("mark not found".into()))?;
        let image = image.or(stored.image.map(crate::marks::ImageData::Existing));
        let snapshot_len = self.history.undo.len();
        let ids = [id.to_string()];
        self.remove_marks(stored.def.kind, Some(&ids), false)?;
        self.add_mark_with(def, Some(id), image, 0)?;
        // One undo step for remove + add.
        while self.history.undo.len() > snapshot_len + 1 {
            let last = self.history.undo.pop();
            if let (Some(first), Some(_)) = (self.history.undo.last_mut(), last) {
                first.label = "edit page mark".into();
            }
        }
        Ok(())
    }

    /// Redraws the marks of `kind` in the order of `ids`.
    pub fn reorder_marks(&mut self, kind: crate::marks::Kind, ids: &[String]) -> Result<()> {
        let stored: Vec<_> = self.marks()?.into_iter().filter(|m| m.def.kind == kind).collect();
        let snapshot_len = self.history.undo.len();
        self.remove_marks(kind, None, false)?;
        for id in ids {
            if let Some(m) = stored.iter().find(|m| &m.id == id) {
                self.add_mark_with(&m.def, Some(&m.id), m.image.map(crate::marks::ImageData::Existing), 0)?;
            }
        }
        while self.history.undo.len() > snapshot_len + 1 {
            self.history.undo.pop();
        }
        Ok(())
    }

    /// A one-page document showing `page` with the mark applied (preview).
    pub fn mark_preview(
        &self,
        page: usize,
        def: &crate::marks::MarkDef,
        image: Option<crate::marks::ImageData>,
    ) -> Result<Document> {
        let bytes = self.extract_pages_to_bytes(&[page])?;
        let mut def = def.clone();
        def.pages = vec![0];
        let mut ctx = self.mark_context();
        ctx.page_base = page;
        ctx.total = Some(self.page_count());
        // An id of its own: resource names of the page's marks must not clash.
        let (bytes, _) = crate::marks::add(&bytes, None, &def, Some("preview"), image, &ctx)?;
        Self::from_bytes(bytes, "preview.pdf", None)
    }

    // -----------------------------------------------------------------
    // Page shape (step D): cropping, white margins, resizing, margins

    /// White margins of a page (display terms: left, right, top, bottom).
    pub fn white_margins(&self, page: usize, consider_annotations: bool) -> Result<pages::Margins> {
        let geo = self.geometry(page).copied().ok_or(Error::PageOutOfRange(page))?;
        let (w, h) = geo.view_size();
        let pw = 500u32;
        let scale = pw as f32 / w.max(1.0);
        let ph = ((h * scale) as u32).clamp(1, 4000);
        let opts = RenderOptions { annotations: consider_annotations, ..RenderOptions::default() };
        let img = self.render_tile(page, pw, ph, 0, 0, pw, ph, opts)?;
        let (mut x0, mut y0, mut x1, mut y1) = (pw, ph, 0u32, 0u32);
        for y in 0..ph {
            for x in 0..pw {
                let i = ((y * pw + x) * 4) as usize;
                let lum = (u32::from(img.rgba[i]) * 3
                    + u32::from(img.rgba[i + 1]) * 6
                    + u32::from(img.rgba[i + 2]))
                    / 10;
                if lum < 245 {
                    x0 = x0.min(x);
                    x1 = x1.max(x);
                    y0 = y0.min(y);
                    y1 = y1.max(y);
                }
            }
        }
        if x0 > x1 {
            return Ok([0.0; 4]);
        }
        // One pixel of safety so nothing visible is cut.
        let px = 1.0 / scale;
        Ok([
            (x0 as f32 / scale - px).max(0.0),
            ((pw - 1 - x1) as f32 / scale - px).max(0.0),
            (y0 as f32 / scale - px).max(0.0),
            ((ph - 1 - y1) as f32 / scale - px).max(0.0),
        ])
    }

    /// Margins (display terms) that show the whole media box: negative
    /// values that enlarge the visible area up to the media box.
    pub fn media_margins(&self, page: usize) -> Result<pages::Margins> {
        let geo = self.geometry(page).copied().ok_or(Error::PageOutOfRange(page))?;
        let media = self.ask::<Option<RectF>>(Request::MediaBox { page })?.unwrap_or(geo.crop);
        // Map the media box into the view and compare with the visible page.
        let v = geo.pdf_rect_to_view(&media);
        let (w, h) = geo.view_size();
        Ok([v.x0.min(0.0), (w - v.x1).min(0.0), v.y0.min(0.0), (h - v.y1).min(0.0)])
    }

    /// Crops pages by display margins (see [`pages::crop_pages`]).
    pub fn crop_pages_with(
        &mut self,
        items: &[(usize, pages::Margins)],
        opts: pages::CropOptions,
    ) -> Result<()> {
        for (p, _) in items {
            self.check_page(*p)?;
        }
        self.require(self.permissions().modify)?;
        self.edit_bytes("crop pages", |b, pw| pages::crop_pages(b, pw, items, opts))
    }

    /// Removes content objects, comments and/or form fields that lie
    /// completely outside the visible area of `pages`.
    pub fn remove_outside(
        &mut self,
        pages: &[usize],
        content: bool,
        comments: bool,
        fields: bool,
    ) -> Result<usize> {
        for &p in pages {
            self.check_page(p)?;
        }
        self.require(self.permissions().modify)?;
        let mut removed = 0;
        if comments || fields {
            for &page in pages {
                let Some(crop) = self.geometry(page).map(|g| g.crop) else { continue };
                let indices: Vec<usize> = self
                    .annotations(page)?
                    .iter()
                    .enumerate()
                    .filter(|(_, a)| {
                        let wanted = if a.kind == AnnotKind::Widget {
                            fields
                        } else {
                            comments && a.kind != AnnotKind::Popup
                        };
                        wanted && !a.rect.intersects(&crop)
                    })
                    .map(|(i, _)| i)
                    .collect();
                if !indices.is_empty() {
                    removed += indices.len();
                    self.delete_annotations(page, &indices)?;
                }
            }
        }
        if content {
            let crops: Vec<(usize, RectF)> =
                pages.iter().filter_map(|&p| self.geometry(p).map(|g| (p, g.crop))).collect();
            let count: usize =
                self.edit_engine_with("remove cropped content", Request::RemoveOutside { crops })?;
            removed += count;
        }
        Ok(removed)
    }

    /// Crops to a rectangle (PDF space) drawn with the crop tool.
    pub fn crop_to_rect(&mut self, page: usize, rect: RectF) -> Result<()> {
        let geo = self.geometry(page).copied().ok_or(Error::PageOutOfRange(page))?;
        let view = geo.pdf_rect_to_view(&rect.normalized());
        let (w, h) = geo.view_size();
        let m = [view.x0.max(0.0), (w - view.x1).max(0.0), view.y0.max(0.0), (h - view.y1).max(0.0)];
        self.crop_pages_with(&[(page, m)], pages::CropOptions::default())
    }

    /// "Größe ändern" (see [`pages::resize_pages_with`]); returns changed pages.
    pub fn resize_pages_with(&mut self, pages: &[usize], spec: &pages::ResizeSpec) -> Result<usize> {
        for &p in pages {
            self.check_page(p)?;
        }
        self.require(self.permissions().assemble)?;
        let mut changed = 0;
        self.edit_bytes("resize pages", |b, pw| {
            let (bytes, n) = pages::resize_pages_with(b, pw, pages, spec)?;
            changed = n;
            Ok(bytes)
        })?;
        if changed == 0 {
            self.history.undo.pop();
        }
        Ok(changed)
    }

    /// A one-page preview document of `page` resized with `spec`.
    pub fn resize_preview(&self, page: usize, spec: &pages::ResizeSpec) -> Result<Document> {
        let bytes = self.extract_pages_to_bytes(&[page])?;
        let (bytes, _) = pages::resize_pages_with(&bytes, None, &[0], spec)?;
        Self::from_bytes(bytes, "preview.pdf", None)
    }

    /// "Ränder einrichten": display margins per page (negative = smaller).
    pub fn add_page_margins(&mut self, items: &[(usize, pages::Margins)]) -> Result<()> {
        for (p, _) in items {
            self.check_page(*p)?;
        }
        self.require(self.permissions().modify)?;
        self.edit_bytes("page margins", |b, pw| pages::add_margins(b, pw, items))
    }

    // -----------------------------------------------------------------
    // Combining, splitting and overlaying (step C)

    /// A new document with `pages` placed several per sheet.
    pub fn nup_document(&mut self, pages: &[usize], layout: &crate::import::NUpLayout) -> Result<Document> {
        for &p in pages {
            self.check_page(p)?;
        }
        self.sync_bytes()?;
        let src = crate::import::Source::load(&self.bytes, self.password.as_deref())?;
        let bytes = crate::import::nup(&src, pages, layout)?;
        let stem = self.display_name.trim_end_matches(".pdf").trim_end_matches(".PDF").to_string();
        let mut doc = Self::from_bytes(bytes, &format!("{stem} (zusammengeführt).pdf"), None)?;
        doc.modified = true;
        Ok(doc)
    }

    /// Parts of `every` pages.
    pub fn parts_every(&self, every: usize) -> Vec<Vec<usize>> {
        let n = self.page_count();
        let every = every.max(1);
        (0..n).step_by(every).map(|s| (s..(s + every).min(n)).collect()).collect()
    }

    /// Parts no larger than about `max_bytes` (estimated from single pages;
    /// every part has at least one page).
    pub fn parts_by_size(&self, max_bytes: usize) -> Result<Vec<Vec<usize>>> {
        let mut parts: Vec<Vec<usize>> = Vec::new();
        let mut current: Vec<usize> = Vec::new();
        let mut size = 0usize;
        for page in 0..self.page_count() {
            let s = self.extract_pages_to_bytes(&[page])?.len();
            if !current.is_empty() && size + s > max_bytes {
                parts.push(std::mem::take(&mut current));
                size = 0;
            }
            current.push(page);
            size += s;
        }
        if !current.is_empty() {
            parts.push(current);
        }
        Ok(parts)
    }

    /// Parts starting at the bookmarks of the levels 1..=`level` (1 = top
    /// level). Returns (part pages, title of the bookmark starting it).
    pub fn parts_by_bookmarks(&self, level: usize) -> Vec<(Vec<usize>, String)> {
        fn walk(items: &[Bookmark], depth: usize, max: usize, out: &mut Vec<(usize, String)>) {
            for b in items {
                if let Some(p) = b.page {
                    out.push((p, b.title.clone()));
                }
                if depth + 1 < max {
                    walk(&b.children, depth + 1, max, out);
                }
            }
        }
        let mut starts = Vec::new();
        walk(&self.bookmarks(), 0, level.max(1), &mut starts);
        starts.sort_by_key(|s| s.0);
        starts.dedup_by_key(|s| s.0);
        if starts.first().is_none_or(|s| s.0 > 0) {
            starts.insert(0, (0, String::new()));
        }
        let n = self.page_count();
        let mut parts = Vec::new();
        for (i, (start, title)) in starts.iter().enumerate() {
            let end = starts.get(i + 1).map_or(n, |s| s.0).min(n);
            if *start < end {
                parts.push(((*start..end).collect(), title.clone()));
            }
        }
        parts
    }

    /// Cuts pages along lines (see [`pages::split_pages`]).
    pub fn split_pages_at(
        &mut self,
        pages: &[usize],
        xs: &[f32],
        ys: &[f32],
        rtl: bool,
        method: pages::SplitMethod,
    ) -> Result<()> {
        for &p in pages {
            self.check_page(p)?;
        }
        self.require(self.permissions().assemble)?;
        self.edit_bytes("split pages", |b, pw| pages::split_pages(b, pw, pages, xs, ys, rtl, method))
    }

    /// Overlays pages of another PDF; returns the number of changed pages.
    pub fn overlay_from(
        &mut self,
        targets: &[usize],
        source: &Path,
        sources: &[usize],
        opts: &crate::import::OverlayOptions,
    ) -> Result<usize> {
        for &p in targets {
            self.check_page(p)?;
        }
        self.require(self.permissions().modify)?;
        let src = crate::import::Source::load(&read_pdf_file(source)?, None)?;
        if sources.iter().any(|&p| p >= src.page_count()) {
            return Err(Error::Invalid("the other document has not enough pages".into()));
        }
        let mut changed = 0;
        self.edit_bytes("overlay pages", |b, pw| {
            let (bytes, n) = crate::import::overlay(b, pw, targets, &src, sources, opts)?;
            changed = n;
            Ok(bytes)
        })?;
        Ok(changed)
    }

    /// A one-page document showing `page` with the overlay applied (preview).
    pub fn overlay_preview(
        &self,
        page: usize,
        src: &crate::import::Source,
        source_page: usize,
        opts: &crate::import::OverlayOptions,
    ) -> Result<Document> {
        let bytes = self.extract_pages_to_bytes(&[page])?;
        let (bytes, _) = crate::import::overlay(&bytes, None, &[0], src, &[source_page], opts)?;
        Self::from_bytes(bytes, "preview.pdf", None)
    }

    /// Properties of the "Dokumenteigenschaften" pages beyond the metadata.
    pub fn properties(&mut self) -> Result<crate::doc_props::DocProps> {
        self.sync_bytes()?;
        crate::doc_props::read(&self.bytes, self.password.as_deref())
    }

    /// Fonts of the document ("Schriftarten").
    pub fn fonts(&mut self) -> Result<Vec<crate::doc_props::FontEntry>> {
        self.sync_bytes()?;
        crate::doc_props::fonts(&self.bytes, self.password.as_deref())
    }

    /// Writes metadata and properties in one step (one undo entry).
    pub fn set_properties(&mut self, meta: &Metadata, props: &crate::doc_props::DocProps) -> Result<()> {
        self.require(self.permissions().modify)?;
        self.edit_bytes("document properties", |b, pw| {
            let b = pdf_edit::write_metadata(b, pw, meta)?;
            crate::doc_props::write(&b, pw, props)
        })
    }

    /// Security handler information of encrypted documents.
    pub fn security(&self) -> Option<&SecurityInfo> {
        self.security.as_ref()
    }

    /// The version from the file header ("1.7").
    pub fn pdf_version(&self) -> String {
        let head = &self.bytes[..self.bytes.len().min(16)];
        let text = String::from_utf8_lossy(head);
        text.strip_prefix("%PDF-")
            .map(|v| v.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect())
            .unwrap_or_default()
    }

    pub fn set_metadata(&mut self, meta: &Metadata) -> Result<()> {
        self.require(self.permissions().modify)?;
        self.edit_bytes("document properties", |b, pw| pdf_edit::write_metadata(b, pw, meta))
    }

    /// Renders a page at `dpi` and stores it as PNG or JPEG (by extension).
    pub fn export_page_image(&self, page: usize, dpi: f32, dest: &Path) -> Result<()> {
        let geo = self.geometry(page).ok_or(Error::PageOutOfRange(page))?;
        let (w, h) = geo.view_size();
        let scale = dpi.clamp(18.0, 1200.0) / 72.0;
        let pw = ((w * scale).round() as u32).clamp(1, MAX_BITMAP_EDGE);
        let ph = ((h * scale).round() as u32).clamp(1, MAX_BITMAP_EDGE);
        let img = self.render_page(page, pw, ph)?;
        let buffer = image::RgbaImage::from_raw(img.width, img.height, img.rgba)
            .ok_or_else(|| Error::Invalid("bitmap size mismatch".into()))?;
        let ext = dest.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        let mut out = std::io::Cursor::new(Vec::new());
        if ext == "jpg" || ext == "jpeg" {
            image::DynamicImage::ImageRgba8(buffer).to_rgb8().write_to(&mut out, image::ImageFormat::Jpeg)
        } else {
            buffer.write_to(&mut out, image::ImageFormat::Png)
        }
        .map_err(|e| Error::Invalid(e.to_string()))?;
        write_atomic(dest, out.get_ref())
    }
}

/// Largest edge of images embedded as stamps (larger images are scaled down).
const MAX_IMAGE_EDGE: u32 = 2000;

/// Loads an image with strict decoder limits (protection against hostile files).
pub fn load_image(path: &Path) -> Result<pdf_edit::RgbaImage> {
    let mut reader = image::ImageReader::open(path)
        .map_err(|e| Error::io(path, e))?
        .with_guessed_format()
        .map_err(|e| Error::io(path, e))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(12_000);
    limits.max_image_height = Some(12_000);
    limits.max_alloc = Some(512 * 1024 * 1024);
    reader.limits(limits);
    let img = reader.decode().map_err(|e| Error::Invalid(e.to_string()))?;
    let img = if img.width().max(img.height()) > MAX_IMAGE_EDGE {
        img.resize(MAX_IMAGE_EDGE, MAX_IMAGE_EDGE, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let rgba = img.to_rgba8();
    Ok(pdf_edit::RgbaImage { width: rgba.width(), height: rgba.height(), rgba: rgba.into_raw() })
}

/// Computes the page order after moving `pages` before position `to`.
pub fn move_order(n: usize, pages: &[usize], to: usize) -> Result<Vec<usize>> {
    let mut moving: Vec<usize> = pages.to_vec();
    moving.sort_unstable();
    moving.dedup();
    if moving.is_empty() || moving.iter().any(|&p| p >= n) || to > n {
        return Err(Error::Invalid("invalid page move".into()));
    }
    let rest: Vec<usize> = (0..n).filter(|p| !moving.contains(p)).collect();
    let insert_at = rest.iter().filter(|&&p| p < to).count();
    let mut order = rest[..insert_at].to_vec();
    order.extend_from_slice(&moving);
    order.extend_from_slice(&rest[insert_at..]);
    Ok(order)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_order_examples() {
        assert_eq!(move_order(4, &[0], 3).unwrap(), vec![1, 2, 0, 3]);
        assert_eq!(move_order(4, &[3], 0).unwrap(), vec![3, 0, 1, 2]);
        assert_eq!(move_order(4, &[1, 2], 4).unwrap(), vec![0, 3, 1, 2]);
        assert_eq!(move_order(3, &[1], 1).unwrap(), vec![0, 1, 2]);
        assert!(move_order(3, &[5], 0).is_err());
        assert!(move_order(3, &[], 0).is_err());
    }

    #[test]
    fn undo_steps_move_to_disk_instead_of_being_dropped() {
        let mut h = History { memory_limit: 250, ..History::default() };
        for i in 0..6u8 {
            h.push_undo(Snapshot::new(format!("step {i}"), vec![i; 100]));
        }
        assert_eq!(h.undo.len(), 6, "no step dropped");
        let in_memory = h.undo.iter().filter(|s| matches!(s.data, Stored::Memory(_))).count();
        assert!(in_memory <= 2, "{in_memory} steps in memory");
        assert!(matches!(h.undo.last().unwrap().data, Stored::Memory(_)), "the newest stays in memory");
        for (i, s) in h.undo.iter().enumerate() {
            assert_eq!(s.data.read().unwrap(), vec![i as u8; 100], "step {i}");
        }
        let files: Vec<PathBuf> = h
            .undo
            .iter()
            .filter_map(|s| match &s.data {
                Stored::Disk(f) => Some(f.path.clone()),
                Stored::Memory(_) => None,
            })
            .collect();
        assert_eq!(files.len(), 6 - in_memory);
        assert!(files.iter().all(|p| p.exists()));
        drop(h);
        assert!(files.iter().all(|p| !p.exists()), "files are removed with the history");
    }

    #[test]
    fn disk_limit_drops_the_oldest_steps() {
        let mut h = History { memory_limit: 0, disk_limit: 250, ..History::default() };
        for i in 0..6u8 {
            h.push_undo(Snapshot::new(format!("step {i}"), vec![i; 100]));
        }
        assert_eq!(h.undo.len(), 2);
        assert_eq!(h.undo[0].label, "step 4");
        assert_eq!(h.undo[1].data.read().unwrap(), vec![5u8; 100]);
    }

    #[test]
    fn atomic_write_replaces_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.pdf");
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"two");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
