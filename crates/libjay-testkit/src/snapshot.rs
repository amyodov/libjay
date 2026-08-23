//! Snapshot files: the recordings, one per corpus file.
//!
//! `crates/libjay/tests/snapshots/<lang>/<theme>.snap` holds what the
//! reference implementations answered to every expression in
//! `corpus/<lang>/<theme>.txt`. A record is a MAP: one answer per
//! implementation, keyed by a short name (`j`, `gnu`, `dyalog`), so the same
//! file carries what several implementations made of the same sentence and a
//! reader can see where they part. Only `jay-corpus record` writes these;
//! `cargo test` reads them and runs no interpreter.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::Path;

use crate::Lang;

/// The format documentation written at the top of every snapshot file.
pub const HEADER: &str = "\
# Generated file: `cargo run -p libjay-devtools -- record` rewrites it from
# the live reference interpreters. Do not edit by hand; the workflow is in
# docs/testing.md.
#
# One record per expression, in corpus order:
#   `= EXPR`      the sentence. `\\n` is a newline in it and `\\\\` a backslash.
#   `> IMPL: TEXT`  one line of what the implementation IMPL answered.
#   `< TEXT`      one line of libjay's answer (divergence records only).
#   `? TEXT`      why libjay and the implementation it follows differ
#                 (divergence records only).
#   `@ io=N`      the index origin in force from here on (APL; 1 unless said).
#   `# TEXT`      a comment.
# The implementation keys: `j` is jconsole, `gnu` is GNU APL, `dyalog` is
# Dyalog APL. libjay is held to the one its dialect follows — `j` for J,
# `gnu` for APL; any other key gates the PRESET aimed at it, with
# tests/expected/<key>.txt naming what may differ.
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
    /// The sentence, handed to every implementation unchanged.
    pub expr: String,
    /// `⎕IO` for this record. Always 1 in a J snapshot.
    pub io: u8,
    /// Why libjay and the implementation it follows differ; divergence
    /// records only.
    pub note: Option<String>,
    /// libjay's recorded answer; divergence records only.
    pub ours: Option<Side>,
    /// What each implementation answered, keyed by [`crate::impls`].
    pub answers: BTreeMap<String, Side>,
}

impl Record {
    /// A record of one implementation's answer alone.
    pub fn new(expr: &str, io: u8, key: &str, side: Side) -> Record {
        let mut record =
            Record { expr: expr.to_string(), io, note: None, ours: None, answers: BTreeMap::new() };
        record.set(key, side);
        record
    }

    /// What one implementation answered, if this record holds it.
    pub fn answer(&self, key: &str) -> Option<&Side> {
        self.answers.get(key)
    }

    /// The answer of the implementation libjay follows, which every
    /// recorded expression carries.
    pub fn followed(&self, lang: Lang) -> &Side {
        let key = crate::followed_impl(lang);
        self.answers
            .get(key)
            .unwrap_or_else(|| panic!("{:?}: no {key} answer recorded", self.expr))
    }

    /// Record (or replace) one implementation's answer, leaving the others
    /// as they are: recording one implementation never disturbs another's
    /// recordings.
    pub fn set(&mut self, key: &str, side: Side) {
        assert!(crate::is_impl_key(key), "{key:?} is not an implementation key");
        self.answers.insert(key.to_string(), side);
    }

    /// What identifies a record: the sentence and the origin it was read
    /// under. The same sentence under `⎕IO←0` is a different case.
    pub fn key(&self) -> (String, u8) {
        (self.expr.clone(), self.io)
    }
}

const ERROR_MARK: &str = "<error>";

fn write_side(out: &mut String, tag: &str, side: &Side) {
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

/// The order a record lists its implementations in: the ones the language
/// knows, in [`crate::impls`] order, then anything else by name. A key the
/// build does not know about is kept rather than dropped.
fn impl_order(lang: Lang, record: &Record) -> Vec<&str> {
    let known = crate::impls(lang);
    let mut order: Vec<&str> =
        known.iter().copied().filter(|k| record.answers.contains_key(*k)).collect();
    for key in record.answers.keys() {
        if !known.contains(&key.as_str()) {
            order.push(key);
        }
    }
    order
}

/// Rewrite a snapshot file from records. `title` names what was recorded.
pub fn write(path: &Path, lang: Lang, title: &str, records: &[Record]) {
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
            write_side(&mut out, "<", ours);
        }
        for key in impl_order(lang, record) {
            write_side(&mut out, &format!("> {key}:"), &record.answers[key]);
        }
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).unwrap_or_else(|e| panic!("creating {}: {e}", dir.display()));
    }
    std::fs::write(path, out).unwrap_or_else(|e| panic!("writing {}: {e}", path.display()));
}

/// The implementation key and the text of a `> ` line: `gnu: 1 2 3` is
/// GNU APL answering `1 2 3`, and `gnu:` is it answering an empty line.
fn split_answer(rest: &str, line_no: usize) -> (&str, &str) {
    let (key, text) = rest
        .split_once(':')
        .unwrap_or_else(|| panic!("line {line_no}: a `> ` line needs an `IMPL: ` key"));
    assert!(crate::is_impl_key(key), "line {line_no}: {key:?} is not an implementation key");
    (key, text.strip_prefix(' ').unwrap_or(text))
}

fn extend_side(slot: &mut Option<Side>, text: &str, line_no: usize) {
    match slot {
        None if text == ERROR_MARK => *slot = Some(Side::Error),
        None => *slot = Some(Side::Out(text.to_string())),
        Some(Side::Out(s)) => {
            s.push('\n');
            s.push_str(text);
        }
        Some(Side::Error) => panic!("line {line_no}: output after {ERROR_MARK}"),
    }
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
                answers: BTreeMap::new(),
            }),
            "?" | "<" | ">" => {
                let record = records
                    .last_mut()
                    .unwrap_or_else(|| panic!("line {line_no}: {tag} before any expression"));
                match tag {
                    "?" => record.note = Some(rest.to_string()),
                    "<" => extend_side(&mut record.ours, rest, line_no),
                    _ => {
                        let (key, text) = split_answer(rest, line_no);
                        let mut slot = record.answers.remove(key);
                        extend_side(&mut slot, text, line_no);
                        record.answers.insert(key.to_string(), slot.expect("a side was written"));
                    }
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
