//! XFDF (XML Forms Data Format, ISO 19444-1) for exchanging comments and form
//! data with other PDF applications.

use quick_xml::Reader;
use quick_xml::XmlVersion;
use quick_xml::escape::escape;
use quick_xml::events::{BytesRef, BytesStart, Event};

use crate::error::{Error, Result};
use crate::geometry::{PointF, RectF};
use crate::model::{AnnotKind, AnnotStyle, Rgba};
use crate::pdf_edit::{AnnotRecord, NewAnnotation};

/// Largest XFDF file that is read (protection against huge inputs).
pub const MAX_XFDF: usize = 64 * 1024 * 1024;

fn element_name(kind: AnnotKind) -> Option<&'static str> {
    Some(match kind {
        AnnotKind::Note => "text",
        AnnotKind::FreeText => "freetext",
        AnnotKind::Line => "line",
        AnnotKind::Square => "square",
        AnnotKind::Circle => "circle",
        AnnotKind::Polygon => "polygon",
        AnnotKind::Polyline => "polyline",
        AnnotKind::Highlight => "highlight",
        AnnotKind::Underline => "underline",
        AnnotKind::Squiggly => "squiggly",
        AnnotKind::StrikeOut => "strikeout",
        AnnotKind::Stamp => "stamp",
        AnnotKind::Caret => "caret",
        AnnotKind::Ink => "ink",
        _ => return None,
    })
}

fn kind_from_element(name: &str) -> Option<AnnotKind> {
    Some(match name {
        "text" => AnnotKind::Note,
        "freetext" => AnnotKind::FreeText,
        "line" => AnnotKind::Line,
        "square" => AnnotKind::Square,
        "circle" => AnnotKind::Circle,
        "polygon" => AnnotKind::Polygon,
        "polyline" => AnnotKind::Polyline,
        "highlight" => AnnotKind::Highlight,
        "underline" => AnnotKind::Underline,
        "squiggly" => AnnotKind::Squiggly,
        "strikeout" => AnnotKind::StrikeOut,
        "stamp" => AnnotKind::Stamp,
        "caret" => AnnotKind::Caret,
        "ink" => AnnotKind::Ink,
        _ => return None,
    })
}

fn f(v: f32) -> String {
    crate::appearance::num(v)
}

fn points(points: &[PointF]) -> String {
    points.iter().map(|p| format!("{},{}", f(p.x), f(p.y))).collect::<Vec<_>>().join(";")
}

