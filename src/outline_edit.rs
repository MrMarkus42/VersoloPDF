//! Bookmark commands of the ribbon tab "Lesezeichen":
//! which bookmarks a command works on, destinations and named destinations
//! ("Sprungmarken"), zoom, actions, sorting, merging, checking, styled new
//! bookmarks and the page order by bookmarks.

use lopdf::{Dictionary, Object, ObjectId};

use crate::error::Result;
use crate::model::Rgba;
use crate::outline::{MAX_DEPTH, NewBookmark, children, outline_root, recount, relink, title_of};
use crate::outline_format::{ItemInfo, collapse_spaces, expand};
use crate::pdf_edit::{Edit, guarded, text_string};

const MAX_ITEMS: usize = 20_000;

// ---------------------------------------------------------------------------
// Which bookmarks

/// "Lesezeichenquelle auswählen" and "Ebenen verarbeiten".
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Scope {
    /// Path of the selected bookmark (child indices); None: all bookmarks.
    pub selected: Option<Vec<usize>>,
    /// With a selection: also everything below it.
    pub children: bool,
    /// Only this level (1 = top level) …
    pub level: Option<usize>,
    /// … and the levels below it.
    pub below: bool,
}

impl Scope {
    pub fn all() -> Self {
        Self::default()
    }

    /// Whether the bookmark at `path` belongs to the scope.
    pub fn contains(&self, path: &[usize]) -> bool {
        let in_selection = match &self.selected {
            None => true,
            Some(sel) => {
                path == sel.as_slice() || (self.children && path.starts_with(sel) && path.len() > sel.len())
            }
        };
        let depth = path.len();
        let in_level = match self.level {
            None => true,
            Some(l) => depth == l || (self.below && depth > l),
        };
        in_selection && in_level
    }
}

/// A bookmark with its place in the outline.
#[derive(Debug, Clone)]
struct Item {
    id: ObjectId,
    /// Child indices from the top level (0-based).
    path: Vec<usize>,
    /// Titles from the top level down to this item.
    titles: Vec<String>,
}

fn walk(
    edit: &Edit,
    node: ObjectId,
    path: &mut Vec<usize>,
    titles: &mut Vec<String>,
    out: &mut Vec<Item>,
) -> Result<()> {
    if path.len() >= MAX_DEPTH {
        return Ok(());
    }
    for (i, kid) in children(edit, node)?.into_iter().enumerate() {
        if out.len() >= MAX_ITEMS || out.iter().any(|it| it.id == kid) {
            break;
        }
        path.push(i);
        titles.push(title_of(edit, kid));
        out.push(Item { id: kid, path: path.clone(), titles: titles.clone() });
        walk(edit, kid, path, titles, out)?;
        titles.pop();
        path.pop();
    }
    Ok(())
}

/// All bookmarks (depth first) and the outline root.
fn all_items(edit: &mut Edit) -> Result<Option<(ObjectId, Vec<Item>)>> {
    let Some(root) = outline_root(edit, false)? else { return Ok(None) };
    let mut out = Vec::new();
    walk(edit, root, &mut Vec::new(), &mut Vec::new(), &mut out)?;
    Ok(Some((root, out)))
}

fn info(item: &Item, edit: &Edit, pages: &[ObjectId], sequence: usize, base: &ItemInfo) -> ItemInfo {
    ItemInfo {
        title: item.titles.last().cloned().unwrap_or_default(),
        path_titles: item.titles.clone(),
        path_index: item.path.iter().map(|i| i + 1).collect(),
        sequence,
        page: target_page(edit, item.id, pages),
        ..base.clone()
    }
}

/// Runs `f` on every bookmark in `scope`; `f` returns whether it changed
/// the item. Returns the bytes and the number of changed items.
fn for_each(
    bytes: &[u8],
    password: Option<&str>,
    scope: &Scope,
    mut f: impl FnMut(&mut Edit, ObjectId, &ItemInfo) -> Result<bool>,
    base: &ItemInfo,
) -> Result<(Vec<u8>, usize)> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let Some((_, items)) = all_items(&mut edit)? else { return Ok((bytes.to_vec(), 0)) };
        let pages = edit.page_ids();
        let mut changed = 0;
        let mut seq = 0;
        for item in items.iter().filter(|it| scope.contains(&it.path)) {
            seq += 1;
            let data = info(item, &edit, &pages, seq, base);
            if f(&mut edit, item.id, &data)? {
                changed += 1;
            }
        }
        if changed == 0 {
            return Ok((bytes.to_vec(), 0));
        }
        Ok((edit.finish()?, changed))
    })
}

/// Rewrites the titles in `scope` with `f`.
pub fn map_titles(
    bytes: &[u8],
    password: Option<&str>,
    scope: &Scope,
    base: &ItemInfo,
    f: impl Fn(&str, &ItemInfo) -> String,
) -> Result<(Vec<u8>, usize)> {
    for_each(
        bytes,
        password,
        scope,
        |edit, id, data| {
            let new = f(&data.title, data);
            if new == data.title {
                return Ok(false);
            }
            edit.dict_mut(id)?.set("Title", text_string(&new));
            Ok(true)
        },
        base,
    )
}

// ---------------------------------------------------------------------------
// Targets

