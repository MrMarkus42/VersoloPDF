//! Ribbon tab "Anmerkungen": layout, the display
//! filter ("Anzeigen"), pop-ups, calculators, text correction, the Format
//! tab of the active comment tool and "Formate für Anmerkungen".

use egui::{Context, RichText, Ui, vec2};

use super::VersoloPdfApp;
use super::commands::Cmd;
use super::dialogs::Outcome;
use crate::model::{AnnotKind, AnnotStyle, Rgba};
use crate::pdf_edit::NewAnnotation;
use crate::ui::i18n::Language;
use crate::ui::icons::Icon;
use crate::ui::tab::AnnotRef;
use crate::ui::tools::Tool;
use crate::ui::widgets::{big_menu_button, group, small_menu_button};

/// Tools listed in "Formate für Anmerkungen".
const STYLE_TOOLS: [Tool; 25] = [
    Tool::Highlight,
    Tool::AreaHighlight,
    Tool::ContentHighlight,
    Tool::Underline,
    Tool::StrikeOut,
    Tool::Squiggly,
    Tool::Replace,
    Tool::Caret,
    Tool::Note,
    Tool::FileAttach,
    Tool::Typewriter,
    Tool::TextBox,
    Tool::Callout,
    Tool::Line,
    Tool::Arrow,
    Tool::Rectangle,
    Tool::Ellipse,
    Tool::Polygon,
    Tool::Polyline,
    Tool::Cloud,
    Tool::Pencil,
    Tool::Stamp,
    Tool::MeasureDistance,
    Tool::MeasurePerimeter,
    Tool::MeasureArea,
];

/// The commands; used as `Cmd::Comment(..)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommentCmd {
    ShowAll,
    HideAll,
    ShowFlagged,
    HideSelected,
    OnlySelected,
    ShowAllAgain,
    OpenPopups,
    OpenNonEmptyPopups,
    ClosePopups,
    Calculator,
    CloseCalculators,
    CommentStyles,
    KeepTool,
    Exclusive,
    ToolProperties,
}

impl CommentCmd {
    pub fn names(self) -> (&'static str, &'static str) {
        use CommentCmd::*;
        match self {
            ShowAll => ("Alle Anmerkungen anzeigen", "Show all comments"),
            HideAll => ("Alle Anmerkungen verbergen", "Hide all comments"),
            ShowFlagged => ("Versteckte Anmerkungen anzeigen", "Show hidden comments"),
            HideSelected => ("Ausgewählte verbergen", "Hide selected"),
            OnlySelected => ("Nur ausgewählte anzeigen", "Show only selected"),
            ShowAllAgain => ("Wieder alle anzeigen", "Show all again"),
            OpenPopups => ("Alle Pop-ups öffnen", "Open all pop-ups"),
            OpenNonEmptyPopups => ("Alle nicht-leeren Kommentare öffnen", "Open all non-empty comments"),
            ClosePopups => ("Alle Pop-ups schließen", "Close all pop-ups"),
            Calculator => ("Rechner", "Calculator"),
            CloseCalculators => ("Alle schließen", "Close all"),
            CommentStyles => ("Formate für Anmerkungen", "Comment styles"),
            KeepTool => ("Auswahl beibehalten", "Keep tool selected"),
            Exclusive => ("Exklusiver Modus", "Exclusive mode"),
            ToolProperties => ("Eigenschaften", "Properties"),
        }
    }

    pub fn icon(self) -> Icon {
        use CommentCmd::*;
        match self {
            ShowAll | ShowFlagged | ShowAllAgain | OnlySelected => Icon::Eye,
            HideAll | HideSelected => Icon::Close,
            OpenPopups | OpenNonEmptyPopups | ClosePopups => Icon::Note,
            Calculator => Icon::Calculator,
            CloseCalculators => Icon::Close,
            CommentStyles => Icon::Theme,
            KeepTool => Icon::Pointer,
            Exclusive => Icon::Lock,
            ToolProperties => Icon::Properties,
        }
    }

    pub fn shortcut(self) -> Option<(&'static str, &'static str)> {
        match self {
            CommentCmd::ShowAll => Some(("Strg+8", "Ctrl+8")),
            CommentCmd::HideAll => Some(("Strg+Umschalt+8", "Ctrl+Shift+8")),
            _ => None,
        }
    }
}

