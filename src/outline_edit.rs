//! Destinations of bookmarks: named destinations ("Sprungmarken") and
//! explicit ones, the zoom of a destination, actions, and checking for
//! broken bookmarks (ribbon tab "Lesezeichen").

use lopdf::{Dictionary, Object, ObjectId};

use crate::error::Result;
use crate::outline::{MAX_DEPTH, children, descendants, outline_root, recount, relink, title_of};
use crate::pdf_edit::{Edit, guarded};

/// Where a bookmark keeps its target.
#[derive(Debug, Clone, PartialEq)]
enum Slot {
    /// `/Dest` with its value.
    Dest(Object),
    /// GoTo action with its `/D` (None if missing).
    GoTo(Option<Object>),
    /// Another action (URI, JavaScript, …): its type.
    Other(String),
    Nothing,
}

fn slot(edit: &Edit, id: ObjectId) -> Result<Slot> {
    let dict = edit.object(id)?.as_dict()?;
    if let Ok(d) = dict.get(b"Dest") {
        return Ok(Slot::Dest(edit.resolve(d)?.clone()));
    }
    let Ok(a) = dict.get(b"A") else { return Ok(Slot::Nothing) };
    let action = edit.resolve(a)?.as_dict()?;
    let kind = action.get(b"S").ok().and_then(|s| s.as_name().ok()).unwrap_or(b"");
    if kind == b"GoTo" {
        let d = match action.get(b"D") {
            Ok(d) => Some(edit.resolve(d)?.clone()),
            Err(_) => None,
        };
        return Ok(Slot::GoTo(d));
    }
    Ok(Slot::Other(String::from_utf8_lossy(kind).into_owned()))
}

/// Writes `dest` where the bookmark keeps its target (`/Dest` or the `/D`
/// of its GoTo action).
fn set_target(edit: &mut Edit, id: ObjectId, dest: Object) -> Result<()> {
    let action = edit.object(id)?.as_dict()?.get(b"A").ok().cloned();
    match action {
        Some(Object::Reference(aid)) => {
            edit.dict_mut(aid)?.set("D", dest);
        }
        Some(Object::Dictionary(mut a)) => {
            a.set("D", dest);
            edit.dict_mut(id)?.set("A", Object::Dictionary(a));
        }
        _ => {
            edit.dict_mut(id)?.set("Dest", dest);
        }
    }
    Ok(())
}

fn catalog_id(edit: &Edit) -> Result<ObjectId> {
    Ok(edit.prev().trailer.get(b"Root")?.as_reference()?)
}

/// Looks `name` up in a name tree.
fn tree_lookup(edit: &Edit, node: &Dictionary, name: &[u8], depth: usize) -> Option<Object> {
    if depth > MAX_DEPTH {
        return None;
    }
    if let Some(names) =
        node.get(b"Names").ok().and_then(|n| edit.resolve(n).ok()).and_then(|n| n.as_array().ok())
    {
        for pair in names.chunks(2) {
            if let [Object::String(key, _), value] = pair
                && key.as_slice() == name
            {
                return Some(value.clone());
            }
        }
    }
    let kids = edit.resolve(node.get(b"Kids").ok()?).ok()?.as_array().ok()?;
    kids.iter().find_map(|kid| {
        let kid = edit.resolve(kid).ok()?.as_dict().ok()?;
        tree_lookup(edit, kid, name, depth + 1)
    })
}

/// The explicit destination (array) of a named destination.
fn lookup_named(edit: &Edit, name: &[u8]) -> Option<Vec<Object>> {
    let catalog = edit.object(catalog_id(edit).ok()?).ok()?.as_dict().ok()?;
    let found = catalog
        .get(b"Dests")
        .ok()
        .and_then(|d| edit.resolve(d).ok()?.as_dict().ok()?.get(name).ok().cloned())
        .or_else(|| {
            let names = edit.resolve(catalog.get(b"Names").ok()?).ok()?.as_dict().ok()?;
            let tree = edit.resolve(names.get(b"Dests").ok()?).ok()?.as_dict().ok()?;
            tree_lookup(edit, tree, name, 0)
        })?;
    match edit.resolve(&found).ok()? {
        Object::Array(a) => Some(a.clone()),
        Object::Dictionary(d) => edit.resolve(d.get(b"D").ok()?).ok()?.as_array().ok().cloned(),
        _ => None,
    }
}

