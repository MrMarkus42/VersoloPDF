//! "Dokumenteigenschaften" beyond title/author:
//! view when opening (catalog /PageMode, /PageLayout, /OpenAction,
//! /ViewerPreferences), custom information entries, advanced settings
//! (base URL, trapping, print presets, reading direction, language),
//! document actions, document-level JavaScript, XMP metadata and fonts.

use lopdf::{Dictionary, Document, Object, ObjectId, StringFormat};

use crate::error::{Error, Result};
use crate::pdf_edit::{Edit, decode_text_string, guarded, text_string};

/// Standard keys of the information dictionary (not "Eigene").
pub const STANDARD_INFO_KEYS: [&str; 9] =
    ["Title", "Author", "Subject", "Keywords", "Creator", "Producer", "CreationDate", "ModDate", "Trapped"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PageMode {
    /// "Standard" (not set)
    #[default]
    Default,
    /// "Nur Seite"
    UseNone,
    /// "Lesezeichenleiste und Seite"
    UseOutlines,
    /// "Vorschauleiste und Seite"
    UseThumbs,
    /// "Ebenenleiste und Seite"
    UseOC,
    /// "Anhangsleiste und Seite"
    UseAttachments,
}

impl PageMode {
    pub const ALL: [PageMode; 6] = [
        PageMode::Default,
        PageMode::UseNone,
        PageMode::UseOutlines,
        PageMode::UseThumbs,
        PageMode::UseOC,
        PageMode::UseAttachments,
    ];

    fn pdf(self) -> Option<&'static [u8]> {
        match self {
            PageMode::Default => None,
            PageMode::UseNone => Some(b"UseNone"),
            PageMode::UseOutlines => Some(b"UseOutlines"),
            PageMode::UseThumbs => Some(b"UseThumbs"),
            PageMode::UseOC => Some(b"UseOC"),
            PageMode::UseAttachments => Some(b"UseAttachments"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LayoutMode {
    /// "Standard" (not set)
    #[default]
    Default,
    /// "Einzelne Seite"
    SinglePage,
    /// "Fortlaufend"
    OneColumn,
    /// "Zwei Seiten - Fortlaufend"
    TwoColumnLeft,
    /// "Zwei Seiten - Fortlaufend mit Titelseite"
    TwoColumnRight,
    /// "Zwei Seiten"
    TwoPageLeft,
    /// "Zwei Seiten mit Titelseite"
    TwoPageRight,
}

impl LayoutMode {
    pub const ALL: [LayoutMode; 7] = [
        LayoutMode::Default,
        LayoutMode::SinglePage,
        LayoutMode::OneColumn,
        LayoutMode::TwoColumnLeft,
        LayoutMode::TwoColumnRight,
        LayoutMode::TwoPageLeft,
        LayoutMode::TwoPageRight,
    ];

    fn pdf(self) -> Option<&'static [u8]> {
        match self {
            LayoutMode::Default => None,
            LayoutMode::SinglePage => Some(b"SinglePage"),
            LayoutMode::OneColumn => Some(b"OneColumn"),
            LayoutMode::TwoColumnLeft => Some(b"TwoColumnLeft"),
            LayoutMode::TwoColumnRight => Some(b"TwoColumnRight"),
            LayoutMode::TwoPageLeft => Some(b"TwoPageLeft"),
            LayoutMode::TwoPageRight => Some(b"TwoPageRight"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OpenZoom {
    /// Keep the viewer's zoom (/XYZ null null null).
    #[default]
    Default,
    /// Whole page (/Fit)
    FitPage,
    /// Page width (/FitH)
    FitWidth,
    /// Visible content (/FitBH)
    FitContent,
}

/// "Aktion beim Öffnen".
#[derive(Debug, Clone, PartialEq, Default)]
pub enum OpenAction {
    /// "nicht festgelegt"
    #[default]
    None,
    /// "Zu Seite springen" (0-based page)
    GoTo { page: usize, zoom: OpenZoom },
    /// "Eigene": another action of the document; kept unchanged.
    Other(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Trapped {
    #[default]
    Unknown,
    Yes,
    No,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DuplexPreset {
    #[default]
    Default,
    Simplex,
    LongEdge,
    ShortEdge,
}

/// All editable properties of the dialog's pages 4 to 8.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DocProps {
    // Ansicht beim Öffnen
    pub page_mode: PageMode,
    pub layout: LayoutMode,
    pub open_action: OpenAction,
    /// "In Titel anzeigen: Dokumenttitel" (otherwise the file name)
    pub show_title: bool,
    pub fullscreen: bool,
    pub fit_window: bool,
    pub center_window: bool,
    pub hide_menubar: bool,
    pub hide_toolbar: bool,
    pub hide_window_ui: bool,
    // Eigene
    pub custom: Vec<(String, String)>,
    // Erweitert
    pub base_url: String,
    pub trapped: Trapped,
    /// "Skalierung: nicht festgelegt" = /PrintScaling /None
    pub print_no_scaling: bool,
    pub duplex: DuplexPreset,
    pub pick_tray: bool,
    /// "Seitenbereich drucken" as 1-based text ("1-3, 5")
    pub print_range: String,
    /// "Anzahl der Kopien" (None: Standard)
    pub copies: Option<u32>,
    pub right_to_left: bool,
    pub language: String,
    // Aktionen: (trigger key WC/WS/DS/WP/DP, JavaScript or description)
    pub actions: Vec<(String, String)>,
    // JavaScript: (name, code)
    pub javascript: Vec<(String, String)>,
    /// XMP metadata packet (read only).
    pub xmp: Option<String>,
}

/// One font of the document ("Schriftarten").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontEntry {
    pub name: String,
    pub kind: String,
    pub encoding: String,
    pub embedded: bool,
    pub subset: bool,
}

fn load(bytes: &[u8], password: Option<&str>) -> Result<Document> {
    let mut options = match password {
        Some(pw) => lopdf::LoadOptions::with_password(pw),
        None => lopdf::LoadOptions::default(),
    };
    options.max_decompressed_size = Some(crate::pdf_edit::MAX_DECOMPRESSED_STREAM);
    Ok(Document::load_mem_with_options(bytes, options)?)
}

fn resolve<'a>(doc: &'a Document, obj: &'a Object) -> &'a Object {
    match obj {
        Object::Reference(id) => doc.get_object(*id).unwrap_or(obj),
        o => o,
    }
}

fn dict_of<'a>(doc: &'a Document, obj: &'a Object) -> Option<&'a Dictionary> {
    match resolve(doc, obj) {
        Object::Dictionary(d) => Some(d),
        Object::Stream(s) => Some(&s.dict),
        _ => None,
    }
}

fn name(dict: &Dictionary, key: &[u8]) -> Option<Vec<u8>> {
    dict.get(key).ok().and_then(|o| o.as_name().ok()).map(<[u8]>::to_vec)
}

fn text(doc: &Document, obj: &Object) -> Option<String> {
    match resolve(doc, obj) {
        Object::String(s, _) => Some(decode_text_string(s)),
        Object::Name(n) => Some(String::from_utf8_lossy(n).into_owned()),
        Object::Stream(s) => s.decompressed_content().ok().map(|c| String::from_utf8_lossy(&c).into_owned()),
        _ => None,
    }
}

fn flag(dict: Option<&Dictionary>, key: &[u8]) -> bool {
    dict.and_then(|d| d.get(key).ok()).and_then(|o| o.as_bool().ok()).unwrap_or(false)
}

/// Text of a JavaScript action (JS as string or stream).
fn js_of(doc: &Document, action: &Object) -> Option<String> {
    let a = dict_of(doc, action)?;
    if name(a, b"S").as_deref() != Some(b"JavaScript") {
        return Some(format!("({})", String::from_utf8_lossy(&name(a, b"S").unwrap_or_default())));
    }
    text(doc, a.get(b"JS").ok()?)
}

/// Reads the properties of pages 4 to 8 of the dialog.
pub fn read(bytes: &[u8], password: Option<&str>) -> Result<DocProps> {
    guarded(|| {
        let doc = load(bytes, password)?;
        let cat_id = doc.trailer.get(b"Root")?.as_reference()?;
        let cat = doc.get_dictionary(cat_id)?.clone();
        let mut p = DocProps::default();
        let mode = name(&cat, b"PageMode").unwrap_or_default();
        p.fullscreen = mode == b"FullScreen";
        p.page_mode =
            PageMode::ALL.into_iter().find(|m| m.pdf() == Some(mode.as_slice())).unwrap_or_default();
        let layout = name(&cat, b"PageLayout").unwrap_or_default();
        p.layout =
            LayoutMode::ALL.into_iter().find(|m| m.pdf() == Some(layout.as_slice())).unwrap_or_default();
        let pages: Vec<ObjectId> = doc.get_pages().into_values().collect();
        p.open_action = match cat.get(b"OpenAction").ok().map(|o| resolve(&doc, o)) {
            None => OpenAction::None,
            Some(Object::Array(dest)) => {
                let page = dest
                    .first()
                    .and_then(|o| o.as_reference().ok())
                    .and_then(|id| pages.iter().position(|p| *p == id));
                let zoom = match dest.get(1).and_then(|o| o.as_name().ok()) {
                    Some(b"Fit") => OpenZoom::FitPage,
                    Some(b"FitH") => OpenZoom::FitWidth,
                    Some(b"FitBH" | b"FitB") => OpenZoom::FitContent,
                    _ => OpenZoom::Default,
                };
                match page {
                    Some(page) => OpenAction::GoTo { page, zoom },
                    None => OpenAction::Other("GoTo".into()),
                }
            }
            Some(Object::Dictionary(a)) => {
                OpenAction::Other(String::from_utf8_lossy(&name(a, b"S").unwrap_or_default()).into_owned())
            }
            Some(_) => OpenAction::Other(String::new()),
        };
        let vp = cat.get(b"ViewerPreferences").ok().and_then(|v| dict_of(&doc, v));
        p.show_title = flag(vp, b"DisplayDocTitle");
        p.fit_window = flag(vp, b"FitWindow");
        p.center_window = flag(vp, b"CenterWindow");
        p.hide_menubar = flag(vp, b"HideMenubar");
        p.hide_toolbar = flag(vp, b"HideToolbar");
        p.hide_window_ui = flag(vp, b"HideWindowUI");
        if let Some(vp) = vp {
            p.print_no_scaling = name(vp, b"PrintScaling").as_deref() == Some(b"None");
            p.duplex = match name(vp, b"Duplex").as_deref() {
                Some(b"Simplex") => DuplexPreset::Simplex,
                Some(b"DuplexFlipLongEdge") => DuplexPreset::LongEdge,
                Some(b"DuplexFlipShortEdge") => DuplexPreset::ShortEdge,
                _ => DuplexPreset::Default,
            };
            p.pick_tray = flag(Some(vp), b"PickTrayByPDFSize");
            p.copies =
                vp.get(b"NumCopies").ok().and_then(|n| n.as_i64().ok()).map(|n| n.clamp(1, 999) as u32);
            p.right_to_left = name(vp, b"Direction").as_deref() == Some(b"R2L");
            if let Ok(range) = vp.get(b"PrintPageRange").and_then(Object::as_array) {
                let nums: Vec<i64> = range.iter().filter_map(|o| o.as_i64().ok()).collect();
                p.print_range = nums
                    .chunks(2)
                    .filter(|c| c.len() == 2)
                    .map(|c| {
                        if c[0] == c[1] {
                            format!("{}", c[0] + 1)
                        } else {
                            format!("{}-{}", c[0] + 1, c[1] + 1)
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
            }
        }
        p.base_url = cat
            .get(b"URI")
            .ok()
            .and_then(|u| dict_of(&doc, u))
            .and_then(|u| u.get(b"Base").ok())
            .and_then(|b| text(&doc, b))
            .unwrap_or_default();
        p.language = cat.get(b"Lang").ok().and_then(|l| text(&doc, l)).unwrap_or_default();
        if let Some(info) = doc.trailer.get(b"Info").ok().and_then(|i| dict_of(&doc, i)) {
            p.trapped = match info.get(b"Trapped").ok().map(|t| resolve(&doc, t)) {
                Some(Object::Name(n)) if n == b"True" => Trapped::Yes,
                Some(Object::Name(n)) if n == b"False" => Trapped::No,
                Some(Object::Boolean(true)) => Trapped::Yes,
                Some(Object::Boolean(false)) => Trapped::No,
                _ => Trapped::Unknown,
            };
            for (k, v) in info.iter() {
                let key = String::from_utf8_lossy(k).into_owned();
                if !STANDARD_INFO_KEYS.contains(&key.as_str())
                    && let Some(value) = text(&doc, v)
                {
                    p.custom.push((key, value));
                }
            }
        }
        if let Some(aa) = cat.get(b"AA").ok().and_then(|a| dict_of(&doc, a)) {
            for key in ["WC", "WS", "DS", "WP", "DP"] {
                if let Some(js) = aa.get(key.as_bytes()).ok().and_then(|a| js_of(&doc, a)) {
                    p.actions.push((key.into(), js));
                }
            }
        }
        if let Some(tree) = cat
            .get(b"Names")
            .ok()
            .and_then(|n| dict_of(&doc, n))
            .and_then(|n| n.get(b"JavaScript").ok())
            .and_then(|j| dict_of(&doc, j))
        {
            collect_names(&doc, tree, &mut p.javascript, 0);
        }
        p.xmp = cat.get(b"Metadata").ok().and_then(|m| text(&doc, m)).filter(|x| !x.trim().is_empty());
        Ok(p)
    })
}

/// "Alle JavaScripte": every JavaScript of the document as (where, code):
/// document level, document actions, open action, page actions, links and
/// form fields. Only read for display; MrPDF never runs JavaScript.
pub fn all_javascripts(bytes: &[u8], password: Option<&str>) -> Result<Vec<(String, String)>> {
    guarded(|| {
        let doc = load(bytes, password)?;
        let props = read(bytes, password)?;
        let mut out: Vec<(String, String)> = Vec::new();
        for (name, code) in props.javascript {
            out.push((format!("Dokument: {name}"), code));
        }
        for (key, code) in props.actions {
            out.push((format!("Dokumentaktion: {key}"), code));
        }
        let is_js = |a: &Dictionary| name(a, b"S").as_deref() == Some(b"JavaScript");
        let js_text = |obj: &Object| -> Option<String> {
            let a = dict_of(&doc, obj)?;
            if is_js(a) { text(&doc, a.get(b"JS").ok()?) } else { None }
        };
        let cat = doc.get_dictionary(doc.trailer.get(b"Root")?.as_reference()?)?;
        if let Some(code) = cat.get(b"OpenAction").ok().and_then(js_text) {
            out.push(("Beim Öffnen".into(), code));
        }
        for (no, page_id) in doc.get_pages() {
            let Ok(page) = doc.get_dictionary(page_id) else { continue };
            if let Some(aa) = page.get(b"AA").ok().and_then(|a| dict_of(&doc, a)) {
                for (key, label) in [(&b"O"[..], "öffnen"), (b"C", "schließen")] {
                    if let Some(code) = aa.get(key).ok().and_then(js_text) {
                        out.push((format!("Seite {no} {label}"), code));
                    }
                }
            }
            let annots = page.get(b"Annots").ok().and_then(|a| match a {
                Object::Reference(id) => doc.get_object(*id).ok().and_then(|o| o.as_array().ok()).cloned(),
                Object::Array(v) => Some(v.clone()),
                _ => None,
            });
            for annot in annots.unwrap_or_default().iter().take(5000) {
                let Some(d) = dict_of(&doc, annot) else { continue };
                let field = d
                    .get(b"T")
                    .ok()
                    .or_else(|| {
                        d.get(b"Parent").ok().and_then(|p| dict_of(&doc, p)).and_then(|p| p.get(b"T").ok())
                    })
                    .and_then(|t| text(&doc, t))
                    .unwrap_or_default();
                let place = if field.is_empty() {
                    format!("Seite {no}, Verknüpfung")
                } else {
                    format!("Feld „{field}“")
                };
                if let Some(code) = d.get(b"A").ok().and_then(js_text) {
                    out.push((format!("{place}: Mausklick"), code));
                }
                let aa = d
                    .get(b"AA")
                    .ok()
                    .or_else(|| {
                        d.get(b"Parent").ok().and_then(|p| dict_of(&doc, p)).and_then(|p| p.get(b"AA").ok())
                    })
                    .and_then(|a| dict_of(&doc, a));
                if let Some(aa) = aa {
                    for (key, label) in [
                        (&b"K"[..], "Tastenanschlag"),
                        (b"F", "Format"),
                        (b"V", "Prüfen"),
                        (b"C", "Berechnen"),
                        (b"U", "Maus loslassen"),
                        (b"Fo", "Fokus"),
                        (b"Bl", "Fokus verlieren"),
                    ] {
                        if let Some(code) = aa.get(key).ok().and_then(js_text) {
                            out.push((format!("{place}: {label}"), code));
                        }
                    }
                }
                if out.len() > 5000 {
                    break;
                }
            }
        }
        Ok(out)
    })
}

fn collect_names(doc: &Document, node: &Dictionary, out: &mut Vec<(String, String)>, depth: u32) {
    if depth > 16 || out.len() > 1000 {
        return;
    }
    if let Ok(names) = node.get(b"Names").and_then(Object::as_array) {
        for pair in names.chunks(2) {
            if let [key, action] = pair {
                let key = text(doc, key).unwrap_or_default();
                out.push((key, js_of(doc, action).unwrap_or_default()));
            }
        }
    }
    if let Ok(kids) = node.get(b"Kids").and_then(Object::as_array) {
        for kid in kids {
            if let Some(d) = dict_of(doc, kid) {
                collect_names(doc, d, out, depth + 1);
            }
        }
    }
}

/// Parses "1-3, 5" into PDF page pairs (0-based) for /PrintPageRange.
fn range_pairs(text: &str, pages: usize) -> Option<Vec<Object>> {
    let mut out = Vec::new();
    for part in text.split([',', ';']).map(str::trim).filter(|p| !p.is_empty()) {
        let (a, b) = match part.split_once('-') {
            Some((a, b)) => (
                a.trim().parse::<usize>().ok()?,
                if b.trim().is_empty() { pages } else { b.trim().parse::<usize>().ok()? },
            ),
            None => {
                let n = part.parse::<usize>().ok()?;
                (n, n)
            }
        };
        if a == 0 || a > b || b > pages {
            return None;
        }
        out.push(Object::Integer(a as i64 - 1));
        out.push(Object::Integer(b as i64 - 1));
    }
    Some(out)
}

/// Whether "Seitenbereich drucken" is valid for `pages` pages.
pub fn valid_print_range(text: &str, pages: usize) -> bool {
    text.trim().is_empty() || range_pairs(text, pages).is_some_and(|r| !r.is_empty())
}

fn js_action(code: &str) -> Object {
    let mut a = Dictionary::new();
    a.set("S", Object::Name(b"JavaScript".to_vec()));
    a.set("JS", text_string(code));
    Object::Dictionary(a)
}

/// Writes the properties of pages 4 to 8 (unchanged "Eigene" open actions
/// and non-JavaScript document actions are kept).
pub fn write(bytes: &[u8], password: Option<&str>, p: &DocProps) -> Result<Vec<u8>> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let cat_id = edit.prev().trailer.get(b"Root")?.as_reference()?;
        let pages = edit.page_ids();
        let page_count = pages.len();
        let old_vp = {
            let cat = edit.object(cat_id)?.as_dict()?;
            match cat.get(b"ViewerPreferences") {
                Ok(v) => edit.resolve(v)?.as_dict().ok().cloned().unwrap_or_default(),
                Err(_) => Dictionary::new(),
            }
        };
        let old_aa = {
            let cat = edit.object(cat_id)?.as_dict()?;
            match cat.get(b"AA") {
                Ok(v) => edit.resolve(v)?.as_dict().ok().cloned().unwrap_or_default(),
                Err(_) => Dictionary::new(),
            }
        };
        let old_names = {
            let cat = edit.object(cat_id)?.as_dict()?;
            match cat.get(b"Names") {
                Ok(v) => edit.resolve(v)?.as_dict().ok().cloned().unwrap_or_default(),
                Err(_) => Dictionary::new(),
            }
        };
        // Viewer preferences.
        let mut vp = old_vp;
        for (key, on) in [
            ("DisplayDocTitle", p.show_title),
            ("FitWindow", p.fit_window),
            ("CenterWindow", p.center_window),
            ("HideMenubar", p.hide_menubar),
            ("HideToolbar", p.hide_toolbar),
            ("HideWindowUI", p.hide_window_ui),
            ("PickTrayByPDFSize", p.pick_tray),
        ] {
            if on {
                vp.set(key, Object::Boolean(true));
            } else {
                vp.remove(key.as_bytes());
            }
        }
        if p.print_no_scaling {
            vp.set("PrintScaling", Object::Name(b"None".to_vec()));
        } else {
            vp.remove(b"PrintScaling");
        }
        match p.duplex {
            DuplexPreset::Default => {
                vp.remove(b"Duplex");
            }
            d => vp.set(
                "Duplex",
                Object::Name(match d {
                    DuplexPreset::Simplex => b"Simplex".to_vec(),
                    DuplexPreset::LongEdge => b"DuplexFlipLongEdge".to_vec(),
                    _ => b"DuplexFlipShortEdge".to_vec(),
                }),
            ),
        }
        match p.copies {
            Some(n) => vp.set("NumCopies", Object::Integer(i64::from(n.clamp(1, 999)))),
            None => {
                vp.remove(b"NumCopies");
            }
        }
        match range_pairs(&p.print_range, page_count) {
            Some(r) if !r.is_empty() => vp.set("PrintPageRange", Object::Array(r)),
            _ => {
                vp.remove(b"PrintPageRange");
            }
        }
        if p.right_to_left {
            vp.set("Direction", Object::Name(b"R2L".to_vec()));
        } else {
            vp.remove(b"Direction");
        }
        // Document actions: exactly those of `p.actions`; entries shown as
        // "(Type)" are other actions of the document and kept unchanged.
        let mut aa = Dictionary::new();
        for (key, code) in &p.actions {
            if !code.trim().is_empty() && !code.starts_with('(') {
                aa.set(key.clone(), js_action(code));
            } else if code.starts_with('(')
                && let Ok(v) = old_aa.get(key.as_bytes())
            {
                aa.set(key.clone(), v.clone());
            }
        }
        // Document level JavaScript as a flat name tree.
        let mut names = old_names;
        if p.javascript.is_empty() {
            names.remove(b"JavaScript");
        } else {
            let mut sorted = p.javascript.clone();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            let list: Vec<Object> = sorted
                .iter()
                .flat_map(|(n, code)| {
                    [Object::String(n.as_bytes().to_vec(), StringFormat::Literal), js_action(code)]
                })
                .collect();
            let mut tree = Dictionary::new();
            tree.set("Names", Object::Array(list));
            let id = edit.add(Object::Dictionary(tree));
            names.set("JavaScript", Object::Reference(id));
        }
        // Information dictionary: custom entries and trapping.
        let existing = edit.prev().trailer.get(b"Info").ok().cloned();
        let info_id = match existing {
            Some(Object::Reference(id)) => id,
            Some(Object::Dictionary(dict)) => edit.add(Object::Dictionary(dict)),
            _ => edit.add(Object::Dictionary(Dictionary::new())),
        };
        {
            let info = edit.dict_mut(info_id)?;
            let old_custom: Vec<Vec<u8>> = info
                .iter()
                .map(|(k, _)| k.clone())
                .filter(|k| !STANDARD_INFO_KEYS.iter().any(|s| s.as_bytes() == k.as_slice()))
                .collect();
            for k in old_custom {
                info.remove(&k);
            }
            for (k, v) in &p.custom {
                let k = k.trim();
                if valid_custom_key(k) {
                    info.set(k.as_bytes().to_vec(), text_string(v));
                }
            }
            match p.trapped {
                Trapped::Unknown => {
                    info.remove(b"Trapped");
                }
                Trapped::Yes => info.set("Trapped", Object::Name(b"True".to_vec())),
                Trapped::No => info.set("Trapped", Object::Name(b"False".to_vec())),
            }
        }
        edit.inc.new_document.trailer.set("Info", Object::Reference(info_id));
        // Catalog.
        let open_action = match &p.open_action {
            OpenAction::None => None,
            OpenAction::GoTo { page, zoom } => {
                let target = *pages
                    .get((*page).min(page_count.saturating_sub(1)))
                    .ok_or(Error::PageOutOfRange(*page))?;
                let mut dest = vec![Object::Reference(target)];
                match zoom {
                    OpenZoom::Default => {
                        dest.extend([Object::Name(b"XYZ".to_vec()), Object::Null, Object::Null, Object::Null])
                    }
                    OpenZoom::FitPage => dest.push(Object::Name(b"Fit".to_vec())),
                    OpenZoom::FitWidth => dest.extend([Object::Name(b"FitH".to_vec()), Object::Null]),
                    OpenZoom::FitContent => dest.extend([Object::Name(b"FitBH".to_vec()), Object::Null]),
                }
                Some(Some(Object::Array(dest)))
            }
            // Keep whatever the document has.
            OpenAction::Other(_) => Some(None),
        };
        let cat = edit.dict_mut(cat_id)?;
        match (p.fullscreen, p.page_mode.pdf()) {
            (true, _) => cat.set("PageMode", Object::Name(b"FullScreen".to_vec())),
            (false, Some(m)) => cat.set("PageMode", Object::Name(m.to_vec())),
            (false, None) => {
                cat.remove(b"PageMode");
            }
        }
        if p.fullscreen
            && let Some(m) = p.page_mode.pdf()
        {
            vp.set("NonFullScreenPageMode", Object::Name(m.to_vec()));
        }
        match p.layout.pdf() {
            Some(l) => cat.set("PageLayout", Object::Name(l.to_vec())),
            None => {
                cat.remove(b"PageLayout");
            }
        }
        match open_action {
            None => {
                cat.remove(b"OpenAction");
            }
            Some(Some(a)) => cat.set("OpenAction", a),
            Some(None) => {}
        }
        if vp.is_empty() {
            cat.remove(b"ViewerPreferences");
        } else {
            cat.set("ViewerPreferences", Object::Dictionary(vp));
        }
        if aa.is_empty() {
            cat.remove(b"AA");
        } else {
            cat.set("AA", Object::Dictionary(aa));
        }
        if names.is_empty() {
            cat.remove(b"Names");
        } else {
            cat.set("Names", Object::Dictionary(names));
        }
        if p.base_url.trim().is_empty() {
            cat.remove(b"URI");
        } else {
            let mut uri = Dictionary::new();
            uri.set("Base", Object::string_literal(p.base_url.trim()));
            cat.set("URI", Object::Dictionary(uri));
        }
        if p.language.trim().is_empty() {
            cat.remove(b"Lang");
        } else {
            cat.set("Lang", text_string(p.language.trim()));
        }
        edit.finish()
    })
}

/// A custom property name must not be a standard key and must be a valid
/// PDF name.
pub fn valid_custom_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 127
        && !STANDARD_INFO_KEYS.contains(&key)
        && key.chars().all(|c| c.is_ascii_graphic() && !"()<>[]{}/%#".contains(c))
}

/// All fonts of the document ("Schriftarten").
pub fn fonts(bytes: &[u8], password: Option<&str>) -> Result<Vec<FontEntry>> {
    guarded(|| {
        let doc = load(bytes, password)?;
        let mut out: Vec<FontEntry> = Vec::new();
        for obj in doc.objects.values() {
            let Object::Dictionary(d) = obj else { continue };
            if name(d, b"Type").as_deref() != Some(b"Font") {
                continue;
            }
            let subtype = name(d, b"Subtype").unwrap_or_default();
            if matches!(subtype.as_slice(), b"CIDFontType0" | b"CIDFontType2") {
                continue; // listed through their Type0 parent
            }
            let raw = String::from_utf8_lossy(&name(d, b"BaseFont").unwrap_or_default()).into_owned();
            let (font_name, subset) = match raw.split_once('+') {
                Some((tag, rest)) if tag.len() == 6 && tag.chars().all(|c| c.is_ascii_uppercase()) => {
                    (rest.to_string(), true)
                }
                _ => (raw.clone(), false),
            };
            // The descriptor is in the font or (Type0) in its descendant.
            let descendant = d
                .get(b"DescendantFonts")
                .ok()
                .and_then(|a| resolve(&doc, a).as_array().ok())
                .and_then(|a| a.first())
                .and_then(|f| dict_of(&doc, f));
            let desc = d
                .get(b"FontDescriptor")
                .ok()
                .or_else(|| descendant.and_then(|f| f.get(b"FontDescriptor").ok()))
                .and_then(|f| dict_of(&doc, f));
            let embedded =
                desc.is_some_and(|f| f.has(b"FontFile") || f.has(b"FontFile2") || f.has(b"FontFile3"));
            let kind = match subtype.as_slice() {
                b"Type0" => match descendant.and_then(|f| name(f, b"Subtype")).as_deref() {
                    Some(b"CIDFontType2") => "TrueType (CID)".to_string(),
                    _ => "Type 1 (CID)".to_string(),
                },
                b"TrueType" => "TrueType".into(),
                b"Type3" => "Type 3".into(),
                b"MMType1" => "Multiple Master".into(),
                _ => "Type 1".into(),
            };
            let encoding = match d.get(b"Encoding").ok().map(|e| resolve(&doc, e)) {
                Some(Object::Name(n)) => String::from_utf8_lossy(n).into_owned(),
                Some(Object::Dictionary(e)) => {
                    String::from_utf8_lossy(&name(e, b"BaseEncoding").unwrap_or_else(|| b"Custom".to_vec()))
                        .into_owned()
                }
                _ => "Standard".into(),
            };
            let entry = FontEntry { name: font_name, kind, encoding, embedded, subset };
            if !out.contains(&entry) {
                out.push(entry);
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    })
}

/// Adds an XMP stream to a test document.
#[cfg(test)]
fn with_xmp(doc: &mut Document, cat: ObjectId) {
    let xmp = doc.add_object(lopdf::Stream::new(Dictionary::new(), b"<x:xmpmeta>test</x:xmpmeta>".to_vec()));
    doc.get_object_mut(cat).unwrap().as_dict_mut().unwrap().set("Metadata", xmp);
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;

    fn sample() -> Vec<u8> {
        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let font = doc.add_object(dictionary! {"Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica", "Encoding" => "WinAnsiEncoding"});
        let mut kids = Vec::new();
        for _ in 0..3 {
            kids.push(Object::Reference(doc.add_object(
                dictionary! {"Type" => "Page", "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
                "Resources" => dictionary! {"Font" => dictionary! {"F1" => font}}},
            )));
        }
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {"Type" => "Pages", "Kids" => kids, "Count" => 3}),
        );
        let submit = doc.add_object(dictionary! {"S" => "SubmitForm"});
        let cat = doc.add_object(dictionary! {"Type" => "Catalog", "Pages" => pages_id,
        "AA" => dictionary! {"WC" => submit}});
        with_xmp(&mut doc, cat);
        let info = doc.add_object(
            dictionary! {"Title" => Object::string_literal("T"), "Projekt" => Object::string_literal("Alt")},
        );
        doc.trailer.set("Root", cat);
        doc.trailer.set("Info", info);
        let mut out = Vec::new();
        doc.save_to(&mut out).unwrap();
        out
    }

    #[test]
    fn properties_round_trip() {
        let bytes = sample();
        let read0 = read(&bytes, None).unwrap();
        assert_eq!(read0.custom, vec![("Projekt".to_string(), "Alt".to_string())]);
        assert_eq!(read0.actions, vec![("WC".to_string(), "(SubmitForm)".to_string())]);
        assert!(read0.xmp.as_deref().is_some_and(|x| x.contains("xmpmeta")));
        let p = DocProps {
            page_mode: PageMode::UseOutlines,
            layout: LayoutMode::TwoColumnRight,
            open_action: OpenAction::GoTo { page: 2, zoom: OpenZoom::FitWidth },
            show_title: true,
            center_window: true,
            hide_toolbar: true,
            custom: vec![("Kunde".into(), "Müller & Söhne".into()), ("Title".into(), "ignored".into())],
            base_url: "https://example.org/".into(),
            trapped: Trapped::No,
            print_no_scaling: true,
            duplex: DuplexPreset::LongEdge,
            pick_tray: true,
            print_range: "1-2, 3".into(),
            copies: Some(2),
            right_to_left: true,
            language: "de-DE".into(),
            actions: vec![
                ("WC".into(), "(SubmitForm)".into()),
                ("DP".into(), "app.alert('gedruckt');".into()),
            ],
            javascript: vec![("init".into(), "var x = 1;".into())],
            ..Default::default()
        };
        let out = write(&bytes, None, &p).unwrap();
        let back = read(&out, None).unwrap();
        assert_eq!(back.page_mode, PageMode::UseOutlines);
        assert_eq!(back.layout, LayoutMode::TwoColumnRight);
        assert_eq!(back.open_action, OpenAction::GoTo { page: 2, zoom: OpenZoom::FitWidth });
        assert!(back.show_title && back.center_window && back.hide_toolbar && !back.fit_window);
        assert_eq!(back.custom, vec![("Kunde".to_string(), "Müller & Söhne".to_string())]);
        assert_eq!(back.base_url, "https://example.org/");
        assert_eq!(back.trapped, Trapped::No);
        assert!(back.print_no_scaling && back.pick_tray && back.right_to_left);
        assert_eq!(back.duplex, DuplexPreset::LongEdge);
        assert_eq!(back.print_range, "1-2, 3");
        assert_eq!(back.copies, Some(2));
        assert_eq!(back.language, "de-DE");
        assert_eq!(back.actions.len(), 2, "{:?}", back.actions);
        assert!(back.actions.contains(&("DP".to_string(), "app.alert('gedruckt');".to_string())));
        assert!(
            back.actions.contains(&("WC".to_string(), "(SubmitForm)".to_string())),
            "other actions are kept"
        );
        assert_eq!(back.javascript, vec![("init".to_string(), "var x = 1;".to_string())]);
        // Back to defaults removes everything again.
        let cleared = write(&out, None, &DocProps::default()).unwrap();
        let none = read(&cleared, None).unwrap();
        assert_eq!(none.page_mode, PageMode::Default);
        assert_eq!(none.open_action, OpenAction::None);
        assert!(none.custom.is_empty() && none.javascript.is_empty() && none.actions.is_empty());
        assert!(!valid_print_range("4", 3) && valid_print_range("", 3) && valid_print_range("2-", 3));
        assert!(valid_custom_key("Kunde") && !valid_custom_key("Author") && !valid_custom_key("a b"));
    }

    #[test]
    fn font_list() {
        let fonts = fonts(&sample(), None).unwrap();
        assert_eq!(
            fonts,
            vec![FontEntry {
                name: "Helvetica".into(),
                kind: "Type 1".into(),
                encoding: "WinAnsiEncoding".into(),
                embedded: false,
                subset: false
            }]
        );
    }
}
