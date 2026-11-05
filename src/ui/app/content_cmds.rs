//! "Text bearbeiten" / "Objekte bearbeiten":
//! running the content commands and canvas changes, the keyboard, the
//! group "Objekte" of the Start tab and the contextual tabs "Format" and
//! "Anordnen".

use egui::{Context, RichText, Ui};

use super::VersoloPdfApp;
use super::bookmark_dialogs::color_button;
use super::commands::Cmd;
use crate::content::{
    Affine, Align, Arrange, BlendMode, ContentObject, FontChoice, ObjectFilter, ObjectStyle, ZOrder, arrange,
    text_blocks,
};
use crate::document::{ClipItem, ObjectClip};
use crate::geometry::RectF;
use crate::model::Rgba;
use crate::ui::content_state::{ContentCmd, ContentOp, TextEditing, union};
use crate::ui::i18n::Language;
use crate::ui::icons::Icon;
use crate::ui::tools::Tool;
use crate::ui::widgets::{big_menu_button, group};

/// Values of the "Format" tab for the selected objects.
#[derive(Debug, Clone, PartialEq)]
pub struct FormatUi {
    /// Selection the values were read from: (tab, page, objects, revision).
    pub key: (u64, usize, Vec<usize>, u64),
    pub fill: Rgba,
    pub stroke: Rgba,
    pub width: f32,
    pub fill_opacity: f32,
    pub stroke_opacity: f32,
    pub dash: u8,
    pub blend: BlendMode,
    /// "Umwandeln": X, Y, width, height (points).
    pub frame: [f32; 4],
}

fn tr(de: bool, g: &'static str, e: &'static str) -> &'static str {
    if de { g } else { e }
}

impl VersoloPdfApp {
    fn ct_de(&self) -> bool {
        self.settings.language == Language::German
    }

    /// Page and selected objects of the content tools.
    fn content_selection(&self) -> Option<(usize, Vec<usize>)> {
        let tab = self.active_tab()?;
        let c = &tab.content;
        (!c.selected.is_empty() && c.revision == tab.doc.revision()).then(|| (c.page, c.selected.clone()))
    }

    fn selected_objects(&mut self) -> Vec<ContentObject> {
        let Some((page, sel)) = self.content_selection() else { return Vec::new() };
        let Some(tab) = self.tab() else { return Vec::new() };
        tab.objects(page).iter().filter(|o| sel.contains(&o.index)).cloned().collect()
    }

    pub(super) fn content_enabled(&self, cmd: ContentCmd) -> bool {
        use ContentCmd::*;
        let Some(tab) = self.active_tab() else { return false };
        let has_sel = self.content_selection().is_some();
        match cmd {
            Paste => self.object_clip.as_ref().is_some_and(|c| !c.is_empty()),
            SelectAll | SelectKind(_) | Filter(_) | AddText | AddImage | AlignToPage => true,
            Arrange(crate::content::Arrange::SpaceH | crate::content::Arrange::SpaceV) => {
                tab.content.selected.len() >= 2 && has_sel
            }
            Arrange(
                crate::content::Arrange::DistLeft
                | crate::content::Arrange::DistCenterH
                | crate::content::Arrange::DistRight
                | crate::content::Arrange::DistTop
                | crate::content::Arrange::DistCenterV
                | crate::content::Arrange::DistBottom,
            ) => tab.content.selected.len() >= 3 && has_sel,
            Arrange(
                crate::content::Arrange::SameSize
                | crate::content::Arrange::SameWidth
                | crate::content::Arrange::SameHeight,
            ) => tab.content.selected.len() >= 2 && has_sel,
            ExportImage | ReplaceImage => has_sel && tab.content.selected.len() == 1,
            _ => has_sel,
        }
    }

    pub(super) fn content_selected(&self, cmd: ContentCmd) -> bool {
        match cmd {
            ContentCmd::Filter(f) => {
                self.tool == Tool::EditObjects && self.active_tab().is_some_and(|t| t.content.filter == f)
            }
            ContentCmd::AlignToPage => self.align_to_page,
            _ => false,
        }
    }

    /// Runs a change of the page content as one undo step; `f` returns the
    /// objects to select afterwards.
    fn content_edit(
        &mut self,
        ctx: &Context,
        page: usize,
        f: impl FnOnce(&mut crate::Document) -> crate::Result<Vec<usize>>,
    ) {
        self.edit(ctx, |tab| {
            let new = f(&mut tab.doc)?;
            let rev = tab.doc.revision();
            tab.content.select(page, new, rev);
            Ok(())
        });
    }

