//! Comparison of two documents: the line based text report and the side
//! by side comparison (words with their place on the page, page pairing,
//! changed graphics).

use std::collections::HashMap;
use std::hash::Hash;

use crate::geometry::RectF;
use crate::text::PageText;

/// One line of a comparison result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// Line present in both documents: (page in old, page in new, text).
    Same(usize, usize, String),
    /// Line only in the old document.
    Removed(usize, String),
    /// Line only in the new document.
    Added(usize, String),
}

/// Upper bound for the dynamic-programming table (cells).
const MAX_CELLS: usize = 16_000_000;

/// Compares two documents given as (page, line) lists.
pub fn diff(old: &[(usize, String)], new: &[(usize, String)]) -> Vec<Change> {
    // Common prefix and suffix need no table.
    let prefix = old.iter().zip(new).take_while(|(a, b)| a.1 == b.1).count();
    let suffix =
        old[prefix..].iter().rev().zip(new[prefix..].iter().rev()).take_while(|(a, b)| a.1 == b.1).count();
    let a = &old[prefix..old.len() - suffix];
    let b = &new[prefix..new.len() - suffix];

    let mut out: Vec<Change> =
        old[..prefix].iter().zip(&new[..prefix]).map(|(x, y)| Change::Same(x.0, y.0, x.1.clone())).collect();
    if a.len().saturating_mul(b.len()) > MAX_CELLS {
        // Too large for an exact comparison: report the middle as replaced.
        out.extend(a.iter().map(|(p, t)| Change::Removed(*p, t.clone())));
        out.extend(b.iter().map(|(p, t)| Change::Added(*p, t.clone())));
    } else {
        // lcs[i][j] = length of the longest common subsequence of a[i..], b[j..].
        let (n, m) = (a.len(), b.len());
        let mut lcs = vec![0u32; (n + 1) * (m + 1)];
        let idx = |i: usize, j: usize| i * (m + 1) + j;
        for i in (0..n).rev() {
            for j in (0..m).rev() {
                lcs[idx(i, j)] = if a[i].1 == b[j].1 {
                    lcs[idx(i + 1, j + 1)] + 1
                } else {
                    lcs[idx(i + 1, j)].max(lcs[idx(i, j + 1)])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < n || j < m {
            if i < n && j < m && a[i].1 == b[j].1 {
                out.push(Change::Same(a[i].0, b[j].0, a[i].1.clone()));
                i += 1;
                j += 1;
            } else if j < m && (i == n || lcs[idx(i, j + 1)] >= lcs[idx(i + 1, j)]) {
                out.push(Change::Added(b[j].0, b[j].1.clone()));
                j += 1;
            } else {
                out.push(Change::Removed(a[i].0, a[i].1.clone()));
                i += 1;
            }
        }
    }
    let start_old = old.len() - suffix;
    let start_new = new.len() - suffix;
    out.extend(
        old[start_old..].iter().zip(&new[start_new..]).map(|(x, y)| Change::Same(x.0, y.0, x.1.clone())),
    );
    out
}

// ---------------------------------------------------------------------
// Side by side comparison

/// A word of a page with its box (PDF space).
#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub page: usize,
    pub text: String,
    pub rect: RectF,
    /// Text line on the page (counted over the whole document).
    pub line: u32,
}

/// The words of a page; `line` continues the line numbering.
pub fn page_words(page: usize, text: &PageText, line: &mut u32) -> Vec<Word> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut rect: Option<RectF> = None;
    let mut flush = |word: &mut String, rect: &mut Option<RectF>, line: u32| {
        if !word.is_empty()
            && let Some(r) = rect.take()
        {
            out.push(Word { page, text: std::mem::take(word), rect: r, line });
        }
        word.clear();
        *rect = None;
    };
    for c in &text.chars {
        if c.ch == '\n' || c.ch == '\r' {
            flush(&mut word, &mut rect, *line);
            *line += 1;
        } else if c.ch.is_whitespace() || c.ch.is_control() {
            flush(&mut word, &mut rect, *line);
        } else if is_punctuation(c.ch) {
            // Punctuation is a word of its own: "Monat." and "Monat" match.
            flush(&mut word, &mut rect, *line);
            word.push(c.ch);
            if !c.generated && !c.rect.is_empty() {
                rect = Some(c.rect.normalized());
            }
            flush(&mut word, &mut rect, *line);
        } else {
            word.push(c.ch);
            if !c.generated && !c.rect.is_empty() {
                rect = Some(rect.map_or(c.rect.normalized(), |r| r.union(&c.rect)));
            }
        }
    }
    flush(&mut word, &mut rect, *line);
    *line += 1;
    out
}

/// Characters compared as words of their own.
fn is_punctuation(c: char) -> bool {
    (c.is_ascii_punctuation() && !matches!(c, '-' | '_' | '/' | '@' | '&' | '%'))
        || matches!(c, '„' | '“' | '”' | '‚' | '‘' | '’' | '«' | '»' | '–' | '—' | '…')
}

/// Splits word indices where the page changes.
fn by_page(words: &[Word], ids: &[usize]) -> Vec<Vec<usize>> {
    let mut out: Vec<Vec<usize>> = Vec::new();
    for &i in ids {
        match out.last_mut() {
            Some(g) if words[g[0]].page == words[i].page => g.push(i),
            _ => out.push(vec![i]),
        }
    }
    out
}

/// One step of a sequence comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// a[i] equals b[j].
    Equal(usize, usize),
    /// a[i] only in the first sequence.
    Delete(usize),
    /// b[j] only in the second sequence.
    Insert(usize),
}

