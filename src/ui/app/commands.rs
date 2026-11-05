//! Central command table: every action of the ribbon, the quick start search
//! and keyboard shortcuts is a [`Cmd`]. Each command has a label, an icon and
//! an availability state; [`VersoloPdfApp::run`] executes it.

use egui::Context;

use super::comment_cmds::CommentCmd;
use super::structure::MarkKind;
use super::view_cmds::ViewCmd;
use super::{Dialog, InsertPosition, RangePurpose, SidePanel, VersoloPdfApp};
use crate::ui::i18n::Language;
use crate::ui::icons::Icon;
use crate::ui::tools::Tool;
use crate::ui::view::{PageLayout, ZoomMode};

/// Page side for "Weiße Ränder abschneiden".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    All,
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cmd {
    // File and quick access
    New,
    Open,
    Save,
    SaveAs,
    Print,
    PrintAll,
    Close,
    CloseAll,
    EmailAll,
    SaveOptimized,
    SpaceUsage,
    Revert,
    RenameFile,
    MoveFile,
    TrashFile,
    Exit,
    DocProperties,
    Settings,
    ExportSettings,
    ImportSettings,
    ResetSettings,
    Email,
    OpenFolder,
    ViewBack,
    ViewForward,
    Undo,
    Redo,
    // Tools
    Tool(Tool),
    CopyText,
    SelectAll,
    Search,
    InsertImage,
    WebLinksCreate,
    WebLinksRemove,
    // View
    FirstPage,
    PrevPage,
    NextPage,
    LastPage,
    ActualSize,
    FitPage,
    FitWidth,
    FitContent,
    ZoomIn,
    ZoomOut,
    RotateViewLeft,
    RotateViewRight,
    LayoutSingle,
    LayoutContinuous,
    LayoutFacing,
    LayoutCover,
    ToggleSidebar,
    ToggleProperties,
    Fullscreen,
    DarkMode,
    Panel(SidePanel),
    // Comments
    CommentList,
    ToggleAnnotations,
    SummarizeComments,
    ExportComments,
    ImportComments,
    PrevComment,
    NextComment,
    Flatten,
    DeleteComment,
    DeleteAllComments,
    // Protect
    PasswordProtect,
    RemoveProtection,
    ApplyRedactions,
    SearchRedact,
    /// "Dokument signieren" (digital signature with a digital ID).
    SignDocument,
    /// "Zertifizieren" (visible or invisible) and "Zeitstempel".
    Certify,
    CertifyInvisible,
    TimeStampDocument,
    /// "Signaturen überprüfen".
    VerifySignatures,
    // Form
    HighlightFields,
    ResetForm,
    ExportFormData,
    ImportFormData,
    // Document structure
    InsertBlank,
    InsertFromFile,
    AppendFile,
    DeletePages,
    ExtractPages,
    MovePageUp,
    MovePageDown,
    MergeFiles,
    SplitDocument,
    DuplicatePages,
    ReplacePages,
    SwapPages,
    NormalizePages,
    FindDuplicatePages,
    ReversePages,
    RotateLeft,
    RotateRight,
    RotatePages,
    CropPages,
    ResizePages,
    Margins,
    Watermark,
    Background,
    HeaderFooter,
    Bates,
    PageLabels,
    InsertClipboard,
    InsertImages,
    InsertText,
    InsertRtf,
    DeleteBlankPages,
    MovePages,
    /// Several pages per sheet (Seiten zusammenfuehren).
    NUp,
    SplitPages,
    /// "Seiten an Hilfslinien aufteilen": the split dialog with the guides as lines.
    SplitAtGuides,
    Overlay,
    Rotate180,
    RemoveCropped,
    TrimWhite(Side),
    RemoveMarks(MarkKind),
    ManageMarks(MarkKind),
    BatesFiles,
    // Convert
    FromFiles,
    ContentsToImages,
    ExportImages,
    ExportText,
    ExportWord,
    ExportExcel,
    ExportPowerPoint,
    RecognizeText,
    ReplaceFonts,
    RecolorPages,
    SpellCheck,
    DeletePageComments,
    FlattenAll,
    Grayscale,
    // Review / accessibility
    WordCount,
    CompareDocuments,
    AccessibilityCheck,
    // Bookmarks
    BookmarkAdd,
    BookmarkFromSelection,
    BookmarkEveryNth,
    BookmarkFromHighlights,
    BookmarkFromTextFile,
    BookmarkUpper,
    BookmarkLower,
    BookmarkTitleCase,
    BookmarkReplace,
    BookmarkSort,
    BookmarkDedupe,
    BookmarkDeleteAll,
    BookmarkToc,
    BookmarkExportHtml,
    BookmarkExportText,
    BookmarkFromToc,
    BookmarkAddText,
    BookmarkZoom,
    BookmarkResolveNamed,
    BookmarkRemoveActions,
    BookmarkCheck,
    BookmarkLinks,
    BookmarkSortPages,
    BookmarkToNamed,
    BookmarkCase,
    // Help
    Manual,
    Support,
    ReportProblem,
    /// "Fehlerprotokoll öffnen": the folder of the log and crash reports.
    OpenLog,
    CommandLineHelp,
    CheckUpdate,
    About,
    FileAssociation,
    Homepage,
    /// Commands of the ribbon tab "Ansicht" (see `view_cmds.rs`).
    View(super::view_cmds::ViewCmd),
    /// Commands of the ribbon tab "Anmerkungen" (see `comment_cmds.rs`).
    Comment(super::comment_cmds::CommentCmd),
    /// Commands of the ribbon tab "Formular" (see `form_cmds.rs`).
    Form(super::form_cmds::FormCmd),
    /// "Text bearbeiten" / "Objekte bearbeiten".
    Content(crate::ui::content_state::ContentCmd),
}

pub const HOMEPAGE: &str = "https://github.com/MrMarkus42/VersoloPDF";

