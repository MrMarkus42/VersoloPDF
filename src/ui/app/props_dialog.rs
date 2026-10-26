//! "Dokumenteigenschaften" (Strg+D):
//! eight pages with a search box, OK / Abbrechen.

use std::path::PathBuf;

use egui::{RichText, Ui, vec2};

use super::commands::Cmd;
use super::dialogs::{Action, Outcome};
use super::page_dialogs::tr;
use super::{Dialog, MrPdfApp};
use crate::doc_props::{
    DocProps, DuplexPreset, FontEntry, LayoutMode, OpenAction, OpenZoom, PageMode, Trapped,
};
use crate::model::Metadata;
use crate::ui::icons::Icon;
use crate::ui::pickers::section;

/// An entry being edited on the pages "Eigene", "Aktionen" or "JavaScript":
/// (index or None for a new one, name, value).
pub type EditRow = (Option<usize>, String, String);

/// State of the dialog.
#[derive(Debug, Clone, PartialEq)]
pub struct PropsState {
    pub page: usize,
    pub search: String,
    pub meta: Metadata,
    pub props: DocProps,
    original: (Metadata, DocProps),
    pub fonts: Vec<FontEntry>,
    pub file_name: String,
    pub folder: Option<PathBuf>,
    pub file_size: u64,
    pub version: String,
    pub pages: usize,
    pub page_size: String,
    /// Lines of the "Sicherheit" page: (label, value).
    pub security: Vec<(String, String)>,
    pub method: String,
    pub encryption: String,
    pub read_only: bool,
    pub selected: Option<usize>,
    pub editing: Option<EditRow>,
    pub show_xmp: bool,
}

/// Category names with search words.
fn pages(de: bool) -> [(Icon, &'static str, &'static str); 8] {
    [
        (
            Icon::Info,
            tr(de, "Beschreibung", "Description"),
            "dateiname ort größe titel autor betreff schlüsselwörter produzent anwendung version seitenzahl erstellt geändert metadaten file name title author subject keywords producer",
        ),
        (Icon::TextBox, tr(de, "Schriftarten", "Fonts"), "schrift font eingebettet embedded"),
        (
            Icon::Lock,
            tr(de, "Sicherheit", "Security"),
            "sicherungsmethode verschlüsselung berechtigungen kennwort drucken kopieren security password permissions",
        ),
        (
            Icon::Eye,
            tr(de, "Ansicht beim Öffnen", "Initial view"),
            "navigationsansicht seitenlayout aktion fenster titel vollbild menüleiste symbolleisten layout window fullscreen",
        ),
        (Icon::Properties, tr(de, "Eigene", "Custom"), "eigene eigenschaften custom properties"),
        (
            Icon::Gear,
            tr(de, "Erweitert", "Advanced"),
            "basis-url eingeschränkt trapped skalierung duplex papiergröße seitenbereich kopien bindung sprache base url print language",
        ),
        (
            Icon::Pointer,
            tr(de, "Aktionen", "Actions"),
            "aktionen schließen speichern drucken actions close save print",
        ),
        (Icon::Console, tr(de, "JavaScript", "JavaScript"), "javascript skript script"),
    ]
}

fn trigger_name(key: &str, de: bool) -> &'static str {
    match key {
        "WC" => tr(de, "Vor dem Schließen des Dokuments", "Before closing the document"),
        "WS" => tr(de, "Vor dem Speichern des Dokuments", "Before saving the document"),
        "DS" => tr(de, "Nach dem Speichern des Dokuments", "After saving the document"),
        "WP" => tr(de, "Vor dem Drucken des Dokuments", "Before printing the document"),
        _ => tr(de, "Nach dem Drucken des Dokuments", "After printing the document"),
    }
}

const TRIGGERS: [&str; 5] = ["WC", "WS", "DS", "WP", "DP"];

fn combo<T: PartialEq + Copy>(ui: &mut Ui, id: &str, value: &mut T, options: &[(T, &str)], width: f32) {
    let current = options.iter().find(|o| o.0 == *value).map_or("", |o| o.1);
    egui::ComboBox::from_id_salt(id).selected_text(current).width(width).show_ui(ui, |ui| {
        for (v, name) in options {
            ui.selectable_value(value, *v, *name);
        }
    });
}

fn size_text(bytes: u64) -> String {
    match bytes {
        b if b >= 1024 * 1024 => format!("{:.2} MB ({b} Byte)", b as f64 / 1048576.0),
        b if b >= 1024 => format!("{:.1} KB ({b} Byte)", b as f64 / 1024.0),
        b => format!("{b} Byte"),
    }
}

impl MrPdfApp {
    fn pp_de(&self) -> bool {
        self.settings.language == crate::ui::i18n::Language::German
    }

