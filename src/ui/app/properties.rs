//! Right pane: style of the current tool or properties of the selection.

use egui::{Panel, RichText, Ui};

use super::VersoloPdfApp;
use super::commands::Cmd;
use crate::model::{AnnotKind, AnnotStyle, Rgba};
use crate::pdf_edit::AnnotUpdate;
use crate::ui::i18n::Strings;
use crate::ui::icons::Icon;
use crate::ui::tools::{PALETTE, Tool};
use crate::ui::widgets::{from_color32, icon_button, swatch, to_color32};

/// Colour palette plus custom colour button. Returns the picked colour.
fn color_row(ui: &mut Ui, current: Rgba) -> Option<Rgba> {
    let mut picked = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for c in PALETTE {
            if swatch(ui, c, c == current).clicked() {
                picked = Some(c);
            }
        }
        let mut color = to_color32(current);
        if ui.color_edit_button_srgba(&mut color).changed() {
            picked = Some(from_color32(color));
        }
    });
    picked
}

pub(super) fn style_editor(ui: &mut Ui, t: &Strings, tool: Tool, style: &mut AnnotStyle) -> bool {
    let mut changed = false;
    ui.label(t.color);
    if let Some(c) = color_row(ui, style.color) {
        style.color = c;
        changed = true;
    }
    if tool.has_fill() {
        let mut fill = style.fill.is_some();
        if ui.checkbox(&mut fill, t.fill).changed() {
            style.fill = fill.then_some(style.fill.unwrap_or(Rgba::rgb(255, 255, 210)));
            changed = true;
        }
        if let Some(current) = style.fill
            && let Some(c) = color_row(ui, current)
        {
            style.fill = Some(c);
            changed = true;
        }
    }
    if tool.has_width() {
        ui.label(t.line_width);
        changed |=
            ui.add(egui::Slider::new(&mut style.width, 0.0..=20.0).step_by(0.5).suffix(" pt")).changed();
    }
    ui.label(t.opacity);
    let mut pct = (style.opacity * 100.0).round();
    if ui.add(egui::Slider::new(&mut pct, 10.0..=100.0).suffix(" %").max_decimals(0)).changed() {
        style.opacity = pct / 100.0;
        changed = true;
    }
    if tool == Tool::TextBox {
        ui.label(t.font_size);
        changed |= ui.add(egui::Slider::new(&mut style.font_size, 6.0..=72.0).suffix(" pt")).changed();
    }
    changed
}

