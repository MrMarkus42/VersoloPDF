//! "Rechtschreibprüfung" with Hunspell dictionaries (.aff/.dic) found on
//! the computer (LibreOffice, Linux packages) or chosen by the user; VersoloPDF
//! ships no dictionary. Supported: word lists with prefix and suffix rules
//! (also combined), forbidden words and, for compound languages such as
//! German, words made of dictionary words with linking "s", "es", "n",
//! "en" (simplified: the compound flags of the dictionary are not used).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// Largest dictionary file that is read.
const MAX_DIC: u64 = 64 * 1024 * 1024;

type Flag = u32;

#[derive(Debug, Clone)]
enum Cond {
    Any,
    Char(char),
    Set(Vec<char>, bool),
}

#[derive(Debug, Clone)]
struct Affix {
    flag: Flag,
    cross: bool,
    strip: String,
    add: String,
    cond: Vec<Cond>,
    /// Continuation flags ("en/xo").
    cont: Vec<Flag>,
}

/// A loaded Hunspell dictionary.
#[derive(Debug, Default)]
pub struct Dictionary {
    words: HashMap<String, Vec<Vec<Flag>>>,
    /// Lower-case word -> forms in the dictionary (for compound parts).
    lower: HashMap<String, Vec<String>>,
    prefixes: Vec<Affix>,
    suffixes: Vec<Affix>,
    forbidden: Option<Flag>,
    only_in_compound: Option<Flag>,
    need_affix: Option<Flag>,
    compound_min: usize,
    compounds: bool,
    /// Flags that let a word start a compound: the compound flags and the
    /// affix flags whose rules lead to them. Empty: every word may.
    joiners: HashSet<Flag>,
    word_chars: HashSet<char>,
    try_chars: Vec<char>,
    /// Words the user added ("Zum Wörterbuch hinzufügen").
    personal: HashSet<String>,
}

#[derive(Clone, Copy)]
enum FlagMode {
    Char,
    Long,
    Num,
}

fn parse_flags(s: &str, mode: FlagMode) -> Vec<Flag> {
    match mode {
        FlagMode::Char => s.chars().map(|c| c as u32).collect(),
        FlagMode::Long => s
            .chars()
            .collect::<Vec<_>>()
            .chunks(2)
            .map(|p| (p[0] as u32) << 16 | p.get(1).map_or(0, |c| *c as u32))
            .collect(),
        FlagMode::Num => s.split(',').filter_map(|n| n.trim().parse().ok()).collect(),
    }
}

fn parse_cond(s: &str) -> Vec<Cond> {
    if s == "." {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '.' => out.push(Cond::Any),
            '[' => {
                let negate = chars.peek() == Some(&'^');
                if negate {
                    chars.next();
                }
                let mut set = Vec::new();
                for d in chars.by_ref() {
                    if d == ']' {
                        break;
                    }
                    set.push(d);
                }
                out.push(Cond::Set(set, negate));
            }
            c => out.push(Cond::Char(c)),
        }
    }
    out
}

fn cond_ok(cond: &[Cond], chars: &[char]) -> bool {
    cond.len() <= chars.len()
        && cond.iter().zip(chars).all(|(c, ch)| match c {
            Cond::Any => true,
            Cond::Char(x) => x == ch,
            Cond::Set(set, negate) => set.contains(ch) != *negate,
        })
}

/// Decodes a dictionary file (UTF-8 or ISO 8859-1).
fn decode(bytes: &[u8]) -> String {
    match String::from_utf8(bytes.to_vec()) {
        Ok(s) => s,
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    }
}

impl Dictionary {
    /// Loads `name.aff` and `name.dic` (`dic` is the path of the .dic file).
    pub fn load(dic: &Path) -> Result<Self> {
        let aff = dic.with_extension("aff");
        let read = |p: &Path| -> Result<String> {
            let len = std::fs::metadata(p).map_err(|e| Error::io(p, e))?.len();
            if len > MAX_DIC {
                return Err(Error::FileTooLarge { size: len, limit: MAX_DIC });
            }
            Ok(decode(&std::fs::read(p).map_err(|e| Error::io(p, e))?))
        };
        Ok(Self::parse(&read(&aff)?, &read(dic)?))
    }

