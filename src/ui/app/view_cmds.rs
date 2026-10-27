//! Commands of the ribbon tab "Ansicht" that MrPDF had not had before:
//! page display options, rulers, grid, guides, split
//! view, tab and pane menus, designs, loupe, pan & zoom and read aloud.

use egui::{Context, Ui};

use super::settings_dialog::Cat;
use super::{MrPdfApp, SidePanel};
use crate::geometry::PointF;
use crate::ui::i18n::Language;
use crate::ui::icons::Icon;
use crate::ui::tab::{ScrollRequest, Split};
use crate::ui::theme::Design;
use crate::ui::tools::Tool;
use crate::ui::view::{self, PageLayout, ZoomMode};

/// The commands; used as `Cmd::View(..)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ViewCmd {
    LayoutTwoPagesContinuous,
    PageGaps,
    RightToLeft,
    StandardLayout,
    SideBySide,
    RotateReset,
    Rulers,
    Grid,
    Guides,
    Transparency,
    PageInfo,
    Snap,
    SnapGrid,
    SnapGuides,
    SnapSettings,
    ReplaceColors,
    AutoScroll,
    HighlightLinks,
    LoupeWindow,
    PanZoom,
    SplitHorizontal,
    SplitVertical,
    SplitGrid,
    NewTab,
    SortTabs,
    CloseOthers,
    ReopenClosed,
    ResetPanes,
    ResetDocPanes,
    Design(Design),
    CustomizeUi,
    SnapshotSettings,
    ReadStart,
    ReadStop,
    ReadBack,
    ReadForward,
    ReadShow,
    ReadPage,
    ReadToEnd,
    ReadSelection,
    ReadFields,
    ReadAnnots,
    ReadFollow,
    Transitions,
    MoveTabNewVertical,
    MoveTabNewHorizontal,
    MoveTabNewWindow,
    MoveTabMain,
    MoveTabs,
    ArrangeVertical,
    ArrangeHorizontal,
    MergeGroups,
    MergeWindows,
    NewPortfolio,
    PortfolioList,
    PortfolioCover,
    PortfolioAddFiles,
    PortfolioAddFolder,
    PortfolioMark,
}

