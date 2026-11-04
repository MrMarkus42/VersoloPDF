//! "Dokumente vergleichen": the dialog (old and new document with preview
//! and page range), the comparison shown side by side in
//! two tab groups with the differences marked in colour, the results list
//! on the right and scrolling both documents together.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use egui::{Color32, Context, RichText, Ui, vec2};

use super::dialogs::{Action, Outcome};
use super::page_dialogs::{footer, tr};
use super::preview::{PreviewOpts, doc_preview, page_strip};
use super::windows::MoveTarget;
use super::{Dialog, MrPdfApp};
use crate::Document;
use crate::compare::{self, DiffKind, Difference, Mark, Word};
use crate::geometry::{PointF, RectF};
use crate::model::Rgba;
use crate::report::Line;
use crate::ui::i18n::Language;
use crate::ui::pickers::{PageRange, RangeMode};
use crate::ui::tab::{CompareMark, ScrollRequest};

/// Where a side of the comparison comes from.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    /// An open document (tab id).
    Tab(u64),
    /// A file chosen in the dialog (loaded into `MrPdfApp::compare_files`).
    File(PathBuf),
}

/// One side ("Altes Dokument" / "Neues Dokument") of the dialog.
#[derive(Debug, Clone, PartialEq)]
pub struct Side {
    pub source: Option<Source>,
    pub range: PageRange,
    /// Page shown in the preview.
    pub page: usize,
}

/// State of the dialog.
#[derive(Debug, Clone, PartialEq)]
pub struct CompareSetup {
    pub old: Side,
    pub new: Side,
    /// "Nur Text vergleichen": no comparison of pictures and drawings.
    pub text_only: bool,
}

/// Steps of the calculation (done a little in every frame).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    OldText,
    NewText,
    Words,
    Graphics,
}

struct Job {
    stage: Stage,
    /// Position within the current stage.
    k: usize,
    old_words: Vec<Word>,
    new_words: Vec<Word>,
    line: (u32, u32),
}

/// A running or finished comparison.
pub struct Comparison {
    pub old: u64,
    pub new: u64,
    old_pages: Vec<usize>,
    new_pages: Vec<usize>,
    text_only: bool,
    job: Option<Job>,
    pub diffs: Vec<Difference>,
    pairs: Vec<(Option<usize>, Option<usize>)>,
    pub selected: Option<usize>,
    /// Kinds not shown.
    hidden: Vec<DiffKind>,
    /// Scroll both documents together.
    sync: bool,
    /// Scroll position and zoom of both documents in the last frame.
    last: [(egui::Vec2, f32); 2],
    started: Instant,
    took: Option<Duration>,
}

const KINDS: [DiffKind; 6] = [
    DiffKind::Changed,
    DiffKind::Inserted,
    DiffKind::Deleted,
    DiffKind::Graphic,
    DiffKind::PageInserted,
    DiffKind::PageDeleted,
];

fn kind_name(kind: DiffKind, de: bool) -> &'static str {
    match kind {
        DiffKind::Inserted => tr(de, "Eingefügt", "Inserted"),
        DiffKind::Deleted => tr(de, "Gelöscht", "Deleted"),
        DiffKind::Changed => tr(de, "Geändert", "Changed"),
        DiffKind::Graphic => tr(de, "Grafik geändert", "Graphics changed"),
        DiffKind::PageInserted => tr(de, "Seite eingefügt", "Page inserted"),
        DiffKind::PageDeleted => tr(de, "Seite gelöscht", "Page deleted"),
    }
}

/// The whole page as a rectangle in PDF space.
fn page_rect(doc: &Document, page: usize) -> Option<RectF> {
    let g = doc.geometry(page)?;
    let (w, h) = g.view_size();
    Some(RectF::from_points(g.view_to_pdf(PointF::new(0.0, 0.0)), g.view_to_pdf(PointF::new(w, h))))
}

/// A PDF rectangle as 0..1 of the page (top left origin).
fn to_unit(doc: &Document, page: usize, r: RectF) -> Option<[f32; 4]> {
    let g = doc.geometry(page)?;
    let (w, h) = g.view_size();
    let a = g.pdf_to_view(PointF::new(r.x0, r.y0));
    let b = g.pdf_to_view(PointF::new(r.x1, r.y1));
    Some([a.x.min(b.x) / w, a.y.min(b.y) / h, a.x.max(b.x) / w, a.y.max(b.y) / h])
}

