//! An open PDF document: rendering, inspection and editing.
//!
//! The PDFium document is always the authoritative state. Edits that PDFium
//! supports natively (page rotation, page insertion/removal, form filling)
//! are applied to it directly; structural edits (annotations, page order,
//! metadata) are applied to the serialised bytes by [`crate::pdf_edit`] and
//! the result is reloaded. Every edit records an undo snapshot.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use pdfium_render::prelude::*;

use crate::engine;
use crate::error::{Error, Result};
use crate::forms::{self, FieldValue};
use crate::geometry::{PageGeometry, PointF, RectF, Rotation};
use crate::model::{
    AnnotKind, AnnotStyle, AnnotationInfo, Bookmark, FieldKind, FormFieldInfo, LinkInfo, LinkTarget,
    Metadata, Permissions, SearchOptions,
};
use crate::outline;
use crate::pages;
use crate::pdf_edit::{self, AnnotDetails, AnnotUpdate, NewAnnotation};
use crate::security::{self, ProtectOptions, SecurityInfo};
use crate::text::{CharInfo, PageText};
use crate::xfdf;

/// Files larger than this are refused to protect against memory exhaustion.
pub const MAX_FILE_SIZE: u64 = 1 << 31;

/// Largest bitmap edge length that will be rendered.
pub const MAX_BITMAP_EDGE: u32 = 16_384;

const MAX_UNDO_STEPS: usize = 50;
const MAX_UNDO_BYTES: usize = 512 * 1024 * 1024;
const MAX_BOOKMARKS: usize = 20_000;
const MAX_BOOKMARK_DEPTH: usize = 32;

/// Display options for rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderOptions {
    /// Additional clockwise rotation (view rotation).
    pub rotation: Rotation,
    /// Draw annotations and form fields.
    pub annotations: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self { rotation: Rotation::R0, annotations: true }
    }
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

struct Snapshot {
    label: String,
    bytes: Vec<u8>,
}

#[derive(Default)]
struct History {
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
}

impl History {
    fn push_undo(&mut self, snapshot: Snapshot) {
        self.undo.push(snapshot);
        self.redo.clear();
        self.trim();
    }

    fn trim(&mut self) {
        let mut total: usize = self.undo.iter().map(|s| s.bytes.len()).sum();
        while self.undo.len() > MAX_UNDO_STEPS || (total > MAX_UNDO_BYTES && self.undo.len() > 1) {
            total -= self.undo.remove(0).bytes.len();
        }
    }
}

pub struct Document {
    pdf: PdfDocument<'static>,
    /// Serialised state; stale while `engine_dirty` is set.
    bytes: Vec<u8>,
    engine_dirty: bool,
    path: Option<PathBuf>,
    display_name: String,
    password: Option<String>,
    geometry: Vec<PageGeometry>,
    history: History,
    revision: u64,
    modified: bool,
    /// Security handler details of encrypted documents.
    security: Option<SecurityInfo>,
}

fn map_load_error(err: PdfiumError, had_password: bool) -> Error {
    match err {
        PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError) => {
            if had_password {
                Error::WrongPassword
            } else {
                Error::PasswordRequired
            }
        }
        PdfiumError::PdfiumLibraryInternalError(
            PdfiumInternalError::FormatError | PdfiumInternalError::FileError,
        ) => Error::InvalidFormat,
        other => Error::from(other),
    }
}

fn to_rectf(r: &PdfRect) -> RectF {
    RectF::new(r.left().value, r.bottom().value, r.right().value, r.top().value).normalized()
}

fn page_index(index: usize) -> Result<PdfPageIndex> {
    PdfPageIndex::try_from(index).map_err(|_| Error::PageOutOfRange(index))
}

fn to_pdf_rotation(r: Rotation) -> PdfPageRenderRotation {
    match r {
        Rotation::R0 => PdfPageRenderRotation::None,
        Rotation::R90 => PdfPageRenderRotation::Degrees90,
        Rotation::R180 => PdfPageRenderRotation::Degrees180,
        Rotation::R270 => PdfPageRenderRotation::Degrees270,
    }
}

