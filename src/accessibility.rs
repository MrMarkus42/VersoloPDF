//! Structural accessibility checks (in the spirit of PDF/UA quick checks).

use lopdf::{Document, LoadOptions, Object};

use crate::error::{Error, Result};
use crate::pdf_edit::{MAX_DECOMPRESSED_STREAM, guarded};

/// Facts about the document structure relevant for accessibility.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StructureInfo {
    pub title: bool,
    pub display_title: bool,
    pub language: Option<String>,
    pub tagged: bool,
    pub form_fields: usize,
    pub fields_without_tooltip: usize,
    /// Pages with annotations but without a defined tab order.
    pub pages_without_tab_order: Vec<usize>,
}

fn resolve<'a>(doc: &'a Document, obj: &'a Object) -> Option<&'a Object> {
    match obj {
        Object::Reference(id) => doc.get_object(*id).ok(),
        other => Some(other),
    }
}

fn text(doc: &Document, obj: &Object) -> Option<String> {
    match resolve(doc, obj)? {
        Object::String(s, _) => Some(crate::pdf_edit::decode_text_string(s)),
        _ => None,
    }
}

pub fn check_structure(bytes: &[u8], password: Option<&str>) -> Result<StructureInfo> {
    guarded(|| {
        let mut options = match password {
            Some(pw) => LoadOptions::with_password(pw),
            None => LoadOptions::default(),
        };
        options.max_decompressed_size = Some(MAX_DECOMPRESSED_STREAM);
        let doc = Document::load_mem_with_options(bytes, options)?;
        if doc.is_encrypted() {
            return Err(Error::EncryptedNotSupported);
        }
        let catalog = doc.catalog()?;
        let mut info = StructureInfo::default();

        let info_dict =
            doc.trailer.get(b"Info").ok().and_then(|i| resolve(&doc, i)).and_then(|i| i.as_dict().ok());
        info.title = info_dict
            .and_then(|d| d.get(b"Title").ok())
            .and_then(|t| text(&doc, t))
            .is_some_and(|t| !t.trim().is_empty());
        info.display_title = catalog
            .get(b"ViewerPreferences")
            .ok()
            .and_then(|v| resolve(&doc, v))
            .and_then(|v| v.as_dict().ok())
            .and_then(|v| v.get(b"DisplayDocTitle").ok())
            .and_then(|b| b.as_bool().ok())
            == Some(true);
        info.language =
            catalog.get(b"Lang").ok().and_then(|l| text(&doc, l)).filter(|l| !l.trim().is_empty());
        let marked = catalog
            .get(b"MarkInfo")
            .ok()
            .and_then(|m| resolve(&doc, m))
            .and_then(|m| m.as_dict().ok())
            .and_then(|m| m.get(b"Marked").ok())
            .and_then(|b| b.as_bool().ok())
            == Some(true);
        info.tagged = marked && catalog.has(b"StructTreeRoot");

        for (no, page_id) in doc.get_pages() {
            let Ok(page) = doc.get_dictionary(page_id) else { continue };
            let annots = page
                .get(b"Annots")
                .ok()
                .and_then(|a| resolve(&doc, a))
                .and_then(|a| a.as_array().ok())
                .cloned()
                .unwrap_or_default();
            let mut has_annots = false;
            for a in annots.iter().take(10_000) {
                let Some(d) = resolve(&doc, a).and_then(|o| o.as_dict().ok()) else { continue };
                let subtype = d.get(b"Subtype").ok().and_then(|s| s.as_name().ok());
                if subtype == Some(b"Popup") {
                    continue;
                }
                has_annots = true;
                if subtype == Some(b"Widget") {
                    info.form_fields += 1;
                    // The tooltip may sit on the widget or its parent field.
                    let mut tooltip = d.get(b"TU").ok().and_then(|t| text(&doc, t));
                    if tooltip.is_none()
                        && let Some(parent) = d
                            .get(b"Parent")
                            .ok()
                            .and_then(|p| resolve(&doc, p))
                            .and_then(|p| p.as_dict().ok())
                    {
                        tooltip = parent.get(b"TU").ok().and_then(|t| text(&doc, t));
                    }
                    if tooltip.is_none_or(|t| t.trim().is_empty()) {
                        info.fields_without_tooltip += 1;
                    }
                }
            }
            if has_annots && page.get(b"Tabs").ok().and_then(|t| t.as_name().ok()) != Some(b"S") {
                info.pages_without_tab_order.push(no as usize - 1);
            }
        }
        Ok(info)
    })
}