fn explicit(edit: &Edit, dest: &Object) -> Option<Vec<Object>> {
    match dest {
        Object::Array(a) => Some(a.clone()),
        Object::Name(n) | Object::String(n, _) => lookup_named(edit, n),
        _ => None,
    }
}

fn is_named(dest: &Object) -> bool {
    matches!(dest, Object::Name(_) | Object::String(..))
}

fn dest_of(slot: &Slot) -> Option<&Object> {
    match slot {
        Slot::Dest(d) | Slot::GoTo(Some(d)) => Some(d),
        _ => None,
    }
}

/// Page index a bookmark leads to (explicit or named destination).
pub(crate) fn target_page(edit: &Edit, id: ObjectId, pages: &[ObjectId]) -> Option<usize> {
    let slot = slot(edit, id).ok()?;
    let target = explicit(edit, dest_of(&slot)?)?.first()?.as_reference().ok()?;
    pages.iter().position(|p| *p == target)
}

fn all_items(edit: &mut Edit) -> Result<Option<(ObjectId, Vec<ObjectId>)>> {
    let Some(root) = outline_root(edit, false)? else { return Ok(None) };
    let mut items = Vec::new();
    descendants(edit, root, 0, &mut items)?;
    Ok(Some((root, items)))
}

/// Runs `f` on every bookmark; `f` returns whether it changed the item.
fn for_each(
    bytes: &[u8],
    password: Option<&str>,
    mut f: impl FnMut(&mut Edit, ObjectId) -> Result<bool>,
) -> Result<(Vec<u8>, usize)> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let Some((_, items)) = all_items(&mut edit)? else { return Ok((bytes.to_vec(), 0)) };
        let mut changed = 0;
        for id in items {
            if f(&mut edit, id)? {
                changed += 1;
            }
        }
        if changed == 0 {
            return Ok((bytes.to_vec(), 0));
        }
        Ok((edit.finish()?, changed))
    })
}

/// "Sprungmarken zu normalen Zielen": named destinations become explicit.
pub fn resolve_named(bytes: &[u8], password: Option<&str>) -> Result<(Vec<u8>, usize)> {
    for_each(bytes, password, |edit, id| {
        let slot = slot(edit, id)?;
        match dest_of(&slot) {
            Some(d) if is_named(d) => match explicit(edit, d) {
                Some(arr) => {
                    set_target(edit, id, Object::Array(arr))?;
                    Ok(true)
                }
                None => Ok(false),
            },
            _ => Ok(false),
        }
    })
}

/// A destination name made of the title: letters, digits and "_".
fn name_for(title: &str, used: &mut Vec<Vec<u8>>) -> Vec<u8> {
    let mut base: String = title
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .split('_')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    base.truncate(40);
    if base.is_empty() {
        base = "Lesezeichen".into();
    }
    let mut name = base.clone().into_bytes();
    let mut n = 2;
    while used.contains(&name) {
        name = format!("{base}_{n}").into_bytes();
        n += 1;
    }
    used.push(name.clone());
    name
}

/// "In Sprungmarken umwandeln": explicit destinations become named ones
/// (stored in the catalog's `/Dests` dictionary).
pub fn to_named(bytes: &[u8], password: Option<&str>) -> Result<(Vec<u8>, usize)> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let Some((_, items)) = all_items(&mut edit)? else { return Ok((bytes.to_vec(), 0)) };
        let mut todo = Vec::new();
        for id in items {
            if let Some(Object::Array(arr)) = dest_of(&slot(&edit, id)?) {
                todo.push((id, arr.clone()));
            }
        }
        if todo.is_empty() {
            return Ok((bytes.to_vec(), 0));
        }
        let cat = catalog_id(&edit)?;
        let existing = edit.object(cat)?.as_dict()?.get(b"Dests").ok().cloned();
        let dests = match existing {
            Some(Object::Reference(id)) => id,
            other => {
                let dict = match other {
                    Some(Object::Dictionary(d)) => d,
                    _ => Dictionary::new(),
                };
                let id = edit.add(Object::Dictionary(dict));
                edit.dict_mut(cat)?.set("Dests", Object::Reference(id));
                id
            }
        };
        let mut used: Vec<Vec<u8>> = edit.object(dests)?.as_dict()?.iter().map(|(k, _)| k.clone()).collect();
        for (id, arr) in &todo {
            let name = name_for(&title_of(&edit, *id), &mut used);
            edit.dict_mut(dests)?.set(name.clone(), Object::Array(arr.clone()));
            set_target(&mut edit, *id, Object::Name(name))?;
        }
        Ok((edit.finish()?, todo.len()))
    })
}

