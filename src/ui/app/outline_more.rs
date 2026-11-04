//! "Lesezeichen" tab, second part: table of contents, add text, zoom,
//! destinations, actions, check, links and page order.

use egui::{Context, RichText, Ui};

use super::commands::Cmd;
use super::dialogs::{Action, Outcome, buttons};
use super::{Dialog, MrPdfApp};
use crate::outline_edit::Zoom;
use crate::ui::pickers::parse_pages;

/// Choices of "Vergrößerung ändern".
pub const ZOOMS: [(Zoom, &str, &str); 6] = [
    (Zoom::Inherit, "Zoom übernehmen", "Inherit zoom"),
    (Zoom::Percent(100.0), "Fester Zoom", "Fixed zoom"),
    (Zoom::Fit, "Ganze Seite", "Fit page"),
    (Zoom::FitWidth, "Seitenbreite", "Fit width"),
    (Zoom::FitHeight, "Seitenhöhe", "Fit height"),
    (Zoom::FitVisible, "Sichtbarer Inhalt", "Fit visible"),
];

fn same_kind(a: Zoom, b: Zoom) -> bool {
    std::mem::discriminant(&a) == std::mem::discriminant(&b)
}

impl MrPdfApp {
    fn ol_de(&self) -> bool {
        self.settings.language == crate::ui::i18n::Language::German
    }

    /// Commands of this part; false if `cmd` is not one.
    pub(super) fn run_outline_more(&mut self, ctx: &Context, cmd: Cmd) -> bool {
        let de = self.ol_de();
        let changed = if de { "Geänderte Lesezeichen" } else { "Changed bookmarks" };
        let (n, cur) = self.active_tab().map_or((0, 0), |t| (t.doc.page_count(), t.current_page));
        match cmd {
            Cmd::BookmarkFromToc => {
                self.dialog =
                    Some(Dialog::BookmarkFromToc { pages: format!("{}", cur + 1), auto: true, offset: 0 })
            }
            Cmd::BookmarkAddText => {
                self.dialog = Some(Dialog::BookmarkAddText { prefix: String::new(), suffix: String::new() })
            }
            Cmd::BookmarkZoom => {
                self.dialog = Some(Dialog::BookmarkZoom { zoom: Zoom::Inherit, percent: 100.0 })
            }
            Cmd::BookmarkResolveNamed => {
                let result = self.tab().map(|t| t.doc.bookmarks_resolve_named());
                self.outline_result(ctx, result, changed);
            }
            Cmd::BookmarkToNamed => {
                let result = self.tab().map(|t| t.doc.bookmarks_to_named());
                self.outline_result(ctx, result, changed);
            }
            Cmd::BookmarkRemoveActions => {
                let result = self.tab().map(|t| t.doc.bookmarks_remove_actions());
                self.outline_result(ctx, result, changed);
            }
            Cmd::BookmarkCheck => match self.tab().map(|t| t.doc.check_bookmarks()) {
                Some(Ok(problems)) => self.dialog = Some(Dialog::BookmarkCheck { problems }),
                Some(Err(err)) => self.show_error(&err),
                None => {}
            },
            Cmd::BookmarkLinks => {
                self.dialog = Some(Dialog::BookmarkLinks { pages: format!("1-{}", n.max(1)) })
            }
            Cmd::BookmarkSortPages => {
                let result = self.tab().map(|t| t.doc.sort_pages_by_bookmarks());
                match result {
                    Some(Ok(true)) => {
                        if let Some(tab) = self.tab() {
                            tab.after_edit();
                        }
                        self.set_status(ctx, if de { "Seiten sortiert" } else { "Pages sorted" });
                    }
                    Some(Ok(false)) => self.set_status(
                        ctx,
                        if de {
                            "Die Seiten folgen schon den Lesezeichen"
                        } else {
                            "Pages already follow the bookmarks"
                        },
                    ),
                    Some(Err(err)) => self.show_error(&err),
                    None => {}
                }
            }
            _ => return false,
        }
        true
    }

