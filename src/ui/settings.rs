//! Persistent user preferences.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::{AnnotStyle, StampPreset};
use crate::ui::i18n::{Language, Strings};
use crate::ui::pickers::{PresetStore, Unit};
use crate::ui::tools::Tool;
use crate::ui::view::{PageLayout, ZoomMode};

pub const MAX_RECENT: usize = 50;
/// Upper bound for saved sessions and documents per session.
pub const MAX_SESSIONS: usize = 50;
pub const MAX_SESSION_FILES: usize = 200;

/// A set of open documents ("Sitzung").
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    pub name: String,
    pub files: Vec<PathBuf>,
    /// Index of the active document.
    pub active: usize,
    /// Date and time it was saved (for display).
    pub saved: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub language: Language,
    pub author: String,
    pub recent_files: Vec<PathBuf>,
    /// Pinned documents shown above the recent files.
    pub pinned_files: Vec<PathBuf>,
    /// Documents open when MrPDF was last closed.
    pub last_session: Session,
    /// Sessions saved by name.
    pub sessions: Vec<Session>,
    /// Reopen the last session at start when no files are given.
    pub restore_session: bool,
    /// Addresses entered in "Von URL öffnen".
    pub url_history: Vec<String>,
    /// "Informationen zum Wiederherstellen speichern alle n Minute(n)".
    pub recovery: bool,
    pub recovery_minutes: u32,
    /// "Wiederherstellungsleiste anzeigen, wenn Daten verfügbar sind".
    pub recovery_bar: bool,
    pub dark_mode: bool,
    pub show_sidebar: bool,
    pub show_properties: bool,
    pub layout: PageLayout,
    pub zoom_mode: ZoomMode,
    pub styles: Vec<(Tool, AnnotStyle)>,
    pub stamp: StampPreset,
    pub highlight_fields: bool,
    /// Length unit of page dialogs.
    pub unit: Unit,
    /// Saved dialog settings (see [crate::ui::pickers::preset_combo]).
    pub presets: PresetStore,

    // --- "Einstellungen" dialog ---
    /// Allgemein: "Tab-Leiste verbergen, wenn nur ein Dokument geöffnet ist".
    pub hide_single_tab: bool,
    /// Allgemein: "Das Dateimenü beim Start anzeigen".
    pub backstage_at_start: bool,
    /// Dokumente: folder offered by "Speichern unter".
    pub save_as_folder: SaveFolder,
    pub last_save_folder: Option<PathBuf>,
    /// Seitenanzeige: reopen documents at the page last shown.
    pub restore_last_page: bool,
    pub last_pages: Vec<(PathBuf, usize)>,
    /// Anmerkungen: "Neu erstellte Anmerkung automatisch auswählen".
    pub auto_select_new: bool,
    /// Oberfläche anpassen: "Dunkler Modus aus System".
    pub follow_system_theme: bool,
    /// Oberfläche anpassen: size of the user interface (1.0 = 100 %).
    pub ui_scale: f32,
    /// Vollbild: "Beenden durch Drücken der ESC-Taste".
    pub esc_exits_fullscreen: bool,
    /// Werkzeuge: tool at start and whether the last tool is remembered.
    pub default_tool: Tool,
    pub remember_tool: bool,
    pub last_tool: Tool,
    /// Sicherheit: what happens when a document opens a web page.
    pub link_policy: LinkPolicy,
    /// Sicherheit: web sites (host names) always allowed.
    pub trusted_hosts: Vec<String>,
    /// E-Mails versenden: subject pattern (%[FileName]).
    pub email_subject: String,
    /// Identität (the name is `author`).
    pub identity: Identity,
    /// Bildschirmfotos: resolution (0: twice the screen resolution), greyscale.
    pub snapshot_dpi: u32,
    pub snapshot_gray: bool,
    /// Barrierefreiheit: "Farben im Dokument ersetzen".
    pub recolor: bool,
    pub recolor_background: [u8; 3],
    pub recolor_text: [u8; 3],

    // --- Ribbon tab "Ansicht" ---
    /// "Titelseite in der zweiseitigen Ansicht einzeln".
    pub cover_alone: bool,
    /// "Seitenzwischenräume".
    pub page_gaps: bool,
    /// "Rechts-nach-Links-Seitenansicht".
    pub right_to_left: bool,
    /// "Seiten nebeneinander" (instead of "Standardlayout").
    pub side_by_side: bool,
    pub show_rulers: bool,
    pub show_grid: bool,
    pub show_guides: bool,
    /// "Transparenzgitter" instead of white paper.
    pub transparency_grid: bool,
    /// "Seitenmaße/Position" in the status bar.
    pub show_page_info: bool,
    /// "Einrasten" (at the grid and/or the guides).
    pub snap: bool,
    pub snap_grid: bool,
    pub snap_guides: bool,
    /// "Verweise hervorheben".
    pub highlight_links: bool,
    /// Grid: distance of the main lines (mm) and number of subdivisions.
    pub grid_mm: f32,
    pub grid_subdivisions: u32,
    /// "Lupe" window and its magnification.
    pub show_loupe: bool,
    pub loupe_factor: f32,
    /// "Ausschnitt und Vergrößerung" window.
    pub show_pan_zoom: bool,
    /// "Design" of the user interface.
    pub design: crate::ui::theme::Design,
    /// "Automatisch blättern": points per second.
    pub auto_scroll_speed: f32,
    /// "Vorlesen": volume 0–100, speed −10…10 and options.
    pub read_volume: u32,
    pub read_rate: i32,
    pub read_fields: bool,
    pub read_annots: bool,
    pub read_follow: bool,

    // --- "Vollbild" (presentation) ---
    /// Background colour around the page.
    pub fs_background: [u8; 3],
    /// Transition used for pages without one ("Standardübergang").
    pub fs_default_transition: crate::transitions::Transition,
    /// "Immer den Standardübergang verwenden".
    pub fs_always_default: bool,
    /// "Nach letzter Seite wiederholen".
    pub fs_loop: bool,
    /// "Automatisch weiter nach n Sekunden" (0: off).
    pub fs_auto_seconds: u32,
    /// "Navigationsleiste anzeigen".
    pub fs_nav_bar: bool,
    /// Ribbon tab "Ansicht": document tabs grouped side by side (true) or
    /// on top of each other ("Tabs anordnen").
    pub tab_groups_vertical: bool,
    /// "Auswahl beibehalten": tools whose stay-active behaviour differs
    /// from [`Tool::is_sticky`].
    pub keep_tool: Vec<(Tool, bool)>,
    /// "Exklusiver Modus": while a comment tool is active, comments of its
    /// type can be selected and moved; other comments are ignored.
    pub exclusive_mode: bool,
    /// Tax rate of the calculator in percent ("TAX+", "TAX−").
    pub tax_rate: f64,
    /// "Felder hervorheben ▾": field kinds that are not highlighted.
    pub highlight_off: Vec<FieldGroup>,
    /// "Nur erforderliche Felder".
    pub highlight_required_only: bool,
    /// Texterkennung: path of the Tesseract program (None: search it) and
    /// the languages ("deu+eng").
    pub tesseract: Option<PathBuf>,
    pub ocr_languages: String,
}

