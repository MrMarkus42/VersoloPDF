//! The main window.

mod actions;
mod backstage;
mod bookmark_big_dialogs;
pub mod bookmark_dialogs;
mod calculator_ui;
pub mod commands;
pub mod comment_cmds;
pub mod compare_view;
mod content_cmds;
mod convert_dialogs;
mod dialogs;
mod export_dialog;
mod features;
mod file_ops;
pub mod form_cmds;
mod formdata;
mod insert_source;
pub mod keytips;
mod labels_dialog;
mod layout_dialogs;
mod marks_dialogs;
mod office_dialogs;
mod optimize_dialog;
mod outline_tools;
mod page_dialogs;
mod portfolio_view;
pub mod presentation;
mod preview;
mod print_dialog;
mod properties;
mod props_dialog;
mod review;
pub mod review_cmds;
mod ribbon;
pub mod settings_dialog;
mod shape_dialogs;
mod sidebar;
mod sign_dialog;
mod statusbar;
mod structure;
mod style_flyout;
pub mod view_cmds;
pub mod windows;

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Duration;

use egui::{Context, Key, KeyboardShortcut, Modifiers, Ui, ViewportCommand};

use crate::geometry::RectF;
use crate::ui::cache::Budget;
use crate::ui::canvas::{self, CanvasCtx, CanvasEvent};
use crate::ui::i18n::Strings;
use crate::ui::settings::Settings;
use crate::ui::tab::{AnnotRef, DocTab};
use crate::ui::theme;
use crate::ui::tools::Tool;

const SETTINGS_KEY: &str = "mrpdf-settings";

