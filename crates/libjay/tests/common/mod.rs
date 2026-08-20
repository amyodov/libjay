//! The snapshot file format shared by the two differential suites.
//!
//! A snapshot is a plain-text list of records: an expression and the answer
//! a reference interpreter gave for it. `cargo test` reads these files and
//! never runs an interpreter; the refresh step (docs/testing.md) is the only
//! thing that writes them.
//!
//! Each test binary uses part of this module, so unused items here are
//! expected.
#![allow(dead_code)]

use std::fmt::Write as _;

/// The format documentation written at the top of every snapshot file.
pub const HEADER: &str = "\
# Generated file: `LIBJAY_REFRESH_ORACLE=write cargo test -p libjay` rewrites
# it from the live reference interpreter. Do not edit by hand; the workflow is
# in docs/testing.md.
#
# One record per expression, in generator order:
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
    /// The convention both suites use in memory: `None` is a refusal.
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
    /// `⎕IO` for this record. Always 1 in the J snapshot.
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
}

/// A run of records under one heading, kept only so the file reads well.
pub struct Section {
    pub title: String,
    pub records: Vec<Record>,
}

pub fn section(title: &str, records: Vec<Record>) -> Section {
    Section { title: title.to_string(), records }
}

const ERROR_MARK: &str = "<error>";

fn escape(expr: &str) -> String {
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

fn unescape(text: &str, line_no: usize) -> String {
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

/// Rewrite a snapshot file from records. `title` names the reference.
pub fn write(path: &str, title: &str, sections: &[Section]) {
    let mut out = String::new();
    let _ = writeln!(out, "# {title}\n{HEADER}");
    let mut io = 1u8;
    for section in sections {
        let _ = writeln!(out, "\n# --- {} ---", section.title);
        for record in &section.records {
            if record.io != io {
                io = record.io;
                let _ = writeln!(out, "@ io={io}");
            }
            let _ = writeln!(out, "= {}", escape(&record.expr));
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
    }
    std::fs::write(path, out).unwrap_or_else(|e| panic!("writing {path}: {e}"));
}

/// Every way a freshly measured list differs from a recorded one, as lines
/// for a failure message. `same` is the caller's tolerance-aware comparison.
pub fn drift(
    recorded: &[Record],
    fresh: &[Record],
    same: &dyn Fn(&Side, &Side) -> bool,
) -> Vec<String> {
    let mut out = Vec::new();
    for (i, new) in fresh.iter().enumerate() {
        let Some(old) = recorded.get(i) else {
            out.push(format!("record {i}: {:?} is missing from the snapshot", new.expr));
            continue;
        };
        if old.expr != new.expr {
            out.push(format!(
                "record {i}: expected {:?}, the snapshot has {:?}",
                new.expr, old.expr
            ));
            continue;
        }
        for (label, new_side, old_side) in [
            ("libjay", &new.ours, &old.ours),
            ("reference", &new.theirs, &old.theirs),
        ] {
            match (new_side, old_side) {
                (Some(n), Some(o)) if !same(n, o) => out.push(format!(
                    "{}\n  {label} now: {}\n  snapshot:  {}",
                    new.expr,
                    n.describe(),
                    o.describe()
                )),
                (Some(_), None) | (None, Some(_)) => {
                    out.push(format!("{}: the record no longer holds {label}'s answer", new.expr))
                }
                _ => {}
            }
        }
    }
    if recorded.len() > fresh.len() {
        out.push(format!(
            "the snapshot has {} records the expression list no longer has",
            recorded.len() - fresh.len()
        ));
    }
    out
}

/// Read a snapshot file. Panics with the line number on a malformed one.
pub fn read(path: &str) -> Vec<Record> {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("reading {path}: {e} (regenerate it: see docs/testing.md)"));
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
                expr: unescape(rest, line_no),
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
