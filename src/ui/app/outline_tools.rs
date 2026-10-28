//! "Lesezeichen" tab: creating bookmarks in bulk, editing all titles,
//! sorting, merging duplicates, table of contents and export.

use egui::{Context, Ui};

use super::commands::Cmd;
use super::dialogs::{Action, Outcome, buttons};
use super::{Dialog, MrPdfApp};
use crate::outline::{NewBookmark, parse_bookmark_list, title_case};
use crate::ui::i18n::Language;

/// Upper bound for bookmark list files.
const MAX_LIST_FILE: u64 = 4 * 1024 * 1024;

impl MrPdfApp {
    fn outline_de(&self) -> bool {
        self.settings.language == Language::German
    }

    /// Starts a bookmark command. Returns false if `cmd` is not one.
    pub(super) fn run_outline(&mut self, ctx: &Context, cmd: Cmd) -> bool {
        let de = self.outline_de();
        let lang = self.settings.language;
        // Without a document the commands are disabled in the ribbon.
        if self.active_tab().is_none() {
            return false;
        }
        match cmd {
            Cmd::BookmarkFromSelection => self.bookmarks_from_text(ctx),
            Cmd::BookmarkEveryNth => {
                let prefix = if de { "Seite " } else { "Page " };
                self.dialog = Some(Dialog::BookmarkEveryNth { every: 1, prefix: prefix.into() });
            }
            Cmd::BookmarkFromHighlights => {
                let result = self.tab().map(|t| t.doc.bookmarks_from_highlights());
                self.outline_status(ctx, result, if de { "Neue Lesezeichen" } else { "New bookmarks" });
            }
            Cmd::BookmarkFromTextFile => self.bookmarks_from_file(ctx),
            Cmd::BookmarkUpper => self.map_titles(ctx, |s| s.to_uppercase()),
            Cmd::BookmarkLower => self.map_titles(ctx, |s| s.to_lowercase()),
            Cmd::BookmarkTitleCase => self.map_titles(ctx, title_case),
            Cmd::BookmarkReplace => {
                self.dialog = Some(Dialog::BookmarkReplace {
                    find: String::new(),
                    replace: String::new(),
                    match_case: false,
                })
            }
            Cmd::BookmarkSort => self.dialog = Some(Dialog::BookmarkSort { by_page: true }),
            Cmd::BookmarkDedupe => {
                let result = self.tab().map(|t| t.doc.remove_duplicate_bookmarks());
                self.outline_status(
                    ctx,
                    result,
                    if de { "Entfernte Duplikate" } else { "Removed duplicates" },
                );
            }
            Cmd::BookmarkDeleteAll => {
                let text = if de {
                    "Alle Lesezeichen des Dokuments löschen?"
                } else {
                    "Delete all bookmarks of the document?"
                };
                self.dialog = Some(Dialog::Confirm { title: cmd.label(lang).into(), text: text.into(), cmd });
            }
            Cmd::BookmarkToc => {
                let heading = if de { "Inhaltsverzeichnis" } else { "Contents" };
                let result = self.tab().map(|t| t.doc.insert_toc(heading));
                self.outline_status(ctx, result, if de { "Eingefügte Seiten" } else { "Inserted pages" });
                if let Some(tab) = self.tab() {
                    tab.jump_to_page(0);
                }
            }
            Cmd::BookmarkExportHtml => self.export_bookmarks(ctx, true),
            Cmd::BookmarkExportText => self.export_bookmarks(ctx, false),
            _ => return self.run_outline_more(ctx, cmd),
        }
        true
    }

    fn outline_status(&mut self, ctx: &Context, result: Option<crate::Result<usize>>, label: &str) {
        if let Some(tab) = self.tab() {
            tab.after_edit();
        }
        match result {
            Some(Ok(n)) => self.set_status(ctx, format!("{label}: {n}")),
            Some(Err(err)) => self.show_error(&err),
            None => {}
        }
        ctx.request_repaint();
    }