fn from_pdf_rotation(r: PdfPageRenderRotation) -> Rotation {
    match r {
        PdfPageRenderRotation::None => Rotation::R0,
        PdfPageRenderRotation::Degrees90 => Rotation::R90,
        PdfPageRenderRotation::Degrees180 => Rotation::R180,
        PdfPageRenderRotation::Degrees270 => Rotation::R270,
    }
}

fn annot_kind(t: PdfPageAnnotationType) -> AnnotKind {
    use PdfPageAnnotationType as T;
    match t {
        T::Text => AnnotKind::Note,
        T::FreeText => AnnotKind::FreeText,
        T::Line => AnnotKind::Line,
        T::Square => AnnotKind::Square,
        T::Circle => AnnotKind::Circle,
        T::Polygon => AnnotKind::Polygon,
        T::Polyline => AnnotKind::Polyline,
        T::Highlight => AnnotKind::Highlight,
        T::Underline => AnnotKind::Underline,
        T::Squiggly => AnnotKind::Squiggly,
        T::Strikeout => AnnotKind::StrikeOut,
        T::Stamp => AnnotKind::Stamp,
        T::Caret => AnnotKind::Caret,
        T::Ink => AnnotKind::Ink,
        T::Popup => AnnotKind::Popup,
        T::FileAttachment => AnnotKind::FileAttachment,
        T::Link => AnnotKind::Link,
        T::Widget | T::XfaWidget => AnnotKind::Widget,
        T::Redacted => AnnotKind::Redact,
        _ => AnnotKind::Other,
    }
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
    let tmp = dir.join(format!(".{file_name}.{}-{nonce:x}.mrpdf-tmp", std::process::id()));
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
        let pdf = Self::load(bytes.clone(), password)?;
        let mut doc = Self {
            pdf,
            bytes,
            engine_dirty: false,
            path: None,
            display_name: display_name.to_string(),
            password: password.map(str::to_string),
            geometry: Vec::new(),
            history: History::default(),
            revision: 0,
            modified: false,
            security: None,
        };
        doc.refresh_security();
        doc.refresh_geometry();
        Ok(doc)
    }

    /// Creates a new document with one empty A4 page.
    pub fn new_blank(display_name: &str) -> Result<Self> {
        let pdfium = engine::pdfium()?;
        let mut pdf = pdfium.create_new_pdf()?;
        pdf.pages_mut().create_page_at_end(PdfPagePaperSize::a4())?;
        let bytes = pdf.save_to_bytes()?;
        let mut doc = Self::from_bytes(bytes, display_name, None)?;
        doc.modified = true;
        Ok(doc)
    }

    fn load(bytes: Vec<u8>, password: Option<&str>) -> Result<PdfDocument<'static>> {
        let pdfium = engine::pdfium()?;
        pdfium.load_pdf_from_byte_vec(bytes, password).map_err(|e| map_load_error(e, password.is_some()))
    }

    fn refresh_geometry(&mut self) {
        self.geometry = self
            .pdf
            .pages()
            .iter()
            .map(|page| {
                let crop = page
                    .boundaries()
                    .crop()
                    .or_else(|_| page.boundaries().media())
                    .map(|b| to_rectf(&b.bounds))
                    .unwrap_or_else(|_| RectF::new(0.0, 0.0, page.width().value, page.height().value));
                let rotation = page.rotation().map(from_pdf_rotation).unwrap_or_default();
                PageGeometry::new(crop, rotation)
            })
            .collect();
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

    pub fn is_modified(&self) -> bool {
        self.modified
    }

    pub fn is_encrypted(&self) -> bool {
        self.password.is_some()
            || self.security.is_some()
            || !matches!(
                self.pdf.permissions().security_handler_revision(),
                Ok(PdfSecurityHandlerRevision::Unprotected)
            )
    }

    fn refresh_security(&mut self) {
        let encrypted = !matches!(
            self.pdf.permissions().security_handler_revision(),
            Ok(PdfSecurityHandlerRevision::Unprotected)
        );
        self.security =
            if encrypted { security::security_info(&self.bytes, self.password.as_deref()) } else { None };
    }
    pub fn can_undo(&self) -> Option<&str> {
        self.history.undo.last().map(|s| s.label.as_str())
    }

    pub fn can_redo(&self) -> Option<&str> {
        self.history.redo.last().map(|s| s.label.as_str())
    }

    fn page(&self, index: usize) -> Result<PdfPage<'static>> {
        if index >= self.page_count() {
            return Err(Error::PageOutOfRange(index));
        }
        Ok(self.pdf.pages().get(page_index(index)?)?)
    }

    pub fn page_label(&self, index: usize) -> Option<String> {
        self.page(index).ok().and_then(|p| p.label().map(str::to_string))
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
        let page = self.page(index)?;
        let to_px = |v: u32| Pixels::try_from(v).map_err(|_| Error::Invalid("render size".into()));
        let mut bitmap = PdfBitmap::empty(to_px(w)?, to_px(h)?, PdfBitmapFormat::BGRA)?;
        let config = PdfRenderConfig::new()
            .set_fixed_size(to_px(full_w)?, to_px(full_h)?)
            .set_origin(-to_px(x)?, -to_px(y)?)
            // Form data must stay on: without it pdfium-render 0.9 takes a matrix
            // path that ignores the requested size. Form fields are therefore
            // always drawn; nnotations only hides comments.
            .render_form_data(true)
            .render_annotations(opts.annotations)
            .rotate(to_pdf_rotation(opts.rotation), false)
            .set_clear_color(PdfColor::WHITE);
        page.render_into_bitmap_with_config(&mut bitmap, &config)?;
        Ok(RenderedImage { width: w, height: h, rgba: bitmap.as_rgba_bytes() })
    }

    // -----------------------------------------------------------------
    // Content inspection

    pub fn page_text(&self, index: usize) -> Result<PageText> {
        let page = self.page(index)?;
        let text = page.text()?;
        let chars = text
            .chars()
            .iter()
            .map(|c| CharInfo {
                ch: c.unicode_char().unwrap_or('\u{FFFD}'),
                rect: c.loose_bounds().map(|r| to_rectf(&r)).unwrap_or_default(),
                generated: c.is_generated().unwrap_or(false),
            })
            .collect();
        Ok(PageText { chars })
    }

    pub fn bookmarks(&self) -> Vec<Bookmark> {
        fn collect(
            first: Option<PdfBookmark<'_>>,
            depth: usize,
            budget: &mut usize,
            out: &mut Vec<Bookmark>,
        ) {
            let mut current = first;
            while let Some(bookmark) = current {
                if *budget == 0 {
                    return;
                }
                *budget -= 1;
                let page = bookmark
                    .destination()
                    .and_then(|d| d.page_index().ok())
                    .or_else(|| {
                        bookmark.action().and_then(|a| {
                            a.as_local_destination_action()
                                .and_then(|l| l.destination().ok())
                                .and_then(|d| d.page_index().ok())
                        })
                    })
                    .and_then(|i| usize::try_from(i).ok());
                let mut children = Vec::new();
                if depth < MAX_BOOKMARK_DEPTH {
                    collect(bookmark.first_child(), depth + 1, budget, &mut children);
                }
                out.push(Bookmark { title: bookmark.title().unwrap_or_default(), page, children });
                current = bookmark.next_sibling();
            }
        }
        let mut out = Vec::new();
        let mut budget = MAX_BOOKMARKS;
        collect(self.pdf.bookmarks().root(), 0, &mut budget, &mut out);
        out
    }

    pub fn links(&self, index: usize) -> Result<Vec<LinkInfo>> {
        let page = self.page(index)?;
        let mut out = Vec::new();
        for link in page.links().iter() {
            let Ok(rect) = link.rect() else { continue };
            let target = if let Some(page) =
                link.destination().and_then(|d| d.page_index().ok()).and_then(|i| usize::try_from(i).ok())
            {
                LinkTarget::Page(page)
            } else if let Some(action) = link.action() {
                if let Some(uri) = action.as_uri_action().and_then(|u| u.uri().ok()) {
                    LinkTarget::Uri(uri)
                } else if let Some(page) = action
                    .as_local_destination_action()
                    .and_then(|l| l.destination().ok())
                    .and_then(|d| d.page_index().ok())
                    .and_then(|i| usize::try_from(i).ok())
                {
                    LinkTarget::Page(page)
                } else {
                    LinkTarget::Unsupported
                }
            } else {
                LinkTarget::Unsupported
            };
            out.push(LinkInfo { rect: to_rectf(&rect), target });
        }
        Ok(out)
    }

    pub fn annotations(&self, index: usize) -> Result<Vec<AnnotationInfo>> {
        let page = self.page(index)?;
        let mut out = Vec::new();
        for (i, annot) in page.annotations().iter().enumerate() {
            let Ok(bounds) = annot.bounds() else { continue };
            out.push(AnnotationInfo {
                page: index,
                index: i,
                kind: annot_kind(annot.annotation_type()),
                rect: to_rectf(&bounds),
                contents: annot.contents().unwrap_or_default(),
                author: annot.creator().unwrap_or_default(),
                modified: annot
                    .modification_date()
                    .map(|d| pdf_edit::format_pdf_date(&d))
                    .unwrap_or_default(),
                // Note: `stroke_color()` of pdfium-render 0.9 calls a page
                // object function on the annotation handle and crashes, so
                // colours are read from the PDF structure on demand instead
                // (see `annotation_details`).
                color: None,
            });
        }
        Ok(out)
    }

    pub fn annotation_details(&mut self, page: usize, index: usize) -> Result<AnnotDetails> {
        self.sync_bytes()?;
        pdf_edit::annotation_details(&self.bytes, self.password.as_deref(), page, index)
    }

    pub fn form_fields(&self, index: usize) -> Result<Vec<FormFieldInfo>> {
        let page = self.page(index)?;
        let mut out = Vec::new();
        for (i, annot) in page.annotations().iter().enumerate() {
            let Some(field) = annot.as_form_field() else { continue };
            let Ok(bounds) = annot.bounds() else { continue };
            let mut info = FormFieldInfo {
                page: index,
                annot_index: i,
                rect: to_rectf(&bounds),
                kind: FieldKind::Unknown,
                name: field.name().unwrap_or_default(),
                value: String::new(),
                checked: false,
                options: Vec::new(),
                read_only: field.is_read_only(),
            };
            if let Some(text) = field.as_text_field() {
                info.kind = FieldKind::Text { multiline: text.is_multiline(), password: text.is_password() };
                info.value = text.value().unwrap_or_default();
            } else if let Some(cb) = field.as_checkbox_field() {
                info.kind = FieldKind::CheckBox;
                info.checked = cb.is_checked().unwrap_or(false);
            } else if let Some(radio) = field.as_radio_button_field() {
                info.kind = FieldKind::RadioButton;
                info.checked = radio.is_checked().unwrap_or(false);
            } else if let Some(combo) = field.as_combo_box_field() {
                info.kind = FieldKind::ComboBox;
                info.value = combo.value().unwrap_or_default();
                info.options = combo.options().iter().filter_map(|o| o.label().cloned()).collect();
            } else if let Some(list) = field.as_list_box_field() {
                info.kind = FieldKind::ListBox;
                info.value = list.value().unwrap_or_default();
                info.options = list.options().iter().filter_map(|o| o.label().cloned()).collect();
            } else if field.as_push_button_field().is_some() {
                info.kind = FieldKind::PushButton;
            } else if field.as_signature_field().is_some() {
                info.kind = FieldKind::Signature;
            }
            out.push(info);
        }
        Ok(out)
    }

    pub fn metadata(&self) -> Metadata {
        let meta = self.pdf.metadata();
        let get = |tag| meta.get(tag).map(|t| t.value().to_string()).unwrap_or_default();
        Metadata {
            title: get(PdfDocumentMetadataTagType::Title),
            author: get(PdfDocumentMetadataTagType::Author),
            subject: get(PdfDocumentMetadataTagType::Subject),
            keywords: get(PdfDocumentMetadataTagType::Keywords),
            creator: get(PdfDocumentMetadataTagType::Creator),
            producer: get(PdfDocumentMetadataTagType::Producer),
            created: pdf_edit::format_pdf_date(&get(PdfDocumentMetadataTagType::CreationDate)),
            modified: pdf_edit::format_pdf_date(&get(PdfDocumentMetadataTagType::ModificationDate)),
        }
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
            let p = self.pdf.permissions();
            let pdfium_all = [
                p.can_print_high_quality(),
                p.can_modify_document_content(),
                p.can_add_or_modify_text_annotations(),
                p.can_assemble_document(),
                p.can_fill_existing_interactive_form_fields(),
            ]
            .iter()
            .all(|r| matches!(r, Ok(true)));
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
            self.bytes = self.pdf.save_to_bytes()?;
            self.engine_dirty = false;
        }
        Ok(())
    }

    /// Returns the current serialised document.
    pub fn to_bytes(&mut self) -> Result<Vec<u8>> {
        self.sync_bytes()?;
        Ok(self.bytes.clone())
    }

    fn reload(&mut self, bytes: Vec<u8>) -> Result<()> {
        let pdf = Self::load(bytes.clone(), self.password.as_deref())?;
        self.pdf = pdf;
        self.bytes = bytes;
        self.engine_dirty = false;
        self.refresh_geometry();
        self.revision += 1;
        Ok(())
    }

    fn mark_changed(&mut self) {
        self.revision += 1;
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
        self.history.push_undo(Snapshot { label: label.to_string(), bytes: old });
        self.modified = true;
        Ok(())
    }

    /// Applies an edit through PDFium.
    fn edit_engine(
        &mut self,
        label: &str,
        f: impl FnOnce(&mut PdfDocument<'static>) -> Result<()>,
    ) -> Result<()> {
        self.sync_bytes()?;
        let snapshot = self.bytes.clone();
        match f(&mut self.pdf) {
            Ok(()) => {
                self.engine_dirty = true;
                self.history.push_undo(Snapshot { label: label.to_string(), bytes: snapshot });
                self.refresh_geometry();
                self.mark_changed();
                Ok(())
            }
            Err(err) => {
                // Discard a possibly half-applied change.
                self.reload(snapshot)?;
                Err(err)
            }
        }
    }

    pub fn undo(&mut self) -> Result<bool> {
        let Some(snapshot) = self.history.undo.pop() else {
            return Ok(false);
        };
        self.sync_bytes()?;
        let current = Snapshot { label: snapshot.label.clone(), bytes: self.bytes.clone() };
        self.reload(snapshot.bytes)?;
        self.history.redo.push(current);
        self.modified = true;
        Ok(true)
    }

    pub fn redo(&mut self) -> Result<bool> {
        let Some(snapshot) = self.history.redo.pop() else {
            return Ok(false);
        };
        self.sync_bytes()?;
        let current = Snapshot { label: snapshot.label.clone(), bytes: self.bytes.clone() };
        self.reload(snapshot.bytes)?;
        self.history.undo.push(current);
        self.modified = true;
        Ok(true)
    }

    /// Saves to the current file.
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
        self.edit_bytes(&label, |b, pw| pdf_edit::add_annotation(b, pw, page, annot))
    }

    pub fn update_annotation(&mut self, page: usize, index: usize, update: &AnnotUpdate) -> Result<()> {
        self.check_page(page)?;
        self.require(self.permissions().annotate)?;
        self.edit_bytes("edit annotation", |b, pw| pdf_edit::update_annotation(b, pw, page, index, update))
    }

    pub fn delete_annotations(&mut self, page: usize, indices: &[usize]) -> Result<()> {
        self.check_page(page)?;
        self.require(self.permissions().annotate)?;
        self.edit_bytes("delete annotations", |b, pw| pdf_edit::delete_annotations(b, pw, page, indices))
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
        self.edit_bytes("delete annotation", |b, pw| pdf_edit::delete_annotation(b, pw, page, index))
    }

    /// Converts all annotations and form fields of the given pages into
    /// regular page content.
    pub fn flatten_pages(&mut self, pages: &[usize]) -> Result<()> {
        for &p in pages {
            self.check_page(p)?;
        }
        self.require(self.permissions().modify)?;
        self.edit_engine("flatten", |pdf| {
            for &p in pages {
                let mut page = pdf.pages().get(page_index(p)?)?;
                page.flatten()?;
            }
            Ok(())
        })
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
        let pdfium = engine::pdfium()?;
        let mut target = pdfium.create_new_pdf()?;
        for path in paths {
            let source = Self::load(read_pdf_file(path)?, None)?;
            let count = source.pages().len();
            if count > 0 {
                let at = target.pages().len();
                target.pages_mut().copy_page_range_from_document(&source, 0..=count - 1, at)?;
            }
        }
        let bytes = target.save_to_bytes()?;
        let mut doc = Self::from_bytes(bytes, display_name, None)?;
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
        self.sync_bytes()?;
        let source = Self::load(self.bytes.clone(), self.password.as_deref())?;
        let spec = pages.iter().map(|p| (p + 1).to_string()).collect::<Vec<_>>().join(",");
        self.edit_engine("duplicate pages", |pdf| {
            pdf.pages_mut().copy_pages_from_document(&source, &spec, page_index(last + 1)?)?;
            Ok(())
        })
    }

    /// Replaces pages (consecutive) by the same number of pages of another
    /// PDF, starting at source_start there.
    pub fn replace_pages(&mut self, pages: &[usize], source: &Path, source_start: usize) -> Result<()> {
        let mut pages = pages.to_vec();
        pages.sort_unstable();
        pages.dedup();
        let (Some(&first), Some(&last)) = (pages.first(), pages.last()) else { return Ok(()) };
        self.check_page(last)?;
        self.require(self.permissions().assemble)?;
        let other = Self::load(read_pdf_file(source)?, None)?;
        let available = usize::try_from(other.pages().len()).unwrap_or(0);
        let count = pages.len().min(available.saturating_sub(source_start));
        if count == 0 {
            return Err(Error::Invalid("the other document has not enough pages".into()));
        }
        self.edit_engine("replace pages", |pdf| {
            for &p in pages.iter().rev() {
                pdf.pages().get(page_index(p)?)?.delete()?;
            }
            pdf.pages_mut().copy_page_range_from_document(
                &other,
                page_index(source_start)?..=page_index(source_start + count - 1)?,
                page_index(first)?,
            )?;
            Ok(())
        })
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

    pub fn field_values(&mut self) -> Result<Vec<(String, String)>> {
        self.sync_bytes()?;
        forms::field_values(&self.bytes, self.password.as_deref())
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
        let headings: Vec<(usize, String)> =
            self.heading_bookmarks()?.into_iter().map(|b| (b.page, b.title)).collect();
        let mut pages = Vec::new();
        for page in 0..self.page_count() {
            let lines = self
                .page_text(page)?
                .lines()
                .into_iter()
                .map(|(t, _)| {
                    let heading = headings.iter().any(|(p, h)| *p == page && *h == t);
                    (t, heading)
                })
                .collect();
            pages.push(lines);
        }
        write_atomic(dest, &crate::convert::docx(&self.display_name, &pages))
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
        let pdfium = engine::pdfium()?;
        let mut target = pdfium.create_new_pdf()?;
        for path in paths {
            let is_pdf = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf"));
            let bytes = if is_pdf {
                read_pdf_file(path)?
            } else {
                crate::convert::image_page_pdf(&load_image(path)?, 150.0)?
            };
            let source = Self::load(bytes, None)?;
            let count = source.pages().len();
            if count > 0 {
                let at = target.pages().len();
                target.pages_mut().copy_page_range_from_document(&source, 0..=count - 1, at)?;
            }
        }
        let bytes = target.save_to_bytes()?;
        let mut doc = Self::from_bytes(bytes, display_name, None)?;
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
        self.pdf.attachments().iter().map(|a| (a.name(), a.len())).collect()
    }

    /// Writes an embedded file to disk. Attachments are never opened or executed.
    pub fn save_attachment(&self, index: usize, dest: &Path) -> Result<()> {
        let index = u16::try_from(index).map_err(|_| Error::Invalid("attachment index".into()))?;
        let bytes = self.pdf.attachments().get(index)?.save_to_bytes()?;
        write_atomic(dest, &bytes)
    }

    pub fn add_attachment(&mut self, source: &Path) -> Result<()> {
        self.require(self.permissions().modify)?;
        let bytes = read_pdf_file(source)?;
        let name = source
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| Error::Invalid("invalid file name".into()))?;
        self.edit_engine("attach file", |pdf| {
            pdf.attachments_mut().create_attachment_from_bytes(&name, &bytes)?;
            Ok(())
        })
    }

    pub fn delete_attachment(&mut self, index: usize) -> Result<()> {
        self.require(self.permissions().modify)?;
        let index = u16::try_from(index).map_err(|_| Error::Invalid("attachment index".into()))?;
        self.edit_engine("delete attachment", |pdf| {
            pdf.attachments_mut().delete_at_index(index)?;
            Ok(())
        })
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
        self.edit_engine("rotate pages", |pdf| {
            for (p, rot) in rotations {
                let mut page = pdf.pages().get(page_index(p)?)?;
                page.set_rotation(to_pdf_rotation(rot));
            }
            Ok(())
        })
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
        self.edit_engine("delete pages", |pdf| {
            for &p in pages.iter().rev() {
                pdf.pages().get(page_index(p)?)?.delete()?;
            }
            Ok(())
        })
    }

    /// Inserts an empty page of the given size (points) before `at`.
    pub fn insert_blank_page(&mut self, at: usize, width: f32, height: f32) -> Result<()> {
        if at > self.page_count() || !(1.0..=14_400.0).contains(&width) || !(1.0..=14_400.0).contains(&height)
        {
            return Err(Error::Invalid("invalid page size or position".into()));
        }
        self.require(self.permissions().assemble)?;
        self.edit_engine("insert page", |pdf| {
            pdf.pages_mut().create_page_at_index(
                PdfPagePaperSize::new_custom(PdfPoints::new(width), PdfPoints::new(height)),
                page_index(at)?,
            )?;
            Ok(())
        })
    }

    /// Inserts all pages of another PDF before `at`.
    pub fn insert_pages_from(&mut self, at: usize, source: &Path, password: Option<&str>) -> Result<usize> {
        if at > self.page_count() {
            return Err(Error::PageOutOfRange(at));
        }
        self.require(self.permissions().assemble)?;
        let other = Self::load(read_pdf_file(source)?, password)?;
        let count = usize::try_from(other.pages().len()).unwrap_or(0);
        if count == 0 {
            return Ok(0);
        }
        self.edit_engine("insert pages", |pdf| {
            pdf.pages_mut().copy_page_range_from_document(
                &other,
                0..=page_index(count - 1)?,
                page_index(at)?,
            )?;
            Ok(())
        })?;
        Ok(count)
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
        let spec = pages.iter().map(|p| (p + 1).to_string()).collect::<Vec<_>>().join(",");
        let pdfium = engine::pdfium()?;
        let mut target = pdfium.create_new_pdf()?;
        target.pages_mut().copy_pages_from_document(&self.pdf, &spec, 0)?;
        Ok(target.save_to_bytes()?)
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
    fn atomic_write_replaces_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.pdf");
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"two");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
