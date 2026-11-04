//! Commands beyond the basic file/view/comment functions.

use egui::Context;

use super::commands::Cmd;
use super::{Dialog, MrPdfApp};
use crate::geometry::RectF;
use crate::model::AnnotKind;
use crate::pdf_edit::{LinkSpec, NewAnnotation};
use crate::ui::canvas::CanvasEvent;
use crate::ui::i18n::Language;
use crate::ui::tab::AnnotRef;
use crate::ui::tools::Tool;

/// Commands that are not implemented yet; they are shown greyed out.
pub const PENDING: &[Cmd] = &[];

/// Files above this size are refused as attachments.
const MAX_ATTACHMENT: u64 = 100 * 1024 * 1024;

impl MrPdfApp {
    fn german(&self) -> bool {
        self.settings.language == Language::German
    }

    pub(super) fn info(&mut self, title: &str, text: String) {
        self.dialog = Some(Dialog::Message { title: title.to_string(), text });
    }

    /// Commands handled outside the central dispatcher.
    pub(super) fn run_feature(&mut self, ctx: &Context, cmd: Cmd) {
        let de = self.german();
        match cmd {
            Cmd::WebLinksCreate => {
                let result = self.tab().map(|t| t.doc.create_web_links());
                self.after_count(ctx, result, if de { "Neue Web-Links" } else { "New web links" });
            }
            Cmd::WebLinksRemove => {
                let result = self.tab().map(|t| t.doc.remove_web_links());
                self.after_count(ctx, result, if de { "Entfernte Web-Links" } else { "Removed web links" });
            }
            Cmd::Email => self.send_email(ctx),
            Cmd::EmailAll => self.send_email_all(ctx),
            Cmd::OpenFolder => {
                if let Some(path) = self.active_tab().and_then(|t| t.doc.path()).map(|p| p.to_path_buf()) {
                    reveal_in_file_manager(&path);
                }
            }
            Cmd::Manual => {
                let t = self.t();
                let text = format!("{}\n\n{}", t.manual_text, t.shortcuts_text);
                self.info(Cmd::Manual.label(self.settings.language), text);
            }
            Cmd::CommandLineHelp => {
                let text = if de {
                    "Aufruf:\n  mrpdf [Optionen] [DATEI.pdf ...]\n\nAlle angegebenen PDF-Dateien werden in eigenen Tabs geöffnet. Läuft MrPDF schon, öffnet das laufende Programm sie.\n\nOptionen:\n  --new-window   eigenes Fenster, auch wenn MrPDF schon läuft\n  --opengl       Darstellung mit OpenGL statt DirectX/Vulkan/Metal\n\nUmgebungsvariablen:\n  MRPDF_PDFIUM_DIR: Ordner mit der PDFium-Bibliothek (absoluter Pfad)\n  MRPDF_RENDERER=opengl: wie --opengl\n  RUST_LOG=info: ausführliche Protokollausgabe"
                } else {
                    "Usage:\n  mrpdf [options] [FILE.pdf ...]\n\nAll given PDF files are opened in tabs. If MrPDF is running already, the running program opens them.\n\nOptions:\n  --new-window   a window of its own even if MrPDF is running\n  --opengl       draw with OpenGL instead of DirectX/Vulkan/Metal\n\nEnvironment variables:\n  MRPDF_PDFIUM_DIR: folder of the PDFium library (absolute path)\n  MRPDF_RENDERER=opengl: same as --opengl\n  RUST_LOG=info: verbose logging"
                };
                self.info(Cmd::CommandLineHelp.label(self.settings.language), text.to_string());
            }
            Cmd::FileAssociation => {
                if cfg!(windows) {
                    // Windows only lets the user choose default apps in the settings.
                    let _ = std::process::Command::new("explorer").arg("ms-settings:defaultapps").spawn();
                }
                let text = if de {
                    "Um MrPDF als Standardprogramm für PDF-Dateien festzulegen:\nWindows-Einstellungen > Apps > Standard-Apps > „.pdf“ > MrPDF auswählen.\n(Unter Linux/macOS über die Dateieigenschaften „Öffnen mit“.)"
                } else {
                    "To make MrPDF the default program for PDF files:\nWindows Settings > Apps > Default apps > \".pdf\" > choose MrPDF.\n(On Linux/macOS use \"Open with\" in the file properties.)"
                };
                self.info(Cmd::FileAssociation.label(self.settings.language), text.to_string());
            }
            other => {
                if !self.run_forms(ctx, other)
                    && !self.run_outline(ctx, other)
                    && !self.run_review(ctx, other)
                {
                    self.run_structure(ctx, other);
                }
            }
        }
    }