/// Commands offered by the quick start search.
pub const SEARCHABLE: &[Cmd] = &[
    Cmd::New,
    Cmd::Open,
    Cmd::Save,
    Cmd::SaveAs,
    Cmd::Print,
    Cmd::PrintAll,
    Cmd::Close,
    Cmd::CloseAll,
    Cmd::EmailAll,
    Cmd::SaveOptimized,
    Cmd::SpaceUsage,
    Cmd::Revert,
    Cmd::RenameFile,
    Cmd::MoveFile,
    Cmd::TrashFile,
    Cmd::DocProperties,
    Cmd::Settings,
    Cmd::ExportSettings,
    Cmd::ImportSettings,
    Cmd::ResetSettings,
    Cmd::Email,
    Cmd::OpenFolder,
    Cmd::ViewBack,
    Cmd::ViewForward,
    Cmd::Undo,
    Cmd::Redo,
    Cmd::Tool(Tool::Hand),
    Cmd::Tool(Tool::SelectText),
    Cmd::Tool(Tool::SelectAnnot),
    Cmd::Tool(Tool::Zoom),
    Cmd::Tool(Tool::Snapshot),
    Cmd::Tool(Tool::Highlight),
    Cmd::Tool(Tool::AreaHighlight),
    Cmd::Tool(Tool::Underline),
    Cmd::Tool(Tool::StrikeOut),
    Cmd::Tool(Tool::Squiggly),
    Cmd::Tool(Tool::Replace),
    Cmd::Tool(Tool::Caret),
    Cmd::Tool(Tool::ContentHighlight),
    Cmd::Tool(Tool::TextCorrection),
    Cmd::Comment(CommentCmd::ShowAll),
    Cmd::Comment(CommentCmd::HideAll),
    Cmd::Comment(CommentCmd::OpenPopups),
    Cmd::Comment(CommentCmd::ClosePopups),
    Cmd::Comment(CommentCmd::Calculator),
    Cmd::Comment(CommentCmd::CommentStyles),
    Cmd::Tool(Tool::FieldDate),
    Cmd::Tool(Tool::FieldBarcode),
    Cmd::Tool(Tool::FieldImage),
    Cmd::Form(super::form_cmds::FormCmd::FormPane),
    Cmd::Form(super::form_cmds::FormCmd::DetectForms),
    Cmd::Form(super::form_cmds::FormCmd::EmailFormData),
    Cmd::Form(super::form_cmds::FormCmd::FillForms),
    Cmd::Form(super::form_cmds::FormCmd::AllJavaScripts),
    Cmd::Tool(Tool::EditText),
    Cmd::Tool(Tool::EditObjects),
    Cmd::Content(crate::ui::content_state::ContentCmd::AddText),
    Cmd::Content(crate::ui::content_state::ContentCmd::AddImage),
    Cmd::Tool(Tool::Note),
    Cmd::Tool(Tool::FileAttach),
    Cmd::Tool(Tool::TextBox),
    Cmd::Tool(Tool::Typewriter),
    Cmd::Tool(Tool::Callout),
    Cmd::Tool(Tool::Rectangle),
    Cmd::Tool(Tool::Ellipse),
    Cmd::Tool(Tool::Line),
    Cmd::Tool(Tool::Arrow),
    Cmd::Tool(Tool::Polygon),
    Cmd::Tool(Tool::Polyline),
    Cmd::Tool(Tool::Cloud),
    Cmd::Tool(Tool::Pencil),
    Cmd::Tool(Tool::Eraser),
    Cmd::Tool(Tool::Stamp),
    Cmd::Tool(Tool::MeasureDistance),
    Cmd::Tool(Tool::MeasurePerimeter),
    Cmd::Tool(Tool::MeasureArea),
    Cmd::Tool(Tool::Link),
    Cmd::Tool(Tool::Redact),
    Cmd::Tool(Tool::Signature),
    Cmd::Tool(Tool::SelectFields),
    Cmd::Tool(Tool::FieldText),
    Cmd::Tool(Tool::FieldButton),
    Cmd::Tool(Tool::FieldCheckBox),
    Cmd::Tool(Tool::FieldRadio),
    Cmd::Tool(Tool::FieldList),
    Cmd::Tool(Tool::FieldCombo),
    Cmd::Tool(Tool::FieldSignature),
    Cmd::CopyText,
    Cmd::SelectAll,
    Cmd::Search,
    Cmd::InsertImage,
    Cmd::WebLinksCreate,
    Cmd::WebLinksRemove,
    Cmd::FirstPage,
    Cmd::PrevPage,
    Cmd::NextPage,
    Cmd::LastPage,
    Cmd::ActualSize,
    Cmd::FitPage,
    Cmd::FitWidth,
    Cmd::FitContent,
    Cmd::ZoomIn,
    Cmd::ZoomOut,
    Cmd::RotateViewLeft,
    Cmd::RotateViewRight,
    Cmd::LayoutSingle,
    Cmd::LayoutContinuous,
    Cmd::LayoutFacing,
    Cmd::LayoutCover,
    Cmd::ToggleSidebar,
    Cmd::ToggleProperties,
    Cmd::Fullscreen,
    Cmd::DarkMode,
    Cmd::CommentList,
    Cmd::ToggleAnnotations,
    Cmd::SummarizeComments,
    Cmd::ExportComments,
    Cmd::ImportComments,
    Cmd::PrevComment,
    Cmd::NextComment,
    Cmd::Flatten,
    Cmd::DeleteComment,
    Cmd::DeleteAllComments,
    Cmd::PasswordProtect,
    Cmd::RemoveProtection,
    Cmd::ApplyRedactions,
    Cmd::SearchRedact,
    Cmd::SignDocument,
    Cmd::Certify,
    Cmd::CertifyInvisible,
    Cmd::TimeStampDocument,
    Cmd::VerifySignatures,
    Cmd::HighlightFields,
    Cmd::ResetForm,
    Cmd::ExportFormData,
    Cmd::ImportFormData,
    Cmd::InsertBlank,
    Cmd::InsertFromFile,
    Cmd::AppendFile,
    Cmd::DeletePages,
    Cmd::ExtractPages,
    Cmd::MovePageUp,
    Cmd::MovePageDown,
    Cmd::MergeFiles,
    Cmd::SplitDocument,
    Cmd::DuplicatePages,
    Cmd::ReplacePages,
    Cmd::SwapPages,
    Cmd::NormalizePages,
    Cmd::FindDuplicatePages,
    Cmd::ReversePages,
    Cmd::RotateLeft,
    Cmd::RotateRight,
    Cmd::RotatePages,
    Cmd::CropPages,
    Cmd::ResizePages,
    Cmd::Margins,
    Cmd::Watermark,
    Cmd::Background,
    Cmd::HeaderFooter,
    Cmd::Bates,
    Cmd::PageLabels,
    Cmd::InsertClipboard,
    Cmd::InsertImages,
    Cmd::InsertText,
    Cmd::InsertRtf,
    Cmd::DeleteBlankPages,
    Cmd::MovePages,
    Cmd::NUp,
    Cmd::SplitPages,
    Cmd::SplitAtGuides,
    Cmd::Overlay,
    Cmd::Rotate180,
    Cmd::Tool(Tool::Crop),
    Cmd::RemoveCropped,
    Cmd::TrimWhite(Side::All),
    Cmd::TrimWhite(Side::Left),
    Cmd::TrimWhite(Side::Right),
    Cmd::TrimWhite(Side::Top),
    Cmd::TrimWhite(Side::Bottom),
    Cmd::RemoveMarks(MarkKind::Watermark),
    Cmd::RemoveMarks(MarkKind::Background),
    Cmd::RemoveMarks(MarkKind::HeaderFooter),
    Cmd::RemoveMarks(MarkKind::Bates),
    Cmd::ManageMarks(MarkKind::Watermark),
    Cmd::ManageMarks(MarkKind::Background),
    Cmd::ManageMarks(MarkKind::HeaderFooter),
    Cmd::ManageMarks(MarkKind::Bates),
    Cmd::BatesFiles,
    Cmd::FromFiles,
    Cmd::ContentsToImages,
    Cmd::ExportImages,
    Cmd::ExportText,
    Cmd::ExportWord,
    Cmd::ExportExcel,
    Cmd::ExportPowerPoint,
    Cmd::RecognizeText,
    Cmd::ReplaceFonts,
    Cmd::RecolorPages,
    Cmd::SpellCheck,
    Cmd::DeletePageComments,
    Cmd::FlattenAll,
    Cmd::Grayscale,
    Cmd::WordCount,
    Cmd::CompareDocuments,
    Cmd::AccessibilityCheck,
    Cmd::BookmarkAdd,
    Cmd::BookmarkFromSelection,
    Cmd::BookmarkEveryNth,
    Cmd::BookmarkFromHighlights,
    Cmd::BookmarkFromTextFile,
    Cmd::BookmarkUpper,
    Cmd::BookmarkLower,
    Cmd::BookmarkTitleCase,
    Cmd::BookmarkReplace,
    Cmd::BookmarkSort,
    Cmd::BookmarkDedupe,
    Cmd::BookmarkDeleteAll,
    Cmd::BookmarkToc,
    Cmd::BookmarkExportHtml,
    Cmd::BookmarkExportText,
    Cmd::BookmarkFromToc,
    Cmd::BookmarkAddText,
    Cmd::BookmarkZoom,
    Cmd::BookmarkResolveNamed,
    Cmd::BookmarkRemoveActions,
    Cmd::BookmarkCheck,
    Cmd::BookmarkLinks,
    Cmd::BookmarkSortPages,
    Cmd::BookmarkToNamed,
    Cmd::BookmarkCase,
    Cmd::Manual,
    Cmd::Support,
    Cmd::ReportProblem,
    Cmd::OpenLog,
    Cmd::CommandLineHelp,
    Cmd::CheckUpdate,
    Cmd::About,
    Cmd::FileAssociation,
    Cmd::Homepage,
    Cmd::Tool(Tool::DynamicZoom),
    Cmd::Tool(Tool::Loupe),
    Cmd::View(ViewCmd::LayoutTwoPagesContinuous),
    Cmd::View(ViewCmd::PageGaps),
    Cmd::View(ViewCmd::RightToLeft),
    Cmd::View(ViewCmd::SideBySide),
    Cmd::View(ViewCmd::RotateReset),
    Cmd::View(ViewCmd::Rulers),
    Cmd::View(ViewCmd::Grid),
    Cmd::View(ViewCmd::Guides),
    Cmd::View(ViewCmd::Transparency),
    Cmd::View(ViewCmd::PageInfo),
    Cmd::View(ViewCmd::Snap),
    Cmd::View(ViewCmd::ReplaceColors),
    Cmd::View(ViewCmd::AutoScroll),
    Cmd::View(ViewCmd::HighlightLinks),
    Cmd::View(ViewCmd::PanZoom),
    Cmd::View(ViewCmd::SplitHorizontal),
    Cmd::View(ViewCmd::SplitVertical),
    Cmd::View(ViewCmd::SplitGrid),
    Cmd::View(ViewCmd::SortTabs),
    Cmd::View(ViewCmd::CloseOthers),
    Cmd::View(ViewCmd::ReopenClosed),
    Cmd::View(ViewCmd::ResetPanes),
    Cmd::View(ViewCmd::ReadPage),
    Cmd::View(ViewCmd::ReadToEnd),
    Cmd::View(ViewCmd::ReadSelection),
    Cmd::View(ViewCmd::ReadStop),
];

