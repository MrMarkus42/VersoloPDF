//! Document side of the ribbon tab "Lesezeichen".

use super::Document;
use crate::error::{Error, Result};
use crate::geometry::RectF;
use crate::model::{Bookmark, Rgba};
use crate::outline::NewBookmark;
use crate::outline_edit::{
    self, ActionKind, Appearance, Finding, MergeOptions, Placement, Problem, Scope, Zoom, ZoomTargets,
};
use crate::outline_format::{ItemInfo, NumberStyle, TextRule, TitleFormat, collapse_spaces};
use crate::pdf_edit::{self, LinkLook, LinkSpec};
use crate::report::{self, Leader, Line, Margins};

/// "Seitenzahlen ausgeben als".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TocPageNumbers {
    /// "Original Seitenzahlen".
    #[default]
    Original,
    /// "Seitentitel" and "Paginierung (falls vorhanden)": the page label.
    Label,
}

/// Look of one level of a table of contents.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TocLevel {
    pub size: f32,
    pub bold: bool,
    pub color: Rgba,
}

/// Options of "Inhaltsverzeichnis erstellen".
#[derive(Debug, Clone, PartialEq)]
pub struct TocOptions {
    /// "Titel erstellen" with its text.
    pub title: Option<String>,
    pub title_size: f32,
    /// "Unterer Abstand" below the title (points).
    pub title_gap: f32,
    /// Look per level (the last one is used for deeper levels).
    pub levels: Vec<TocLevel>,
    /// "Inhaltsverzeichnis vor Seite" (0-based); None: "Als letzte Seite".
    pub before: Option<usize>,
    /// Page size; None: like the page at the insert position.
    pub size: Option<(f32, f32)>,
    pub margins: Margins,
    /// Levels to use (1-based, inclusive); None: all.
    pub level_range: Option<(usize, usize)>,
    /// "Ungültige Lesezeichen überspringen".
    pub skip_invalid: bool,
    /// "Mehrfache Lesezeichen aus Lesezeichentiteln entfernen".
    pub remove_repeated: bool,
    pub page_numbers: TocPageNumbers,
    /// Format: None = "Kapitel" (no page number).
    pub leader: Option<Leader>,
    /// "Inhaltsnummerierung" of the top level.
    pub numbering: Option<NumberStyle>,
    pub number_prefix: String,
    pub number_start: usize,
    /// "Zeilenabstand" (extra, points).
    pub spacing: f32,
    /// "Einrücken" per level (points).
    pub indent: f32,
}

impl Default for TocOptions {
    fn default() -> Self {
        Self {
            title: Some("Inhaltsverzeichnis".into()),
            title_size: 18.0,
            title_gap: 0.0,
            levels: vec![
                TocLevel { size: 12.0, bold: true, color: Rgba::BLACK },
                TocLevel { size: 10.0, bold: false, color: Rgba::BLACK },
            ],
            before: Some(0),
            size: None,
            margins: [72.0; 4],
            level_range: None,
            skip_invalid: true,
            remove_repeated: false,
            page_numbers: TocPageNumbers::Original,
            leader: Some(Leader::Dots),
            numbering: None,
            number_prefix: String::new(),
            number_start: 1,
            spacing: 0.0,
            indent: 36.0,
        }
    }
}

/// Options of "Verweise aus Lesezeichen".
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LinkOptions {
    pub scope: Scope,
    pub pages: Vec<usize>,
    /// "Textformat": only text lines matching this rule (level unused).
    pub filter: Option<TextRule>,
    pub look: LinkLook,
    pub ignore_case: bool,
    pub ignore_spaces: bool,
    pub whole_words: bool,
    /// "Verweise über bestehenden erzeugen".
    pub over_existing: bool,
    /// "Weißraum aus Lesezeichen entfernen".
    pub trim: bool,
    /// "Textmuster": only titles matching it.
    pub pattern: Option<String>,
}

/// Options of "Aus Seitentext".
#[derive(Debug, Clone, PartialEq)]
pub struct TextBookmarkOptions {
    /// Rules with the look and formatting of their bookmarks.
    pub rules: Vec<(TextRule, Appearance, TitleFormat)>,
    pub pages: Vec<usize>,
    pub placement: Placement,
    /// "Text mit Stoppwörtern ignorieren".
    pub stop_words: Option<Vec<String>>,
    /// "Aufeinanderfolgende doppelte Lesezeichen ignorieren".
    pub skip_repeated: bool,
}

