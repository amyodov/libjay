//! Corpus files: the inputs, as plain text.
//!
//! A corpus file is one expression per line, grouped by theme into
//! `crates/libjay/tests/corpus/<lang>/<theme>.txt`. Every other line begins
//! with a marker no sentence of either language can begin with:
//!
//! - `// TEXT` is a comment, and a blank line is ignored.
//! - `@ io=N` sets the index origin for the lines after it (APL; 1 unless
//!   said). `@` opens no sentence: it is a conjunction in J and an operator
//!   in APL, both of which need something to their left.
//! - `@ reference=NAME` names the implementation this whole theme is
//!   recorded against, when it is not the one libjay follows. A theme so
//!   marked is reference DATA: nothing holds libjay to it.
//! - `? TEXT` after an expression is a note about it, and only
//!   `divergences.txt` may carry one: elsewhere `?` is roll, so a line
//!   starting with it is an expression. The expected-different list of a
//!   dialect gate is read by [`read_annotated`], which is the same format
//!   with the note required rather than forbidden.
//! - `~ CLAUSES` after an expression and its note is a FAMILY RULE: the
//!   divergence above it stands for a whole family of sentences, and the
//!   clauses say which. Only `divergences.txt` carries one. `~` opens no
//!   J sentence (it is an adverb, and needs a verb to its left) and no APL
//!   line in the corpus begins with `~ `; an APL sentence that would has
//!   to be written parenthesised.
//!
//! The comment marker is `//`, not `#`, because `#` is J's tally: `# i. 5 2`
//! is one of the expressions below. `//` opens no sentence in either
//! language.
//!
//! Inside an expression `\n` is a newline, `\t` a tab and `\\` a
//! backslash. Any other escape is a malformed line, reported by name.

use std::path::{Path, PathBuf};

/// One input: a sentence, the index origin it is read under, the note that
/// follows it in a divergence corpus, and the family rule that widens it
/// from one sentence to a class of them.
#[derive(Clone, Debug)]
pub struct Entry {
    pub expr: String,
    pub io: u8,
    pub note: Option<String>,
    /// The clauses of a `~ ` line, unparsed: the reader knows the format of
    /// a corpus file, and the sweeper knows what a family rule means.
    pub family: Option<String>,
}

/// The corpus root, `crates/libjay/tests/corpus`.
pub fn root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../libjay/tests/corpus"))
}

/// The snapshot root, `crates/libjay/tests/snapshots`.
pub fn snapshot_root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../libjay/tests/snapshots"))
}

/// Every corpus file of one language, in name order.
pub fn files(lang: crate::Lang) -> Vec<PathBuf> {
    let dir = root().join(crate::lang_dir(lang));
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()))
        .map(|entry| entry.expect("directory entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "txt"))
        .collect();
    paths.sort();
    paths
}

/// The snapshot that records the answers to one corpus file.
pub fn snapshot_of(corpus: &Path) -> PathBuf {
    let lang = corpus.parent().and_then(|p| p.file_name()).expect("corpus/<lang>/<theme>.txt");
    let stem = corpus.file_stem().expect("a corpus file has a name");
    snapshot_root().join(lang).join(stem).with_extension("snap")
}

/// The corpus file's name as a diagnostic names it: `j/arithmetic.txt`.
pub fn label(path: &Path) -> String {
    let lang = path.parent().and_then(|p| p.file_name()).unwrap_or_default();
    format!("{}/{}", lang.to_string_lossy(), path.file_name().unwrap_or_default().to_string_lossy())
}

/// The divergence corpus is the one file whose records hold both answers,
/// and the one that carries notes.
pub fn is_divergences(path: &Path) -> bool {
    path.file_stem().is_some_and(|s| s == "divergences")
}

/// The implementation a theme is recorded against, when its file says so
/// with `@ reference=NAME`. `None` is the ordinary case: the theme is
/// recorded against the implementation libjay follows and replayed against
/// it.
///
/// A theme naming another implementation is one that implementation alone
/// can answer — a Dyalog-only feature GNU APL cannot parse, say. Its
/// records carry that key and no other, the recorder writes nothing else
/// into it, and the replay measures libjay against it without failing:
/// what differs is the backlog of a future dialect, not a regression in
/// this one.
pub fn reference(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    text.lines().find_map(|line| {
        line.trim_end().strip_prefix("@ ").and_then(|d| d.strip_prefix("reference=")).map(|name| {
            assert!(crate::is_impl_key(name), "{name:?} is not an implementation key");
            name.to_string()
        })
    })
}

/// The key a theme's replay holds libjay to, and `None` when the theme is
/// another implementation's reference data.
pub fn gate_of(lang: crate::Lang, path: &Path) -> Option<&'static str> {
    let followed = crate::followed_impl(lang);
    match reference(path) {
        Some(named) if named != followed => None,
        _ => Some(followed),
    }
}