/// What a rendered picture shows: tab, page, document revision, size.
type PictureKey = (u64, usize, u64, [u32; 2]);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    /// Contextual tab "Anordnen" of the content tools.
    Arrange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SidePanel {
    Thumbnails,
    Bookmarks,
    Comments,
    Attachments,
    /// "Formular": all form fields of the document.
    Fields,
    Search,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfterSave {
    CloseTab,
    CloseAll,
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
        /// "Diese Webseite immer erlauben" (adds it to the trust list).
        trust: bool,
    },
    /// "Dokumenteigenschaften".
    Properties(Box<props_dialog::PropsState>),
    PageRange {
        purpose: RangePurpose,
        text: String,
        error: bool,
    },
    /// "Als Bilddateien exportieren".
    ExportImages(Box<export_dialog::ExportImagesState>),
    /// "Einstellungen für Word-Umwandlung".
    /// "Einstellungen für Word-Umwandlung".
    ExportWord(Box<office_dialogs::WordState>),
    /// "Text auf Seiten erkennen".
    RecognizeText(Box<convert_dialogs::OcrState>),
    /// "Schriften ersetzen".
    ReplaceFonts(Box<convert_dialogs::FontsState>),
    /// "Seiten neu einfärben".
    Recolor(Box<convert_dialogs::RecolorState>),
    /// "Einstellungen für Excel-Umwandlung".
    ExportExcel(Box<office_dialogs::ExcelState>),
    /// "Einstellungen für PowerPoint-Umwandlung".
    ExportPowerPoint(Box<office_dialogs::PowerPointState>),
    /// "Einstellungen" (Strg+K).
    Settings(Box<settings_dialog::SettingsPage>),
    /// "Drucken" / "Alle drucken".
    Print(Box<print_dialog::PrintState>),
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
    Swap {
        a: u32,
        b: u32,
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
    Shape(Box<shape_dialogs::ShapeState>),
    Marks(Box<marks_dialogs::MarksState>),
    Labels(Box<labels_dialog::LabelsState>),
    /// "Dokumente vergleichen".
    Compare(Box<compare_view::CompareSetup>),
    SearchRedact {
        query: String,
        match_case: bool,
        whole_word: bool,
    },
    /// The dialogs of the ribbon tab "Lesezeichen".
    Bookmark(Box<bookmark_dialogs::BookmarkDialog>),
    About,
    /// "Von URL öffnen".
    OpenUrl {
        url: String,
    },
    /// "Umbenennen".
    RenameFile {
        name: String,
        error: Option<String>,
    },
    /// "PDF optimieren".
    Optimize(Box<optimize_dialog::OptimizeState>),
    /// "Speicherverbrauch untersuchen".
    SpaceUsage(Box<crate::optimize::Usage>),
    /// "Seitenübergänge".
    Transitions(Box<presentation::TransState>),
    /// "Mehrere Tabs verschieben".
    MoveTabs(Box<windows::MoveTabsState>),
    /// "PDF-Portfolio erstellen".
    NewPortfolio(Box<portfolio_view::NewPortfolioState>),
    /// "Formate für Anmerkungen": the tool whose style is edited.
    CommentStyles(Tool),
    /// "PDF-Formulare ausfüllen".
    FillForms(Box<form_cmds::FillState>),
    /// "Alle JavaScripte".
    Scripts(Box<form_cmds::ScriptList>),
    /// "Rechtschreibprüfung".
    SpellCheck(Box<review_cmds::SpellState>),
    /// "Dokument signieren".
    Sign(Box<sign_dialog::SignState>),
    /// "Signaturen überprüfen".
    Signatures(Vec<crate::sign::pdf::SigInfo>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangePurpose {
    Extract,
    Flatten,
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
    /// Files handed over by later starts of MrPDF (see crate::instance).
    inbox: Option<crate::instance::Inbox>,
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
    /// Background dialog: the previewed page covers its background, and
    /// "Über den Inhalt legen" was switched on for that once.
    mark_covered: (bool, bool),
    /// Clipboard content read for "Seiten aus der Zwischenablage einfügen".
    clipboard: (Option<String>, Option<crate::pdf_edit::RgbaImage>),
    /// Source document of the overlay dialog.
    overlay_source: Option<(PathBuf, crate::import::Source)>,
    /// Open page of the "Datei" backstage (None: documents are shown).
    backstage: Option<backstage::Page>,
    /// Search text of the recent files list in the backstage.
    recent_filter: String,
    /// Collapsed ribbon: the commands of the chosen tab are shown over the
    /// document for now.
    ribbon_peek: bool,
    /// Height of the ribbon panel in the last frame (for tests).
    ribbon_height: f32,
    /// Shown sub-page of "Öffnen".
    open_view: backstage::OpenView,
    /// Name field of "Aktuelle Sitzung speichern".
    session_name: String,
    /// First-page pictures of recent files (None: could not be rendered).
    recent_thumbs: std::collections::HashMap<PathBuf, Option<egui::TextureHandle>>,
    /// Running "Von URL öffnen" download: address and result channel.
    download: Option<(String, std::sync::mpsc::Receiver<crate::Result<PathBuf>>)>,
    /// Folder of recovery copies (None: switched off).
    recovery_dir: Option<PathBuf>,
    /// Recovery copies of an earlier session (None: not looked for yet).
    recovery_offer: Option<Vec<(String, crate::files::RecoveryEntry)>>,
    /// Time of the last recovery copy.
    last_recovery: f64,
    /// Options being edited in "Einstellungen" (applied with OK / Übernehmen).
    settings_draft: Option<Box<Settings>>,
    /// "Größe der Benutzeroberfläche" last applied.
    applied_scale: Option<f32>,
    /// Design last applied (see `applied_dark`).
    applied_design: Option<theme::Design>,
    applied_modern: Option<bool>,
    /// Documents closed in this session ("Kürzlich geschlossene Dokumente").
    closed_files: Vec<PathBuf>,
    /// "Automatisch blättern": time of the last step (None: off).
    auto_scroll: Option<f64>,
    /// "Lupe": rendered picture and what it shows (page, point, factor).
    loupe: Option<(egui::TextureHandle, (usize, i32, i32, u32))>,
    /// "Ausschnitt und Vergrößerung": page picture and what it shows.
    pan_zoom_tex: Option<(egui::TextureHandle, PictureKey)>,
    /// Portfolio file lists per tab id: (revision, files).
    portfolio_cache: std::collections::HashMap<u64, (u64, Vec<crate::portfolio::Entry>)>,
    /// Tab last shown in each (window, group).
    group_focus: std::collections::HashMap<(u32, u32), u64>,
    /// Keyboard operation of the ribbon (Alt / F10 and letters).
    keytips: keytips::KeyTips,
    /// "Dokumente vergleichen": the comparison shown side by side.
    compare: Option<Box<compare_view::Comparison>>,
    /// Files chosen in the comparison dialog (not opened as tabs yet).
    compare_files: Vec<(std::path::PathBuf, crate::Document)>,
    /// "Vollbild": the running presentation.
    presentation: Option<presentation::Presentation>,
    /// "Vorlesen": the running speech output.
    speech: Option<crate::speech::Reader>,
    /// Position of the dialog window after the user moved it (per kind of dialog).
    dialog_pos: Option<(std::mem::Discriminant<Dialog>, egui::Pos2)>,
    /// Measured widths of the ribbon per (tab, language, level, content key).
    ribbon_widths: std::collections::HashMap<(RibbonTab, crate::ui::i18n::Language, usize, u8), f32>,
    /// Number of groups of each ribbon tab (last drawn).
    ribbon_groups: std::collections::HashMap<RibbonTab, usize>,
    /// Open "Rechner" windows and the number of the next one.
    calculators: Vec<calculator_ui::CalcWindow>,
    next_calculator: usize,
    /// Loaded spelling dictionary and its file.
    spell_dict: Option<(PathBuf, crate::spell::Dictionary)>,
    /// Copied page objects ("Objekte bearbeiten") and their bounding box.
    object_clip: Option<crate::document::ObjectClip>,
    clip_rect: Option<crate::geometry::RectF>,
    /// Pastes since the last copy (each one a bit further away).
    paste_count: u32,
    /// "Ausrichtung": align to the page instead of the selection.
    align_to_page: bool,
    /// Values of the content "Format" tab.
    content_format: Option<content_cmds::FormatUi>,
}

impl MrPdfApp {
    pub fn new(cc: &eframe::CreationContext<'_>, files: Vec<PathBuf>) -> Self {
        let mut settings: Settings =
            cc.storage.and_then(|s| eframe::get_value(s, SETTINGS_KEY)).unwrap_or_default();
        settings.plain_paths();
        Self::with_settings(&cc.egui_ctx, settings, files)
    }

    /// Receives the files of later starts of MrPDF (one program window).
    pub fn set_inbox(&mut self, ctx: &Context, inbox: crate::instance::Inbox) {
        inbox.set_context(ctx);
        self.inbox = Some(inbox);
    }

    /// Opens files handed over by a later start and brings the window to
    /// the front.
    fn check_inbox(&mut self, ctx: &Context) {
        let Some(files) = self.inbox.as_ref().and_then(|i| i.take()) else { return };
        self.pending_open.extend(files);
        ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(ViewportCommand::Focus);
    }

    /// Creates the application without persistent storage (used by tests).
    pub fn with_settings(ctx: &Context, settings: Settings, files: Vec<PathBuf>) -> Self {
        ctx.options_mut(|o| o.zoom_with_keyboard = false);
        // "Letzte Sitzung beim Start wiederherstellen".
        let files: Vec<PathBuf> = if files.is_empty() && settings.restore_session {
            settings.last_session.files.iter().filter(|p| p.exists()).cloned().collect()
        } else {
            files
        };
        // "Das Dateimenü beim Start anzeigen" (only when nothing is opened).
        let backstage = (settings.backstage_at_start && files.is_empty()).then_some(backstage::Page::Open);
        let tool = if settings.remember_tool { settings.last_tool } else { settings.default_tool };
        Self {
            side: SidePanel::Thumbnails,
            settings,
            tabs: Vec::new(),
            active: 0,
            ribbon: RibbonTab::Home,
            tool,
            dialog: None,
            status: None,
            pending_open: files.into(),
            inbox: None,
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
            mark_covered: (false, false),
            clipboard: (None, None),
            overlay_source: None,
            backstage,
            recent_filter: String::new(),
            ribbon_peek: false,
            ribbon_height: 0.0,
            open_view: backstage::OpenView::Recent,
            session_name: String::new(),
            recent_thumbs: Default::default(),
            download: None,
            recovery_dir: crate::files::default_recovery_dir(),
            recovery_offer: None,
            last_recovery: 0.0,
            settings_draft: None,
            applied_scale: None,
            applied_design: None,
            applied_modern: None,
            closed_files: Vec::new(),
            auto_scroll: None,
            loupe: None,
            pan_zoom_tex: None,
            presentation: None,
            group_focus: Default::default(),
            keytips: Default::default(),
            compare: None,
            compare_files: Vec::new(),
            portfolio_cache: Default::default(),
            speech: None,
            dialog_pos: None,
            ribbon_widths: Default::default(),
            ribbon_groups: Default::default(),
            calculators: Vec::new(),
            next_calculator: 1,
            spell_dict: None,
            object_clip: None,
            clip_rect: None,
            paste_count: 0,
            align_to_page: false,
            content_format: None,
        }
    }

    /// Applies options that act on the window or the open tabs (theme,
    /// size of the user interface, replaced document colours).
    fn apply_view_settings(&mut self, ctx: &Context) {
        let design = self.settings.design;
        let dark = design.is_dark()
            || if self.settings.follow_system_theme {
                ctx.system_theme().map_or(self.settings.dark_mode, |t| t == egui::Theme::Dark)
            } else {
                self.settings.dark_mode
            };
        let modern = self.settings.modern_look;
        if self.applied_modern != Some(modern) {
            theme::apply_fonts(ctx, modern);
        }
        if self.applied_dark != Some(dark)
            || self.applied_design != Some(design)
            || self.applied_modern != Some(modern)
        {
            theme::apply(ctx, dark, design, modern);
            self.applied_dark = Some(dark);
            self.applied_design = Some(design);
            self.applied_modern = Some(modern);
        }
        let scale = self.settings.ui_scale.clamp(0.5, 2.0);
        if self.applied_scale != Some(scale) {
            // Only touch the zoom when the option was changed, so the
            // user's own zoom of the window is kept otherwise.
            if self.applied_scale.is_some() || scale != 1.0 {
                ctx.set_zoom_factor(scale);
            }
            self.applied_scale = Some(scale);
        }
        let recolor =
            self.settings.recolor.then_some((self.settings.recolor_background, self.settings.recolor_text));
        let transparent = self.settings.transparency_grid;
        for tab in &mut self.tabs {
            if tab.recolor != recolor || tab.transparent != transparent {
                tab.recolor = recolor;
                tab.transparent = transparent;
                tab.textures.clear();
            }
        }
    }

    pub fn t(&self) -> &'static Strings {
        self.settings.strings()
    }

    fn tab(&mut self) -> Option<&mut DocTab> {
        self.tabs.get_mut(self.active)
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Height of the ribbon (quick access row, tabs and commands).
    pub fn ribbon_height(&self) -> f32 {
        self.ribbon_height
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

    /// Page shown by the running presentation ("Vollbild").
    pub fn presentation_page(&self) -> Option<usize> {
        self.presentation.as_ref().map(|p| p.page)
    }

    /// Tab by index (all windows).
    pub fn tab_at(&self, index: usize) -> Option<&DocTab> {
        self.tabs.get(index)
    }

    pub fn dialog(&self) -> Option<&Dialog> {
        self.dialog.as_ref()
    }

    /// The message shown in the status bar.
    pub fn status_text(&self) -> Option<&str> {
        self.status.as_ref().map(|(s, _)| s.as_str())
    }

    pub fn set_status(&mut self, ctx: &Context, text: impl Into<String>) {
        self.status = Some((text.into(), ctx.input(|i| i.time)));
    }

    fn select_tool(&mut self, tool: Tool) {
        // Leaving "Text bearbeiten" applies the open text.
        if tool != Tool::EditText
            && let Some(tab) = self.tab()
            && let Some(edit) = tab.content.text.take()
            && edit.changed()
        {
            self.events.push(CanvasEvent::Content(edit.to_op()));
        }
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
        // "Objekte bearbeiten" uses Strg+C/X/V/D/A, Entf and the arrows.
        self.content_shortcuts(ctx);
        let cmd = Modifiers::COMMAND;
        let pressed =
            |m: Modifiers, k: Key| ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(m, k)));
        if self.backstage_open() && pressed(Modifiers::NONE, Key::Escape) {
            self.close_backstage();
        }
        // "Vollbild: Beenden durch Drücken der ESC-Taste".
        if self.settings.esc_exits_fullscreen
            && ctx.input(|i| i.viewport().fullscreen.unwrap_or(false))
            && pressed(Modifiers::NONE, Key::Escape)
        {
            ctx.send_viewport_cmd(ViewportCommand::Fullscreen(false));
        }
        if pressed(cmd, Key::K) {
            self.run(ctx, commands::Cmd::Settings);
        }
        // Ribbon tab "Ansicht".
        {
            use commands::Cmd;
            use view_cmds::ViewCmd as V;
            let shift = cmd | Modifiers::SHIFT;
            let list: [(Modifiers, Key, Cmd); 13] = [
                (cmd, Key::R, Cmd::View(V::Rulers)),
                (shift, Key::G, Cmd::View(V::PageGaps)),
                (shift, Key::Num0, Cmd::View(V::RotateReset)),
                (shift, Key::Num6, Cmd::View(V::ReplaceColors)),
                (shift, Key::Num3, Cmd::DarkMode),
                (shift, Key::L, Cmd::View(V::AutoScroll)),
                (shift, Key::W, Cmd::View(V::ReopenClosed)),
                (cmd | Modifiers::ALT, Key::L, Cmd::Tool(Tool::Loupe)),
                (cmd, Key::B, Cmd::Panel(SidePanel::Bookmarks)),
                (cmd, Key::T, Cmd::Panel(SidePanel::Thumbnails)),
                (cmd, Key::M, Cmd::Panel(SidePanel::Comments)),
                (shift, Key::A, Cmd::Panel(SidePanel::Attachments)),
                (shift, Key::F, Cmd::Search),
            ];
            // Extra modifiers are ignored by egui: Ctrl+T must not take Ctrl+Shift+T.
            let (shift_held, alt_held) = ctx.input(|i| (i.modifiers.shift, i.modifiers.alt));
            for (m, k, c) in list {
                if (m.shift || !shift_held) && (m.alt || !alt_held) && pressed(m, k) {
                    self.run(ctx, c);
                }
            }
        }
        // Esc ends "Automatisch blättern".
        if self.auto_scroll.is_some() && pressed(Modifiers::NONE, Key::Escape) {
            self.auto_scroll = None;
        }
        if pressed(cmd, Key::Q) {
            self.run(ctx, commands::Cmd::Exit);
        }
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
        } else if pressed(cmd, Key::I) {
            self.run(ctx, commands::Cmd::Form(form_cmds::FormCmd::FormPane));
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
            self.fit_content();
        }
        // "Alle Anmerkungen verbergen / anzeigen".
        if pressed(cmd | Modifiers::SHIFT, Key::Num8) {
            self.run(ctx, commands::Cmd::Comment(comment_cmds::CommentCmd::HideAll));
        } else if pressed(cmd, Key::Num8) {
            self.run(ctx, commands::Cmd::Comment(comment_cmds::CommentCmd::ShowAll));
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
        if self.tool == Tool::TextCorrection && self.active_tab().is_some_and(|t| t.selection.is_some()) {
            // "Textkorrektur": the keys act on the selected text.
            let typed = ctx.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Text(t) if !t.trim().is_empty() => Some(t.clone()),
                    _ => None,
                })
            });
            if key(Key::Delete) || key(Key::Backspace) {
                self.correct_text(comment_cmds::Correction::Delete);
            } else if key(Key::Insert) {
                self.correct_text(comment_cmds::Correction::Insert);
            } else if let Some(text) = typed {
                self.correct_text(comment_cmds::Correction::Replace(text));
            }
        } else if key(Key::Delete) || key(Key::Backspace) {
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
        // One-letter tool keys ("H" Hand, "V" Textauswahl, …); not while
        // "Textkorrektur" takes typed letters.
        let correcting = self.tool == Tool::TextCorrection;
        for &(label, tool) in crate::ui::tools::TOOL_KEYS {
            if !correcting && Key::from_name(label).is_some_and(key) {
                self.run(ctx, commands::Cmd::Tool(tool));
            }
        }
        if ctx.input(|i| i.key_pressed(Key::F3)) {
            let back = ctx.input(|i| i.modifiers.shift);
            if let Some(tab) = self.tab() {
                tab.next_hit(!back);
            }
        }
        if ctx.input(|i| i.key_pressed(Key::F11)) {
            self.run(ctx, commands::Cmd::Fullscreen);
        }
        if ctx.input(|i| i.key_pressed(Key::F4)) {
            self.settings.show_sidebar = !self.settings.show_sidebar;
        }
        // Strg+F1: "Menüband reduzieren" / "anheften" (as in office programs).
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::F1)) {
            self.toggle_ribbon();
        }
    }

    fn handle_dropped_files(&mut self, ctx: &Context) {
        let dropped: Vec<PathBuf> =
            ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        for path in dropped {
            if page_dialogs::openable(&path) {
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
        if self.window_groups(0).len() > 1 {
            self.central_groups(ui);
            return;
        }
        let group = self.window_groups(0).first().copied().unwrap_or(0);
        match self.shown_in_group(0, group) {
            Some(i) => self.doc_view(ui, i),
            None => self.start_page(ui),
        }
    }

    /// The document view of tab `index`.
    pub(super) fn doc_view(&mut self, ui: &mut Ui, index: usize) {
        // A portfolio shows its file list (until "Deckblatt anzeigen").
        if let Some(tab) = self.tabs.get_mut(index)
            && !tab.show_cover
            && tab.is_portfolio()
        {
            self.portfolio_ui(ui, index);
            return;
        }
        let t = self.t();
        let ctx = ui.ctx().clone();
        let mut budget = Budget::new(Duration::from_millis(40));
        let tool = self.tool;
        let style = self.settings.style(tool);
        let stamp = self.settings.stamp;
        let highlight_fields = self.settings.highlight_fields;
        let view = self.view_options();
        if let Some(tab) = self.tabs.get_mut(index) {
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
                view,
                exclusive: self.settings.exclusive_mode,
                highlight_off: self.settings.highlight_off.clone(),
                highlight_required_only: self.settings.highlight_required_only,
            };
            canvas::show_document(ui, tab, &mut cx);
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

    /// The display options of the ribbon tab "Ansicht" for the canvas.
    fn view_options(&self) -> canvas::ViewOptions {
        let s = &self.settings;
        canvas::ViewOptions {
            layout: crate::ui::view::LayoutOptions {
                mode: s.layout,
                cover_alone: s.cover_alone,
                gaps: s.page_gaps,
                right_to_left: s.right_to_left,
                side_by_side: s.side_by_side,
            },
            grid: s.show_grid.then_some((s.grid_mm * 72.0 / 25.4, s.grid_subdivisions)),
            guides: s.show_guides,
            transparency: s.transparency_grid,
            highlight_links: s.highlight_links,
            snap: s.snap.then_some((s.snap_grid, s.snap_guides)),
            rulers: s.show_rulers,
            unit: s.unit,
            loupe: (s.show_loupe || self.tool == Tool::Loupe).then_some(s.loupe_factor),
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
                for path in self.settings.recent_files.iter().take(12).cloned().collect::<Vec<_>>() {
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
        self.apply_view_settings(&ctx);
        self.check_inbox(&ctx);
        if let Some(path) = self.pending_open.pop_front() {
            self.open_path(&ctx, path, None);
        }
        self.handle_dropped_files(&ctx);
        self.poll_download(&ctx);
        self.recovery_tick(&ctx);
        self.view_tick(&ctx);
        self.compare_tick(&ctx);
        self.note_group_focus();
        // "Vollbild": only the presentation.
        if self.presentation.is_some() {
            self.handle_close_request(&ctx);
            egui::CentralPanel::no_frame().show(ui, |ui| self.presentation_ui(ui));
            self.dialogs(&ctx);
            self.process_events(&ctx);
            return;
        }
        // Keyboard letters of the ribbon first: while shown, the keys are
        // theirs, not the shortcuts'.
        self.keytips_input(&ctx);
        self.shortcuts(&ctx);
        self.handle_close_request(&ctx);

        self.ribbon_ui(ui);
        if self.backstage_open() {
            self.backstage_ui(ui);
        } else {
            self.recovery_bar(ui);
            self.status_bar(ui);
            self.tab_bar(ui);
            if !self.tabs.is_empty() {
                if self.settings.show_sidebar {
                    self.sidebar(ui);
                }
                if self.compare.is_some() {
                    self.compare_panel(ui);
                } else if self.settings.show_properties {
                    self.properties_panel(ui);
                }
            }
            egui::CentralPanel::no_frame().show(ui, |ui| self.central(ui));
            self.view_windows(&ctx);
            self.calculator_windows(&ctx);
        }
        self.doc_windows(&ctx);
        self.dialogs(&ctx);
        self.keytips_paint(&ctx);
        self.process_events(&ctx);
        self.update_title(&ctx);
        if !self.pending_open.is_empty() {
            ctx.request_repaint();
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        // Called regularly and on exit: remember the open documents.
        if let Some(session) = self.current_session(String::new()) {
            self.settings.last_session = session;
        }
        self.remember_pages();
        self.settings.last_tool = self.tool;
        eframe::set_value(storage, SETTINGS_KEY, &self.settings);
    }
}