impl Document {
    /// File name, page count and title for placeholders.
    pub fn outline_base(&self) -> ItemInfo {
        ItemInfo {
            pages: self.page_count(),
            file_name: self.display_name().to_string(),
            doc_title: self.metadata().title,
            ..Default::default()
        }
    }

    /// Runs a bookmark edit that returns the number of changed items.
    fn edit_outline(
        &mut self,
        label: &str,
        f: impl FnOnce(&[u8], Option<&str>) -> Result<(Vec<u8>, usize)>,
    ) -> Result<usize> {
        self.require(self.permissions().modify)?;
        let mut changed = 0;
        self.edit_bytes(label, |b, pw| {
            let (bytes, n) = f(b, pw)?;
            changed = n;
            Ok(bytes)
        })?;
        Ok(changed)
    }

    /// Rewrites the titles in `scope` ("Text hinzufügen", "Groß-/Klein-
    /// schreibung", "Suchen und ersetzen").
    pub fn bookmarks_map_titles(
        &mut self,
        scope: &Scope,
        f: impl Fn(&str, &ItemInfo) -> String,
    ) -> Result<usize> {
        let base = self.outline_base();
        self.edit_outline("edit bookmarks", |b, pw| outline_edit::map_titles(b, pw, scope, &base, f))
    }

    /// "Sprungmarken zu normalen Zielen".
    pub fn bookmarks_resolve_named(&mut self, scope: &Scope, remove_names: bool) -> Result<usize> {
        self.edit_outline("resolve named destinations", |b, pw| {
            outline_edit::resolve_named(b, pw, scope, remove_names)
        })
    }

    /// "In Sprungmarken umwandeln" with the name `mask`.
    pub fn bookmarks_to_named(&mut self, scope: &Scope, mask: &str) -> Result<usize> {
        let base = self.outline_base();
        self.edit_outline("create named destinations", |b, pw| {
            outline_edit::to_named(b, pw, scope, mask, &base)
        })
    }

    /// "Vergrößerung ändern".
    pub fn bookmarks_set_zoom(&mut self, scope: &Scope, zoom: Zoom, targets: ZoomTargets) -> Result<usize> {
        self.edit_outline("bookmark zoom", |b, pw| outline_edit::set_zoom(b, pw, scope, zoom, targets))
    }

    /// "Aktionen entfernen".
    pub fn bookmarks_remove_actions(
        &mut self,
        scope: &Scope,
        kinds: &[ActionKind],
        remove_bookmarks: bool,
        keep_children: bool,
    ) -> Result<usize> {
        self.edit_outline("remove bookmark actions", |b, pw| {
            outline_edit::remove_actions(b, pw, scope, kinds, remove_bookmarks, keep_children)
        })
    }

    /// "Sortieren".
    pub fn sort_bookmarks_in(
        &mut self,
        scope: &Scope,
        by_page: bool,
        descending: bool,
        ignore_case: bool,
    ) -> Result<()> {
        self.require(self.permissions().modify)?;
        self.edit_bytes("sort bookmarks", |b, pw| {
            outline_edit::sort(b, pw, scope, by_page, descending, ignore_case)
        })
    }

    /// "Duplikate zusammenfassen". Returns the number of merged bookmarks.
    pub fn merge_duplicate_bookmarks(&mut self, scope: &Scope, opts: MergeOptions) -> Result<usize> {
        self.edit_outline("merge duplicate bookmarks", |b, pw| {
            outline_edit::merge_duplicates(b, pw, scope, opts)
        })
    }

    /// "Überprüfen".
    pub fn check_bookmarks(&mut self, wanted: &[Problem]) -> Result<Vec<Finding>> {
        self.sync_bytes()?;
        outline_edit::check(&self.bytes, self.password.as_deref(), wanted)
    }