    fn map_titles(&mut self, ctx: &Context, f: fn(&str) -> String) {
        let de = self.outline_de();
        let result = self.tab().map(|t| t.doc.map_bookmark_titles(f));
        self.outline_status(ctx, result, if de { "Geänderte Lesezeichen" } else { "Changed bookmarks" });
    }

    /// The selected text becomes a bookmark; without a selection the
    /// headings of the document are used.
    fn bookmarks_from_text(&mut self, ctx: &Context) {
        let de = self.outline_de();
        let Some(tab) = self.tab() else { return };
        let page = tab.current_page;
        let items = match tab.selected_text().map(|t| t.split_whitespace().collect::<Vec<_>>().join(" ")) {
            Some(text) if !text.is_empty() => {
                let page = tab.selection.map_or(page, |s| s.page);
                Ok(vec![NewBookmark { title: text.chars().take(200).collect(), page, level: 0 }])
            }
            _ => tab.doc.heading_bookmarks(),
        };
        let result = items.and_then(|items| tab.doc.append_bookmarks(&items));
        self.outline_status(ctx, Some(result), if de { "Neue Lesezeichen" } else { "New bookmarks" });
        self.side = super::SidePanel::Bookmarks;
        self.settings.show_sidebar = true;
    }

    fn bookmarks_from_file(&mut self, ctx: &Context) {
        let de = self.outline_de();
        let Some(path) = rfd::FileDialog::new()
            .set_title(Cmd::BookmarkFromTextFile.label(self.settings.language))
            .add_filter(if de { "Textdatei" } else { "Text file" }, &["txt"])
            .pick_file()
        else {
            return;
        };
        let text = match std::fs::metadata(&path) {
            Ok(m) if m.len() > MAX_LIST_FILE => {
                Err(crate::Error::FileTooLarge { size: m.len(), limit: MAX_LIST_FILE })
            }
            _ => std::fs::read(&path)
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .map_err(|e| crate::Error::io(&path, e)),
        };
        let result = self.tab().map(|t| {
            let items = parse_bookmark_list(&text?, t.doc.page_count());
            t.doc.append_bookmarks(&items)
        });
        self.outline_status(ctx, result, if de { "Neue Lesezeichen" } else { "New bookmarks" });
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
                let msg = if self.outline_de() { "Exportiert: " } else { "Exported: " };
                self.set_status(ctx, format!("{msg}{}", path.display()));
            }
            Err(err) => self.show_error(&err),
        }
    }

    pub(super) fn outline_dialog(&mut self, ui: &mut Ui, dialog: &mut Dialog) -> (Outcome, Option<Action>) {
        let t = self.t();
        let de = self.outline_de();
        let lang = self.settings.language;
        let mut outcome = Outcome::Keep;
        let mut action: Option<Action> = None;
        match dialog {
            Dialog::BookmarkEveryNth { every, prefix } => {
                super::dialogs::set_title(ui, Cmd::BookmarkEveryNth.label(lang));
                egui::Grid::new("every-nth").num_columns(2).show(ui, |ui| {
                    ui.label(if de { "Alle n Seiten:" } else { "Every n pages:" });
                    ui.add(egui::DragValue::new(every).range(1..=1000));
                    ui.end_row();
                    ui.label(if de { "Titel:" } else { "Title:" });
                    ui.text_edit_singleline(prefix);
                    ui.end_row();
                });
                ui.label(
                    egui::RichText::new(if de {
                        "Die Seitenzahl wird an den Titel angehängt."
                    } else {
                        "The page number is appended to the title."
                    })
                    .weak()
                    .small(),
                );
                let (ok, cancel) = buttons(ui, t.ok, t.cancel);
                if ok {
                    let (every, prefix) = (*every as usize, prefix.clone());
                    action = Some(Box::new(move |app, ctx| {
                        let n = app.active_tab().map_or(0, |t| t.doc.page_count());
                        let items: Vec<NewBookmark> = (0..n)
                            .step_by(every.max(1))
                            .map(|p| NewBookmark { title: format!("{prefix}{}", p + 1), page: p, level: 0 })
                            .collect();
                        let result = app.tab().map(|t| t.doc.append_bookmarks(&items));
                        let label = if app.outline_de() { "Neue Lesezeichen" } else { "New bookmarks" };
                        app.outline_status(ctx, result, label);
                    }));
                    outcome = Outcome::Close;
                } else if cancel {
                    outcome = Outcome::Close;
                }
            }
            Dialog::BookmarkReplace { find, replace, match_case } => {
                super::dialogs::set_title(ui, Cmd::BookmarkReplace.label(lang));
                egui::Grid::new("bm-replace").num_columns(2).show(ui, |ui| {
                    ui.label(if de { "Suchen:" } else { "Find:" });
                    ui.text_edit_singleline(find);
                    ui.end_row();
                    ui.label(if de { "Ersetzen durch:" } else { "Replace with:" });
                    ui.text_edit_singleline(replace);
                    ui.end_row();
                });
                ui.checkbox(match_case, if de { "Groß-/Kleinschreibung beachten" } else { "Match case" });
                let (ok, cancel) = buttons(ui, t.ok, t.cancel);
                if ok && !find.is_empty() {
                    let (find, replace, case) = (find.clone(), replace.clone(), *match_case);
                    action = Some(Box::new(move |app, ctx| {
                        let result = app
                            .tab()
                            .map(|t| t.doc.map_bookmark_titles(|s| replace_text(s, &find, &replace, case)));
                        let label =
                            if app.outline_de() { "Geänderte Lesezeichen" } else { "Changed bookmarks" };
                        app.outline_status(ctx, result, label);
                    }));
                    outcome = Outcome::Close;
                } else if cancel {
                    outcome = Outcome::Close;
                }
            }
            Dialog::BookmarkSort { by_page } => {
                super::dialogs::set_title(ui, Cmd::BookmarkSort.label(lang));
                ui.radio_value(by_page, true, if de { "Nach Seitenzahl" } else { "By page number" });
                ui.radio_value(by_page, false, if de { "Nach Titel (A–Z)" } else { "By title (A–Z)" });
                let (ok, cancel) = buttons(ui, t.ok, t.cancel);
                if ok {
                    let by_page = *by_page;
                    action = Some(Box::new(move |app, ctx| app.edit(ctx, |t| t.doc.sort_bookmarks(by_page))));
                    outcome = Outcome::Close;
                } else if cancel {
                    outcome = Outcome::Close;
                }
            }
            other => return self.outline_more_dialog(ui, other),
        }
        (outcome, action)
    }
}

/// Replaces all occurrences of `find` (optionally ignoring case).
pub fn replace_text(s: &str, find: &str, replace: &str, match_case: bool) -> String {
    if find.is_empty() {
        return s.to_string();
    }
    if match_case {
        return s.replace(find, replace);
    }
    let lower: Vec<char> = s.chars().flat_map(char::to_lowercase).collect();
    let needle: Vec<char> = find.chars().flat_map(char::to_lowercase).collect();
    let chars: Vec<char> = s.chars().collect();
    // Only exact one-to-one lower-casing keeps indices aligned.
    if lower.len() != chars.len() {
        return s.replace(find, replace);
    }
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if lower[i..].starts_with(&needle) {
            out.push_str(replace);
            i += needle.len();
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::replace_text;

    #[test]
    fn replace_ignores_case() {
        assert_eq!(replace_text("Kapitel kapitel KAPITEL", "kapitel", "Teil", false), "Teil Teil Teil");
        assert_eq!(replace_text("Kapitel kapitel", "kapitel", "Teil", true), "Kapitel Teil");
    }
}
