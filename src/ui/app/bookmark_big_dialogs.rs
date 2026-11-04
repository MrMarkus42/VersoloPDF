//! The large dialogs of the ribbon tab "Lesezeichen": Aus Seitentext (rules),
//! Überprüfen, Inhaltsverzeichnis erstellen and Verweise aus Lesezeichen.

use egui::{RichText, Ui};

use super::bookmark_dialogs::{
    FormatEdit, ScopeState, appearance_ui, color_button, doc_action, format_ui, hint, scope_ui,
};
use super::dialogs::{Action, Outcome};
use super::page_dialogs::{footer, tr};
use super::{Dialog, MrPdfApp, PaperChoice};
use crate::document::{LinkOptions, TextBookmarkOptions, TocLevel, TocOptions, TocPageNumbers};
use crate::model::Rgba;
use crate::outline_edit::{Appearance, Finding, Placement, Problem};
use crate::outline_format::{CaseRule, TextRule};
use crate::pdf_edit::{LinkBorder, LinkHighlight, LinkLook};
use crate::report::Leader;
use crate::ui::pickers::{PageRange, Unit, length, section, segmented};

// ---------------------------------------------------------------------------
// Aus Seitentext

/// A rule being edited ("Einstellungen zur Erstellung von Lesezeichen").
#[derive(Debug, Clone, PartialEq)]
pub struct RuleEdit {
    pub index: Option<usize>,
    pub page: u8,
    pub level: u32,
    pub fonts_on: bool,
    pub fonts: String,
    pub size_on: bool,
    pub size: f32,
    pub tolerance: f32,
    pub color_on: bool,
    pub color: Rgba,
    pub case: CaseRule,
    pub multiline: bool,
    pub pattern_on: bool,
    pub pattern: String,
    pub pattern_case: bool,
    pub restrict: bool,
    pub look: Appearance,
    pub zoom_text: String,
    pub format: FormatEdit,
}

impl RuleEdit {
    fn from_rule(index: Option<usize>, rule: &TextRule, look: Appearance, format: FormatEdit) -> Self {
        Self {
            index,
            page: 0,
            level: rule.level as u32 + 1,
            fonts_on: rule.fonts.is_some(),
            fonts: rule.fonts.clone().unwrap_or_default().join("\n"),
            size_on: rule.size.is_some(),
            size: rule.size.map_or(12.0, |s| s.0),
            tolerance: rule.size.map_or(0.2, |s| s.1),
            color_on: rule.color.is_some(),
            color: rule.color.map_or(Rgba::BLACK, |[r, g, b]| Rgba::rgb(r, g, b)),
            case: rule.case,
            multiline: rule.multiline,
            pattern_on: rule.pattern.is_some(),
            pattern: rule.pattern.clone().unwrap_or_default(),
            pattern_case: rule.pattern_case,
            restrict: rule.restrict_to_pattern,
            look,
            zoom_text: String::new(),
            format,
        }
    }

    fn rule(&self) -> TextRule {
        TextRule {
            level: self.level.saturating_sub(1) as usize,
            fonts: self.fonts_on.then(|| super::bookmark_dialogs::word_list(&self.fonts)),
            size: self.size_on.then_some((self.size, self.tolerance)),
            color: self.color_on.then_some([self.color.r, self.color.g, self.color.b]),
            case: self.case,
            multiline: self.multiline,
            pattern: self.pattern_on.then(|| self.pattern.clone()),
            pattern_case: self.pattern_case,
            restrict_to_pattern: self.restrict,
        }
    }
}

/// State of "Erstelle Lesezeichen aus Seitentext".
#[derive(Debug, Clone, PartialEq)]
pub struct FromTextState {
    pub rules: Vec<(TextRule, Appearance, FormatEdit)>,
    pub selected: Option<usize>,
    pub range: PageRange,
    pub placement: Placement,
    pub stop_on: bool,
    pub stop_words: String,
    pub edit_stop: bool,
    pub skip_repeated: bool,
    pub editing: Option<Box<RuleEdit>>,
    pub error: Option<String>,
}

fn rule_summary(de: bool, r: &TextRule) -> String {
    let mut parts = vec![format!("{} {}", tr(de, "Ebene:", "Level:"), r.level + 1)];
    if let Some(f) = &r.fonts {
        parts.push(format!("{} {}", tr(de, "Schriftname:", "Font:"), f.join(", ")));
    }
    if let Some((s, t)) = r.size {
        parts.push(format!("{} {s:.1} pt (±{t:.1})", tr(de, "Schriftgröße:", "Font size:")));
    }
    if let Some([cr, cg, cb]) = r.color {
        parts.push(format!("{} RGB({cr},{cg},{cb})", tr(de, "Textfarbe:", "Colour:")));
    }
    if let Some(p) = &r.pattern {
        parts.push(format!("{} {p}", tr(de, "Muster:", "Pattern:")));
    }
    parts.join("\n")
}

impl MrPdfApp {
    pub(super) fn new_text_rules_state(&mut self) -> FromTextState {
        let rules = self
            .tab()
            .and_then(|t| t.doc.suggested_text_rules().ok())
            .unwrap_or_default()
            .into_iter()
            .map(|r| (r, Appearance::default(), FormatEdit::default()))
            .collect();
        FromTextState {
            rules,
            selected: None,
            range: PageRange::all(),
            placement: Placement::After,
            stop_on: false,
            stop_words: String::new(),
            edit_stop: false,
            skip_repeated: true,
            editing: None,
            error: None,
        }
    }

