//! "Schriften ersetzen": a font of the document is replaced by one of the
//! 14 standard PDF fonts (which every PDF program has). The glyph widths
//! stay, so the text keeps its places; the embedded font program is left
//! out. Only simple fonts (Type 1, TrueType) can be replaced; composite
//! fonts (Type 0) address glyphs by number and would show wrong letters.

use lopdf::Object;

use crate::error::Result;
use crate::pdf_edit::{Edit, guarded};

/// The standard fonts offered as replacement.
pub const STANDARD_FONTS: [&str; 12] = [
    "Helvetica",
    "Helvetica-Bold",
    "Helvetica-Oblique",
    "Helvetica-BoldOblique",
    "Times-Roman",
    "Times-Bold",
    "Times-Italic",
    "Times-BoldItalic",
    "Courier",
    "Courier-Bold",
    "Courier-Oblique",
    "Courier-BoldOblique",
];

/// Whether fonts of this kind (`/Subtype`, also as listed: "Type 1") can
/// be replaced.
pub fn replaceable(kind: &str) -> bool {
    let k: String = kind.chars().filter(|c| !c.is_whitespace()).collect();
    matches!(k.as_str(), "Type1" | "TrueType" | "MMType1")
}

/// A sensible standard font for a font name (bold/italic, serif, mono).
pub fn suggestion(name: &str) -> &'static str {
    let l = name.to_ascii_lowercase();
    let bold = l.contains("bold") || l.contains("black") || l.contains("heavy");
    let italic = l.contains("italic") || l.contains("oblique");
    let family = if l.contains("courier") || l.contains("mono") || l.contains("consol") {
        0
    } else if l.contains("times")
        || (l.contains("serif") && !l.contains("sans"))
        || l.contains("roman")
        || l.contains("georgia")
        || l.contains("garamond")
    {
        1
    } else {
        2
    };
    let index = match (family, bold, italic) {
        (2, false, false) => 0,
        (2, true, false) => 1,
        (2, false, true) => 2,
        (2, true, true) => 3,
        (1, false, false) => 4,
        (1, true, false) => 5,
        (1, false, true) => 6,
        (1, true, true) => 7,
        (_, false, false) => 8,
        (_, true, false) => 9,
        (_, false, true) => 10,
        _ => 11,
    };
    STANDARD_FONTS[index]
}

/// Replaces the fonts named in `map` (BaseFont as listed, new standard
/// font). Returns the new bytes and the number of font objects changed.
pub fn replace_fonts(
    bytes: &[u8],
    password: Option<&str>,
    map: &[(String, String)],
) -> Result<(Vec<u8>, usize)> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let candidates: Vec<(lopdf::ObjectId, String)> = edit
            .prev()
            .objects
            .iter()
            .filter_map(|(id, o)| {
                let d = o.as_dict().ok()?;
                if d.get(b"Type").ok()?.as_name().ok()? != b"Font" {
                    return None;
                }
                let subtype = String::from_utf8_lossy(d.get(b"Subtype").ok()?.as_name().ok()?).into_owned();
                if !replaceable(&subtype) {
                    return None;
                }
                let base = String::from_utf8_lossy(d.get(b"BaseFont").ok()?.as_name().ok()?).into_owned();
                Some((*id, base))
            })
            .collect();
        let mut count = 0;
        for (id, base) in candidates {
            let Some((_, new)) = map.iter().find(|(old, _)| *old == base) else { continue };
            if !STANDARD_FONTS.contains(&new.as_str()) {
                continue;
            }
            let dict = edit.dict_mut(id)?;
            dict.set("Subtype", Object::Name(b"Type1".to_vec()));
            dict.set("BaseFont", Object::Name(new.as_bytes().to_vec()));
            // Named encodings other than the standard ones (and missing ones)
            // become WinAnsi; difference tables are kept.
            let keep = match dict.get(b"Encoding") {
                Ok(Object::Name(n)) => {
                    matches!(n.as_slice(), b"WinAnsiEncoding" | b"MacRomanEncoding" | b"StandardEncoding")
                }
                Ok(Object::Dictionary(_) | Object::Reference(_)) => true,
                _ => false,
            };
            if !keep {
                dict.set("Encoding", Object::Name(b"WinAnsiEncoding".to_vec()));
            }
            dict.remove(b"FontDescriptor");
            count += 1;
        }
        Ok((edit.finish()?, count))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggestions() {
        assert_eq!(suggestion("ABCDEF+Arial-BoldMT"), "Helvetica-Bold");
        assert_eq!(suggestion("TimesNewRomanPS-ItalicMT"), "Times-Italic");
        assert_eq!(suggestion("CourierNewPSMT"), "Courier");
        assert_eq!(suggestion("Calibri"), "Helvetica");
        assert!(
            replaceable("TrueType")
                && replaceable("Type 1")
                && !replaceable("Type0")
                && !replaceable("Type 3")
        );
    }
}
