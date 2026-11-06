//! The macOS menu bar ("VersoloPDF", "Ablage", "Bearbeiten", …) with
//! VersoloPDF's commands. The structure is defined here (and tested on
//! every system); `crate::macos` turns it into the menus of macOS.
//!
//! A menu entry whose keys VersoloPDF already handles (⌘S, ⌘Z, ⌘C, …)
//! hands VersoloPDF exactly that key press: the command behaves the same
//! whether it is chosen in the menu or typed, also in text fields, and
//! nothing runs twice (macOS gives a key to the menu or to the window,
//! never to both). The other entries run their command.

use egui::{Event, Key, Modifiers};

use super::SidePanel;
use super::commands::Cmd;
use super::comment_cmds::CommentCmd;
use super::view_cmds::ViewCmd;
use crate::ui::i18n::Language;

/// A key combination of a menu entry (⌘ is always part of it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Keys {
    pub key: Key,
    pub shift: bool,
    pub alt: bool,
    /// The control key (⌃), as in ⌃⌘F.
    pub ctrl: bool,
}

impl Keys {
    const fn cmd(key: Key) -> Keys {
        Keys { key, shift: false, alt: false, ctrl: false }
    }

    const fn shift(key: Key) -> Keys {
        Keys { key, shift: true, alt: false, ctrl: false }
    }

    /// The character macOS shows and matches ("s", "0", "+", ",").
    pub fn character(self) -> String {
        match self.key {
            Key::Plus => "+".into(),
            Key::Minus => "-".into(),
            Key::Period => ".".into(),
            Key::Comma => ",".into(),
            k => k.name().to_lowercase(),
        }
    }

    /// The modifiers as egui reports them for this key press on a Mac.
    pub fn modifiers(self) -> Modifiers {
        Modifiers {
            alt: self.alt,
            ctrl: self.ctrl,
            shift: self.shift,
            mac_cmd: cfg!(target_os = "macos"),
            command: true,
        }
    }
}

/// Reads a shortcut as shown in VersoloPDF ("Strg+Umschalt+S", "Ctrl+0").
/// Only combinations with Strg/Ctrl (⌘ on the Mac) become menu keys; single
/// letters (tool keys) and function keys stay with the window.
pub fn parse_keys(text: &str) -> Option<Keys> {
    let (mods, key) = text.rsplit_once('+')?;
    let mut keys = Keys { key: Key::from_name(key)?, shift: false, alt: false, ctrl: false };
    let mut command = false;
    for m in mods.split('+') {
        match m {
            "Strg" | "Ctrl" => command = true,
            "Umschalt" | "Shift" => keys.shift = true,
            "Alt" => keys.alt = true,
            _ => return None,
        }
    }
    command.then_some(keys)
}

/// A shortcut as shown on this system: "Strg+Umschalt+S" on Windows and
/// Linux, "⇧⌘S" on the Mac.
pub fn key_text(text: &str) -> std::borrow::Cow<'_, str> {
    if cfg!(target_os = "macos") { std::borrow::Cow::Owned(mac_key_text(text)) } else { text.into() }
}

/// "Strg+Umschalt+S" → "⇧⌘S" (Apple's order ⌃⌥⇧⌘); other texts stay.
pub fn mac_key_text(text: &str) -> String {
    match parse_keys(text) {
        Some(k) => {
            let mut s = String::new();
            for (on, sign) in [(k.ctrl, '⌃'), (k.alt, '⌥'), (k.shift, '⇧'), (true, '⌘')] {
                if on {
                    s.push(sign);
                }
            }
            s.push_str(&k.character().to_uppercase());
            s
        }
        None => text.to_string(),
    }
}

/// What a click on an entry does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Runs the command.
    Run(Cmd),
    /// Hands VersoloPDF the key press (the command, if any, decides whether
    /// the entry is greyed out).
    Press(Keys, Option<Cmd>),
}

/// Extra events of the clipboard keys, as egui sends them for a key press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clip {
    Cut,
    Copy,
    Paste,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub label: (&'static str, &'static str),
    pub action: Action,
    /// The keys shown in the menu.
    pub keys: Option<Keys>,
    pub clip: Option<Clip>,
}