    /// Deletes the bookmarks at `paths`; their children move up.
    pub fn delete_bookmark_paths(&mut self, paths: &[Vec<usize>]) -> Result<usize> {
        self.edit_outline("delete bookmarks", |b, pw| outline_edit::delete_paths(b, pw, paths))
    }

    /// Adds bookmarks, each with its look ("Aussehen"), placed as told.
    pub fn add_bookmarks_styled(
        &mut self,
        items: &[(NewBookmark, Appearance)],
        placement: Placement,
    ) -> Result<usize> {
        self.require(self.permissions().modify)?;
        if items.is_empty() && placement != Placement::Replace {
            return Ok(0);
        }
        self.edit_bytes("add bookmarks", |b, pw| outline_edit::add_styled(b, pw, items, placement))?;
        Ok(items.len())
    }

    /// "Seiten sortieren": pages in the order of the bookmarks. Returns
    /// whether the order changed.
    pub fn sort_pages_by_bookmarks(&mut self) -> Result<bool> {
        self.require(self.permissions().assemble)?;
        self.sync_bytes()?;
        let targets = outline_edit::pages_in_order(&self.bytes, self.password.as_deref())?;
        let order = outline_edit::page_order_by_bookmarks(self.page_count(), &targets);
        if order.iter().enumerate().all(|(i, &p)| i == p) {
            return Ok(false);
        }
        self.edit_bytes("sort pages by bookmarks", |b, pw| pdf_edit::reorder_pages(b, pw, &order))?;
        Ok(true)
    }

    /// "Aus Inhaltsverzeichnis": bookmarks from the entries ("Titel ....
    /// 12") of the given pages. `offset` is added to the printed page
    /// numbers; None finds it from where the first title appears.
    pub fn toc_bookmarks(&self, toc_pages: &[usize], offset: Option<i64>) -> Result<Vec<NewBookmark>> {
        let mut entries = Vec::new();
        for &page in toc_pages {
            let lines: Vec<String> = self.page_text(page)?.lines().into_iter().map(|(l, _)| l).collect();
            entries.extend(outline_edit::parse_toc_lines(&lines));
        }
        let n = self.page_count() as i64;
        let offset = match offset {
            Some(o) => o,
            None => {
                let after = toc_pages.iter().max().map_or(0, |p| p + 1);
                let simple = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
                let mut found = 0;
                if let Some(e) = entries.first() {
                    let title = simple(&e.title);
                    for page in after..self.page_count() {
                        if simple(&self.page_text(page)?.all_text()).contains(&title) {
                            found = page as i64 - (e.number - 1);
                            break;
                        }
                    }
                }
                found
            }
        };
        Ok(entries
            .into_iter()
            .map(|e| NewBookmark {
                title: e.title,
                page: (e.number - 1 + offset).clamp(0, (n - 1).max(0)) as usize,
                level: e.level,
            })
            .collect())
    }

    /// Suggested rules for "Aus Seitentext": one per font size clearly
    /// larger than the body text (at most three, largest = top level).
    pub fn suggested_text_rules(&self) -> Result<Vec<TextRule>> {
        let mut weight: std::collections::BTreeMap<i32, usize> = std::collections::BTreeMap::new();
        for page in 0..self.page_count().min(60) {
            for line in self.page_text_styled(page)?.styled_lines() {
                *weight.entry((line.size * 2.0).round() as i32).or_default() += line.text.chars().count();
            }
        }
        let Some((&body, _)) = weight.iter().max_by_key(|(_, n)| **n) else { return Ok(Vec::new()) };
        let mut sizes: Vec<i32> = weight.keys().copied().filter(|s| *s as f32 >= body as f32 * 1.2).collect();
        sizes.sort_unstable_by(|a, b| b.cmp(a));
        Ok(sizes
            .into_iter()
            .take(3)
            .enumerate()
            .map(|(level, s)| TextRule { level, size: Some((s as f32 / 2.0, 0.3)), ..Default::default() })
            .collect())
    }