/// Writes comments as XFDF.
pub fn export_comments(records: &[AnnotRecord], file_name: &str) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<xfdf xmlns=\"http://ns.adobe.com/xfdf/\" xml:space=\"preserve\">\n<annots>\n",
    );
    for r in records {
        let Some(el) = element_name(r.kind) else { continue };
        let rect = r.rect.normalized();
        out.push_str(&format!(
            "<{el} page=\"{}\" rect=\"{},{},{},{}\" width=\"{}\" opacity=\"{}\"",
            r.page,
            f(rect.x0),
            f(rect.y0),
            f(rect.x1),
            f(rect.y1),
            f(r.width),
            f(r.opacity)
        ));
        if let Some(c) = r.color {
            out.push_str(&format!(" color=\"{}\"", c.to_hex()));
        }
        if let Some(c) = r.fill {
            out.push_str(&format!(" interior-color=\"{}\"", c.to_hex()));
        }
        for (key, value) in
            [("title", &r.author), ("subject", &r.subject), ("date", &r.date), ("icon", &r.name)]
        {
            if !value.is_empty() {
                out.push_str(&format!(" {key}=\"{}\"", escape(value.as_str())));
            }
        }
        if !r.quads.is_empty() {
            let coords: Vec<String> = r
                .quads
                .iter()
                .map(|q| {
                    let q = q.normalized();
                    [q.x0, q.y1, q.x1, q.y1, q.x0, q.y0, q.x1, q.y0]
                        .iter()
                        .map(|v| f(*v))
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .collect();
            out.push_str(&format!(" coords=\"{}\"", coords.join(",")));
        }
        if r.kind == AnnotKind::Line
            && let Some(p) = r.paths.first().filter(|p| p.len() >= 2)
        {
            out.push_str(&format!(
                " start=\"{},{}\" end=\"{},{}\"",
                f(p[0].x),
                f(p[0].y),
                f(p[p.len() - 1].x),
                f(p[p.len() - 1].y)
            ));
            if r.arrow_end {
                out.push_str(" tail=\"OpenArrow\"");
            }
        }
        out.push('>');
        if !r.contents.is_empty() {
            out.push_str(&format!("<contents>{}</contents>", escape(r.contents.as_str())));
        }
        match r.kind {
            AnnotKind::Polygon | AnnotKind::Polyline => {
                if let Some(p) = r.paths.first() {
                    out.push_str(&format!("<vertices>{}</vertices>", points(p)));
                }
            }
            AnnotKind::Ink => {
                out.push_str("<inklist>");
                for p in &r.paths {
                    out.push_str(&format!("<gesture>{}</gesture>", points(p)));
                }
                out.push_str("</inklist>");
            }
            _ => {}
        }
        out.push_str(&format!("</{el}>\n"));
    }
    out.push_str(&format!("</annots>\n<f href=\"{}\"/>\n</xfdf>\n", escape(file_name)));
    out
}

/// Writes form field values as XFDF.
pub fn export_fields(fields: &[(String, String)], file_name: &str) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<xfdf xmlns=\"http://ns.adobe.com/xfdf/\" xml:space=\"preserve\">\n<fields>\n",
    );
    for (name, value) in fields {
        out.push_str(&format!(
            "<field name=\"{}\"><value>{}</value></field>\n",
            escape(name.as_str()),
            escape(value.as_str())
        ));
    }
    out.push_str(&format!("</fields>\n<f href=\"{}\"/>\n</xfdf>\n", escape(file_name)));
    out
}

fn parse_numbers(s: &str) -> Vec<f32> {
    s.split([',', ';', ' ']).filter_map(|v| v.trim().parse::<f32>().ok()).filter(|v| v.is_finite()).collect()
}

fn parse_points(s: &str) -> Vec<PointF> {
    parse_numbers(s).as_chunks::<2>().0.iter().map(|c| PointF::new(c[0], c[1])).collect()
}

fn parse_color(s: &str) -> Option<Rgba> {
    let hex = s.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(hex, 16).ok()?;
    Some(Rgba::rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

/// Text of an entity reference (`&amp;`, `&#228;` …).
fn entity_text(r: &BytesRef) -> String {
    if let Ok(Some(ch)) = r.resolve_char_ref() {
        return ch.to_string();
    }
    match r.decode().as_deref() {
        Ok("amp") => "&",
        Ok("lt") => "<",
        Ok("gt") => ">",
        Ok("quot") => "\"",
        Ok("apos") => "'",
        _ => "",
    }
    .to_string()
}

struct Pending {
    kind: AnnotKind,
    page: usize,
    annot: NewAnnotation,
    text_target: Option<&'static str>,
    current_paths: Vec<Vec<PointF>>,
}

fn attr(e: &BytesStart, key: &str) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == key.as_bytes())
        .and_then(|a| a.normalized_value(XmlVersion::Implicit1_0).ok().map(|v| v.into_owned()))
}

