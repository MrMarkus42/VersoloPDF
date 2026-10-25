//! An open PDF document: rendering, inspection and editing.
//!
//! The PDFium document is always the authoritative state. Edits that PDFium
//! supports natively (page rotation, page insertion/removal, form filling)
//! are applied to it directly; structural edits (annotations, page order,
//! metadata) are applied to the serialised bytes by [`crate::pdf_edit`] and
//! the result is reloaded. Every edit records an undo snapshot.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use pdfium_render::prelude::*;

use crate::engine;
use crate::forms::{self, FieldValue};
use crate::error::{Error, Result};
use crate::geometry::{PageGeometry, RectF, Rotation};
use crate::model::{
    AnnotKind, AnnotationInfo, Bookmark, FieldKind, FormFieldInfo, LinkInfo, LinkTarget, Metadata,
    Permissions,
};
use crate::pdf_edit::{self, AnnotDetails, AnnotUpdate, NewAnnotation};
use crate::text::{CharInfo, PageText};

/// Files larger than this are refused to protect against memory exhaustion.
pub const MAX_FILE_SIZE: u64 = 1 << 31;

/// Largest bitmap edge length that will be rendered.
pub const MAX_BITMAP_EDGE: u32 = 16_384;

const MAX_UNDO_STEPS: usize = 50;
const MAX_UNDO_BYTES: usize = 512 * 1024 * 1024;
const MAX_BOOKMARKS: usize = 20_000;
const MAX_BOOKMARK_DEPTH: usize = 32;

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
        return Err(Error::FileTooLarge {
            size: meta.len(),
            limit: MAX_FILE_SIZE,
        });
    }
    fs::read(path).map_err(|e| Error::io(path, e))
}

