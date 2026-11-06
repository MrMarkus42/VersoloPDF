//! State of "Objekte bearbeiten" and "Text bearbeiten" in a tab, and the
//! content commands.

use egui::Pos2;

use crate::content::{Affine, Arrange, ObjectFilter, TextBlock, TextStyle, ZOrder};
use crate::geometry::{PointF, RectF};
use crate::ui::icons::Icon;

/// Selection and interaction of the content tools.
#[derive(Debug, Clone, Default)]
pub struct ContentEdit {
    pub page: usize,
    /// Selected object indices on `page`.
    pub selected: Vec<usize>,
    /// Document revision the selection belongs to.
    pub revision: u64,
    pub drag: Option<ObjDrag>,
    pub text: Option<TextEditing>,
    /// "Objekte bearbeiten ▾": which kinds can be selected.
    pub filter: ObjectFilter,
    /// Page and point of the last right click ("Einfügen" there).
    pub menu_at: Option<(usize, PointF)>,
}

impl ContentEdit {
    pub fn select(&mut self, page: usize, indices: Vec<usize>, revision: u64) {
        self.page = page;
        self.selected = indices;
        self.revision = revision;
    }

    pub fn clear(&mut self) {
        self.selected.clear();
        self.drag = None;
    }
}

/// A drag on the page with the object tool.
#[derive(Debug, Clone, PartialEq)]
pub enum ObjDrag {
    /// Moving the selection; screen positions.
    Move { start: Pos2, cur: Pos2 },
    /// Resizing with handle `handle` (0–7 clockwise from top left);
    /// screen rectangles.
    Resize { handle: u8, start: egui::Rect, cur: egui::Rect },
    /// Rotating around the centre (screen); angle in degrees (screen, clockwise).
    Rotate { center: Pos2, start: f32, cur: f32 },
    /// Rubber band selection (screen).
    Rubber { start: Pos2, cur: Pos2 },
}

/// A text block open in the editor on the page.
#[derive(Debug, Clone, PartialEq)]
pub struct TextEditing {
    pub page: usize,
    pub block: TextBlock,
    pub text: String,
    pub style: TextStyle,
    /// Wrap width (0: lines as typed).
    pub width: f32,
    pub focus_requested: bool,
    /// Select the whole text when the editor opens (new sample text).
    pub select_all: bool,
}

impl TextEditing {
    /// Opens `block` of `page` in the editor.
    pub fn open(page: usize, block: TextBlock) -> Self {
        // Several lines keep their width (the text is wrapped again).
        let width = if block.lines.len() > 1 { block.rect.width() } else { 0.0 };
        Self {
            page,
            text: block.text(),
            style: TextStyle::of_block(&block),
            width,
            block,
            focus_requested: false,
            select_all: false,
        }
    }

    pub fn changed(&self) -> bool {
        self.text != self.block.text() || self.style != TextStyle::of_block(&self.block)
    }

    /// The change that applies the edited text.
    pub fn to_op(&self) -> ContentOp {
        let b = &self.block;
        ContentOp::ReplaceText {
            page: self.page,
            objects: b.objects.clone(),
            text: self.text.clone(),
            style: self.style.clone(),
            left: b.left,
            baseline: b.baselines.first().copied().unwrap_or(b.rect.y1 - b.size),
            width: self.width,
            leading: b.leading() * self.style.size / b.size.max(1.0),
        }
    }
}

/// Changes the canvas asks the application to make.
#[derive(Debug, Clone, PartialEq)]
pub enum ContentOp {
    Transform {
        page: usize,
        moves: Vec<(usize, Affine)>,
    },
    ReplaceText {
        page: usize,
        objects: Vec<usize>,
        text: String,
        style: TextStyle,
        left: f32,
        baseline: f32,
        width: f32,
        leading: f32,
    },
    Command(ContentCmd),
    /// "Einfügen" at a point of a page (context menu).
    PasteAt {
        page: usize,
        at: PointF,
    },
}

