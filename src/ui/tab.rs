//! State of one open document tab: view settings, caches and selection.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use egui::{Pos2, Rect, Vec2};

use crate::document::Document;
use crate::geometry::{PointF, RectF};
use crate::model::{AnnotationInfo, Bookmark, FormFieldInfo, LinkInfo, SearchHit, SearchOptions};
use crate::pdf_edit::AnnotDetails;
use crate::search;
use crate::text::{PageText, TextRange};
use crate::ui::cache::TextureCache;
use crate::ui::view::{Layout, PT_TO_SCREEN, PageLayout, ZoomMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnnotRef {
    pub page: usize,
    pub index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextSelection {
    pub page: usize,
    pub range: TextRange,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Interaction {
    None,
    Pan,
    SelectText { page: usize, anchor: usize },
    Markup { page: usize, anchor: Option<usize>, start: PointF, current: PointF },
    Shape { page: usize, start: PointF, current: PointF, points: Vec<PointF> },
    MoveAnnot { annot: AnnotRef, delta: Vec2 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldEdit {
    pub page: usize,
    pub annot_index: usize,
    pub value: String,
    pub multiline: bool,
    pub password: bool,
    pub rect: RectF,
    pub focus_requested: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScrollRequest {
    /// Show the top of the page.
    Page(usize),
    /// Bring a point (view space of the page) into the centre of the view.
    Point(usize, PointF),
    /// Set the scroll offset directly.
    Offset(Vec2),
}

#[derive(Debug, Default)]
pub struct SearchState {
    pub query: String,
    pub options: Option<SearchOptions>,
    pub hits: Vec<SearchHit>,
    pub current: Option<usize>,
    /// Next page to search; `None` when finished.
    pub next_page: Option<usize>,
    pub revision: u64,
}

impl SearchState {
    pub fn is_running(&self) -> bool {
        self.next_page.is_some()
    }
}

struct Cached<T> {
    revision: u64,
    value: Arc<T>,
}

pub struct DocTab {
    pub id: u64,
    pub doc: Document,
    pub zoom: f32,
    pub zoom_mode: ZoomMode,
    pub layout_mode: PageLayout,
    pub current_page: usize,
    pub layout: Layout,
    pub viewport: Rect,
    pub scroll_offset: Vec2,
    pub pending_scroll: Option<ScrollRequest>,
    /// Content-space offset applied to centre narrow layouts.
    pub content_shift: Vec2,
    pub textures: TextureCache,
    texts: HashMap<usize, Cached<PageText>>,
    annots: HashMap<usize, Cached<Vec<AnnotationInfo>>>,
    links: HashMap<usize, Cached<Vec<LinkInfo>>>,
    fields: HashMap<usize, Cached<Vec<FormFieldInfo>>>,
    bookmarks: Option<Cached<Vec<Bookmark>>>,
    pub selection: Option<TextSelection>,
    pub selected_annot: Option<AnnotRef>,
    details: Option<(AnnotRef, u64, AnnotDetails)>,
    pub search: SearchState,
    pub interaction: Interaction,
    pub field_edit: Option<FieldEdit>,
    pub last_pointer: Option<Pos2>,
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

impl DocTab {
    pub fn new(doc: Document, zoom_mode: ZoomMode, layout_mode: PageLayout) -> Self {
        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            doc,
            zoom: 1.0,
            zoom_mode,
            layout_mode,
            current_page: 0,
            layout: Layout::default(),
            viewport: Rect::NOTHING,
            scroll_offset: Vec2::ZERO,
            pending_scroll: None,
            content_shift: Vec2::ZERO,
            textures: TextureCache::default(),
            texts: HashMap::new(),
            annots: HashMap::new(),
            links: HashMap::new(),
            fields: HashMap::new(),
            bookmarks: None,
            selection: None,
            selected_annot: None,
            details: None,
            search: SearchState::default(),
            interaction: Interaction::None,
            field_edit: None,
            last_pointer: None,
        }
    }

    pub fn title(&self) -> String {
        let name = self.doc.display_name();
        if self.doc.is_modified() {
            format!("{name} *")
        } else {
            name.to_string()
        }
    }

    pub fn scale(&self) -> f32 {
        self.zoom * PT_TO_SCREEN
    }

    pub fn page_sizes(&self) -> Vec<(f32, f32)> {
        (0..self.doc.page_count())
            .map(|i| self.doc.geometry(i).map_or((595.0, 842.0), |g| g.view_size()))
            .collect()
    }

    pub fn go_to_page(&mut self, page: usize) {
        if self.doc.page_count() == 0 {
            return;
        }
        let page = page.min(self.doc.page_count() - 1);
        self.current_page = page;
        self.pending_scroll = Some(ScrollRequest::Page(page));
    }

    pub fn page_text(&mut self, page: usize) -> Option<Arc<PageText>> {
        let rev = self.doc.revision();
        if let Some(c) = self.texts.get(&page).filter(|c| c.revision == rev) {
            return Some(c.value.clone());
        }
        let text = Arc::new(self.doc.page_text(page).ok()?);
        self.texts.insert(page, Cached { revision: rev, value: text.clone() });
        Some(text)
    }

    pub fn annotations(&mut self, page: usize) -> Arc<Vec<AnnotationInfo>> {
        let rev = self.doc.revision();
        if let Some(c) = self.annots.get(&page).filter(|c| c.revision == rev) {
            return c.value.clone();
        }
        let list = Arc::new(self.doc.annotations(page).unwrap_or_default());
        self.annots.insert(page, Cached { revision: rev, value: list.clone() });
        list
    }

    pub fn links(&mut self, page: usize) -> Arc<Vec<LinkInfo>> {
        let rev = self.doc.revision();
        if let Some(c) = self.links.get(&page).filter(|c| c.revision == rev) {
            return c.value.clone();
        }
        let list = Arc::new(self.doc.links(page).unwrap_or_default());
        self.links.insert(page, Cached { revision: rev, value: list.clone() });
        list
    }

    pub fn fields(&mut self, page: usize) -> Arc<Vec<FormFieldInfo>> {
        let rev = self.doc.revision();
        if let Some(c) = self.fields.get(&page).filter(|c| c.revision == rev) {
            return c.value.clone();
        }
        let list = Arc::new(self.doc.form_fields(page).unwrap_or_default());
        self.fields.insert(page, Cached { revision: rev, value: list.clone() });
        list
    }

    pub fn bookmarks(&mut self) -> Arc<Vec<Bookmark>> {
        let rev = self.doc.revision();
        if let Some(c) = self.bookmarks.as_ref().filter(|c| c.revision == rev) {
            return c.value.clone();
        }
        let list = Arc::new(self.doc.bookmarks());
        self.bookmarks = Some(Cached { revision: rev, value: list.clone() });
        list
    }

    /// All annotations of the document that are user comments.
    pub fn all_comments(&mut self) -> Vec<AnnotationInfo> {
        (0..self.doc.page_count())
            .flat_map(|p| self.annotations(p).iter().filter(|a| a.kind.is_comment()).cloned().collect::<Vec<_>>())
            .collect()
    }

    pub fn selected_details(&mut self) -> Option<AnnotDetails> {
        let sel = self.selected_annot?;
        let rev = self.doc.revision();
        if let Some((r, v, d)) = &self.details
            && *r == sel
            && *v == rev
        {
            return Some(d.clone());
        }
        let details = self.doc.annotation_details(sel.page, sel.index).ok()?;
        self.details = Some((sel, rev, details.clone()));
        Some(details)
    }

    pub fn selected_annotation(&mut self) -> Option<AnnotationInfo> {
        let sel = self.selected_annot?;
        self.annotations(sel.page).get(sel.index).cloned()
    }

    pub fn selected_text(&mut self) -> Option<String> {
        let sel = self.selection?;
        let text = self.page_text(sel.page)?;
        Some(text.text_in(sel.range))
    }

    /// Called after any document change.
    pub fn after_edit(&mut self) {
        if let Some(sel) = self.selected_annot
            && sel.index >= self.annotations(sel.page).len()
        {
            self.selected_annot = None;
        }
        if self.current_page >= self.doc.page_count() {
            self.current_page = self.doc.page_count().saturating_sub(1);
        }
        self.selection = None;
        if !self.search.query.is_empty() && self.search.revision != self.doc.revision() {
            let query = self.search.query.clone();
            if let Some(opts) = self.search.options {
                self.start_search(query, opts);
            }
        }
    }

    pub fn start_search(&mut self, query: String, options: SearchOptions) {
        self.search = SearchState {
            next_page: (!query.trim().is_empty() && self.doc.page_count() > 0).then_some(0),
            query,
            options: Some(options),
            hits: Vec::new(),
            current: None,
            revision: self.doc.revision(),
        };
    }

    /// Searches a few pages; returns true if more work remains.
    pub fn continue_search(&mut self, budget: std::time::Duration) -> bool {
        let start = std::time::Instant::now();
        let Some(opts) = self.search.options else {
            return false;
        };
        while let Some(page) = self.search.next_page {
            if page >= self.doc.page_count() {
                self.search.next_page = None;
                break;
            }
            if let Some(text) = self.page_text(page) {
                let hits = search::search_page(page, &text, &self.search.query, opts);
                self.search.hits.extend(hits);
            }
            self.search.next_page = Some(page + 1);
            if self.search.current.is_none() && !self.search.hits.is_empty() {
                self.search.current = Some(0);
                self.reveal_hit(0);
            }
            if start.elapsed() > budget {
                return true;
            }
        }
        false
    }

    pub fn reveal_hit(&mut self, index: usize) {
        if let Some(hit) = self.search.hits.get(index) {
            let page = hit.page;
            if let (Some(geo), Some(r)) = (self.doc.geometry(page).copied(), hit.rects.first()) {
                let v = geo.pdf_rect_to_view(r).center();
                self.current_page = page;
                self.pending_scroll = Some(ScrollRequest::Point(page, v));
            }
        }
    }

    pub fn next_hit(&mut self, forward: bool) {
        let n = self.search.hits.len();
        if n == 0 {
            return;
        }
        let next = match self.search.current {
            None => 0,
            Some(i) if forward => (i + 1) % n,
            Some(i) => (i + n - 1) % n,
        };
        self.search.current = Some(next);
        self.reveal_hit(next);
    }
}