/// What "Textkorrektur" does with the selected text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Correction {
    Delete,
    Insert,
    Replace(String),
}

/// Writes an embedded media file to the temporary folder; None if its
/// type is not a known media type (never executables or documents).
fn write_media_temp(name: &str, data: &[u8]) -> Option<std::path::PathBuf> {
    use crate::media::MediaKind;
    let clean: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' })
        .take(80)
        .collect();
    let ext = clean.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase())?;
    let known = [MediaKind::Video, MediaKind::Audio, MediaKind::Model3d]
        .iter()
        .any(|k| k.extensions().contains(&ext.as_str()));
    if !known {
        return None;
    }
    let dir = std::env::temp_dir().join("versolopdf-media");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("{}-{clean}", std::process::id()));
    std::fs::write(&path, data).ok()?;
    Some(path)
}

/// Opens a file with the program the system uses for its type (no shell).
fn open_with_system(path: &std::path::Path) {
    if crate::mail::no_external() {
        return;
    }
    let result = if cfg!(windows) {
        std::process::Command::new("explorer").arg(path).spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(path).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(path).spawn()
    };
    if let Err(err) = result {
        log::warn!("could not open media file: {err}");
    }
}

/// A ready-made style of the Format tab ("Formstil").
struct Preset {
    de: &'static str,
    en: &'static str,
    color: Rgba,
    /// Filled shape (false: only the frame).
    filled: bool,
}

const PRESET_COLORS: [(&str, &str, &str, Rgba); 7] = [
    ("Rot", "Roter Rahmen", "Red", Rgba::rgb(229, 57, 53)),
    ("Violett", "Violetter Rahmen", "Violet", Rgba::rgb(142, 36, 170)),
    ("Indigo", "Indigofarbener Rahmen", "Indigo", Rgba::rgb(63, 81, 181)),
    ("Blau", "Blauer Rahmen", "Blue", Rgba::rgb(30, 136, 229)),
    ("Grün", "Grüner Rahmen", "Green", Rgba::rgb(46, 160, 67)),
    ("Gelb", "Gelber Rahmen", "Yellow", Rgba::rgb(255, 235, 59)),
    ("Schwarz", "Schwarzer Rahmen", "Black", Rgba::rgb(0, 0, 0)),
];

fn presets(tool: Tool) -> Vec<Preset> {
    let mut out: Vec<Preset> = PRESET_COLORS
        .iter()
        .map(|(de, _, en, c)| Preset { de, en, color: *c, filled: tool.has_fill() })
        .collect();
    if tool.has_fill() {
        out.extend(PRESET_COLORS.iter().map(|(_, de, en, c)| Preset { de, en, color: *c, filled: false }));
    }
    out
}

fn preset_matches(p: &Preset, tool: Tool, s: &AnnotStyle) -> bool {
    s.color == p.color && (!tool.has_fill() || s.fill.is_some() == p.filled)
}

/// One entry of the "Formstil" gallery; returns true if clicked.
fn preset_button(ui: &mut Ui, p: &Preset, label: &str, selected: bool) -> bool {
    // The label wraps between words (centred, at most two lines in the
    // presets) instead of running into its neighbours.
    let font = egui::FontId::proportional(10.5);
    let widest_word = label
        .split_whitespace()
        .map(|w| ui.painter().layout_no_wrap(w.to_string(), font.clone(), egui::Color32::WHITE).size().x)
        .fold(0.0, f32::max);
    let width = (widest_word + 8.0).max(52.0);
    let mut job =
        egui::text::LayoutJob::simple(label.to_string(), font, ui.visuals().text_color(), width - 4.0);
    job.halign = egui::Align::Center;
    let galley = ui.painter().layout_job(job);
    let height = (32.0 + galley.size().y + 2.0).max(50.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(width, height), egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, label));
    let painter = ui.painter();
    if selected || resp.hovered() {
        painter.rect_filled(
            rect,
            3.0,
            ui.visuals().selection.bg_fill.gamma_multiply(if selected { 0.6 } else { 0.3 }),
        );
    }
    let swatch = egui::Rect::from_center_size(rect.center_top() + vec2(0.0, 16.0), vec2(28.0, 20.0));
    let c = crate::ui::widgets::to_color32(p.color);
    if p.filled {
        painter.rect_filled(swatch, 2.0, c);
    } else {
        painter.rect_stroke(swatch, 2.0, egui::Stroke::new(2.0, c), egui::StrokeKind::Inside);
    }
    painter.galley(egui::pos2(rect.center().x, rect.top() + 31.0), galley, ui.visuals().text_color());
    resp.on_hover_text(label).clicked()
}

