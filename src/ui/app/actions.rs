//! Commands of the main window (file handling, editing, navigation).

use std::path::{Path, PathBuf};

use egui::Context;

use super::{AfterSave, Dialog, InsertPosition, MrPdfApp, RangePurpose};
use crate::document::Document;
use crate::error::Error;
use crate::model::AnnotKind;
use crate::pdf_edit::{AnnotUpdate, NewAnnotation};
use crate::ui::canvas::CanvasEvent;
use crate::ui::i18n::Strings;
use crate::ui::tab::DocTab;
use crate::ui::tools::Tool;
use crate::ui::view::{self, ZoomMode};

/// Human readable, localised description of an error.
pub fn error_text(t: &Strings, err: &Error) -> String {
    match err {
        Error::EngineUnavailable(_) => t.err_engine.to_string(),
        Error::PasswordRequired => t.err_password_required.to_string(),
        Error::WrongPassword => t.err_wrong_password.to_string(),
        Error::InvalidFormat => t.err_invalid_pdf.to_string(),
        Error::FileTooLarge { .. } => t.err_too_large.to_string(),
        Error::Io { path, source } => match source.kind() {
            std::io::ErrorKind::NotFound => format!("{}\n{}", t.err_not_found, path.display()),
            std::io::ErrorKind::PermissionDenied => format!("{}\n{}", t.err_access_denied, path.display()),
            _ => format!("{} {}\n\n{} {source}", t.err_io, path.display(), t.err_details),
        },
        Error::EncryptedNotSupported => t.err_encrypted.to_string(),
        Error::NotPermitted => t.err_not_permitted.to_string(),
        Error::PageOutOfRange(index) => format!("{} {}", t.err_page_missing, index + 1),
        // Library errors are technical: plain words first, details after.
        Error::Pdfium(detail) | Error::Structure(detail) => {
            format!("{}\n\n{} {detail}", t.err_engine_step, t.err_details)
        }
        other => format!("{}\n{other}", t.err_generic),
    }
}

/// Parses page ranges like `1-3, 5, 8-` (1-based) into zero-based indices.
pub fn parse_page_range(text: &str, page_count: usize) -> Option<Vec<usize>> {
    crate::ui::pickers::parse_pages(text, page_count)
}

fn ensure_pdf_extension(mut path: PathBuf) -> PathBuf {
    if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")) {
        path.set_extension("pdf");
    }
    path
}

/// Only these URI schemes may be opened from documents.
pub fn is_safe_uri(uri: &str) -> bool {
    let lower = uri.trim().to_ascii_lowercase();
    (lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("mailto:"))
        && !uri.chars().any(|c| c.is_control() || c.is_whitespace())
        && uri.len() <= 4096
}

impl MrPdfApp {
    pub(super) fn show_error(&mut self, err: &Error) {
        let t = self.t();
        let text = error_text(t, err);
        // In the error log the first line only (it may name a file).
        log::error!("Fehlermeldung: {}", text.lines().next().unwrap_or_default());
        self.dialog = Some(Dialog::Message { title: t.error.to_string(), text });
    }

    /// After a crash in the last run: where the report is.
    pub fn show_crash_notice(&mut self, report: &std::path::Path) {
        let de = self.settings.language == crate::ui::i18n::Language::German;
        let text = if de {
            format!(
                "MrPDF wurde beim letzten Mal unerwartet beendet. Dafür wurde ein Fehlerbericht gespeichert:\n\n{}\n\nÜber Hilfe › Fehlerprotokoll öffnen finden Sie ihn wieder; Sie können ihn dem Entwickler schicken. Er enthält keine Inhalte Ihrer Dokumente. Nicht gespeicherte Änderungen bietet MrPDF zur Wiederherstellung an, sofern vorhanden.",
                report.display()
            )
        } else {
            format!(
                "MrPDF closed unexpectedly last time. An error report was saved:\n\n{}\n\nHelp › Open error log shows it again; you can send it to the developer. It contains nothing of your documents. Unsaved changes are offered for recovery where available.",
                report.display()
            )
        };
        let title = if de { "Unerwartet beendet" } else { "Closed unexpectedly" };
        self.dialog = Some(Dialog::Message { title: title.into(), text });
    }

