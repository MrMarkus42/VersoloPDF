//! Editing page content: reading the page objects, moving, scaling,
//! rotating, deleting, restyling and reordering them, copying and pasting,
//! images, and replacing text blocks. The work is done by the PDF engine
//! (`engine/host.rs`).

use super::Document;
use crate::content::{Affine, ContentObject, ObjectStyle, TextStyle, ZOrder, z_order};
use crate::engine::proto::{Bytes, Picture, Request, TextOrigin, WireClip};
use crate::error::{Error, Result};
use crate::model::Rgba;

/// Objects copied with "Kopieren" / "Ausschneiden".
#[derive(Debug, Clone, Default)]
pub struct ObjectClip {
    pub items: Vec<ClipItem>,
}

impl ObjectClip {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// One copied object.
#[derive(Debug, Clone)]
pub enum ClipItem {
    Text {
        text: String,
        font: String,
        size: f32,
        color: Rgba,
        bold: bool,
        italic: bool,
        serif: bool,
        mono: bool,
        matrix: Affine,
    },
    Image {
        image: image::DynamicImage,
        matrix: Affine,
    },
    Path {
        segments: Vec<(PathSeg, f32, f32, bool)>,
        fill: Option<Rgba>,
        stroke: Option<Rgba>,
        width: f32,
        matrix: Affine,
    },
}

/// Segment kinds of a copied path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathSeg {
    Move,
    Line,
    Bezier,
}

impl ClipItem {
    fn to_wire(&self) -> WireClip {
        match self {
            ClipItem::Text { text, font, size, color, bold, italic, serif, mono, matrix } => WireClip::Text {
                text: text.clone(),
                font: font.clone(),
                size: *size,
                color: *color,
                bold: *bold,
                italic: *italic,
                serif: *serif,
                mono: *mono,
                matrix: *matrix,
            },
            ClipItem::Image { image, matrix } => {
                WireClip::Image { picture: Picture::from_image(image), matrix: *matrix }
            }
            ClipItem::Path { segments, fill, stroke, width, matrix } => WireClip::Path {
                segments: segments
                    .iter()
                    .map(|(k, x, y, c)| {
                        let k = match k {
                            PathSeg::Move => 0,
                            PathSeg::Line => 1,
                            PathSeg::Bezier => 2,
                        };
                        (k, *x, *y, *c)
                    })
                    .collect(),
                fill: *fill,
                stroke: *stroke,
                width: *width,
                matrix: *matrix,
            },
        }
    }

    fn from_wire(w: WireClip) -> Option<Self> {
        Some(match w {
            WireClip::Text { text, font, size, color, bold, italic, serif, mono, matrix } => {
                ClipItem::Text { text, font, size, color, bold, italic, serif, mono, matrix }
            }
            WireClip::Image { picture, matrix } => ClipItem::Image { image: picture.to_image()?, matrix },
            WireClip::Path { segments, fill, stroke, width, matrix } => ClipItem::Path {
                segments: segments
                    .into_iter()
                    .map(|(k, x, y, c)| {
                        let k = match k {
                            0 => PathSeg::Move,
                            1 => PathSeg::Line,
                            _ => PathSeg::Bezier,
                        };
                        (k, x, y, c)
                    })
                    .collect(),
                fill,
                stroke,
                width,
                matrix,
            },
        })
    }
}

/// A system font with many characters (for text outside WinAnsi). Read by
/// the program: the engine has no access to the user's files.
fn system_font_bytes(bold: bool) -> Option<Vec<u8>> {
    let candidates: &[&str] = if bold {
        &[
            r"C:\Windows\Fonts\arialbd.ttf",
            r"C:\Windows\Fonts\segoeuib.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
            "/Library/Fonts/Arial Bold.ttf",
            "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
        ]
    } else {
        &[
            r"C:\Windows\Fonts\arial.ttf",
            r"C:\Windows\Fonts\segoeui.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/Library/Fonts/Arial.ttf",
            "/System/Library/Fonts/Supplemental/Arial.ttf",
        ]
    };
    candidates.iter().find_map(|p| std::fs::read(p).ok().filter(|b| b.len() < 64 << 20))
}