impl VersoloPdfApp {
    fn cm_de(&self) -> bool {
        self.settings.language == Language::German
    }

    fn filter(&self) -> crate::model::AnnotFilter {
        self.active_tab().map(|t| t.doc.annot_filter().clone()).unwrap_or_default()
    }

    pub(super) fn comment_enabled(&self, cmd: CommentCmd) -> bool {
        use CommentCmd::*;
        let tab = self.active_tab();
        match cmd {
            Calculator | CommentStyles | KeepTool | Exclusive | ToolProperties => true,
            CloseCalculators => !self.calculators.is_empty(),
            HideSelected | OnlySelected => tab.is_some_and(|t| t.selected_annot.is_some()),
            ShowAllAgain => {
                let f = self.filter();
                f.only.is_some() || !f.hidden_annots.is_empty()
            }
            ClosePopups => tab.is_some_and(|t| !t.open_popups.is_empty()),
            _ => tab.is_some(),
        }
    }

    pub(super) fn comment_selected(&self, cmd: CommentCmd) -> bool {
        use CommentCmd::*;
        let tab = self.active_tab();
        match cmd {
            ShowAll => tab.is_some_and(|t| t.show_annotations && t.doc.annot_filter().is_empty()),
            HideAll => tab.is_some_and(|t| !t.show_annotations),
            ShowFlagged => self.filter().show_flagged,
            OnlySelected => self.filter().only.is_some(),
            KeepTool => self.settings.keeps_tool(self.tool),
            Exclusive => self.settings.exclusive_mode,
            ToolProperties => self.settings.show_properties,
            _ => false,
        }
    }

    pub(super) fn run_comment(&mut self, ctx: &Context, cmd: CommentCmd) {
        use CommentCmd::*;
        match cmd {
            ShowAll => {
                if let Some(tab) = self.tab() {
                    tab.show_annotations = true;
                    tab.set_annot_filter(Default::default());
                    tab.textures.clear();
                }
            }
            HideAll => {
                if let Some(tab) = self.tab() {
                    tab.show_annotations = false;
                    tab.textures.clear();
                }
            }
            ShowFlagged | HideSelected | OnlySelected | ShowAllAgain => {
                let Some(tab) = self.tab() else { return };
                let mut f = tab.doc.annot_filter().clone();
                let sel = tab.selected_annot.map(|a| (a.page, a.index));
                match cmd {
                    ShowFlagged => f.show_flagged = !f.show_flagged,
                    HideSelected => {
                        if let Some(s) = sel {
                            f.hidden_annots.push(s);
                            tab.selected_annot = None;
                        }
                    }
                    OnlySelected => f.only = sel.map(|s| vec![s]),
                    _ => {
                        f.only = None;
                        f.hidden_annots.clear();
                    }
                }
                tab.set_annot_filter(f);
            }
            OpenPopups | OpenNonEmptyPopups => {
                let Some(tab) = self.tab() else { return };
                let mut open = Vec::new();
                for page in 0..tab.doc.page_count().min(2000) {
                    for a in tab.annotations(page).iter() {
                        let has_text = !a.contents.trim().is_empty();
                        if a.kind.is_comment() && (cmd == OpenPopups || has_text) {
                            open.push(AnnotRef { page, index: a.index });
                        }
                    }
                }
                tab.open_popups = open;
            }
            ClosePopups => {
                if let Some(tab) = self.tab() {
                    tab.open_popups.clear();
                }
            }
            Calculator => self.open_calculator(),
            CloseCalculators => self.close_calculators(),
            CommentStyles => {
                let tool = if self.tool.annot_kind().is_some() { self.tool } else { Tool::Highlight };
                self.dialog = Some(super::Dialog::CommentStyles(tool));
            }
            KeepTool => {
                let keep = self.settings.keeps_tool(self.tool);
                self.settings.set_keeps_tool(self.tool, !keep);
            }
            Exclusive => self.settings.exclusive_mode = !self.settings.exclusive_mode,
            ToolProperties => self.settings.show_properties = !self.settings.show_properties,
        }
        let _ = ctx;
    }

