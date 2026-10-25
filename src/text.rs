//! Text layout information of a page and text selection logic.

use crate::geometry::{PointF, RectF};

/// One character of the page text as reported by PDFium.
#[derive(Debug, Clone, PartialEq)]
pub struct CharInfo {
    pub ch: char,
    /// Bounding box in PDF user space. Empty for generated characters
    /// (spaces and line breaks synthesised by the text extractor).
    pub rect: RectF,
    pub generated: bool,
}

/// All characters of one page, in reading order.
#[derive(Debug, Clone, Default)]
pub struct PageText {
    pub chars: Vec<CharInfo>,
}

/// An inclusive range of character indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextRange {
    pub start: usize,
    pub end: usize,
}

impl TextRange {
    pub fn new(a: usize, b: usize) -> Self {
        Self {
            start: a.min(b),
            end: a.max(b),
        }
    }
}

impl PageText {
    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }

    /// Returns the index of the character under `p` (PDF space), or the
    /// closest character on the same line if `p` lies between characters.
    pub fn hit_test(&self, p: PointF, tolerance: f32) -> Option<usize> {
        let mut best: Option<(usize, f32)> = None;
        for (i, c) in self.chars.iter().enumerate() {
            if c.rect.is_empty() {
                continue;
            }
            if c.rect.contains(p) {
                return Some(i);
            }
            let r = c.rect.normalized();
            let dx = if p.x < r.x0 {
                r.x0 - p.x
            } else if p.x > r.x1 {
                p.x - r.x1
            } else {
                0.0
            };
            let dy = if p.y < r.y0 {
                r.y0 - p.y
            } else if p.y > r.y1 {
                p.y - r.y1
            } else {
                0.0
            };
            // Vertical distance weighs more: selecting continues along the line.
            let d = dx + dy * 4.0;
            if best.is_none_or(|(_, bd)| d < bd) {
                best = Some((i, d));
            }
        }
        best.filter(|&(_, d)| d <= tolerance).map(|(i, _)| i)
    }

    /// Extracts the text of `range`, normalising line breaks to `\n`.
    pub fn text_in(&self, range: TextRange) -> String {
        let end = range.end.min(self.chars.len().saturating_sub(1));
        if self.chars.is_empty() || range.start > end {
            return String::new();
        }
        let mut out = String::new();
        for c in &self.chars[range.start..=end] {
            match c.ch {
                '\r' => {}
                '\u{2}' => out.push('-'), // soft hyphen marker used by PDFium
                ch if ch.is_control() && ch != '\n' && ch != '\t' => {}
                ch => out.push(ch),
            }
        }
        out
    }

    pub fn all_text(&self) -> String {
        if self.chars.is_empty() {
            String::new()
        } else {
            self.text_in(TextRange::new(0, self.chars.len() - 1))
        }
    }

    /// Range of the word containing character `index`.
    pub fn word_at(&self, index: usize) -> Option<TextRange> {
        let is_word = |i: usize| {
            self.chars
                .get(i)
                .is_some_and(|c| c.ch.is_alphanumeric() || c.ch == '_' || c.ch == '-')
        };
        if !is_word(index) {
            return self.chars.get(index).map(|_| TextRange::new(index, index));
        }
        let mut start = index;
        while start > 0 && is_word(start - 1) {
            start -= 1;
        }
        let mut end = index;
        while is_word(end + 1) {
            end += 1;
        }
        Some(TextRange::new(start, end))
    }

    /// Merges the boxes of the characters in `range` into one rectangle per
    /// line (PDF space). Used for selection display and text markup.
    pub fn line_rects(&self, range: TextRange) -> Vec<RectF> {
        let mut out: Vec<RectF> = Vec::new();
        let end = range.end.min(self.chars.len().saturating_sub(1));
        if self.chars.is_empty() || range.start > end {
            return out;
        }
        for c in &self.chars[range.start..=end] {
            if c.rect.is_empty() || c.ch == '\n' || c.ch == '\r' {
                continue;
            }
            let r = c.rect.normalized();
            if let Some(last) = out.last_mut() {
                let overlap = r.y1.min(last.y1) - r.y0.max(last.y0);
                let min_h = r.height().min(last.height()).max(0.1);
                let same_line = overlap > 0.5 * min_h && r.x0 >= last.x0 - min_h;
                if same_line {
                    *last = last.union(&r);
                    continue;
                }
            }
            out.push(r);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str, x: f32, y: f32) -> Vec<CharInfo> {
        text.chars()
            .enumerate()
            .map(|(i, ch)| CharInfo {
                ch,
                rect: RectF::new(x + i as f32 * 6.0, y, x + i as f32 * 6.0 + 5.0, y + 10.0),
                generated: false,
            })
            .collect()
    }

    fn sample() -> PageText {
        let mut chars = line("Hello world", 10.0, 100.0);
        chars.push(CharInfo {
            ch: '\r',
            rect: RectF::default(),
            generated: true,
        });
        chars.push(CharInfo {
            ch: '\n',
            rect: RectF::default(),
            generated: true,
        });
        chars.extend(line("Second", 10.0, 80.0));
        PageText { chars }
    }

    #[test]
    fn hit_test_finds_char() {
        let t = sample();
        assert_eq!(t.hit_test(PointF::new(12.0, 105.0), 5.0), Some(0));
        assert_eq!(t.hit_test(PointF::new(12.0, 85.0), 5.0), Some(13));
        assert_eq!(t.hit_test(PointF::new(500.0, 500.0), 5.0), None);
    }

    #[test]
    fn text_extraction_normalises_breaks() {
        let t = sample();
        assert_eq!(t.all_text(), "Hello world\nSecond");
        assert_eq!(t.text_in(TextRange::new(6, 10)), "world");
    }

    #[test]
    fn word_selection() {
        let t = sample();
        assert_eq!(t.word_at(7), Some(TextRange::new(6, 10)));
        assert_eq!(t.word_at(5), Some(TextRange::new(5, 5)));
    }

    #[test]
    fn line_rects_merge_per_line() {
        let t = sample();
        let rects = t.line_rects(TextRange::new(0, t.chars.len() - 1));
        assert_eq!(rects.len(), 2);
        assert_eq!(rects[0].x0, 10.0);
        assert_eq!(rects[0].x1, 10.0 + 10.0 * 6.0 + 5.0);
    }
}