impl ViewCmd {
    pub fn names(self) -> (&'static str, &'static str) {
        use ViewCmd::*;
        match self {
            LayoutTwoPagesContinuous => ("Zwei Seiten fortlaufend", "Two pages continuous"),
            PageGaps => ("Seitenzwischenräume", "Page gaps"),
            RightToLeft => ("Rechts-nach-Links-Seitenansicht", "Right-to-left page view"),
            StandardLayout => ("Standardlayout", "Standard layout"),
            SideBySide => ("Seiten nebeneinander", "Pages side by side"),
            RotateReset => ("Ausrichtung zurücksetzen", "Reset orientation"),
            Rulers => ("Lineale", "Rulers"),
            Grid => ("Gitter", "Grid"),
            Guides => ("Hilfslinien", "Guides"),
            Transparency => ("Transparenzgitter", "Transparency grid"),
            PageInfo => ("Seitenmaße/Position", "Page size/position"),
            Snap => ("Einrasten", "Snap"),
            SnapGrid => ("Am Gitter", "To grid"),
            SnapGuides => ("An Hilfslinien", "To guides"),
            SnapSettings => ("Einstellungen für das Einrasten…", "Snap settings…"),
            ReplaceColors => ("Dokumentfarben ersetzen", "Replace document colours"),
            AutoScroll => ("Automatisch blättern", "Auto scroll"),
            HighlightLinks => ("Verweise hervorheben", "Highlight links"),
            LoupeWindow => ("Lupe", "Loupe"),
            PanZoom => ("Ausschnitt und Vergrößerung", "Pan and zoom"),
            SplitHorizontal => ("Horizontal teilen", "Split horizontally"),
            SplitVertical => ("Vertikal teilen", "Split vertically"),
            SplitGrid => ("Als Tabelle teilen", "Split as table"),
            NewTab => ("Neues Tab", "New tab"),
            SortTabs => ("Tabs alphabetisch sortieren", "Sort tabs alphabetically"),
            CloseOthers => ("Alle anderen Tabs schließen", "Close all other tabs"),
            ReopenClosed => {
                ("Kürzlich geschlossene Dokumente erneut öffnen", "Reopen recently closed documents")
            }
            ResetPanes => ("Hauptleisten zurücksetzen", "Reset main panes"),
            ResetDocPanes => ("Dokumentleisten zurücksetzen", "Reset document panes"),
            Design(d) => d.names(),
            CustomizeUi => ("Oberfläche anpassen…", "Customize UI…"),
            SnapshotSettings => ("Einstellungen für Bildschirmfotos…", "Snapshot settings…"),
            ReadStart => ("Beginn", "Start"),
            ReadStop => ("Ende", "Stop"),
            ReadBack => ("Zurück", "Back"),
            ReadForward => ("Vorwärts", "Forward"),
            ReadShow => ("Anzeigen", "Show"),
            ReadPage => ("Seite vorlesen", "Read page"),
            ReadToEnd => ("Bis zum Ende vorlesen", "Read to end"),
            ReadSelection => ("Auswahl vorlesen", "Read selection"),
            ReadFields => ("Formularfelder vorlesen", "Read form fields"),
            ReadAnnots => ("Anmerkungen vorlesen", "Read comments"),
            ReadFollow => ("Beim Vorlesen automatisch blättern", "Follow the reading"),
            Transitions => ("Seitenübergänge", "Page transitions"),
            MoveTabNewVertical => ("Neue vertikale Tab-Gruppe", "New vertical tab group"),
            MoveTabNewHorizontal => ("Neue horizontale Tab-Gruppe", "New horizontal tab group"),
            MoveTabNewWindow => ("Neues Fenster", "New window"),
            MoveTabMain => ("Hauptfenster", "Main window"),
            MoveTabs => ("Mehrere Tabs verschieben…", "Move several tabs…"),
            ArrangeVertical => ("Vertikal", "Vertical"),
            ArrangeHorizontal => ("Horizontal", "Horizontal"),
            MergeGroups => ("Alle Tabs in Tab-Gruppe zusammenfassen", "Merge all tabs into one tab group"),
            MergeWindows => {
                ("Alle Tabs in aktivem Fenster zusammenfassen", "Merge all tabs into the active window")
            }
            NewPortfolio => ("PDF-Portfolio", "PDF portfolio"),
            PortfolioList => ("Dateiliste anzeigen", "Show file list"),
            PortfolioCover => ("Deckblatt anzeigen", "Show cover page"),
            PortfolioAddFiles => ("Dateien hinzufügen…", "Add files…"),
            PortfolioAddFolder => ("Ordner hinzufügen…", "Add folder…"),
            PortfolioMark => ("Als PDF-Portfolio anzeigen", "Show as PDF portfolio"),
        }
    }

    pub fn icon(self) -> Icon {
        use ViewCmd::*;
        match self {
            LayoutTwoPagesContinuous => Icon::CoverPages,
            PageGaps => Icon::Continuous,
            RightToLeft => Icon::Back,
            StandardLayout => Icon::Continuous,
            SideBySide => Icon::TwoPages,
            RotateReset => Icon::Undo,
            Rulers => Icon::MeasureDistance,
            Grid | Transparency => Icon::Thumbnails,
            Guides => Icon::Line,
            PageInfo => Icon::Resize,
            Snap | SnapGrid | SnapGuides | SnapSettings => Icon::Margins,
            ReplaceColors => Icon::Accessibility,
            AutoScroll => Icon::Next,
            HighlightLinks => Icon::Link,
            LoupeWindow => Icon::Search,
            PanZoom => Icon::FitPage,
            SplitHorizontal | SplitVertical | SplitGrid => Icon::Split,
            NewTab => Icon::New,
            SortTabs => Icon::Sort,
            CloseOthers => Icon::Close,
            ReopenClosed => Icon::Open,
            ResetPanes | ResetDocPanes => Icon::Sidebar,
            Design(_) | CustomizeUi => Icon::Theme,
            SnapshotSettings => Icon::Camera,
            ReadStart | ReadPage | ReadToEnd | ReadSelection => Icon::Accessibility,
            ReadStop => Icon::Close,
            ReadBack => Icon::Prev,
            ReadForward => Icon::Next,
            ReadShow => Icon::Eye,
            ReadFields => Icon::Form,
            ReadAnnots => Icon::Comments,
            ReadFollow => Icon::Continuous,
            Transitions => Icon::Next,
            MoveTabNewVertical | ArrangeVertical => Icon::TwoPages,
            MoveTabNewHorizontal | ArrangeHorizontal => Icon::Continuous,
            MoveTabNewWindow | MoveTabMain | MoveTabs => Icon::Export,
            MergeGroups | MergeWindows => Icon::Merge,
            NewPortfolio | PortfolioList | PortfolioMark => Icon::Attachment,
            PortfolioCover => Icon::SinglePage,
            PortfolioAddFiles => Icon::InsertFile,
            PortfolioAddFolder => Icon::Open,
        }
    }

