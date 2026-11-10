//! The engine side: everything VersoloPDF does with PDFium.
//!
//! In protected mode this code runs in a separate helper process that has
//! no access to the user's files, the network or other programs (see
//! [`super::sandbox`]); it only sees the documents the program sends it.
//! Without protection it runs inside the program (see [`super::client`]).

use std::collections::HashMap;

use pdfium_render::prelude::*;

use super::proto::{
    Bytes, DrawSpec, Fault, Hello, LoadInfo, PROTOCOL, Picture, Request, TextOrigin, WireClip, answer_err,
    answer_ok, read_frame, write_frame,
};
use crate::content::{
    Affine, Align, BlendMode, ContentObject, FontChoice, ObjectKind, ObjectStyle, bounds, win_ansi_only,
    wrap_text,
};
use crate::error::{Error, Result};
use crate::geometry::{PointF, RectF, Rotation};
use crate::model::{
    AnnotFilter, AnnotKind, AnnotationInfo, Bookmark, FieldKind, FormFieldInfo, LinkInfo, LinkTarget,
    Metadata, Rgba,
};
use crate::text::{CharInfo, PageText};

/// Argument that starts the program as engine helper.
pub const ENGINE_ARG: &str = "--pdf-engine";

const MAX_BOOKMARKS: usize = 20_000;
const MAX_BOOKMARK_DEPTH: usize = 32;
/// Largest bitmap edge length that will be rendered.
const MAX_BITMAP_EDGE: u32 = 16_384;

/// The documents of one helper: the open document and its password.
#[derive(Default)]
pub struct Host {
    pdf: Option<PdfDocument<'static>>,
    password: Option<String>,
}

fn pdfium() -> Result<&'static Pdfium> {
    super::pdfium_lib::pdfium()
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

fn load(bytes: Vec<u8>, password: Option<&str>) -> Result<PdfDocument<'static>> {
    pdfium()?.load_pdf_from_byte_vec(bytes, password).map_err(|e| map_load_error(e, password.is_some()))
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
        T::Screen | T::ThreeD | T::Sound | T::RichMedia | T::Movie => AnnotKind::Media,
        _ => AnnotKind::Other,
    }
}

fn to_rgba(c: PdfColor) -> Rgba {
    Rgba { r: c.red(), g: c.green(), b: c.blue(), a: c.alpha() }
}

fn to_pdf_color(c: Rgba) -> PdfColor {
    PdfColor::new(c.r, c.g, c.b, c.a)
}

fn to_matrix(m: Affine) -> PdfMatrix {
    let [a, b, c, d, e, f] = m.0;
    PdfMatrix::new(a, b, c, d, e, f)
}

fn from_matrix(m: &PdfMatrix) -> Affine {
    Affine([m.a(), m.b(), m.c(), m.d(), m.e(), m.f()])
}

fn kind_of(t: PdfPageObjectType) -> ObjectKind {
    match t {
        PdfPageObjectType::Text => ObjectKind::Text,
        PdfPageObjectType::Image => ObjectKind::Image,
        PdfPageObjectType::Path => ObjectKind::Path,
        PdfPageObjectType::Shading => ObjectKind::Shading,
        PdfPageObjectType::XObjectForm => ObjectKind::Form,
        _ => ObjectKind::Other,
    }
}

fn blend(b: BlendMode) -> PdfPageObjectBlendMode {
    match b {
        BlendMode::Normal => PdfPageObjectBlendMode::Normal,
        BlendMode::Multiply => PdfPageObjectBlendMode::Multiply,
        BlendMode::Screen => PdfPageObjectBlendMode::Screen,
        BlendMode::Overlay => PdfPageObjectBlendMode::Overlay,
        BlendMode::Darken => PdfPageObjectBlendMode::Darken,
        BlendMode::Lighten => PdfPageObjectBlendMode::Lighten,
        BlendMode::Difference => PdfPageObjectBlendMode::Difference,
        BlendMode::Exclusion => PdfPageObjectBlendMode::Exclusion,
    }
}

/// Font look read from a PDF font: (name without subset prefix, bold,
/// italic, serif, monospace).
fn font_look(font: &PdfFont) -> (String, bool, bool, bool, bool) {
    let name = font.name();
    let base = name.split_once('+').map_or(name.as_str(), |(_, r)| r).to_string();
    let lower = base.to_lowercase();
    let bold = font.weight().is_ok_and(|w| {
        matches!(
            w,
            PdfFontWeight::Weight600
                | PdfFontWeight::Weight700Bold
                | PdfFontWeight::Weight800
                | PdfFontWeight::Weight900
        ) || matches!(w, PdfFontWeight::Custom(v) if v >= 600)
    }) || font.is_bold_reenforced()
        || lower.contains("bold")
        || lower.contains("black")
        || lower.contains("heavy");
    let italic = font.is_italic() || lower.contains("italic") || lower.contains("oblique");
    let mono = font.is_fixed_pitch() || lower.contains("courier") || lower.contains("mono");
    const SERIF: [&str; 14] = [
        "times",
        "roman",
        "nimbusrom",
        "georgia",
        "garamond",
        "cambria",
        "minion",
        "palatino",
        "palladio",
        "baskerville",
        "century",
        "bookman",
        "antiqua",
        "caslon",
    ];
    let serif = !mono
        && (font.is_serif()
            || SERIF.iter().any(|s| lower.contains(s))
            || lower.contains("serif") && !lower.contains("sans"));
    (base, bold, italic, serif, mono)
}

/// A standard font for a family and style.
fn standard_font(
    fonts: &mut PdfFonts,
    choice: &FontChoice,
    serif: bool,
    mono: bool,
    bold: bool,
    italic: bool,
) -> PdfFontToken {
    let family = match choice {
        FontChoice::Times => 1,
        FontChoice::Courier => 2,
        FontChoice::Helvetica => 0,
        FontChoice::Original if mono => 2,
        FontChoice::Original if serif => 1,
        FontChoice::Original => 0,
    };
    match (family, bold, italic) {
        (1, false, false) => fonts.times_roman(),
        (1, true, false) => fonts.times_bold(),
        (1, false, true) => fonts.times_italic(),
        (1, true, true) => fonts.times_bold_italic(),
        (2, false, false) => fonts.courier(),
        (2, true, false) => fonts.courier_bold(),
        (2, false, true) => fonts.courier_oblique(),
        (2, true, true) => fonts.courier_bold_oblique(),
        (_, false, false) => fonts.helvetica(),
        (_, true, false) => fonts.helvetica_bold(),
        (_, false, true) => fonts.helvetica_oblique(),
        (_, true, true) => fonts.helvetica_bold_oblique(),
    }
}

