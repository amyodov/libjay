//! Gating a dialect that follows an implementation libjay does not ship as
//! its default.
//!
//! A snapshot record is a map, and a key libjay's default dialect is not
//! held to — `dyalog:` beside GNU APL's `gnu:` — is still a recording of a
//! real interpreter. A PRESET aimed at that implementation
//! (`Dialect::dyalog()`) can therefore be replayed against it exactly as
//! the shipped dialect is replayed against the one it follows, and this is
//! that replay: every recorded answer under the key, evaluated under the
//! preset, compared, and a difference is a failure.
//!
//! What keeps the gate honest is the expected-different list,
//! `crates/libjay/tests/expected/<key>.txt`. A row there names one
//! expression and says, in a `? ` note, why the preset does not match:
//! either a DIVERGENCE, a rule libjay keeps on purpose, or a GAP, work that
//! is queued and named. Nothing else may differ, and a row that has stopped
//! differing is a failure too — closing a gap deletes its row, and the gate
//! tightens by itself.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use jay::Dialect;

use crate::snapshot::Side;
use crate::{Lang, compare, corpus, eval, snapshot};

/// What a row of the expected-different list promises.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// libjay differs on purpose and is not going to stop: a rule of its
    /// own it keeps in every dialect, or a place the recorded answer is the
    /// reference's own quirk.
    Divergence,
    /// Work that is not done yet. The tag names the queue item in
    /// docs/status.md, and closing it deletes the row.
    Gap,
}

impl Kind {
    fn word(self) -> &'static str {
        match self {
            Kind::Divergence => "divergence",
            Kind::Gap => "gap",
        }
    }
}

/// One row: why this expression is allowed to differ.
#[derive(Clone, Debug)]
pub struct Expected {
    pub kind: Kind,
    /// The cause, shared by every row that has it: `inner-product`,
    /// `shy-result`. Gap tags are the rows of docs/status.md's table.
    pub tag: String,
    pub reason: String,
}

impl Expected {
    /// How the row reads in a message.
    pub fn describe(&self) -> String {
        format!("{} {}: {}", self.kind.word(), self.tag, self.reason)
    }
}

/// `1 row`, `2 rows`.
fn rows(n: usize) -> String {
    match n {
        1 => "1 row".to_string(),
        n => format!("{n} rows"),
    }
}

/// The expected-different list of one implementation key.
pub fn list_path(key: &str) -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../libjay/tests/expected"))
        .join(key)
        .with_extension("txt")
}

/// How the list names itself in a message.
pub fn list_label(key: &str) -> String {
    format!("tests/expected/{key}.txt")
}

/// Read the list, keyed the way a snapshot record is: sentence and index
/// origin. Panics on a malformed note, a missing one, or a duplicate row.
pub fn read_list(key: &str) -> HashMap<(String, u8), Expected> {
    let path = list_path(key);
    let mut list = HashMap::new();
    for entry in corpus::read_annotated(&path) {
        let note = entry.note.as_deref().unwrap_or_else(|| {
            panic!(
                "{}: {}\n  every row needs a `? divergence TAG: reason` or \
                 `? gap TAG: reason` note",
                list_label(key),
                entry.expr
            )
        });
        let expected = parse_note(key, &entry.expr, note);
        if list.insert((entry.expr.clone(), entry.io), expected).is_some() {
            panic!("{}: {} is listed twice", list_label(key), entry.expr);
        }
    }
    list
}

fn parse_note(key: &str, expr: &str, note: &str) -> Expected {
    let bad = |what: &str| -> ! {
        panic!(
            "{}: {expr}\n  {what}\n  a note reads `divergence TAG: reason` or \
             `gap TAG: reason`, and the reason is the point of it",
            list_label(key)
        )
    };
    let Some((head, reason)) = note.split_once(':') else { bad("no `:` in the note") };
    let Some((word, tag)) = head.split_once(' ') else { bad("no tag before the `:`") };
    let kind = match word {
        "divergence" => Kind::Divergence,
        "gap" => Kind::Gap,
        _ => bad("the note starts with `divergence` or `gap`"),
    };
    if tag.is_empty() || !tag.chars().all(|c| c.is_ascii_lowercase() || c == '-') {
        bad("a tag is lowercase words joined by `-`");
    }
    let reason = reason.trim();
    if reason.is_empty() {
        bad("the reason is missing");
    }
    Expected { kind, tag: tag.to_string(), reason: reason.to_string() }
}

