//! Dialogs "PDF optimieren" (Optimiert speichern) and "Speicherverbrauch
//! untersuchen".

use egui::{Context, RichText, Ui, vec2};

use super::commands::Cmd;
use super::dialogs::{Action, Outcome};
use super::page_dialogs::{footer, tr};
use super::{Dialog, MrPdfApp};
use crate::optimize::{
    Compression, Downsample, EmbedAnnots, FontInfo, FontRemoval, ImageClass, OptimizeOptions, Quality, Usage,
    UsageKind, is_standard_font,
};
use crate::ui::pickers::{preset_combo, section};

/// State of the "PDF optimieren" dialog.
#[derive(Debug, Clone, PartialEq)]
pub struct OptimizeState {
    pub options: OptimizeOptions,
    /// Shown page: 0 Bilder, 1 Schriftarten, 2 Objekte, 3 Benutzerdaten, 4 Aufräumen.
    pub page: usize,
    pub version: String,
    pub fonts: Vec<FontInfo>,
    /// Selection in the font lists (name).
    pub font_selection: Option<String>,
    pub preset_name: String,
}

fn size_text(bytes: u64) -> String {
    match bytes {
        b if b >= 1024 * 1024 => format!("{:.1} MB", b as f64 / (1024.0 * 1024.0)).replace('.', ","),
        b if b >= 1024 => format!("{:.1} KB", b as f64 / 1024.0).replace('.', ","),
        b => format!("{b} Byte"),
    }
}

fn usage_name(kind: UsageKind, de: bool) -> &'static str {
    match kind {
        UsageKind::Images => tr(de, "Bilder", "Images"),
        UsageKind::Fonts => tr(de, "Schriftarten", "Fonts"),
        UsageKind::Forms => tr(de, "Formular-XObjekte", "Form XObjects"),
        UsageKind::Content => tr(de, "Seiteninhalt", "Page content"),
        UsageKind::Annotations => tr(de, "Anmerkungen und Formularfelder", "Comments and form fields"),
        UsageKind::Bookmarks => tr(de, "Lesezeichen", "Bookmarks"),
        UsageKind::Structure => tr(de, "Struktur-Informationen (Tags)", "Structure information (tags)"),
        UsageKind::Metadata => {
            tr(de, "Dokumentinformationen und Metadaten", "Document information and metadata")
        }
        UsageKind::Attachments => tr(de, "Datei-Anhänge", "File attachments"),
        UsageKind::Other => tr(de, "Sonstige Objekte", "Other objects"),
        UsageKind::Overhead => {
            tr(de, "Querverweistabelle und Dateistruktur", "Cross reference table and file structure")
        }
    }
}

fn downsample_combo(ui: &mut Ui, de: bool, id: &str, value: &mut Downsample) {
    let names = [
        (Downsample::None, tr(de, "Nicht herunterrechnen", "Do not downsample")),
        (Downsample::Average, tr(de, "Durchschnittliches Herunterrechnen auf", "Average downsampling to")),
        (Downsample::Subsample, tr(de, "Herunterrechnen auf", "Subsampling to")),
        (Downsample::Bicubic, tr(de, "Bikubisches Herunterrechnen auf", "Bicubic downsampling to")),
    ];
    let current = names.iter().find(|n| n.0 == *value).map_or("", |n| n.1);
    egui::ComboBox::from_id_salt(id).selected_text(current).width(230.0).show_ui(ui, |ui| {
        for (v, n) in names {
            ui.selectable_value(value, v, n);
        }
    });
}