    /// "Textkorrektur": acts on the selected text.
    pub(super) fn correct_text(&mut self, correction: Correction) {
        let style = |tool| self.settings.style(tool);
        let (strike, caret) = (style(Tool::StrikeOut), style(Tool::Caret));
        let replace = style(Tool::Replace);
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        let Some(sel) = tab.selection.take() else { return };
        let Some(text) = tab.page_text(sel.page) else { return };
        let quads = text.line_rects(sel.range);
        let words = text.text_in(sel.range);
        let Some(last) = quads.last().copied() else { return };
        let page = sel.page;
        let event = match correction {
            Correction::Delete => {
                let mut annot = NewAnnotation::new(AnnotKind::StrikeOut, strike);
                annot.quads = quads;
                annot.contents = words;
                crate::ui::canvas::CanvasEvent::Create { page, annot: Box::new(annot) }
            }
            Correction::Insert => {
                let mut annot = NewAnnotation::new(AnnotKind::Caret, caret);
                let h = last.height().max(6.0);
                annot.rect = crate::geometry::RectF::new(
                    last.x1 - 5.0,
                    last.y0 - h * 0.3,
                    last.x1 + 5.0,
                    last.y0 + h * 0.4,
                );
                crate::ui::canvas::CanvasEvent::RequestText {
                    page,
                    tool: Tool::Caret,
                    template: Box::new(annot),
                }
            }
            Correction::Replace(typed) => {
                let mut annot = NewAnnotation::new(AnnotKind::StrikeOut, replace);
                annot.quads = quads;
                annot.label = words;
                self.dialog = Some(super::Dialog::TextInput {
                    page,
                    tool: Tool::Replace,
                    template: Some(Box::new(annot)),
                    text: typed,
                    edit: None,
                });
                return;
            }
        };
        self.events.push(event);
    }

    /// Media tools: asks for the file and embeds it.
    pub(super) fn add_media_file(
        &mut self,
        ctx: &Context,
        page: usize,
        rect: crate::geometry::RectF,
        tool: Tool,
    ) {
        use crate::media::MediaKind;
        let lang = self.settings.language;
        let kind = match tool {
            Tool::Video => MediaKind::Video,
            Tool::Model3d => MediaKind::Model3d,
            _ => MediaKind::Audio,
        };
        let exts: &[&str] = if tool == Tool::SoundNote { &["wav"] } else { kind.extensions() };
        let Some(path) = rfd::FileDialog::new()
            .set_title(Cmd::Tool(tool).label(lang))
            .add_filter("Media", exts)
            .pick_file()
        else {
            return;
        };
        let too_big =
            std::fs::metadata(&path).map(|m| m.len() > crate::media::MAX_MEDIA as u64).unwrap_or(true);
        if too_big {
            let t = self.t();
            self.info(t.error, t.err_too_large.to_string());
            return;
        }
        match std::fs::read(&path) {
            Ok(data) => {
                let name =
                    path.file_name().map_or_else(|| "media".into(), |n| n.to_string_lossy().into_owned());
                self.add_media_bytes(ctx, page, rect, tool, &name, &data);
            }
            Err(e) => self.show_error(&crate::Error::Io { path, source: e }),
        }
    }

    /// Embeds `data` with a media tool (also used by tests).
    pub fn add_media_bytes(
        &mut self,
        ctx: &Context,
        page: usize,
        rect: crate::geometry::RectF,
        tool: Tool,
        name: &str,
        data: &[u8],
    ) {
        use crate::media::MediaKind;
        let author = self.settings.author.clone();
        let kind = match tool {
            Tool::Video => MediaKind::Video,
            Tool::Model3d => MediaKind::Model3d,
            _ => MediaKind::Audio,
        };
        if tool == Tool::SoundNote {
            self.edit(ctx, |tab| tab.doc.add_sound(page, rect, name, data, &author));
        } else {
            self.edit(ctx, |tab| tab.doc.add_media(page, rect, kind, name, data, &author));
        }
        self.tool = Tool::Hand;
    }