/// Entries macOS itself provides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standard {
    Services,
    Hide,
    HideOthers,
    ShowAll,
    Minimize,
    Zoom,
    BringAllToFront,
}

impl Standard {
    pub fn label(self, de: bool) -> &'static str {
        let (g, e) = match self {
            Standard::Services => ("Dienste", "Services"),
            Standard::Hide => ("VersoloPDF ausblenden", "Hide VersoloPDF"),
            Standard::HideOthers => ("Andere ausblenden", "Hide Others"),
            Standard::ShowAll => ("Alle einblenden", "Show All"),
            Standard::Minimize => ("Im Dock ablegen", "Minimize"),
            Standard::Zoom => ("Zoomen", "Zoom"),
            Standard::BringAllToFront => ("Alle nach vorne bringen", "Bring All to Front"),
        };
        if de { g } else { e }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    Item(Item),
    Standard(Standard),
    Separator,
}

/// Role of a menu for macOS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The program menu ("VersoloPDF").
    App,
    Plain,
    /// "Fenster": macOS adds the list of windows.
    Window,
    /// "Hilfe": macOS adds the search field for menu commands.
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Menu {
    pub title: (&'static str, &'static str),
    pub role: Role,
    pub entries: Vec<Entry>,
}

/// A command; keys VersoloPDF knows for it are shown and pressed.
fn cmd(c: Cmd) -> Entry {
    let keys = c.shortcut().and_then(|(de, _)| parse_keys(de));
    let action = match keys {
        Some(k) => Action::Press(k, Some(c)),
        None => Action::Run(c),
    };
    Entry::Item(Item { label: c.names(), action, keys, clip: None })
}

/// A command with a key of its own on the Mac (VersoloPDF itself does not
/// react to it): the menu runs the command.
fn cmd_mac_key(c: Cmd, keys: Keys) -> Entry {
    Entry::Item(Item { label: c.names(), action: Action::Run(c), keys: Some(keys), clip: None })
}

/// A key press VersoloPDF handles without a menu shortcut of its own.
fn press(label: (&'static str, &'static str), keys: Keys, c: Option<Cmd>, clip: Option<Clip>) -> Entry {
    Entry::Item(Item { label, action: Action::Press(keys, c), keys: Some(keys), clip })
}

