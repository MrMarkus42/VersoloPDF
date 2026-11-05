//! "Dokument signieren" and "Signaturen überprüfen" (digital signatures
//! with a digital ID, see `crate::sign`).

use std::path::{Path, PathBuf};

use egui::{Color32, Context, RichText, Ui};

use super::dialogs::{Action, Outcome};
use super::page_dialogs::{footer, tr};
use super::{Dialog, MrPdfApp};
use crate::geometry::RectF;
use crate::sign::cert::NewId;
use crate::sign::cms::Check;
use crate::sign::id::{self, DigitalId};
use crate::sign::pdf::{self as sigpdf, Place, SigInfo, SignOptions};
use crate::ui::i18n::Language;

/// Which digital ID signs.
#[derive(Debug, Clone, PartialEq)]
pub enum IdChoice {
    /// A .pfx file from MrPDF's folder of digital IDs.
    Stored(PathBuf),
    /// Another .pfx / .p12 file.
    File(Option<PathBuf>),
    /// A new self-signed ID.
    New,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    Visible,
    Invisible,
    Field,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corner {
    BottomRight,
    BottomLeft,
    TopRight,
    TopLeft,
}

/// Names of the corners in the order of `Settings::sign_corner`.
pub(super) fn corner_names(de: bool) -> Vec<&'static str> {
    Corner::ALL.iter().map(|c| c.label(de)).collect()
}

impl Corner {
    const ALL: [Corner; 4] = [Corner::BottomRight, Corner::BottomLeft, Corner::TopRight, Corner::TopLeft];

