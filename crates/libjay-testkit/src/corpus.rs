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
//! - `? TEXT` after an expression is a note about it, and only
//!   `divergences.txt` may carry one: elsewhere `?` is roll, so a line
//!   starting with it is an expression.
//!
//! The comment marker is `//`, not `#`, because `#` is J's tally: `# i. 5 2`
//! is one of the expressions below. `//` opens no sentence in either
//! language.
//!
//! Inside an expression `\n` is a newline and `\\` a backslash.

use std::path::{Path, PathBuf};

/// One input: a sentence, the index origin it is read under, and the note
/// that follows it in a divergence corpus.
#[derive(Clone, Debug)]
pub struct Entry {
    pub expr: String,
    pub io: u8,
    pub note: Option<String>,
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

/// Read a corpus file. Panics with the line number on a malformed one.
pub fn read(path: &Path) -> Vec<Entry> {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    let mut entries: Vec<Entry> = Vec::new();
    let mut io = 1u8;
    let notes_allowed = is_divergences(path);
    for (i, line) in text.lines().enumerate() {
        let line_no = i + 1;
        let trimmed = line.trim_end();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("@ ") {
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
        entries.push(Entry { expr: unescape(trimmed, line_no), io, note: None });
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
/// `\n`, and a literal backslash `\\`.
pub fn escape(expr: &str) -> String {
    let mut out = String::with_capacity(expr.len());
    for c in expr.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            _ => out.push(c),
        }
    }
    out
}

/// The inverse of [`escape`].
pub fn unescape(text: &str, line_no: usize) -> String {
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
            other => panic!("line {line_no}: bad escape \\{}", other.unwrap_or(' ')),
        }
    }
    out
}
