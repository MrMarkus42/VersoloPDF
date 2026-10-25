//! Interactive tools and their default styles.

use serde::{Deserialize, Serialize};

use crate::model::{AnnotKind, AnnotStyle, Rgba};
use crate::ui::icons::Icon;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Tool {
    Hand,
    SelectText,
    Highlight,
    Underline,
    StrikeOut,
    Squiggly,
    Note,
    TextBox,
    Rectangle,
    Ellipse,
    Line,
    Arrow,
    Pencil,
    Stamp,
}

impl Tool {
    pub const COMMENT_TOOLS: [Tool; 12] = [
        Tool::Highlight,
        Tool::Underline,
        Tool::StrikeOut,
        Tool::Squiggly,
        Tool::Note,
        Tool::TextBox,
        Tool::Rectangle,
        Tool::Ellipse,
        Tool::Line,
        Tool::Arrow,
        Tool::Pencil,
        Tool::Stamp,
    ];

    pub fn icon(self) -> Icon {
        match self {
            Tool::Hand => Icon::Hand,
            Tool::SelectText => Icon::SelectText,
            Tool::Highlight => Icon::Highlight,
            Tool::Underline => Icon::Underline,
            Tool::StrikeOut => Icon::StrikeOut,
            Tool::Squiggly => Icon::Squiggly,
            Tool::Note => Icon::Note,
            Tool::TextBox => Icon::TextBox,
            Tool::Rectangle => Icon::Rectangle,
            Tool::Ellipse => Icon::Ellipse,
            Tool::Line => Icon::Line,
            Tool::Arrow => Icon::Arrow,
            Tool::Pencil => Icon::Pencil,
            Tool::Stamp => Icon::Stamp,
        }
    }

    pub fn annot_kind(self) -> Option<AnnotKind> {
        Some(match self {
            Tool::Hand | Tool::SelectText => return None,
            Tool::Highlight => AnnotKind::Highlight,
            Tool::Underline => AnnotKind::Underline,
            Tool::StrikeOut => AnnotKind::StrikeOut,
            Tool::Squiggly => AnnotKind::Squiggly,
            Tool::Note => AnnotKind::Note,
            Tool::TextBox => AnnotKind::FreeText,
            Tool::Rectangle => AnnotKind::Square,
            Tool::Ellipse => AnnotKind::Circle,
            Tool::Line | Tool::Arrow => AnnotKind::Line,
            Tool::Pencil => AnnotKind::Ink,
            Tool::Stamp => AnnotKind::Stamp,
        })
    }

    /// Tools that work on a text selection.
    pub fn is_markup(self) -> bool {
        matches!(self, Tool::Highlight | Tool::Underline | Tool::StrikeOut | Tool::Squiggly)
    }

    /// Tools that draw a shape by dragging.
    pub fn is_shape(self) -> bool {
        matches!(
            self,
            Tool::Rectangle | Tool::Ellipse | Tool::Line | Tool::Arrow | Tool::Pencil | Tool::TextBox
        )
    }

    pub fn has_width(self) -> bool {
        matches!(
            self,
            Tool::Rectangle | Tool::Ellipse | Tool::Line | Tool::Arrow | Tool::Pencil | Tool::TextBox
        )
    }

    pub fn has_fill(self) -> bool {
        matches!(self, Tool::Rectangle | Tool::Ellipse | Tool::TextBox)
    }

    pub fn default_style(self) -> AnnotStyle {
        let base = AnnotStyle::default();
        match self {
            Tool::Highlight => AnnotStyle { color: Rgba::YELLOW, opacity: 1.0, ..base },
            Tool::Underline | Tool::Squiggly => AnnotStyle { color: Rgba::GREEN, ..base },
            Tool::StrikeOut => AnnotStyle { color: Rgba::RED, ..base },
            Tool::Note => AnnotStyle { color: Rgba::YELLOW, ..base },
            Tool::TextBox => AnnotStyle { color: Rgba::BLACK, width: 0.0, font_size: 12.0, ..base },
            Tool::Pencil => AnnotStyle { color: Rgba::BLUE, width: 2.0, ..base },
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