/// The action types of "Aktionen entfernen".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionKind {
    JavaScript,
    /// "Zu Seite im Dokument springen" (also plain destinations).
    GoTo,
    GoToR,
    GoToE,
    Uri,
    Launch,
    /// "Befehl ausführen".
    Named,
    ResetForm,
    SubmitForm,
    Sound,
    /// "Zu 3D/Multimedia-Ansicht wechseln".
    Media,
    /// "Felder anzeigen/verbergen".
    Hide,
    Other,
}

impl ActionKind {
    pub const ALL: [ActionKind; 13] = [
        ActionKind::JavaScript,
        ActionKind::GoTo,
        ActionKind::GoToR,
        ActionKind::GoToE,
        ActionKind::Uri,
        ActionKind::Launch,
        ActionKind::Named,
        ActionKind::ResetForm,
        ActionKind::SubmitForm,
        ActionKind::Sound,
        ActionKind::Media,
        ActionKind::Hide,
        ActionKind::Other,
    ];

    pub fn label(self, de: bool) -> &'static str {
        let (g, e) = match self {
            ActionKind::JavaScript => ("JavaScript ausführen", "Run JavaScript"),
            ActionKind::GoTo => ("Zu Seite im Dokument springen", "Go to a page in the document"),
            ActionKind::GoToR => {
                ("Zu Seite in anderem Dokument springen", "Go to a page in another document")
            }
            ActionKind::GoToE => {
                ("Zu Seite in eingebettetem Dokument springen", "Go to a page in an embedded document")
            }
            ActionKind::Uri => ("Web-Seite öffnen", "Open a web page"),
            ActionKind::Launch => ("Datei öffnen", "Open a file"),
            ActionKind::Named => ("Befehl ausführen", "Run a command"),
            ActionKind::ResetForm => ("Formular zurücksetzen", "Reset a form"),
            ActionKind::SubmitForm => ("Formular übermitteln", "Submit a form"),
            ActionKind::Sound => ("Audio abspielen", "Play audio"),
            ActionKind::Media => ("Zu 3D/Multimedia-Ansicht wechseln", "Go to a 3D/multimedia view"),
            ActionKind::Hide => ("Felder anzeigen/verbergen", "Show/hide fields"),
            ActionKind::Other => ("Andere", "Other"),
        };
        if de { g } else { e }
    }

    fn from_name(s: &[u8]) -> ActionKind {
        match s {
            b"JavaScript" => ActionKind::JavaScript,
            b"GoTo" => ActionKind::GoTo,
            b"GoToR" => ActionKind::GoToR,
            b"GoToE" => ActionKind::GoToE,
            b"URI" => ActionKind::Uri,
            b"Launch" => ActionKind::Launch,
            b"Named" => ActionKind::Named,
            b"ResetForm" => ActionKind::ResetForm,
            b"SubmitForm" => ActionKind::SubmitForm,
            b"Sound" => ActionKind::Sound,
            b"GoTo3DView" | b"Rendition" | b"Movie" | b"RichMediaExecute" => ActionKind::Media,
            b"Hide" => ActionKind::Hide,
            _ => ActionKind::Other,
        }
    }
}

/// Where a bookmark keeps its target.
#[derive(Debug, Clone, PartialEq)]
enum Slot {
    /// `/Dest` with its value.
    Dest(Object),
    /// An action: its type, `/D` (if any) and the action dictionary.
    Action(ActionKind, Option<Object>, Dictionary),
    /// Action dictionary without type.
    Broken,
    Nothing,
}

fn slot(edit: &Edit, id: ObjectId) -> Result<Slot> {
    let dict = edit.object(id)?.as_dict()?;
    if let Ok(d) = dict.get(b"Dest") {
        return Ok(Slot::Dest(edit.resolve(d)?.clone()));
    }
    let Ok(a) = dict.get(b"A") else { return Ok(Slot::Nothing) };
    let Ok(action) = edit.resolve(a)?.as_dict() else { return Ok(Slot::Broken) };
    let Some(kind) = action.get(b"S").ok().and_then(|s| s.as_name().ok()) else { return Ok(Slot::Broken) };
    let d = match action.get(b"D") {
        Ok(d) => Some(edit.resolve(d)?.clone()),
        Err(_) => None,
    };
    Ok(Slot::Action(ActionKind::from_name(kind), d, action.clone()))
}

fn kind_of(slot: &Slot) -> Option<ActionKind> {
    match slot {
        Slot::Dest(_) => Some(ActionKind::GoTo),
        Slot::Action(k, ..) => Some(*k),
        Slot::Broken => Some(ActionKind::Other),
        Slot::Nothing => None,
    }
}

/// Writes `dest` where the bookmark keeps its target (`/Dest` or the `/D`
/// of its action).
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

