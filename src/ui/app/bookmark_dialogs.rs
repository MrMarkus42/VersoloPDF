//! Dialogs of the ribbon tab "Lesezeichen":
//! shared parts and the smaller dialogs. The large
//! ones (Aus Seitentext, Inhaltsverzeichnis, Verweise, Überprüfen) are in
//! `bookmark_big_dialogs.rs`.

use egui::{Context, RichText, Ui};

use super::bookmark_big_dialogs::{CheckState, FromTextState, LinksState, TocState};
use super::commands::Cmd;
use super::dialogs::{Action, Outcome};
use super::page_dialogs::{footer, tr};
use super::{Dialog, MrPdfApp};
use crate::Document;
use crate::model::Rgba;
use crate::outline::NewBookmark;
use crate::outline_edit::{ActionKind, Appearance, MergeOptions, Placement, Scope, Zoom, ZoomTargets};
use crate::outline_format::{
    CaseMode, FindReplace, ItemInfo, NumberStyle, Numbering, PLACEHOLDERS, TitleFormat, change_case,
    collapse_spaces, expand,
};
use crate::ui::i18n::Language;
use crate::ui::pickers::{PageRange, section, segmented};
use crate::ui::widgets::{from_color32, to_color32};

/// "Lesezeichenquelle auswählen".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScopeState {
    /// "Ausgewählte Lesezeichen" (else all).
    pub selected: bool,
    /// "Auf alle untergeordneten Lesezeichen anwenden".
    pub children: bool,
}

impl ScopeState {
    pub fn scope(&self, selection: Option<&Vec<usize>>) -> Scope {
        match selection {
            Some(path) if self.selected => {
                Scope { selected: Some(path.clone()), children: self.children, ..Default::default() }
            }
            _ => Scope::all(),
        }
    }
}

/// "Formatierung" being edited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatEdit {
    pub case: CaseMode,
    pub exclude_on: bool,
    pub exclude: String,
    pub replace_on: bool,
    pub replace: FindReplace,
    pub prefix_on: bool,
    pub prefix: String,
    pub suffix_on: bool,
    pub suffix: String,
    pub numbering: Numbering,
    pub style: NumberStyle,
    pub max_len: usize,
    pub double_spaces: bool,
    pub hyphens: bool,
    pub line_breaks: bool,
}

impl Default for FormatEdit {
    fn default() -> Self {
        Self {
            case: CaseMode::Keep,
            exclude_on: false,
            exclude: String::new(),
            replace_on: false,
            replace: FindReplace::default(),
            prefix_on: false,
            prefix: String::new(),
            suffix_on: false,
            suffix: String::new(),
            numbering: Numbering::Keep,
            style: NumberStyle::Arabic,
            max_len: 256,
            double_spaces: false,
            hyphens: false,
            line_breaks: false,
        }
    }
}

pub fn word_list(text: &str) -> Vec<String> {
    text.split(['\n', ',', ';']).map(|w| w.trim().to_string()).filter(|w| !w.is_empty()).collect()
}

impl FormatEdit {
    pub fn to_format(&self) -> TitleFormat {
        TitleFormat {
            case: self.case,
            exclude: if self.exclude_on { word_list(&self.exclude) } else { Vec::new() },
            replace: self.replace_on.then(|| self.replace.clone()),
            prefix: self.prefix_on.then(|| self.prefix.clone()),
            suffix: self.suffix_on.then(|| self.suffix.clone()),
            numbering: self.numbering,
            number_style: self.style,
            max_len: self.max_len,
            double_spaces: self.double_spaces,
            hyphens: self.hyphens,
            line_breaks: self.line_breaks,
        }
    }
}

/// An open bookmark dialog.
#[derive(Debug, Clone, PartialEq)]
pub enum BookmarkDialog {
    EveryNth {
        every: u32,
        template: String,
        range: PageRange,
    },
    FromText(Box<FromTextState>),
    FromToc {
        pages: String,
        auto: bool,
        offset: i32,
    },
    FromHighlights {
        range: PageRange,
        look: Appearance,
        format: FormatEdit,
        settings: Option<u8>,
    },
    FromTextFile {
        path: String,
        separator: String,
    },
    AddText {
        prefix: String,
        suffix: String,
        trim: bool,
        selected: bool,
        all_levels: bool,
        level: u32,
        below: bool,
    },
    Case {
        scope: ScopeState,
        mode: CaseMode,
        exclude_on: bool,
        exclude: String,
    },
    Zoom {
        scope: ScopeState,
        zoom: Zoom,
        text: String,
        targets: ZoomTargets,
    },
    ResolveNamed {
        scope: ScopeState,
        remove: bool,
    },
    Replace {
        scope: ScopeState,
        fr: FindReplace,
    },
    RemoveActions {
        scope: ScopeState,
        kinds: Vec<ActionKind>,
        remove_bookmarks: bool,
        keep_children: bool,
    },
    Sort {
        scope: ScopeState,
        by_page: bool,
        ignore_case: bool,
        descending: bool,
    },
    Check(Box<CheckState>),
    Merge {
        scope: ScopeState,
        opts: MergeOptions,
    },
    Toc(Box<TocState>),
    Links(Box<LinksState>),
    ToNamed {
        scope: ScopeState,
        mask: String,
    },
}

// ---------------------------------------------------------------------------
// Shared widgets

