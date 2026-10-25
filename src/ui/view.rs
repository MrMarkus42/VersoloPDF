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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PageLayout {
    Single,
    Continuous,
    Facing,
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
}

impl Layout {
    /// Lays out pages of the given view sizes (PDF points) at `zoom`.
    pub fn compute(sizes: &[(f32, f32)], zoom: f32, mode: PageLayout, current: usize) -> Layout {
        let scale = zoom * PT_TO_SCREEN;
        let size_of = |i: usize| {
            let (w, h) = sizes[i];
            vec2(w * scale, h * scale)
        };
        let mut slots = Vec::new();
        let mut width: f32 = 0.0;
        let mut y = MARGIN;
        match mode {
            PageLayout::Single => {
                if let Some(i) = (current < sizes.len()).then_some(current) {
                    let s = size_of(i);
                    slots.push(Slot { page: i, rect: Rect::from_min_size(pos2(0.0, y), s) });
                    width = s.x;
                    y += s.y + MARGIN;
                }
            }
            PageLayout::Continuous => {
                for i in 0..sizes.len() {
                    let s = size_of(i);
                    slots.push(Slot { page: i, rect: Rect::from_min_size(pos2(0.0, y), s) });
                    width = width.max(s.x);
                    y += s.y + PAGE_GAP;
                }
                y += MARGIN - PAGE_GAP;
            }
            PageLayout::Facing => {
                let mut i = 0;
                while i < sizes.len() {
                    let a = size_of(i);
                    let b = (i + 1 < sizes.len()).then(|| size_of(i + 1));
                    let row_h = a.y.max(b.map_or(0.0, |b| b.y));
                    slots.push(Slot { page: i, rect: Rect::from_min_size(pos2(0.0, y), a) });
                    let mut row_w = a.x;
                    if let Some(b) = b {
                        slots.push(Slot {
                            page: i + 1,
                            rect: Rect::from_min_size(pos2(a.x + PAGE_GAP, y), b),
                        });
                        row_w += PAGE_GAP + b.x;
                    }
                    width = width.max(row_w);
                    y += row_h + PAGE_GAP;
                    i += 2;
                }
                y += MARGIN - PAGE_GAP;
            }
        }
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
        Layout { slots, size: vec2(total_w, y.max(MARGIN * 2.0)) }
    }

    pub fn slot(&self, page: usize) -> Option<&Slot> {
        self.slots.iter().find(|s| s.page == page)
    }

    /// The page shown at content position `y` (or the closest one).
    pub fn page_at(&self, y: f32) -> Option<usize> {
        self.slots
            .iter()
            .min_by(|a, b| {
                let da = dist(a.rect, y);
                let db = dist(b.rect, y);
                da.total_cmp(&db)
            })
            .map(|s| s.page)
    }
}

fn dist(r: Rect, y: f32) -> f32 {
    if y < r.min.y {
        r.min.y - y
    } else if y > r.max.y {
        y - r.max.y
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

    #[test]
    fn continuous_layout_stacks_pages() {
        let l = Layout::compute(&[(100.0, 200.0), (100.0, 200.0)], 1.0, PageLayout::Continuous, 0);
        assert_eq!(l.slots.len(), 2);
        assert!(l.slots[1].rect.min.y > l.slots[0].rect.max.y);
        assert_eq!(l.slots[0].rect.center().x, l.size.x / 2.0);
        assert_eq!(l.page_at(l.slots[1].rect.center().y), Some(1));
    }

    #[test]
    fn facing_layout_pairs_pages() {
        let l = Layout::compute(&[(100.0, 100.0); 3], 1.0, PageLayout::Facing, 0);
        assert_eq!(l.slots[0].rect.min.y, l.slots[1].rect.min.y);
        assert!(l.slots[2].rect.min.y > l.slots[0].rect.max.y);
    }

    #[test]
    fn single_layout_shows_current_page() {
        let l = Layout::compute(&[(100.0, 100.0); 3], 1.0, PageLayout::Single, 2);
        assert_eq!(l.slots.len(), 1);
        assert_eq!(l.slots[0].page, 2);
    }

    #[test]
    fn zoom_steps() {
        assert_eq!(zoom_in(1.0), 1.25);
        assert_eq!(zoom_out(1.0), 0.75);
        assert_eq!(zoom_in(64.0), MAX_ZOOM);
        assert!((fit_width_zoom(100.0, 100.0 * PT_TO_SCREEN + 2.0 * MARGIN, false) - 1.0).abs() < 1e-4);
    }
}
