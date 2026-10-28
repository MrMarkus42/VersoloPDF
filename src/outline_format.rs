//! Bookmark titles: case, find and replace, numbering, placeholders
//! ("Formatierung" of the bookmark dialogs).

use crate::error::{Error, Result};

/// "Groß-/Kleinschreibung".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CaseMode {
    /// "Nicht ändern".
    #[default]
    Keep,
    /// "Jedes Wort Groß".
    EachWord,
    /// "Erstes wort groß".
    FirstWord,
    /// "ALLES GROSS".
    Upper,
    /// "alles klein".
    Lower,
}

fn cap(word: &str) -> String {
    let mut c = word.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c.flat_map(char::to_lowercase)).collect(),
        None => String::new(),
    }
}

/// Changes the case of `s`; words in `exclude` (any case) stay as they are.
pub fn change_case(s: &str, mode: CaseMode, exclude: &[String]) -> String {
    if mode == CaseMode::Keep {
        return s.to_string();
    }
    let excluded = |w: &str| exclude.iter().any(|e| e.trim().to_lowercase() == w.to_lowercase());
    let mut out = String::with_capacity(s.len());
    let mut first = true;
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String, first: &mut bool| {
        if word.is_empty() {
            return;
        }
        let w = std::mem::take(word);
        let new = if excluded(&w) {
            w.clone()
        } else {
            match mode {
                CaseMode::EachWord => cap(&w),
                CaseMode::FirstWord if *first => cap(&w),
                CaseMode::FirstWord => w.to_lowercase(),
                CaseMode::Upper => w.to_uppercase(),
                CaseMode::Lower => w.to_lowercase(),
                CaseMode::Keep => w.clone(),
            }
        };
        if w.chars().any(char::is_alphanumeric) {
            *first = false;
        }
        out.push_str(&new);
    };
    for c in s.chars() {
        if c.is_alphanumeric() || c == '\'' {
            word.push(c);
        } else {
            flush(&mut word, &mut out, &mut first);
            out.push(c);
        }
    }
    flush(&mut word, &mut out, &mut first);
    out
}

/// "Suchen und ersetzen" in titles.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FindReplace {
    pub find: String,
    pub replace: String,
    pub match_case: bool,
    pub whole_word: bool,
    pub regex: bool,
}

impl FindReplace {
    /// The search as a regular expression.
    pub fn compile(&self) -> Result<regex::Regex> {
        let core = if self.regex { self.find.clone() } else { regex::escape(&self.find) };
        let core = if self.whole_word { format!(r"\b(?:{core})\b") } else { core };
        regex::RegexBuilder::new(&core)
            .case_insensitive(!self.match_case)
            .size_limit(1 << 20)
            .build()
            .map_err(|e| Error::Invalid(format!("invalid search pattern: {e}")))
    }

    /// Replaces all matches; with `regex`, `$1` … refer to groups.
    pub fn apply(&self, re: &regex::Regex, s: &str) -> String {
        if self.find.is_empty() {
            return s.to_string();
        }
        if self.regex {
            re.replace_all(s, self.replace.as_str()).into_owned()
        } else {
            re.replace_all(s, regex::NoExpand(&self.replace)).into_owned()
        }
    }
}

/// Style of numbers ("Benutze", "Stil").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NumberStyle {
    #[default]
    Arabic,
    RomanUpper,
    RomanLower,
    LetterUpper,
    LetterLower,
}

impl NumberStyle {
    pub const ALL: [NumberStyle; 5] = [
        NumberStyle::Arabic,
        NumberStyle::RomanUpper,
        NumberStyle::RomanLower,
        NumberStyle::LetterUpper,
        NumberStyle::LetterLower,
    ];

    pub fn label(self) -> &'static str {
        match self {
            NumberStyle::Arabic => "1, 2, 3 …",
            NumberStyle::RomanUpper => "I, II, III …",
            NumberStyle::RomanLower => "i, ii, iii …",
            NumberStyle::LetterUpper => "A, B, C …",
            NumberStyle::LetterLower => "a, b, c …",
        }
    }

    pub fn format(self, n: usize) -> String {
        match self {
            NumberStyle::Arabic => n.to_string(),
            NumberStyle::RomanUpper => roman(n),
            NumberStyle::RomanLower => roman(n).to_lowercase(),
            NumberStyle::LetterUpper => letters(n),
            NumberStyle::LetterLower => letters(n).to_lowercase(),
        }
    }
}