/// "Lesezeichenquelle auswählen" box.
pub(super) fn scope_ui(ui: &mut Ui, de: bool, s: &mut ScopeState, has_selection: bool, label: &str) {
    section(ui, tr(de, "Lesezeichenquelle auswählen", "Bookmark source"), |ui| {
        ui.label(label);
        if !has_selection {
            s.selected = false;
        }
        let shown = if s.selected {
            tr(de, "Ausgewählte Lesezeichen", "Selected bookmarks")
        } else {
            tr(de, "Alle Lesezeichen", "All bookmarks")
        };
        egui::ComboBox::from_id_salt(ui.id().with("scope")).selected_text(shown).width(220.0).show_ui(
            ui,
            |ui| {
                ui.selectable_value(&mut s.selected, false, tr(de, "Alle Lesezeichen", "All bookmarks"));
                ui.add_enabled_ui(has_selection, |ui| {
                    ui.selectable_value(
                        &mut s.selected,
                        true,
                        tr(de, "Ausgewählte Lesezeichen", "Selected bookmarks"),
                    )
                    .on_disabled_hover_text(tr(
                        de,
                        "Zuerst ein Lesezeichen in der Lesezeichen-Leiste auswählen.",
                        "Select a bookmark in the bookmarks pane first.",
                    ));
                });
            },
        );
        let mut children = if s.selected { s.children } else { true };
        ui.add_enabled_ui(s.selected, |ui| {
            ui.checkbox(
                &mut children,
                tr(de, "Auf alle untergeordneten Lesezeichen anwenden", "Apply to all child bookmarks"),
            );
        });
        if s.selected {
            s.children = children;
        }
    });
}

/// "[…]": appends a placeholder to `text`.
pub(super) fn placeholder_button(ui: &mut Ui, de: bool, salt: &str, text: &mut String) {
    let resp = ui.button("[…]").on_hover_text(tr(de, "Platzhalter einfügen", "Insert placeholder"));
    egui::Popup::menu(&resp).id(ui.id().with(("ph", salt))).show(|ui| {
        ui.set_min_width(220.0);
        for (ph, g, e) in PLACEHOLDERS {
            if ui.button(if de { g } else { e }).on_hover_text(ph).clicked() {
                text.push_str(ph);
                ui.close();
            }
        }
    });
}

/// Zoom list with a field for any percentage.
pub(super) fn zoom_ui(ui: &mut Ui, de: bool, salt: &str, zoom: &mut Zoom, text: &mut String) {
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt(ui.id().with(("zoom", salt)))
            .selected_text(zoom.label(de))
            .width(210.0)
            .show_ui(ui, |ui| {
                for z in Zoom::MODES {
                    if ui.selectable_label(*zoom == z, z.label(de)).clicked() {
                        *zoom = z;
                    }
                }
                ui.separator();
                for p in Zoom::PERCENTS {
                    let z = Zoom::Percent(p);
                    if ui.selectable_label(*zoom == z, z.label(de)).clicked() {
                        *zoom = z;
                        *text = z.label(de);
                    }
                }
            });
        let resp = ui.add(egui::TextEdit::singleline(text).desired_width(60.0).hint_text("%"));
        if resp.changed()
            && let Some(z) = Zoom::parse_percent(text)
        {
            *zoom = z;
        }
    });
}

pub(super) fn color_button(ui: &mut Ui, color: &mut Rgba) {
    let mut c = to_color32(*color);
    if egui::color_picker::color_edit_button_srgba(ui, &mut c, egui::color_picker::Alpha::Opaque).changed() {
        *color = from_color32(c);
    }
}

/// "Aussehen" page.
pub(super) fn appearance_ui(ui: &mut Ui, de: bool, look: &mut Appearance, zoom_text: &mut String) {
    ui.label(tr(
        de,
        "Legen Sie das Aussehen der erstellten Lesezeichen fest:",
        "Set the look of the new bookmarks:",
    ));
    egui::Grid::new(ui.id().with("look")).num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        ui.label(tr(de, "Textfarbe:", "Text colour:"));
        color_button(ui, &mut look.color);
        ui.end_row();
        ui.label(tr(de, "Textformat:", "Text style:"));
        let styles = [
            ((false, false), tr(de, "Normal", "Normal")),
            ((false, true), tr(de, "Kursiv", "Italic")),
            ((true, false), tr(de, "Fett", "Bold")),
            ((true, true), tr(de, "Fett und Kursiv", "Bold and italic")),
        ];
        let current = styles.iter().find(|(s, _)| *s == (look.bold, look.italic)).map_or("", |(_, l)| l);
        egui::ComboBox::from_id_salt(ui.id().with("style")).selected_text(current).width(210.0).show_ui(
            ui,
            |ui| {
                for ((b, i), label) in styles {
                    if ui.selectable_label((look.bold, look.italic) == (b, i), label).clicked() {
                        look.bold = b;
                        look.italic = i;
                    }
                }
            },
        );
        ui.end_row();
        ui.label(tr(de, "Vergrößerung:", "Zoom:"));
        zoom_ui(ui, de, "look", &mut look.zoom, zoom_text);
        ui.end_row();
        ui.label("");
        ui.checkbox(&mut look.expand, tr(de, "Alle Unterknoten ausklappen", "Expand all child nodes"));
        ui.end_row();
    });
}