    fn after_count(&mut self, ctx: &Context, result: Option<crate::Result<usize>>, label: &str) {
        if let Some(tab) = self.tab() {
            tab.after_edit();
        }
        match result {
            Some(Ok(n)) => self.set_status(ctx, format!("{label}: {n}")),
            Some(Err(err)) => self.show_error(&err),
            None => {}
        }
    }

    /// "Per E-Mail senden": a new message in the default e-mail program
    /// with the document attached (its current state).
    fn send_email(&mut self, ctx: &Context) {
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        let name = tab.doc.display_name().to_string();
        let file = match mail_file(tab) {
            Ok(f) => f,
            Err(err) => {
                self.show_error(&err);
                return;
            }
        };
        let subject = mail_subject(&self.settings.email_subject, &name);
        self.mail(ctx, Cmd::Email, &subject, vec![file]);
    }

    /// "Alle Dokumente per E-Mail senden": one message with all open
    /// documents attached.
    fn send_email_all(&mut self, ctx: &Context) {
        let mut files = Vec::new();
        for tab in &mut self.tabs {
            match mail_file(tab) {
                Ok(f) => files.push(f),
                Err(err) => {
                    self.show_error(&err);
                    return;
                }
            }
        }
        if files.is_empty() {
            return;
        }
        let subject = if self.german() { "PDF-Dokumente" } else { "PDF documents" };
        self.mail(ctx, Cmd::EmailAll, subject, files);
    }

    /// Hands the message to the mail program and tells the user what to
    /// expect.
    pub(super) fn mail(&mut self, ctx: &Context, cmd: Cmd, subject: &str, files: Vec<std::path::PathBuf>) {
        let list = files.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("\n");
        let de = self.german();
        if crate::mail::compose(subject, files) {
            let text = if de {
                "Das E-Mail-Programm wird mit einer neuen Nachricht geöffnet, die Datei ist angehängt."
            } else {
                "Your e-mail program opens a new message with the file attached."
            };
            self.set_status(ctx, text);
        } else {
            let text = if de {
                format!("Das E-Mail-Programm wurde geöffnet. Bitte hängen Sie an:\n{list}")
            } else {
                format!("Your e-mail program was opened. Please attach:\n{list}")
            };
            self.info(cmd.label(self.settings.language), text);
        }
    }

    /// Canvas events of the newer tools.
    pub(super) fn handle_tool_event(&mut self, ctx: &Context, event: CanvasEvent) {
        match event {
            CanvasEvent::RequestText { page, tool, template } => {
                self.dialog = Some(Dialog::TextInput {
                    page,
                    tool,
                    template: Some(template),
                    text: String::new(),
                    edit: None,
                });
            }
            CanvasEvent::Snapshot { page, rect } => self.snapshot(ctx, page, rect),
            CanvasEvent::Erase(annots) => {
                if let Some(first) = annots.first() {
                    let page = first.page;
                    let indices: Vec<usize> =
                        annots.iter().filter(|a| a.page == page).map(|a| a.index).collect();
                    self.edit(ctx, |tab| {
                        tab.selected_annot = None;
                        tab.doc.delete_annotations(page, &indices)
                    });
                }
            }
            CanvasEvent::CreateLink { page, rect } => {
                let current = self.active_tab().map_or(0, |t| t.current_page);
                self.dialog = Some(Dialog::Link {
                    page,
                    rect,
                    url: "https://".into(),
                    target_page: current as u32 + 1,
                    to_page: false,
                });
            }
            CanvasEvent::AttachFile { page, rect } => self.attach_file(ctx, page, rect),
            CanvasEvent::CropPage { page, rect } => {
                self.edit(ctx, |tab| tab.doc.crop_to_rect(page, rect));
                self.select_tool(Tool::Hand);
            }
            CanvasEvent::CreateField { page, rect, tool } => self.create_field(ctx, page, rect, tool),
            CanvasEvent::AddMedia { page, rect, tool } => self.add_media_file(ctx, page, rect, tool),
            CanvasEvent::PlayMedia(annot) => self.play_media(ctx, annot),
            CanvasEvent::PressButton { page, annot_index } => self.press_button(ctx, page, annot_index),
            _ => {}
        }
    }

