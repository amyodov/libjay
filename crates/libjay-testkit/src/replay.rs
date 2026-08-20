//! Replaying one recorded corpus file against libjay.
//!
//! This is the testing half: libjay evaluates every expression of a corpus
//! file and the answer is compared with what the reference gave when the
//! file was last recorded. No subprocess, no external binary, nothing
//! outside the repository. Collecting the recordings is the other half, and
//! lives in `jay-corpus` (docs/testing.md).

use std::path::Path;

use crate::snapshot::{Record, Side};
use crate::{Lang, compare, corpus, eval, snapshot};

/// Replay a corpus file, panicking with every mismatch at once. The panic
/// names the theme file, so a failure says which corpus to look at.
pub fn corpus_file(lang: Lang, path: &Path) {
    let label = corpus::label(path);
    let entries = corpus::read(path);
    assert!(!entries.is_empty(), "{label} has no expressions");
    let recorded = snapshot::index(snapshot::read(&corpus::snapshot_of(path)));
    let divergences = corpus::is_divergences(path);
    let mut failures = Vec::new();
    for entry in &entries {
        let Some(record) = recorded.get(&(entry.expr.clone(), entry.io)) else {
            failures.push(format!("{}\n  unrecorded: run jay-corpus record", entry.expr));
            continue;
        };
        let ours = Side::of(eval::eval(lang, &entry.expr, entry.io));
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
    eprintln!("{label}: agreement on {} expressions", entries.len());
}

/// Every corpus file of a language has a snapshot, and every expression in
/// it a record.
///
/// The replay cases are generated from a glob expanded when the test binary
/// is compiled, so a corpus file added since the last build is not a case of
/// its own until the next one. This runs at execution time and sees the
/// directory as it is now, which is what catches a file that was added and
/// never recorded. (CI compiles from scratch, so its glob is always
/// current.)
pub fn every_file_is_recorded(lang: Lang) {
    let mut failures = Vec::new();
    for path in corpus::files(lang) {
        let label = corpus::label(&path);
        let recorded = snapshot::index(snapshot::read(&corpus::snapshot_of(&path)));
        for entry in corpus::read(&path) {
            if !recorded.contains_key(&(entry.expr.clone(), entry.io)) {
                let expr = &entry.expr;
                failures.push(format!("{label}: {expr}\n  unrecorded: run jay-corpus record"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} corpus expressions have no recorded answer:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// libjay against the reference's recorded answer.
fn check(lang: Lang, record: &Record, ours: &Side) -> Option<String> {
    let theirs = record.reference();
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
    if compare::sides_match(lang, recorded, record.reference()) {
        let expr = &record.expr;
        return Some(format!("{expr}\n  the recorded answers agree, so the note should go"));
    }
    None
}