/// Writes `bytes` to `path` atomically (temporary file + rename), so a
/// failure never leaves a half-written document behind.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let file_name = path
        .file_name()
        .ok_or_else(|| Error::Invalid("invalid file name".into()))?
        .to_string_lossy();
    let tmp = dir.join(format!(".{file_name}.{}.mrpdf-tmp", std::process::id()));
    let result = (|| -> std::io::Result<()> {
        let mut file = fs::File::create(&tmp)?;
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
        };
        doc.refresh_geometry();
        Ok(doc)
    }

    /// Creates a new document with one empty A4 page.
    pub fn new_blank(display_name: &str) -> Result<Self> {
        let pdfium = engine::pdfium()?;
        let mut pdf = pdfium.create_new_pdf()?;
        pdf.pages_mut()
            .create_page_at_end(PdfPagePaperSize::a4())?;
        let bytes = pdf.save_to_bytes()?;
        let mut doc = Self::from_bytes(bytes, display_name, None)?;
        doc.modified = true;
        Ok(doc)
    }

    fn load(bytes: Vec<u8>, password: Option<&str>) -> Result<PdfDocument<'static>> {
        let pdfium = engine::pdfium()?;
        pdfium
            .load_pdf_from_byte_vec(bytes, password)
            .map_err(|e| map_load_error(e, password.is_some()))
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
                    .unwrap_or_else(|_| {
                        RectF::new(0.0, 0.0, page.width().value, page.height().value)
                    });
                let rotation = page
                    .rotation()
                    .map(from_pdf_rotation)
                    .unwrap_or_default();
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
            || !matches!(
                self.pdf.permissions().security_handler_revision(),
                Ok(PdfSecurityHandlerRevision::Unprotected) | Err(_)
            )
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
        self.page(index)
            .ok()
            .and_then(|p| p.label().map(str::to_string))
    }

    // -----------------------------------------------------------------
    // Rendering

    /// Renders a whole page into a bitmap of the given size.
    pub fn render_page(&self, index: usize, width: u32, height: u32) -> Result<RenderedImage> {
        self.render_tile(index, width, height, 0, 0, width, height)
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
    ) -> Result<RenderedImage> {
        let limit = MAX_BITMAP_EDGE * 8;
        if w == 0 || h == 0 || w > MAX_BITMAP_EDGE || h > MAX_BITMAP_EDGE || full_w > limit || full_h > limit {
            return Err(Error::Invalid(format!("invalid render size {w}x{h}")));
        }
        let page = self.page(index)?;
        let to_px = |v: u32| Pixels::try_from(v).map_err(|_| Error::Invalid("render size".into()));
        let mut bitmap = PdfBitmap::empty(to_px(w)?, to_px(h)?, PdfBitmapFormat::BGRA)?;
        let config = PdfRenderConfig::new()
            .set_fixed_size(to_px(full_w)?, to_px(full_h)?)
            .set_origin(-to_px(x)?, -to_px(y)?)
            .render_form_data(true)
            .render_annotations(true)
            .set_clear_color(PdfColor::WHITE);
        page.render_into_bitmap_with_config(&mut bitmap, &config)?;
        Ok(RenderedImage {
            width: w,
            height: h,
            rgba: bitmap.as_rgba_bytes(),
        })
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
                rect: c
                    .loose_bounds()
                    .map(|r| to_rectf(&r))
                    .unwrap_or_default(),
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
                out.push(Bookmark {
                    title: bookmark.title().unwrap_or_default(),
                    page,
                    children,
                });
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
            let target = if let Some(page) = link
                .destination()
                .and_then(|d| d.page_index().ok())
                .and_then(|i| usize::try_from(i).ok())
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
            out.push(LinkInfo {
                rect: to_rectf(&rect),
                target,
            });
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
                info.kind = FieldKind::Text {
                    multiline: text.is_multiline(),
                    password: text.is_password(),
                };
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
                info.options = combo
                    .options()
                    .iter()
                    .filter_map(|o| o.label().cloned())
                    .collect();
            } else if let Some(list) = field.as_list_box_field() {
                info.kind = FieldKind::ListBox;
                info.value = list.value().unwrap_or_default();
                info.options = list
                    .options()
                    .iter()
                    .filter_map(|o| o.label().cloned())
                    .collect();
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

    pub fn permissions(&self) -> Permissions {
        let p = self.pdf.permissions();
        Permissions {
            encrypted: self.is_encrypted(),
            print: p.can_print_high_quality().unwrap_or(true)
                || p.can_print_only_low_quality().unwrap_or(true),
            modify: p.can_modify_document_content().unwrap_or(true),
            copy: p.can_extract_text_and_graphics().unwrap_or(true),
            annotate: p.can_add_or_modify_text_annotations().unwrap_or(true),
            fill_forms: p.can_fill_existing_interactive_form_fields().unwrap_or(true),
            assemble: p.can_assemble_document().unwrap_or(true),
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
        self.history.push_undo(Snapshot {
            label: label.to_string(),
            bytes: old,
        });
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
                self.history.push_undo(Snapshot {
                    label: label.to_string(),
                    bytes: snapshot,
                });
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
        let current = Snapshot {
            label: snapshot.label.clone(),
            bytes: self.bytes.clone(),
        };
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
        let current = Snapshot {
            label: snapshot.label.clone(),
            bytes: self.bytes.clone(),
        };
        self.reload(snapshot.bytes)?;
        self.history.undo.push(current);
        self.modified = true;
        Ok(true)
    }

    /// Saves to the current file.
    pub fn save(&mut self) -> Result<()> {
        let path = self
            .path
            .clone()
            .ok_or_else(|| Error::Invalid("document has no file name yet".into()))?;
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

    pub fn add_annotation(&mut self, page: usize, annot: &NewAnnotation) -> Result<()> {
        self.check_page(page)?;
        let label = format!("add {}", annot.kind.pdf_subtype());
        self.edit_bytes(&label, |b, pw| pdf_edit::add_annotation(b, pw, page, annot))
    }

    pub fn update_annotation(&mut self, page: usize, index: usize, update: &AnnotUpdate) -> Result<()> {
        self.check_page(page)?;
        self.edit_bytes("edit annotation", |b, pw| {
            pdf_edit::update_annotation(b, pw, page, index, update)
        })
    }

    pub fn delete_annotation(&mut self, page: usize, index: usize) -> Result<()> {
        self.check_page(page)?;
        self.edit_bytes("delete annotation", |b, pw| {
            pdf_edit::delete_annotation(b, pw, page, index)
        })
    }

    /// Converts all annotations and form fields of the given pages into
    /// regular page content.
    pub fn flatten_pages(&mut self, pages: &[usize]) -> Result<()> {
        for &p in pages {
            self.check_page(p)?;
        }
        self.edit_engine("flatten", |pdf| {
            for &p in pages {
                let mut page = pdf.pages().get(page_index(p)?)?;
                page.flatten()?;
            }
            Ok(())
        })
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
    // -----------------------------------------------------------------
    // Pages

    fn check_page(&self, page: usize) -> Result<()> {
        if page < self.page_count() {
            Ok(())
        } else {
            Err(Error::PageOutOfRange(page))
        }
    }

    /// Rotates the given pages by `delta` degrees (multiple of 90).
    pub fn rotate_pages(&mut self, pages: &[usize], delta: i32) -> Result<()> {
        for &p in pages {
            self.check_page(p)?;
        }
        let rotations: Vec<(usize, Rotation)> = pages
            .iter()
            .map(|&p| (p, self.geometry[p].rotation.rotated_by(delta)))
            .collect();
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
        self.edit_engine("delete pages", |pdf| {
            for &p in pages.iter().rev() {
                pdf.pages().get(page_index(p)?)?.delete()?;
            }
            Ok(())
        })
    }

    /// Inserts an empty page of the given size (points) before `at`.
    pub fn insert_blank_page(&mut self, at: usize, width: f32, height: f32) -> Result<()> {
        if at > self.page_count() || !(1.0..=14_400.0).contains(&width) || !(1.0..=14_400.0).contains(&height) {
            return Err(Error::Invalid("invalid page size or position".into()));
        }
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
        let spec = pages
            .iter()
            .map(|p| (p + 1).to_string())
            .collect::<Vec<_>>()
            .join(",");
        let pdfium = engine::pdfium()?;
        let mut target = pdfium.create_new_pdf()?;
        target
            .pages_mut()
            .copy_pages_from_document(&self.pdf, &spec, 0)?;
        Ok(target.save_to_bytes()?)
    }

    /// Moves `pages` so that they start at position `to` (index in the
    /// document *before* the move).
    pub fn move_pages(&mut self, pages: &[usize], to: usize) -> Result<()> {
        let n = self.page_count();
        let order = move_order(n, pages, to)?;
        if order.iter().enumerate().all(|(i, &p)| i == p) {
            return Ok(());
        }
        self.edit_bytes("move pages", |b, pw| pdf_edit::reorder_pages(b, pw, &order))
    }

    pub fn set_metadata(&mut self, meta: &Metadata) -> Result<()> {
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
        let ext = dest
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        let mut out = std::io::Cursor::new(Vec::new());
        if ext == "jpg" || ext == "jpeg" {
            image::DynamicImage::ImageRgba8(buffer)
                .to_rgb8()
                .write_to(&mut out, image::ImageFormat::Jpeg)
        } else {
            buffer.write_to(&mut out, image::ImageFormat::Png)
        }
        .map_err(|e| Error::Invalid(e.to_string()))?;
        write_atomic(dest, out.get_ref())
    }
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
