//! Filling in interactive form fields (AcroForm).
//!
//! Values are written to the field dictionary and new appearance streams are
//! generated for the widgets, so the result displays correctly in every
//! viewer without relying on `/NeedAppearances`.

use lopdf::{Dictionary, Object, ObjectId, Stream};

use crate::appearance::{self, StdFont, num, win_ansi};
use crate::error::{Error, Result};
use crate::pdf_edit::{
    Edit, font_size_from_da, guarded, name, number, numbers, read_rect, string, text_string,
};

/// The new value of a form field.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldValue {
    Text(String),
    Checked(bool),
    Choice(String),
}

const FLAG_MULTILINE: u32 = 1 << 12;
const FLAG_PASSWORD: u32 = 1 << 13;
const FLAG_RADIO: u32 = 1 << 15;
const FLAG_PUSHBUTTON: u32 = 1 << 16;
const FLAG_READONLY: u32 = 1;
const MAX_DEPTH: usize = 32;

/// Looks up an inheritable field attribute along the `/Parent` chain.
fn inherited(edit: &Edit, mut id: ObjectId, key: &[u8]) -> Option<Object> {
    for _ in 0..MAX_DEPTH {
        let dict = edit.object(id).ok()?.as_dict().ok()?;
        if let Ok(value) = dict.get(key) {
            return Some(value.clone());
        }
        id = dict.get(b"Parent").ok()?.as_reference().ok()?;
    }
    None
}

fn acroform(edit: &Edit) -> Option<Dictionary> {
    let catalog = edit.prev().catalog().ok()?;
    let obj = catalog.get(b"AcroForm").ok()?;
    edit.resolve(obj).ok()?.as_dict().ok().cloned()
}

/// Splits a default appearance string into font name, size and colour operators.
fn parse_da(da: &str) -> (String, f32, String) {
    let tokens: Vec<&str> = da.split_whitespace().collect();
    let mut font = "Helv".to_string();
    if let Some(i) = tokens.iter().position(|t| *t == "Tf")
        && i >= 2
    {
        font = tokens[i - 2].trim_start_matches('/').to_string();
    }
    let size = font_size_from_da(da).unwrap_or(0.0);
    let mut color = "0 g".to_string();
    if let Some(i) = tokens.iter().position(|t| matches!(*t, "g" | "rg" | "k")) {
        let n = match tokens[i] {
            "g" => 1,
            "rg" => 3,
            _ => 4,
        };
        if i >= n {
            color = tokens[i - n..=i].join(" ");
        }
    }
    (font, size, color)
}

/// Escapes text for a literal string in a content stream.
fn escape(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + 2);
    for &b in bytes {
        if matches!(b, b'(' | b')' | b'\\') {
            out.push(b'\\');
        }
        out.push(b);
    }
    out
}

struct WidgetStyle {
    w: f32,
    h: f32,
    background: Option<[f32; 3]>,
    border: Option<[f32; 3]>,
    border_width: f32,
}

fn widget_style(widget: &Dictionary) -> Option<WidgetStyle> {
    let rect = read_rect(widget)?;
    let mk = match widget.get(b"MK") {
        Ok(Object::Dictionary(d)) => Some(d.clone()),
        _ => None,
    };
    let color = |key: &[u8]| -> Option<[f32; 3]> {
        let v = numbers(mk.as_ref()?.get(key).ok());
        match v.as_slice() {
            [g] => Some([*g, *g, *g]),
            [r, g, b] => Some([*r, *g, *b]),
            [c, m, y, k] => Some([(1.0 - c) * (1.0 - k), (1.0 - m) * (1.0 - k), (1.0 - y) * (1.0 - k)]),
            _ => None,
        }
    };
    let border_width = match widget.get(b"BS") {
        Ok(Object::Dictionary(bs)) => bs.get(b"W").ok().and_then(number).unwrap_or(1.0),
        _ => 1.0,
    };
    Some(WidgetStyle {
        w: rect.width(),
        h: rect.height(),
        background: color(b"BG"),
        border: color(b"BC"),
        border_width,
    })
}