/// Kinds of form fields in "Felder hervorheben ▾".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FieldGroup {
    Buttons,
    CheckBoxes,
    RadioButtons,
    ComboBoxes,
    ListBoxes,
    TextFields,
    Signatures,
}

impl FieldGroup {
    pub const ALL: [FieldGroup; 7] = [
        FieldGroup::Buttons,
        FieldGroup::CheckBoxes,
        FieldGroup::RadioButtons,
        FieldGroup::ComboBoxes,
        FieldGroup::ListBoxes,
        FieldGroup::TextFields,
        FieldGroup::Signatures,
    ];

    pub fn of(kind: &crate::model::FieldKind) -> Option<FieldGroup> {
        use crate::model::FieldKind;
        Some(match kind {
            FieldKind::PushButton => FieldGroup::Buttons,
            FieldKind::CheckBox => FieldGroup::CheckBoxes,
            FieldKind::RadioButton => FieldGroup::RadioButtons,
            FieldKind::ComboBox => FieldGroup::ComboBoxes,
            FieldKind::ListBox => FieldGroup::ListBoxes,
            FieldKind::Text { .. } => FieldGroup::TextFields,
            FieldKind::Signature => FieldGroup::Signatures,
            FieldKind::Unknown => return None,
        })
    }

    pub fn names(self) -> (&'static str, &'static str) {
        match self {
            FieldGroup::Buttons => ("Schaltflächen", "Buttons"),
            FieldGroup::CheckBoxes => ("Auswahlfelder (Häkchen)", "Check boxes"),
            FieldGroup::RadioButtons => ("Auswahlfelder (Knöpfe)", "Radio buttons"),
            FieldGroup::ComboBoxes => ("Auswahlmenüs", "Combo boxes"),
            FieldGroup::ListBoxes => ("Listen", "List boxes"),
            FieldGroup::TextFields => ("Textfelder", "Text fields"),
            FieldGroup::Signatures => ("Signaturen", "Signatures"),
        }
    }
}

