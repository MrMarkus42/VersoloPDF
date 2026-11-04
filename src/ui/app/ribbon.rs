//! Quick access toolbar, ribbon tabs and the quick start command search.
//!
//! The tabs and groups of the ribbon.

use egui::{Context, Frame, Margin, Panel, RichText, Ui};

use super::commands::Side;
use super::commands::{Cmd, SEARCHABLE, Unavailable};
use super::structure::MarkKind;
use super::{MrPdfApp, RibbonTab, SidePanel};
use crate::model::{Rgba, StampPreset};
use crate::pdf_edit::AnnotUpdate;
use crate::ui::i18n::Language;
use crate::ui::icons::Icon;
use crate::ui::theme;
use crate::ui::tools::{PALETTE, Tool};
use crate::ui::widgets::{
    RibbonFit, big_button, big_menu_button, group, icon_button_named, menu_item, ribbon_fit, set_ribbon_fit,
    small_button, small_menu_button, swatch,
};

const UA: fn(Unavailable, &'static str, &'static str, Icon) -> Cmd = Cmd::Unavailable;

/// An entry of a ribbon drop-down menu.
pub(super) enum Entry {
    C(Cmd),
    Sep,
    /// Sub menu: icon, (German, English) label, commands.
    Sub(Icon, (&'static str, &'static str), &'static [Cmd]),
}

/// Topic colour of a ribbon tab (same colours as the icons).
fn tab_color(tab: RibbonTab) -> egui::Color32 {
    use egui::Color32 as C;
    match tab {
        RibbonTab::Home | RibbonTab::View | RibbonTab::Organize | RibbonTab::Bookmarks => theme::ACCENT,
        RibbonTab::Comment => C::from_rgb(234, 88, 12),
        RibbonTab::Protect => C::from_rgb(217, 119, 6),
        RibbonTab::Form => C::from_rgb(124, 58, 237),
        RibbonTab::Convert | RibbonTab::Share => C::from_rgb(13, 148, 136),
        RibbonTab::Review | RibbonTab::Accessibility => C::from_rgb(22, 163, 74),
        RibbonTab::Help => C::from_gray(130),
        RibbonTab::Format | RibbonTab::Arrange => C::from_rgb(200, 140, 0),
    }
}

impl MrPdfApp {
    pub(super) fn big(&mut self, ui: &mut Ui, ctx: &Context, cmd: Cmd) {
        let label = cmd.label(self.settings.language);
        let resp = big_button(ui, cmd.icon(), label, self.cmd_selected(cmd), self.cmd_enabled(cmd));
        self.finish_button(ui, ctx, cmd, resp);
    }

    pub(super) fn small(&mut self, ui: &mut Ui, ctx: &Context, cmd: Cmd) {
        let label = cmd.label(self.settings.language);
        let resp = small_button(ui, cmd.icon(), label, self.cmd_selected(cmd), self.cmd_enabled(cmd));
        self.finish_button(ui, ctx, cmd, resp);
    }

    /// Tooltip and click of a ribbon button; comment tools also get their
    /// hold-down palette of style presets.
    fn finish_button(&mut self, ui: &mut Ui, ctx: &Context, cmd: Cmd, resp: egui::Response) {
        let mut tip = self.cmd_tooltip(cmd);
        let mut run = true;
        if let Cmd::Tool(tool) = cmd
            && super::style_flyout::has_presets(tool)
        {
            run = self.style_flyout(ui, ctx, &resp, tool);
            tip.push_str(self.hold_hint());
        }
        let resp = resp.on_hover_text(tip);
        if resp.clicked() && run {
            self.run(ctx, cmd);
        }
    }

    pub(super) fn icon(&mut self, ui: &mut Ui, ctx: &Context, cmd: Cmd) {
        let mut tip = self.cmd_tooltip(cmd);
        let presets = matches!(cmd, Cmd::Tool(t) if super::style_flyout::has_presets(t));
        if presets {
            tip.push_str(self.hold_hint());
        }
        let resp = icon_button_named(
            ui,
            cmd.icon(),
            cmd.label(self.settings.language),
            &tip,
            self.cmd_selected(cmd),
            self.cmd_enabled(cmd),
        );
        let run = match cmd {
            Cmd::Tool(tool) if presets => self.style_flyout(ui, ctx, &resp, tool),
            _ => true,
        };
        if resp.clicked() && run {
            self.run(ctx, cmd);
        }
    }

    fn hold_hint(&self) -> &'static str {
        if self.settings.language == Language::German {
            "\nGedrückt halten: Formatvorlagen"
        } else {
            "\nHold down: style presets"
        }
    }