/// Content commands of the ribbon, the menus and the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContentCmd {
    Cut,
    Copy,
    Paste,
    Delete,
    Duplicate,
    SelectAll,
    /// "Auswählen › Text/Bilder/Formen".
    SelectKind(ObjectFilter),
    Deselect,
    /// "Objekte bearbeiten ▾": tool with a filter.
    Filter(ObjectFilter),
    Arrange(Arrange),
    /// Align to the page instead of the selection.
    AlignToPage,
    RotateLeft,
    RotateRight,
    Rotate180,
    FlipH,
    FlipV,
    Z(ZOrder),
    ExportImage,
    ReplaceImage,
    /// Open the selected text in the text editor.
    EditText,
    /// "Hinzufügen › Text hinzufügen" (page content, not a comment).
    AddText,
    /// "Hinzufügen › Bild hinzufügen…".
    AddImage,
}

impl ContentCmd {
    pub fn names(self) -> (&'static str, &'static str) {
        use ContentCmd::*;
        match self {
            Cut => ("Ausschneiden", "Cut"),
            Copy => ("Kopieren", "Copy"),
            Paste => ("Einfügen", "Paste"),
            Delete => ("Löschen", "Delete"),
            Duplicate => ("Duplizieren", "Duplicate"),
            SelectAll => ("Alle auswählen", "Select all"),
            SelectKind(ObjectFilter::Text) => ("Alle Texte auswählen", "Select all text"),
            SelectKind(ObjectFilter::Images) => ("Alle Bilder auswählen", "Select all images"),
            SelectKind(ObjectFilter::Shapes) => ("Alle Formen auswählen", "Select all shapes"),
            SelectKind(ObjectFilter::All) => ("Alle Objekte auswählen", "Select all objects"),
            Deselect => ("Abwählen", "Deselect"),
            Filter(ObjectFilter::All) => ("Alle", "All"),
            Filter(ObjectFilter::Text) => ("Text", "Text"),
            Filter(ObjectFilter::Images) => ("Bilder", "Images"),
            Filter(ObjectFilter::Shapes) => ("Formen", "Shapes"),
            Arrange(a) => arrange_names(a),
            AlignToPage => ("An Seite ausrichten", "Align to page"),
            RotateLeft => ("90° gegen Uhrzeigersinn drehen", "Rotate 90° counter-clockwise"),
            RotateRight => ("90° im Uhrzeigersinn drehen", "Rotate 90° clockwise"),
            Rotate180 => ("Um 180° drehen", "Rotate 180°"),
            FlipH => ("Horizontal spiegeln", "Flip horizontally"),
            FlipV => ("Vertikal spiegeln", "Flip vertically"),
            Z(ZOrder::Forward) => ("Nach vorn", "Bring forward"),
            Z(ZOrder::Backward) => ("Nach hinten", "Send backward"),
            Z(ZOrder::Front) => ("In den Vordergrund", "Bring to front"),
            Z(ZOrder::Back) => ("In den Hintergrund", "Send to back"),
            ExportImage => ("Bild exportieren…", "Export image…"),
            ReplaceImage => ("Bild ersetzen…", "Replace image…"),
            EditText => ("Text bearbeiten", "Edit text"),
            AddText => ("Text hinzufügen", "Add text"),
            AddImage => ("Bild hinzufügen…", "Add image…"),
        }
    }

    pub fn icon(self) -> Icon {
        use ContentCmd::*;
        match self {
            Cut => Icon::Cut,
            Copy => Icon::Copy,
            Duplicate => Icon::Duplicate,
            Paste => Icon::Paste,
            Delete => Icon::Delete,
            SelectAll | SelectKind(_) | Filter(_) | Deselect => Icon::EditObjects,
            Arrange(a) => arrange_icon(a),
            AlignToPage => Icon::SinglePage,
            RotateLeft => Icon::RotateLeft,
            RotateRight => Icon::RotateRight,
            Rotate180 => Icon::Rotate180,
            FlipH => Icon::FlipH,
            FlipV => Icon::FlipV,
            Z(ZOrder::Forward) => Icon::BringForward,
            Z(ZOrder::Front) => Icon::BringToFront,
            Z(ZOrder::Backward) => Icon::SendBackward,
            Z(ZOrder::Back) => Icon::SendToBack,
            ExportImage => Icon::Export,
            ReplaceImage => Icon::Image,
            EditText => Icon::EditText,
            AddText => Icon::TextBox,
            AddImage => Icon::Image,
        }
    }