impl Cmd {
    /// German and English label.
    pub fn names(self) -> (&'static str, &'static str) {
        use Cmd::*;
        match self {
            New => ("Neues Dokument", "New document"),
            Open => ("Öffnen", "Open"),
            Save => ("Speichern", "Save"),
            SaveAs => ("Speichern unter", "Save as"),
            Print => ("Drucken", "Print"),
            Close => ("Schließen", "Close"),
            CloseAll => ("Alle schließen", "Close all"),
            PrintAll => ("Alle drucken", "Print all"),
            EmailAll => ("Alle Dokumente per E-Mail senden", "Send all documents by e-mail"),
            SaveOptimized => ("Optimiert speichern", "Save optimized"),
            SpaceUsage => ("Speicherverbrauch untersuchen", "Examine space usage"),
            Revert => ("Zurücksetzen", "Revert"),
            RenameFile => ("Umbenennen", "Rename"),
            MoveFile => ("Datei verschieben", "Move file"),
            TrashFile => ("In den Papierkorb werfen", "Move to recycle bin"),
            Exit => ("Beenden", "Exit"),
            DocProperties => ("Dokumenteigenschaften", "Document properties"),
            Settings => ("Einstellungen", "Settings"),
            ExportSettings => ("Einstellungen exportieren", "Export settings"),
            ImportSettings => ("Einstellungen importieren", "Import settings"),
            ResetSettings => ("Einstellungen zurücksetzen", "Reset settings"),
            Email => ("Per E-Mail senden", "Send by e-mail"),
            OpenFolder => ("Ordner öffnen", "Open folder"),
            ViewBack => ("Letzte Ansicht", "Previous view"),
            ViewForward => ("Nächste Ansicht", "Next view"),
            Undo => ("Rückgängig", "Undo"),
            Redo => ("Wiederholen", "Redo"),
            Tool(t) => tool_names(t),
            CopyText => ("Text kopieren", "Copy text"),
            SelectAll => ("Alles auswählen", "Select all"),
            Search => ("Suche", "Search"),
            InsertImage => ("Bild", "Image"),
            WebLinksCreate => ("Web-Links zu URLs erstellen", "Create web links from URLs"),
            WebLinksRemove => ("Alle Web-Links entfernen", "Remove all web links"),
            FirstPage => ("Erste Seite", "First page"),
            PrevPage => ("Vorherige Seite", "Previous page"),
            NextPage => ("Nächste Seite", "Next page"),
            LastPage => ("Letzte Seite", "Last page"),
            ActualSize => ("Tatsächliche Größe", "Actual size"),
            FitPage => ("Ganze Seite", "Fit page"),
            FitWidth => ("Breite", "Fit width"),
            ZoomIn => ("Vergrößern", "Zoom in"),
            ZoomOut => ("Verkleinern", "Zoom out"),
            RotateViewLeft => ("Ansicht links drehen", "Rotate view left"),
            RotateViewRight => ("Ansicht rechts drehen", "Rotate view right"),
            LayoutSingle => ("Einzelne Seite", "Single page"),
            LayoutContinuous => ("Fortlaufend", "Continuous"),
            LayoutFacing => ("Zwei Seiten", "Two pages"),
            LayoutCover => {
                ("Titelseite in der zweiseitigen Ansicht einzeln", "Show cover page alone in two-page view")
            }
            ToggleSidebar => ("Seitenleiste", "Sidebar"),
            ToggleProperties => ("Eigenschaften", "Properties"),
            Fullscreen => ("Vollbild", "Full screen"),
            DarkMode => ("Design", "Theme"),
            Panel(SidePanel::Thumbnails) => ("Seiten", "Pages"),
            Panel(SidePanel::Bookmarks) => ("Lesezeichen", "Bookmarks"),
            Panel(SidePanel::Comments) => ("Kommentare", "Comments"),
            Panel(SidePanel::Attachments) => ("Anhänge", "Attachments"),
            Panel(SidePanel::Fields) => ("Formular", "Form"),
            Panel(SidePanel::Layers) => ("Ebenen", "Layers"),
            Panel(SidePanel::Search) => ("Suchen", "Search"),
            CommentList => ("Kommentarliste", "Comment list"),
            ToggleAnnotations => ("Anzeigen", "Show"),
            SummarizeComments => ("Anmerkungen zusammenfassen", "Summarize comments"),
            ExportComments => ("Exportieren", "Export"),
            ImportComments => ("Importieren", "Import"),
            PrevComment => ("Vorherige", "Previous"),
            NextComment => ("Nächste", "Next"),
            Flatten => ("Anmerkungen einbetten", "Flatten comments"),
            DeleteComment => ("Anmerkung löschen", "Delete comment"),
            DeleteAllComments => ("Anmerkungen löschen", "Delete comments"),
            PasswordProtect => ("Sicherheitseinstellungen", "Security settings"),
            RemoveProtection => ("Schutz entfernen", "Remove protection"),
            ApplyRedactions => ("Übernehmen", "Apply"),
            SearchRedact => ("Suchen und schwärzen", "Search and redact"),
            SignDocument => ("Dokument signieren", "Sign document"),
            Certify => ("Zertifizieren", "Certify"),
            CertifyInvisible => ("Zertifizieren (unsichtbar)", "Certify (invisible)"),
            TimeStampDocument => ("Zeitstempel", "Time stamp"),
            VerifySignatures => ("Signaturen überprüfen", "Check signatures"),
            HighlightFields => ("Felder hervorheben", "Highlight fields"),
            ResetForm => ("Formular zurücksetzen", "Reset form"),
            ExportFormData => ("Formulardaten exportieren", "Export form data"),
            ImportFormData => ("Formulardaten importieren", "Import form data"),
            InsertBlank => ("Leere Seiten einfügen", "Insert blank pages"),
            InsertFromFile => ("Seiten einfügen", "Insert pages"),
            AppendFile => ("Datei anhängen", "Append file"),
            DeletePages => ("Seiten löschen", "Delete pages"),
            ExtractPages => ("Seiten extrahieren", "Extract pages"),
            MovePageUp => ("Seite nach vorne", "Move page up"),
            MovePageDown => ("Seite nach hinten", "Move page down"),
            MergeFiles => ("Dateien zusammenführen", "Merge files"),
            SplitDocument => ("Dokument aufteilen", "Split document"),
            InsertClipboard => ("Seiten aus der Zwischenablage einfügen", "Insert pages from clipboard"),
            InsertImages => ("Bilder einfügen", "Insert images"),
            InsertText => ("Text einfügen", "Insert text file"),
            InsertRtf => ("RTF einfügen", "Insert RTF"),
            DeleteBlankPages => ("Leere Seiten löschen", "Delete blank pages"),
            MovePages => ("Seiten verschieben", "Move pages"),
            NUp => ("Seiten zusammenführen", "Combine pages (n-up)"),
            SplitPages => ("Seiten aufteilen", "Split pages"),
            SplitAtGuides => ("Seiten an Hilfslinien aufteilen", "Split pages at guides"),
            Overlay => ("Überlagern", "Overlay"),
            Rotate180 => ("Um 180° drehen", "Rotate 180°"),
            RemoveCropped => ("Abgeschnittene Inhalte entfernen", "Remove cropped content"),
            TrimWhite(Side::All) => ("Weiße Ränder abschneiden", "Trim white margins"),
            TrimWhite(Side::Left) => ("Weiße Ränder abschneiden: nur links", "Trim white margins: left only"),
            TrimWhite(Side::Right) => {
                ("Weiße Ränder abschneiden: nur rechts", "Trim white margins: right only")
            }
            TrimWhite(Side::Top) => ("Weiße Ränder abschneiden: nur oben", "Trim white margins: top only"),
            TrimWhite(Side::Bottom) => {
                ("Weiße Ränder abschneiden: nur unten", "Trim white margins: bottom only")
            }
            RemoveMarks(MarkKind::Watermark) => ("Alle Wasserzeichen entfernen", "Remove all watermarks"),
            RemoveMarks(MarkKind::Background) => ("Hintergrund entfernen", "Remove background"),
            RemoveMarks(MarkKind::HeaderFooter) => {
                ("Alle Kopf- und Fußzeilen entfernen", "Remove all headers and footers")
            }
            RemoveMarks(MarkKind::Bates) => ("Alle Paginierungen entfernen", "Remove all Bates numbers"),
            ManageMarks(MarkKind::Watermark) => ("Wasserzeichen verwalten", "Manage watermarks"),
            ManageMarks(MarkKind::Background) => ("Hintergründe verwalten", "Manage backgrounds"),
            ManageMarks(MarkKind::HeaderFooter) => {
                ("Kopf- und Fußzeilen verwalten", "Manage headers and footers")
            }
            ManageMarks(MarkKind::Bates) => ("Paginierung verwalten", "Manage Bates numbering"),
            BatesFiles => {
                ("Paginierung zu mehreren Dateien hinzufügen", "Add Bates numbering to several files")
            }
            FitContent => ("An Inhalt anpassen", "Fit content"),
            DuplicatePages => ("Seiten duplizieren", "Duplicate pages"),
            ReplacePages => ("Seiten ersetzen", "Replace pages"),
            SwapPages => ("Seiten austauschen", "Swap pages"),
            NormalizePages => ("Seiten normalisieren", "Normalize pages"),
            FindDuplicatePages => ("Duplikate finden", "Find duplicates"),
            ReversePages => ("Seitenreihenfolge umkehren", "Reverse page order"),
            RotateLeft => ("Links drehen", "Rotate left"),
            RotateRight => ("Rechts drehen", "Rotate right"),
            RotatePages => ("Seiten drehen", "Rotate pages"),
            CropPages => ("Seiten zuschneiden", "Crop pages"),
            ResizePages => ("Größe ändern", "Resize"),
            Margins => ("Ränder", "Margins"),
            Watermark => ("Wasserzeichen", "Watermark"),
            Background => ("Hintergrund", "Background"),
            HeaderFooter => ("Kopf- und Fußzeilen", "Header and footer"),
            Bates => ("Paginierung", "Bates numbering"),
            PageLabels => ("Seiten nummerieren", "Number pages"),
            FromFiles => ("Aus Dateien", "From files"),
            ContentsToImages => ("Inhalte in Bilder umwandeln", "Convert contents to images"),
            ExportImages => ("In Bilder", "To images"),
            ExportText => ("In Text", "To text"),
            ExportWord => ("In MS Word", "To MS Word"),
            ExportExcel => ("In MS Excel", "To MS Excel"),
            ExportPowerPoint => ("In MS PowerPoint", "To MS PowerPoint"),
            RecognizeText => ("Text auf Seiten erkennen", "Recognize text"),
            ReplaceFonts => ("Schriften ersetzen", "Replace fonts"),
            RecolorPages => ("Seiten neu einfärben", "Recolor pages"),
            SpellCheck => ("Rechtschreibprüfung", "Spell check"),
            DeletePageComments => ("Anmerkungen dieser Seite löschen", "Delete comments of this page"),
            FlattenAll => ("Alle Anmerkungen einbetten", "Flatten all comments"),
            Grayscale => ("Farben umwandeln", "Convert colours"),
            WordCount => ("Wörter zählen", "Count words"),
            CompareDocuments => ("Dokumente vergleichen", "Compare documents"),
            AccessibilityCheck => ("Barrierefreiheit prüfen", "Check accessibility"),
            BookmarkAdd => ("Lesezeichen hinzufügen", "Add bookmark"),
            BookmarkFromSelection => ("Aus Seitentext", "From page text"),
            BookmarkEveryNth => ("Lesezeichen für jede n-te Seite", "Bookmark every n-th page"),
            BookmarkFromHighlights => ("Aus Hervorhebungen", "From highlights"),
            BookmarkFromTextFile => ("Aus Textdatei", "From text file"),
            BookmarkUpper => ("GROSSBUCHSTABEN", "UPPER CASE"),
            BookmarkLower => ("kleinbuchstaben", "lower case"),
            BookmarkTitleCase => ("Erster Buchstabe groß", "Title case"),
            BookmarkReplace => ("Suchen und ersetzen", "Find and replace"),
            BookmarkSort => ("Sortieren", "Sort"),
            BookmarkDedupe => ("Duplikate zusammenfassen", "Merge duplicates"),
            BookmarkDeleteAll => ("Alle Lesezeichen löschen", "Delete all bookmarks"),
            BookmarkToc => ("Inhaltsverzeichnis erstellen", "Create table of contents"),
            BookmarkExportHtml => ("Als HTML exportieren", "Export as HTML"),
            BookmarkExportText => ("Als Textdatei exportieren", "Export as text file"),
            BookmarkFromToc => ("Aus Inhaltsverzeichnis", "From table of contents"),
            BookmarkAddText => ("Text hinzufügen", "Add text"),
            BookmarkZoom => ("Vergrößerung ändern", "Change zoom"),
            BookmarkResolveNamed => ("Sprungmarken zu normalen Zielen", "Named destinations to normal ones"),
            BookmarkRemoveActions => ("Aktionen entfernen", "Remove actions"),
            BookmarkCheck => ("Überprüfen", "Check"),
            BookmarkLinks => ("Verweise aus Lesezeichen", "Links from bookmarks"),
            BookmarkSortPages => ("Seiten sortieren", "Sort pages"),
            BookmarkCase => ("Groß-/Kleinschreibung", "Change case"),
            BookmarkToNamed => ("In Sprungmarken umwandeln", "Convert to named destinations"),
            Manual => ("Handbuch", "Manual"),
            Support => ("Support", "Support"),
            ReportProblem => ("Problem melden", "Report a problem"),
            OpenLog => ("Fehlerprotokoll öffnen", "Open error log"),
            CommandLineHelp => ("Hilfe zur Kommandozeile", "Command line help"),
            CheckUpdate => ("Nach neuer Version suchen", "Check for updates"),
            About => ("Über", "About"),
            FileAssociation => ("Dateizuordnung", "File association"),
            Homepage => ("Homepage", "Homepage"),
            View(v) => v.names(),
            Comment(c) => c.names(),
            Form(f) => f.names(),
            Content(c) => c.names(),
        }
    }