    fn label(self, de: bool) -> &'static str {
        match self {
            Corner::BottomRight => tr(de, "Unten rechts", "Bottom right"),
            Corner::BottomLeft => tr(de, "Unten links", "Bottom left"),
            Corner::TopRight => tr(de, "Oben rechts", "Top right"),
            Corner::TopLeft => tr(de, "Oben links", "Top left"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SignState {
    stored: Vec<PathBuf>,
    choice: IdChoice,
    password: String,
    new_name: String,
    new_email: String,
    new_org: String,
    new_repeat: String,
    reason: String,
    location: String,
    contact: String,
    placement: Placement,
    page: usize,
    pages: usize,
    corner: Corner,
    fields: Vec<String>,
    field: String,
    /// The document has unsaved changes / signatures already.
    modified: bool,
    signed: bool,
    error: Option<String>,
    focused: bool,
}

fn file_label(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// A file name for a new ID from the person's name.
fn id_file_name(dir: &Path, name: &str) -> PathBuf {
    let base: String =
        name.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' }).collect();
    let base = base.trim().to_string();
    let base = if base.is_empty() { "Digitale ID".to_string() } else { base };
    (1..)
        .map(|n| dir.join(if n == 1 { format!("{base}.pfx") } else { format!("{base} ({n}).pfx") }))
        .find(|p| !p.exists())
        .unwrap_or_else(|| dir.join("Digitale ID.pfx"))
}

/// The rectangle of a visible signature in a page corner.
fn corner_rect(crop: RectF, corner: Corner) -> RectF {
    let (w, h, m) = (200.0f32.min(crop.width() - 20.0), 60.0f32.min(crop.height() - 20.0), 36.0f32);
    let m = m.min((crop.width() - w) / 2.0).min((crop.height() - h) / 2.0).max(0.0);
    let x0 = match corner {
        Corner::BottomLeft | Corner::TopLeft => crop.x0 + m,
        _ => crop.x1 - m - w,
    };
    let y0 = match corner {
        Corner::BottomLeft | Corner::BottomRight => crop.y0 + m,
        _ => crop.y1 - m - h,
    };
    RectF::new(x0, y0, x0 + w, y0 + h)
}

/// "2027-11-04 18:49:33 UTC" as "04.11.2027".
fn day(s: &str) -> String {
    let b = s.as_bytes();
    if b.len() >= 10 && b[4] == b'-' && b[7] == b'-' {
        format!("{}.{}.{}", &s[8..10], &s[5..7], &s[0..4])
    } else {
        s.to_string()
    }
}

/// Text of the check result.
pub fn check_text(s: &SigInfo, de: bool) -> (bool, String) {
    match &s.check {
        Check::Valid if s.whole_file => (
            true,
            tr(
                de,
                "Gültig – das Dokument wurde seit dem Signieren nicht verändert.",
                "Valid – the document has not been changed since it was signed.",
            )
            .into(),
        ),
        Check::Valid => (
            true,
            tr(
                de,
                "Gültig für die signierte Fassung – das Dokument wurde danach noch geändert (z. B. weitere Signaturen oder Anmerkungen).",
                "Valid for the signed version – the document was changed afterwards (e.g. more signatures or comments).",
            )
            .into(),
        ),
        Check::DigestMismatch => (
            false,
            tr(
                de,
                "Ungültig – der signierte Inhalt wurde nachträglich verändert.",
                "Invalid – the signed content was changed afterwards.",
            )
            .into(),
        ),
        Check::BadSignature => {
            (false, tr(de, "Ungültig – die Signatur ist beschädigt.", "Invalid – the signature is damaged.").into())
        }
        Check::Unsupported(why) => (false, format!("{}: {why}", tr(de, "Nicht prüfbar", "Cannot be checked"))),
    }
}

impl MrPdfApp {
    fn sign_de(&self) -> bool {
        self.settings.language == Language::German
    }

    /// "Dokument signieren".
    pub(super) fn open_sign_dialog(&mut self) {
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        let modified = tab.doc.is_modified();
        let pages = tab.doc.page_count();
        let page = tab.current_page.min(pages.saturating_sub(1));
        let (fields, signed) = match tab.doc.to_bytes() {
            Ok(b) => {
                let signed = crate::sign::pdf::has_signatures(&b);
                (sigpdf::empty_signature_fields(&b), signed)
            }
            Err(_) => (Vec::new(), false),
        };
        let stored = id::ids_dir().map(|d| id::list(&d)).unwrap_or_default();
        let choice = match stored.first() {
            Some(p) => IdChoice::Stored(p.clone()),
            None => IdChoice::New,
        };
        let placement = if fields.is_empty() { Placement::Visible } else { Placement::Field };
        self.dialog = Some(Dialog::Sign(Box::new(SignState {
            stored,
            choice,
            password: String::new(),
            new_name: String::new(),
            new_email: String::new(),
            new_org: String::new(),
            new_repeat: String::new(),
            // Presets from Einstellungen › Signaturen.
            reason: self.settings.sign_reason.clone(),
            location: self.settings.sign_location.clone(),
            contact: self.settings.sign_contact.clone(),
            placement,
            page,
            pages,
            corner: Corner::ALL
                .get(usize::from(self.settings.sign_corner))
                .copied()
                .unwrap_or(Corner::BottomRight),
            field: fields.first().cloned().unwrap_or_default(),
            fields,
            modified,
            signed,
            error: None,
            focused: false,
        })));
    }

    /// Loads or creates the chosen digital ID.
    fn signing_id(s: &SignState, de: bool) -> Result<DigitalId, String> {
        let read = |p: &Path| -> Result<DigitalId, String> {
            let data = std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?;
            DigitalId::from_pfx(&data, &s.password).map_err(|e| e.to_string())
        };
        match &s.choice {
            IdChoice::Stored(p) => read(p),
            IdChoice::File(Some(p)) => read(p),
            IdChoice::File(None) => Err(tr(
                de,
                "Bitte eine Datei mit der digitalen ID wählen.",
                "Please choose a digital ID file.",
            )
            .into()),
            IdChoice::New => {
                if s.new_name.trim().is_empty() {
                    return Err(tr(de, "Bitte einen Namen eingeben.", "Please enter a name.").into());
                }
                if s.password.chars().count() < 6 {
                    return Err(tr(
                        de,
                        "Das Kennwort muss mindestens 6 Zeichen haben.",
                        "The password needs at least 6 characters.",
                    )
                    .into());
                }
                if s.password != s.new_repeat {
                    return Err(tr(de, "Die Kennwörter sind verschieden.", "The passwords differ.").into());
                }
                let new = DigitalId::create(&NewId {
                    name: s.new_name.trim(),
                    email: s.new_email.trim(),
                    organisation: s.new_org.trim(),
                    years: 5,
                })
                .map_err(|e| e.to_string())?;
                let pfx = new.to_pfx(&s.password).map_err(|e| e.to_string())?;
                let dir = id::ids_dir().ok_or_else(|| "Kein Datenordner".to_string())?;
                std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                let path = id_file_name(&dir, s.new_name.trim());
                std::fs::write(&path, pfx).map_err(|e| format!("{}: {e}", path.display()))?;
                log::info!("Neue digitale ID gespeichert: {}", path.display());
                Ok(new)
            }
        }
    }

    pub(super) fn sign_dialog(&mut self, ui: &mut Ui, s: &mut SignState) -> (Outcome, Option<Action>) {
        let de = self.sign_de();
        super::dialogs::set_title(ui, tr(de, "Dokument signieren", "Sign document"));
        ui.set_width(460.0);
        let strong = |ui: &mut Ui, text: &str| {
            ui.add_space(6.0);
            ui.label(RichText::new(text).strong());
        };

        strong(ui, tr(de, "Digitale ID", "Digital ID"));
        let shown = match &s.choice {
            IdChoice::Stored(p) => file_label(p),
            IdChoice::File(_) => tr(de, "Andere Datei (.pfx, .p12) …", "Other file (.pfx, .p12) …").into(),
            IdChoice::New => tr(de, "Neue digitale ID erstellen …", "Create a new digital ID …").into(),
        };
        egui::ComboBox::from_id_salt("sign-id").width(ui.available_width()).selected_text(shown).show_ui(
            ui,
            |ui| {
                for p in &s.stored {
                    let c = IdChoice::Stored(p.clone());
                    if ui.selectable_label(s.choice == c, file_label(p)).clicked() {
                        s.choice = c;
                    }
                }
                let file = matches!(s.choice, IdChoice::File(_));
                if ui
                    .selectable_label(
                        file,
                        tr(de, "Andere Datei (.pfx, .p12) …", "Other file (.pfx, .p12) …"),
                    )
                    .clicked()
                    && !file
                {
                    s.choice = IdChoice::File(None);
                }
                if ui
                    .selectable_label(
                        s.choice == IdChoice::New,
                        tr(de, "Neue digitale ID erstellen …", "Create a new digital ID …"),
                    )
                    .clicked()
                {
                    s.choice = IdChoice::New;
                }
            },
        );
        if let IdChoice::File(path) = &mut s.choice {
            ui.horizontal(|ui| {
                if ui.button(tr(de, "Datei auswählen …", "Choose file …")).clicked()
                    && let Some(p) = rfd::FileDialog::new()
                        .add_filter(tr(de, "Digitale ID", "Digital ID"), &["pfx", "p12", "PFX", "P12"])
                        .pick_file()
                {
                    *path = Some(p);
                }
                let name =
                    path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned());
                ui.label(name.unwrap_or_else(|| tr(de, "(keine Datei gewählt)", "(no file chosen)").into()));
            });
        }
        egui::Grid::new("sign-id-grid").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            if s.choice == IdChoice::New {
                ui.label(tr(de, "Name:", "Name:"));
                ui.text_edit_singleline(&mut s.new_name);
                ui.end_row();
                ui.label(tr(de, "E-Mail:", "E-mail:"));
                ui.text_edit_singleline(&mut s.new_email);
                ui.end_row();
                ui.label(tr(de, "Organisation:", "Organisation:"));
                ui.text_edit_singleline(&mut s.new_org);
                ui.end_row();
            }
            let label = ui.label(tr(de, "Kennwort:", "Password:"));
            let pw = ui.add(egui::TextEdit::singleline(&mut s.password).password(true)).labelled_by(label.id);
            // The password is what is needed first for a stored ID.
            if !s.focused {
                s.focused = true;
                if s.choice != IdChoice::New {
                    pw.request_focus();
                }
            }
            ui.end_row();
            if s.choice == IdChoice::New {
                ui.label(tr(de, "Kennwort wiederholen:", "Repeat password:"));
                ui.add(egui::TextEdit::singleline(&mut s.new_repeat).password(true));
                ui.end_row();
            }
        });
        if s.choice == IdChoice::New {
            ui.label(
                RichText::new(tr(
                    de,
                    "Die neue ID wird kennwortgeschützt in MrPDFs Ordner für digitale IDs gespeichert. Sie ist selbst ausgestellt: Empfänger sehen eine gültige Signatur, aber die Identität ist nicht von einer Zertifizierungsstelle bestätigt.",
                    "The new ID is stored password protected in MrPDF's folder of digital IDs. It is self-signed: recipients see a valid signature, but no certificate authority confirms the identity.",
                ))
                .small()
                .weak(),
            );
        }

        strong(ui, tr(de, "Angaben", "Details"));
        egui::Grid::new("sign-info").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label(tr(de, "Grund:", "Reason:"));
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut s.reason).desired_width(200.0));
                egui::ComboBox::from_id_salt("sign-reason").width(24.0).selected_text("").show_ui(ui, |ui| {
                    let reasons: &[&str] = if de {
                        &[
                            "Ich bin der Verfasser dieses Dokuments",
                            "Ich habe dieses Dokument geprüft",
                            "Ich stimme diesem Dokument zu",
                            "Ich bestätige die Richtigkeit",
                        ]
                    } else {
                        &[
                            "I am the author of this document",
                            "I have reviewed this document",
                            "I approve this document",
                            "I confirm the accuracy",
                        ]
                    };
                    for r in reasons {
                        if ui.selectable_label(s.reason == *r, *r).clicked() {
                            s.reason = (*r).to_string();
                        }
                    }
                });
            });
            ui.end_row();
            ui.label(tr(de, "Ort:", "Location:"));
            ui.add(egui::TextEdit::singleline(&mut s.location).desired_width(200.0));
            ui.end_row();
            ui.label(tr(de, "Kontakt:", "Contact:"));
            ui.add(egui::TextEdit::singleline(&mut s.contact).desired_width(200.0));
            ui.end_row();
        });

        strong(ui, tr(de, "Darstellung", "Appearance"));
        ui.horizontal(|ui| {
            ui.radio_value(&mut s.placement, Placement::Visible, tr(de, "Sichtbar", "Visible"));
            ui.radio_value(&mut s.placement, Placement::Invisible, tr(de, "Unsichtbar", "Invisible"));
            if !s.fields.is_empty() {
                ui.radio_value(
                    &mut s.placement,
                    Placement::Field,
                    tr(de, "In Signaturfeld", "In signature field"),
                );
            }
        });
        match s.placement {
            Placement::Visible => {
                ui.horizontal(|ui| {
                    ui.label(tr(de, "Seite:", "Page:"));
                    let mut shown = s.page + 1;
                    ui.add(egui::DragValue::new(&mut shown).range(1..=s.pages.max(1)));
                    s.page = shown.saturating_sub(1);
                    ui.add_space(12.0);
                    ui.label(tr(de, "Position:", "Position:"));
                    egui::ComboBox::from_id_salt("sign-corner").selected_text(s.corner.label(de)).show_ui(
                        ui,
                        |ui| {
                            for c in Corner::ALL {
                                ui.selectable_value(&mut s.corner, c, c.label(de));
                            }
                        },
                    );
                });
            }
            Placement::Field => {
                ui.horizontal(|ui| {
                    ui.label(tr(de, "Feld:", "Field:"));
                    egui::ComboBox::from_id_salt("sign-field").selected_text(s.field.as_str()).show_ui(
                        ui,
                        |ui| {
                            for f in &s.fields {
                                ui.selectable_value(&mut s.field, f.clone(), f.as_str());
                            }
                        },
                    );
                });
            }
            Placement::Invisible => {
                ui.label(
                    RichText::new(tr(
                        de,
                        "Die Signatur ist nur in der Signaturprüfung zu sehen.",
                        "The signature only shows when signatures are checked.",
                    ))
                    .small()
                    .weak(),
                );
            }
        }
        if s.modified && s.signed {
            ui.add_space(4.0);
            ui.colored_label(
                Color32::from_rgb(190, 110, 0),
                tr(
                    de,
                    "Das Dokument hat ungespeicherte Änderungen. Beim Signieren wird es neu geschrieben; die vorhandenen Signaturen werden dadurch ungültig.",
                    "The document has unsaved changes. Signing rewrites it, which makes the existing signatures invalid.",
                ),
            );
        }
        if let Some(e) = &s.error {
            ui.add_space(4.0);
            ui.colored_label(Color32::from_rgb(200, 40, 40), e);
        }
        ui.add_space(4.0);
        ui.label(
            RichText::new(tr(
                de,
                "Nach „Signieren“ wählen Sie, wo die signierte Datei gespeichert wird.",
                "After “Sign” you choose where the signed file is saved.",
            ))
            .small()
            .weak(),
        );

        let (ok, cancel, _) = footer(ui, de, true, false, tr(de, "Signieren", "Sign"));
        if cancel {
            return (Outcome::Close, None);
        }
        if !ok {
            return (Outcome::Keep, None);
        }
        let id = match Self::signing_id(s, de) {
            Ok(id) => id,
            Err(e) => {
                // A wrong password is typed again: empty field with the focus.
                if e.contains("Kennwort") && s.choice != IdChoice::New {
                    s.password.clear();
                    s.focused = false;
                }
                s.error = Some(e);
                return (Outcome::Keep, None);
            }
        };
        let place = match s.placement {
            Placement::Invisible => Place::Invisible,
            Placement::Field => Place::Field(s.field.clone()),
            Placement::Visible => {
                let crop = self
                    .tabs
                    .get(self.active)
                    .and_then(|t| t.doc.geometry(s.page))
                    .map(|g| g.crop)
                    .unwrap_or(RectF::new(0.0, 0.0, 595.0, 842.0));
                Place::Rect { page: s.page, rect: corner_rect(crop, s.corner) }
            }
        };
        let opts = SignOptions {
            place,
            reason: s.reason.trim().into(),
            location: s.location.trim().into(),
            contact: s.contact.trim().into(),
        };
        (Outcome::Close, Some(Box::new(move |app, ctx| app.sign_active(ctx, &id, &opts))))
    }

    /// Signs the active document and saves the result where the user wants.
    fn sign_active(&mut self, ctx: &Context, id: &DigitalId, opts: &SignOptions) {
        let de = self.sign_de();
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        let bytes = match tab.doc.to_bytes() {
            Ok(b) => b,
            Err(e) => return self.show_error(&e),
        };
        let current = tab.doc.path().map(Path::to_path_buf);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        let signed = match sigpdf::sign(&bytes, id, opts, now, sigpdf::local_offset_minutes()) {
            Ok(b) => b,
            Err(e) => return self.show_error(&e),
        };
        let stem = current
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| tab.title().trim_end_matches(".pdf").to_string());
        let mut dialog = rfd::FileDialog::new()
            .add_filter("PDF", &["pdf"])
            .set_file_name(format!("{stem}{}.pdf", tr(de, "_signiert", "_signed")));
        if let Some(dir) = current.as_ref().and_then(|p| p.parent()) {
            dialog = dialog.set_directory(dir);
        }
        // MRPDF_SIGN_SAVE_PATH replaces the file dialog (tests).
        let chosen = match std::env::var_os("MRPDF_SIGN_SAVE_PATH") {
            Some(p) => Some(PathBuf::from(p)),
            None => dialog.save_file(),
        };
        let Some(dest) = chosen else { return };
        if let Err(e) = std::fs::write(&dest, &signed) {
            return self.show_error(&crate::Error::io(&dest, e));
        }
        log::info!("Dokument signiert: {}", dest.display());
        // The signed file replaces an open document of the same name, else opens.
        let canonical = crate::files::canonical_path(&dest);
        if let Some(i) = self
            .tabs
            .iter()
            .position(|t| t.doc.path().is_some_and(|p| crate::files::canonical_path(p) == canonical))
        {
            self.active = i;
            self.revert(ctx);
        } else {
            self.open_path(ctx, dest, None);
        }
        self.set_status(ctx, tr(de, "Dokument signiert und gespeichert", "Document signed and saved"));
    }

    /// "Signaturen überprüfen".
    pub(super) fn show_signatures(&mut self, ctx: &Context) {
        let de = self.sign_de();
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        let sigs = tab.doc.to_bytes().map(|b| sigpdf::signatures(&b)).unwrap_or_default();
        if sigs.is_empty() {
            self.set_status(
                ctx,
                tr(
                    de,
                    "Das Dokument enthält keine digitalen Signaturen",
                    "The document has no digital signatures",
                ),
            );
            return;
        }
        self.dialog = Some(Dialog::Signatures(sigs));
    }

    pub(super) fn signatures_dialog(&mut self, ui: &mut Ui, sigs: &[SigInfo]) -> Outcome {
        let de = self.sign_de();
        super::dialogs::set_title(ui, tr(de, "Signaturen überprüfen", "Check signatures"));
        ui.set_width(520.0);
        let modified = self.tabs.get(self.active).is_some_and(|t| t.doc.is_modified());
        for (i, s) in sigs.iter().enumerate() {
            if i > 0 {
                ui.separator();
            }
            let (good, text) = check_text(s, de);
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::hover());
                let color =
                    if good { Color32::from_rgb(30, 140, 60) } else { Color32::from_rgb(200, 40, 40) };
                ui.painter().circle_filled(rect.center(), 10.0, color);
                let c = rect.center();
                let st = egui::Stroke::new(2.0, Color32::WHITE);
                if good {
                    ui.painter().line_segment([c + egui::vec2(-5.0, 0.0), c + egui::vec2(-1.5, 4.0)], st);
                    ui.painter().line_segment([c + egui::vec2(-1.5, 4.0), c + egui::vec2(5.0, -4.0)], st);
                } else {
                    ui.painter().line_segment([c + egui::vec2(-4.0, -4.0), c + egui::vec2(4.0, 4.0)], st);
                    ui.painter().line_segment([c + egui::vec2(-4.0, 4.0), c + egui::vec2(4.0, -4.0)], st);
                }
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "{} {} – {}",
                            tr(de, "Signatur", "Signature"),
                            i + 1,
                            if s.signer.is_empty() { s.field.as_str() } else { s.signer.as_str() }
                        ))
                        .strong(),
                    );
                    ui.label(text);
                });
            });
            egui::Grid::new(("sig-info", i)).num_columns(2).spacing([12.0, 3.0]).show(ui, |ui| {
                let mut row = |k: &str, v: &str| {
                    if !v.is_empty() {
                        ui.label(RichText::new(k).weak());
                        ui.label(v);
                        ui.end_row();
                    }
                };
                row(tr(de, "Signiert am:", "Signed on:"), &s.time);
                row(tr(de, "Grund:", "Reason:"), &s.reason);
                row(tr(de, "Ort:", "Location:"), &s.location);
                row(tr(de, "Feld:", "Field:"), &s.field);
                row(tr(de, "Zertifikat gültig bis:", "Certificate valid until:"), &day(&s.cert_valid_until));
                let who = if s.self_signed {
                    tr(
                        de,
                        "Selbst ausgestellte digitale ID – die Identität ist nicht von einer Zertifizierungsstelle bestätigt.",
                        "Self-signed digital ID – no certificate authority confirms the identity.",
                    )
                    .to_string()
                } else if !s.issuer.is_empty() {
                    format!(
                        "{} {} {}",
                        tr(de, "Ausgestellt von", "Issued by"),
                        s.issuer,
                        tr(de, "(Vertrauen in den Aussteller wird nicht geprüft)", "(trust in the issuer is not checked)")
                    )
                } else {
                    String::new()
                };
                row(tr(de, "Identität:", "Identity:"), &who);
            });
        }
        if modified {
            ui.add_space(6.0);
            ui.colored_label(
                Color32::from_rgb(190, 110, 0),
                tr(
                    de,
                    "Hinweis: Das Dokument hat ungespeicherte Änderungen. Beim Speichern werden die Signaturen ungültig.",
                    "Note: the document has unsaved changes. Saving makes the signatures invalid.",
                ),
            );
        }
        ui.add_space(8.0);
        let close = ui.button(tr(de, "Schließen", "Close")).clicked()
            || ui.input(|i| i.key_pressed(egui::Key::Escape) || i.key_pressed(egui::Key::Enter));
        if close { Outcome::Close } else { Outcome::Keep }
    }

    /// Status after saving tab `index`: warns when saving broke signatures.
    pub(super) fn saved_message(&mut self, index: usize, saved: &str) -> String {
        let de = self.sign_de();
        let Some(tab) = self.tabs.get_mut(index) else { return saved.into() };
        let Ok(bytes) = tab.doc.to_bytes() else { return saved.into() };
        if !sigpdf::has_signatures(&bytes) {
            return saved.into();
        }
        let bad = sigpdf::signatures(&bytes).iter().filter(|s| !s.valid()).count();
        if bad == 0 {
            return saved.into();
        }
        format!(
            "{saved} – {}",
            tr(
                de,
                "Achtung: Durch die Änderungen sind digitale Signaturen ungültig geworden.",
                "Warning: the changes made digital signatures invalid.",
            )
        )
    }

    /// Status hint after opening a signed document.
    pub(super) fn note_signatures(&mut self, ctx: &Context) {
        let de = self.sign_de();
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        let Ok(bytes) = tab.doc.to_bytes() else { return };
        if !sigpdf::has_signatures(&bytes) {
            return;
        }
        let sigs = sigpdf::signatures(&bytes);
        if sigs.is_empty() {
            return;
        }
        let bad = sigs.iter().filter(|s| !s.valid()).count();
        let text = match (sigs.len(), bad, de) {
            (n, 0, true) => format!(
                "Digital signiert: {n} gültige Signatur{} – Details unter Schützen › Signaturen überprüfen",
                if n == 1 { "" } else { "en" }
            ),
            (n, 0, false) => format!(
                "Digitally signed: {n} valid signature{} – details under Protect › Check signatures",
                if n == 1 { "" } else { "s" }
            ),
            (n, b, true) => {
                format!("Achtung: {b} von {n} Signaturen ungültig – Schützen › Signaturen überprüfen")
            }
            (n, b, false) => format!("Warning: {b} of {n} signatures invalid – Protect › Check signatures"),
        };
        self.set_status(ctx, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_stay_on_the_page() {
        let a4 = RectF::new(0.0, 0.0, 595.0, 842.0);
        for c in Corner::ALL {
            let r = corner_rect(a4, c);
            assert!(r.x0 >= 0.0 && r.y0 >= 0.0 && r.x1 <= 595.0 && r.y1 <= 842.0, "{c:?} {r:?}");
            assert!((r.width() - 200.0).abs() < 0.1 && (r.height() - 60.0).abs() < 0.1);
        }
        let tiny = RectF::new(10.0, 10.0, 150.0, 70.0);
        let r = corner_rect(tiny, Corner::TopLeft);
        assert!(r.x0 >= 10.0 && r.x1 <= 150.0 && r.y0 >= 10.0 && r.y1 <= 70.0, "{r:?}");
    }

    #[test]
    fn id_names_do_not_overwrite() {
        let dir = std::env::temp_dir().join(format!("mrpdf-ids-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let first = id_file_name(&dir, "Erika M/ustermann");
        assert_eq!(first.file_name().unwrap(), "Erika M_ustermann.pfx");
        std::fs::write(&first, b"x").unwrap();
        assert_eq!(id_file_name(&dir, "Erika M/ustermann").file_name().unwrap(), "Erika M_ustermann (2).pfx");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