fn roman(mut n: usize) -> String {
    if n == 0 || n > 39_999 {
        return n.to_string();
    }
    let table = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut out = String::new();
    for (v, s) in table {
        while n >= v {
            out.push_str(s);
            n -= v;
        }
    }
    out
}

/// 1 = A, 26 = Z, 27 = AA.
fn letters(mut n: usize) -> String {
    let mut out = Vec::new();
    while n > 0 {
        n -= 1;
        out.push(b'A' + (n % 26) as u8);
        n /= 26;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// "Nummerieren".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Numbering {
    /// "Keine Änderung".
    #[default]
    Keep,
    /// "Hinzufügen".
    Add,
    /// "Löschen": removes a leading "1.2 " / "IV. " / "a) ".
    Remove,
}

/// Leading numbering of a title ("1.2.", "IV", "a)") followed by a space.
fn strip_numbering(s: &str) -> &str {
    let t = s.trim_start();
    let end = t.find(char::is_whitespace).unwrap_or(0);
    if end == 0 {
        return s;
    }
    let head = &t[..end];
    let core = head.trim_end_matches(['.', ')', ':']);
    let numeric = !core.is_empty() && core.chars().all(|c| c.is_ascii_digit() || c == '.');
    let roman_like =
        !core.is_empty() && core.len() <= 6 && core.chars().all(|c| "IVXLCDMivxlcdm".contains(c));
    let letter =
        core.chars().count() == 1 && core.chars().all(|c| c.is_ascii_alphabetic()) && head.len() > core.len();
    if numeric || (roman_like && head.len() > core.len()) || letter { t[end..].trim_start() } else { s }
}

/// "Formatierung" of new or changed titles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleFormat {
    pub case: CaseMode,
    /// "Folgende Wörter ausschließen" (one per line).
    pub exclude: Vec<String>,
    pub replace: Option<FindReplace>,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub numbering: Numbering,
    pub number_style: NumberStyle,
    /// "Maximale Länge" in characters.
    pub max_len: usize,
    pub double_spaces: bool,
    /// "Trennstriche entfernen": hyphens at line ends and soft hyphens.
    pub hyphens: bool,
    pub line_breaks: bool,
}

impl Default for TitleFormat {
    fn default() -> Self {
        Self {
            case: CaseMode::Keep,
            exclude: Vec::new(),
            replace: None,
            prefix: None,
            suffix: None,
            numbering: Numbering::Keep,
            number_style: NumberStyle::Arabic,
            max_len: 256,
            double_spaces: false,
            hyphens: false,
            line_breaks: false,
        }
    }
}

impl TitleFormat {
    /// The compiled search of `replace` (None if there is none).
    pub fn compile(&self) -> Result<Option<regex::Regex>> {
        match &self.replace {
            Some(r) if !r.find.is_empty() => Ok(Some(r.compile()?)),
            _ => Ok(None),
        }
    }

    /// Formats `title`; `number` is the running number for "Hinzufügen",
    /// `info` fills placeholders in prefix and suffix.
    pub fn apply(&self, title: &str, number: usize, re: Option<&regex::Regex>, info: &ItemInfo) -> String {
        let mut s = title.to_string();
        if self.hyphens {
            s = s.replace('\u{ad}', "").replace("-\r\n", "").replace("-\n", "").replace("-\r", "");
        }
        if self.line_breaks {
            s = s.replace("\r\n", " ").replace(['\n', '\r'], " ");
        }
        if self.double_spaces {
            s = collapse_spaces(&s);
        }
        s = change_case(&s, self.case, &self.exclude);
        if let (Some(fr), Some(re)) = (&self.replace, re) {
            s = fr.apply(re, &s);
        }
        match self.numbering {
            Numbering::Keep => {}
            Numbering::Remove => s = strip_numbering(&s).to_string(),
            Numbering::Add => s = format!("{}. {}", self.number_style.format(number), strip_numbering(&s)),
        }
        if let Some(p) = &self.prefix {
            s = format!("{}{s}", expand(p, info));
        }
        if let Some(p) = &self.suffix {
            s.push_str(&expand(p, info));
        }
        truncate(&s, self.max_len.max(1))
    }
}