/// Builds a text field / combo box appearance.
#[allow(clippy::too_many_arguments)]
fn text_appearance(
    style: &WidgetStyle,
    da: &str,
    font_ref: Option<Object>,
    value: &str,
    multiline: bool,
    password: bool,
    quadding: i64,
) -> Stream {
    let (font_name, mut size, color) = parse_da(da);
    let shown = if password { "*".repeat(value.chars().count()) } else { value.to_string() };
    let encoded = win_ansi(&shown);
    let metrics = StdFont::Helvetica;
    let (w, h) = (style.w, style.h);
    let pad = 2.0 + style.border_width;
    if size <= 0.0 {
        // Auto size: fit height (single line) and width.
        size = if multiline { 12.0 } else { ((h - 2.0 * pad) * 0.8).clamp(4.0, 24.0) };
        while !multiline && size > 4.0 && metrics.text_width(&encoded, size) > w - 2.0 * pad {
            size -= 0.5;
        }
    }

    let mut out: Vec<u8> = Vec::new();
    let mut push = |s: &str| {
        out.extend_from_slice(s.as_bytes());
        out.push(b'\n');
    };
    if let Some([r, g, b]) = style.background {
        push(&format!("{} {} {} rg 0 0 {} {} re f", num(r), num(g), num(b), num(w), num(h)));
    }
    if let Some([r, g, b]) = style.border {
        let bw = style.border_width;
        push(&format!(
            "{} {} {} RG {} w {} {} {} {} re S",
            num(r),
            num(g),
            num(b),
            num(bw),
            num(bw / 2.0),
            num(bw / 2.0),
            num(w - bw),
            num(h - bw)
        ));
    }
    push("/Tx BMC q");
    push(&format!("{} {} {} {} re W n", num(pad / 2.0), num(pad / 2.0), num(w - pad), num(h - pad)));
    let lines: Vec<Vec<u8>> = if multiline {
        appearance::wrap_text(&shown, metrics, size, (w - 2.0 * pad).max(size))
    } else {
        vec![encoded]
    };
    let mut y = if multiline { h - pad - size * 0.85 } else { (h - size * 0.7) / 2.0 };
    let mut body = Vec::new();
    for line in &lines {
        let tw = metrics.text_width(line, size);
        let x = match quadding {
            1 => (w - tw) / 2.0,
            2 => w - pad - tw,
            _ => pad,
        };
        body.extend_from_slice(
            format!("BT /{font_name} {} Tf {color} {} {} Td (", num(size), num(x), num(y)).as_bytes(),
        );
        body.extend_from_slice(&escape(line));
        body.extend_from_slice(b") Tj ET\n");
        y -= size * 1.15;
        if y < -size {
            break;
        }
    }
    out.extend_from_slice(&body);
    out.extend_from_slice(b"Q EMC\n");

    let mut fonts = Dictionary::new();
    match font_ref {
        Some(f) => fonts.set(font_name.as_bytes().to_vec(), f),
        None => {
            let mut f = Dictionary::new();
            f.set("Type", Object::Name(b"Font".to_vec()));
            f.set("Subtype", Object::Name(b"Type1".to_vec()));
            f.set("BaseFont", Object::Name(b"Helvetica".to_vec()));
            f.set("Encoding", Object::Name(b"WinAnsiEncoding".to_vec()));
            fonts.set(font_name.as_bytes().to_vec(), Object::Dictionary(f));
        }
    }
    let mut resources = Dictionary::new();
    resources.set("Font", Object::Dictionary(fonts));
    let mut dict = Dictionary::new();
    dict.set("Type", Object::Name(b"XObject".to_vec()));
    dict.set("Subtype", Object::Name(b"Form".to_vec()));
    dict.set("BBox", Object::Array(vec![0.into(), 0.into(), Object::Real(w), Object::Real(h)]));
    dict.set("Resources", Object::Dictionary(resources));
    Stream::new(dict, out)
}

/// Name of the "on" appearance state of a check box or radio widget.
fn on_state(widget: &Dictionary) -> String {
    if let Ok(Object::Dictionary(ap)) = widget.get(b"AP")
        && let Ok(Object::Dictionary(n)) = ap.get(b"N")
    {
        for (key, _) in n.iter() {
            if key.as_slice() != b"Off" {
                return String::from_utf8_lossy(key).into_owned();
            }
        }
    }
    "Yes".to_string()
}