    /// Double click on a media comment: opens its file with the program
    /// the system uses for that type. Only media file types are opened.
    pub(super) fn play_media(&mut self, ctx: &Context, annot: AnnotRef) {
        let Some(tab) = self.tab() else { return };
        let file = match tab.doc.media_file(annot.page, annot.index) {
            Ok(Some(f)) => f,
            Ok(None) => return,
            Err(err) => {
                self.show_error(&err);
                return;
            }
        };
        let de = self.cm_de();
        match write_media_temp(&file.0, &file.1) {
            Some(path) => {
                open_with_system(&path);
                self.set_status(ctx, if de { "Medium wird abgespielt" } else { "Playing media" });
            }
            None => self.info(
                if de { "Abspielen" } else { "Play" },
                if de {
                    "Diese Datei ist kein bekanntes Medienformat und wird aus Sicherheitsgründen nicht geöffnet.".into()
                } else {
                    "This file is no known media format and is not opened for security reasons.".into()
                },
            ),
        }
    }

    /// The ribbon tab "Anmerkungen".
    pub(super) fn ribbon_comment(&mut self, ui: &mut Ui, ctx: &Context) {
        let de = self.cm_de();
        self.tools_group(ui, ctx);
        group(ui, if de { "Bearbeiten" } else { "Edit" }, |ui| {
            self.big(ui, ctx, Cmd::Tool(Tool::SelectAnnot));
        });
        group(ui, "Text", |ui| {
            self.smalls(
                ui,
                ctx,
                &[Cmd::Tool(Tool::Typewriter), Cmd::Tool(Tool::TextBox), Cmd::Tool(Tool::Callout)],
            );
        });
        group(ui, if de { "Notizen" } else { "Notes" }, |ui| {
            self.smalls(
                ui,
                ctx,
                &[Cmd::Tool(Tool::Note), Cmd::Tool(Tool::FileAttach), Cmd::Tool(Tool::SoundNote)],
            );
        });
        group(ui, if de { "Textmarkierung" } else { "Text markup" }, |ui| {
            self.smalls(
                ui,
                ctx,
                &[
                    Cmd::Tool(Tool::Highlight),
                    Cmd::Tool(Tool::AreaHighlight),
                    Cmd::Tool(Tool::ContentHighlight),
                ],
            );
            self.smalls(
                ui,
                ctx,
                &[Cmd::Tool(Tool::Underline), Cmd::Tool(Tool::StrikeOut), Cmd::Tool(Tool::Caret)],
            );
            self.smalls(
                ui,
                ctx,
                &[Cmd::Tool(Tool::Replace), Cmd::Tool(Tool::TextCorrection), Cmd::Tool(Tool::Squiggly)],
            );
            self.calculator_button(ui, ctx);
        });
        group(ui, if de { "Zeichnen" } else { "Drawing" }, |ui| {
            ui.vertical(|ui| {
                for row in [
                    &[Tool::Line, Tool::Ellipse, Tool::Cloud][..],
                    &[Tool::Arrow, Tool::Polyline],
                    &[Tool::Rectangle, Tool::Polygon],
                ] {
                    ui.horizontal(|ui| {
                        for &tool in row {
                            self.icon(ui, ctx, Cmd::Tool(tool));
                        }
                    });
                }
            });
            self.big(ui, ctx, Cmd::Tool(Tool::Stamp));
            ui.vertical(|ui| {
                ui.add_space(4.0);
                self.stamp_picker(ui);
                self.small(ui, ctx, Cmd::InsertImage);
            });
            self.smalls(ui, ctx, &[Cmd::Tool(Tool::Pencil), Cmd::Tool(Tool::Eraser)]);
        });
        group(ui, if de { "Messen" } else { "Measure" }, |ui| {
            self.smalls(
                ui,
                ctx,
                &[
                    Cmd::Tool(Tool::MeasureDistance),
                    Cmd::Tool(Tool::MeasurePerimeter),
                    Cmd::Tool(Tool::MeasureArea),
                ],
            );
        });
        group(ui, if de { "Medien" } else { "Media" }, |ui| {
            for tool in [Tool::Video, Tool::AudioClip, Tool::Model3d] {
                self.big(ui, ctx, Cmd::Tool(tool));
            }
        });
        group(ui, if de { "Anmerkungen verwalten" } else { "Manage comments" }, |ui| {
            self.big(ui, ctx, Cmd::SummarizeComments);
            ui.vertical(|ui| {
                self.small(ui, ctx, Cmd::ImportComments);
                self.small(ui, ctx, Cmd::ExportComments);
                self.show_menu(ui, ctx, false);
            });
            self.smalls(ui, ctx, &[Cmd::Flatten, Cmd::CommentList, Cmd::Comment(CommentCmd::CommentStyles)]);
        });
    }