    /// Font, size and colour of the selected text ("Format von
    /// ausgewähltem Text verwenden").
    fn selection_style(&mut self) -> Option<(String, f32, [u8; 3])> {
        let tab = self.tab()?;
        let sel = tab.selection?;
        let text = tab.doc.page_text_styled(sel.page).ok()?;
        let i = (sel.range.start..=sel.range.end)
            .find(|&i| text.chars.get(i).is_some_and(|c| !c.generated && !c.ch.is_whitespace()))?;
        let style = text.styles.get(i)?;
        let font = text.fonts.get(style.font as usize).map(|f| f.name.clone()).unwrap_or_default();
        Some((font, style.size, style.color))
    }

    pub(super) fn text_rules_dialog(
        &mut self,
        ui: &mut Ui,
        s: &mut FromTextState,
    ) -> (Outcome, Option<Action>) {
        let de = self.bm_de();
        let (n, cur) = self.active_tab().map_or((0, 0), |t| (t.doc.page_count(), t.current_page));
        if let Some(edit) = s.editing.as_mut() {
            super::dialogs::set_title(
                ui,
                tr(de, "Einstellungen zur Erstellung von Lesezeichen", "Bookmark creation settings"),
            );
            let mut use_selection = false;
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(190.0);
                    ui.selectable_value(&mut edit.page, 0, tr(de, "Einstellungen zur Texterkennung", "Text recognition"));
                    ui.selectable_value(&mut edit.page, 1, tr(de, "Aussehen", "Appearance"));
                    ui.selectable_value(&mut edit.page, 2, tr(de, "Formatierung", "Formatting"));
                });
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.set_width(540.0);
                    match edit.page {
                    0 => {
                        ui.label(tr(
                            de,
                            "Wählen Sie folgende Textattribute, um automatisiert Lesezeichen aus dem Text zu erstellen:",
                            "Choose the text attributes that make a line a bookmark:",
                        ));
                        ui.horizontal(|ui| {
                            ui.label(tr(de, "Lesezeichenebene:", "Bookmark level:"));
                            ui.add(egui::DragValue::new(&mut edit.level).range(1..=32));
                        });
                        section(ui, tr(de, "Textformat", "Text format"), |ui| {
                            ui.checkbox(&mut edit.fonts_on, tr(de, "Schriftart(en):", "Font(s):"));
                            ui.add_enabled(
                                edit.fonts_on,
                                egui::TextEdit::multiline(&mut edit.fonts).desired_rows(3).desired_width(360.0).hint_text(tr(
                                    de,
                                    "Ein Schriftname pro Zeile",
                                    "One font name per line",
                                )),
                            );
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut edit.size_on, tr(de, "Schriftgröße:", "Font size:"));
                                ui.add_enabled(edit.size_on, egui::DragValue::new(&mut edit.size).range(1.0..=500.0).speed(0.1).suffix(" pt"));
                                ui.label(tr(de, "Toleranz:", "Tolerance:"));
                                ui.add_enabled(
                                    edit.size_on,
                                    egui::DragValue::new(&mut edit.tolerance).range(0.0..=20.0).speed(0.05).suffix(" pt"),
                                );
                            });
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut edit.color_on, tr(de, "Textfarbe:", "Text colour:"));
                                ui.add_enabled_ui(edit.color_on, |ui| color_button(ui, &mut edit.color));
                            });
                            use_selection = ui
                                .button(tr(de, "Format von ausgewähltem Text verwenden", "Use the format of the selected text"))
                                .clicked();
                        });
                        section(ui, tr(de, "Regeln für den Textinhalt", "Rules for the text"), |ui| {
                            ui.horizontal(|ui| {
                                ui.label(tr(de, "Groß-/Kleinschreibung:", "Case:"));
                                egui::ComboBox::from_id_salt("rule-case").selected_text(edit.case.label(de)).width(240.0).show_ui(
                                    ui,
                                    |ui| {
                                        for c in CaseRule::ALL {
                                            ui.selectable_value(&mut edit.case, c, c.label(de));
                                        }
                                    },
                                );
                            });
                            ui.checkbox(&mut edit.multiline, tr(de, "Mehrzeilige Lesezeichentitel zulassen", "Allow titles over several lines"));
                            ui.checkbox(&mut edit.pattern_on, tr(de, "Textmuster (Regulärer Ausdruck)", "Text pattern (regular expression)"));
                            ui.add_enabled(edit.pattern_on, egui::TextEdit::singleline(&mut edit.pattern).desired_width(340.0));
                            ui.add_enabled_ui(edit.pattern_on, |ui| {
                                ui.horizontal(|ui| {
                                    ui.checkbox(&mut edit.pattern_case, tr(de, "Groß/Klein beachten", "Match case"));
                                    ui.checkbox(&mut edit.restrict, tr(de, "Lesezeichentitel auf Textmuster einschränken", "Title is the matched text only"));
                                });
                            });
                            if edit.pattern_on
                                && let Err(e) = edit.rule().compile()
                            {
                                ui.colored_label(ui.visuals().error_fg_color, e.to_string());
                            }
                        });
                    }
                    1 => appearance_ui(ui, de, &mut edit.look, &mut edit.zoom_text),
                    _ => format_ui(ui, de, &mut edit.format),
                    }
                });
            });
            if use_selection {
                match self.selection_style() {
                    Some((font, size, color)) => {
                        edit.fonts_on = true;
                        edit.fonts = font;
                        edit.size_on = true;
                        edit.size = (size * 10.0).round() / 10.0;
                        edit.color_on = true;
                        edit.color = Rgba::rgb(color[0], color[1], color[2]);
                    }
                    None => {
                        s.error = Some(
                            tr(
                                de,
                                "Bitte zuerst Text auf der Seite markieren.",
                                "Please select text on the page first.",
                            )
                            .into(),
                        )
                    }
                }
            }
            let valid = edit.rule().compile().is_ok();
            let (ok, cancel, _) = footer(ui, de, valid, false, "OK");
            if ok {
                let entry = (edit.rule(), edit.look, edit.format.clone());
                match edit.index {
                    Some(i) if i < s.rules.len() => s.rules[i] = entry,
                    _ => s.rules.push(entry),
                }
                s.editing = None;
            } else if cancel {
                s.editing = None;
            }
            return (Outcome::Keep, None);
        }

        super::dialogs::set_title(
            ui,
            tr(de, "Erstelle Lesezeichen aus Seitentext", "Create bookmarks from page text"),
        );
        let mut open_edit: Option<Option<usize>> = None;
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(300.0);
                ui.horizontal(|ui| {
                    if ui.button(tr(de, "Hinzufügen…", "Add…")).clicked() {
                        open_edit = Some(None);
                    }
                    if ui
                        .add_enabled(s.selected.is_some(), egui::Button::new(tr(de, "Bearbeiten…", "Edit…")))
                        .clicked()
                    {
                        open_edit = Some(s.selected);
                    }
                    if ui
                        .add_enabled(s.selected.is_some(), egui::Button::new(tr(de, "Löschen", "Delete")))
                        .clicked()
                        && let Some(i) = s.selected.take()
                    {
                        s.rules.remove(i.min(s.rules.len() - 1));
                    }
                });
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("bm-rules")
                        .min_scrolled_height(260.0)
                        .max_height(260.0)
                        .show(ui, |ui| {
                            ui.set_width(280.0);
                            if s.rules.is_empty() {
                                ui.label(RichText::new(tr(de, "Noch keine Regeln.", "No rules yet.")).weak());
                            }
                            for (i, (rule, _, _)) in s.rules.iter().enumerate() {
                                let resp = ui.selectable_label(s.selected == Some(i), rule_summary(de, rule));
                                if resp.clicked() {
                                    s.selected = Some(i);
                                }
                                if resp.double_clicked() {
                                    open_edit = Some(Some(i));
                                }
                            }
                        });
                });
            });
            ui.vertical(|ui| {
                ui.set_width(330.0);
                s.range.ui(ui, de, n, cur, &PageRange::ALL_MODES);
                section(ui, tr(de, "Einstellungen", "Settings"), |ui| {
                    ui.radio_value(
                        &mut s.placement,
                        Placement::Replace,
                        tr(de, "Existierende Lesezeichen ersetzen", "Replace existing bookmarks"),
                    );
                    ui.radio_value(
                        &mut s.placement,
                        Placement::Merge,
                        tr(
                            de,
                            "Existierende Lesezeichen mit neuen zusammenführen",
                            "Merge existing with new bookmarks",
                        ),
                    )
                    .on_hover_text(tr(
                        de,
                        "Die oberste Ebene wird nach Seiten geordnet.",
                        "The top level is ordered by page.",
                    ));
                    let insert = matches!(s.placement, Placement::Before | Placement::After);
                    if ui
                        .radio(
                            insert,
                            tr(
                                de,
                                "Neue Lesezeichen in existierende Lesezeichen einfügen:",
                                "Insert new bookmarks into existing ones:",
                            ),
                        )
                        .clicked()
                        && !insert
                    {
                        s.placement = Placement::After;
                    }
                    ui.horizontal(|ui| {
                        ui.add_space(20.0);
                        ui.label(tr(de, "Bereich:", "Position:"));
                        ui.add_enabled_ui(insert, |ui| {
                            segmented(
                                ui,
                                &mut s.placement,
                                &[
                                    (Placement::Before, tr(de, "Vor", "Before").into()),
                                    (Placement::After, tr(de, "Nach", "After").into()),
                                ],
                            );
                        });
                    });
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.checkbox(
                            &mut s.stop_on,
                            tr(de, "Text mit Stoppwörtern ignorieren", "Ignore text with stop words"),
                        );
                        if ui
                            .button("…")
                            .on_hover_text(tr(de, "Stoppwörter bearbeiten", "Edit stop words"))
                            .clicked()
                        {
                            s.edit_stop = !s.edit_stop;
                        }
                    });
                    if s.edit_stop {
                        ui.add(egui::TextEdit::multiline(&mut s.stop_words).desired_rows(3).hint_text(tr(
                            de,
                            "Ein Wort pro Zeile",
                            "One word per line",
                        )));
                    }
                    ui.checkbox(
                        &mut s.skip_repeated,
                        tr(
                            de,
                            "Aufeinanderfolgende doppelte Lesezeichen ignorieren",
                            "Ignore repeated bookmarks",
                        ),
                    );
                });
            });
        });
        if let Some(err) = &s.error {
            ui.colored_label(ui.visuals().error_fg_color, err.as_str());
        }
        if let Some(index) = open_edit {
            s.error = None;
            let edit = match index.and_then(|i| s.rules.get(i)) {
                Some((r, l, f)) => RuleEdit::from_rule(index, r, *l, f.clone()),
                None => {
                    let level = s.rules.len().min(31);
                    RuleEdit::from_rule(
                        None,
                        &TextRule { level, ..Default::default() },
                        Appearance::default(),
                        FormatEdit::default(),
                    )
                }
            };
            s.editing = Some(Box::new(edit));
        }
        let pages = s.range.resolve(n, cur);
        let (ok, cancel, _) = footer(ui, de, !s.rules.is_empty() && pages.is_some(), false, "OK");
        let mut action = None;
        if ok && let Some(pages) = pages {
            let opts = TextBookmarkOptions {
                rules: s.rules.iter().map(|(r, l, f)| (r.clone(), *l, f.to_format())).collect(),
                pages,
                placement: s.placement,
                stop_words: s.stop_on.then(|| super::bookmark_dialogs::word_list(&s.stop_words)),
                skip_repeated: s.skip_repeated,
            };
            action = Some(Box::new(move |app: &mut MrPdfApp, ctx: &egui::Context| {
                let result = app.tab().map(|t| t.doc.add_text_bookmarks(&opts));
                let label = if app.bm_de() { "Neue Lesezeichen" } else { "New bookmarks" };
                app.outline_result(ctx, result, label);
                app.side = super::SidePanel::Bookmarks;
                app.settings.show_sidebar = true;
            }) as Action);
        }
        (if ok || cancel { Outcome::Close } else { Outcome::Keep }, action)
    }
}