    pub fn shortcut(self) -> Option<(&'static str, &'static str)> {
        use ViewCmd::*;
        Some(match self {
            PageGaps => ("Strg+Umschalt+G", "Ctrl+Shift+G"),
            RotateReset => ("Strg+Umschalt+0", "Ctrl+Shift+0"),
            Rulers => ("Strg+R", "Ctrl+R"),
            ReplaceColors => ("Strg+Umschalt+6", "Ctrl+Shift+6"),
            AutoScroll => ("Strg+Umschalt+L", "Ctrl+Shift+L"),
            ReopenClosed => ("Strg+Umschalt+W", "Ctrl+Shift+W"),
            _ => return None,
        })
    }
}

/// Text of a page for reading aloud: the page text and, if wanted, the
/// values of form fields and the texts of comments.
fn reading_text(tab: &mut crate::ui::tab::DocTab, page: usize, fields: bool, annots: bool) -> String {
    let mut text = tab.page_text(page).map(|t| t.all_text()).unwrap_or_default();
    if fields {
        for f in tab.fields(page).iter() {
            if !f.value.is_empty() {
                text.push_str(&format!("\n{}: {}", f.name, f.value));
            }
        }
    }
    if annots {
        for a in tab.annotations(page).iter() {
            if a.kind.is_comment() && !a.contents.trim().is_empty() {
                text.push('\n');
                text.push_str(&a.contents);
            }
        }
    }
    text
}

impl MrPdfApp {
    fn vc_de(&self) -> bool {
        self.settings.language == Language::German
    }

    pub(super) fn view_enabled(&self, cmd: ViewCmd) -> bool {
        use ViewCmd::*;
        let has_doc = !self.tabs.is_empty();
        match cmd {
            ReplaceColors | Design(_) | CustomizeUi | SnapshotSettings | ResetPanes | ResetDocPanes
            | NewTab | Rulers | Grid | Guides | Transparency | PageInfo | Snap | SnapGrid | SnapGuides
            | SnapSettings | HighlightLinks | PageGaps | RightToLeft | StandardLayout | SideBySide
            | ReadFields | ReadAnnots | ReadFollow => true,
            ReopenClosed => !self.closed_files.is_empty(),
            SortTabs | CloseOthers | MoveTabs => self.tabs.len() > 1,
            NewPortfolio => true,
            MoveTabMain => self.tabs.get(self.active).is_some_and(|t| t.window != 0),
            MergeGroups => {
                let w = self.tabs.get(self.active).map_or(0, |t| t.window);
                self.window_groups(w).len() > 1
            }
            MergeWindows => !self.extra_windows().is_empty(),
            ArrangeVertical | ArrangeHorizontal => self.window_groups(0).len() > 1,
            PortfolioList | PortfolioCover | PortfolioAddFiles | PortfolioAddFolder => {
                self.tabs.get(self.active).is_some_and(|t| t.portfolio.is_some_and(|(_, p)| p))
            }
            ReadStop | ReadBack | ReadForward | ReadShow => self.speech.is_some(),
            ReadSelection => self.active_tab().is_some_and(|t| t.selection.is_some()),
            _ => has_doc,
        }
    }

