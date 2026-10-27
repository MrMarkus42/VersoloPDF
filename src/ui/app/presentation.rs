//! "Vollbild": only the pages, one at a time, with page
//! transitions ("Seitenübergänge"), and the dialog to set the transitions.

use std::collections::HashMap;

use egui::{Color32, Context, Key, Painter, PointerButton, Pos2, Rect, TextureHandle, Ui, Vec2, pos2, vec2};

use super::dialogs::{Action, Outcome};
use super::page_dialogs::tr;
use super::{Dialog, MrPdfApp};
use crate::transitions::{Direction, Speed, Style, Transition};
use crate::ui::i18n::Language;
use crate::ui::pickers::{PageRange, section};

/// State of the "Seitenübergänge" dialog.
#[derive(Debug, Clone, PartialEq)]
pub struct TransState {
    pub transition: Transition,
    /// "Zufällig": a different effect for every page.
    pub random: bool,
    pub auto: bool,
    pub seconds: u32,
    pub range: PageRange,
}

/// The running presentation.
pub struct Presentation {
    tab_id: u64,
    pub page: usize,
    transitions: Vec<Transition>,
    /// Page picture per page at the current size.
    textures: HashMap<usize, TextureHandle>,
    size_px: [u32; 2],
    /// Running animation: previous page, start time, transition.
    from: Option<(usize, f64, Transition)>,
    /// Time the current page appeared.
    shown_at: f64,
    /// Last pointer movement (the navigation bar shows for a while).
    last_motion: f64,
}

impl Presentation {
    pub fn tab_id(&self) -> u64 {
        self.tab_id
    }
}

/// A pseudo random value in 0..1 for a cell (stable between frames).
fn cell_noise(x: usize, y: usize) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    (h % 10_000) as f32 / 10_000.0
}

fn full_uv() -> Rect {
    Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0))
}

/// Draws the part `part` (screen) of a page picture placed at `rect`.
fn draw_part(p: &Painter, tex: &TextureHandle, rect: Rect, part: Rect) {
    let part = part.intersect(rect);
    if part.width() <= 0.0 || part.height() <= 0.0 {
        return;
    }
    let uv = Rect::from_min_max(
        pos2((part.min.x - rect.min.x) / rect.width(), (part.min.y - rect.min.y) / rect.height()),
        pos2((part.max.x - rect.min.x) / rect.width(), (part.max.y - rect.min.y) / rect.height()),
    );
    p.image(tex.id(), part, uv, Color32::WHITE);
}

/// Unit vector of the movement of a transition.
fn movement(dir: Option<Direction>) -> Vec2 {
    match dir {
        Some(Direction::Right) => vec2(1.0, 0.0),
        Some(Direction::Left) => vec2(-1.0, 0.0),
        Some(Direction::Up) => vec2(0.0, -1.0),
        Some(Direction::DownRight) => vec2(1.0, 1.0),
        _ => vec2(0.0, 1.0),
    }
}