// ---------------------------------------------------------------------------
// Überprüfen

/// State of "Lesezeichen überprüfen".
#[derive(Debug, Clone, PartialEq)]
pub struct CheckState {
    pub wanted: Vec<Problem>,
    /// None: not checked yet.
    pub findings: Option<Vec<Finding>>,
    pub selected: Vec<usize>,
}

impl Default for CheckState {
    fn default() -> Self {
        Self { wanted: Problem::ALL.to_vec(), findings: None, selected: Vec::new() }
    }
}

impl MrPdfApp {
    pub(super) fn check_dialog(&mut self, ui: &mut Ui, s: &mut CheckState) -> (Outcome, Option<Action>) {
        let de = self.bm_de();
        let ctx = ui.ctx().clone();
        super::dialogs::set_title(ui, tr(de, "Lesezeichen überprüfen", "Check bookmarks"));
        for p in Problem::ALL {
            let mut on = s.wanted.contains(&p);
            if ui.checkbox(&mut on, p.option(de)).changed() {
                s.wanted.retain(|x| *x != p);
                if on {
                    s.wanted.push(p);
                }
            }
        }
        ui.add_enabled(
            false,
            egui::Checkbox::new(
                &mut false,
                tr(de, "Verweise zu externen Dateien überprüfen", "Check links to external files"),
            ),
        );
        let mut delete = false;
        let mut edit = None;
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.horizontal(|ui| {
                let one = s.selected.len() == 1;
                if ui.add_enabled(one, egui::Button::new(tr(de, "Bearbeiten…", "Edit…"))).clicked() {
                    edit = s.selected.first().copied();
                }
                delete = ui
                    .add_enabled(!s.selected.is_empty(), egui::Button::new(tr(de, "Löschen", "Delete")))
                    .clicked();
            });
            egui::ScrollArea::vertical()
                .id_salt("bm-check")
                .min_scrolled_height(160.0)
                .max_height(200.0)
                .show(ui, |ui| {
                    ui.set_width(440.0);
                    match &s.findings {
                        None => {
                            ui.label(
                                RichText::new(tr(de, "Noch nicht überprüft.", "Not checked yet.")).weak(),
                            );
                        }
                        Some(f) if f.is_empty() => {
                            ui.label(tr(de, "Keine Probleme gefunden.", "No problems found."));
                        }
                        Some(f) => {
                            for (i, finding) in f.iter().enumerate() {
                                let title = if finding.title.trim().is_empty() {
                                    "–"
                                } else {
                                    finding.title.as_str()
                                };
                                let label = format!("{title}   ({})", finding.problem.text(de));
                                if ui.selectable_label(s.selected.contains(&i), label).clicked() {
                                    if ui.input(|i| i.modifiers.command || i.modifiers.shift) {
                                        if !s.selected.contains(&i) {
                                            s.selected.push(i);
                                        }
                                    } else {
                                        s.selected = vec![i];
                                    }
                                }
                            }
                        }
                    }
                });
        });
        let paths = |s: &CheckState| -> Vec<Vec<usize>> {
            s.findings.as_ref().map_or(Vec::new(), |f| {
                s.selected.iter().filter_map(|i| f.get(*i)).map(|x| x.path.clone()).collect()
            })
        };
        let mut run = false;
        let mut close = false;
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !s.selected.is_empty(),
                    egui::Button::new(tr(de, "Lesezeichen markieren", "Mark bookmark")),
                )
                .clicked()
                && let Some(path) = paths(s).into_iter().next()
            {
                if let Some(tab) = self.tab() {
                    tab.selected_bookmark = Some(path);
                }
                self.side = super::SidePanel::Bookmarks;
                self.settings.show_sidebar = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                close = ui.button(tr(de, "Schließen", "Close")).clicked();
                run = ui
                    .add_enabled(
                        !s.wanted.is_empty(),
                        egui::Button::new(RichText::new(tr(de, "Überprüfen", "Check")).strong()),
                    )
                    .clicked();
            });
        });
        if delete {
            let p = paths(s);
            let result = self.tab().map(|t| t.doc.delete_bookmark_paths(&p));
            self.outline_result(&ctx, result, tr(de, "Gelöschte Lesezeichen", "Deleted bookmarks"));
            run = true;
        }
        if run {
            match self.tab().map(|t| t.doc.check_bookmarks(&s.wanted)) {
                Some(Ok(f)) => s.findings = Some(f),
                Some(Err(e)) => self.show_error(&e),
                None => {}
            }
            s.selected.clear();
        }
        if let Some(i) = edit
            && let Some(f) = s.findings.as_ref().and_then(|f| f.get(i)).cloned()
        {
            let cur = self.active_tab().map_or(0, |t| t.current_page);
            if let Some(tab) = self.tab() {
                tab.selected_bookmark = Some(f.path.clone());
            }
            let action: Action = Box::new(move |app, _| {
                app.dialog = Some(Dialog::BookmarkTitle {
                    parent: None,
                    path: Some(f.path),
                    page: cur,
                    title: f.title,
                });
            });
            return (Outcome::Close, Some(action));
        }
        let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
        (if close || esc { Outcome::Close } else { Outcome::Keep }, None)
    }
}

