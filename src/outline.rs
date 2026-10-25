//! Editing the document outline (bookmarks).
//!
//! Bookmarks are addressed by their path of child indices from the root,
//! e.g. `[2, 0]` is the first child of the third top-level bookmark. The
//! order matches [`crate::Document::bookmarks`].

use lopdf::{Dictionary, Object, ObjectId};

use crate::error::{Error, Result};
use crate::pdf_edit::{Edit, guarded, text_string};

const MAX_ITEMS: usize = 20_000;
const MAX_DEPTH: usize = 32;

fn reference(dict: &Dictionary, key: &[u8]) -> Option<ObjectId> {
    dict.get(key).ok().and_then(|o| o.as_reference().ok())
}

/// The `/Outlines` root dictionary, created if missing.
fn outline_root(edit: &mut Edit, create: bool) -> Result<Option<ObjectId>> {
    let catalog_id = edit.prev().trailer.get(b"Root")?.as_reference()?;
    let existing = edit.object(catalog_id)?.as_dict()?.get(b"Outlines").ok().cloned();
    match existing {
        Some(Object::Reference(id)) => Ok(Some(id)),
        Some(Object::Dictionary(dict)) => {
            let id = edit.add(Object::Dictionary(dict));
            edit.dict_mut(catalog_id)?.set("Outlines", Object::Reference(id));
            Ok(Some(id))
        }
        _ if create => {
            let mut dict = Dictionary::new();
            dict.set("Type", Object::Name(b"Outlines".to_vec()));
            dict.set("Count", Object::Integer(0));
            let id = edit.add(Object::Dictionary(dict));
            edit.dict_mut(catalog_id)?.set("Outlines", Object::Reference(id));
            Ok(Some(id))
        }
        _ => Ok(None),
    }
}

fn children(edit: &Edit, parent: ObjectId) -> Result<Vec<ObjectId>> {
    let mut out = Vec::new();
    let mut next = reference(edit.object(parent)?.as_dict()?, b"First");
    while let Some(id) = next {
        if out.len() >= MAX_ITEMS || out.contains(&id) {
            break;
        }
        out.push(id);
        next = reference(edit.object(id)?.as_dict()?, b"Next");
    }
    Ok(out)
}

fn resolve_path(edit: &Edit, root: ObjectId, path: &[usize]) -> Result<ObjectId> {
    if path.len() > MAX_DEPTH {
        return Err(Error::Invalid("bookmark path too deep".into()));
    }
    let mut node = root;
    for &i in path {
        node = *children(edit, node)?.get(i).ok_or_else(|| Error::Invalid("bookmark not found".into()))?;
    }
    Ok(node)
}

/// Recomputes `/Count` of `node` and its descendants. Returns the number of
/// items visible below `node` when it is open.
fn recount(edit: &mut Edit, node: ObjectId, depth: usize) -> Result<i64> {
    if depth > MAX_DEPTH {
        return Ok(0);
    }
    let kids = children(edit, node)?;
    let mut visible = 0;
    for kid in &kids {
        let below = recount(edit, *kid, depth + 1)?;
        let open = edit
            .object(*kid)?
            .as_dict()?
            .get(b"Count")
            .ok()
            .and_then(|c| c.as_i64().ok())
            .is_none_or(|c| c >= 0);
        visible += 1 + if open { below } else { 0 };
    }
    let dict = edit.object(node)?.as_dict()?;
    let is_root = dict.get(b"Type").ok().and_then(|t| t.as_name().ok()) == Some(b"Outlines".as_slice());
    let closed = dict.get(b"Count").ok().and_then(|c| c.as_i64().ok()).is_some_and(|c| c < 0);
    let count = if kids.is_empty() {
        None
    } else if closed && !is_root {
        Some(-(kids.len() as i64))
    } else {
        Some(visible)
    };
    let current = dict.get(b"Count").ok().and_then(|c| c.as_i64().ok());
    if current != count {
        let dict = edit.dict_mut(node)?;
        match count {
            Some(c) => dict.set("Count", Object::Integer(c)),
            None => {
                dict.remove(b"Count");
            }
        }
    }
    Ok(visible)
}

