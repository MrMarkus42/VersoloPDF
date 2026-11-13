//! Ribbon tab "Überprüfen": spell check of comments,
//! form fields and page text, plus the comment menus.

use std::path::{Path, PathBuf};

use egui::{Context, RichText, Ui};

use super::commands::Cmd;
use super::dialogs::Outcome;
use super::page_dialogs::tr;
use super::ribbon::Entry;
use super::{Dialog, RangePurpose, VersoloPdfApp};
use crate::model::{FieldKind, SearchHit};
use crate::spell::{Dictionary, find_dictionaries, words};
use crate::text::TextRange;
use crate::ui::i18n::Language;
use crate::ui::icons::Icon;
use crate::ui::tab::AnnotRef;
use crate::ui::tools::Tool;
use crate::ui::widgets::group;

/// Where a misspelt word stands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Place {
    Comment(AnnotRef),
    /// Text field: page and widget index.
    Field(usize, usize),
    /// Page text: page (cannot be changed).
    Page(usize),
}

impl Place {
    pub fn page(self) -> usize {
        match self {
            Place::Comment(a) => a.page,
            Place::Field(p, _) | Place::Page(p) => p,
        }
    }
}

/// A word the dictionary does not know.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub place: Place,
    pub word: String,
    /// Position in the comment, field or page text (characters, end exclusive).
    pub start: usize,
    pub end: usize,
    pub context: String,
}

/// State of "Rechtschreibprüfung".
#[derive(Debug, Clone, PartialEq)]
pub struct SpellState {
    pub dictionaries: Vec<(String, PathBuf)>,
    pub chosen: Option<PathBuf>,
    pub comments: bool,
    pub fields: bool,
    pub page_text: bool,
    /// None: not checked yet.
    pub findings: Option<Vec<Finding>>,
    pub selected: usize,
    pub replacement: String,
    /// Suggestions for the selected finding (None: not computed yet).
    pub suggestions: Option<Vec<String>>,
    pub error: Option<String>,
}

fn context_of(text: &str, start: usize, end: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    let a = start.saturating_sub(25);
    let b = (end + 25).min(chars.len());
    let s: String = chars[a..b].iter().map(|&c| if c.is_control() { ' ' } else { c }).collect();
    format!("{}{s}{}", if a > 0 { "…" } else { "" }, if b < chars.len() { "…" } else { "" })
}

/// `text` with the characters `start..end` replaced by `new`.
pub fn replace_chars(text: &str, start: usize, end: usize, new: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let (start, end) = (start.min(chars.len()), end.min(chars.len()));
    let mut out: String = chars[..start].iter().collect();
    out.push_str(new);
    out.extend(&chars[end..]);
    out
}

fn unknown_words(dict: &Dictionary, text: &str, place: Place, out: &mut Vec<Finding>) {
    for (start, end, word) in words(text) {
        if !dict.check(&word) {
            out.push(Finding { place, context: context_of(text, start, end), word, start, end });
        }
    }
}

impl VersoloPdfApp {
    fn rv_de(&self) -> bool {
        self.settings.language == Language::German
    }

    /// Loads the dictionary (once) with the personal words.
    fn ensure_dictionary(&mut self, path: &Path) -> crate::Result<()> {
        if self.spell_dict.as_ref().is_some_and(|(p, _)| p == path) {
            return Ok(());
        }
        let mut d = Dictionary::load(path)?;
        for w in &self.settings.spell_personal {
            d.add_personal(w);
        }
        self.spell_dict = Some((path.to_path_buf(), d));
        Ok(())
    }