    /// Opens "Dokumenteigenschaften".
    pub fn open_properties(&mut self) {
        let de = self.pp_de();
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        let meta = tab.doc.metadata();
        let props = tab.doc.properties().unwrap_or_default();
        let fonts = tab.doc.fonts().unwrap_or_default();
        let perms = tab.doc.permissions();
        let mm = |pt: f32| pt * 25.4 / 72.0;
        let page_size = tab
            .doc
            .geometry(tab.current_page)
            .map(|g| {
                let (w, h) = g.view_size();
                format!("{:.0} x {:.0} mm", mm(w), mm(h))
            })
            .unwrap_or_default();
        let yes = |b: bool| {
            if b { tr(de, "Erlaubt", "Allowed") } else { tr(de, "Nicht erlaubt", "Not allowed") }.to_string()
        };
        let info = tab.doc.security();
        let bit = |n: u32| info.is_none_or(|s| s.bits & (1 << (n - 1)) != 0);
        let print = if !perms.print {
            tr(de, "Nicht erlaubt", "Not allowed")
        } else if info.is_some_and(|s| s.revision >= 3) && !bit(12) {
            tr(de, "Niedrige Auflösung", "Low resolution")
        } else {
            tr(de, "Hohe Auflösung", "High resolution")
        };
        let security = vec![
            (tr(de, "Drucken", "Printing").to_string(), print.to_string()),
            (tr(de, "Dokument ändern", "Changing the document").into(), yes(perms.modify)),
            (tr(de, "Aufbau des Dokuments ändern", "Document assembly").into(), yes(perms.assemble)),
            (tr(de, "Inhalt kopieren", "Content copying").into(), yes(perms.copy)),
            (
                tr(de, "Zugriff auf den Inhalt möglich", "Content accessibility").into(),
                yes(perms.copy || bit(10)),
            ),
            (tr(de, "Seiten extrahieren", "Page extraction").into(), yes(perms.copy)),
            (tr(de, "Kommentieren", "Commenting").into(), yes(perms.annotate)),
            (tr(de, "Formularfelder ausfüllen", "Filling of form fields").into(), yes(perms.fill_forms)),
            (tr(de, "Signieren", "Signing").into(), yes(perms.fill_forms)),
            (tr(de, "Erstellung von Vorlageseiten", "Creation of template pages").into(), yes(perms.modify)),
        ];
        let (method, encryption) = match info {
            None if !perms.encrypted => (
                tr(de, "Kein Schutz", "No security").to_string(),
                tr(de, "nicht festgelegt", "not set").to_string(),
            ),
            None => (tr(de, "Schutz durch Kennwort", "Password security").into(), "?".into()),
            Some(s) => (
                tr(de, "Schutz durch Kennwort", "Password security").into(),
                match s.revision {
                    2 => "RC4 40 Bit".to_string(),
                    3 => "RC4 128 Bit".into(),
                    4 => "RC4/AES 128 Bit".into(),
                    5 | 6 => "AES 256 Bit".into(),
                    r => format!("Revision {r}"),
                },
            ),
        };
        let read_only = perms.encrypted || !perms.modify;
        let state = PropsState {
            page: 0,
            search: String::new(),
            original: (meta.clone(), props.clone()),
            meta,
            props,
            fonts,
            file_name: tab
                .doc
                .path()
                .and_then(|p| p.file_name())
                .map_or_else(|| tab.doc.display_name().to_string(), |n| n.to_string_lossy().into_owned()),
            folder: tab.doc.path().and_then(|p| p.parent()).map(PathBuf::from),
            file_size: tab.doc.file_size() as u64,
            version: tab.doc.pdf_version(),
            pages: tab.doc.page_count(),
            page_size,
            security,
            method,
            encryption,
            read_only,
            selected: None,
            editing: None,
            show_xmp: false,
        };
        self.dialog = Some(Dialog::Properties(Box::new(state)));
    }