/// Draws the transition from `old` to `new` at progress `t` (0..1).
pub fn draw_transition(
    p: &Painter,
    rect: Rect,
    old: &TextureHandle,
    new: &TextureHandle,
    tr: &Transition,
    t: f32,
) {
    let t = t.clamp(0.0, 1.0);
    let dir = tr.direction_or_default();
    let size = rect.size();
    let clip = p.with_clip_rect(rect.intersect(p.clip_rect()));
    match tr.style {
        Style::None | Style::Replace => {
            p.image(new.id(), rect, full_uv(), Color32::WHITE);
        }
        Style::Fade => {
            p.image(old.id(), rect, full_uv(), Color32::WHITE);
            p.image(new.id(), rect, full_uv(), Color32::from_white_alpha((t * 255.0) as u8));
        }
        Style::Dissolve | Style::Glitter => {
            p.image(old.id(), rect, full_uv(), Color32::WHITE);
            let (nx, ny) = (32usize, 24usize);
            let m = movement(dir);
            for y in 0..ny {
                for x in 0..nx {
                    let noise = cell_noise(x, y);
                    let threshold = if tr.style == Style::Glitter {
                        // Sweep in the direction with some sparkle.
                        let fx = if m.x >= 0.0 { x as f32 / nx as f32 } else { 1.0 - x as f32 / nx as f32 };
                        let fy = if m.y >= 0.0 { y as f32 / ny as f32 } else { 1.0 - y as f32 / ny as f32 };
                        let along = (fx * m.x.abs() + fy * m.y.abs()) / (m.x.abs() + m.y.abs()).max(1.0);
                        0.75 * along + 0.25 * noise
                    } else {
                        noise
                    };
                    if threshold < t {
                        let cell = Rect::from_min_size(
                            rect.min + vec2(size.x * x as f32 / nx as f32, size.y * y as f32 / ny as f32),
                            vec2(size.x / nx as f32 + 0.5, size.y / ny as f32 + 0.5),
                        );
                        draw_part(p, new, rect, cell);
                    }
                }
            }
        }
        Style::Wipe => {
            p.image(old.id(), rect, full_uv(), Color32::WHITE);
            let part = match dir {
                Some(Direction::Right) => Rect::from_min_size(rect.min, vec2(size.x * t, size.y)),
                Some(Direction::Left) => {
                    Rect::from_min_max(pos2(rect.max.x - size.x * t, rect.min.y), rect.max)
                }
                Some(Direction::Up) => {
                    Rect::from_min_max(pos2(rect.min.x, rect.max.y - size.y * t), rect.max)
                }
                _ => Rect::from_min_size(rect.min, vec2(size.x, size.y * t)),
            };
            draw_part(p, new, rect, part);
        }
        Style::Push | Style::Cover | Style::Uncover => {
            let m = movement(dir);
            let shift = vec2(m.x * size.x, m.y * size.y);
            match tr.style {
                Style::Push => {
                    clip.image(old.id(), rect.translate(shift * t), full_uv(), Color32::WHITE);
                    clip.image(new.id(), rect.translate(shift * (t - 1.0)), full_uv(), Color32::WHITE);
                }
                Style::Cover => {
                    clip.image(old.id(), rect, full_uv(), Color32::WHITE);
                    clip.image(new.id(), rect.translate(shift * (t - 1.0)), full_uv(), Color32::WHITE);
                }
                _ => {
                    clip.image(new.id(), rect, full_uv(), Color32::WHITE);
                    clip.image(old.id(), rect.translate(shift * t), full_uv(), Color32::WHITE);
                }
            }
        }
        Style::Split => {
            p.image(old.id(), rect, full_uv(), Color32::WHITE);
            let horizontal = matches!(dir, Some(Direction::HorizontalIn | Direction::HorizontalOut));
            let inward = matches!(dir, Some(Direction::HorizontalIn | Direction::VerticalIn));
            let c = rect.center();
            if inward {
                // Two bands grow from the edges towards the middle.
                if horizontal {
                    let h = size.y * t / 2.0;
                    draw_part(p, new, rect, Rect::from_min_size(rect.min, vec2(size.x, h)));
                    draw_part(p, new, rect, Rect::from_min_max(pos2(rect.min.x, rect.max.y - h), rect.max));
                } else {
                    let w = size.x * t / 2.0;
                    draw_part(p, new, rect, Rect::from_min_size(rect.min, vec2(w, size.y)));
                    draw_part(p, new, rect, Rect::from_min_max(pos2(rect.max.x - w, rect.min.y), rect.max));
                }
            } else if horizontal {
                draw_part(p, new, rect, Rect::from_center_size(c, vec2(size.x, size.y * t)));
            } else {
                draw_part(p, new, rect, Rect::from_center_size(c, vec2(size.x * t, size.y)));
            }
        }
        Style::Blinds => {
            p.image(old.id(), rect, full_uv(), Color32::WHITE);
            let n = 8;
            let horizontal = dir != Some(Direction::Vertical);
            for i in 0..n {
                let part = if horizontal {
                    let band = size.y / n as f32;
                    Rect::from_min_size(rect.min + vec2(0.0, band * i as f32), vec2(size.x, band * t + 0.5))
                } else {
                    let band = size.x / n as f32;
                    Rect::from_min_size(rect.min + vec2(band * i as f32, 0.0), vec2(band * t + 0.5, size.y))
                };
                draw_part(p, new, rect, part);
            }
        }
        Style::Box => {
            if dir == Some(Direction::Inward) {
                p.image(new.id(), rect, full_uv(), Color32::WHITE);
                draw_part(p, old, rect, Rect::from_center_size(rect.center(), size * (1.0 - t)));
            } else {
                p.image(old.id(), rect, full_uv(), Color32::WHITE);
                draw_part(p, new, rect, Rect::from_center_size(rect.center(), size * t));
            }
        }
        Style::FlyIn | Style::FlyOut => {
            let m = movement(dir);
            let shift = vec2(m.x * size.x, m.y * size.y);
            if tr.style == Style::FlyIn {
                clip.image(old.id(), rect, full_uv(), Color32::WHITE);
                let r = Rect::from_center_size(rect.center(), size * (0.3 + 0.7 * t))
                    .translate(shift * (t - 1.0) * 0.5);
                clip.image(new.id(), r, full_uv(), Color32::WHITE);
            } else {
                clip.image(new.id(), rect, full_uv(), Color32::WHITE);
                let r =
                    Rect::from_center_size(rect.center(), size * (1.0 - 0.7 * t)).translate(shift * t * 0.5);
                clip.image(old.id(), r, full_uv(), Color32::from_white_alpha(((1.0 - t) * 255.0) as u8));
            }
        }
    }
}