/// Removes `name` from the catalog's `/Dests` and from the name tree
/// (nodes that are objects of their own).
fn remove_name(edit: &mut Edit, name: &[u8]) -> Result<()> {
    fn tree(edit: &mut Edit, node: ObjectId, name: &[u8], depth: usize) -> Result<()> {
        if depth > MAX_DEPTH {
            return Ok(());
        }
        let dict = edit.object(node)?.as_dict()?.clone();
        if let Ok(Object::Array(names)) = dict.get(b"Names") {
            let kept: Vec<Object> = names
                .chunks(2)
                .filter(|p| !matches!(p.first(), Some(Object::String(k, _)) if k.as_slice() == name))
                .flatten()
                .cloned()
                .collect();
            if kept.len() != names.len() {
                edit.dict_mut(node)?.set("Names", Object::Array(kept));
            }
        }
        if let Ok(Object::Array(kids)) = dict.get(b"Kids") {
            for kid in kids.iter().filter_map(|k| k.as_reference().ok()) {
                tree(edit, kid, name, depth + 1)?;
            }
        }
        Ok(())
    }
    let cat = catalog_id(edit)?;
    let catalog = edit.object(cat)?.as_dict()?.clone();
    match catalog.get(b"Dests") {
        Ok(Object::Reference(id)) => {
            edit.dict_mut(*id)?.remove(name);
        }
        Ok(Object::Dictionary(d)) => {
            let mut d = d.clone();
            d.remove(name);
            edit.dict_mut(cat)?.set("Dests", Object::Dictionary(d));
        }
        _ => {}
    }
    let names = match catalog.get(b"Names") {
        Ok(Object::Reference(id)) => edit.object(*id)?.as_dict().ok().cloned(),
        Ok(Object::Dictionary(d)) => Some(d.clone()),
        _ => None,
    };
    if let Some(Ok(Object::Reference(root))) = names.as_ref().map(|n| n.get(b"Dests")) {
        tree(edit, *root, name, 0)?;
    }
    Ok(())
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

fn name_bytes(dest: &Object) -> Option<&[u8]> {
    match dest {
        Object::Name(n) | Object::String(n, _) => Some(n),
        _ => None,
    }
}

/// The destination of a jump inside the document.
fn local_dest(slot: &Slot) -> Option<&Object> {
    match slot {
        Slot::Dest(d) | Slot::Action(ActionKind::GoTo, Some(d), _) => Some(d),
        _ => None,
    }
}

/// Page index a bookmark leads to (explicit or named destination).
pub(crate) fn target_page(edit: &Edit, id: ObjectId, pages: &[ObjectId]) -> Option<usize> {
    let slot = slot(edit, id).ok()?;
    let target = explicit(edit, local_dest(&slot)?)?.first()?.as_reference().ok()?;
    pages.iter().position(|p| *p == target)
}

/// "Sprungmarken zu normalen Zielen": named destinations become explicit;
/// with `remove_names` the converted names are removed from the document.
pub fn resolve_named(
    bytes: &[u8],
    password: Option<&str>,
    scope: &Scope,
    remove_names: bool,
) -> Result<(Vec<u8>, usize)> {
    let mut done: Vec<Vec<u8>> = Vec::new();
    let (out, n) = for_each(
        bytes,
        password,
        scope,
        |edit, id, _| {
            let slot = slot(edit, id)?;
            let Some(d) = local_dest(&slot).filter(|d| is_named(d)) else { return Ok(false) };
            let Some(arr) = explicit(edit, d) else { return Ok(false) };
            if let Some(name) = name_bytes(d) {
                done.push(name.to_vec());
            }
            set_target(edit, id, Object::Array(arr))?;
            Ok(true)
        },
        &ItemInfo::default(),
    )?;
    if !remove_names || n == 0 {
        return Ok((out, n));
    }
    guarded(|| {
        let mut edit = Edit::open(&out, password)?;
        for name in &done {
            remove_name(&mut edit, name)?;
        }
        Ok((edit.finish()?, n))
    })
}

/// "In Sprungmarken umwandeln": explicit destinations become named ones
/// (catalog `/Dests`); the name comes from `mask` (placeholders).
pub fn to_named(
    bytes: &[u8],
    password: Option<&str>,
    scope: &Scope,
    mask: &str,
    base: &ItemInfo,
) -> Result<(Vec<u8>, usize)> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let Some((_, items)) = all_items(&mut edit)? else { return Ok((bytes.to_vec(), 0)) };
        let pages = edit.page_ids();
        let mut todo = Vec::new();
        let mut seq = 0;
        for item in items.iter().filter(|it| scope.contains(&it.path)) {
            seq += 1;
            if let Some(Object::Array(arr)) = local_dest(&slot(&edit, item.id)?) {
                let name = expand(mask, &info(item, &edit, &pages, seq, base));
                todo.push((item.id, arr.clone(), name));
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
        for (id, arr, name) in &todo {
            let name = unique_name(name, &mut used);
            edit.dict_mut(dests)?.set(name.clone(), Object::Array(arr.clone()));
            set_target(&mut edit, *id, Object::Name(name))?;
        }
        Ok((edit.finish()?, todo.len()))
    })
}