    /// Draws "Dokumenteigenschaften".
    pub(super) fn properties_dialog(&mut self, ui: &mut Ui, s: &mut PropsState) -> (Outcome, Option<Action>) {
        let de = self.pp_de();
        ui.set_max_width(760.0);
        ui.heading(tr(de, "Dokumenteigenschaften", "Document properties"));
        let mut space_usage = false;
        let mut protect = false;
        ui.horizontal_top(|ui| {
            // Left: search and categories.
            ui.vertical(|ui| {
                ui.set_width(190.0);
                ui.add(
                    egui::TextEdit::singleline(&mut s.search)
                        .hint_text(tr(de, "Suchen… (STRG+F)", "Search… (CTRL+F)"))
                        .desired_width(186.0),
                );
                let query = s.search.trim().to_lowercase();
                for (i, (icon, name, words)) in pages(de).into_iter().enumerate() {
                    if !query.is_empty() && !name.to_lowercase().contains(&query) && !words.contains(&query) {
                        continue;
                    }
                    ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), egui::Sense::hover());
                        crate::ui::icons::paint(
                            ui.painter(),
                            rect,
                            icon,
                            crate::ui::theme::icon_color(ui.visuals().dark_mode),
                        );
                        if ui.selectable_label(s.page == i, name).clicked() && s.page != i {
                            s.page = i;
                            s.selected = None;
                            s.editing = None;
                        }
                    });
                }
                ui.add_space(12.0);
                space_usage =
                    ui.button(tr(de, "Speicherverbrauch untersuchen…", "Examine space usage…")).clicked();
            });
            ui.add_space(8.0);
            ui.vertical(|ui| {
                ui.set_width(520.0);
                ui.set_min_height(380.0);
                ui.add_enabled_ui(!s.read_only || matches!(s.page, 0..=2), |ui| match s.page {
                    0 => self.props_description(ui, de, s),
                    1 => props_fonts(ui, de, s),
                    2 => protect = props_security(ui, de, s),
                    3 => props_view(ui, de, s),
                    4 => props_custom(ui, de, s),
                    5 => props_advanced(ui, de, s),
                    6 => props_actions(ui, de, s),
                    _ => props_javascript(ui, de, s),
                });
                if s.read_only {
                    ui.label(
                        RichText::new(tr(
                            de,
                            "Das Dokument ist geschützt; die Eigenschaften können nur gelesen werden.",
                            "The document is protected; the properties are read only.",
                        ))
                        .small()
                        .weak(),
                    );
                }
            });
        });
        if space_usage {
            return (
                Outcome::Close,
                Some(Box::new(|app, _| {
                    let _ = app.run_optimize(Cmd::SpaceUsage);
                })),
            );
        }
        if protect {
            return (Outcome::Close, Some(Box::new(|app, ctx| app.run(ctx, Cmd::PasswordProtect))));
        }
        let (ok, cancel, _) = super::page_dialogs::footer(ui, de, true, false, "OK");
        if cancel {
            return (Outcome::Close, None);
        }
        if !ok {
            return (Outcome::Keep, None);
        }
        let changed = (s.meta.clone(), s.props.clone()) != s.original;
        if !changed || s.read_only {
            return (Outcome::Close, None);
        }
        let (meta, props) = (s.meta.clone(), s.props.clone());
        (
            Outcome::Close,
            Some(Box::new(move |app, ctx| app.edit(ctx, |tab| tab.doc.set_properties(&meta, &props)))),
        )
    }

    fn props_description(&mut self, ui: &mut Ui, de: bool, s: &mut PropsState) {
        section(ui, tr(de, "Dateiinformationen", "File information"), |ui| {
            egui::Grid::new("props-file").num_columns(2).spacing(vec2(10.0, 4.0)).show(ui, |ui| {
                ui.label(tr(de, "Dateiname:", "File name:"));
                ui.label(&s.file_name);
                ui.end_row();
                ui.label(tr(de, "Ort:", "Location:"));
                ui.horizontal(|ui| match &s.folder {
                    Some(f) => {
                        ui.label(f.display().to_string());
                        if crate::ui::widgets::icon_button(
                            ui,
                            Icon::Open,
                            tr(de, "Ordner öffnen", "Open folder"),
                            false,
                            true,
                        )
                        .clicked()
                            && let Some(path) = self.tabs.get(self.active).and_then(|t| t.doc.path())
                        {
                            super::features::reveal_in_file_manager(path);
                        }
                    }
                    None => {
                        ui.label(RichText::new(tr(de, "<Unbekannt>", "<Unknown>")).weak());
                    }
                });
                ui.end_row();
                ui.label(tr(de, "Dateigröße:", "File size:"));
                ui.label(size_text(s.file_size));
                ui.end_row();
            });
        });
        section(ui, tr(de, "Dokumentinformationen", "Document information"), |ui| {
            let ro = s.read_only;
            egui::Grid::new("props-meta").num_columns(2).spacing(vec2(10.0, 4.0)).show(ui, |ui| {
                for (label, value) in [
                    (tr(de, "Dokumenttitel:", "Title:"), &mut s.meta.title),
                    (tr(de, "Autor:", "Author:"), &mut s.meta.author),
                    (tr(de, "Betreff:", "Subject:"), &mut s.meta.subject),
                ] {
                    ui.label(label);
                    ui.add_enabled(!ro, egui::TextEdit::singleline(value).desired_width(380.0));
                    ui.end_row();
                }
                ui.label(tr(de, "Schlüsselwörter:", "Keywords:"));
                ui.add_enabled(
                    !ro,
                    egui::TextEdit::multiline(&mut s.meta.keywords).desired_rows(2).desired_width(380.0),
                );
                ui.end_row();
                for (label, value) in [
                    (tr(de, "PDF-Produzent:", "PDF producer:"), s.meta.producer.clone()),
                    (tr(de, "Anwendung:", "Application:"), s.meta.creator.clone()),
                    (tr(de, "PDF-Version:", "PDF version:"), s.version.clone()),
                    (tr(de, "Seitenzahl:", "Pages:"), s.pages.to_string()),
                    (tr(de, "Seitengröße:", "Page size:"), s.page_size.clone()),
                    (tr(de, "Erstellt:", "Created:"), s.meta.created.clone()),
                    (tr(de, "Geändert:", "Modified:"), s.meta.modified.clone()),
                ] {
                    ui.label(label);
                    ui.label(value);
                    ui.end_row();
                }
            });
            let has_xmp = s.props.xmp.is_some();
            if ui
                .add_enabled(
                    has_xmp,
                    egui::Button::new(tr(de, "Zusätzliche Metadaten…", "Additional metadata…")),
                )
                .on_disabled_hover_text(tr(
                    de,
                    "Das Dokument enthält keine XMP-Metadaten.",
                    "The document has no XMP metadata.",
                ))
                .clicked()
            {
                s.show_xmp = !s.show_xmp;
            }
            if s.show_xmp
                && let Some(xmp) = &s.props.xmp
            {
                egui::ScrollArea::vertical().max_height(140.0).show(ui, |ui| {
                    ui.add(egui::Label::new(RichText::new(xmp).monospace().small()).wrap());
                });
            }
        });
    }
}

