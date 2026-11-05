//! "Überprüfen", "Barrierefreiheit" and "Umwandeln": word count, document
//! comparison, accessibility report, new document from files, grayscale and
//! text / Word export.

use egui::Context;

use super::commands::Cmd;
use super::{Dialog, RangePurpose, VersoloPdfApp};
use crate::model::Rgba;
use crate::report::Line;
use crate::ui::i18n::Language;

const GREEN: Rgba = Rgba::rgb(20, 130, 50);
const RED: Rgba = Rgba::rgb(190, 30, 30);

impl VersoloPdfApp {
    fn review_de(&self) -> bool {
        self.settings.language == Language::German
    }

    /// Starts a review/convert command. Returns false if `cmd` is not one.
    pub(super) fn run_review(&mut self, ctx: &Context, cmd: Cmd) -> bool {
        let de = self.review_de();
        let lang = self.settings.language;
        match cmd {
            Cmd::FromFiles => self.new_from_files(ctx),
            Cmd::WordCount => {
                let Some(tab) = self.active_tab() else { return true };
                match tab.doc.word_count() {
                    Ok(c) => {
                        let text = if de {
                            format!(
                                "Seiten: {}\nWörter: {}\nZeichen (mit Leerzeichen): {}\nZeichen (ohne Leerzeichen): {}",
                                c.pages, c.words, c.characters, c.characters_no_spaces
                            )
                        } else {
                            format!(
                                "Pages: {}\nWords: {}\nCharacters (with spaces): {}\nCharacters (without spaces): {}",
                                c.pages, c.words, c.characters, c.characters_no_spaces
                            )
                        };
                        self.dialog = Some(Dialog::Message { title: cmd.label(lang).into(), text });
                    }
                    Err(err) => self.show_error(&err),
                }
            }
            Cmd::CompareDocuments => self.open_compare_dialog(),
            Cmd::SpellCheck => self.open_spell_check(),
            Cmd::DeletePageComments => self.delete_page_comments(ctx),
            Cmd::FlattenAll => self.flatten_all(ctx),
            Cmd::AccessibilityCheck => self.accessibility_report(ctx),
            Cmd::ContentsToImages => {
                let n = self.active_tab().map_or(0, |t| t.doc.page_count());
                self.dialog = Some(Dialog::PageRange {
                    purpose: RangePurpose::ToImages,
                    text: format!("1-{n}"),
                    error: false,
                });
            }
            Cmd::Grayscale => {
                let text = if de {
                    "Das Dokument in Graustufen umwandeln? Texte, Grafiken, Bilder und Kommentarfarben werden grau; Farbverläufe bleiben unverändert. Rückgängig machen ist möglich."
                } else {
                    "Convert the document to grayscale? Text, graphics, images and comment colours become gray; gradients stay unchanged. This can be undone."
                };
                self.dialog = Some(Dialog::Confirm { title: cmd.label(lang).into(), text: text.into(), cmd });
            }
            Cmd::ExportText => self.export_as(ctx, false),
            Cmd::ExportWord => self.export_as(ctx, true),
            _ => return false,
        }
        true
    }

    pub(super) fn open_report(&mut self, ctx: &Context, title: &str, lines: &[Line]) {
        let result = crate::report::build_pdf(title, lines)
            .and_then(|bytes| crate::Document::from_bytes(bytes, &format!("{title}.pdf"), None));
        match result {
            Ok(doc) => {
                self.add_tab(doc);
                ctx.request_repaint();
            }
            Err(err) => self.show_error(&err),
        }
    }

    fn new_from_files(&mut self, ctx: &Context) {
        let de = self.review_de();
        let Some(files) = rfd::FileDialog::new()
            .set_title(Cmd::FromFiles.label(self.settings.language))
            .add_filter(
                if de { "PDF und Bilder" } else { "PDF and images" },
                &[&["pdf"][..], &super::page_dialogs::IMAGE_EXT[..]].concat(),
            )
            .pick_files()
        else {
            return;
        };
        match crate::Document::from_files(&files, if de { "Neu.pdf" } else { "New.pdf" }) {
            Ok(doc) => {
                self.add_tab(doc);
                ctx.request_repaint();
            }
            Err(err) => self.show_error(&err),
        }
    }

    fn export_as(&mut self, ctx: &Context, word: bool) {
        let Some(tab) = self.active_tab() else { return };
        let stem = tab.doc.display_name().trim_end_matches(".pdf").trim_end_matches(".PDF").to_string();
        let (ext, filter, cmd) =
            if word { ("docx", "Word", Cmd::ExportWord) } else { ("txt", "Text", Cmd::ExportText) };
        let Some(path) = rfd::FileDialog::new()
            .set_title(cmd.label(self.settings.language))
            .add_filter(filter, &[ext])
            .set_file_name(format!("{stem}.{ext}"))
            .save_file()
        else {
            return;
        };
        let result =
            self.active_tab().map(|t| if word { t.doc.export_word(&path) } else { t.doc.export_text(&path) });
        match result {
            Some(Ok(())) => {
                let msg = if self.review_de() { "Exportiert: " } else { "Exported: " };
                self.set_status(ctx, format!("{msg}{}", path.display()));
            }
            Some(Err(err)) => self.show_error(&err),
            None => {}
        }
    }