/// `wanted` (or "Lesezeichen"), made unique with "_2", "_3" ….
fn unique_name(wanted: &str, used: &mut Vec<Vec<u8>>) -> Vec<u8> {
    let mut base: String = wanted.trim().chars().filter(|c| !c.is_control()).take(120).collect();
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

// ---------------------------------------------------------------------------
// Zoom

/// View of a destination ("Vergrößerung").
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Zoom {
    /// "Geerbt": keep the reader's zoom.
    Inherit,
    /// "Tatsächliche Größe".
    ActualSize,
    /// "Seite einpassen".
    Fit,
    /// "Breite einpassen".
    FitWidth,
    /// "Höhe einpassen".
    FitHeight,
    /// "Sichtbarer Bereich einpassen".
    FitVisible,
    /// "Sichtbare Breite einpassen".
    FitVisibleWidth,
    /// "Sichtbare Höhe einpassen".
    FitVisibleHeight,
    /// Fixed zoom in percent.
    Percent(f32),
}

impl Zoom {
    /// The named choices of the list (percentages follow).
    pub const MODES: [Zoom; 8] = [
        Zoom::Inherit,
        Zoom::ActualSize,
        Zoom::Fit,
        Zoom::FitWidth,
        Zoom::FitHeight,
        Zoom::FitVisible,
        Zoom::FitVisibleWidth,
        Zoom::FitVisibleHeight,
    ];
    pub const PERCENTS: [f32; 13] =
        [8.33, 12.5, 25.0, 50.0, 100.0, 150.0, 200.0, 400.0, 800.0, 1600.0, 2400.0, 3200.0, 6400.0];

    pub fn label(self, de: bool) -> String {
        let (g, e) = match self {
            Zoom::Inherit => ("Geerbt", "Inherit"),
            Zoom::ActualSize => ("Tatsächliche Größe", "Actual size"),
            Zoom::Fit => ("Seite einpassen", "Fit page"),
            Zoom::FitWidth => ("Breite einpassen", "Fit width"),
            Zoom::FitHeight => ("Höhe einpassen", "Fit height"),
            Zoom::FitVisible => ("Sichtbarer Bereich einpassen", "Fit visible"),
            Zoom::FitVisibleWidth => ("Sichtbare Breite einpassen", "Fit visible width"),
            Zoom::FitVisibleHeight => ("Sichtbare Höhe einpassen", "Fit visible height"),
            Zoom::Percent(p) => {
                let s = format!("{}%", (p * 100.0).round() / 100.0);
                return if de { s.replace('.', ",") } else { s };
            }
        };
        (if de { g } else { e }).to_string()
    }

    /// Reads "150", "150 %" or "8,33%".
    pub fn parse_percent(text: &str) -> Option<Zoom> {
        let t = text.trim().trim_end_matches('%').trim().replace(',', ".");
        let p: f32 = t.parse().ok()?;
        (1.0..=6400.0).contains(&p).then_some(Zoom::Percent(p))
    }
}

fn number(o: Option<&Object>) -> Object {
    match o {
        Some(o @ (Object::Integer(_) | Object::Real(_))) => o.clone(),
        _ => Object::Null,
    }
}

/// Destination `arr` (page first) with `zoom`, keeping the position on the
/// page where the old one had it.
fn with_zoom(arr: &[Object], zoom: Zoom) -> Option<Vec<Object>> {
    let page = arr.first()?.clone();
    let kind = arr.get(1).and_then(|k| k.as_name().ok()).unwrap_or(b"");
    let (left, top) = match kind {
        b"XYZ" => (number(arr.get(2)), number(arr.get(3))),
        b"FitH" | b"FitBH" => (Object::Null, number(arr.get(2))),
        b"FitV" | b"FitBV" => (number(arr.get(2)), Object::Null),
        _ => (Object::Null, Object::Null),
    };
    Some(dest_array(page, zoom, left, top))
}

fn dest_array(page: Object, zoom: Zoom, left: Object, top: Object) -> Vec<Object> {
    let name = |n: &str| Object::Name(n.as_bytes().to_vec());
    match zoom {
        Zoom::Inherit => vec![page, name("XYZ"), left, top, Object::Null],
        Zoom::ActualSize => vec![page, name("XYZ"), left, top, Object::Real(1.0)],
        Zoom::Percent(p) => vec![page, name("XYZ"), left, top, Object::Real(p.clamp(1.0, 6400.0) / 100.0)],
        Zoom::Fit => vec![page, name("Fit")],
        Zoom::FitWidth => vec![page, name("FitH"), top],
        Zoom::FitHeight => vec![page, name("FitV"), left],
        Zoom::FitVisible => vec![page, name("FitB")],
        Zoom::FitVisibleWidth => vec![page, name("FitBH"), top],
        Zoom::FitVisibleHeight => vec![page, name("FitBV"), left],
    }
}

/// Which targets "Vergrößerung ändern" touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZoomTargets {
    /// "Sprungmarken in Lesezeichen ändern" (they become explicit).
    pub named: bool,
    pub gotor: bool,
    pub gotoe: bool,
}

/// "Vergrößerung ändern".
pub fn set_zoom(
    bytes: &[u8],
    password: Option<&str>,
    scope: &Scope,
    zoom: Zoom,
    targets: ZoomTargets,
) -> Result<(Vec<u8>, usize)> {
    for_each(
        bytes,
        password,
        scope,
        |edit, id, _| {
            let slot = slot(edit, id)?;
            let dest = match &slot {
                Slot::Dest(d) | Slot::Action(ActionKind::GoTo, Some(d), _) => d,
                Slot::Action(ActionKind::GoToR, Some(d), _) if targets.gotor => d,
                Slot::Action(ActionKind::GoToE, Some(d), _) if targets.gotoe => d,
                _ => return Ok(false),
            };
            if is_named(dest) && !targets.named {
                return Ok(false);
            }
            let Some(arr) = explicit(edit, dest) else { return Ok(false) };
            let Some(new) = with_zoom(&arr, zoom) else { return Ok(false) };
            if new == arr && !is_named(dest) {
                return Ok(false);
            }
            set_target(edit, id, Object::Array(new))?;
            Ok(true)
        },
        &ItemInfo::default(),
    )
}

// ---------------------------------------------------------------------------
// Removing