/// Read a corpus file. Panics with the line number on a malformed one.
pub fn read(path: &Path) -> Vec<Entry> {
    read_lines(path, is_divergences(path))
}

/// Read a list in the corpus format whose entries all carry a `? ` note:
/// the expected-different list a dialect gate reads. The file holds no
/// inputs of its own — every line names an expression recorded elsewhere —
/// so the note is the point of it and nothing forbids one.
pub fn read_annotated(path: &Path) -> Vec<Entry> {
    read_lines(path, true)
}

fn read_lines(path: &Path, notes_allowed: bool) -> Vec<Entry> {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    let mut entries: Vec<Entry> = Vec::new();
    let mut io = 1u8;
    for (i, line) in text.lines().enumerate() {
        let line_no = i + 1;
        let trimmed = line.trim_end();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("@ ") {
            if rest.starts_with("reference=") {
                assert!(entries.is_empty(), "line {line_no}: `@ reference=` is a file-level directive, so it belongs before the first expression");
                continue;
            }
            io = rest
                .strip_prefix("io=")
                .and_then(|n| n.parse().ok())
                .unwrap_or_else(|| panic!("line {line_no}: unknown directive {rest:?}"));
            continue;
        }
        if let Some(note) = trimmed.strip_prefix("? ") {
            assert!(notes_allowed, "line {line_no}: a `? ` note belongs to divergences.txt");
            let entry = entries
                .last_mut()
                .unwrap_or_else(|| panic!("line {line_no}: a note before any expression"));
            entry.note = Some(note.to_string());
            continue;
        }
        if let Some(rule) = trimmed.strip_prefix("~ ") {
            assert!(notes_allowed, "line {line_no}: a `~ ` family rule belongs to divergences.txt");
            let entry = entries
                .last_mut()
                .unwrap_or_else(|| panic!("line {line_no}: a family rule before any expression"));
            assert!(
                entry.note.is_some(),
                "line {line_no}: a family rule widens a divergence, so the `? ` note saying why comes first"
            );
            assert!(
                entry.family.is_none(),
                "line {line_no}: one family rule to a divergence; put every clause on the one line"
            );
            entry.family = Some(rule.trim().to_string());
            continue;
        }
        let expr = try_unescape(trimmed)
            .unwrap_or_else(|e| panic!("{}: line {line_no}: {e}", path.display()));
        entries.push(Entry { expr, io, note: None, family: None });
    }
    entries
}

/// Append expressions to a corpus file, skipping the ones it already has.
/// Returns how many were written.
pub fn append(path: &Path, exprs: &[String]) -> usize {
    let existing: Vec<String> = if path.exists() {
        read(path).into_iter().map(|e| e.expr).collect()
    } else {
        Vec::new()
    };
    let mut text = if path.exists() {
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
    } else {
        String::new()
    };
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    let mut written = 0;
    for expr in exprs {
        if existing.contains(expr) {
            continue;
        }
        text.push_str(&escape(expr));
        text.push('\n');
        written += 1;
    }
    std::fs::write(path, text).unwrap_or_else(|e| panic!("writing {}: {e}", path.display()));
    written
}

/// A sentence as one line: the newlines in a multi-sentence program become
/// `\n`, a tab `\t`, and a literal backslash `\\`.
pub fn escape(expr: &str) -> String {
    let mut out = String::with_capacity(expr.len());
    for c in expr.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}

/// What an escape in a corpus line can be wrong in: the two ways
/// [`unescape`] can be handed something it cannot read.
///
/// The reader reports this rather than panicking, so a hand-written corpus
/// line naming an escape the format does not have is a diagnostic about
/// that line and not a crash in the middle of a recording.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EscapeError {
    /// A `\` followed by a character the format does not spell that way.
    Unknown(char),
    /// A `\` at the very end of the line, with nothing to escape.
    Dangling,
}

impl std::fmt::Display for EscapeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EscapeError::Unknown(c) => {
                write!(f, "unknown escape \\{c}: a corpus line spells only \\n, \\t and \\\\")
            }
            EscapeError::Dangling => {
                write!(f, "a line ends in a lone \\, which escapes nothing")
            }
        }
    }
}

impl std::error::Error for EscapeError {}

/// The inverse of [`escape`], with the line number in the diagnostic.
/// Panics on a malformed escape, which is a malformed corpus file.
pub fn unescape(text: &str, line_no: usize) -> String {
    try_unescape(text).unwrap_or_else(|e| panic!("line {line_no}: {e}"))
}

/// The inverse of [`escape`], reporting a malformed escape by name.
pub fn try_unescape(text: &str) -> std::result::Result<String, EscapeError> {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(other) => return Err(EscapeError::Unknown(other)),
            None => return Err(EscapeError::Dangling),
        }
    }
    Ok(out)
}