/// Folder offered by "Speichern unter".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SaveFolder {
    /// "Letzter Ordner"
    #[default]
    Last,
    /// The folder of the document.
    Document,
}

/// Opening web pages from documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LinkPolicy {
    /// Ask unless the site is in the trust list ("Vertrauensliste").
    #[default]
    Ask,
    Allow,
    Block,
}

/// "Identität": details of the user (used for comments and stamps).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Identity {
    pub title: String,
    pub organisation: String,
    pub department: String,
    pub email: String,
}

/// Host name of a web address (lower case), for the trust list.
pub fn url_host(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let host = rest.split(['/', '?', '#']).next()?.rsplit('@').next()?;
    let host = host.split(':').next()?.trim().to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: Language::detect(),
            author: default_author(),
            recent_files: Vec::new(),
            pinned_files: Vec::new(),
            last_session: Session::default(),
            sessions: Vec::new(),
            restore_session: false,
            url_history: Vec::new(),
            recovery: true,
            recovery_minutes: 5,
            recovery_bar: true,
            dark_mode: false,
            show_sidebar: true,
            show_properties: true,
            layout: PageLayout::Continuous,
            zoom_mode: ZoomMode::FitWidth,
            styles: Vec::new(),
            stamp: StampPreset::Approved,
            highlight_fields: true,
            unit: Unit::Mm,
            presets: Vec::new(),
            hide_single_tab: false,
            backstage_at_start: true,
            save_as_folder: SaveFolder::Last,
            last_save_folder: None,
            restore_last_page: true,
            last_pages: Vec::new(),
            auto_select_new: true,
            follow_system_theme: false,
            ui_scale: 1.0,
            esc_exits_fullscreen: true,
            default_tool: Tool::Hand,
            remember_tool: false,
            last_tool: Tool::Hand,
            link_policy: LinkPolicy::Ask,
            trusted_hosts: Vec::new(),
            email_subject: "%[FileName]".into(),
            identity: Identity::default(),
            snapshot_dpi: 0,
            snapshot_gray: false,
            recolor: false,
            recolor_background: [255, 255, 255],
            recolor_text: [0, 0, 0],
            cover_alone: false,
            page_gaps: true,
            right_to_left: false,
            side_by_side: false,
            show_rulers: false,
            show_grid: false,
            show_guides: true,
            transparency_grid: false,
            show_page_info: true,
            snap: false,
            snap_grid: true,
            snap_guides: true,
            highlight_links: false,
            grid_mm: 10.0,
            grid_subdivisions: 10,
            show_loupe: false,
            loupe_factor: 4.0,
            show_pan_zoom: false,
            design: crate::ui::theme::Design::Standard,
            auto_scroll_speed: 60.0,
            read_volume: 100,
            read_rate: 0,
            read_fields: false,
            read_annots: false,
            read_follow: true,
            fs_background: [0, 0, 0],
            fs_default_transition: crate::transitions::Transition {
                style: crate::transitions::Style::Replace,
                ..Default::default()
            },
            fs_always_default: false,
            fs_loop: false,
            fs_auto_seconds: 0,
            fs_nav_bar: true,
            tab_groups_vertical: true,
            keep_tool: Vec::new(),
            exclusive_mode: false,
            tax_rate: 19.0,
            // Buttons and combo boxes are not highlighted.
            highlight_off: vec![FieldGroup::Buttons, FieldGroup::ComboBoxes],
            highlight_required_only: false,
            tesseract: None,
            ocr_languages: "deu+eng".into(),
        }
    }
}