/// Removes the items `gone` from the outline; with `keep_children` their
/// children move up in their place, else they go too.
fn remove_items(edit: &mut Edit, root: ObjectId, gone: &[ObjectId], keep_children: bool) -> Result<usize> {
    fn level(
        edit: &mut Edit,
        node: ObjectId,
        gone: &[ObjectId],
        keep: bool,
        depth: usize,
    ) -> Result<(Vec<ObjectId>, usize)> {
        let kids = children(edit, node)?;
        if depth > MAX_DEPTH {
            return Ok((kids, 0));
        }
        let mut kept = Vec::new();
        let mut removed = 0;
        for kid in kids {
            if gone.contains(&kid) {
                removed += 1;
                if keep {
                    let (grand, n) = level(edit, kid, gone, keep, depth + 1)?;
                    removed += n;
                    kept.extend(grand);
                }
            } else {
                let (grand, n) = level(edit, kid, gone, keep, depth + 1)?;
                if n > 0 {
                    relink(edit, kid, &grand)?;
                }
                removed += n;
                kept.push(kid);
            }
        }
        Ok((kept, removed))
    }
    let (kept, removed) = level(edit, root, gone, keep_children, 0)?;
    if removed > 0 {
        relink(edit, root, &kept)?;
        recount(edit, root, 0)?;
    }
    Ok(removed)
}

/// "Aktionen entfernen": removes the actions of the chosen kinds (a GoTo
/// becomes no target at all); with `remove_bookmarks` the bookmarks with
/// such actions are removed instead (`keep_children`: their children stay).
pub fn remove_actions(
    bytes: &[u8],
    password: Option<&str>,
    scope: &Scope,
    kinds: &[ActionKind],
    remove_bookmarks: bool,
    keep_children: bool,
) -> Result<(Vec<u8>, usize)> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let Some((root, items)) = all_items(&mut edit)? else { return Ok((bytes.to_vec(), 0)) };
        let mut hit = Vec::new();
        for item in items.iter().filter(|it| scope.contains(&it.path)) {
            if kind_of(&slot(&edit, item.id)?).is_some_and(|k| kinds.contains(&k)) {
                hit.push(item.id);
            }
        }
        if hit.is_empty() {
            return Ok((bytes.to_vec(), 0));
        }
        let n = if remove_bookmarks {
            remove_items(&mut edit, root, &hit, keep_children)?
        } else {
            for id in &hit {
                let d = edit.dict_mut(*id)?;
                d.remove(b"A");
                d.remove(b"Dest");
            }
            hit.len()
        };
        Ok((edit.finish()?, n))
    })
}

/// Removes the bookmarks at `paths`; their children move up.
pub fn delete_paths(bytes: &[u8], password: Option<&str>, paths: &[Vec<usize>]) -> Result<(Vec<u8>, usize)> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let Some((root, items)) = all_items(&mut edit)? else { return Ok((bytes.to_vec(), 0)) };
        let gone: Vec<ObjectId> =
            items.iter().filter(|it| paths.contains(&it.path)).map(|it| it.id).collect();
        if gone.is_empty() {
            return Ok((bytes.to_vec(), 0));
        }
        let n = remove_items(&mut edit, root, &gone, true)?;
        Ok((edit.finish()?, n))
    })
}

// ---------------------------------------------------------------------------
// Sorting and merging

/// Parents whose children lists a sort or merge works on: with a selection
/// the selected bookmark (and with `children` everything below it), else
/// every list whose items lie in the chosen levels.
fn sibling_lists(root: ObjectId, items: &[Item], scope: &Scope) -> Vec<ObjectId> {
    let mut out = Vec::new();
    let mut add = |p: ObjectId| {
        if !out.contains(&p) {
            out.push(p);
        }
    };
    match &scope.selected {
        Some(sel) => {
            for it in items {
                if it.path == *sel || (scope.children && it.path.starts_with(sel)) {
                    add(it.id);
                }
            }
        }
        None => {
            let level_ok = |depth: usize| scope.contains(&vec![0; depth]);
            if level_ok(1) {
                add(root);
            }
            for it in items {
                if level_ok(it.path.len() + 1) {
                    add(it.id);
                }
            }
        }
    }
    out
}

/// "Sortieren": by referenced page (then title) or alphabetically.
pub fn sort(
    bytes: &[u8],
    password: Option<&str>,
    scope: &Scope,
    by_page: bool,
    descending: bool,
    ignore_case: bool,
) -> Result<Vec<u8>> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let Some((root, items)) = all_items(&mut edit)? else { return Ok(bytes.to_vec()) };
        let pages = edit.page_ids();
        for parent in sibling_lists(root, &items, scope) {
            let mut kids: Vec<(ObjectId, String, usize)> = children(&edit, parent)?
                .into_iter()
                .map(|k| {
                    let t = title_of(&edit, k);
                    let t = if ignore_case { t.to_lowercase() } else { t };
                    (k, t, target_page(&edit, k, &pages).unwrap_or(usize::MAX))
                })
                .collect();
            let cmp = |a: &(ObjectId, String, usize), b: &(ObjectId, String, usize)| {
                if by_page {
                    a.2.cmp(&b.2).then_with(|| crate::outline::natural_cmp(&a.1, &b.1))
                } else {
                    crate::outline::natural_cmp(&a.1, &b.1)
                }
            };
            kids.sort_by(|a, b| if descending { cmp(b, a) } else { cmp(a, b) });
            let ids: Vec<ObjectId> = kids.iter().map(|k| k.0).collect();
            relink(&mut edit, parent, &ids)?;
        }
        edit.finish()
    })
}