// ---------------------------------------------------------------------------
// Inhaltsverzeichnis erstellen

/// "Typ" of the contents pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TocSize {
    /// "Abhängig von der Position".
    ByPosition,
    /// "Nach Seite".
    ByPage,
    Standard,
    Custom,
}

/// State of "Inhaltsverzeichnis erstellen".
#[derive(Debug, Clone, PartialEq)]
pub struct TocState {
    pub opts: TocOptions,
    pub title_on: bool,
    pub title: String,
    pub before: u32,
    pub last: bool,
    pub size: TocSize,
    pub size_page: u32,
    pub paper: PaperChoice,
    pub landscape: bool,
    pub custom: (f32, f32),
    pub unit: Unit,
    pub all_levels: bool,
    pub top: u32,
    pub deepest: u32,
    pub numbering_on: bool,
}

impl TocState {
    pub fn new(de: bool) -> Self {
        let title = tr(de, "Inhaltsverzeichnis", "Contents").to_string();
        Self {
            opts: TocOptions { title: Some(title.clone()), ..Default::default() },
            title_on: true,
            title,
            before: 1,
            last: false,
            size: TocSize::Standard,
            size_page: 1,
            paper: PaperChoice::A4,
            landscape: false,
            custom: (595.28, 841.89),
            unit: Unit::Mm,
            all_levels: true,
            top: 1,
            deepest: 999,
            numbering_on: false,
        }
    }
}