/// "Formatierung" page.
pub(super) fn format_ui(ui: &mut Ui, de: bool, f: &mut FormatEdit) {
    ui.label(tr(
        de,
        "Texteigenschaften für die neuen Lesezeichentitel auswählen:",
        "Text properties of the new titles:",
    ));
    section(ui, tr(de, "Groß-/Kleinschreibung", "Case"), |ui| {
        segmented(
            ui,
            &mut f.case,
            &[
                (CaseMode::Keep, tr(de, "Nicht ändern", "Keep").into()),
                (CaseMode::EachWord, tr(de, "Jedes Wort Groß", "Each Word").into()),
                (CaseMode::FirstWord, tr(de, "Erstes wort groß", "First word").into()),
                (CaseMode::Upper, tr(de, "ALLES GROSS", "UPPER").into()),
                (CaseMode::Lower, tr(de, "alles klein", "lower").into()),
            ],
        );
        ui.add_enabled_ui(f.case != CaseMode::Keep, |ui| {
            ui.checkbox(&mut f.exclude_on, tr(de, "Folgende Wörter ausschließen:", "Exclude these words:"));
            ui.add_enabled(
                f.exclude_on,
                egui::TextEdit::multiline(&mut f.exclude).desired_rows(2).desired_width(380.0),
            );
        });
    });
    section(ui, tr(de, "Suchen- und Ersetzen-Einstellungen", "Find and replace"), |ui| {
        ui.checkbox(
            &mut f.replace_on,
            tr(de, "In Lesezeichentiteln suchen und ersetzen", "Find and replace in titles"),
        );
        ui.add_enabled_ui(f.replace_on, |ui| replace_fields(ui, de, &mut f.replace, "fmt"));
    });
    egui::Grid::new(ui.id().with("fmt")).num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        ui.checkbox(&mut f.prefix_on, tr(de, "Text voranstellen:", "Prefix:"));
        ui.horizontal(|ui| {
            ui.add_enabled(f.prefix_on, egui::TextEdit::singleline(&mut f.prefix).desired_width(200.0));
            placeholder_button(ui, de, "prefix", &mut f.prefix);
        });
        ui.end_row();
        ui.checkbox(&mut f.suffix_on, tr(de, "Text anhängen:", "Suffix:"));
        ui.horizontal(|ui| {
            ui.add_enabled(f.suffix_on, egui::TextEdit::singleline(&mut f.suffix).desired_width(200.0));
            placeholder_button(ui, de, "suffix", &mut f.suffix);
        });
        ui.end_row();
        ui.label(tr(de, "Nummerieren:", "Numbering:"));
        ui.horizontal(|ui| {
            let label = |n: Numbering| match n {
                Numbering::Keep => tr(de, "Keine Änderung", "No change"),
                Numbering::Add => tr(de, "Hinzufügen", "Add"),
                Numbering::Remove => tr(de, "Löschen", "Remove"),
            };
            egui::ComboBox::from_id_salt(ui.id().with("num")).selected_text(label(f.numbering)).show_ui(
                ui,
                |ui| {
                    for n in [Numbering::Keep, Numbering::Add, Numbering::Remove] {
                        ui.selectable_value(&mut f.numbering, n, label(n));
                    }
                },
            );
            ui.label(tr(de, "Benutze:", "Use:"));
            ui.add_enabled_ui(f.numbering == Numbering::Add, |ui| {
                number_style_combo(ui, "fmtstyle", &mut f.style)
            });
        });
        ui.end_row();
        ui.label(tr(de, "Maximale Länge:", "Maximum length:"));
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut f.max_len).range(1..=4096));
            ui.label(tr(de, "(Zeichen)", "(characters)"));
        });
        ui.end_row();
    });
    ui.horizontal(|ui| {
        ui.checkbox(&mut f.double_spaces, tr(de, "Doppelte Leerzeichen entfernen", "Remove double spaces"));
        ui.checkbox(&mut f.hyphens, tr(de, "Trennstriche entfernen", "Remove hyphens"));
    });
    ui.checkbox(
        &mut f.line_breaks,
        tr(de, "Zeilenumbrüche durch Leerzeichen ersetzen", "Replace line breaks with spaces"),
    );
}

pub(super) fn number_style_combo(ui: &mut Ui, salt: &str, style: &mut NumberStyle) {
    egui::ComboBox::from_id_salt(ui.id().with(salt)).selected_text(style.label()).show_ui(ui, |ui| {
        for s in NumberStyle::ALL {
            ui.selectable_value(style, s, s.label());
        }
    });
}

/// Search, replace and their options.
pub(super) fn replace_fields(ui: &mut Ui, de: bool, fr: &mut FindReplace, salt: &str) {
    egui::Grid::new(ui.id().with(("fr", salt))).num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        ui.label(tr(de, "Suchen nach:", "Find:"));
        ui.add(egui::TextEdit::singleline(&mut fr.find).desired_width(260.0));
        ui.end_row();
        ui.label(tr(de, "Ersetzen durch:", "Replace with:"));
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut fr.replace).desired_width(220.0));
            placeholder_button(ui, de, salt, &mut fr.replace);
        });
        ui.end_row();
    });
    ui.horizontal(|ui| {
        ui.checkbox(&mut fr.match_case, tr(de, "Groß-/Kleinschreibung", "Match case"));
        ui.checkbox(&mut fr.whole_word, tr(de, "Nur ganze Wörter", "Whole words only"));
        ui.checkbox(&mut fr.regex, tr(de, "Reguläre Ausdrücke verwenden", "Use regular expressions"));
    });
    if !fr.find.is_empty()
        && let Err(e) = fr.compile()
    {
        ui.colored_label(ui.visuals().error_fg_color, e.to_string());
    }
}

/// A document change as dialog action, reporting the count in the status bar.
pub(super) fn doc_action(
    de_label: &'static str,
    en_label: &'static str,
    f: impl FnOnce(&mut Document) -> crate::Result<usize> + 'static,
) -> Action {
    Box::new(move |app, ctx| {
        let result = app.tab().map(|t| f(&mut t.doc));
        let label = if app.bm_de() { de_label } else { en_label };
        app.outline_result(ctx, result, label);
    })
}

const CHANGED: (&str, &str) = ("Geänderte Lesezeichen", "Changed bookmarks");
const NEW: (&str, &str) = ("Neue Lesezeichen", "New bookmarks");

impl MrPdfApp {
    pub(super) fn bm_de(&self) -> bool {
        self.settings.language == Language::German
    }