/// View of a destination ("Vergrößerung ändern").
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Zoom {
    /// "Zoom übernehmen": keep the reader's zoom.
    Inherit,
    /// Fixed zoom in percent.
    Percent(f32),
    /// Whole page.
    Fit,
    /// Page width.
    FitWidth,
    /// Page height.
    FitHeight,
    /// Visible content.
    FitVisible,
}

fn number(o: Option<&Object>) -> Object {
    match o {
        Some(o @ (Object::Integer(_) | Object::Real(_))) => o.clone(),
        _ => Object::Null,
    }
}

/// Rewrites destination `arr` (page first) with `zoom`, keeping the
/// position on the page where the old one had it.
fn with_zoom(arr: &[Object], zoom: Zoom) -> Option<Vec<Object>> {
    let page = arr.first()?.clone();
    let kind = arr.get(1).and_then(|k| k.as_name().ok()).unwrap_or(b"");
    let (left, top) = match kind {
        b"XYZ" => (number(arr.get(2)), number(arr.get(3))),
        b"FitH" | b"FitBH" => (Object::Null, number(arr.get(2))),
        b"FitV" | b"FitBV" => (number(arr.get(2)), Object::Null),
        _ => (Object::Null, Object::Null),
    };
    let name = |n: &str| Object::Name(n.as_bytes().to_vec());
    Some(match zoom {
        Zoom::Inherit => vec![page, name("XYZ"), left, top, Object::Null],
        Zoom::Percent(p) => vec![page, name("XYZ"), left, top, Object::Real(p.clamp(1.0, 6400.0) / 100.0)],
        Zoom::Fit => vec![page, name("Fit")],
        Zoom::FitWidth => vec![page, name("FitH"), top],
        Zoom::FitHeight => vec![page, name("FitV"), left],
        Zoom::FitVisible => vec![page, name("FitB")],
    })
}

/// "Vergrößerung ändern" for all bookmarks. Named destinations become
/// explicit ones.
pub fn set_zoom(bytes: &[u8], password: Option<&str>, zoom: Zoom) -> Result<(Vec<u8>, usize)> {
    for_each(bytes, password, |edit, id| {
        let slot = slot(edit, id)?;
        let Some(arr) = dest_of(&slot).and_then(|d| explicit(edit, d)) else { return Ok(false) };
        let Some(new) = with_zoom(&arr, zoom) else { return Ok(false) };
        if new == arr {
            return Ok(false);
        }
        set_target(edit, id, Object::Array(new))?;
        Ok(true)
    })
}

/// "Aktionen entfernen": GoTo actions become plain destinations, other
/// actions (web links, scripts, …) are removed.
pub fn remove_actions(bytes: &[u8], password: Option<&str>) -> Result<(Vec<u8>, usize)> {
    for_each(bytes, password, |edit, id| {
        match slot(edit, id)? {
            Slot::GoTo(d) => {
                let dict = edit.dict_mut(id)?;
                dict.remove(b"A");
                if let Some(d) = d {
                    dict.set("Dest", d);
                }
            }
            Slot::Other(_) => {
                edit.dict_mut(id)?.remove(b"A");
            }
            Slot::Dest(_) | Slot::Nothing => return Ok(false),
        }
        Ok(true)
    })
}

/// What is wrong with a bookmark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    /// Neither destination nor action.
    NoTarget,
    /// Named destination that does not exist.
    UnknownName,
    /// Destination on a page that is not in the document.
    NoPage,
}

impl Problem {
    pub fn text(self, de: bool) -> &'static str {
        match (self, de) {
            (Problem::NoTarget, true) => "kein Ziel",
            (Problem::NoTarget, false) => "no target",
            (Problem::UnknownName, true) => "Sprungmarke fehlt",
            (Problem::UnknownName, false) => "named destination missing",
            (Problem::NoPage, true) => "Seite fehlt",
            (Problem::NoPage, false) => "page missing",
        }
    }
}