/// Options of "Duplikate zusammenfassen".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MergeOptions {
    pub ignore_case: bool,
    pub ignore_spaces: bool,
    /// Title of the result from the last duplicate (else the first).
    pub title_from_last: bool,
    /// Target of the result from the last duplicate (else the first).
    pub action_from_last: bool,
}

/// "Duplikate zusammenfassen": siblings with the same title become one
/// bookmark; the children of all of them are kept below it.
pub fn merge_duplicates(
    bytes: &[u8],
    password: Option<&str>,
    scope: &Scope,
    opts: MergeOptions,
) -> Result<(Vec<u8>, usize)> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let Some((root, items)) = all_items(&mut edit)? else { return Ok((bytes.to_vec(), 0)) };
        let key = |t: &str| {
            let t = if opts.ignore_spaces { collapse_spaces(t.trim()) } else { t.to_string() };
            if opts.ignore_case { t.to_lowercase() } else { t }
        };
        let mut merged = 0;
        for parent in sibling_lists(root, &items, scope) {
            let kids = children(&edit, parent)?;
            let mut groups: Vec<(String, Vec<ObjectId>)> = Vec::new();
            for k in &kids {
                let t = key(&title_of(&edit, *k));
                match groups.iter_mut().find(|(g, _)| *g == t) {
                    Some((_, ids)) => ids.push(*k),
                    None => groups.push((t, vec![*k])),
                }
            }
            if groups.iter().all(|(_, ids)| ids.len() == 1) {
                continue;
            }
            let mut kept = Vec::new();
            for (_, ids) in &groups {
                let first = ids[0];
                let last = *ids.last().expect("group is not empty");
                if ids.len() > 1 {
                    if opts.title_from_last {
                        let t = title_of(&edit, last);
                        edit.dict_mut(first)?.set("Title", text_string(&t));
                    }
                    if opts.action_from_last {
                        let src = edit.object(last)?.as_dict()?.clone();
                        let d = edit.dict_mut(first)?;
                        d.remove(b"Dest");
                        d.remove(b"A");
                        for key in [b"Dest".as_slice(), b"A".as_slice()] {
                            if let Ok(v) = src.get(key) {
                                d.set(key.to_vec(), v.clone());
                            }
                        }
                    }
                    let mut all_kids = children(&edit, first)?;
                    for other in &ids[1..] {
                        all_kids.extend(children(&edit, *other)?);
                    }
                    relink(&mut edit, first, &all_kids)?;
                    merged += ids.len() - 1;
                }
                kept.push(first);
            }
            relink(&mut edit, parent, &kept)?;
        }
        if merged == 0 {
            return Ok((bytes.to_vec(), 0));
        }
        recount(&mut edit, root, 0)?;
        Ok((edit.finish()?, merged))
    })
}

// ---------------------------------------------------------------------------
// Checking

/// What is wrong with a bookmark ("Lesezeichen überprüfen").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Problem {
    /// "Warnen bei leerem Titel".
    EmptyTitle,
    /// "… bei leerer Aktionsliste und ohne Kindelemente".
    EmptyNoChildren,
    /// "… bei ungültigen GoTo-Aktionen" (page or named destination missing).
    InvalidGoTo,
    /// "… bei fehlender GoTo-Aktion".
    MissingGoTo,
    /// "… bei ungültiger GoToR- oder GoToE-Aktionen".
    InvalidRemote,
    /// "… bei ungültigen Start- und URL-Aktionen".
    InvalidLaunchUri,
    /// "… bei anderen ungültigen Aktionen".
    InvalidOther,
}

impl Problem {
    pub const ALL: [Problem; 7] = [
        Problem::EmptyTitle,
        Problem::EmptyNoChildren,
        Problem::InvalidGoTo,
        Problem::MissingGoTo,
        Problem::InvalidRemote,
        Problem::InvalidLaunchUri,
        Problem::InvalidOther,
    ];

    /// Label of the check option.
    pub fn option(self, de: bool) -> &'static str {
        let (g, e) = match self {
            Problem::EmptyTitle => ("Warnen bei leerem Titel", "Warn about an empty title"),
            Problem::EmptyNoChildren => (
                "Warnen bei leerer Aktionsliste und ohne Kindelemente",
                "Warn about no action and no children",
            ),
            Problem::InvalidGoTo => {
                ("Warnen bei ungültigen GoTo-Aktionen", "Warn about invalid GoTo actions")
            }
            Problem::MissingGoTo => ("Warnen bei fehlender GoTo-Aktion", "Warn about a missing GoTo action"),
            Problem::InvalidRemote => (
                "Warnen bei ungültiger GoToR- oder GoToE-Aktionen",
                "Warn about invalid GoToR or GoToE actions",
            ),
            Problem::InvalidLaunchUri => {
                ("Warnen bei ungültigen Start- und URL-Aktionen", "Warn about invalid launch and URL actions")
            }
            Problem::InvalidOther => {
                ("Warnen bei anderen ungültigen Aktionen", "Warn about other invalid actions")
            }
        };
        if de { g } else { e }
    }

    /// Short text in the result list.
    pub fn text(self, de: bool) -> &'static str {
        let (g, e) = match self {
            Problem::EmptyTitle => ("leerer Titel", "empty title"),
            Problem::EmptyNoChildren => ("keine Aktion, keine Unterlesezeichen", "no action, no children"),
            Problem::InvalidGoTo => ("Ziel ungültig", "invalid target"),
            Problem::MissingGoTo => ("kein Ziel", "no target"),
            Problem::InvalidRemote => {
                ("Verweis auf anderes Dokument ungültig", "invalid link to another document")
            }
            Problem::InvalidLaunchUri => ("Datei- oder Web-Verweis ungültig", "invalid file or web link"),
            Problem::InvalidOther => ("ungültige Aktion", "invalid action"),
        };
        if de { g } else { e }
    }
}

