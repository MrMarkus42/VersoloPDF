//! Page layout and zoom handling.

use egui::{Rect, Vec2, pos2, vec2};
use serde::{Deserialize, Serialize};

/// Screen points per PDF point at 100 % zoom (96 dpi screens).
pub const PT_TO_SCREEN: f32 = 96.0 / 72.0;
pub const PAGE_GAP: f32 = 12.0;
pub const MARGIN: f32 = 16.0;
pub const MIN_ZOOM: f32 = 0.05;
pub const MAX_ZOOM: f32 = 64.0;

pub const ZOOM_STEPS: [f32; 20] = [
    0.05, 0.1, 0.25, 0.33, 0.5, 0.67, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0, 4.0, 6.0, 8.0, 12.0, 16.0, 24.0, 32.0,
    64.0,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ZoomMode {
    Custom,
    FitWidth,
    FitPage,
}

/// "Seitenanzeige": the four page layouts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PageLayout {
    /// "Einzelne Seite": one page at a time.
    Single,
    /// "Fortlaufend".
    Continuous,
    /// "Zwei Seiten": one spread at a time.
    TwoPages,
    /// "Zwei Seiten fortlaufend" (older settings: "Facing", "Cover").
    #[serde(alias = "Facing", alias = "Cover")]
    TwoPagesContinuous,
}

impl PageLayout {
    /// Layouts that show one page or spread at a time (the mouse wheel flips).
    pub fn paged(self) -> bool {
        matches!(self, PageLayout::Single | PageLayout::TwoPages)
    }

    pub fn two_up(self) -> bool {
        matches!(self, PageLayout::TwoPages | PageLayout::TwoPagesContinuous)
    }
}

/// Options of the page display besides the layout (global view settings).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutOptions {
    pub mode: PageLayout,
    /// "Titelseite in der zweiseitigen Ansicht einzeln".
    pub cover_alone: bool,
    /// "Seitenzwischenräume".
    pub gaps: bool,
    /// "Rechts-nach-Links-Seitenansicht".
    pub right_to_left: bool,
    /// "Seiten nebeneinander": scrolling layouts run horizontally.
    pub side_by_side: bool,
}

impl LayoutOptions {
    pub fn new(mode: PageLayout) -> Self {
        LayoutOptions { mode, cover_alone: false, gaps: true, right_to_left: false, side_by_side: false }
    }