/// Largest table for the exact comparison of a stretch without anchors.
const MAX_STRETCH_CELLS: usize = 4_000_000;

/// Compares two sequences ("patience" method: elements that occur exactly
/// once in both are anchors, the stretches between them are compared
/// recursively, small stretches exactly). Fast also for whole books.
pub fn diff_seq<T: Eq + Hash>(a: &[T], b: &[T]) -> Vec<Op> {
    let mut out = Vec::with_capacity(a.len().max(b.len()));
    patience(a, b, 0, 0, &mut out);
    out
}

fn patience<T: Eq + Hash>(a: &[T], b: &[T], ao: usize, bo: usize, out: &mut Vec<Op>) {
    // Common start and end.
    let pre = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    out.extend((0..pre).map(|k| Op::Equal(ao + k, bo + k)));
    let (a, b) = (&a[pre..], &b[pre..]);
    let (ao, bo) = (ao + pre, bo + pre);
    let suf = a.iter().rev().zip(b.iter().rev()).take_while(|(x, y)| x == y).count();
    let (ma, mb) = (&a[..a.len() - suf], &b[..b.len() - suf]);
    if ma.is_empty() || mb.is_empty() {
        out.extend((0..ma.len()).map(|k| Op::Delete(ao + k)));
        out.extend((0..mb.len()).map(|k| Op::Insert(bo + k)));
    } else {
        // Elements occurring once in each: (position in a, position in b).
        let mut count: HashMap<&T, (u32, u32, usize, usize)> = HashMap::new();
        for (i, x) in ma.iter().enumerate() {
            let e = count.entry(x).or_insert((0, 0, i, 0));
            e.0 += 1;
        }
        for (j, y) in mb.iter().enumerate() {
            if let Some(e) = count.get_mut(y) {
                e.1 += 1;
                e.3 = j;
            }
        }
        let mut uniq: Vec<(usize, usize)> =
            count.values().filter(|e| e.0 == 1 && e.1 == 1).map(|e| (e.2, e.3)).collect();
        uniq.sort_unstable();
        let anchors = longest_increasing(&uniq);
        if anchors.is_empty() {
            exact(ma, mb, ao, bo, out);
        } else {
            let (mut pa, mut pb) = (0, 0);
            for (i, j) in anchors {
                patience(&ma[pa..i], &mb[pb..j], ao + pa, bo + pb, out);
                out.push(Op::Equal(ao + i, bo + j));
                (pa, pb) = (i + 1, j + 1);
            }
            patience(&ma[pa..], &mb[pb..], ao + pa, bo + pb, out);
        }
    }
    let (ea, eb) = (ao + ma.len(), bo + mb.len());
    out.extend((0..suf).map(|k| Op::Equal(ea + k, eb + k)));
}