/// The whole menu bar.
pub fn menu_bar() -> Vec<Menu> {
    use Entry::Separator as Sep;
    use Entry::Standard as Std;
    vec![
        Menu {
            title: ("VersoloPDF", "VersoloPDF"),
            role: Role::App,
            entries: vec![
                Entry::Item(Item {
                    label: ("Über VersoloPDF", "About VersoloPDF"),
                    action: Action::Run(Cmd::About),
                    keys: None,
                    clip: None,
                }),
                Sep,
                cmd(Cmd::CheckUpdate),
                cmd_mac_key(Cmd::Settings, Keys::cmd(Key::Comma)),
                Sep,
                Std(Standard::Services),
                Sep,
                Std(Standard::Hide),
                Std(Standard::HideOthers),
                Std(Standard::ShowAll),
                Sep,
                Entry::Item(Item {
                    label: ("VersoloPDF beenden", "Quit VersoloPDF"),
                    action: Action::Press(Keys::cmd(Key::Q), Some(Cmd::Exit)),
                    keys: Some(Keys::cmd(Key::Q)),
                    clip: None,
                }),
            ],
        },
        Menu {
            title: ("Ablage", "File"),
            role: Role::Plain,
            entries: vec![
                cmd(Cmd::New),
                cmd(Cmd::Open),
                Sep,
                cmd(Cmd::Close),
                cmd(Cmd::CloseAll),
                cmd(Cmd::Save),
                cmd(Cmd::SaveAs),
                cmd(Cmd::SaveOptimized),
                cmd(Cmd::Revert),
                Sep,
                cmd(Cmd::RenameFile),
                cmd(Cmd::MoveFile),
                cmd(Cmd::OpenFolder),
                Sep,
                cmd(Cmd::Email),
                cmd(Cmd::EmailAll),
                Sep,
                cmd(Cmd::DocProperties),
                Sep,
                cmd(Cmd::Print),
                cmd(Cmd::PrintAll),
            ],
        },
        Menu {
            title: ("Bearbeiten", "Edit"),
            role: Role::Plain,
            entries: vec![
                press(Cmd::Undo.names(), Keys::cmd(Key::Z), Some(Cmd::Undo), None),
                press(Cmd::Redo.names(), Keys::shift(Key::Z), Some(Cmd::Redo), None),
                Sep,
                press(("Ausschneiden", "Cut"), Keys::cmd(Key::X), None, Some(Clip::Cut)),
                press(("Kopieren", "Copy"), Keys::cmd(Key::C), None, Some(Clip::Copy)),
                press(("Einsetzen", "Paste"), Keys::cmd(Key::V), None, Some(Clip::Paste)),
                press(Cmd::SelectAll.names(), Keys::cmd(Key::A), None, None),
                Sep,
                press(("Suchen …", "Find …"), Keys::cmd(Key::F), Some(Cmd::Search), None),
                press(("Befehl suchen …", "Find a command …"), Keys::cmd(Key::Period), None, None),
                Sep,
                cmd(Cmd::Tool(crate::ui::tools::Tool::Hand)),
                cmd(Cmd::Tool(crate::ui::tools::Tool::SelectText)),
                cmd(Cmd::Tool(crate::ui::tools::Tool::SelectAnnot)),
                cmd(Cmd::Tool(crate::ui::tools::Tool::EditText)),
                cmd(Cmd::Tool(crate::ui::tools::Tool::EditObjects)),
            ],
        },
        Menu {
            title: ("Darstellung", "View"),
            role: Role::Plain,
            entries: vec![
                cmd(Cmd::Panel(SidePanel::Thumbnails)),
                cmd(Cmd::Panel(SidePanel::Bookmarks)),
                cmd(Cmd::Panel(SidePanel::Comments)),
                cmd(Cmd::Panel(SidePanel::Attachments)),
                cmd(Cmd::ToggleSidebar),
                cmd(Cmd::ToggleProperties),
                Sep,
                cmd(Cmd::FitPage),
                cmd(Cmd::ActualSize),
                cmd(Cmd::FitWidth),
                cmd(Cmd::FitContent),
                press(Cmd::ZoomIn.names(), Keys::cmd(Key::Plus), Some(Cmd::ZoomIn), None),
                press(Cmd::ZoomOut.names(), Keys::cmd(Key::Minus), Some(Cmd::ZoomOut), None),
                Sep,
                cmd(Cmd::LayoutSingle),
                cmd(Cmd::LayoutContinuous),
                cmd(Cmd::LayoutFacing),
                Sep,
                cmd(Cmd::RotateViewLeft),
                cmd(Cmd::RotateViewRight),
                Sep,
                cmd(Cmd::View(ViewCmd::Rulers)),
                cmd(Cmd::DarkMode),
                Sep,
                cmd_mac_key(Cmd::Fullscreen, Keys { key: Key::F, shift: false, alt: false, ctrl: true }),
            ],
        },
        Menu {
            title: ("Dokument", "Document"),
            role: Role::Plain,
            entries: vec![
                cmd(Cmd::InsertFromFile),
                cmd(Cmd::InsertBlank),
                cmd(Cmd::DeletePages),
                cmd(Cmd::ExtractPages),
                cmd(Cmd::RotatePages),
                cmd(Cmd::CropPages),
                Sep,
                cmd(Cmd::MergeFiles),
                cmd(Cmd::SplitDocument),
                Sep,
                cmd(Cmd::Watermark),
                cmd(Cmd::HeaderFooter),
                cmd(Cmd::RecognizeText),
                Sep,
                cmd(Cmd::PasswordProtect),
                cmd(Cmd::RemoveProtection),
                cmd(Cmd::SignDocument),
                cmd(Cmd::VerifySignatures),
            ],
        },
        Menu {
            title: ("Kommentare", "Comments"),
            role: Role::Plain,
            entries: vec![
                cmd(Cmd::Tool(crate::ui::tools::Tool::Highlight)),
                cmd(Cmd::Tool(crate::ui::tools::Tool::Note)),
                cmd(Cmd::Tool(crate::ui::tools::Tool::TextBox)),
                Sep,
                cmd(Cmd::CommentList),
                cmd(Cmd::Comment(CommentCmd::ShowAll)),
                cmd(Cmd::Comment(CommentCmd::HideAll)),
                Sep,
                cmd(Cmd::SummarizeComments),
                cmd(Cmd::ExportComments),
                cmd(Cmd::ImportComments),
                cmd(Cmd::Flatten),
            ],
        },
        Menu {
            title: ("Fenster", "Window"),
            role: Role::Window,
            entries: vec![
                Std(Standard::Minimize),
                Std(Standard::Zoom),
                Sep,
                cmd(Cmd::View(ViewCmd::ReopenClosed)),
                Sep,
                Std(Standard::BringAllToFront),
            ],
        },
        Menu {
            title: ("Hilfe", "Help"),
            role: Role::Help,
            entries: vec![
                cmd(Cmd::Manual),
                cmd(Cmd::Homepage),
                Sep,
                cmd(Cmd::OpenLog),
                cmd(Cmd::ReportProblem),
            ],
        },
    ]
}