pub fn collapse_spaces(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars() {
        if c == ' ' || c == '\t' {
            if !space {
                out.push(' ');
            }
            space = true;
        } else {
            out.push(c);
            space = false;
        }
    }
    out
}

fn truncate(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// Data of a bookmark for the placeholders.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemInfo {
    pub title: String,
    /// Titles from the top level down to this one.
    pub path_titles: Vec<String>,
    /// Indices (1-based) from the top level down to this one.
    pub path_index: Vec<usize>,
    /// Running number of the item among those processed (1-based).
    pub sequence: usize,
    /// Target page (0-based).
    pub page: Option<usize>,
    pub pages: usize,
    pub file_name: String,
    pub doc_title: String,
}

/// The placeholders offered in the menus: (placeholder, German, English).
pub const PLACEHOLDERS: [(&str, &str, &str); 13] = [
    ("%[Title]", "Lesezeichentitel", "Bookmark title"),
    ("%[HTitle]", "Hierarchischer Lesezeichentitel", "Hierarchical bookmark title"),
    ("%[HIndex]", "Hierarchischer Index", "Hierarchical index"),
    ("%[Index]", "Index des Eintrags", "Index of the entry"),
    ("%[Level]", "Lesezeichenebene", "Bookmark level"),
    ("%[Seq]", "Sequenznummer", "Sequence number"),
    ("%[Page]", "Seitenzahl", "Page number"),
    ("%[Pages]", "Anzahl der Seiten", "Number of pages"),
    ("%[DocTitle]", "Dokumenttitel", "Document title"),
    ("%[FileName]", "Dateiname", "File name"),
    ("%[Date]", "Datum", "Date"),
    ("%[Year]", "Jahr", "Year"),
    ("%[Time]", "Uhrzeit", "Time"),
];

/// Fills the placeholders of `mask`.
pub fn expand(mask: &str, info: &ItemInfo) -> String {
    if !mask.contains("%[") {
        return mask.to_string();
    }
    let now = crate::pdf_edit::pdf_date_now();
    let d: Vec<char> = now.chars().filter(|c| c.is_ascii_digit()).collect();
    let part = |a: usize, b: usize| d.get(a..b).map(|s| s.iter().collect::<String>()).unwrap_or_default();
    let (year, month, day) = (part(0, 4), part(4, 6), part(6, 8));
    let time = format!("{}:{}", part(8, 10), part(10, 12));
    let stem = info.file_name.trim_end_matches(".pdf").trim_end_matches(".PDF");
    let index = info.path_index.last().copied().unwrap_or(info.sequence);
    let hindex = info.path_index.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(".");
    mask.replace("%[Title]", &info.title)
        .replace("%[HTitle]", &info.path_titles.join(" > "))
        .replace("%[HIndex]", &hindex)
        .replace("%[Index]", &index.to_string())
        .replace("%[Level]", &info.path_index.len().max(1).to_string())
        .replace("%[Seq]", &info.sequence.to_string())
        .replace("%[Page]", &info.page.map(|p| (p + 1).to_string()).unwrap_or_default())
        .replace("%[Pages]", &info.pages.to_string())
        .replace("%[DocTitle]", &info.doc_title)
        .replace("%[FileName]", stem)
        .replace("%[Date]", &format!("{day}.{month}.{year}"))
        .replace("%[Year]", &year)
        .replace("%[Time]", &time)
}

/// "Regeln für den Textinhalt: Groß-/Kleinschreibung".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CaseRule {
    /// "Keine Einschränkungen".
    #[default]
    Any,
    /// "Alle Zeichen sind groß".
    AllUpper,
    /// "Erstes Zeichen ist ein Großbuchstabe".
    FirstUpper,
    /// "Erstes Zeichen ist eine Zahl".
    FirstDigit,
}

impl CaseRule {
    pub const ALL: [CaseRule; 4] =
        [CaseRule::Any, CaseRule::AllUpper, CaseRule::FirstUpper, CaseRule::FirstDigit];

