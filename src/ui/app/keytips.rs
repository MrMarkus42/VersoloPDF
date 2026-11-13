//! Keyboard operation of the ribbon ("Tastaturbedienung"), as in Office:
//! Alt (or F10) shows a letter on every tab, the letter opens the tab and
//! shows letters on its commands, a command's letters run it (a drop-down
//! opens and shows letters on its entries). Esc goes back one level.
//!
//! The ribbon helpers register their buttons every frame; the letters are
//! given out after the frame and typed keys are matched before the next.

use std::cell::{Cell, RefCell};

use egui::{Color32, Context, Event, Id, Key, LayerId, Order, Rect, Ui};

use super::RibbonTab;
use super::VersoloPdfApp;

/// What the letters are shown on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// Tabs, "Datei" and the quick access buttons.
    Tabs,
    /// The commands of the open tab.
    Commands,
    /// The entries of an open drop-down.
    Menu,
}

/// What choosing a letter does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Tab(RibbonTab),
    File,
    Button,
    /// Opens the drop-down with this popup id.
    MenuButton(Id),
}

#[derive(Debug, Clone)]
pub struct Tip {
    pub id: Id,
    pub label: String,
    pub rect: Rect,
    pub kind: Kind,
}

#[derive(Default)]
pub struct KeyTips {
    level: Option<Level>,
    /// Level the open drop-down returns to with Esc.
    menu_from: Option<Level>,
    typed: String,
    /// The letters of the last frame.
    shown: Vec<(Tip, String)>,
    /// Buttons registered in this frame.
    frame: RefCell<Vec<Tip>>,
    /// Where the ribbon is drawing (row of tabs or the commands).
    zone: Cell<Option<Level>>,
    /// The button chosen by its letters, run when drawn.
    fire: Cell<Option<Id>>,
    alt_down: bool,
    alt_clean: bool,
}

/// Letters for the labels: the first letter of each label if free, else
/// another of its letters, else a free letter; when the letters run out,
/// a digit and a letter. No code is the start of another.
pub fn assign(labels: &[String]) -> Vec<String> {
    const LETTERS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZÄÖÜ";
    let mut used: Vec<char> = Vec::new();
    let mut out: Vec<Option<String>> = vec![None; labels.len()];
    let letters_of = |s: &str| -> Vec<char> {
        s.chars().flat_map(char::to_uppercase).filter(|c| LETTERS.contains(*c)).collect()
    };
    // First letters, then other letters of the label.
    for pass in 0..2 {
        for (i, label) in labels.iter().enumerate() {
            if out[i].is_some() {
                continue;
            }
            let ls = letters_of(label);
            let candidates: Vec<char> = if pass == 0 { ls.into_iter().take(1).collect() } else { ls };
            if let Some(c) = candidates.into_iter().find(|c| !used.contains(c)) {
                used.push(c);
                out[i] = Some(c.to_string());
            }
        }
    }
    // Any free letter, then digit + letter.
    let mut free = LETTERS.chars().filter(|c| !used.contains(c));
    let mut pairs = ('1'..='9').flat_map(|d| LETTERS.chars().map(move |c| format!("{d}{c}")));
    for o in out.iter_mut().filter(|o| o.is_none()) {
        *o = Some(free.next().map(String::from).or_else(|| pairs.next()).unwrap_or_default());
    }
    out.into_iter().map(Option::unwrap_or_default).collect()
}

impl KeyTips {
    pub fn active(&self) -> bool {
        self.level.is_some()
    }

    pub fn level(&self) -> Option<Level> {
        self.level
    }

    /// The ribbon draws the tab row (`Tabs`), the commands (`Commands`)
    /// or something else (`None`).
    pub fn set_zone(&self, zone: Option<Level>) {
        self.zone.set(zone);
    }

    /// Registers a button of the zone being drawn (or, with `menu`, an
    /// entry of a drop-down); true when its letters were typed.
    pub fn button(
        &self,
        ui: &Ui,
        resp: &egui::Response,
        label: &str,
        enabled: bool,
        kind: Kind,
        menu: bool,
    ) -> bool {
        // Chosen in this frame (the letters are gone already).
        if self.fire.get() == Some(resp.id) {
            self.fire.set(None);
            return true;
        }
        let Some(level) = self.level else { return false };
        let item_level = if menu { Some(Level::Menu) } else { self.zone.get() };
        if enabled && item_level == Some(level) && ui.is_rect_visible(resp.rect) && !label.is_empty() {
            self.frame.borrow_mut().push(Tip {
                id: resp.id,
                label: label.to_string(),
                rect: resp.rect,
                kind,
            });
        }
        false
    }

    /// The letters shown now: (label, letters).
    pub fn codes(&self) -> Vec<(String, String)> {
        self.shown.iter().map(|(t, c)| (t.label.clone(), c.clone())).collect()
    }
}

impl VersoloPdfApp {
    /// The keyboard letters shown now (label, letters) and their level.
    pub fn keytip_codes(&self) -> (Option<Level>, Vec<(String, String)>) {
        (self.keytips.level(), self.keytips.codes())
    }

