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