/// The entries with a command, in menu order: an entry's number (its
/// "tag" for macOS) is its position in this list.
pub fn items(menus: &[Menu]) -> Vec<&Item> {
    menus
        .iter()
        .flat_map(|m| &m.entries)
        .filter_map(|e| match e {
            Entry::Item(i) => Some(i),
            _ => None,
        })
        .collect()
}

/// The events egui gets for a key press of an entry.
pub fn key_events(keys: Keys, clip: Option<Clip>, clipboard: impl FnOnce() -> Option<String>) -> Vec<Event> {
    let mut events = vec![Event::Key {
        key: keys.key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: keys.modifiers(),
    }];
    match clip {
        Some(Clip::Copy) => events.push(Event::Copy),
        Some(Clip::Cut) => events.push(Event::Cut),
        Some(Clip::Paste) => {
            if let Some(text) = clipboard() {
                events.push(Event::Paste(text));
            }
        }
        None => {}
    }
    events
}

/// The menu label in the user's language.
pub fn pick(label: (&'static str, &'static str), lang: Language) -> &'static str {
    if lang == Language::German { label.0 } else { label.1 }
}

#[cfg(target_os = "macos")]
impl super::VersoloPdfApp {
    /// Once per frame: builds the menu bar (again when the language
    /// changed), passes on the greyed / ticked states and carries out
    /// chosen entries.
    pub(super) fn mac_menu(&mut self, ctx: &egui::Context) {
        let lang = self.settings.language;
        let menus = menu_bar();
        if self.mac_menu_language != Some(lang) {
            crate::macos::set_menu_bar(&menus, lang == Language::German);
            self.mac_menu_language = Some(lang);
        }
        let items = items(&menus);
        let dialog = self.dialog.is_some();
        let states = items
            .iter()
            .map(|item| match item.action {
                Action::Run(c) => (!dialog && self.cmd_enabled(c), self.cmd_selected(c)),
                Action::Press(_, Some(c)) => (self.cmd_enabled(c), self.cmd_selected(c)),
                Action::Press(_, None) => (true, false),
            })
            .collect();
        crate::macos::set_menu_states(states);
        for tag in crate::macos::take_menu_clicks() {
            let Some(item) = items.get(tag) else { continue };
            match item.action {
                Action::Run(c) => self.run(ctx, c),
                Action::Press(keys, _) => {
                    let events = key_events(keys, item.clip, || {
                        arboard::Clipboard::new().ok().and_then(|mut c| c.get_text().ok())
                    });
                    ctx.input_mut(|i| i.events.extend(events));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_are_read_and_shown_the_mac_way() {
        assert_eq!(parse_keys("Strg+Umschalt+S"), Some(Keys::shift(Key::S)));
        assert_eq!(parse_keys("Ctrl+0"), Some(Keys::cmd(Key::Num0)));
        assert_eq!(
            parse_keys("Strg+Alt+L"),
            Some(Keys { key: Key::L, shift: false, alt: true, ctrl: false })
        );
        // Tool letters and keys without Strg stay with the window.
        assert_eq!(parse_keys("H"), None);
        assert_eq!(parse_keys("Umschalt+F3"), None);
        assert_eq!(mac_key_text("Strg+Umschalt+S"), "⇧⌘S");
        assert_eq!(mac_key_text("Strg+Alt+L"), "⌥⌘L");
        assert_eq!(mac_key_text("Strg+0"), "⌘0");
        assert_eq!(mac_key_text("Entf"), "Entf");
        assert_eq!(Keys::cmd(Key::Comma).character(), ",");
        assert_eq!(Keys::cmd(Key::S).character(), "s");
    }

    #[test]
    fn every_shown_shortcut_of_the_program_can_be_read() {
        for c in super::super::commands::SEARCHABLE {
            if let Some((de, en)) = c.shortcut()
                && (de.contains("Strg") || en.contains("Ctrl"))
            {
                assert_eq!(parse_keys(de), parse_keys(en), "{c:?}");
                assert!(parse_keys(de).is_some(), "{c:?}: {de}");
            }
        }
    }

    #[test]
    fn menu_bar_is_complete_and_unambiguous() {
        let menus = menu_bar();
        assert_eq!(menus[0].role, Role::App);
        assert!(menus.iter().any(|m| m.role == Role::Window) && menus.iter().any(|m| m.role == Role::Help));
        let items = items(&menus);
        assert!(items.len() > 60, "{}", items.len());
        // No key combination twice.
        let mut seen = Vec::new();
        for item in &items {
            if let Some(k) = item.keys {
                assert!(!seen.contains(&k), "{:?} twice ({})", k, item.label.0);
                seen.push(k);
            }
            assert!(!item.label.0.is_empty() && !item.label.1.is_empty());
        }
        // The usual Mac keys are there.
        for k in [
            Keys::cmd(Key::Q),
            Keys::cmd(Key::Comma),
            Keys::cmd(Key::S),
            Keys::cmd(Key::Z),
            Keys::cmd(Key::C),
        ] {
            assert!(seen.contains(&k), "{k:?}");
        }
        // Entries with keys VersoloPDF handles press them; ⌘, and ⌃⌘F run.
        let save = items.iter().find(|i| i.label == Cmd::Save.names()).unwrap();
        assert_eq!(save.action, Action::Press(Keys::cmd(Key::S), Some(Cmd::Save)));
        let settings = items.iter().find(|i| i.label == Cmd::Settings.names()).unwrap();
        assert_eq!(settings.action, Action::Run(Cmd::Settings));
    }

    #[test]
    fn pressed_keys_reach_the_program_like_typed_ones() {
        let ev = key_events(Keys::cmd(Key::V), Some(Clip::Paste), || Some("Text".into()));
        assert_eq!(ev.len(), 2);
        assert!(
            matches!(&ev[0], Event::Key { key: Key::V, pressed: true, modifiers, .. } if modifiers.command)
        );
        assert_eq!(ev[1], Event::Paste("Text".into()));
        // The app's own shortcut check (consume_shortcut) accepts the press:
        // ⌘S matches "Strg+S", ⇧⌘S only "Strg+Umschalt+S".
        let mods = |k: Keys| match &key_events(k, None, || None)[0] {
            Event::Key { modifiers, .. } => *modifiers,
            _ => unreachable!(),
        };
        assert!(mods(Keys::cmd(Key::S)).matches_logically(Modifiers::COMMAND));
        assert!(!mods(Keys::cmd(Key::S)).matches_logically(Modifiers::COMMAND | Modifiers::SHIFT));
        assert!(mods(Keys::shift(Key::S)).matches_logically(Modifiers::COMMAND | Modifiers::SHIFT));
    }
}