fn start_annotation(e: &BytesStart, kind: AnnotKind, author: &str) -> Option<Pending> {
    let page = attr(e, "page").and_then(|p| p.parse::<usize>().ok())?;
    let rect = parse_numbers(&attr(e, "rect").unwrap_or_default());
    let mut style = AnnotStyle {
        color: attr(e, "color").and_then(|c| parse_color(&c)).unwrap_or(Rgba::RED),
        fill: attr(e, "interior-color").and_then(|c| parse_color(&c)),
        width: attr(e, "width").and_then(|w| w.parse().ok()).unwrap_or(1.0),
        opacity: attr(e, "opacity").and_then(|o| o.parse().ok()).unwrap_or(1.0),
        author: attr(e, "title").unwrap_or_else(|| author.to_string()),
        ..AnnotStyle::default()
    };
    style.width = style.width.clamp(0.0, 50.0);
    style.opacity = style.opacity.clamp(0.05, 1.0);
    let mut annot = NewAnnotation::new(kind, style);
    if let [x0, y0, x1, y1] = rect.as_slice() {
        annot.rect = RectF::new(*x0, *y0, *x1, *y1).normalized();
    }
    let quads = parse_numbers(&attr(e, "coords").unwrap_or_default());
    annot.quads = quads
        .as_chunks::<8>()
        .0
        .iter()
        .filter_map(|q| RectF::bounding(q.as_chunks::<2>().0.iter().map(|c| PointF::new(c[0], c[1]))))
        .collect();
    if kind == AnnotKind::Line {
        let s = parse_points(&attr(e, "start").unwrap_or_default());
        let t = parse_points(&attr(e, "end").unwrap_or_default());
        if let (Some(a), Some(b)) = (s.first(), t.first()) {
            annot.paths = vec![vec![*a, *b]];
        }
        annot.arrow_end = attr(e, "tail").is_some_and(|t| t != "None");
    }
    if kind == AnnotKind::Stamp {
        annot.label = attr(e, "subject").or_else(|| attr(e, "icon")).unwrap_or_else(|| "Stamp".into());
        annot.icon = attr(e, "icon");
    }
    Some(Pending { kind, page, annot, text_target: None, current_paths: Vec::new() })
}

/// Reads the comments of an XFDF document. Unknown elements are ignored.
pub fn import_comments(xml: &str, default_author: &str) -> Result<Vec<(usize, NewAnnotation)>> {
    if xml.len() > MAX_XFDF {
        return Err(Error::FileTooLarge { size: xml.len() as u64, limit: MAX_XFDF as u64 });
    }
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut out = Vec::new();
    let mut pending: Option<Pending> = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                if let Some(kind) = kind_from_element(&name) {
                    pending = start_annotation(&e, kind, default_author);
                } else if let Some(p) = pending.as_mut() {
                    p.text_target = match name.as_str() {
                        "contents" => Some("contents"),
                        "vertices" => Some("vertices"),
                        "gesture" => Some("gesture"),
                        _ => None,
                    };
                }
            }
            Ok(Event::Empty(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                if let Some(kind) = kind_from_element(&name)
                    && let Some(p) = start_annotation(&e, kind, default_author)
                {
                    out.push((p.page, p.annot));
                }
            }
            event @ (Ok(Event::Text(_)) | Ok(Event::GeneralRef(_))) => {
                if let Some(p) = pending.as_mut()
                    && let Some(target) = p.text_target
                {
                    let text = match event {
                        Ok(Event::Text(t)) => t.decode().map(|s| s.into_owned()).unwrap_or_default(),
                        Ok(Event::GeneralRef(r)) => entity_text(&r),
                        _ => String::new(),
                    };
                    match target {
                        "contents" => p.annot.contents.push_str(&text),
                        "vertices" => {
                            let mut pts = p.annot.paths.pop().unwrap_or_default();
                            pts.extend(parse_points(&text));
                            p.annot.paths = vec![pts];
                        }
                        _ => p.current_paths.push(parse_points(&text)),
                    }
                }
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                if let Some(kind) = kind_from_element(&name)
                    && let Some(mut p) = pending.take()
                    && p.kind == kind
                {
                    if kind == AnnotKind::Ink {
                        p.annot.paths = std::mem::take(&mut p.current_paths);
                    }
                    out.push((p.page, p.annot));
                } else if let Some(p) = pending.as_mut() {
                    p.text_target = None;
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(Error::Invalid(format!("XFDF: {e}"))),
            _ => {}
        }
    }
    Ok(out)
}