    /// "▾" next to Rückgängig/Wiederholen: the list of steps; choosing one
    /// undoes (or redoes) it together with all steps above it.
    fn history_menu(&mut self, ui: &mut Ui, ctx: &Context, redo: bool) {
        let de = self.settings.language == Language::German;
        let lang = self.settings.language;
        let steps: Vec<String> = self
            .active_tab()
            .map(|t| if redo { t.doc.redo_list() } else { t.doc.undo_list() })
            .unwrap_or_default()
            .into_iter()
            .map(|s| crate::ui::undo_names::undo_name(s, lang))
            .collect();
        let tip = match (redo, de) {
            (false, true) => "Liste der letzten Aktionen",
            (false, false) => "List of recent actions",
            (true, true) => "Liste der rückgängig gemachten Aktionen",
            (true, false) => "List of undone actions",
        };
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(12.0, 24.0), egui::Sense::click());
        let enabled = !steps.is_empty();
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, tip));
        if ui.is_rect_visible(rect) {
            if enabled && resp.hovered() {
                ui.painter().rect_filled(rect, 3.0, theme::ACCENT.gamma_multiply(0.15));
            }
            let c = if enabled { ui.visuals().text_color() } else { ui.visuals().weak_text_color() };
            let m = rect.center();
            ui.painter().line(
                vec![m + egui::vec2(-3.0, -1.5), m + egui::vec2(0.0, 1.5), m + egui::vec2(3.0, -1.5)],
                egui::Stroke::new(1.3, c),
            );
        }
        let resp = resp.on_hover_text(tip);
        if !enabled {
            return;
        }
        let mut chosen = None;
        egui::Popup::menu(&resp).show(|ui| {
            ui.set_min_width(240.0);
            let hover_id = ui.id().with("history-hover");
            let hovered: Option<usize> = ui.data(|d| d.get_temp(hover_id));
            let mut now_hovered = None;
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                for (i, name) in steps.iter().enumerate() {
                    // Hovering a row marks it and all newer steps.
                    let marked = hovered.is_some_and(|h| i <= h);
                    let r = ui.add(egui::Button::selectable(marked, name).min_size(egui::vec2(230.0, 0.0)));
                    if r.hovered() {
                        now_hovered = Some(i);
                    }
                    if r.clicked() {
                        chosen = Some(i + 1);
                        ui.close();
                    }
                }
            });
            ui.data_mut(|d| d.insert_temp(hover_id, now_hovered));
            let n = now_hovered.map_or(0, |h| h + 1);
            let footer = match (redo, de, n) {
                (_, true, 0) => "Eintrag wählen: bis dorthin zurück".to_string(),
                (_, false, 0) => "Choose an entry to go back to it".to_string(),
                (false, true, _) => format!("{n} Aktion(en) rückgängig machen"),
                (false, false, _) => format!("Undo {n} action(s)"),
                (true, true, _) => format!("{n} Aktion(en) wiederholen"),
                (true, false, _) => format!("Redo {n} action(s)"),
            };
            ui.separator();
            ui.label(RichText::new(footer).weak().small());
        });
        if let Some(n) = chosen {
            self.travel_history(ctx, n, redo);
        }
    }

    pub(super) fn smalls(&mut self, ui: &mut Ui, ctx: &Context, cmds: &[Cmd]) {
        ui.vertical(|ui| {
            for &cmd in cmds {
                self.small(ui, ctx, cmd);
            }
        });
    }

    /// A small button opening a drop-down menu of commands.
    fn menu(&mut self, ui: &mut Ui, ctx: &Context, icon: Icon, label: &str, cmds: &[Cmd]) {
        let entries: Vec<Entry> = cmds.iter().map(|c| Entry::C(*c)).collect();
        let enabled = !self.tabs.is_empty();
        let resp = small_menu_button(ui, icon, label, enabled, false);
        self.popup_menu(ctx, &resp, &entries);
    }

    /// Large button with drop-down ("Einfügen ▾").
    pub(super) fn big_menu(
        &mut self,
        ui: &mut Ui,
        ctx: &Context,
        icon: Icon,
        label: (&str, &str),
        entries: &[Entry],
    ) {
        let label = if self.settings.language == Language::German { label.0 } else { label.1 };
        let enabled = entries.iter().any(|e| match e {
            Entry::C(c) => self.cmd_enabled(*c),
            Entry::Sub(_, _, cmds) => cmds.iter().any(|c| self.cmd_enabled(*c)),
            Entry::Sep => false,
        });
        let resp = big_menu_button(ui, icon, label, enabled || !self.tabs.is_empty(), false);
        self.popup_menu(ctx, &resp, entries);
    }

    /// Small button with drop-down ("Aufteilen ▾").
    fn small_menu(&mut self, ui: &mut Ui, ctx: &Context, icon: Icon, label: (&str, &str), entries: &[Entry]) {
        let label = if self.settings.language == Language::German { label.0 } else { label.1 };
        let resp = small_menu_button(ui, icon, label, !self.tabs.is_empty(), false);
        self.popup_menu(ctx, &resp, entries);
    }

    pub(super) fn popup_menu(&mut self, ctx: &Context, resp: &egui::Response, entries: &[Entry]) {
        let mut chosen = None;
        egui::Popup::menu(resp).show(|ui| {
            ui.set_min_width(260.0);
            for entry in entries {
                match entry {
                    Entry::Sep => {
                        ui.separator();
                    }
                    Entry::C(cmd) => {
                        if self.menu_row(ui, *cmd) {
                            chosen = Some(*cmd);
                            ui.close();
                        }
                    }
                    Entry::Sub(icon, label, cmds) => {
                        let text = if self.settings.language == Language::German { label.0 } else { label.1 };
                        let _ = icon;
                        ui.menu_button(text, |ui| {
                            ui.set_min_width(180.0);
                            for cmd in *cmds {
                                if self.menu_row(ui, *cmd) {
                                    chosen = Some(*cmd);
                                    ui.close();
                                }
                            }
                        });
                    }
                }
            }
        });
        if let Some(cmd) = chosen {
            self.run(ctx, cmd);
        }
    }

    /// One command row of a drop-down; returns true if clicked.
    pub(super) fn menu_row(&self, ui: &mut Ui, cmd: Cmd) -> bool {
        let lang = self.settings.language;
        let (de, en) = cmd.menu_names();
        let label = if lang == Language::German { de } else { en };
        let shortcut = cmd.shortcut().map(|(de, en)| if lang == Language::German { de } else { en });
        menu_item(ui, cmd.icon(), label, shortcut, self.cmd_enabled(cmd), self.cmd_selected(cmd), false)
            .on_hover_text(self.cmd_tooltip(cmd))
            .clicked()
    }

    pub(super) fn tools_group(&mut self, ui: &mut Ui, ctx: &Context) {
        let t = self.t();
        group(ui, t.group_tools, |ui| {
            ui.vertical(|ui| {
                self.small(ui, ctx, Cmd::Tool(Tool::Hand));
                self.small(ui, ctx, Cmd::Tool(Tool::SelectText));
                let other = if self.settings.language == Language::German {
                    "Andere Werkzeuge"
                } else {
                    "Other tools"
                };
                self.small_menu(
                    ui,
                    ctx,
                    Icon::Gear,
                    (other, other),
                    &[
                        Entry::C(Cmd::Tool(Tool::SelectAnnot)),
                        Entry::Sub(
                            Icon::Camera,
                            ("Bildschirmfoto", "Snapshot"),
                            &[
                                Cmd::Tool(Tool::Snapshot),
                                Cmd::View(super::view_cmds::ViewCmd::SnapshotSettings),
                            ],
                        ),
                        Entry::Sub(
                            Icon::Copy,
                            ("Zwischenablage", "Clipboard"),
                            &[Cmd::CopyText, Cmd::SelectAll],
                        ),
                        Entry::Sub(Icon::Search, ("Suche", "Search"), &[Cmd::Search]),
                        Entry::Sep,
                        Entry::C(Cmd::ActualSize),
                        Entry::C(Cmd::FitPage),
                        Entry::C(Cmd::FitWidth),
                        Entry::C(Cmd::FitContent),
                        Entry::Sep,
                        Entry::C(Cmd::Tool(Tool::Zoom)),
                    ],
                );
            });
        });
    }

    pub(super) fn ribbon_ui(&mut self, ui: &mut Ui) {
        let t = self.t();
        let ctx = ui.ctx().clone();
        // Form fields have no Format tab; they are edited in the properties panel.
        let has_selection = self
            .tab()
            .and_then(|tab| tab.selected_annotation())
            .is_some_and(|a| a.kind != crate::model::AnnotKind::Widget);
        // An active comment tool has a Format tab for its default style
        // (it wins over the comment it has just created).
        let tool_format = self.tool.annot_kind().is_some();
        // "Objekte bearbeiten" / "Text bearbeiten": "Format" and "Anordnen".
        let content_tabs = self.content_tabs();
        if (!has_selection && !tool_format && !content_tabs && self.ribbon == RibbonTab::Format)
            || (!content_tabs && self.ribbon == RibbonTab::Arrange)
        {
            self.ribbon = RibbonTab::Home;
        }
        Panel::top("ribbon")
            .frame(
                Frame::new()
                    .fill(theme::ribbon_background(ui.visuals().dark_mode, self.settings.design))
                    .inner_margin(Margin::symmetric(6, 3)),
            )
            .show(ui, |ui| {
                // Row 1: quick access toolbar and quick start search (title bar row).
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
                    crate::ui::icons::paint(ui.painter(), rect, Icon::Logo, theme::BRAND);
                    for cmd in [
                        Cmd::Open,
                        Cmd::Save,
                        Cmd::Print,
                        Cmd::Email,
                        Cmd::Undo,
                        Cmd::Redo,
                        Cmd::ViewBack,
                        Cmd::ViewForward,
                    ] {
                        self.icon(ui, &ctx, cmd);
                        // A small arrow lists all steps to undo or redo.
                        if matches!(cmd, Cmd::Undo | Cmd::Redo) {
                            self.history_menu(ui, &ctx, cmd == Cmd::Redo);
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        self.quick_start(ui, &ctx);
                    });
                });
                // Row 2: file menu and tabs.
                ui.horizontal(|ui| {
                    self.file_menu(ui);
                    let mut tabs = vec![
                        (RibbonTab::Home, t.tab_home),
                        (RibbonTab::View, t.tab_view),
                        (RibbonTab::Comment, t.tab_comment),
                        (RibbonTab::Protect, t.tab_protect),
                        (RibbonTab::Form, t.tab_form),
                        (RibbonTab::Organize, t.tab_organize),
                        (RibbonTab::Convert, t.tab_convert),
                        (RibbonTab::Share, t.tab_share),
                        (RibbonTab::Review, t.tab_review),
                        (RibbonTab::Accessibility, t.tab_accessibility),
                        (RibbonTab::Bookmarks, t.tab_bookmarks),
                        (RibbonTab::Help, t.tab_help),
                    ];
                    if has_selection || tool_format || content_tabs {
                        tabs.push((RibbonTab::Format, t.format_tab));
                    }
                    if content_tabs {
                        tabs.push((
                            RibbonTab::Arrange,
                            if self.settings.language == Language::German { "Anordnen" } else { "Arrange" },
                        ));
                    }
                    self.tab_row(ui, &tabs);
                });
                if self.backstage_open() {
                    return;
                }
                ui.add_space(2.0);
                // Fit the ribbon into the window: level 0 full size, 1 dense
                // (narrower large buttons, less space), 2 small buttons
                // without labels, 3.. collapse groups from the right. The
                // first group (the most used commands) never collapses.
                // Each level's width is measured when drawn; the first level
                // known to fit (or not measured yet) is used.
                let avail = ui.available_width();
                let lang = self.settings.language;
                let content_key = if self.ribbon == RibbonTab::Format && content_tabs {
                    250u8
                } else if self.ribbon == RibbonTab::Format && tool_format {
                    100u8.wrapping_add(self.tool as u8)
                } else if self.ribbon == RibbonTab::Format {
                    self.tab().and_then(|tab| tab.selected_annotation()).map_or(0, |a| a.kind as u8)
                } else {
                    0
                };
                let tab_now = self.ribbon;
                let groups = self.ribbon_groups.get(&tab_now).copied().unwrap_or(0);
                let mut level = 0;
                let last_level = 1 + groups;
                while level < last_level
                    && self
                        .ribbon_widths
                        .get(&(tab_now, lang, level, content_key))
                        .is_some_and(|w| *w > avail + 0.5)
                {
                    level += 1;
                }
                set_ribbon_fit(
                    &ctx,
                    RibbonFit {
                        dense: level >= 1,
                        compact: level >= 2,
                        collapse_from: if level >= 3 {
                            groups.saturating_sub(level - 2).max(1)
                        } else {
                            usize::MAX
                        },
                        ..RibbonFit::FULL
                    },
                );
                let mut drawn_width = 0.0;
                egui::ScrollArea::horizontal()
                    .id_salt("ribbon-scroll")
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
                    .show(ui, |ui| {
                        ui.style_mut().spacing.scroll.floating = false;
                        // The ribbon keeps its compact grid in every look,
                        // so all groups fit; the room goes to dialogs and
                        // panels.
                        ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);
                        ui.spacing_mut().button_padding = egui::vec2(6.0, 3.0);
                        ui.spacing_mut().interact_size.y = 18.0;
                        drawn_width = ui
                            .horizontal(|ui| {
                                ui.set_min_height(82.0);
                                match self.ribbon {
                                    RibbonTab::Home => self.ribbon_home(ui, &ctx),
                                    RibbonTab::View => self.ribbon_view(ui, &ctx),
                                    RibbonTab::Comment => self.ribbon_comment(ui, &ctx),
                                    RibbonTab::Protect => self.ribbon_protect(ui, &ctx),
                                    RibbonTab::Form => self.ribbon_form(ui, &ctx),
                                    RibbonTab::Organize => self.ribbon_organize(ui, &ctx),
                                    RibbonTab::Convert => self.ribbon_convert(ui, &ctx),
                                    RibbonTab::Share => self.ribbon_share(ui, &ctx),
                                    RibbonTab::Review => self.ribbon_review(ui, &ctx),
                                    RibbonTab::Accessibility => self.ribbon_accessibility(ui, &ctx),
                                    RibbonTab::Bookmarks => self.ribbon_bookmarks(ui, &ctx),
                                    RibbonTab::Help => self.ribbon_help(ui, &ctx),
                                    RibbonTab::Format if content_tabs => self.ribbon_content_format(ui, &ctx),
                                    RibbonTab::Arrange => self.ribbon_arrange(ui, &ctx),
                                    RibbonTab::Format if tool_format => self.ribbon_tool_format(ui, &ctx),
                                    RibbonTab::Format => self.ribbon_format(ui, &ctx),
                                }
                            })
                            .response
                            .rect
                            .width();
                    });
                let counted = ribbon_fit(&ctx).next_group;
                set_ribbon_fit(&ctx, RibbonFit::FULL);
                // A command may have switched the tab while drawing.
                if self.ribbon == tab_now {
                    self.ribbon_widths.insert((tab_now, lang, level, content_key), drawn_width);
                    self.ribbon_groups.insert(tab_now, counted);
                    if drawn_width > avail + 0.5 && level < 1 + counted.max(groups) {
                        ctx.request_repaint();
                    }
                }
            });
    }

    /// The ribbon tabs; tabs that do not fit go into a "»" menu at the end.
    fn tab_row(&mut self, ui: &mut Ui, tabs: &[(RibbonTab, &str)]) {
        let modern = theme::modern();
        let size = if modern { 14.5 } else { 13.0 };
        let text_of = |tab: RibbonTab, label: &str| {
            let text = RichText::new(label).size(size);
            if matches!(tab, RibbonTab::Format | RibbonTab::Arrange) {
                text.color(egui::Color32::from_rgb(160, 110, 0)).strong()
            } else {
                text
            }
        };
        let pad = ui.spacing().item_spacing.x + 2.0 * ui.spacing().button_padding.x;
        let widths: Vec<f32> = tabs
            .iter()
            .map(|(_, label)| {
                let font = egui::FontId::proportional(size);
                ui.painter().layout_no_wrap(label.to_string(), font, egui::Color32::WHITE).size().x + pad
            })
            .collect();
        let avail = ui.available_width();
        let backstage = self.backstage_open();
        // Tabs that must stay in view when the row is too narrow: the
        // context tabs ("Format", "Anordnen") and the active one; the other
        // tabs go behind "»" from the right.
        let keep = |tab: RibbonTab| {
            matches!(tab, RibbonTab::Format | RibbonTab::Arrange) || (tab == self.ribbon && !backstage)
        };
        let mut shown_idx: Vec<usize> = (0..tabs.len()).collect();
        let mut hidden_idx: Vec<usize> = Vec::new();
        if widths.iter().sum::<f32>() > avail {
            // Room for the "»" button.
            let mut used =
                34.0 + (0..tabs.len()).filter(|&i| keep(tabs[i].0)).map(|i| widths[i]).sum::<f32>();
            shown_idx.retain(|&i| {
                if keep(tabs[i].0) {
                    return true;
                }
                used += widths[i];
                let fits = used <= avail && hidden_idx.is_empty();
                if !fits {
                    hidden_idx.push(i);
                }
                fits
            });
        }
        let mut chosen = None;
        for &(tab, label) in shown_idx.iter().map(|&i| &tabs[i]) {
            let active = self.ribbon == tab && !backstage;
            // Modern: the active tab only by its bar and bold text.
            let resp = if modern {
                let text = text_of(tab, label);
                ui.selectable_label(false, if active { text.strong() } else { text })
            } else {
                ui.selectable_label(active, text_of(tab, label))
            };
            // Each tab has its topic colour: a bar under the active tab and
            // a faint one on hover.
            let bar = |alpha: f32| {
                let r = resp.rect;
                let line = egui::Rect::from_min_max(
                    egui::pos2(r.left() + 4.0, r.bottom() - 2.5),
                    egui::pos2(r.right() - 4.0, r.bottom()),
                );
                ui.painter().rect_filled(line, 1.0, tab_color(tab).gamma_multiply(alpha));
            };
            if active {
                bar(1.0);
            } else if resp.hovered() {
                bar(0.45);
            }
            if resp.clicked() {
                chosen = Some(tab);
            }
        }
        if !hidden_idx.is_empty() {
            let hidden_selected = hidden_idx.iter().any(|&i| tabs[i].0 == self.ribbon) && !backstage;
            let de = self.settings.language == Language::German;
            let more = ui
                .selectable_label(hidden_selected, RichText::new("»").size(14.0))
                .on_hover_text(if de { "Weitere Registerkarten" } else { "More tabs" });
            egui::Popup::menu(&more).show(|ui| {
                for &(tab, label) in hidden_idx.iter().map(|&i| &tabs[i]) {
                    if ui.selectable_label(self.ribbon == tab && !backstage, text_of(tab, label)).clicked() {
                        chosen = Some(tab);
                        ui.close();
                    }
                }
            });
        }
        if let Some(tab) = chosen {
            self.ribbon = tab;
            self.close_backstage();
        }
    }

    /// The command search ("Schnellstart") at the right end of the tab row.
    fn quick_start(&mut self, ui: &mut Ui, ctx: &Context) {
        let t = self.t();
        let id = egui::Id::new("quick-start");
        let edit = ui.add(
            egui::TextEdit::singleline(&mut self.quick_query)
                .id(id)
                .hint_text(t.quick_start)
                .desired_width(ui.available_width().clamp(60.0, 190.0)),
        );
        if self.quick_focus {
            edit.request_focus();
            self.quick_focus = false;
        }
        let query = self.quick_query.trim().to_lowercase();
        if query.is_empty() {
            return;
        }
        let lang = self.settings.language;
        let matches: Vec<Cmd> = SEARCHABLE
            .iter()
            .copied()
            .filter(|c| self.cmd_implemented(*c))
            .filter(|c| {
                let (de, en) = c.names();
                de.to_lowercase().contains(&query) || en.to_lowercase().contains(&query)
            })
            .take(12)
            .collect();
        let mut run = None;
        if edit.has_focus() || edit.lost_focus() {
            egui::Area::new(id.with("popup"))
                .order(egui::Order::Foreground)
                .fixed_pos(edit.rect.left_bottom() + egui::vec2(0.0, 2.0))
                .show(ui.ctx(), |ui| {
                    Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_min_width(260.0);
                        if matches.is_empty() {
                            ui.label(RichText::new(t.no_command).weak());
                        }
                        for cmd in &matches {
                            ui.add_enabled_ui(self.cmd_enabled(*cmd), |ui| {
                                if ui.button(cmd.label(lang)).clicked() {
                                    run = Some(*cmd);
                                }
                            });
                        }
                    });
                });
        }
        if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            run = matches.first().copied();
            // The Enter key must not also confirm a dialog opened by the command.
            ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        }
        if let Some(cmd) = run {
            self.quick_query.clear();
            self.run(ctx, cmd);
        }
    }

    /// The "Datei" button: opens and closes the backstage view.
    fn file_menu(&mut self, ui: &mut Ui) {
        let t = self.t();
        let open = self.backstage_open();
        let fill = if open { theme::BRAND.gamma_multiply(0.8) } else { theme::BRAND };
        let button = egui::Button::new(RichText::new(t.menu_file).color(egui::Color32::WHITE).strong())
            .fill(fill)
            .selected(open);
        if ui.add(button).clicked() {
            self.toggle_backstage();
        }
    }

    pub(super) fn stamp_picker(&mut self, ui: &mut Ui) {
        let t = self.t();
        let current = self.settings.stamp;
        egui::ComboBox::from_id_salt("stamp-preset")
            .selected_text(t.stamp_label(current))
            .width(120.0)
            .show_ui(ui, |ui| {
                for preset in StampPreset::ALL {
                    if ui.selectable_label(current == preset, t.stamp_label(preset)).clicked() {
                        self.settings.stamp = preset;
                        let mut style = self.settings.style(Tool::Stamp);
                        style.color = preset.color();
                        self.settings.set_style(Tool::Stamp, style);
                        self.select_tool(Tool::Stamp);
                    }
                }
            });
    }

    fn zoom_box(&mut self, ui: &mut Ui) {
        let de = self.settings.language == Language::German;
        let Some(tab) = self.tab() else {
            ui.add_enabled(false, egui::DragValue::new(&mut 100.0).suffix(" %"));
            return;
        };
        let mut pct = tab.zoom * 100.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            if ui
                .add(
                    egui::DragValue::new(&mut pct)
                        .range(5.0..=6400.0)
                        .suffix(" %")
                        .speed(1.0)
                        .max_decimals(0),
                )
                .changed()
            {
                tab.zoom = pct / 100.0;
                tab.zoom_mode = crate::ui::view::ZoomMode::Custom;
            }
            // Preset values.
            let resp = ui.small_button("▾");
            egui::Popup::menu(&resp).show(|ui| {
                use crate::ui::view::ZoomMode;
                for p in
                    [10.0, 25.0, 50.0, 75.0, 100.0, 125.0, 150.0, 200.0, 400.0, 800.0, 1600.0, 3200.0, 6400.0]
                {
                    if ui.button(format!("{p:.0} %")).clicked() {
                        tab.zoom = p / 100.0;
                        tab.zoom_mode = ZoomMode::Custom;
                    }
                }
                ui.separator();
                if ui.button(if de { "Seitengröße" } else { "Fit page" }).clicked() {
                    tab.zoom_mode = ZoomMode::FitPage;
                }
                if ui.button(if de { "Seitenbreite" } else { "Fit width" }).clicked() {
                    tab.zoom_mode = ZoomMode::FitWidth;
                }
            });
        });
    }

    fn ribbon_home(&mut self, ui: &mut Ui, ctx: &Context) {
        let t = self.t();
        let de = self.settings.language == Language::German;
        group(ui, t.group_tools, |ui| {
            self.big(ui, ctx, Cmd::Tool(Tool::Hand));
            self.big(ui, ctx, Cmd::Tool(Tool::SelectText));
            self.big(ui, ctx, Cmd::Tool(Tool::SelectAnnot));
            ui.vertical(|ui| {
                self.small(ui, ctx, Cmd::Tool(Tool::Snapshot));
                self.menu(
                    ui,
                    ctx,
                    Icon::Copy,
                    if de { "Zwischenablage" } else { "Clipboard" },
                    &[Cmd::CopyText, Cmd::SelectAll],
                );
                self.small(ui, ctx, Cmd::Search);
            });
        });
        group(ui, t.tab_view, |ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    self.icon(ui, ctx, Cmd::ZoomOut);
                    self.zoom_box(ui);
                    self.icon(ui, ctx, Cmd::ZoomIn);
                });
                ui.horizontal(|ui| {
                    for cmd in [
                        Cmd::ActualSize,
                        Cmd::FitPage,
                        Cmd::FitWidth,
                        Cmd::RotateViewLeft,
                        Cmd::RotateViewRight,
                    ] {
                        self.icon(ui, ctx, cmd);
                    }
                });
                ui.horizontal(|ui| {
                    for p in [
                        SidePanel::Bookmarks,
                        SidePanel::Thumbnails,
                        SidePanel::Comments,
                        SidePanel::Attachments,
                    ] {
                        self.icon(ui, ctx, Cmd::Panel(p));
                    }
                });
            });
        });
        self.ribbon_objects_group(ui, ctx);
        group(ui, if de { "Anmerkungen" } else { "Comments" }, |ui| {
            self.smalls(
                ui,
                ctx,
                &[Cmd::Tool(Tool::Typewriter), Cmd::Tool(Tool::Highlight), Cmd::Tool(Tool::Underline)],
            );
            self.smalls(
                ui,
                ctx,
                &[Cmd::Tool(Tool::Note), Cmd::Tool(Tool::Arrow), Cmd::Tool(Tool::Rectangle)],
            );
            ui.vertical(|ui| {
                self.big(ui, ctx, Cmd::Tool(Tool::Stamp));
            });
            ui.vertical(|ui| {
                ui.add_space(4.0);
                self.stamp_picker(ui);
            });
        });
        group(ui, if de { "Verweise" } else { "Links" }, |ui| {
            self.big(ui, ctx, Cmd::Tool(Tool::Link));
            self.smalls(
                ui,
                ctx,
                &[
                    Cmd::WebLinksCreate,
                    Cmd::WebLinksRemove,
                    UA(
                        Unavailable::Barcodes,
                        "Web-Links zu Barcodes erstellen",
                        "Web links to barcodes",
                        Icon::Link,
                    ),
                ],
            );
        });
        group(ui, t.tab_protect, |ui| {
            self.big(
                ui,
                ctx,
                UA(Unavailable::Signatures, "Dokument signieren", "Sign document", Icon::Signature),
            );
        });
    }

    fn ribbon_view(&mut self, ui: &mut Ui, ctx: &Context) {
        use super::view_cmds::ViewCmd as V;
        use crate::ui::theme::Design;
        let t = self.t();
        let de = self.settings.language == Language::German;
        let v = Cmd::View;
        self.tools_group(ui, ctx);
        group(ui, "Navigation", |ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    self.icon(ui, ctx, Cmd::FirstPage);
                    self.icon(ui, ctx, Cmd::PrevPage);
                    self.page_box(ui);
                    self.icon(ui, ctx, Cmd::NextPage);
                    self.icon(ui, ctx, Cmd::LastPage);
                });
                self.small(ui, ctx, Cmd::ViewBack);
                self.small(ui, ctx, Cmd::ViewForward);
            });
        });
        group(ui, if de { "Größe" } else { "Size" }, |ui| {
            self.big(ui, ctx, Cmd::ActualSize);
            self.smalls(ui, ctx, &[Cmd::FitPage, Cmd::FitWidth, Cmd::FitContent]);
            ui.vertical(|ui| {
                self.zoom_box(ui);
                self.small(ui, ctx, Cmd::ZoomIn);
                self.small(ui, ctx, Cmd::ZoomOut);
            });
            self.big(ui, ctx, Cmd::Tool(Tool::Zoom));
            self.smalls(ui, ctx, &[Cmd::Tool(Tool::DynamicZoom), Cmd::Tool(Tool::Loupe), v(V::PanZoom)]);
        });
        group(ui, if de { "Seitenanzeige" } else { "Page display" }, |ui| {
            self.big_menu(
                ui,
                ctx,
                Icon::RotateRight,
                ("Ansicht drehen", "Rotate view"),
                &[
                    Entry::C(Cmd::RotateViewLeft),
                    Entry::C(Cmd::RotateViewRight),
                    Entry::Sep,
                    Entry::C(v(V::RotateReset)),
                ],
            );
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    for cmd in [
                        Cmd::LayoutSingle,
                        Cmd::LayoutContinuous,
                        Cmd::LayoutFacing,
                        v(V::LayoutTwoPagesContinuous),
                    ] {
                        self.icon(ui, ctx, cmd);
                    }
                });
                ui.horizontal(|ui| {
                    for cmd in [v(V::PageGaps), Cmd::LayoutCover, v(V::RightToLeft)] {
                        self.icon(ui, ctx, cmd);
                    }
                    let resp = crate::ui::widgets::small_menu_button(ui, Icon::TwoPages, "", true, false)
                        .on_hover_text(if de { "Seitenanordnung" } else { "Page arrangement" });
                    self.popup_menu(
                        ctx,
                        &resp,
                        &[Entry::C(v(V::StandardLayout)), Entry::C(v(V::SideBySide))],
                    );
                });
            });
            self.smalls(ui, ctx, &[v(V::Rulers), v(V::Grid), v(V::Guides)]);
            self.smalls(
                ui,
                ctx,
                &[
                    UA(Unavailable::ThinLines, "Dünne Linien", "Thin lines", Icon::Line),
                    v(V::Transparency),
                    v(V::PageInfo),
                ],
            );
            ui.vertical(|ui| {
                let resp = crate::ui::widgets::small_menu_button(ui, Icon::Margins, "", true, false)
                    .on_hover_text(if de { "Einrasten" } else { "Snap" });
                self.popup_menu(
                    ctx,
                    &resp,
                    &[
                        Entry::C(v(V::Snap)),
                        Entry::Sep,
                        Entry::C(v(V::SnapGrid)),
                        Entry::C(v(V::SnapGuides)),
                        Entry::C(UA(Unavailable::SnapObjects, "An Objekten", "To objects", Icon::Margins)),
                        Entry::Sep,
                        Entry::C(v(V::SnapSettings)),
                    ],
                );
                self.icon(ui, ctx, v(V::Transitions));
                self.icon(ui, ctx, v(V::AutoScroll));
            });
            ui.vertical(|ui| {
                self.icon(ui, ctx, v(V::ReplaceColors));
                self.icon(ui, ctx, Cmd::DarkMode);
                self.icon(ui, ctx, v(V::HighlightLinks));
            });
        });
        group(ui, if de { "Teilen" } else { "Split" }, |ui| {
            self.big_menu(
                ui,
                ctx,
                Icon::Split,
                ("Teilen", "Split"),
                &[Entry::C(v(V::SplitHorizontal)), Entry::C(v(V::SplitVertical)), Entry::C(v(V::SplitGrid))],
            );
        });
        group(ui, if de { "Fenster" } else { "Window" }, |ui| {
            ui.vertical(|ui| {
                self.small_menu(
                    ui,
                    ctx,
                    Icon::Thumbnails,
                    ("Dokument-Tabs", "Document tabs"),
                    &[
                        Entry::C(v(V::NewTab)),
                        Entry::Sub(
                            Icon::Export,
                            ("Tab verschieben nach", "Move tab to"),
                            &[
                                Cmd::View(V::MoveTabNewHorizontal),
                                Cmd::View(V::MoveTabNewVertical),
                                Cmd::View(V::MoveTabNewWindow),
                                Cmd::View(V::MoveTabMain),
                            ],
                        ),
                        Entry::C(v(V::MoveTabs)),
                        Entry::Sub(
                            Icon::Thumbnails,
                            ("Tabs anordnen", "Arrange tabs"),
                            &[Cmd::View(V::ArrangeHorizontal), Cmd::View(V::ArrangeVertical)],
                        ),
                        Entry::Sep,
                        Entry::C(v(V::MergeGroups)),
                        Entry::C(v(V::MergeWindows)),
                        Entry::Sep,
                        Entry::C(v(V::SortTabs)),
                        Entry::Sep,
                        Entry::C(Cmd::CloseAll),
                        Entry::C(v(V::CloseOthers)),
                        Entry::C(Cmd::Close),
                        Entry::Sep,
                        Entry::C(v(V::ReopenClosed)),
                    ],
                );
                let pane = |de: &'static str, en: &'static str, icon: Icon| {
                    Entry::C(UA(Unavailable::Pane, de, en, icon))
                };
                self.small_menu(
                    ui,
                    ctx,
                    Icon::Sidebar,
                    ("Leisten", "Panes"),
                    &[
                        Entry::C(Cmd::Panel(SidePanel::Bookmarks)),
                        Entry::C(Cmd::Panel(SidePanel::Thumbnails)),
                        Entry::C(Cmd::Panel(SidePanel::Comments)),
                        Entry::C(Cmd::Panel(SidePanel::Attachments)),
                        Entry::C(Cmd::Panel(SidePanel::Search)),
                        pane("Digitale Signaturen", "Digital signatures", Icon::Signature),
                        pane("Ebenen", "Layers", Icon::Thumbnails),
                        pane("Inhalt", "Content", Icon::TextBox),
                        pane("Sprungmarken", "Destinations", Icon::Bookmarks),
                        pane("3D-Modellbaum", "3D model tree", Icon::Thumbnails),
                        Entry::Sep,
                        pane("Struktur", "Structure", Icon::Accessibility),
                        pane("Strukturreihenfolge", "Structure order", Icon::Accessibility),
                        Entry::Sep,
                        Entry::C(v(V::LoupeWindow)),
                        Entry::C(v(V::PanZoom)),
                        pane("Stempelpalette", "Stamp palette", Icon::Stamp),
                        Entry::Sep,
                        pane("Rechtschreibprüfung", "Spell checking", Icon::SpellCheck),
                        pane("Verlauf", "History", Icon::Undo),
                        Entry::Sep,
                        Entry::C(Cmd::ToggleProperties),
                        Entry::C(Cmd::ToggleSidebar),
                        Entry::Sep,
                        Entry::C(v(V::ResetPanes)),
                        Entry::C(v(V::ResetDocPanes)),
                    ],
                );
                self.small_menu(
                    ui,
                    ctx,
                    Icon::Attachment,
                    ("Portfolio", "Portfolio"),
                    &[
                        Entry::C(v(V::PortfolioList)),
                        Entry::C(v(V::PortfolioCover)),
                        Entry::Sep,
                        Entry::C(v(V::PortfolioAddFiles)),
                        Entry::C(v(V::PortfolioAddFolder)),
                        Entry::Sep,
                        Entry::C(v(V::PortfolioMark)),
                        Entry::C(v(V::NewPortfolio)),
                    ],
                );
            });
            self.big_menu(
                ui,
                ctx,
                Icon::Theme,
                ("Design", "Design"),
                &[
                    Entry::C(v(V::Design(Design::Standard))),
                    Entry::C(v(V::Design(Design::White))),
                    Entry::C(v(V::Design(Design::DarkGray))),
                    Entry::C(v(V::Design(Design::Black))),
                    Entry::Sep,
                    Entry::C(v(V::CustomizeUi)),
                    Entry::C(v(V::ReplaceColors)),
                    Entry::C(Cmd::DarkMode),
                ],
            );
            ui.vertical(|ui| {
                self.small(ui, ctx, Cmd::Fullscreen);
                ui.label(t.language);
                self.language_picker(ui);
            });
        });
        group(ui, if de { "Vorlesen" } else { "Read aloud" }, |ui| {
            self.big(ui, ctx, v(V::ReadStart));
            self.big(ui, ctx, v(V::ReadStop));
            self.smalls(ui, ctx, &[v(V::ReadBack), v(V::ReadForward), v(V::ReadShow)]);
            self.smalls(ui, ctx, &[v(V::ReadPage), v(V::ReadToEnd), v(V::ReadSelection)]);
            ui.vertical(|ui| {
                let mut volume = self.settings.read_volume;
                ui.add(egui::Slider::new(&mut volume, 0..=100).show_value(false)).on_hover_text(if de {
                    "Lautstärke"
                } else {
                    "Volume"
                });
                self.settings.read_volume = volume;
                let mut rate = self.settings.read_rate;
                ui.add(egui::Slider::new(&mut rate, -10..=10).show_value(false)).on_hover_text(if de {
                    "Tempo"
                } else {
                    "Speed"
                });
                self.settings.read_rate = rate;
                ui.horizontal(|ui| {
                    self.icon(ui, ctx, v(V::ReadFields));
                    self.icon(ui, ctx, v(V::ReadAnnots));
                    self.icon(
                        ui,
                        ctx,
                        UA(
                            Unavailable::Highlighting,
                            "Wörter hervorheben",
                            "Highlight words",
                            Icon::Highlight,
                        ),
                    );
                    self.icon(ui, ctx, v(V::ReadFollow));
                });
            });
        });
    }

    /// Editable page number "n / m".
    fn page_box(&mut self, ui: &mut Ui) {
        let Some(tab) = self.tab() else {
            ui.add_enabled(false, egui::DragValue::new(&mut 1).suffix(" / 0"));
            return;
        };
        let count = tab.doc.page_count().max(1);
        let mut n = tab.current_page + 1;
        let resp =
            ui.add(egui::DragValue::new(&mut n).range(1..=count).speed(0.05).suffix(format!(" / {count}")));
        if resp.changed() {
            tab.jump_to_page(n - 1);
        }
    }

    fn language_picker(&mut self, ui: &mut Ui) {
        let current = self.settings.language;
        egui::ComboBox::from_id_salt("language").selected_text(current.name()).show_ui(ui, |ui| {
            for lang in Language::ALL {
                if ui.selectable_label(current == lang, lang.name()).clicked() {
                    self.settings.language = lang;
                }
            }
        });
    }

    fn ribbon_protect(&mut self, ui: &mut Ui, ctx: &Context) {
        let de = self.settings.language == Language::German;
        self.tools_group(ui, ctx);
        group(ui, if de { "Digitale Signatur" } else { "Digital signature" }, |ui| {
            self.smalls(
                ui,
                ctx,
                &[
                    UA(Unavailable::Signatures, "Dokument signieren", "Sign document", Icon::Signature),
                    UA(Unavailable::Signatures, "Signatur einfügen", "Insert signature", Icon::Signature),
                    UA(Unavailable::Signatures, "Signaturen einfügen", "Insert signatures", Icon::Signature),
                ],
            );
            self.smalls(
                ui,
                ctx,
                &[
                    UA(Unavailable::Signatures, "Zertifizieren", "Certify", Icon::Lock),
                    UA(
                        Unavailable::Signatures,
                        "Zertifizieren (unsichtbar)",
                        "Certify (invisible)",
                        Icon::Lock,
                    ),
                    UA(Unavailable::Signatures, "Zeitstempel", "Time stamp", Icon::Info),
                ],
            );
        });
        group(ui, if de { "Unterschrift und Initialen" } else { "Signature and initials" }, |ui| {
            self.big(ui, ctx, Cmd::Tool(Tool::Signature));
            self.big(ui, ctx, Cmd::Tool(Tool::FieldSignature));
        });
        group(ui, if de { "Schwärzen" } else { "Redact" }, |ui| {
            self.smalls(ui, ctx, &[Cmd::Tool(Tool::Redact), Cmd::ApplyRedactions, Cmd::SearchRedact]);
        });
        group(ui, if de { "Dokumentenschutz" } else { "Document protection" }, |ui| {
            self.big(ui, ctx, Cmd::PasswordProtect);
            self.big(ui, ctx, Cmd::RemoveProtection);
            self.big(ui, ctx, Cmd::DocProperties);
        });
    }

    fn ribbon_organize(&mut self, ui: &mut Ui, ctx: &Context) {
        let de = self.settings.language == Language::German;
        self.tools_group(ui, ctx);
        group(ui, if de { "Seiten" } else { "Pages" }, |ui| {
            self.big_menu(
                ui,
                ctx,
                Icon::InsertPage,
                ("Einfügen", "Insert"),
                &[
                    Entry::C(Cmd::InsertFromFile),
                    Entry::C(Cmd::InsertBlank),
                    Entry::C(Cmd::InsertClipboard),
                    Entry::C(Cmd::InsertImages),
                    Entry::C(UA(
                        Unavailable::Scanner,
                        "Gescannte Seiten einfügen…",
                        "Insert scanned pages…",
                        Icon::Camera,
                    )),
                    Entry::C(Cmd::InsertText),
                    Entry::C(Cmd::InsertRtf),
                ],
            );
            self.big_menu(
                ui,
                ctx,
                Icon::DeletePage,
                ("Löschen", "Delete"),
                &[Entry::C(Cmd::DeletePages), Entry::C(Cmd::DeleteBlankPages)],
            );
            self.big(ui, ctx, Cmd::ExtractPages);
            self.big(ui, ctx, Cmd::MovePages);
            ui.vertical(|ui| {
                self.small(ui, ctx, Cmd::NUp);
                self.small_menu(
                    ui,
                    ctx,
                    Icon::Split,
                    ("Aufteilen", "Split"),
                    &[
                        Entry::C(Cmd::SplitDocument),
                        Entry::C(Cmd::SplitPages),
                        Entry::C(UA(
                            Unavailable::Guides,
                            "Seiten an Hilfslinien aufteilen",
                            "Split pages at guides",
                            Icon::Split,
                        )),
                    ],
                );
                self.small(ui, ctx, Cmd::DuplicatePages);
            });
            self.smalls(ui, ctx, &[Cmd::ReplacePages, Cmd::SwapPages, Cmd::Overlay]);
            self.smalls(ui, ctx, &[Cmd::NormalizePages, Cmd::FindDuplicatePages, Cmd::ReversePages]);
        });
        group(ui, if de { "Form der Seite ändern" } else { "Page shape" }, |ui| {
            self.big_menu(
                ui,
                ctx,
                Icon::RotateRight,
                ("Drehen", "Rotate"),
                &[
                    Entry::C(Cmd::RotatePages),
                    Entry::Sep,
                    Entry::C(Cmd::RotateLeft),
                    Entry::C(Cmd::RotateRight),
                    Entry::C(Cmd::Rotate180),
                ],
            );
            self.big_menu(
                ui,
                ctx,
                Icon::Crop,
                ("Zuschneiden", "Crop"),
                &[
                    Entry::C(Cmd::CropPages),
                    Entry::C(Cmd::Tool(Tool::Crop)),
                    Entry::C(Cmd::RemoveCropped),
                    Entry::Sub(
                        Icon::Margins,
                        ("Weiße Ränder abschneiden", "Trim white margins"),
                        &[
                            Cmd::TrimWhite(Side::All),
                            Cmd::TrimWhite(Side::Left),
                            Cmd::TrimWhite(Side::Right),
                            Cmd::TrimWhite(Side::Top),
                            Cmd::TrimWhite(Side::Bottom),
                        ],
                    ),
                ],
            );
            self.big(ui, ctx, Cmd::ResizePages);
            self.big(ui, ctx, Cmd::Margins);
        });
        group(ui, if de { "Seitenmarkierungen" } else { "Page marks" }, |ui| {
            for (cmd, kind, icon, label) in [
                (Cmd::Watermark, MarkKind::Watermark, Icon::Watermark, ("Wasserzeichen", "Watermark")),
                (Cmd::Background, MarkKind::Background, Icon::Background, ("Hintergrund", "Background")),
                (
                    Cmd::HeaderFooter,
                    MarkKind::HeaderFooter,
                    Icon::HeaderFooter,
                    ("Kopf- und Fußzeilen", "Header and footer"),
                ),
            ] {
                self.big_menu(
                    ui,
                    ctx,
                    icon,
                    label,
                    &[
                        Entry::C(cmd),
                        Entry::C(Cmd::RemoveMarks(kind)),
                        Entry::Sep,
                        Entry::C(Cmd::ManageMarks(kind)),
                    ],
                );
            }
            self.big_menu(
                ui,
                ctx,
                Icon::Numbering,
                ("Paginierung", "Bates numbering"),
                &[
                    Entry::C(Cmd::Bates),
                    Entry::C(Cmd::BatesFiles),
                    Entry::C(Cmd::RemoveMarks(MarkKind::Bates)),
                    Entry::Sep,
                    Entry::C(Cmd::ManageMarks(MarkKind::Bates)),
                ],
            );
            self.big(ui, ctx, Cmd::PageLabels);
        });
    }

    fn ribbon_convert(&mut self, ui: &mut Ui, ctx: &Context) {
        let de = self.settings.language == Language::German;
        self.tools_group(ui, ctx);
        group(ui, if de { "Erstellen" } else { "Create" }, |ui| {
            self.big(ui, ctx, UA(Unavailable::Scanner, "Scannen", "Scan", Icon::Camera));
            self.big_menu(
                ui,
                ctx,
                Icon::InsertFile,
                ("Aus Dateien", "From files"),
                &[Entry::C(Cmd::FromFiles), Entry::C(Cmd::MergeFiles)],
            );
            self.big(ui, ctx, Cmd::New);
        });
        group(ui, if de { "Ändern" } else { "Change" }, |ui| {
            self.big(ui, ctx, Cmd::ContentsToImages);
            self.big(ui, ctx, Cmd::SaveOptimized);
            self.big(ui, ctx, Cmd::ReplaceFonts);
        });
        group(ui, if de { "Exportieren" } else { "Export" }, |ui| {
            for cmd in
                [Cmd::ExportImages, Cmd::ExportWord, Cmd::ExportExcel, Cmd::ExportPowerPoint, Cmd::ExportText]
            {
                self.big(ui, ctx, cmd);
            }
        });
        group(ui, if de { "Texterkennung" } else { "Text recognition" }, |ui| {
            self.big(ui, ctx, Cmd::RecognizeText);
        });
        group(ui, if de { "Seiten erweitern" } else { "Enhance pages" }, |ui| {
            for (de_label, en) in [
                ("Gescannte Seiten optimieren", "Optimize scanned pages"),
                ("Seiten begradigen", "Deskew pages"),
                ("Gescannte Seiten korrigieren", "Correct scanned pages"),
            ] {
                self.big(ui, ctx, UA(Unavailable::ScanCleanup, de_label, en, Icon::Image));
            }
        });
        group(ui, if de { "Farben umwandeln" } else { "Convert colours" }, |ui| {
            self.big(ui, ctx, Cmd::Grayscale);
            self.big(ui, ctx, Cmd::RecolorPages);
        });
    }

    fn ribbon_share(&mut self, ui: &mut Ui, ctx: &Context) {
        let de = self.settings.language == Language::German;
        group(ui, if de { "Teilen" } else { "Share" }, |ui| {
            self.big(ui, ctx, Cmd::Email);
            self.big(ui, ctx, Cmd::OpenFolder);
            self.big(ui, ctx, Cmd::Print);
        });
    }

    fn ribbon_accessibility(&mut self, ui: &mut Ui, ctx: &Context) {
        let de = self.settings.language == Language::German;
        group(ui, if de { "Barrierefreiheit" } else { "Accessibility" }, |ui| {
            self.big(ui, ctx, Cmd::AccessibilityCheck);
            self.big(ui, ctx, UA(Unavailable::Speech, "Vorlesen", "Read aloud", Icon::Accessibility));
        });
    }

    fn ribbon_bookmarks(&mut self, ui: &mut Ui, ctx: &Context) {
        let de = self.settings.language == Language::German;
        // Refreshes the cached bookmark list the enabled state depends on.
        if let Some(tab) = self.tab() {
            tab.bookmarks();
        }
        let groups: [(&str, &str, &[Cmd]); 3] = [
            (
                "Erstellen",
                "Create",
                &[
                    Cmd::BookmarkFromSelection,
                    Cmd::BookmarkEveryNth,
                    Cmd::BookmarkFromToc,
                    Cmd::BookmarkFromHighlights,
                    Cmd::BookmarkFromTextFile,
                ],
            ),
            (
                "Ändern",
                "Change",
                &[
                    Cmd::BookmarkAddText,
                    Cmd::BookmarkCase,
                    Cmd::BookmarkZoom,
                    Cmd::BookmarkResolveNamed,
                    Cmd::BookmarkReplace,
                    Cmd::BookmarkRemoveActions,
                    Cmd::BookmarkSort,
                    Cmd::BookmarkCheck,
                    Cmd::BookmarkDedupe,
                ],
            ),
            (
                "Umwandeln",
                "Convert",
                &[
                    Cmd::BookmarkToc,
                    Cmd::BookmarkLinks,
                    Cmd::BookmarkSortPages,
                    Cmd::BookmarkToNamed,
                    Cmd::BookmarkExportHtml,
                    Cmd::BookmarkExportText,
                ],
            ),
        ];
        for (g, e, cmds) in groups {
            group(ui, if de { g } else { e }, |ui| {
                for &cmd in cmds {
                    self.big(ui, ctx, cmd);
                }
            });
        }
    }

    fn ribbon_help(&mut self, ui: &mut Ui, ctx: &Context) {
        let t = self.t();
        let de = self.settings.language == Language::German;
        group(ui, if de { "Oberfläche" } else { "Interface" }, |ui| {
            self.big(ui, ctx, Cmd::DarkMode);
            ui.vertical(|ui| {
                ui.label(t.language);
                self.language_picker(ui);
            });
        });
        group(ui, t.tab_help, |ui| {
            for cmd in [Cmd::Manual, Cmd::Support, Cmd::ReportProblem, Cmd::CommandLineHelp] {
                self.big(ui, ctx, cmd);
            }
        });
        group(ui, if de { "Programm" } else { "Program" }, |ui| {
            for cmd in [Cmd::CheckUpdate, Cmd::About, Cmd::FileAssociation, Cmd::Homepage] {
                self.big(ui, ctx, cmd);
            }
        });
    }

    /// Contextual tab with the style of the selected comment.
    fn ribbon_format(&mut self, ui: &mut Ui, ctx: &Context) {
        let t = self.t();
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        let Some(info) = tab.selected_annotation() else { return };
        let details = tab.selected_details();
        group(ui, super::sidebar::kind_name(t, info.kind), |ui| {
            ui.vertical(|ui| {
                ui.label(format!("{} {}", t.page, info.page + 1));
                if !info.author.is_empty() {
                    ui.label(RichText::new(&info.author).weak());
                }
            });
        });
        let Some(d) = details.filter(|d| d.restylable) else {
            ui.label(RichText::new(t.not_restylable).weak());
            return;
        };
        let mut update = AnnotUpdate::default();
        group(ui, t.color, |ui| {
            ui.vertical(|ui| {
                for row in PALETTE.chunks(6) {
                    ui.horizontal(|ui| {
                        for &c in row {
                            if swatch(ui, c, Some(c) == d.color).clicked() {
                                update.color = Some(c);
                            }
                        }
                    });
                }
            });
        });
        group(ui, t.fill, |ui| {
            ui.vertical(|ui| {
                let mut on = d.fill.is_some();
                if ui.checkbox(&mut on, t.fill).changed() {
                    update.fill = Some(on.then_some(Rgba::rgb(255, 255, 210)));
                }
                ui.horizontal(|ui| {
                    for &c in &PALETTE[..6] {
                        if swatch(ui, c, Some(c) == d.fill).clicked() {
                            update.fill = Some(Some(c));
                        }
                    }
                });
            });
        });
        group(ui, t.line_width, |ui| {
            ui.vertical(|ui| {
                for row in [[0.5f32, 1.0, 2.0], [3.0, 5.0, 8.0]] {
                    ui.horizontal(|ui| {
                        for w in row {
                            if ui.selectable_label((d.width - w).abs() < 0.01, format!("{w} pt")).clicked() {
                                update.width = Some(w);
                            }
                        }
                    });
                }
            });
        });
        group(ui, t.opacity, |ui| {
            ui.vertical(|ui| {
                for row in [[25u8, 50, 75], [100, 0, 0]] {
                    ui.horizontal(|ui| {
                        for pct in row.into_iter().filter(|p| *p > 0) {
                            let value = f32::from(pct) / 100.0;
                            if ui
                                .selectable_label((d.opacity - value).abs() < 0.01, format!("{pct} %"))
                                .clicked()
                            {
                                update.opacity = Some(value);
                            }
                        }
                    });
                }
            });
        });
        group(ui, t.delete, |ui| {
            self.big(ui, ctx, Cmd::DeleteComment);
        });
        if update != AnnotUpdate::default() {
            self.update_selected(ctx, update);
        }
    }
}