    pub fn parse(aff: &str, dic: &str) -> Self {
        let mut d = Dictionary { compound_min: 3, ..Dictionary::default() };
        let mut mode = FlagMode::Char;
        // The flag mode comes first; read it before the rest.
        for line in aff.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.first() == Some(&"FLAG") {
                mode = match parts.get(1).copied() {
                    Some("long") => FlagMode::Long,
                    Some("num") => FlagMode::Num,
                    _ => FlagMode::Char,
                };
            }
        }
        let one = |s: Option<&&str>| s.and_then(|s| parse_flags(s, mode).first().copied());
        for line in aff.lines() {
            let line = line.split('#').next().unwrap_or_default();
            let parts: Vec<&str> = line.split_whitespace().collect();
            match parts.as_slice() {
                ["FORBIDDENWORD", f, ..] => d.forbidden = one(Some(f)),
                ["ONLYINCOMPOUND", f, ..] => d.only_in_compound = one(Some(f)),
                ["NEEDAFFIX", f, ..] | ["PSEUDOROOT", f, ..] => d.need_affix = one(Some(f)),
                ["COMPOUNDMIN", n, ..] => d.compound_min = n.parse().unwrap_or(3).max(1),
                ["COMPOUNDFLAG", f, ..] | ["COMPOUNDBEGIN", f, ..] | ["COMPOUNDMIDDLE", f, ..] => {
                    d.compounds = true;
                    d.joiners.extend(one(Some(f)));
                }
                ["COMPOUNDRULE", ..] => d.compounds = true,
                ["WORDCHARS", chars, ..] => d.word_chars = chars.chars().collect(),
                ["TRY", chars, ..] => d.try_chars = chars.chars().collect(),
                [kind @ ("PFX" | "SFX"), flag, strip, add, rest @ ..]
                    if !rest.is_empty() || strip.parse::<u32>().is_err() =>
                {
                    // Header lines are "PFX A Y 3"; rules have a condition.
                    if rest.is_empty() {
                        continue;
                    }
                    let Some(flag) = one(Some(flag)) else { continue };
                    let cross = aff
                        .lines()
                        .find(|l| {
                            let p: Vec<&str> = l.split_whitespace().collect();
                            p.len() == 4
                                && p[0] == *kind
                                && one(p.get(1)) == Some(flag)
                                && p[3].parse::<u32>().is_ok()
                        })
                        .is_some_and(|l| l.split_whitespace().nth(2) == Some("Y"));
                    let strip = if *strip == "0" { String::new() } else { (*strip).to_string() };
                    let (add, cont) = add.split_once('/').unwrap_or((add, ""));
                    let add = if add == "0" { String::new() } else { add.to_string() };
                    let cont = parse_flags(cont, mode);
                    let affix = Affix { flag, cross, strip, add, cond: parse_cond(rest[0]), cont };
                    if *kind == "PFX" {
                        d.prefixes.push(affix);
                    } else {
                        d.suffixes.push(affix);
                    }
                }
                _ => {}
            }
        }
        // Affixes that lead to a compound flag also mark compound starts.
        let leading: Vec<Flag> = d
            .prefixes
            .iter()
            .chain(&d.suffixes)
            .filter(|a| a.cont.iter().any(|c| d.joiners.contains(c)))
            .map(|a| a.flag)
            .collect();
        d.joiners.extend(leading);
        for line in dic.lines().skip(1) {
            if line.starts_with('\t') || line.trim().is_empty() {
                continue;
            }
            let entry = line.split(['\t', ' ']).next().unwrap_or_default();
            let (word, flags) = match entry.find('/') {
                Some(i) if !entry[..i].ends_with('\\') => (&entry[..i], &entry[i + 1..]),
                _ => (entry, ""),
            };
            let word = word.replace("\\/", "/");
            if word.is_empty() {
                continue;
            }
            d.lower.entry(word.to_lowercase()).or_default().push(word.clone());
            d.words.entry(word).or_default().push(parse_flags(flags, mode));
        }
        d
    }

    /// Number of dictionary entries.
    pub fn len(&self) -> usize {
        self.words.len()
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    pub fn add_personal(&mut self, word: &str) {
        self.personal.insert(word.to_string());
    }

    fn has(&self, flags: &[Flag], f: Option<Flag>) -> bool {
        f.is_some_and(|f| flags.contains(&f))
    }

    /// Entries of `word` that allow `need` (an affix flag, or None for the
    /// bare word).
    fn entry_ok(&self, word: &str, need: Option<Flag>, also: Option<Flag>, in_compound: bool) -> bool {
        self.words.get(word).is_some_and(|homonyms| {
            homonyms.iter().any(|flags| {
                !self.has(flags, self.forbidden)
                    && (in_compound || !self.has(flags, self.only_in_compound))
                    && match need {
                        None => !self.has(flags, self.need_affix),
                        Some(f) => flags.contains(&f),
                    }
                    && also.is_none_or(|f| flags.contains(&f))
            })
        })
    }

    fn forbidden(&self, word: &str) -> bool {
        self.words.get(word).is_some_and(|h| h.iter().any(|f| self.has(f, self.forbidden)))
    }

    /// The word itself or with one prefix and/or one suffix.
    fn lookup(&self, word: &str, in_compound: bool) -> bool {
        if self.forbidden(word) {
            return false;
        }
        if self.entry_ok(word, None, None, in_compound) {
            return true;
        }
        let chars: Vec<char> = word.chars().collect();
        // Suffixes (optionally after a prefix).
        let suffix_base = |w: &str, prefix: Option<Flag>| -> bool {
            let wc: Vec<char> = w.chars().collect();
            self.suffixes.iter().any(|s| {
                if (prefix.is_some() && !s.cross) || !w.ends_with(&s.add) {
                    return false;
                }
                let keep = wc.len() - s.add.chars().count();
                if keep == 0 {
                    return false;
                }
                let base: String = wc[..keep].iter().collect::<String>() + &s.strip;
                let bc: Vec<char> = base.chars().collect();
                let tail: Vec<char> = bc[bc.len().saturating_sub(s.cond.len())..].to_vec();
                cond_ok(&s.cond, &tail) && self.entry_ok(&base, Some(s.flag), prefix, in_compound)
            })
        };
        if suffix_base(word, None) {
            return true;
        }
        self.prefixes.iter().any(|p| {
            if !word.starts_with(&p.add) {
                return false;
            }
            let rest = &chars[p.add.chars().count()..];
            if rest.is_empty() {
                return false;
            }
            let base: String = p.strip.clone() + &rest.iter().collect::<String>();
            let bc: Vec<char> = base.chars().collect();
            (cond_ok(&p.cond, &bc) && self.entry_ok(&base, Some(p.flag), None, in_compound))
                || (p.cross && suffix_base(&base, Some(p.flag)))
        })
    }

    /// A part of a compound: as written, capitalised or in lower case. In
    /// words written in lower case (adjectives, verbs: "hellblau") the
    /// parts must be lower case words too, so "gelessen" is no "Gel" +
    /// "essen".
    fn part(&self, part: &str, nouns: bool) -> bool {
        if part.chars().count() < self.compound_min.max(2) {
            return false;
        }
        let mut forms = vec![part.to_string()];
        if nouns {
            let mut c = part.chars();
            if let Some(first) = c.next() {
                forms.push(first.to_uppercase().collect::<String>() + c.as_str());
            }
        }
        forms.push(part.to_lowercase());
        forms.iter().any(|f| self.lookup(f, true))
    }

    /// Whether the dictionary lets `head` (any case) start a compound.
    fn joins(&self, head: &str) -> bool {
        if self.joiners.is_empty() {
            return true;
        }
        self.lower.get(&head.to_lowercase()).is_some_and(|forms| {
            forms.iter().any(|f| {
                self.words
                    .get(f)
                    .is_some_and(|h| h.iter().any(|flags| flags.iter().any(|x| self.joiners.contains(x))))
            })
        })
    }

    fn compound(&self, word: &str, depth: usize) -> bool {
        let nouns = word.chars().next().is_some_and(char::is_uppercase);
        self.compound_with(word, depth, nouns)
    }

    fn compound_with(&self, word: &str, depth: usize, nouns: bool) -> bool {
        if depth > 3 {
            return false;
        }
        let idx: Vec<usize> = word.char_indices().map(|(i, _)| i).chain([word.len()]).collect();
        let n = idx.len() - 1;
        let min = self.compound_min.max(2);
        for &cut in idx.iter().take(n.saturating_sub(min) + 1).skip(min) {
            let (first, rest) = word.split_at(cut);
            for link in ["", "s", "es", "n", "en", "-"] {
                let Some(head) = first.strip_suffix(link) else { continue };
                if head.is_empty() || !self.part(head, nouns) || !self.joins(head) {
                    continue;
                }
                if self.part(rest, nouns) || self.compound_with(rest, depth + 1, nouns) {
                    return true;
                }
            }
        }
        false
    }

    /// Whether `word` is spelled correctly.
    pub fn check(&self, word: &str) -> bool {
        let word = word.trim_matches(|c: char| !c.is_alphanumeric() && !self.word_chars.contains(&c));
        let word = word.trim_end_matches(['.', '-']);
        if word.chars().count() < 2
            || word.chars().any(|c| c.is_ascii_digit())
            || self.personal.contains(word)
            || word.contains('@')
            || word.contains("://")
        {
            return true;
        }
        let mut forms = vec![word.to_string()];
        let lower = word.to_lowercase();
        let mut c = lower.chars();
        let capital = c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default();
        if word.chars().skip(1).all(|c| !c.is_uppercase()) {
            forms.push(lower.clone());
        } else if word.chars().all(|c| !c.is_lowercase()) {
            forms.push(capital.clone());
            forms.push(lower.clone());
        }
        if forms.iter().any(|f| self.lookup(f, false)) {
            return true;
        }
        // Hyphenated words: every part must be right.
        if word.contains('-') {
            return word.split('-').filter(|p| !p.is_empty()).all(|p| self.check(p));
        }
        self.compounds && forms.iter().any(|f| self.compound(f, 0))
    }

    /// Up to `max` corrections: words one edit away (letters from TRY).
    pub fn suggest(&self, word: &str, max: usize) -> Vec<String> {
        let chars: Vec<char> = word.chars().collect();
        let letters: Vec<char> = if self.try_chars.is_empty() {
            "esianrtolcdugmphbyfvkwzxqjäöüß".chars().collect()
        } else {
            self.try_chars.clone()
        };
        let mut out: Vec<String> = Vec::new();
        let push = |w: String, out: &mut Vec<String>| {
            if out.len() < max && !out.contains(&w) && w != word && self.check(&w) {
                out.push(w);
            }
        };
        for i in 0..chars.len() {
            // Swapped neighbours, missing and wrong letters first.
            if i + 1 < chars.len() {
                let mut c = chars.clone();
                c.swap(i, i + 1);
                push(c.into_iter().collect(), &mut out);
            }
            let mut c = chars.clone();
            c.remove(i);
            push(c.into_iter().collect(), &mut out);
        }
        for i in 0..chars.len() {
            for &l in &letters {
                let mut c = chars.clone();
                c[i] = l;
                push(c.into_iter().collect(), &mut out);
            }
        }
        for i in 0..=chars.len() {
            for &l in &letters {
                let mut c = chars.clone();
                c.insert(i, l);
                push(c.into_iter().collect(), &mut out);
            }
            if out.len() >= max {
                break;
            }
        }
        // Capitalised form of the word (German nouns).
        if let Some(w) = self.lower.get(&word.to_lowercase()).and_then(|v| v.first())
            && w != word
            && !out.contains(w)
        {
            out.insert(0, w.clone());
        }
        out.truncate(max);
        out
    }
}