const LEADERS: [(Option<Leader>, &str); 4] = [
    (Some(Leader::Dots), "Kapitel……….1"),
    (Some(Leader::Line), "Kapitel______1"),
    (Some(Leader::Space), "Kapitel          1"),
    (None, "Kapitel"),
];

impl MrPdfApp {
    pub(super) fn toc_dialog(&mut self, ui: &mut Ui, s: &mut TocState) -> (Outcome, Option<Action>) {
        let de = self.bm_de();
        let n = self.active_tab().map_or(0, |t| t.doc.page_count());
        super::dialogs::set_title(ui, tr(de, "Inhaltsverzeichnis erstellen", "Create table of contents"));
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(320.0);
                section(ui, tr(de, "Titeleinstellungen", "Title"), |ui| {
                    ui.checkbox(&mut s.title_on, tr(de, "Titel erstellen", "Create title"));
                    ui.add_enabled(
                        s.title_on,
                        egui::TextEdit::multiline(&mut s.title).desired_rows(2).desired_width(290.0),
                    );
                    ui.add_enabled_ui(s.title_on, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(tr(de, "Schriftgröße:", "Font size:"));
                            ui.add(
                                egui::DragValue::new(&mut s.opts.title_size).range(4.0..=72.0).suffix(" pt"),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.label(tr(de, "Unterer Abstand:", "Space below:"));
                            length(ui, &mut s.opts.title_gap, s.unit, 0.0..=200.0);
                        });
                    });
                });
                section(ui, tr(de, "Ebenen", "Levels"), |ui| {
                    ui.horizontal(|ui| {
                        if ui.button(tr(de, "Hinzufügen", "Add")).clicked() {
                            let last = s.opts.levels.last().copied().unwrap_or(TocLevel {
                                size: 10.0,
                                bold: false,
                                color: Rgba::BLACK,
                            });
                            s.opts.levels.push(TocLevel { size: (last.size - 1.0).max(6.0), ..last });
                        }
                        if ui
                            .add_enabled(
                                s.opts.levels.len() > 1,
                                egui::Button::new(tr(de, "Löschen", "Delete")),
                            )
                            .clicked()
                        {
                            s.opts.levels.pop();
                        }
                    });
                    egui::ScrollArea::vertical().id_salt("toc-levels").max_height(200.0).show(ui, |ui| {
                        egui::Grid::new("toc-levels-grid").num_columns(4).spacing([6.0, 4.0]).show(
                            ui,
                            |ui| {
                                for (i, lv) in s.opts.levels.iter_mut().enumerate() {
                                    ui.label(format!("{} {}", tr(de, "Ebene", "Level"), i + 1));
                                    ui.add(
                                        egui::DragValue::new(&mut lv.size).range(4.0..=72.0).suffix(" pt"),
                                    );
                                    ui.checkbox(&mut lv.bold, tr(de, "Fett", "Bold"));
                                    color_button(ui, &mut lv.color);
                                    ui.end_row();
                                }
                            },
                        );
                    });
                    hint(
                        ui,
                        tr(
                            de,
                            "Tiefere Ebenen nutzen die letzte Einstellung.",
                            "Deeper levels use the last style.",
                        ),
                    );
                });
            });
            ui.vertical(|ui| {
                ui.set_width(470.0);
                section(ui, tr(de, "Seiteneinstellungen", "Page settings"), |ui| {
                    ui.horizontal(|ui| {
                        ui.label(tr(de, "Inhaltsverzeichnis vor Seite:", "Contents before page:"));
                        ui.add_enabled(
                            !s.last,
                            egui::DragValue::new(&mut s.before).range(1..=n.max(1) as u32),
                        );
                        ui.checkbox(&mut s.last, tr(de, "Als letzte Seite", "As last page"));
                    });
                    ui.horizontal(|ui| {
                        ui.label(tr(de, "Typ:", "Type:"));
                        segmented(
                            ui,
                            &mut s.size,
                            &[
                                (
                                    TocSize::ByPosition,
                                    tr(de, "Abhängig von der Position", "By position").into(),
                                ),
                                (TocSize::ByPage, tr(de, "Nach Seite", "By page").into()),
                                (TocSize::Standard, "Standard".into()),
                                (TocSize::Custom, tr(de, "Eigene", "Custom").into()),
                            ],
                        );
                    });
                    match s.size {
                        TocSize::ByPage => {
                            ui.horizontal(|ui| {
                                ui.label(tr(de, "Seite:", "Page:"));
                                ui.add(egui::DragValue::new(&mut s.size_page).range(1..=n.max(1) as u32));
                            });
                        }
                        TocSize::Standard => {
                            ui.horizontal(|ui| {
                                ui.label(tr(de, "Größe:", "Size:"));
                                let label = |p: PaperChoice| match p {
                                    PaperChoice::A3 => "A3",
                                    PaperChoice::A5 => "A5",
                                    PaperChoice::Letter => "Letter",
                                    PaperChoice::Legal => "Legal",
                                    _ => "A4",
                                };
                                egui::ComboBox::from_id_salt("toc-paper")
                                    .selected_text(label(s.paper))
                                    .show_ui(ui, |ui| {
                                        for p in [
                                            PaperChoice::A4,
                                            PaperChoice::A3,
                                            PaperChoice::A5,
                                            PaperChoice::Letter,
                                            PaperChoice::Legal,
                                        ] {
                                            ui.selectable_value(&mut s.paper, p, label(p));
                                        }
                                    });
                            });
                        }
                        TocSize::Custom => {
                            ui.horizontal(|ui| {
                                ui.label(tr(de, "Breite:", "Width:"));
                                length(ui, &mut s.custom.0, s.unit, 72.0..=14400.0);
                                ui.label(tr(de, "Höhe:", "Height:"));
                                length(ui, &mut s.custom.1, s.unit, 72.0..=14400.0);
                            });
                        }
                        TocSize::ByPosition => {}
                    }
                    ui.add_enabled_ui(matches!(s.size, TocSize::Standard | TocSize::Custom), |ui| {
                        ui.horizontal(|ui| {
                            ui.label(tr(de, "Ausrichtung:", "Orientation:"));
                            segmented(
                                ui,
                                &mut s.landscape,
                                &[
                                    (false, tr(de, "Hochformat", "Portrait").into()),
                                    (true, tr(de, "Querformat", "Landscape").into()),
                                ],
                            );
                        });
                    });
                    ui.label(tr(de, "Seitenränder:", "Margins:"));
                    egui::Grid::new("toc-margins").num_columns(4).spacing([6.0, 4.0]).show(ui, |ui| {
                        let [l, t, r, b] = &mut s.opts.margins;
                        ui.label(tr(de, "Links:", "Left:"));
                        length(ui, l, s.unit, 0.0..=300.0);
                        ui.label(tr(de, "Oben:", "Top:"));
                        length(ui, t, s.unit, 0.0..=300.0);
                        ui.end_row();
                        ui.label(tr(de, "Rechts:", "Right:"));
                        length(ui, r, s.unit, 0.0..=300.0);
                        ui.label(tr(de, "Unten:", "Bottom:"));
                        length(ui, b, s.unit, 0.0..=300.0);
                        ui.end_row();
                    });
                });
                section(ui, tr(de, "Lesezeichen", "Bookmarks"), |ui| {
                    ui.checkbox(
                        &mut s.all_levels,
                        tr(de, "Alle Lesezeichenebenen verwenden", "Use all bookmark levels"),
                    );
                    ui.add_enabled_ui(!s.all_levels, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(tr(de, "Oberste Lesezeichenebene für Verzeichnis:", "Top level:"));
                            ui.add(egui::DragValue::new(&mut s.top).range(1..=999));
                        });
                        ui.horizontal(|ui| {
                            ui.label(tr(de, "Tiefste Lesezeichenebene für Verzeichnis:", "Deepest level:"));
                            ui.add(egui::DragValue::new(&mut s.deepest).range(1..=999));
                        });
                    });
                    ui.checkbox(
                        &mut s.opts.skip_invalid,
                        tr(de, "Ungültige Lesezeichen überspringen", "Skip invalid bookmarks"),
                    );
                    ui.checkbox(
                        &mut s.opts.remove_repeated,
                        tr(
                            de,
                            "Mehrfache Lesezeichen aus Lesezeichentiteln entfernen",
                            "Remove repeated bookmarks",
                        ),
                    );
                });
                section(ui, tr(de, "Seitennummerierung", "Page numbers"), |ui| {
                    egui::Grid::new("toc-numbers").num_columns(2).spacing([6.0, 4.0]).show(ui, |ui| {
                        ui.label(tr(de, "Seitenzahlen ausgeben als:", "Show page numbers as:"));
                        let label = |p: TocPageNumbers| match p {
                            TocPageNumbers::Original => {
                                tr(de, "Original Seitenzahlen", "Original page numbers")
                            }
                            TocPageNumbers::Label => {
                                tr(de, "Paginierung (falls vorhanden)", "Page labels (if any)")
                            }
                        };
                        egui::ComboBox::from_id_salt("toc-pn")
                            .selected_text(label(s.opts.page_numbers))
                            .width(200.0)
                            .show_ui(ui, |ui| {
                                for p in [TocPageNumbers::Original, TocPageNumbers::Label] {
                                    ui.selectable_value(&mut s.opts.page_numbers, p, label(p));
                                }
                            });
                        ui.end_row();
                        ui.label(tr(de, "Format:", "Format:"));
                        let current =
                            LEADERS.iter().find(|(l, _)| *l == s.opts.leader).map_or("", |(_, t)| t);
                        egui::ComboBox::from_id_salt("toc-leader")
                            .selected_text(current)
                            .width(200.0)
                            .show_ui(ui, |ui| {
                                for (l, t) in LEADERS {
                                    ui.selectable_value(&mut s.opts.leader, l, t);
                                }
                            });
                        ui.end_row();
                    });
                });
                section(ui, tr(de, "Inhaltsnummerierung", "Entry numbering"), |ui| {
                    ui.horizontal(|ui| {
                        ui.label(tr(de, "Stil:", "Style:"));
                        let mut style = s.opts.numbering.unwrap_or_default();
                        egui::ComboBox::from_id_salt("toc-num-on")
                            .selected_text(if s.numbering_on {
                                style.label()
                            } else {
                                tr(de, "Keine", "None")
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut s.numbering_on, false, tr(de, "Keine", "None"));
                                for st in crate::outline_format::NumberStyle::ALL {
                                    if ui
                                        .selectable_label(s.numbering_on && style == st, st.label())
                                        .clicked()
                                    {
                                        s.numbering_on = true;
                                        style = st;
                                    }
                                }
                            });
                        s.opts.numbering = s.numbering_on.then_some(style);
                    });
                    ui.add_enabled_ui(s.numbering_on, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(tr(de, "Präfix:", "Prefix:"));
                            ui.add(egui::TextEdit::singleline(&mut s.opts.number_prefix).desired_width(80.0));
                            ui.label(tr(de, "Starten mit:", "Start at:"));
                            ui.add(egui::DragValue::new(&mut s.opts.number_start).range(1..=9999));
                        });
                        if let Some(st) = s.opts.numbering {
                            let preview: Vec<String> = (0..3)
                                .map(|i| {
                                    format!("{}{}", s.opts.number_prefix, st.format(s.opts.number_start + i))
                                })
                                .collect();
                            ui.label(format!(
                                "{} {}, …",
                                tr(de, "Vorschau:", "Preview:"),
                                preview.join(", ")
                            ));
                        }
                    });
                });
                section(ui, tr(de, "Ausgabeeinstellungen", "Output"), |ui| {
                    ui.horizontal(|ui| {
                        ui.label(tr(de, "Zeilenabstand:", "Line spacing:"));
                        length(ui, &mut s.opts.spacing, s.unit, 0.0..=100.0);
                        ui.label(tr(de, "Einrücken:", "Indent:"));
                        length(ui, &mut s.opts.indent, s.unit, 0.0..=200.0);
                    });
                });
            });
        });
        let (ok, cancel, _) = footer(ui, de, n > 0, false, tr(de, "Erstellen", "Create"));
        let mut action = None;
        if ok {
            let mut opts = s.opts.clone();
            opts.title = (s.title_on && !s.title.trim().is_empty()).then(|| s.title.clone());
            opts.before = (!s.last).then(|| s.before.saturating_sub(1) as usize);
            opts.level_range = (!s.all_levels).then(|| (s.top as usize, s.deepest.max(s.top) as usize));
            let orient =
                |(w, h): (f32, f32)| if s.landscape { (w.max(h), w.min(h)) } else { (w.min(h), w.max(h)) };
            opts.size = match s.size {
                TocSize::ByPosition => None,
                TocSize::ByPage => {
                    let p = s.size_page.saturating_sub(1) as usize;
                    self.active_tab().and_then(|t| t.doc.geometry(p)).map(|g| g.view_size())
                }
                TocSize::Standard => Some(orient(s.paper.size().unwrap_or(crate::report::A4))),
                TocSize::Custom => Some(orient(s.custom)),
            };
            action = Some(Box::new(move |app: &mut MrPdfApp, ctx: &egui::Context| {
                let result = app.tab().map(|t| t.doc.insert_toc_with(&opts));
                let label = if app.bm_de() { "Eingefügte Seiten" } else { "Inserted pages" };
                app.outline_result(ctx, result, label);
                if let (Some(b), Some(tab)) = (opts.before, app.tab()) {
                    tab.jump_to_page(b);
                }
            }) as Action);
        }
        (if ok || cancel { Outcome::Close } else { Outcome::Keep }, action)
    }
}