    pub(super) fn open_spell_check(&mut self) {
        let dictionaries = find_dictionaries();
        let preferred = if self.rv_de() { "de" } else { "en" };
        let chosen = self
            .settings
            .spell_dictionary
            .clone()
            .filter(|p| p.is_file())
            .or_else(|| dictionaries.iter().find(|(n, _)| n.starts_with(preferred)).map(|(_, p)| p.clone()))
            .or_else(|| dictionaries.first().map(|(_, p)| p.clone()));
        self.dialog = Some(Dialog::SpellCheck(Box::new(SpellState {
            dictionaries,
            chosen,
            comments: true,
            fields: true,
            page_text: false,
            findings: None,
            selected: 0,
            replacement: String::new(),
            suggestions: None,
            error: None,
        })));
    }

    /// The unknown words of the active document.
    pub fn spell_findings(
        &mut self,
        dict: &Dictionary,
        comments: bool,
        fields: bool,
        page_text: bool,
    ) -> Vec<Finding> {
        let mut out = Vec::new();
        let Some(tab) = self.tabs.get_mut(self.active) else { return out };
        for page in 0..tab.doc.page_count() {
            if comments {
                for a in tab.annotations(page).iter().filter(|a| a.kind.is_comment()) {
                    unknown_words(
                        dict,
                        &a.contents,
                        Place::Comment(AnnotRef { page, index: a.index }),
                        &mut out,
                    );
                }
            }
            if fields {
                let text_fields = tab.fields(page);
                for f in
                    text_fields.iter().filter(|f| matches!(f.kind, FieldKind::Text { password: false, .. }))
                {
                    unknown_words(dict, &f.value, Place::Field(page, f.annot_index), &mut out);
                }
            }
            if page_text && let Some(text) = tab.page_text(page) {
                // One character per page text character, so positions match.
                let plain: String = text.chars.iter().map(|c| c.ch).collect();
                unknown_words(dict, &plain, Place::Page(page), &mut out);
            }
            if out.len() > 10_000 {
                break;
            }
        }
        out
    }

    /// Goes to the page of a finding and marks it.
    fn show_finding(&mut self, f: &Finding) {
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        match f.place {
            Place::Comment(a) => tab.selected_annot = Some(a),
            Place::Field(page, index) => tab.selected_annot = Some(AnnotRef { page, index }),
            Place::Page(page) => {
                let rects = tab
                    .page_text(page)
                    .map(|t| t.line_rects(TextRange::new(f.start, f.end.saturating_sub(1))))
                    .unwrap_or_default();
                tab.search.hits = vec![SearchHit { page, rects, context: f.context.clone() }];
                tab.search.current = Some(0);
            }
        }
        tab.jump_to_page(f.place.page());
    }

    /// "Ändern": replaces the word in its comment or form field.
    pub fn change_finding(&mut self, ctx: &Context, f: &Finding, new: &str) {
        let Some(tab) = self.tab() else { return };
        match f.place {
            Place::Comment(a) => {
                let Some(old) =
                    tab.annotations(a.page).iter().find(|x| x.index == a.index).map(|x| x.contents.clone())
                else {
                    return;
                };
                let update = crate::pdf_edit::AnnotUpdate {
                    contents: Some(replace_chars(&old, f.start, f.end, new)),
                    ..Default::default()
                };
                self.edit(ctx, |tab| tab.doc.update_annotation(a.page, a.index, &update));
            }
            Place::Field(page, index) => {
                let Some(old) =
                    tab.fields(page).iter().find(|x| x.annot_index == index).map(|x| x.value.clone())
                else {
                    return;
                };
                let value = replace_chars(&old, f.start, f.end, new);
                self.edit(ctx, |tab| tab.doc.set_text_field(page, index, &value));
            }
            Place::Page(_) => {}
        }
    }

    /// Runs the check with the chosen dictionary.
    pub fn run_spell_check(&mut self, s: &mut SpellState) {
        let Some(path) = s.chosen.clone() else { return };
        if let Err(err) = self.ensure_dictionary(&path) {
            s.error = Some(err.to_string());
            return;
        }
        self.settings.spell_dictionary = Some(path);
        let Some((p, dict)) = self.spell_dict.take() else { return };
        s.findings = Some(self.spell_findings(&dict, s.comments, s.fields, s.page_text));
        self.spell_dict = Some((p, dict));
        s.selected = 0;
        s.suggestions = None;
        s.error = None;
    }