    /// Runs an edit on the active tab and refreshes dependent state.
    pub(super) fn edit(&mut self, ctx: &Context, f: impl FnOnce(&mut DocTab) -> crate::Result<()>) {
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        let result = f(tab);
        tab.after_edit();
        if let Err(err) = result {
            self.show_error(&err);
        }
        ctx.request_repaint();
    }

    pub fn open_dialog(&mut self) {
        let t = self.t();
        let de = self.settings.language == crate::ui::i18n::Language::German;
        let mut dialog = rfd::FileDialog::new().set_title(t.open_file);
        let types = super::page_dialogs::OPEN_TYPES;
        let all: Vec<&str> = types.iter().flat_map(|t| t.2.iter().copied()).collect();
        for (i, (german, english, exts)) in types.iter().enumerate() {
            dialog = dialog.add_filter(if de { *german } else { *english }, exts);
            if i == 0 {
                // "PDF-Dokument" first, then all types.
                let label = if de { "Alle unterstützten Dateitypen" } else { "All supported file types" };
                dialog = dialog.add_filter(label, &all);
            }
        }
        if let Some(files) = dialog.pick_files() {
            self.pending_open.extend(files);
        }
    }

    /// Opens a document in a new tab (or activates it if already open).
    /// Other supported files are converted into a new document.
    pub fn open_path(&mut self, ctx: &Context, path: PathBuf, password: Option<String>) {
        if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
            && super::page_dialogs::openable(&path)
        {
            self.import_file(ctx, path);
            return;
        }
        let canonical = crate::files::canonical_path(&path);
        if let Some(i) = self
            .tabs
            .iter()
            .position(|t| t.doc.path().is_some_and(|p| crate::files::canonical_path(p) == canonical))
        {
            self.active = i;
            return;
        }
        match Document::open(&canonical, password.as_deref()) {
            Ok(doc) => {
                self.add_tab(doc);
                self.settings.add_recent(&canonical);
                // "Zuletzt angezeigte Seite wiederherstellen".
                if self.settings.restore_last_page
                    && let Some(page) = self.settings.last_page(&canonical)
                    && let Some(tab) = self.tabs.last_mut()
                    && page > 0
                    && page < tab.doc.page_count()
                {
                    tab.go_to_page(page);
                }
                self.note_signatures(ctx);
                ctx.request_repaint();
            }
            Err(Error::PasswordRequired) => {
                self.dialog =
                    Some(Dialog::Password { path: canonical, password: String::new(), wrong: false });
            }
            Err(Error::WrongPassword) => {
                self.dialog =
                    Some(Dialog::Password { path: canonical, password: String::new(), wrong: true });
            }
            Err(err) => {
                self.settings.recent_files.retain(|p| p != &canonical);
                self.show_error(&err);
            }
        }
    }

    pub fn add_tab(&mut self, doc: Document) {
        let mut tab = DocTab::new(doc, self.settings.zoom_mode, self.settings.layout);
        // A new document opens in the window and tab group of the active one.
        if let Some(active) = self.tabs.get(self.active) {
            tab.window = active.window;
            tab.group = active.group;
        }
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
        // A new or opened document is shown at once (leaves the backstage).
        self.backstage = None;
    }

    pub fn new_document(&mut self, ctx: &Context) {
        let t = self.t();
        match Document::new_blank(t.untitled) {
            Ok(doc) => {
                self.add_tab(doc);
                ctx.request_repaint();
            }
            Err(err) => self.show_error(&err),
        }
    }

    /// Saves the tab with `tab_id` (default: active). Returns true on success.
    pub fn save(&mut self, ctx: &Context, tab_id: Option<u64>) -> bool {
        let index = match tab_id {
            Some(id) => self.tabs.iter().position(|t| t.id == id),
            None => (!self.tabs.is_empty()).then_some(self.active),
        };
        let Some(index) = index else { return false };
        if self.tabs[index].doc.path().is_none() {
            return self.save_as(ctx, Some(self.tabs[index].id));
        }
        match self.tabs[index].doc.save() {
            Ok(()) => {
                let t = self.t();
                let msg = self.saved_message(index, t.saved);
                self.set_status(ctx, msg);
                self.forget_recovery(self.tabs[index].id);
                true
            }
            Err(err) => {
                self.show_error(&err);
                false
            }
        }
    }