    pub fn label(self, lang: Language) -> &'static str {
        let (de, en) = self.names();
        if lang == Language::German { de } else { en }
    }

    pub fn icon(self) -> Icon {
        use Cmd::*;
        match self {
            New => Icon::New,
            Open => Icon::Open,
            Save => Icon::Save,
            SaveAs => Icon::SaveAs,
            Print => Icon::Print,
            Close | CloseAll => Icon::Close,
            PrintAll => Icon::Print,
            EmailAll => Icon::Mail,
            SaveOptimized => Icon::SaveAs,
            SpaceUsage => Icon::Count,
            Revert => Icon::Undo,
            RenameFile => Icon::TextBox,
            MoveFile => Icon::Open,
            TrashFile => Icon::Delete,
            Exit => Icon::Close,
            DocProperties => Icon::Info,
            Settings => Icon::Gear,
            ExportSettings => Icon::Export,
            ImportSettings => Icon::Import,
            ResetSettings => Icon::Undo,
            Email => Icon::Mail,
            OpenFolder => Icon::Open,
            ViewBack => Icon::Back,
            ViewForward => Icon::Forward,
            Undo => Icon::Undo,
            Redo => Icon::Redo,
            Tool(t) => t.icon(),
            CopyText => Icon::Copy,
            SelectAll => Icon::SelectText,
            Search => Icon::Search,
            InsertImage => Icon::Image,
            WebLinksCreate => Icon::Link,
            WebLinksRemove => Icon::LinkOff,
            FirstPage => Icon::First,
            PrevPage => Icon::Prev,
            NextPage => Icon::Next,
            LastPage => Icon::Last,
            ActualSize => Icon::ActualSize,
            FitPage => Icon::FitPage,
            FitWidth => Icon::FitWidth,
            ZoomIn => Icon::ZoomIn,
            ZoomOut => Icon::ZoomOut,
            RotateViewLeft => Icon::RotateLeft,
            RotateViewRight => Icon::RotateRight,
            LayoutSingle => Icon::SinglePage,
            LayoutContinuous => Icon::Continuous,
            LayoutFacing => Icon::TwoPages,
            LayoutCover => Icon::CoverPages,
            ToggleSidebar => Icon::Sidebar,
            ToggleProperties => Icon::Properties,
            Fullscreen => Icon::Fullscreen,
            DarkMode => Icon::Theme,
            Panel(SidePanel::Thumbnails) => Icon::Thumbnails,
            Panel(SidePanel::Bookmarks) => Icon::Bookmarks,
            Panel(SidePanel::Comments) => Icon::Comments,
            Panel(SidePanel::Attachments) => Icon::Attachment,
            Panel(SidePanel::Fields) => Icon::Form,
            Panel(SidePanel::Layers) => Icon::Background,
            Panel(SidePanel::Search) => Icon::Search,
            CommentList => Icon::Comments,
            ToggleAnnotations => Icon::Eye,
            SummarizeComments => Icon::Summary,
            ExportComments => Icon::Export,
            ImportComments => Icon::Import,
            PrevComment => Icon::Prev,
            NextComment => Icon::Next,
            Flatten => Icon::Flatten,
            DeleteComment => Icon::Delete,
            DeleteAllComments => Icon::Delete,
            PasswordProtect => Icon::Lock,
            RemoveProtection => Icon::Unlock,
            ApplyRedactions => Icon::Redact,
            SearchRedact => Icon::Search,
            SignDocument => Icon::Signature,
            Certify | CertifyInvisible => Icon::Lock,
            TimeStampDocument => Icon::Info,
            VerifySignatures => Icon::Accessibility,
            HighlightFields => Icon::Form,
            ResetForm => Icon::Undo,
            ExportFormData => Icon::Export,
            ImportFormData => Icon::Import,
            InsertBlank => Icon::InsertPage,
            InsertFromFile => Icon::InsertFile,
            AppendFile => Icon::InsertFile,
            DeletePages => Icon::DeletePage,
            ExtractPages => Icon::ExtractPages,
            MovePageUp => Icon::MoveUp,
            MovePageDown => Icon::MoveDown,
            MergeFiles => Icon::Merge,
            SplitDocument => Icon::Split,
            DuplicatePages => Icon::Duplicate,
            ReplacePages => Icon::InsertFile,
            SwapPages => Icon::Swap,
            NormalizePages => Icon::Resize,
            FindDuplicatePages => Icon::Search,
            ReversePages => Icon::Swap,
            RotateLeft => Icon::RotateLeft,
            RotateRight => Icon::RotateRight,
            RotatePages => Icon::RotateRight,
            CropPages => Icon::Crop,
            ResizePages => Icon::Resize,
            Margins => Icon::Margins,
            Watermark => Icon::Watermark,
            Background => Icon::Background,
            HeaderFooter => Icon::HeaderFooter,
            Bates => Icon::Numbering,
            PageLabels => Icon::Numbering,
            FromFiles => Icon::InsertFile,
            ContentsToImages => Icon::Image,
            ExportImages => Icon::Image,
            ExportText => Icon::TextFile,
            ExportWord => Icon::WordFile,
            ExportExcel => Icon::Thumbnails,
            ExportPowerPoint => Icon::Fullscreen,
            RecognizeText => Icon::Search,
            ReplaceFonts => Icon::TextBox,
            RecolorPages => Icon::Theme,
            SpellCheck => Icon::SpellCheck,
            DeletePageComments => Icon::Delete,
            FlattenAll => Icon::Flatten,
            Grayscale => Icon::Theme,
            WordCount => Icon::Count,
            CompareDocuments => Icon::Compare,
            AccessibilityCheck => Icon::Accessibility,
            BookmarkAdd => Icon::Bookmarks,
            BookmarkFromSelection => Icon::SelectText,
            BookmarkEveryNth => Icon::Numbering,
            BookmarkFromHighlights => Icon::Highlight,
            BookmarkFromTextFile => Icon::TextFile,
            BookmarkUpper | BookmarkLower | BookmarkTitleCase => Icon::TextBox,
            BookmarkReplace => Icon::Search,
            BookmarkSort => Icon::Sort,
            BookmarkDedupe => Icon::Merge,
            BookmarkDeleteAll => Icon::Delete,
            BookmarkToc => Icon::Summary,
            BookmarkExportHtml => Icon::Export,
            BookmarkExportText => Icon::TextFile,
            BookmarkFromToc => Icon::Summary,
            BookmarkAddText => Icon::Typewriter,
            BookmarkZoom => Icon::ZoomIn,
            BookmarkResolveNamed | BookmarkToNamed => Icon::Link,
            BookmarkRemoveActions => Icon::LinkOff,
            BookmarkCheck => Icon::Search,
            BookmarkLinks => Icon::Link,
            BookmarkSortPages => Icon::Sort,
            BookmarkCase => Icon::TextBox,
            Manual => Icon::Help,
            Support => Icon::Help,
            ReportProblem => Icon::Bug,
            OpenLog => Icon::TextFile,
            CommandLineHelp => Icon::Console,
            CheckUpdate => Icon::Web,
            About => Icon::Logo,
            FileAssociation => Icon::Gear,
            Homepage => Icon::Web,
            InsertClipboard => Icon::Paste,
            InsertImages => Icon::Image,
            InsertText => Icon::TextFile,
            InsertRtf => Icon::WordFile,
            DeleteBlankPages => Icon::DeletePage,
            MovePages => Icon::MoveDown,
            NUp => Icon::Merge,
            SplitPages => Icon::Split,
            SplitAtGuides => Icon::Split,
            Overlay => Icon::Background,
            Rotate180 => Icon::Rotate180,
            RemoveCropped => Icon::Crop,
            TrimWhite(_) => Icon::Margins,
            RemoveMarks(_) => Icon::Delete,
            ManageMarks(_) => Icon::Gear,
            BatesFiles => Icon::InsertFile,
            FitContent => Icon::FitPage,
            View(v) => v.icon(),
            Comment(c) => c.icon(),
            Form(f) => f.icon(),
            Content(c) => c.icon(),
        }
    }

    /// Label inside drop-down menus with a shorter text.
    pub fn menu_names(self) -> (&'static str, &'static str) {
        use Cmd::*;
        match self {
            Watermark | Background | HeaderFooter | Bates => ("Hinzufügen…", "Add…"),
            RemoveMarks(MarkKind::Background) => ("Entfernen…", "Remove…"),
            RemoveMarks(_) => ("Alle entfernen…", "Remove all…"),
            ManageMarks(_) => ("Verwalten…", "Manage…"),
            BatesFiles => ("Zu mehreren Dateien hinzufügen…", "Add to several files…"),
            TrimWhite(Side::All) => ("Alle Ränder", "All margins"),
            TrimWhite(Side::Left) => ("Nur links", "Left only"),
            TrimWhite(Side::Right) => ("Nur rechts", "Right only"),
            TrimWhite(Side::Top) => ("Nur oben", "Top only"),
            TrimWhite(Side::Bottom) => ("Nur unten", "Bottom only"),
            Tool(crate::ui::tools::Tool::Crop) => ("Seite zuschneiden", "Crop page"),
            RotateLeft => ("90° gegen den Uhrzeigersinn", "90° counter-clockwise"),
            RotateRight => ("90° im Uhrzeigersinn", "90° clockwise"),
            Rotate180 => ("180°", "180°"),
            InsertFromFile => ("Seiten einfügen…", "Insert pages…"),
            InsertBlank => ("Leere Seiten einfügen…", "Insert blank pages…"),
            InsertClipboard => ("Seiten aus der Zwischenablage einfügen…", "Insert pages from clipboard…"),
            InsertImages => ("Bilder einfügen…", "Insert images…"),
            InsertText => ("Text einfügen…", "Insert text…"),
            InsertRtf => ("RTF einfügen…", "Insert RTF…"),
            DeletePages => ("Seiten löschen…", "Delete pages…"),
            DeleteBlankPages => ("Leere Seiten löschen…", "Delete blank pages…"),
            SplitDocument => ("Dokument aufteilen…", "Split document…"),
            SplitPages => ("Seiten aufteilen…", "Split pages…"),
            SplitAtGuides => ("Seiten an Hilfslinien aufteilen…", "Split pages at guides…"),
            CropPages => ("Seiten zuschneiden…", "Crop pages…"),
            RotatePages => ("Seiten drehen…", "Rotate pages…"),
            FitPage => ("An Seitengröße anpassen", "Fit page"),
            FitWidth => ("An Breite anpassen", "Fit width"),
            other => other.names(),
        }
    }

    /// Keyboard shortcut shown in menus.
    pub fn shortcut(self) -> Option<(&'static str, &'static str)> {
        use Cmd::*;
        Some(match self {
            New => ("Strg+N", "Ctrl+N"),
            Open => ("Strg+O", "Ctrl+O"),
            Save => ("Strg+S", "Ctrl+S"),
            SaveAs => ("Strg+Umschalt+S", "Ctrl+Shift+S"),
            Print => ("Strg+P", "Ctrl+P"),
            Close => ("Strg+W", "Ctrl+W"),
            Exit => ("Strg+Q", "Ctrl+Q"),
            DocProperties => ("Strg+D", "Ctrl+D"),
            Settings => ("Strg+K", "Ctrl+K"),
            InsertFromFile => ("Strg+Umschalt+I", "Ctrl+Shift+I"),
            DeletePages => ("Strg+Umschalt+D", "Ctrl+Shift+D"),
            CropPages => ("Strg+Umschalt+T", "Ctrl+Shift+T"),
            FitPage => ("Strg+0", "Ctrl+0"),
            ActualSize => ("Strg+1", "Ctrl+1"),
            FitWidth => ("Strg+2", "Ctrl+2"),
            FitContent => ("Strg+3", "Ctrl+3"),
            Tool(crate::ui::tools::Tool::Loupe) => ("Strg+Alt+L", "Ctrl+Alt+L"),
            Tool(t) => {
                let k = t.key()?;
                (k, k)
            }
            DarkMode => ("Strg+Umschalt+3", "Ctrl+Shift+3"),
            Panel(SidePanel::Bookmarks) => ("Strg+B", "Ctrl+B"),
            Panel(SidePanel::Thumbnails) => ("Strg+T", "Ctrl+T"),
            Panel(SidePanel::Comments) => ("Strg+M", "Ctrl+M"),
            Panel(SidePanel::Attachments) => ("Strg+Umschalt+A", "Ctrl+Shift+A"),
            View(v) => return v.shortcut(),
            Comment(c) => return c.shortcut(),
            Form(f) => return f.shortcut(),
            Content(c) => return c.shortcut(),
            _ => return None,
        })
    }
}