    /// Scrolling layouts arranged in one row.
    pub fn horizontal(&self) -> bool {
        self.side_by_side && !self.mode.paged()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slot {
    pub page: usize,
    /// Rectangle in content coordinates (screen points, origin top-left).
    pub rect: Rect,
}

#[derive(Debug, Clone, Default)]
pub struct Layout {
    pub slots: Vec<Slot>,
    pub size: Vec2,
    /// Pages run from left to right in one row ("Seiten nebeneinander").
    pub horizontal: bool,
}

impl Layout {
    /// Lays out pages of the given view sizes (PDF points) at `zoom`.
    pub fn compute(sizes: &[(f32, f32)], zoom: f32, opts: LayoutOptions, current: usize) -> Layout {
        let scale = zoom * PT_TO_SCREEN;
        let gap = if opts.gaps { PAGE_GAP } else { 0.0 };
        let size_of = |i: usize| {
            let (w, h) = sizes[i];
            vec2(w * scale, h * scale)
        };
        // Rows of pages: (first page, number of pages).
        let mut rows: Vec<(usize, usize)> = Vec::new();
        if opts.mode.two_up() {
            let mut i = 0;
            while i < sizes.len() {
                let alone = opts.cover_alone && i == 0;
                let n = if alone || i + 1 >= sizes.len() { 1 } else { 2 };
                rows.push((i, n));
                i += n;
            }
        } else {
            rows.extend((0..sizes.len()).map(|i| (i, 1)));
        }
        if opts.mode.paged() {
            rows.retain(|(first, n)| (*first..first + n).contains(&current));
        }
        let mut slots = Vec::new();
        let horizontal = opts.horizontal();
        if horizontal {
            // All pages in one row, vertically centred.
            let height = (0..sizes.len()).map(|i| size_of(i).y).fold(0.0, f32::max);
            let mut order: Vec<usize> = (0..sizes.len()).collect();
            if opts.right_to_left {
                order.reverse();
            }
            let mut x = MARGIN;
            for i in order {
                let s = size_of(i);
                slots.push(Slot {
                    page: i,
                    rect: Rect::from_min_size(pos2(x, MARGIN + (height - s.y) / 2.0), s),
                });
                x += s.x + gap;
            }
            let width = (x - gap + MARGIN).max(2.0 * MARGIN);
            return Layout { slots, size: vec2(width, height + 2.0 * MARGIN), horizontal };
        }
        let mut width: f32 = 0.0;
        let mut y = MARGIN;
        for (first, n) in rows {
            let mut pages: Vec<usize> = (first..first + n).collect();
            if opts.right_to_left {
                pages.reverse();
            }
            let row_h = pages.iter().map(|&i| size_of(i).y).fold(0.0, f32::max);
            let mut x = 0.0;
            for &i in &pages {
                let s = size_of(i);
                slots.push(Slot { page: i, rect: Rect::from_min_size(pos2(x, y), s) });
                x += s.x + gap;
            }
            let row_w = x - gap;
            width = width.max(row_w);
            y += row_h + gap;
        }
        y += MARGIN - gap;
        let total_w = width + 2.0 * MARGIN;
        // Centre each row horizontally.
        let mut row_start = 0;
        while row_start < slots.len() {
            let top = slots[row_start].rect.min.y;
            let mut row_end = row_start;
            while row_end + 1 < slots.len() && slots[row_end + 1].rect.min.y == top {
                row_end += 1;
            }
            let row_w = slots[row_end].rect.max.x - slots[row_start].rect.min.x;
            let dx = (total_w - row_w) / 2.0;
            for slot in &mut slots[row_start..=row_end] {
                slot.rect = slot.rect.translate(vec2(dx, 0.0));
            }
            row_start = row_end + 1;
        }
        Layout { slots, size: vec2(total_w, y.max(MARGIN * 2.0)), horizontal }
    }

    pub fn slot(&self, page: usize) -> Option<&Slot> {
        self.slots.iter().find(|s| s.page == page)
    }

    /// The page shown at content position `p` (or the closest one); only
    /// `y` matters for vertical layouts, only `x` for horizontal ones.
    pub fn page_at(&self, p: egui::Pos2) -> Option<usize> {
        let horizontal = self.horizontal;
        let d = |r: Rect| if horizontal { dist(r.min.x, r.max.x, p.x) } else { dist(r.min.y, r.max.y, p.y) };
        self.slots.iter().min_by(|a, b| d(a.rect).total_cmp(&d(b.rect))).map(|s| s.page)
    }
}

fn dist(min: f32, max: f32, v: f32) -> f32 {
    if v < min {
        min - v
    } else if v > max {
        v - max
    } else {
        0.0
    }
}

pub fn fit_width_zoom(max_page_width: f32, viewport_width: f32, two_up: bool) -> f32 {
    let avail = (viewport_width - 2.0 * MARGIN - if two_up { PAGE_GAP } else { 0.0 }).max(50.0);
    let w = if two_up { max_page_width * 2.0 } else { max_page_width };
    (avail / (w.max(1.0) * PT_TO_SCREEN)).clamp(MIN_ZOOM, MAX_ZOOM)
}

pub fn fit_page_zoom(page: (f32, f32), viewport: Vec2, two_up: bool) -> f32 {
    let zw = fit_width_zoom(page.0, viewport.x, two_up);
    let zh = (viewport.y - 2.0 * MARGIN).max(50.0) / (page.1.max(1.0) * PT_TO_SCREEN);
    zw.min(zh).clamp(MIN_ZOOM, MAX_ZOOM)
}

pub fn zoom_in(z: f32) -> f32 {
    ZOOM_STEPS.iter().copied().find(|&s| s > z * 1.001).unwrap_or(MAX_ZOOM)
}

pub fn zoom_out(z: f32) -> f32 {
    ZOOM_STEPS.iter().rev().copied().find(|&s| s < z * 0.999).unwrap_or(MIN_ZOOM)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(mode: PageLayout) -> LayoutOptions {
        LayoutOptions::new(mode)
    }

    #[test]
    fn continuous_layout_stacks_pages() {
        let l = Layout::compute(&[(100.0, 200.0), (100.0, 200.0)], 1.0, opts(PageLayout::Continuous), 0);
        assert_eq!(l.slots.len(), 2);
        assert!(l.slots[1].rect.min.y > l.slots[0].rect.max.y);
        assert_eq!(l.slots[0].rect.center().x, l.size.x / 2.0);
        assert_eq!(l.page_at(l.slots[1].rect.center()), Some(1));
        // Without gaps the pages touch.
        let l = Layout::compute(
            &[(100.0, 200.0), (100.0, 200.0)],
            1.0,
            LayoutOptions { gaps: false, ..opts(PageLayout::Continuous) },
            0,
        );
        assert_eq!(l.slots[1].rect.min.y, l.slots[0].rect.max.y);
    }

    #[test]
    fn cover_option_keeps_first_page_alone() {
        let o = LayoutOptions { cover_alone: true, ..opts(PageLayout::TwoPagesContinuous) };
        let l = Layout::compute(&[(100.0, 100.0); 3], 1.0, o, 0);
        assert!(l.slots[1].rect.min.y > l.slots[0].rect.max.y);
        assert_eq!(l.slots[1].rect.min.y, l.slots[2].rect.min.y);
    }

    #[test]
    fn two_page_layouts() {
        let l = Layout::compute(&[(100.0, 100.0); 3], 1.0, opts(PageLayout::TwoPagesContinuous), 0);
        assert_eq!(l.slots[0].rect.min.y, l.slots[1].rect.min.y);
        assert!(l.slots[0].rect.min.x < l.slots[1].rect.min.x);
        assert!(l.slots[2].rect.min.y > l.slots[0].rect.max.y);
        // One spread at a time, right to left.
        let o = LayoutOptions { right_to_left: true, ..opts(PageLayout::TwoPages) };
        let l = Layout::compute(&[(100.0, 100.0); 4], 1.0, o, 3);
        assert_eq!(l.slots.len(), 2);
        assert_eq!((l.slots[0].page, l.slots[1].page), (3, 2));
        assert!(l.slots[0].rect.min.x < l.slots[1].rect.min.x);
    }

    #[test]
    fn single_layout_shows_current_page() {
        let l = Layout::compute(&[(100.0, 100.0); 3], 1.0, opts(PageLayout::Single), 2);
        assert_eq!(l.slots.len(), 1);
        assert_eq!(l.slots[0].page, 2);
    }

    #[test]
    fn side_by_side_runs_horizontally() {
        let o = LayoutOptions { side_by_side: true, ..opts(PageLayout::Continuous) };
        let l = Layout::compute(&[(100.0, 100.0); 3], 1.0, o, 0);
        assert!(l.horizontal);
        assert_eq!(l.slots[0].rect.min.y, l.slots[2].rect.min.y);
        assert!(l.slots[2].rect.min.x > l.slots[1].rect.max.x);
        assert_eq!(l.page_at(l.slots[2].rect.center()), Some(2));
        // Old settings files still load.
        let old: PageLayout = ron::from_str("Cover").unwrap();
        assert_eq!(old, PageLayout::TwoPagesContinuous);
    }

    #[test]
    fn zoom_steps() {
        assert_eq!(zoom_in(1.0), 1.25);
        assert_eq!(zoom_out(1.0), 0.75);
        assert_eq!(zoom_in(64.0), MAX_ZOOM);
        assert!((fit_width_zoom(100.0, 100.0 * PT_TO_SCREEN + 2.0 * MARGIN, false) - 1.0).abs() < 1e-4);
    }
}