    /// "Aus Hervorhebungen": the text under the highlight comments of
    /// `pages` (or their comment text).
    pub fn highlight_bookmarks(&mut self, pages: &[usize]) -> Result<Vec<NewBookmark>> {
        let mut items = Vec::new();
        for c in self.comments()? {
            if c.kind != crate::model::AnnotKind::Highlight || !pages.contains(&c.page) {
                continue;
            }
            let areas = if c.quads.is_empty() { vec![c.rect] } else { c.quads.clone() };
            let mut title = self.page_text(c.page)?.text_in_rects(&areas);
            if title.is_empty() {
                title = c.contents.trim().to_string();
            }
            if !title.is_empty() {
                items.push(NewBookmark { title: title.chars().take(256).collect(), page: c.page, level: 0 });
            }
        }
        Ok(items)
    }

    /// "Aus Seitentext": the bookmarks the rules find on `opts.pages`, with
    /// the look of their rule. Lines of the same rule that follow each
    /// other are joined when the rule allows several lines.
    pub fn text_bookmarks(&self, opts: &TextBookmarkOptions) -> Result<Vec<(NewBookmark, usize)>> {
        let compiled: Vec<_> = opts
            .rules
            .iter()
            .map(|(r, _, f)| Ok((r.compile()?, f.compile()?)))
            .collect::<Result<Vec<_>>>()?;
        let stop = |t: &str| {
            opts.stop_words.as_ref().is_some_and(|words| {
                t.split(|c: char| !c.is_alphanumeric())
                    .any(|w| words.iter().any(|s| !s.trim().is_empty() && s.trim().eq_ignore_ascii_case(w)))
            })
        };
        let mut out: Vec<(NewBookmark, usize)> = Vec::new();
        let mut last_rule: Option<(usize, usize)> = None;
        let base = self.outline_base();
        for &page in &opts.pages {
            let text = self.page_text_styled(page)?;
            for line in text.styled_lines() {
                let hit =
                    opts.rules.iter().enumerate().find_map(|(i, (rule, _, _))| {
                        rule.title(&line, compiled[i].0.as_ref()).map(|t| (i, t))
                    });
                let Some((rule, title)) = hit else {
                    last_rule = None;
                    continue;
                };
                if stop(&title) {
                    continue;
                }
                if opts.rules[rule].0.multiline
                    && last_rule == Some((rule, page))
                    && let Some((prev, _)) = out.last_mut()
                {
                    prev.title.push(' ');
                    prev.title.push_str(&title);
                    continue;
                }
                last_rule = Some((rule, page));
                out.push((NewBookmark { title, page, level: opts.rules[rule].0.level }, rule));
            }
        }
        // Formatting after joining, so placeholders see the whole title.
        for (n, (b, rule)) in out.iter_mut().enumerate() {
            let info =
                ItemInfo { title: b.title.clone(), page: Some(b.page), sequence: n + 1, ..base.clone() };
            b.title = opts.rules[*rule].2.apply(&b.title, n + 1, compiled[*rule].1.as_ref(), &info);
        }
        if opts.skip_repeated {
            out.dedup_by(|a, b| a.0.title == b.0.title && a.0.level == b.0.level);
        }
        out.retain(|(b, _)| !b.title.trim().is_empty());
        Ok(out)
    }

    /// Adds the bookmarks of `text_bookmarks`; each rule keeps its look.
    pub fn add_text_bookmarks(&mut self, opts: &TextBookmarkOptions) -> Result<usize> {
        let found = self.text_bookmarks(opts)?;
        if found.is_empty() {
            return Ok(0);
        }
        let items: Vec<(NewBookmark, Appearance)> =
            found.into_iter().map(|(b, rule)| (b, opts.rules[rule].1)).collect();
        self.add_bookmarks_styled(&items, opts.placement)
    }