/// A finding of the check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub path: Vec<usize>,
    pub title: String,
    pub problem: Problem,
}

fn has_string(d: &Dictionary, key: &[u8]) -> bool {
    match d.get(key) {
        Ok(Object::String(s, _)) => !s.is_empty(),
        Ok(Object::Dictionary(_) | Object::Reference(_)) => true,
        _ => false,
    }
}

fn problems(edit: &Edit, item: &Item, pages: &[ObjectId]) -> Result<Vec<Problem>> {
    let mut out = Vec::new();
    if item.titles.last().is_none_or(|t| t.trim().is_empty()) {
        out.push(Problem::EmptyTitle);
    }
    let slot = slot(edit, item.id)?;
    let local_ok = |d: &Object| match explicit(edit, d).as_ref().and_then(|a| a.first()) {
        Some(Object::Reference(p)) => pages.contains(p),
        _ => false,
    };
    match &slot {
        Slot::Nothing => {
            if children(edit, item.id)?.is_empty() {
                out.push(Problem::EmptyNoChildren);
            }
            out.push(Problem::MissingGoTo);
        }
        Slot::Dest(d) | Slot::Action(ActionKind::GoTo, Some(d), _) => {
            if !local_ok(d) {
                out.push(Problem::InvalidGoTo);
            }
        }
        Slot::Action(ActionKind::GoTo, None, _) => out.push(Problem::MissingGoTo),
        Slot::Action(ActionKind::GoToR, d, a) => {
            if d.is_none() || !has_string(a, b"F") {
                out.push(Problem::InvalidRemote);
            }
        }
        Slot::Action(ActionKind::GoToE, d, _) => {
            if d.is_none() {
                out.push(Problem::InvalidRemote);
            }
        }
        Slot::Action(ActionKind::Uri, _, a) => {
            if !has_string(a, b"URI") {
                out.push(Problem::InvalidLaunchUri);
            }
        }
        Slot::Action(ActionKind::Launch, _, a) => {
            if !has_string(a, b"F") && a.get(b"Win").is_err() {
                out.push(Problem::InvalidLaunchUri);
            }
        }
        Slot::Action(ActionKind::Other, ..) | Slot::Broken => out.push(Problem::InvalidOther),
        Slot::Action(..) => {}
    }
    Ok(out)
}

/// "Überprüfen": every bookmark with one of the `wanted` problems (the
/// first one found is reported).
pub fn check(bytes: &[u8], password: Option<&str>, wanted: &[Problem]) -> Result<Vec<Finding>> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let Some((_, items)) = all_items(&mut edit)? else { return Ok(Vec::new()) };
        let pages = edit.page_ids();
        let mut out = Vec::new();
        for item in &items {
            if let Some(p) = problems(&edit, item, &pages)?.into_iter().find(|p| wanted.contains(p)) {
                out.push(Finding {
                    path: item.path.clone(),
                    title: item.titles.last().cloned().unwrap_or_default(),
                    problem: p,
                });
            }
        }
        Ok(out)
    })
}

// ---------------------------------------------------------------------------
// New bookmarks with appearance

/// "Aussehen" of new bookmarks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Appearance {
    pub color: Rgba,
    pub bold: bool,
    pub italic: bool,
    pub zoom: Zoom,
    /// "Alle Unterknoten ausklappen".
    pub expand: bool,
}

impl Default for Appearance {
    fn default() -> Self {
        Self { color: Rgba::BLACK, bold: false, italic: false, zoom: Zoom::Inherit, expand: false }
    }
}

/// Where new bookmarks go ("Einstellungen" of "Aus Seitentext").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Placement {
    /// "Existierende Lesezeichen ersetzen".
    Replace,
    /// "… mit neuen zusammenführen": the top level is ordered by page.
    Merge,
    /// "Neue Lesezeichen in existierende einfügen: Vor".
    Before,
    /// "… Nach".
    #[default]
    After,
}