    pub fn save_as(&mut self, ctx: &Context, tab_id: Option<u64>) -> bool {
        let index = match tab_id {
            Some(id) => self.tabs.iter().position(|t| t.id == id),
            None => (!self.tabs.is_empty()).then_some(self.active),
        };
        let Some(index) = index else { return false };
        let t = self.t();
        let de = self.settings.language == crate::ui::i18n::Language::German;
        let name = self.tabs[index].doc.display_name().to_string();
        // File types that MrPDF can write; the type is taken
        // from the extension of the chosen name.
        let mut dialog = rfd::FileDialog::new()
            .set_title(t.save_as)
            .add_filter(if de { "PDF-Dokument" } else { "PDF document" }, &["pdf"])
            .add_filter(if de { "Einfache Textdatei" } else { "Plain text file" }, &["txt"])
            .add_filter("MS Word Document", &["docx"])
            .set_file_name(name);
        let doc_dir = self.tabs[index].doc.path().and_then(Path::parent).map(Path::to_path_buf);
        let last_dir = self.settings.last_save_folder.clone().filter(|d| d.is_dir());
        let dir = match self.settings.save_as_folder {
            crate::ui::settings::SaveFolder::Document => doc_dir.or(last_dir),
            crate::ui::settings::SaveFolder::Last => last_dir.or(doc_dir),
        };
        if let Some(dir) = dir {
            dialog = dialog.set_directory(dir);
        }
        let Some(path) = dialog.save_file() else {
            return false;
        };
        self.settings.last_save_folder = path.parent().map(Path::to_path_buf);
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        if ext == "txt" || ext == "docx" {
            let doc = &self.tabs[index].doc;
            let result = if ext == "txt" { doc.export_text(&path) } else { doc.export_word(&path) };
            return match result {
                Ok(()) => {
                    let msg = if de { "Exportiert: " } else { "Exported: " };
                    self.set_status(ctx, format!("{msg}{}", path.display()));
                    true
                }
                Err(err) => {
                    self.show_error(&err);
                    false
                }
            };
        }
        let path = ensure_pdf_extension(path);
        match self.tabs[index].doc.save_as(&path) {
            Ok(()) => {
                self.settings.add_recent(&path);
                let msg = self.saved_message(index, t.saved);
                self.set_status(ctx, msg);
                self.forget_recovery(self.tabs[index].id);
                true
            }
            Err(err) => {
                self.show_error(&err);
                false
            }
        }
    }

    /// Opens a web address of a link or button (asking first, as set in
    /// the options).
    pub(super) fn open_uri(&mut self, ctx: &Context, uri: String) {
        use crate::ui::settings::{LinkPolicy, url_host};
        let trusted = url_host(&uri).is_some_and(|h| self.settings.trusted_hosts.contains(&h));
        if !is_safe_uri(&uri) || self.settings.link_policy == LinkPolicy::Block {
            let t = self.t();
            self.set_status(ctx, t.link_blocked);
        } else if self.settings.link_policy == LinkPolicy::Allow || trusted {
            ctx.open_url(egui::OpenUrl::new_tab(uri));
        } else {
            self.dialog = Some(Dialog::ConfirmLink { uri, trust: false });
        }
    }

    pub fn request_close_tab(&mut self, ctx: &Context, index: usize) {
        let Some(tab) = self.tabs.get(index) else { return };
        if tab.doc.is_modified() {
            self.active = index;
            self.dialog = Some(Dialog::Unsaved { tab_id: tab.id, then: AfterSave::CloseTab });
        } else {
            self.close_tab(ctx, tab.id);
        }
    }

    /// "Alle schließen": asks for every changed document, then closes all.
    pub fn request_close_all(&mut self, ctx: &Context) {
        match self.tabs.iter().position(|t| t.doc.is_modified()) {
            Some(i) => {
                self.active = i;
                self.dialog = Some(Dialog::Unsaved { tab_id: self.tabs[i].id, then: AfterSave::CloseAll });
            }
            None => {
                for id in self.tabs.iter().map(|t| t.id).collect::<Vec<_>>() {
                    self.forget_recovery(id);
                }
                self.tabs.clear();
                self.active = 0;
                ctx.request_repaint();
            }
        }
    }

    /// Remembers the shown page of every open document (for
    /// "Zuletzt angezeigte Seite wiederherstellen").
    pub(super) fn remember_pages(&mut self) {
        for tab in &self.tabs {
            if let Some(path) = tab.doc.path() {
                self.settings.remember_page(path, tab.current_page);
            }
        }
    }