/// Folders where Hunspell dictionaries are usually installed.
pub fn dictionary_folders() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = vec![
        PathBuf::from("/usr/share/hunspell"),
        PathBuf::from("/usr/share/myspell"),
        PathBuf::from("/usr/share/myspell/dicts"),
        PathBuf::from("/Library/Spelling"),
    ];
    for ext in [
        r"C:\Program Files\LibreOffice\share\extensions",
        r"C:\Program Files (x86)\LibreOffice\share\extensions",
        r"C:\Program Files\OpenOffice 4\share\extensions",
        "/Applications/LibreOffice.app/Contents/Resources/extensions",
        "/usr/lib/libreoffice/share/extensions",
    ] {
        if let Ok(dirs) = std::fs::read_dir(ext) {
            out.extend(dirs.flatten().map(|d| d.path()).filter(|p| p.is_dir()));
        }
    }
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        out.push(Path::new(&home).join("Library").join("Spelling"));
    }
    out
}

/// Installed dictionaries: (language code like "de_DE", path of the .dic).
pub fn find_dictionaries() -> Vec<(String, PathBuf)> {
    let mut out: Vec<(String, PathBuf)> = Vec::new();
    for dir in dictionary_folders() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("dic"))
                && p.with_extension("aff").is_file()
            {
                let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                if !out.iter().any(|(n, _)| *n == name) {
                    out.push((name, p));
                }
            }
        }
    }
    out.sort();
    out
}

