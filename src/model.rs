//! Plain data types describing document content, independent of PDFium.

use serde::{Deserialize, Serialize};

use crate::geometry::{PointF, RectF};

/// An sRGB colour with alpha, each channel 0..=255.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const BLACK: Rgba = Rgba::rgb(0, 0, 0);
    pub const WHITE: Rgba = Rgba::rgb(255, 255, 255);
    pub const RED: Rgba = Rgba::rgb(229, 57, 53);
    pub const BLUE: Rgba = Rgba::rgb(30, 100, 220);
    pub const GREEN: Rgba = Rgba::rgb(46, 160, 67);
    pub const YELLOW: Rgba = Rgba::rgb(255, 235, 59);

    /// Components as PDF colour operands (0.0..=1.0).
    pub fn to_pdf(self) -> [f32; 3] {
        [f32::from(self.r) / 255.0, f32::from(self.g) / 255.0, f32::from(self.b) / 255.0]
    }

    pub fn from_pdf(c: &[f32]) -> Option<Self> {
        let conv = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        match c {
            [g] => Some(Self::rgb(conv(*g), conv(*g), conv(*g))),
            [r, g, b] => Some(Self::rgb(conv(*r), conv(*g), conv(*b))),
            [c, m, y, k] => Some(Self::rgb(
                conv((1.0 - c) * (1.0 - k)),
                conv((1.0 - m) * (1.0 - k)),
                conv((1.0 - y) * (1.0 - k)),
            )),
            _ => None,
        }
    }

    pub fn to_hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }
}

/// Annotation sub types MrPDF knows about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AnnotKind {
    Note,
    FreeText,
    Line,
    Square,
    Circle,
    Polygon,
    Polyline,
    Highlight,
    Underline,
    Squiggly,
    StrikeOut,
    Stamp,
    Caret,
    Ink,
    Popup,
    FileAttachment,
    Link,
    Widget,
    Redact,
    /// Video, audio or 3D (screen, sound, 3D, rich media, movie).
    Media,
    Other,
}

impl AnnotKind {
    /// Whether the annotation is a user comment shown in the comments list.
    pub fn is_comment(self) -> bool {
        !matches!(self, AnnotKind::Popup | AnnotKind::Link | AnnotKind::Widget | AnnotKind::Other)
    }

    /// Whether the annotation's rectangle may be moved by dragging.
    pub fn is_movable(self) -> bool {
        matches!(
            self,
            AnnotKind::Note
                | AnnotKind::FreeText
                | AnnotKind::Line
                | AnnotKind::Square
                | AnnotKind::Circle
                | AnnotKind::Polygon
                | AnnotKind::Polyline
                | AnnotKind::Stamp
                | AnnotKind::Ink
                | AnnotKind::FileAttachment
        )
    }

    pub fn is_text_markup(self) -> bool {
        matches!(
            self,
            AnnotKind::Highlight | AnnotKind::Underline | AnnotKind::Squiggly | AnnotKind::StrikeOut
        )
    }

    pub fn pdf_subtype(self) -> &'static str {
        match self {
            AnnotKind::Note => "Text",
            AnnotKind::FreeText => "FreeText",
            AnnotKind::Line => "Line",
            AnnotKind::Square => "Square",
            AnnotKind::Circle => "Circle",
            AnnotKind::Polygon => "Polygon",
            AnnotKind::Polyline => "PolyLine",
            AnnotKind::Highlight => "Highlight",
            AnnotKind::Underline => "Underline",
            AnnotKind::Squiggly => "Squiggly",
            AnnotKind::StrikeOut => "StrikeOut",
            AnnotKind::Stamp => "Stamp",
            AnnotKind::Caret => "Caret",
            AnnotKind::Ink => "Ink",
            AnnotKind::Popup => "Popup",
            AnnotKind::FileAttachment => "FileAttachment",
            AnnotKind::Link => "Link",
            AnnotKind::Widget => "Widget",
            AnnotKind::Redact => "Redact",
            AnnotKind::Media => "Screen",
            AnnotKind::Other => "Unknown",
        }
    }
}

/// Summary of an existing annotation.
#[derive(Debug, Clone, PartialEq)]
pub struct AnnotationInfo {
    pub page: usize,
    /// Index into the page's `/Annots` array.
    pub index: usize,
    pub kind: AnnotKind,
    /// Bounding box in PDF user space.
    pub rect: RectF,
    pub contents: String,
    pub author: String,
    pub modified: String,
    pub color: Option<Rgba>,
}

/// Which comments are shown ("Anmerkungen > Anzeigen"). Only affects the
/// display; the document is not changed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AnnotFilter {
    pub hidden_kinds: Vec<AnnotKind>,
    pub hidden_authors: Vec<String>,
    /// (page, index) of comments hidden one by one ("Ausgewählte verbergen").
    pub hidden_annots: Vec<(usize, usize)>,
    /// If set, only these comments are shown ("Nur ausgewählte anzeigen").
    pub only: Option<Vec<(usize, usize)>>,
    /// Also show comments that carry the PDF "hidden" flag.
    pub show_flagged: bool,
}