    pub(super) fn run_content(&mut self, ctx: &Context, cmd: ContentCmd) {
        use ContentCmd::*;
        let de = self.ct_de();
        let sel = self.content_selection();
        let current = self.active_tab().map_or(0, |t| t.current_page);
        match cmd {
            Filter(f) => {
                if let Some(tab) = self.tab() {
                    tab.content.filter = f;
                }
                self.select_tool(Tool::EditObjects);
            }
            SelectAll | SelectKind(_) => {
                let filter = match cmd {
                    SelectKind(f) => f,
                    _ => self.active_tab().map_or(ObjectFilter::All, |t| t.content.filter),
                };
                if self.tool != Tool::EditObjects {
                    self.select_tool(Tool::EditObjects);
                }
                if let Some(tab) = self.tab() {
                    let page = tab.current_page;
                    let all: Vec<usize> = tab
                        .objects(page)
                        .iter()
                        .filter(|o| filter.accepts(o.kind) && !o.invisible)
                        .map(|o| o.index)
                        .collect();
                    let rev = tab.doc.revision();
                    tab.content.select(page, all, rev);
                }
            }
            Deselect => {
                if let Some(tab) = self.tab() {
                    tab.content.clear();
                }
            }
            AlignToPage => self.align_to_page = !self.align_to_page,
            AddText => self.add_content_text(ctx, current),
            AddImage => self.add_content_image(ctx, current),
            Paste => {
                let page = sel.as_ref().map_or(current, |s| s.0);
                self.paste_count += 1;
                let d = 10.0 * self.paste_count as f32;
                self.paste_clip(ctx, page, (d, -d));
            }
            _ => {
                let Some((page, indices)) = sel else { return };
                match cmd {
                    Copy | Cut => {
                        let rect = union(self.selected_objects().iter().map(|o| o.rect));
                        match self.active_tab().map(|t| t.doc.copy_objects(page, &indices)) {
                            Some(Ok(clip)) => {
                                let n = clip.items.len();
                                self.object_clip = Some(clip);
                                self.clip_rect = rect;
                                self.paste_count = 0;
                                if cmd == Cut {
                                    self.content_edit(ctx, page, |doc| {
                                        doc.delete_objects(page, &indices).map(|_| Vec::new())
                                    });
                                }
                                self.set_status(
                                    ctx,
                                    if de {
                                        format!("{n} Objekte kopiert")
                                    } else {
                                        format!("{n} objects copied")
                                    },
                                );
                            }
                            Some(Err(e)) => self.show_error(&e),
                            None => {}
                        }
                    }
                    Delete => self.content_edit(ctx, page, |doc| {
                        doc.delete_objects(page, &indices).map(|_| Vec::new())
                    }),
                    Duplicate => self
                        .content_edit(ctx, page, |doc| doc.duplicate_objects(page, &indices, (12.0, -12.0))),
                    Z(how) => self.content_edit(ctx, page, |doc| doc.reorder_objects(page, &indices, how)),
                    Arrange(how) => {
                        let objs = self.selected_objects();
                        let rects: Vec<RectF> = objs.iter().map(|o| o.rect).collect();
                        let page_box = self.active_tab().and_then(|t| t.doc.geometry(page)).map(|g| g.crop);
                        let to_page = self.align_to_page || rects.len() == 1;
                        let reference = match (to_page, page_box) {
                            (true, Some(b)) => b,
                            _ => union(rects.iter().copied()).unwrap_or_default(),
                        };
                        let moves: Vec<(usize, Affine)> =
                            objs.iter().map(|o| o.index).zip(arrange(&rects, how, reference)).collect();
                        self.content_edit(ctx, page, |doc| {
                            doc.transform_objects_each(page, &moves).map(|_| indices.clone())
                        });
                    }
                    RotateLeft | RotateRight | Rotate180 | FlipH | FlipV => {
                        let Some(r) = union(self.selected_objects().iter().map(|o| o.rect)) else { return };
                        let m = match cmd {
                            RotateLeft => Affine::rotate(90.0),
                            RotateRight => Affine::rotate(-90.0),
                            Rotate180 => Affine::rotate(180.0),
                            FlipH => Affine::scale(-1.0, 1.0),
                            _ => Affine::scale(1.0, -1.0),
                        };
                        let m = Affine::about(m, r.center());
                        self.content_edit(ctx, page, |doc| {
                            doc.transform_objects(page, &indices, m).map(|_| indices.clone())
                        });
                    }
                    ExportImage => self.export_object_image(ctx, page, indices[0]),
                    ReplaceImage => {
                        let Some(path) = rfd::FileDialog::new()
                            .add_filter(tr(de, "Bilder", "Images"), &super::page_dialogs::IMAGE_EXT)
                            .pick_file()
                        else {
                            return;
                        };
                        match image::open(&path) {
                            Ok(img) => {
                                let i = indices[0];
                                self.content_edit(ctx, page, |doc| {
                                    doc.replace_object_image(page, i, &img).map(|_| vec![i])
                                });
                            }
                            Err(e) => self.set_status(ctx, e.to_string()),
                        }
                    }
                    EditText => self.open_selected_text(),
                    _ => {}
                }
            }
        }
    }