/// Longest chain of pairs increasing in the second element (the first is
/// sorted already).
fn longest_increasing(pairs: &[(usize, usize)]) -> Vec<(usize, usize)> {
    // tails[k] = index of the smallest end of a chain of length k + 1.
    let mut tails: Vec<usize> = Vec::new();
    let mut prev = vec![usize::MAX; pairs.len()];
    for (k, &(_, j)) in pairs.iter().enumerate() {
        let pos = tails.partition_point(|&t| pairs[t].1 < j);
        if pos > 0 {
            prev[k] = tails[pos - 1];
        }
        if pos == tails.len() {
            tails.push(k);
        } else {
            tails[pos] = k;
        }
    }
    let mut chain = Vec::with_capacity(tails.len());
    let mut k = tails.last().copied().unwrap_or(usize::MAX);
    while k != usize::MAX {
        chain.push(pairs[k]);
        k = prev[k];
    }
    chain.reverse();
    chain
}

/// Exact comparison (longest common subsequence); too large stretches
/// count as replaced.
fn exact<T: Eq>(a: &[T], b: &[T], ao: usize, bo: usize, out: &mut Vec<Op>) {
    let (n, m) = (a.len(), b.len());
    if n.saturating_mul(m) > MAX_STRETCH_CELLS {
        out.extend((0..n).map(|k| Op::Delete(ao + k)));
        out.extend((0..m).map(|k| Op::Insert(bo + k)));
        return;
    }
    let idx = |i: usize, j: usize| i * (m + 1) + j;
    let mut lcs = vec![0u32; (n + 1) * (m + 1)];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[idx(i, j)] = if a[i] == b[j] {
                lcs[idx(i + 1, j + 1)] + 1
            } else {
                lcs[idx(i + 1, j)].max(lcs[idx(i, j + 1)])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            out.push(Op::Equal(ao + i, bo + j));
            i += 1;
            j += 1;
        } else if i < n && (j == m || lcs[idx(i + 1, j)] >= lcs[idx(i, j + 1)]) {
            out.push(Op::Delete(ao + i));
            i += 1;
        } else {
            out.push(Op::Insert(bo + j));
            j += 1;
        }
    }
}

/// Kind of a difference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiffKind {
    /// Text only in the new document.
    Inserted,
    /// Text only in the old document.
    Deleted,
    /// Text replaced by other text.
    Changed,
    /// Pictures, lines, colours: the page looks different outside the text.
    Graphic,
    /// A page only in the new document.
    PageInserted,
    /// A page only in the old document.
    PageDeleted,
}

/// A marked area: page and rectangle in PDF space; `caret` marks only the
/// place where text is missing on this side.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mark {
    pub page: usize,
    pub rect: RectF,
    pub caret: bool,
}

/// One difference with its places in both documents.
#[derive(Debug, Clone, PartialEq)]
pub struct Difference {
    pub kind: DiffKind,
    pub old: Vec<Mark>,
    pub new: Vec<Mark>,
    pub old_text: String,
    pub new_text: String,
}

impl Difference {
    /// The first page in the old and in the new document.
    pub fn pages(&self) -> (Option<usize>, Option<usize>) {
        (self.old.first().map(|m| m.page), self.new.first().map(|m| m.page))
    }
}

