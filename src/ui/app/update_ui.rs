//! Einstellungen › "Neue Version": looks for the newest release on GitHub
//! (by hand, or as often as the user chose after agreeing once) and installs
//! it on Windows and macOS. The checks are in `crate::update`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};

use egui::{Context, RichText, Ui};

use super::page_dialogs::tr;
use super::{AfterSave, Dialog, VersoloPdfApp};
use crate::ui::pickers::section;
use crate::ui::settings::{Settings, UpdateFreq, UpdateMode};
use crate::update::{self, Downloaded, Release};

/// State of the update search and installation.
#[derive(Default)]
pub enum UpdateState {
    #[default]
    Idle,
    /// Asking GitHub; `manual`: started by the user (else automatic).
    Checking {
        rx: Receiver<crate::Result<Option<Release>>>,
        manual: bool,
    },
    UpToDate,
    /// The project has no published release yet.
    NoRelease,
    Failed(String),
    Available(Release),
    /// Downloading and checking; `notify`: say so when done.
    Downloading {
        release: Release,
        rx: Receiver<crate::Result<Downloaded>>,
        notify: bool,
    },
    /// Downloaded and checked; `at_exit`: installed when VersoloPDF closes.
    Ready {
        release: Release,
        update: Downloaded,
        at_exit: bool,
    },
    /// The installed program puts the new version in place.
    Installing(Receiver<crate::Result<()>>),
}

/// The automatic check and the question at first start happen once per run.
static AUTO_CHECK_DONE: AtomicBool = AtomicBool::new(false);
static CONSENT_ASKED: AtomicBool = AtomicBool::new(false);
/// "Jetzt installieren" was chosen (see `request_update_install`).
static INSTALL_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Where the running program is installed (folder on Windows, bundle on
/// macOS).
fn program_dir() -> Option<PathBuf> {
    update::install_target()
}

/// Seconds since 1970.
fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Program arguments the new version is started with.
fn restart_args() -> Vec<String> {
    std::env::args().skip(1).filter(|a| a == "--opengl").collect()
}

/// Updates are possible at all: a signing key is built in and no test run.
fn updates_enabled() -> bool {
    update::key_configured() && !crate::mail::no_external()
}

/// How this copy was installed, for the settings page.
fn install_kind(de: bool) -> &'static str {
    let Some(dir) = program_dir() else {
        return tr(de, "Entwicklerversion (keine Updates)", "Development build (no updates)");
    };
    if cfg!(target_os = "macos") {
        return tr(de, "Programm-Paket (VersoloPDF.app)", "Application bundle (VersoloPDF.app)");
    }
    if !dir.join("unins000.exe").exists() {
        return tr(de, "Ohne Installation (entpacktes Paket)", "Without installer (unpacked package)");
    }
    if update::folder_writable(&dir) {
        tr(de, "Installiert nur für mich", "Installed for me only")
    } else {
        tr(de, "Installiert für alle Benutzer", "Installed for all users")
    }
}

/// A button that fills the width (dialog answers).
fn answer(ui: &mut Ui, text: &str) -> bool {
    ui.add_sized([ui.available_width(), 28.0], egui::Button::new(format!("›  {text}"))).clicked()
}

impl VersoloPdfApp {
    fn up_de(&self) -> bool {
        self.settings.language == crate::ui::i18n::Language::German
    }

