//! "Ebenen": optional content groups (PDF layers, e.g. in plans and maps).
//! Lists them and switches them on or off in the document's default view
//! (/OCProperties /D), which every PDF viewer uses when opening the file.

use lopdf::{Dictionary, Object, ObjectId};

use crate::pdf_edit::{Edit, decode_text_string};
use crate::{Error, Result};

/// One layer of the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layer {
    pub name: String,
    pub visible: bool,
    /// Locked layers (/D /Locked) cannot be switched.
    pub locked: bool,
}

fn refs(edit: &Edit, obj: Option<&Object>) -> Vec<ObjectId> {
    obj.and_then(|o| edit.resolve(o).ok())
        .and_then(|o| o.as_array().ok())
        .map(|a| a.iter().filter_map(|r| r.as_reference().ok()).collect())
        .unwrap_or_default()
}

/// Where /OCProperties lives: (holder, inline?) and its dictionary.
fn properties(edit: &Edit) -> Option<Dictionary> {
    let catalog = edit.prev().catalog().ok()?;
    edit.resolve(catalog.get(b"OCProperties").ok()?).ok()?.as_dict().ok().cloned()
}

fn default_config(edit: &Edit, props: &Dictionary) -> Dictionary {
    props
        .get(b"D")
        .ok()
        .and_then(|d| edit.resolve(d).ok())
        .and_then(|d| d.as_dict().ok())
        .cloned()
        .unwrap_or_default()
}

fn groups(edit: &Edit, props: &Dictionary) -> Vec<ObjectId> {
    refs(edit, props.get(b"OCGs").ok())
}

fn visible_in(edit: &Edit, config: &Dictionary, id: ObjectId) -> bool {
    let base_off = config.get(b"BaseState").and_then(Object::as_name).is_ok_and(|n| n == b"OFF");
    if base_off {
        refs(edit, config.get(b"ON").ok()).contains(&id)
    } else {
        !refs(edit, config.get(b"OFF").ok()).contains(&id)
    }
}

/// The layers of a document (empty if it has none).
pub fn layers(bytes: &[u8], password: Option<&str>) -> Result<Vec<Layer>> {
    let edit = Edit::open(bytes, password)?;
    let Some(props) = properties(&edit) else { return Ok(Vec::new()) };
    let config = default_config(&edit, &props);
    let locked = refs(&edit, config.get(b"Locked").ok());
    Ok(groups(&edit, &props)
        .into_iter()
        .map(|id| {
            let name = edit
                .object(id)
                .ok()
                .and_then(|o| o.as_dict().ok())
                .and_then(|d| d.get(b"Name").ok())
                .and_then(|n| n.as_str().ok())
                .map(decode_text_string)
                .unwrap_or_default();
            Layer { name, visible: visible_in(&edit, &config, id), locked: locked.contains(&id) }
        })
        .collect())
}

/// Shows or hides layer `index` in the default view; returns the new bytes.
pub fn set_visible(bytes: &[u8], password: Option<&str>, index: usize, visible: bool) -> Result<Vec<u8>> {
    let mut edit = Edit::open(bytes, password)?;
    let props = properties(&edit).ok_or_else(|| Error::Invalid("Das Dokument hat keine Ebenen.".into()))?;
    let id =
        *groups(&edit, &props).get(index).ok_or_else(|| Error::Invalid("Ebene nicht vorhanden".into()))?;
    let mut config = default_config(&edit, &props);
    let base_off = config.get(b"BaseState").and_then(Object::as_name).is_ok_and(|n| n == b"OFF");
    // In /ON or /OFF, as the base state needs it.
    for (key, member) in [(&b"OFF"[..], !visible), (&b"ON"[..], visible)] {
        let mut list = refs(&edit, config.get(key).ok());
        list.retain(|r| *r != id);
        if member && (key == b"OFF" || base_off) {
            list.push(id);
        }
        config.set(key.to_vec(), Object::Array(list.into_iter().map(Object::Reference).collect()));
    }
    // The configuration is written where it was (inline or as an object).
    let catalog_id = edit.prev().trailer.get(b"Root")?.as_reference()?;
    let props_ref = edit.object(catalog_id)?.as_dict()?.get(b"OCProperties").ok().cloned();
    let d_ref = props.get(b"D").ok().and_then(|d| d.as_reference().ok());
    if let Some(d) = d_ref {
        *edit.dict_mut(d)? = config;
    } else {
        let mut new_props = props;
        new_props.set("D", Object::Dictionary(config));
        match props_ref {
            Some(Object::Reference(p)) => *edit.dict_mut(p)? = new_props,
            _ => {
                edit.dict_mut(catalog_id)?.set("OCProperties", Object::Dictionary(new_props));
            }
        }
    }
    edit.finish()
}
