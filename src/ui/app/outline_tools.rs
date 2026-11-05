//! "Lesezeichen" tab: command dispatch, quick case changes, deleting and
//! exporting. The dialogs are in `bookmark_dialogs.rs`.

use egui::Context;

use super::commands::Cmd;
use super::{Dialog, VersoloPdfApp};
use crate::outline::title_case;
use crate::outline_edit::Scope;

impl VersoloPdfApp {
    /// Starts a bookmark command. Returns false if `cmd` is not one.
    pub(super) fn run_outline(&mut self, ctx: &Context, cmd: Cmd) -> bool {
        let de = self.bm_de();
        let lang = self.settings.language;
        // Without a document the commands are disabled in the ribbon.
        if self.active_tab().is_none() {
            return false;
        }
        match cmd {
            Cmd::BookmarkUpper => self.quick_case(ctx, |s| s.to_uppercase()),
            Cmd::BookmarkLower => self.quick_case(ctx, |s| s.to_lowercase()),
            Cmd::BookmarkTitleCase => self.quick_case(ctx, title_case),
            Cmd::BookmarkDeleteAll => {
                let text = if de {
                    "Alle Lesezeichen des Dokuments löschen?"
                } else {
                    "Delete all bookmarks of the document?"
                };
                self.dialog = Some(Dialog::Confirm { title: cmd.label(lang).into(), text: text.into(), cmd });
            }
            Cmd::BookmarkSortPages => self.sort_pages_by_bookmarks(ctx),
            Cmd::BookmarkExportHtml => self.export_bookmarks(ctx, true),
            Cmd::BookmarkExportText => self.export_bookmarks(ctx, false),
            _ => return self.open_bookmark_dialog(cmd),
        }
        true
    }

    fn quick_case(&mut self, ctx: &Context, f: fn(&str) -> String) {
        let de = self.bm_de();
        let result = self.tab().map(|t| t.doc.bookmarks_map_titles(&Scope::all(), |s, _| f(s)));
        self.outline_result(ctx, result, if de { "Geänderte Lesezeichen" } else { "Changed bookmarks" });
    }

    fn export_bookmarks(&mut self, ctx: &Context, html: bool) {
        let Some(tab) = self.active_tab() else { return };
        let stem = tab.doc.display_name().trim_end_matches(".pdf").trim_end_matches(".PDF").to_string();
        let content = tab.doc.export_bookmarks(html);
        let (ext, filter) = if html { ("html", "HTML") } else { ("txt", "Text") };
        let cmd = if html { Cmd::BookmarkExportHtml } else { Cmd::BookmarkExportText };
        let Some(path) = rfd::FileDialog::new()
            .set_title(cmd.label(self.settings.language))
            .add_filter(filter, &[ext])
            .set_file_name(format!("{stem}.{ext}"))
            .save_file()
        else {
            return;
        };
        match crate::document::write_atomic(&path, content.as_bytes()) {
            Ok(()) => {
                let msg = if self.bm_de() { "Exportiert: " } else { "Exported: " };
                self.set_status(ctx, format!("{msg}{}", path.display()));
            }
            Err(err) => self.show_error(&err),
        }
    }
}
