//! "Datei" commands on the file itself (Zurücksetzen, Umbenennen, Datei
//! verschieben, In den Papierkorb werfen) and the recovery copies of
//! unsaved documents with the recovery bar.

use std::path::{Path, PathBuf};

use egui::{Context, RichText, Ui};

use super::commands::Cmd;
use super::dialogs::{Action, Outcome, buttons};
use super::{Dialog, MrPdfApp};
use crate::files::{self, RecoveryEntry};
use crate::ui::i18n::Language;

impl MrPdfApp {
    fn fo_de(&self) -> bool {
        self.settings.language == Language::German
    }

    fn active_path(&self) -> Option<PathBuf> {
        self.active_tab().and_then(|t| t.doc.path()).map(Path::to_path_buf)
    }

    /// Folder for recovery copies; `None` switches them off (used by tests).
    pub fn set_recovery_dir(&mut self, dir: Option<PathBuf>) {
        self.recovery_dir = dir;
        self.recovery_offer = None;
    }

    fn recovery_id(tab_id: u64) -> String {
        format!("{}-{tab_id}", std::process::id())
    }

    /// Deletes the recovery copy of a tab (after saving or closing it).
    pub(super) fn forget_recovery(&mut self, tab_id: u64) {
        if let Some(dir) = &self.recovery_dir {
            files::remove_recovery(dir, &Self::recovery_id(tab_id));
        }
    }

    /// Writes recovery copies of all changed documents now.
    pub fn write_recovery_now(&mut self) {
        let Some(dir) = self.recovery_dir.clone() else { return };
        let saved = super::backstage::now_text(self.settings.language);
        let mut failed = None;
        for tab in &mut self.tabs {
            let id = Self::recovery_id(tab.id);
            if !tab.doc.is_modified() {
                files::remove_recovery(&dir, &id);
                continue;
            }
            let entry = RecoveryEntry {
                name: tab.doc.display_name().to_string(),
                original: tab.doc.path().map(Path::to_path_buf),
                saved: saved.clone(),
            };
            let result =
                tab.doc.to_bytes().and_then(|bytes| files::write_recovery(&dir, &id, &entry, &bytes));
            if let Err(err) = result {
                failed = Some(err);
            }
        }
        if let Some(err) = failed {
            log::warn!("recovery copy failed: {err}");
        }
    }

    /// Called every frame: finds old recovery copies once and writes new
    /// ones every few minutes.
    pub(super) fn recovery_tick(&mut self, ctx: &Context) {
        let Some(dir) = self.recovery_dir.clone() else { return };
        if self.recovery_offer.is_none() {
            // Copies of documents open in this window are not "found".
            let own: Vec<String> = self.tabs.iter().map(|t| Self::recovery_id(t.id)).collect();
            let found: Vec<_> =
                files::list_recovery(&dir).into_iter().filter(|(id, _)| !own.contains(id)).collect();
            self.recovery_offer = Some(if self.settings.recovery_bar { found } else { Vec::new() });
        }
        if !self.settings.recovery || self.settings.recovery_minutes == 0 {
            return;
        }
        let now = ctx.input(|i| i.time);
        let interval = f64::from(self.settings.recovery_minutes.min(600)) * 60.0;
        if now - self.last_recovery >= interval {
            self.last_recovery = now;
            self.write_recovery_now();
        }
    }