fn props_fonts(ui: &mut Ui, de: bool, s: &mut PropsState) {
    egui::Frame::new().stroke(ui.visuals().widgets.noninteractive.bg_stroke).show(ui, |ui| {
        ui.set_min_size(vec2(510.0, 330.0));
        egui::ScrollArea::vertical().max_height(330.0).show(ui, |ui| {
            egui::Grid::new("props-fonts").num_columns(4).striped(true).spacing(vec2(12.0, 3.0)).show(
                ui,
                |ui| {
                    for h in [
                        tr(de, "Schriftart", "Font"),
                        tr(de, "Typ", "Type"),
                        tr(de, "Kodierung", "Encoding"),
                        tr(de, "Eingebettet", "Embedded"),
                    ] {
                        ui.label(RichText::new(h).strong());
                    }
                    ui.end_row();
                    for f in &s.fonts {
                        ui.label(&f.name);
                        ui.label(&f.kind);
                        ui.label(&f.encoding);
                        ui.label(match (f.embedded, f.subset) {
                            (true, true) => tr(de, "Teilmenge", "Subset"),
                            (true, false) => tr(de, "Ja", "Yes"),
                            _ => tr(de, "Nein", "No"),
                        });
                        ui.end_row();
                    }
                },
            );
        });
    });
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
        ui.label(
            RichText::new(format!("{} {}", s.fonts.len(), tr(de, "Schriftarten gefunden", "fonts found")))
                .small(),
        );
    });
}

/// Returns true for "Einstellungen ändern…".
fn props_security(ui: &mut Ui, de: bool, s: &mut PropsState) -> bool {
    let mut change = false;
    section(ui, tr(de, "Dokumentenschutz", "Document security"), |ui| {
        ui.label(tr(
            de,
            "Der Dokumentenschutz legt fest, welche Änderungen erlaubt sind.",
            "The document security defines which changes are allowed.",
        ));
        egui::Grid::new("props-sec").num_columns(2).spacing(vec2(10.0, 4.0)).show(ui, |ui| {
            ui.label(tr(de, "Sicherungsmethode:", "Security method:"));
            ui.horizontal(|ui| {
                ui.label(RichText::new(&s.method).strong());
                change = ui.button(tr(de, "Einstellungen ändern…", "Change settings…")).clicked();
            });
            ui.end_row();
            ui.label(tr(de, "Verschlüsselung:", "Encryption:"));
            ui.label(&s.encryption);
            ui.end_row();
        });
        ui.label(
            RichText::new(tr(
                de,
                "Schutz durch Zertifikat, Microsoft Purview und FileOpen werden nicht unterstützt.",
                "Certificate security, Microsoft Purview and FileOpen are not supported.",
            ))
            .small()
            .weak(),
        );
    });
    section(ui, tr(de, "Berechtigungen für das Dokument", "Document permissions"), |ui| {
        egui::Grid::new("props-perm").num_columns(2).spacing(vec2(10.0, 3.0)).show(ui, |ui| {
            for (label, value) in &s.security {
                ui.label(format!("{label}:"));
                ui.label(value);
                ui.end_row();
            }
        });
    });
    change
}