/// One image class: downsampling, resolution, compression, quality.
fn image_class_ui(ui: &mut Ui, de: bool, id: &str, title: &str, class: &mut ImageClass, lossy: bool) {
    section(ui, title, |ui| {
        ui.horizontal(|ui| {
            downsample_combo(ui, de, &format!("{id}-down"), &mut class.downsample);
            let on = class.downsample != Downsample::None;
            ui.add_enabled(on, egui::DragValue::new(&mut class.dpi).range(9.0..=2400.0).suffix(" dpi"));
            ui.label(tr(de, "für Bilder über:", "for images above:"));
            ui.add_enabled(on, egui::DragValue::new(&mut class.above).range(9.0..=4800.0).suffix(" dpi"));
        });
        ui.horizontal(|ui| {
            ui.label(tr(de, "Kompression", "Compression"));
            let mut names = vec![(Compression::Keep, tr(de, "Existierende behalten", "Keep existing"))];
            if lossy {
                names.push((Compression::Jpeg, "JPEG"));
            }
            names.push((Compression::Zip, "ZIP"));
            if lossy {
                names.push((Compression::JpegOrZip, "JPEG/ZIP"));
            }
            if !lossy && matches!(class.compression, Compression::Jpeg | Compression::JpegOrZip) {
                class.compression = Compression::Zip;
            }
            let current = names.iter().find(|n| n.0 == class.compression).map_or("", |n| n.1);
            egui::ComboBox::from_id_salt(format!("{id}-comp")).selected_text(current).width(140.0).show_ui(
                ui,
                |ui| {
                    for (v, n) in &names {
                        ui.selectable_value(&mut class.compression, *v, *n);
                    }
                    ui.add_enabled(
                        false,
                        egui::Button::new(if lossy { "JPEG2000, MRC" } else { "JBIG2, CCITT, RLE" })
                            .frame(false),
                    )
                    .on_disabled_hover_text(tr(
                        de,
                        "Diese Verfahren werden von MrPDF nicht erzeugt.",
                        "MrPDF does not create these encodings.",
                    ));
                },
            );
            if lossy {
                ui.label(tr(de, "Qualität:", "Quality:"));
                let names = [
                    (Quality::Minimal, tr(de, "Minimal", "Minimum")),
                    (Quality::Low, tr(de, "Niedrig", "Low")),
                    (Quality::Medium, tr(de, "Mittel", "Medium")),
                    (Quality::High, tr(de, "Hoch", "High")),
                    (Quality::Maximum, tr(de, "Maximal", "Maximum")),
                ];
                let current = names.iter().find(|n| n.0 == class.quality).map_or("", |n| n.1);
                let jpeg = matches!(
                    class.compression,
                    Compression::Jpeg | Compression::JpegOrZip | Compression::Keep
                );
                ui.add_enabled_ui(jpeg, |ui| {
                    egui::ComboBox::from_id_salt(format!("{id}-q"))
                        .selected_text(current)
                        .width(90.0)
                        .show_ui(ui, |ui| {
                            for (v, n) in names {
                                ui.selectable_value(&mut class.quality, v, n);
                            }
                        });
                });
            }
        });
    });
}

fn removal_name(r: FontRemoval, de: bool) -> &'static str {
    match r {
        FontRemoval::None => tr(de, "Keine eingebettete Schriftart entfernen", "Remove no embedded font"),
        FontRemoval::Recommended => tr(de, "Empfohlene Schriftarten entfernen", "Remove recommended fonts"),
        FontRemoval::All => tr(de, "Alle Schriftarten entfernen", "Remove all fonts"),
        FontRemoval::Manual => {
            tr(de, "Zu entfernende Schriftarten manuell auswählen", "Choose fonts to remove manually")
        }
    }
}

/// Whether the font will be removed with the current settings.
fn removes(o: &OptimizeOptions, f: &FontInfo) -> bool {
    f.simple
        && match o.font_removal {
            FontRemoval::None => false,
            FontRemoval::Recommended => is_standard_font(&f.name),
            FontRemoval::All => true,
            FontRemoval::Manual => o.remove_fonts.contains(&f.name),
        }
}

impl MrPdfApp {
    fn opt_de(&self) -> bool {
        self.settings.language == crate::ui::i18n::Language::German
    }

