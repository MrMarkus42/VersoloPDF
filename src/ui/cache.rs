//! GPU texture cache for rendered pages, detail tiles and thumbnails.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use egui::{ColorImage, Context, TextureHandle, TextureOptions};

use crate::document::{Document, RenderOptions, RenderedImage};

/// Limits how much rendering happens in one frame to keep the UI responsive.
pub struct Budget {
    start: Instant,
    limit: Duration,
    rendered: usize,
    pub pending: bool,
}

impl Budget {
    pub fn new(limit: Duration) -> Self {
        Self { start: Instant::now(), limit, rendered: 0, pending: false }
    }

    /// Whether another render may start. The first render per frame is
    /// always allowed so progress is guaranteed.
    pub fn allow(&mut self) -> bool {
        if self.rendered == 0 || self.start.elapsed() < self.limit {
            self.rendered += 1;
            true
        } else {
            self.pending = true;
            false
        }
    }
}

pub struct CachedImage {
    pub tex: TextureHandle,
    pub px: [u32; 2],
    pub revision: u64,
    last_used: u64,
}

pub struct Tile {
    pub tex: TextureHandle,
    pub revision: u64,
    pub full_px: [u32; 2],
    /// x, y, w, h in pixels of the full-size page.
    pub area: [u32; 4],
    last_used: u64,
}

#[derive(Default)]
pub struct TextureCache {
    pages: HashMap<usize, CachedImage>,
    tiles: HashMap<usize, Tile>,
    thumbs: HashMap<usize, CachedImage>,
    frame: u64,
}

fn upload(ctx: &Context, name: String, img: &RenderedImage) -> TextureHandle {
    let image = ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.rgba);
    ctx.load_texture(name, image, TextureOptions::LINEAR)
}

impl TextureCache {
    pub fn begin_frame(&mut self) {
        self.frame += 1;
    }

    pub fn clear(&mut self) {
        self.pages.clear();
        self.tiles.clear();
        self.thumbs.clear();
    }

    /// Returns a page texture, re-rendering it if it is missing or outdated.
    /// May return an outdated texture while the budget is exhausted.
    #[allow(clippy::too_many_arguments)]
    pub fn page(
        &mut self,
        ctx: &Context,
        doc: &Document,
        page: usize,
        px: [u32; 2],
        budget: &mut Budget,
        id: u64,
        opts: RenderOptions,
    ) -> Option<&CachedImage> {
        let fresh = self.pages.get(&page).is_some_and(|c| c.revision == doc.revision() && c.px == px);
        if !fresh && budget.allow() {
            match doc.render_tile(page, px[0], px[1], 0, 0, px[0], px[1], opts) {
                Ok(img) => {
                    let tex = upload(ctx, format!("doc{id}-p{page}"), &img);
                    self.pages.insert(
                        page,
                        CachedImage { tex, px, revision: doc.revision(), last_used: self.frame },
                    );
                }
                Err(err) => log::warn!("rendering page {page} failed: {err}"),
            }
        }
        let frame = self.frame;
        self.pages.get_mut(&page).map(|c| {
            c.last_used = frame;
            &*c
        })
    }

    /// Returns a high resolution tile covering `area` of the page.
    #[allow(clippy::too_many_arguments)]
    pub fn tile(
        &mut self,
        ctx: &Context,
        doc: &Document,
        page: usize,
        full_px: [u32; 2],
        want: [u32; 4],
        budget: &mut Budget,
        id: u64,
        opts: RenderOptions,
    ) -> Option<&Tile> {
        let covers = |t: &Tile| {
            t.revision == doc.revision()
                && t.full_px == full_px
                && t.area[0] <= want[0]
                && t.area[1] <= want[1]
                && t.area[0] + t.area[2] >= want[0] + want[2]
                && t.area[1] + t.area[3] >= want[1] + want[3]
        };
        let fresh = self.tiles.get(&page).is_some_and(covers);
        if !fresh && budget.allow() {
            // Render with a margin so small scroll movements reuse the tile.
            let margin = 256;
            let x = want[0].saturating_sub(margin);
            let y = want[1].saturating_sub(margin);
            let w = (want[2] + 2 * margin).min(full_px[0] - x);
            let h = (want[3] + 2 * margin).min(full_px[1] - y);
            match doc.render_tile(page, full_px[0], full_px[1], x, y, w, h, opts) {
                Ok(img) => {
                    let tex = upload(ctx, format!("doc{id}-t{page}"), &img);
                    self.tiles.insert(
                        page,
                        Tile {
                            tex,
                            revision: doc.revision(),
                            full_px,
                            area: [x, y, w, h],
                            last_used: self.frame,
                        },
                    );
                }
                Err(err) => log::warn!("rendering tile of page {page} failed: {err}"),
            }
        }
        let frame = self.frame;
        self.tiles.get_mut(&page).filter(|t| t.full_px == full_px).map(|t| {
            t.last_used = frame;
            &*t
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn thumbnail(
        &mut self,
        ctx: &Context,
        doc: &Document,
        page: usize,
        px: [u32; 2],
        budget: &mut Budget,
        id: u64,
        opts: RenderOptions,
    ) -> Option<&CachedImage> {
        let fresh = self.thumbs.get(&page).is_some_and(|c| c.revision == doc.revision() && c.px == px);
        if !fresh && budget.allow() {
            match doc.render_tile(page, px[0], px[1], 0, 0, px[0], px[1], opts) {
                Ok(img) => {
                    let tex = upload(ctx, format!("doc{id}-th{page}"), &img);
                    self.thumbs.insert(
                        page,
                        CachedImage { tex, px, revision: doc.revision(), last_used: self.frame },
                    );
                }
                Err(err) => log::warn!("rendering thumbnail {page} failed: {err}"),
            }
        }
        let frame = self.frame;
        self.thumbs.get_mut(&page).map(|c| {
            c.last_used = frame;
            &*c
        })
    }

    /// Drops textures that have not been used recently.
    pub fn evict(&mut self) {
        let frame = self.frame;
        self.pages.retain(|_, c| frame - c.last_used < 120);
        self.tiles.retain(|_, t| frame - t.last_used < 30);
        if self.thumbs.len() > 400 {
            self.thumbs.retain(|_, c| frame - c.last_used < 600);
        }
    }
}