    pub(super) fn view_selected(&self, cmd: ViewCmd) -> bool {
        use ViewCmd::*;
        let s = &self.settings;
        let tab = self.active_tab();
        match cmd {
            LayoutTwoPagesContinuous => tab.is_some_and(|t| t.layout_mode == PageLayout::TwoPagesContinuous),
            PageGaps => s.page_gaps,
            RightToLeft => s.right_to_left,
            StandardLayout => !s.side_by_side,
            SideBySide => s.side_by_side,
            Rulers => s.show_rulers,
            Grid => s.show_grid,
            Guides => s.show_guides,
            Transparency => s.transparency_grid,
            PageInfo => s.show_page_info,
            Snap => s.snap,
            SnapGrid => s.snap_grid,
            SnapGuides => s.snap_guides,
            ReplaceColors => s.recolor,
            AutoScroll => self.auto_scroll.is_some(),
            HighlightLinks => s.highlight_links,
            LoupeWindow => s.show_loupe,
            PanZoom => s.show_pan_zoom,
            SplitHorizontal => tab.is_some_and(|t| t.split == Split::Horizontal),
            SplitVertical => tab.is_some_and(|t| t.split == Split::Vertical),
            SplitGrid => tab.is_some_and(|t| t.split == Split::Grid),
            Design(d) => s.design == d,
            ReadFields => s.read_fields,
            ReadAnnots => s.read_annots,
            ReadFollow => s.read_follow,
            ArrangeVertical => s.tab_groups_vertical,
            ArrangeHorizontal => !s.tab_groups_vertical,
            PortfolioList => tab.is_some_and(|t| !t.show_cover),
            PortfolioCover => tab.is_some_and(|t| t.show_cover),
            PortfolioMark => tab.is_some_and(|t| t.portfolio.is_some_and(|(_, p)| p)),
            _ => false,
        }
    }