    /// Opens "PDF optimieren" or "Speicherverbrauch untersuchen".
    pub(super) fn run_optimize(&mut self, cmd: Cmd) -> bool {
        let de = self.opt_de();
        match cmd {
            Cmd::SaveOptimized => {
                let Some(tab) = self.tabs.get_mut(self.active) else { return true };
                let bytes = match tab.doc.to_bytes() {
                    Ok(b) => b,
                    Err(err) => {
                        self.show_error(&err);
                        return true;
                    }
                };
                let version = bytes
                    .get(5..8)
                    .map(|v| String::from_utf8_lossy(v).into_owned())
                    .filter(|v| v.starts_with("1.") || v.starts_with("2."))
                    .unwrap_or_else(|| "1.7".into());
                let fonts = crate::optimize::list_fonts(&bytes, None).unwrap_or_default();
                self.dialog = Some(Dialog::Optimize(Box::new(OptimizeState {
                    options: OptimizeOptions::default(),
                    page: 0,
                    version,
                    fonts,
                    font_selection: None,
                    preset_name: String::new(),
                })));
            }
            Cmd::SpaceUsage => {
                let Some(tab) = self.tabs.get_mut(self.active) else { return true };
                let result = tab.doc.to_bytes().and_then(|b| crate::optimize::space_usage(&b, None));
                match result {
                    Ok(usage) => self.dialog = Some(Dialog::SpaceUsage(Box::new(usage))),
                    Err(crate::Error::EncryptedNotSupported) => {
                        self.dialog = Some(Dialog::Message {
                            title: cmd.label(self.settings.language).into(),
                            text: tr(
                                de,
                                "Für verschlüsselte Dokumente ist das nicht möglich. Bitte zuerst den Schutz entfernen.",
                                "This is not possible for encrypted documents. Please remove the protection first.",
                            )
                            .into(),
                        })
                    }
                    Err(err) => self.show_error(&err),
                }
            }
            _ => return false,
        }
        true
    }

    /// "Speicherverbrauch untersuchen": bytes per kind of content.
    pub(super) fn space_usage_dialog(&mut self, ui: &mut Ui, usage: &Usage) -> (Outcome, Option<Action>) {
        let de = self.opt_de();
        ui.set_max_width(560.0);
        super::dialogs::set_title(ui, Cmd::SpaceUsage.label(self.settings.language));
        ui.label(format!("{}: {}", tr(de, "Dateigröße", "File size"), size_text(usage.total)));
        ui.add_space(6.0);
        egui::Grid::new("usage").num_columns(4).striped(true).spacing(vec2(12.0, 4.0)).show(ui, |ui| {
            for h in [
                tr(de, "Inhalt", "Content"),
                tr(de, "Objekte", "Objects"),
                tr(de, "Größe", "Size"),
                tr(de, "Anteil", "Share"),
            ] {
                ui.label(RichText::new(h).strong());
            }
            ui.end_row();
            for (kind, bytes, count) in &usage.parts {
                let share = if usage.total > 0 { *bytes as f32 / usage.total as f32 } else { 0.0 };
                ui.label(usage_name(*kind, de));
                ui.label(if *kind == UsageKind::Overhead { String::new() } else { count.to_string() });
                ui.label(size_text(*bytes));
                ui.add(
                    egui::ProgressBar::new(share)
                        .desired_width(160.0)
                        .text(format!("{:.1} %", share * 100.0).replace('.', ",")),
                );
                ui.end_row();
            }
        });
        ui.label(
            RichText::new(tr(
                de,
                "Die Werte sind auf Grundlage der aktuellen Fassung berechnet (auch wenn sie noch nicht gespeichert ist).",
                "The values are computed from the current version (even if it is not saved yet).",
            ))
            .small()
            .weak(),
        );
        ui.add_space(6.0);
        let close = ui.button("OK").clicked()
            || ui.input(|i| i.key_pressed(egui::Key::Escape) || i.key_pressed(egui::Key::Enter));
        (if close { Outcome::Close } else { Outcome::Keep }, None)
    }