fn props_view(ui: &mut Ui, de: bool, s: &mut PropsState) {
    let p = &mut s.props;
    section(ui, tr(de, "Layout beim Öffnen", "Layout when opening"), |ui| {
        egui::Grid::new("props-layout").num_columns(2).spacing(vec2(10.0, 5.0)).show(ui, |ui| {
            ui.label(tr(de, "Navigationsansicht:", "Navigation tab:"));
            combo(
                ui,
                "props-pagemode",
                &mut p.page_mode,
                &[
                    (PageMode::Default, "Standard"),
                    (PageMode::UseNone, tr(de, "Nur Seite", "Page only")),
                    (
                        PageMode::UseOutlines,
                        tr(de, "Lesezeichenleiste und Seite", "Bookmarks panel and page"),
                    ),
                    (PageMode::UseThumbs, tr(de, "Vorschauleiste und Seite", "Thumbnails panel and page")),
                    (PageMode::UseOC, tr(de, "Ebenenleiste und Seite", "Layers panel and page")),
                    (
                        PageMode::UseAttachments,
                        tr(de, "Anhangsleiste und Seite", "Attachments panel and page"),
                    ),
                ],
                260.0,
            );
            ui.end_row();
            ui.label(tr(de, "Seitenlayout:", "Page layout:"));
            combo(
                ui,
                "props-layout",
                &mut p.layout,
                &[
                    (LayoutMode::Default, "Standard"),
                    (LayoutMode::SinglePage, tr(de, "Einzelne Seite", "Single page")),
                    (LayoutMode::OneColumn, tr(de, "Fortlaufend", "Continuous")),
                    (
                        LayoutMode::TwoColumnLeft,
                        tr(de, "Zwei Seiten - Fortlaufend", "Two pages - continuous"),
                    ),
                    (
                        LayoutMode::TwoColumnRight,
                        tr(de, "Zwei Seiten - Fortlaufend mit Titelseite", "Two pages - continuous, cover"),
                    ),
                    (LayoutMode::TwoPageLeft, tr(de, "Zwei Seiten", "Two pages")),
                    (LayoutMode::TwoPageRight, tr(de, "Zwei Seiten mit Titelseite", "Two pages with cover")),
                ],
                260.0,
            );
            ui.end_row();
        });
    });
    section(ui, tr(de, "Aktion beim Öffnen", "Open action"), |ui| {
        let kind = match &p.open_action {
            OpenAction::None => 0,
            OpenAction::GoTo { .. } => 1,
            OpenAction::Other(_) => 2,
        };
        let mut new_kind = kind;
        ui.horizontal(|ui| {
            ui.label(tr(de, "Aktionsart:", "Action type:"));
            let names = [
                tr(de, "nicht festgelegt", "not set"),
                tr(de, "Zu Seite springen", "Go to page"),
                tr(de, "Eigene", "Custom"),
            ];
            egui::ComboBox::from_id_salt("props-action").selected_text(names[kind]).width(200.0).show_ui(
                ui,
                |ui| {
                    ui.selectable_value(&mut new_kind, 0, names[0]);
                    ui.selectable_value(&mut new_kind, 1, names[1]);
                    ui.add_enabled_ui(kind == 2, |ui| {
                        ui.selectable_value(&mut new_kind, 2, names[2]);
                    });
                },
            );
        });
        if new_kind != kind {
            p.open_action = match new_kind {
                1 => OpenAction::GoTo { page: 0, zoom: OpenZoom::Default },
                _ => OpenAction::None,
            };
        }
        match &mut p.open_action {
            OpenAction::GoTo { page, zoom } => {
                ui.horizontal(|ui| {
                    ui.label(tr(de, "Seite:", "Page:"));
                    let mut n = *page + 1;
                    if ui.add(egui::DragValue::new(&mut n).range(1..=s.pages.max(1))).changed() {
                        *page = n - 1;
                    }
                    ui.label(tr(de, "Vergrößerung:", "Zoom:"));
                    combo(
                        ui,
                        "props-zoom",
                        zoom,
                        &[
                            (OpenZoom::Default, "Standard"),
                            (OpenZoom::FitPage, tr(de, "Ganze Seite", "Fit page")),
                            (OpenZoom::FitWidth, tr(de, "Seitenbreite", "Fit width")),
                            (OpenZoom::FitContent, tr(de, "Sichtbarer Inhalt", "Fit visible")),
                        ],
                        150.0,
                    );
                });
            }
            OpenAction::Other(kind) => {
                ui.label(
                    RichText::new(format!(
                        "{} ({kind})",
                        tr(
                            de,
                            "Eine andere Aktion des Dokuments bleibt erhalten",
                            "Another action of the document is kept"
                        )
                    ))
                    .weak(),
                );
            }
            OpenAction::None => {}
        }
    });
    section(ui, tr(de, "Fenster", "Window"), |ui| {
        ui.horizontal(|ui| {
            ui.label(tr(de, "In Titel anzeigen:", "Show in title:"));
            combo(
                ui,
                "props-title",
                &mut p.show_title,
                &[
                    (false, tr(de, "Dateiname", "File name")),
                    (true, tr(de, "Dokumenttitel", "Document title")),
                ],
                160.0,
            );
        });
        ui.checkbox(&mut p.fullscreen, tr(de, "Im Vollbildmodus öffnen", "Open in full screen mode"));
        ui.checkbox(
            &mut p.fit_window,
            tr(de, "Fenstergröße an erste Seite im Dokument anpassen", "Resize window to the first page"),
        );
        ui.checkbox(
            &mut p.center_window,
            tr(de, "Fenster auf Bildschirm zentrieren", "Center window on screen"),
        );
    });
    section(ui, tr(de, "Benutzeroberfläche", "User interface"), |ui| {
        ui.horizontal(|ui| {
            ui.checkbox(&mut p.hide_menubar, tr(de, "Menüleiste verbergen", "Hide menu bar"));
            ui.checkbox(&mut p.hide_toolbar, tr(de, "Symbolleisten verbergen", "Hide toolbars"));
            ui.checkbox(
                &mut p.hide_window_ui,
                tr(de, "Kontrollelemente für Fenster verbergen", "Hide window controls"),
            );
        });
    });
}