/// Adds a bookmark pointing to `page` as last child of `parent` (root if empty).
pub fn add_bookmark(
    bytes: &[u8],
    password: Option<&str>,
    parent: &[usize],
    title: &str,
    page: usize,
) -> Result<Vec<u8>> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let page_id = edit.page_id(page)?;
        let root = outline_root(&mut edit, true)?.expect("outline root was created");
        let parent_id = resolve_path(&edit, root, parent)?;
        let last = children(&edit, parent_id)?.last().copied();

        let mut item = Dictionary::new();
        item.set("Title", text_string(title));
        item.set("Parent", Object::Reference(parent_id));
        item.set("Dest", Object::Array(vec![Object::Reference(page_id), Object::Name(b"Fit".to_vec())]));
        if let Some(prev) = last {
            item.set("Prev", Object::Reference(prev));
        }
        let id = edit.add(Object::Dictionary(item));
        match last {
            Some(prev) => edit.dict_mut(prev)?.set("Next", Object::Reference(id)),
            None => edit.dict_mut(parent_id)?.set("First", Object::Reference(id)),
        }
        edit.dict_mut(parent_id)?.set("Last", Object::Reference(id));
        recount(&mut edit, root, 0)?;
        edit.finish()
    })
}

pub fn rename_bookmark(bytes: &[u8], password: Option<&str>, path: &[usize], title: &str) -> Result<Vec<u8>> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let root = outline_root(&mut edit, false)?.ok_or_else(|| Error::Invalid("no bookmarks".into()))?;
        if path.is_empty() {
            return Err(Error::Invalid("bookmark not found".into()));
        }
        let id = resolve_path(&edit, root, path)?;
        edit.dict_mut(id)?.set("Title", text_string(title));
        edit.finish()
    })
}

/// Removes a bookmark together with its children.
pub fn delete_bookmark(bytes: &[u8], password: Option<&str>, path: &[usize]) -> Result<Vec<u8>> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let root = outline_root(&mut edit, false)?.ok_or_else(|| Error::Invalid("no bookmarks".into()))?;
        let (last, parent_path) =
            path.split_last().ok_or_else(|| Error::Invalid("bookmark not found".into()))?;
        let parent_id = resolve_path(&edit, root, parent_path)?;
        let siblings = children(&edit, parent_id)?;
        siblings.get(*last).ok_or_else(|| Error::Invalid("bookmark not found".into()))?;
        let prev = last.checked_sub(1).and_then(|i| siblings.get(i)).copied();
        let next = siblings.get(last + 1).copied();
        match prev {
            Some(p) => match next {
                Some(n) => edit.dict_mut(p)?.set("Next", Object::Reference(n)),
                None => {
                    edit.dict_mut(p)?.remove(b"Next");
                }
            },
            None => match next {
                Some(n) => edit.dict_mut(parent_id)?.set("First", Object::Reference(n)),
                None => {
                    edit.dict_mut(parent_id)?.remove(b"First");
                }
            },
        }
        match next {
            Some(n) => match prev {
                Some(p) => edit.dict_mut(n)?.set("Prev", Object::Reference(p)),
                None => {
                    edit.dict_mut(n)?.remove(b"Prev");
                }
            },
            None => match prev {
                Some(p) => edit.dict_mut(parent_id)?.set("Last", Object::Reference(p)),
                None => {
                    edit.dict_mut(parent_id)?.remove(b"Last");
                }
            },
        }
        recount(&mut edit, root, 0)?;
        edit.finish()
    })
}

// ---------------------------------------------------------------------------
// Whole-outline operations. Existing items are kept as objects (only their
// links are rewritten), so destinations and actions stay intact.