    /// The bar "Nicht gespeicherte Dokumente gefunden" above the documents.
    pub(super) fn recovery_bar(&mut self, ui: &mut Ui) {
        let Some(offer) = self.recovery_offer.as_ref().filter(|o| !o.is_empty()) else { return };
        let de = self.fo_de();
        let names: Vec<String> = offer.iter().map(|(_, e)| format!("{} ({})", e.name, e.saved)).collect();
        let mut restore = false;
        let mut discard = false;
        egui::Panel::top("recovery-bar")
            .frame(
                egui::Frame::new()
                    .fill(egui::Color32::from_rgb(255, 243, 205))
                    .inner_margin(egui::Margin::symmetric(8, 5)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let text = if de {
                        format!(
                            "{} nicht gespeicherte(s) Dokument(e) einer früheren Sitzung gefunden.",
                            names.len()
                        )
                    } else {
                        format!("Found {} unsaved document(s) of an earlier session.", names.len())
                    };
                    ui.label(RichText::new(text).color(egui::Color32::from_rgb(90, 60, 0)))
                        .on_hover_text(names.join("\n"));
                    restore = ui.button(if de { "Wiederherstellen" } else { "Restore" }).clicked();
                    discard = ui.button(if de { "Verwerfen" } else { "Discard" }).clicked();
                });
            });
        if restore {
            self.restore_recovery(ui.ctx());
        } else if discard {
            self.discard_recovery();
        }
    }

    fn restore_recovery(&mut self, ctx: &Context) {
        let (Some(dir), Some(offer)) = (self.recovery_dir.clone(), self.recovery_offer.take()) else {
            return;
        };
        self.recovery_offer = Some(Vec::new());
        for (id, entry) in offer {
            let result = files::read_recovery(&dir, &id)
                .and_then(|bytes| crate::Document::recovered(bytes, &entry.name, entry.original.as_deref()));
            match result {
                Ok(doc) => {
                    self.add_tab(doc);
                    files::remove_recovery(&dir, &id);
                }
                Err(err) => self.show_error(&err),
            }
        }
        ctx.request_repaint();
    }

    fn discard_recovery(&mut self) {
        if let (Some(dir), Some(offer)) = (&self.recovery_dir, self.recovery_offer.take()) {
            for (id, _) in offer {
                files::remove_recovery(dir, &id);
            }
        }
        self.recovery_offer = Some(Vec::new());
    }

    /// Commands of this module; returns false for other commands.
    pub(super) fn run_file_op(&mut self, ctx: &Context, cmd: Cmd) -> bool {
        let de = self.fo_de();
        let lang = self.settings.language;
        match cmd {
            Cmd::Revert => {
                if self.active_tab().is_some_and(|t| t.doc.is_modified()) {
                    self.dialog = Some(Dialog::Confirm {
                        title: cmd.label(lang).into(),
                        text: if de {
                            "Alle Änderungen verwerfen und die zuletzt gespeicherte Fassung der Datei laden?"
                        } else {
                            "Discard all changes and load the last saved version of the file?"
                        }
                        .into(),
                        cmd,
                    });
                } else {
                    self.revert(ctx);
                }
            }
            Cmd::RenameFile => {
                // The name without ".pdf"; the extension is added again.
                let name = self
                    .active_path()
                    .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
                    .unwrap_or_default();
                self.dialog = Some(Dialog::RenameFile { name, error: None });
            }
            Cmd::MoveFile => self.move_file(ctx),
            Cmd::TrashFile => {
                let name = self.active_tab().map(|t| t.doc.display_name().to_string()).unwrap_or_default();
                self.dialog = Some(Dialog::Confirm {
                    title: cmd.label(lang).into(),
                    text: if de {
                        format!("„{name}“ schließen und die Datei in den Papierkorb verschieben?")
                    } else {
                        format!("Close \"{name}\" and move the file to the recycle bin?")
                    },
                    cmd,
                });
            }
            Cmd::ExportSettings => self.export_settings(ctx),
            Cmd::ImportSettings => self.import_settings(ctx),
            Cmd::ResetSettings => {
                self.dialog = Some(Dialog::Confirm {
                    title: cmd.label(lang).into(),
                    text: if de {
                        "Alle Einstellungen auf die Standardwerte zurücksetzen? Die Liste der zuletzt \
                         verwendeten Dokumente und die Sitzungen bleiben erhalten."
                    } else {
                        "Reset all settings to their defaults? The list of recent documents and the \
                         sessions are kept."
                    }
                    .into(),
                    cmd,
                });
            }
            _ => return false,
        }
        true
    }

    /// After the confirmation of "Zurücksetzen" / "In den Papierkorb werfen"
    /// / "Einstellungen zurücksetzen".
    pub(super) fn run_file_op_confirmed(&mut self, ctx: &Context, cmd: Cmd) -> bool {
        match cmd {
            Cmd::Revert => self.revert(ctx),
            Cmd::TrashFile => self.trash(ctx),
            Cmd::ResetSettings => self.reset_settings(ctx),
            _ => return false,
        }
        true
    }

    fn revert(&mut self, ctx: &Context) {
        let Some(tab) = self.tab() else { return };
        let page = tab.current_page;
        let result = tab.doc.revert();
        tab.selected_annot = None;
        tab.after_edit();
        if result.is_ok() {
            tab.go_to_page(page.min(tab.doc.page_count().saturating_sub(1)));
        }
        let id = tab.id;
        match result {
            Ok(()) => {
                self.forget_recovery(id);
                let de = self.fo_de();
                self.set_status(
                    ctx,
                    if de { "Zuletzt gespeicherte Fassung geladen" } else { "Last saved version loaded" },
                );
            }
            Err(err) => self.show_error(&err),
        }
        ctx.request_repaint();
    }

    /// Updates paths in the recent and pinned lists after a rename or move.
    fn replace_path(&mut self, from: &Path, to: &Path) {
        for list in [&mut self.settings.recent_files, &mut self.settings.pinned_files] {
            for p in list.iter_mut() {
                if p == from {
                    *p = to.to_path_buf();
                }
            }
        }
        self.recent_thumbs.remove(from);
        if let Some(tab) = self.tab() {
            tab.doc.set_file_path(to);
        }
    }

    fn move_file(&mut self, ctx: &Context) {
        let Some(from) = self.active_path() else { return };
        let de = self.fo_de();
        let mut dialog =
            rfd::FileDialog::new().set_title(if de { "Datei verschieben nach" } else { "Move file to" });
        if let Some(dir) = from.parent() {
            dialog = dialog.set_directory(dir);
        }
        let Some(folder) = dialog.pick_folder() else { return };
        match files::move_file(&from, &folder) {
            Ok(to) => {
                self.replace_path(&from, &to);
                let msg = if de { "Verschoben nach " } else { "Moved to " };
                self.set_status(ctx, format!("{msg}{}", folder.display()));
            }
            Err(err) => self.show_error(&err),
        }
    }

    fn trash(&mut self, ctx: &Context) {
        let Some(path) = self.active_path() else { return };
        let Some(id) = self.active_tab().map(|t| t.id) else { return };
        match files::move_to_trash(&path) {
            Ok(()) => {
                self.close_tab(ctx, id);
                self.settings.recent_files.retain(|p| p != &path);
                self.settings.pinned_files.retain(|p| p != &path);
                let de = self.fo_de();
                self.set_status(
                    ctx,
                    if de { "In den Papierkorb verschoben" } else { "Moved to the recycle bin" },
                );
            }
            Err(err) => self.show_error(&err),
        }
    }

    /// The "Umbenennen" dialog.
    pub(super) fn rename_dialog(
        &mut self,
        ui: &mut Ui,
        name: &mut String,
        error: &mut Option<String>,
    ) -> (Outcome, Option<Action>) {
        let de = self.fo_de();
        let t = self.t();
        super::dialogs::set_title(ui, Cmd::RenameFile.label(self.settings.language));
        ui.label(if de { "Neuer Dateiname:" } else { "New file name:" });
        let edit = ui.add(egui::TextEdit::singleline(name).desired_width(360.0));
        // The only input of the dialog keeps the keyboard focus.
        if !edit.has_focus() {
            edit.request_focus();
        }
        if let Some(e) = error {
            ui.colored_label(egui::Color32::from_rgb(200, 40, 40), e.as_str());
        }
        let (ok, cancel) = buttons(ui, t.ok, t.cancel);
        if cancel {
            return (Outcome::Close, None);
        }
        if !ok {
            return (Outcome::Keep, None);
        }
        let Some(from) = self.active_path() else { return (Outcome::Close, None) };
        match files::rename_file(&from, name) {
            Ok(to) => {
                self.replace_path(&from, &to);
                (Outcome::Close, None)
            }
            Err(err) => {
                *error = Some(match err {
                    crate::Error::Invalid(_) => {
                        if de {
                            "Ungültiger Dateiname. Nicht erlaubt sind / \\ : * ? \" < > |".to_string()
                        } else {
                            "Invalid file name. Not allowed: / \\ : * ? \" < > |".to_string()
                        }
                    }
                    other => super::actions::error_text(t, &other),
                });
                (Outcome::Keep, None)
            }
        }
    }
}