/// The widget annotations belonging to a terminal field.
fn widgets_of(edit: &Edit, field_id: ObjectId) -> Vec<ObjectId> {
    let Ok(field) = edit.object(field_id).and_then(|o| Ok(o.as_dict()?)) else {
        return Vec::new();
    };
    let kids: Vec<ObjectId> = match field.get(b"Kids") {
        Ok(obj) => edit
            .resolve(obj)
            .ok()
            .and_then(|o| o.as_array().ok())
            .map(|a| a.iter().filter_map(|k| k.as_reference().ok()).collect())
            .unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    if kids.is_empty() { vec![field_id] } else { kids }
}

/// Sets the value of the field whose widget is annotation `annot_index` on `page`.
pub fn set_field_value(
    bytes: &[u8],
    password: Option<&str>,
    page: usize,
    annot_index: usize,
    value: &FieldValue,
) -> Result<Vec<u8>> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let page_id = edit.page_id(page)?;
        let widget_id = edit.annotation_id(page_id, annot_index)?;
        let widget = edit.object(widget_id)?.as_dict()?.clone();
        if name(&widget, b"Subtype").as_deref() != Some("Widget") {
            return Err(Error::Invalid("not a form field".into()));
        }
        // The terminal field is the widget itself (merged) or its parent.
        let field_id = if widget.has(b"T") || !widget.has(b"Parent") {
            widget_id
        } else {
            widget.get(b"Parent")?.as_reference()?
        };
        let ft = inherited(&edit, field_id, b"FT")
            .and_then(|o| o.as_name().ok().map(|n| String::from_utf8_lossy(n).into_owned()))
            .unwrap_or_default();
        let flags = inherited(&edit, field_id, b"Ff").and_then(|o| number(&o)).unwrap_or(0.0) as u32;
        if flags & FLAG_READONLY != 0 {
            return Err(Error::NotPermitted);
        }
        let form = acroform(&edit);

        match (ft.as_str(), value) {
            ("Tx", FieldValue::Text(text)) => {
                edit.dict_mut(field_id)?.set("V", text_string(text));
                regenerate_text(&mut edit, field_id, form.as_ref(), text, flags)?;
            }
            ("Ch", FieldValue::Choice(choice)) => {
                edit.dict_mut(field_id)?.set("V", text_string(choice));
                regenerate_text(&mut edit, field_id, form.as_ref(), choice, flags & !FLAG_MULTILINE)?;
            }
            ("Btn", FieldValue::Checked(checked)) if flags & FLAG_PUSHBUTTON == 0 => {
                let on = on_state(&widget);
                if flags & FLAG_RADIO != 0 {
                    let group_value = if *checked { on.clone() } else { "Off".into() };
                    edit.dict_mut(field_id)?.set("V", Object::Name(group_value.into_bytes()));
                    for kid in widgets_of(&edit, field_id) {
                        let kid_on = on_state(edit.object(kid)?.as_dict()?);
                        let state = if *checked && kid == widget_id { kid_on } else { "Off".into() };
                        edit.dict_mut(kid)?.set("AS", Object::Name(state.into_bytes()));
                    }
                } else {
                    let state = if *checked { on } else { "Off".into() };
                    edit.dict_mut(field_id)?.set("V", Object::Name(state.clone().into_bytes()));
                    for kid in widgets_of(&edit, field_id) {
                        edit.dict_mut(kid)?.set("AS", Object::Name(state.clone().into_bytes()));
                    }
                }
            }
            _ => return Err(Error::Invalid("value does not match the field type".into())),
        }
        edit.finish()
    })
}