impl VersoloPdfApp {
    pub(super) fn properties_panel(&mut self, ui: &mut Ui) {
        let t = self.t();
        Panel::right("properties").resizable(true).default_size(230.0).size_range(180.0..=420.0).show(
            ui,
            |ui| {
                ui.horizontal(|ui| {
                    ui.strong(t.properties);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icon_button(ui, Icon::Close, t.close, false, true).clicked() {
                            self.settings.show_properties = false;
                        }
                    });
                });
                ui.separator();
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    let has_selection = self.active_tab().is_some_and(|t| t.selected_annot.is_some());
                    if has_selection {
                        self.selection_properties(ui);
                    } else {
                        self.tool_properties(ui);
                    }
                });
            },
        );
    }

    fn tool_properties(&mut self, ui: &mut Ui) {
        let t = self.t();
        let tool = self.tool;
        ui.label(RichText::new(super::sidebar::kind_name_for_tool(self.settings.language, tool)).strong());
        if tool.annot_kind().is_none() {
            ui.label(RichText::new(t.tool_hint(tool)).weak());
            return;
        }
        ui.label(RichText::new(t.tool_hint(tool)).weak().small());
        ui.add_space(6.0);
        let mut style = self.settings.style(tool);
        if style_editor(ui, t, tool, &mut style) {
            self.settings.set_style(tool, style);
        }
        ui.add_space(6.0);
        ui.label(t.author);
        ui.text_edit_singleline(&mut self.settings.author);
    }

    fn selection_properties(&mut self, ui: &mut Ui) {
        let t = self.t();
        let ctx = ui.ctx().clone();
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        let Some(info) = tab.selected_annotation() else {
            tab.selected_annot = None;
            return;
        };
        let details = tab.selected_details();
        let rev = tab.doc.revision();
        let sel = tab.selected_annot.expect("checked above");
        if info.kind == AnnotKind::Widget {
            let field = tab
                .doc
                .form_fields(sel.page)
                .ok()
                .and_then(|f| f.into_iter().find(|f| f.annot_index == sel.index));
            self.field_properties(ui, &ctx, sel.page, field);
            return;
        }
        ui.label(RichText::new(super::sidebar::kind_name(t, info.kind)).strong());
        ui.label(format!("{} {}", t.page, info.page + 1));
        if !info.author.is_empty() {
            ui.label(format!("{}: {}", t.author, info.author));
        }
        if !info.modified.is_empty() {
            ui.label(RichText::new(&info.modified).weak().small());
        }
        ui.separator();

        // Contents editor, re-initialised whenever the selection changes.
        let needs_init = !matches!(&self.props_contents, Some((r, v, _)) if *r == sel && *v == rev);
        if needs_init {
            self.props_contents = Some((sel, rev, info.contents.clone()));
        }
        let mut update = AnnotUpdate::default();
        if info.kind != AnnotKind::Stamp
            && let Some((_, _, text)) = self.props_contents.as_mut()
        {
            ui.label(if info.kind == AnnotKind::FreeText { t.text } else { t.comment_text });
            ui.add(egui::TextEdit::multiline(text).desired_rows(4).desired_width(f32::INFINITY));
            if ui.add_enabled(*text != info.contents, egui::Button::new(t.apply)).clicked() {
                update.contents = Some(text.clone());
            }
            ui.separator();
        }

        if let Some(d) = details.filter(|d| d.restylable) {
            let tool = match info.kind {
                AnnotKind::Square => Tool::Rectangle,
                AnnotKind::Circle => Tool::Ellipse,
                AnnotKind::Line => Tool::Line,
                AnnotKind::Ink | AnnotKind::Polyline | AnnotKind::Polygon => Tool::Pencil,
                AnnotKind::FreeText => Tool::TextBox,
                _ => Tool::Highlight,
            };
            let stored = AnnotStyle {
                color: d.color.unwrap_or(Rgba::BLACK),
                fill: d.fill,
                width: d.width,
                opacity: d.opacity,
                font_size: d.font_size,
                author: String::new(),
            };
            // The edited style lives across frames so sliders can be dragged
            // and are applied once on release (one undo step per change).
            if !matches!(&self.props_style, Some((r, v, _)) if *r == sel && *v == rev) {
                self.props_style = Some((sel, rev, stored.clone()));
            }
            if let Some((_, _, style)) = self.props_style.as_mut() {
                style_editor(ui, t, tool, style);
                let dragging = ctx.input(|i| i.pointer.any_down());
                if style.color != stored.color {
                    update.color = Some(style.color);
                }
                if style.fill != stored.fill {
                    update.fill = Some(style.fill);
                }
                if !dragging {
                    if (style.width - stored.width).abs() > f32::EPSILON {
                        update.width = Some(style.width);
                    }
                    if (style.opacity - stored.opacity).abs() > f32::EPSILON {
                        update.opacity = Some(style.opacity);
                    }
                    if (style.font_size - stored.font_size).abs() > f32::EPSILON {
                        update.font_size = Some(style.font_size);
                    }
                }
            }
        } else {
            ui.label(RichText::new(t.not_restylable).weak().small());
        }
        ui.add_space(8.0);
        if ui.button(t.delete_comment).clicked() {
            self.delete_selected_annotation(&ctx);
            return;
        }
        if update != AnnotUpdate::default() {
            self.update_selected(&ctx, update);
        }
    }

    /// Properties of a selected form field (form designer).
    fn field_properties(
        &mut self,
        ui: &mut Ui,
        ctx: &egui::Context,
        page: usize,
        field: Option<crate::model::FormFieldInfo>,
    ) {
        use crate::model::FieldKind;
        let t = self.t();
        let de = self.settings.language == crate::ui::i18n::Language::German;
        ui.label(RichText::new(if de { "Formularfeld" } else { "Form field" }).strong());
        ui.label(format!("{} {}", t.page, page + 1));
        if let Some(f) = field {
            let kind = match f.kind {
                FieldKind::Text { .. } => Cmd::Tool(Tool::FieldText),
                FieldKind::CheckBox => Cmd::Tool(Tool::FieldCheckBox),
                FieldKind::RadioButton => Cmd::Tool(Tool::FieldRadio),
                FieldKind::ComboBox => Cmd::Tool(Tool::FieldCombo),
                FieldKind::ListBox => Cmd::Tool(Tool::FieldList),
                FieldKind::PushButton => Cmd::Tool(Tool::FieldButton),
                FieldKind::Signature => Cmd::Tool(Tool::FieldSignature),
                FieldKind::Unknown => Cmd::Tool(Tool::SelectFields),
            };
            ui.label(format!("{}: {}", if de { "Typ" } else { "Type" }, kind.label(self.settings.language)));
            ui.label(format!("Name: {}", f.name));
            if !f.value.is_empty() {
                ui.label(format!("{}: {}", if de { "Wert" } else { "Value" }, f.value));
            }
            if !f.options.is_empty() {
                ui.label(format!("{}: {}", if de { "Einträge" } else { "Items" }, f.options.join(", ")));
            }
        }
        ui.add_space(4.0);
        ui.label(RichText::new(t.tool_hint(Tool::SelectFields)).weak().small());
        ui.add_space(8.0);
        if ui.button(if de { "Feld löschen" } else { "Delete field" }).clicked() {
            self.delete_selected_annotation(ctx);
        }
    }
}
