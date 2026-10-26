//! The main window.

mod actions;
pub mod commands;
mod dialogs;
mod features;
mod formdata;
mod layout_dialogs;
mod outline_tools;
mod page_dialogs;
mod preview;
mod properties;
mod review;
mod ribbon;
mod sidebar;
mod statusbar;
mod structure;

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Duration;

use egui::{Context, Key, KeyboardShortcut, Modifiers, Ui, ViewportCommand};

use crate::geometry::RectF;
use crate::model::Metadata;
use crate::ui::cache::Budget;
use crate::ui::canvas::{self, CanvasCtx, CanvasEvent};
use crate::ui::i18n::Strings;
use crate::ui::settings::Settings;
use crate::ui::tab::{AnnotRef, DocTab};
use crate::ui::theme;
use crate::ui::tools::Tool;

const SETTINGS_KEY: &str = "mrpdf-settings";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RibbonTab {
    Home,
    View,
    Comment,
    Protect,
    Form,
    Organize,
    Convert,
    Share,
    Review,
    Accessibility,
    Bookmarks,
    Help,
    /// Contextual tab shown while a comment is selected.
    Format,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SidePanel {
    Thumbnails,
    Bookmarks,
    Comments,
    Attachments,
    Search,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfterSave {
    CloseTab,
    Exit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertPosition {
    Before,
    After,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PaperChoice {
    SameAsCurrent,
    A4,
    A3,
    A5,
    Letter,
    Legal,
}

impl PaperChoice {
    pub fn size(self) -> Option<(f32, f32)> {
        match self {
            PaperChoice::SameAsCurrent => None,
            PaperChoice::A4 => Some((595.28, 841.89)),
            PaperChoice::A3 => Some((841.89, 1190.55)),
            PaperChoice::A5 => Some((419.53, 595.28)),
            PaperChoice::Letter => Some((612.0, 792.0)),
            PaperChoice::Legal => Some((612.0, 1008.0)),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Dialog {
    Password {
        path: PathBuf,
        password: String,
        wrong: bool,
    },
    Message {
        title: String,
        text: String,
    },
    Unsaved {
        tab_id: u64,
        then: AfterSave,
    },
    TextInput {
        page: usize,
        tool: Tool,
        template: Option<Box<crate::pdf_edit::NewAnnotation>>,
        text: String,
        edit: Option<AnnotRef>,
    },
    Link {
        page: usize,
        rect: RectF,
        url: String,
        target_page: u32,
        to_page: bool,
    },
    ConfirmLink {
        uri: String,
    },
    Properties {
        meta: Metadata,
    },
    InsertBlank {
        count: u32,
        paper: PaperChoice,
        landscape: bool,
        position: InsertPosition,
    },
    PageRange {
        purpose: RangePurpose,
        text: String,
        error: bool,
    },
    ExportImage {
        range: String,
        dpi: u32,
        jpeg: bool,
        error: bool,
    },
    Settings {
        author: String,
    },
    Print {
        range: String,
        copies: u32,
        error: bool,
    },
    Choice {
        page: usize,
        annot_index: usize,
        options: Vec<String>,
        selected: String,
    },
    Protect {
        opts: crate::security::ProtectOptions,
        repeat: String,
        error: Option<&'static str>,
    },
    BookmarkTitle {
        parent: Option<Vec<usize>>,
        path: Option<Vec<usize>>,
        page: usize,
        title: String,
    },
    Split {
        every: u32,
    },
    Replace {
        range: String,
        source_start: u32,
        error: bool,
    },
    Swap {
        a: u32,
        b: u32,
    },
    Resize {
        range: String,
        paper: PaperChoice,
        landscape: bool,
        fit: bool,
        error: bool,
    },
    Margins {
        range: String,
        mm: [f32; 4],
        crop: bool,
        error: bool,
    },
    PageMark(Box<structure::MarkForm>),
    PageLabels {
        start: u32,
        style: crate::pages::LabelStyle,
        prefix: String,
        first: u32,
    },
    Duplicates {
        groups: Vec<Vec<usize>>,
    },
    Confirm {
        title: String,
        text: String,
        cmd: commands::Cmd,
    },
    NewField(Box<formdata::FieldForm>),
    Pages(Box<page_dialogs::PageDialogState>),
    Layout(Box<layout_dialogs::LayoutState>),
    SearchRedact {
        query: String,
        match_case: bool,
        whole_word: bool,
    },
    BookmarkEveryNth {
        every: u32,
        prefix: String,
    },
    BookmarkReplace {
        find: String,
        replace: String,
        match_case: bool,
    },
    BookmarkSort {
        by_page: bool,
    },
    About,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangePurpose {
    Extract,
    Delete,
    Rotate(i32),
    Flatten,
    Duplicate,
    ToImages,
}

pub struct MrPdfApp {
    settings: Settings,
    tabs: Vec<DocTab>,
    active: usize,
    ribbon: RibbonTab,
    tool: Tool,
    side: SidePanel,
    dialog: Option<Dialog>,
    status: Option<(String, f64)>,
    pending_open: VecDeque<PathBuf>,
    exit_confirmed: bool,
    search_query: String,
    search_case: bool,
    search_word: bool,
    focus_search: bool,
    goto_text: String,
    events: Vec<CanvasEvent>,
    last_title: String,
    applied_dark: Option<bool>,
    props_contents: Option<(AnnotRef, u64, String)>,
    props_style: Option<(AnnotRef, u64, crate::model::AnnotStyle)>,
    quick_query: String,
    quick_focus: bool,
    /// Last radio group and number of buttons placed in it.
    last_radio: (String, u32),
    /// Page preview shown in the current dialog.
    preview: [Option<preview::PreviewCache>; preview::SLOTS],
    /// Extra document shown in a dialog (source file, generated pages).
    dialog_doc: Option<(String, crate::Document)>,
    /// Clipboard content read for "Seiten aus der Zwischenablage einfügen".
    clipboard: (Option<String>, Option<crate::pdf_edit::RgbaImage>),
    /// Source document of the overlay dialog.
    overlay_source: Option<(PathBuf, crate::import::Source)>,
}

impl MrPdfApp {
    pub fn new(cc: &eframe::CreationContext<'_>, files: Vec<PathBuf>) -> Self {
        let settings: Settings =
            cc.storage.and_then(|s| eframe::get_value(s, SETTINGS_KEY)).unwrap_or_default();
        Self::with_settings(&cc.egui_ctx, settings, files)
    }

    /// Creates the application without persistent storage (used by tests).
    pub fn with_settings(ctx: &Context, settings: Settings, files: Vec<PathBuf>) -> Self {
        ctx.options_mut(|o| o.zoom_with_keyboard = false);
        Self {
            side: SidePanel::Thumbnails,
            settings,
            tabs: Vec::new(),
            active: 0,
            ribbon: RibbonTab::Home,
            tool: Tool::Hand,
            dialog: None,
            status: None,
            pending_open: files.into(),
            exit_confirmed: false,
            search_query: String::new(),
            search_case: false,
            search_word: false,
            focus_search: false,
            goto_text: String::new(),
            events: Vec::new(),
            last_title: String::new(),
            applied_dark: None,
            props_contents: None,
            props_style: None,
            quick_query: String::new(),
            quick_focus: false,
            last_radio: ("RadioGroup1".into(), 0),
            preview: Default::default(),
            dialog_doc: None,
            clipboard: (None, None),
            overlay_source: None,
        }
    }

    pub fn t(&self) -> &'static Strings {
        self.settings.strings()
    }

    fn tab(&mut self) -> Option<&mut DocTab> {
        self.tabs.get_mut(self.active)
    }

    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    pub fn active_tab(&self) -> Option<&DocTab> {
        self.tabs.get(self.active)
    }

    pub fn active_tab_mut(&mut self) -> Option<&mut DocTab> {
        self.tabs.get_mut(self.active)
    }

    /// Switches to tab index (ignored if out of range).
    pub fn set_active_tab(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active = index;
        }
    }

    pub fn tool(&self) -> Tool {
        self.tool
    }

    pub fn dialog(&self) -> Option<&Dialog> {
        self.dialog.as_ref()
    }

    pub fn set_status(&mut self, ctx: &Context, text: impl Into<String>) {
        self.status = Some((text.into(), ctx.input(|i| i.time)));
    }

    fn select_tool(&mut self, tool: Tool) {
        self.tool = tool;
        if let Some(tab) = self.tab() {
            tab.interaction = crate::ui::tab::Interaction::None;
            tab.polygon.clear();
        }
    }

    fn shortcuts(&mut self, ctx: &Context) {
        if self.dialog.is_some() {
            return;
        }
        let cmd = Modifiers::COMMAND;
        let pressed =
            |m: Modifiers, k: Key| ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(m, k)));
        if pressed(cmd, Key::O) {
            self.open_dialog();
        }
        if pressed(cmd, Key::N) {
            self.new_document(ctx);
        }
        if pressed(cmd | Modifiers::SHIFT, Key::S) {
            self.save_as(ctx, None);
        } else if pressed(cmd, Key::S) {
            self.save(ctx, None);
        }
        if pressed(cmd, Key::W) {
            self.request_close_tab(ctx, self.active);
        }
        if pressed(cmd, Key::P) {
            self.open_print_dialog(ctx);
        }

        if pressed(cmd, Key::F) {
            self.settings.show_sidebar = true;
            self.side = SidePanel::Search;
            self.focus_search = true;
        }
        // Shift variants first: Ctrl+D would also match Ctrl+Shift+D.
        if pressed(cmd | Modifiers::SHIFT, Key::I) {
            self.run(ctx, commands::Cmd::InsertFromFile);
        }
        if pressed(cmd | Modifiers::SHIFT, Key::D) {
            self.run(ctx, commands::Cmd::DeletePages);
        } else if pressed(cmd, Key::D) {
            self.open_properties();
        }
        if pressed(cmd | Modifiers::SHIFT, Key::T) {
            self.run(ctx, commands::Cmd::CropPages);
        }
        if pressed(cmd, Key::Plus) || pressed(cmd, Key::Equals) {
            self.zoom_step(true);
        }
        if pressed(cmd, Key::Minus) {
            self.zoom_step(false);
        }
        if pressed(cmd, Key::Num0) {
            self.set_zoom_mode(crate::ui::view::ZoomMode::FitPage);
        }
        if pressed(cmd, Key::Num1) {
            self.set_zoom(1.0);
        }
        if pressed(cmd, Key::Num2) {
            self.set_zoom_mode(crate::ui::view::ZoomMode::FitWidth);
        }
        if pressed(cmd, Key::Num3) {
            self.run(ctx, commands::Cmd::FitContent);
        }
        if pressed(cmd, Key::Period) {
            self.quick_focus = true;
        }
        if pressed(Modifiers::ALT, Key::ArrowLeft) {
            self.run(ctx, commands::Cmd::ViewBack);
        }
        if pressed(Modifiers::ALT, Key::ArrowRight) {
            self.run(ctx, commands::Cmd::ViewForward);
        }
        if pressed(cmd, Key::Tab) && !self.tabs.is_empty() {
            self.active = (self.active + 1) % self.tabs.len();
        }
        // The remaining shortcuts belong to text fields while one has focus.
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        if pressed(cmd, Key::Z) {
            self.undo(ctx);
        }
        if pressed(cmd, Key::Y) || pressed(cmd | Modifiers::SHIFT, Key::Z) {
            self.redo(ctx);
        }
        if pressed(cmd, Key::C) {
            self.copy_selection(ctx);
        }
        if pressed(cmd, Key::A) {
            self.select_all_text();
        }
        let key = |k: Key| ctx.input(|i| i.key_pressed(k) && i.modifiers.is_none());
        if key(Key::Delete) || key(Key::Backspace) {
            self.delete_selected_annotation(ctx);
        }
        if key(Key::Escape) {
            if let Some(tab) = self.tab() {
                tab.selected_annot = None;
                tab.selection = None;
            }
            self.select_tool(Tool::Hand);
        }
        let page_count = self.active_tab().map_or(0, |t| t.doc.page_count());
        if let Some(tab) = self.tab() {
            let cur = tab.current_page;
            if key(Key::PageDown) && tab.layout_mode == crate::ui::view::PageLayout::Single {
                tab.go_to_page((cur + 1).min(page_count.saturating_sub(1)));
            } else if key(Key::PageDown) {
                tab.go_to_page(cur + 1);
            }
            if key(Key::PageUp) {
                tab.go_to_page(cur.saturating_sub(1));
            }
            if key(Key::Home) {
                tab.go_to_page(0);
            }
            if key(Key::End) {
                tab.go_to_page(page_count.saturating_sub(1));
            }
        }
        if key(Key::H) {
            self.select_tool(Tool::Hand);
        }
        if key(Key::V) {
            self.select_tool(Tool::SelectText);
        }
        if ctx.input(|i| i.key_pressed(Key::F3)) {
            let back = ctx.input(|i| i.modifiers.shift);
            if let Some(tab) = self.tab() {
                tab.next_hit(!back);
            }
        }
        if ctx.input(|i| i.key_pressed(Key::F11)) {
            let full = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            ctx.send_viewport_cmd(ViewportCommand::Fullscreen(!full));
        }
        if ctx.input(|i| i.key_pressed(Key::F4)) {
            self.settings.show_sidebar = !self.settings.show_sidebar;
        }
    }

    fn handle_dropped_files(&mut self, ctx: &Context) {
        let dropped: Vec<PathBuf> =
            ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        for path in dropped {
            if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")) {
                self.pending_open.push_back(path);
            } else {
                let t = self.t();
                self.set_status(ctx, format!("{}: {}", t.not_a_pdf, path.display()));
            }
        }
    }

    fn update_title(&mut self, ctx: &Context) {
        let title = match self.active_tab() {
            Some(tab) => format!("{} - MrPDF", tab.title()),
            None => "MrPDF".to_string(),
        };
        if title != self.last_title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }
    }

    fn handle_close_request(&mut self, ctx: &Context) {
        if !ctx.input(|i| i.viewport().close_requested()) || self.exit_confirmed {
            return;
        }
        if let Some(tab) = self.tabs.iter().find(|t| t.doc.is_modified()) {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            let tab_id = tab.id;
            if let Some(i) = self.tabs.iter().position(|t| t.id == tab_id) {
                self.active = i;
            }
            self.dialog = Some(Dialog::Unsaved { tab_id, then: AfterSave::Exit });
        }
    }

    fn central(&mut self, ui: &mut Ui) {
        let t = self.t();
        let ctx = ui.ctx().clone();
        let mut budget = Budget::new(Duration::from_millis(40));
        let tool = self.tool;
        let style = self.settings.style(tool);
        let stamp = self.settings.stamp;
        let highlight_fields = self.settings.highlight_fields;
        if let Some(tab) = self.tabs.get_mut(self.active) {
            let mut cx = CanvasCtx {
                tool,
                style,
                stamp_label: t.stamp_label(stamp).to_string(),
                stamp_icon: stamp.pdf_name(),
                highlight_fields,
                decimal_comma: self.settings.language == crate::ui::i18n::Language::German,
                budget: &mut budget,
                events: &mut self.events,
                t,
            };
            canvas::show(ui, tab, &mut cx);
            if tab.search.is_running() && tab.continue_search(Duration::from_millis(15)) {
                ctx.request_repaint();
            }
        } else {
            self.start_page(ui);
        }
        if budget.pending {
            ctx.request_repaint();
        }
    }

    fn start_page(&mut self, ui: &mut Ui) {
        let t = self.t();
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.2);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(72.0, 72.0), egui::Sense::hover());
            crate::ui::icons::paint(ui.painter(), rect, crate::ui::icons::Icon::Logo, theme::BRAND);
            ui.heading("MrPDF");
            ui.label(t.welcome);
            ui.add_space(12.0);
            if ui.button(t.open_file).clicked() {
                self.open_dialog();
            }
            if ui.button(t.new_document).clicked() {
                let ctx = ui.ctx().clone();
                self.new_document(&ctx);
            }
            if !self.settings.recent_files.is_empty() {
                ui.add_space(16.0);
                ui.strong(t.recent_files);
                for path in self.settings.recent_files.clone() {
                    let label =
                        path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    if ui.link(label).on_hover_text(path.display().to_string()).clicked() {
                        self.pending_open.push_back(path);
                    }
                }
            }
        });
    }

    fn process_events(&mut self, ctx: &Context) {
        let events = std::mem::take(&mut self.events);
        for event in events {
            self.handle_canvas_event(ctx, event);
        }
    }
}

impl eframe::App for MrPdfApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.applied_dark != Some(self.settings.dark_mode) {
            theme::apply(&ctx, self.settings.dark_mode);
            self.applied_dark = Some(self.settings.dark_mode);
        }
        if let Some(path) = self.pending_open.pop_front() {
            self.open_path(&ctx, path, None);
        }
        self.handle_dropped_files(&ctx);
        self.shortcuts(&ctx);
        self.handle_close_request(&ctx);

        self.ribbon_ui(ui);
        self.status_bar(ui);
        self.tab_bar(ui);
        if !self.tabs.is_empty() {
            if self.settings.show_sidebar {
                self.sidebar(ui);
            }
            if self.settings.show_properties {
                self.properties_panel(ui);
            }
        }
        egui::CentralPanel::no_frame().show(ui, |ui| self.central(ui));
        self.dialogs(&ctx);
        self.process_events(&ctx);
        self.update_title(&ctx);
        if !self.pending_open.is_empty() {
            ctx.request_repaint();
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, SETTINGS_KEY, &self.settings);
    }
}