/// A bookmark to create: `level` 0 is top level, deeper levels nest below
/// the previous item of the next lower level.
#[derive(Debug, Clone, PartialEq)]
pub struct NewBookmark {
    pub title: String,
    pub page: usize,
    pub level: usize,
}

/// Sets First/Last/Prev/Next/Parent so `kids` become the children of `parent`.
fn relink(edit: &mut Edit, parent: ObjectId, kids: &[ObjectId]) -> Result<()> {
    for (i, kid) in kids.iter().enumerate() {
        let d = edit.dict_mut(*kid)?;
        d.set("Parent", Object::Reference(parent));
        match i.checked_sub(1).and_then(|p| kids.get(p)) {
            Some(prev) => d.set("Prev", Object::Reference(*prev)),
            None => {
                d.remove(b"Prev");
            }
        }
        match kids.get(i + 1) {
            Some(next) => d.set("Next", Object::Reference(*next)),
            None => {
                d.remove(b"Next");
            }
        }
    }
    let p = edit.dict_mut(parent)?;
    match (kids.first(), kids.last()) {
        (Some(first), Some(last)) => {
            p.set("First", Object::Reference(*first));
            p.set("Last", Object::Reference(*last));
        }
        _ => {
            p.remove(b"First");
            p.remove(b"Last");
        }
    }
    Ok(())
}

/// All items below `node` (depth first).
fn descendants(edit: &Edit, node: ObjectId, depth: usize, out: &mut Vec<ObjectId>) -> Result<()> {
    if depth > MAX_DEPTH {
        return Ok(());
    }
    for kid in children(edit, node)? {
        if out.len() >= MAX_ITEMS || out.contains(&kid) {
            break;
        }
        out.push(kid);
        descendants(edit, kid, depth + 1, out)?;
    }
    Ok(())
}

fn title_of(edit: &Edit, id: ObjectId) -> String {
    edit.object(id)
        .ok()
        .and_then(|o| o.as_dict().ok())
        .and_then(|d| crate::pdf_edit::string(d, b"Title"))
        .unwrap_or_default()
}

/// Page index of an item's explicit destination (`/Dest` or GoTo action).
fn page_of(edit: &Edit, id: ObjectId, pages: &[ObjectId]) -> Option<usize> {
    let dict = edit.object(id).ok()?.as_dict().ok()?;
    let dest = match dict.get(b"Dest") {
        Ok(d) => edit.resolve(d).ok()?.clone(),
        Err(_) => {
            let action = edit.resolve(dict.get(b"A").ok()?).ok()?.as_dict().ok()?;
            edit.resolve(action.get(b"D").ok()?).ok()?.clone()
        }
    };
    let target = dest.as_array().ok()?.first()?.as_reference().ok()?;
    pages.iter().position(|p| *p == target)
}

/// Rewrites every bookmark title with `f`. Returns the bytes and the number
/// of changed titles.
pub fn map_titles(
    bytes: &[u8],
    password: Option<&str>,
    f: impl Fn(&str) -> String,
) -> Result<(Vec<u8>, usize)> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let Some(root) = outline_root(&mut edit, false)? else { return Ok((bytes.to_vec(), 0)) };
        let mut items = Vec::new();
        descendants(&edit, root, 0, &mut items)?;
        let mut changed = 0;
        for id in items {
            let old = title_of(&edit, id);
            let new = f(&old);
            if new != old {
                edit.dict_mut(id)?.set("Title", text_string(&new));
                changed += 1;
            }
        }
        if changed == 0 {
            return Ok((bytes.to_vec(), 0));
        }
        Ok((edit.finish()?, changed))
    })
}