    /// Before the frame's widgets: Alt / F10, typed letters, Esc.
    pub(super) fn keytips_input(&mut self, ctx: &Context) {
        let (alt, f10, other, focus_lost, pressed) = ctx.input(|i| {
            (
                i.modifiers.alt,
                i.key_pressed(Key::F10),
                i.events
                    .iter()
                    .any(|e| matches!(e, Event::Key { .. } | Event::Text(_) | Event::PointerButton { .. })),
                i.events.iter().any(|e| matches!(e, Event::WindowFocused(false))),
                i.pointer.any_pressed(),
            )
        });
        let k = &mut self.keytips;
        // Alt pressed and released alone (not Alt+key, not Alt+Tab).
        let mut toggle = f10;
        if alt && !k.alt_down {
            k.alt_clean = true;
        }
        if alt && (other || focus_lost) {
            k.alt_clean = false;
        }
        if !alt && k.alt_down && k.alt_clean {
            toggle = true;
        }
        k.alt_down = alt;
        if toggle {
            k.level = if k.level.is_some() { None } else { Some(Level::Tabs) };
            k.typed.clear();
            ctx.input_mut(|i| i.events.retain(|e| !matches!(e, Event::Key { key: Key::F10, .. })));
            ctx.request_repaint();
            return;
        }
        let Some(level) = k.level else { return };
        // A click ends the keyboard mode.
        if pressed || focus_lost {
            k.level = None;
            k.typed.clear();
            return;
        }
        let (texts, escape, back) = ctx.input(|i| {
            let texts: String = i
                .events
                .iter()
                .filter_map(|e| if let Event::Text(t) = e { Some(t.as_str()) } else { None })
                .collect();
            (texts, i.key_pressed(Key::Escape), i.key_pressed(Key::Backspace))
        });
        // The keys belong to the letters now, not to shortcuts or fields.
        ctx.input_mut(|i| i.events.retain(|e| !matches!(e, Event::Text(_) | Event::Key { .. })));
        if escape {
            k.typed.clear();
            k.level = match level {
                Level::Menu => {
                    egui::Popup::close_all(ctx);
                    k.menu_from.take().or(Some(Level::Commands))
                }
                Level::Commands => Some(Level::Tabs),
                Level::Tabs => None,
            };
            ctx.request_repaint();
            return;
        }
        if back {
            k.typed.pop();
        }
        for c in texts.chars().flat_map(char::to_uppercase) {
            k.typed.push(c);
            let typed = k.typed.clone();
            if let Some((tip, _)) = k.shown.iter().find(|(_, code)| *code == typed).cloned() {
                k.typed.clear();
                self.keytip_chosen(ctx, level, &tip);
                ctx.request_repaint();
                return;
            }
            if !k.shown.iter().any(|(_, code)| code.starts_with(&typed)) {
                k.typed.clear();
            }
        }
        ctx.request_repaint();
    }

    fn keytip_chosen(&mut self, ctx: &Context, level: Level, tip: &Tip) {
        match tip.kind {
            Kind::Tab(tab) => {
                if level == Level::Menu {
                    egui::Popup::close_all(ctx);
                }
                self.ribbon = tab;
                self.close_backstage();
                self.keytips.level = Some(Level::Commands);
                if self.settings.ribbon_collapsed {
                    self.ribbon_peek = true;
                }
            }
            Kind::File => {
                self.toggle_backstage();
                self.keytips.level = None;
            }
            Kind::Button => {
                self.keytips.fire.set(Some(tip.id));
                self.keytips.level = None;
            }
            Kind::MenuButton(popup) => {
                egui::Popup::open_id(ctx, popup);
                self.keytips.menu_from = Some(level);
                self.keytips.level = Some(Level::Menu);
            }
        }
    }

    /// After the frame's widgets: letters for the registered buttons and
    /// their labels on the screen.
    pub(super) fn keytips_paint(&mut self, ctx: &Context) {
        let k = &mut self.keytips;
        let items = std::mem::take(&mut *k.frame.borrow_mut());
        k.zone.set(None);
        let Some(level) = k.level else {
            k.shown.clear();
            return;
        };
        // A drop-down closed by other means: back to its level.
        if level == Level::Menu && items.is_empty() && !egui::Popup::is_any_open(ctx) {
            k.level = k.menu_from.take();
        }
        let labels: Vec<String> = items.iter().map(|t| t.label.clone()).collect();
        let codes = assign(&labels);
        k.shown = items.into_iter().zip(codes).collect();
        let painter = ctx.layer_painter(LayerId::new(Order::Tooltip, Id::unique("versolopdf-keytips")));
        let font = egui::FontId::proportional(12.0);
        for (tip, code) in &k.shown {
            if !code.starts_with(&k.typed) {
                continue;
            }
            let galley = painter.layout_no_wrap(code.clone(), font.clone(), Color32::WHITE);
            let size = galley.size() + egui::vec2(8.0, 4.0);
            // Menu entries: over the icon at the left; buttons: below the
            // middle.
            let center = if level == Level::Menu {
                egui::pos2(tip.rect.left() + size.x / 2.0 + 2.0, tip.rect.center().y)
            } else {
                egui::pos2(tip.rect.center().x, tip.rect.bottom() - size.y / 2.0 + 2.0)
            };
            let r = Rect::from_center_size(center, size);
            painter.rect(
                r,
                3.0,
                Color32::from_rgb(45, 45, 50),
                egui::Stroke::new(1.0, Color32::WHITE),
                egui::StrokeKind::Inside,
            );
            painter.galley(r.min + egui::vec2(4.0, 2.0), galley, Color32::WHITE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::assign;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn first_letters_then_others() {
        let c = assign(&s(&["Datei", "Start", "Ansicht", "Anmerkungen", "Schützen", "Überprüfen"]));
        assert_eq!(c, ["D", "S", "A", "N", "C", "Ü"]);
    }

    #[test]
    fn many_items_get_two_characters_without_clashes() {
        let labels: Vec<String> = (0..60).map(|i| format!("x{i}")).collect();
        let c = assign(&labels);
        for (i, a) in c.iter().enumerate() {
            assert!(!a.is_empty());
            for (j, b) in c.iter().enumerate() {
                if i != j {
                    assert!(!b.starts_with(a.as_str()), "{a} / {b}");
                }
            }
        }
    }
}