/// The words of a text with their character positions (start, end exclusive).
pub fn words(text: &str) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let chars: Vec<char> = text.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        let part = c.is_alphanumeric() || (*c == '-' || *c == '\'') && start.is_some();
        match (part, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                out.push((s, i, chars[s..i].iter().collect()));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push((s, chars.len(), chars[s..].iter().collect()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const AFF: &str = "SET UTF-8\nTRY esianrt\nFORBIDDENWORD d\nONLYINCOMPOUND o\nCOMPOUNDBEGIN x\nCOMPOUNDMIN 2\n\
        PFX U Y 1\nPFX U 0 un .\n\
        SFX N Y 2\nSFX N 0 en [^e]\nSFX N 0 n e\n\
        SFX S Y 1\nSFX S 0 es .\n\
        SFX j Y 1\nSFX j 0 0/xo .\n";
    const DIC: &str = "7\nHaus/Sj\nTür/N\nklar/U\nRose/Nj\nWeg/j\nfalsch/d\ngel\n";

    #[test]
    fn words_affixes_and_compounds() {
        let d = Dictionary::parse(AFF, DIC);
        assert_eq!(d.len(), 7);
        assert!(!d.check("Geltür"), "gel may not start a compound");
        for ok in [
            "Haus",
            "Hauses",
            "Türen",
            "Rosen",
            "klar",
            "unklar",
            "Haustür",
            "Haustüren",
            "Rosenweg",
            "HAUS",
            "Hausweg-Türen",
        ] {
            assert!(d.check(ok), "{ok} should be right");
        }
        for wrong in ["Hause", "Tühr", "falsch", "unweg", "Haustühr", "Hasu"] {
            assert!(!d.check(wrong), "{wrong} should be wrong");
        }
        assert!(
            d.check("2026") && d.check("a") && d.check("www://x"),
            "numbers, letters, addresses are skipped"
        );
        assert_eq!(d.suggest("Hasu", 3).first().map(String::as_str), Some("Haus"));
        assert!(d.suggest("Tühr", 5).contains(&"Tür".to_string()));
        let mut d = d;
        d.add_personal("VersoloPDF");
        assert!(d.check("VersoloPDF"));
        assert_eq!(
            words("Ein Haus-Tür, 12 Wege!"),
            vec![
                (0, 3, "Ein".into()),
                (4, 12, "Haus-Tür".into()),
                (14, 16, "12".into()),
                (17, 21, "Wege".into()),
            ]
        );
    }
}