/// One rectangle per text line of the words `ids`.
fn line_marks(words: &[Word], ids: &[usize]) -> Vec<Mark> {
    let mut out: Vec<(u32, Mark)> = Vec::new();
    for &i in ids {
        let w = &words[i];
        match out.last_mut() {
            Some((line, m)) if *line == w.line && m.page == w.page => m.rect = m.rect.union(&w.rect),
            _ => out.push((w.line, Mark { page: w.page, rect: w.rect, caret: false })),
        }
    }
    out.into_iter().map(|(_, m)| m).collect()
}

/// A thin mark before word `i` (or after the last word) where text is
/// missing on this side.
fn caret(words: &[Word], i: usize) -> Vec<Mark> {
    let (w, at_end) = match words.get(i) {
        Some(w) => (w, false),
        None => match words.last() {
            Some(w) => (w, true),
            None => return Vec::new(),
        },
    };
    let r = w.rect.normalized();
    let x = if at_end { r.x1 + 1.0 } else { r.x0 - 1.0 };
    vec![Mark { page: w.page, rect: RectF::new(x - 1.0, r.y0, x + 1.0, r.y1), caret: true }]
}

/// Text differences of two word lists, each stretch of changes one entry.
pub fn word_differences(old: &[Word], new: &[Word]) -> Vec<Difference> {
    let a: Vec<&str> = old.iter().map(|w| w.text.as_str()).collect();
    let b: Vec<&str> = new.iter().map(|w| w.text.as_str()).collect();
    let ops = diff_seq(&a, &b);
    let mut out = Vec::new();
    let (mut dels, mut ins): (Vec<usize>, Vec<usize>) = (Vec::new(), Vec::new());
    // Next words on both sides after a stretch (for the carets).
    let mut flush = |dels: &mut Vec<usize>, ins: &mut Vec<usize>, next_a: usize, next_b: usize| {
        if dels.is_empty() && ins.is_empty() {
            return;
        }
        // Words as text again, no space before closing punctuation.
        let join = |words: &[Word], ids: &[usize]| {
            let mut s = String::new();
            for &i in ids {
                let t = words[i].text.as_str();
                let tight = t
                    .chars()
                    .all(|c| matches!(c, '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '“' | '”' | '’'))
                    || s.ends_with(['(', '[', '„', '‚']);
                if !s.is_empty() && !tight {
                    s.push(' ');
                }
                s.push_str(t);
            }
            s
        };
        // A stretch over several pages: the parts on the same position pair
        // up, the others are only removed or only added.
        let (gd, gi) = (by_page(old, dels), by_page(new, ins));
        for k in 0..gd.len().max(gi.len()) {
            let (d, i): (&[usize], &[usize]) =
                (gd.get(k).map_or(&[], |v| v.as_slice()), gi.get(k).map_or(&[], |v| v.as_slice()));
            let kind = match (d.is_empty(), i.is_empty()) {
                (false, false) => DiffKind::Changed,
                (false, true) => DiffKind::Deleted,
                _ => DiffKind::Inserted,
            };
            out.push(Difference {
                kind,
                old: if d.is_empty() { caret(old, next_a) } else { line_marks(old, d) },
                new: if i.is_empty() { caret(new, next_b) } else { line_marks(new, i) },
                old_text: join(old, d),
                new_text: join(new, i),
            });
        }
        dels.clear();
        ins.clear();
    };
    for op in ops {
        match op {
            Op::Equal(i, j) => flush(&mut dels, &mut ins, i, j),
            Op::Delete(i) => dels.push(i),
            Op::Insert(j) => ins.push(j),
        }
    }
    flush(&mut dels, &mut ins, old.len(), new.len());
    out
}

/// Pairs the pages `old_pages` and `new_pages` (sorted page numbers) of
/// both documents: pages sharing most unchanged words belong together,
/// pages in between are paired in order, the rest exists only on one side.
/// Ordered by the old document; page numbers in the result.
pub fn pair_pages(
    old: &[Word],
    new: &[Word],
    old_pages: &[usize],
    new_pages: &[usize],
) -> Vec<(Option<usize>, Option<usize>)> {
    let index = |pages: &[usize]| -> HashMap<usize, usize> {
        pages.iter().enumerate().map(|(i, &p)| (p, i)).collect()
    };
    let (ia, ib) = (index(old_pages), index(new_pages));
    let a: Vec<&str> = old.iter().map(|w| w.text.as_str()).collect();
    let b: Vec<&str> = new.iter().map(|w| w.text.as_str()).collect();
    let mut votes: HashMap<(usize, usize), usize> = HashMap::new();
    for op in diff_seq(&a, &b) {
        if let Op::Equal(i, j) = op
            && let (Some(&p), Some(&q)) = (ia.get(&old[i].page), ib.get(&new[j].page))
        {
            *votes.entry((p, q)).or_default() += 1;
        }
    }
    let (n_old, n_new) = (old_pages.len(), new_pages.len());
    // Best partner of each old page, kept only while increasing.
    let mut anchors: Vec<(usize, usize)> = Vec::new();
    for p in 0..n_old {
        let best = votes.iter().filter(|(k, _)| k.0 == p).max_by_key(|(k, v)| (**v, std::cmp::Reverse(k.1)));
        if let Some((&(_, n), _)) = best
            && anchors.last().is_none_or(|&(_, last)| n > last)
        {
            anchors.push((p, n));
        }
    }
    let mut out = Vec::new();
    let (mut o, mut n) = (0, 0);
    let fill = |o: &mut usize, n: &mut usize, to_o: usize, to_n: usize, out: &mut Vec<_>| {
        while *o < to_o && *n < to_n {
            out.push((Some(*o), Some(*n)));
            *o += 1;
            *n += 1;
        }
        while *o < to_o {
            out.push((Some(*o), None));
            *o += 1;
        }
        while *n < to_n {
            out.push((None, Some(*n)));
            *n += 1;
        }
    };
    for (ao, an) in anchors {
        fill(&mut o, &mut n, ao, an, &mut out);
        out.push((Some(ao), Some(an)));
        (o, n) = (ao + 1, an + 1);
    }
    fill(&mut o, &mut n, n_old, n_new, &mut out);
    out.into_iter().map(|(p, q)| (p.map(|p| old_pages[p]), q.map(|q| new_pages[q]))).collect()
}

/// Changed areas of two pictures of the same page size (RGBA, `w` × `h`),
/// as rectangles in 0..1 of the page (top left origin). Areas covered by
/// `text` (same units, both documents' words) are left out: text is
/// compared word by word, and moved text would mark everything below.
pub fn graphic_regions(a: &[u8], b: &[u8], w: usize, h: usize, text: &[[f32; 4]]) -> Vec<[f32; 4]> {
    const CELL: usize = 6;
    let (cw, ch) = (w.div_ceil(CELL), h.div_ceil(CELL));
    let mut cells = vec![false; cw * ch];
    let lum = |p: &[u8]| (p[0] as i32 * 3 + p[1] as i32 * 6 + p[2] as i32) / 10;
    for y in 0..h {
        for x in 0..w {
            let k = (y * w + x) * 4;
            if k + 3 < a.len() && k + 3 < b.len() && (lum(&a[k..k + 3]) - lum(&b[k..k + 3])).abs() > 48 {
                cells[(y / CELL) * cw + x / CELL] = true;
            }
        }
    }
    // Cells inside text (slightly enlarged) do not count.
    for t in text {
        let x0 = ((t[0] * w as f32 - 2.0) / CELL as f32).floor().max(0.0) as usize;
        let y0 = ((t[1] * h as f32 - 2.0) / CELL as f32).floor().max(0.0) as usize;
        let x1 = (((t[2] * w as f32 + 2.0) / CELL as f32).ceil() as usize).min(cw);
        let y1 = (((t[3] * h as f32 + 2.0) / CELL as f32).ceil() as usize).min(ch);
        for y in y0..y1 {
            for x in x0..x1 {
                cells[y * cw + x] = false;
            }
        }
    }
    // Groups of neighbouring cells (gaps of one cell bridged).
    let mut seen = vec![false; cw * ch];
    let mut out = Vec::new();
    for start in 0..cells.len() {
        if !cells[start] || seen[start] {
            continue;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (cw, ch, 0, 0);
        let mut count = 0;
        let mut stack = vec![start];
        seen[start] = true;
        while let Some(c) = stack.pop() {
            let (cx, cy) = (c % cw, c / cw);
            count += 1;
            (x0, y0, x1, y1) = (x0.min(cx), y0.min(cy), x1.max(cx), y1.max(cy));
            for dy in -2i32..=2 {
                for dx in -2i32..=2 {
                    let (nx, ny) = (cx as i32 + dx, cy as i32 + dy);
                    if nx < 0 || ny < 0 || nx >= cw as i32 || ny >= ch as i32 {
                        continue;
                    }
                    let n = ny as usize * cw + nx as usize;
                    if cells[n] && !seen[n] {
                        seen[n] = true;
                        stack.push(n);
                    }
                }
            }
        }
        // Single specks are noise (anti-aliasing).
        if count < 2 {
            continue;
        }
        out.push([
            (x0 * CELL) as f32 / w as f32,
            (y0 * CELL) as f32 / h as f32,
            (((x1 + 1) * CELL).min(w)) as f32 / w as f32,
            (((y1 + 1) * CELL).min(h)) as f32 / h as f32,
        ]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(page: usize, text: &str) -> Vec<Word> {
        text.split_whitespace()
            .enumerate()
            .map(|(k, t)| Word {
                page,
                text: t.into(),
                rect: RectF::new(k as f32 * 10.0, 0.0, k as f32 * 10.0 + 8.0, 10.0),
                line: 0,
            })
            .collect()
    }

    #[test]
    fn patience_matches_exact_comparison() {
        let a: Vec<char> = "the quick brown fox jumps over the lazy dog".chars().collect();
        let b: Vec<char> = "the quick red fox jumped over a lazy dog!".chars().collect();
        let ops = diff_seq(&a, &b);
        // Rebuilding both sides from the steps gives the inputs back.
        let mut ra = Vec::new();
        let mut rb = Vec::new();
        for op in &ops {
            match *op {
                Op::Equal(i, j) => {
                    assert_eq!(a[i], b[j]);
                    ra.push(a[i]);
                    rb.push(b[j]);
                }
                Op::Delete(i) => ra.push(a[i]),
                Op::Insert(j) => rb.push(b[j]),
            }
        }
        assert_eq!(ra, a);
        assert_eq!(rb, b);
        let equal = ops.iter().filter(|o| matches!(o, Op::Equal(..))).count();
        assert!(equal >= 30, "{equal}");
    }

    #[test]
    fn large_inputs_stay_fast() {
        let a: Vec<u32> = (0..200_000).collect();
        let mut b = a.clone();
        b.remove(50_000);
        b.insert(150_000, 999_999);
        let t = std::time::Instant::now();
        let ops = diff_seq(&a, &b);
        assert!(t.elapsed().as_secs_f32() < 2.0);
        assert_eq!(ops.iter().filter(|o| !matches!(o, Op::Equal(..))).count(), 2);
    }

    #[test]
    fn words_become_differences() {
        let old = words(0, "Der Preis beträgt 100 Euro pro Monat");
        let new = words(0, "Der Preis beträgt 120 Euro pro Monat inklusive Steuer");
        let d = word_differences(&old, &new);
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].kind, DiffKind::Changed);
        assert_eq!((d[0].old_text.as_str(), d[0].new_text.as_str()), ("100", "120"));
        assert_eq!(d[1].kind, DiffKind::Inserted);
        assert_eq!(d[1].new_text, "inklusive Steuer");
        // The insertion is one mark over both words, a caret in the old text.
        assert_eq!(d[1].new.len(), 1);
        assert_eq!(d[1].new[0].rect, RectF::new(70.0, 0.0, 88.0, 10.0));
        assert!(d[1].old[0].caret);
    }

    #[test]
    fn differences_split_at_page_breaks() {
        let old = words(0, "Preis 100 Euro Laufzeit zwölf Monate");
        let mut new = words(0, "Preis 100 Euro");
        new.extend(words(1, "Eine neue Seite"));
        let d = word_differences(&old, &new);
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].kind, DiffKind::Changed);
        assert_eq!(d[0].old_text, "Laufzeit zwölf Monate");
        assert_eq!(d[0].new_text, "Eine neue Seite");
        let mut new2 = words(0, "Preis 100 Euro Rabatt");
        new2.extend(words(1, "Eine neue Seite"));
        let d = word_differences(&old, &new2);
        assert_eq!(d.len(), 2, "{d:?}");
        assert_eq!((d[0].old_text.as_str(), d[0].new_text.as_str()), ("Laufzeit zwölf Monate", "Rabatt"));
        assert_eq!((d[1].kind, d[1].new_text.as_str()), (DiffKind::Inserted, "Eine neue Seite"));
        assert!(d[1].old.iter().all(|m| m.caret));
    }

    #[test]
    fn pages_are_paired_around_inserted_page() {
        let mut old = words(0, "alpha beta gamma");
        old.extend(words(1, "delta epsilon zeta"));
        let mut new = words(0, "alpha beta gamma");
        new.extend(words(1, "ganz neue Seite"));
        new.extend(words(2, "delta epsilon zeta"));
        let pairs = pair_pages(&old, &new, &[0, 1], &[0, 1, 2]);
        assert_eq!(pairs, vec![(Some(0), Some(0)), (None, Some(1)), (Some(1), Some(2))]);
        // Only some pages chosen: page numbers stay the document's.
        let pairs = pair_pages(&old[3..], &new[6..], &[1], &[2]);
        assert_eq!(pairs, vec![(Some(1), Some(2))]);
    }

    #[test]
    fn graphic_regions_ignore_text_and_specks() {
        let (w, h) = (60, 60);
        let white = vec![255u8; w * h * 4];
        let mut b = white.clone();
        // A dark block at (30..42, 30..42), a speck at (2, 2), a change inside text at (5..15, 40..45).
        for y in 0..h {
            for x in 0..w {
                let block = (30..42).contains(&x) && (30..42).contains(&y);
                let speck = x == 2 && y == 2;
                let text = (5..15).contains(&x) && (40..45).contains(&y);
                if block || speck || text {
                    let k = (y * w + x) * 4;
                    b[k..k + 3].copy_from_slice(&[0, 0, 0]);
                }
            }
        }
        let r = graphic_regions(&white, &b, w, h, &[[5.0 / 60.0, 40.0 / 60.0, 15.0 / 60.0, 45.0 / 60.0]]);
        assert_eq!(r.len(), 1, "{r:?}");
        assert!((r[0][0] - 0.5).abs() < 0.01 && (r[0][2] - 0.7).abs() < 0.01, "{r:?}");
    }

    fn lines(v: &[&str]) -> Vec<(usize, String)> {
        v.iter().map(|s| (0, s.to_string())).collect()
    }

    #[test]
    fn finds_insertions_and_deletions() {
        let old = lines(&["a", "b", "c", "d"]);
        let new = lines(&["a", "c", "x", "d"]);
        let d = diff(&old, &new);
        assert_eq!(
            d,
            vec![
                Change::Same(0, 0, "a".into()),
                Change::Removed(0, "b".into()),
                Change::Same(0, 0, "c".into()),
                Change::Added(0, "x".into()),
                Change::Same(0, 0, "d".into()),
            ]
        );
    }

    #[test]
    fn identical_and_empty() {
        let a = lines(&["x", "y"]);
        assert!(diff(&a, &a).iter().all(|c| matches!(c, Change::Same(..))));
        assert_eq!(diff(&[], &a).len(), 2);
        assert_eq!(diff(&a, &[]).len(), 2);
    }
}
