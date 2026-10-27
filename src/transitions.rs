//! Page transitions ("Seitenübergänge", PDF `/Trans` and `/Dur` of a page)
//! as used by the full screen presentation.

use lopdf::{Dictionary, Object};
use serde::{Deserialize, Serialize};

use crate::Result;
use crate::pdf_edit::{Edit, guarded, number, real};

/// Effect of a transition (the names; PDF style in brackets).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Style {
    /// "Kein Übergangseffekt" (no /Trans entry).
    #[default]
    None,
    /// "Aufdecken" (Uncover).
    Uncover,
    /// "Auflösen" (Dissolve).
    Dissolve,
    /// "Aufteilen" (Split).
    Split,
    /// "Ausfliegen" (Fly, outward).
    FlyOut,
    /// "Box".
    Box,
    /// "Deckblatt" (Cover).
    Cover,
    /// "Einfliegen" (Fly, inward).
    FlyIn,
    /// "Ersetzen" (R).
    Replace,
    /// "Glitzern" (Glitter).
    Glitter,
    /// "Kamm" (Blinds).
    Blinds,
    /// "Stoßen" (Push).
    Push,
    /// "Überblenden" (Fade).
    Fade,
    /// "Wischen" (Wipe).
    Wipe,
}

impl Style {
    /// All effects in the order of the list.
    pub const ALL: [Style; 14] = [
        Style::None,
        Style::Uncover,
        Style::Dissolve,
        Style::Split,
        Style::FlyOut,
        Style::Box,
        Style::Cover,
        Style::FlyIn,
        Style::Replace,
        Style::Glitter,
        Style::Blinds,
        Style::Push,
        Style::Fade,
        Style::Wipe,
    ];

    pub fn names(self) -> (&'static str, &'static str) {
        match self {
            Style::None => ("Kein Übergangseffekt", "No transition"),
            Style::Uncover => ("Aufdecken", "Uncover"),
            Style::Dissolve => ("Auflösen", "Dissolve"),
            Style::Split => ("Aufteilen", "Split"),
            Style::FlyOut => ("Ausfliegen", "Fly out"),
            Style::Box => ("Box", "Box"),
            Style::Cover => ("Deckblatt", "Cover"),
            Style::FlyIn => ("Einfliegen", "Fly in"),
            Style::Replace => ("Ersetzen", "Replace"),
            Style::Glitter => ("Glitzern", "Glitter"),
            Style::Blinds => ("Kamm", "Blinds"),
            Style::Push => ("Stoßen", "Push"),
            Style::Fade => ("Überblenden", "Fade"),
            Style::Wipe => ("Wischen", "Wipe"),
        }
    }

    fn pdf_name(self) -> &'static [u8] {
        match self {
            Style::None | Style::Replace => b"R",
            Style::Uncover => b"Uncover",
            Style::Dissolve => b"Dissolve",
            Style::Split => b"Split",
            Style::FlyOut | Style::FlyIn => b"Fly",
            Style::Box => b"Box",
            Style::Cover => b"Cover",
            Style::Glitter => b"Glitter",
            Style::Blinds => b"Blinds",
            Style::Push => b"Push",
            Style::Fade => b"Fade",
            Style::Wipe => b"Wipe",
        }
    }

    /// The directions this effect offers.
    pub fn directions(self) -> &'static [Direction] {
        use Direction::*;
        match self {
            Style::Wipe | Style::Push | Style::Cover | Style::Uncover | Style::FlyIn | Style::FlyOut => {
                &[Down, Up, Left, Right]
            }
            Style::Glitter => &[Right, Down, DownRight],
            Style::Split => &[HorizontalIn, HorizontalOut, VerticalIn, VerticalOut],
            Style::Blinds => &[Horizontal, Vertical],
            Style::Box => &[Inward, Outward],
            _ => &[],
        }
    }
}

/// Direction of a transition (PDF `/Di`, `/Dm`, `/M`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Direction {
    /// "Ab": from the top downwards (270°).
    Down,
    /// "Auf" (90°).
    Up,
    /// "Links": towards the left (180°).
    Left,
    /// "Rechts" (0°).
    Right,
    /// "Rechts unten" (315°, Glitter).
    DownRight,
    HorizontalIn,
    HorizontalOut,
    VerticalIn,
    VerticalOut,
    Horizontal,
    Vertical,
    Inward,
    Outward,
}