    /// "Rechner" with its drop-down of open calculators.
    fn calculator_button(&mut self, ui: &mut Ui, ctx: &Context) {
        let de = self.cm_de();
        let resp = big_menu_button(
            ui,
            Icon::Calculator,
            if de { "Rechner" } else { "Calculator" },
            true,
            false,
        )
        .on_hover_text(if de {
            "Tischrechner mit Rechenstreifen; die Rechnung lässt sich als Textfeld auf der Seite einfügen."
        } else {
            "Desk calculator with a tape; the calculation can be placed on the page as a text box."
        });
        let mut chosen = None;
        egui::Popup::menu(&resp).show(|ui| {
            ui.set_min_width(220.0);
            let new = Cmd::Comment(CommentCmd::Calculator);
            if self.menu_row(ui, new) {
                chosen = Some(new);
            }
            ui.separator();
            let names = self.calculator_names();
            if names.is_empty() {
                ui.add_enabled(
                    false,
                    egui::Label::new(if de { "Keine Berechnungen" } else { "No calculations" }),
                );
            }
            for (id, name) in names {
                if ui.button(name).clicked() {
                    ctx.move_to_top(egui::LayerId::new(
                        egui::Order::Middle,
                        egui::Id::new(("calculator", id)),
                    ));
                    ui.close();
                }
            }
            let close = Cmd::Comment(CommentCmd::CloseCalculators);
            if self.menu_row(ui, close) {
                chosen = Some(close);
            }
        });
        if let Some(cmd) = chosen {
            self.run(ctx, cmd);
        }
    }

