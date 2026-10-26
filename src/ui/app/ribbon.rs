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
    big_button, big_menu_button, group, icon_button, menu_item, small_button, small_menu_button, swatch,
};

const UA: fn(Unavailable, &'static str, &'static str, Icon) -> Cmd = Cmd::Unavailable;

/// An entry of a ribbon drop-down menu.
pub(super) enum Entry {
    C(Cmd),
    Sep,
    /// Sub menu: icon, (German, English) label, commands.
    Sub(Icon, (&'static str, &'static str), &'static [Cmd]),
}

impl MrPdfApp {
    fn big(&mut self, ui: &mut Ui, ctx: &Context, cmd: Cmd) {
        let label = cmd.label(self.settings.language);
        let resp = big_button(ui, cmd.icon(), label, self.cmd_selected(cmd), self.cmd_enabled(cmd))
            .on_hover_text(self.cmd_tooltip(cmd));
        if resp.clicked() {
            self.run(ctx, cmd);
        }
    }

    fn small(&mut self, ui: &mut Ui, ctx: &Context, cmd: Cmd) {
        let label = cmd.label(self.settings.language);
        let resp = small_button(ui, cmd.icon(), label, self.cmd_selected(cmd), self.cmd_enabled(cmd))
            .on_hover_text(self.cmd_tooltip(cmd));
        if resp.clicked() {
            self.run(ctx, cmd);
        }
    }

    fn icon(&mut self, ui: &mut Ui, ctx: &Context, cmd: Cmd) {
        let resp = icon_button(
            ui,
            cmd.icon(),
            &self.cmd_tooltip(cmd),
            self.cmd_selected(cmd),
            self.cmd_enabled(cmd),
        );
        if resp.clicked() {
            self.run(ctx, cmd);
        }
    }

    fn smalls(&mut self, ui: &mut Ui, ctx: &Context, cmds: &[Cmd]) {
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
    fn big_menu(&mut self, ui: &mut Ui, ctx: &Context, icon: Icon, label: (&str, &str), entries: &[Entry]) {
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

    fn popup_menu(&mut self, ctx: &Context, resp: &egui::Response, entries: &[Entry]) {
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
    fn menu_row(&self, ui: &mut Ui, cmd: Cmd) -> bool {
        let lang = self.settings.language;
        let (de, en) = cmd.menu_names();
        let label = if lang == Language::German { de } else { en };
        let shortcut = cmd.shortcut().map(|(de, en)| if lang == Language::German { de } else { en });
        menu_item(ui, cmd.icon(), label, shortcut, self.cmd_enabled(cmd), self.cmd_selected(cmd), false)
            .on_hover_text(self.cmd_tooltip(cmd))
            .clicked()
    }

    fn tools_group(&mut self, ui: &mut Ui, ctx: &Context) {
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
                self.menu(
                    ui,
                    ctx,
                    Icon::Gear,
                    other,
                    &[
                        Cmd::Tool(Tool::SelectAnnot),
                        Cmd::Tool(Tool::Zoom),
                        Cmd::Tool(Tool::Snapshot),
                        Cmd::Tool(Tool::Eraser),
                        Cmd::Tool(Tool::Link),
                        Cmd::Tool(Tool::MeasureDistance),
                    ],
                );
            });
        });
    }