    pub fn label(self, de: bool) -> &'static str {
        let (g, e) = match self {
            CaseRule::Any => ("Keine Einschränkungen", "No restrictions"),
            CaseRule::AllUpper => ("Alle Zeichen sind groß", "All characters are upper case"),
            CaseRule::FirstUpper => ("Erstes Zeichen ist ein Großbuchstabe", "First character is upper case"),
            CaseRule::FirstDigit => ("Erstes Zeichen ist eine Zahl", "First character is a digit"),
        };
        if de { g } else { e }
    }

    fn accepts(self, text: &str) -> bool {
        let first = text.chars().find(|c| !c.is_whitespace());
        match self {
            CaseRule::Any => true,
            CaseRule::AllUpper => {
                text.chars().any(char::is_alphabetic) && !text.chars().any(char::is_lowercase)
            }
            CaseRule::FirstUpper => first.is_some_and(char::is_uppercase),
            CaseRule::FirstDigit => first.is_some_and(|c| c.is_ascii_digit()),
        }
    }
}

/// "Einstellungen zur Texterkennung": which text lines become bookmarks.
#[derive(Debug, Clone, PartialEq)]
pub struct TextRule {
    /// Bookmark level (0 = top).
    pub level: usize,
    /// "Schriftart(en)" (None: any font). Matched by name, ignoring the
    /// subset prefix ("ABCDEF+").
    pub fonts: Option<Vec<String>>,
    /// "Schriftgröße" and "Toleranz" in points.
    pub size: Option<(f32, f32)>,
    /// "Textfarbe".
    pub color: Option<[u8; 3]>,
    pub case: CaseRule,
    /// "Mehrzeilige Lesezeichentitel zulassen".
    pub multiline: bool,
    /// "Textmuster (Regulärer Ausdruck)".
    pub pattern: Option<String>,
    pub pattern_case: bool,
    /// "Lesezeichentitel auf Textmuster einschränken".
    pub restrict_to_pattern: bool,
}

impl Default for TextRule {
    fn default() -> Self {
        Self {
            level: 0,
            fonts: None,
            size: Some((12.0, 0.2)),
            color: None,
            case: CaseRule::Any,
            multiline: false,
            pattern: None,
            pattern_case: false,
            restrict_to_pattern: false,
        }
    }
}

fn base_font(name: &str) -> &str {
    match name.split_once('+') {
        Some((prefix, rest)) if prefix.len() == 6 && prefix.chars().all(|c| c.is_ascii_uppercase()) => rest,
        _ => name,
    }
}

impl TextRule {
    pub fn compile(&self) -> Result<Option<regex::Regex>> {
        match self.pattern.as_deref().filter(|p| !p.is_empty()) {
            Some(p) => regex::RegexBuilder::new(p)
                .case_insensitive(!self.pattern_case)
                .size_limit(1 << 20)
                .build()
                .map(Some)
                .map_err(|e| Error::Invalid(format!("invalid search pattern: {e}"))),
            None => Ok(None),
        }
    }