impl MrPdfApp {
    fn ps_de(&self) -> bool {
        self.settings.language == Language::German
    }

    /// Opens "Seitenübergänge" for the active document.
    pub(super) fn open_transitions_dialog(&mut self) {
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        let current = tab.current_page;
        let t = tab.doc.transitions().ok().and_then(|v| v.get(current).copied()).unwrap_or_default();
        self.dialog = Some(Dialog::Transitions(Box::new(TransState {
            transition: t,
            random: false,
            auto: t.advance.is_some(),
            seconds: t.advance.map_or(5, |s| s.round().max(1.0) as u32),
            range: PageRange::all(),
        })));
    }

    pub(super) fn transitions_dialog(
        &mut self,
        ui: &mut Ui,
        s: &mut TransState,
    ) -> (Outcome, Option<Action>) {
        let de = self.ps_de();
        super::dialogs::set_title(ui, tr(de, "Seitenübergänge", "Page transitions"));
        let (count, current) = self.active_tab().map_or((0, 0), |t| (t.doc.page_count(), t.current_page));
        section(ui, tr(de, "Seitenübergang", "Page transition"), |ui| {
            egui::Grid::new("trans").num_columns(2).spacing(vec2(10.0, 4.0)).show(ui, |ui| {
                ui.label(tr(de, "Übergang:", "Transition:"));
                let current_name = if s.random {
                    tr(de, "Zufällig", "Random")
                } else {
                    let (g, e) = s.transition.style.names();
                    tr(de, g, e)
                };
                egui::ComboBox::from_id_salt("trans-style").selected_text(current_name).width(220.0).show_ui(
                    ui,
                    |ui| {
                        for style in Style::ALL {
                            let (g, e) = style.names();
                            if ui
                                .selectable_label(!s.random && s.transition.style == style, tr(de, g, e))
                                .clicked()
                            {
                                s.random = false;
                                s.transition.style = style;
                                s.transition.direction = None;
                            }
                        }
                        if ui.selectable_label(s.random, tr(de, "Zufällig", "Random")).clicked() {
                            s.random = true;
                        }
                    },
                );
                ui.end_row();
                let directions = if s.random { &[][..] } else { s.transition.style.directions() };
                ui.label(tr(de, "Richtung:", "Direction:"));
                ui.add_enabled_ui(!directions.is_empty(), |ui| {
                    let shown = s.transition.direction_or_default();
                    let text = shown.map_or("", |d| tr(de, d.names().0, d.names().1));
                    egui::ComboBox::from_id_salt("trans-dir").selected_text(text).width(220.0).show_ui(
                        ui,
                        |ui| {
                            for d in directions {
                                let (g, e) = d.names();
                                ui.selectable_value(&mut s.transition.direction, Some(*d), tr(de, g, e));
                            }
                        },
                    );
                });
                ui.end_row();
                ui.label(tr(de, "Dauer:", "Duration:"));
                let animated = s.random || !matches!(s.transition.style, Style::None | Style::Replace);
                ui.add_enabled_ui(animated, |ui| {
                    let (g, e) = s.transition.speed.names();
                    egui::ComboBox::from_id_salt("trans-speed")
                        .selected_text(tr(de, g, e))
                        .width(220.0)
                        .show_ui(ui, |ui| {
                            for sp in Speed::ALL {
                                let (g, e) = sp.names();
                                ui.selectable_value(&mut s.transition.speed, sp, tr(de, g, e));
                            }
                        });
                });
                ui.end_row();
            });
            ui.checkbox(
                &mut s.auto,
                tr(de, "Automatisch zur nächsten Seite nach", "Go to the next page after"),
            );
            ui.horizontal(|ui| {
                ui.add_space(24.0);
                ui.add_enabled(s.auto, egui::DragValue::new(&mut s.seconds).range(1..=3600));
                ui.label(tr(de, "Sekunden", "seconds"));
            });
        });
        s.range.ui(ui, de, count, current, &PageRange::ALL_MODES);
        let pages = s.range.resolve(count, current);
        let (ok, cancel, _) = super::page_dialogs::footer(ui, de, pages.is_some(), false, "OK");
        if cancel {
            return (Outcome::Close, None);
        }
        if !ok {
            return (Outcome::Keep, None);
        }
        let Some(pages) = pages else { return (Outcome::Keep, None) };
        let mut t = s.transition;
        t.advance = s.auto.then_some(s.seconds as f32);
        let random = s.random;
        (
            Outcome::Close,
            Some(Box::new(move |app, ctx| app.edit(ctx, |tab| tab.doc.set_transitions(&pages, t, random)))),
        )
    }

