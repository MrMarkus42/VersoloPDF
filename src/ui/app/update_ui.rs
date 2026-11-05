//! Einstellungen › "Neue Version": looks for the newest release on GitHub
//! (by hand or once a day at start) and installs it on Windows and macOS.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};

use egui::{Context, RichText, Ui};

use super::page_dialogs::tr;
use super::{Dialog, VersoloPdfApp};
use crate::ui::pickers::section;
use crate::ui::settings::Settings;
use crate::update::{self, Release};

/// State of the update search and installation.
#[derive(Default)]
pub enum UpdateState {
    #[default]
    Idle,
    /// Asking GitHub; `manual`: started in the settings (else at start).
    Checking {
        rx: Receiver<crate::Result<Option<Release>>>,
        manual: bool,
    },
    UpToDate,
    /// The project has no published release yet.
    NoRelease,
    Failed(String),
    Available(Release),
    /// Downloading, checking and unpacking; the result is the folder of the
    /// new program.
    Downloading(Release, Receiver<crate::Result<PathBuf>>),
    /// Ready to install: the folder of the new program and its version.
    Ready(PathBuf, String),
    /// The new program copies itself into the program folder.
    Installing(Receiver<crate::Result<()>>),
}

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

impl VersoloPdfApp {
    /// Starts asking GitHub for the newest release.
    pub(super) fn start_update_check(&mut self, ctx: &Context, manual: bool) {
        if crate::mail::no_external() {
            // Automated tests never use the network.
            self.update = UpdateState::Failed("VERSOLOPDF_NO_EXTERNAL".into());
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let repaint = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(update::fetch_latest());
            repaint.request_repaint();
        });
        self.update = UpdateState::Checking { rx, manual };
    }

    /// At start: at most once a day, if switched on.
    pub(super) fn auto_update_check(&mut self, ctx: &Context) {
        if !self.settings.update_check
            || crate::mail::no_external()
            || !matches!(self.update, UpdateState::Idle)
        {
            return;
        }
        let now = now();
        if now.saturating_sub(self.settings.update_last_check) < 24 * 3600 {
            return;
        }
        self.settings.update_last_check = now;
        self.start_update_check(ctx, false);
    }

    /// Takes results of the background work.
    pub(super) fn poll_update(&mut self, ctx: &Context) {
        let de = self.up_de();
        match &self.update {
            UpdateState::Checking { rx, manual } => {
                let manual = *manual;
                match rx.try_recv() {
                    Ok(Ok(Some(release)))
                        if update::is_newer(&release.version, update::current_version()) =>
                    {
                        if !manual && self.dialog.is_none() {
                            // Found at start: say so once.
                            self.dialog = Some(Dialog::Message {
                                title: tr(de, "Neue Version", "New version").into(),
                                text: if de {
                                    format!(
                                        "VersoloPDF {} ist erschienen (installiert: {}).\n\nInstallieren über Datei › Einstellungen › Neue Version.",
                                        release.version,
                                        update::current_version()
                                    )
                                } else {
                                    format!(
                                        "VersoloPDF {} has been released (installed: {}).\n\nInstall it with File › Settings › Updates.",
                                        release.version,
                                        update::current_version()
                                    )
                                },
                            });
                        }
                        self.update = UpdateState::Available(release);
                    }
                    Ok(Ok(Some(_))) => self.update = UpdateState::UpToDate,
                    Ok(Ok(None)) => self.update = UpdateState::NoRelease,
                    Ok(Err(err)) => {
                        log::warn!("Suche nach neuer Version fehlgeschlagen: {err}");
                        self.update =
                            if manual { UpdateState::Failed(err.to_string()) } else { UpdateState::Idle };
                    }
                    Err(TryRecvError::Empty) => {
                        ctx.request_repaint_after(std::time::Duration::from_millis(300))
                    }
                    Err(TryRecvError::Disconnected) => self.update = UpdateState::Idle,
                }
            }
            UpdateState::Installing(rx) => match rx.try_recv() {
                Ok(Ok(())) => {
                    // The new version starts as its own window, this one closes
                    // (asking about unsaved changes first).
                    let exe = program_dir().map(|d| update::program_exe(&d));
                    let started = exe.as_ref().is_some_and(|exe| {
                        std::process::Command::new(exe)
                            .arg(update::WAIT_ARG)
                            .arg(std::process::id().to_string())
                            .args(restart_args())
                            .spawn()
                            .is_ok()
                    });
                    if started {
                        log::info!("Neue Version installiert, wird gestartet");
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        self.update = UpdateState::Idle;
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
                Err(TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(300)),
                Err(TryRecvError::Disconnected) => self.update = UpdateState::Idle,
            },
            UpdateState::Downloading(release, rx) => match rx.try_recv() {
                Ok(Ok(folder)) => {
                    let version = release.version.clone();
                    self.update = UpdateState::Ready(folder, version);
                }
                Ok(Err(err)) => {
                    log::warn!("Neue Version nicht geladen: {err}");
                    self.update = UpdateState::Failed(err.to_string());
                }
                Err(TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(300)),
                Err(TryRecvError::Disconnected) => self.update = UpdateState::Idle,
            },
            _ => {}
        }
    }

    fn up_de(&self) -> bool {
        self.settings.language == crate::ui::i18n::Language::German
    }

    /// Downloads, checks and unpacks the Windows package in the background.
    fn start_update_download(&mut self, ctx: &Context, release: Release) {
        let (tx, rx) = std::sync::mpsc::channel();
        let repaint = ctx.clone();
        let job = release.clone();
        std::thread::spawn(move || {
            let dir = std::env::temp_dir().join("versolopdf-update").join(&job.version);
            let result = update::download_package(&job, &dir).and_then(|zip| update::unpack(&zip));
            let _ = tx.send(result);
            repaint.request_repaint();
        });
        self.update = UpdateState::Downloading(release, rx);
    }

    /// Lets the new program copy itself into the program folder (in the
    /// background; Windows may ask for administrator rights).
    fn start_install(&mut self, ctx: &Context, folder: PathBuf) {
        let Some(target) = program_dir() else {
            self.update = UpdateState::Failed("program folder unknown".into());
            return;
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let repaint = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(update::run_installer(&update::program_exe(&folder), &target));
            repaint.request_repaint();
        });
        self.update = UpdateState::Installing(rx);
    }

    /// The page "Neue Version" of the settings.
    pub(super) fn update_page(&mut self, ui: &mut Ui, de: bool, d: &mut Settings) {
        let ctx = ui.ctx().clone();
        let current = update::current_version();
        section(ui, tr(de, "Version", "Version"), |ui| {
            ui.label(if de {
                format!("Installierte Version: {current}")
            } else {
                format!("Installed version: {current}")
            });
            ui.checkbox(
                &mut d.update_check,
                tr(
                    de,
                    "Beim Start nach einer neuen Version suchen (höchstens einmal am Tag)",
                    "Look for a new version at start (at most once a day)",
                ),
            );
            ui.label(
                RichText::new(if de {
                    format!(
                        "Gesucht wird unter github.com/{} (Veröffentlichungen). Dabei wird nur die Versionsnummer abgefragt.",
                        update::REPOSITORY
                    )
                } else {
                    format!(
                        "VersoloPDF looks at github.com/{} (releases). Only the version number is asked for.",
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
                UpdateState::Checking { .. } | UpdateState::Downloading(..) | UpdateState::Installing(_)
            );
            if ui
                .add_enabled(
                    !busy,
                    egui::Button::new(tr(
                        de,
                        "Jetzt nach neuer Version suchen",
                        "Check for a new version now",
                    )),
                )
                .clicked()
            {
                self.start_update_check(&ctx, true);
            }
            ui.add_space(4.0);
            let mut download = None;
            let mut install = None;
            match &self.update {
                UpdateState::Idle => {}
                UpdateState::Checking { .. } => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(tr(de, "Suche läuft …", "Checking …"));
                    });
                }
                UpdateState::UpToDate => {
                    ui.label(tr(
                        de,
                        "✔ VersoloPDF ist auf dem neuesten Stand.",
                        "✔ VersoloPDF is up to date.",
                    ));
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
                    if !release.notes.trim().is_empty() {
                        egui::ScrollArea::vertical().id_salt("update-notes").max_height(140.0).show(
                            ui,
                            |ui| {
                                ui.label(release.notes.trim());
                            },
                        );
                    }
                    ui.horizontal(|ui| {
                        let package = update::package_of(release).filter(|_| program_dir().is_some());
                        if let Some((asset, _)) = package {
                            let size = asset.size;
                            let text = if de {
                                format!(
                                    "Herunterladen und installieren ({:.0} MB)",
                                    size as f64 / 1_048_576.0
                                )
                            } else {
                                format!("Download and install ({:.0} MB)", size as f64 / 1_048_576.0)
                            };
                            if ui.button(text).clicked() {
                                download = Some(release.clone());
                            }
                        }
                        let page_ok =
                            release.page.starts_with(&format!("https://github.com/{}/", update::REPOSITORY));
                        if page_ok
                            && ui
                                .button(tr(de, "Seite auf GitHub öffnen", "Open the page on GitHub"))
                                .clicked()
                        {
                            ctx.open_url(egui::OpenUrl::new_tab(release.page.clone()));
                        }
                    });
                }
                UpdateState::Downloading(..) => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(tr(de, "Wird heruntergeladen und geprüft …", "Downloading and checking …"));
                    });
                }
                UpdateState::Ready(folder, version) => {
                    ui.label(if de {
                        format!("VersoloPDF {version} ist heruntergeladen und geprüft.")
                    } else {
                        format!("VersoloPDF {version} has been downloaded and checked.")
                    });
                    let protected =
                        !program_dir().is_some_and(|d| update::folder_writable(&update::install_folder(&d)));
                    let hint = match (protected, de) {
                        (true, true) if cfg!(target_os = "macos") => {
                            "Der Programme-Ordner ist geschützt: macOS fragt gleich nach dem Kennwort eines Administrators. Danach startet VersoloPDF in der neuen Version neu (ungespeicherte Änderungen werden vorher abgefragt)."
                        }
                        (true, false) if cfg!(target_os = "macos") => {
                            "The Applications folder is protected: macOS will ask for an administrator's password. Then VersoloPDF starts again in the new version (asking about unsaved changes first)."
                        }
                        (true, true) => {
                            "Der Programmordner ist geschützt: Windows fragt gleich nach Administratorrechten. Danach startet VersoloPDF in der neuen Version neu (ungespeicherte Änderungen werden vorher abgefragt)."
                        }
                        (true, false) => {
                            "The program folder is protected: Windows will ask for administrator rights. Then VersoloPDF starts again in the new version (asking about unsaved changes first)."
                        }
                        (false, true) => {
                            "VersoloPDF startet danach in der neuen Version neu (ungespeicherte Änderungen werden vorher abgefragt)."
                        }
                        (false, false) => {
                            "VersoloPDF then starts again in the new version (asking about unsaved changes first)."
                        }
                    };
                    ui.label(RichText::new(hint).small().weak());
                    if ui
                        .button(tr(de, "Jetzt installieren und neu starten", "Install and restart now"))
                        .clicked()
                    {
                        install = Some(folder.clone());
                    }
                }
                UpdateState::Installing(_) => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(tr(de, "Wird installiert …", "Installing …"));
                    });
                }
            }
            if let Some(release) = download {
                self.start_update_download(&ctx, release);
            }
            if let Some(folder) = install {
                self.start_install(&ctx, folder);
            }
        });
    }
}