/// Upper bound of remembered page positions.
const MAX_LAST_PAGES: usize = 200;

fn default_author() -> String {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_default()
        .chars()
        .filter(|c| !c.is_control())
        .take(64)
        .collect()
}

impl Settings {
    pub fn strings(&self) -> &'static Strings {
        self.language.strings()
    }

    pub fn style(&self, tool: Tool) -> AnnotStyle {
        let mut style = self
            .styles
            .iter()
            .find(|(t, _)| *t == tool)
            .map(|(_, s)| s.clone())
            .unwrap_or_else(|| tool.default_style());
        style.author = self.author.clone();
        style
    }

    pub fn set_style(&mut self, tool: Tool, style: AnnotStyle) {
        if let Some(entry) = self.styles.iter_mut().find(|(t, _)| *t == tool) {
            entry.1 = style;
        } else {
            self.styles.push((tool, style));
        }
    }

    /// Whether `tool` stays active after creating an object.
    pub fn keeps_tool(&self, tool: Tool) -> bool {
        self.keep_tool
            .iter()
            .find(|(t, _)| *t == tool)
            .map_or(Self::keeps_by_default(tool), |(_, keep)| *keep)
    }

    /// Drawing tools and tools that set several points stay active.
    fn keeps_by_default(tool: Tool) -> bool {
        tool.is_sticky() || tool.gesture() == crate::ui::tools::Gesture::Vertices
    }

    pub fn set_keeps_tool(&mut self, tool: Tool, keep: bool) {
        self.keep_tool.retain(|(t, _)| *t != tool);
        if keep != Self::keeps_by_default(tool) {
            self.keep_tool.push((tool, keep));
        }
    }

    /// "Formate für Anmerkungen > Zurücksetzen".
    pub fn reset_style(&mut self, tool: Tool) {
        self.styles.retain(|(t, _)| *t != tool);
    }

    /// Paths stored by earlier versions with the Windows `\\?\` prefix are
    /// written plainly (duplicates that result are dropped).
    pub fn plain_paths(&mut self) {
        use crate::files::strip_verbatim;
        fn list(paths: &mut Vec<PathBuf>) {
            let mut seen = Vec::new();
            for p in std::mem::take(paths) {
                let p = strip_verbatim(p);
                if !seen.contains(&p) {
                    seen.push(p);
                }
            }
            *paths = seen;
        }
        list(&mut self.recent_files);
        list(&mut self.pinned_files);
        list(&mut self.last_session.files);
        for s in &mut self.sessions {
            list(&mut s.files);
        }
        let mut pages: Vec<(PathBuf, usize)> = Vec::new();
        for (p, page) in std::mem::take(&mut self.last_pages) {
            let p = strip_verbatim(p);
            if !pages.iter().any(|(q, _)| *q == p) {
                pages.push((p, page));
            }
        }
        self.last_pages = pages;
        self.last_save_folder = self.last_save_folder.take().map(strip_verbatim);
    }

    pub fn add_recent(&mut self, path: &Path) {
        self.recent_files.retain(|p| p != path);
        self.recent_files.insert(0, path.to_path_buf());
        self.recent_files.truncate(MAX_RECENT);
    }

    /// Remembers the page shown for a document.
    pub fn remember_page(&mut self, path: &Path, page: usize) {
        self.last_pages.retain(|(p, _)| p != path);
        self.last_pages.insert(0, (path.to_path_buf(), page));
        self.last_pages.truncate(MAX_LAST_PAGES);
    }

    /// The page last shown for a document.
    pub fn last_page(&self, path: &Path) -> Option<usize> {
        self.last_pages.iter().find(|(p, _)| p == path).map(|(_, page)| *page)
    }

    /// Resets all options but keeps the history (recent files, pins,
    /// sessions, page positions) and the saved presets.
    pub fn reset_options(&mut self) {
        let old = std::mem::take(self);
        self.keep_history(old);
    }

    /// Takes the history (recent files, pins, sessions, page positions,
    /// addresses) and the saved presets from `old`.
    pub fn keep_history(&mut self, old: Settings) {
        self.recent_files = old.recent_files;
        self.pinned_files = old.pinned_files;
        self.last_session = old.last_session;
        self.sessions = old.sessions;
        self.url_history = old.url_history;
        self.last_pages = old.last_pages;
        self.presets = old.presets;
    }

    /// Keeps the values of the view options in a sensible range.
    pub fn limit_view_values(&mut self) {
        self.grid_mm = if self.grid_mm.is_finite() { self.grid_mm.clamp(1.0, 200.0) } else { 10.0 };
        self.grid_subdivisions = self.grid_subdivisions.clamp(1, 20);
        self.loupe_factor =
            if self.loupe_factor.is_finite() { self.loupe_factor.clamp(1.5, 16.0) } else { 4.0 };
        self.auto_scroll_speed =
            if self.auto_scroll_speed.is_finite() { self.auto_scroll_speed.clamp(5.0, 2000.0) } else { 60.0 };
        self.read_volume = self.read_volume.min(100);
        self.read_rate = self.read_rate.clamp(-10, 10);
        self.fs_auto_seconds = self.fs_auto_seconds.min(3600);
        self.tax_rate = if self.tax_rate.is_finite() { self.tax_rate.clamp(0.0, 100.0) } else { 19.0 };
        if let Some(a) = self.fs_default_transition.advance {
            self.fs_default_transition.advance = (a.is_finite() && a > 0.0).then_some(a.min(3600.0));
        }
    }

    /// Writes the settings to a file (RON).
    pub fn export(&self, path: &Path) -> crate::Result<()> {
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(|e| crate::Error::Invalid(e.to_string()))?;
        std::fs::write(path, text).map_err(|e| crate::Error::io(path, e))
    }

    /// Reads settings written by [`Settings::export`].
    pub fn import(path: &Path) -> crate::Result<Settings> {
        let len = std::fs::metadata(path).map_err(|e| crate::Error::io(path, e))?.len();
        if len > 16 * 1024 * 1024 {
            return Err(crate::Error::FileTooLarge { size: len, limit: 16 * 1024 * 1024 });
        }
        let text = std::fs::read_to_string(path).map_err(|e| crate::Error::io(path, e))?;
        let mut s: Settings = ron::from_str(&text).map_err(|e| crate::Error::Invalid(e.to_string()))?;
        s.ui_scale = s.ui_scale.clamp(0.5, 2.0);
        s.recovery_minutes = s.recovery_minutes.min(600);
        s.limit_view_values();
        Ok(s)
    }

    /// Pins or unpins a document of the recent files list.
    pub fn toggle_pin(&mut self, path: &Path) {
        if self.pinned_files.iter().any(|p| p == path) {
            self.pinned_files.retain(|p| p != path);
        } else if self.pinned_files.len() < MAX_RECENT {
            self.pinned_files.push(path.to_path_buf());
        }
    }

    /// Saves `session` under its name (replacing one with the same name).
    pub fn store_session(&mut self, session: Session) {
        self.sessions.retain(|s| s.name != session.name);
        self.sessions.insert(0, session);
        self.sessions.truncate(MAX_SESSIONS);
    }
}