    pub(super) fn selected_bookmark(&self) -> Option<Vec<usize>> {
        self.active_tab().and_then(|t| t.selected_bookmark.clone())
    }

    pub(super) fn outline_result(
        &mut self,
        ctx: &Context,
        result: Option<crate::Result<usize>>,
        label: &str,
    ) {
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

    /// Opens the dialog of a bookmark command; false if it has none.
    pub(super) fn open_bookmark_dialog(&mut self, cmd: Cmd) -> bool {
        let de = self.bm_de();
        let s = ScopeState::default();
        let d = match cmd {
            Cmd::BookmarkFromSelection => BookmarkDialog::FromText(Box::new(self.new_text_rules_state())),
            Cmd::BookmarkEveryNth => BookmarkDialog::EveryNth {
                every: 1,
                template: tr(de, "Seite %[Page]", "Page %[Page]").into(),
                range: PageRange::all(),
            },
            Cmd::BookmarkFromToc => {
                let cur = self.active_tab().map_or(0, |t| t.current_page);
                BookmarkDialog::FromToc { pages: (cur + 1).to_string(), auto: true, offset: 0 }
            }
            Cmd::BookmarkFromHighlights => BookmarkDialog::FromHighlights {
                range: PageRange::all(),
                look: Appearance::default(),
                format: FormatEdit::default(),
                settings: None,
            },
            Cmd::BookmarkFromTextFile => {
                BookmarkDialog::FromTextFile { path: String::new(), separator: String::new() }
            }
            Cmd::BookmarkAddText => BookmarkDialog::AddText {
                prefix: String::new(),
                suffix: String::new(),
                trim: false,
                selected: false,
                all_levels: true,
                level: 1,
                below: false,
            },
            Cmd::BookmarkCase => BookmarkDialog::Case {
                scope: s,
                mode: CaseMode::EachWord,
                exclude_on: false,
                exclude: String::new(),
            },
            Cmd::BookmarkZoom => BookmarkDialog::Zoom {
                scope: s,
                zoom: Zoom::ActualSize,
                text: String::new(),
                targets: ZoomTargets { named: true, gotor: false, gotoe: false },
            },
            Cmd::BookmarkResolveNamed => BookmarkDialog::ResolveNamed { scope: s, remove: false },
            Cmd::BookmarkReplace => BookmarkDialog::Replace { scope: s, fr: FindReplace::default() },
            Cmd::BookmarkRemoveActions => BookmarkDialog::RemoveActions {
                scope: s,
                kinds: Vec::new(),
                remove_bookmarks: false,
                keep_children: true,
            },
            Cmd::BookmarkSort => {
                BookmarkDialog::Sort { scope: s, by_page: true, ignore_case: true, descending: false }
            }
            Cmd::BookmarkCheck => BookmarkDialog::Check(Box::default()),
            Cmd::BookmarkDedupe => BookmarkDialog::Merge { scope: s, opts: MergeOptions::default() },
            Cmd::BookmarkToc => BookmarkDialog::Toc(Box::new(TocState::new(de))),
            Cmd::BookmarkLinks => BookmarkDialog::Links(Box::default()),
            Cmd::BookmarkToNamed => BookmarkDialog::ToNamed { scope: s, mask: "%[Title]".into() },
            _ => return false,
        };
        self.dialog = Some(Dialog::Bookmark(Box::new(d)));
        true
    }

    pub(super) fn bookmark_dialog(
        &mut self,
        ui: &mut Ui,
        d: &mut BookmarkDialog,
    ) -> (Outcome, Option<Action>) {
        let de = self.bm_de();
        let lang = self.settings.language;
        let (n, cur) = self.active_tab().map_or((0, 0), |t| (t.doc.page_count(), t.current_page));
        let selection = self.selected_bookmark();
        let has_sel = selection.is_some();
        let mut action: Option<Action> = None;
        let close = match d {
            BookmarkDialog::EveryNth { every, template, range } => {
                super::dialogs::set_title(ui, Cmd::BookmarkEveryNth.label(lang));
                section(ui, tr(de, "Lesezeicheneigenschaften", "Bookmark properties"), |ui| {
                    ui.horizontal(|ui| {
                        ui.label(tr(de, "Lesezeichen alle", "Bookmark every"));
                        ui.add(egui::DragValue::new(every).range(1..=10_000));
                        ui.label(tr(de, "Seiten im ausgewählten Bereich", "pages in the selected range"));
                    });
                    ui.label(tr(de, "Titelvorlage:", "Title template:"));
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(template).desired_width(260.0));
                        placeholder_button(ui, de, "nth", template);
                    });
                });
                range.ui(ui, de, n, cur, &PageRange::ALL_MODES);
                let pages = range.resolve(n, cur);
                let (ok, cancel, _) = footer(ui, de, pages.is_some(), false, "OK");
                if ok && let Some(pages) = pages {
                    let (every, template) = (*every as usize, template.clone());
                    action = Some(doc_action(NEW.0, NEW.1, move |doc| {
                        let base = doc.outline_base();
                        let items: Vec<NewBookmark> = pages
                            .iter()
                            .step_by(every.max(1))
                            .enumerate()
                            .map(|(i, &p)| {
                                let info = ItemInfo { page: Some(p), sequence: i + 1, ..base.clone() };
                                NewBookmark { title: expand(&template, &info), page: p, level: 0 }
                            })
                            .collect();
                        doc.append_bookmarks(&items)
                    }));
                }
                ok || cancel
            }
            BookmarkDialog::FromToc { pages, auto, offset } => {
                super::dialogs::set_title(ui, Cmd::BookmarkFromToc.label(lang));
                let parsed = crate::ui::pickers::parse_pages(pages, n);
                egui::Grid::new("bm-toc").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                    ui.label(tr(de, "Seiten des Verzeichnisses:", "Contents pages:"));
                    ui.text_edit_singleline(pages);
                    ui.end_row();
                    ui.label(tr(de, "Seitenversatz:", "Page offset:"));
                    ui.horizontal(|ui| {
                        ui.checkbox(auto, tr(de, "Automatisch", "Automatic"));
                        ui.add_enabled(!*auto, egui::DragValue::new(offset).range(-9999..=9999));
                    });
                    ui.end_row();
                });
                hint(
                    ui,
                    tr(
                        de,
                        "Zeilen wie „2.1 Titel ...... 14“ werden zu Lesezeichen. Der Versatz gleicht gedruckte und echte Seitenzahlen aus.",
                        "Lines like \"2.1 Title ...... 14\" become bookmarks. The offset matches printed and real page numbers.",
                    ),
                );
                let (ok, cancel, _) = footer(ui, de, parsed.is_some(), false, "OK");
                if ok && let Some(sel) = parsed {
                    let off = (!*auto).then_some(i64::from(*offset));
                    action = Some(Box::new(move |app, ctx| {
                        let result = app.tab().map(|t| {
                            let items = t.doc.toc_bookmarks(&sel, off)?;
                            t.doc.append_bookmarks(&items)
                        });
                        let label = if app.bm_de() { NEW.0 } else { NEW.1 };
                        app.outline_result(ctx, result, label);
                        app.side = super::SidePanel::Bookmarks;
                        app.settings.show_sidebar = true;
                    }));
                }
                ok || cancel
            }
            BookmarkDialog::FromHighlights { range, look, format, settings } => {
                if let Some(page) = settings {
                    super::dialogs::set_title(
                        ui,
                        tr(de, "Einstellungen zur Erstellung von Lesezeichen", "Bookmark creation settings"),
                    );
                    let mut zoom_text = String::new();
                    ui.horizontal_top(|ui| {
                        ui.vertical(|ui| {
                            ui.set_width(150.0);
                            ui.selectable_value(page, 0, tr(de, "Aussehen", "Appearance"));
                            ui.selectable_value(page, 1, tr(de, "Formatierung", "Formatting"));
                        });
                        ui.add_space(12.0);
                        ui.vertical(|ui| {
                            if *page == 0 {
                                appearance_ui(ui, de, look, &mut zoom_text);
                            } else {
                                format_ui(ui, de, format);
                            }
                        });
                    });
                    let (ok, cancel, _) = footer(ui, de, true, false, "OK");
                    if ok || cancel {
                        *settings = None;
                    }
                    return (Outcome::Keep, None);
                }
                super::dialogs::set_title(
                    ui,
                    tr(
                        de,
                        "Lesezeichen aus hervorgehobenem Text erzeugen",
                        "Bookmarks from highlighted text",
                    ),
                );
                range.ui(ui, de, n, cur, &PageRange::ALL_MODES);
                let pages = range.resolve(n, cur);
                let (ok, cancel, open) = footer(ui, de, pages.is_some(), true, "OK");
                if open {
                    *settings = Some(0);
                }
                if ok && let Some(pages) = pages {
                    let (look, fmt) = (*look, format.to_format());
                    action = Some(doc_action(NEW.0, NEW.1, move |doc| {
                        let re = fmt.compile()?;
                        let base = doc.outline_base();
                        let items: Vec<(NewBookmark, Appearance)> = doc
                            .highlight_bookmarks(&pages)?
                            .into_iter()
                            .enumerate()
                            .map(|(i, mut b)| {
                                let info = ItemInfo {
                                    title: b.title.clone(),
                                    page: Some(b.page),
                                    sequence: i + 1,
                                    ..base.clone()
                                };
                                b.title = fmt.apply(&b.title, i + 1, re.as_ref(), &info);
                                (b, look)
                            })
                            .collect();
                        doc.add_bookmarks_styled(&items, Placement::After)
                    }));
                }
                ok || cancel
            }
            BookmarkDialog::FromTextFile { path, separator } => {
                super::dialogs::set_title(
                    ui,
                    tr(de, "Lesezeichen aus Textdatei erstellen", "Bookmarks from a text file"),
                );
                ui.label(tr(de, "Dateiname:", "File name:"));
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(path).desired_width(300.0));
                    if ui.button("📂").on_hover_text(tr(de, "Datei auswählen", "Choose file")).clicked()
                        && let Some(p) = rfd::FileDialog::new()
                            .add_filter(tr(de, "Textdatei", "Text file"), &["txt", "csv"])
                            .pick_file()
                    {
                        *path = p.display().to_string();
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(tr(de, "Feldtrenner:", "Field separator:"));
                    ui.add(egui::TextEdit::singleline(separator).desired_width(40.0)).on_hover_text(tr(
                        de,
                        "Trennt Titel und Seitenzahl. Leer: Tabulator oder Semikolon.",
                        "Separates title and page number. Empty: tab or semicolon.",
                    ));
                });
                let (ok, cancel, _) = footer(ui, de, !path.trim().is_empty(), false, "OK");
                if ok {
                    let (path, sep) = (std::path::PathBuf::from(path.trim()), separator.replace("\\t", "\t"));
                    action = Some(doc_action(NEW.0, NEW.1, move |doc| {
                        let text = read_list(&path)?;
                        let sep = (!sep.is_empty()).then_some(sep.as_str());
                        let items = crate::outline::parse_bookmark_list_with(&text, doc.page_count(), sep);
                        doc.append_bookmarks(&items)
                    }));
                }
                ok || cancel
            }
            BookmarkDialog::AddText { prefix, suffix, trim, selected, all_levels, level, below } => {
                super::dialogs::set_title(
                    ui,
                    tr(de, "Text zu Lesezeichentitel hinzufügen", "Add text to bookmark titles"),
                );
                section(ui, tr(de, "Text hinzufügen", "Add text"), |ui| {
                    ui.label(tr(de, "Präfix hinzufügen:", "Add prefix:"));
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(prefix).desired_width(240.0));
                        placeholder_button(ui, de, "pre", prefix);
                    });
                    ui.label(tr(de, "Suffix hinzufügen:", "Add suffix:"));
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(suffix).desired_width(240.0));
                        placeholder_button(ui, de, "suf", suffix);
                    });
                    ui.checkbox(
                        trim,
                        tr(
                            de,
                            "Zusätzliche Leerzeichen aus Lesezeichentitel entfernen",
                            "Remove extra spaces from titles",
                        ),
                    );
                });
                section(ui, tr(de, "Anwenden auf:", "Apply to:"), |ui| {
                    if !has_sel {
                        *selected = false;
                    }
                    ui.radio_value(selected, false, tr(de, "Alle Lesezeichen", "All bookmarks"));
                    ui.add_enabled_ui(has_sel, |ui| {
                        ui.radio_value(
                            selected,
                            true,
                            tr(de, "Ausgewählte Lesezeichen", "Selected bookmarks"),
                        );
                    });
                });
                section(ui, tr(de, "Ebenen verarbeiten:", "Levels:"), |ui| {
                    ui.checkbox(all_levels, tr(de, "Alle Ebenen verarbeiten", "All levels"));
                    ui.add_enabled_ui(!*all_levels, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(tr(de, "Ebene auswählen:", "Level:"));
                            ui.add(egui::DragValue::new(level).range(1..=32));
                        });
                        ui.checkbox(
                            below,
                            tr(de, "Alle darunterliegenden Ebenen einschließen", "Include all levels below"),
                        );
                    });
                });
                let ok_enabled = !prefix.is_empty() || !suffix.is_empty() || *trim;
                let (ok, cancel, _) = footer(ui, de, ok_enabled, false, "OK");
                if ok {
                    let mut scope =
                        ScopeState { selected: *selected, children: true }.scope(selection.as_ref());
                    if !*all_levels {
                        scope.level = Some(*level as usize);
                        scope.below = *below;
                    }
                    let (p, s, trim) = (prefix.clone(), suffix.clone(), *trim);
                    action = Some(doc_action(CHANGED.0, CHANGED.1, move |doc| {
                        doc.bookmarks_map_titles(&scope, |t, info| {
                            let t = if trim { collapse_spaces(t.trim()) } else { t.to_string() };
                            format!("{}{t}{}", expand(&p, info), expand(&s, info))
                        })
                    }));
                }
                ok || cancel
            }
            BookmarkDialog::Case { scope, mode, exclude_on, exclude } => {
                super::dialogs::set_title(ui, tr(de, "Groß-/Kleinschreibung ändern", "Change case"));
                scope_ui(ui, de, scope, has_sel, tr(de, "Lesezeichen:", "Bookmarks:"));
                section(ui, tr(de, "Groß-/Kleinschreibung", "Case"), |ui| {
                    segmented(
                        ui,
                        mode,
                        &[
                            (CaseMode::EachWord, tr(de, "Jedes Wort Groß", "Each Word").into()),
                            (CaseMode::FirstWord, tr(de, "Erstes wort groß", "First word").into()),
                            (CaseMode::Upper, tr(de, "ALLES GROSS", "UPPER").into()),
                            (CaseMode::Lower, tr(de, "alles klein", "lower").into()),
                        ],
                    );
                    ui.checkbox(exclude_on, tr(de, "Folgende Wörter ausschließen:", "Exclude these words:"));
                    ui.add_enabled(
                        *exclude_on,
                        egui::TextEdit::multiline(exclude).desired_rows(3).desired_width(320.0),
                    );
                });
                let (ok, cancel, _) = footer(ui, de, true, false, "OK");
                if ok {
                    let sc = scope.scope(selection.as_ref());
                    let (mode, words) = (*mode, if *exclude_on { word_list(exclude) } else { Vec::new() });
                    action = Some(doc_action(CHANGED.0, CHANGED.1, move |doc| {
                        doc.bookmarks_map_titles(&sc, |t, _| change_case(t, mode, &words))
                    }));
                }
                ok || cancel
            }
            BookmarkDialog::Zoom { scope, zoom, text, targets } => {
                super::dialogs::set_title(ui, Cmd::BookmarkZoom.label(lang));
                section(ui, tr(de, "Vergrößerungseinstellungen", "Zoom settings"), |ui| {
                    ui.label(tr(de, "Vergrößerung:", "Zoom:"));
                    zoom_ui(ui, de, "zoom", zoom, text);
                    ui.checkbox(
                        &mut targets.named,
                        tr(de, "Sprungmarken in Lesezeichen ändern", "Change named destinations"),
                    );
                    ui.checkbox(
                        &mut targets.gotor,
                        tr(
                            de,
                            "Vergrößerungsstufe von GoToR-Aktionen ändern",
                            "Change zoom of GoToR actions",
                        ),
                    );
                    ui.checkbox(
                        &mut targets.gotoe,
                        tr(
                            de,
                            "Vergrößerungsstufe von GoToE-Aktionen ändern",
                            "Change zoom of GoToE actions",
                        ),
                    );
                });
                scope_ui(ui, de, scope, has_sel, tr(de, "Lesezeichen:", "Bookmarks:"));
                let (ok, cancel, _) = footer(ui, de, true, false, "OK");
                if ok {
                    let (sc, z, t) = (scope.scope(selection.as_ref()), *zoom, *targets);
                    action =
                        Some(doc_action(CHANGED.0, CHANGED.1, move |doc| doc.bookmarks_set_zoom(&sc, z, t)));
                }
                ok || cancel
            }
            BookmarkDialog::ResolveNamed { scope, remove } => {
                super::dialogs::set_title(
                    ui,
                    tr(de, "Sprungmarken zu normalen Zielen umwandeln", "Named destinations to normal ones"),
                );
                scope_ui(ui, de, scope, has_sel, tr(de, "Lesezeichen:", "Bookmarks:"));
                ui.checkbox(
                    remove,
                    tr(
                        de,
                        "Umgewandelte Sprungmarken aus dem Dokument entfernen",
                        "Remove converted named destinations",
                    ),
                );
                let (ok, cancel, _) = footer(ui, de, true, false, "OK");
                if ok {
                    let (sc, rm) = (scope.scope(selection.as_ref()), *remove);
                    action = Some(doc_action(CHANGED.0, CHANGED.1, move |doc| {
                        doc.bookmarks_resolve_named(&sc, rm)
                    }));
                }
                ok || cancel
            }
            BookmarkDialog::Replace { scope, fr } => {
                super::dialogs::set_title(
                    ui,
                    tr(de, "Lesezeichentext suchen und ersetzen", "Find and replace in bookmarks"),
                );
                section(ui, tr(de, "Suchen- und Ersetzen-Einstellungen", "Find and replace"), |ui| {
                    replace_fields(ui, de, fr, "replace")
                });
                scope_ui(ui, de, scope, has_sel, tr(de, "Lesezeichen:", "Bookmarks:"));
                let valid = !fr.find.is_empty() && fr.compile().is_ok();
                let (ok, cancel, _) = footer(ui, de, valid, false, "OK");
                if ok && valid {
                    let (sc, fr) = (scope.scope(selection.as_ref()), fr.clone());
                    action = Some(doc_action(CHANGED.0, CHANGED.1, move |doc| {
                        let re = fr.compile()?;
                        doc.bookmarks_map_titles(&sc, |t, info| {
                            let with = FindReplace { replace: expand(&fr.replace, info), ..fr.clone() };
                            with.apply(&re, t)
                        })
                    }));
                }
                ok || cancel
            }
            BookmarkDialog::RemoveActions { scope, kinds, remove_bookmarks, keep_children } => {
                super::dialogs::set_title(ui, Cmd::BookmarkRemoveActions.label(lang));
                scope_ui(ui, de, scope, has_sel, tr(de, "Lesezeichen:", "Bookmarks:"));
                section(ui, tr(de, "Einstellungen", "Settings"), |ui| {
                    ui.checkbox(
                        remove_bookmarks,
                        tr(
                            de,
                            "Entferne Lesezeichen, die den ausgewählten Kriterien entsprechen",
                            "Remove bookmarks that match",
                        ),
                    );
                    ui.add_enabled_ui(*remove_bookmarks, |ui| {
                        ui.indent("keep", |ui| {
                            ui.checkbox(
                                keep_children,
                                tr(de, "Behalte untergeordnete Lesezeichen", "Keep child bookmarks"),
                            )
                        });
                    });
                });
                section(ui, tr(de, "Aktion auswählen", "Choose actions"), |ui| {
                    ui.horizontal(|ui| {
                        if ui.button(tr(de, "Alles auswählen", "Select all")).clicked() {
                            *kinds = ActionKind::ALL.to_vec();
                        }
                        if ui.button(tr(de, "Alle abwählen", "Select none")).clicked() {
                            kinds.clear();
                        }
                        if ui.button(tr(de, "Auswahl umkehren", "Invert")).clicked() {
                            *kinds = ActionKind::ALL.iter().copied().filter(|k| !kinds.contains(k)).collect();
                        }
                    });
                    egui::ScrollArea::vertical().id_salt("bm-actions").max_height(200.0).show(ui, |ui| {
                        for k in ActionKind::ALL {
                            let mut on = kinds.contains(&k);
                            if ui.checkbox(&mut on, k.label(de)).changed() {
                                kinds.retain(|x| *x != k);
                                if on {
                                    kinds.push(k);
                                }
                            }
                        }
                    });
                });
                let (ok, cancel, _) = footer(ui, de, !kinds.is_empty(), false, "OK");
                if ok {
                    let (sc, kinds, rb, kc) =
                        (scope.scope(selection.as_ref()), kinds.clone(), *remove_bookmarks, *keep_children);
                    action = Some(doc_action(CHANGED.0, CHANGED.1, move |doc| {
                        doc.bookmarks_remove_actions(&sc, &kinds, rb, kc)
                    }));
                }
                ok || cancel
            }
            BookmarkDialog::Sort { scope, by_page, ignore_case, descending } => {
                super::dialogs::set_title(ui, tr(de, "Lesezeichen sortieren", "Sort bookmarks"));
                section(ui, tr(de, "Sortieren nach", "Sort by"), |ui| {
                    ui.radio_value(
                        by_page,
                        true,
                        tr(de, "Referenzierter Seitenzahl", "Referenced page number"),
                    );
                    ui.radio_value(by_page, false, tr(de, "Alphabetisch", "Alphabetical"));
                });
                scope_ui(ui, de, scope, has_sel, tr(de, "Lesezeichen:", "Bookmarks:"));
                ui.checkbox(ignore_case, tr(de, "Groß-/Kleinschreibung ignorieren", "Ignore case"));
                ui.horizontal(|ui| {
                    ui.label(tr(de, "Sortierung:", "Order:"));
                    egui::ComboBox::from_id_salt("bm-sort-dir")
                        .selected_text(if *descending {
                            tr(de, "Absteigend", "Descending")
                        } else {
                            tr(de, "Aufsteigend", "Ascending")
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(descending, false, tr(de, "Aufsteigend", "Ascending"));
                            ui.selectable_value(descending, true, tr(de, "Absteigend", "Descending"));
                        });
                });
                let (ok, cancel, _) = footer(ui, de, true, false, "OK");
                if ok {
                    let (sc, bp, ic, desc) =
                        (scope.scope(selection.as_ref()), *by_page, *ignore_case, *descending);
                    action = Some(Box::new(move |app, ctx| {
                        app.edit(ctx, |t| t.doc.sort_bookmarks_in(&sc, bp, desc, ic))
                    }));
                }
                ok || cancel
            }
            BookmarkDialog::Merge { scope, opts } => {
                super::dialogs::set_title(
                    ui,
                    tr(de, "Doppelte Lesezeichen zusammenfassen", "Merge duplicate bookmarks"),
                );
                scope_ui(ui, de, scope, has_sel, tr(de, "Untersuchen:", "Examine:"));
                ui.checkbox(&mut opts.ignore_case, tr(de, "Groß-/Kleinschreibung ignorieren", "Ignore case"));
                ui.checkbox(
                    &mut opts.ignore_spaces,
                    tr(de, "Aufeinanderfolgende Leerzeichen ignorieren", "Ignore repeated spaces"),
                );
                let first_last = |ui: &mut Ui, salt: &str, last: &mut bool| {
                    egui::ComboBox::from_id_salt(salt)
                        .selected_text(if *last {
                            tr(de, "Vom letzten Lesezeichen verwenden", "From the last bookmark")
                        } else {
                            tr(de, "Vom ersten Lesezeichen verwenden", "From the first bookmark")
                        })
                        .width(240.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                last,
                                false,
                                tr(de, "Vom ersten Lesezeichen verwenden", "From the first bookmark"),
                            );
                            ui.selectable_value(
                                last,
                                true,
                                tr(de, "Vom letzten Lesezeichen verwenden", "From the last bookmark"),
                            );
                        });
                };
                egui::Grid::new("bm-merge").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                    ui.label(tr(de, "Titel des Ergebnisses:", "Title of the result:"));
                    first_last(ui, "bm-merge-title", &mut opts.title_from_last);
                    ui.end_row();
                    ui.label(tr(de, "Aktion des Ergebnisses:", "Action of the result:"));
                    first_last(ui, "bm-merge-action", &mut opts.action_from_last);
                    ui.end_row();
                });
                let (ok, cancel, _) = footer(ui, de, true, false, "OK");
                if ok {
                    let (sc, o) = (scope.scope(selection.as_ref()), *opts);
                    action =
                        Some(doc_action("Zusammengefasste Lesezeichen", "Merged bookmarks", move |doc| {
                            doc.merge_duplicate_bookmarks(&sc, o)
                        }));
                }
                ok || cancel
            }
            BookmarkDialog::ToNamed { scope, mask } => {
                super::dialogs::set_title(ui, Cmd::BookmarkToNamed.label(lang));
                section(ui, tr(de, "Name der Sprungmarke", "Name of the destination"), |ui| {
                    ui.label(tr(
                        de,
                        "Der Name wird nach folgender Maske erzeugt:",
                        "The name is made from this mask:",
                    ));
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(mask).desired_width(260.0));
                        placeholder_button(ui, de, "mask", mask);
                    });
                });
                scope_ui(ui, de, scope, has_sel, tr(de, "Lesezeichen:", "Bookmarks:"));
                let (ok, cancel, _) = footer(ui, de, !mask.trim().is_empty(), false, "OK");
                if ok {
                    let (sc, m) = (scope.scope(selection.as_ref()), mask.clone());
                    action =
                        Some(doc_action(CHANGED.0, CHANGED.1, move |doc| doc.bookmarks_to_named(&sc, &m)));
                }
                ok || cancel
            }
            BookmarkDialog::FromText(state) => return self.text_rules_dialog(ui, state),
            BookmarkDialog::Check(state) => return self.check_dialog(ui, state),
            BookmarkDialog::Toc(state) => return self.toc_dialog(ui, state),
            BookmarkDialog::Links(state) => return self.links_dialog(ui, state),
        };
        (if close { Outcome::Close } else { Outcome::Keep }, action)
    }

    /// "Seiten sortieren" (no dialog; a message when nothing changes).
    pub(super) fn sort_pages_by_bookmarks(&mut self, ctx: &Context) {
        let de = self.bm_de();
        match self.tab().map(|t| t.doc.sort_pages_by_bookmarks()) {
            Some(Ok(true)) => {
                if let Some(tab) = self.tab() {
                    tab.after_edit();
                }
                self.set_status(ctx, tr(de, "Seiten sortiert", "Pages sorted"));
            }
            Some(Ok(false)) => {
                self.dialog = Some(Dialog::Message {
                    title: tr(de, "Lesezeichen", "Bookmarks").into(),
                    text: tr(
                        de,
                        "Die Seiten sind bereits nach Lesezeichen sortiert.",
                        "The pages already follow the bookmarks.",
                    )
                    .into(),
                })
            }
            Some(Err(err)) => self.show_error(&err),
            None => {}
        }
    }
}

pub(super) fn hint(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).weak().small());
}

/// Upper bound for bookmark list files.
const MAX_LIST_FILE: u64 = 4 * 1024 * 1024;

fn read_list(path: &std::path::Path) -> crate::Result<String> {
    match std::fs::metadata(path) {
        Ok(m) if m.len() > MAX_LIST_FILE => {
            Err(crate::Error::FileTooLarge { size: m.len(), limit: MAX_LIST_FILE })
        }
        _ => std::fs::read(path)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .map_err(|e| crate::Error::io(path, e)),
    }
}
