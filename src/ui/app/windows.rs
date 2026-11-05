//! Tab groups and documents in their own windows (Ansicht › Dokument-Tabs:
//! "Tab verschieben nach", "Mehrere Tabs verschieben", "Tabs anordnen",
//! "… zusammenfassen").

use egui::{Context, RichText, Ui};

use super::dialogs::{Action, Outcome};
use super::page_dialogs::tr;
use super::{Dialog, MrPdfApp};
use crate::ui::i18n::Language;
use crate::ui::icons::Icon;
use crate::ui::theme;
use crate::ui::widgets::icon_button;

/// Where tabs are moved to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveTarget {
    /// A new group beside the others ("Neue vertikale Tab-Gruppe").
    NewVerticalGroup,
    /// A new group below the others ("Neue horizontale Tab-Gruppe").
    NewHorizontalGroup,
    /// "Neues Fenster".
    NewWindow,
    /// Back into the main window.
    MainWindow,
}

impl MoveTarget {
    pub fn names(self) -> (&'static str, &'static str) {
        match self {
            MoveTarget::NewVerticalGroup => ("Neue vertikale Tab-Gruppe", "New vertical tab group"),
            MoveTarget::NewHorizontalGroup => ("Neue horizontale Tab-Gruppe", "New horizontal tab group"),
            MoveTarget::NewWindow => ("Neues Fenster", "New window"),
            MoveTarget::MainWindow => ("Hauptfenster", "Main window"),
        }
    }
}

/// State of "Mehrere Tabs verschieben".
#[derive(Debug, Clone, PartialEq)]
pub struct MoveTabsState {
    pub selected: Vec<u64>,
    pub search: String,
    pub target: MoveTarget,
}

/// What the user did in a tab strip.
enum StripAction {
    Show(usize),
    Close(usize),
    Move(usize, MoveTarget),
}

impl MrPdfApp {
    fn wn_de(&self) -> bool {
        self.settings.language == Language::German
    }

    /// The tab groups of a window, in order.
    pub(super) fn window_groups(&self, window: u32) -> Vec<u32> {
        let mut groups: Vec<u32> = self.tabs.iter().filter(|t| t.window == window).map(|t| t.group).collect();
        groups.sort_unstable();
        groups.dedup();
        groups
    }

    /// The other windows that have documents.
    pub(super) fn extra_windows(&self) -> Vec<u32> {
        let mut w: Vec<u32> = self.tabs.iter().map(|t| t.window).filter(|w| *w != 0).collect();
        w.sort_unstable();
        w.dedup();
        w
    }

    fn group_tabs(&self, window: u32, group: u32) -> Vec<usize> {
        (0..self.tabs.len())
            .filter(|&i| self.tabs[i].window == window && self.tabs[i].group == group)
            .collect()
    }

    /// The tab shown in a group: the active one, else the one last shown.
    pub(super) fn shown_in_group(&self, window: u32, group: u32) -> Option<usize> {
        let tabs = self.group_tabs(window, group);
        if tabs.contains(&self.active) {
            return Some(self.active);
        }
        let last = self.group_focus.get(&(window, group));
        tabs.iter().copied().find(|&i| Some(&self.tabs[i].id) == last).or_else(|| tabs.first().copied())
    }

    /// Remembers the tab shown in the active tab's group.
    pub(super) fn note_group_focus(&mut self) {
        if let Some(t) = self.tabs.get(self.active) {
            self.group_focus.insert((t.window, t.group), t.id);
        }
    }

    /// Moves tabs to a target; "Neues Fenster" starts a separate MrPDF with
    /// the full ribbon for them.
    pub(super) fn move_tabs_to(&mut self, ctx: &Context, ids: &[u64], target: MoveTarget) {
        if target == MoveTarget::NewWindow {
            self.new_window = Some((ids.to_vec(), Vec::new()));
            self.continue_new_window(ctx);
        } else {
            self.move_tabs(ids, target);
        }
    }