    pub(super) fn spell_dialog(&mut self, ui: &mut Ui, s: &mut SpellState) -> Outcome {
        let de = self.rv_de();
        let ctx = ui.ctx().clone();
        super::dialogs::set_title(ui, tr(de, "Rechtschreibprüfung", "Spell check"));
        ui.set_max_width(640.0);
        ui.horizontal(|ui| {
            ui.label(tr(de, "Wörterbuch:", "Dictionary:"));
            let shown = match &s.chosen {
                Some(p) => p.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                None => tr(de, "(keines gefunden)", "(none found)").to_string(),
            };
            egui::ComboBox::from_id_salt("spell-dict").selected_text(shown).width(170.0).show_ui(ui, |ui| {
                for (name, path) in &s.dictionaries {
                    if ui.selectable_label(s.chosen.as_ref() == Some(path), name).clicked() {
                        s.chosen = Some(path.clone());
                        s.findings = None;
                    }
                }
            });
            if ui.button(tr(de, "Andere…", "Other…")).clicked()
                && let Some(p) = rfd::FileDialog::new().add_filter("Hunspell", &["dic"]).pick_file()
            {
                s.chosen = Some(p);
                s.findings = None;
            }
        });
        if s.chosen.is_none() {
            ui.label(
                RichText::new(tr(
                    de,
                    "VersoloPDF bringt kein Wörterbuch mit. Es nutzt Hunspell-Wörterbücher (.dic mit .aff daneben), wie sie LibreOffice mitbringt. Mit „Andere…“ die .dic-Datei wählen.",
                    "VersoloPDF ships no dictionary. It uses Hunspell dictionaries (.dic with the .aff beside it), as LibreOffice ships them. Choose the .dic file with \"Other…\".",
                ))
                .small()
                .weak(),
            );
        }
        ui.horizontal(|ui| {
            ui.label(tr(de, "Prüfen:", "Check:"));
            ui.checkbox(&mut s.comments, tr(de, "Kommentare", "Comments"));
            ui.checkbox(&mut s.fields, tr(de, "Formularfelder", "Form fields"));
            ui.checkbox(&mut s.page_text, tr(de, "Seitentext (nur anzeigen)", "Page text (show only)"));
        });
        let can_check = s.chosen.is_some() && !self.tabs.is_empty();
        let start = egui::Button::new(RichText::new(tr(de, "Prüfung starten", "Start check")).strong());
        if ui.add_enabled(can_check, start).clicked() {
            self.run_spell_check(s);
        }
        if let Some(err) = &s.error {
            ui.colored_label(ui.visuals().error_fg_color, err.as_str());
        }
        ui.add_space(4.0);
        let mut action = None;
        if let Some(findings) = &s.findings {
            if findings.is_empty() {
                ui.label(
                    RichText::new(tr(de, "Keine unbekannten Wörter gefunden.", "No unknown words found."))
                        .strong(),
                );
            } else {
                s.selected = s.selected.min(findings.len() - 1);
                ui.label(if de {
                    format!("Unbekannte Wörter: {}", findings.len())
                } else {
                    format!("Unknown words: {}", findings.len())
                });
                let mut pick = None;
                ui.horizontal_top(|ui| {
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        egui::ScrollArea::vertical().id_salt("spell-list").max_height(240.0).show(ui, |ui| {
                            ui.set_width(220.0);
                            for (i, f) in findings.iter().enumerate() {
                                let label =
                                    format!("{}  ({} {})", f.word, tr(de, "S.", "p."), f.place.page() + 1);
                                if ui.selectable_label(s.selected == i, label).clicked() {
                                    pick = Some(i);
                                }
                            }
                        });
                    });
                    ui.vertical(|ui| {
                        let f = &findings[s.selected];
                        let kind = match f.place {
                            Place::Comment(_) => tr(de, "In einem Kommentar:", "In a comment:"),
                            Place::Field(..) => tr(de, "In einem Formularfeld:", "In a form field:"),
                            Place::Page(_) => tr(de, "Im Seitentext:", "In the page text:"),
                        };
                        ui.label(RichText::new(kind).weak());
                        ui.label(&f.context);
                        ui.add_space(4.0);
                        let editable = !matches!(f.place, Place::Page(_));
                        ui.horizontal(|ui| {
                            ui.label(tr(de, "Ändern in:", "Change to:"));
                            let edit = egui::TextEdit::singleline(&mut s.replacement).desired_width(170.0);
                            ui.add_enabled(editable, edit);
                        });
                        ui.label(RichText::new(tr(de, "Vorschläge:", "Suggestions:")).weak());
                        egui::ScrollArea::vertical().id_salt("spell-sugg").max_height(110.0).show(ui, |ui| {
                            match s.suggestions.as_deref() {
                                Some([]) => {
                                    ui.label(RichText::new(tr(de, "(keine)", "(none)")).weak());
                                }
                                Some(list) => {
                                    for sug in list {
                                        if ui.selectable_label(s.replacement == *sug, sug).clicked() {
                                            s.replacement = sug.clone();
                                        }
                                    }
                                }
                                None => {}
                            }
                        });
                        ui.horizontal_wrapped(|ui| {
                            let change = egui::Button::new(tr(de, "Ändern", "Change"));
                            if ui.add_enabled(editable && !s.replacement.is_empty(), change).clicked() {
                                action = Some(SpellAction::Change);
                            }
                            if ui.button(tr(de, "Ignorieren", "Ignore")).clicked() {
                                action = Some(SpellAction::Ignore);
                            }
                            if ui.button(tr(de, "Alle ignorieren", "Ignore all")).clicked() {
                                action = Some(SpellAction::IgnoreAll);
                            }
                            if ui.button(tr(de, "Zum Wörterbuch hinzufügen", "Add to dictionary")).clicked()
                            {
                                action = Some(SpellAction::Add);
                            }
                        });
                    });
                });
                if let Some(i) = pick {
                    s.selected = i;
                    s.suggestions = None;
                }
            }
        }
        if let Some(a) = action {
            self.spell_action(&ctx, s, a);
        }
        // Suggestions for the newly selected word; its place is shown too.
        if s.suggestions.is_none()
            && let Some(f) = s.findings.as_ref().and_then(|v| v.get(s.selected)).cloned()
        {
            let list = self.spell_dict.as_ref().map(|(_, d)| d.suggest(&f.word, 8)).unwrap_or_default();
            s.replacement = list.first().cloned().unwrap_or_else(|| f.word.clone());
            s.suggestions = Some(list);
            self.show_finding(&f);
        }
        ui.add_space(6.0);
        let mut close = false;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            close = ui.button(tr(de, "Schließen", "Close")).clicked();
        });
        if close || ui.input(|i| i.key_pressed(egui::Key::Escape)) { Outcome::Close } else { Outcome::Keep }
    }

    fn spell_action(&mut self, ctx: &Context, s: &mut SpellState, action: SpellAction) {
        let Some(findings) = s.findings.as_mut() else { return };
        let Some(f) = findings.get(s.selected).cloned() else { return };
        match action {
            SpellAction::Change => {
                let new = s.replacement.clone();
                self.change_finding(ctx, &f, &new);
                // Later words in the same text move with the change.
                let delta = new.chars().count() as isize - (f.end - f.start) as isize;
                findings.remove(s.selected);
                for o in findings.iter_mut().filter(|o| o.place == f.place && o.start >= f.end) {
                    o.start = o.start.saturating_add_signed(delta);
                    o.end = o.end.saturating_add_signed(delta);
                }
            }
            SpellAction::Ignore => {
                findings.remove(s.selected);
            }
            SpellAction::IgnoreAll => findings.retain(|o| o.word != f.word),
            SpellAction::Add => {
                if !self.settings.spell_personal.contains(&f.word) {
                    self.settings.spell_personal.push(f.word.clone());
                }
                if let Some((_, d)) = self.spell_dict.as_mut() {
                    d.add_personal(&f.word);
                }
                findings.retain(|o| o.word != f.word);
            }
        }
        s.suggestions = None;
    }

    /// "Anmerkungen dieser Seite löschen".
    pub(super) fn delete_page_comments(&mut self, ctx: &Context) {
        let Some(tab) = self.tab() else { return };
        let page = tab.current_page;
        let indices: Vec<usize> =
            tab.annotations(page).iter().filter(|a| a.kind.is_comment()).map(|a| a.index).collect();
        if indices.is_empty() {
            return;
        }
        self.edit(ctx, |tab| {
            tab.selected_annot = None;
            tab.doc.delete_annotations(page, &indices)
        });
    }

    /// "Alle Anmerkungen einbetten" (all pages, without asking).
    pub(super) fn flatten_all(&mut self, ctx: &Context) {
        let Some(tab) = self.active_tab() else { return };
        let pages = (0..tab.doc.page_count()).collect();
        self.apply_range(ctx, RangePurpose::Flatten, pages);
    }

    /// The ribbon tab "Überprüfen".
    pub(super) fn ribbon_review(&mut self, ui: &mut Ui, ctx: &Context) {
        let de = self.rv_de();
        self.tools_group(ui, ctx);
        group(ui, tr(de, "Rechtschreibung", "Spelling"), |ui| {
            self.big(ui, ctx, Cmd::Tool(Tool::TextCorrection));
            self.big(ui, ctx, Cmd::SpellCheck);
            self.big(ui, ctx, Cmd::WordCount);
            self.big(ui, ctx, Cmd::CompareDocuments);
        });
        group(ui, tr(de, "Anmerkungen", "Comments"), |ui| {
            let add: Vec<Entry> = ADD_TOOLS.iter().map(|&t| Entry::C(Cmd::Tool(t))).collect();
            self.big_menu(ui, ctx, Icon::Note, ("Anmerkungen hinzufügen", "Add comments"), &add);
            self.big_menu(
                ui,
                ctx,
                Icon::Delete,
                ("Anmerkungen löschen", "Delete comments"),
                &[
                    Entry::C(Cmd::DeleteComment),
                    Entry::C(Cmd::DeletePageComments),
                    Entry::C(Cmd::DeleteAllComments),
                ],
            );
            self.big(ui, ctx, Cmd::PrevComment);
            self.big(ui, ctx, Cmd::NextComment);
            self.big(ui, ctx, Cmd::CommentList);
            self.show_menu(ui, ctx, true);
            self.big_menu(
                ui,
                ctx,
                Icon::Flatten,
                ("Anmerkungen einbetten", "Flatten comments"),
                &[Entry::C(Cmd::Flatten), Entry::C(Cmd::FlattenAll)],
            );
        });
    }
}

#[derive(Debug, Clone, Copy)]
enum SpellAction {
    Change,
    Ignore,
    IgnoreAll,
    Add,
}

/// "Anmerkungen hinzufügen ⏷".
const ADD_TOOLS: [Tool; 16] = [
    Tool::Note,
    Tool::Typewriter,
    Tool::TextBox,
    Tool::Callout,
    Tool::Highlight,
    Tool::Underline,
    Tool::StrikeOut,
    Tool::Squiggly,
    Tool::Replace,
    Tool::Caret,
    Tool::Rectangle,
    Tool::Ellipse,
    Tool::Line,
    Tool::Arrow,
    Tool::Pencil,
    Tool::Stamp,
];