    /// "Anzeigen ▾": which comments are shown, and their pop-ups.
    pub(super) fn show_menu(&mut self, ui: &mut Ui, ctx: &Context, big: bool) {
        let de = self.cm_de();
        let label = if de { "Anzeigen" } else { "Show" };
        let resp = if big {
            big_menu_button(ui, Icon::Eye, label, !self.tabs.is_empty(), false)
        } else {
            small_menu_button(ui, Icon::Eye, label, !self.tabs.is_empty(), false)
        };
        let mut chosen = None;
        let t = self.t();
        egui::Popup::menu(&resp).show(|ui| {
            ui.set_min_width(300.0);
            let mut row = |app: &VersoloPdfApp, ui: &mut Ui, c: CommentCmd| {
                if app.menu_row(ui, Cmd::Comment(c)) {
                    chosen = Some(c);
                }
            };
            row(self, ui, CommentCmd::ShowAll);
            row(self, ui, CommentCmd::HideAll);
            ui.separator();
            row(self, ui, CommentCmd::ShowFlagged);
            ui.separator();
            // Kinds and authors that occur in the document.
            let (kinds, authors) = self.comment_kinds_and_authors();
            let filter = self.filter();
            let mut new_filter = filter.clone();
            ui.menu_button(if de { "Nach Typ" } else { "By type" }, |ui| {
                ui.set_min_width(200.0);
                if kinds.is_empty() {
                    ui.add_enabled(
                        false,
                        egui::Label::new(if de { "Keine Anmerkungen" } else { "No comments" }),
                    );
                }
                for kind in &kinds {
                    let mut on = !filter.hidden_kinds.contains(kind);
                    if ui.checkbox(&mut on, super::sidebar::kind_name(t, *kind)).changed() {
                        new_filter.hidden_kinds.retain(|k| k != kind);
                        if !on {
                            new_filter.hidden_kinds.push(*kind);
                        }
                    }
                }
            });
            ui.menu_button(if de { "Nach Autor" } else { "By author" }, |ui| {
                ui.set_min_width(200.0);
                if authors.is_empty() {
                    ui.add_enabled(
                        false,
                        egui::Label::new(if de { "Keine Anmerkungen" } else { "No comments" }),
                    );
                }
                for author in &authors {
                    let mut on = !filter.hidden_authors.contains(author);
                    let label = if author.is_empty() {
                        if de { "(ohne Autor)" } else { "(no author)" }
                    } else {
                        author
                    };
                    if ui.checkbox(&mut on, label).changed() {
                        new_filter.hidden_authors.retain(|a| a != author);
                        if !on {
                            new_filter.hidden_authors.push(author.clone());
                        }
                    }
                }
            });
            ui.menu_button(if de { "Ausgewählte" } else { "Selected" }, |ui| {
                ui.set_min_width(220.0);
                row(self, ui, CommentCmd::HideSelected);
                row(self, ui, CommentCmd::OnlySelected);
                row(self, ui, CommentCmd::ShowAllAgain);
            });
            ui.add_enabled_ui(false, |ui| {
                ui.menu_button(if de { "Nach Status" } else { "By status" }, |_| {});
            })
            .response
            .on_disabled_hover_text(if de {
                "VersoloPDF kennt noch keinen Prüfstatus für Anmerkungen."
            } else {
                "VersoloPDF does not know review states of comments yet."
            });
            ui.separator();
            row(self, ui, CommentCmd::OpenPopups);
            row(self, ui, CommentCmd::OpenNonEmptyPopups);
            row(self, ui, CommentCmd::ClosePopups);
            if new_filter != filter
                && let Some(tab) = self.tab()
            {
                tab.set_annot_filter(new_filter);
            }
            if chosen.is_some() {
                ui.close();
            }
        });
        if let Some(c) = chosen {
            self.run(ctx, Cmd::Comment(c));
        }
    }

    /// Comment kinds and authors of the active document (first 2000 pages).
    fn comment_kinds_and_authors(&mut self) -> (Vec<AnnotKind>, Vec<String>) {
        let (mut kinds, mut authors) = (Vec::new(), Vec::<String>::new());
        if let Some(tab) = self.tab() {
            for page in 0..tab.doc.page_count().min(2000) {
                for a in tab.annotations(page).iter().filter(|a| a.kind.is_comment()) {
                    if !kinds.contains(&a.kind) {
                        kinds.push(a.kind);
                    }
                    if !authors.contains(&a.author) {
                        authors.push(a.author.clone());
                    }
                }
            }
        }
        authors.sort();
        (kinds, authors)
    }

