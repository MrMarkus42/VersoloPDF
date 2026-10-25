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