fn problem(edit: &Edit, id: ObjectId, pages: &[ObjectId]) -> Result<Option<Problem>> {
    let slot = slot(edit, id)?;
    let Some(dest) = dest_of(&slot) else {
        return Ok(match slot {
            Slot::Other(_) => None,
            _ => Some(Problem::NoTarget),
        });
    };
    let Some(arr) = explicit(edit, dest) else {
        return Ok(Some(if is_named(dest) { Problem::UnknownName } else { Problem::NoTarget }));
    };
    Ok(match arr.first() {
        Some(Object::Reference(p)) if pages.contains(p) => None,
        // A page number (destination into another file) is not checked.
        Some(Object::Integer(_)) => None,
        _ => Some(Problem::NoPage),
    })
}

/// "Überprüfen": the broken bookmarks as (title, problem).
pub fn check(bytes: &[u8], password: Option<&str>) -> Result<Vec<(String, Problem)>> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let Some((_, items)) = all_items(&mut edit)? else { return Ok(Vec::new()) };
        let pages = edit.page_ids();
        let mut out = Vec::new();
        for id in items {
            if let Some(p) = problem(&edit, id, &pages)? {
                out.push((title_of(&edit, id), p));
            }
        }
        Ok(out)
    })
}

/// Removes the broken bookmarks; their children move up in their place.
pub fn remove_broken(bytes: &[u8], password: Option<&str>) -> Result<(Vec<u8>, usize)> {
    fn level(
        edit: &mut Edit,
        node: ObjectId,
        pages: &[ObjectId],
        depth: usize,
    ) -> Result<(Vec<ObjectId>, usize)> {
        let mut kept = Vec::new();
        let mut removed = 0;
        if depth > MAX_DEPTH {
            return Ok((children(edit, node)?, 0));
        }
        for kid in children(edit, node)? {
            let (grand, n) = level(edit, kid, pages, depth + 1)?;
            removed += n;
            if problem(edit, kid, pages)?.is_some() {
                removed += 1;
                kept.extend(grand);
            } else {
                relink(edit, kid, &grand)?;
                kept.push(kid);
            }
        }
        Ok((kept, removed))
    }
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let Some(root) = outline_root(&mut edit, false)? else { return Ok((bytes.to_vec(), 0)) };
        let pages = edit.page_ids();
        let (kept, removed) = level(&mut edit, root, &pages, 0)?;
        if removed == 0 {
            return Ok((bytes.to_vec(), 0));
        }
        relink(&mut edit, root, &kept)?;
        recount(&mut edit, root, 0)?;
        Ok((edit.finish()?, removed))
    })
}

/// Page of every bookmark in outline order (depth first; None if unknown).
pub fn pages_in_order(bytes: &[u8], password: Option<&str>) -> Result<Vec<Option<usize>>> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let Some((_, items)) = all_items(&mut edit)? else { return Ok(Vec::new()) };
        let pages = edit.page_ids();
        Ok(items.iter().map(|id| target_page(&edit, *id, &pages)).collect())
    })
}

/// New page order for "Seiten sortieren": every bookmarked page starts a
/// section that runs to the next bookmarked page; sections follow the
/// order of the bookmarks. Pages before the first section stay in front.
pub fn page_order_by_bookmarks(page_count: usize, targets: &[Option<usize>]) -> Vec<usize> {
    let mut starts: Vec<usize> = targets.iter().flatten().copied().filter(|p| *p < page_count).collect();
    starts.sort_unstable();
    starts.dedup();
    let Some(&first) = starts.first() else { return (0..page_count).collect() };
    let section = |start: usize| {
        let end = starts.iter().copied().find(|s| *s > start).unwrap_or(page_count);
        start..end
    };
    let mut order: Vec<usize> = (0..first).collect();
    let mut done = Vec::new();
    for p in targets.iter().flatten().copied().filter(|p| *p < page_count) {
        if !done.contains(&p) {
            done.push(p);
            order.extend(section(p));
        }
    }
    order
}

/// One entry of a printed table of contents.
#[derive(Debug, Clone, PartialEq)]
pub struct TocEntry {
    pub title: String,
    /// Printed page number.
    pub number: i64,
    pub level: usize,
}