fn regenerate_text(
    edit: &mut Edit,
    field_id: ObjectId,
    form: Option<&Dictionary>,
    text: &str,
    flags: u32,
) -> Result<()> {
    let da = inherited(edit, field_id, b"DA")
        .and_then(|o| match o {
            Object::String(b, _) => Some(String::from_utf8_lossy(&b).into_owned()),
            _ => None,
        })
        .or_else(|| form.and_then(|f| string(f, b"DA")))
        .unwrap_or_else(|| "/Helv 0 Tf 0 g".to_string());
    let (font_name, _, _) = parse_da(&da);
    let font_ref = form
        .and_then(|f| f.get(b"DR").ok())
        .and_then(|dr| edit.resolve(dr).ok().and_then(|d| d.as_dict().ok()).cloned())
        .and_then(|dr| dr.get(b"Font").ok().cloned())
        .and_then(|fonts| edit.resolve(&fonts).ok().and_then(|f| f.as_dict().ok()).cloned())
        .and_then(|fonts| fonts.get(font_name.as_bytes()).ok().cloned());
    let quadding = inherited(edit, field_id, b"Q").and_then(|o| o.as_i64().ok()).unwrap_or(0);
    for widget_id in widgets_of(edit, field_id) {
        let widget = edit.object(widget_id)?.as_dict()?.clone();
        let Some(style) = widget_style(&widget) else { continue };
        let stream = text_appearance(
            &style,
            &da,
            font_ref.clone(),
            text,
            flags & FLAG_MULTILINE != 0,
            flags & FLAG_PASSWORD != 0,
            quadding,
        );
        let stream_id = edit.add(stream);
        let mut ap = Dictionary::new();
        ap.set("N", Object::Reference(stream_id));
        edit.dict_mut(widget_id)?.set("AP", Object::Dictionary(ap));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Creating fields, resetting and bulk values

/// Type of a new form field.
#[derive(Debug, Clone, PartialEq)]
pub enum NewFieldKind {
    Text { multiline: bool },
    CheckBox,
    Radio { group: String, export: String },
    ComboBox { options: Vec<String> },
    ListBox { options: Vec<String> },
    ResetButton { caption: String },
    Signature { label: String },
}

impl NewFieldKind {
    /// Prefix of automatically numbered field names.
    pub fn default_name(&self) -> &'static str {
        match self {
            NewFieldKind::Text { .. } => "Text",
            NewFieldKind::CheckBox => "CheckBox",
            NewFieldKind::Radio { .. } => "RadioGroup",
            NewFieldKind::ComboBox { .. } => "ComboBox",
            NewFieldKind::ListBox { .. } => "ListBox",
            NewFieldKind::ResetButton { .. } => "Button",
            NewFieldKind::Signature { .. } => "Signature",
        }
    }
}

/// Returns the AcroForm dictionary id, creating the form if necessary.
fn ensure_acroform(edit: &mut Edit) -> Result<ObjectId> {
    let catalog_id = edit.prev().trailer.get(b"Root")?.as_reference()?;
    let existing = edit.object(catalog_id)?.as_dict()?.get(b"AcroForm").ok().cloned();
    let form_id = match existing {
        Some(Object::Reference(id)) => id,
        Some(Object::Dictionary(dict)) => edit.add(Object::Dictionary(dict)),
        _ => edit.add(Object::Dictionary(Dictionary::new())),
    };
    edit.dict_mut(catalog_id)?.set("AcroForm", Object::Reference(form_id));
    let form = edit.object(form_id)?.as_dict()?.clone();
    let fields = match form.get(b"Fields") {
        Ok(obj) => edit.resolve(obj)?.as_array().cloned().unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    let mut dr = match form.get(b"DR") {
        Ok(obj) => edit.resolve(obj)?.as_dict().cloned().unwrap_or_default(),
        Err(_) => Dictionary::new(),
    };
    let mut fonts = match dr.get(b"Font") {
        Ok(obj) => edit.resolve(obj)?.as_dict().cloned().unwrap_or_default(),
        Err(_) => Dictionary::new(),
    };
    if !fonts.has(b"Helv") {
        let mut f = Dictionary::new();
        f.set("Type", Object::Name(b"Font".to_vec()));
        f.set("Subtype", Object::Name(b"Type1".to_vec()));
        f.set("BaseFont", Object::Name(b"Helvetica".to_vec()));
        f.set("Encoding", Object::Name(b"WinAnsiEncoding".to_vec()));
        let id = edit.add(Object::Dictionary(f));
        fonts.set("Helv", Object::Reference(id));
    }
    dr.set("Font", Object::Dictionary(fonts));
    let form = edit.dict_mut(form_id)?;
    form.set("Fields", Object::Array(fields));
    form.set("DR", Object::Dictionary(dr));
    if !form.has(b"DA") {
        form.set("DA", Object::string_literal("/Helv 0 Tf 0 g"));
    }
    Ok(form_id)
}

fn form_stream(w: f32, h: f32, content: String, font: bool) -> Stream {
    let mut dict = Dictionary::new();
    dict.set("Type", Object::Name(b"XObject".to_vec()));
    dict.set("Subtype", Object::Name(b"Form".to_vec()));
    dict.set("BBox", Object::Array(vec![0.into(), 0.into(), Object::Real(w), Object::Real(h)]));
    let mut resources = Dictionary::new();
    if font {
        let mut f = Dictionary::new();
        f.set("Type", Object::Name(b"Font".to_vec()));
        f.set("Subtype", Object::Name(b"Type1".to_vec()));
        f.set("BaseFont", Object::Name(b"Helvetica".to_vec()));
        f.set("Encoding", Object::Name(b"WinAnsiEncoding".to_vec()));
        let mut fonts = Dictionary::new();
        fonts.set("Helv", Object::Dictionary(f));
        resources.set("Font", Object::Dictionary(fonts));
    }
    dict.set("Resources", Object::Dictionary(resources));
    Stream::new(dict, content.into_bytes())
}

fn box_border(w: f32, h: f32) -> String {
    format!("1 g 0 0 {} {} re f 0.5 G 1 w 0.5 0.5 {} {} re S\n", num(w), num(h), num(w - 1.0), num(h - 1.0))
}

fn centered_text(w: f32, h: f32, text: &str, size: f32) -> String {
    let bytes = win_ansi(text);
    let tw = StdFont::Helvetica.text_width(&bytes, size);
    let mut s =
        format!("BT /Helv {} Tf 0 g {} {} Td (", num(size), num((w - tw) / 2.0), num((h - size * 0.7) / 2.0));
    s.push_str(&String::from_utf8_lossy(&escape(&bytes)));
    s.push_str(") Tj ET\n");
    s
}

fn unique_field_name(edit: &Edit, form_id: ObjectId, base: &str, numbered: bool) -> Result<String> {
    let names = field_names(edit, form_id)?;
    if numbered {
        return Ok((1..).map(|i| format!("{base}{i}")).find(|n| !names.contains(n)).expect("infinite range"));
    }
    if !names.iter().any(|n| n == base) {
        return Ok(base.to_string());
    }
    Ok((2..).map(|i| format!("{base}{i}")).find(|n| !names.contains(n)).expect("infinite range"))
}

/// Full names of all terminal fields.
fn field_names(edit: &Edit, form_id: ObjectId) -> Result<Vec<String>> {
    Ok(terminal_fields(edit, form_id)?.into_iter().map(|(name, _)| name).collect())
}

/// (full name, field id) of all terminal fields.
fn terminal_fields(edit: &Edit, form_id: ObjectId) -> Result<Vec<(String, ObjectId)>> {
    fn walk(edit: &Edit, id: ObjectId, prefix: &str, depth: usize, out: &mut Vec<(String, ObjectId)>) {
        if depth > MAX_DEPTH || out.len() > 100_000 {
            return;
        }
        let Ok(dict) = edit.object(id).and_then(|o| Ok(o.as_dict()?)) else { return };
        let own = string(dict, b"T").unwrap_or_default();
        let name = match (prefix.is_empty(), own.is_empty()) {
            (true, _) => own.clone(),
            (false, true) => prefix.to_string(),
            (false, false) => format!("{prefix}.{own}"),
        };
        let kids: Vec<ObjectId> = dict
            .get(b"Kids")
            .ok()
            .and_then(|k| edit.resolve(k).ok())
            .and_then(|k| k.as_array().ok())
            .map(|a| a.iter().filter_map(|k| k.as_reference().ok()).collect())
            .unwrap_or_default();
        // Kids that carry a /T are fields; otherwise they are widgets of this field.
        let field_kids: Vec<ObjectId> = kids
            .iter()
            .copied()
            .filter(|k| edit.object(*k).ok().and_then(|o| o.as_dict().ok()).is_some_and(|d| d.has(b"T")))
            .collect();
        if field_kids.is_empty() {
            out.push((name, id));
        } else {
            for k in field_kids {
                walk(edit, k, &name, depth + 1, out);
            }
        }
    }
    let form = edit.object(form_id)?.as_dict()?;
    let fields: Vec<ObjectId> = match form.get(b"Fields") {
        Ok(obj) => edit.resolve(obj)?.as_array()?.iter().filter_map(|f| f.as_reference().ok()).collect(),
        Err(_) => Vec::new(),
    };
    let mut out = Vec::new();
    for f in fields {
        walk(edit, f, "", 0, &mut out);
    }
    Ok(out)
}

fn acroform_id(edit: &Edit) -> Option<ObjectId> {
    let catalog = edit.prev().catalog().ok()?;
    catalog.get(b"AcroForm").ok()?.as_reference().ok()
}

/// Creates a new form field with its widget on `page`. Returns the bytes and
/// the field name actually used (made unique).
pub fn create_field(
    bytes: &[u8],
    password: Option<&str>,
    page: usize,
    rect: crate::geometry::RectF,
    kind: &NewFieldKind,
    name: &str,
) -> Result<(Vec<u8>, String)> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let page_id = edit.page_id(page)?;
        let form_id = ensure_acroform(&mut edit)?;
        let rect = rect.normalized();
        let (w, h) = (rect.width().max(4.0), rect.height().max(4.0));
        let auto = name.trim().is_empty();
        let base = if auto { kind.default_name() } else { name.trim() };

        let mut widget = Dictionary::new();
        widget.set("Type", Object::Name(b"Annot".to_vec()));
        widget.set("Subtype", Object::Name(b"Widget".to_vec()));
        widget.set("Rect", crate::pdf_edit::rect_array(&rect));
        widget.set("P", Object::Reference(page_id));
        widget.set("F", Object::Integer(4));
        let mut mk = Dictionary::new();
        mk.set("BC", Object::Array(vec![Object::Real(0.5), Object::Real(0.5), Object::Real(0.5)]));
        mk.set("BG", Object::Array(vec![Object::Real(1.0), Object::Real(1.0), Object::Real(1.0)]));
        let mut bs = Dictionary::new();
        bs.set("W", Object::Integer(1));
        bs.set("S", Object::Name(b"S".to_vec()));
        widget.set("BS", Object::Dictionary(bs));
        widget.set("DA", Object::string_literal("/Helv 0 Tf 0 g"));

        let used_name = if matches!(kind, NewFieldKind::Radio { .. }) {
            String::new()
        } else {
            unique_field_name(&edit, form_id, base, auto)?
        };
        let mut ap = Dictionary::new();
        match kind {
            NewFieldKind::Text { multiline } => {
                widget.set("FT", Object::Name(b"Tx".to_vec()));
                widget.set("V", text_string(""));
                if *multiline {
                    widget.set("Ff", Object::Integer(i64::from(FLAG_MULTILINE)));
                }
            }
            NewFieldKind::ComboBox { options } | NewFieldKind::ListBox { options } => {
                let combo = matches!(kind, NewFieldKind::ComboBox { .. });
                widget.set("FT", Object::Name(b"Ch".to_vec()));
                if combo {
                    widget.set("Ff", Object::Integer(1 << 17));
                }
                widget.set("Opt", Object::Array(options.iter().map(|o| text_string(o)).collect()));
                if let Some(first) = options.first() {
                    widget.set("V", text_string(first));
                }
                if !combo {
                    // List box: show all options, the first one selected.
                    let size = 10.0;
                    let mut content = box_border(w, h);
                    for (i, o) in options.iter().enumerate() {
                        let y = h - 3.0 - (i as f32 + 1.0) * size * 1.25;
                        if y < 0.0 {
                            break;
                        }
                        if i == 0 {
                            content.push_str(&format!(
                                "0.6 0.75 0.95 rg 1 {} {} {} re f\n",
                                num(y - 2.0),
                                num(w - 2.0),
                                num(size * 1.25)
                            ));
                        }
                        let bytes = escape(&win_ansi(o));
                        content.push_str(&format!("BT /Helv {} Tf 0 g 3 {} Td (", num(size), num(y + 1.0)));
                        content.push_str(&String::from_utf8_lossy(&bytes));
                        content.push_str(") Tj ET\n");
                    }
                    let id = edit.add(form_stream(w, h, content, true));
                    ap.set("N", Object::Reference(id));
                }
            }
            NewFieldKind::CheckBox => {
                widget.set("FT", Object::Name(b"Btn".to_vec()));
                widget.set("V", Object::Name(b"Off".to_vec()));
                widget.set("AS", Object::Name(b"Off".to_vec()));
                let check = format!(
                    "{}0 g 1.5 w 1 J 1 j {} {} m {} {} l {} {} l S\n",
                    box_border(w, h),
                    num(w * 0.22),
                    num(h * 0.5),
                    num(w * 0.42),
                    num(h * 0.25),
                    num(w * 0.8),
                    num(h * 0.78)
                );
                let on = edit.add(form_stream(w, h, check, false));
                let off = edit.add(form_stream(w, h, box_border(w, h), false));
                let mut n = Dictionary::new();
                n.set("Yes", Object::Reference(on));
                n.set("Off", Object::Reference(off));
                ap.set("N", Object::Dictionary(n));
            }
            NewFieldKind::Radio { group, export } => {
                let export = if export.trim().is_empty() {
                    "Option1".to_string()
                } else {
                    export.trim().replace(' ', "_")
                };
                let group =
                    if group.trim().is_empty() { "Gruppe".to_string() } else { group.trim().to_string() };
                let r = w.min(h) / 2.0 - 1.0;
                let k = 0.552_284_8 * r;
                let (cx, cy) = (w / 2.0, h / 2.0);
                let circle = |r: f32, k: f32, op: &str| {
                    format!(
                        "{} {} m {} {} {} {} {} {} c {} {} {} {} {} {} c {} {} {} {} {} {} c {} {} {} {} {} {} c {op}\n",
                        num(cx + r),
                        num(cy),
                        num(cx + r),
                        num(cy + k),
                        num(cx + k),
                        num(cy + r),
                        num(cx),
                        num(cy + r),
                        num(cx - k),
                        num(cy + r),
                        num(cx - r),
                        num(cy + k),
                        num(cx - r),
                        num(cy),
                        num(cx - r),
                        num(cy - k),
                        num(cx - k),
                        num(cy - r),
                        num(cx),
                        num(cy - r),
                        num(cx + k),
                        num(cy - r),
                        num(cx + r),
                        num(cy - k),
                        num(cx + r),
                        num(cy)
                    )
                };
                let base_circle = format!("1 g 0.5 G 1 w {}", circle(r, k, "B"));
                let on_content = format!("{base_circle}0 g {}", circle(r * 0.45, k * 0.45, "f"));
                let on = edit.add(form_stream(w, h, on_content, false));
                let off = edit.add(form_stream(w, h, base_circle, false));
                let mut n = Dictionary::new();
                n.set(export.as_bytes().to_vec(), Object::Reference(on));
                n.set("Off", Object::Reference(off));
                ap.set("N", Object::Dictionary(n));
                widget.set("AS", Object::Name(b"Off".to_vec()));
                // Join an existing radio group of the same name or create it.
                let existing =
                    terminal_fields(&edit, form_id)?.into_iter().find(|(n, _)| *n == group).map(|(_, id)| id);
                let parent = match existing {
                    Some(id) => id,
                    None => {
                        let mut p = Dictionary::new();
                        p.set("FT", Object::Name(b"Btn".to_vec()));
                        p.set("Ff", Object::Integer(i64::from(FLAG_RADIO | (1 << 14))));
                        p.set("T", text_string(&group));
                        p.set("V", Object::Name(b"Off".to_vec()));
                        p.set("Kids", Object::Array(Vec::new()));
                        let id = edit.add(Object::Dictionary(p));
                        if let Ok(Object::Array(fields)) = edit.dict_mut(form_id)?.get_mut(b"Fields") {
                            fields.push(Object::Reference(id));
                        }
                        id
                    }
                };
                widget.set("Parent", Object::Reference(parent));
                widget.set("MK", Object::Dictionary(mk.clone()));
                let ap_obj = Object::Dictionary(ap.clone());
                widget.set("AP", ap_obj);
                let wid = edit.add(Object::Dictionary(widget.clone()));
                if let Ok(Object::Array(kids)) = edit.dict_mut(parent)?.get_mut(b"Kids") {
                    kids.push(Object::Reference(wid));
                } else {
                    edit.dict_mut(parent)?.set("Kids", Object::Array(vec![Object::Reference(wid)]));
                }
                edit.annots_mut(page_id)?.push(Object::Reference(wid));
                return Ok((edit.finish()?, group));
            }
            NewFieldKind::ResetButton { caption } => {
                widget.set("FT", Object::Name(b"Btn".to_vec()));
                widget.set("Ff", Object::Integer(i64::from(FLAG_PUSHBUTTON)));
                mk.set("BG", Object::Array(vec![Object::Real(0.85), Object::Real(0.85), Object::Real(0.85)]));
                mk.set("CA", text_string(caption));
                let mut action = Dictionary::new();
                action.set("S", Object::Name(b"ResetForm".to_vec()));
                widget.set("A", Object::Dictionary(action));
                let content = format!(
                    "0.85 g 0 0 {} {} re f 0.4 G 1 w 0.5 0.5 {} {} re S\n{}",
                    num(w),
                    num(h),
                    num(w - 1.0),
                    num(h - 1.0),
                    centered_text(w, h, caption, (h * 0.5).clamp(6.0, 14.0))
                );
                let id = edit.add(form_stream(w, h, content, true));
                ap.set("N", Object::Reference(id));
            }
            NewFieldKind::Signature { label } => {
                widget.set("FT", Object::Name(b"Sig".to_vec()));
                let mut content = format!(
                    "0.97 0.97 1 rg 0 0 {} {} re f 0.5 G 1 w 0.5 0.5 {} {} re S 0.3 G 8 {} m {} {} l S\n",
                    num(w),
                    num(h),
                    num(w - 1.0),
                    num(h - 1.0),
                    num(h * 0.3),
                    num(w - 8.0),
                    num(h * 0.3)
                );
                let bytes = escape(&win_ansi(label));
                content.push_str(&format!("BT /Helv 7 Tf 0.4 g 8 {} Td (", num(h * 0.3 - 9.0)));
                content.push_str(&String::from_utf8_lossy(&bytes));
                content.push_str(") Tj ET\n");
                let id = edit.add(form_stream(w, h, content, true));
                ap.set("N", Object::Reference(id));
            }
        }
        widget.set("T", text_string(&used_name));
        widget.set("MK", Object::Dictionary(mk));
        if !ap.is_empty() {
            widget.set("AP", Object::Dictionary(ap));
        }
        let wid = edit.add(Object::Dictionary(widget));
        if let Ok(Object::Array(fields)) = edit.dict_mut(form_id)?.get_mut(b"Fields") {
            fields.push(Object::Reference(wid));
        }
        edit.annots_mut(page_id)?.push(Object::Reference(wid));
        // Text-like fields get their (empty) appearance generated.
        if matches!(kind, NewFieldKind::Text { .. } | NewFieldKind::ComboBox { .. }) {
            let form = acroform(&edit);
            let flags =
                if matches!(kind, NewFieldKind::Text { multiline: true }) { FLAG_MULTILINE } else { 0 };
            let value = match kind {
                NewFieldKind::ComboBox { options } => options.first().cloned().unwrap_or_default(),
                _ => String::new(),
            };
            regenerate_text(&mut edit, wid, form.as_ref(), &value, flags)?;
        }
        Ok((edit.finish()?, used_name))
    })
}