    /// "Verweise aus Lesezeichen": where a bookmark title appears in the
    /// text of `opts.pages`, a link to the bookmark's page is added.
    /// Returns the number of new links.
    pub fn links_from_bookmarks_with(&mut self, opts: &LinkOptions) -> Result<usize> {
        self.require(self.permissions().modify)?;
        let marks = self.scoped_targets(&opts.scope)?;
        if marks.is_empty() {
            return Err(Error::Invalid("the document has no bookmarks".into()));
        }
        let pattern = match opts.pattern.as_deref().filter(|p| !p.is_empty()) {
            Some(p) => Some(
                regex::RegexBuilder::new(p)
                    .size_limit(1 << 20)
                    .build()
                    .map_err(|e| Error::Invalid(format!("invalid search pattern: {e}")))?,
            ),
            None => None,
        };
        let filter = match &opts.filter {
            Some(f) => Some((f, f.compile()?)),
            None => None,
        };
        let mut per_page = Vec::new();
        for &page in &opts.pages {
            let text = self.page_text_styled(page)?;
            let lines = text.styled_lines();
            let existing: Vec<RectF> = if opts.over_existing {
                Vec::new()
            } else {
                self.links(page)?.into_iter().map(|l| l.rect).collect()
            };
            let mut links = Vec::new();
            for (title, target) in &marks {
                let mut t = if opts.trim { title.trim().to_string() } else { title.clone() };
                if opts.ignore_spaces {
                    t = collapse_spaces(&t);
                }
                if t.trim().is_empty() || *target == page {
                    continue;
                }
                if pattern.as_ref().is_some_and(|re| !re.is_match(&t)) {
                    continue;
                }
                let mut core = regex::escape(&t);
                if opts.ignore_spaces {
                    core = core.replace(' ', r"\s+");
                }
                if opts.whole_words {
                    core = format!(r"\b{core}\b");
                }
                let Ok(re) = regex::RegexBuilder::new(&core).case_insensitive(opts.ignore_case).build()
                else {
                    continue;
                };
                for line in &lines {
                    if let Some((rule, re)) = &filter
                        && rule.title(line, re.as_ref()).is_none()
                    {
                        continue;
                    }
                    let Some(m) = re.find(&line.text) else { continue };
                    let rect = line_part(&text, line, m.start(), m.end());
                    if existing.iter().any(|r| r.intersects(&rect)) {
                        continue;
                    }
                    links.push((rect, LinkSpec::Page(*target)));
                    break;
                }
            }
            if !links.is_empty() {
                per_page.push((page, links));
            }
        }
        let count = per_page.iter().map(|(_, l)| l.len()).sum();
        if count == 0 {
            return Ok(0);
        }
        let look = opts.look;
        self.edit_bytes("links from bookmarks", |b, pw| {
            let mut bytes = b.to_vec();
            for (page, links) in &per_page {
                bytes = pdf_edit::add_links_styled(&bytes, pw, *page, links, &look)?;
            }
            Ok(bytes)
        })?;
        Ok(count)
    }

    /// "Verweise aus Lesezeichen" with the default options on `pages`.
    pub fn links_from_bookmarks(&mut self, pages: &[usize]) -> Result<usize> {
        let opts = LinkOptions { pages: pages.to_vec(), ignore_case: true, ..Default::default() };
        self.links_from_bookmarks_with(&opts)
    }

    /// (title, page) of the bookmarks in `scope` that lead to a page.
    fn scoped_targets(&self, scope: &Scope) -> Result<Vec<(String, usize)>> {
        fn walk(items: &[Bookmark], path: &mut Vec<usize>, scope: &Scope, out: &mut Vec<(String, usize)>) {
            for (i, b) in items.iter().enumerate() {
                path.push(i);
                if let Some(p) = b.page
                    && scope.contains(path)
                {
                    out.push((b.title.clone(), p));
                }
                walk(&b.children, path, scope, out);
                path.pop();
            }
        }
        let mut out = Vec::new();
        walk(&self.bookmarks(), &mut Vec::new(), scope, &mut out);
        Ok(out)
    }