impl AnnotFilter {
    pub fn is_empty(&self) -> bool {
        *self == AnnotFilter::default()
    }

    /// Whether the annotation should be hidden. `flagged` is its own
    /// hidden flag; form fields, links and pop-ups keep it.
    pub fn hides(&self, page: usize, index: usize, kind: AnnotKind, author: &str, flagged: bool) -> bool {
        if !kind.is_comment() {
            return flagged;
        }
        (flagged && !self.show_flagged)
            || self.hidden_kinds.contains(&kind)
            || self.hidden_authors.iter().any(|a| a == author)
            || self.hidden_annots.contains(&(page, index))
            || self.only.as_ref().is_some_and(|only| !only.contains(&(page, index)))
    }
}

/// Visual style for newly created annotations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnnotStyle {
    pub color: Rgba,
    pub fill: Option<Rgba>,
    /// Border / stroke width in points.
    pub width: f32,
    /// 0.0 = invisible, 1.0 = opaque.
    pub opacity: f32,
    pub font_size: f32,
    pub author: String,
}

impl Default for AnnotStyle {
    fn default() -> Self {
        Self {
            color: Rgba::RED,
            fill: None,
            width: 2.0,
            opacity: 1.0,
            font_size: 12.0,
            author: String::new(),
        }
    }
}

/// Geometry of a new vector annotation (all coordinates in PDF user space).
#[derive(Debug, Clone, PartialEq)]
pub enum ShapeGeometry {
    Rectangle(RectF),
    Ellipse(RectF),
    Line { from: PointF, to: PointF, arrow_end: bool },
    Ink(Vec<Vec<PointF>>),
    Polygon(Vec<PointF>),
    Polyline(Vec<PointF>),
}

/// Stamp presets offered in the user interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StampPreset {
    Approved,
    NotApproved,
    Draft,
    Confidential,
    Final,
    ForComment,
    Void,
}

impl StampPreset {
    pub const ALL: [StampPreset; 7] = [
        StampPreset::Approved,
        StampPreset::NotApproved,
        StampPreset::Draft,
        StampPreset::Confidential,
        StampPreset::Final,
        StampPreset::ForComment,
        StampPreset::Void,
    ];

    /// Name used for the `/Name` entry (standard names where possible).
    pub fn pdf_name(self) -> &'static str {
        match self {
            StampPreset::Approved => "Approved",
            StampPreset::NotApproved => "NotApproved",
            StampPreset::Draft => "Draft",
            StampPreset::Confidential => "Confidential",
            StampPreset::Final => "Final",
            StampPreset::ForComment => "ForComment",
            StampPreset::Void => "Void",
        }
    }

    pub fn color(self) -> Rgba {
        match self {
            StampPreset::Approved | StampPreset::Final => Rgba::rgb(30, 130, 60),
            StampPreset::NotApproved | StampPreset::Void | StampPreset::Confidential => {
                Rgba::rgb(200, 30, 30)
            }
            StampPreset::Draft | StampPreset::ForComment => Rgba::rgb(30, 70, 180),
        }
    }
}

/// An entry of the document outline.
#[derive(Debug, Clone, PartialEq)]
pub struct Bookmark {
    pub title: String,
    pub page: Option<usize>,
    pub children: Vec<Bookmark>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LinkTarget {
    Page(usize),
    Uri(String),
    Unsupported,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LinkInfo {
    pub rect: RectF,
    pub target: LinkTarget,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FieldKind {
    Text { multiline: bool, password: bool },
    CheckBox,
    RadioButton,
    ComboBox,
    ListBox,
    PushButton,
    Signature,
    Unknown,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FormFieldInfo {
    pub page: usize,
    pub annot_index: usize,
    pub rect: RectF,
    pub kind: FieldKind,
    pub name: String,
    pub value: String,
    pub checked: bool,
    pub options: Vec<String>,
    pub read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Metadata {
    pub title: String,
    pub author: String,
    pub subject: String,
    pub keywords: String,
    pub creator: String,
    pub producer: String,
    pub created: String,
    pub modified: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Permissions {
    pub encrypted: bool,
    pub print: bool,
    pub modify: bool,
    pub copy: bool,
    pub annotate: bool,
    pub fill_forms: bool,
    pub assemble: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchOptions {
    pub match_case: bool,
    pub whole_word: bool,
}

/// One hit of a text search.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub page: usize,
    /// One rectangle per line the hit spans (PDF space).
    pub rects: Vec<RectF>,
    /// A short excerpt around the hit for the result list.
    pub context: String,
}