/// Short explanation shown under the name in a command's tooltip.
fn explain(cmd: Cmd) -> Option<(&'static str, &'static str)> {
    use crate::ui::tools::Tool as T;
    use Cmd::*;
    Some(match cmd {
        Tool(T::Hand) => (
            "Seite verschieben; Links und Formularfelder lassen sich anklicken.",
            "Move the page; links and form fields can be clicked.",
        ),
        Tool(T::SelectText) => ("Text markieren und kopieren.", "Select and copy text."),
        Tool(T::SelectAnnot) => {
            ("Anmerkungen auswählen, verschieben und ändern.", "Select, move and change comments.")
        }
        Tool(T::Zoom) => ("Klicken vergrößert, mit Umschalt verkleinert.", "Click zooms in, with Shift out."),
        Tool(T::EditText) => (
            "Den Text der Seite selbst ändern, wie in einer Textverarbeitung.",
            "Change the page's own text, like in a word processor.",
        ),
        Tool(T::EditObjects) => (
            "Text, Bilder und Grafiken der Seite auswählen, verschieben und formatieren.",
            "Select, move and format the page's text, images and graphics.",
        ),
        Tool(T::Highlight) => {
            ("Text farbig hervorheben wie mit einem Textmarker.", "Highlight text like a marker.")
        }
        Tool(T::Note) => ("Eine Notiz an eine Stelle der Seite heften.", "Pin a note to a spot on the page."),
        Tool(T::Typewriter) => ("Text direkt auf die Seite schreiben.", "Type text directly onto the page."),
        Tool(T::Stamp) => ("Einen Stempel wie „Genehmigt“ aufbringen.", "Put a stamp such as “Approved”."),
        Tool(T::Redact) => (
            "Bereiche zum Schwärzen markieren; „Übernehmen“ entfernt sie endgültig.",
            "Mark areas for redaction; “Apply” removes them for good.",
        ),
        SaveOptimized => (
            "Datei kleiner machen, z. B. Bilder verkleinern und Unnötiges entfernen.",
            "Make the file smaller, e.g. shrink images and drop unused data.",
        ),
        FitContent => {
            ("Vergrößern, bis der Inhalt die Breite füllt.", "Zoom until the content fills the width.")
        }
        SummarizeComments => (
            "Alle Anmerkungen als übersichtliche Liste in ein neues Dokument.",
            "All comments as a clear list in a new document.",
        ),
        Flatten => (
            "Anmerkungen fest in die Seite einbrennen; sie sind danach nicht mehr änderbar.",
            "Burn comments into the page; they can no longer be changed.",
        ),
        Certify | CertifyInvisible => (
            "Als erste Signatur festlegen, welche Änderungen am Dokument danach noch erlaubt sind.",
            "As the first signature, set which changes to the document are allowed afterwards.",
        ),
        TimeStampDocument => (
            "Von einem Zeitstempeldienst bestätigen lassen, dass das Dokument jetzt in diesem Zustand vorliegt.",
            "Have a time stamp authority confirm that the document exists in this state now.",
        ),
        PasswordProtect => (
            "Kennwort zum Öffnen oder Einschränkungen für Drucken und Ändern festlegen.",
            "Set a password to open, or limit printing and changes.",
        ),
        SignDocument => (
            "Das Dokument mit Ihrer digitalen ID fälschungssicher unterschreiben.",
            "Sign the document tamper-evidently with your digital ID.",
        ),
        VerifySignatures => (
            "Prüfen, ob die digitalen Signaturen gültig sind und wer unterschrieben hat.",
            "Check whether the digital signatures are valid and who signed.",
        ),
        ApplyRedactions => (
            "Markierte Stellen endgültig schwärzen – der Inhalt darunter wird gelöscht.",
            "Redact marked areas for good – the content below is deleted.",
        ),
        InsertBlank => {
            ("Leere Seiten vor oder nach einer Seite einfügen.", "Insert blank pages before or after a page.")
        }
        DeletePages => {
            ("Ausgewählte Seiten oder einen Seitenbereich löschen.", "Delete chosen pages or a page range.")
        }
        ExtractPages => ("Seiten als neue PDF-Datei speichern.", "Save pages as a new PDF file."),
        MergeFiles => ("Mehrere Dateien zu einem PDF zusammenfügen.", "Combine several files into one PDF."),
        SplitDocument => (
            "Dokument nach Seitenzahl, Lesezeichen oder Größe in mehrere Dateien teilen.",
            "Split the document into several files by pages, bookmarks or size.",
        ),
        CropPages => ("Seitenränder abschneiden.", "Cut off page margins."),
        ResizePages => ("Papierformat der Seiten ändern.", "Change the paper size of pages."),
        Watermark => {
            ("Text oder Bild schräg über die Seiten legen.", "Lay text or an image across the pages.")
        }
        HeaderFooter => (
            "Kopf- und Fußzeilen, z. B. Seitenzahlen oder Datum.",
            "Headers and footers, e.g. page numbers or date.",
        ),
        Bates => (
            "Fortlaufende Nummern auf jede Seite (für Akten).",
            "Running numbers on every page (for records).",
        ),
        NUp => ("Mehrere Seiten verkleinert auf ein Blatt.", "Several pages shrunk onto one sheet."),
        Overlay => (
            "Seiten einer anderen Datei darüber oder darunter legen.",
            "Lay pages of another file over or under.",
        ),
        ExportImages => {
            ("Bilder aus dem Dokument als Dateien speichern.", "Save the document's images as files.")
        }
        ExportWord => {
            ("Text und Absätze als Word-Dokument speichern.", "Save text and paragraphs as a Word document.")
        }
        RecognizeText => (
            "Text auf den Seiten erkennen, damit er durchsucht werden kann.",
            "Recognize text on the pages so it can be searched.",
        ),
        ReplaceFonts => {
            ("Schriftarten im Dokument gegen andere tauschen.", "Swap fonts in the document for others.")
        }
        SpellCheck => ("Rechtschreibung prüfen und Fehler zeigen.", "Check spelling and show mistakes."),
        WordCount => ("Wörter im Dokument zählen.", "Count the words in the document."),
        CompareDocuments => (
            "Zwei Fassungen vergleichen und Unterschiede markieren.",
            "Compare two versions and mark the differences.",
        ),
        AccessibilityCheck => (
            "Prüfen, ob das Dokument für Screenreader gut lesbar ist.",
            "Check whether screen readers can read the document well.",
        ),
        BookmarkToc => (
            "Inhaltsverzeichnis aus den Lesezeichen als Seite einfügen.",
            "Insert a table of contents page from the bookmarks.",
        ),
        _ => return None,
    })
}