    /// The title this rule takes from `line`, if the line matches.
    pub fn title(&self, line: &crate::text::StyledLine, re: Option<&regex::Regex>) -> Option<String> {
        if let Some(fonts) = &self.fonts
            && !fonts.iter().any(|f| base_font(f).eq_ignore_ascii_case(base_font(&line.font)))
        {
            return None;
        }
        if let Some((size, tol)) = self.size
            && (line.size - size).abs() > tol.max(0.0) + 0.001
        {
            return None;
        }
        if let Some(c) = self.color
            && c != line.color
        {
            return None;
        }
        if !self.case.accepts(&line.text) {
            return None;
        }
        match re {
            Some(re) => {
                let m = re.find(&line.text)?;
                Some(if self.restrict_to_pattern { m.as_str().to_string() } else { line.text.clone() })
            }
            None => Some(line.text.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_rules_match_lines() {
        let line = |text: &str, size: f32, font: &str| crate::text::StyledLine {
            text: text.into(),
            font: font.into(),
            bold: false,
            italic: false,
            size,
            color: [0, 0, 0],
            rect: crate::geometry::RectF::new(0.0, 0.0, 1.0, 1.0),
            first: 0,
        };
        let rule = TextRule { size: Some((18.0, 0.5)), ..Default::default() };
        assert_eq!(rule.title(&line("Kapitel 1", 18.2, "Arial"), None).as_deref(), Some("Kapitel 1"));
        assert!(rule.title(&line("Text", 12.0, "Arial"), None).is_none());
        let fonts = TextRule { size: None, fonts: Some(vec!["Arial-BoldMT".into()]), ..Default::default() };
        assert!(fonts.title(&line("A", 9.0, "ABCDEF+Arial-BoldMT"), None).is_some());
        let pat = TextRule {
            size: None,
            case: CaseRule::FirstDigit,
            pattern: Some(r"^\d+(\.\d+)*".into()),
            restrict_to_pattern: true,
            ..Default::default()
        };
        let re = pat.compile().unwrap();
        assert_eq!(pat.title(&line("2.3 Teil", 9.0, "X"), re.as_ref()).as_deref(), Some("2.3"));
        assert!(pat.title(&line("Teil 2", 9.0, "X"), re.as_ref()).is_none());
    }

    #[test]
    fn cases_with_exclusions() {
        let ex = vec!["PDF".to_string(), "und".to_string()];
        assert_eq!(
            change_case("kapitel EINS und PDF-teil", CaseMode::EachWord, &ex),
            "Kapitel Eins und PDF-Teil"
        );
        assert_eq!(change_case("KAPITEL eins Und pdf", CaseMode::FirstWord, &ex), "Kapitel eins Und pdf");
        assert_eq!(change_case("Grüße aus Köln", CaseMode::Upper, &[]), "GRÜSSE AUS KÖLN");
    }

    #[test]
    fn find_replace_modes() {
        let fr = FindReplace {
            find: "kap".into(),
            replace: "Teil".into(),
            whole_word: true,
            ..Default::default()
        };
        let re = fr.compile().unwrap();
        assert_eq!(fr.apply(&re, "Kap 1, Kapitel 2"), "Teil 1, Kapitel 2");
        let fr =
            FindReplace { find: r"(\d+)".into(), replace: "<$1>".into(), regex: true, ..Default::default() };
        assert_eq!(fr.apply(&fr.compile().unwrap(), "A 12 B 3"), "A <12> B <3>");
        let literal = FindReplace { find: "a.b".into(), replace: "$1".into(), ..Default::default() };
        assert_eq!(literal.apply(&literal.compile().unwrap(), "xa.bx aXbx"), "x$1x aXbx");
        assert!(FindReplace { find: "(".into(), regex: true, ..Default::default() }.compile().is_err());
    }

    #[test]
    fn numbers_and_numbering() {
        assert_eq!(NumberStyle::RomanUpper.format(14), "XIV");
        assert_eq!(NumberStyle::LetterLower.format(28), "ab");
        let f = TitleFormat { numbering: Numbering::Remove, ..Default::default() };
        let info = ItemInfo::default();
        assert_eq!(f.apply("2.1 Grundlagen", 1, None, &info), "Grundlagen");
        assert_eq!(f.apply("IV. Anhang", 1, None, &info), "Anhang");
        assert_eq!(f.apply("Ich bin", 1, None, &info), "Ich bin", "a word is no number");
        let f = TitleFormat { numbering: Numbering::Add, max_len: 12, ..Default::default() };
        assert_eq!(f.apply("1 Einleitung", 3, None, &info), "3. Einleitun");
    }

    #[test]
    fn placeholders() {
        let info = ItemInfo {
            title: "Teil".into(),
            path_titles: vec!["Buch".into(), "Teil".into()],
            path_index: vec![2, 3],
            sequence: 7,
            page: Some(4),
            pages: 9,
            file_name: "Bericht.pdf".into(),
            doc_title: String::new(),
        };
        assert_eq!(
            expand("%[HIndex] %[HTitle] S.%[Page]/%[Pages] %[FileName]", &info),
            "2.3 Buch > Teil S.5/9 Bericht"
        );
        assert_eq!(expand("%[Level]-%[Index]-%[Seq]", &info), "2-3-7");
    }
}
