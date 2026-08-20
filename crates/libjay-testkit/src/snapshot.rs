//! Snapshot files: the recordings, one per corpus file.
//!
//! `crates/libjay/tests/snapshots/<lang>/<theme>.snap` holds what the
//! reference interpreter answered to every expression in
//! `corpus/<lang>/<theme>.txt`. Only `jay-corpus record` writes these;
//! `cargo test` reads them and runs no interpreter.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;

/// The format documentation written at the top of every snapshot file.
pub const HEADER: &str = "\
# Generated file: `cargo run -p libjay-devtools -- record` rewrites it from
# the live reference interpreter. Do not edit by hand; the workflow is in
# docs/testing.md.
#
# One record per expression, in corpus order:
#   `= EXPR`  the sentence. `\\n` is a newline in it and `\\\\` a backslash.
#   `> TEXT`  one line of the reference's answer.
#   `< TEXT`  one line of libjay's answer (divergence records only).
#   `? TEXT`  why the two are expected to differ (divergence records only).
#   `@ io=N`  the index origin in force from here on (APL; 1 unless said).
#   `# TEXT`  a comment.
# A side whose single line is `<error>` refused the sentence. An empty answer
# is one empty line. Trailing spaces are not recorded: the comparison is
# whitespace-insensitive within a line, but the number of lines is not.";

/// One implementation's answer to one expression.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Side {
    Out(String),
    Error,
}

impl Side {
    /// The convention the evaluators use: `None` is a refusal.
    pub fn of(answer: Option<String>) -> Side {
        match answer {
            Some(s) => Side::Out(s),
            None => Side::Error,
        }
    }

    pub fn text(&self) -> Option<&str> {
        match self {
            Side::Out(s) => Some(s),
            Side::Error => None,
        }
    }

    /// The answer as it reads in a failure message.
    pub fn describe(&self) -> String {
        match self {
            Side::Out(s) => format!("{s:?}"),
            Side::Error => "error".to_string(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Record {
    /// The sentence, handed to both implementations unchanged.
    pub expr: String,
    /// `⎕IO` for this record. Always 1 in a J snapshot.
    pub io: u8,
    /// Why the implementations differ; divergence records only.
    pub note: Option<String>,
    /// libjay's recorded answer; divergence records only.
    pub ours: Option<Side>,
    /// The reference's recorded answer.
    pub theirs: Option<Side>,
}

impl Record {
    /// A record of the reference's answer alone.
    pub fn new(expr: &str, io: u8, theirs: Side) -> Record {
        Record { expr: expr.to_string(), io, note: None, ours: None, theirs: Some(theirs) }
    }

    /// The reference's answer, which every non-divergence record carries.
    pub fn reference(&self) -> &Side {
        self.theirs.as_ref().unwrap_or_else(|| panic!("{:?}: no reference answer", self.expr))
    }

    /// What identifies a record: the sentence and the origin it was read
    /// under. The same sentence under `⎕IO←0` is a different case.
    pub fn key(&self) -> (String, u8) {
        (self.expr.clone(), self.io)
    }
}

const ERROR_MARK: &str = "<error>";

fn write_side(out: &mut String, tag: char, side: &Side) {
    let text = match side {
        Side::Out(s) => s.as_str(),
        Side::Error => ERROR_MARK,
    };
    for line in text.split('\n') {
        let line = line.trim_end();
        if line.is_empty() {
            let _ = writeln!(out, "{tag}");
        } else {
            let _ = writeln!(out, "{tag} {line}");
        }
    }
}

/// Rewrite a snapshot file from records. `title` names what was recorded.
pub fn write(path: &Path, title: &str, records: &[Record]) {
    let mut out = String::new();
    let _ = writeln!(out, "# {title}\n{HEADER}\n");
    let mut io = 1u8;
    for record in records {
        if record.io != io {
            io = record.io;
            let _ = writeln!(out, "@ io={io}");
        }
        let _ = writeln!(out, "= {}", crate::corpus::escape(&record.expr));
        if let Some(note) = &record.note {
            let _ = writeln!(out, "? {note}");
        }
        if let Some(ours) = &record.ours {
            write_side(&mut out, '<', ours);
        }
        if let Some(theirs) = &record.theirs {
            write_side(&mut out, '>', theirs);
        }
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).unwrap_or_else(|e| panic!("creating {}: {e}", dir.display()));
    }
    std::fs::write(path, out).unwrap_or_else(|e| panic!("writing {}: {e}", path.display()));
}

/// Read a snapshot file. Panics with the line number on a malformed one.
/// A snapshot that is not there yet reads as no records.
pub fn read(path: &Path) -> Vec<Record> {
    if !path.exists() {
        return Vec::new();
    }
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    let mut records: Vec<Record> = Vec::new();
    let mut io = 1u8;
    for (i, line) in text.lines().enumerate() {
        let line_no = i + 1;
        let (tag, rest) = match line.split_at_checked(1) {
            None => continue, // blank separator
            Some(pair) => pair,
        };
        let rest = rest.strip_prefix(' ').unwrap_or(rest);
        match tag {
            "#" => {}
            "@" => {
                io = rest
                    .strip_prefix("io=")
                    .and_then(|n| n.parse().ok())
                    .unwrap_or_else(|| panic!("line {line_no}: unknown setting {rest:?}"));
            }
            "=" => records.push(Record {
                expr: crate::corpus::unescape(rest, line_no),
                io,
                note: None,
                ours: None,
                theirs: None,
            }),
            "?" | "<" | ">" => {
                let record = records
                    .last_mut()
                    .unwrap_or_else(|| panic!("line {line_no}: {tag} before any expression"));
                if tag == "?" {
                    record.note = Some(rest.to_string());
                    continue;
                }
                let side = if tag == "<" { &mut record.ours } else { &mut record.theirs };
                match side {
                    None if rest == ERROR_MARK => *side = Some(Side::Error),
                    None => *side = Some(Side::Out(rest.to_string())),
                    Some(Side::Out(s)) => {
                        s.push('\n');
                        s.push_str(rest);
                    }
                    Some(Side::Error) => panic!("line {line_no}: output after {ERROR_MARK}"),
                }
            }
            _ => panic!("line {line_no}: unknown tag {tag:?}"),
        }
    }
    records
}

/// A snapshot indexed by sentence, which is how a replay looks a record up:
/// adding a line in the middle of a corpus file must not shift the rest.
pub fn index(records: Vec<Record>) -> HashMap<(String, u8), Record> {
    records.into_iter().map(|r| (r.key(), r)).collect()
}
