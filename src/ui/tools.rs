//! Interactive tools and their default styles.

use serde::{Deserialize, Serialize};

use crate::model::{AnnotKind, AnnotStyle, Rgba};
use crate::ui::icons::Icon;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Tool {
    Hand,
    SelectText,
    SelectAnnot,
    Zoom,
    Snapshot,
    Highlight,
    AreaHighlight,
    Underline,
    StrikeOut,
    Squiggly,
    Replace,
    Caret,
    Note,
    FileAttach,
    TextBox,
    Typewriter,
    Callout,
    Rectangle,
    Ellipse,
    Line,
    Arrow,
    Polygon,
    Polyline,
    Cloud,
    Pencil,
    Eraser,
    Stamp,
    MeasureDistance,
    MeasurePerimeter,
    MeasureArea,
    Link,
    Redact,
    Signature,
    SelectFields,
    FieldText,
    FieldButton,
    FieldCheckBox,
    FieldRadio,
    FieldList,
    FieldCombo,
    FieldSignature,
}

/// How a tool is operated on the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gesture {
    /// Hand, selection and other non-drawing tools.
    None,
    /// Drag over text.
    TextMarkup,
    /// Single click places an object.
    Click,
    /// Drag a rectangle (a click uses a default size).
    Rect,
    /// Drag a line from start to end.
    Line,
    /// Free drawing.
    Freehand,
    /// Click vertices, double click or Enter finishes.
    Vertices,
}

impl Tool {
    pub fn icon(self) -> Icon {
        match self {
            Tool::Hand => Icon::Hand,
            Tool::SelectText => Icon::SelectText,
            Tool::SelectAnnot | Tool::SelectFields => Icon::Pointer,
            Tool::Zoom => Icon::ZoomIn,
            Tool::Snapshot => Icon::Camera,
            Tool::Highlight => Icon::Highlight,
            Tool::AreaHighlight => Icon::AreaHighlight,
            Tool::Underline => Icon::Underline,
            Tool::StrikeOut => Icon::StrikeOut,
            Tool::Squiggly => Icon::Squiggly,
            Tool::Replace => Icon::Replace,
            Tool::Caret => Icon::Caret,
            Tool::Note => Icon::Note,
            Tool::FileAttach => Icon::Attachment,
            Tool::TextBox => Icon::TextBox,
            Tool::Typewriter => Icon::Typewriter,
            Tool::Callout => Icon::Callout,
            Tool::Rectangle => Icon::Rectangle,
            Tool::Ellipse => Icon::Ellipse,
            Tool::Line => Icon::Line,
            Tool::Arrow => Icon::Arrow,
            Tool::Polygon => Icon::Polygon,
            Tool::Polyline => Icon::Polyline,
            Tool::Cloud => Icon::Cloud,
            Tool::Pencil => Icon::Pencil,
            Tool::Eraser => Icon::Eraser,
            Tool::Stamp => Icon::Stamp,
            Tool::MeasureDistance => Icon::MeasureDistance,
            Tool::MeasurePerimeter => Icon::MeasurePerimeter,
            Tool::MeasureArea => Icon::MeasureArea,
            Tool::Link => Icon::Link,
            Tool::Redact => Icon::Redact,
            Tool::Signature => Icon::Signature,
            Tool::FieldText => Icon::FieldText,
            Tool::FieldButton => Icon::FieldButton,
            Tool::FieldCheckBox => Icon::CheckBox,
            Tool::FieldRadio => Icon::Radio,
            Tool::FieldList => Icon::ListBox,
            Tool::FieldCombo => Icon::ComboBox,
            Tool::FieldSignature => Icon::Signature,
        }
    }

    pub fn gesture(self) -> Gesture {
        match self {
            Tool::Hand | Tool::SelectText | Tool::SelectAnnot | Tool::SelectFields | Tool::Eraser => {
                Gesture::None
            }
            Tool::Highlight | Tool::Underline | Tool::StrikeOut | Tool::Squiggly | Tool::Replace => {
                Gesture::TextMarkup
            }
            Tool::Note | Tool::Stamp | Tool::Caret | Tool::FileAttach | Tool::Typewriter => Gesture::Click,
            Tool::Zoom
            | Tool::Snapshot
            | Tool::AreaHighlight
            | Tool::TextBox
            | Tool::Rectangle
            | Tool::Ellipse
            | Tool::Link
            | Tool::Redact
            | Tool::FieldText
            | Tool::FieldButton
            | Tool::FieldCheckBox
            | Tool::FieldRadio
            | Tool::FieldList
            | Tool::FieldCombo
            | Tool::FieldSignature => Gesture::Rect,
            Tool::Line | Tool::Arrow | Tool::MeasureDistance | Tool::Callout => Gesture::Line,
            Tool::Pencil | Tool::Signature => Gesture::Freehand,
            Tool::Polygon | Tool::Polyline | Tool::Cloud | Tool::MeasurePerimeter | Tool::MeasureArea => {
                Gesture::Vertices
            }
        }
    }