    pub(super) fn run_view(&mut self, ctx: &Context, cmd: ViewCmd) {
        use ViewCmd::*;
        let s = &mut self.settings;
        match cmd {
            LayoutTwoPagesContinuous => self.set_layout(PageLayout::TwoPagesContinuous),
            PageGaps => s.page_gaps = !s.page_gaps,
            RightToLeft => s.right_to_left = !s.right_to_left,
            StandardLayout => s.side_by_side = false,
            SideBySide => s.side_by_side = true,
            RotateReset => {
                if let Some(tab) = self.tab() {
                    tab.view_rotation = crate::geometry::Rotation::R0;
                    tab.textures.clear();
                }
            }
            Rulers => s.show_rulers = !s.show_rulers,
            Grid => s.show_grid = !s.show_grid,
            Guides => s.show_guides = !s.show_guides,
            Transparency => s.transparency_grid = !s.transparency_grid,
            PageInfo => s.show_page_info = !s.show_page_info,
            Snap => s.snap = !s.snap,
            SnapGrid => s.snap_grid = !s.snap_grid,
            SnapGuides => s.snap_guides = !s.snap_guides,
            SnapSettings => self.open_settings_at(Cat::Measure),
            ReplaceColors => s.recolor = !s.recolor,
            AutoScroll => {
                self.auto_scroll = match self.auto_scroll {
                    Some(_) => None,
                    None => Some(ctx.input(|i| i.time)),
                }
            }
            HighlightLinks => s.highlight_links = !s.highlight_links,
            LoupeWindow => s.show_loupe = !s.show_loupe,
            PanZoom => s.show_pan_zoom = !s.show_pan_zoom,
            SplitHorizontal | SplitVertical | SplitGrid => {
                let split = match cmd {
                    SplitHorizontal => Split::Horizontal,
                    SplitVertical => Split::Vertical,
                    _ => Split::Grid,
                };
                if let Some(tab) = self.tab() {
                    tab.set_split(split);
                }
            }
            NewTab => self.backstage = Some(super::backstage::Page::Open),
            SortTabs => {
                let active = self.tabs.get(self.active).map(|t| t.id);
                self.tabs.sort_by_key(|t| t.doc.display_name().to_lowercase());
                if let Some(id) = active {
                    self.active = self.tabs.iter().position(|t| t.id == id).unwrap_or(0);
                }
            }
            CloseOthers => {
                let keep = self.tabs.get(self.active).map(|t| t.id);
                let others: Vec<u64> =
                    self.tabs.iter().map(|t| t.id).filter(|id| Some(*id) != keep).collect();
                // Changed documents ask first (like "Alle schließen").
                for id in others {
                    if let Some(i) = self.tabs.iter().position(|t| t.id == id) {
                        if self.tabs[i].doc.is_modified() {
                            self.request_close_tab(ctx, i);
                            return;
                        }
                        self.close_tab(ctx, id);
                    }
                }
                if let Some(id) = keep {
                    self.active = self.tabs.iter().position(|t| t.id == id).unwrap_or(0);
                }
            }
            ReopenClosed => {
                if let Some(path) = self.closed_files.pop() {
                    self.pending_open.push_back(path);
                }
            }
            ResetPanes => {
                s.show_sidebar = true;
                s.show_properties = true;
                s.show_loupe = false;
                s.show_pan_zoom = false;
                self.side = SidePanel::Thumbnails;
            }
            ResetDocPanes => {
                s.show_sidebar = true;
                self.side = SidePanel::Thumbnails;
            }
            Design(d) => s.design = d,
            CustomizeUi => self.open_settings_at(Cat::Interface),
            SnapshotSettings => self.open_settings_at(Cat::Snapshots),
            ReadStart | ReadToEnd => self.read_from(ctx, None, true),
            ReadPage => self.read_from(ctx, None, false),
            ReadSelection => {
                let text = self.tab().and_then(|t| t.selected_text()).unwrap_or_default();
                self.read_text(ctx, text, None, false);
            }
            ReadStop => self.speech = None,
            ReadBack | ReadForward => {
                if let Some((page, to_end)) = self.speech.as_ref().and_then(|r| r.page.map(|p| (p, r.to_end)))
                {
                    let count = self.active_tab().map_or(0, |t| t.doc.page_count());
                    let page = if cmd == ReadBack {
                        page.saturating_sub(1)
                    } else {
                        (page + 1).min(count.max(1) - 1)
                    };
                    self.read_from(ctx, Some(page), to_end);
                }
            }
            ReadShow => {
                if let Some(page) = self.speech.as_ref().and_then(|r| r.page)
                    && let Some(tab) = self.tab()
                {
                    tab.go_to_page(page);
                }
            }
            ReadFields => s.read_fields = !s.read_fields,
            ReadAnnots => s.read_annots = !s.read_annots,
            ReadFollow => s.read_follow = !s.read_follow,
            Transitions => self.open_transitions_dialog(),
            MoveTabNewVertical | MoveTabNewHorizontal | MoveTabNewWindow | MoveTabMain => {
                use super::windows::MoveTarget;
                let target = match cmd {
                    MoveTabNewVertical => MoveTarget::NewVerticalGroup,
                    MoveTabNewHorizontal => MoveTarget::NewHorizontalGroup,
                    MoveTabNewWindow => MoveTarget::NewWindow,
                    _ => MoveTarget::MainWindow,
                };
                if let Some(id) = self.tabs.get(self.active).map(|t| t.id) {
                    self.move_tabs(&[id], target);
                }
            }
            MoveTabs => self.open_move_tabs(),
            ArrangeVertical => s.tab_groups_vertical = true,
            ArrangeHorizontal => s.tab_groups_vertical = false,
            MergeGroups => self.merge_groups(),
            MergeWindows => self.merge_windows(),
            NewPortfolio => {
                self.backstage = None;
                self.open_new_portfolio();
            }
            PortfolioList | PortfolioCover => {
                if let Some(tab) = self.tab() {
                    tab.show_cover = cmd == PortfolioCover;
                }
            }
            PortfolioAddFiles | PortfolioAddFolder => {
                let index = self.active;
                self.portfolio_add(ctx, index, cmd == PortfolioAddFolder);
            }
            PortfolioMark => {
                let on = !self.tabs.get(self.active).is_some_and(|t| t.portfolio.is_some_and(|(_, p)| p));
                self.edit(ctx, |tab| tab.doc.set_portfolio(on));
                if let Some(tab) = self.tab() {
                    tab.show_cover = false;
                }
            }
        }
    }