    /// "PDF optimieren".
    pub(super) fn optimize_dialog(
        &mut self,
        ui: &mut Ui,
        state: &mut OptimizeState,
    ) -> (Outcome, Option<Action>) {
        let de = self.opt_de();
        ui.set_max_width(720.0);
        super::dialogs::set_title(ui, tr(de, "PDF optimieren", "Optimize PDF"));
        // Presets and space usage.
        let mut usage = false;
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("opt-builtin")
                .selected_text(tr(de, "Vorgabe wählen…", "Choose preset…"))
                .width(170.0)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(
                            false,
                            tr(de, "Standard – für alle Geräte", "Standard – for all devices"),
                        )
                        .clicked()
                    {
                        state.options = OptimizeOptions::default();
                    }
                    if ui
                        .selectable_label(
                            false,
                            tr(de, "Mobil – für mobile Geräte", "Mobile – for mobile devices"),
                        )
                        .clicked()
                    {
                        state.options = OptimizeOptions::mobile();
                    }
                });
            let mut name = std::mem::take(&mut state.preset_name);
            let _ = preset_combo(ui, de, "optimize", &mut state.options, &mut self.settings, &mut name);
            state.preset_name = name;
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                usage = ui.button(tr(de, "Platzverbrauch untersuchen…", "Examine space usage…")).clicked();
            });
        });
        ui.horizontal(|ui| {
            ui.label(format!(
                "{} {}",
                tr(de, "Aktuelle PDF-Version:", "Current PDF version:"),
                state.version
            ));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let versions = [
                    (None, tr(de, "Existierende behalten", "Keep existing").to_string()),
                    (
                        Some("1.3"),
                        tr(de, "Version 1.3 (Acrobat 4.0 und neuer)", "Version 1.3 (Acrobat 4.0 and later)")
                            .into(),
                    ),
                    (
                        Some("1.4"),
                        tr(de, "Version 1.4 (Acrobat 5.0 und neuer)", "Version 1.4 (Acrobat 5.0 and later)")
                            .into(),
                    ),
                    (
                        Some("1.5"),
                        tr(de, "Version 1.5 (Acrobat 6.0 und neuer)", "Version 1.5 (Acrobat 6.0 and later)")
                            .into(),
                    ),
                    (
                        Some("1.6"),
                        tr(de, "Version 1.6 (Acrobat 7.0 und neuer)", "Version 1.6 (Acrobat 7.0 and later)")
                            .into(),
                    ),
                    (
                        Some("1.7"),
                        tr(de, "Version 1.7 (Acrobat 8.0 und neuer)", "Version 1.7 (Acrobat 8.0 and later)")
                            .into(),
                    ),
                ];
                let current = versions
                    .iter()
                    .find(|v| v.0 == state.options.version.as_deref())
                    .map(|v| v.1.clone())
                    .unwrap_or_default();
                egui::ComboBox::from_id_salt("opt-version").selected_text(current).width(240.0).show_ui(
                    ui,
                    |ui| {
                        for (v, n) in &versions {
                            if ui
                                .selectable_label(state.options.version.as_deref() == *v, n.as_str())
                                .clicked()
                            {
                                state.options.version = v.map(str::to_string);
                            }
                        }
                    },
                );
                ui.label(tr(de, "Kompatibel mit:", "Compatible with:"));
            });
        });
        ui.separator();
        let o = &mut state.options;
        ui.horizontal_top(|ui| {
            // Left: pages with check boxes.
            ui.vertical(|ui| {
                ui.set_width(170.0);
                let pages: [(&mut bool, &str); 5] = [
                    (&mut o.images, tr(de, "Bilder", "Images")),
                    (&mut o.fonts, tr(de, "Schriftarten", "Fonts")),
                    (&mut o.objects, tr(de, "Objekte entfernen", "Remove objects")),
                    (&mut o.user_data, tr(de, "Benutzerdaten entfernen", "Remove user data")),
                    (&mut o.cleanup, tr(de, "Aufräumen", "Clean up")),
                ];
                for (i, (flag, name)) in pages.into_iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.checkbox(flag, "");
                        if ui.selectable_label(state.page == i, name).clicked() {
                            state.page = i;
                        }
                    });
                }
            });
            // A vertical separator would grow to the height of the screen.
            ui.add_space(8.0);
            ui.vertical(|ui| {
                ui.set_width(500.0);
                ui.set_min_height(330.0);
                match state.page {
                    0 => ui.add_enabled_ui(o.images, |ui| {
                        image_class_ui(ui, de, "opt-color", tr(de, "Farbbilder", "Colour images"), &mut o.color, true);
                        image_class_ui(ui, de, "opt-gray", tr(de, "Bilder in Graustufen", "Greyscale images"), &mut o.gray, true);
                        image_class_ui(ui, de, "opt-indexed", tr(de, "Indizierte Bilder", "Indexed images"), &mut o.indexed, false);
                        image_class_ui(ui, de, "opt-mono", tr(de, "Schwarz-Weiß-Bilder", "Black and white images"), &mut o.mono, false);
                        ui.checkbox(&mut o.only_if_smaller, tr(de, "Bilder nur optimieren, falls Platz eingespart wird", "Only optimize images if space is saved"));
                        ui.checkbox(&mut o.annotation_images, tr(de, "Anmerkungen ebenfalls optimieren", "Optimize comments too"));
                    }),
                    1 => ui.add_enabled_ui(o.fonts, |ui| {
                        section(ui, tr(de, "Eingebettete Schriftarten entfernen", "Remove embedded fonts"), |ui| {
                            egui::ComboBox::from_id_salt("opt-fonts")
                                .selected_text(removal_name(o.font_removal, de))
                                .width(320.0)
                                .show_ui(ui, |ui| {
                                    for r in [FontRemoval::None, FontRemoval::Recommended, FontRemoval::All, FontRemoval::Manual] {
                                        ui.selectable_value(&mut o.font_removal, r, removal_name(r, de));
                                    }
                                });
                            let manual = o.font_removal == FontRemoval::Manual;
                            let (kept, gone): (Vec<&FontInfo>, Vec<&FontInfo>) = state.fonts.iter().partition(|f| !removes(o, f));
                            let mut move_out = false;
                            let mut move_in = false;
                            ui.horizontal_top(|ui| {
                                for (title, list, left) in [
                                    (tr(de, "Eingebettete Schriftarten:", "Embedded fonts:"), &kept, true),
                                    (tr(de, "Schriftarten, die entfernt werden:", "Fonts that will be removed:"), &gone, false),
                                ] {
                                    ui.vertical(|ui| {
                                        ui.set_width(190.0);
                                        ui.label(title);
                                        egui::Frame::new().stroke(ui.visuals().widgets.noninteractive.bg_stroke).show(ui, |ui| {
                                            ui.set_min_size(vec2(186.0, 150.0));
                                            egui::ScrollArea::vertical().id_salt(title).max_height(150.0).show(ui, |ui| {
                                                for f in list {
                                                    let label = if f.simple { f.name.clone() } else { format!("{} ({})", f.name, f.subtype) };
                                                    let selected = state.font_selection.as_deref() == Some(f.name.as_str());
                                                    let r = ui.add_enabled(f.simple, egui::Button::selectable(selected, label)).on_disabled_hover_text(tr(
                                                        de,
                                                        "Zusammengesetzte (CID-)Schriftarten bleiben immer eingebettet.",
                                                        "Composite (CID) fonts always stay embedded.",
                                                    ));
                                                    if r.clicked() {
                                                        state.font_selection = Some(f.name.clone());
                                                    }
                                                }
                                            });
                                        });
                                    });
                                    if left {
                                        ui.vertical(|ui| {
                                            ui.add_space(60.0);
                                            move_out = ui.add_enabled(manual, egui::Button::new(tr(de, "Entfernen >>", "Remove >>"))).clicked();
                                            move_in = ui.add_enabled(manual, egui::Button::new(tr(de, "<< Behalten", "<< Keep"))).clicked();
                                        });
                                    }
                                }
                            });
                            if let Some(sel) = state.font_selection.clone() {
                                if move_out && !o.remove_fonts.contains(&sel) {
                                    o.remove_fonts.push(sel.clone());
                                }
                                if move_in {
                                    o.remove_fonts.retain(|n| *n != sel);
                                }
                            }
                            ui.label(
                                RichText::new(tr(
                                    de,
                                    "Ohne eingebettete Schriftart zeigt das Anzeigeprogramm eine ähnliche Systemschrift. Empfohlen wird das nur für Standardschriften (Arial/Helvetica, Times, Courier).",
                                    "Without an embedded font the viewer uses a similar system font. This is only recommended for standard fonts (Arial/Helvetica, Times, Courier).",
                                ))
                                .small()
                                .weak(),
                            );
                        });
                        let mut merge = false;
                        ui.add_enabled(false, egui::Checkbox::new(&mut merge, tr(de, "Schriftteilmengen zusammenführen", "Merge font subsets")))
                            .on_disabled_hover_text(tr(de, "Wird von MrPDF nicht unterstützt.", "Not supported by MrPDF."));
                    }),
                    2 => ui.add_enabled_ui(o.objects, |ui| {
                        section(ui, tr(de, "Objekteinstellungen entfernen", "Remove object settings"), |ui| {
                            ui.checkbox(&mut o.form_actions, tr(de, "Übermittlungs-, Import- und Rücksetz-Aktionen von Formularen entfernen", "Remove submit, import and reset actions of forms"));
                            ui.horizontal(|ui| {
                                ui.label(tr(de, "Anmerkungen einbetten", "Embed comments"));
                                let names = [
                                    (EmbedAnnots::None, tr(de, "Keine Anmerkung einbetten", "Embed no comment")),
                                    (EmbedAnnots::All, tr(de, "Anmerkungen und Formularfelder einbetten", "Embed comments and form fields")),
                                ];
                                let current = names.iter().find(|n| n.0 == o.embed_annots).map_or("", |n| n.1);
                                egui::ComboBox::from_id_salt("opt-embed").selected_text(current).width(260.0).show_ui(ui, |ui| {
                                    for (v, n) in names {
                                        ui.selectable_value(&mut o.embed_annots, v, n);
                                    }
                                });
                            });
                            ui.checkbox(&mut o.javascript, tr(de, "Alle JavaScript-Aktionen entfernen", "Remove all JavaScript actions"));
                            ui.checkbox(&mut o.alternate_images, tr(de, "Alle alternativen Bilder entfernen", "Remove all alternate images"));
                            ui.checkbox(&mut o.thumbnails, tr(de, "Alle eingebetteten Seitenvorschaubilder entfernen", "Remove all embedded page thumbnails"));
                            let mut fragments = false;
                            ui.add_enabled(false, egui::Checkbox::new(&mut fragments, tr(de, "Bildfragmente erkennen und zusammenfassen", "Detect and merge image fragments")))
                                .on_disabled_hover_text(tr(de, "Wird von MrPDF nicht unterstützt.", "Not supported by MrPDF."));
                            ui.checkbox(&mut o.print_settings, tr(de, "Eingebettete Druckeinstellungen entfernen", "Remove embedded print settings"));
                            ui.checkbox(&mut o.bookmarks, tr(de, "Lesezeichen entfernen", "Remove bookmarks"));
                        });
                    }),
                    3 => ui.add_enabled_ui(o.user_data, |ui| {
                        section(ui, tr(de, "Benutzerspezifische Informationen entfernen", "Remove user specific information"), |ui| {
                            ui.checkbox(&mut o.all_annotations, tr(de, "Alle Kommentare, Verweise, Formulare und Multimediainhalte entfernen", "Remove all comments, links, forms and multimedia"));
                            ui.checkbox(&mut o.links, tr(de, "Alle Verweise entfernen", "Remove all links"));
                            ui.checkbox(&mut o.metadata, tr(de, "Dokumentinformationen und Metadaten entfernen", "Remove document information and metadata"));
                            ui.checkbox(&mut o.attachments, tr(de, "Datei-Anhänge entfernen", "Remove file attachments"));
                            ui.checkbox(&mut o.external_refs, tr(de, "Externe Querverweise entfernen", "Remove external cross references"));
                            ui.checkbox(&mut o.private_data, tr(de, "Private Daten anderer Anwendungen entfernen", "Remove private data of other applications"));
                            let mut layers = false;
                            ui.add_enabled(false, egui::Checkbox::new(&mut layers, tr(de, "Versteckte Ebenen entfernen und sichtbare Ebenen zusammenfassen", "Remove hidden layers and merge visible layers")))
                                .on_disabled_hover_text(tr(de, "Wird von MrPDF nicht unterstützt.", "Not supported by MrPDF."));
                        });
                    }),
                    _ => ui.add_enabled_ui(o.cleanup, |ui| {
                        section(ui, tr(de, "Aufräumen", "Clean up"), |ui| {
                            for (flag, de_text, en_text) in [
                                (&mut o.dup_fonts, "Doppelte Schriftarten entfernen", "Remove duplicate fonts"),
                                (&mut o.dup_images, "Doppelte Bilder entfernen", "Remove duplicate images"),
                                (&mut o.dup_forms, "Doppelte Formulare entfernen", "Remove duplicate forms"),
                                (&mut o.compress_streams, "Nicht komprimierte Streams mit Flate-Algorithmus komprimieren", "Compress uncompressed streams with Flate"),
                                (&mut o.lzw_to_flate, "Flate-Komprimierung statt LZW-Komprimierung für Streams verwenden", "Use Flate instead of LZW compression for streams"),
                                (&mut o.invalid_bookmarks, "Ungültige Lesezeichen entfernen", "Remove invalid bookmarks"),
                                (&mut o.invalid_links, "Ungültige Verweise entfernen", "Remove invalid links"),
                                (&mut o.unused_dests, "Nicht verwendete Sprungmarken entfernen", "Remove unused named destinations"),
                                (&mut o.outside_crop, "Inhalte außerhalb der Crop-Box suchen und entfernen", "Find and remove content outside the crop box"),
                                (&mut o.unused_layers, "Unbenutzte Ebenen entfernen", "Remove unused layers"),
                                (&mut o.optimize_content, "Seiteninhalt optimieren", "Optimize page content"),
                            ] {
                                ui.checkbox(flag, tr(de, de_text, en_text));
                            }
                        });
                    }),
                };
            });
        });
        if usage {
            // The report replaces this dialog.
            return (
                Outcome::Close,
                Some(Box::new(|app, _| {
                    let _ = app.run_optimize(Cmd::SpaceUsage);
                })),
            );
        }
        let (ok, cancel, _) = footer(ui, de, true, false, "OK");
        if cancel {
            return (Outcome::Close, None);
        }
        if !ok {
            return (Outcome::Keep, None);
        }
        let options = state.options.clone();
        (Outcome::Close, Some(Box::new(move |app, ctx| app.save_optimized(ctx, &options))))
    }

    /// Asks for the target file, optimizes and opens the result.
    fn save_optimized(&mut self, ctx: &Context, options: &OptimizeOptions) {
        let de = self.opt_de();
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        let stem = tab.doc.display_name().trim_end_matches(".pdf").trim_end_matches(".PDF").to_string();
        let mut dialog = rfd::FileDialog::new()
            .set_title(tr(de, "Optimiert speichern", "Save optimized"))
            .add_filter(tr(de, "PDF-Dokument", "PDF document"), &["pdf"])
            .set_file_name(format!("{stem}.pdf"));
        if let Some(dir) = tab.doc.path().and_then(std::path::Path::parent) {
            dialog = dialog.set_directory(dir);
        }
        let Some(path) = dialog.save_file() else { return };
        let path = if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")) {
            path
        } else {
            path.with_extension("pdf")
        };
        self.save_optimized_to(ctx, &path, options);
    }

    /// Optimizes the active document into `path` and shows the result.
    pub fn save_optimized_to(&mut self, ctx: &Context, path: &std::path::Path, options: &OptimizeOptions) {
        let de = self.opt_de();
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        let path = path.to_path_buf();
        let result = Self::optimized_bytes(tab, options).and_then(|(bytes, report)| {
            std::fs::write(&path, &bytes).map_err(|e| crate::Error::io(&path, e))?;
            Ok(report)
        });
        let report = match result {
            Ok(r) => r,
            Err(err) => {
                self.show_error(&err);
                return;
            }
        };
        // Show the optimized file in this tab.
        match crate::Document::open(&path, None) {
            Ok(doc) => {
                let id = self.tabs[self.active].id;
                self.forget_recovery(id);
                let tab = &mut self.tabs[self.active];
                let page = tab.current_page;
                tab.doc = doc;
                tab.after_edit();
                tab.go_to_page(page.min(tab.doc.page_count().saturating_sub(1)));
                self.settings.add_recent(&path);
            }
            Err(err) => self.show_error(&err),
        }
        let saved = report.before as f64 - report.after as f64;
        let percent = if report.before > 0 { saved / report.before as f64 * 100.0 } else { 0.0 };
        let text = if de {
            format!(
                "Gespeichert: {}\n\nVorher: {}\nNachher: {} ({:.0} % {})\n\nGeänderte Bilder: {}\nEntfernte Schriftarten: {}\nZusammengefasste Duplikate: {}\nEntfernte Objekte: {}",
                path.display(),
                size_text(report.before as u64),
                size_text(report.after as u64),
                percent.abs(),
                if saved >= 0.0 { "kleiner" } else { "größer" },
                report.images_changed,
                report.fonts_removed,
                report.duplicates_merged,
                report.objects_removed
            )
        } else {
            format!(
                "Saved: {}\n\nBefore: {}\nAfter: {} ({:.0} % {})\n\nChanged images: {}\nRemoved fonts: {}\nMerged duplicates: {}\nRemoved objects: {}",
                path.display(),
                size_text(report.before as u64),
                size_text(report.after as u64),
                percent.abs(),
                if saved >= 0.0 { "smaller" } else { "larger" },
                report.images_changed,
                report.fonts_removed,
                report.duplicates_merged,
                report.objects_removed
            )
        };
        self.dialog = Some(Dialog::Message { title: tr(de, "PDF optimieren", "Optimize PDF").into(), text });
        ctx.request_repaint();
    }

    /// Engine steps (embedding comments, content outside the crop box) on a
    /// copy, then the structural optimization.
    fn optimized_bytes(
        tab: &mut crate::ui::tab::DocTab,
        options: &OptimizeOptions,
    ) -> crate::Result<(Vec<u8>, crate::optimize::OptimizeReport)> {
        let mut bytes = tab.doc.to_bytes()?;
        let before = bytes.len();
        let engine_steps = options.objects && options.embed_annots == EmbedAnnots::All
            || options.cleanup && options.outside_crop;
        if engine_steps {
            let mut copy = crate::Document::from_bytes(bytes, "copy.pdf", None)?;
            let pages: Vec<usize> = (0..copy.page_count()).collect();
            if options.objects && options.embed_annots == EmbedAnnots::All {
                copy.flatten_pages(&pages)?;
            }
            if options.cleanup && options.outside_crop {
                copy.remove_outside(&pages, true, true, true)?;
            }
            bytes = copy.to_bytes()?;
        }
        let (out, mut report) = crate::optimize::optimize(&bytes, None, options)?;
        report.before = before;
        Ok((out, report))
    }
}