/// Sorts the bookmarks on every level by page (then title) or by title.
pub fn sort(bytes: &[u8], password: Option<&str>, by_page: bool) -> Result<Vec<u8>> {
    fn sort_level(
        edit: &mut Edit,
        node: ObjectId,
        by_page: bool,
        pages: &[ObjectId],
        depth: usize,
    ) -> Result<()> {
        if depth > MAX_DEPTH {
            return Ok(());
        }
        let mut kids: Vec<(ObjectId, String, usize)> = children(edit, node)?
            .into_iter()
            .map(|k| (k, title_of(edit, k).to_lowercase(), page_of(edit, k, pages).unwrap_or(usize::MAX)))
            .collect();
        if by_page {
            kids.sort_by(|a, b| a.2.cmp(&b.2).then_with(|| natural_cmp(&a.1, &b.1)));
        } else {
            kids.sort_by(|a, b| natural_cmp(&a.1, &b.1));
        }
        let ids: Vec<ObjectId> = kids.iter().map(|k| k.0).collect();
        relink(edit, node, &ids)?;
        for id in ids {
            sort_level(edit, id, by_page, pages, depth + 1)?;
        }
        Ok(())
    }
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let Some(root) = outline_root(&mut edit, false)? else { return Ok(bytes.to_vec()) };
        let pages = edit.page_ids();
        sort_level(&mut edit, root, by_page, &pages, 0)?;
        edit.finish()
    })
}

/// Compares strings so that "Kapitel 2" comes before "Kapitel 10".
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let mut x = a.chars().peekable();
    let mut y = b.chars().peekable();
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let mut n = String::new();
                while let Some(c) = x.peek().copied().filter(char::is_ascii_digit) {
                    n.push(c);
                    x.next();
                }
                let mut m = String::new();
                while let Some(d) = y.peek().copied().filter(char::is_ascii_digit) {
                    m.push(d);
                    y.next();
                }
                let (n, m) = (n.trim_start_matches('0'), m.trim_start_matches('0'));
                let ord = n.len().cmp(&m.len()).then_with(|| n.cmp(m));
                if ord != std::cmp::Ordering::Equal {
                    return ord;
                }
            }
            (Some(c), Some(d)) => {
                if c != d {
                    return c.cmp(&d);
                }
                x.next();
                y.next();
            }
        }
    }
}

/// Removes items whose title and target page repeat an earlier sibling.
/// Returns the bytes and the number of removed items.
pub fn remove_duplicates(bytes: &[u8], password: Option<&str>) -> Result<(Vec<u8>, usize)> {
    fn level(edit: &mut Edit, node: ObjectId, pages: &[ObjectId], depth: usize) -> Result<usize> {
        if depth > MAX_DEPTH {
            return Ok(0);
        }
        let mut seen = Vec::new();
        let mut kept = Vec::new();
        let mut removed = 0;
        for kid in children(edit, node)? {
            let key = (title_of(edit, kid), page_of(edit, kid, pages));
            if seen.contains(&key) {
                removed += 1;
            } else {
                seen.push(key);
                kept.push(kid);
            }
        }
        if removed > 0 {
            relink(edit, node, &kept)?;
        }
        for kid in kept {
            removed += level(edit, kid, pages, depth + 1)?;
        }
        Ok(removed)
    }
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let Some(root) = outline_root(&mut edit, false)? else { return Ok((bytes.to_vec(), 0)) };
        let pages = edit.page_ids();
        let removed = level(&mut edit, root, &pages, 0)?;
        if removed == 0 {
            return Ok((bytes.to_vec(), 0));
        }
        recount(&mut edit, root, 0)?;
        Ok((edit.finish()?, removed))
    })
}

/// Removes the whole outline.
pub fn delete_all(bytes: &[u8], password: Option<&str>) -> Result<Vec<u8>> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let catalog_id = edit.prev().trailer.get(b"Root")?.as_reference()?;
        edit.dict_mut(catalog_id)?.remove(b"Outlines");
        edit.finish()
    })
}