    /// Next step of "Neues Fenster": asks about unsaved changes one document
    /// at a time, then starts the new MrPDF and closes the tabs here.
    pub(super) fn continue_new_window(&mut self, ctx: &Context) {
        let Some((ids, asked)) = self.new_window.clone() else { return };
        let ask =
            self.tabs.iter().find(|t| ids.contains(&t.id) && t.doc.is_modified() && !asked.contains(&t.id));
        if let Some(tab) = ask {
            self.active = self.tabs.iter().position(|t| t.id == tab.id).unwrap_or(self.active);
            self.dialog = Some(Dialog::Unsaved { tab_id: tab.id, then: super::AfterSave::NewWindow });
            return;
        }
        self.new_window = None;
        // Documents without a file (never saved) stay here.
        let moved: Vec<(u64, std::path::PathBuf)> = self
            .tabs
            .iter()
            .filter(|t| ids.contains(&t.id))
            .filter_map(|t| t.doc.path().map(|p| (t.id, p.to_path_buf())))
            .collect();
        let de = self.wn_de();
        if moved.is_empty() {
            self.set_status(
                ctx,
                tr(
                    de,
                    "Nur gespeicherte Dokumente lassen sich in ein neues Fenster verschieben.",
                    "Only saved documents can be moved to a new window.",
                ),
            );
            return;
        }
        let paths: Vec<std::path::PathBuf> = moved.iter().map(|(_, p)| p.clone()).collect();
        if let Err(err) = start_new_window(&paths) {
            log::error!("Neues Fenster nicht gestartet: {err}");
            self.dialog = Some(Dialog::Message {
                title: tr(de, "Neues Fenster", "New window").into(),
                text: if de {
                    format!("MrPDF konnte kein neues Fenster starten:\n{err}")
                } else {
                    format!("MrPDF could not start a new window:\n{err}")
                },
            });
            return;
        }
        for (id, path) in &moved {
            // Changes not saved on purpose are dropped with the tab here.
            self.close_tab(ctx, *id);
            self.closed_files.retain(|p| p != path);
        }
        self.spawned_windows.push(paths);
    }

    /// Moves tabs (by id) to another group or window.
    pub(super) fn move_tabs(&mut self, ids: &[u64], target: MoveTarget) {
        let active_window = self.tabs.get(self.active).map_or(0, |t| t.window);
        let (window, group) = match target {
            MoveTarget::NewWindow => (self.tabs.iter().map(|t| t.window).max().unwrap_or(0) + 1, 0),
            MoveTarget::MainWindow => (0, 0),
            MoveTarget::NewVerticalGroup | MoveTarget::NewHorizontalGroup => {
                self.settings.tab_groups_vertical = target == MoveTarget::NewVerticalGroup;
                let group = self
                    .tabs
                    .iter()
                    .filter(|t| t.window == active_window)
                    .map(|t| t.group)
                    .max()
                    .unwrap_or(0)
                    + 1;
                (active_window, group)
            }
        };
        for tab in self.tabs.iter_mut().filter(|t| ids.contains(&t.id)) {
            tab.window = window;
            tab.group = group;
        }
        self.compact_groups();
        if let Some(i) = self.tabs.iter().position(|t| ids.first() == Some(&t.id)) {
            self.active = i;
        }
    }

    /// Group numbers without gaps (an emptied group disappears).
    fn compact_groups(&mut self) {
        let windows: Vec<u32> = {
            let mut w: Vec<u32> = self.tabs.iter().map(|t| t.window).collect();
            w.sort_unstable();
            w.dedup();
            w
        };
        for w in windows {
            for (new, old) in self.window_groups(w).into_iter().enumerate() {
                for t in self.tabs.iter_mut().filter(|t| t.window == w && t.group == old) {
                    t.group = new as u32;
                }
            }
        }
    }

    /// "Alle Tabs in Tab-Gruppe zusammenfassen": one group in the active window.
    pub(super) fn merge_groups(&mut self) {
        let window = self.tabs.get(self.active).map_or(0, |t| t.window);
        for t in self.tabs.iter_mut().filter(|t| t.window == window) {
            t.group = 0;
        }
    }

    /// "Alle Tabs in aktivem Fenster zusammenfassen".
    pub(super) fn merge_windows(&mut self) {
        let (window, group) = self.tabs.get(self.active).map_or((0, 0), |t| (t.window, t.group));
        for t in self.tabs.iter_mut() {
            if t.window != window {
                t.window = window;
                t.group = group;
            }
        }
        self.compact_groups();
    }