    pub fn close_tab(&mut self, ctx: &Context, tab_id: u64) {
        self.forget_recovery(tab_id);
        if let Some(i) = self.tabs.iter().position(|t| t.id == tab_id) {
            if let Some(path) = self.tabs[i].doc.path().map(Path::to_path_buf) {
                self.settings.remember_page(&path, self.tabs[i].current_page);
                self.remember_closed(path);
            }
            self.tabs.remove(i);
            if self.active >= self.tabs.len() {
                self.active = self.tabs.len().saturating_sub(1);
            }
        }
        ctx.request_repaint();
    }

    /// Continues after the "unsaved changes" question was answered.
    pub fn after_unsaved(&mut self, ctx: &Context, tab_id: u64, then: AfterSave, save: bool) {
        if save && !self.save(ctx, Some(tab_id)) {
            return;
        }
        match then {
            AfterSave::NewWindow => {
                if let Some((_, asked)) = self.new_window.as_mut() {
                    asked.push(tab_id);
                }
                self.continue_new_window(ctx);
            }
            AfterSave::CloseTab => self.close_tab(ctx, tab_id),
            AfterSave::CloseAll => {
                self.close_tab(ctx, tab_id);
                self.request_close_all(ctx);
            }
            AfterSave::Exit => {
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                    // Discard: mark as handled so the next check moves on.
                    if !save {
                        let id = tab.id;
                        self.close_tab(ctx, id);
                    }
                }
                if let Some(next) = self.tabs.iter().find(|t| t.doc.is_modified()) {
                    self.dialog = Some(Dialog::Unsaved { tab_id: next.id, then: AfterSave::Exit });
                } else {
                    self.exit_confirmed = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }

    /// Undoes the last change and names it in the status bar.
    pub fn undo(&mut self, ctx: &Context) {
        let label = self.undo_label(false);
        self.edit(ctx, |tab| tab.doc.undo().map(|_| ()));
        if let Some(label) = label {
            self.set_status(ctx, label);
        }
    }

    pub fn redo(&mut self, ctx: &Context) {
        let label = self.undo_label(true);
        self.edit(ctx, |tab| tab.doc.redo().map(|_| ()));
        if let Some(label) = label {
            self.set_status(ctx, label);
        }
    }

    /// Undoes (or redoes) `n` steps at once, chosen in the history list.
    pub fn travel_history(&mut self, ctx: &Context, n: usize, redo: bool) {
        self.edit(ctx, |tab| if redo { tab.doc.redo_steps(n) } else { tab.doc.undo_steps(n) }.map(|_| ()));
        let de = self.settings.language == crate::ui::i18n::Language::German;
        let text = match (redo, de) {
            (false, true) => format!("{n} Schritte rückgängig gemacht"),
            (false, false) => format!("{n} steps undone"),
            (true, true) => format!("{n} Schritte wiederholt"),
            (true, false) => format!("{n} steps redone"),
        };
        self.set_status(ctx, text);
    }

    pub fn zoom_step(&mut self, zoom_in: bool) {
        if let Some(tab) = self.tab() {
            tab.zoom = if zoom_in { view::zoom_in(tab.zoom) } else { view::zoom_out(tab.zoom) };
            tab.zoom_mode = ZoomMode::Custom;
            let page = tab.current_page;
            tab.pending_scroll = Some(crate::ui::tab::ScrollRequest::Page(page));
        }
    }

    pub fn set_zoom(&mut self, zoom: f32) {
        if let Some(tab) = self.tab() {
            tab.zoom = zoom.clamp(view::MIN_ZOOM, view::MAX_ZOOM);
            tab.zoom_mode = ZoomMode::Custom;
            let page = tab.current_page;
            tab.pending_scroll = Some(crate::ui::tab::ScrollRequest::Page(page));
        }
    }

    /// "An Inhalt anpassen": zooms so the printed area of the current page
    /// (without white margins) fills the window width.
    pub fn fit_content(&mut self) {
        let Some(tab) = self.tab() else { return };
        let page = tab.current_page;
        let Some(geo) = tab.doc.geometry(page).copied() else { return };
        let Ok(m) = tab.doc.white_margins(page, true) else { return };
        let (w, h) = geo.view_size();
        let content_w = (w - m[0] - m[1]).max(20.0);
        let viewport_w = tab.viewport.width().max(100.0) - 24.0;
        tab.zoom = (viewport_w / (content_w * view::PT_TO_SCREEN)).clamp(view::MIN_ZOOM, view::MAX_ZOOM);
        tab.zoom_mode = ZoomMode::Custom;
        // Centre on the middle of the printed area.
        let centre =
            crate::geometry::PointF::new(m[0] + content_w / 2.0, m[2] + (h - m[2] - m[3]).max(0.0) / 2.0);
        tab.pending_scroll = Some(crate::ui::tab::ScrollRequest::Point(page, centre));
    }

    pub fn set_zoom_mode(&mut self, mode: ZoomMode) {
        self.settings.zoom_mode = mode;
        if let Some(tab) = self.tab() {
            tab.zoom_mode = mode;
            let page = tab.current_page;
            tab.pending_scroll = Some(crate::ui::tab::ScrollRequest::Page(page));
        }
    }

    pub fn set_layout(&mut self, layout: view::PageLayout) {
        self.settings.layout = layout;
        if let Some(tab) = self.tab() {
            tab.layout_mode = layout;
            let page = tab.current_page;
            tab.pending_scroll = Some(crate::ui::tab::ScrollRequest::Page(page));
        }
    }

    pub fn copy_selection(&mut self, ctx: &Context) {
        let text = self.tab().and_then(|t| t.selected_text());
        if let Some(text) = text.filter(|t| !t.is_empty()) {
            let permitted = self.active_tab().is_some_and(|t| t.doc.permissions().copy);
            let t = self.t();
            if permitted {
                ctx.copy_text(text);
                self.set_status(ctx, t.copied);
            } else {
                self.set_status(ctx, t.err_not_permitted);
            }
        }
    }

    pub fn select_all_text(&mut self) {
        if let Some(tab) = self.tab() {
            let page = tab.current_page;
            if let Some(text) = tab.page_text(page)
                && !text.is_empty()
            {
                tab.selection = Some(crate::ui::tab::TextSelection {
                    page,
                    range: crate::text::TextRange::new(0, text.chars.len() - 1),
                });
            }
        }
    }

    pub fn delete_selected_annotation(&mut self, ctx: &Context) {
        let selected = self.active_tab().map(|t| t.selected_annots()).unwrap_or_default();
        let Some(first) = selected.first().copied() else {
            return;
        };
        self.edit(ctx, |tab| {
            tab.select_annots(Vec::new());
            if selected.len() == 1 {
                tab.doc.delete_annotation(first.page, first.index)
            } else {
                let indices: Vec<usize> = selected.iter().map(|a| a.index).collect();
                tab.doc.delete_annotations(first.page, &indices)
            }
        });
    }

    /// MrPDF's print dialog ("Drucken").
    pub fn open_print_dialog(&mut self, _ctx: &Context) {
        self.open_print_dialog_for(false);
    }

    /// Creates a text markup annotation from the current text selection.
    pub fn markup_selection(&mut self, ctx: &Context, tool: Tool) {
        let style = self.settings.style(tool);
        let Some(kind) = tool.annot_kind() else { return };
        self.edit(ctx, |tab| {
            let Some(sel) = tab.selection else { return Ok(()) };
            let Some(text) = tab.page_text(sel.page) else { return Ok(()) };
            let mut annot = NewAnnotation::new(kind, style);
            annot.quads = text.line_rects(sel.range);
            annot.contents = text.text_in(sel.range);
            tab.doc.add_annotation(sel.page, &annot)
        });
    }

    pub fn current_pages(&self) -> Vec<usize> {
        self.active_tab().map(|t| vec![t.current_page]).unwrap_or_default()
    }

    pub fn rotate_current(&mut self, ctx: &Context, delta: i32) {
        let pages = self.current_pages();
        self.edit(ctx, |tab| tab.doc.rotate_pages(&pages, delta));
    }

    pub fn move_current_page(&mut self, ctx: &Context, up: bool) {
        self.edit(ctx, |tab| {
            let cur = tab.current_page;
            let n = tab.doc.page_count();
            let target = if up {
                if cur == 0 {
                    return Ok(());
                }
                cur - 1
            } else {
                if cur + 1 >= n {
                    return Ok(());
                }
                cur + 2
            };
            tab.doc.move_pages(&[cur], target)?;
            let new_page = if up { cur - 1 } else { cur + 1 };
            tab.go_to_page(new_page);
            Ok(())
        });
    }

    pub fn move_page_to(&mut self, ctx: &Context, page: usize, to: usize) {
        self.edit(ctx, |tab| {
            tab.doc.move_pages(&[page], to)?;
            let new_page = if to > page { to - 1 } else { to };
            tab.go_to_page(new_page);
            Ok(())
        });
    }

    pub fn insert_from_file(&mut self, ctx: &Context, position: InsertPosition) {
        let t = self.t();
        let Some(path) = rfd::FileDialog::new()
            .set_title(t.insert_from_file)
            .add_filter("PDF", &["pdf", "PDF"])
            .pick_file()
        else {
            return;
        };
        self.edit(ctx, |tab| {
            let at = match position {
                InsertPosition::Before => tab.current_page,
                InsertPosition::After => tab.current_page + 1,
                InsertPosition::End => tab.doc.page_count(),
            };
            let count = tab.doc.insert_pages_from(at, &path, None)?;
            if count > 0 {
                tab.go_to_page(at);
            }
            Ok(())
        });
    }

    pub fn apply_range(&mut self, ctx: &Context, purpose: RangePurpose, pages: Vec<usize>) {
        match purpose {
            RangePurpose::Flatten => self.edit(ctx, |tab| tab.doc.flatten_pages(&pages)),
            RangePurpose::ToImages => {
                self.edit(ctx, |tab| tab.doc.contents_to_images(&pages, 200.0).map(|_| ()))
            }
            RangePurpose::Extract => {
                let t = self.t();
                let Some(tab) = self.active_tab() else { return };
                let stem =
                    tab.doc.display_name().trim_end_matches(".pdf").trim_end_matches(".PDF").to_string();
                let Some(dest) = rfd::FileDialog::new()
                    .set_title(t.extract_pages)
                    .add_filter("PDF", &["pdf"])
                    .set_file_name(format!("{stem}-{}.pdf", t.extract_suffix))
                    .save_file()
                else {
                    return;
                };
                let dest = ensure_pdf_extension(dest);
                let result = self.active_tab().map(|tab| tab.doc.extract_pages(&pages, &dest));
                match result {
                    Some(Ok(())) => {
                        self.set_status(ctx, t.saved);
                        self.pending_open.push_back(dest);
                    }
                    Some(Err(err)) => self.show_error(&err),
                    None => {}
                }
            }
        }
    }

    pub fn handle_canvas_event(&mut self, ctx: &Context, event: CanvasEvent) {
        match event {
            CanvasEvent::Create { page, annot } => {
                // "Neu erstellte Anmerkung automatisch auswählen".
                let select = self.settings.auto_select_new;
                self.edit(ctx, |tab| {
                    tab.doc.add_annotation(page, &annot)?;
                    let index = tab.doc.annotations(page)?.len().saturating_sub(1);
                    tab.selected_annot = select.then_some(crate::ui::tab::AnnotRef { page, index });
                    Ok(())
                });
                if !self.settings.keeps_tool(self.tool) {
                    // Like most editors, return to the hand tool after placing a single object.
                    self.tool = Tool::Hand;
                }
            }
            CanvasEvent::EditText(annot) => {
                let info = self.tab().and_then(|t| t.annotations(annot.page).get(annot.index).cloned());
                if let Some(info) = info {
                    let tool = match info.kind {
                        AnnotKind::Note => Tool::Note,
                        AnnotKind::Caret => Tool::Caret,
                        _ => Tool::TextBox,
                    };
                    self.dialog = Some(Dialog::TextInput {
                        page: annot.page,
                        tool,
                        template: None,
                        text: info.contents,
                        edit: Some(annot),
                    });
                }
            }
            CanvasEvent::Move { annot, dx, dy } => {
                self.edit(ctx, |tab| {
                    // The whole selection moves if the dragged comment is part of it.
                    let selected = tab.selected_annots();
                    let indices: Vec<usize> = if selected.contains(&annot) {
                        selected.iter().map(|a| a.index).collect()
                    } else {
                        vec![annot.index]
                    };
                    tab.doc.move_annotations(annot.page, &indices, dx, dy)
                });
            }
            CanvasEvent::Reshape { annot, reshape, contents } => {
                self.edit(ctx, |tab| {
                    let update = AnnotUpdate { reshape: Some(reshape), contents, ..AnnotUpdate::default() };
                    tab.doc.update_annotation(annot.page, annot.index, &update)
                });
            }
            CanvasEvent::OpenUri(uri) => self.open_uri(ctx, uri),
            CanvasEvent::CommitField { page, annot_index, mut value } => {
                // "Datum": the input is checked and written in the field's format.
                let format = self
                    .tab()
                    .and_then(|t| t.doc.field_extras(page, annot_index).ok())
                    .and_then(|e| e.date_format);
                if let Some(format) = format
                    && !value.trim().is_empty()
                {
                    match crate::forms::normalize_date(&value, &format) {
                        Some(date) => value = date,
                        None => {
                            let de = self.settings.language == crate::ui::i18n::Language::German;
                            let msg = if de {
                                format!("Kein gültiges Datum (Format {format})")
                            } else {
                                format!("Not a valid date (format {format})")
                            };
                            self.set_status(ctx, msg);
                            return;
                        }
                    }
                }
                let unchanged = self
                    .tab()
                    .and_then(|t| {
                        t.fields(page).iter().find(|f| f.annot_index == annot_index).map(|f| f.value == value)
                    })
                    .unwrap_or(false);
                if !unchanged {
                    self.edit(ctx, |tab| tab.doc.set_text_field(page, annot_index, &value));
                }
            }
            CanvasEvent::SetChecked { page, annot_index, checked } => {
                self.edit(ctx, |tab| tab.doc.set_checked(page, annot_index, checked));
            }
            CanvasEvent::ChooseOption { page, annot_index, options, current } => {
                self.dialog = Some(Dialog::Choice { page, annot_index, options, selected: current });
            }
            CanvasEvent::MarkupSelection(tool) => self.markup_selection(ctx, tool),
            CanvasEvent::CopySelection => self.copy_selection(ctx),
            CanvasEvent::DeleteSelected => self.delete_selected_annotation(ctx),
            CanvasEvent::ShowProperties => self.settings.show_properties = true,
            CanvasEvent::Status(text) => self.set_status(ctx, text),
            CanvasEvent::Content(op) => self.content_op(ctx, op),
            other => self.handle_tool_event(ctx, other),
        }
    }

    /// Asks for an image file and places it as stamp on the current page.
    pub fn insert_image(&mut self, ctx: &Context) {
        let t = self.t();
        let Some(path) = rfd::FileDialog::new()
            .set_title(t.insert_image)
            .add_filter(t.images, &super::page_dialogs::IMAGE_EXT)
            .pick_file()
        else {
            return;
        };
        let style = self.settings.style(Tool::Stamp);
        self.edit(ctx, |tab| {
            let page = tab.current_page;
            tab.doc.add_image_stamp(page, &path, &style)?;
            let index = tab.doc.annotations(page)?.len().saturating_sub(1);
            tab.selected_annot = Some(crate::ui::tab::AnnotRef { page, index });
            Ok(())
        });
    }

    pub fn update_selected(&mut self, ctx: &Context, update: AnnotUpdate) {
        let Some(sel) = self.active_tab().and_then(|t| t.selected_annot) else {
            return;
        };
        self.edit(ctx, |tab| tab.doc.update_annotation(sel.page, sel.index, &update));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_ranges() {
        assert_eq!(parse_page_range("1-3, 5", 10), Some(vec![0, 1, 2, 4]));
        assert_eq!(parse_page_range("8-", 10), Some(vec![7, 8, 9]));
        assert_eq!(parse_page_range("-2", 10), Some(vec![0, 1]));
        assert_eq!(parse_page_range("3,3,1", 10), Some(vec![0, 2]));
        assert_eq!(parse_page_range("0", 10), None);
        assert_eq!(parse_page_range("11", 10), None);
        assert_eq!(parse_page_range("5-2", 10), None);
        assert_eq!(parse_page_range("a", 10), None);
        assert_eq!(parse_page_range("", 10), None);
    }

    #[test]
    fn uri_filter() {
        assert!(is_safe_uri("https://example.org/a?b=c"));
        assert!(is_safe_uri("mailto:someone@example.org"));
        assert!(!is_safe_uri("file:///etc/passwd"));
        assert!(!is_safe_uri("javascript:alert(1)"));
        assert!(!is_safe_uri("https://exa mple.org"));
        assert!(!is_safe_uri("C:\\Windows\\system32\\calc.exe"));
    }
}