    pub fn annot_kind(self) -> Option<AnnotKind> {
        Some(match self {
            Tool::Highlight | Tool::AreaHighlight => AnnotKind::Highlight,
            Tool::Underline => AnnotKind::Underline,
            Tool::StrikeOut | Tool::Replace => AnnotKind::StrikeOut,
            Tool::Squiggly => AnnotKind::Squiggly,
            Tool::Caret => AnnotKind::Caret,
            Tool::Note => AnnotKind::Note,
            Tool::FileAttach => AnnotKind::FileAttachment,
            Tool::TextBox | Tool::Typewriter | Tool::Callout => AnnotKind::FreeText,
            Tool::Rectangle => AnnotKind::Square,
            Tool::Ellipse => AnnotKind::Circle,
            Tool::Line | Tool::Arrow | Tool::MeasureDistance => AnnotKind::Line,
            Tool::Polygon | Tool::Cloud | Tool::MeasureArea => AnnotKind::Polygon,
            Tool::Polyline | Tool::MeasurePerimeter => AnnotKind::Polyline,
            Tool::Pencil | Tool::Signature => AnnotKind::Ink,
            Tool::Stamp => AnnotKind::Stamp,
            Tool::Redact => AnnotKind::Redact,
            _ => return None,
        })
    }

    /// Tools that work on a text selection.
    pub fn is_markup(self) -> bool {
        self.gesture() == Gesture::TextMarkup
    }

    /// Tools that create form fields.
    pub fn is_field(self) -> bool {
        matches!(
            self,
            Tool::FieldText
                | Tool::FieldButton
                | Tool::FieldCheckBox
                | Tool::FieldRadio
                | Tool::FieldList
                | Tool::FieldCombo
                | Tool::FieldSignature
        )
    }

    pub fn is_measure(self) -> bool {
        matches!(self, Tool::MeasureDistance | Tool::MeasurePerimeter | Tool::MeasureArea)
    }

    pub fn has_width(self) -> bool {
        matches!(
            self,
            Tool::Rectangle
                | Tool::Ellipse
                | Tool::Line
                | Tool::Arrow
                | Tool::Pencil
                | Tool::Signature
                | Tool::TextBox
                | Tool::Callout
                | Tool::Polygon
                | Tool::Polyline
                | Tool::Cloud
                | Tool::MeasureDistance
                | Tool::MeasurePerimeter
                | Tool::MeasureArea
        )
    }

    pub fn has_fill(self) -> bool {
        matches!(
            self,
            Tool::Rectangle | Tool::Ellipse | Tool::TextBox | Tool::Callout | Tool::Polygon | Tool::Cloud
        )
    }

    pub fn has_font(self) -> bool {
        matches!(self, Tool::TextBox | Tool::Typewriter | Tool::Callout)
    }

    /// Whether the tool stays active after creating an object.
    pub fn is_sticky(self) -> bool {
        matches!(
            self,
            Tool::Pencil
                | Tool::Highlight
                | Tool::AreaHighlight
                | Tool::Underline
                | Tool::StrikeOut
                | Tool::Squiggly
                | Tool::Eraser
                | Tool::Redact
                | Tool::Zoom
                | Tool::Signature
        )
    }

    pub fn default_style(self) -> AnnotStyle {
        let base = AnnotStyle::default();
        match self {
            Tool::Highlight | Tool::AreaHighlight => AnnotStyle { color: Rgba::YELLOW, ..base },
            Tool::Underline | Tool::Squiggly => AnnotStyle { color: Rgba::GREEN, ..base },
            Tool::StrikeOut | Tool::Replace => AnnotStyle { color: Rgba::RED, ..base },
            Tool::Caret => AnnotStyle { color: Rgba::BLUE, ..base },
            Tool::Note | Tool::FileAttach => AnnotStyle { color: Rgba::YELLOW, ..base },
            Tool::TextBox => AnnotStyle { color: Rgba::BLACK, width: 0.0, font_size: 12.0, ..base },
            Tool::Typewriter => AnnotStyle { color: Rgba::BLACK, width: 0.0, font_size: 12.0, ..base },
            Tool::Callout => AnnotStyle {
                color: Rgba::BLACK,
                width: 1.0,
                font_size: 12.0,
                fill: Some(Rgba::rgb(255, 255, 220)),
                ..base
            },
            Tool::Pencil => AnnotStyle { color: Rgba::BLUE, width: 2.0, ..base },
            Tool::Signature => AnnotStyle { color: Rgba::rgb(20, 30, 90), width: 1.5, ..base },
            Tool::Cloud => AnnotStyle { color: Rgba::RED, width: 1.5, ..base },
            Tool::MeasureDistance | Tool::MeasurePerimeter | Tool::MeasureArea => {
                AnnotStyle { color: Rgba::BLUE, width: 1.0, font_size: 10.0, ..base }
            }
            Tool::Redact => AnnotStyle { color: Rgba::RED, width: 1.0, fill: Some(Rgba::BLACK), ..base },
            Tool::Stamp => AnnotStyle { color: Rgba::RED, ..base },
            _ => base,
        }
    }
}

/// Colours offered in the quick palette.
pub const PALETTE: [Rgba; 12] = [
    Rgba::rgb(0, 0, 0),
    Rgba::rgb(128, 128, 128),
    Rgba::rgb(255, 255, 255),
    Rgba::rgb(229, 57, 53),
    Rgba::rgb(251, 140, 0),
    Rgba::rgb(255, 235, 59),
    Rgba::rgb(124, 179, 66),
    Rgba::rgb(46, 160, 67),
    Rgba::rgb(0, 172, 193),
    Rgba::rgb(30, 100, 220),
    Rgba::rgb(142, 36, 170),
    Rgba::rgb(236, 64, 122),
];