    /// Starts "Vollbild": the active document page by page, without the
    /// rest of the program.
    pub(super) fn start_presentation(&mut self, ctx: &Context) {
        let now = ctx.input(|i| i.time);
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        let transitions = tab.doc.transitions().unwrap_or_default();
        self.presentation = Some(Presentation {
            tab_id: tab.id,
            page: tab.current_page,
            transitions,
            textures: HashMap::new(),
            size_px: [0, 0],
            from: None,
            shown_at: now,
            last_motion: now,
        });
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
    }

    pub(super) fn stop_presentation(&mut self, ctx: &Context) {
        if let Some(p) = self.presentation.take()
            && let Some(tab) = self.tabs.iter_mut().find(|t| t.id == p.tab_id)
        {
            tab.go_to_page(p.page);
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
    }

    /// The transition shown when `page` appears.
    fn transition_for(&self, p: &Presentation, page: usize) -> Transition {
        let own = p.transitions.get(page).copied().unwrap_or_default();
        if self.settings.fs_always_default || own.style == Style::None {
            Transition { advance: own.advance, ..self.settings.fs_default_transition }
        } else {
            own
        }
    }

    fn presentation_go(&mut self, now: f64, page: usize) {
        let Some(p) = self.presentation.as_ref() else { return };
        if page == p.page {
            return;
        }
        let t = self.transition_for(p, page);
        let Some(p) = self.presentation.as_mut() else { return };
        p.from = (t.seconds() > 0.0).then_some((p.page, now, t));
        p.page = page;
        p.shown_at = now;
    }

    /// Draws the presentation (instead of the whole window).
    pub(super) fn presentation_ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        let Some(p) = self.presentation.as_ref() else { return };
        let Some(index) = self.tabs.iter().position(|t| t.id == p.tab_id) else {
            self.stop_presentation(&ctx);
            return;
        };
        let count = self.tabs[index].doc.page_count();
        let (page, from) = (p.page, p.from);
        // Navigation.
        let (next, prev, first, last, exit) = ctx.input(|i| {
            let pressed = |k: Key| i.key_pressed(k);
            let wheel = i.smooth_scroll_delta.y;
            (
                pressed(Key::ArrowRight)
                    || pressed(Key::ArrowDown)
                    || pressed(Key::PageDown)
                    || pressed(Key::Space)
                    || pressed(Key::Enter)
                    || wheel < -0.5,
                pressed(Key::ArrowLeft)
                    || pressed(Key::ArrowUp)
                    || pressed(Key::PageUp)
                    || pressed(Key::Backspace)
                    || wheel > 0.5,
                pressed(Key::Home),
                pressed(Key::End),
                pressed(Key::Escape),
            )
        });
        let rect = ui.max_rect();
        let response = ui.allocate_rect(rect, egui::Sense::click());
        if ui.input(|i| i.pointer.delta() != Vec2::ZERO)
            && let Some(p) = self.presentation.as_mut()
        {
            p.last_motion = now;
        }
        let bar_visible =
            self.settings.fs_nav_bar && now - self.presentation.as_ref().map_or(0.0, |p| p.last_motion) < 2.5;
        let mut nav: Option<i64> = None;
        if exit && self.settings.esc_exits_fullscreen {
            self.stop_presentation(&ctx);
            return;
        }
        let pointer_on_bar = bar_visible
            && ctx.input(|i| i.pointer.interact_pos()).is_some_and(|p| p.y > rect.bottom() - 60.0);
        if nav.is_none() && !pointer_on_bar {
            if next || response.clicked_by(PointerButton::Primary) {
                nav = Some(1);
            } else if prev || response.clicked_by(PointerButton::Secondary) {
                nav = Some(-1);
            }
        }
        if first {
            self.presentation_go(now, 0);
        } else if last {
            self.presentation_go(now, count.saturating_sub(1));
        } else if let Some(step) = nav {
            let target = page as i64 + step;
            if target >= 0 && (target as usize) < count {
                self.presentation_go(now, target as usize);
            } else if target >= count as i64 && self.settings.fs_loop {
                self.presentation_go(now, 0);
            }
        }
        // Automatic advance.
        if let Some(p) = self.presentation.as_ref() {
            let t = self.transition_for(p, p.page);
            let auto = t
                .advance
                .or((self.settings.fs_auto_seconds > 0).then_some(self.settings.fs_auto_seconds as f32));
            if let Some(secs) = auto {
                let due = now - p.shown_at >= secs as f64;
                if due {
                    let page = p.page;
                    if page + 1 < count {
                        self.presentation_go(now, page + 1);
                    } else if self.settings.fs_loop {
                        self.presentation_go(now, 0);
                    }
                } else {
                    ctx.request_repaint_after(std::time::Duration::from_secs_f64(
                        (secs as f64 - (now - p.shown_at)).max(0.05),
                    ));
                }
            }
        }
        // The page fitted into the screen.
        let Some(p) = self.presentation.as_ref() else { return };
        let page = p.page;
        let tab = &self.tabs[index];
        let Some(geo) = tab.display_geometry(page) else { return };
        let (w, h) = geo.view_size();
        let scale = (rect.width() / w.max(1.0)).min(rect.height() / h.max(1.0));
        let target = Rect::from_center_size(rect.center(), vec2(w * scale, h * scale));
        let ppp = ctx.pixels_per_point();
        let px =
            [((target.width() * ppp) as u32).clamp(1, 8192), ((target.height() * ppp) as u32).clamp(1, 8192)];
        let opts = tab.render_options();
        let mut wanted = vec![page];
        if let Some((old, _, _)) = from {
            wanted.push(old);
        }
        if page + 1 < count {
            wanted.push(page + 1);
        }
        let Some(p) = self.presentation.as_mut() else { return };
        if p.size_px != px {
            p.textures.clear();
            p.size_px = px;
        }
        for pg in wanted {
            if !p.textures.contains_key(&pg)
                && let Ok(img) = tab.doc.render_tile(pg, px[0], px[1], 0, 0, px[0], px[1], opts)
            {
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [img.width as usize, img.height as usize],
                    &img.rgba,
                );
                p.textures.insert(
                    pg,
                    ctx.load_texture(format!("mrpdf-show-{pg}"), image, egui::TextureOptions::LINEAR),
                );
            }
        }
        p.textures.retain(|k, _| (k + 2 >= page) && (*k <= page + 2));
        let bg = self.settings.fs_background;
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::from_rgb(bg[0], bg[1], bg[2]));
        let Some(new_tex) = p.textures.get(&page).cloned() else { return };
        match p.from {
            Some((old, start, t)) if (now - start) < t.seconds() as f64 => {
                let progress = ((now - start) / t.seconds() as f64) as f32;
                match p.textures.get(&old) {
                    Some(old_tex) => draw_transition(&painter, target, old_tex, &new_tex, &t, progress),
                    None => {
                        painter.image(new_tex.id(), target, full_uv(), Color32::WHITE);
                    }
                }
                ctx.request_repaint();
            }
            _ => {
                p.from = None;
                painter.image(new_tex.id(), target, full_uv(), Color32::WHITE);
            }
        }
        // Navigation bar (shown for a while after moving the mouse).
        if bar_visible {
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
            let de = self.ps_de();
            let bar = Rect::from_center_size(pos2(rect.center().x, rect.bottom() - 34.0), vec2(260.0, 36.0));
            ui.painter().rect_filled(bar, 6.0, Color32::from_black_alpha(170));
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(bar.shrink(4.0))
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            child.visuals_mut().override_text_color = Some(Color32::WHITE);
            let mut step = 0i64;
            if child.button("◀").on_hover_text(tr(de, "Vorherige Seite", "Previous page")).clicked() {
                step = -1;
            }
            child.label(format!("{} / {count}", page + 1));
            if child.button("▶").on_hover_text(tr(de, "Nächste Seite", "Next page")).clicked() {
                step = 1;
            }
            if child.button(tr(de, "Beenden", "Exit")).clicked() {
                self.stop_presentation(&ctx);
                return;
            }
            let target = page as i64 + step;
            if step != 0 && target >= 0 && (target as usize) < count {
                self.presentation_go(now, target as usize);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn noise_is_spread() {
        let values: Vec<f32> = (0..100).map(|i| super::cell_noise(i % 10, i / 10)).collect();
        let low = values.iter().filter(|v| **v < 0.5).count();
        assert!((30..=70).contains(&low), "{low}");
    }
}