/// Toolbar of the list pages: Hinzufügen…, Bearbeiten…, Löschen, Alle entfernen.
fn list_buttons(
    ui: &mut Ui,
    de: bool,
    has_selection: bool,
    can_add: bool,
    all: bool,
) -> (bool, bool, bool, bool) {
    let mut r = (false, false, false, false);
    ui.horizontal(|ui| {
        r.0 = ui.add_enabled(can_add, egui::Button::new(tr(de, "Hinzufügen…", "Add…"))).clicked();
        r.1 = ui.add_enabled(has_selection, egui::Button::new(tr(de, "Bearbeiten…", "Edit…"))).clicked();
        r.2 = ui.add_enabled(has_selection, egui::Button::new(tr(de, "Löschen", "Delete"))).clicked();
        if all {
            r.3 = ui.button(tr(de, "Alle entfernen", "Remove all")).clicked();
        }
    });
    r
}

/// Inline editor of a (name, value) row; returns Some(true) for
/// "Übernehmen", Some(false) for "Abbrechen".
fn edit_row(ui: &mut Ui, de: bool, row: &mut EditRow, multiline: bool, name_fixed: bool) -> Option<bool> {
    let mut result = None;
    egui::Frame::new()
        .stroke(ui.visuals().widgets.active.bg_stroke)
        .inner_margin(egui::Margin::same(6))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(tr(de, "Name:", "Name:"));
                ui.add_enabled(!name_fixed, egui::TextEdit::singleline(&mut row.1).desired_width(200.0));
            });
            if multiline {
                ui.add(
                    egui::TextEdit::multiline(&mut row.2).code_editor().desired_rows(6).desired_width(490.0),
                );
            } else {
                ui.horizontal(|ui| {
                    ui.label(tr(de, "Wert:", "Value:"));
                    ui.add(egui::TextEdit::singleline(&mut row.2).desired_width(380.0));
                });
            }
            ui.horizontal(|ui| {
                if ui.button(tr(de, "Übernehmen", "Apply")).clicked() {
                    result = Some(true);
                }
                if ui.button(tr(de, "Abbrechen", "Cancel")).clicked() {
                    result = Some(false);
                }
            });
        });
    result
}

fn props_custom(ui: &mut Ui, de: bool, s: &mut PropsState) {
    let (add, edit, delete, all) = list_buttons(ui, de, s.selected.is_some(), true, true);
    if add {
        s.editing = Some((None, String::new(), String::new()));
    }
    if edit
        && let Some(i) = s.selected
        && let Some((k, v)) = s.props.custom.get(i)
    {
        s.editing = Some((Some(i), k.clone(), v.clone()));
    }
    if delete && let Some(i) = s.selected {
        if i < s.props.custom.len() {
            s.props.custom.remove(i);
        }
        s.selected = None;
    }
    if all {
        s.props.custom.clear();
        s.selected = None;
    }
    egui::Frame::new().stroke(ui.visuals().widgets.noninteractive.bg_stroke).show(ui, |ui| {
        ui.set_min_size(vec2(510.0, 200.0));
        if s.props.custom.is_empty() {
            // A fixed size: centered_and_justified would fill the whole screen.
            ui.add_sized(
                [500.0, 190.0],
                egui::Label::new(
                    RichText::new(tr(
                        de,
                        "Keine eigenen Eigenschaften definiert",
                        "No custom properties defined",
                    ))
                    .weak(),
                ),
            );
        }
        egui::Grid::new("props-custom").num_columns(2).striped(true).spacing(vec2(12.0, 3.0)).show(
            ui,
            |ui| {
                for (i, (k, v)) in s.props.custom.iter().enumerate() {
                    if ui.selectable_label(s.selected == Some(i), k).clicked() {
                        s.selected = Some(i);
                    }
                    ui.label(v);
                    ui.end_row();
                }
            },
        );
    });
    if let Some(row) = &mut s.editing {
        let valid = crate::doc_props::valid_custom_key(row.1.trim())
            && !s.props.custom.iter().enumerate().any(|(i, (k, _))| k == row.1.trim() && Some(i) != row.0);
        match edit_row(ui, de, row, false, false) {
            Some(true) if valid => {
                let entry = (row.1.trim().to_string(), row.2.clone());
                match row.0 {
                    Some(i) if i < s.props.custom.len() => s.props.custom[i] = entry,
                    _ => s.props.custom.push(entry),
                }
                s.editing = None;
            }
            Some(false) => s.editing = None,
            _ => {}
        }
        if !valid && s.editing.as_ref().is_some_and(|r| !r.1.is_empty()) {
            ui.colored_label(
                egui::Color32::from_rgb(200, 40, 40),
                tr(de, "Ungültiger oder schon vergebener Name.", "Invalid or duplicate name."),
            );
        }
    }
    ui.label(
        RichText::new(tr(
            de,
            "Jede Eigenschaft benötigt einen eindeutigen Namen, der nicht einer der Standardnamen Title, Author, Subject, Keywords, Creator, Producer, CreationDate, ModDate oder Trapped sein darf.",
            "Every property needs a unique name that is not one of the standard names Title, Author, Subject, Keywords, Creator, Producer, CreationDate, ModDate or Trapped.",
        ))
        .small()
        .weak(),
    );
}