/// Current values of all fields as (full name, value).
pub fn field_values(bytes: &[u8], password: Option<&str>) -> Result<Vec<(String, String)>> {
    guarded(|| {
        let edit = Edit::open(bytes, password)?;
        let Some(form_id) = acroform_id(&edit) else { return Ok(Vec::new()) };
        let mut out = Vec::new();
        for (name, id) in terminal_fields(&edit, form_id)? {
            let value = match inherited(&edit, id, b"V") {
                Some(Object::String(b, _)) => crate::pdf_edit::decode_text_string(&b),
                Some(Object::Name(n)) => String::from_utf8_lossy(&n).into_owned(),
                _ => String::new(),
            };
            let ft = inherited(&edit, id, b"FT").and_then(|o| o.as_name().ok().map(|n| n.to_vec()));
            if ft.as_deref() == Some(b"Sig") {
                continue;
            }
            out.push((name, value));
        }
        Ok(out)
    })
}

/// Sets field values by full field name (e.g. from XFDF). Unknown names are
/// ignored; returns the bytes and the number of fields set.
pub fn set_values_by_name(
    bytes: &[u8],
    password: Option<&str>,
    values: &[(String, String)],
) -> Result<(Vec<u8>, usize)> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let Some(form_id) = acroform_id(&edit) else { return Ok((bytes.to_vec(), 0)) };
        let fields = terminal_fields(&edit, form_id)?;
        let form = acroform(&edit);
        let mut count = 0;
        for (name, value) in values {
            let Some((_, field_id)) = fields.iter().find(|(n, _)| n == name) else { continue };
            set_terminal_value(&mut edit, *field_id, form.as_ref(), value)?;
            count += 1;
        }
        Ok((edit.finish()?, count))
    })
}