/// Replay one corpus file's recorded answers under a preset aimed at the
/// implementation that gave them, failing on any difference the list does
/// not account for — and on any row of the list that has converged.
///
/// A file with no answer under the key is a no-op: the gate is per record,
/// not per theme, so a theme recorded against one implementation alone and
/// a theme recorded against both are treated the same way.
pub fn gate_file(lang: Lang, key: &str, preset: Dialect, path: &Path) {
    let label = corpus::label(path);
    let list = read_list(key);
    let mut failures = Vec::new();
    let (mut recorded, mut expected_different) = (0usize, 0usize);
    for record in snapshot::read(&corpus::snapshot_of(path)) {
        let Some(theirs) = record.answer(key) else { continue };
        recorded += 1;
        let ours = Side::of(eval::eval_as(lang, &record.expr, record.io, preset));
        match (compare::sides_match(lang, &ours, theirs), list.get(&record.key())) {
            (true, None) => {}
            (false, Some(_)) => expected_different += 1,
            (true, Some(row)) => failures.push(format!(
                "{}\n  agrees now, so its row goes from {}\n  the row: {}",
                record.expr,
                list_label(key),
                row.describe()
            )),
            (false, None) => failures.push(format!(
                "{}\n  ours:     {}\n  snapshot: {}\n  \
                 a new difference: fix it, or say why in {}",
                record.expr,
                ours.describe(),
                theirs.describe(),
                list_label(key)
            )),
        }
    }
    assert!(
        failures.is_empty(),
        "{label}: {} of {recorded} recorded {} answers are not where the {} dialect \
         says they are:\n{}",
        failures.len(),
        crate::impl_name(key),
        key,
        failures.join("\n")
    );
    if recorded > 0 {
        eprintln!(
            "{label}: the {key} dialect answers {} of {recorded} recorded {} answers\
             {}",
            recorded - expected_different,
            crate::impl_name(key),
            match expected_different {
                0 => String::new(),
                n => format!("; {n} expected different"),
            }
        );
    }
}

/// Every row of the list names an expression that has a recorded answer
/// under the key, and the list's shape is reported: how many rows are a
/// divergence and how many a gap, and the largest causes by tag.
///
/// [`gate_file`] catches a row that has converged; this catches one whose
/// expression has been edited or removed, which would otherwise sit in the
/// file for ever exempting nothing.
pub fn list_is_live(lang: Lang, key: &str) {
    let list = read_list(key);
    let mut recorded = 0usize;
    let mut keys: HashSet<(String, u8)> = HashSet::new();
    for path in corpus::files(lang) {
        for record in snapshot::read(&corpus::snapshot_of(&path)) {
            if record.answer(key).is_some() {
                recorded += 1;
                keys.insert(record.key());
            }
        }
    }
    let mut orphans: Vec<String> = list
        .iter()
        .filter(|(id, _)| !keys.contains(id))
        .map(|((expr, _), row)| format!("{expr}\n  the row: {}", row.describe()))
        .collect();
    orphans.sort();
    assert!(
        orphans.is_empty(),
        "{}: {} name an expression with no recorded {} answer:\n{}",
        list_label(key),
        rows(orphans.len()),
        crate::impl_name(key),
        orphans.join("\n")
    );
    let mut by_tag: BTreeMap<(Kind, &str), usize> = BTreeMap::new();
    for row in list.values() {
        *by_tag.entry((row.kind, row.tag.as_str())).or_default() += 1;
    }
    let count = |kind: Kind| list.values().filter(|r| r.kind == kind).count();
    eprintln!(
        "{}: {} of {} recorded {} answers — {} divergence, {} gap",
        list_label(key),
        rows(list.len()),
        recorded,
        crate::impl_name(key),
        count(Kind::Divergence),
        count(Kind::Gap)
    );
    let mut causes: Vec<((Kind, &str), usize)> = by_tag.into_iter().collect();
    causes.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    for ((kind, tag), n) in causes {
        eprintln!("  {n:>3}  {} {tag}", kind.word());
    }
}