/// Reads "1.2 Titel ........ 12" (dot leaders, spaces or tabs before the
/// page number). The level comes from the numbering ("1" = 0, "1.2" = 1).
pub fn parse_toc_line(line: &str) -> Option<TocEntry> {
    let line = line.trim();
    let digits = line.len() - line.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    if digits == 0 || digits > 5 {
        return None;
    }
    let (head, number) = line.split_at(line.len() - digits);
    let number: i64 = number.parse().ok()?;
    // Something must separate title and number.
    if !head.ends_with([' ', '\t', '.', '…', '·']) {
        return None;
    }
    let title = head.trim_end_matches([' ', '\t', '.', '…', '·', '_']).trim();
    if title.chars().count() < 2 || title.chars().count() > 200 || !title.chars().any(char::is_alphabetic) {
        return None;
    }
    let numbering = title.split_whitespace().next().unwrap_or("");
    let level = if numbering.chars().all(|c| c.is_ascii_digit() || c == '.')
        && numbering.chars().any(|c| c.is_ascii_digit())
        && title.split_whitespace().count() > 1
    {
        numbering.trim_end_matches('.').matches('.').count().min(MAX_DEPTH - 1)
    } else {
        0
    };
    Some(TocEntry { title: title.to_string(), number: number.max(1), level })
}

/// Entries of the text lines of a contents page. A page number set apart
/// (right-aligned, read as a line of its own) belongs to the line before.
pub fn parse_toc_lines(lines: &[String]) -> Vec<TocEntry> {
    let number_only = |s: &str| {
        let s = s.trim();
        (1..=5).contains(&s.len()) && s.chars().all(|c| c.is_ascii_digit())
    };
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();
        let joined = match lines.get(i + 1) {
            Some(next) if number_only(next) && parse_toc_line(line).is_none() => {
                i += 1;
                format!("{line} {}", next.trim())
            }
            _ => line.to_string(),
        };
        out.extend(parse_toc_line(&joined));
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toc_numbers_on_their_own_line() {
        let lines: Vec<String> = ["Inhalt", "Kapitel A", "3", "1.1 Teil", "12", "Anhang ..... 20"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let e = parse_toc_lines(&lines);
        let got: Vec<_> = e.iter().map(|e| (e.title.as_str(), e.number, e.level)).collect();
        assert_eq!(got, [("Kapitel A", 3, 0), ("1.1 Teil", 12, 1), ("Anhang", 20, 0)]);
    }

    #[test]
    fn toc_lines() {
        let e = parse_toc_line("2.1 Grundlagen ............ 14").unwrap();
        assert_eq!(e, TocEntry { title: "2.1 Grundlagen".into(), number: 14, level: 1 });
        assert_eq!(parse_toc_line("Einleitung 3").unwrap().level, 0);
        assert_eq!(parse_toc_line("Anhang…… 120").unwrap().title, "Anhang");
        assert!(parse_toc_line("Seite 2 von 10x").is_none());
        assert!(parse_toc_line("2024").is_none(), "number only");
        assert!(parse_toc_line("Postleitzahl12345").is_none(), "no separator");
    }

    #[test]
    fn page_order_follows_bookmarks() {
        // Bookmarks: page 4, page 1, page 4 again; 6 pages.
        let order = page_order_by_bookmarks(6, &[Some(4), Some(1), Some(4), None]);
        assert_eq!(order, vec![0, 4, 5, 1, 2, 3]);
        assert_eq!(page_order_by_bookmarks(3, &[]), vec![0, 1, 2]);
    }

    #[test]
    fn zoom_keeps_position() {
        let p = Object::Reference((5, 0));
        let old = vec![p.clone(), Object::Name(b"XYZ".to_vec()), 10.into(), 700.into(), Object::Null];
        let new = with_zoom(&old, Zoom::Percent(150.0)).unwrap();
        assert_eq!(new[3], Object::Integer(700));
        assert_eq!(new[4], Object::Real(1.5));
        assert_eq!(
            with_zoom(&old, Zoom::FitWidth).unwrap(),
            vec![p, Object::Name(b"FitH".to_vec()), 700.into()]
        );
    }

    #[test]
    fn names_are_unique() {
        let mut used = Vec::new();
        assert_eq!(name_for("Kapitel 1: Einführung", &mut used), b"Kapitel_1_Einf_hrung".to_vec());
        assert_eq!(name_for("Kapitel 1: Einführung", &mut used), b"Kapitel_1_Einf_hrung_2".to_vec());
        assert_eq!(name_for("äöü", &mut used), b"Lesezeichen".to_vec());
    }
}