/// Resets all fields to their default values (`/DV`) or empty.
pub fn reset_form(bytes: &[u8], password: Option<&str>) -> Result<Vec<u8>> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let Some(form_id) = acroform_id(&edit) else { return Ok(bytes.to_vec()) };
        let form = acroform(&edit);
        for (_, id) in terminal_fields(&edit, form_id)? {
            let default = match inherited(&edit, id, b"DV") {
                Some(Object::String(b, _)) => crate::pdf_edit::decode_text_string(&b),
                Some(Object::Name(n)) => String::from_utf8_lossy(&n).into_owned(),
                _ => String::new(),
            };
            set_terminal_value(&mut edit, id, form.as_ref(), &default)?;
        }
        edit.finish()
    })
}

fn set_terminal_value(
    edit: &mut Edit,
    field_id: ObjectId,
    form: Option<&Dictionary>,
    value: &str,
) -> Result<()> {
    let ft = inherited(edit, field_id, b"FT")
        .and_then(|o| o.as_name().ok().map(|n| n.to_vec()))
        .unwrap_or_default();
    let flags = inherited(edit, field_id, b"Ff").and_then(|o| number(&o)).unwrap_or(0.0) as u32;
    if flags & FLAG_READONLY != 0 {
        return Ok(());
    }
    match ft.as_slice() {
        b"Tx" => {
            edit.dict_mut(field_id)?.set("V", text_string(value));
            regenerate_text(edit, field_id, form, value, flags)?;
        }
        b"Ch" => {
            edit.dict_mut(field_id)?.set("V", text_string(value));
            if flags & (1 << 17) != 0 {
                regenerate_text(edit, field_id, form, value, flags & !FLAG_MULTILINE)?;
            }
        }
        b"Btn" if flags & FLAG_PUSHBUTTON == 0 => {
            let kids = widgets_of(edit, field_id);
            let mut chosen = "Off".to_string();
            for kid in &kids {
                let on = on_state(edit.object(*kid)?.as_dict()?);
                let checked = if flags & FLAG_RADIO != 0 {
                    !value.is_empty() && on == value
                } else {
                    !value.is_empty() && value != "Off"
                };
                if checked {
                    chosen = on.clone();
                }
                let state = if checked { on } else { "Off".into() };
                edit.dict_mut(*kid)?.set("AS", Object::Name(state.into_bytes()));
            }
            edit.dict_mut(field_id)?.set("V", Object::Name(chosen.into_bytes()));
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_appearance_is_split() {
        assert_eq!(parse_da("/Helv 12 Tf 0 0 1 rg"), ("Helv".into(), 12.0, "0 0 1 rg".into()));
        assert_eq!(parse_da("0 g /F1 0 Tf"), ("F1".into(), 0.0, "0 g".into()));
        assert_eq!(parse_da(""), ("Helv".into(), 0.0, "0 g".into()));
    }

    #[test]
    fn appearance_contains_escaped_text() {
        let style =
            WidgetStyle { w: 200.0, h: 20.0, background: None, border: Some([0.0; 3]), border_width: 1.0 };
        let s = text_appearance(&style, "/Helv 0 Tf 0 g", None, "a(b)", false, false, 0);
        let content = String::from_utf8_lossy(&s.content);
        assert!(content.contains("(a\\(b\\)) Tj"), "{content}");
        assert!(content.contains("/Tx BMC"));
        let pw = text_appearance(&style, "/Helv 10 Tf 0 g", None, "geheim", false, true, 0);
        assert!(String::from_utf8_lossy(&pw.content).contains("(******)"));
    }
}
