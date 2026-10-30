//! State of one open document tab: view settings, caches and selection.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use egui::{Pos2, Rect, Vec2};

use crate::document::Document;
use crate::document::RenderOptions;
use crate::geometry::{PageGeometry, PointF, RectF, Rotation};
use crate::model::{AnnotationInfo, Bookmark, FormFieldInfo, LinkInfo, SearchHit, SearchOptions};
use crate::pdf_edit::AnnotDetails;
use crate::search;
use crate::text::{PageText, TextRange};
use crate::ui::cache::TextureCache;
use crate::ui::view::{Layout, PT_TO_SCREEN, PageLayout, ZoomMode};

/// Whether `path` (child indices) names a bookmark of `items`.
pub fn bookmark_exists(items: &[Bookmark], path: &[usize]) -> bool {
    match path.split_first() {
        None => false,
        Some((i, rest)) => {
            items.get(*i).is_some_and(|b| rest.is_empty() || bookmark_exists(&b.children, rest))
        }
    }
}

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

/// Pictures shown while comments are dragged: for each comment the page
/// area without comments (covers the old place) and the comment alone.
pub struct MovePreview {
    /// Dragged comment, page revision and page size in pixels.
    pub key: (AnnotRef, u64, [u32; 2]),
    /// Area relative to the page's top left corner (points), page without
    /// comments, comments only.
    pub parts: Vec<(egui::Rect, egui::TextureHandle, egui::TextureHandle)>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Interaction {
    None,
    Pan,
    SelectText {
        page: usize,
        anchor: usize,
    },
    Markup {
        page: usize,
        anchor: Option<usize>,
        start: PointF,
        current: PointF,
    },
    Shape {
        page: usize,
        start: PointF,
        current: PointF,
        points: Vec<PointF>,
    },
    /// Dragging a comment (with the other selected comments of its page).
    MoveAnnot {
        annot: AnnotRef,
        delta: Vec2,
    },
    /// Selection rectangle of "Anmerkungen bearbeiten" (view space).
    Marquee {
        page: usize,
        start: PointF,
        current: PointF,
        /// Shift held: add to the selection.
        add: bool,
    },
    /// Eraser: annotations touched so far.
    Erase {
        page: usize,
        hits: Vec<usize>,
    },
    /// Moving a guide ("Hilfslinie"); index into `DocTab::guides`.
    Guide {
        index: usize,
    },
    /// "Stufenlose Vergrößerung": zooming around the pressed point.
    DynamicZoom {
        anchor: Pos2,
    },
}

/// A guide ("Hilfslinie") shown on every page; position in view points
/// from the left (vertical guide) or top (horizontal guide) page edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Guide {
    pub vertical: bool,
    pub pos: f32,
}

/// "Teilen": how the document window is split.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Split {
    #[default]
    None,
    /// Two views on top of each other.
    Horizontal,
    /// Two views side by side.
    Vertical,
    /// Four views ("Als Tabelle teilen").
    Grid,
}

impl Split {
    pub fn panes(self) -> usize {
        match self {
            Split::None => 1,
            Split::Horizontal | Split::Vertical => 2,
            Split::Grid => 4,
        }
    }
}

