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
        }
    }
}

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

    pub fn add_recent(&mut self, path: &Path) {
        self.recent_files.retain(|p| p != path);
        self.recent_files.insert(0, path.to_path_buf());
        self.recent_files.truncate(MAX_RECENT);
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
    fn styles_fall_back_to_defaults() {
        let mut s = Settings::default();
        assert_eq!(s.style(Tool::Highlight).color, Tool::Highlight.default_style().color);
        let mut style = s.style(Tool::Rectangle);
        style.width = 7.0;
        s.set_style(Tool::Rectangle, style);
        assert_eq!(s.style(Tool::Rectangle).width, 7.0);
    }
}