    pub fn shortcut(self) -> Option<(&'static str, &'static str)> {
        Some(match self {
            ContentCmd::Cut => ("Strg+X", "Ctrl+X"),
            ContentCmd::Copy => ("Strg+C", "Ctrl+C"),
            ContentCmd::Paste => ("Strg+V", "Ctrl+V"),
            ContentCmd::Delete => ("Entf", "Del"),
            ContentCmd::Duplicate => ("Strg+D", "Ctrl+D"),
            ContentCmd::SelectAll => ("Strg+A", "Ctrl+A"),
            _ => return None,
        })
    }
}

fn arrange_icon(a: Arrange) -> Icon {
    match a {
        Arrange::Left => Icon::AlignLeft,
        Arrange::CenterH => Icon::AlignCenterH,
        Arrange::Right => Icon::AlignRight,
        Arrange::Top => Icon::AlignTop,
        Arrange::CenterV => Icon::AlignCenterV,
        Arrange::Bottom => Icon::AlignBottom,
        Arrange::SpaceH => Icon::SpaceH,
        Arrange::SpaceV => Icon::SpaceV,
        Arrange::DistLeft => Icon::DistLeft,
        Arrange::DistCenterH => Icon::DistCenterH,
        Arrange::DistRight => Icon::DistRight,
        Arrange::DistTop => Icon::DistTop,
        Arrange::DistCenterV => Icon::DistCenterV,
        Arrange::DistBottom => Icon::DistBottom,
        Arrange::SameSize => Icon::SameSize,
        Arrange::SameWidth => Icon::SameWidth,
        Arrange::SameHeight => Icon::SameHeight,
    }
}

fn arrange_names(a: Arrange) -> (&'static str, &'static str) {
    match a {
        Arrange::Left => ("Links ausrichten", "Align left"),
        Arrange::CenterH => ("Horizontal zentrieren", "Centre horizontally"),
        Arrange::Right => ("Rechts ausrichten", "Align right"),
        Arrange::Top => ("Oben ausrichten", "Align top"),
        Arrange::CenterV => ("Vertikal zentrieren", "Centre vertically"),
        Arrange::Bottom => ("Unten ausrichten", "Align bottom"),
        Arrange::SpaceH => ("Abstand horizontal", "Space horizontally"),
        Arrange::SpaceV => ("Abstand vertikal", "Space vertically"),
        Arrange::DistLeft => ("Linke Kanten verteilen", "Distribute left edges"),
        Arrange::DistCenterH => ("Horizontal zentriert verteilen", "Distribute centres horizontally"),
        Arrange::DistRight => ("Rechte Kanten verteilen", "Distribute right edges"),
        Arrange::DistTop => ("Obere Kanten verteilen", "Distribute top edges"),
        Arrange::DistCenterV => ("Vertikal zentriert verteilen", "Distribute centres vertically"),
        Arrange::DistBottom => ("Untere Kanten verteilen", "Distribute bottom edges"),
        Arrange::SameSize => ("Selbe Größe", "Same size"),
        Arrange::SameWidth => ("Selbe Breite", "Same width"),
        Arrange::SameHeight => ("Selbe Höhe", "Same height"),
    }
}

/// Bounding box of rectangles.
pub fn union(rects: impl IntoIterator<Item = RectF>) -> Option<RectF> {
    rects.into_iter().reduce(|a, b| a.union(&b))
}