    /// Starts asking GitHub for the newest release.
    pub(super) fn start_update_check(&mut self, ctx: &Context, manual: bool) {
        if crate::mail::no_external() {
            // Automated tests never use the network.
            self.update = UpdateState::Failed("VERSOLOPDF_NO_EXTERNAL".into());
            return;
        }
        if !update::key_configured() {
            self.update = UpdateState::Failed(
                tr(
                    self.up_de(),
                    "Diese Ausgabe von VersoloPDF hat keinen Update-Schlüssel und sucht nicht nach neuen Versionen.",
                    "This build of VersoloPDF has no update key and does not look for new versions.",
                )
                .into(),
            );
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let repaint = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(update::fetch_latest());
            repaint.request_repaint();
        });
        self.settings.update_last_check = now();
        self.update = UpdateState::Checking { rx, manual };
    }

    /// Once per run: asks whether VersoloPDF may look for new versions (first
    /// start), or looks if that is due.
    pub(super) fn auto_update_check(&mut self, ctx: &Context) {
        if !updates_enabled() || !matches!(self.update, UpdateState::Idle) {
            return;
        }
        if !self.settings.update_asked {
            if self.dialog.is_none() && !CONSENT_ASKED.swap(true, Ordering::Relaxed) {
                self.dialog = Some(Dialog::UpdateConsent);
            }
            return;
        }
        if AUTO_CHECK_DONE.swap(true, Ordering::Relaxed) {
            return;
        }
        if self.settings.update_freq.due(self.settings.update_last_check, now()) {
            self.start_update_check(ctx, false);
        }
    }

    /// The answer to the question at first start.
    pub(super) fn answer_update_consent(&mut self, yes: bool) {
        self.settings.update_asked = true;
        self.settings.update_freq = if yes { UpdateFreq::Weekly } else { UpdateFreq::Never };
        log::info!("Suche nach neuen Versionen: {}", if yes { "wöchentlich" } else { "aus" });
    }

    /// Takes results of the background work.
    pub(super) fn poll_update(&mut self, ctx: &Context) {
        let de = self.up_de();
        let wait = std::time::Duration::from_millis(300);
        if INSTALL_REQUESTED.swap(false, Ordering::Relaxed) {
            if matches!(self.dialog, Some(Dialog::Settings(_))) {
                // The settings close (keeping what was chosen there).
                self.close_settings_keep_draft();
            }
            self.install_update_when_saved(ctx);
            return;
        }
        match &self.update {
            UpdateState::Checking { rx, manual } => {
                let manual = *manual;
                match rx.try_recv() {
                    Ok(Ok(Some(release)))
                        if update::is_newer(&release.version, update::current_version()) =>
                    {
                        if manual {
                            self.update = UpdateState::Available(release);
                        } else if release.version == self.settings.update_skip {
                            self.update = UpdateState::Idle;
                        } else {
                            self.found_automatically(ctx, release);
                        }
                    }
                    Ok(Ok(Some(_))) => self.update = UpdateState::UpToDate,
                    Ok(Ok(None)) => self.update = UpdateState::NoRelease,
                    Ok(Err(err)) => {
                        log::warn!("Suche nach neuer Version fehlgeschlagen: {err}");
                        self.update =
                            if manual { UpdateState::Failed(err.to_string()) } else { UpdateState::Idle };
                    }
                    Err(TryRecvError::Empty) => ctx.request_repaint_after(wait),
                    Err(TryRecvError::Disconnected) => self.update = UpdateState::Idle,
                }
            }
            UpdateState::Downloading { release, rx, notify } => match rx.try_recv() {
                Ok(Ok(update)) => {
                    let (release, notify) = (release.clone(), *notify);
                    let at_exit = self.settings.update_mode == UpdateMode::AtExit && !notify;
                    log::info!("Neue Version {} heruntergeladen und geprüft", release.version);
                    self.update = UpdateState::Ready { release, update, at_exit };
                    if notify && self.dialog.is_none() {
                        self.dialog = Some(Dialog::UpdateFound);
                    }
                }
                Ok(Err(err)) => {
                    log::warn!("Neue Version nicht geladen: {err}");
                    self.update = UpdateState::Failed(err.to_string());
                }
                Err(TryRecvError::Empty) => ctx.request_repaint_after(wait),
                Err(TryRecvError::Disconnected) => self.update = UpdateState::Idle,
            },
            UpdateState::Installing(rx) => match rx.try_recv() {
                Ok(Ok(())) => {
                    // The new version starts as its own window, this one closes
                    // (all documents were saved or closed before).
                    let exe = program_dir().map(|d| update::program_exe(&d));
                    let started = exe.as_ref().is_some_and(|exe| {
                        std::process::Command::new(exe)
                            .arg(update::WAIT_ARG)
                            .arg(std::process::id().to_string())
                            .args(restart_args())
                            .spawn()
                            .is_ok()
                    });
                    self.update = UpdateState::Idle;
                    if started {
                        log::info!("Neue Version installiert, wird gestartet");
                        self.exit_confirmed = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    } else {
                        self.update = UpdateState::Failed(
                            tr(
                                de,
                                "Die neue Version ist installiert. Bitte VersoloPDF beenden und neu starten.",
                                "The new version is installed. Please close and start VersoloPDF again.",
                            )
                            .into(),
                        );
                    }
                }
                Ok(Err(err)) => {
                    log::warn!("Neue Version nicht installiert: {err}");
                    self.update = UpdateState::Failed(err.to_string());
                }
                Err(TryRecvError::Empty) => ctx.request_repaint_after(wait),
                Err(TryRecvError::Disconnected) => self.update = UpdateState::Idle,
            },
            _ => {}
        }
    }

    /// A newer version found by the automatic check: as the user chose.
    fn found_automatically(&mut self, ctx: &Context, release: Release) {
        log::info!("Neue Version {} gefunden", release.version);
        let installable = update::package_of(&release).is_some() && program_dir().is_some();
        match self.settings.update_mode {
            UpdateMode::Offer | UpdateMode::AtExit if installable => {
                let notify = self.settings.update_mode == UpdateMode::Offer;
                self.start_update_download(ctx, release, notify);
            }
            _ => {
                self.update = UpdateState::Available(release);
                if self.dialog.is_none() {
                    self.dialog = Some(Dialog::UpdateFound);
                }
            }
        }
    }

    /// Downloads and checks the package in the background.
    fn start_update_download(&mut self, ctx: &Context, release: Release, notify: bool) {
        let Some(dir) = update::updates_dir() else {
            self.update = UpdateState::Failed("no folder for downloads".into());
            return;
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let repaint = ctx.clone();
        let job = release.clone();
        std::thread::spawn(move || {
            let _ = tx.send(update::download_package(&job, &dir));
            repaint.request_repaint();
        });
        self.update = UpdateState::Downloading { release, rx, notify };
    }

    /// "Jetzt installieren und neu starten" (from a dialog or the settings):
    /// handled at the next frame, outside the dialog that asked.
    fn request_update_install(&mut self, ctx: &Context) {
        INSTALL_REQUESTED.store(true, Ordering::Relaxed);
        ctx.request_repaint();
    }

    /// First every unsaved document is saved or closed (the question for
    /// each), then the update is installed.
    pub(super) fn install_update_when_saved(&mut self, ctx: &Context) {
        if !matches!(self.update, UpdateState::Ready { .. }) {
            return;
        }
        if let Some(tab) = self.tabs.iter().find(|t| t.doc.is_modified()) {
            let tab_id = tab.id;
            if let Some(i) = self.tabs.iter().position(|t| t.id == tab_id) {
                self.active = i;
            }
            self.dialog = Some(Dialog::Unsaved { tab_id, then: AfterSave::Update });
            return;
        }
        self.start_install(ctx);
    }

    /// Lets the installed program put the new version in place (in the
    /// background; the system may ask for administrator rights).
    fn start_install(&mut self, ctx: &Context) {
        let UpdateState::Ready { update, .. } = &self.update else { return };
        let Some(target) = program_dir() else {
            self.update = UpdateState::Failed("program folder unknown".into());
            return;
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let repaint = ctx.clone();
        let job = update.clone();
        std::thread::spawn(move || {
            let _ = tx.send(update::run_installer(&job, &target));
            repaint.request_repaint();
        });
        self.update = UpdateState::Installing(rx);
    }

    /// When VersoloPDF closes: installs an update waiting for that. The new
    /// version starts at the next start.
    pub(super) fn install_update_at_exit(&mut self) {
        let UpdateState::Ready { update, at_exit: true, .. } = &self.update else { return };
        let Some(target) = program_dir() else { return };
        match update::run_installer(update, &target) {
            Ok(()) => log::info!("Neue Version {} beim Beenden installiert", update.version),
            Err(err) => log::warn!("Neue Version beim Beenden nicht installiert: {err}"),
        }
        self.update = UpdateState::Idle;
    }

    /// Text about the administrator question before installing.
    fn install_hint(de: bool) -> &'static str {
        let protected = !program_dir().is_some_and(|d| update::folder_writable(&update::install_folder(&d)));
        match (protected, de) {
            (true, true) if cfg!(target_os = "macos") => {
                "Der Programme-Ordner ist geschützt: macOS fragt gleich nach dem Kennwort eines Administrators."
            }
            (true, false) if cfg!(target_os = "macos") => {
                "The Applications folder is protected: macOS will ask for an administrator's password."
            }
            (true, true) => {
                "VersoloPDF ist für alle Benutzer installiert: Windows fragt gleich nach Administratorrechten."
            }
            (true, false) => {
                "VersoloPDF is installed for all users: Windows will ask for administrator rights."
            }
            (false, true) => "Ungespeicherte Dokumente werden vorher abgefragt.",
            (false, false) => "You are asked about unsaved documents first.",
        }
    }

    /// The window "Neue Version" (found by the automatic check). Returns true
    /// when it should close.
    pub(super) fn update_found_ui(&mut self, ui: &mut Ui) -> bool {
        let de = self.up_de();
        let ctx = ui.ctx().clone();
        let current = update::current_version();
        let release = match &self.update {
            UpdateState::Available(r) | UpdateState::Ready { release: r, .. } => r.clone(),
            UpdateState::Downloading { release, .. } => release.clone(),
            _ => return true,
        };
        ui.label(
            RichText::new(if de {
                format!("VersoloPDF {} ist erschienen (installiert: {current}).", release.version)
            } else {
                format!("VersoloPDF {} has been released (installed: {current}).", release.version)
            })
            .strong(),
        );
        notes(ui, &release);
        ui.add_space(6.0);
        let mut close = false;
        match &self.update {
            UpdateState::Ready { .. } => {
                ui.label(RichText::new(Self::install_hint(de)).small().weak());
                ui.add_space(4.0);
                if answer(ui, tr(de, "Jetzt installieren und neu starten", "Install and restart now")) {
                    self.request_update_install(&ctx);
                    return true;
                }
                if answer(ui, tr(de, "Beim Beenden installieren", "Install when closing")) {
                    if let UpdateState::Ready { at_exit, .. } = &mut self.update {
                        *at_exit = true;
                    }
                    close = true;
                }
            }
            UpdateState::Available(release) => {
                if update::package_of(release).is_some() && program_dir().is_some() {
                    if answer(ui, tr(de, "Herunterladen und prüfen", "Download and check")) {
                        let release = release.clone();
                        self.start_update_download(&ctx, release, true);
                        close = true;
                    }
                } else {
                    ui.label(tr(
                        de,
                        "Bitte die neue Version von der Projektseite auf GitHub laden.",
                        "Please download the new version from the project page on GitHub.",
                    ));
                }
            }
            _ => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(tr(de, "Wird heruntergeladen und geprüft …", "Downloading and checking …"));
                });
            }
        }
        if answer(ui, tr(de, "Später", "Later")) {
            close = true;
        }
        if answer(ui, tr(de, "Diese Version überspringen", "Skip this version")) {
            self.settings.update_skip = release.version.clone();
            self.update = UpdateState::Idle;
            close = true;
        }
        close
    }

    /// The page "Neue Version" of the settings.
    pub(super) fn update_page(&mut self, ui: &mut Ui, de: bool, d: &mut Settings) {
        let ctx = ui.ctx().clone();
        let current = update::current_version();
        let enabled = update::key_configured();
        section(ui, tr(de, "Version", "Version"), |ui| {
            ui.label(if de {
                format!("Installierte Version: {current}")
            } else {
                format!("Installed version: {current}")
            });
            ui.label(if de {
                format!("Art der Installation: {}", install_kind(de))
            } else {
                format!("Kind of installation: {}", install_kind(de))
            });
            if !enabled {
                ui.label(
                    RichText::new(tr(
                        de,
                        "Diese Ausgabe hat keinen Update-Schlüssel: Sie sucht nicht nach neuen Versionen.",
                        "This build has no update key: it does not look for new versions.",
                    ))
                    .color(ui.visuals().warn_fg_color),
                );
            }
        });
        ui.add_enabled_ui(enabled, |ui| {
            section(ui, tr(de, "Automatisch suchen", "Look automatically"), |ui| {
                egui::Grid::new("update-options").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    ui.label(tr(de, "Nach neuen Versionen suchen:", "Look for new versions:"));
                    egui::ComboBox::from_id_salt("update-freq")
                        .selected_text(d.update_freq.label(de))
                        .show_ui(ui, |ui| {
                            for f in UpdateFreq::ALL {
                                if ui.selectable_value(&mut d.update_freq, f, f.label(de)).clicked() {
                                    d.update_asked = true;
                                }
                            }
                        });
                    ui.end_row();
                    ui.label(tr(de, "Bei einer neuen Version:", "When there is a new version:"));
                    egui::ComboBox::from_id_salt("update-mode")
                        .selected_text(d.update_mode.label(de))
                        .width(300.0)
                        .show_ui(ui, |ui| {
                            for m in UpdateMode::ALL {
                                ui.selectable_value(&mut d.update_mode, m, m.label(de));
                            }
                        });
                    ui.end_row();
                });
                let last = d.update_last_check;
                if last > 0 {
                    let days = now().saturating_sub(last) / 86_400;
                    ui.label(RichText::new(match (days, de) {
                        (0, true) => "Zuletzt gesucht: heute".to_string(),
                        (0, false) => "Last checked: today".to_string(),
                        (n, true) => format!("Zuletzt gesucht: vor {n} Tag(en)"),
                        (n, false) => format!("Last checked: {n} day(s) ago"),
                    }).small().weak());
                }
                if !d.update_skip.is_empty() {
                    ui.horizontal(|ui| {
                        ui.label(if de {
                            format!("Übersprungen: Version {}", d.update_skip)
                        } else {
                            format!("Skipped: version {}", d.update_skip)
                        });
                        if ui.small_button(tr(de, "Wieder anbieten", "Offer again")).clicked() {
                            d.update_skip.clear();
                        }
                    });
                }
                ui.label(
                    RichText::new(if de {
                        format!(
                            "Gefragt wird nur github.com/{} nach der neuesten Version. Jede neue Version ist vom Herausgeber signiert; VersoloPDF prüft das vor der Installation und installiert sonst nichts.",
                            update::REPOSITORY
                        )
                    } else {
                        format!(
                            "Only github.com/{} is asked for the newest version. Each version is signed by the publisher; VersoloPDF checks that before installing and installs nothing else.",
                            update::REPOSITORY
                        )
                    })
                    .small()
                    .weak(),
                );
            });
            section(ui, tr(de, "Neue Version", "New version"), |ui| {
                let busy = matches!(
                    self.update,
                    UpdateState::Checking { .. } | UpdateState::Downloading { .. } | UpdateState::Installing(_)
                );
                if ui
                    .add_enabled(!busy, egui::Button::new(tr(de, "Jetzt nach neuer Version suchen", "Check for a new version now")))
                    .clicked()
                {
                    self.start_update_check(&ctx, true);
                }
                ui.add_space(4.0);
                let mut download = None;
                let mut install = false;
                match &self.update {
                    UpdateState::Idle => {}
                    UpdateState::Checking { .. } => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(tr(de, "Suche läuft …", "Checking …"));
                        });
                    }
                    UpdateState::UpToDate => {
                        ui.label(tr(de, "✔ VersoloPDF ist auf dem neuesten Stand.", "✔ VersoloPDF is up to date."));
                    }
                    UpdateState::NoRelease => {
                        ui.label(tr(
                            de,
                            "Auf GitHub ist noch keine Version veröffentlicht.",
                            "No version has been published on GitHub yet.",
                        ));
                    }
                    UpdateState::Failed(err) => {
                        ui.label(
                            RichText::new(format!("{}: {err}", tr(de, "Fehlgeschlagen", "Failed")))
                                .color(ui.visuals().warn_fg_color),
                        );
                    }
                    UpdateState::Available(release) => {
                        ui.label(
                            RichText::new(if de {
                                format!("VersoloPDF {} ist verfügbar.", release.version)
                            } else {
                                format!("VersoloPDF {} is available.", release.version)
                            })
                            .strong(),
                        );
                        notes(ui, release);
                        let package = update::package_of(release).filter(|_| program_dir().is_some());
                        if let Some((asset, _)) = package {
                            let mb = asset.size as f64 / 1_048_576.0;
                            let text = if de {
                                format!("Herunterladen und prüfen ({mb:.0} MB)")
                            } else {
                                format!("Download and check ({mb:.0} MB)")
                            };
                            if ui.button(text).clicked() {
                                download = Some(release.clone());
                            }
                        }
                    }
                    UpdateState::Downloading { .. } => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(tr(de, "Wird heruntergeladen und geprüft …", "Downloading and checking …"));
                        });
                    }
                    UpdateState::Ready { release, at_exit, .. } => {
                        ui.label(if de {
                            format!("VersoloPDF {} ist heruntergeladen, die Signatur ist geprüft.", release.version)
                        } else {
                            format!("VersoloPDF {} has been downloaded, its signature is checked.", release.version)
                        });
                        if *at_exit {
                            ui.label(tr(
                                de,
                                "Sie wird beim Beenden von VersoloPDF installiert.",
                                "It is installed when VersoloPDF closes.",
                            ));
                        }
                        ui.label(RichText::new(Self::install_hint(de)).small().weak());
                        install = ui
                            .button(tr(de, "Jetzt installieren und neu starten", "Install and restart now"))
                            .clicked();
                    }
                    UpdateState::Installing(_) => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(tr(de, "Wird installiert …", "Installing …"));
                        });
                    }
                }
                if let Some(release) = download {
                    self.start_update_download(&ctx, release, false);
                }
                if install {
                    self.request_update_install(&ctx);
                }
            });
        });
    }

    /// The question at first start: may VersoloPDF look for new versions?
    pub(super) fn update_consent_ui(&mut self, ui: &mut Ui) -> bool {
        let de = self.up_de();
        ui.label(tr(
            de,
            "Soll VersoloPDF nach neuen Versionen suchen?\n\nDabei fragt VersoloPDF einmal pro Woche bei GitHub nach, welche Version die neueste ist. Es wird nichts über Sie oder Ihre Dokumente gesendet. Neue Versionen werden nur installiert, wenn sie vom Herausgeber signiert sind und Sie zustimmen.\n\nÄndern lässt sich das jederzeit unter Einstellungen › Neue Version.",
            "Should VersoloPDF look for new versions?\n\nOnce a week VersoloPDF then asks GitHub which version is the newest. Nothing about you or your documents is sent. New versions are only installed when they are signed by the publisher and you agree.\n\nYou can change this at any time in Settings › New version.",
        ));
        ui.add_space(8.0);
        if answer(ui, tr(de, "Ja, wöchentlich suchen", "Yes, look once a week")) {
            self.answer_update_consent(true);
            return true;
        }
        if answer(ui, tr(de, "Nein", "No")) {
            self.answer_update_consent(false);
            return true;
        }
        false
    }
}

/// The release notes as plain text (nothing in them can be clicked or run).
fn notes(ui: &mut Ui, release: &Release) {
    if !release.notes.trim().is_empty() {
        egui::ScrollArea::vertical().id_salt("update-notes").max_height(160.0).show(ui, |ui| {
            ui.label(release.notes.trim());
        });
    }
}