    /// Opens the text block of the selected text object in the editor.
    fn open_selected_text(&mut self) {
        if self.tool != Tool::EditText {
            self.select_tool(Tool::EditText);
        }
        let Some(tab) = self.tab() else { return };
        if tab.content.text.is_some() {
            return;
        }
        let page = tab.content.page;
        let Some(first) = tab.content.selected.first().copied() else { return };
        let objects = tab.objects(page);
        if let Some(block) = text_blocks(&objects).into_iter().find(|b| b.objects.contains(&first)) {
            let width = if block.lines.len() > 1 { block.rect.width() } else { 0.0 };
            let rev = tab.doc.revision();
            tab.content.select(page, block.objects.clone(), rev);
            tab.content.text = Some(TextEditing {
                page,
                text: block.text(),
                style: crate::content::TextStyle::of_block(&block),
                width,
                block,
                focus_requested: false,
            });
        }
    }

    fn paste_clip(&mut self, ctx: &Context, page: usize, offset: (f32, f32)) {
        let Some(clip) = self.object_clip.clone() else { return };
        if self.tool != Tool::EditObjects {
            self.select_tool(Tool::EditObjects);
        }
        self.content_edit(ctx, page, |doc| doc.paste_objects(page, &clip, offset));
    }

    fn add_content_text(&mut self, ctx: &Context, page: usize) {
        let Some(geo) = self.active_tab().and_then(|t| t.doc.geometry(page)) else { return };
        let c = geo.crop.center();
        let de = self.ct_de();
        let clip = ObjectClip {
            items: vec![ClipItem::Text {
                text: tr(de, "Neuer Text", "New text").into(),
                font: "Helvetica".into(),
                size: 14.0,
                color: Rgba::BLACK,
                bold: false,
                italic: false,
                serif: false,
                mono: false,
                matrix: Affine::translate(c.x - 40.0, c.y),
            }],
        };
        self.select_tool(Tool::EditText);
        self.content_edit(ctx, page, |doc| doc.paste_objects(page, &clip, (0.0, 0.0)));
        if let Some(tab) = self.tab() {
            tab.current_page = page;
        }
        self.open_selected_text();
    }

    fn add_content_image(&mut self, ctx: &Context, page: usize) {
        let de = self.ct_de();
        let Some(path) = rfd::FileDialog::new()
            .add_filter(tr(de, "Bilder", "Images"), &super::page_dialogs::IMAGE_EXT)
            .pick_file()
        else {
            return;
        };
        let img = match image::open(&path) {
            Ok(i) => i,
            Err(e) => {
                self.set_status(ctx, e.to_string());
                return;
            }
        };
        let Some(geo) = self.active_tab().and_then(|t| t.doc.geometry(page)) else { return };
        let page_box = geo.crop;
        // 96 dpi, at most 60 % of the page.
        let (mut w, mut h) = (img.width() as f32 * 0.75, img.height() as f32 * 0.75);
        let k = (page_box.width() * 0.6 / w).min(page_box.height() * 0.6 / h).min(1.0);
        w *= k;
        h *= k;
        let c = page_box.center();
        let clip = ObjectClip {
            items: vec![ClipItem::Image {
                image: img,
                matrix: Affine([w, 0.0, 0.0, h, c.x - w / 2.0, c.y - h / 2.0]),
            }],
        };
        self.paste_new(ctx, page, clip);
    }

    fn paste_new(&mut self, ctx: &Context, page: usize, clip: ObjectClip) {
        if self.tool != Tool::EditObjects {
            self.select_tool(Tool::EditObjects);
        }
        self.content_edit(ctx, page, |doc| doc.paste_objects(page, &clip, (0.0, 0.0)));
    }