    fn accessibility_report(&mut self, ctx: &Context) {
        let de = self.review_de();
        let Some(tab) = self.tab() else { return };
        let name = tab.doc.display_name().to_string();
        let result = (|| {
            let s = tab.doc.accessibility_structure()?;
            let empty = tab.doc.pages_without_text()?;
            let pages = tab.doc.page_count();
            let bookmarks = !tab.doc.bookmarks().is_empty();
            Ok::<_, crate::Error>((s, empty, pages, bookmarks))
        })();
        let (s, empty, pages, bookmarks) = match result {
            Ok(r) => r,
            Err(err) => {
                self.show_error(&err);
                return;
            }
        };
        let list =
            |v: &[usize]| v.iter().take(20).map(|p| (p + 1).to_string()).collect::<Vec<_>>().join(", ");
        // (passed, title, explanation)
        let checks: Vec<(bool, String, String)> = if de {
            vec![
                (s.title, "Dokumenttitel".into(), "Der Titel wird in den Dokumenteigenschaften festgelegt (Datei > Dokumenteigenschaften).".into()),
                (s.display_title, "Titel statt Dateiname anzeigen".into(), "Viewer sollen den Titel anzeigen (DisplayDocTitle).".into()),
                (s.language.is_some(), "Dokumentsprache".into(), s.language.clone().map_or("Keine Sprache angegeben (/Lang). Vorleseprogramme wählen sonst die falsche Aussprache.".into(), |l| format!("Sprache: {l}"))),
                (s.tagged, "Tags (logische Struktur)".into(), "Ohne Tags können Vorleseprogramme Überschriften, Listen und Lesereihenfolge nicht erkennen.".into()),
                (empty.is_empty(), "Durchsuchbarer Text".into(), if empty.is_empty() { "Alle Seiten enthalten Text.".into() } else { format!("Seiten ohne Text (vermutlich gescannt, Texterkennung nötig): {}", list(&empty)) }),
                (pages <= 20 || bookmarks, "Lesezeichen".into(), "Dokumente mit mehr als 20 Seiten sollten Lesezeichen haben.".into()),
                (s.fields_without_tooltip == 0, "Beschriftung der Formularfelder".into(), format!("{} von {} Formularfeldern ohne Kurzbeschreibung (Tooltip).", s.fields_without_tooltip, s.form_fields)),
                (s.pages_without_tab_order.is_empty(), "Tab-Reihenfolge".into(), if s.pages_without_tab_order.is_empty() { "In Ordnung.".into() } else { format!("Seiten mit Anmerkungen ohne Tab-Reihenfolge nach Struktur: {}", list(&s.pages_without_tab_order)) }),
            ]
        } else {
            vec![
                (
                    s.title,
                    "Document title".into(),
                    "Set the title in the document properties (File > Document properties).".into(),
                ),
                (
                    s.display_title,
                    "Show title instead of file name".into(),
                    "Viewers should display the title (DisplayDocTitle).".into(),
                ),
                (
                    s.language.is_some(),
                    "Document language".into(),
                    s.language.clone().map_or(
                        "No language specified (/Lang). Screen readers may use the wrong pronunciation."
                            .into(),
                        |l| format!("Language: {l}"),
                    ),
                ),
                (
                    s.tagged,
                    "Tags (logical structure)".into(),
                    "Without tags screen readers cannot detect headings, lists and reading order.".into(),
                ),
                (
                    empty.is_empty(),
                    "Searchable text".into(),
                    if empty.is_empty() {
                        "All pages contain text.".into()
                    } else {
                        format!("Pages without text (probably scanned, OCR needed): {}", list(&empty))
                    },
                ),
                (
                    pages <= 20 || bookmarks,
                    "Bookmarks".into(),
                    "Documents with more than 20 pages should have bookmarks.".into(),
                ),
                (
                    s.fields_without_tooltip == 0,
                    "Form field descriptions".into(),
                    format!(
                        "{} of {} form fields without a tooltip.",
                        s.fields_without_tooltip, s.form_fields
                    ),
                ),
                (
                    s.pages_without_tab_order.is_empty(),
                    "Tab order".into(),
                    if s.pages_without_tab_order.is_empty() {
                        "OK.".into()
                    } else {
                        format!(
                            "Pages with annotations without structure tab order: {}",
                            list(&s.pages_without_tab_order)
                        )
                    },
                ),
            ]
        };
        let passed = checks.iter().filter(|c| c.0).count();
        let title = if de { "Barrierefreiheit – Prüfbericht" } else { "Accessibility report" };
        let mut lines = vec![
            Line::title(title),
            Line::small(name),
            Line::new(if de {
                format!("{passed} von {} Prüfungen bestanden", checks.len())
            } else {
                format!("{passed} of {} checks passed", checks.len())
            }),
        ];
        for (ok, name, detail) in checks {
            let status = match (ok, de) {
                (true, true) => "Bestanden",
                (true, false) => "Passed",
                (false, true) => "Nicht bestanden",
                (false, false) => "Failed",
            };
            let mut head = Line { bold: true, space_before: 8.0, right: status.into(), ..Line::new(name) };
            head.swatch = Some(if ok { GREEN } else { RED });
            lines.push(head);
            lines.push(Line::new(detail).indent(14.0));
        }
        self.open_report(ctx, title, &lines);
    }
}