pub fn tool_names(tool: Tool) -> (&'static str, &'static str) {
    match tool {
        Tool::Hand => ("Hand", "Hand"),
        Tool::SelectText => ("Textauswahl", "Select text"),
        Tool::SelectAnnot => ("Anmerkungen bearbeiten", "Edit comments"),
        Tool::Zoom => ("Vergrößerung", "Zoom tool"),
        Tool::Snapshot => ("Bildschirmfoto", "Snapshot"),
        Tool::Highlight => ("Text hervorheben", "Highlight text"),
        Tool::AreaHighlight => ("Bereich hervorheben", "Highlight area"),
        Tool::Underline => ("Unterstreichen", "Underline"),
        Tool::StrikeOut => ("Durchstreichen", "Strike out"),
        Tool::Squiggly => ("Wellenlinie", "Squiggly"),
        Tool::Replace => ("Text ersetzen", "Replace text"),
        Tool::Caret => ("Text einfügen", "Insert text"),
        Tool::Note => ("Kommentar", "Comment"),
        Tool::FileAttach => ("Dateianhang", "File attachment"),
        Tool::TextBox => ("Textfeld", "Text box"),
        Tool::Typewriter => ("Schreibmaschine", "Typewriter"),
        Tool::Callout => ("Beschreibung", "Callout"),
        Tool::Rectangle => ("Rechteck", "Rectangle"),
        Tool::Ellipse => ("Oval", "Oval"),
        Tool::Line => ("Linie", "Line"),
        Tool::Arrow => ("Pfeil", "Arrow"),
        Tool::Polygon => ("Vieleck", "Polygon"),
        Tool::Polyline => ("Streckenzug", "Polyline"),
        Tool::Cloud => ("Wolke", "Cloud"),
        Tool::Pencil => ("Stift", "Pencil"),
        Tool::Eraser => ("Radiergummi", "Eraser"),
        Tool::Stamp => ("Stempel", "Stamp"),
        Tool::MeasureDistance => ("Entfernung", "Distance"),
        Tool::MeasurePerimeter => ("Umfang", "Perimeter"),
        Tool::MeasureArea => ("Fläche", "Area"),
        Tool::Link => ("Verweis erstellen", "Create link"),
        Tool::Redact => ("Zum Schwärzen markieren", "Mark for redaction"),
        Tool::Signature => ("Freies Zeichen", "Draw signature"),
        Tool::SelectFields => ("Felder auswählen", "Select fields"),
        Tool::FieldText => ("Text", "Text"),
        Tool::FieldButton => ("Schaltfläche", "Button"),
        Tool::FieldCheckBox => ("Auswahlfeld (Häkchen)", "Check box"),
        Tool::FieldRadio => ("Auswahlfeld (Knopf)", "Radio button"),
        Tool::FieldList => ("Liste", "List box"),
        Tool::FieldCombo => ("Menü", "Combo box"),
        Tool::FieldSignature => ("Signatur", "Signature field"),
        Tool::FieldDate => ("Datum", "Date"),
        Tool::FieldBarcode => ("Barcode (QR-Code)", "Barcode (QR code)"),
        Tool::FieldImage => ("Bild", "Image"),
        Tool::Crop => ("Seite zuschneiden (Werkzeug)", "Crop page (tool)"),
        Tool::DynamicZoom => ("Stufenlose Vergrößerung", "Dynamic zoom"),
        Tool::Loupe => ("Lupe", "Loupe"),
        Tool::ContentHighlight => ("Inhalt hervorheben", "Highlight content"),
        Tool::TextCorrection => ("Textkorrektur", "Text correction"),
        Tool::Video => ("Video hinzufügen", "Add video"),
        Tool::AudioClip => ("Audio hinzufügen", "Add audio"),
        Tool::Model3d => ("3D hinzufügen", "Add 3D"),
        Tool::SoundNote => ("Audio", "Audio"),
        Tool::EditText => ("Text bearbeiten", "Edit text"),
        Tool::EditObjects => ("Objekte bearbeiten", "Edit objects"),
    }
}