    fn export_object_image(&mut self, ctx: &Context, page: usize, index: usize) {
        let de = self.ct_de();
        let img = match self.active_tab().map(|t| t.doc.object_image(page, index)) {
            Some(Ok(i)) => i,
            Some(Err(e)) => return self.show_error(&e),
            None => return,
        };
        let Some(path) = rfd::FileDialog::new()
            .add_filter("PNG", &["png"])
            .add_filter("JPEG", &["jpg", "jpeg"])
            .set_file_name(format!("{}-{}.png", tr(de, "Bild", "Image"), index + 1))
            .save_file()
        else {
            return;
        };
        let result = if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("jpg") || e.eq_ignore_ascii_case("jpeg"))
        {
            img.to_rgb8().save(&path)
        } else {
            img.save(&path)
        };
        match result {
            Ok(()) => {
                self.set_status(ctx, format!("{}{}", tr(de, "Gespeichert: ", "Saved: "), path.display()))
            }
            Err(e) => self.set_status(ctx, e.to_string()),
        }
    }

    /// A change asked for by the canvas.
    pub(super) fn content_op(&mut self, ctx: &Context, op: ContentOp) {
        match op {
            ContentOp::Command(cmd) => self.run_content(ctx, cmd),
            ContentOp::Transform { page, moves } => {
                let keep: Vec<usize> = moves.iter().map(|(i, _)| *i).collect();
                self.content_edit(ctx, page, |doc| doc.transform_objects_each(page, &moves).map(|_| keep));
            }
            ContentOp::ReplaceText { page, objects, text, style, left, baseline, width, leading } => {
                self.content_edit(ctx, page, |doc| {
                    doc.replace_text_block(page, &objects, &text, &style, left, baseline, width, leading)
                });
            }
            ContentOp::PasteAt { page, at } => {
                let offset = self.clip_rect.map_or((0.0, 0.0), |r| (at.x - r.x0, at.y - r.y1));
                self.paste_clip(ctx, page, offset);
            }
        }
    }

    /// Keys of "Objekte bearbeiten"; true when one was used.
    pub(super) fn content_shortcuts(&mut self, ctx: &Context) -> bool {
        use egui::{Key, KeyboardShortcut, Modifiers};
        if self.tool != Tool::EditObjects || ctx.egui_wants_keyboard_input() || self.active_tab().is_none() {
            return false;
        }
        let cmd = Modifiers::COMMAND;
        let pressed =
            |m: Modifiers, k: Key| ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(m, k)));
        for (k, c) in [
            (Key::X, ContentCmd::Cut),
            (Key::C, ContentCmd::Copy),
            (Key::V, ContentCmd::Paste),
            (Key::D, ContentCmd::Duplicate),
            (Key::A, ContentCmd::SelectAll),
        ] {
            if pressed(cmd, k) {
                self.run(ctx, Cmd::Content(c));
                return true;
            }
        }
        if self.content_selection().is_none() {
            return false;
        }
        if pressed(Modifiers::NONE, Key::Delete) || pressed(Modifiers::NONE, Key::Backspace) {
            self.run(ctx, Cmd::Content(ContentCmd::Delete));
            return true;
        }
        if pressed(Modifiers::NONE, Key::Escape) {
            self.run(ctx, Cmd::Content(ContentCmd::Deselect));
            return true;
        }
        let shift = ctx.input(|i| i.modifiers.shift);
        let step = if shift { 10.0 } else { 1.0 };
        let mut d = (0.0, 0.0);
        for (k, dx, dy) in [
            (Key::ArrowLeft, -1.0, 0.0),
            (Key::ArrowRight, 1.0, 0.0),
            (Key::ArrowUp, 0.0, 1.0),
            (Key::ArrowDown, 0.0, -1.0),
        ] {
            let mods = if shift { Modifiers::SHIFT } else { Modifiers::NONE };
            if pressed(mods, k) {
                d = (d.0 + dx * step, d.1 + dy * step);
            }
        }
        if d != (0.0, 0.0)
            && let Some((page, indices)) = self.content_selection()
        {
            // The view may be rotated: arrows move on screen.
            let (dx, dy) = self
                .active_tab()
                .and_then(|t| t.display_geometry(page))
                .map_or(d, |g| g.view_delta_to_pdf(d.0, -d.1));
            let m = Affine::translate(dx, dy);
            self.content_op(
                ctx,
                ContentOp::Transform { page, moves: indices.iter().map(|i| (*i, m)).collect() },
            );
            return true;
        }
        false
    }

    /// Group "Objekte" of the Start tab.
    pub(super) fn ribbon_objects_group(&mut self, ui: &mut Ui, ctx: &Context) {
        let de = self.ct_de();
        group(ui, tr(de, "Objekte", "Objects"), |ui| {
            self.big(ui, ctx, Cmd::Tool(Tool::EditText));
            // "Objekte bearbeiten ▾": the button selects the tool, the menu the filter.
            let selected = self.tool == Tool::EditObjects;
            let label = tr(de, "Objekte bearbeiten", "Edit objects");
            let resp = big_menu_button(ui, Icon::EditObjects, label, !self.tabs.is_empty(), selected);
            self.popup_menu(
                ctx,
                &resp,
                &[ObjectFilter::All, ObjectFilter::Text, ObjectFilter::Images, ObjectFilter::Shapes]
                    .map(|f| super::ribbon::Entry::C(Cmd::Content(ContentCmd::Filter(f)))),
            );
            self.big_menu(
                ui,
                ctx,
                Icon::InsertFile,
                ("Hinzufügen", "Add"),
                &[
                    super::ribbon::Entry::C(Cmd::Content(ContentCmd::AddText)),
                    super::ribbon::Entry::C(Cmd::Content(ContentCmd::AddImage)),
                ],
            );
            self.big_menu(
                ui,
                ctx,
                Icon::Pointer,
                ("Auswahl", "Selection"),
                &[
                    super::ribbon::Entry::C(Cmd::Content(ContentCmd::Cut)),
                    super::ribbon::Entry::C(Cmd::Content(ContentCmd::Copy)),
                    super::ribbon::Entry::C(Cmd::Content(ContentCmd::Paste)),
                    super::ribbon::Entry::C(Cmd::Content(ContentCmd::Delete)),
                    super::ribbon::Entry::Sep,
                    super::ribbon::Entry::C(Cmd::Content(ContentCmd::Duplicate)),
                    super::ribbon::Entry::C(Cmd::Content(ContentCmd::SelectAll)),
                    super::ribbon::Entry::C(Cmd::Content(ContentCmd::Deselect)),
                ],
            );
        });
    }

    /// Whether the contextual tabs "Format" / "Anordnen" of the content
    /// tools are shown.
    pub(super) fn content_tabs(&self) -> bool {
        matches!(self.tool, Tool::EditObjects | Tool::EditText)
            && self.active_tab().is_some_and(|t| t.content.text.is_some() || !t.content.selected.is_empty())
    }

    fn format_ui_for(&mut self) -> Option<FormatUi> {
        let tab_id = self.active_tab()?.id;
        let (page, sel) = self.content_selection()?;
        let rev = self.active_tab()?.doc.revision();
        let key = (tab_id, page, sel, rev);
        if let Some(f) = self.content_format.as_ref().filter(|f| f.key == key) {
            return Some(f.clone());
        }
        let objs = self.selected_objects();
        let first = objs.first()?;
        let r = union(objs.iter().map(|o| o.rect)).unwrap_or_default();
        let f = FormatUi {
            key,
            fill: first.fill.unwrap_or(Rgba::BLACK),
            stroke: first.stroke.unwrap_or(Rgba::BLACK),
            width: first.stroke_width,
            fill_opacity: first.fill.map_or(100.0, |c| f32::from(c.a) / 2.55),
            stroke_opacity: first.stroke.map_or(100.0, |c| f32::from(c.a) / 2.55),
            dash: 0,
            blend: BlendMode::Normal,
            frame: [r.x0, r.y0, r.width(), r.height()],
        };
        self.content_format = Some(f.clone());
        Some(f)
    }

    fn apply_style(&mut self, ctx: &Context, style: ObjectStyle) {
        let Some((page, indices)) = self.content_selection() else { return };
        self.content_edit(ctx, page, |doc| {
            doc.style_objects(page, &indices, &style).map(|_| indices.clone())
        });
    }

    /// Contextual tab "Format" of the content tools.
    pub(super) fn ribbon_content_format(&mut self, ui: &mut Ui, ctx: &Context) {
        let de = self.ct_de();
        let editing = self.active_tab().and_then(|t| t.content.text.clone());
        let Some(mut f) = self.format_ui_for().or_else(|| {
            editing.as_ref().map(|e| FormatUi {
                key: (0, 0, Vec::new(), 0),
                fill: e.style.color,
                stroke: Rgba::BLACK,
                width: 0.0,
                fill_opacity: 100.0,
                stroke_opacity: 100.0,
                dash: 0,
                blend: BlendMode::Normal,
                frame: [0.0; 4],
            })
        }) else {
            return;
        };
        let mut style = ObjectStyle::default();
        let old = f.clone();
        group(ui, tr(de, "Format", "Format"), |ui| {
            ui.vertical(|ui| {
                ui.label(tr(de, "Füllfarbe", "Fill"));
                color_button(ui, &mut f.fill);
            });
            ui.add_enabled_ui(editing.is_none(), |ui| {
                ui.vertical(|ui| {
                    ui.label(tr(de, "Linienfarbe", "Line"));
                    color_button(ui, &mut f.stroke);
                });
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(tr(de, "Breite:", "Width:"));
                        let r = ui.add(
                            egui::DragValue::new(&mut f.width).range(0.0..=72.0).speed(0.1).suffix(" pt"),
                        );
                        if r.drag_stopped() || r.lost_focus() {
                            style.stroke_width = Some(f.width);
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(tr(de, "Rahmen:", "Border:"));
                        let names = [
                            tr(de, "Durchgehend", "Solid"),
                            tr(de, "Gestrichelt", "Dashed"),
                            tr(de, "Gepunktet", "Dotted"),
                        ];
                        egui::ComboBox::from_id_salt("content-dash")
                            .selected_text(names[f.dash as usize])
                            .width(110.0)
                            .show_ui(ui, |ui| {
                                for (i, n) in names.iter().enumerate() {
                                    if ui.selectable_value(&mut f.dash, i as u8, *n).clicked() {
                                        style.dash = Some(match i {
                                            1 => vec![4.0, 3.0],
                                            2 => vec![1.0, 2.0],
                                            _ => Vec::new(),
                                        });
                                    }
                                }
                            });
                    });
                });
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(tr(de, "Deckkraft:", "Opacity:"));
                        let r =
                            ui.add(egui::DragValue::new(&mut f.fill_opacity).range(0.0..=100.0).suffix(" %"));
                        if r.drag_stopped() || r.lost_focus() {
                            style.fill_opacity = Some(f.fill_opacity / 100.0);
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(tr(de, "Rahmentransparenz:", "Line opacity:"));
                        let r = ui
                            .add(egui::DragValue::new(&mut f.stroke_opacity).range(0.0..=100.0).suffix(" %"));
                        if r.drag_stopped() || r.lost_focus() {
                            style.stroke_opacity = Some(f.stroke_opacity / 100.0);
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(tr(de, "Mischmodus:", "Blend mode:"));
                        egui::ComboBox::from_id_salt("content-blend")
                            .selected_text(f.blend.label(de))
                            .width(120.0)
                            .show_ui(ui, |ui| {
                                for b in BlendMode::ALL {
                                    if ui.selectable_value(&mut f.blend, b, b.label(de)).clicked() {
                                        style.blend = Some(b);
                                    }
                                }
                            });
                    });
                });
            });
        });
        if f.fill != old.fill {
            match &editing {
                Some(_) => {
                    if let Some(t) = self.tab().and_then(|t| t.content.text.as_mut()) {
                        t.style.color = f.fill;
                    }
                }
                None => style.fill = Some(f.fill),
            }
        }
        if f.stroke != old.stroke {
            style.stroke = Some(f.stroke);
        }
        if editing.is_none() {
            self.content_format = Some(f);
        }
        if style != ObjectStyle::default() {
            self.apply_style(ctx, style);
        }
        self.text_format_group(ui);
    }

    /// "Textformat": font, size, bold, italic, alignment of the open text.
    fn text_format_group(&mut self, ui: &mut Ui) {
        let de = self.ct_de();
        let Some(tab) = self.tab() else { return };
        let Some(edit) = tab.content.text.as_mut() else {
            group(ui, tr(de, "Textformat", "Text format"), |ui| {
                ui.label(
                    RichText::new(tr(de, "Text anklicken, um ihn zu bearbeiten", "Click text to edit it"))
                        .weak(),
                );
            });
            return;
        };
        let s = &mut edit.style;
        group(ui, tr(de, "Textformat", "Text format"), |ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("content-font")
                        .selected_text(s.font.label(de))
                        .width(150.0)
                        .show_ui(ui, |ui| {
                            for f in FontChoice::ALL {
                                let label = f.label(de);
                                ui.selectable_value(&mut s.font, f, label);
                            }
                        });
                    ui.add(egui::DragValue::new(&mut s.size).range(2.0..=400.0).speed(0.2).suffix(" pt"));
                });
                ui.horizontal(|ui| {
                    ui.toggle_value(&mut s.bold, RichText::new("B").strong())
                        .on_hover_text(tr(de, "Fett", "Bold"));
                    ui.toggle_value(&mut s.italic, RichText::new("I").italics())
                        .on_hover_text(tr(de, "Kursiv", "Italic"));
                    ui.separator();
                    for (a, label, tip) in [
                        (Align::Left, "⬅", tr(de, "Linksbündig", "Align left")),
                        (Align::Center, "↔", tr(de, "Zentriert", "Centre")),
                        (Align::Right, "➡", tr(de, "Rechtsbündig", "Align right")),
                    ] {
                        if ui.selectable_label(s.align == a, label).on_hover_text(tip).clicked() {
                            s.align = a;
                        }
                    }
                });
            });
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(tr(
                        de,
                        "Übernehmen: Strg+Enter oder Klick daneben",
                        "Apply: Ctrl+Enter or click outside",
                    ))
                    .weak()
                    .small(),
                );
                ui.label(RichText::new(tr(de, "Verwerfen: Esc", "Discard: Esc")).weak().small());
            });
        });
        // Bold/italic need another font than the original.
        if (s.bold != edit.block.bold || s.italic != edit.block.italic) && s.font == FontChoice::Original {
            s.font = if edit.block.monospace {
                FontChoice::Courier
            } else if edit.block.serif {
                FontChoice::Times
            } else {
                FontChoice::Helvetica
            };
        }
    }

    /// Contextual tab "Anordnen".
    pub(super) fn ribbon_arrange(&mut self, ui: &mut Ui, ctx: &Context) {
        let de = self.ct_de();
        let mut run: Option<ContentCmd> = None;
        // Small buttons with their own icon (the last argument is a fallback
        // for commands without one).
        let mut btn = |app: &VersoloPdfApp, ui: &mut Ui, c: ContentCmd, label: (&str, &str), _icon: Icon| {
            let text = if de { label.0 } else { label.1 };
            let (g, e) = c.names();
            let resp = crate::ui::widgets::small_button(
                ui,
                c.icon(),
                text,
                app.content_selected(c),
                app.content_enabled(c),
            );
            if resp.on_hover_text(if de { g } else { e }).clicked() {
                run = Some(c);
            }
        };
        group(ui, tr(de, "Ausrichtung", "Align"), |ui| {
            ui.vertical(|ui| {
                btn(self, ui, ContentCmd::Arrange(Arrange::Left), ("Links", "Left"), Icon::Margins);
                btn(self, ui, ContentCmd::Arrange(Arrange::CenterH), ("Mittig", "Centre"), Icon::Margins);
                btn(self, ui, ContentCmd::Arrange(Arrange::Right), ("Rechts", "Right"), Icon::Margins);
            });
            ui.vertical(|ui| {
                btn(self, ui, ContentCmd::Arrange(Arrange::Top), ("Oben", "Top"), Icon::Margins);
                btn(self, ui, ContentCmd::Arrange(Arrange::CenterV), ("Mittig", "Middle"), Icon::Margins);
                btn(self, ui, ContentCmd::Arrange(Arrange::Bottom), ("Unten", "Bottom"), Icon::Margins);
            });
            btn(self, ui, ContentCmd::AlignToPage, ("An Seite", "To page"), Icon::SinglePage);
        });
        group(ui, tr(de, "Verteilen", "Distribute"), |ui| {
            ui.vertical(|ui| {
                btn(
                    self,
                    ui,
                    ContentCmd::Arrange(Arrange::SpaceH),
                    ("Abstand horiz.", "Space horiz."),
                    Icon::Resize,
                );
                btn(
                    self,
                    ui,
                    ContentCmd::Arrange(Arrange::SpaceV),
                    ("Abstand vert.", "Space vert."),
                    Icon::Resize,
                );
            });
            ui.vertical(|ui| {
                btn(self, ui, ContentCmd::Arrange(Arrange::DistLeft), ("Links", "Left"), Icon::Margins);
                btn(
                    self,
                    ui,
                    ContentCmd::Arrange(Arrange::DistCenterH),
                    ("Zentriert", "Centre"),
                    Icon::Margins,
                );
                btn(self, ui, ContentCmd::Arrange(Arrange::DistRight), ("Rechts", "Right"), Icon::Margins);
            });
            ui.vertical(|ui| {
                btn(self, ui, ContentCmd::Arrange(Arrange::DistTop), ("Oben", "Top"), Icon::Margins);
                btn(self, ui, ContentCmd::Arrange(Arrange::DistCenterV), ("Mittig", "Middle"), Icon::Margins);
                btn(self, ui, ContentCmd::Arrange(Arrange::DistBottom), ("Unten", "Bottom"), Icon::Margins);
            });
        });
        group(ui, tr(de, "Größe", "Size"), |ui| {
            ui.vertical(|ui| {
                btn(
                    self,
                    ui,
                    ContentCmd::Arrange(Arrange::SameSize),
                    ("Selbe Größe", "Same size"),
                    Icon::Resize,
                );
                btn(
                    self,
                    ui,
                    ContentCmd::Arrange(Arrange::SameWidth),
                    ("Selbe Breite", "Same width"),
                    Icon::Resize,
                );
                btn(
                    self,
                    ui,
                    ContentCmd::Arrange(Arrange::SameHeight),
                    ("Selbe Höhe", "Same height"),
                    Icon::Resize,
                );
            });
        });
        group(ui, tr(de, "Drehen", "Rotate"), |ui| {
            ui.vertical(|ui| {
                btn(
                    self,
                    ui,
                    ContentCmd::RotateLeft,
                    ("90° gegen Uhrzeigersinn", "90° counter-clockwise"),
                    Icon::RotateLeft,
                );
                btn(
                    self,
                    ui,
                    ContentCmd::RotateRight,
                    ("90° im Uhrzeigersinn", "90° clockwise"),
                    Icon::RotateRight,
                );
                btn(self, ui, ContentCmd::Rotate180, ("Um 180° drehen", "Rotate 180°"), Icon::RotateRight);
            });
        });
        group(ui, tr(de, "Spiegeln", "Flip"), |ui| {
            ui.vertical(|ui| {
                btn(self, ui, ContentCmd::FlipH, ("Horizontal", "Horizontal"), Icon::Swap);
                btn(self, ui, ContentCmd::FlipV, ("Vertikal", "Vertical"), Icon::Swap);
            });
        });
        group(ui, "Z-Order", |ui| {
            ui.vertical(|ui| {
                btn(self, ui, ContentCmd::Z(ZOrder::Forward), ("Nach vorn", "Forward"), Icon::MoveUp);
                btn(self, ui, ContentCmd::Z(ZOrder::Front), ("In den Vordergrund", "To front"), Icon::MoveUp);
            });
            ui.vertical(|ui| {
                btn(self, ui, ContentCmd::Z(ZOrder::Backward), ("Nach hinten", "Backward"), Icon::MoveDown);
                btn(self, ui, ContentCmd::Z(ZOrder::Back), ("In den Hintergrund", "To back"), Icon::MoveDown);
            });
        });
        // "Umwandeln": position and size of the selection.
        if let Some(mut f) = self.format_ui_for() {
            let old = f.frame;
            let mut apply = false;
            group(ui, tr(de, "Umwandeln", "Transform"), |ui| {
                egui::Grid::new("content-frame").num_columns(4).spacing([4.0, 4.0]).show(ui, |ui| {
                    for (i, label) in ["X:", "Y:", tr(de, "B:", "W:"), tr(de, "H:", "H:")].iter().enumerate()
                    {
                        ui.label(*label);
                        let r = ui.add(
                            egui::DragValue::new(&mut f.frame[i])
                                .speed(0.5)
                                .range(if i >= 2 { 0.5..=20000.0 } else { -20000.0..=20000.0 })
                                .suffix(" pt"),
                        );
                        apply |= r.drag_stopped() || r.lost_focus();
                        if i % 2 == 1 {
                            ui.end_row();
                        }
                    }
                });
            });
            self.content_format = Some(f.clone());
            if apply
                && f.frame != old
                && let Some((page, indices)) = self.content_selection()
            {
                let from = RectF::new(old[0], old[1], old[0] + old[2], old[1] + old[3]);
                let to = RectF::new(f.frame[0], f.frame[1], f.frame[0] + f.frame[2], f.frame[1] + f.frame[3]);
                let m = crate::content::fit_rect(&from, &to);
                self.content_edit(ctx, page, |doc| {
                    doc.transform_objects(page, &indices, m).map(|_| indices.clone())
                });
            }
        }
        group(ui, tr(de, "Bearbeiten", "Edit"), |ui| {
            self.big(ui, ctx, Cmd::Content(ContentCmd::Duplicate));
            self.big(ui, ctx, Cmd::Content(ContentCmd::Delete));
        });
        if let Some(c) = run {
            self.run(ctx, Cmd::Content(c));
        }
    }
}