    /// A row of document tabs; `shown` is highlighted.
    fn tab_strip(
        &mut self,
        ui: &mut Ui,
        indices: &[usize],
        shown: Option<usize>,
        salt: impl std::hash::Hash + std::fmt::Debug,
    ) -> Option<StripAction> {
        let de = self.wn_de();
        let t = self.t();
        let mut action = None;
        egui::ScrollArea::horizontal()
            .id_salt(("doc-tabs-scroll", salt))
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for &i in indices {
                        let tab = &self.tabs[i];
                        let selected = Some(i) == shown;
                        let focused = i == self.active;
                        let frame = egui::Frame::new()
                            .inner_margin(egui::Margin::symmetric(8, 3))
                            .corner_radius(egui::CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 })
                            .fill(if selected {
                                ui.visuals().extreme_bg_color
                            } else {
                                ui.visuals().faint_bg_color
                            })
                            .stroke(egui::Stroke::new(
                                if focused { 1.5 } else { 1.0 },
                                if selected {
                                    theme::ACCENT
                                } else {
                                    ui.visuals().widgets.noninteractive.bg_stroke.color
                                },
                            ));
                        frame.show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let title = RichText::new(tab.title());
                                let title = if selected { title.strong() } else { title };
                                let label = ui.add(egui::Label::new(title).sense(egui::Sense::click()));
                                let label = match tab.doc.path() {
                                    Some(p) => label.on_hover_text(p.display().to_string()),
                                    None => label,
                                };
                                if label.clicked() {
                                    action = Some(StripAction::Show(i));
                                }
                                if label.middle_clicked() {
                                    action = Some(StripAction::Close(i));
                                }
                                label.context_menu(|ui| {
                                    let in_window = tab.window != 0;
                                    for target in [
                                        MoveTarget::NewVerticalGroup,
                                        MoveTarget::NewHorizontalGroup,
                                        MoveTarget::NewWindow,
                                        MoveTarget::MainWindow,
                                    ] {
                                        if target == MoveTarget::MainWindow && !in_window {
                                            continue;
                                        }
                                        let (g, e) = target.names();
                                        let text = if de {
                                            format!("Verschieben: {g}")
                                        } else {
                                            format!("Move: {e}")
                                        };
                                        if ui.button(text).clicked() {
                                            action = Some(StripAction::Move(i, target));
                                            ui.close();
                                        }
                                    }
                                    ui.separator();
                                    if ui.button(tr(de, "Schließen", "Close")).clicked() {
                                        action = Some(StripAction::Close(i));
                                        ui.close();
                                    }
                                });
                                if icon_button(ui, Icon::Close, t.close, false, true).clicked() {
                                    action = Some(StripAction::Close(i));
                                }
                            });
                        });
                    }
                });
            });
        action
    }

    fn apply_strip(&mut self, ctx: &Context, action: Option<StripAction>) {
        match action {
            Some(StripAction::Show(i)) => self.active = i,
            Some(StripAction::Close(i)) => self.request_close_tab(ctx, i),
            Some(StripAction::Move(i, target)) => {
                let id = self.tabs[i].id;
                self.move_tabs_to(ctx, &[id], target);
            }
            None => {}
        }
    }

    /// The tab bar of the main window when it has one group.
    pub(super) fn main_tab_strip(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let group = self.window_groups(0).first().copied().unwrap_or(0);
        let indices = self.group_tabs(0, group);
        let shown = self.shown_in_group(0, group);
        let action = self.tab_strip(ui, &indices, shown, ("main", group));
        self.apply_strip(&ctx, action);
    }

    /// The document area of the main window with several tab groups.
    pub(super) fn central_groups(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let groups = self.window_groups(0);
        let area = ui.available_rect_before_wrap();
        ui.allocate_rect(area, egui::Sense::hover());
        let n = groups.len().max(1) as f32;
        let vertical = self.settings.tab_groups_vertical;
        let pressed = ctx.input(|i| i.pointer.any_pressed().then(|| i.pointer.interact_pos()).flatten());
        for (k, group) in groups.into_iter().enumerate() {
            let k = k as f32;
            let rect = if vertical {
                egui::Rect::from_min_max(
                    egui::pos2(area.left() + area.width() * k / n, area.top()),
                    egui::pos2(area.left() + area.width() * (k + 1.0) / n, area.bottom()),
                )
            } else {
                egui::Rect::from_min_max(
                    egui::pos2(area.left(), area.top() + area.height() * k / n),
                    egui::pos2(area.right(), area.top() + area.height() * (k + 1.0) / n),
                )
            };
            let rect = rect.shrink(2.0);
            let shown = self.shown_in_group(0, group);
            // A press inside a group makes its document the active one.
            if pressed.is_some_and(|p| rect.contains(p))
                && let Some(i) = shown
            {
                self.active = i;
            }
            let mut child = ui.new_child(
                egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)),
            );
            child.set_clip_rect(rect);
            let indices = self.group_tabs(0, group);
            let action = self.tab_strip(&mut child, &indices, shown, ("group", group));
            self.apply_strip(&ctx, action);
            if let Some(i) = self.shown_in_group(0, group) {
                self.doc_view(&mut child, i);
            }
            let focused = self.tabs.get(self.active).is_some_and(|t| t.window == 0 && t.group == group);
            let stroke = if focused {
                egui::Stroke::new(1.5, theme::ACCENT)
            } else {
                egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color)
            };
            ui.painter().rect_stroke(rect, 0.0, stroke, egui::StrokeKind::Outside);
        }
    }

    /// Documents moved into their own windows.
    pub(super) fn doc_windows(&mut self, ctx: &Context) {
        for w in self.extra_windows() {
            let id = egui::ViewportId::from_hash_of(("mrpdf-window", w));
            let title = self
                .shown_in_group(w, 0)
                .map_or_else(|| "MrPDF".to_string(), |i| format!("{} - MrPDF", self.tabs[i].title()));
            let builder = egui::ViewportBuilder::default().with_title(title).with_inner_size([900.0, 720.0]);
            let close = ctx.show_viewport_immediate(id, builder, |ui, _class| self.doc_window_ui(ui, w));
            if close {
                // Closing a window brings its documents back (nothing is lost).
                let ids: Vec<u64> = self.tabs.iter().filter(|t| t.window == w).map(|t| t.id).collect();
                self.move_tabs(&ids, MoveTarget::MainWindow);
            }
        }
    }

    /// One document window; returns true when it should close.
    fn doc_window_ui(&mut self, ui: &mut Ui, window: u32) -> bool {
        let ctx = ui.ctx().clone();
        let de = self.wn_de();
        let mut close = ui.input(|i| i.viewport().close_requested());
        if ui.input(|i| i.pointer.any_pressed())
            && let Some(i) = self.shown_in_group(window, 0)
        {
            self.active = i;
        }
        let panel =
            egui::Frame::new().fill(ui.visuals().panel_fill).inner_margin(egui::Margin::symmetric(6, 4));
        panel.show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button(tr(de, "Ins Hauptfenster", "To main window")).clicked() {
                    close = true;
                }
                ui.separator();
                if let Some(i) = self.shown_in_group(window, 0) {
                    let tab = &mut self.tabs[i];
                    let count = tab.doc.page_count();
                    let cur = tab.current_page;
                    if ui.add_enabled(cur > 0, egui::Button::new("◀")).clicked() {
                        tab.go_to_page(cur - 1);
                    }
                    ui.label(format!("{} / {count}", cur + 1));
                    if ui.add_enabled(cur + 1 < count, egui::Button::new("▶")).clicked() {
                        tab.go_to_page(cur + 1);
                    }
                    ui.separator();
                    if ui.button("−").clicked() {
                        tab.zoom = crate::ui::view::zoom_out(tab.zoom);
                        tab.zoom_mode = crate::ui::view::ZoomMode::Custom;
                    }
                    ui.label(format!("{:.0} %", tab.zoom * 100.0));
                    if ui.button("+").clicked() {
                        tab.zoom = crate::ui::view::zoom_in(tab.zoom);
                        tab.zoom_mode = crate::ui::view::ZoomMode::Custom;
                    }
                    if ui.button(tr(de, "Breite", "Width")).clicked() {
                        tab.zoom_mode = crate::ui::view::ZoomMode::FitWidth;
                    }
                    if ui.button(tr(de, "Seite", "Page")).clicked() {
                        tab.zoom_mode = crate::ui::view::ZoomMode::FitPage;
                    }
                }
            });
            let indices = self.group_tabs(window, 0);
            let shown = self.shown_in_group(window, 0);
            let action = self.tab_strip(ui, &indices, shown, ("window", window));
            self.apply_strip(&ctx, action);
        });
        if let Some(i) = self.shown_in_group(window, 0) {
            // Canvas events of this window act on its document.
            let previous = self.active;
            self.active = i;
            self.doc_view(ui, i);
            self.process_events(&ctx);
            if previous < self.tabs.len() {
                self.active = previous;
            }
        }
        close
    }

    /// Opens "Mehrere Tabs verschieben".
    pub(super) fn open_move_tabs(&mut self) {
        self.dialog = Some(Dialog::MoveTabs(Box::new(MoveTabsState {
            selected: Vec::new(),
            search: String::new(),
            target: MoveTarget::NewVerticalGroup,
        })));
    }

    pub(super) fn move_tabs_dialog(
        &mut self,
        ui: &mut Ui,
        s: &mut MoveTabsState,
    ) -> (Outcome, Option<Action>) {
        let de = self.wn_de();
        super::dialogs::set_title(ui, tr(de, "Mehrere Tabs verschieben", "Move several tabs"));
        ui.horizontal(|ui| {
            ui.label(tr(de, "Tabs zum Verschieben auswählen:", "Select the tabs to move:"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(if de {
                    format!("({} von {} ausgewählt)", s.selected.len(), self.tabs.len())
                } else {
                    format!("({} of {} selected)", s.selected.len(), self.tabs.len())
                });
            });
        });
        ui.horizontal(|ui| {
            if ui.button(tr(de, "Alles auswählen", "Select all")).clicked() {
                s.selected = self.tabs.iter().map(|t| t.id).collect();
            }
            if ui
                .add_enabled(
                    !s.selected.is_empty(),
                    egui::Button::new(tr(de, "Alle abwählen", "Deselect all")),
                )
                .clicked()
            {
                s.selected.clear();
            }
            if ui.button(tr(de, "Auswahl umkehren", "Invert selection")).clicked() {
                s.selected = self.tabs.iter().map(|t| t.id).filter(|id| !s.selected.contains(id)).collect();
            }
        });
        ui.add(
            egui::TextEdit::singleline(&mut s.search)
                .hint_text(tr(de, "Suchen…", "Search…"))
                .desired_width(f32::INFINITY),
        );
        let query = s.search.trim().to_lowercase();
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_min_size(egui::vec2(380.0, 160.0));
            ui.label(RichText::new(tr(de, "Titel des Tabs", "Tab title")).weak());
            egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                for tab in &self.tabs {
                    let title = tab.title();
                    if !query.is_empty() && !title.to_lowercase().contains(&query) {
                        continue;
                    }
                    let mut on = s.selected.contains(&tab.id);
                    if ui.checkbox(&mut on, title).changed() {
                        if on {
                            s.selected.push(tab.id);
                        } else {
                            s.selected.retain(|id| *id != tab.id);
                        }
                    }
                }
            });
        });
        ui.label(tr(de, "Alle ausgewählten Tabs verschieben in:", "Move all selected tabs to:"));
        for target in [
            MoveTarget::NewVerticalGroup,
            MoveTarget::NewHorizontalGroup,
            MoveTarget::NewWindow,
            MoveTarget::MainWindow,
        ] {
            let (g, e) = target.names();
            ui.radio_value(&mut s.target, target, tr(de, g, e));
        }
        let (ok, cancel, _) = super::page_dialogs::footer(ui, de, !s.selected.is_empty(), false, "OK");
        if cancel {
            return (Outcome::Close, None);
        }
        if !ok {
            return (Outcome::Keep, None);
        }
        let (ids, target) = (s.selected.clone(), s.target);
        (Outcome::Close, Some(Box::new(move |app, ctx| app.move_tabs_to(ctx, &ids, target))))
    }
}

/// Starts a separate MrPDF window with the files (not handed to this one).
/// Automated tests (`MRPDF_NO_EXTERNAL`) start nothing.
fn start_new_window(paths: &[std::path::PathBuf]) -> std::io::Result<()> {
    if crate::mail::no_external() {
        return Ok(());
    }
    let exe = std::env::current_exe()?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--new-window");
    if std::env::args_os().any(|a| a == "--opengl") {
        cmd.arg("--opengl");
    }
    cmd.args(paths);
    cmd.spawn().map(drop)
}