    pub(super) fn ribbon_ui(&mut self, ui: &mut Ui) {
        let t = self.t();
        let dark = self.settings.dark_mode;
        let ctx = ui.ctx().clone();
        // Form fields have no Format tab; they are edited in the properties panel.
        let has_selection = self
            .tab()
            .and_then(|tab| tab.selected_annotation())
            .is_some_and(|a| a.kind != crate::model::AnnotKind::Widget);
        if !has_selection && self.ribbon == RibbonTab::Format {
            self.ribbon = RibbonTab::Home;
        }
        Panel::top("ribbon")
            .frame(Frame::new().fill(theme::ribbon_background(dark)).inner_margin(Margin::symmetric(6, 3)))
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
                    if has_selection {
                        tabs.push((RibbonTab::Format, t.format_tab));
                    }
                    for (tab, label) in tabs {
                        let mut text = RichText::new(label).size(13.0);
                        if tab == RibbonTab::Format {
                            text = text.color(egui::Color32::from_rgb(160, 110, 0)).strong();
                        }
                        if ui.selectable_label(self.ribbon == tab, text).clicked() {
                            self.ribbon = tab;
                        }
                    }
                });
                ui.add_space(2.0);
                egui::ScrollArea::horizontal()
                    .id_salt("ribbon-scroll")
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
                    .show(ui, |ui| {
                        ui.style_mut().spacing.scroll.floating = false;
                        ui.horizontal(|ui| {
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
                                RibbonTab::Format => self.ribbon_format(ui, &ctx),
                            }
                        });
                    });
            });
    }

    /// The command search ("Schnellstart") at the right end of the tab row.
    fn quick_start(&mut self, ui: &mut Ui, ctx: &Context) {
        let t = self.t();
        let id = egui::Id::new("quick-start");
        let edit = ui.add(
            egui::TextEdit::singleline(&mut self.quick_query)
                .id(id)
                .hint_text(t.quick_start)
                .desired_width(190.0),
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

    fn file_menu(&mut self, ui: &mut Ui) {
        let t = self.t();
        let ctx = ui.ctx().clone();
        let lang = self.settings.language;
        let button = egui::Button::new(RichText::new(t.menu_file).color(egui::Color32::WHITE).strong())
            .fill(theme::BRAND);
        egui::containers::menu::MenuButton::from_button(button).ui(ui, |ui| {
            ui.set_min_width(220.0);
            let mut chosen = None;
            for cmd in [Cmd::New, Cmd::Open] {
                if ui.button(cmd.label(lang)).clicked() {
                    chosen = Some(cmd);
                }
            }
            ui.menu_button(t.recent_files, |ui| {
                if self.settings.recent_files.is_empty() {
                    ui.label(t.none);
                }
                for path in self.settings.recent_files.clone() {
                    if ui.button(path.display().to_string()).clicked() {
                        self.pending_open.push_back(path);
                        ui.close();
                    }
                }
            });
            ui.separator();
            for cmd in [Cmd::Save, Cmd::SaveAs, Cmd::Print, Cmd::ExportImages, Cmd::Email] {
                if ui.add_enabled(self.cmd_enabled(cmd), egui::Button::new(cmd.label(lang))).clicked() {
                    chosen = Some(cmd);
                }
            }
            ui.separator();
            for cmd in [Cmd::DocProperties, Cmd::Settings] {
                if ui.add_enabled(self.cmd_enabled(cmd), egui::Button::new(cmd.label(lang))).clicked() {
                    chosen = Some(cmd);
                }
            }
            ui.separator();
            for cmd in [Cmd::Close, Cmd::Exit] {
                if ui.add_enabled(self.cmd_enabled(cmd), egui::Button::new(cmd.label(lang))).clicked() {
                    chosen = Some(cmd);
                }
            }
            if let Some(cmd) = chosen {
                ui.close();
                self.run(&ctx, cmd);
            }
        });
    }

    fn stamp_picker(&mut self, ui: &mut Ui) {
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
        let Some(tab) = self.tab() else {
            ui.add_enabled(false, egui::DragValue::new(&mut 100.0).suffix(" %"));
            return;
        };
        let mut pct = tab.zoom * 100.0;
        if ui
            .add(egui::DragValue::new(&mut pct).range(5.0..=6400.0).suffix(" %").speed(1.0).max_decimals(0))
            .changed()
        {
            tab.zoom = pct / 100.0;
            tab.zoom_mode = crate::ui::view::ZoomMode::Custom;
        }
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
        group(ui, if de { "Objekte" } else { "Objects" }, |ui| {
            self.big(ui, ctx, UA(Unavailable::TextEditing, "Text bearbeiten", "Edit text", Icon::TextBox));
            self.big(
                ui,
                ctx,
                UA(Unavailable::TextEditing, "Objekte bearbeiten", "Edit objects", Icon::Pointer),
            );
            ui.vertical(|ui| {
                self.small(ui, ctx, Cmd::InsertImage);
                self.small(ui, ctx, Cmd::Tool(Tool::Typewriter));
            });
        });
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
        let t = self.t();
        let de = self.settings.language == Language::German;
        self.tools_group(ui, ctx);
        group(ui, "Navigation", |ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    self.icon(ui, ctx, Cmd::FirstPage);
                    self.icon(ui, ctx, Cmd::PrevPage);
                    if let Some(tab) = self.active_tab() {
                        ui.label(format!("{} / {}", tab.current_page + 1, tab.doc.page_count()));
                    }
                    self.icon(ui, ctx, Cmd::NextPage);
                    self.icon(ui, ctx, Cmd::LastPage);
                });
                self.small(ui, ctx, Cmd::ViewBack);
                self.small(ui, ctx, Cmd::ViewForward);
            });
        });
        group(ui, if de { "Größe" } else { "Size" }, |ui| {
            self.big(ui, ctx, Cmd::ActualSize);
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    self.icon(ui, ctx, Cmd::FitPage);
                    self.icon(ui, ctx, Cmd::FitWidth);
                    self.zoom_box(ui);
                });
                self.small(ui, ctx, Cmd::ZoomIn);
                self.small(ui, ctx, Cmd::ZoomOut);
            });
            self.big(ui, ctx, Cmd::Tool(Tool::Zoom));
        });
        group(ui, if de { "Drehen" } else { "Rotate" }, |ui| {
            self.smalls(ui, ctx, &[Cmd::RotateViewLeft, Cmd::RotateViewRight]);
        });
        group(ui, if de { "Seitenanzeige" } else { "Page display" }, |ui| {
            self.smalls(ui, ctx, &[Cmd::LayoutSingle, Cmd::LayoutContinuous]);
            self.smalls(ui, ctx, &[Cmd::LayoutFacing, Cmd::LayoutCover]);
        });
        group(ui, if de { "Fenster" } else { "Window" }, |ui| {
            self.smalls(ui, ctx, &[Cmd::ToggleSidebar, Cmd::ToggleProperties, Cmd::Fullscreen]);
            self.big(ui, ctx, Cmd::DarkMode);
            ui.vertical(|ui| {
                ui.label(t.language);
                self.language_picker(ui);
            });
        });
        group(ui, if de { "Vorlesen" } else { "Read aloud" }, |ui| {
            self.big(ui, ctx, UA(Unavailable::Speech, "Vorlesen", "Read aloud", Icon::Accessibility));
        });
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

    fn ribbon_comment(&mut self, ui: &mut Ui, ctx: &Context) {
        let de = self.settings.language == Language::German;
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
                &[
                    Cmd::Tool(Tool::Note),
                    Cmd::Tool(Tool::FileAttach),
                    UA(Unavailable::Media, "Audio", "Audio", Icon::Comments),
                ],
            );
        });
        group(ui, if de { "Textmarkierung" } else { "Text markup" }, |ui| {
            self.smalls(
                ui,
                ctx,
                &[Cmd::Tool(Tool::Highlight), Cmd::Tool(Tool::AreaHighlight), Cmd::Tool(Tool::Squiggly)],
            );
            self.smalls(ui, ctx, &[Cmd::Tool(Tool::Underline), Cmd::Tool(Tool::StrikeOut)]);
            self.smalls(ui, ctx, &[Cmd::Tool(Tool::Replace), Cmd::Tool(Tool::Caret)]);
        });
        group(ui, if de { "Zeichnen" } else { "Drawing" }, |ui| {
            ui.vertical(|ui| {
                for row in [
                    [Tool::Line, Tool::Ellipse, Tool::Polygon],
                    [Tool::Arrow, Tool::Rectangle, Tool::Polyline],
                    [Tool::Cloud, Tool::Pencil, Tool::Eraser],
                ] {
                    ui.horizontal(|ui| {
                        for tool in row {
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
            self.smalls(
                ui,
                ctx,
                &[
                    UA(Unavailable::Media, "Video hinzufügen", "Add video", Icon::Image),
                    UA(Unavailable::Media, "Audio hinzufügen", "Add audio", Icon::Comments),
                    UA(Unavailable::Media, "3D hinzufügen", "Add 3D", Icon::Background),
                ],
            );
        });
        group(ui, if de { "Anmerkungen verwalten" } else { "Manage comments" }, |ui| {
            self.big(ui, ctx, Cmd::SummarizeComments);
            self.smalls(ui, ctx, &[Cmd::ImportComments, Cmd::ExportComments, Cmd::ToggleAnnotations]);
            self.smalls(ui, ctx, &[Cmd::CommentList, Cmd::Flatten, Cmd::DeleteComment]);
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

    fn ribbon_form(&mut self, ui: &mut Ui, ctx: &Context) {
        let de = self.settings.language == Language::German;
        self.tools_group(ui, ctx);
        group(ui, if de { "Felder" } else { "Fields" }, |ui| {
            self.big(ui, ctx, Cmd::Tool(Tool::SelectFields));
        });
        group(ui, if de { "Formularfelder" } else { "Form fields" }, |ui| {
            for tool in [
                Tool::FieldText,
                Tool::FieldButton,
                Tool::FieldCheckBox,
                Tool::FieldRadio,
                Tool::FieldList,
                Tool::FieldCombo,
                Tool::FieldSignature,
            ] {
                self.big(ui, ctx, Cmd::Tool(tool));
            }
            self.smalls(ui, ctx, &[Cmd::HighlightFields]);
        });
        group(ui, if de { "Formulardaten" } else { "Form data" }, |ui| {
            self.smalls(ui, ctx, &[Cmd::ResetForm, Cmd::ImportFormData, Cmd::ExportFormData]);
            let fill = if de { "Formulare ausfüllen" } else { "Fill in forms" };
            if big_button(ui, Icon::Form, fill, self.tool == Tool::Hand, !self.tabs.is_empty()).clicked() {
                self.select_tool(Tool::Hand);
            }
        });
        group(ui, "JavaScript", |ui| {
            self.smalls(
                ui,
                ctx,
                &[
                    UA(Unavailable::JavaScript, "JavaScript-Konsole", "JavaScript console", Icon::Console),
                    UA(Unavailable::JavaScript, "Dokumentaktionen", "Document actions", Icon::Console),
                    UA(Unavailable::JavaScript, "Alle JavaScripte", "All JavaScripts", Icon::Console),
                ],
            );
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
            self.big(ui, ctx, Cmd::FromFiles);
            self.big(ui, ctx, Cmd::MergeFiles);
            self.big(ui, ctx, Cmd::New);
        });
        group(ui, if de { "Ändern" } else { "Change" }, |ui| {
            self.big(ui, ctx, Cmd::ContentsToImages);
        });
        group(ui, if de { "Exportieren" } else { "Export" }, |ui| {
            self.big(ui, ctx, Cmd::ExportImages);
            self.big(ui, ctx, Cmd::ExportWord);
            self.big(ui, ctx, Cmd::ExportText);
            self.smalls(
                ui,
                ctx,
                &[
                    UA(Unavailable::Office, "In MS Excel", "To MS Excel", Icon::Export),
                    UA(Unavailable::Office, "In MS PowerPoint", "To MS PowerPoint", Icon::Export),
                ],
            );
        });
        group(ui, if de { "Texterkennung" } else { "Text recognition" }, |ui| {
            self.big(
                ui,
                ctx,
                UA(Unavailable::Ocr, "Text auf Seiten erkennen", "Recognize text", Icon::Search),
            );
        });
        group(ui, if de { "Farben umwandeln" } else { "Convert colours" }, |ui| {
            self.big(ui, ctx, Cmd::Grayscale);
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

    fn ribbon_review(&mut self, ui: &mut Ui, ctx: &Context) {
        let de = self.settings.language == Language::German;
        self.tools_group(ui, ctx);
        group(ui, if de { "Rechtschreibung" } else { "Spelling" }, |ui| {
            self.big(ui, ctx, UA(Unavailable::Spelling, "Rechtschreibprüfung", "Spell check", Icon::Count));
            self.big(ui, ctx, Cmd::WordCount);
            self.big(ui, ctx, Cmd::CompareDocuments);
        });
        group(ui, if de { "Anmerkungen" } else { "Comments" }, |ui| {
            self.big(ui, ctx, Cmd::DeleteAllComments);
            self.big(ui, ctx, Cmd::PrevComment);
            self.big(ui, ctx, Cmd::NextComment);
            self.big(ui, ctx, Cmd::ToggleAnnotations);
            self.big(ui, ctx, Cmd::Flatten);
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
        group(ui, if de { "Erstellen" } else { "Create" }, |ui| {
            for cmd in [
                Cmd::BookmarkAdd,
                Cmd::BookmarkFromSelection,
                Cmd::BookmarkEveryNth,
                Cmd::BookmarkFromHighlights,
                Cmd::BookmarkFromTextFile,
            ] {
                self.big(ui, ctx, cmd);
            }
        });
        group(ui, if de { "Ändern" } else { "Change" }, |ui| {
            self.menu(
                ui,
                ctx,
                Icon::TextBox,
                if de { "Groß-/Kleinschreibung" } else { "Case" },
                &[Cmd::BookmarkUpper, Cmd::BookmarkLower, Cmd::BookmarkTitleCase],
            );
            self.smalls(ui, ctx, &[Cmd::BookmarkReplace, Cmd::BookmarkSort]);
            self.smalls(ui, ctx, &[Cmd::BookmarkDedupe, Cmd::BookmarkDeleteAll]);
        });
        group(ui, if de { "Umwandeln" } else { "Convert" }, |ui| {
            self.big(ui, ctx, Cmd::BookmarkToc);
            self.big(ui, ctx, Cmd::BookmarkExportHtml);
            self.big(ui, ctx, Cmd::BookmarkExportText);
        });
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