fn props_advanced(ui: &mut Ui, de: bool, s: &mut PropsState) {
    let p = &mut s.props;
    section(ui, tr(de, "PDF-Einstellungen", "PDF settings"), |ui| {
        egui::Grid::new("props-pdf").num_columns(2).spacing(vec2(10.0, 5.0)).show(ui, |ui| {
            ui.label(tr(de, "Basis-URL:", "Base URL:"));
            ui.add(egui::TextEdit::singleline(&mut p.base_url).desired_width(330.0));
            ui.end_row();
            ui.label(tr(de, "Eingeschränkt:", "Trapped:"));
            combo(
                ui,
                "props-trapped",
                &mut p.trapped,
                &[
                    (Trapped::Yes, tr(de, "Ja", "Yes")),
                    (Trapped::No, tr(de, "Nein", "No")),
                    (Trapped::Unknown, tr(de, "Unbekannt", "Unknown")),
                ],
                150.0,
            );
            ui.end_row();
        });
    });
    section(ui, tr(de, "Voreinstellungen für Druckdialog", "Print dialog presets"), |ui| {
        egui::Grid::new("props-print").num_columns(2).spacing(vec2(10.0, 5.0)).show(ui, |ui| {
            ui.label(tr(de, "Skalierung:", "Scaling:"));
            combo(
                ui,
                "props-scaling",
                &mut p.print_no_scaling,
                &[(false, "Standard"), (true, tr(de, "Keine Skalierung", "None"))],
                150.0,
            );
            ui.end_row();
            ui.label(tr(de, "Duplex-Modus:", "Duplex mode:"));
            combo(
                ui,
                "props-duplex",
                &mut p.duplex,
                &[
                    (DuplexPreset::Default, "Standard"),
                    (DuplexPreset::Simplex, tr(de, "Ohne", "None")),
                    (DuplexPreset::LongEdge, tr(de, "Lange Seite", "Long edge")),
                    (DuplexPreset::ShortEdge, tr(de, "Kurze Seite", "Short edge")),
                ],
                150.0,
            );
            ui.end_row();
            ui.label("");
            ui.checkbox(
                &mut p.pick_tray,
                tr(de, "Papiergröße nach Seitengröße wählen", "Choose paper source by page size"),
            );
            ui.end_row();
            ui.label(tr(de, "Seitenbereich drucken:", "Print page range:"));
            ui.horizontal(|ui| {
                let valid = crate::doc_props::valid_print_range(&p.print_range, s.pages);
                ui.add(
                    egui::TextEdit::singleline(&mut p.print_range)
                        .desired_width(160.0)
                        .hint_text("1-3, 5")
                        .text_color_opt((!valid).then_some(egui::Color32::from_rgb(200, 40, 40))),
                );
                ui.label(format!("({} {} {})", tr(de, "insg.", "total"), s.pages, tr(de, "Seiten", "pages")));
            });
            ui.end_row();
            ui.label(tr(de, "Anzahl der Kopien:", "Number of copies:"));
            let mut n = p.copies.unwrap_or(0);
            egui::ComboBox::from_id_salt("props-copies")
                .selected_text(if n == 0 { "Standard".to_string() } else { n.to_string() })
                .width(150.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut n, 0, "Standard");
                    for c in 2..=5 {
                        ui.selectable_value(&mut n, c, c.to_string());
                    }
                });
            p.copies = (n > 0).then_some(n);
            ui.end_row();
        });
    });
    section(ui, tr(de, "Vorleseeinstellungen", "Reading options"), |ui| {
        egui::Grid::new("props-read").num_columns(2).spacing(vec2(10.0, 5.0)).show(ui, |ui| {
            ui.label(tr(de, "Bindung:", "Binding:"));
            combo(
                ui,
                "props-binding",
                &mut p.right_to_left,
                &[
                    (false, tr(de, "Links nach Rechts", "Left to right")),
                    (true, tr(de, "Rechts nach Links", "Right to left")),
                ],
                160.0,
            );
            ui.end_row();
            ui.label(tr(de, "Sprache:", "Language:"));
            ui.add(egui::TextEdit::singleline(&mut p.language).desired_width(160.0).hint_text("de-DE"));
            ui.end_row();
        });
    });
}