    /// Format tab of the active comment tool (its default style).
    pub(super) fn ribbon_tool_format(&mut self, ui: &mut Ui, ctx: &Context) {
        let de = self.cm_de();
        let tool = self.tool;
        let mut style = self.settings.style(tool);
        let before = style.clone();
        group(ui, if de { "Formstil" } else { "Style" }, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for p in presets(tool) {
                let label = if de { p.de } else { p.en };
                if preset_button(ui, &p, label, preset_matches(&p, tool, &style)) {
                    style.color = p.color;
                    if tool.has_fill() {
                        style.fill = p.filled.then_some(p.color);
                    }
                }
            }
        });
        group(ui, if de { "Farben" } else { "Colours" }, |ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    let mut c = crate::ui::widgets::to_color32(style.color);
                    if ui.color_edit_button_srgba(&mut c).changed() {
                        style.color = crate::ui::widgets::from_color32(c);
                    }
                    let line = tool.has_width() || tool.has_fill();
                    ui.label(match (de, line) {
                        (true, true) => "Linienfarbe",
                        (true, false) => "Farbe",
                        (false, true) => "Line colour",
                        (false, false) => "Colour",
                    });
                });
                if tool.has_fill() {
                    ui.horizontal(|ui| {
                        let mut c = crate::ui::widgets::to_color32(style.fill.unwrap_or(Rgba::WHITE));
                        if ui.color_edit_button_srgba(&mut c).changed() {
                            style.fill = Some(crate::ui::widgets::from_color32(c));
                        }
                        ui.label(if de { "Füllfarbe" } else { "Fill colour" });
                    });
                    let mut none = style.fill.is_none();
                    if ui.checkbox(&mut none, if de { "Keine Füllung" } else { "No fill" }).changed() {
                        style.fill = if none { None } else { Some(style.color) };
                    }
                }
            });
        });
        group(ui, if de { "Linie" } else { "Line" }, |ui| {
            // In a vertical layout the grid stays at the top of the group.
            ui.vertical(|ui| {
                egui::Grid::new("tool-format-line").num_columns(2).show(ui, |ui| {
                    if tool.has_width() {
                        ui.label(if de { "Breite:" } else { "Width:" });
                        ui.add(
                            egui::DragValue::new(&mut style.width).range(0.0..=20.0).speed(0.1).suffix(" pt"),
                        );
                        ui.end_row();
                    }
                    ui.label(if de { "Deckkraft:" } else { "Opacity:" });
                    let mut pct = (style.opacity * 100.0).round();
                    if ui.add(egui::DragValue::new(&mut pct).range(10.0..=100.0).suffix(" %")).changed() {
                        style.opacity = pct / 100.0;
                    }
                    ui.end_row();
                    if tool.has_font() {
                        ui.label(if de { "Schriftgröße:" } else { "Font size:" });
                        ui.add(
                            egui::DragValue::new(&mut style.font_size)
                                .range(4.0..=144.0)
                                .speed(0.2)
                                .suffix(" pt"),
                        );
                        ui.end_row();
                    }
                });
            });
        });
        group(ui, if de { "Eigenschaften" } else { "Properties" }, |ui| {
            self.smalls(
                ui,
                ctx,
                &[
                    Cmd::Comment(CommentCmd::KeepTool),
                    Cmd::Comment(CommentCmd::Exclusive),
                    Cmd::Comment(CommentCmd::ToolProperties),
                ],
            );
        });
        if style != before {
            self.settings.set_style(tool, style);
        }
    }

    /// "Formate für Anmerkungen": default styles of all comment tools.
    pub(super) fn comment_styles_dialog(&mut self, ui: &mut Ui, selected: &mut Tool) -> Outcome {
        let de = self.cm_de();
        let lang = self.settings.language;
        super::dialogs::set_title(ui, if de { "Formate für Anmerkungen" } else { "Comment styles" });
        let tools = STYLE_TOOLS;
        ui.horizontal_top(|ui| {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(220.0);
                egui::ScrollArea::vertical().max_height(340.0).show(ui, |ui| {
                    ui.set_width(220.0);
                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        for &tool in &tools {
                            let style = self.settings.style(tool);
                            ui.horizontal(|ui| {
                                let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), egui::Sense::hover());
                                ui.painter().rect_filled(r, 2.0, crate::ui::widgets::to_color32(style.color));
                                if ui
                                    .selectable_label(*selected == tool, Cmd::Tool(tool).label(lang))
                                    .clicked()
                                {
                                    *selected = tool;
                                }
                            });
                        }
                    });
                });
            });
            ui.vertical(|ui| {
                ui.set_width(260.0);
                ui.label(RichText::new(Cmd::Tool(*selected).label(lang)).strong());
                let t = self.t();
                let mut style = self.settings.style(*selected);
                if super::properties::style_editor(ui, t, *selected, &mut style) {
                    self.settings.set_style(*selected, style);
                }
                ui.add_space(8.0);
                if ui.button(if de { "Auf Standard zurücksetzen" } else { "Reset to default" }).clicked() {
                    self.settings.reset_style(*selected);
                }
                if ui.button(if de { "Alle zurücksetzen" } else { "Reset all" }).clicked() {
                    self.settings.styles.clear();
                }
            });
        });
        let (ok, cancel, _) =
            super::page_dialogs::footer(ui, de, true, false, if de { "Schließen" } else { "Close" });
        if ok || cancel { Outcome::Close } else { Outcome::Keep }
    }
}