impl VersoloPdfApp {
    fn lang(&self) -> Language {
        self.settings.language
    }

    /// Whether the command is implemented.
    pub fn cmd_implemented(&self, cmd: Cmd) -> bool {
        !super::features::PENDING.contains(&cmd)
    }

    /// Whether the command can be executed right now.
    pub fn cmd_enabled(&self, cmd: Cmd) -> bool {
        use Cmd::*;
        if !self.cmd_implemented(cmd) {
            return false;
        }
        let tab = self.active_tab();
        let has_doc = tab.is_some();
        match cmd {
            New | Open | Exit | Settings | ExportSettings | ImportSettings | ResetSettings
            | ToggleSidebar | ToggleProperties | Fullscreen | DarkMode | FromFiles | MergeFiles
            | CompareDocuments | Manual | Support | ReportProblem | OpenLog | CommandLineHelp
            | CheckUpdate | About | FileAssociation | Homepage => true,
            Undo => tab.and_then(|t| t.doc.can_undo()).is_some(),
            Redo => tab.and_then(|t| t.doc.can_redo()).is_some(),
            ViewBack => tab.is_some_and(|t| t.can_go_back()),
            ViewForward => tab.is_some_and(|t| t.can_go_forward()),
            FirstPage | PrevPage => tab.is_some_and(|t| t.current_page > 0),
            NextPage | LastPage => tab.is_some_and(|t| t.current_page + 1 < t.doc.page_count()),
            CopyText => tab.is_some_and(|t| t.selection.is_some()),
            Panel(SidePanel::Layers) => tab.is_some_and(|t| t.has_layers()),
            DeleteComment => tab.is_some_and(|t| t.selected_annot.is_some()),
            RemoveProtection => tab.is_some_and(|t| t.doc.is_encrypted() && t.doc.has_full_access()),
            PasswordProtect => tab.is_some_and(|t| t.doc.has_full_access()),
            SignDocument => tab.is_some_and(|t| !t.doc.is_encrypted()),
            Certify | CertifyInvisible | TimeStampDocument => tab.is_some_and(|t| !t.doc.is_encrypted()),
            SplitAtGuides => tab.is_some_and(|t| !t.guides.is_empty()),
            MovePageUp => tab.is_some_and(|t| t.current_page > 0),
            MovePageDown => tab.is_some_and(|t| t.current_page + 1 < t.doc.page_count()),
            DeletePages | SwapPages => tab.is_some_and(|t| t.doc.page_count() > 1),
            Email | OpenFolder | Revert | RenameFile | MoveFile | TrashFile => {
                tab.is_some_and(|t| t.doc.path().is_some())
            }
            EmailAll => self.tabs.iter().any(|t| t.doc.path().is_some()),
            LayoutCover => true,
            // "Ändern" and "Umwandeln" of the bookmark tab need bookmarks.
            BookmarkAddText
            | BookmarkCase
            | BookmarkUpper
            | BookmarkLower
            | BookmarkTitleCase
            | BookmarkZoom
            | BookmarkResolveNamed
            | BookmarkReplace
            | BookmarkRemoveActions
            | BookmarkSort
            | BookmarkCheck
            | BookmarkDedupe
            | BookmarkDeleteAll
            | BookmarkToc
            | BookmarkLinks
            | BookmarkSortPages
            | BookmarkToNamed
            | BookmarkExportHtml
            | BookmarkExportText => tab.is_some_and(|t| t.has_bookmarks()),
            View(v) => self.view_enabled(v),
            Comment(c) => self.comment_enabled(c),
            Form(f) => self.form_enabled(f),
            Content(c) => self.content_enabled(c),
            _ => has_doc,
        }
    }

    /// Whether the command is currently "on" (tools, toggles, modes).
    pub fn cmd_selected(&self, cmd: Cmd) -> bool {
        use Cmd::*;
        let tab = self.active_tab();
        match cmd {
            Tool(t) => self.tool == t,
            FitPage => tab.is_some_and(|t| t.zoom_mode == ZoomMode::FitPage),
            FitWidth => tab.is_some_and(|t| t.zoom_mode == ZoomMode::FitWidth),
            LayoutSingle => tab.is_some_and(|t| t.layout_mode == PageLayout::Single),
            LayoutContinuous => tab.is_some_and(|t| t.layout_mode == PageLayout::Continuous),
            LayoutFacing => tab.is_some_and(|t| t.layout_mode == PageLayout::TwoPages),
            LayoutCover => self.settings.cover_alone,
            View(v) => self.view_selected(v),
            Comment(c) => self.comment_selected(c),
            Form(f) => self.form_selected(f),
            Content(c) => self.content_selected(c),
            ToggleSidebar => self.settings.show_sidebar,
            ToggleProperties => self.settings.show_properties,
            DarkMode => self.settings.dark_mode,
            HighlightFields => self.settings.highlight_fields,
            ToggleAnnotations => tab.is_some_and(|t| t.show_annotations),
            Panel(p) => self.settings.show_sidebar && self.side == p,
            CommentList => self.settings.show_sidebar && self.side == SidePanel::Comments,
            _ => false,
        }
    }

    /// Tooltip: label plus the reason if the command is not offered.
    pub fn cmd_tooltip(&self, cmd: Cmd) -> String {
        let lang = self.lang();
        match cmd {
            _ if !self.cmd_implemented(cmd) => format!(
                "{}\n{}",
                cmd.label(lang),
                if lang == Language::German { "In Vorbereitung." } else { "In preparation." }
            ),
            _ => {
                let de = lang == Language::German;
                let doc = self.active_tab().map(|t| &t.doc);
                // "Rückgängig: Seiten löschen".
                let step = match cmd {
                    Cmd::Undo => doc.and_then(|d| d.can_undo()),
                    Cmd::Redo => doc.and_then(|d| d.can_redo()),
                    _ => None,
                };
                let mut tip = match step {
                    Some(step) => {
                        format!("{}: {}", cmd.label(lang), crate::ui::undo_names::undo_name(step, lang))
                    }
                    None => cmd.label(lang).to_string(),
                };
                if let Some((g, e)) = cmd.shortcut() {
                    tip = format!("{tip}  ({})", if de { g } else { e });
                }
                if let Some((g, e)) = explain(cmd) {
                    tip = format!("{tip}\n{}", if de { g } else { e });
                }
                tip
            }
        }
    }

    /// Label of the undo/redo menu entry with the step it undoes.
    pub fn undo_label(&self, redo: bool) -> Option<String> {
        let lang = self.lang();
        let doc = &self.active_tab()?.doc;
        let step = if redo { doc.can_redo()? } else { doc.can_undo()? };
        let cmd = if redo { Cmd::Redo } else { Cmd::Undo };
        Some(format!("{}: {}", cmd.label(lang), crate::ui::undo_names::undo_name(step, lang)))
    }