/// Adds `items` (see [`NewBookmark`]), each with its look, placed as
/// `placement` says. The destination keeps the top of the page in view.
pub fn add_styled(
    bytes: &[u8],
    password: Option<&str>,
    items: &[(NewBookmark, Appearance)],
    placement: Placement,
) -> Result<Vec<u8>> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let pages = edit.page_ids();
        if placement == Placement::Replace {
            let cat = catalog_id(&edit)?;
            edit.dict_mut(cat)?.remove(b"Outlines");
        }
        let root = outline_root(&mut edit, true)?.expect("outline root was created");
        let old_top = children(&edit, root)?;
        let mut stack: Vec<(usize, ObjectId)> = Vec::new();
        let mut lists: Vec<(ObjectId, Vec<ObjectId>)> = vec![(root, Vec::new())];
        for (item, look) in items.iter().take(MAX_ITEMS) {
            let [r, g, b] = look.color.to_pdf();
            let level = item.level.min(MAX_DEPTH - 1);
            while stack.last().is_some_and(|(l, _)| *l >= level) {
                stack.pop();
            }
            let parent = stack.last().map_or(root, |(_, id)| *id);
            let mut dict = Dictionary::new();
            dict.set("Title", text_string(&item.title));
            if let Some(page_id) = pages.get(item.page) {
                let top = crate::pages::visible_box(&edit, *page_id).y1;
                let arr = dest_array(Object::Reference(*page_id), look.zoom, Object::Null, Object::Real(top));
                dict.set("Dest", Object::Array(arr));
            }
            if look.color != Rgba::BLACK {
                dict.set("C", Object::Array(vec![Object::Real(r), Object::Real(g), Object::Real(b)]));
            }
            let flags = i64::from(look.italic) | (i64::from(look.bold) << 1);
            if flags != 0 {
                dict.set("F", Object::Integer(flags));
            }
            if !look.expand {
                // Closed; recount fills in the number.
                dict.set("Count", Object::Integer(-1));
            }
            let id = edit.add(Object::Dictionary(dict));
            match lists.iter_mut().find(|(p, _)| *p == parent) {
                Some((_, kids)) => kids.push(id),
                None => lists.push((parent, vec![id])),
            }
            stack.push((level, id));
        }
        for (parent, kids) in &mut lists {
            if *parent == root {
                let mut all = match placement {
                    Placement::Before => kids.iter().chain(&old_top).copied().collect::<Vec<_>>(),
                    _ => old_top.iter().chain(kids.iter()).copied().collect(),
                };
                if placement == Placement::Merge {
                    let key: Vec<(ObjectId, usize)> = all
                        .iter()
                        .map(|id| (*id, target_page(&edit, *id, &pages).unwrap_or(usize::MAX)))
                        .collect();
                    let mut key = key;
                    key.sort_by_key(|(_, p)| *p);
                    all = key.into_iter().map(|(id, _)| id).collect();
                }
                relink(&mut edit, root, &all)?;
            } else {
                relink(&mut edit, *parent, kids)?;
            }
        }
        recount(&mut edit, root, 0)?;
        edit.finish()
    })
}

// ---------------------------------------------------------------------------
// Page order

/// Page of every bookmark in outline order (depth first; None if unknown).
pub fn pages_in_order(bytes: &[u8], password: Option<&str>) -> Result<Vec<Option<usize>>> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let Some((_, items)) = all_items(&mut edit)? else { return Ok(Vec::new()) };
        let pages = edit.page_ids();
        Ok(items.iter().map(|it| target_page(&edit, it.id, &pages)).collect())
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

// ---------------------------------------------------------------------------
// Printed table of contents

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
    let leader_only = |s: &str| !s.trim().is_empty() && s.chars().all(|c| ".…·_ \t".contains(c));
    let mut out = Vec::new();
    // A title line still waiting for its page number.
    let mut pending: Option<String> = None;
    for line in lines.iter().map(|l| l.trim()) {
        if leader_only(line) {
            continue;
        }
        if number_only(line)
            && let Some(title) = pending.take()
        {
            out.extend(parse_toc_line(&format!("{title} {line}")));
            continue;
        }
        match parse_toc_line(line) {
            Some(e) => {
                out.push(e);
                pending = None;
            }
            None => pending = Some(line.to_string()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toc_numbers_on_their_own_line() {
        let lines: Vec<String> =
            ["Inhalt", "Kapitel A", "3", "1.1 Teil", ".........", "12", "Anhang ..... 20"]
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
            vec![p.clone(), Object::Name(b"FitH".to_vec()), 700.into()]
        );
        assert_eq!(
            with_zoom(&old, Zoom::FitVisibleHeight).unwrap(),
            vec![p, Object::Name(b"FitBV".to_vec()), 10.into()]
        );
        assert_eq!(Zoom::parse_percent("8,33 %"), Some(Zoom::Percent(8.33)));
        assert_eq!(Zoom::Percent(12.5).label(true), "12,5%");
    }

    #[test]
    fn names_are_unique() {
        let mut used = Vec::new();
        assert_eq!(unique_name("Kapitel 1", &mut used), b"Kapitel 1".to_vec());
        assert_eq!(unique_name("Kapitel 1", &mut used), b"Kapitel 1_2".to_vec());
        assert_eq!(unique_name("  ", &mut used), b"Lesezeichen".to_vec());
    }

    #[test]
    fn scope_filters_paths() {
        let sel = Scope { selected: Some(vec![1]), children: true, ..Default::default() };
        assert!(sel.contains(&[1]) && sel.contains(&[1, 0, 2]) && !sel.contains(&[0]) && !sel.contains(&[2]));
        let only = Scope { selected: Some(vec![1]), ..Default::default() };
        assert!(only.contains(&[1]) && !only.contains(&[1, 0]));
        let level = Scope { level: Some(2), ..Default::default() };
        assert!(level.contains(&[0, 1]) && !level.contains(&[0]) && !level.contains(&[0, 1, 1]));
        let below = Scope { level: Some(2), below: true, ..Default::default() };
        assert!(below.contains(&[0, 1, 1]));
    }
}