impl Direction {
    pub fn names(self) -> (&'static str, &'static str) {
        match self {
            Direction::Down => ("Ab", "Down"),
            Direction::Up => ("Auf", "Up"),
            Direction::Left => ("Links", "Left"),
            Direction::Right => ("Rechts", "Right"),
            Direction::DownRight => ("Rechts unten", "Down right"),
            Direction::HorizontalIn => ("Horizontal nach innen", "Horizontal inward"),
            Direction::HorizontalOut => ("Horizontal nach außen", "Horizontal outward"),
            Direction::VerticalIn => ("Vertikal nach innen", "Vertical inward"),
            Direction::VerticalOut => ("Vertikal nach außen", "Vertical outward"),
            Direction::Horizontal => ("Horizontal", "Horizontal"),
            Direction::Vertical => ("Vertikal", "Vertical"),
            Direction::Inward => ("Nach innen", "Inward"),
            Direction::Outward => ("Nach außen", "Outward"),
        }
    }

    fn degrees(self) -> Option<i64> {
        Some(match self {
            Direction::Right => 0,
            Direction::Up => 90,
            Direction::Left => 180,
            Direction::Down => 270,
            Direction::DownRight => 315,
            _ => return None,
        })
    }
}

/// "Dauer" of the effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Speed {
    /// "Langsam" (2 s).
    #[default]
    Slow,
    /// "Mittel" (1 s).
    Medium,
    /// "Schnell" (0,5 s).
    Fast,
}

impl Speed {
    pub const ALL: [Speed; 3] = [Speed::Slow, Speed::Medium, Speed::Fast];

    pub fn seconds(self) -> f32 {
        match self {
            Speed::Slow => 2.0,
            Speed::Medium => 1.0,
            Speed::Fast => 0.5,
        }
    }

    pub fn names(self) -> (&'static str, &'static str) {
        match self {
            Speed::Slow => ("Langsam", "Slow"),
            Speed::Medium => ("Mittel", "Medium"),
            Speed::Fast => ("Schnell", "Fast"),
        }
    }

    fn from_seconds(s: f32) -> Speed {
        if s >= 1.5 {
            Speed::Slow
        } else if s >= 0.75 {
            Speed::Medium
        } else {
            Speed::Fast
        }
    }
}

/// The transition shown when a page appears.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Transition {
    pub style: Style,
    pub direction: Option<Direction>,
    pub speed: Speed,
    /// "Automatisch zur nächsten Seite nach n Sekunden" (PDF `/Dur`).
    pub advance: Option<f32>,
}

impl Transition {
    /// Length of the animation in seconds.
    pub fn seconds(&self) -> f32 {
        match self.style {
            Style::None | Style::Replace => 0.0,
            _ => self.speed.seconds(),
        }
    }

    /// The direction used for drawing (the first offered one by default).
    pub fn direction_or_default(&self) -> Option<Direction> {
        let offered = self.style.directions();
        self.direction.filter(|d| offered.contains(d)).or_else(|| offered.first().copied())
    }

    fn to_dict(self) -> Dictionary {
        let mut d = Dictionary::new();
        d.set("Type", Object::Name(b"Trans".to_vec()));
        d.set("S", Object::Name(self.style.pdf_name().to_vec()));
        d.set("D", real(self.speed.seconds()));
        let dir = self.direction_or_default();
        match (self.style, dir) {
            (Style::Split, Some(d0)) => {
                let horizontal = matches!(d0, Direction::HorizontalIn | Direction::HorizontalOut);
                let inward = matches!(d0, Direction::HorizontalIn | Direction::VerticalIn);
                d.set("Dm", Object::Name(if horizontal { b"H".to_vec() } else { b"V".to_vec() }));
                d.set("M", Object::Name(if inward { b"I".to_vec() } else { b"O".to_vec() }));
            }
            (Style::Blinds, Some(d0)) => {
                d.set(
                    "Dm",
                    Object::Name(if d0 == Direction::Horizontal { b"H".to_vec() } else { b"V".to_vec() }),
                );
            }
            (Style::Box, Some(d0)) => {
                d.set("M", Object::Name(if d0 == Direction::Inward { b"I".to_vec() } else { b"O".to_vec() }));
            }
            (_, Some(d0)) => {
                if let Some(deg) = d0.degrees() {
                    d.set("Di", Object::Integer(deg));
                }
                if matches!(self.style, Style::FlyIn | Style::FlyOut) {
                    d.set(
                        "M",
                        Object::Name(if self.style == Style::FlyIn { b"I".to_vec() } else { b"O".to_vec() }),
                    );
                }
            }
            _ => {}
        }
        d
    }