/// A rectangle in 0..1 of the page as PDF rectangle.
fn from_unit(doc: &Document, page: usize, u: [f32; 4]) -> Option<RectF> {
    let g = doc.geometry(page)?;
    let (w, h) = g.view_size();
    Some(RectF::from_points(
        g.view_to_pdf(PointF::new(u[0] * w, u[1] * h)),
        g.view_to_pdf(PointF::new(u[2] * w, u[3] * h)),
    ))
}

impl Comparison {
    /// The calculation is still going on.
    pub fn running(&self) -> bool {
        self.job.is_some()
    }
}

impl MrPdfApp {
    /// The comparison shown, if any.
    pub fn comparison(&self) -> Option<&Comparison> {
        self.compare.as_deref()
    }

    /// Vertical scroll positions of the old and the new document.
    pub fn compared_scroll(&self) -> Option<(f32, f32)> {
        let c = self.compare.as_ref()?;
        let y = |id| self.tab_index(id).map(|i| self.tabs[i].scroll_offset.y);
        Some((y(c.old)?, y(c.new)?))
    }

    fn cmp_de(&self) -> bool {
        self.settings.language == Language::German
    }

    fn tab_index(&self, id: u64) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }

    /// "Dokumente vergleichen": the dialog, the active document as old one.
    pub(super) fn open_compare_dialog(&mut self) {
        self.clear_previews();
        let old = self.tabs.get(self.active).map(|t| Source::Tab(t.id));
        // Another open document is the obvious new one.
        let new =
            self.tabs.iter().enumerate().find(|(i, _)| *i != self.active).map(|(_, t)| Source::Tab(t.id));
        let side = |source| Side { source, range: PageRange::all(), page: 0 };
        self.dialog = Some(Dialog::Compare(Box::new(CompareSetup {
            old: side(old),
            new: side(new),
            text_only: false,
        })));
    }

    /// The document of a side (open tab or loaded file).
    fn source_doc(&self, source: &Option<Source>) -> Option<&Document> {
        match source.as_ref()? {
            Source::Tab(id) => self.tab_index(*id).map(|i| &self.tabs[i].doc),
            Source::File(path) => self.compare_files.iter().find(|(p, _)| p == path).map(|(_, d)| d),
        }
    }

    fn source_name(&self, source: &Option<Source>) -> String {
        match source {
            Some(Source::Tab(id)) => self.tab_index(*id).map(|i| self.tabs[i].title()).unwrap_or_default(),
            Some(Source::File(p)) => {
                p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
            }
            None => String::new(),
        }
    }

    /// One column of the dialog.
    fn compare_side(&mut self, ui: &mut Ui, side: &mut Side, slot: usize, title: &str) {
        let de = self.cmp_de();
        ui.label(title);
        ui.horizontal(|ui| {
            let shown = self.source_name(&side.source);
            let placeholder =
                tr(de, "Klicken Sie hier, um ein Dokument auszuwählen …", "Click to choose a document …");
            egui::ComboBox::from_id_salt(("compare-source", slot))
                .width(ui.available_width() - 36.0)
                .selected_text(if shown.is_empty() { placeholder.to_string() } else { shown })
                .show_ui(ui, |ui| {
                    for t in &self.tabs {
                        let s = Some(Source::Tab(t.id));
                        if ui.selectable_label(side.source == s, t.title()).clicked() {
                            side.source = s;
                            side.page = 0;
                        }
                    }
                    for (p, _) in &self.compare_files {
                        let s = Some(Source::File(p.clone()));
                        let name =
                            p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        if ui.selectable_label(side.source == s, name).clicked() {
                            side.source = s;
                            side.page = 0;
                        }
                    }
                });
            if crate::ui::widgets::icon_button(
                ui,
                crate::ui::icons::Icon::Open,
                tr(de, "Datei auswählen …", "Choose a file …"),
                false,
                true,
            )
            .clicked()
                && let Some(path) = rfd::FileDialog::new().add_filter("PDF", &["pdf", "PDF"]).pick_file()
            {
                if !self.compare_files.iter().any(|(p, _)| *p == path) {
                    match Document::open(&path, None) {
                        Ok(doc) => self.compare_files.push((path.clone(), doc)),
                        Err(err) => {
                            self.show_error(&err);
                            return;
                        }
                    }
                }
                side.source = Some(Source::File(path));
                side.page = 0;
            }
        });
        let n = self.source_doc(&side.source).map_or(0, Document::page_count);
        // Smaller previews in small windows, so OK stays in view.
        let height = (ui.ctx().content_rect().height() - 520.0).clamp(140.0, 300.0);
        let size = vec2(ui.available_width(), height);
        let opts = PreviewOpts::new(de);
        let shown = match side.source.clone() {
            Some(Source::Tab(id)) => self.tab_index(id).and_then(|i| {
                side.page = side.page.min(n.saturating_sub(1));
                let tab = &self.tabs[i];
                doc_preview(ui, &mut self.preview[slot], &tab.doc, tab.id, side.page, size, opts)
            }),
            Some(Source::File(path)) => {
                side.page = side.page.min(n.saturating_sub(1));
                let doc = self.compare_files.iter().find(|(p, _)| *p == path).map(|(_, d)| d);
                doc.and_then(|d| {
                    doc_preview(ui, &mut self.preview[slot], d, 0xc0 + slot as u64, side.page, size, opts)
                })
            }
            None => None,
        };
        if shown.is_none() {
            let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
            ui.painter().rect_filled(rect, 0.0, Color32::from_gray(128));
        }
        if n > 0 {
            page_strip(ui, &mut side.page, n);
        }
        side.range.ui(ui, de, n, side.page, &[RangeMode::All, RangeMode::Custom]);
    }

    pub(super) fn compare_dialog(&mut self, ui: &mut Ui, s: &mut CompareSetup) -> (Outcome, Option<Action>) {
        let de = self.cmp_de();
        super::dialogs::set_title(ui, tr(de, "Dokumente vergleichen", "Compare documents"));
        ui.set_width(760.0);
        ui.horizontal_top(|ui| {
            let w = (ui.available_width() - 40.0) / 2.0;
            // Each side its own id space (both have a page range grid).
            ui.push_id("compare-old", |ui| {
                ui.vertical(|ui| {
                    ui.set_width(w);
                    self.compare_side(ui, &mut s.old, 0, tr(de, "Altes Dokument:", "Old document:"));
                })
            });
            ui.vertical(|ui| {
                ui.add_space(170.0);
                if crate::ui::widgets::icon_button(
                    ui,
                    crate::ui::icons::Icon::Swap,
                    tr(de, "Altes und neues Dokument tauschen", "Swap old and new document"),
                    false,
                    true,
                )
                .clicked()
                {
                    std::mem::swap(&mut s.old, &mut s.new);
                    self.clear_previews();
                }
            });
            ui.push_id("compare-new", |ui| {
                ui.vertical(|ui| {
                    ui.set_width(w);
                    self.compare_side(ui, &mut s.new, 1, tr(de, "Neues Dokument:", "New document:"));
                })
            });
        });
        ui.separator();
        ui.label(RichText::new(tr(de, "Einstellungen", "Settings")).weak());
        ui.checkbox(&mut s.text_only, tr(de, "Nur Text vergleichen", "Compare text only")).on_hover_text(tr(
            de,
            "Ohne Bilder, Linien und Farben; schneller bei großen Dokumenten.",
            "Without pictures, lines and colours; faster for large documents.",
        ));
        let same = s.old.source.is_some() && s.old.source == s.new.source;
        if same {
            ui.colored_label(
                Color32::from_rgb(200, 40, 40),
                tr(de, "Bitte zwei verschiedene Dokumente wählen.", "Please choose two different documents."),
            );
        }
        let pages = |side: &Side| {
            let n = self.source_doc(&side.source).map_or(0, Document::page_count);
            side.range.resolve(n, side.page)
        };
        let (old_pages, new_pages) = (pages(&s.old), pages(&s.new));
        let ready = !same && old_pages.is_some() && new_pages.is_some();
        let (ok, cancel, _) = footer(ui, de, ready, false, "OK");
        if cancel {
            self.clear_previews();
            return (Outcome::Close, None);
        }
        if ok && let (Some(op), Some(np)) = (old_pages, new_pages) {
            self.clear_previews();
            let (old, new, text_only) = (s.old.source.clone(), s.new.source.clone(), s.text_only);
            return (
                Outcome::Close,
                Some(Box::new(move |app, ctx| app.start_compare(ctx, old, new, op, np, text_only))),
            );
        }
        (Outcome::Keep, None)
    }

    /// The tab of a side, opening a chosen file as a new tab.
    fn compare_tab(&mut self, source: Option<Source>) -> Option<u64> {
        match source? {
            Source::Tab(id) => self.tab_index(id).map(|_| id),
            Source::File(path) => {
                let canonical = crate::files::canonical_path(&path);
                if let Some(t) = self
                    .tabs
                    .iter()
                    .find(|t| t.doc.path().map(crate::files::canonical_path) == Some(canonical.clone()))
                {
                    return Some(t.id);
                }
                let k = self.compare_files.iter().position(|(p, _)| *p == path)?;
                let (_, doc) = self.compare_files.remove(k);
                self.add_tab(doc);
                self.tabs.last().map(|t| t.id)
            }
        }
    }

    /// Opens both documents side by side and starts the calculation.
    fn start_compare(
        &mut self,
        ctx: &Context,
        old: Option<Source>,
        new: Option<Source>,
        old_pages: Vec<usize>,
        new_pages: Vec<usize>,
        text_only: bool,
    ) {
        self.end_compare();
        let (Some(old), Some(new)) = (self.compare_tab(old), self.compare_tab(new)) else { return };
        self.compare_files.clear();
        // Old document on the left, new one in a group on the right.
        self.move_tabs(&[old], MoveTarget::MainWindow);
        self.merge_groups();
        self.move_tabs(&[new], MoveTarget::NewVerticalGroup);
        if let (Some(a), Some(b)) = (self.tab_index(old), self.tab_index(new)) {
            self.group_focus.insert((0, self.tabs[a].group), old);
            self.group_focus.insert((0, self.tabs[b].group), new);
            // The same view on both sides.
            let (zoom, mode) = (self.tabs[a].zoom, self.tabs[a].zoom_mode);
            self.tabs[b].zoom = zoom;
            self.tabs[b].zoom_mode = mode;
            self.active = a;
        }
        self.settings.show_properties = false;
        self.compare = Some(Box::new(Comparison {
            old,
            new,
            old_pages,
            new_pages,
            text_only,
            job: Some(Job {
                stage: Stage::OldText,
                k: 0,
                old_words: Vec::new(),
                new_words: Vec::new(),
                line: (0, 0),
            }),
            diffs: Vec::new(),
            pairs: Vec::new(),
            selected: None,
            hidden: Vec::new(),
            sync: true,
            last: [(egui::Vec2::ZERO, 0.0); 2],
            started: Instant::now(),
            took: None,
        }));
        ctx.request_repaint();
    }

    /// "Vergleich beenden": marks away, one tab group again.
    pub(super) fn end_compare(&mut self) {
        if self.compare.take().is_some() {
            for t in &mut self.tabs {
                t.compare_marks.clear();
            }
            self.merge_groups();
        }
    }

    /// Every frame: continues the calculation, keeps both documents
    /// scrolled together.
    pub(super) fn compare_tick(&mut self, ctx: &Context) {
        let Some(c) = self.compare.as_ref() else { return };
        let (Some(a), Some(b)) = (self.tab_index(c.old), self.tab_index(c.new)) else {
            // A compared document was closed.
            self.end_compare();
            return;
        };
        if self.compare.as_ref().is_some_and(|c| c.job.is_some()) {
            self.compare_work(a, b, Instant::now() + Duration::from_millis(30));
            ctx.request_repaint();
        }
        self.compare_sync(a, b);
    }

    fn compare_work(&mut self, a: usize, b: usize, until: Instant) {
        let Some(mut c) = self.compare.take() else { return };
        let Some(mut job) = c.job.take() else {
            self.compare = Some(c);
            return;
        };
        let mut finished = false;
        while Instant::now() < until && !finished {
            match job.stage {
                Stage::OldText | Stage::NewText => {
                    let old = job.stage == Stage::OldText;
                    let (pages, tab) = if old { (&c.old_pages, a) } else { (&c.new_pages, b) };
                    match pages.get(job.k).copied() {
                        Some(p) => {
                            let line = if old { &mut job.line.0 } else { &mut job.line.1 };
                            if let Some(text) = self.tabs[tab].page_text(p) {
                                let words = compare::page_words(p, &text, line);
                                if old { job.old_words.extend(words) } else { job.new_words.extend(words) }
                            }
                            job.k += 1;
                        }
                        None => {
                            job.stage = if old { Stage::NewText } else { Stage::Words };
                            job.k = 0;
                        }
                    }
                }
                Stage::Words => {
                    c.diffs = compare::word_differences(&job.old_words, &job.new_words);
                    c.pairs = compare::pair_pages(&job.old_words, &job.new_words, &c.old_pages, &c.new_pages);
                    self.page_differences(&mut c, a, b);
                    job.stage = Stage::Graphics;
                    job.k = 0;
                    finished = c.text_only;
                }
                Stage::Graphics => match c.pairs.get(job.k).copied() {
                    Some((Some(p), Some(q))) => {
                        if let Some(d) = self.graphic_difference(&job, a, b, p, q) {
                            c.diffs.extend(d);
                        }
                        job.k += 1;
                    }
                    Some(_) => job.k += 1,
                    None => finished = true,
                },
            }
        }
        if finished {
            self.sort_differences(&mut c);
            c.took = Some(c.started.elapsed());
            c.selected = None;
        } else {
            c.job = Some(job);
        }
        self.compare = Some(c);
        self.update_compare_marks();
    }

    /// Pages only in one document; their text differences go into them.
    fn page_differences(&self, c: &mut Comparison, a: usize, b: usize) {
        let only_old: Vec<usize> =
            c.pairs.iter().filter_map(|p| if p.1.is_none() { p.0 } else { None }).collect();
        let only_new: Vec<usize> =
            c.pairs.iter().filter_map(|p| if p.0.is_none() { p.1 } else { None }).collect();
        // A "change" between pages that do not belong together is text
        // removed on one and added on the other.
        let paired = |p: usize, q: usize| c.pairs.contains(&(Some(p), Some(q)));
        let mut split = Vec::with_capacity(c.diffs.len());
        for d in std::mem::take(&mut c.diffs) {
            match d.pages() {
                (Some(p), Some(q)) if d.kind == DiffKind::Changed && !paired(p, q) => {
                    split.push(Difference {
                        kind: DiffKind::Deleted,
                        old: d.old,
                        new: Vec::new(),
                        old_text: d.old_text,
                        new_text: String::new(),
                    });
                    split.push(Difference {
                        kind: DiffKind::Inserted,
                        old: Vec::new(),
                        new: d.new,
                        old_text: String::new(),
                        new_text: d.new_text,
                    });
                }
                _ => split.push(d),
            }
        }
        c.diffs = split;
        let inside = |marks: &[Mark], pages: &[usize]| {
            !marks.is_empty() && marks.iter().all(|m| pages.contains(&m.page))
        };
        c.diffs.retain(|d| {
            let old_gone = d.new.iter().all(|m| m.caret) && inside(&d.old, &only_old);
            let new_gone = d.old.iter().all(|m| m.caret) && inside(&d.new, &only_new);
            !(old_gone || new_gone)
        });
        let (da, db) = (&self.tabs[a].doc, &self.tabs[b].doc);
        for &p in &only_old {
            if let Some(r) = page_rect(da, p) {
                c.diffs.push(Difference {
                    kind: DiffKind::PageDeleted,
                    old: vec![Mark { page: p, rect: r, caret: false }],
                    new: Vec::new(),
                    old_text: String::new(),
                    new_text: String::new(),
                });
            }
        }
        for &q in &only_new {
            if let Some(r) = page_rect(db, q) {
                c.diffs.push(Difference {
                    kind: DiffKind::PageInserted,
                    old: Vec::new(),
                    new: vec![Mark { page: q, rect: r, caret: false }],
                    old_text: String::new(),
                    new_text: String::new(),
                });
            }
        }
    }

    /// Changed pictures and drawings of a page pair (outside text).
    fn graphic_difference(
        &self,
        job: &Job,
        a: usize,
        b: usize,
        p: usize,
        q: usize,
    ) -> Option<Vec<Difference>> {
        let (da, db) = (&self.tabs[a].doc, &self.tabs[b].doc);
        let (vw, vh) = da.geometry(p)?.view_size();
        let w = 360u32;
        let h = ((w as f32 * vh / vw.max(1.0)).round() as u32).clamp(1, 1200);
        let ia = da.render_page(p, w, h).ok()?;
        let ib = db.render_page(q, w, h).ok()?;
        let mut text: Vec<[f32; 4]> = Vec::new();
        text.extend(job.old_words.iter().filter(|x| x.page == p).filter_map(|x| to_unit(da, p, x.rect)));
        text.extend(job.new_words.iter().filter(|x| x.page == q).filter_map(|x| to_unit(db, q, x.rect)));
        let regions = compare::graphic_regions(&ia.rgba, &ib.rgba, w as usize, h as usize, &text);
        Some(
            regions
                .into_iter()
                .filter_map(|u| {
                    Some(Difference {
                        kind: DiffKind::Graphic,
                        old: vec![Mark { page: p, rect: from_unit(da, p, u)?, caret: false }],
                        new: vec![Mark { page: q, rect: from_unit(db, q, u)?, caret: false }],
                        old_text: String::new(),
                        new_text: String::new(),
                    })
                })
                .collect(),
        )
    }

    /// Reading order: by page pair, then from the top of the page.
    fn sort_differences(&self, c: &mut Comparison) {
        let pairs = c.pairs.clone();
        let order = |d: &Difference| {
            let (p, q) = d.pages();
            let pair = pairs
                .iter()
                .position(|x| (p.is_some() && x.0 == p) || (q.is_some() && x.1 == q))
                .unwrap_or(usize::MAX);
            let top = d.old.first().or(d.new.first()).map_or(0.0, |m| -m.rect.normalized().y1);
            (pair, (top * 10.0) as i64)
        };
        c.diffs.sort_by_key(order);
    }

    /// The marks shown on both documents (filtered, chosen one stronger).
    fn update_compare_marks(&mut self) {
        let Some(c) = self.compare.as_ref() else { return };
        let mut old = Vec::new();
        let mut new = Vec::new();
        for (i, d) in c.diffs.iter().enumerate().filter(|(_, d)| !c.hidden.contains(&d.kind)) {
            let selected = c.selected == Some(i);
            let mark =
                |m: &Mark| CompareMark { page: m.page, rect: m.rect, kind: d.kind, caret: m.caret, selected };
            old.extend(d.old.iter().map(mark));
            new.extend(d.new.iter().map(mark));
        }
        let (o, n) = (c.old, c.new);
        if let Some(i) = self.tab_index(o) {
            self.tabs[i].compare_marks = old;
        }
        if let Some(i) = self.tab_index(n) {
            self.tabs[i].compare_marks = new;
        }
    }

    /// Shows difference `k` in both documents.
    fn go_to_difference(&mut self, k: usize) {
        let Some(c) = self.compare.as_mut() else { return };
        let Some(d) = c.diffs.get(k).cloned() else { return };
        c.selected = Some(k);
        let (o, n) = (c.old, c.new);
        for (id, marks) in [(o, &d.old), (n, &d.new)] {
            let Some(i) = self.tab_index(id) else { continue };
            let tab = &mut self.tabs[i];
            if let Some(m) = marks.first()
                && let Some(g) = tab.doc.geometry(m.page)
            {
                let r = m.rect.normalized();
                let centre = g.pdf_to_view(PointF::new((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0));
                tab.pending_scroll = Some(ScrollRequest::Point(m.page, centre));
            }
        }
        self.update_compare_marks();
    }

    /// Scrolls (and zooms) the other document along with the one moved.
    fn compare_sync(&mut self, a: usize, b: usize) {
        let Some(c) = self.compare.as_mut() else { return };
        let now = [
            (self.tabs[a].scroll_offset, self.tabs[a].zoom),
            (self.tabs[b].scroll_offset, self.tabs[b].zoom),
        ];
        let moved =
            |k: usize| (now[k].0 - c.last[k].0).length() > 0.5 || (now[k].1 - c.last[k].1).abs() > 1e-4;
        let (ma, mb) = (moved(0), moved(1));
        c.last = now;
        if !c.sync || ma == mb {
            return;
        }
        let (from, to, forward) = if ma { (a, b, true) } else { (b, a, false) };
        // Zoom along.
        if (self.tabs[from].zoom - self.tabs[to].zoom).abs() > 1e-4 {
            let (zoom, mode) = (self.tabs[from].zoom, self.tabs[from].zoom_mode);
            self.tabs[to].zoom = zoom;
            self.tabs[to].zoom_mode = mode;
        }
        // The page at the top of the view and how far into it.
        let y = self.tabs[from].scroll_offset.y;
        let Some(slot) = self.tabs[from].layout.slots.iter().find(|s| s.rect.bottom() > y).cloned() else {
            return;
        };
        let frac = ((y - slot.rect.top()) / slot.rect.height().max(1.0)).clamp(0.0, 1.0);
        let partner = c.pairs.iter().find_map(|&(p, q)| {
            if forward {
                (p == Some(slot.page)).then_some(q).flatten()
            } else {
                (q == Some(slot.page)).then_some(p).flatten()
            }
        });
        let Some(target) = partner else { return };
        let Some(other) = self.tabs[to].layout.slots.iter().find(|s| s.page == target).cloned() else {
            return;
        };
        let offset = vec2(self.tabs[to].scroll_offset.x, other.rect.top() + frac * other.rect.height());
        self.tabs[to].pending_scroll = Some(ScrollRequest::Offset(offset));
        let k = if forward { 1 } else { 0 };
        c.last[k].0 = offset;
    }

    /// The results list on the right.
    pub(super) fn compare_panel(&mut self, ui: &mut Ui) {
        let de = self.cmp_de();
        let Some(c) = self.compare.as_ref() else { return };
        let names = (
            self.tab_index(c.old).map(|i| self.tabs[i].title()).unwrap_or_default(),
            self.tab_index(c.new).map(|i| self.tabs[i].title()).unwrap_or_default(),
        );
        let mut go: Option<usize> = None;
        let mut end = false;
        let mut report = false;
        egui::Panel::right("compare-results").resizable(true).default_size(330.0).show(ui, |ui| {
            let Some(c) = self.compare.as_mut() else { return };
            ui.add_space(4.0);
            ui.heading(tr(de, "Vergleichsergebnisse", "Comparison results"));
            ui.label(RichText::new(format!("{}: {}", tr(de, "Alt (links)", "Old (left)"), names.0)).small());
            ui.label(
                RichText::new(format!("{}: {}", tr(de, "Neu (rechts)", "New (right)"), names.1)).small(),
            );
            ui.separator();
            if let Some(job) = &c.job {
                let (step, done, total) = match job.stage {
                    Stage::OldText => (
                        tr(de, "Text des alten Dokuments …", "Text of the old document …"),
                        job.k,
                        c.old_pages.len(),
                    ),
                    Stage::NewText => (
                        tr(de, "Text des neuen Dokuments …", "Text of the new document …"),
                        job.k,
                        c.new_pages.len(),
                    ),
                    Stage::Words => (tr(de, "Wörter vergleichen …", "Comparing words …"), 0, 1),
                    Stage::Graphics => (
                        tr(de, "Bilder und Grafiken vergleichen …", "Comparing pictures and graphics …"),
                        job.k,
                        c.pairs.len(),
                    ),
                };
                ui.label(step);
                ui.add(egui::ProgressBar::new(done as f32 / total.max(1) as f32).show_percentage());
                if ui.button(tr(de, "Abbrechen", "Cancel")).clicked() {
                    end = true;
                }
                return;
            }
            // Summary with show/hide per kind.
            let total = c.diffs.len();
            ui.label(if total == 0 {
                RichText::new(tr(de, "Keine Unterschiede gefunden.", "No differences found.")).strong()
            } else {
                RichText::new(if de {
                    format!("{total} Unterschiede")
                } else {
                    format!("{total} differences")
                })
                .strong()
            });
            let mut changed_filter = false;
            egui::Grid::new("compare-kinds").num_columns(2).show(ui, |ui| {
                for kind in KINDS {
                    let n = c.diffs.iter().filter(|d| d.kind == kind).count();
                    if n == 0 {
                        continue;
                    }
                    let mut on = !c.hidden.contains(&kind);
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(12.0, 12.0), egui::Sense::hover());
                        ui.painter().rect_filled(r, 2.0, CompareMark::color(kind));
                        if ui.checkbox(&mut on, kind_name(kind, de)).changed() {
                            changed_filter = true;
                        }
                    });
                    ui.label(n.to_string());
                    ui.end_row();
                    if on {
                        c.hidden.retain(|k| *k != kind);
                    } else if !c.hidden.contains(&kind) {
                        c.hidden.push(kind);
                    }
                }
            });
            if changed_filter {
                c.selected = None;
            }
            ui.horizontal(|ui| {
                let visible: Vec<usize> =
                    (0..c.diffs.len()).filter(|&i| !c.hidden.contains(&c.diffs[i].kind)).collect();
                let pos = c.selected.and_then(|s| visible.iter().position(|&i| i == s));
                let prev = match pos {
                    Some(p) if p > 0 => Some(visible[p - 1]),
                    None => visible.last().copied(),
                    _ => None,
                };
                let next = match pos {
                    Some(p) => visible.get(p + 1).copied(),
                    None => visible.first().copied(),
                };
                if ui
                    .add_enabled(prev.is_some(), egui::Button::new(tr(de, "◀ Vorheriger", "◀ Previous")))
                    .clicked()
                {
                    go = prev;
                }
                if ui.add_enabled(next.is_some(), egui::Button::new(tr(de, "Nächster ▶", "Next ▶"))).clicked()
                {
                    go = next;
                }
            });
            ui.checkbox(&mut c.sync, tr(de, "Gemeinsam blättern und zoomen", "Scroll and zoom together"));
            if let Some(t) = c.took {
                ui.label(
                    RichText::new(if de {
                        format!("Berechnet in {:.1} s", t.as_secs_f32())
                    } else {
                        format!("Calculated in {:.1} s", t.as_secs_f32())
                    })
                    .small()
                    .weak(),
                );
            }
            ui.separator();
            // The list (only the visible rows are laid out).
            let visible: Vec<usize> =
                (0..c.diffs.len()).filter(|&i| !c.hidden.contains(&c.diffs[i].kind)).collect();
            // Two lines of small text plus room, in the font in use.
            let row_h =
                ui.text_style_height(&egui::TextStyle::Small) * 2.0 + ui.spacing().item_spacing.y + 12.0;
            let footer_h = 70.0;
            egui::ScrollArea::vertical()
                .max_height((ui.available_height() - footer_h).max(80.0))
                .auto_shrink([false, false])
                .show_rows(ui, row_h, visible.len(), |ui, range| {
                    for &i in &visible[range] {
                        let d = &c.diffs[i];
                        let (p, q) = d.pages();
                        let page = |x: Option<usize>| x.map_or("–".to_string(), |x| (x + 1).to_string());
                        let snippet = |s: &str| {
                            let s: String = s.chars().take(48).collect();
                            if s.is_empty() { String::new() } else { format!("„{s}“") }
                        };
                        let text = match d.kind {
                            DiffKind::Changed => {
                                format!("{} ▶ {}", snippet(&d.old_text), snippet(&d.new_text))
                            }
                            DiffKind::Deleted => snippet(&d.old_text),
                            DiffKind::Inserted => snippet(&d.new_text),
                            _ => String::new(),
                        };
                        let selected = c.selected == Some(i);
                        let resp = ui
                            .horizontal(|ui| {
                                ui.set_min_height(row_h - 4.0);
                                let (r, _) =
                                    ui.allocate_exact_size(vec2(6.0, row_h - 8.0), egui::Sense::hover());
                                ui.painter().rect_filled(r, 2.0, CompareMark::color(d.kind));
                                ui.vertical(|ui| {
                                    ui.label(
                                        RichText::new(format!(
                                            "{}   {} {} / {}",
                                            kind_name(d.kind, de),
                                            tr(de, "S.", "p."),
                                            page(p),
                                            page(q)
                                        ))
                                        .strong()
                                        .small(),
                                    );
                                    if !text.is_empty() {
                                        ui.add(egui::Label::new(RichText::new(text).small()).truncate());
                                    }
                                });
                            })
                            .response
                            .interact(egui::Sense::click());
                        if selected {
                            ui.painter().rect_stroke(
                                resp.rect.expand(1.0),
                                3.0,
                                egui::Stroke::new(1.5, crate::ui::theme::ACCENT),
                                egui::StrokeKind::Outside,
                            );
                        }
                        if resp.clicked() {
                            go = Some(i);
                        }
                    }
                });
            ui.separator();
            ui.horizontal(|ui| {
                if ui
                    .button(tr(de, "Bericht erstellen", "Create report"))
                    .on_hover_text(tr(
                        de,
                        "Alle Unterschiede als neues PDF-Dokument",
                        "All differences as a new PDF document",
                    ))
                    .clicked()
                {
                    report = true;
                }
                if ui.button(tr(de, "Vergleich beenden", "End comparison")).clicked() {
                    end = true;
                }
            });
        });
        if let Some(k) = go {
            self.go_to_difference(k);
        }
        if report {
            let ctx = ui.ctx().clone();
            self.compare_report(&ctx);
        }
        if end {
            self.end_compare();
        } else {
            self.update_compare_marks();
        }
    }

    /// "Bericht erstellen": the differences as a new document.
    fn compare_report(&mut self, ctx: &Context) {
        let de = self.cmp_de();
        let Some(c) = self.compare.as_ref() else { return };
        let name = |id: u64| self.tab_index(id).map(|i| self.tabs[i].title()).unwrap_or_default();
        let title = tr(de, "Dokumentvergleich", "Document comparison");
        let mut lines = vec![
            Line::title(title),
            Line::small(format!(
                "{} {}  ·  {} {}",
                tr(de, "Alt:", "Old:"),
                name(c.old),
                tr(de, "Neu:", "New:"),
                name(c.new)
            )),
        ];
        let mut summary = Vec::new();
        for kind in KINDS {
            let n = c.diffs.iter().filter(|d| d.kind == kind).count();
            if n > 0 {
                summary.push(format!("{n} × {}", kind_name(kind, de)));
            }
        }
        lines.push(Line::new(if summary.is_empty() {
            tr(de, "Keine Unterschiede gefunden.", "No differences found.").to_string()
        } else {
            summary.join(", ")
        }));
        let color = |k: DiffKind| {
            let c = CompareMark::color(k);
            Rgba::rgb(c.r(), c.g(), c.b())
        };
        for d in &c.diffs {
            let (p, q) = d.pages();
            let page = |x: Option<usize>| x.map_or("–".to_string(), |x| (x + 1).to_string());
            let head = format!("{}  {} {} / {}", kind_name(d.kind, de), tr(de, "S.", "p."), page(p), page(q));
            lines.push(Line { space_before: 6.0, ..Line::new(head).color(color(d.kind)) });
            if !d.old_text.is_empty() {
                lines.push(Line::small(format!("{} {}", tr(de, "Alt:", "Old:"), d.old_text)));
            }
            if !d.new_text.is_empty() {
                lines.push(Line::small(format!("{} {}", tr(de, "Neu:", "New:"), d.new_text)));
            }
        }
        self.open_report(ctx, title, &lines);
    }
}
