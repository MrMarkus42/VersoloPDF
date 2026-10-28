//! Source list of "Seiten einfügen": the documents open in MrPDF first,
//! then the recently used PDF files.

use egui::{Context, RichText, Ui};

use super::MrPdfApp;
use super::page_dialogs::{InsertSource, file_name, tr};

impl MrPdfApp {
    /// Draws the entries of the source drop-down; returns the clicked one.
    pub(super) fn insert_source_list(
        &self,
        ui: &mut Ui,
        de: bool,
        current: Option<&InsertSource>,
    ) -> Option<InsertSource> {
        let mut chosen = None;
        ui.set_min_width(360.0);
        ui.label(RichText::new(tr(de, "Geöffnete Dokumente", "Open documents")).small().strong());
        for (i, tab) in self.tabs.iter().enumerate() {
            let mut name = tab.doc.display_name().to_string();
            if tab.doc.is_modified() {
                name.push_str(" *");
            }
            if i == self.active {
                name.push_str(tr(de, "  (dieses Dokument)", "  (this document)"));
            }
            let selected = matches!(current, Some(InsertSource::Open(id, _)) if *id == tab.id);
            let resp = ui.selectable_label(selected, name);
            let resp = match tab.doc.path() {
                Some(p) => resp.on_hover_text(p.display().to_string()),
                None => resp,
            };
            if resp.clicked() {
                chosen = Some(InsertSource::Open(tab.id, tab.doc.display_name().to_string()));
            }
        }
        // Recent files that are not open anyway.
        let recent: Vec<_> = self
            .settings
            .recent_files
            .iter()
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")))
            .filter(|p| !self.tabs.iter().any(|t| t.doc.path() == Some(p.as_path())))
            .collect();
        if !recent.is_empty() {
            ui.separator();
            ui.label(RichText::new(tr(de, "Zuletzt verwendete Dateien", "Recent files")).small().strong());
        }
        for path in recent {
            let selected = matches!(current, Some(InsertSource::File(p)) if p == path);
            let folder = path.parent().map(|d| d.display().to_string()).unwrap_or_default();
            let resp = ui.selectable_label(selected, file_name(path)).on_hover_text(folder);
            if resp.clicked() {
                chosen = Some(InsertSource::File(path.clone()));
            }
        }
        chosen
    }

    /// Inserts the pages `sel` (0-based) of `source` before page `at`.
    pub(super) fn insert_from_source(
        &mut self,
        ctx: &Context,
        source: InsertSource,
        at: usize,
        sel: &[usize],
    ) {
        match source {
            InsertSource::File(p) => {
                self.edit(ctx, |tab| tab.doc.insert_pages_selected(at, &p, None, sel).map(|_| ()))
            }
            InsertSource::Open(id, _) => {
                let Some(snapshot) = self.tabs.iter_mut().find(|t| t.id == id).map(|t| t.doc.snapshot())
                else {
                    return;
                };
                match snapshot {
                    Ok((bytes, pw)) => self.edit(ctx, |tab| {
                        tab.doc.insert_pages_from_bytes(at, bytes, pw.as_deref(), sel).map(|_| ())
                    }),
                    Err(err) => self.show_error(&err),
                }
            }
        }
    }
}