fn normalize(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn spec_of(pages: &[usize]) -> String {
    pages.iter().map(|p| (p + 1).to_string()).collect::<Vec<_>>().join(",")
}

impl Host {
    pub fn new() -> Self {
        Self::default()
    }

    fn doc(&self) -> Result<&PdfDocument<'static>> {
        self.pdf.as_ref().ok_or_else(|| Error::Invalid("no document loaded".into()))
    }

    fn doc_mut(&mut self) -> Result<&mut PdfDocument<'static>> {
        self.pdf.as_mut().ok_or_else(|| Error::Invalid("no document loaded".into()))
    }

    fn page(&self, index: usize) -> Result<PdfPage<'static>> {
        let pdf = self.doc()?;
        if index >= pdf.pages().len() as usize {
            return Err(Error::PageOutOfRange(index));
        }
        Ok(pdf.pages().get(page_index(index)?)?)
    }

    /// The current document loaded once more (a source for copying pages
    /// within it, or for changes that must not touch it).
    fn copy_of_document(&self) -> Result<PdfDocument<'static>> {
        let bytes = self.doc()?.save_to_bytes()?;
        load(bytes, self.password.as_deref())
    }

    /// Runs a request and encodes its answer (without the status byte).
    pub fn handle(&mut self, req: Request) -> Result<Vec<u8>> {
        use Request as R;
        match req {
            R::Load { bytes, password } => {
                let pdf = load(bytes.0, password.as_deref())?;
                self.password = password;
                self.pdf = Some(pdf);
                answer_ok(&self.load_info()?)
            }
            R::CreateBlank { sizes } => {
                let mut pdf = pdfium()?.create_new_pdf()?;
                for (w, h) in sizes {
                    pdf.pages_mut().create_page_at_end(PdfPagePaperSize::new_custom(
                        PdfPoints::new(w),
                        PdfPoints::new(h),
                    ))?;
                }
                answer_ok(&Bytes(pdf.save_to_bytes()?))
            }
            R::Combine { sources } => {
                let mut target = pdfium()?.create_new_pdf()?;
                for source in sources {
                    let source = load(source.0, None)?;
                    let count = source.pages().len();
                    if count > 0 {
                        let at = target.pages().len();
                        target.pages_mut().copy_page_range_from_document(&source, 0..=count - 1, at)?;
                    }
                }
                answer_ok(&Bytes(target.save_to_bytes()?))
            }
            R::Save => answer_ok(&Bytes(self.doc()?.save_to_bytes()?)),
            R::Info => answer_ok(&self.load_info()?),
            R::Geometry { page } => answer_ok(&self.geometry(page)?),
            R::PageLabel { page } => answer_ok(&self.page(page)?.label().map(str::to_string)),
            R::Render { page, full, part, spec, filter } => {
                answer_ok(&self.render(page, full, part, spec, filter.as_ref())?)
            }
            R::RenderWithoutText { page, size, annotations } => {
                let copy = self.copy_of_document()?;
                if page >= copy.pages().len() as usize {
                    return Err(Error::PageOutOfRange(page));
                }
                let mut p = copy.pages().get(page_index(page)?)?;
                let objects = p.objects_mut();
                for i in (0..objects.len()).rev() {
                    if objects.get(i).is_ok_and(|o| o.object_type() == PdfPageObjectType::Text) {
                        let _ = objects.remove_object_at_index(i);
                    }
                }
                let spec = DrawSpec {
                    rotation: Rotation::R0,
                    annotations,
                    background: [255, 255, 255],
                    printing: false,
                    transparent: false,
                };
                answer_ok(&render_page_into(&p, size, (0, 0, size.0, size.1), spec)?)
            }
            R::PageText { page } => answer_ok(&self.page_text(page)?),
            R::PageTextStyled { page } => answer_ok(&self.page_text_styled(page)?),
            R::Bookmarks => answer_ok(&self.bookmarks()?),
            R::Links { page } => answer_ok(&self.links(page)?),
            R::Annotations { page } => answer_ok(&self.annotations(page)?),
            R::ObjectAt { page, point } => answer_ok(&self.object_at(page, point)?),
            R::FormFields { page } => answer_ok(&self.form_fields(page)?),
            R::Metadata => answer_ok(&self.metadata()?),
            R::Attachments => {
                let list: Vec<(String, usize)> =
                    self.doc()?.attachments().iter().map(|a| (a.name(), a.len())).collect();
                answer_ok(&list)
            }
            R::Attachment { index } => {
                answer_ok(&Bytes(self.doc()?.attachments().get(index)?.save_to_bytes()?))
            }
            R::PathBounds { page } => {
                let p = self.page(page)?;
                let mut out = Vec::new();
                for object in p.objects().iter().take(20_000) {
                    if object.object_type() != PdfPageObjectType::Path {
                        continue;
                    }
                    if let Ok(quad) = object.bounds() {
                        out.push(to_rectf(&quad.to_rect()));
                    }
                }
                answer_ok(&out)
            }
            R::PageImages { page } => answer_ok(&self.page_images(page)?),
            R::MediaBox { page } => {
                let p = self.page(page)?;
                answer_ok(&p.boundaries().media().ok().map(|b| to_rectf(&b.bounds)))
            }
            R::ExtractPages { spec } => {
                let mut target = pdfium()?.create_new_pdf()?;
                target.pages_mut().copy_pages_from_document(self.doc()?, &spec, 0)?;
                answer_ok(&Bytes(target.save_to_bytes()?))
            }
            R::Flatten { pages } => {
                let pdf = self.doc()?;
                for p in pages {
                    pdf.pages().get(page_index(p)?)?.flatten()?;
                }
                answer_ok(&())
            }
            R::DuplicatePages { spec, at } => {
                let source = self.copy_of_document()?;
                let at = page_index(at)?;
                self.doc_mut()?.pages_mut().copy_pages_from_document(&source, &spec, at)?;
                answer_ok(&())
            }
            R::ReplacePages { pages, source, start, count } => {
                let other = load(source.0, None)?;
                let available = other.pages().len() as usize;
                let count = count.min(available.saturating_sub(start));
                if count == 0 {
                    return Err(Error::Invalid("the other document has not enough pages".into()));
                }
                let first = *pages.iter().min().ok_or_else(|| Error::Invalid("no pages".into()))?;
                let pdf = self.doc_mut()?;
                for &p in pages.iter().rev() {
                    pdf.pages().get(page_index(p)?)?.delete()?;
                }
                pdf.pages_mut().copy_page_range_from_document(
                    &other,
                    page_index(start)?..=page_index(start + count - 1)?,
                    page_index(first)?,
                )?;
                answer_ok(&())
            }
            R::AddAttachment { name, bytes } => {
                self.doc_mut()?.attachments_mut().create_attachment_from_bytes(&name, &bytes.0)?;
                answer_ok(&())
            }
            R::DeleteAttachment { index } => {
                self.doc_mut()?.attachments_mut().delete_at_index(index)?;
                answer_ok(&())
            }
            R::RotatePages { pages } => {
                let pdf = self.doc()?;
                for (p, rot) in pages {
                    pdf.pages().get(page_index(p)?)?.set_rotation(to_pdf_rotation(rot));
                }
                answer_ok(&())
            }
            R::DeletePages { pages } => {
                let mut pages = pages;
                pages.sort_unstable();
                pages.dedup();
                let pdf = self.doc()?;
                for &p in pages.iter().rev() {
                    pdf.pages().get(page_index(p)?)?.delete()?;
                }
                answer_ok(&())
            }
            R::InsertBlank { at, size, count } => {
                let pdf = self.doc_mut()?;
                for _ in 0..count {
                    pdf.pages_mut().create_page_at_index(
                        PdfPagePaperSize::new_custom(PdfPoints::new(size.0), PdfPoints::new(size.1)),
                        page_index(at)?,
                    )?;
                }
                answer_ok(&())
            }
            R::InsertPages { at, source, password, spec } => {
                let other = load(source.0, password.as_deref())?;
                let count = other.pages().len() as usize;
                let pdf = self.doc_mut()?;
                let inserted = match spec {
                    None if count > 0 => {
                        pdf.pages_mut().copy_page_range_from_document(
                            &other,
                            0..=page_index(count - 1)?,
                            page_index(at)?,
                        )?;
                        count
                    }
                    None => 0,
                    Some(pages) => {
                        let pages: Vec<usize> = pages
                            .split(',')
                            .filter_map(|p| p.trim().parse::<usize>().ok())
                            .filter(|p| *p >= 1 && *p <= count)
                            .map(|p| p - 1)
                            .collect();
                        if !pages.is_empty() {
                            pdf.pages_mut().copy_pages_from_document(
                                &other,
                                &spec_of(&pages),
                                page_index(at)?,
                            )?;
                        }
                        pages.len()
                    }
                };
                answer_ok(&inserted)
            }
            R::RemoveOutside { crops } => {
                let pdf = self.doc()?;
                let mut count = 0usize;
                for (p, crop) in &crops {
                    let mut page = pdf.pages().get(page_index(*p)?)?;
                    let objects = page.objects();
                    let mut outside = Vec::new();
                    for i in 0..objects.len() {
                        let Ok(obj) = objects.get(i) else { continue };
                        let Ok(b) = obj.bounds() else { continue };
                        if !to_rectf(&b.to_rect()).intersects(crop) {
                            outside.push(i);
                        }
                    }
                    for i in outside.into_iter().rev() {
                        page.objects_mut().remove_object_at_index(i)?;
                        count += 1;
                    }
                }
                answer_ok(&count)
            }
            R::PageObjects { page } => answer_ok(&self.page_objects(page)?),
            R::ObjectCount { page } => answer_ok(&self.page(page)?.objects().len()),
            R::TransformObjects { page, moves } => {
                self.edit_objects(page, |objects| {
                    for (i, m) in &moves {
                        if *m == Affine::IDENTITY {
                            continue;
                        }
                        objects.get(*i)?.apply_matrix(to_matrix(*m))?;
                    }
                    Ok(())
                })?;
                answer_ok(&())
            }
            R::DeleteObjects { page, indices } => {
                let mut sorted = indices;
                sorted.sort_unstable();
                sorted.dedup();
                self.edit_objects(page, |objects| {
                    for i in sorted.into_iter().rev() {
                        objects.remove_object_at_index(i)?;
                    }
                    Ok(())
                })?;
                answer_ok(&())
            }
            R::StyleObjects { page, indices, style } => {
                self.edit_objects(page, |objects| style_objects(objects, &indices, &style))?;
                answer_ok(&())
            }
            R::ReorderObjects { page, order } => {
                let count = self.page(page)?.objects().len();
                if order.len() != count || {
                    let mut seen = order.clone();
                    seen.sort_unstable();
                    seen.dedup();
                    seen.len() != count || seen.last().is_some_and(|l| *l >= count)
                } {
                    return Err(Error::Invalid("object order".into()));
                }
                self.edit_objects(page, |objects| {
                    // Take every object off the page, then add them back in order.
                    let mut taken: Vec<Option<PdfPageObject>> = Vec::with_capacity(count);
                    for _ in 0..count {
                        taken.push(Some(objects.remove_object_at_index(0)?));
                    }
                    for old in &order {
                        let obj = taken[*old].take().ok_or_else(|| Error::Invalid("object order".into()))?;
                        objects.add_object(obj)?;
                    }
                    Ok(())
                })?;
                answer_ok(&())
            }
            R::CopyObjects { page, indices } => answer_ok(&self.copy_objects(page, &indices)?),
            R::PasteObjects { page, items, offset } => answer_ok(&self.paste_objects(page, &items, offset)?),
            R::ObjectImage { page, index } => {
                let p = self.page(page)?;
                let obj = p.objects().get(index)?;
                let img = obj.as_image_object().ok_or_else(|| Error::Invalid("not an image".into()))?;
                let image = img.get_processed_image(self.doc()?).or_else(|_| img.get_raw_image())?;
                answer_ok(&Picture::from_image(&image))
            }
            R::ReplaceObjectImage { page, index, picture } => {
                let image = picture.to_image().ok_or_else(|| Error::Invalid("invalid image".into()))?;
                self.edit_objects(page, |objects| {
                    let mut obj = objects.get(index)?;
                    let img =
                        obj.as_image_object_mut().ok_or_else(|| Error::Invalid("not an image".into()))?;
                    img.set_image(&image)?;
                    Ok(())
                })?;
                answer_ok(&())
            }
            R::TextOrigin { page, first } => {
                let p = self.page(page)?;
                let objects = p.objects();
                let found = objects.get(first).ok().and_then(|o| {
                    o.as_text_object().map(|t| {
                        let font = t.font();
                        (font.name(), font.is_embedded().unwrap_or(true), font_look(&font))
                    })
                });
                let origin = match found {
                    Some((raw_name, embedded, (_, _, _, serif, mono))) => {
                        TextOrigin { found: true, raw_name, embedded, serif, mono, pos: first }
                    }
                    None => TextOrigin {
                        found: false,
                        raw_name: String::new(),
                        embedded: false,
                        serif: false,
                        mono: false,
                        pos: objects.len(),
                    },
                };
                answer_ok(&origin)
            }
            R::ReplaceText {
                page,
                old,
                text,
                style,
                left,
                baseline,
                width,
                leading,
                keep_font,
                system_font,
            } => {
                let n = self.replace_text(
                    page,
                    &old,
                    &text,
                    &style,
                    (left, baseline, width, leading),
                    keep_font,
                    system_font,
                )?;
                answer_ok(&n)
            }
            R::ProbeRestrictions { file, address } => answer_ok(&probe(&file, &address)),
            // Inside the program this would end the program: refused there.
            R::TestCrash if std::env::args_os().nth(1).is_some_and(|a| a == ENGINE_ARG) => {
                std::process::abort()
            }
            R::TestCrash => Err(Error::Invalid("not in a helper".into())),
        }
    }

    fn load_info(&self) -> Result<LoadInfo> {
        let pdf = self.doc()?;
        let sizes = pdf
            .pages()
            .page_sizes()
            .unwrap_or_default()
            .iter()
            .map(|s| (s.width().value, s.height().value))
            .collect();
        let p = pdf.permissions();
        let all_rights = [
            p.can_print_high_quality(),
            p.can_modify_document_content(),
            p.can_add_or_modify_text_annotations(),
            p.can_assemble_document(),
            p.can_fill_existing_interactive_form_fields(),
        ]
        .iter()
        .all(|r| matches!(r, Ok(true)));
        Ok(LoadInfo {
            pages: pdf.pages().len() as usize,
            sizes,
            unprotected: matches!(p.security_handler_revision(), Ok(PdfSecurityHandlerRevision::Unprotected)),
            all_rights,
        })
    }

    /// Visible area and rotation of a page.
    fn geometry(&self, index: usize) -> Result<(RectF, Rotation)> {
        let page = self.page(index)?;
        let crop = page
            .boundaries()
            .crop()
            .or_else(|_| page.boundaries().media())
            .map(|b| to_rectf(&b.bounds))
            .unwrap_or_else(|_| RectF::new(0.0, 0.0, page.width().value, page.height().value));
        let rotation = page.rotation().map(from_pdf_rotation).unwrap_or_default();
        Ok((crop, rotation))
    }

    fn render(
        &self,
        index: usize,
        full: (u32, u32),
        part: (u32, u32, u32, u32),
        spec: DrawSpec,
        filter: Option<&AnnotFilter>,
    ) -> Result<Picture> {
        let page = self.page(index)?;
        // The comment filter sets the hidden flags only for this rendering
        // and restores them afterwards.
        let mut restore = Vec::new();
        if let Some(filter) = filter.filter(|_| spec.annotations && !spec.printing) {
            for (i, mut annot) in page.annotations().iter().enumerate() {
                let flagged = annot.is_hidden();
                let kind = annot_kind(annot.annotation_type());
                let author = annot.creator().unwrap_or_default();
                let hide = filter.hides(index, i, kind, &author, flagged);
                if hide != flagged && annot.set_is_hidden(hide).is_ok() {
                    restore.push((i, flagged));
                }
            }
        }
        let rendered = render_page_into(&page, full, part, spec);
        for (i, flagged) in restore {
            if let Ok(mut annot) = page.annotations().get(i) {
                let _ = annot.set_is_hidden(flagged);
            }
        }
        rendered
    }

    fn page_text(&self, index: usize) -> Result<PageText> {
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
        Ok(PageText { chars, ..PageText::default() })
    }

    fn page_text_styled(&self, index: usize) -> Result<PageText> {
        let page = self.page(index)?;
        let text = page.text()?;
        let mut out = PageText::default();
        let mut known: HashMap<String, u16> = HashMap::new();
        for c in text.chars().iter() {
            out.chars.push(CharInfo {
                ch: c.unicode_char().unwrap_or('\u{FFFD}'),
                rect: c.loose_bounds().map(|r| to_rectf(&r)).unwrap_or_default(),
                generated: c.is_generated().unwrap_or(false),
            });
            let name = c.font_name();
            let font = match known.get(&name) {
                Some(i) => *i,
                None => {
                    let bold = c.font_weight().is_some_and(|w| {
                        matches!(
                            w,
                            PdfFontWeight::Weight600
                                | PdfFontWeight::Weight700Bold
                                | PdfFontWeight::Weight800
                                | PdfFontWeight::Weight900
                        )
                    }) || c.font_is_bold_reenforced();
                    let i = out.fonts.len().min(u16::MAX as usize) as u16;
                    out.fonts.push(crate::text::FontInfo {
                        name: name.clone(),
                        bold,
                        italic: c.font_is_italic(),
                    });
                    known.insert(name, i);
                    i
                }
            };
            let color = c.fill_color().map(|f| [f.red(), f.green(), f.blue()]).unwrap_or([0, 0, 0]);
            let origin = c.origin().map(|(x, y)| [x.value, y.value]).unwrap_or_default();
            let size = c.scaled_font_size().value;
            out.styles.push(crate::text::CharStyle { font, color, size, origin });
        }
        Ok(out)
    }

    fn bookmarks(&self) -> Result<Vec<Bookmark>> {
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
        collect(self.doc()?.bookmarks().root(), 0, &mut budget, &mut out);
        Ok(out)
    }

    fn links(&self, index: usize) -> Result<Vec<LinkInfo>> {
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

    fn annotations(&self, index: usize) -> Result<Vec<AnnotationInfo>> {
        let page = self.page(index)?;
        let mut out = Vec::new();
        for (i, annot) in page.annotations().iter().enumerate() {
            let Ok(b) = annot.bounds() else { continue };
            out.push(AnnotationInfo {
                page: index,
                index: i,
                kind: annot_kind(annot.annotation_type()),
                rect: to_rectf(&b),
                contents: annot.contents().unwrap_or_default(),
                author: annot.creator().unwrap_or_default(),
                modified: annot
                    .modification_date()
                    .map(|d| crate::pdf_edit::format_pdf_date(&d))
                    .unwrap_or_default(),
                // `stroke_color()` of pdfium-render 0.9 crashes on
                // annotations; colours are read from the file instead.
                color: None,
            });
        }
        Ok(out)
    }

    fn object_at(&self, index: usize, point: PointF) -> Result<Option<RectF>> {
        let page = self.page(index)?;
        let page_area = (page.width().value * page.height().value).max(1.0);
        let mut best: Option<RectF> = None;
        for object in page.objects().iter().take(100_000) {
            let Ok(quad) = object.bounds() else { continue };
            let r = to_rectf(&quad.to_rect());
            let area = r.width() * r.height();
            if !r.inflate(1.0).contains(point) || area > page_area * 0.8 {
                continue;
            }
            if best.is_none_or(|b| area < b.width() * b.height()) {
                best = Some(r);
            }
        }
        Ok(best)
    }

    fn form_fields(&self, index: usize) -> Result<Vec<FormFieldInfo>> {
        let page = self.page(index)?;
        let mut out = Vec::new();
        for (i, annot) in page.annotations().iter().enumerate() {
            let Some(field) = annot.as_form_field() else { continue };
            let Ok(b) = annot.bounds() else { continue };
            let mut info = FormFieldInfo {
                page: index,
                annot_index: i,
                rect: to_rectf(&b),
                kind: FieldKind::Unknown,
                name: field.name().unwrap_or_default(),
                value: String::new(),
                checked: false,
                options: Vec::new(),
                read_only: field.is_read_only(),
                required: field.is_required(),
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

    fn metadata(&self) -> Result<Metadata> {
        let meta = self.doc()?.metadata();
        let get = |tag| meta.get(tag).map(|t| t.value().to_string()).unwrap_or_default();
        let date = |s: String| crate::pdf_edit::format_pdf_date(&s);
        Ok(Metadata {
            title: get(PdfDocumentMetadataTagType::Title),
            author: get(PdfDocumentMetadataTagType::Author),
            subject: get(PdfDocumentMetadataTagType::Subject),
            keywords: get(PdfDocumentMetadataTagType::Keywords),
            creator: get(PdfDocumentMetadataTagType::Creator),
            producer: get(PdfDocumentMetadataTagType::Producer),
            created: date(get(PdfDocumentMetadataTagType::CreationDate)),
            modified: date(get(PdfDocumentMetadataTagType::ModificationDate)),
        })
    }

    fn page_images(&self, index: usize) -> Result<Vec<(RectF, Bytes)>> {
        let page = self.page(index)?;
        let mut out = Vec::new();
        for object in page.objects().iter().take(10_000) {
            let Some(image) = object.as_image_object() else { continue };
            let Ok(quad) = object.bounds() else { continue };
            let rect = to_rectf(&quad.to_rect());
            if rect.width() < 12.0 || rect.height() < 12.0 {
                continue;
            }
            let Ok(raw) = image.get_raw_image() else { continue };
            let raw = if raw.width().max(raw.height()) > 3000 { raw.thumbnail(3000, 3000) } else { raw };
            let mut png = Vec::new();
            if raw.to_rgba8().write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).is_ok() {
                out.push((rect, Bytes(png)));
            }
            if out.len() >= 200 {
                break;
            }
        }
        Ok(out)
    }

    fn page_objects(&self, page: usize) -> Result<Vec<ContentObject>> {
        let p = self.page(page)?;
        let mut out = Vec::new();
        for (index, obj) in p.objects().iter().enumerate() {
            let Ok(q) = obj.bounds() else { continue };
            let quad = [
                PointF::new(q.x1().value, q.y1().value),
                PointF::new(q.x2().value, q.y2().value),
                PointF::new(q.x3().value, q.y3().value),
                PointF::new(q.x4().value, q.y4().value),
            ];
            let matrix = obj.matrix().map(|m| from_matrix(&m)).unwrap_or_default();
            let mut o = ContentObject {
                index,
                kind: kind_of(obj.object_type()),
                quad,
                rect: bounds(&quad),
                matrix,
                fill: obj.fill_color().ok().map(to_rgba),
                stroke: obj.stroke_color().ok().map(to_rgba),
                stroke_width: obj.stroke_width().map_or(0.0, |w| w.value),
                text: String::new(),
                font: String::new(),
                font_size: 0.0,
                bold: false,
                italic: false,
                serif: false,
                monospace: false,
                invisible: false,
            };
            if let Some(t) = obj.as_text_object() {
                o.text = t.text().trim_end().to_string();
                let (name, bold, italic, serif, mono) = font_look(&t.font());
                o.font = name;
                o.bold = bold;
                o.italic = italic;
                o.serif = serif;
                o.monospace = mono;
                o.font_size = t.unscaled_font_size().value * matrix.scale_y();
                o.invisible = !t.is_visible();
            }
            out.push(o);
        }
        Ok(out)
    }

    /// Runs `f` on the objects of `page` and writes the page content anew.
    fn edit_objects(&mut self, page: usize, f: impl FnOnce(&mut PdfPageObjects) -> Result<()>) -> Result<()> {
        let mut p = self.page(page)?;
        f(p.objects_mut())?;
        // Some changes (colours) are only written when the content is regenerated.
        p.regenerate_content()?;
        Ok(())
    }

    fn copy_objects(&self, page: usize, indices: &[usize]) -> Result<Vec<WireClip>> {
        let p = self.page(page)?;
        let objects = p.objects();
        let mut items = Vec::new();
        for &i in indices {
            let obj = objects.get(i)?;
            let matrix = obj.matrix().map(|m| from_matrix(&m)).unwrap_or_default();
            if let Some(t) = obj.as_text_object() {
                let (font, bold, italic, serif, mono) = font_look(&t.font());
                items.push(WireClip::Text {
                    text: t.text().trim_end().to_string(),
                    font,
                    size: t.unscaled_font_size().value,
                    color: obj.fill_color().map(to_rgba).unwrap_or(Rgba::BLACK),
                    bold,
                    italic,
                    serif,
                    mono,
                    matrix,
                });
            } else if let Some(img) = obj.as_image_object() {
                if let Ok(image) = img.get_raw_image() {
                    items.push(WireClip::Image { picture: Picture::from_image(&image), matrix });
                }
            } else if let Some(path) = obj.as_path_object() {
                let segments = path
                    .segments()
                    .raw()
                    .iter()
                    .filter_map(|s| {
                        let kind = match s.segment_type() {
                            PdfPathSegmentType::MoveTo => 0,
                            PdfPathSegmentType::LineTo => 1,
                            PdfPathSegmentType::BezierTo => 2,
                            PdfPathSegmentType::Unknown => return None,
                        };
                        Some((kind, s.x().value, s.y().value, s.is_close()))
                    })
                    .collect();
                let fill = path.fill_mode().is_ok_and(|m| m != PdfPathFillMode::None);
                let stroke = path.is_stroked().unwrap_or(false);
                items.push(WireClip::Path {
                    segments,
                    fill: fill.then(|| obj.fill_color().map(to_rgba).unwrap_or(Rgba::BLACK)),
                    stroke: stroke.then(|| obj.stroke_color().map(to_rgba).unwrap_or(Rgba::BLACK)),
                    width: obj.stroke_width().map_or(1.0, |w| w.value),
                    matrix,
                });
            }
        }
        Ok(items)
    }

    /// Adds copied objects on top, moved by `offset`; text keeps its font
    /// when the page already uses one of the same name. Returns the indices
    /// of the new objects.
    fn paste_objects(&mut self, page: usize, items: &[WireClip], offset: (f32, f32)) -> Result<Vec<usize>> {
        if items.is_empty() {
            return Ok(Vec::new());
        }
        let mut fonts: Vec<(String, PdfFontToken)> = Vec::new();
        let before = {
            let p = self.page(page)?;
            for obj in p.objects().iter() {
                if let Some(t) = obj.as_text_object() {
                    let font = t.font();
                    let (name, ..) = font_look(&font);
                    if !fonts.iter().any(|(n, _)| *n == name) {
                        fonts.push((name, font.token()));
                    }
                }
            }
            p.objects().len()
        };
        let move_by = Affine::translate(offset.0, offset.1);
        let idx = page_index(page)?;
        self.add_objects(idx, items, &fonts, move_by)?;
        let after = self.page(page)?.objects().len();
        Ok((before..after).collect())
    }

    fn add_objects(
        &mut self,
        idx: PdfPageIndex,
        items: &[WireClip],
        fonts: &[(String, PdfFontToken)],
        move_by: Affine,
    ) -> Result<()> {
        let pdf = self.doc_mut()?;
        let mut std_fonts = Vec::new();
        for item in items {
            if let WireClip::Text { font, bold, italic, serif, mono, .. } = item {
                let token = match fonts.iter().find(|(n, _)| n == font) {
                    Some((_, t)) => *t,
                    None => {
                        standard_font(pdf.fonts_mut(), &FontChoice::Original, *serif, *mono, *bold, *italic)
                    }
                };
                std_fonts.push(token);
            }
        }
        let doc: &PdfDocument = pdf;
        let mut p = doc.pages().get(idx)?;
        let mut text_fonts = std_fonts.into_iter();
        for item in items {
            match item {
                WireClip::Text { text, size, color, matrix, .. } => {
                    let token = text_fonts.next().ok_or_else(|| Error::Invalid("font".into()))?;
                    let mut t = PdfPageTextObject::new(doc, text, token, PdfPoints::new(*size))?;
                    t.set_fill_color(to_pdf_color(*color))?;
                    t.apply_matrix(to_matrix(matrix.then(move_by)))?;
                    p.objects_mut().add_text_object(t)?;
                }
                WireClip::Image { picture, matrix } => {
                    let image = picture.to_image().ok_or_else(|| Error::Invalid("invalid image".into()))?;
                    let mut img = PdfPageImageObject::new(doc, &image)?;
                    img.apply_matrix(to_matrix(matrix.then(move_by)))?;
                    p.objects_mut().add_image_object(img)?;
                }
                WireClip::Path { segments, fill, stroke, width, matrix } => {
                    let (x, y) = segments.first().map_or((0.0, 0.0), |s| (s.1, s.2));
                    let mut path = PdfPagePathObject::new(
                        doc,
                        PdfPoints::new(x),
                        PdfPoints::new(y),
                        stroke.map(to_pdf_color),
                        stroke.map(|_| PdfPoints::new(*width)),
                        fill.map(to_pdf_color),
                    )?;
                    let mut bez: Vec<(f32, f32)> = Vec::new();
                    for (kind, sx, sy, close) in segments.iter().skip(1) {
                        match kind {
                            0 => path.move_to(PdfPoints::new(*sx), PdfPoints::new(*sy))?,
                            1 => path.line_to(PdfPoints::new(*sx), PdfPoints::new(*sy))?,
                            _ => {
                                bez.push((*sx, *sy));
                                if bez.len() == 3 {
                                    let pt = |i: usize| (PdfPoints::new(bez[i].0), PdfPoints::new(bez[i].1));
                                    let ((x1, y1), (x2, y2), (x3, y3)) = (pt(0), pt(1), pt(2));
                                    path.bezier_to(x3, y3, x1, y1, x2, y2)?;
                                    bez.clear();
                                }
                            }
                        }
                        if *close {
                            path.close_path()?;
                        }
                    }
                    path.apply_matrix(to_matrix(matrix.then(move_by)))?;
                    p.objects_mut().add_path_object(path)?;
                }
            }
        }
        Ok(())
    }

    /// Replaces the text objects `old` by `text`; returns the number of new
    /// objects (they start at the index of the first old one).
    #[allow(clippy::too_many_arguments)]
    fn replace_text(
        &mut self,
        page: usize,
        old: &[usize],
        text: &str,
        style: &crate::content::TextStyle,
        (left, baseline, width, leading): (f32, f32, f32, f32),
        keep_font: bool,
        system_font: Option<Bytes>,
    ) -> Result<usize> {
        let mut old = old.to_vec();
        old.sort_unstable();
        old.dedup();
        let idx = page_index(page)?;
        let (orig_font, serif, mono, pos) = {
            let p = self.page(page)?;
            let objects = p.objects();
            let first = old.first().copied().unwrap_or(objects.len());
            let found = objects.get(first).ok().and_then(|o| {
                o.as_text_object().map(|t| {
                    let font = t.font();
                    (font.token(), font_look(&font))
                })
            });
            match found {
                Some((token, (_, _, _, serif, mono))) => (Some(token), serif, mono, first),
                None => (None, false, false, objects.len()),
            }
        };
        let orig_font = orig_font.filter(|_| keep_font);
        let size = style.size.clamp(1.0, 1000.0);
        let pdf = self.doc_mut()?;
        let std = standard_font(pdf.fonts_mut(), &style.font, serif, mono, style.bold, style.italic);
        let system = if win_ansi_only(text) {
            None
        } else {
            system_font.and_then(|b| pdf.fonts_mut().load_true_type_from_bytes(&b.0, true).ok())
        };
        let doc: &PdfDocument = pdf;
        let mut p = doc.pages().get(idx)?;
        // Candidate fonts in order; the first one that shows the text wins.
        let mut candidates = Vec::new();
        if style.font == FontChoice::Original
            && let Some(t) = orig_font
        {
            candidates.push(t);
        }
        if let Some(s) = system {
            candidates.push(s);
        }
        candidates.push(std);
        let measure_with = |token: PdfFontToken| {
            move |s: &str| -> f32 {
                if s.is_empty() {
                    return 0.0;
                }
                PdfPageTextObject::new(doc, s, token, PdfPoints::new(size))
                    .ok()
                    .and_then(|o| o.width().ok())
                    .map_or(s.chars().count() as f32 * size * 0.5, |w| w.value)
            }
        };
        // The old objects go first: new text on top of the same old text
        // would be taken for "fake bold" by the text extraction.
        for i in old.iter().rev() {
            p.objects_mut().remove_object_at_index(*i)?;
        }
        let mut new_count = 0;
        for (n, token) in candidates.iter().enumerate() {
            let measure = measure_with(*token);
            let lines = if width > 1.0 {
                wrap_text(text, width, &measure)
            } else {
                text.split('\n').map(str::to_string).collect()
            };
            let mut placed = 0;
            let mut ok = true;
            for (k, line) in lines.iter().enumerate() {
                if line.is_empty() {
                    continue;
                }
                let w = measure(line);
                let x = match style.align {
                    Align::Left => left,
                    Align::Center => left + (width.max(w) - w) / 2.0,
                    Align::Right => left + width.max(w) - w,
                };
                let y = baseline - leading * k as f32;
                let mut t = PdfPageTextObject::new(doc, line, *token, PdfPoints::new(size))?;
                t.set_fill_color(to_pdf_color(style.color))?;
                t.translate(PdfPoints::new(x), PdfPoints::new(y))?;
                let added = p.objects_mut().insert_object_at_index(pos + placed, t.into())?;
                placed += 1;
                // The font must show every character (subset fonts often lack some).
                let shown = added.as_text_object().map(|t| t.text()).unwrap_or_default();
                if normalize(&shown) != normalize(line) {
                    ok = false;
                }
            }
            let last = n + 1 == candidates.len();
            if ok || last {
                new_count = placed;
                break;
            }
            for _ in 0..placed {
                p.objects_mut().remove_object_at_index(pos)?;
            }
        }
        Ok(new_count)
    }
}

fn render_page_into(
    page: &PdfPage<'static>,
    (full_w, full_h): (u32, u32),
    (x, y, w, h): (u32, u32, u32, u32),
    spec: DrawSpec,
) -> Result<Picture> {
    let limit = MAX_BITMAP_EDGE * 8;
    if w == 0 || h == 0 || w > MAX_BITMAP_EDGE || h > MAX_BITMAP_EDGE || full_w > limit || full_h > limit {
        return Err(Error::Invalid(format!("invalid render size {w}x{h}")));
    }
    let to_px = |v: u32| Pixels::try_from(v).map_err(|_| Error::Invalid("render size".into()));
    let mut bitmap = PdfBitmap::empty(to_px(w)?, to_px(h)?, PdfBitmapFormat::BGRA)?;
    let config = PdfRenderConfig::new()
        .set_fixed_size(to_px(full_w)?, to_px(full_h)?)
        .set_origin(-to_px(x)?, -to_px(y)?)
        // Form data must stay on: without it pdfium-render 0.9 takes a matrix
        // path that ignores the requested size. Form fields are therefore
        // always drawn; `annotations` only hides comments.
        .render_form_data(true)
        .render_annotations(spec.annotations)
        .rotate(to_pdf_rotation(spec.rotation), false)
        .use_print_quality(spec.printing)
        .set_clear_color(PdfColor::new(
            spec.background[0],
            spec.background[1],
            spec.background[2],
            if spec.transparent { 0 } else { 255 },
        ));
    page.render_into_bitmap_with_config(&mut bitmap, &config)?;
    Ok(Picture { width: w, height: h, rgba: Bytes(bitmap.as_rgba_bytes()) })
}

fn style_objects(objects: &mut PdfPageObjects, indices: &[usize], style: &ObjectStyle) -> Result<()> {
    for &i in indices {
        let mut o = objects.get(i)?;
        let text = o.object_type() == PdfPageObjectType::Text;
        if let Some(c) = style.fill {
            let alpha = o.fill_color().map_or(255, |f| f.alpha());
            o.set_fill_color(to_pdf_color(Rgba { a: alpha, ..c }))?;
        }
        if let Some(op) = style.fill_opacity {
            let c = o.fill_color().unwrap_or(PdfColor::new(0, 0, 0, 255));
            o.set_fill_color(PdfColor::new(
                c.red(),
                c.green(),
                c.blue(),
                (op.clamp(0.0, 1.0) * 255.0).round() as u8,
            ))?;
        }
        if !text || style.stroke.is_some() {
            if let Some(c) = style.stroke {
                let alpha = o.stroke_color().map_or(255, |f| f.alpha());
                o.set_stroke_color(to_pdf_color(Rgba { a: alpha, ..c }))?;
            }
            if let Some(op) = style.stroke_opacity {
                let c = o.stroke_color().unwrap_or(PdfColor::new(0, 0, 0, 255));
                o.set_stroke_color(PdfColor::new(
                    c.red(),
                    c.green(),
                    c.blue(),
                    (op.clamp(0.0, 1.0) * 255.0).round() as u8,
                ))?;
            }
        }
        if let Some(w) = style.stroke_width {
            o.set_stroke_width(PdfPoints::new(w.max(0.0)))?;
        }
        if let Some(d) = &style.dash {
            let pts: Vec<PdfPoints> = d.iter().map(|v| PdfPoints::new(*v)).collect();
            o.set_dash_array(&pts, PdfPoints::ZERO)?;
        }
        if let Some(b) = style.blend {
            o.set_blend_mode(blend(b))?;
        }
    }
    Ok(())
}

/// What the helper can do outside its own memory (for the tests of the
/// restrictions): (what, whether it worked). `file` is a file of the user;
/// writing is tried in its folder. (On Windows the helper has a private
/// temporary folder of its own; that is not the user's.)
fn probe(file: &str, address: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    out.push(("read file".into(), std::fs::read(file).is_ok()));
    let folder = std::path::Path::new(file).parent().map(std::path::Path::to_path_buf).unwrap_or_default();
    let target = folder.join(format!("versolopdf-probe-{}", std::process::id()));
    let wrote = std::fs::write(&target, b"x").is_ok();
    std::fs::remove_file(&target).ok();
    out.push(("write file".into(), wrote));
    let connected = address
        .parse::<std::net::SocketAddr>()
        .ok()
        .is_some_and(|a| std::net::TcpStream::connect_timeout(&a, std::time::Duration::from_secs(3)).is_ok());
    out.push(("network".into(), connected));
    let program = if cfg!(windows) { "cmd.exe" } else { "/bin/sh" };
    let started = std::process::Command::new(program)
        .args(if cfg!(windows) { ["/c", "exit"] } else { ["-c", "exit"] })
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok();
    out.push(("start program".into(), started));
    out
}

/// Renders a small page with text in the standard fonts once, so PDFium
/// reads its font lists before the restrictions apply.
fn warm_up() {
    let Ok(pdfium) = pdfium() else { return };
    let Ok(mut pdf) = pdfium.create_new_pdf() else { return };
    let tokens = [pdf.fonts_mut().helvetica(), pdf.fonts_mut().times_roman(), pdf.fonts_mut().courier()];
    let Ok(mut page) = pdf.pages_mut().create_page_at_end(PdfPagePaperSize::a4()) else { return };
    for (i, token) in tokens.into_iter().enumerate() {
        let _ = page.objects_mut().create_text_object(
            PdfPoints::new(50.0),
            PdfPoints::new(700.0 - 30.0 * i as f32),
            "Äpfel & Birnen",
            token,
            PdfPoints::new(12.0),
        );
    }
    let spec = DrawSpec {
        rotation: Rotation::R0,
        annotations: true,
        background: [255, 255, 255],
        printing: false,
        transparent: false,
    };
    let _ = render_page_into(&page, (60, 80), (0, 0, 60, 80), spec);
}

/// Main loop of the helper process: answers requests on stdin/stdout until
/// the program closes the connection. Returns the exit code.
pub fn run_helper() -> i32 {
    let mut input = std::io::stdin().lock();
    let mut output = std::io::stdout().lock();
    // PDFium is loaded and initialised before anything is restricted.
    let ready = pdfium().map(|_| warm_up());
    let (locked, detail) = match super::sandbox::lockdown() {
        Ok(detail) => (true, detail),
        Err(why) => (false, why),
    };
    let hello = Hello { protocol: PROTOCOL, locked, detail };
    let first = match &ready {
        Ok(()) => answer_ok(&hello),
        Err(err) => Ok(answer_err(&Fault::Engine(err.to_string()))),
    };
    if first.and_then(|f| write_frame(&mut output, &f).map_err(|e| Error::Invalid(e.to_string()))).is_err()
        || ready.is_err()
    {
        return 1;
    }
    let mut host = Host::new();
    loop {
        let frame = match read_frame(&mut input) {
            Ok(Some(f)) => f,
            _ => return 0,
        };
        let answer = match super::proto::decode::<Request>(&frame) {
            Ok(req) => {
                // A panic in one request must not leave a half-written answer.
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| host.handle(req))) {
                    Ok(Ok(a)) => a,
                    Ok(Err(e)) => answer_err(&Fault::from(e)),
                    Err(_) => answer_err(&Fault::Engine("internal error of the PDF engine".into())),
                }
            }
            Err(e) => answer_err(&Fault::from(e)),
        };
        if write_frame(&mut output, &answer).is_err() {
            return 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Without restrictions the probe succeeds, so a failing probe in the
    /// helper really shows the restrictions.
    #[test]
    fn probe_works_without_restrictions() {
        let dir = std::env::temp_dir().join(format!("versolopdf-probe-src-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("datei.txt");
        std::fs::write(&file, b"x").unwrap();
        let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let results = probe(&file.display().to_string(), &server.local_addr().unwrap().to_string());
        std::fs::remove_dir_all(&dir).ok();
        for (what, worked) in results {
            assert!(worked, "{what} should work outside the helper");
        }
    }
}