    fn from_dict(d: &Dictionary) -> Transition {
        let name = |key: &[u8]| d.get(key).ok().and_then(|o| o.as_name().ok()).map(<[u8]>::to_vec);
        let inward = name(b"M").as_deref() == Some(b"I");
        let style = match name(b"S").as_deref() {
            Some(b"Uncover") => Style::Uncover,
            Some(b"Dissolve") => Style::Dissolve,
            Some(b"Split") => Style::Split,
            Some(b"Fly") if inward => Style::FlyIn,
            Some(b"Fly") => Style::FlyOut,
            Some(b"Box") => Style::Box,
            Some(b"Cover") => Style::Cover,
            Some(b"Glitter") => Style::Glitter,
            Some(b"Blinds") => Style::Blinds,
            Some(b"Push") => Style::Push,
            Some(b"Fade") => Style::Fade,
            Some(b"Wipe") => Style::Wipe,
            _ => Style::Replace,
        };
        let horizontal = name(b"Dm").as_deref() != Some(b"V");
        let degrees = d.get(b"Di").ok().and_then(number).map(|v| v.round() as i64);
        let direction = match style {
            Style::Split => Some(match (horizontal, inward) {
                (true, true) => Direction::HorizontalIn,
                (true, false) => Direction::HorizontalOut,
                (false, true) => Direction::VerticalIn,
                (false, false) => Direction::VerticalOut,
            }),
            Style::Blinds => Some(if horizontal { Direction::Horizontal } else { Direction::Vertical }),
            Style::Box => Some(if inward { Direction::Inward } else { Direction::Outward }),
            _ => match degrees {
                Some(0) => Some(Direction::Right),
                Some(90) => Some(Direction::Up),
                Some(180) => Some(Direction::Left),
                Some(270) => Some(Direction::Down),
                Some(315) => Some(Direction::DownRight),
                _ => None,
            },
        };
        let speed = Speed::from_seconds(d.get(b"D").ok().and_then(number).unwrap_or(1.0));
        Transition { style, direction, speed, advance: None }
    }
}

/// Reads the transitions of all pages.
pub fn read(bytes: &[u8], password: Option<&str>) -> Result<Vec<Transition>> {
    guarded(|| {
        let edit = Edit::open(bytes, password)?;
        let mut out = Vec::new();
        for id in edit.page_ids() {
            let page = edit.object(id)?.as_dict()?;
            let mut t = match page.get(b"Trans") {
                Ok(obj) => edit.resolve(obj)?.as_dict().map(Transition::from_dict).unwrap_or_default(),
                Err(_) => Transition::default(),
            };
            t.advance = page.get(b"Dur").ok().and_then(number).filter(|v| v.is_finite() && *v > 0.0);
            out.push(t);
        }
        Ok(out)
    })
}

/// Sets the transition of `pages` (Style::None removes it). `random`
/// gives every page a different effect ("Zufällig").
pub fn write(
    bytes: &[u8],
    password: Option<&str>,
    pages: &[usize],
    t: Transition,
    random: bool,
) -> Result<Vec<u8>> {
    guarded(|| {
        let mut edit = Edit::open(bytes, password)?;
        let effects = &Style::ALL[1..];
        for (n, &p) in pages.iter().enumerate() {
            let id = edit.page_id(p)?;
            let mut t = t;
            if random {
                // A simple scramble is enough to vary the effects.
                t.style = effects[(p * 7 + n * 3 + 5) % effects.len()];
                t.direction = None;
            }
            let page = edit.dict_mut(id)?;
            if t.style == Style::None {
                page.remove(b"Trans");
            } else {
                page.set("Trans", Object::Dictionary(t.to_dict()));
            }
            match t.advance {
                Some(s) if s.is_finite() && s > 0.0 => page.set("Dur", real(s.min(3600.0))),
                _ => {
                    page.remove(b"Dur");
                }
            }
        }
        edit.finish()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dictionaries_round_trip() {
        for style in Style::ALL.into_iter().skip(1) {
            for dir in style.directions().iter().copied().map(Some).chain([None]) {
                let t = Transition { style, direction: dir, speed: Speed::Fast, advance: None };
                let back = Transition::from_dict(&t.to_dict());
                assert_eq!(back.style, style, "{t:?}");
                assert_eq!(back.direction_or_default(), t.direction_or_default(), "{t:?}");
                assert_eq!(back.speed, Speed::Fast);
            }
        }
        assert_eq!(Transition::default().seconds(), 0.0);
    }
}