fn props_actions(ui: &mut Ui, de: bool, s: &mut PropsState) {
    let has = s.selected.is_some();
    let mut edit = false;
    let mut clear = false;
    ui.horizontal(|ui| {
        edit = ui.add_enabled(has, egui::Button::new(tr(de, "Bearbeiten…", "Edit…"))).clicked();
        clear = ui.add_enabled(has, egui::Button::new(tr(de, "Leeren", "Clear"))).clicked();
    });
    egui::Frame::new().stroke(ui.visuals().widgets.noninteractive.bg_stroke).show(ui, |ui| {
        ui.set_min_size(vec2(510.0, 220.0));
        for (i, key) in TRIGGERS.iter().enumerate() {
            let value = s.props.actions.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
            if ui
                .selectable_label(s.selected == Some(i), RichText::new(trigger_name(key, de)).strong())
                .clicked()
            {
                s.selected = Some(i);
            }
            ui.label(
                RichText::new(value.unwrap_or_else(|| tr(de, "<Leer>", "<Empty>").into()))
                    .monospace()
                    .small()
                    .weak(),
            );
        }
    });
    if let Some(i) = s.selected {
        let key = TRIGGERS[i];
        if clear {
            s.props.actions.retain(|(k, _)| k != key);
        }
        if edit {
            let code =
                s.props.actions.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()).unwrap_or_default();
            s.editing = Some((
                Some(i),
                trigger_name(key, de).to_string(),
                if code.starts_with('(') { String::new() } else { code },
            ));
        }
    }
    if let Some(row) = &mut s.editing {
        match edit_row(ui, de, row, true, true) {
            Some(true) => {
                let key = TRIGGERS[row.0.unwrap_or(0).min(4)];
                s.props.actions.retain(|(k, _)| k != key);
                if !row.2.trim().is_empty() {
                    s.props.actions.push((key.to_string(), row.2.clone()));
                }
                s.editing = None;
            }
            Some(false) => s.editing = None,
            None => {}
        }
    }
    ui.label(
        RichText::new(tr(
            de,
            "MrPDF speichert JavaScript, führt es aber aus Sicherheitsgründen nicht aus.",
            "MrPDF stores JavaScript but does not run it for security reasons.",
        ))
        .small()
        .weak(),
    );
}

fn props_javascript(ui: &mut Ui, de: bool, s: &mut PropsState) {
    let (add, edit, delete, _) = list_buttons(ui, de, s.selected.is_some(), true, false);
    if add {
        s.editing = Some((None, String::new(), String::new()));
    }
    if edit
        && let Some(i) = s.selected
        && let Some((k, v)) = s.props.javascript.get(i)
    {
        s.editing = Some((Some(i), k.clone(), v.clone()));
    }
    if delete && let Some(i) = s.selected {
        if i < s.props.javascript.len() {
            s.props.javascript.remove(i);
        }
        s.selected = None;
    }
    egui::Frame::new().stroke(ui.visuals().widgets.noninteractive.bg_stroke).show(ui, |ui| {
        ui.set_min_size(vec2(510.0, 200.0));
        for (i, (name, code)) in s.props.javascript.iter().enumerate() {
            if ui.selectable_label(s.selected == Some(i), RichText::new(name).strong()).clicked() {
                s.selected = Some(i);
            }
            ui.label(RichText::new(code.lines().next().unwrap_or("")).monospace().small().weak());
        }
    });
    if let Some(row) = &mut s.editing {
        let valid = !row.1.trim().is_empty()
            && !s
                .props
                .javascript
                .iter()
                .enumerate()
                .any(|(i, (k, _))| k == row.1.trim() && Some(i) != row.0);
        match edit_row(ui, de, row, true, false) {
            Some(true) if valid => {
                let entry = (row.1.trim().to_string(), row.2.clone());
                match row.0 {
                    Some(i) if i < s.props.javascript.len() => s.props.javascript[i] = entry,
                    _ => s.props.javascript.push(entry),
                }
                s.editing = None;
            }
            Some(false) => s.editing = None,
            _ => {}
        }
    }
    ui.label(
        RichText::new(tr(
            de,
            "MrPDF speichert JavaScript, führt es aber aus Sicherheitsgründen nicht aus.",
            "MrPDF stores JavaScript but does not run it for security reasons.",
        ))
        .small()
        .weak(),
    );
}