    /// Reads `page` (default: the current page) aloud, then the following
    /// pages if `to_end`.
    fn read_from(&mut self, ctx: &Context, page: Option<usize>, to_end: bool) {
        let (fields, annots, follow) =
            (self.settings.read_fields, self.settings.read_annots, self.settings.read_follow);
        let Some(tab) = self.tab() else { return };
        let page = page.unwrap_or(tab.current_page).min(tab.doc.page_count().saturating_sub(1));
        let text = reading_text(tab, page, fields, annots);
        if follow {
            tab.go_to_page(page);
        }
        self.read_text(ctx, text, Some(page), to_end);
    }

    fn read_text(&mut self, ctx: &Context, text: String, page: Option<usize>, to_end: bool) {
        self.speech = None;
        // An empty page is skipped when reading to the end.
        let text = if text.trim().is_empty() { " ".to_string() } else { text };
        match crate::speech::Reader::start(&text, self.settings.read_volume, self.settings.read_rate) {
            Ok(mut reader) => {
                reader.page = page;
                reader.to_end = to_end;
                self.speech = Some(reader);
                ctx.request_repaint_after(std::time::Duration::from_millis(300));
            }
            Err(err) => {
                let de = self.vc_de();
                let msg = if de {
                    "Vorlesen ist nicht möglich: Auf diesem Computer wurde keine Sprachausgabe gefunden."
                } else {
                    "Read aloud is not possible: no speech output was found on this computer."
                };
                log::warn!("speech: {err}");
                self.info(if de { "Vorlesen" } else { "Read aloud" }, msg.to_string());
            }
        }
    }

    /// Called every frame: read aloud continues with the next page, auto
    /// scroll moves on, the loupe follows its tool.
    pub(super) fn view_tick(&mut self, ctx: &Context) {
        if self.tool == Tool::Loupe {
            self.settings.show_loupe = true;
        }
        // Read aloud.
        if let Some(reader) = &mut self.speech {
            if reader.is_running() {
                ctx.request_repaint_after(std::time::Duration::from_millis(300));
            } else {
                let next = reader.to_end.then_some(reader.page).flatten().map(|p| p + 1);
                self.speech = None;
                let count = self.active_tab().map_or(0, |t| t.doc.page_count());
                if let Some(page) = next.filter(|p| *p < count) {
                    self.read_from(ctx, Some(page), true);
                }
            }
        }
        // Auto scroll.
        let Some(last) = self.auto_scroll else { return };
        let now = ctx.input(|i| i.time);
        let speed = self.settings.auto_scroll_speed;
        let horizontal = self.settings.side_by_side;
        let Some(tab) = self.tab() else {
            self.auto_scroll = None;
            return;
        };
        if tab.layout_mode.paged() {
            // One page or spread every five seconds.
            if now - last >= 5.0 {
                let step = if tab.layout_mode.two_up() { 2 } else { 1 };
                let next = tab.current_page + step;
                if next < tab.doc.page_count() {
                    tab.go_to_page(next);
                    self.auto_scroll = Some(now);
                } else {
                    self.auto_scroll = None;
                }
            }
        } else {
            let dt = (now - last).clamp(0.0, 0.5) as f32;
            let mut offset = tab.scroll_offset;
            let max = (tab.layout.size - tab.viewport.size()).max(egui::Vec2::ZERO);
            if horizontal {
                offset.x = (offset.x + speed * dt).min(max.x);
            } else {
                offset.y = (offset.y + speed * dt).min(max.y);
            }
            let at_end = if horizontal { offset.x >= max.x } else { offset.y >= max.y };
            tab.pending_scroll = Some(ScrollRequest::Offset(offset));
            self.auto_scroll = if at_end && now - last > 0.0 { None } else { Some(now) };
        }
        ctx.request_repaint();
    }