    /// "Inhaltsverzeichnis erstellen" with options. Returns the number of
    /// inserted pages.
    pub fn insert_toc_with(&mut self, opts: &TocOptions) -> Result<usize> {
        self.require(self.permissions().modify && self.permissions().assemble)?;
        fn walk(items: &[Bookmark], depth: usize, out: &mut Vec<(String, Option<usize>, usize)>) {
            for b in items {
                out.push((b.title.clone(), b.page, depth));
                walk(&b.children, depth + 1, out);
            }
        }
        let mut entries = Vec::new();
        walk(&self.bookmarks(), 0, &mut entries);
        if let Some((lo, hi)) = opts.level_range {
            entries.retain(|(_, _, d)| (lo.max(1)..=hi.max(lo)).contains(&(d + 1)));
        }
        if opts.skip_invalid {
            entries.retain(|(_, p, _)| p.is_some());
        }
        if opts.remove_repeated {
            let mut seen = Vec::new();
            entries.retain(|(t, _, d)| {
                let key = (t.trim().to_lowercase(), *d);
                if seen.contains(&key) {
                    false
                } else {
                    seen.push(key);
                    true
                }
            });
        }
        if entries.is_empty() {
            return Err(Error::Invalid("the document has no bookmarks".into()));
        }
        let n = self.page_count();
        let before = opts.before.unwrap_or(n).min(n);
        let labels: Vec<Option<String>> = match opts.page_numbers {
            TocPageNumbers::Original => Vec::new(),
            TocPageNumbers::Label => (0..n).map(|p| self.page_label(p)).collect(),
        };
        let make = |inserted: usize| {
            let mut lines = Vec::new();
            if let Some(title) = &opts.title {
                lines.push(Line {
                    size: opts.title_size.clamp(4.0, 72.0),
                    bold: true,
                    ..Line::new(title.clone())
                });
                if opts.title_gap > 0.0 {
                    lines.push(Line { size: 1.0, space_before: opts.title_gap, ..Line::new("") });
                }
            }
            let mut number = opts.number_start;
            for (title, page, depth) in &entries {
                let style = opts.levels.get(*depth).or(opts.levels.last()).copied().unwrap_or(TocLevel {
                    size: 10.0,
                    bold: false,
                    color: Rgba::BLACK,
                });
                let mut text = title.clone();
                if *depth == 0
                    && let Some(st) = opts.numbering
                {
                    text = format!("{}{} {text}", opts.number_prefix, st.format(number));
                    number += 1;
                }
                let right = match (opts.leader, page) {
                    (None, _) | (_, None) => String::new(),
                    (Some(_), Some(p)) => match labels.get(*p).cloned().flatten() {
                        Some(l) if !l.is_empty() => l,
                        _ => (p + 1 + if *p >= before { inserted } else { 0 }).to_string(),
                    },
                };
                lines.push(Line {
                    size: style.size.clamp(4.0, 72.0),
                    bold: style.bold,
                    color: style.color,
                    indent: *depth as f32 * opts.indent.max(0.0),
                    space_before: if *depth == 0 { 4.0 } else { 0.0 },
                    link: *page,
                    right,
                    leader: opts.leader.unwrap_or(Leader::Space),
                    ..Line::new(text)
                });
            }
            lines
        };
        let size = opts.size.unwrap_or_else(|| {
            let near = before.min(n.saturating_sub(1));
            self.geometry(near).map_or(report::A4, |g| g.view_size())
        });
        let placement =
            report::Placement { before, size: Some(size), margins: opts.margins, spacing: opts.spacing };
        // The page numbers shown include the inserted pages themselves.
        let count = report::layout_with(&make(0), size, opts.margins, opts.spacing).len();
        let lines = make(count);
        let mut inserted = 0;
        self.edit_bytes("insert table of contents", |b, pw| {
            let (bytes, k) = report::insert_at(b, pw, &lines, &placement)?;
            inserted = k;
            Ok(bytes)
        })?;
        Ok(inserted)
    }
}

/// Area of the characters `start..end` (byte offsets in `line.text`).
fn line_part(
    text: &crate::text::PageText,
    line: &crate::text::StyledLine,
    start: usize,
    end: usize,
) -> RectF {
    // Count visible characters of the line up to the match.
    let skip = line.text[..start].chars().filter(|c| !c.is_whitespace()).count();
    let take = line.text[start..end].chars().filter(|c| !c.is_whitespace()).count().max(1);
    let mut rect: Option<RectF> = None;
    let mut seen = 0;
    for c in text.chars.iter().skip(line.first) {
        if c.ch == '\n' || c.ch == '\r' {
            break;
        }
        if c.generated || c.ch.is_whitespace() || c.rect.is_empty() {
            continue;
        }
        if seen >= skip && seen < skip + take {
            rect = Some(rect.map_or(c.rect, |r| r.union(&c.rect)));
        }
        seen += 1;
        if seen >= skip + take {
            break;
        }
    }
    rect.unwrap_or(line.rect)
}