impl Document {
    /// The objects of a page (back to front).
    pub fn page_objects(&self, page: usize) -> Result<Vec<ContentObject>> {
        self.check_page(page)?;
        let mut list: Vec<ContentObject> = self.ask(Request::PageObjects { page })?;
        // Indices are positions on the page; anything else is refused.
        if list.windows(2).any(|w| w[0].index >= w[1].index) {
            return Err(Error::Pdfium("inconsistent objects from the PDF engine".into()));
        }
        list.truncate(1_000_000);
        Ok(list)
    }

    fn object_count(&self, page: usize) -> Result<usize> {
        self.ask(Request::ObjectCount { page })
    }

    /// Runs a change of the objects of `page` as one undo step.
    fn edit_objects<T: serde::de::DeserializeOwned>(
        &mut self,
        label: &str,
        page: usize,
        req: Request,
    ) -> Result<T> {
        self.require(self.permissions().modify)?;
        self.check_page(page)?;
        self.edit_engine_with(label, req)
    }

    /// Applies `m` to the objects (move, scale, rotate, flip).
    pub fn transform_objects(&mut self, page: usize, indices: &[usize], m: Affine) -> Result<()> {
        self.transform_objects_each(page, &indices.iter().map(|i| (*i, m)).collect::<Vec<_>>())
    }

    /// Applies a matrix per object (arranging).
    pub fn transform_objects_each(&mut self, page: usize, moves: &[(usize, Affine)]) -> Result<()> {
        if moves.is_empty() {
            return Ok(());
        }
        self.edit_objects("move objects", page, Request::TransformObjects { page, moves: moves.to_vec() })
    }

    pub fn delete_objects(&mut self, page: usize, indices: &[usize]) -> Result<()> {
        self.edit_objects("delete objects", page, Request::DeleteObjects { page, indices: indices.to_vec() })
    }

    /// "Format" of objects: colours, line, opacity, blend mode.
    pub fn style_objects(&mut self, page: usize, indices: &[usize], style: &ObjectStyle) -> Result<()> {
        let req = Request::StyleObjects { page, indices: indices.to_vec(), style: style.clone() };
        self.edit_objects("format objects", page, req)
    }

    /// "Z-Order". Returns the new indices of the selected objects.
    pub fn reorder_objects(&mut self, page: usize, indices: &[usize], how: ZOrder) -> Result<Vec<usize>> {
        self.check_page(page)?;
        let count = self.object_count(page)?;
        let order = z_order(count, indices, how);
        if order.iter().enumerate().all(|(i, o)| i == *o) {
            return Ok(indices.to_vec());
        }
        self.edit_objects::<()>(
            "change stacking order",
            page,
            Request::ReorderObjects { page, order: order.clone() },
        )?;
        Ok(order.iter().enumerate().filter(|(_, o)| indices.contains(o)).map(|(i, _)| i).collect())
    }

    /// Copies objects for "Einfügen" (text, images and paths; other kinds are
    /// skipped).
    pub fn copy_objects(&self, page: usize, indices: &[usize]) -> Result<ObjectClip> {
        self.check_page(page)?;
        let items: Vec<WireClip> = self.ask(Request::CopyObjects { page, indices: indices.to_vec() })?;
        Ok(ObjectClip { items: items.into_iter().filter_map(ClipItem::from_wire).collect() })
    }

    /// "Einfügen": adds copied objects on top, moved by `offset`. Text keeps
    /// its font when the page already uses a font of the same name.
    /// Returns the indices of the new objects.
    pub fn paste_objects(
        &mut self,
        page: usize,
        clip: &ObjectClip,
        offset: (f32, f32),
    ) -> Result<Vec<usize>> {
        if clip.is_empty() {
            return Ok(Vec::new());
        }
        let before = self.object_count(page)?;
        let items: Vec<WireClip> = clip.items.iter().map(ClipItem::to_wire).collect();
        let new: Vec<usize> =
            self.edit_objects("paste objects", page, Request::PasteObjects { page, items, offset })?;
        // Only indices of objects that were added.
        Ok(new.into_iter().filter(|i| *i >= before && *i < before + clip.items.len()).collect())
    }

    /// "Duplizieren": copies of the objects, moved by `offset`.
    pub fn duplicate_objects(
        &mut self,
        page: usize,
        indices: &[usize],
        offset: (f32, f32),
    ) -> Result<Vec<usize>> {
        let clip = self.copy_objects(page, indices)?;
        self.paste_objects(page, &clip, offset)
    }