    /// The floating windows "Lupe" and "Ausschnitt und Vergrößerung".
    pub(super) fn view_windows(&mut self, ctx: &Context) {
        let de = self.vc_de();
        if self.settings.show_loupe {
            let mut open = true;
            egui::Window::new(if de { "Lupe" } else { "Loupe" })
                .id(egui::Id::new("mrpdf-loupe"))
                .open(&mut open)
                .resizable(false)
                .default_pos(ctx.content_rect().left_top() + egui::vec2(260.0, 200.0))
                .show(ctx, |ui| self.loupe_ui(ui));
            if !open {
                self.settings.show_loupe = false;
                if self.tool == Tool::Loupe {
                    self.select_tool(Tool::Hand);
                }
            }
        }
        if self.settings.show_pan_zoom {
            let mut open = true;
            egui::Window::new(if de { "Ausschnitt und Vergrößerung" } else { "Pan and zoom" })
                .id(egui::Id::new("mrpdf-pan-zoom"))
                .open(&mut open)
                .resizable(false)
                .default_pos(ctx.content_rect().right_bottom() + egui::vec2(-300.0, -420.0))
                .show(ctx, |ui| self.pan_zoom_ui(ui));
            self.settings.show_pan_zoom = open;
        }
    }

    fn loupe_ui(&mut self, ui: &mut Ui) {
        let de = self.vc_de();
        const SIZE: [u32; 2] = [320, 220];
        let mut factor = self.settings.loupe_factor;
        ui.horizontal(|ui| {
            ui.label(if de { "Vergrößerung:" } else { "Magnification:" });
            ui.add(
                egui::Slider::new(&mut factor, 1.5..=16.0)
                    .logarithmic(true)
                    .custom_formatter(|v, _| format!("{:.0} %", v * 100.0)),
            );
        });
        self.settings.loupe_factor = factor;
        let ctx = ui.ctx().clone();
        let ppp = ctx.pixels_per_point();
        let target = self.active_tab().and_then(|t| t.loupe_at.map(|(p, v)| (t.id, p, v)));
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(SIZE[0] as f32, SIZE[1] as f32), egui::Sense::hover());
        ui.painter().rect_filled(rect, 0.0, egui::Color32::from_gray(128));
        let Some((_, page, at)) = target else {
            ui.label(if de {
                "Mit dem Werkzeug „Lupe“ über die Seite fahren."
            } else {
                "Move the \"Loupe\" tool over the page."
            });
            return;
        };
        let px = [(SIZE[0] as f32 * ppp) as u32, (SIZE[1] as f32 * ppp) as u32];
        let key = (page, (at.x * 4.0) as i32, (at.y * 4.0) as i32, (factor * 100.0) as u32);
        if self.loupe.as_ref().is_none_or(|(_, k)| *k != key) {
            let Some(tab) = self.active_tab() else { return };
            let Some(geo) = tab.display_geometry(page) else { return };
            let (w, h) = geo.view_size();
            let s = factor * view::PT_TO_SCREEN * ppp;
            let full = [(w * s).round().max(1.0) as u32, (h * s).round().max(1.0) as u32];
            let x = ((at.x * s) as i64 - px[0] as i64 / 2).clamp(0, (full[0] as i64 - px[0] as i64).max(0))
                as u32;
            let y = ((at.y * s) as i64 - px[1] as i64 / 2).clamp(0, (full[1] as i64 - px[1] as i64).max(0))
                as u32;
            let tw = px[0].min(full[0]);
            let th = px[1].min(full[1]);
            if let Ok(img) = tab.doc.render_tile(page, full[0], full[1], x, y, tw, th, tab.render_options()) {
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [img.width as usize, img.height as usize],
                    &img.rgba,
                );
                let tex = ctx.load_texture("mrpdf-loupe", image, egui::TextureOptions::LINEAR);
                self.loupe = Some((tex, key));
            }
        }
        if let Some((tex, _)) = &self.loupe {
            let size = tex.size_vec2() / ppp;
            let r = egui::Rect::from_min_size(rect.min, size.min(rect.size()));
            ui.painter().image(
                tex.id(),
                r,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
    }

    fn pan_zoom_ui(&mut self, ui: &mut Ui) {
        let de = self.vc_de();
        let ctx = ui.ctx().clone();
        let Some(tab) = self.tabs.get_mut(self.active) else {
            ui.label(if de { "Kein Dokument geöffnet." } else { "No document open." });
            return;
        };
        let page = tab.current_page;
        let Some(geo) = tab.display_geometry(page) else { return };
        let (w, h) = geo.view_size();
        let width = 220.0;
        let height = (width * h / w.max(1.0)).min(320.0);
        let size = egui::vec2(height * w / h.max(1.0), height);
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
        let ppp = ctx.pixels_per_point();
        let px = [(size.x * ppp) as u32, (size.y * ppp) as u32];
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, egui::Color32::WHITE);
        // An own picture: the thumbnail cache belongs to the sidebar.
        let key = (tab.id, page, tab.doc.revision(), px);
        if self.pan_zoom_tex.as_ref().is_none_or(|(_, k)| *k != key)
            && let Ok(img) = tab.doc.render_tile(page, px[0], px[1], 0, 0, px[0], px[1], tab.render_options())
        {
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [img.width as usize, img.height as usize],
                &img.rgba,
            );
            self.pan_zoom_tex =
                Some((ctx.load_texture("mrpdf-pan-zoom", image, egui::TextureOptions::LINEAR), key));
        }
        if let Some((tex, _)) = &self.pan_zoom_tex {
            painter.image(
                tex.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        // The visible part of the page as a red frame.
        if let Some(slot) = tab.layout.slot(page) {
            let page_rect = slot.rect.translate(tab.content_shift);
            let visible = page_rect.intersect(tab.viewport);
            if visible.is_positive() {
                let map = |p: egui::Pos2| {
                    rect.min + (p - page_rect.min) * (rect.width() / page_rect.width().max(1.0))
                };
                let frame = egui::Rect::from_min_max(map(visible.min), map(visible.max));
                painter.rect_stroke(
                    frame,
                    0.0,
                    egui::Stroke::new(2.0, egui::Color32::RED),
                    egui::StrokeKind::Inside,
                );
            }
        }
        // Click or drag: show that point in the middle of the view.
        if (response.clicked() || response.dragged())
            && let Some(p) = response.interact_pointer_pos()
        {
            let v = (p - rect.min) / rect.width() * w;
            tab.pending_scroll = Some(ScrollRequest::Point(page, PointF::new(v.x, v.y.min(h))));
        }
        ui.horizontal(|ui| {
            if ui.button("−").on_hover_text(if de { "Verkleinern" } else { "Zoom out" }).clicked() {
                tab.zoom = view::zoom_out(tab.zoom);
                tab.zoom_mode = ZoomMode::Custom;
            }
            let mut pct = tab.zoom * 100.0;
            if ui
                .add(egui::Slider::new(&mut pct, 5.0..=1600.0).logarithmic(true).suffix(" %").max_decimals(0))
                .changed()
            {
                tab.zoom = pct / 100.0;
                tab.zoom_mode = ZoomMode::Custom;
            }
            if ui.button("+").on_hover_text(if de { "Vergrößern" } else { "Zoom in" }).clicked() {
                tab.zoom = view::zoom_in(tab.zoom);
                tab.zoom_mode = ZoomMode::Custom;
            }
        });
    }

    /// Remembers a closed document for "Kürzlich geschlossene Dokumente".
    pub(super) fn remember_closed(&mut self, path: std::path::PathBuf) {
        self.closed_files.retain(|p| p != &path);
        self.closed_files.push(path);
        if self.closed_files.len() > 20 {
            self.closed_files.remove(0);
        }
    }
}