    /// Executes a command.
    pub fn run(&mut self, ctx: &Context, cmd: Cmd) {
        use Cmd::*;
        if !self.cmd_enabled(cmd) {
            return;
        }
        // Closes the drop-down of a collapsed ribbon group.
        crate::ui::widgets::note_command_ran(ctx);
        // A ribbon shown over the document (collapsed ribbon) closes again.
        self.ribbon_peek = false;
        // Page dialogs of "Dokumentaufbau" (insert, delete, extract, …).
        if let View(v) = cmd {
            self.run_view(ctx, v);
            return;
        }
        if let Comment(c) = cmd {
            self.run_comment(ctx, c);
            return;
        }
        if let Form(f) = cmd {
            self.run_form(ctx, f);
            return;
        }
        if let Content(c) = cmd {
            self.run_content(ctx, c);
            return;
        }
        if self.run_page_dialog(cmd)
            || self.run_layout_dialog(cmd)
            || self.run_shape(ctx, cmd)
            || self.run_marks(cmd)
            || self.run_labels(cmd)
            || self.run_file_op(ctx, cmd)
            || self.run_optimize(cmd)
            || self.run_export(cmd)
        {
            return;
        }
        match cmd {
            New => self.new_document(ctx),
            Open => self.open_dialog(),
            Save => {
                self.save(ctx, None);
            }
            SaveAs => {
                self.save_as(ctx, None);
            }
            Print => self.open_print_dialog(ctx),
            PrintAll => self.open_print_dialog_for(true),
            Close => self.request_close_tab(ctx, self.active),
            CloseAll => self.request_close_all(ctx),
            Exit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            DocProperties => self.open_properties(),
            Settings => self.open_settings(),
            Undo => self.undo(ctx),
            Redo => self.redo(ctx),
            ViewBack => {
                if let Some(tab) = self.tab() {
                    tab.go_back();
                }
            }
            ViewForward => {
                if let Some(tab) = self.tab() {
                    tab.go_forward();
                }
            }
            Tool(t) => self.select_tool(t),
            CopyText => self.copy_selection(ctx),
            SelectAll => self.select_all_text(),
            Search | Panel(SidePanel::Search) => {
                self.settings.show_sidebar = true;
                self.side = SidePanel::Search;
                self.focus_search = true;
            }
            Panel(p) => {
                if self.settings.show_sidebar && self.side == p {
                    self.settings.show_sidebar = false;
                } else {
                    self.settings.show_sidebar = true;
                    self.side = p;
                }
            }
            InsertImage => self.insert_image(ctx),
            FirstPage => self.navigate(|_, _| 0),
            PrevPage => self.navigate(|cur, _| cur.saturating_sub(1)),
            NextPage => self.navigate(|cur, n| (cur + 1).min(n.saturating_sub(1))),
            LastPage => self.navigate(|_, n| n.saturating_sub(1)),
            ActualSize => self.set_zoom(1.0),
            FitContent => self.fit_content(),
            FitPage => self.set_zoom_mode(ZoomMode::FitPage),
            FitWidth => self.set_zoom_mode(ZoomMode::FitWidth),
            ZoomIn => self.zoom_step(true),
            ZoomOut => self.zoom_step(false),
            RotateViewLeft => self.rotate_view(-90),
            RotateViewRight => self.rotate_view(90),
            LayoutSingle => self.set_layout(PageLayout::Single),
            LayoutContinuous => self.set_layout(PageLayout::Continuous),
            LayoutFacing => self.set_layout(PageLayout::TwoPages),
            LayoutCover => self.settings.cover_alone = !self.settings.cover_alone,
            View(v) => self.run_view(ctx, v),
            Comment(c) => self.run_comment(ctx, c),
            Form(f) => self.run_form(ctx, f),
            Content(c) => self.run_content(ctx, c),
            ToggleSidebar => self.settings.show_sidebar = !self.settings.show_sidebar,
            ToggleProperties => self.settings.show_properties = !self.settings.show_properties,
            Fullscreen => {
                // "Vollbild" shows the document page by page (with transitions).
                if self.presentation.is_some() {
                    self.stop_presentation(ctx);
                } else if !self.tabs.is_empty() {
                    self.start_presentation(ctx);
                } else {
                    let full = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!full));
                }
            }
            DarkMode => {
                // The button decides by itself: it ends "Dunkler Modus aus System".
                let dark = self.applied_dark.unwrap_or(self.settings.dark_mode);
                self.settings.follow_system_theme = false;
                self.settings.dark_mode = !dark;
            }
            CommentList => self.run(ctx, Panel(SidePanel::Comments)),
            ToggleAnnotations => {
                if let Some(tab) = self.tab() {
                    tab.show_annotations = !tab.show_annotations;
                    tab.textures.clear();
                }
            }
            PrevComment => self.step_comment(false),
            NextComment => self.step_comment(true),
            Flatten => {
                if let Some(tab) = self.active_tab() {
                    self.dialog = Some(Dialog::PageRange {
                        purpose: RangePurpose::Flatten,
                        text: format!("1-{}", tab.doc.page_count()),
                        error: false,
                    });
                }
            }
            DeleteComment => self.delete_selected_annotation(ctx),
            PasswordProtect => {
                self.dialog = Some(Dialog::Protect {
                    opts: crate::security::ProtectOptions::default(),
                    repeat: String::new(),
                    error: None,
                })
            }
            SignDocument => self.open_sign_dialog(None, false),
            Certify => self.open_sign_dialog(Some(2), false),
            CertifyInvisible => self.open_sign_dialog(Some(2), true),
            TimeStampDocument => self.ask_time_stamp(),
            VerifySignatures => self.show_signatures(ctx),
            RemoveProtection => {
                self.edit(ctx, |tab| tab.doc.unprotect());
                let t = self.t();
                self.set_status(ctx, t.protection_removed);
            }
            HighlightFields => self.settings.highlight_fields = !self.settings.highlight_fields,
            AppendFile => self.insert_from_file(ctx, InsertPosition::End),
            MovePageUp => self.move_current_page(ctx, true),
            MovePageDown => self.move_current_page(ctx, false),
            RotateLeft => self.rotate_current(ctx, -90),
            RotateRight => self.rotate_current(ctx, 90),
            Rotate180 => self.rotate_current(ctx, 180),
            BookmarkAdd => {
                if let Some(tab) = self.active_tab() {
                    let page = tab.current_page;
                    let t = self.t();
                    self.dialog = Some(Dialog::BookmarkTitle {
                        parent: Some(Vec::new()),
                        path: None,
                        page,
                        title: format!("{} {}", t.page, page + 1),
                    });
                }
            }
            About => self.dialog = Some(Dialog::About),
            Homepage | Support => ctx.open_url(egui::OpenUrl::new_tab(HOMEPAGE)),
            ReportProblem => ctx.open_url(egui::OpenUrl::new_tab(format!("{HOMEPAGE}/issues"))),
            OpenLog => self.open_log(),
            CheckUpdate => ctx.open_url(egui::OpenUrl::new_tab(format!("{HOMEPAGE}/releases"))),
            other => self.run_feature(ctx, other),
        }
    }

    fn navigate(&mut self, target: impl FnOnce(usize, usize) -> usize) {
        if let Some(tab) = self.tab() {
            let page = target(tab.current_page, tab.doc.page_count());
            tab.jump_to_page(page);
        }
    }

    fn rotate_view(&mut self, delta: i32) {
        if let Some(tab) = self.tab() {
            tab.view_rotation = tab.view_rotation.rotated_by(delta);
            tab.textures.clear();
        }
    }

    /// Selects the previous/next comment in document order.
    fn step_comment(&mut self, forward: bool) {
        let Some(tab) = self.tab() else { return };
        let comments = tab.all_comments();
        if comments.is_empty() {
            return;
        }
        let pos = tab
            .selected_annot
            .and_then(|s| comments.iter().position(|c| c.page == s.page && c.index == s.index));
        let next = match (pos, forward) {
            (None, true) => 0,
            (None, false) => comments.len() - 1,
            (Some(i), true) => (i + 1) % comments.len(),
            (Some(i), false) => (i + comments.len() - 1) % comments.len(),
        };
        let c = &comments[next];
        tab.selected_annot = Some(crate::ui::tab::AnnotRef { page: c.page, index: c.index });
        if let Some(geo) = tab.display_geometry(c.page) {
            let v = geo.pdf_rect_to_view(&c.rect).center();
            tab.current_page = c.page;
            tab.pending_scroll = Some(crate::ui::tab::ScrollRequest::Point(c.page, v));
        }
    }
}