    /// The picture of an image object (masks applied).
    pub fn object_image(&self, page: usize, index: usize) -> Result<image::DynamicImage> {
        self.check_page(page)?;
        let picture: Picture = self.ask(Request::ObjectImage { page, index })?;
        picture.to_image().ok_or_else(|| Error::Pdfium("invalid image from the PDF engine".into()))
    }

    /// "Bild ersetzen": the image object shows `image` in the same place.
    pub fn replace_object_image(
        &mut self,
        page: usize,
        index: usize,
        image: &image::DynamicImage,
    ) -> Result<()> {
        let req = Request::ReplaceObjectImage { page, index, picture: Picture::from_image(image) };
        self.edit_objects("replace image", page, req)
    }

    /// Whether the font `raw_name` of `page` can show every character of
    /// `text`. Fonts that are not embedded are complete (the reader
    /// substitutes them); full embedded TrueType/OpenType fonts are asked
    /// for their character map; subset fonts ("ABCDEF+Name") and fonts that
    /// cannot be read only count the characters the document already shows
    /// in them.
    fn font_covers(&mut self, page: usize, raw_name: &str, embedded: bool, text: &str) -> bool {
        if !embedded {
            return true;
        }
        let (prefix, base) = match raw_name.split_once('+') {
            Some((p, b)) if p.len() == 6 && p.chars().all(|c| c.is_ascii_uppercase()) => (true, b),
            _ => (false, raw_name),
        };
        let mut missing: std::collections::BTreeSet<char> =
            text.chars().filter(|c| !c.is_whitespace()).collect();
        if missing.is_empty() {
            return true;
        }
        if !prefix
            && let Ok(programs) = self.font_programs(page)
            && let Some(data) = programs.get(crate::outlines::base_name(base))
            && let Ok(face) = skrifa::FontRef::new(data)
        {
            use skrifa::MetadataProvider;
            let map = face.charmap();
            return missing.iter().all(|c| map.map(*c).is_some());
        }
        let same = |name: &str| name.split_once('+').map_or(name, |(_, b)| b) == base;
        for p in 0..self.page_count() {
            let Ok(t) = self.page_text_styled(p) else { continue };
            for (i, c) in t.chars.iter().enumerate() {
                let font = t.styles.get(i).and_then(|s| t.fonts.get(s.font as usize));
                if font.is_some_and(|f| same(&f.name)) {
                    missing.remove(&c.ch);
                }
            }
            if missing.is_empty() {
                return true;
            }
        }
        false
    }

    /// "Text bearbeiten": replaces the text objects `old` by `text` set in
    /// `style`. The first line's baseline is at `baseline`, lines are
    /// `leading` apart and wrapped to `width` (0: no wrapping), starting at
    /// `left` (alignment within `width`). Returns the indices of the new
    /// objects.
    #[allow(clippy::too_many_arguments)]
    pub fn replace_text_block(
        &mut self,
        page: usize,
        old: &[usize],
        text: &str,
        style: &TextStyle,
        left: f32,
        baseline: f32,
        width: f32,
        leading: f32,
    ) -> Result<Vec<usize>> {
        self.require(self.permissions().modify)?;
        self.check_page(page)?;
        let mut old = old.to_vec();
        old.sort_unstable();
        old.dedup();
        let count = self.object_count(page)?;
        let first = old.first().copied().unwrap_or(count);
        let origin: TextOrigin = self.ask(Request::TextOrigin { page, first })?;
        // The original font only if it has a glyph for every character.
        let keep_font = origin.found && self.font_covers(page, &origin.raw_name, origin.embedded, text);
        let system_font =
            if crate::content::win_ansi_only(text) { None } else { system_font_bytes(style.bold).map(Bytes) };
        let pos = origin.pos.min(count);
        let req = Request::ReplaceText {
            page,
            old,
            text: text.to_string(),
            style: style.clone(),
            left,
            baseline,
            width,
            leading,
            keep_font,
            system_font,
        };
        let new_count: usize = self.edit_objects("edit text", page, req)?;
        Ok((pos..pos + new_count.min(100_000)).collect())
    }
}