/// Appends new bookmarks at the end of the outline (see [`NewBookmark`]).
pub fn append(bytes: &[u8], password: Option<&str>, items: &[NewBookmark]) -> Result<Vec<u8>> {
    guarded(|| {
        if items.len() > MAX_ITEMS {
            return Err(Error::Invalid("too many bookmarks".into()));
        }
        let mut edit = Edit::open(bytes, password)?;
        let pages = edit.page_ids();
        let root = outline_root(&mut edit, true)?.expect("outline root was created");
        // Stack of (level, item id) of the current chain of parents.
        let mut stack: Vec<(usize, ObjectId)> = Vec::new();
        let mut new_children: Vec<(ObjectId, Vec<ObjectId>)> = vec![(root, children(&edit, root)?)];
        for item in items {
            let level = item.level.min(MAX_DEPTH - 1);
            while stack.last().is_some_and(|(l, _)| *l >= level) {
                stack.pop();
            }
            let parent = stack.last().map_or(root, |(_, id)| *id);
            let mut dict = Dictionary::new();
            dict.set("Title", text_string(&item.title));
            if let Some(page_id) = pages.get(item.page) {
                dict.set(
                    "Dest",
                    Object::Array(vec![Object::Reference(*page_id), Object::Name(b"Fit".to_vec())]),
                );
            }
            let id = edit.add(Object::Dictionary(dict));
            match new_children.iter_mut().find(|(p, _)| *p == parent) {
                Some((_, kids)) => kids.push(id),
                None => new_children.push((parent, vec![id])),
            }
            stack.push((level, id));
        }
        for (parent, kids) in &new_children {
            relink(&mut edit, *parent, kids)?;
        }
        recount(&mut edit, root, 0)?;
        edit.finish()
    })
}

/// Parses a bookmark list: one bookmark per line as `title<TAB>page` or
/// `title;page`; leading tabs (or pairs of spaces) give the nesting level.
pub fn parse_bookmark_list(text: &str, page_count: usize) -> Vec<NewBookmark> {
    let mut out = Vec::new();
    for line in text.lines().take(MAX_ITEMS) {
        if line.trim().is_empty() {
            continue;
        }
        let mut level = 0;
        let mut rest = line;
        loop {
            if let Some(r) = rest.strip_prefix('\t') {
                level += 1;
                rest = r;
            } else if let Some(r) = rest.strip_prefix("  ") {
                level += 1;
                rest = r;
            } else {
                break;
            }
        }
        let rest = rest.trim();
        let (title, page) = match rest.rsplit_once(['\t', ';']) {
            Some((t, p)) if p.trim().parse::<usize>().is_ok() => {
                (t.trim(), p.trim().parse::<usize>().unwrap_or(1))
            }
            _ => (rest, 1),
        };
        let page = page.clamp(1, page_count.max(1)) - 1;
        out.push(NewBookmark { title: title.to_string(), page, level });
    }
    out
}

/// Title in "title case": first letter of every word upper case.
pub fn title_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut start = true;
    for c in s.chars() {
        if start {
            out.extend(c.to_uppercase());
        } else {
            out.extend(c.to_lowercase());
        }
        start = c.is_whitespace() || c == '-' || c == '(';
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order() {
        let mut v = vec!["Kapitel 10", "kapitel 2", "Anhang", "Kapitel 1"];
        v.sort_by(|a, b| natural_cmp(&a.to_lowercase(), &b.to_lowercase()));
        assert_eq!(v, vec!["Anhang", "Kapitel 1", "kapitel 2", "Kapitel 10"]);
    }

    #[test]
    fn bookmark_list_is_parsed() {
        let list = "Einleitung\t1\n\tTeil A;2\n  Teil B\t99\n\nAnhang";
        let items = parse_bookmark_list(list, 5);
        assert_eq!(items.len(), 4);
        assert_eq!(items[1], NewBookmark { title: "Teil A".into(), page: 1, level: 1 });
        assert_eq!(items[2].page, 4, "clamped to the last page");
        assert_eq!(items[3], NewBookmark { title: "Anhang".into(), page: 0, level: 0 });
    }

    #[test]
    fn title_case_words() {
        assert_eq!(title_case("hallo WELT (teil-eins)"), "Hallo Welt (Teil-Eins)");
    }
}
