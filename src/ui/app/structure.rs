//! "Dokumentaufbau": the commands not covered by the dialog modules
//! (merging files, swapping pages, the duplicate pages result) and shared
//! helpers. The page dialogs live in `page_dialogs`, `layout_dialogs`,
//! `shape_dialogs`, `marks_dialogs` and `labels_dialog`.

use egui::{Context, Ui};

use super::commands::Cmd;
use super::dialogs::{Action, Outcome, buttons};
use super::{Dialog, MrPdfApp};
use crate::ui::i18n::Language;

/// The four kinds of page marks (see `marks_dialogs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MarkKind {
    Watermark,
    Background,
    HeaderFooter,
    Bates,
}

/// Today's date for headers/footers and file names.
pub(super) fn today(de: bool) -> String {
    let d = crate::pdf_edit::pdf_date_now();
    let digits: String = d.chars().filter(|c| c.is_ascii_digit()).take(8).collect();
    if digits.len() < 8 {
        return String::new();
    }
    let (y, m, day) = (&digits[0..4], &digits[4..6], &digits[6..8]);
    if de { format!("{day}.{m}.{y}") } else { format!("{y}-{m}-{day}") }
}

impl MrPdfApp {
    fn de(&self) -> bool {
        self.settings.language == Language::German
    }

    /// Starts a remaining "Dokumentaufbau" command. Returns false otherwise.
    pub(super) fn run_structure(&mut self, ctx: &Context, cmd: Cmd) -> bool {
        let n = self.active_tab().map_or(0, |t| t.doc.page_count());
        let cur = self.active_tab().map_or(0, |t| t.current_page);
        match cmd {
            Cmd::MergeFiles => self.merge_files(ctx),
            Cmd::SwapPages => {
                self.dialog = Some(Dialog::Swap { a: cur as u32 + 1, b: (cur as u32 + 2).min(n as u32) })
            }
            _ => return false,
        }
        true
    }

    fn merge_files(&mut self, ctx: &Context) {
        let title = Cmd::MergeFiles.label(self.settings.language);
        let Some(files) =
            rfd::FileDialog::new().set_title(title).add_filter("PDF", &["pdf", "PDF"]).pick_files()
        else {
            return;
        };
        let name = if self.de() { "Zusammengeführt.pdf" } else { "Merged.pdf" };
        match crate::Document::merge_files(&files, name) {
            Ok(doc) => {
                self.add_tab(doc);
                ctx.request_repaint();
            }
            Err(err) => self.show_error(&err),
        }
    }

    /// Renders the dialogs of this module.
    pub(super) fn structure_dialog(&mut self, ui: &mut Ui, dialog: &mut Dialog) -> (Outcome, Option<Action>) {
        let t = self.t();
        let de = self.de();
        let lang = self.settings.language;
        let n = self.active_tab().map_or(0, |t| t.doc.page_count());
        let mut outcome = Outcome::Keep;
        let mut action: Option<Action> = None;
        match dialog {
            Dialog::Swap { a, b } => {
                ui.heading(Cmd::SwapPages.label(lang));
                ui.horizontal(|ui| {
                    ui.label(if de { "Tauschen:" } else { "Swap:" });
                    ui.add(egui::DragValue::new(a).range(1..=n.max(1) as u32));
                    ui.label(if de { "Für:" } else { "With:" });
                    ui.add(egui::DragValue::new(b).range(1..=n.max(1) as u32));
                    ui.label(if de { format!("von {n}") } else { format!("of {n}") });
                });
                let (ok, cancel) = buttons(ui, t.ok, t.cancel);
                if ok {
                    let (a, b) = (*a as usize - 1, *b as usize - 1);
                    action = Some(Box::new(move |app, ctx| app.edit(ctx, |tab| tab.doc.swap_pages(a, b))));
                    outcome = Outcome::Close;
                } else if cancel {
                    outcome = Outcome::Close;
                }
            }
            Dialog::Duplicates { groups } => {
                ui.heading(if de { "Doppelte Seiten" } else { "Duplicate pages" });
                for g in groups.iter() {
                    let pages: Vec<String> = g.iter().map(|p| (p + 1).to_string()).collect();
                    ui.label(format!(
                        "{}: {}",
                        if de { "Gleiche Seiten" } else { "Identical pages" },
                        pages.join(", ")
                    ));
                }
                let (delete, cancel) =
                    buttons(ui, if de { "Duplikate löschen" } else { "Delete duplicates" }, t.cancel);
                if delete {
                    let doomed: Vec<usize> = groups.iter().flat_map(|g| g.iter().skip(1).copied()).collect();
                    action =
                        Some(Box::new(move |app, ctx| app.edit(ctx, |tab| tab.doc.delete_pages(&doomed))));
                    outcome = Outcome::Close;
                } else if cancel {
                    outcome = Outcome::Close;
                }
            }
            _ => outcome = Outcome::Close,
        }
        (outcome, action)
    }
}