/// The view state of one pane of a split window.
#[derive(Debug, Clone)]
pub struct ViewState {
    pub zoom: f32,
    pub zoom_mode: ZoomMode,
    pub layout_mode: PageLayout,
    pub current_page: usize,
    pub layout: Layout,
    pub viewport: Rect,
    pub scroll_offset: Vec2,
    pub pending_scroll: Option<ScrollRequest>,
    pub content_shift: Vec2,
    pub canvas_origin: Pos2,
    back: Vec<usize>,
    forward: Vec<usize>,
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
    /// Screen position of the content origin in the last frame.
    pub canvas_origin: Pos2,
    pub textures: TextureCache,
    texts: HashMap<usize, Cached<PageText>>,
    annots: HashMap<usize, Cached<Vec<AnnotationInfo>>>,
    links: HashMap<usize, Cached<Vec<LinkInfo>>>,
    fields: HashMap<usize, Cached<Vec<FormFieldInfo>>>,
    bookmarks: Option<Cached<Vec<Bookmark>>>,
    content_objects: HashMap<usize, Cached<Vec<crate::content::ContentObject>>>,
    /// "Objekte bearbeiten" / "Text bearbeiten".
    pub content: crate::ui::content_state::ContentEdit,
    pub selection: Option<TextSelection>,
    pub selected_annot: Option<AnnotRef>,
    /// Several selected comments of one page (selection rectangle, Shift
    /// click). Valid only while its first entry is selected_annot, so any
    /// other way of selecting a single comment ends the multi selection.
    pub multi_annots: Vec<AnnotRef>,
    /// Pictures of the comments being dragged (see [MovePreview]).
    pub move_preview: Option<MovePreview>,
    /// Bookmark selected in the bookmarks pane (child indices).
    pub selected_bookmark: Option<Vec<usize>>,
    details: Option<(AnnotRef, u64, AnnotDetails)>,
    pub search: SearchState,
    pub interaction: Interaction,
    pub field_edit: Option<FieldEdit>,
    pub last_pointer: Option<Pos2>,
    /// Time of the last page flip by mouse wheel (single page layout).
    pub last_flip: f64,
    /// Pages visited before/after jumps ("Letzte/Nächste Ansicht").
    back: Vec<usize>,
    forward: Vec<usize>,
    /// Additional rotation of the view (does not change the document).
    pub view_rotation: Rotation,
    /// Whether annotations are drawn.
    pub show_annotations: bool,
    /// "Farben im Dokument ersetzen" (background, text); set by the app.
    pub recolor: Option<([u8; 3], [u8; 3])>,
    /// "Transparenzgitter": pages are rendered without white paper.
    pub transparent: bool,
    /// Vertices of a polygon tool in progress (view space of polygon_page).
    pub polygon: Vec<PointF>,
    pub polygon_page: usize,
    /// Guides ("Hilfslinien") of this document.
    pub guides: Vec<Guide>,
    /// Pointer over a page in the last frame: page and view point.
    pub pointer: Option<(usize, PointF)>,
    /// Page and screen rectangle the rulers refer to (last frame).
    pub ruler_page: Option<(usize, Rect)>,
    /// "Lupe": page and view point at the centre of the magnifier.
    pub loupe_at: Option<(usize, PointF)>,
    /// "Teilen": split of the window, the other panes' view states (the
    /// entry of the active pane is unused) and the active pane.
    pub split: Split,
    pub panes: Vec<ViewState>,
    pub active_pane: usize,
    /// Pane being drawn (for widget ids).
    pub drawing_pane: usize,
    /// Window showing this document (0: main window) and its tab group.
    pub window: u32,
    pub group: u32,
    /// Portfolio: (document revision, is portfolio) as last checked.
    pub portfolio: Option<(u64, bool)>,
    /// Portfolio: the cover page is shown instead of the file list.
    pub show_cover: bool,
    /// Portfolio: selected file.
    pub portfolio_sel: Option<usize>,
    /// Comments whose pop-up is shown on the page.
    pub open_popups: Vec<AnnotRef>,
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
            canvas_origin: Pos2::ZERO,
            textures: TextureCache::default(),
            texts: HashMap::new(),
            annots: HashMap::new(),
            links: HashMap::new(),
            fields: HashMap::new(),
            bookmarks: None,
            content_objects: HashMap::new(),
            content: Default::default(),
            selection: None,
            selected_annot: None,
            multi_annots: Vec::new(),
            move_preview: None,
            selected_bookmark: None,
            details: None,
            search: SearchState::default(),
            interaction: Interaction::None,
            field_edit: None,
            last_pointer: None,
            last_flip: 0.0,
            back: Vec::new(),
            forward: Vec::new(),
            view_rotation: Rotation::R0,
            show_annotations: true,
            recolor: None,
            transparent: false,
            polygon: Vec::new(),
            polygon_page: 0,
            guides: Vec::new(),
            pointer: None,
            ruler_page: None,
            loupe_at: None,
            split: Split::None,
            panes: Vec::new(),
            active_pane: 0,
            drawing_pane: 0,
            window: 0,
            group: 0,
            portfolio: None,
            show_cover: false,
            portfolio_sel: None,
            open_popups: Vec::new(),
        }
    }

    /// Changes the comment display filter and redraws the pages.
    pub fn set_annot_filter(&mut self, filter: crate::model::AnnotFilter) {
        if &filter != self.doc.annot_filter() {
            self.doc.set_annot_filter(filter);
            self.textures.clear();
        }
    }

    /// True if the document is a PDF portfolio (checked once per revision).
    pub fn is_portfolio(&mut self) -> bool {
        let rev = self.doc.revision();
        match self.portfolio {
            Some((r, v)) if r == rev => v,
            _ => {
                let v = self.doc.is_portfolio();
                self.portfolio = Some((rev, v));
                v
            }
        }
    }

    /// The view state of the active pane.
    pub fn view_state(&self) -> ViewState {
        ViewState {
            zoom: self.zoom,
            zoom_mode: self.zoom_mode,
            layout_mode: self.layout_mode,
            current_page: self.current_page,
            layout: self.layout.clone(),
            viewport: self.viewport,
            scroll_offset: self.scroll_offset,
            pending_scroll: self.pending_scroll.clone(),
            content_shift: self.content_shift,
            canvas_origin: self.canvas_origin,
            back: self.back.clone(),
            forward: self.forward.clone(),
        }
    }

    /// Exchanges the active view state with `v`.
    pub fn swap_view(&mut self, v: &mut ViewState) {
        std::mem::swap(&mut self.zoom, &mut v.zoom);
        std::mem::swap(&mut self.zoom_mode, &mut v.zoom_mode);
        std::mem::swap(&mut self.layout_mode, &mut v.layout_mode);
        std::mem::swap(&mut self.current_page, &mut v.current_page);
        std::mem::swap(&mut self.layout, &mut v.layout);
        std::mem::swap(&mut self.viewport, &mut v.viewport);
        std::mem::swap(&mut self.scroll_offset, &mut v.scroll_offset);
        std::mem::swap(&mut self.pending_scroll, &mut v.pending_scroll);
        std::mem::swap(&mut self.content_shift, &mut v.content_shift);
        std::mem::swap(&mut self.canvas_origin, &mut v.canvas_origin);
        std::mem::swap(&mut self.back, &mut v.back);
        std::mem::swap(&mut self.forward, &mut v.forward);
    }

    /// "Teilen": splits the window (again: back to one view). New panes
    /// start with the view of the active one.
    pub fn set_split(&mut self, split: Split) {
        let split = if self.split == split { Split::None } else { split };
        if self.active_pane != 0 {
            self.activate_pane(0);
        }
        self.split = split;
        let state = self.view_state();
        self.panes = vec![state; split.panes()];
        self.active_pane = 0;
    }

    /// Makes another pane of a split window the active one.
    pub fn activate_pane(&mut self, pane: usize) {
        if pane == self.active_pane || pane >= self.panes.len() {
            return;
        }
        let current = self.view_state();
        let mut other = std::mem::replace(&mut self.panes[pane], current);
        let old = self.active_pane;
        self.swap_view(&mut other);
        self.panes[old] = other;
        self.active_pane = pane;
    }

    pub fn title(&self) -> String {
        let name = self.doc.display_name();
        if self.doc.is_modified() { format!("{name} *") } else { name.to_string() }
    }

    pub fn scale(&self) -> f32 {
        self.zoom * PT_TO_SCREEN
    }

    pub fn page_sizes(&self) -> Vec<(f32, f32)> {
        (0..self.doc.page_count())
            .map(|i| self.display_geometry(i).map_or((595.0, 842.0), |g| g.view_size()))
            .collect()
    }

    /// Screen position of a PDF point as of the last frame (used by tests and pop-ups).
    pub fn screen_pos(&self, page: usize, pdf: PointF) -> Option<Pos2> {
        let slot = self.layout.slot(page)?;
        let v = self.display_geometry(page)?.pdf_to_view(pdf);
        Some(self.canvas_origin + slot.rect.min.to_vec2() + Vec2::new(v.x, v.y) * self.scale())
    }

    /// Page geometry as displayed (page rotation plus view rotation).
    pub fn display_geometry(&self, page: usize) -> Option<PageGeometry> {
        self.doc
            .geometry(page)
            .map(|g| PageGeometry::new(g.crop, g.rotation.rotated_by(self.view_rotation.degrees())))
    }

    pub fn render_options(&self) -> RenderOptions {
        RenderOptions {
            rotation: self.view_rotation,
            annotations: self.show_annotations,
            recolor: self.recolor,
            transparent: self.transparent,
            filter: self.doc.filter_key(),
            ..RenderOptions::default()
        }
    }

    /// Navigates to page and remembers the current page for "previous view".
    pub fn jump_to_page(&mut self, page: usize) {
        if page != self.current_page {
            self.back.push(self.current_page);
            if self.back.len() > 100 {
                self.back.remove(0);
            }
            self.forward.clear();
        }
        self.go_to_page(page);
    }

    pub fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    pub fn go_back(&mut self) {
        if let Some(page) = self.back.pop() {
            self.forward.push(self.current_page);
            self.go_to_page(page);
        }
    }

    pub fn go_forward(&mut self) {
        if let Some(page) = self.forward.pop() {
            self.back.push(self.current_page);
            self.go_to_page(page);
        }
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
        // Comment edits on other pages keep this entry.
        if let Some(c) = self.texts.get(&page).filter(|c| c.revision >= self.doc.page_revision(page)) {
            return Some(c.value.clone());
        }
        let text = Arc::new(self.doc.page_text(page).ok()?);
        self.texts.insert(page, Cached { revision: rev, value: text.clone() });
        Some(text)
    }

    pub fn annotations(&mut self, page: usize) -> Arc<Vec<AnnotationInfo>> {
        let rev = self.doc.revision();
        if let Some(c) = self.annots.get(&page).filter(|c| c.revision >= self.doc.page_revision(page)) {
            return c.value.clone();
        }
        let list = Arc::new(self.doc.annotations(page).unwrap_or_default());
        self.annots.insert(page, Cached { revision: rev, value: list.clone() });
        list
    }

    /// The objects of a page ("Objekte bearbeiten").
    pub fn objects(&mut self, page: usize) -> Arc<Vec<crate::content::ContentObject>> {
        let rev = self.doc.revision();
        if let Some(c) =
            self.content_objects.get(&page).filter(|c| c.revision >= self.doc.page_revision(page))
        {
            return c.value.clone();
        }
        let list = Arc::new(self.doc.page_objects(page).unwrap_or_default());
        self.content_objects.insert(page, Cached { revision: rev, value: list.clone() });
        list
    }

    pub fn links(&mut self, page: usize) -> Arc<Vec<LinkInfo>> {
        let rev = self.doc.revision();
        if let Some(c) = self.links.get(&page).filter(|c| c.revision >= self.doc.page_revision(page)) {
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
            .flat_map(|p| {
                self.annotations(p).iter().filter(|a| a.kind.is_comment()).cloned().collect::<Vec<_>>()
            })
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

    /// All selected comments: the multi selection or the single one.
    pub fn selected_annots(&self) -> Vec<AnnotRef> {
        match self.selected_annot {
            None => Vec::new(),
            Some(s) if self.multi_annots.first() == Some(&s) => self.multi_annots.clone(),
            Some(s) => vec![s],
        }
    }

    /// Selects several comments of one page (empty: nothing selected).
    pub fn select_annots(&mut self, annots: Vec<AnnotRef>) {
        self.selected_annot = annots.first().copied();
        self.multi_annots = if annots.len() > 1 { annots } else { Vec::new() };
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

    /// Whether the document has bookmarks, from the cached list (true while
    /// it was not read yet for this revision).
    pub fn has_bookmarks(&self) -> bool {
        match &self.bookmarks {
            Some(c) if c.revision == self.doc.revision() => !c.value.is_empty(),
            _ => true,
        }
    }

    /// Called after any document change.
    pub fn after_edit(&mut self) {
        if let Some(sel) = self.selected_annot {
            let count = self.annotations(sel.page).len();
            if sel.index >= count {
                self.selected_annot = None;
            }
            if self.multi_annots.iter().any(|a| a.index >= count) {
                self.multi_annots.clear();
            }
        }
        if self.current_page >= self.doc.page_count() {
            self.current_page = self.doc.page_count().saturating_sub(1);
        }
        if let Some(path) = self.selected_bookmark.clone()
            && !bookmark_exists(&self.bookmarks(), &path)
        {
            self.selected_bookmark = None;
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
            if let (Some(geo), Some(r)) = (self.display_geometry(page), hit.rects.first()) {
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