// ---------------------------------------------------------------------------
// Verweise aus Lesezeichen

/// State of "Verweise aus Lesezeichen erstellen".
#[derive(Debug, Clone, PartialEq)]
pub struct LinksState {
    pub scope: ScopeState,
    pub range: PageRange,
    pub fonts_on: bool,
    pub fonts: String,
    pub size_on: bool,
    pub size: f32,
    pub tolerance: f32,
    pub color_on: bool,
    pub color: Rgba,
    pub look: LinkLook,
    pub opts: LinkOptions,
    pub pattern_on: bool,
    pub pattern: String,
}

impl Default for LinksState {
    fn default() -> Self {
        Self {
            scope: ScopeState::default(),
            range: PageRange::all(),
            fonts_on: false,
            fonts: String::new(),
            size_on: false,
            size: 12.0,
            tolerance: 0.2,
            color_on: false,
            color: Rgba::BLACK,
            look: LinkLook { highlight: LinkHighlight::None, ..Default::default() },
            opts: LinkOptions { ignore_case: true, ..Default::default() },
            pattern_on: false,
            pattern: String::new(),
        }
    }
}

impl MrPdfApp {
    pub(super) fn links_dialog(&mut self, ui: &mut Ui, s: &mut LinksState) -> (Outcome, Option<Action>) {
        let de = self.bm_de();
        let (n, cur) = self.active_tab().map_or((0, 0), |t| (t.doc.page_count(), t.current_page));
        let selection = self.selected_bookmark();
        super::dialogs::set_title(
            ui,
            tr(de, "Verweise aus Lesezeichen erstellen", "Create links from bookmarks"),
        );
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(250.0);
                scope_ui(ui, de, &mut s.scope, selection.is_some(), tr(de, "Lesezeichen:", "Bookmarks:"));
            });
            ui.vertical(|ui| s.range.ui(ui, de, n, cur, &PageRange::ALL_MODES));
        });
        section(ui, tr(de, "Textformat", "Text format"), |ui| {
            ui.horizontal_top(|ui| {
                ui.checkbox(&mut s.fonts_on, tr(de, "Schriftart(en):", "Font(s):"));
                ui.add_enabled(
                    s.fonts_on,
                    egui::TextEdit::multiline(&mut s.fonts)
                        .desired_rows(2)
                        .desired_width(320.0)
                        .hint_text(tr(de, "Ein Schriftname pro Zeile", "One font name per line")),
                );
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut s.size_on, tr(de, "Schriftgröße:", "Font size:"));
                ui.add_enabled(
                    s.size_on,
                    egui::DragValue::new(&mut s.size).range(1.0..=500.0).speed(0.1).suffix(" pt"),
                );
                ui.label(tr(de, "Toleranz:", "Tolerance:"));
                ui.add_enabled(
                    s.size_on,
                    egui::DragValue::new(&mut s.tolerance).range(0.0..=20.0).speed(0.05).suffix(" pt"),
                );
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut s.color_on, tr(de, "Textfarbe:", "Text colour:"));
                ui.add_enabled_ui(s.color_on, |ui| color_button(ui, &mut s.color));
            });
        });
        // Two columns of equal width (a row would give the first all the room).
        ui.columns(2, |cols| {
            let [left, right] = cols else { return };
            section(left, tr(de, "Verknüpfungen", "Links"), |ui| {
                egui::Grid::new("links-look").num_columns(2).spacing([6.0, 4.0]).show(ui, |ui| {
                    ui.label(tr(de, "Strichfarbe:", "Line colour:"));
                    color_button(ui, &mut s.look.color);
                    ui.end_row();
                    ui.label(tr(de, "Rahmen:", "Border:"));
                    let border = |b: LinkBorder| match b {
                        LinkBorder::Solid => tr(de, "Durchgehend", "Solid"),
                        LinkBorder::Dashed => tr(de, "Gestrichelt", "Dashed"),
                        LinkBorder::Underline => tr(de, "Unterstrichen", "Underline"),
                        LinkBorder::Inset => tr(de, "Eingelassen", "Inset"),
                        LinkBorder::Beveled => tr(de, "Abgeschrägt", "Beveled"),
                    };
                    egui::ComboBox::from_id_salt("links-border")
                        .selected_text(border(s.look.border))
                        .show_ui(ui, |ui| {
                            for b in [
                                LinkBorder::Solid,
                                LinkBorder::Dashed,
                                LinkBorder::Underline,
                                LinkBorder::Inset,
                                LinkBorder::Beveled,
                            ] {
                                ui.selectable_value(&mut s.look.border, b, border(b));
                            }
                        });
                    ui.end_row();
                    ui.label(tr(de, "Rahmenbreite:", "Border width:"));
                    ui.add(
                        egui::DragValue::new(&mut s.look.width).range(0.0..=12.0).speed(0.1).suffix(" pt"),
                    );
                    ui.end_row();
                    ui.label(tr(de, "Hervorhebung:", "Highlight:"));
                    let hl = |h: LinkHighlight| match h {
                        LinkHighlight::None => tr(de, "Keine", "None"),
                        LinkHighlight::Invert => tr(de, "Umkehren", "Invert"),
                        LinkHighlight::Outline => tr(de, "Umriss", "Outline"),
                        LinkHighlight::Push => tr(de, "Eindrücken", "Push"),
                    };
                    egui::ComboBox::from_id_salt("links-hl").selected_text(hl(s.look.highlight)).show_ui(
                        ui,
                        |ui| {
                            for h in [
                                LinkHighlight::None,
                                LinkHighlight::Invert,
                                LinkHighlight::Outline,
                                LinkHighlight::Push,
                            ] {
                                ui.selectable_value(&mut s.look.highlight, h, hl(h));
                            }
                        },
                    );
                    ui.end_row();
                });
            });
            section(right, tr(de, "Einstellungen", "Settings"), |ui| {
                ui.vertical(|ui| {
                    ui.checkbox(
                        &mut s.opts.ignore_case,
                        tr(de, "Groß-/Kleinschreibung ignorieren", "Ignore case"),
                    );
                    ui.checkbox(
                        &mut s.opts.ignore_spaces,
                        tr(de, "Aufeinanderfolgende Leerzeichen ignorieren", "Ignore repeated spaces"),
                    );
                    ui.checkbox(
                        &mut s.opts.whole_words,
                        tr(de, "Nur ganze Wörter vergleichen", "Whole words only"),
                    );
                    ui.checkbox(
                        &mut s.opts.over_existing,
                        tr(de, "Verweise über bestehenden erzeugen", "Create links over existing ones"),
                    );
                    ui.checkbox(
                        &mut s.opts.trim,
                        tr(de, "Weißraum aus Lesezeichen entfernen", "Trim white space of titles"),
                    );
                    ui.checkbox(
                        &mut s.pattern_on,
                        tr(de, "Textmuster (Regulärer Ausdruck)", "Text pattern (regular expression)"),
                    )
                    .on_hover_text(tr(
                        de,
                        "Nur Lesezeichen, deren Titel zum Muster passt.",
                        "Only bookmarks whose title matches.",
                    ));
                    ui.add_enabled(
                        s.pattern_on,
                        egui::TextEdit::singleline(&mut s.pattern).desired_width(220.0),
                    );
                });
            });
        });
        let pages = s.range.resolve(n, cur);
        let (ok, cancel, _) = footer(ui, de, pages.is_some(), false, "OK");
        let mut action = None;
        if ok && let Some(pages) = pages {
            let filter = (s.fonts_on || s.size_on || s.color_on).then(|| TextRule {
                fonts: s.fonts_on.then(|| super::bookmark_dialogs::word_list(&s.fonts)),
                size: s.size_on.then_some((s.size, s.tolerance)),
                color: s.color_on.then_some([s.color.r, s.color.g, s.color.b]),
                ..Default::default()
            });
            let opts = LinkOptions {
                scope: s.scope.scope(selection.as_ref()),
                pages,
                filter,
                look: s.look,
                pattern: s.pattern_on.then(|| s.pattern.clone()),
                ..s.opts.clone()
            };
            action = Some(doc_action("Neue Verweise", "New links", move |doc| {
                doc.links_from_bookmarks_with(&opts)
            }));
        }
        (if ok || cancel { Outcome::Close } else { Outcome::Keep }, action)
    }
}