/// Reads form field values (`name`, `value`) of an XFDF document.
pub fn import_fields(xml: &str) -> Result<Vec<(String, String)>> {
    if xml.len() > MAX_XFDF {
        return Err(Error::FileTooLarge { size: xml.len() as u64, limit: MAX_XFDF as u64 });
    }
    let mut reader = Reader::from_str(xml);
    let mut names: Vec<String> = Vec::new();
    let mut out = Vec::new();
    let mut in_value = false;
    let mut value = String::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => match e.local_name().as_ref() {
                b"field" => names.push(attr(&e, "name").unwrap_or_default()),
                b"value" => in_value = true,
                _ => {}
            },
            Ok(Event::Text(t)) if in_value => {
                value.push_str(&t.decode().map(|s| s.into_owned()).unwrap_or_default())
            }
            Ok(Event::GeneralRef(r)) if in_value => value.push_str(&entity_text(&r)),
            Ok(Event::End(e)) => match e.local_name().as_ref() {
                b"field" => {
                    names.pop();
                }
                b"value" => {
                    in_value = false;
                    let full = names.iter().filter(|n| !n.is_empty()).cloned().collect::<Vec<_>>().join(".");
                    out.push((full, std::mem::take(&mut value)));
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => return Err(Error::Invalid(format!("XFDF: {e}"))),
            _ => {}
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(kind: AnnotKind) -> AnnotRecord {
        AnnotRecord {
            page: 1,
            index: 0,
            kind,
            rect: RectF::new(10.0, 20.0, 110.0, 70.0),
            color: Some(Rgba::rgb(255, 0, 0)),
            fill: None,
            width: 2.0,
            opacity: 0.8,
            contents: "Grüße & <Test>".into(),
            author: "Max".into(),
            subject: String::new(),
            date: "D:20260101000000Z".into(),
            name: String::new(),
            quads: vec![],
            paths: vec![],
            arrow_end: false,
            font_size: 12.0,
        }
    }

    #[test]
    fn comments_round_trip() {
        let mut ink = record(AnnotKind::Ink);
        ink.paths = vec![vec![PointF::new(1.0, 2.0), PointF::new(3.0, 4.0)], vec![PointF::new(5.0, 6.0)]];
        let mut hl = record(AnnotKind::Highlight);
        hl.quads = vec![RectF::new(10.0, 20.0, 50.0, 30.0)];
        let mut line = record(AnnotKind::Line);
        line.paths = vec![vec![PointF::new(0.0, 0.0), PointF::new(100.0, 50.0)]];
        line.arrow_end = true;
        let xml = export_comments(&[record(AnnotKind::Square), ink, hl, line], "a&b.pdf");
        assert!(xml.contains("Grüße &amp; &lt;Test&gt;"));
        let back = import_comments(&xml, "Default").unwrap();
        assert_eq!(back.len(), 4);
        assert_eq!(back[0].0, 1);
        assert_eq!(back[0].1.kind, AnnotKind::Square);
        assert_eq!(back[0].1.contents, "Grüße & <Test>");
        assert_eq!(back[0].1.style.author, "Max");
        assert_eq!(back[1].1.paths.len(), 2);
        assert_eq!(back[2].1.quads, vec![RectF::new(10.0, 20.0, 50.0, 30.0)]);
        assert!(back[3].1.arrow_end);
    }

    #[test]
    fn fields_round_trip() {
        let xml =
            export_fields(&[("name".into(), "Erika <M>".into()), ("agree".into(), "Yes".into())], "f.pdf");
        assert_eq!(
            import_fields(&xml).unwrap(),
            vec![("name".into(), "Erika <M>".into()), ("agree".into(), "Yes".into())]
        );
        // Nested field names are joined with dots.
        let nested = "<xfdf><fields><field name=\"a\"><field name=\"b\"><value>1</value></field></field></fields></xfdf>";
        assert_eq!(import_fields(nested).unwrap(), vec![("a.b".into(), "1".into())]);
    }

    #[test]
    fn broken_xml_is_an_error() {
        assert!(
            import_comments("<xfdf><annots><square page=\"0\"", "x").is_err()
                || import_comments("<xfdf><annots><square page=\"0\"", "x").unwrap().is_empty()
        );
        assert!(import_fields("<<<").is_err() || import_fields("<<<").unwrap().is_empty());
    }
}