/// Reads a session file (RON) written by [`write_session`].
pub fn read_session(path: &Path) -> crate::Result<Session> {
    let len = std::fs::metadata(path).map_err(|e| crate::Error::io(path, e))?.len();
    if len > 1024 * 1024 {
        return Err(crate::Error::FileTooLarge { size: len, limit: 1024 * 1024 });
    }
    let text = std::fs::read_to_string(path).map_err(|e| crate::Error::io(path, e))?;
    let mut session: Session = ron::from_str(&text).map_err(|e| crate::Error::Invalid(e.to_string()))?;
    session.files.truncate(MAX_SESSION_FILES);
    Ok(session)
}

/// Writes a session file (RON).
pub fn write_session(path: &Path, session: &Session) -> crate::Result<()> {
    let text = ron::ser::to_string_pretty(session, ron::ser::PrettyConfig::default())
        .map_err(|e| crate::Error::Invalid(e.to_string()))?;
    std::fs::write(path, text).map_err(|e| crate::Error::io(path, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_files_are_unique_and_bounded() {
        let mut s = Settings::default();
        for i in 0..60 {
            s.add_recent(Path::new(&format!("/tmp/{i}.pdf")));
        }
        s.add_recent(Path::new("/tmp/5.pdf"));
        assert_eq!(s.recent_files.len(), MAX_RECENT);
        assert_eq!(s.recent_files[0], Path::new("/tmp/5.pdf"));
        assert_eq!(s.recent_files.iter().filter(|p| p.ends_with("5.pdf")).count(), 1);
    }

    #[test]
    fn verbatim_paths_of_older_versions_are_cleaned() {
        let mut s = Settings {
            recent_files: vec![
                PathBuf::from(r"\\?\C:\a.pdf"),
                PathBuf::from(r"C:\a.pdf"),
                PathBuf::from("/b.pdf"),
            ],
            last_pages: vec![(PathBuf::from(r"\\?\C:\a.pdf"), 3), (PathBuf::from(r"C:\a.pdf"), 1)],
            ..Settings::default()
        };
        s.plain_paths();
        assert_eq!(s.recent_files, [PathBuf::from(r"C:\a.pdf"), PathBuf::from("/b.pdf")]);
        assert_eq!(s.last_pages, [(PathBuf::from(r"C:\a.pdf"), 3)]);
    }

    #[test]
    fn pins_and_sessions() {
        let mut s = Settings::default();
        s.toggle_pin(Path::new("/a.pdf"));
        assert_eq!(s.pinned_files, vec![PathBuf::from("/a.pdf")]);
        s.toggle_pin(Path::new("/a.pdf"));
        assert!(s.pinned_files.is_empty());
        let session = Session {
            name: "Arbeit".into(),
            files: vec!["/a.pdf".into(), "/b.pdf".into()],
            active: 1,
            saved: "x".into(),
        };
        s.store_session(session.clone());
        s.store_session(Session { files: vec!["/c.pdf".into()], ..session.clone() });
        assert_eq!(s.sessions.len(), 1, "same name replaces");
        let dir = std::env::temp_dir().join(format!("mrpdf-session-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("s.mrpdfsession");
        write_session(&file, &session).unwrap();
        assert_eq!(read_session(&file).unwrap(), session);
        std::fs::write(&file, "kein ron").unwrap();
        assert!(read_session(&file).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn options_export_import_reset() {
        let mut s = Settings {
            hide_single_tab: true,
            ui_scale: 1.25,
            email_subject: "Datei %[FileName]".into(),
            ..Settings::default()
        };
        s.add_recent(Path::new("/a.pdf"));
        s.remember_page(Path::new("/a.pdf"), 7);
        assert_eq!(s.last_page(Path::new("/a.pdf")), Some(7));
        let dir = std::env::temp_dir().join(format!("mrpdf-settings-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("e.ron");
        s.export(&file).unwrap();
        let back = Settings::import(&file).unwrap();
        assert!(back.hide_single_tab);
        assert_eq!(back.email_subject, "Datei %[FileName]");
        assert_eq!(back.ui_scale, 1.25);
        // Old files without the new fields still load.
        std::fs::write(&file, "(author: \"X\")").unwrap();
        let old = Settings::import(&file).unwrap();
        assert_eq!(old.author, "X");
        assert!(old.backstage_at_start && old.restore_last_page);
        s.reset_options();
        assert!(!s.hide_single_tab);
        assert_eq!(s.recent_files.len(), 1, "history is kept");
        assert_eq!(s.last_page(Path::new("/a.pdf")), Some(7));
        assert_eq!(url_host("https://user@Example.org:8080/x?y"), Some("example.org".into()));
        assert_eq!(url_host("mailto:x@y"), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn styles_fall_back_to_defaults() {
        let mut s = Settings::default();
        assert_eq!(s.style(Tool::Highlight).color, Tool::Highlight.default_style().color);
        let mut style = s.style(Tool::Rectangle);
        style.width = 7.0;
        s.set_style(Tool::Rectangle, style);
        assert_eq!(s.style(Tool::Rectangle).width, 7.0);
    }
}