    /// Renders an area (default: 2x screen resolution, see "Bildschirmfotos"
    /// in the settings) and copies it to the clipboard.
    fn snapshot(&mut self, ctx: &Context, page: usize, rect: RectF) {
        let Some(tab) = self.active_tab() else { return };
        let Some(geo) = tab.display_geometry(page) else { return };
        let (vw, vh) = geo.view_size();
        let view = geo.pdf_rect_to_view(&rect);
        let dpi = match self.settings.snapshot_dpi {
            0 => 2.0 * 96.0,
            d => d.clamp(36, 1200) as f32,
        };
        // Limit the full page bitmap to about 400 megapixels.
        let scale = (dpi / 72.0).min((4.0e8 / (vw * vh).max(1.0)).sqrt());
        let gray = self.settings.snapshot_gray;
        let full = [(vw * scale).round() as u32, (vh * scale).round() as u32];
        let x = (view.x0 * scale).max(0.0) as u32;
        let y = (view.y0 * scale).max(0.0) as u32;
        let w = ((view.width() * scale) as u32).clamp(1, full[0].saturating_sub(x).max(1));
        let h = ((view.height() * scale) as u32).clamp(1, full[1].saturating_sub(y).max(1));
        let result = tab.doc.render_tile(page, full[0], full[1], x, y, w, h, tab.render_options());
        match result {
            Ok(mut img) => {
                if gray {
                    for px in img.rgba.as_chunks_mut::<4>().0 {
                        let l = (0.299 * px[0] as f32 + 0.587 * px[1] as f32 + 0.114 * px[2] as f32).round()
                            as u8;
                        px[..3].fill(l);
                    }
                }
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [img.width as usize, img.height as usize],
                    &img.rgba,
                );
                ctx.copy_image(image);
                let msg = if self.german() { "Bildschirmfoto kopiert" } else { "Snapshot copied" };
                self.set_status(ctx, msg);
            }
            Err(err) => self.show_error(&err),
        }
    }

    fn attach_file(&mut self, ctx: &Context, page: usize, rect: RectF) {
        let title = Cmd::Tool(Tool::FileAttach).label(self.settings.language);
        let Some(path) = rfd::FileDialog::new().set_title(title).pick_file() else { return };
        let too_big = std::fs::metadata(&path).map(|m| m.len() > MAX_ATTACHMENT).unwrap_or(true);
        if too_big {
            let t = self.t();
            self.info(t.error, t.err_too_large.to_string());
            return;
        }
        let data = match std::fs::read(&path) {
            Ok(d) => d,
            Err(e) => {
                self.show_error(&crate::Error::Io { path: path.clone(), source: e });
                return;
            }
        };
        let name =
            path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
        let mut annot = NewAnnotation::new(AnnotKind::FileAttachment, self.settings.style(Tool::FileAttach));
        annot.rect = rect;
        annot.contents = name.clone();
        annot.attachment = Some((name, data));
        self.handle_canvas_event(ctx, CanvasEvent::Create { page, annot: Box::new(annot) });
    }

    pub(super) fn create_link(&mut self, ctx: &Context, page: usize, rect: RectF, spec: LinkSpec) {
        self.edit(ctx, |tab| tab.doc.add_links(page, &[(rect, spec)]));
    }

    /// Creates the annotation of a text tool after the text was entered.
    pub(super) fn finish_text_template(
        &mut self,
        ctx: &Context,
        page: usize,
        tool: Tool,
        mut template: Box<NewAnnotation>,
        text: String,
    ) {
        if text.trim().is_empty() && matches!(tool, Tool::TextBox | Tool::Typewriter | Tool::Callout) {
            return;
        }
        match tool {
            Tool::Typewriter => {
                // Size the box to the text, anchored at the clicked point.
                let size = template.style.font_size.max(4.0);
                let width = text
                    .lines()
                    .map(|l| {
                        crate::appearance::StdFont::Helvetica
                            .text_width(&crate::appearance::win_ansi(l), size)
                    })
                    .fold(10.0, f32::max)
                    + 6.0;
                let height = text.lines().count().max(1) as f32 * size * 1.2 + 4.0;
                let r = template.rect.normalized();
                template.rect = RectF::new(r.x0, r.y1 - height, r.x0 + width, r.y1);
                template.style.width = 0.0;
                template.style.fill = None;
                template.contents = text;
            }
            Tool::Replace => {
                // Struck-out text plus replacement as comment.
                template.contents = text;
            }
            _ => template.contents = text,
        }
        self.handle_canvas_event(ctx, CanvasEvent::Create { page, annot: template });
        if !self.settings.keeps_tool(self.tool) {
            self.tool = Tool::Hand;
        }
    }

    /// Edits the text of an existing note / text box.
    pub(super) fn finish_text_edit(&mut self, ctx: &Context, annot: AnnotRef, text: String) {
        self.edit(ctx, |tab| {
            tab.doc.update_annotation(
                annot.page,
                annot.index,
                &crate::pdf_edit::AnnotUpdate { contents: Some(text), ..Default::default() },
            )
        });
    }
}

