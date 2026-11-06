//! Right pane: style of the current tool or properties of the selection.

use egui::{Panel, RichText, Ui};

use super::VersoloPdfApp;
use super::commands::Cmd;
use crate::model::{AnnotKind, AnnotStyle, Rgba};
use crate::pdf_edit::AnnotUpdate;
use crate::ui::i18n::Strings;
use crate::ui::icons::Icon;
use crate::ui::tab::AnnotRef;
use crate::ui::tools::{PALETTE, Tool};
use crate::ui::widgets::{from_color32, icon_button, swatch, to_color32};

/// Colour palette plus custom colour button. Returns the picked colour.
pub(super) fn color_row(ui: &mut Ui, current: Rgba) -> Option<Rgba> {
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
    if tool.has_font() {
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
                    // Page content chosen with "Objekte bearbeiten" / "Text bearbeiten".
                    let content = matches!(self.tool, Tool::EditObjects | Tool::EditText)
                        && self.active_tab().is_some_and(|t| {
                            t.content.text.is_some()
                                || (!t.content.selected.is_empty() && t.content.revision == t.doc.revision())
                        });
                    let ctx = ui.ctx().clone();
                    if content {
                        self.content_properties(ui, &ctx);
                        if has_selection {
                            ui.separator();
                        }
                    }
                    if has_selection {
                        self.selection_properties(ui);
                    } else if !content {
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

    /// Properties of a selected form field (form designer): name, quick
    /// info, required, text given in advance, font, size, colours, items.
    fn field_properties(
        &mut self,
        ui: &mut Ui,
        ctx: &egui::Context,
        page: usize,
        field: Option<crate::model::FormFieldInfo>,
    ) {
        use crate::forms::{FieldFont, PropsKind};
        use crate::model::FieldKind;
        let t = self.t();
        let de = self.settings.language == crate::ui::i18n::Language::German;
        let tr = |g: &'static str, e: &'static str| if de { g } else { e };
        ui.label(RichText::new(tr("Formularfeld", "Form field")).strong());
        let Some(f) = field else {
            ui.label(format!("{} {}", t.page, page + 1));
            return;
        };
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
        ui.label(format!("{} – {} {}", kind.label(self.settings.language), t.page, page + 1));

        // The values being edited, read again when the field or the document changes.
        let sel = AnnotRef { page, index: f.annot_index };
        let (tab_id, rev) = self.active_tab().map_or((0, 0), |t| (t.id, t.doc.revision()));
        if !self.props_field.as_ref().is_some_and(|e| e.tab == tab_id && e.sel == sel && e.rev == rev) {
            // Another field was clicked while a change was still being typed:
            // it belongs to the field it was made for.
            if let Some(old) = self.props_field.take()
                && old.tab == tab_id
                && old.rev == rev
                && old.edit != old.orig
                && old.failed.as_ref() != Some(&old.edit)
            {
                let props = old.edit.clone();
                self.edit(ctx, |tab| tab.doc.set_field_props(old.sel.page, old.sel.index, &props));
            }
            let rev = self.active_tab().map_or(0, |t| t.doc.revision());
            let read = self.tab().map(|t| t.doc.field_props(page, f.annot_index));
            self.props_field = match read {
                Some(Ok(p)) => Some(FieldEditor::new(tab_id, sel, rev, p)),
                _ => None,
            };
        }
        let Some(mut e) = self.props_field.take() else { return };
        let p = &mut e.edit;
        ui.add_space(4.0);
        egui::Grid::new("field-props").num_columns(2).spacing([6.0, 4.0]).show(ui, |ui| {
            ui.label(if p.kind == PropsKind::Radio { tr("Gruppe:", "Group:") } else { "Name:" });
            ui.add(egui::TextEdit::singleline(&mut p.name).desired_width(f32::INFINITY));
            ui.end_row();
            ui.label(tr("QuickInfo:", "Tooltip:")).on_hover_text(tr(
                "Erscheint, wenn die Maus auf dem Feld ruht.",
                "Shown when the pointer rests on the field.",
            ));
            ui.add(
                egui::TextEdit::singleline(&mut p.tooltip)
                    .hint_text(tr("z. B. Ihr Nachname", "e.g. your last name"))
                    .desired_width(f32::INFINITY),
            );
            ui.end_row();
        });
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut p.required, tr("Pflichtfeld", "Required"));
            ui.checkbox(&mut p.read_only, tr("Schreibgeschützt", "Read only"));
        });
        match p.kind {
            PropsKind::Text => {
                ui.label(tr("Vorgabetext:", "Default text:")).on_hover_text(tr(
                    "Steht im Feld, bis jemand etwas anderes einträgt; „Formular zurücksetzen“ stellt ihn wieder her.",
                    "Shown in the field until someone types something else; \"Reset form\" restores it.",
                ));
                ui.add(egui::TextEdit::singleline(&mut p.default_value).desired_width(f32::INFINITY));
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut p.multiline, tr("Mehrzeilig", "Multi-line"));
                    ui.checkbox(&mut p.password, tr("Kennwort (Sternchen)", "Password (asterisks)"));
                });
                ui.horizontal(|ui| {
                    ui.label(tr("Höchstens Zeichen:", "Max. characters:"));
                    ui.add(egui::DragValue::new(&mut p.max_len).range(0..=10_000));
                    ui.label(RichText::new(tr("(0 = beliebig)", "(0 = any)")).weak().small());
                });
            }
            PropsKind::ComboBox | PropsKind::ListBox => {
                ui.label(tr("Einträge (einer pro Zeile):", "Items (one per line):"));
                ui.add(
                    egui::TextEdit::multiline(&mut e.options).desired_rows(4).desired_width(f32::INFINITY),
                );
                p.options =
                    e.options.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect();
                ui.horizontal(|ui| {
                    ui.label(tr("Vorauswahl:", "Preselected:"));
                    let shown =
                        if p.default_value.is_empty() { "–".to_string() } else { p.default_value.clone() };
                    egui::ComboBox::from_id_salt("field-props-default")
                        .selected_text(shown)
                        .width(140.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut p.default_value, String::new(), "–");
                            for o in p.options.clone() {
                                ui.selectable_value(&mut p.default_value, o.clone(), o);
                            }
                        });
                });
            }
            _ => {}
        }
        if p.kind.has_text_look() {
            ui.add_space(4.0);
            ui.label(RichText::new(tr("Schrift", "Font")).strong());
            egui::ComboBox::from_id_salt("field-props-font")
                .selected_text(p.font.label())
                .width(170.0)
                .show_ui(ui, |ui| {
                    for font in FieldFont::ALL {
                        ui.selectable_value(&mut p.font, font, font.label());
                    }
                });
            ui.horizontal(|ui| {
                ui.label(tr("Größe:", "Size:"));
                ui.add(
                    egui::DragValue::new(&mut p.font_size)
                        .range(0.0..=144.0)
                        .speed(0.2)
                        .max_decimals(1)
                        .suffix(" pt"),
                );
                if p.font_size == 0.0 {
                    ui.label(RichText::new(tr("automatisch", "automatic")).weak().small());
                }
            });
            ui.horizontal(|ui| {
                ui.label(tr("Ausrichtung:", "Alignment:"));
                for (a, label) in
                    [(0u8, tr("Links", "Left")), (1, tr("Mitte", "Centre")), (2, tr("Rechts", "Right"))]
                {
                    if ui.selectable_label(p.align == a, label).clicked() {
                        p.align = a;
                    }
                }
            });
            ui.label(tr("Textfarbe:", "Text colour:"));
            if let Some(c) = color_row(ui, p.color) {
                p.color = c;
            }
        }
        if p.kind.has_text_look() || p.kind == PropsKind::ListBox {
            ui.add_space(4.0);
            let mut border = p.border.is_some();
            if ui.checkbox(&mut border, tr("Rahmen", "Border")).changed() {
                p.border = border.then_some(p.border.unwrap_or(Rgba::rgb(128, 128, 128)));
            }
            if let Some(c) = p.border
                && let Some(n) = color_row(ui, c)
            {
                p.border = Some(n);
            }
            let mut fill = p.background.is_some();
            if ui.checkbox(&mut fill, tr("Hintergrund", "Background")).changed() {
                p.background = fill.then_some(p.background.unwrap_or(Rgba::rgb(255, 255, 255)));
            }
            if let Some(c) = p.background
                && let Some(n) = color_row(ui, c)
            {
                p.background = Some(n);
            }
        }
        if let Some(err) = &e.error {
            ui.colored_label(ui.visuals().error_fg_color, err);
        }
        // Changes apply by themselves once nothing is typed or dragged any
        // more (leaving a text box, Enter, a click on a box or colour).
        let busy = ui.ctx().egui_wants_keyboard_input() || ui.input(|i| i.pointer.any_down());
        let apply = e.edit != e.orig && !busy && e.failed.as_ref() != Some(&e.edit);
        let mut props = None;
        if apply {
            // A name may be used only once (radio buttons share their group name).
            let name = e.edit.name.trim().to_string();
            let taken = name != e.orig.name
                && self
                    .tab()
                    .and_then(|t| t.doc.field_values().ok())
                    .is_some_and(|all| all.iter().any(|(n, _)| *n == name));
            if name.is_empty() {
                e.error = Some(tr("Bitte einen Namen eingeben.", "Please enter a name.").into());
                e.failed = Some(e.edit.clone());
            } else if taken {
                e.error = Some(if de {
                    format!("Den Namen „{name}“ hat schon ein anderes Feld.")
                } else {
                    format!("Another field is already called \"{name}\".")
                });
                e.failed = Some(e.edit.clone());
            } else {
                e.error = None;
                props = Some(e.edit.clone());
            }
        }
        self.props_field = Some(e);
        if let Some(props) = props {
            let index = f.annot_index;
            self.edit(ctx, |tab| tab.doc.set_field_props(page, index, &props));
        }

        ui.add_space(8.0);
        ui.label(
            RichText::new(tr(
                "Änderungen gelten sofort (Rückgängig: Strg+Z). Zum Ausfüllen: Formular › Ausfüllen oder Werkzeug „Hand“ (Taste H), dann ins Feld klicken.",
                "Changes apply at once (undo: Ctrl+Z). To fill in: Form › Fill in or the Hand tool (key H), then click into the field.",
            ))
            .weak()
            .small(),
        );
        ui.label(RichText::new(t.tool_hint(Tool::SelectFields)).weak().small());
        ui.add_space(8.0);
        if ui.button(tr("Feld löschen", "Delete field")).clicked() {
            self.delete_selected_annotation(ctx);
        }
    }
}

/// The form field properties being edited in the right pane.
pub(super) struct FieldEditor {
    tab: u64,
    sel: AnnotRef,
    rev: u64,
    orig: crate::forms::FieldProps,
    edit: crate::forms::FieldProps,
    /// List items as typed (one per line).
    options: String,
    error: Option<String>,
    /// Values that could not be applied (not tried again).
    failed: Option<crate::forms::FieldProps>,
}

impl FieldEditor {
    fn new(tab: u64, sel: AnnotRef, rev: u64, props: crate::forms::FieldProps) -> Self {
        Self {
            tab,
            sel,
            rev,
            options: props.options.join("\n"),
            edit: props.clone(),
            orig: props,
            error: None,
            failed: None,
        }
    }
}
