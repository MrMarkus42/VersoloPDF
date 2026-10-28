//! Full text search over extracted page text.

use crate::model::{SearchHit, SearchOptions};
use crate::text::{PageText, TextRange};

fn fold(c: char, match_case: bool) -> char {
    if match_case { c } else { c.to_lowercase().next().unwrap_or(c) }
}

/// Finds all occurrences of `query` on one page.
pub fn search_page(page: usize, text: &PageText, query: &str, opts: SearchOptions) -> Vec<SearchHit> {
    let needle: Vec<char> =
        query.chars().map(|c| if c.is_whitespace() { ' ' } else { fold(c, opts.match_case) }).collect();
    if needle.iter().all(|c| *c == ' ') {
        return Vec::new();
    }

    // Normalised haystack plus a map back to character indices. Line breaks
    // count as a single space so phrases can span lines.
    let mut hay: Vec<char> = Vec::with_capacity(text.chars.len());
    let mut map: Vec<usize> = Vec::with_capacity(text.chars.len());
    for (i, c) in text.chars.iter().enumerate() {
        let ch = match c.ch {
            '\r' => continue,
            ch if ch.is_whitespace() => ' ',
            ch => fold(ch, opts.match_case),
        };
        if ch == ' ' && hay.last() == Some(&' ') {
            continue;
        }
        hay.push(ch);
        map.push(i);
    }

    let is_word = |c: Option<&char>| c.is_some_and(|c| c.is_alphanumeric() || *c == '_');
    let mut hits = Vec::new();
    let n = needle.len();
    let mut start = 0;
    while start + n <= hay.len() {
        if hay[start..start + n] == needle[..] {
            let boundary_ok = !opts.whole_word
                || (!is_word(start.checked_sub(1).and_then(|i| hay.get(i))) && !is_word(hay.get(start + n)));
            if boundary_ok {
                let range = TextRange::new(map[start], map[start + n - 1]);
                let ctx_start = start.saturating_sub(30);
                let ctx_end = (start + n + 30).min(hay.len());
                let mut context: String =
                    text.text_in(TextRange::new(map[ctx_start], map[ctx_end - 1])).replace(['\n', '\r'], " ");
                if ctx_start > 0 {
                    context.insert(0, '…');
                }
                if ctx_end < hay.len() {
                    context.push('…');
                }
                hits.push(SearchHit { page, rects: text.line_rects(range), context });
                start += n;
                continue;
            }
        }
        start += 1;
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::RectF;
    use crate::text::CharInfo;

    fn page(s: &str) -> PageText {
        PageText {
            chars: s
                .chars()
                .enumerate()
                .map(|(i, ch)| CharInfo {
                    ch,
                    rect: if ch == '\n' {
                        RectF::default()
                    } else {
                        RectF::new(i as f32 * 5.0, 0.0, i as f32 * 5.0 + 4.0, 10.0)
                    },
                    generated: false,
                })
                .collect(),
            ..PageText::default()
        }
    }

    const CASE: SearchOptions = SearchOptions { match_case: true, whole_word: false };
    const NOCASE: SearchOptions = SearchOptions { match_case: false, whole_word: false };

    #[test]
    fn finds_case_insensitive() {
        let p = page("Ein Test und noch ein TEST.");
        assert_eq!(search_page(0, &p, "test", NOCASE).len(), 2);
        assert_eq!(search_page(0, &p, "test", CASE).len(), 0);
        assert_eq!(search_page(0, &p, "Test", CASE).len(), 1);
    }

    #[test]
    fn whole_word() {
        let p = page("Testfall Test");
        let opts = SearchOptions { match_case: false, whole_word: true };
        assert_eq!(search_page(0, &p, "test", opts).len(), 1);
    }

    #[test]
    fn phrase_across_line_break() {
        let p = page("Hallo\nWelt");
        let hits = search_page(0, &p, "hallo welt", NOCASE);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].rects.len(), 1);
    }

    #[test]
    fn empty_query_finds_nothing() {
        assert!(search_page(0, &page("abc"), "  ", NOCASE).is_empty());
    }

    #[test]
    fn umlauts_fold() {
        let p = page("ÄRGER ärger");
        assert_eq!(search_page(0, &p, "ärger", NOCASE).len(), 2);
    }
}