/// Shows a file in the system file manager (no shell involved).
pub(super) fn reveal_in_file_manager(path: &std::path::Path) {
    let result = if cfg!(windows) {
        let mut arg = std::ffi::OsString::from("/select,");
        arg.push(path.as_os_str());
        std::process::Command::new("explorer").arg(arg).spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg("-R").arg(path).spawn()
    } else {
        let dir = path.parent().unwrap_or(path);
        std::process::Command::new("xdg-open").arg(dir).spawn()
    };
    if let Err(err) = result {
        log::warn!("could not open file manager: {err}");
    }
}

/// Subject of "Per E-Mail senden": `pattern` with %[FileName] replaced
/// (plain text, at most 200 characters, no control characters).
pub(super) fn mail_subject(pattern: &str, file_name: &str) -> String {
    pattern.replace("%[FileName]", file_name).chars().filter(|c| !c.is_control()).take(200).collect()
}

/// The file to attach for a document: the file itself when it is saved
/// and unchanged, else a copy of the current state in the temporary folder
/// (named like the document).
fn mail_file(tab: &mut crate::ui::tab::DocTab) -> crate::Result<std::path::PathBuf> {
    if let Some(p) = tab.doc.path()
        && !tab.doc.is_modified()
    {
        return Ok(p.to_path_buf());
    }
    let dir = std::env::temp_dir().join("mrpdf-mail").join(tab.id.to_string());
    let mut name = tab.doc.display_name().replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
    if !name.to_lowercase().ends_with(".pdf") {
        name.push_str(".pdf");
    }
    let path = dir.join(name);
    let bytes = tab.doc.to_bytes()?;
    std::fs::create_dir_all(&dir)
        .and_then(|()| std::fs::write(&path, bytes))
        .map_err(|e| crate::Error::Io { path: path.clone(), source: e })?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    #[test]
    fn mail_subject_pattern() {
        assert_eq!(super::mail_subject("%[FileName]", "a b.pdf"), "a b.pdf");
        assert_eq!(super::mail_subject("Datei: %[FileName]", "ä"), "Datei: ä");
        assert_eq!(super::mail_subject("x&y=\n", "n"), "x&y=");
    }
}
