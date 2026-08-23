//! Replaying one recorded corpus file against libjay.
//!
//! This is the testing half: libjay evaluates every expression of a corpus
//! file and the answer is compared with what the implementation its dialect
//! FOLLOWS gave when the file was last recorded. No subprocess, no external
//! binary, nothing outside the repository. Collecting the recordings is the
//! other half, and lives in `jay-corpus` (docs/testing.md).
//!
//! A snapshot may hold answers from other implementations too. This replay
//! only counts those — a recorded Dyalog answer that differs from the
//! shipped dialect is the backlog a Dyalog dialect has to explain, not a
//! regression in this one. The preset aimed at that implementation is
//! gated separately, in [`crate::dialect`].

use std::path::Path;

use crate::snapshot::{Record, Side};
use crate::{Lang, compare, corpus, eval, snapshot};

/// How far libjay is from an implementation it does NOT follow: the count
/// of expressions with a recorded answer under that key, and how many of
/// them differ.
#[derive(Clone, Copy, Debug, Default)]
pub struct Backlog {
    pub recorded: usize,
    pub differ: usize,
}

impl Backlog {
    pub fn add(&mut self, other: Backlog) {
        self.recorded += other.recorded;
        self.differ += other.differ;
    }
}

/// Replay a corpus file, panicking with every mismatch at once. The panic
/// names the theme file, so a failure says which corpus to look at.
pub fn corpus_file(lang: Lang, path: &Path) {
    let label = corpus::label(path);
    let entries = corpus::read(path);
    assert!(!entries.is_empty(), "{label} has no expressions");
    let recorded = snapshot::index(snapshot::read(&corpus::snapshot_of(path)));
    let divergences = corpus::is_divergences(path);
    let gate = corpus::gate_of(lang, path);
    let backlog_key = crate::backlog_impl(lang);
    let mut backlog = Backlog::default();
    let mut failures = Vec::new();
    for entry in &entries {
        let Some(record) = recorded.get(&(entry.expr.clone(), entry.io)) else {
            failures.push(format!("{}\n  unrecorded: run jay-corpus record", entry.expr));
            continue;
        };
        let ours = Side::of(eval::eval(lang, &entry.expr, entry.io));
        if let Some(key) = backlog_key && let Some(theirs) = record.answer(key) {
            backlog.recorded += 1;
            if !compare::sides_match(lang, &ours, theirs) {
                backlog.differ += 1;
            }
        }
        // A theme recorded against an implementation libjay does not follow
        // holds the SHIPPED dialect to nothing: the loop above has already
        // counted it, there is no side to compare this dialect with, and
        // the preset aimed at that implementation has its own gate.
        if gate.is_none() {
            continue;
        }
        failures.extend(if divergences {
            check_divergence(lang, record, &ours)
        } else {
            check(lang, record, &ours)
        });
    }
    assert!(
        failures.is_empty(),
        "{label}: {} of {} expressions differ from what is recorded:\n{}",
        failures.len(),
        entries.len(),
        failures.join("\n")
    );
    let held = match gate {
        Some(_) => "agreement on",
        None => "reference data:",
    };
    eprintln!("{label}: {held} {} expressions{}", entries.len(), backlog_line(lang, backlog));
}

/// The summary line the battery prints for an implementation libjay does
/// not follow. Empty when nothing of the kind is recorded yet, so the line
/// says nothing until a recording exists.
pub fn backlog_line(lang: Lang, backlog: Backlog) -> String {
    match crate::backlog_impl(lang) {
        Some(key) if backlog.recorded > 0 => format!(
            "; {} of {} recorded {} answers differ (dialect backlog, not a failure)",
            backlog.differ,
            backlog.recorded,
            crate::impl_name(key)
        ),
        _ => String::new(),
    }
}

/// Every corpus file of a language has a snapshot, and every expression in
/// it a record under the key libjay is held to.
///
/// The replay cases are generated from a glob expanded when the test binary
/// is compiled, so a corpus file added since the last build is not a case of
/// its own until the next one. This runs at execution time and sees the
/// directory as it is now, which is what catches a file that was added and
/// never recorded. (CI compiles from scratch, so its glob is always
/// current.)
pub fn every_file_is_recorded(lang: Lang) {
    let mut failures = Vec::new();
    let mut backlog = Backlog::default();
    for path in corpus::files(lang) {
        let label = corpus::label(&path);
        // A theme marked `@ reference=NAME` is held to THAT key: it is the
        // only implementation that can answer it, and the only one recorded.
        let key = corpus::reference(&path).unwrap_or_else(|| crate::followed_impl(lang).to_string());
        let key = key.as_str();
        let recorded = snapshot::index(snapshot::read(&corpus::snapshot_of(&path)));
        for entry in corpus::read(&path) {
            match recorded.get(&(entry.expr.clone(), entry.io)) {
                Some(record) if record.answer(key).is_some() => {
                    if let Some(other) = crate::backlog_impl(lang)
                        && record.answer(other).is_some()
                    {
                        backlog.recorded += 1;
                    }
                }
                _ => {
                    let expr = &entry.expr;
                    failures.push(format!("{label}: {expr}\n  unrecorded: run jay-corpus record"));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} corpus expressions have no recorded reference answer:\n{}",
        failures.len(),
        failures.join("\n")
    );
    if let Some(other) = crate::backlog_impl(lang) {
        eprintln!(
            "{}: {} expressions carry a recorded {} answer",
            crate::lang_dir(lang),
            backlog.recorded,
            crate::impl_name(other)
        );
    }
}

/// libjay against the recorded answer of the implementation it follows.
fn check(lang: Lang, record: &Record, ours: &Side) -> Option<String> {
    let theirs = record.followed(lang);
    if compare::sides_match(lang, ours, theirs) {
        return None;
    }
    Some(format!(
        "{}\n  ours:     {}\n  snapshot: {}",
        record.expr,
        ours.describe(),
        theirs.describe()
    ))
}

/// A divergence holds libjay to ITS OWN recorded side; that the pair still
/// disagrees is re-measured by `jay-corpus record`, and asserted of the file
/// here, so a hand-edited record that has quietly converged is caught.
fn check_divergence(lang: Lang, record: &Record, ours: &Side) -> Option<String> {
    let Some(recorded) = record.ours.as_ref() else {
        let expr = &record.expr;
        return Some(format!("{expr}\n  no libjay side recorded: run jay-corpus record"));
    };
    if !compare::sides_match(lang, ours, recorded) {
        return Some(format!(
            "{}\n  ours:     {}\n  snapshot: {}",
            record.expr,
            ours.describe(),
            recorded.describe()
        ));
    }
    if record.note.is_none() {
        return Some(format!("{}\n  a divergence needs a `? ` note", record.expr));
    }
    if compare::sides_match(lang, recorded, record.followed(lang)) {
        let expr = &record.expr;
        return Some(format!("{expr}\n  the recorded answers agree, so the note should go"));
    }
    None
}