    fn outline_result(&mut self, ctx: &Context, result: Option<crate::Result<usize>>, label: &str) {
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

    pub(super) fn outline_more_dialog(
        &mut self,
        ui: &mut Ui,
        dialog: &mut Dialog,
    ) -> (Outcome, Option<Action>) {
        let t = self.t();
        let de = self.ol_de();
        let lang = self.settings.language;
        let n = self.active_tab().map_or(0, |t| t.doc.page_count());
        let mut outcome = Outcome::Keep;
        let mut action: Option<Action> = None;
        let hint = |ui: &mut Ui, text: &str| {
            ui.label(RichText::new(text).weak().small());
        };
        match dialog {
            Dialog::BookmarkFromToc { pages, auto, offset } => {
                super::dialogs::set_title(ui, Cmd::BookmarkFromToc.label(lang));
                let parsed = parse_pages(pages, n);
                egui::Grid::new("bm-toc").num_columns(2).show(ui, |ui| {
                    ui.label(if de { "Seiten des Verzeichnisses:" } else { "Contents pages:" });
                    ui.text_edit_singleline(pages);
                    ui.end_row();
                    ui.label(if de { "Seitenversatz:" } else { "Page offset:" });
                    ui.horizontal(|ui| {
                        ui.checkbox(auto, if de { "Automatisch" } else { "Automatic" });
                        ui.add_enabled(!*auto, egui::DragValue::new(offset).range(-9999..=9999));
                    });
                    ui.end_row();
                });
                hint(
                    ui,
                    if de {
                        "Zeilen wie „2.1 Titel ...... 14“ werden zu Lesezeichen. Der Versatz gleicht gedruckte und echte Seitenzahlen aus (z. B. Titelseiten ohne Nummer)."
                    } else {
                        "Lines like \"2.1 Title ...... 14\" become bookmarks. The offset matches printed and real page numbers (e.g. unnumbered title pages)."
                    },
                );
                if parsed.is_none() {
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        if de { "Ungültige Seiten" } else { "Invalid pages" },
                    );
                }
                let (ok, cancel) = buttons(ui, t.ok, t.cancel);
                if ok && let Some(sel) = parsed {
                    let off = (!*auto).then_some(i64::from(*offset));
                    action = Some(Box::new(move |app, ctx| {
                        let result = app.tab().map(|t| {
                            let items = t.doc.toc_bookmarks(&sel, off)?;
                            t.doc.append_bookmarks(&items)
                        });
                        let label = if app.ol_de() { "Neue Lesezeichen" } else { "New bookmarks" };
                        app.outline_result(ctx, result, label);
                        app.side = super::SidePanel::Bookmarks;
                        app.settings.show_sidebar = true;
                    }));
                    outcome = Outcome::Close;
                } else if cancel {
                    outcome = Outcome::Close;
                }
            }
            Dialog::BookmarkAddText { prefix, suffix } => {
                super::dialogs::set_title(ui, Cmd::BookmarkAddText.label(lang));
                egui::Grid::new("bm-add-text").num_columns(2).show(ui, |ui| {
                    ui.label(if de { "Vor dem Titel:" } else { "Before the title:" });
                    ui.text_edit_singleline(prefix);
                    ui.end_row();
                    ui.label(if de { "Nach dem Titel:" } else { "After the title:" });
                    ui.text_edit_singleline(suffix);
                    ui.end_row();
                });
                hint(ui, if de { "Gilt für alle Lesezeichen." } else { "Applies to all bookmarks." });
                let filled = !(prefix.is_empty() && suffix.is_empty());
                let (ok, cancel) = super::dialogs::buttons_if(ui, t.ok, t.cancel, filled);
                if ok {
                    let (p, s) = (prefix.clone(), suffix.clone());
                    action = Some(Box::new(move |app, ctx| {
                        let result = app.tab().map(|t| t.doc.map_bookmark_titles(|x| format!("{p}{x}{s}")));
                        let label = if app.ol_de() { "Geänderte Lesezeichen" } else { "Changed bookmarks" };
                        app.outline_result(ctx, result, label);
                    }));
                    outcome = Outcome::Close;
                } else if cancel {
                    outcome = Outcome::Close;
                }
            }
            Dialog::BookmarkZoom { zoom, percent } => {
                super::dialogs::set_title(ui, Cmd::BookmarkZoom.label(lang));
                ui.label(if de { "Ansicht beim Anspringen:" } else { "View when jumping:" });
                for (choice, g, e) in ZOOMS {
                    if ui.radio(same_kind(*zoom, choice), if de { g } else { e }).clicked() {
                        *zoom = choice;
                    }
                    if matches!(choice, Zoom::Percent(_)) && matches!(zoom, Zoom::Percent(_)) {
                        ui.horizontal(|ui| {
                            ui.add_space(24.0);
                            ui.add(egui::DragValue::new(percent).range(10.0..=6400.0).suffix(" %"));
                        });
                    }
                }
                hint(
                    ui,
                    if de {
                        "Gilt für alle Lesezeichen. Sprungmarken werden dabei zu normalen Zielen."
                    } else {
                        "Applies to all bookmarks. Named destinations become normal ones."
                    },
                );
                let (ok, cancel) = buttons(ui, t.ok, t.cancel);
                if ok {
                    let z = match zoom {
                        Zoom::Percent(_) => Zoom::Percent(*percent),
                        other => *other,
                    };
                    action = Some(Box::new(move |app, ctx| {
                        let result = app.tab().map(|t| t.doc.bookmarks_set_zoom(z));
                        let label = if app.ol_de() { "Geänderte Lesezeichen" } else { "Changed bookmarks" };
                        app.outline_result(ctx, result, label);
                    }));
                    outcome = Outcome::Close;
                } else if cancel {
                    outcome = Outcome::Close;
                }
            }
            Dialog::BookmarkLinks { pages } => {
                super::dialogs::set_title(ui, Cmd::BookmarkLinks.label(lang));
                let parsed = parse_pages(pages, n);
                ui.horizontal(|ui| {
                    ui.label(if de { "Seiten:" } else { "Pages:" });
                    ui.text_edit_singleline(pages);
                });
                hint(
                    ui,
                    if de {
                        "Wo der Titel eines Lesezeichens im Text steht (z. B. im Inhaltsverzeichnis), entsteht ein Verweis auf seine Seite."
                    } else {
                        "Where a bookmark title appears in the text (e.g. in the table of contents), a link to its page is created."
                    },
                );
                let (ok, cancel) = buttons(ui, t.ok, t.cancel);
                if ok && let Some(sel) = parsed {
                    action = Some(Box::new(move |app, ctx| {
                        let result = app.tab().map(|t| t.doc.links_from_bookmarks(&sel));
                        let label = if app.ol_de() { "Neue Verweise" } else { "New links" };
                        app.outline_result(ctx, result, label);
                    }));
                    outcome = Outcome::Close;
                } else if cancel {
                    outcome = Outcome::Close;
                }
            }
            Dialog::BookmarkCheck { problems } => {
                super::dialogs::set_title(ui, if de { "Lesezeichen überprüfen" } else { "Check bookmarks" });
                if problems.is_empty() {
                    ui.label(if de {
                        "Alle Lesezeichen führen zu einem Ziel."
                    } else {
                        "All bookmarks lead to a target."
                    });
                    if buttons(ui, t.ok, t.cancel) != (false, false) {
                        outcome = Outcome::Close;
                    }
                } else {
                    ui.label(if de {
                        format!("{} Lesezeichen ohne gültiges Ziel:", problems.len())
                    } else {
                        format!("{} bookmarks without a valid target:", problems.len())
                    });
                    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                        egui::Grid::new("bm-check").striped(true).num_columns(2).show(ui, |ui| {
                            for (title, p) in problems.iter() {
                                ui.label(title.as_str());
                                ui.label(RichText::new(p.text(de)).weak());
                                ui.end_row();
                            }
                        });
                    });
                    let remove = if de { "Diese Lesezeichen löschen" } else { "Delete these bookmarks" };
                    let (ok, cancel) = buttons(ui, remove, t.cancel);
                    if ok {
                        action = Some(Box::new(|app, ctx| {
                            let result = app.tab().map(|t| t.doc.remove_broken_bookmarks());
                            let label =
                                if app.ol_de() { "Gelöschte Lesezeichen" } else { "Deleted bookmarks" };
                            app.outline_result(ctx, result, label);
                        }));
                        outcome = Outcome::Close;
                    } else if cancel {
                        outcome = Outcome::Close;
                    }
                }
            }
            _ => outcome = Outcome::Close,
        }
        (outcome, action)
    }
}
